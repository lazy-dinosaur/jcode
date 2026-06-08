//! Direct Anthropic API provider
//!
//! Uses the Anthropic Messages API directly without the Python SDK.
//! This provides better control and eliminates the Python dependency.

use super::{EventStream, NativeToolResultSender, Provider};
use crate::auth;
use crate::auth::oauth;
use crate::message::{ContentBlock, Message, Role, StreamEvent, ToolDefinition};
use crate::tool::Tool;
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::StreamExt;
use jcode_provider_core::{
    ANTHROPIC_OAUTH_BETA_HEADERS, CompletionOptions, anthropic_effectively_1m,
    anthropic_map_tool_name_for_oauth as map_tool_name_for_oauth,
    anthropic_map_tool_name_from_oauth as map_tool_name_from_oauth, anthropic_oauth_beta_headers,
    anthropic_stainless_arch as stainless_arch, anthropic_stainless_os as stainless_os,
    anthropic_strip_1m_suffix as strip_1m_suffix,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{RwLock, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

static CACHE_TTL_1H: AtomicBool = AtomicBool::new(false);

/// Enable or disable the 1-hour cache TTL (default: 5-minute)
pub fn set_cache_ttl_1h(enabled: bool) {
    CACHE_TTL_1H.store(enabled, Ordering::Relaxed);
}

/// Check if 1-hour cache TTL is enabled
pub fn is_cache_ttl_1h() -> bool {
    CACHE_TTL_1H.load(Ordering::Relaxed)
}

/// Anthropic Messages API endpoint
const API_URL: &str = "https://api.anthropic.com/v1/messages";

/// OAuth endpoint (with beta=true query param)
const API_URL_OAUTH: &str = "https://api.anthropic.com/v1/messages?beta=true";

/// User-Agent for OAuth requests, matching the official Claude Code CLI.
pub(crate) const CLAUDE_CLI_USER_AGENT: &str = "claude-cli/2.1.123 (external, sdk-cli)";

/// Claude Code billing attribution text observed in the official CLI's system
/// prompt blocks.
pub(crate) const OAUTH_BILLING_HEADER: &str =
    "cc_version=2.1.123; cc_entrypoint=sdk-cli; cch=33f85;";

pub(crate) const OAUTH_BETA_HEADERS: &str = ANTHROPIC_OAUTH_BETA_HEADERS;
#[cfg(test)]
pub(crate) const OAUTH_BETA_HEADERS_1M: &str = jcode_provider_core::ANTHROPIC_OAUTH_BETA_HEADERS_1M;

pub fn effectively_1m(model: &str) -> bool {
    anthropic_effectively_1m(model)
}

fn oauth_beta_headers(model: &str) -> &'static str {
    anthropic_oauth_beta_headers(model)
}

pub(crate) fn new_oauth_request_id() -> String {
    Uuid::new_v4().to_string()
}

fn configured_agent_profile_docs_for_oauth() -> (Vec<String>, Vec<String>) {
    let agents = crate::config::config().agents_for_working_dir(None);
    let mut examples = vec!["general".to_string()];
    examples.extend(agents.profiles.keys().cloned());
    examples.extend(agents.routes.keys().cloned());
    examples.extend(agents.routing.keys().cloned());
    examples.sort();
    examples.dedup();

    let mut docs = vec!["general: default general-purpose subagent".to_string()];
    let mut seen = std::collections::BTreeSet::from(["general".to_string()]);
    for (name, route) in agents.profiles.iter().chain(agents.routes.iter()) {
        if !seen.insert(name.clone()) {
            continue;
        }
        let mut parts = Vec::new();
        if let Some(description) = route
            .description
            .as_deref()
            .map(str::trim)
            .filter(|description| !description.is_empty())
        {
            parts.push(description.to_string());
        }
        if !route.when.is_empty() {
            parts.push(format!("use when: {}", route.when.join("; ")));
        }
        if let Some(model) = route
            .model
            .as_deref()
            .map(str::trim)
            .filter(|model| !model.is_empty())
        {
            parts.push(format!("model: {model}"));
        }
        docs.push(if parts.is_empty() {
            name.clone()
        } else {
            format!("{}: {}", name, parts.join("; "))
        });
    }
    for (name, model) in &agents.routing {
        if seen.insert(name.clone()) {
            docs.push(format!("{}: legacy route model: {}", name, model));
        }
    }
    (examples, docs)
}

fn oauth_agent_input_schema() -> Value {
    let (examples, docs) = configured_agent_profile_docs_for_oauth();
    let description = if docs.is_empty() {
        "Subagent type.".to_string()
    } else {
        format!(
            "Subagent type. Configured agent profiles: {}.",
            docs.join(" | ")
        )
    };
    json!({
        "type": "object",
        "properties": {
            "description": {"type": "string"},
            "prompt": {"type": "string"},
            "subagent_type": {
                "type": "string",
                "description": description,
                "examples": examples
            },
            "run_in_background": {"type": "boolean"}
        },
        "required": ["description", "prompt"],
        "additionalProperties": false
    })
}

fn oauth_known_tool_schema(name: &str) -> Option<ApiTool> {
    match name {
        "Agent" => Some(ApiTool {
            name: "Agent".to_string(),
            description: "Launch a new agent to handle complex, multi-step tasks.".to_string(),
            input_schema: oauth_agent_input_schema(),
            cache_control: None,
        }),
        "Bash" => Some(ApiTool {
            name: "Bash".to_string(),
            description: "Executes a given bash command and returns its output.".to_string(),
            input_schema: json!({"type":"object","properties":{"command":{"type":"string"},"timeout":{"type":"integer"},"run_in_background":{"type":"boolean"}},"required":["command"],"additionalProperties":false}),
            cache_control: None,
        }),
        "Edit" => Some(ApiTool {
            name: "Edit".to_string(),
            description: "Performs exact string replacements in files.".to_string(),
            input_schema: json!({"type":"object","properties":{"file_path":{"type":"string"},"old_string":{"type":"string"},"new_string":{"type":"string"},"replace_all":{"type":"boolean","default":false}},"required":["file_path","old_string","new_string"],"additionalProperties":false}),
            cache_control: None,
        }),
        "Glob" => Some(ApiTool {
            name: "Glob".to_string(),
            description: "Fast file pattern matching tool.".to_string(),
            input_schema: json!({"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string"}},"required":["pattern"],"additionalProperties":false}),
            cache_control: None,
        }),
        "Grep" => Some(ApiTool {
            name: "Grep".to_string(),
            description: "A powerful search tool built on ripgrep.".to_string(),
            input_schema: json!({"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string"},"glob":{"type":"string"},"output_mode":{"type":"string","enum":["content","files_with_matches","count"]},"-B":{"type":"number"},"-A":{"type":"number"},"-C":{"type":"number"},"context":{"type":"number"},"-n":{"type":"boolean"},"-i":{"type":"boolean"},"type":{"type":"string"},"head_limit":{"type":"number"},"offset":{"type":"number"},"multiline":{"type":"boolean"}},"required":["pattern"],"additionalProperties":false}),
            cache_control: None,
        }),
        "Read" => Some(ApiTool {
            name: "Read".to_string(),
            description: "Reads a file from the local filesystem.".to_string(),
            input_schema: json!({"type":"object","properties":{"file_path":{"type":"string"},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","exclusiveMinimum":0},"pages":{"type":"string"}},"required":["file_path"],"additionalProperties":false}),
            cache_control: None,
        }),
        // M13: Keep `ScheduleWakeup` as the OAuth-facing name while advertising
        // the local ScheduleTool dispatch schema.
        "ScheduleWakeup" => Some(ApiTool {
            name: "ScheduleWakeup".to_string(),
            description:
                "Schedule a task for future execution (requires wake_in_minutes or wake_at)."
                    .to_string(),
            input_schema: crate::tool::ambient::ScheduleTool::new().parameters_schema(),
            cache_control: None,
        }),
        "Skill" => Some(ApiTool {
            name: "Skill".to_string(),
            description: "Execute a skill within the main conversation".to_string(),
            input_schema: json!({"type":"object","properties":{"skill":{"type":"string"},"args":{"type":"string"}},"required":["skill"],"additionalProperties":false}),
            cache_control: None,
        }),
        // M12: Keep `ToolSearch` wire-name while advertising the local
        // CodeSearchTool dispatch schema.
        "ToolSearch" => Some(ApiTool {
            name: "ToolSearch".to_string(),
            description: "Search code, docs, and tool examples by semantic query.".to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": {"type": "string", "description": "Search query."},
                    "max_tokens": {
                        "type": "integer",
                        "description": "Maximum tokens of results to return."
                    }
                },
                "additionalProperties": false
            }),
            cache_control: None,
        }),
        "Write" => Some(ApiTool {
            name: "Write".to_string(),
            description: "Writes a file to the local filesystem.".to_string(),
            input_schema: json!({"type":"object","properties":{"file_path":{"type":"string"},"content":{"type":"string"}},"required":["file_path","content"],"additionalProperties":false}),
            cache_control: None,
        }),
        _ => None,
    }
}

pub(crate) fn apply_oauth_attribution_headers(
    req: reqwest::RequestBuilder,
    session_id: &str,
) -> reqwest::RequestBuilder {
    req.header("x-client-request-id", new_oauth_request_id())
        .header("x-app", "cli")
        .header("X-Claude-Code-Session-Id", session_id)
        .header("X-Stainless-Arch", stainless_arch())
        .header("X-Stainless-Lang", "js")
        .header("X-Stainless-OS", stainless_os())
        .header("X-Stainless-Package-Version", "0.81.0")
        .header("X-Stainless-Retry-Count", "0")
        .header("X-Stainless-Runtime", "node")
        .header("X-Stainless-Runtime-Version", "v24.3.0")
        .header("X-Stainless-Timeout", "600")
        .header("anthropic-dangerous-direct-browser-access", "true")
}

#[derive(Debug, Clone, Default)]
struct OAuthClientMetadata {
    device_id: Option<String>,
    account_uuid: Option<String>,
    organization_uuid: Option<String>,
    email_address: Option<String>,
}

fn load_official_claude_client_metadata() -> OAuthClientMetadata {
    let path = match crate::storage::user_home_path(".claude.json") {
        Ok(path) => path,
        Err(_) => return OAuthClientMetadata::default(),
    };
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => return OAuthClientMetadata::default(),
    };
    let parsed: Value = match serde_json::from_str(&content) {
        Ok(parsed) => parsed,
        Err(_) => return OAuthClientMetadata::default(),
    };
    let oauth = parsed.get("oauthAccount");
    OAuthClientMetadata {
        device_id: parsed
            .get("userID")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        account_uuid: oauth
            .and_then(|v| v.get("accountUuid"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        organization_uuid: oauth
            .and_then(|v| v.get("organizationUuid"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        email_address: oauth
            .and_then(|v| v.get("emailAddress"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    }
}

fn oauth_request_metadata(session_id: &str) -> ApiMetadata {
    let official = load_official_claude_client_metadata();
    let device_id = official.device_id.unwrap_or_else(|| {
        Uuid::new_v5(&Uuid::NAMESPACE_DNS, session_id.as_bytes())
            .simple()
            .to_string()
    });
    let account_uuid = official
        .account_uuid
        .unwrap_or_else(|| "unknown-account".to_string());
    let user_id = json!({
        "device_id": device_id,
        "account_uuid": account_uuid,
        "session_id": session_id,
    })
    .to_string();
    ApiMetadata { user_id }
}

#[derive(Serialize)]
struct OAuthEvalRequest {
    attributes: OAuthEvalAttributes,
    #[serde(rename = "forcedVariations")]
    forced_variations: std::collections::BTreeMap<String, Value>,
    #[serde(rename = "forcedFeatures")]
    forced_features: Vec<String>,
    url: String,
}

#[derive(Serialize)]
struct OAuthEvalAttributes {
    id: String,
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(rename = "deviceID")]
    device_id: String,
    platform: String,
    #[serde(rename = "organizationUUID")]
    organization_uuid: String,
    #[serde(rename = "accountUUID")]
    account_uuid: String,
    #[serde(rename = "userType")]
    user_type: String,
    #[serde(rename = "subscriptionType")]
    subscription_type: String,
    #[serde(rename = "rateLimitTier")]
    rate_limit_tier: String,
    #[serde(rename = "firstTokenTime")]
    first_token_time: i64,
    email: String,
    #[serde(rename = "appVersion")]
    app_version: String,
}

async fn oauth_preflight_get(
    client: &Client,
    headers: &reqwest::header::HeaderMap,
    label: &str,
    url: &str,
) -> Result<()> {
    let resp = client
        .get(url)
        .headers(headers.clone())
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = crate::util::http_error_body(resp, "HTTP error").await;
        anyhow::bail!("{} returned {}: {}", label, status, body);
    }

    Ok(())
}

async fn oauth_preflight_post_json<T: Serialize + ?Sized>(
    client: &Client,
    headers: &reqwest::header::HeaderMap,
    label: &str,
    url: &str,
    body: &T,
) -> Result<()> {
    let resp = client
        .post(url)
        .headers(headers.clone())
        .timeout(std::time::Duration::from_secs(5))
        .json(body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = crate::util::http_error_body(resp, "HTTP error").await;
        anyhow::bail!("{} returned {}: {}", label, status, body);
    }

    Ok(())
}

fn record_oauth_preflight_result(label: &str, result: Result<()>) -> bool {
    match result {
        Ok(()) => true,
        Err(err) => {
            crate::logging::warn(&format!(
                "Claude OAuth preflight {} failed; continuing because Claude Code treats this bootstrap traffic as nonessential: {:#}",
                label, err
            ));
            false
        }
    }
}

async fn ensure_oauth_preflight(
    client: &Client,
    token: &str,
    session_id: &str,
    done_flag: &AtomicBool,
) -> Result<()> {
    if done_flag.load(Ordering::Relaxed) {
        return Ok(());
    }

    let official = load_official_claude_client_metadata();
    let Some(device_id) = official.device_id else {
        crate::logging::warn("Skipping Claude OAuth preflight: missing userID in ~/.claude.json");
        return Ok(());
    };
    let Some(account_uuid) = official.account_uuid else {
        crate::logging::warn(
            "Skipping Claude OAuth preflight: missing accountUuid in ~/.claude.json",
        );
        return Ok(());
    };
    let Some(organization_uuid) = official.organization_uuid else {
        crate::logging::warn(
            "Skipping Claude OAuth preflight: missing organizationUuid in ~/.claude.json",
        );
        return Ok(());
    };
    let Some(email_address) = official.email_address else {
        crate::logging::warn(
            "Skipping Claude OAuth preflight: missing emailAddress in ~/.claude.json",
        );
        return Ok(());
    };

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_str(&format!("Bearer {}", token))?,
    );
    headers.insert(
        reqwest::header::USER_AGENT,
        reqwest::header::HeaderValue::from_static(CLAUDE_CLI_USER_AGENT),
    );
    headers.insert(
        reqwest::header::CONTENT_TYPE,
        reqwest::header::HeaderValue::from_static("application/json"),
    );
    headers.insert(
        reqwest::header::HeaderName::from_static("anthropic-beta"),
        reqwest::header::HeaderValue::from_static("oauth-2025-04-20"),
    );

    let mut all_ok = true;
    all_ok &= record_oauth_preflight_result(
        "bootstrap",
        oauth_preflight_get(
            client,
            &headers,
            "bootstrap",
            "https://api.anthropic.com/api/claude_cli/bootstrap",
        )
        .await,
    );
    all_ok &= record_oauth_preflight_result(
        "account settings",
        oauth_preflight_get(
            client,
            &headers,
            "account settings",
            "https://api.anthropic.com/api/oauth/account/settings",
        )
        .await,
    );
    all_ok &= record_oauth_preflight_result(
        "grove",
        oauth_preflight_get(
            client,
            &headers,
            "grove",
            "https://api.anthropic.com/api/claude_code_grove",
        )
        .await,
    );

    let eval = OAuthEvalRequest {
        attributes: OAuthEvalAttributes {
            id: device_id.clone(),
            session_id: session_id.to_string(),
            device_id: device_id.clone(),
            platform: std::env::consts::OS.to_string(),
            organization_uuid,
            account_uuid,
            user_type: "external".to_string(),
            subscription_type: crate::auth::claude::get_subscription_type()
                .unwrap_or_else(|| "pro".to_string()),
            rate_limit_tier: "default_claude_ai".to_string(),
            first_token_time: 1_740_976_801_491,
            email: email_address,
            app_version: "2.1.123".to_string(),
        },
        forced_variations: Default::default(),
        forced_features: Vec::new(),
        url: String::new(),
    };

    all_ok &= record_oauth_preflight_result(
        "eval",
        oauth_preflight_post_json(
            client,
            &headers,
            "eval",
            "https://api.anthropic.com/api/eval/sdk-zAZezfDKGoZuXXKe",
            &eval,
        )
        .await,
    );

    done_flag.store(true, Ordering::Relaxed);
    if all_ok {
        crate::logging::info("Claude OAuth preflight completed successfully");
    }
    Ok(())
}

/// Default model
const DEFAULT_MODEL: &str = "claude-opus-4-8";

/// API version header
const API_VERSION: &str = "2023-06-01";

/// Claude Agent SDK identity block observed in the official Claude Code client.
const CLAUDE_CODE_IDENTITY: &str = "You are a Claude agent, built on Anthropic's Claude Agent SDK.";

/// Maximum number of retries for transient errors
const MAX_RETRIES: u32 = 3;

/// Base delay for exponential backoff (in milliseconds)
const RETRY_BASE_DELAY_MS: u64 = 1000;

/// Default max output tokens for Anthropic models.
/// Set to 32k to avoid truncating long tool calls (e.g. writing large files).
/// Override with JCODE_ANTHROPIC_MAX_TOKENS env var.
const DEFAULT_MAX_TOKENS: u32 = 32_768;

/// Available models
pub const AVAILABLE_MODELS: &[&str] = &[
    "claude-opus-4-8",
    "claude-opus-4-8[1m]",
    "claude-opus-4-7",
    "claude-opus-4-7[1m]",
    "claude-opus-4-6",
    "claude-opus-4-6[1m]",
    "claude-sonnet-4-6",
    "claude-sonnet-4-6[1m]",
    "claude-haiku-4-5-20251001",
    "claude-haiku-4-5",
    "claude-opus-4-5",
    "claude-sonnet-4-5",
    "claude-sonnet-4-20250514",
];

/// Cached OAuth credentials
#[derive(Clone)]
struct CachedCredentials {
    access_token: String,
    refresh_token: String,
    expires_at: i64,
}

/// Direct Anthropic API provider
pub struct AnthropicProvider {
    client: Client,
    model: Arc<std::sync::RwLock<String>>,
    reasoning_effort: Arc<std::sync::RwLock<Option<String>>>,
    /// Cached OAuth credentials (None if using API key)
    credentials: Arc<RwLock<Option<CachedCredentials>>>,
    max_tokens: u32,
    oauth_session_id: String,
    oauth_preflight_done: Arc<AtomicBool>,
}

impl AnthropicProvider {
    const EFFORTS: [&'static str; 6] = ["none", "low", "medium", "high", "xhigh", "max"];

    fn normalize_reasoning_effort(effort: &str) -> Option<&'static str> {
        match effort.trim().to_ascii_lowercase().as_str() {
            "none" | "off" | "disable" | "disabled" => Some("none"),
            "low" => Some("low"),
            "medium" | "med" => Some("medium"),
            "high" => Some("high"),
            "xhigh" | "extra-high" | "extra_high" => Some("xhigh"),
            "max" => Some("max"),
            _ => None,
        }
    }

    fn effort_for_output_config(effort: &str) -> Option<&'static str> {
        match Self::normalize_reasoning_effort(effort)? {
            "none" => None,
            "low" => Some("low"),
            "medium" => Some("medium"),
            "high" => Some("high"),
            "xhigh" => Some("xhigh"),
            "max" => Some("max"),
            _ => None,
        }
    }

    fn manual_thinking_budget_for_effort(effort: &str) -> Option<u32> {
        match Self::normalize_reasoning_effort(effort)? {
            "none" => None,
            "low" => Some(1_024),
            "medium" => Some(4_096),
            "high" => Some(8_192),
            "xhigh" => Some(12_288),
            "max" => Some(16_384),
            _ => None,
        }
    }

    fn supports_adaptive_thinking_effort(model: &str) -> bool {
        let model = strip_1m_suffix(model).trim().to_ascii_lowercase();
        model.starts_with("claude-opus-4-8")
            || model.starts_with("claude-opus-4.8")
            || model.starts_with("claude-opus-4-7")
            || model.starts_with("claude-opus-4.7")
            || model.starts_with("claude-opus-4-6")
            || model.starts_with("claude-opus-4.6")
            || model.starts_with("claude-sonnet-4-6")
            || model.starts_with("claude-sonnet-4.6")
    }

    fn current_reasoning_controls_for_model(
        &self,
        model: &str,
    ) -> (Option<ApiThinking>, Option<ApiOutputConfig>) {
        let effort = self
            .reasoning_effort
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(effort) = effort else {
            return (None, None);
        };

        let Some(output_effort) = Self::effort_for_output_config(&effort) else {
            return (None, None);
        };

        if Self::supports_adaptive_thinking_effort(model) {
            return (
                Some(ApiThinking {
                    kind: "adaptive",
                    budget_tokens: None,
                    display: Some("summarized"),
                }),
                Some(ApiOutputConfig {
                    effort: output_effort,
                }),
            );
        }

        let Some(requested_budget) = Self::manual_thinking_budget_for_effort(&effort) else {
            return (None, None);
        };
        let max_budget = self.max_tokens.saturating_sub(1);
        if max_budget == 0 {
            return (None, None);
        }
        (
            Some(ApiThinking {
                kind: "enabled",
                budget_tokens: Some(requested_budget.min(max_budget)),
                display: None,
            }),
            None,
        )
    }

    #[cfg(test)]
    fn current_thinking_config(&self) -> Option<ApiThinking> {
        let model = self
            .model
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        self.current_reasoning_controls_for_model(&model).0
    }

    #[cfg(test)]
    fn current_output_config(&self) -> Option<ApiOutputConfig> {
        let model = self
            .model
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        self.current_reasoning_controls_for_model(&model).1
    }

    fn is_usage_exhausted() -> bool {
        let usage = crate::usage::get_sync();
        usage.five_hour >= 0.99 && usage.seven_day >= 0.99
    }

    pub fn new() -> Self {
        let model = std::env::var("JCODE_ANTHROPIC_MODEL").unwrap_or_else(|_| {
            if Self::is_usage_exhausted() {
                "claude-sonnet-4-6".to_string()
            } else {
                DEFAULT_MODEL.to_string()
            }
        });

        // Trigger background usage fetch so extra_usage is known before first API call
        let _ = tokio::runtime::Handle::try_current().map(|_| {
            tokio::spawn(async {
                let _ = crate::usage::get().await;
            })
        });

        let max_tokens = std::env::var("JCODE_ANTHROPIC_MAX_TOKENS")
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok())
            .unwrap_or(DEFAULT_MAX_TOKENS);

        Self {
            client: crate::provider::shared_http_client(),
            model: Arc::new(std::sync::RwLock::new(model)),
            reasoning_effort: Arc::new(std::sync::RwLock::new(None)),
            credentials: Arc::new(RwLock::new(None)),
            max_tokens,
            oauth_session_id: Uuid::new_v4().to_string(),
            oauth_preflight_done: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get the access token from credentials
    /// Supports both OAuth tokens and direct API keys
    /// Automatically refreshes OAuth tokens when expired
    async fn get_access_token(&self) -> Result<(String, bool)> {
        // First check for direct API key in environment
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            return Ok((key, false)); // false = not OAuth
        }

        // Check cached credentials
        {
            let cached = self.credentials.read().await;
            if let Some(ref creds) = *cached {
                let now = chrono::Utc::now().timestamp_millis();
                // Return cached token if not expired (with 5 min buffer)
                if creds.expires_at > now + 300_000 {
                    return Ok((creds.access_token.clone(), true));
                }
            }
        }

        // Load fresh credentials or refresh expired ones
        let fresh_creds =
            auth::claude::load_credentials().context("Failed to load Claude credentials")?;

        if !fresh_creds.scopes.is_empty()
            && !oauth::claude_scopes_have_inference(&fresh_creds.scopes)
        {
            anyhow::bail!(
                "Claude OAuth credentials are missing the required user:inference scope (scopes: {}). Run `jcode login --provider claude` to mint a fresh Claude.ai OAuth token, or import/use a fresh Claude Code login.",
                fresh_creds.scopes.join(" ")
            );
        }

        let now = chrono::Utc::now().timestamp_millis();

        // Check if token needs refresh (expired or expiring within 5 minutes)
        if fresh_creds.expires_at < now + 300_000 && !fresh_creds.refresh_token.is_empty() {
            crate::logging::info("OAuth token expired or expiring soon, attempting refresh...");

            let active_label = auth::claude::active_account_label()
                .unwrap_or_else(auth::claude::primary_account_label);
            match oauth::refresh_claude_tokens_for_account(
                &fresh_creds.refresh_token,
                &active_label,
            )
            .await
            {
                Ok(refreshed) => {
                    crate::logging::info("OAuth token refreshed successfully");

                    // Cache the refreshed credentials
                    let mut cached = self.credentials.write().await;
                    *cached = Some(CachedCredentials {
                        access_token: refreshed.access_token.clone(),
                        refresh_token: refreshed.refresh_token,
                        expires_at: refreshed.expires_at,
                    });

                    return Ok((refreshed.access_token, true));
                }
                Err(e) => {
                    crate::logging::error(&format!("OAuth token refresh failed: {}", e));
                    // Fall through to try the possibly-expired token
                }
            }
        }

        // Cache and return the loaded credentials (even if expired, let the API reject it)
        let mut cached = self.credentials.write().await;
        *cached = Some(CachedCredentials {
            access_token: fresh_creds.access_token.clone(),
            refresh_token: fresh_creds.refresh_token,
            expires_at: fresh_creds.expires_at,
        });

        Ok((fresh_creds.access_token, true))
    }

    /// Convert our Message type to Anthropic API format
    /// Also repairs dangling tool_uses by injecting synthetic tool_results
    fn format_messages(&self, messages: &[Message], is_oauth: bool) -> Vec<ApiMessage> {
        use std::collections::HashSet;

        // First pass: collect all tool_use IDs and tool_result IDs
        let mut tool_use_ids: HashSet<String> = HashSet::new();
        let mut tool_result_ids: HashSet<String> = HashSet::new();

        for msg in messages {
            for block in &msg.content {
                match block {
                    ContentBlock::ToolUse { id, .. } => {
                        tool_use_ids.insert(id.clone());
                    }
                    ContentBlock::ToolResult { tool_use_id, .. } => {
                        tool_result_ids.insert(tool_use_id.clone());
                    }
                    _ => {}
                }
            }
        }

        // Find dangling tool_uses (no matching tool_result)
        let dangling: HashSet<_> = tool_use_ids.difference(&tool_result_ids).cloned().collect();
        if !dangling.is_empty() {
            crate::logging::info(&format!(
                "[anthropic] Repairing {} dangling tool_use(s) by injecting synthetic tool_results",
                dangling.len()
            ));
        }

        // Second pass: build messages, injecting synthetic tool_results after assistant messages
        // that have dangling tool_uses
        let mut result: Vec<ApiMessage> = Vec::new();

        for msg in messages {
            let role = match msg.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };

            let content = self.format_content_blocks(&msg.content, is_oauth);

            if !content.is_empty() {
                result.push(ApiMessage {
                    role: role.to_string(),
                    content,
                });
            }

            // If this is an assistant message with dangling tool_uses, inject synthetic results
            if matches!(msg.role, Role::Assistant) {
                let mut synthetic_results: Vec<ApiContentBlock> = Vec::new();
                for block in &msg.content {
                    if let ContentBlock::ToolUse { id, .. } = block
                        && dangling.contains(id)
                    {
                        synthetic_results.push(ApiContentBlock::ToolResult {
                            tool_use_id: crate::message::sanitize_tool_id(id),
                            content: ToolResultContent::Text(
                                "[Session interrupted before tool execution completed]".to_string(),
                            ),
                            is_error: true,
                        });
                    }
                }
                if !synthetic_results.is_empty() {
                    result.push(ApiMessage {
                        role: "user".to_string(),
                        content: synthetic_results,
                    });
                }
            }
        }

        // Third pass: merge consecutive messages of the same role
        // Anthropic API requires strictly alternating user/assistant messages
        let pre_merge_count = result.len();
        let mut merged: Vec<ApiMessage> = Vec::new();
        for msg in result {
            if let Some(last) = merged.last_mut()
                && last.role == msg.role
            {
                last.content.extend(msg.content);
                continue;
            }
            merged.push(msg);
        }

        if merged.len() != pre_merge_count {
            crate::logging::info(&format!(
                "[anthropic] Merged {} consecutive same-role messages",
                pre_merge_count - merged.len()
            ));
        }

        Self::dedupe_tool_results(&mut merged);

        Self::normalize_tool_result_adjacency(&mut merged);

        // Validate: check each assistant message with tool_use has matching tool_result in next user message
        for (i, msg) in merged.iter().enumerate() {
            if msg.role == "assistant" {
                let tool_uses: Vec<&String> = msg
                    .content
                    .iter()
                    .filter_map(|b| {
                        if let ApiContentBlock::ToolUse { id, .. } = b {
                            Some(id)
                        } else {
                            None
                        }
                    })
                    .collect();

                if !tool_uses.is_empty() {
                    // Check next message
                    if let Some(next) = merged.get(i + 1) {
                        if next.role != "user" {
                            crate::logging::warn(&format!(
                                "[anthropic] Message {} has tool_use but next message is {} (should be user)",
                                i, next.role
                            ));
                        } else {
                            let tool_results: std::collections::HashSet<&String> = next
                                .content
                                .iter()
                                .filter_map(|b| {
                                    if let ApiContentBlock::ToolResult { tool_use_id, .. } = b {
                                        Some(tool_use_id)
                                    } else {
                                        None
                                    }
                                })
                                .collect();

                            for tu_id in &tool_uses {
                                if !tool_results.contains(*tu_id) {
                                    crate::logging::warn(&format!(
                                        "[anthropic] Message {} has tool_use {} but no matching tool_result in message {}",
                                        i,
                                        tu_id,
                                        i + 1
                                    ));
                                }
                            }
                        }
                    } else {
                        crate::logging::warn(&format!(
                            "[anthropic] Message {} has tool_use but no next message",
                            i
                        ));
                    }
                }
            }
        }

        merged
    }

    /// Anthropic rejects a request when the same `tool_use_id` appears in more
    /// than one `tool_result` block ("each tool_use must have a single
    /// result"). Our persisted transcript can end up with duplicate
    /// tool_results for one id (e.g. a retried/continued turn that re-recorded
    /// the same result, or merged consecutive user messages). Keep only the
    /// first result per id across the whole conversation and drop the rest.
    fn dedupe_tool_results(messages: &mut Vec<ApiMessage>) {
        use std::collections::HashSet;

        let mut seen: HashSet<String> = HashSet::new();
        let mut removed = 0usize;

        for msg in messages.iter_mut() {
            if msg.role != "user" {
                continue;
            }
            msg.content.retain(|block| {
                if let ApiContentBlock::ToolResult { tool_use_id, .. } = block {
                    if seen.contains(tool_use_id) {
                        removed += 1;
                        return false;
                    }
                    seen.insert(tool_use_id.clone());
                }
                true
            });
        }

        messages.retain(|msg| !msg.content.is_empty());

        if removed > 0 {
            crate::logging::info(&format!(
                "[anthropic] Removed {removed} duplicate tool_result block(s)"
            ));
        }
    }

    /// Anthropic requires every assistant `tool_use` to be followed immediately
    /// by a user message whose *leading* blocks are the corresponding
    /// `tool_result`s. Our persisted transcript can contain consecutive user
    /// messages for parallel tool results, and image read results also add a
    /// descriptive text block after the first tool result. After same-role
    /// merging this can become:
    ///
    ///   assistant: tool_use A, tool_use B
    ///   user: tool_result A, text/image note, tool_result B
    ///
    /// which Anthropic rejects because B is not in the immediate tool-result
    /// prefix. Move all matching tool_result blocks to the front of the next
    /// user message, preserving assistant tool_use order and keeping any extra
    /// user text after the tool results.
    fn normalize_tool_result_adjacency(messages: &mut Vec<ApiMessage>) {
        let mut assistant_index = 0usize;
        let mut repaired = 0usize;

        while assistant_index < messages.len() {
            if messages[assistant_index].role != "assistant" {
                assistant_index += 1;
                continue;
            }

            let tool_use_ids = messages[assistant_index]
                .content
                .iter()
                .filter_map(|block| match block {
                    ApiContentBlock::ToolUse { id, .. } => Some(id.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();

            if tool_use_ids.is_empty() {
                assistant_index += 1;
                continue;
            }

            let user_index = assistant_index + 1;
            if user_index >= messages.len() || messages[user_index].role != "user" {
                messages.insert(
                    user_index,
                    ApiMessage {
                        role: "user".to_string(),
                        content: Vec::new(),
                    },
                );
            }

            if Self::tool_result_prefix_matches(&messages[user_index].content, &tool_use_ids) {
                assistant_index += 1;
                continue;
            }

            let mut ordered_results = Vec::new();
            for tool_use_id in &tool_use_ids {
                let block = Self::remove_tool_result_block(messages, user_index, tool_use_id)
                    .unwrap_or_else(|| ApiContentBlock::ToolResult {
                        tool_use_id: tool_use_id.clone(),
                        content: ToolResultContent::Text(
                            "[Session interrupted before tool execution completed]".to_string(),
                        ),
                        is_error: true,
                    });
                ordered_results.push(block);
            }

            let mut remainder = std::mem::take(&mut messages[user_index].content);
            repaired += ordered_results.len();
            ordered_results.append(&mut remainder);
            messages[user_index].content = ordered_results;

            assistant_index += 1;
        }

        messages.retain(|msg| !msg.content.is_empty());

        if repaired > 0 {
            crate::logging::info(&format!(
                "[anthropic] Normalized {} tool_result block(s) into immediate user prefixes",
                repaired
            ));
        }
    }

    fn tool_result_prefix_matches(content: &[ApiContentBlock], tool_use_ids: &[String]) -> bool {
        if content.len() < tool_use_ids.len() {
            return false;
        }

        content
            .iter()
            .zip(tool_use_ids.iter())
            .all(|(block, expected_id)| {
                matches!(
                    block,
                    ApiContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == expected_id
                )
            })
    }

    fn remove_tool_result_block(
        messages: &mut [ApiMessage],
        start_index: usize,
        tool_use_id: &str,
    ) -> Option<ApiContentBlock> {
        for msg in messages.iter_mut().skip(start_index) {
            if msg.role != "user" {
                continue;
            }

            let Some(position) = msg.content.iter().position(|block| {
                matches!(
                    block,
                    ApiContentBlock::ToolResult { tool_use_id: id, .. } if id == tool_use_id
                )
            }) else {
                continue;
            };

            return Some(msg.content.remove(position));
        }

        None
    }

    /// Convert our ContentBlock to Anthropic API format
    fn format_content_blocks(
        &self,
        blocks: &[ContentBlock],
        is_oauth: bool,
    ) -> Vec<ApiContentBlock> {
        let mut result: Vec<ApiContentBlock> = Vec::new();
        for block in blocks {
            match block {
                ContentBlock::Text { text, .. } => {
                    result.push(ApiContentBlock::Text {
                        text: text.clone(),
                        cache_control: None,
                    });
                }
                ContentBlock::ToolUse { id, name, input } => {
                    result.push(ApiContentBlock::ToolUse {
                        id: crate::message::sanitize_tool_id(id),
                        name: if is_oauth {
                            map_tool_name_for_oauth(name)
                        } else {
                            name.clone()
                        },
                        input: if input.is_object() {
                            input.clone()
                        } else {
                            serde_json::json!({})
                        },
                        cache_control: None,
                    });
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    result.push(ApiContentBlock::ToolResult {
                        tool_use_id: crate::message::sanitize_tool_id(tool_use_id),
                        content: ToolResultContent::Text(content.clone()),
                        is_error: is_error.unwrap_or(false),
                    });
                }
                ContentBlock::Image { media_type, data } => {
                    let img_block = ToolResultContentBlock::Image {
                        source: ApiImageSource {
                            kind: "base64".to_string(),
                            media_type: media_type.clone(),
                            data: data.clone(),
                        },
                    };
                    if let Some(ApiContentBlock::ToolResult { content, .. }) = result.last_mut() {
                        match content {
                            ToolResultContent::Text(text) => {
                                let text_block = ToolResultContentBlock::Text {
                                    text: std::mem::take(text),
                                };
                                *content = ToolResultContent::Blocks(vec![text_block, img_block]);
                            }
                            ToolResultContent::Blocks(blocks) => {
                                blocks.push(img_block);
                            }
                        }
                    } else {
                        result.push(ApiContentBlock::Image {
                            source: ApiImageSource {
                                kind: "base64".to_string(),
                                media_type: media_type.clone(),
                                data: data.clone(),
                            },
                        });
                    }
                }
                _ => {}
            }
        }
        result
    }

    /// Convert tool definitions to Anthropic API format
    /// Adds cache_control to the last tool for prompt caching
    fn format_tools(&self, tools: &[ToolDefinition], is_oauth: bool) -> Vec<ApiTool> {
        if is_oauth {
            let mut seen = HashSet::new();
            let mut api_tools: Vec<ApiTool> = tools
                .iter()
                .filter_map(|tool| {
                    let advertised_name = map_tool_name_for_oauth(&tool.name);
                    if !seen.insert(advertised_name.clone()) {
                        return None;
                    }

                    let mut api_tool =
                        oauth_known_tool_schema(&advertised_name).unwrap_or_else(|| ApiTool {
                            name: advertised_name,
                            description: tool.description.clone(),
                            input_schema: tool.input_schema.clone(),
                            cache_control: None,
                        });
                    api_tool.cache_control = None;
                    Some(api_tool)
                })
                .collect();

            if let Some(last) = api_tools.last_mut() {
                last.cache_control = Some(CacheControlParam::ephemeral());
            }

            return api_tools;
        }

        let len = tools.len();
        tools
            .iter()
            .enumerate()
            .map(|(i, tool)| ApiTool {
                name: tool.name.clone(),
                description: tool.description.clone(),
                input_schema: tool.input_schema.clone(),
                cache_control: if i == len - 1 {
                    Some(CacheControlParam::ephemeral())
                } else {
                    None
                },
            })
            .collect()
    }
}

impl Default for AnthropicProvider {
    fn default() -> Self {
        Self::new()
    }
}

fn log_anthropic_canonical_input(
    model: &str,
    format: &str,
    request: &ApiRequest,
    is_oauth: bool,
    split_prompt: bool,
) {
    let messages_value = serde_json::to_value(&request.messages).unwrap_or(Value::Null);
    let message_items = messages_value.as_array().cloned().unwrap_or_default();
    let system_value = request
        .system
        .as_ref()
        .and_then(|system| serde_json::to_value(system).ok());
    let tools_value = request
        .tools
        .as_ref()
        .and_then(|tools| serde_json::to_value(tools).ok());
    let payload = json!({
        "model": &request.model,
        "max_tokens": request.max_tokens,
        "system": system_value.as_ref(),
        "messages": messages_value,
        "tools": tools_value.as_ref(),
        "temperature": request.temperature,
        "thinking": request.thinking,
    });

    super::fingerprint::log_provider_canonical_input(
        "anthropic",
        model,
        format,
        &payload,
        &message_items,
        system_value.as_ref(),
        tools_value.as_ref(),
        request.tools.as_ref().map(|tools| tools.len()),
        &[
            ("oauth", is_oauth.to_string()),
            ("split_prompt", split_prompt.to_string()),
        ],
    );
}

#[async_trait]
impl Provider for AnthropicProvider {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        system: &str,
        resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        self.complete_with_options(
            messages,
            tools,
            system,
            resume_session_id,
            CompletionOptions::default(),
        )
        .await
    }

    async fn complete_with_options(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        system: &str,
        _resume_session_id: Option<&str>,
        options: CompletionOptions,
    ) -> Result<EventStream> {
        let (token, is_oauth) = self.get_access_token().await?;
        if is_oauth {
            ensure_oauth_preflight(
                &self.client,
                &token,
                &self.oauth_session_id,
                &self.oauth_preflight_done,
            )
            .await?;
        }
        let model = self
            .model
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let api_model = strip_1m_suffix(&model).to_string();
        let (thinking, output_config) = self.current_reasoning_controls_for_model(&model);
        let temperature = if is_oauth || thinking.is_some() {
            Some(1.0)
        } else {
            None
        };

        // Format request
        let api_messages = self.format_messages(messages, is_oauth);
        let api_tools = self.format_tools(tools, is_oauth);

        let request = ApiRequest {
            model: api_model,
            max_tokens: self.max_tokens,
            system: build_system_param(system, is_oauth),
            messages: format_messages_with_identity(api_messages, is_oauth),
            tools: if api_tools.is_empty() {
                None
            } else {
                Some(api_tools)
            },
            metadata: if is_oauth {
                Some(oauth_request_metadata(&self.oauth_session_id))
            } else {
                None
            },
            temperature,
            thinking,
            output_config,
            stream: true,
        };

        log_anthropic_canonical_input(&model, "anthropic_messages", &request, is_oauth, false);

        crate::logging::info(&format!(
            "Anthropic transport: HTTPS SSE stream (oauth={})",
            is_oauth
        ));

        // Create channel for streaming events
        let (tx, rx) = mpsc::channel::<Result<StreamEvent>>(100);

        // Clone what we need for the async task
        let client = self.client.clone();
        let credentials = Arc::clone(&self.credentials);
        let oauth_session_id = self.oauth_session_id.clone();
        let cancel_signal = options.cancel_signal();

        // Spawn task to handle streaming with retry logic.
        // This includes forced OAuth refresh on auth failures.
        tokio::spawn(async move {
            if tx
                .send(Ok(StreamEvent::ConnectionType {
                    connection: "https/sse".to_string(),
                }))
                .await
                .is_err()
            {
                return;
            }
            run_stream_with_retries(
                client,
                token,
                is_oauth,
                request,
                tx,
                credentials,
                model,
                oauth_session_id,
                cancel_signal,
            )
            .await;
        });

        Ok(Box::pin(ReceiverStream::new(rx)))
    }

    fn model(&self) -> String {
        self.model
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn set_model(&self, model: &str) -> Result<()> {
        if !crate::provider::known_anthropic_model_ids()
            .iter()
            .any(|known| known == model)
        {
            anyhow::bail!("Model {} not supported by Anthropic provider", model);
        }
        *self
            .model
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = model.to_string();
        Ok(())
    }

    // ---- M47-C4: Anthropic provider-aware context / thinking ----
    //
    // Anthropic is the only built-in provider that exposes a runtime
    // context-window choice. `"1m"` appends the `[1m]` suffix that the
    // existing 1M-context routing (commit 3ad34ed2 and follow-ups) consumes
    // both in the OAuth beta-header selection (`anthropic_oauth_beta_headers`)
    // and in `context_limit_for_model`. `"200k"` strips the suffix back to the
    // default 200K route. Any other value is a no-op debug log so a single
    // SSOT can carry the field for every persona without raising on us.
    //
    // The matching model is not auto-listed in known_anthropic_model_ids when
    // the `[1m]` suffix is appended at runtime; set_model bypasses the model
    // catalog check that set_context_preference would otherwise fail on. We
    // therefore write the model field directly instead of going through
    // `set_model` so the runtime context preference can be applied even for
    // catalogs that ship only the base model id.

    fn available_contexts(&self) -> Vec<&'static str> {
        vec!["200k", "1m"]
    }

    fn context_preference(&self) -> Option<String> {
        let model = self
            .model
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if model.ends_with("[1m]") {
            Some("1m".to_string())
        } else {
            Some("200k".to_string())
        }
    }

    fn set_context_preference(&self, context: &str) -> Result<()> {
        let normalized = context.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "1m" | "1m-context" | "long" | "long-context" => {
                let mut guard = self
                    .model
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if !guard.ends_with("[1m]") {
                    *guard = format!("{}[1m]", guard);
                }
                Ok(())
            }
            "200k" | "default" | "short" | "short-context" => {
                let mut guard = self
                    .model
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Some(stripped) = guard.strip_suffix("[1m]") {
                    *guard = stripped.to_string();
                }
                Ok(())
            }
            "" => Ok(()),
            other => {
                crate::logging::debug(&format!(
                    "Unrecognized Anthropic context preference '{}' ignored (available: 200k, 1m)",
                    other
                ));
                Ok(())
            }
        }
    }

    // Anthropic models from 4.7 expose extended-thinking via the
    // `interleaved-thinking-2025-05-14` beta (already shipped in OAUTH headers,
    // see `ANTHROPIC_OAUTH_BETA_HEADERS`). The provider currently does not
    // toggle the thinking surface at request time — we just declare the
    // capability so M47-C5 variant resolution can mark `thinking: true` as
    // intentional rather than silently dropped. A future milestone may wire
    // `set_thinking` into request bodies for non-OAuth flows.
    fn supports_thinking(&self) -> bool {
        true
    }

    fn reasoning_effort(&self) -> Option<String> {
        self.reasoning_effort
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn set_reasoning_effort(&self, effort: &str) -> Result<()> {
        let normalized = Self::normalize_reasoning_effort(effort).ok_or_else(|| {
            anyhow::anyhow!(
                "Unsupported Claude effort '{}'; expected none|low|medium|high|max",
                effort
            )
        })?;
        *self
            .reasoning_effort
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(normalized.to_string());
        Ok(())
    }

    fn available_efforts(&self) -> Vec<&'static str> {
        Self::EFFORTS.to_vec()
    }

    fn thinking_enabled(&self) -> Option<bool> {
        Some(
            self.reasoning_effort
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_deref()
                .and_then(Self::effort_for_output_config)
                .is_some(),
        )
    }

    fn set_thinking(&self, enabled: bool) -> Result<()> {
        self.set_reasoning_effort(if enabled { "medium" } else { "none" })
    }

    fn available_models(&self) -> Vec<&'static str> {
        AVAILABLE_MODELS.to_vec()
    }

    fn available_models_for_switching(&self) -> Vec<String> {
        crate::provider::cached_anthropic_model_ids()
            .unwrap_or_else(crate::provider::known_anthropic_model_ids)
    }

    fn available_models_display(&self) -> Vec<String> {
        self.available_models_for_switching()
    }

    async fn prefetch_models(&self) -> Result<()> {
        let (token, is_oauth) = self.get_access_token().await?;
        if token.trim().is_empty() {
            return Ok(());
        }

        let catalog = if is_oauth {
            match crate::provider::fetch_anthropic_model_catalog_oauth(&token).await {
                Ok(catalog) => catalog,
                Err(err) => {
                    crate::logging::warn(&format!(
                        "Anthropic OAuth model catalog refresh failed; keeping fallback list: {}",
                        err
                    ));
                    return Ok(());
                }
            }
        } else {
            crate::provider::fetch_anthropic_model_catalog(&token).await?
        };
        crate::provider::persist_anthropic_model_catalog(&catalog);
        if !catalog.context_limits.is_empty() {
            crate::provider::populate_context_limits(catalog.context_limits);
        }
        if !catalog.available_models.is_empty() {
            crate::provider::populate_anthropic_models(catalog.available_models);
        }
        Ok(())
    }

    fn name(&self) -> &'static str {
        "anthropic"
    }

    fn supports_image_input(&self) -> bool {
        true
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self {
            client: self.client.clone(),
            model: Arc::new(std::sync::RwLock::new(
                self.model
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone(),
            )),
            reasoning_effort: Arc::new(std::sync::RwLock::new(
                self.reasoning_effort
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone(),
            )),
            credentials: Arc::new(RwLock::new(None)),
            max_tokens: self.max_tokens,
            oauth_session_id: self.oauth_session_id.clone(),
            oauth_preflight_done: Arc::new(AtomicBool::new(
                self.oauth_preflight_done.load(Ordering::Relaxed),
            )),
        })
    }

    async fn invalidate_credentials(&self) {
        let mut cached = self.credentials.write().await;
        *cached = None;
    }

    fn native_result_sender(&self) -> Option<NativeToolResultSender> {
        None // Direct API doesn't use native tool bridge
    }

    /// Split system prompt completion for better cache efficiency
    /// Static content is cached, dynamic content is not
    async fn complete_split(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        system_static: &str,
        system_dynamic: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        let (token, is_oauth) = self.get_access_token().await?;
        if is_oauth {
            ensure_oauth_preflight(
                &self.client,
                &token,
                &self.oauth_session_id,
                &self.oauth_preflight_done,
            )
            .await?;
        }
        let model = self
            .model
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let api_model = strip_1m_suffix(&model).to_string();
        let (thinking, output_config) = self.current_reasoning_controls_for_model(&model);
        let temperature = if is_oauth || thinking.is_some() {
            Some(1.0)
        } else {
            None
        };

        // Format request
        let api_messages = self.format_messages(messages, is_oauth);
        let api_tools = self.format_tools(tools, is_oauth);

        let request = ApiRequest {
            model: api_model,
            max_tokens: self.max_tokens,
            system: build_system_param_split(system_static, system_dynamic, is_oauth),
            messages: format_messages_with_identity(api_messages, is_oauth),
            tools: if api_tools.is_empty() {
                None
            } else {
                Some(api_tools)
            },
            metadata: if is_oauth {
                Some(oauth_request_metadata(&self.oauth_session_id))
            } else {
                None
            },
            temperature,
            thinking,
            output_config,
            stream: true,
        };

        log_anthropic_canonical_input(&model, "anthropic_messages_split", &request, is_oauth, true);

        crate::logging::info(&format!(
            "Anthropic transport: HTTPS SSE split stream (oauth={})",
            is_oauth
        ));

        // Create channel for streaming events
        let (tx, rx) = mpsc::channel::<Result<StreamEvent>>(100);

        // Clone what we need for the async task
        let client = self.client.clone();
        let credentials = Arc::clone(&self.credentials);
        let oauth_session_id = self.oauth_session_id.clone();

        // Spawn task to handle streaming with retry logic
        tokio::spawn(async move {
            if tx
                .send(Ok(StreamEvent::ConnectionType {
                    connection: "https/sse".to_string(),
                }))
                .await
                .is_err()
            {
                return;
            }
            run_stream_with_retries(
                client,
                token,
                is_oauth,
                request,
                tx,
                credentials,
                model,
                oauth_session_id,
                None,
            )
            .await;
        });

        Ok(Box::pin(ReceiverStream::new(rx)))
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "stream retry helper needs auth/session/runtime knobs together and is kept local for clarity"
)]
async fn run_stream_with_retries(
    client: Client,
    initial_token: String,
    is_oauth: bool,
    request: ApiRequest,
    tx: mpsc::Sender<Result<StreamEvent>>,
    credentials: Arc<RwLock<Option<CachedCredentials>>>,
    model_name: String,
    oauth_session_id: String,
    cancel_signal: Option<jcode_agent_runtime::InterruptSignal>,
) {
    let mut token = initial_token;
    let mut last_error = None;
    let mut attempted_forced_refresh = false;

    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            // Exponential backoff: 1s, 2s, 4s
            let delay = RETRY_BASE_DELAY_MS * (1 << (attempt - 1));
            let _ = tx
                .send(Ok(StreamEvent::ConnectionPhase {
                    phase: crate::message::ConnectionPhase::Retrying {
                        attempt: attempt + 1,
                        max: MAX_RETRIES,
                    },
                }))
                .await;
            if let Some(signal) = cancel_signal.as_ref() {
                tokio::select! {
                    _ = signal.notified() => {
                        let _ = tx.send(Err(anyhow::anyhow!("request cancelled by user interrupt"))).await;
                        return;
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(delay)) => {}
                }
            } else {
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            }
            crate::logging::info(&format!(
                "Retrying Anthropic API request (attempt {}/{})",
                attempt + 1,
                MAX_RETRIES
            ));
        }

        match stream_response(
            client.clone(),
            token.clone(),
            is_oauth,
            request.clone(),
            tx.clone(),
            &model_name,
            &oauth_session_id,
            cancel_signal.clone(),
        )
        .await
        {
            Ok(()) => return, // Success
            Err(e) => {
                let error_str = e.to_string().to_lowercase();

                // OAuth auth failures: force refresh and retry once immediately.
                if is_oauth && is_oauth_auth_error(&error_str) && !attempted_forced_refresh {
                    attempted_forced_refresh = true;
                    crate::logging::info(
                        "Anthropic OAuth authentication failed, forcing token refresh...",
                    );
                    let _ = tx
                        .send(Ok(StreamEvent::ConnectionPhase {
                            phase: crate::message::ConnectionPhase::Authenticating,
                        }))
                        .await;
                    match force_refresh_oauth_token(Arc::clone(&credentials)).await {
                        Ok(refreshed_token) => {
                            crate::logging::info(
                                "Forced OAuth token refresh succeeded, retrying request.",
                            );
                            token = refreshed_token;
                            last_error = Some(e);
                            continue;
                        }
                        Err(refresh_err) => {
                            let _ = tx
                                .send(Err(anyhow::anyhow!(
                                    "{}\n\nAutomatic Claude OAuth refresh failed: {}\nRun `jcode login --provider claude` (preferred) or `claude`, then retry.",
                                    e,
                                    refresh_err
                                )))
                                .await;
                            return;
                        }
                    }
                }

                // Check if this is a transient/retryable error
                if is_retryable_error(&error_str) && attempt + 1 < MAX_RETRIES {
                    crate::logging::info(&format!("Transient error, will retry: {}", e));
                    last_error = Some(e);
                    continue;
                }

                // Non-retryable or final attempt
                if is_oauth && is_oauth_auth_error(&error_str) {
                    let _ = tx
                        .send(Err(anyhow::anyhow!(
                            "{}\n\nClaude OAuth authentication failed. Run `jcode login --provider claude` (preferred) or `claude`, then retry.",
                            e
                        )))
                        .await;
                } else {
                    let _ = tx.send(Err(e)).await;
                }
                return;
            }
        }
    }

    // All retries exhausted
    if let Some(e) = last_error {
        let _ = tx
            .send(Err(anyhow::anyhow!(
                "Failed after {} retries: {}",
                MAX_RETRIES,
                e
            )))
            .await;
    }
}

async fn force_refresh_oauth_token(
    credentials: Arc<RwLock<Option<CachedCredentials>>>,
) -> Result<String> {
    let refresh_from_cache = {
        let cached = credentials.read().await;
        cached
            .as_ref()
            .map(|c| c.refresh_token.clone())
            .filter(|t| !t.is_empty())
    };

    let refresh_token = if let Some(token) = refresh_from_cache {
        token
    } else {
        let loaded = auth::claude::load_credentials()
            .context("Failed to load Claude credentials for forced refresh")?;
        if loaded.refresh_token.is_empty() {
            anyhow::bail!("No refresh token available in Claude credentials");
        }
        loaded.refresh_token
    };

    let active_label =
        auth::claude::active_account_label().unwrap_or_else(auth::claude::primary_account_label);
    let refreshed =
        match oauth::refresh_claude_tokens_for_account(&refresh_token, &active_label).await {
            Ok(refreshed) => refreshed,
            Err(err) => {
                anyhow::bail!("OAuth refresh endpoint rejected the refresh token: {err:#}");
            }
        };

    {
        let mut cached = credentials.write().await;
        *cached = Some(CachedCredentials {
            access_token: refreshed.access_token.clone(),
            refresh_token: refreshed.refresh_token,
            expires_at: refreshed.expires_at,
        });
    }

    Ok(refreshed.access_token)
}

/// Stream the response from Anthropic API
async fn stream_response(
    client: Client,
    token: String,
    is_oauth: bool,
    request: ApiRequest,
    tx: mpsc::Sender<Result<StreamEvent>>,
    model_name: &str,
    oauth_session_id: &str,
    cancel_signal: Option<jcode_agent_runtime::InterruptSignal>,
) -> Result<()> {
    use crate::message::ConnectionPhase;
    if std::env::var("JCODE_ANTHROPIC_DEBUG")
        .map(|v| v == "1")
        .unwrap_or(false)
        && let Ok(json) = serde_json::to_string_pretty(&request)
    {
        crate::logging::info(&format!("Anthropic request payload:\n{}", json));
    }

    let _ = tx
        .send(Ok(StreamEvent::ConnectionPhase {
            phase: ConnectionPhase::Connecting,
        }))
        .await;

    let connect_start = std::time::Instant::now();
    // Build request with appropriate auth headers
    let url = if is_oauth { API_URL_OAUTH } else { API_URL };

    let mut req = client
        .post(url)
        .header("anthropic-version", API_VERSION)
        .header("content-type", "application/json")
        .header(
            "accept",
            if is_oauth {
                "application/json"
            } else {
                "text/event-stream"
            },
        );

    if is_oauth {
        // OAuth tokens require:
        // 1. Bearer auth (NOT x-api-key)
        // 2. User-Agent matching Claude CLI
        // 3. Multiple beta headers
        // 4. ?beta=true query param (in URL above)
        req = apply_oauth_attribution_headers(
            req.header("Authorization", format!("Bearer {}", token))
                .header("User-Agent", CLAUDE_CLI_USER_AGENT)
                .header("anthropic-beta", oauth_beta_headers(model_name)),
            oauth_session_id,
        );
    } else {
        // Direct API keys use x-api-key
        // Include prompt-caching beta header
        req = req.header("x-api-key", &token).header(
            "anthropic-beta",
            if effectively_1m(model_name) {
                "prompt-caching-2024-07-31,context-1m-2025-08-07"
            } else {
                "prompt-caching-2024-07-31"
            },
        );
    }

    let send_fut = req.json(&request).send();
    let response = if let Some(signal) = cancel_signal.as_ref() {
        tokio::select! {
            _ = signal.notified() => anyhow::bail!("request cancelled by user interrupt"),
            response = send_fut => response,
        }
    } else {
        send_fut.await
    }
    .context("Failed to send request to Anthropic API")?;

    let connect_ms = connect_start.elapsed().as_millis();
    crate::logging::info(&format!(
        "HTTP connection established in {}ms (status={})",
        connect_ms,
        response.status()
    ));

    if !response.status().is_success() {
        let status = response.status();
        let error_text = crate::util::http_error_body(response, "HTTP error").await;
        anyhow::bail!("Anthropic API error ({}): {}", status, error_text);
    }

    let _ = tx
        .send(Ok(StreamEvent::ConnectionPhase {
            phase: ConnectionPhase::WaitingForResponse,
        }))
        .await;

    // Parse SSE stream
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut current_tool_use: Option<ToolUseAccumulator> = None;
    let mut input_tokens: Option<u64> = None;
    let mut output_tokens: Option<u64> = None;
    let mut cache_read_input_tokens: Option<u64> = None;
    let mut cache_creation_input_tokens: Option<u64> = None;
    let mut saw_stream_event = false;
    let mut saw_message_end = false;

    const SSE_CHUNK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

    loop {
        let next_fut = tokio::time::timeout(SSE_CHUNK_TIMEOUT, stream.next());
        let next_result = if let Some(signal) = cancel_signal.as_ref() {
            tokio::select! {
                _ = signal.notified() => anyhow::bail!("stream cancelled by user interrupt"),
                result = next_fut => result,
            }
        } else {
            next_fut.await
        };
        let chunk = match next_result {
            Ok(Some(chunk_result)) => chunk_result.context("Error reading stream chunk")?,
            Ok(None) => break, // stream ended normally
            Err(_) => {
                crate::logging::warn("Anthropic SSE stream timed out (no data for 180s)");
                anyhow::bail!("Stream read timeout: no data received for 180 seconds");
            }
        };
        let chunk_str = String::from_utf8_lossy(&chunk);
        buffer.push_str(&chunk_str);

        // Process complete SSE events
        while let Some(event) = parse_sse_event(&mut buffer) {
            saw_stream_event = true;
            let events = process_sse_event(
                &event,
                &mut current_tool_use,
                &mut input_tokens,
                &mut output_tokens,
                &mut cache_read_input_tokens,
                &mut cache_creation_input_tokens,
                is_oauth,
                &mut saw_message_end,
            );
            for stream_event in events {
                if let StreamEvent::Error { ref message, .. } = stream_event
                    && is_retryable_error(&message.to_lowercase())
                {
                    anyhow::bail!("Retryable stream error: {}", message);
                }
                if tx.send(Ok(stream_event)).await.is_err() {
                    return Ok(()); // Receiver dropped
                }
            }
        }
    }

    // Send final token usage if we have it
    if input_tokens.is_some() || output_tokens.is_some() {
        // Log cache usage for debugging
        if cache_read_input_tokens.is_some() || cache_creation_input_tokens.is_some() {
            crate::logging::info(&format!(
                "Prompt cache: read={:?} created={:?}",
                cache_read_input_tokens, cache_creation_input_tokens
            ));
        }
        let _ = tx
            .send(Ok(StreamEvent::TokenUsage {
                input_tokens,
                output_tokens,
                cache_read_input_tokens,
                cache_creation_input_tokens,
            }))
            .await;
    }

    if let Some(message_end) = anthropic_message_end_for_eof(saw_stream_event, saw_message_end) {
        crate::logging::warn(
            "Anthropic SSE stream ended without message_delta stop_reason or message_stop; emitting synthetic MessageEnd",
        );
        let _ = tx.send(Ok(message_end)).await;
    }

    Ok(())
}

fn anthropic_message_end_for_eof(
    saw_stream_event: bool,
    saw_message_end: bool,
) -> Option<StreamEvent> {
    if saw_stream_event && !saw_message_end {
        Some(StreamEvent::MessageEnd { stop_reason: None })
    } else {
        None
    }
}

/// Check if an error is transient and should be retried
fn is_retryable_error(error_str: &str) -> bool {
    crate::provider::is_transient_transport_error(error_str)
        // Server errors (5xx)
        || error_str.contains("500 internal server error")
        || error_str.contains("502 bad gateway")
        || error_str.contains("503 service unavailable")
        || error_str.contains("504 gateway timeout")
        || error_str.contains("overloaded")
        // Rate limiting (429)
        || error_str.contains("429 too many requests")
        || error_str.contains("rate limit")
        || error_str.contains("rate_limit")
        // API-level server errors (SSE error events)
        || error_str.contains("api_error")
        || error_str.contains("internal server error")
}

fn is_oauth_auth_error(error_str: &str) -> bool {
    error_str.contains("oauth token has expired")
        || error_str.contains("token has expired")
        || error_str.contains("authentication_error")
        || error_str.contains("invalid token")
        || error_str.contains("invalid_grant")
        || error_str.contains("does not meet scope requirement")
        || ((error_str.contains("401 unauthorized") || error_str.contains("403 forbidden"))
            && (error_str.contains("oauth") || error_str.contains("token")))
}

/// Accumulator for tool_use blocks (input comes in chunks)
struct ToolUseAccumulator {
    input_json: String,
}

/// Parse a single SSE event from the buffer
fn parse_sse_event(buffer: &mut String) -> Option<SseEvent> {
    // Look for complete event (ends with double newline)
    let event_end = buffer.find("\n\n")?;
    let event_str = buffer[..event_end].to_string();
    buffer.drain(..event_end + 2);

    let mut event_type = String::new();
    let mut data = String::new();

    for line in event_str.lines() {
        if let Some(rest) = line.strip_prefix("event: ") {
            event_type = rest.to_string();
        } else if let Some(rest) = crate::util::sse_data_line(line) {
            data = rest.to_string();
        }
    }

    if event_type.is_empty() && data.is_empty() {
        return None;
    }

    Some(SseEvent { event_type, data })
}

/// SSE event from the stream
struct SseEvent {
    event_type: String,
    data: String,
}

fn anthropic_text_or_recovered_tool_events(text: String) -> Vec<StreamEvent> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    if is_count_wrapper_noise(&text) {
        return Vec::new();
    }

    if let Some((prefix, tool_name, arguments, suffix)) = parse_xml_wrapped_tool_call(&text) {
        crate::logging::warn(&format!(
            "[anthropic] Recovered XML text-wrapped tool call for '{}'",
            tool_name
        ));
        let mut events = Vec::new();
        if !prefix.is_empty() {
            events.push(StreamEvent::TextDelta(prefix));
        }
        events.push(StreamEvent::ToolUseStart {
            id: format!("fallback_xml_call_{}", Uuid::new_v4()),
            name: tool_name,
        });
        events.push(StreamEvent::ToolInputDelta(arguments.to_string()));
        events.push(StreamEvent::ToolUseEnd);
        if !suffix.is_empty() {
            events.push(StreamEvent::TextDelta(suffix));
        }
        return events;
    }

    vec![StreamEvent::TextDelta(text)]
}

fn is_tool_call_wrapper_noise_line(line: &str) -> bool {
    line.eq_ignore_ascii_case("count") || line.eq_ignore_ascii_case("call")
}

fn is_count_wrapper_noise(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty()
        && trimmed
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .all(is_tool_call_wrapper_noise_line)
}

fn parse_xml_wrapped_tool_call(text: &str) -> Option<(String, String, Value, String)> {
    let invoke_start = text.find("<invoke")?;
    let tag_end_rel = text[invoke_start..].find('>')?;
    let open_tag_end = invoke_start + tag_end_rel + 1;
    let open_tag = &text[invoke_start..open_tag_end];
    let tool_name = parse_xml_attr(open_tag, "name")?;
    let tool_name = tool_name
        .strip_prefix("functions.")
        .or_else(|| tool_name.strip_prefix("tools."))
        .unwrap_or(&tool_name)
        .to_string();
    if tool_name.trim().is_empty() {
        return None;
    }

    let close_tag = "</invoke>";
    let close_rel = text[open_tag_end..].find(close_tag);
    let close_start = close_rel
        .map(|rel| open_tag_end + rel)
        .unwrap_or_else(|| text.len());
    let inner = &text[open_tag_end..close_start];
    let after_close = close_rel
        .map(|_| close_start + close_tag.len())
        .unwrap_or(close_start);

    let arguments = parse_xml_invoke_arguments(inner)?;
    let prefix = sanitize_recovered_tool_prefix(&text[..invoke_start]);
    let suffix = text[after_close..].trim().to_string();
    Some((prefix, tool_name, arguments, suffix))
}

fn sanitize_recovered_tool_prefix(prefix: &str) -> String {
    let trimmed = prefix.trim();
    if trimmed.is_empty() || is_count_wrapper_noise(trimmed) {
        String::new()
    } else {
        trimmed.to_string()
    }
}

fn parse_xml_attr(tag: &str, attr: &str) -> Option<String> {
    let mut search_start = 0usize;
    while let Some(rel_idx) = tag[search_start..].find(attr) {
        let idx = search_start + rel_idx;
        let before_ok = idx == 0
            || tag[..idx]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_whitespace() || ch == '<');
        let after_attr = &tag[idx + attr.len()..];
        let after_eq = after_attr.trim_start();
        if !before_ok || !after_eq.starts_with('=') {
            search_start = idx + attr.len();
            continue;
        }
        let value = after_eq[1..].trim_start();
        let quote = value.chars().next()?;
        let quote = match quote {
            '"' | '\'' => quote,
            '“' | '”' => '”',
            '‘' | '’' => '’',
            _ => return None,
        };
        let value_body = &value[value.chars().next()?.len_utf8()..];
        let end = value_body.find(quote)?;
        return Some(xml_unescape(&value_body[..end]));
    }

    None
}

fn parse_xml_invoke_arguments(inner: &str) -> Option<Value> {
    let trimmed = inner.trim();
    if trimmed.starts_with('{') {
        if let Ok(value) = serde_json::from_str::<Value>(trimmed)
            && value.is_object()
        {
            return Some(value);
        }
    }

    let mut map = serde_json::Map::new();
    let mut cursor = 0usize;
    while let Some(param_rel) = inner[cursor..].find("<parameter") {
        let param_start = cursor + param_rel;
        let tag_end_rel = inner[param_start..].find('>')?;
        let open_tag_end = param_start + tag_end_rel + 1;
        let open_tag = &inner[param_start..open_tag_end];
        let name = parse_xml_attr(open_tag, "name")?;
        let close_tag = "</parameter>";
        let close_rel = inner[open_tag_end..].find(close_tag)?;
        let close_start = open_tag_end + close_rel;
        let raw_value = inner[open_tag_end..close_start].trim();
        map.insert(name, xml_parameter_value(raw_value));
        cursor = close_start + close_tag.len();
    }

    if map.is_empty() {
        None
    } else {
        Some(Value::Object(map))
    }
}

fn xml_parameter_value(raw: &str) -> Value {
    let unescaped = xml_unescape(raw);
    let trimmed = unescaped.trim();
    if trimmed.is_empty() {
        return Value::String(String::new());
    }
    serde_json::from_str::<Value>(trimmed).unwrap_or_else(|_| Value::String(unescaped))
}

fn xml_unescape(raw: &str) -> String {
    raw.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Process an SSE event and return StreamEvents if applicable
fn process_sse_event(
    event: &SseEvent,
    current_tool_use: &mut Option<ToolUseAccumulator>,
    input_tokens: &mut Option<u64>,
    output_tokens: &mut Option<u64>,
    cache_read_input_tokens: &mut Option<u64>,
    cache_creation_input_tokens: &mut Option<u64>,
    is_oauth: bool,
    saw_message_end: &mut bool,
) -> Vec<StreamEvent> {
    let mut events = Vec::new();

    match event.event_type.as_str() {
        "message_start" => {
            // Extract usage from message_start (includes cache info)
            if let Ok(parsed) = serde_json::from_str::<MessageStartEvent>(&event.data)
                && let Some(usage) = parsed.message.usage
            {
                *input_tokens = usage.input_tokens.map(|t| t as u64);
                *cache_read_input_tokens = usage.cache_read_input_tokens.map(|t| t as u64);
                *cache_creation_input_tokens = usage.cache_creation_input_tokens.map(|t| t as u64);
            }
        }
        "content_block_start" => {
            if let Ok(parsed) = serde_json::from_str::<ContentBlockStartEvent>(&event.data) {
                match parsed.content_block {
                    ApiContentBlockStart::Text { .. } => {
                        // Text block starting - nothing to emit yet
                    }
                    ApiContentBlockStart::ToolUse { id, name } => {
                        let mapped_name = if is_oauth {
                            map_tool_name_from_oauth(&name)
                        } else {
                            name.clone()
                        };
                        // Start accumulating tool use
                        *current_tool_use = Some(ToolUseAccumulator {
                            input_json: String::new(),
                        });
                        events.push(StreamEvent::ToolUseStart {
                            id,
                            name: mapped_name,
                        });
                    }
                }
            }
        }
        "content_block_delta" => {
            if let Ok(parsed) = serde_json::from_str::<ContentBlockDeltaEvent>(&event.data) {
                match parsed.delta {
                    ApiDelta::TextDelta { text } => {
                        events.extend(anthropic_text_or_recovered_tool_events(text));
                    }
                    ApiDelta::InputJsonDelta { partial_json } => {
                        if let Some(tool) = current_tool_use {
                            tool.input_json.push_str(&partial_json);
                        }
                        events.push(StreamEvent::ToolInputDelta(partial_json));
                    }
                }
            }
        }
        "content_block_stop" => {
            // If we were accumulating a tool_use, it's complete now
            if current_tool_use.take().is_some() {
                events.push(StreamEvent::ToolUseEnd);
            }
        }
        "message_delta" => {
            if let Ok(parsed) = serde_json::from_str::<MessageDeltaEvent>(&event.data) {
                if let Some(usage) = parsed.usage {
                    *output_tokens = usage.output_tokens.map(|t| t as u64);
                }
                if let Some(stop_reason) = parsed.delta.stop_reason {
                    if !*saw_message_end {
                        *saw_message_end = true;
                        events.push(StreamEvent::MessageEnd {
                            stop_reason: Some(stop_reason),
                        });
                    }
                }
            }
        }
        "message_stop" => {
            // Final message stop. Anthropic usually sends stop_reason in the
            // preceding message_delta, but some transports/surfaces can omit it.
            // Ensure the agent/UI always sees a terminal event.
            if !*saw_message_end {
                *saw_message_end = true;
                events.push(StreamEvent::MessageEnd { stop_reason: None });
            }
        }
        "ping" => {
            // Keepalive, ignore
        }
        "error" => {
            crate::logging::error(&format!("Anthropic stream error: {}", event.data));
            events.push(StreamEvent::Error {
                message: event.data.clone(),
                retry_after_secs: None,
            });
        }
        _ => {
            // Unknown event type, ignore
        }
    }

    events
}

// ============================================================================
// API Types
// ============================================================================

#[derive(Serialize, Clone)]
struct ApiRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<ApiSystem>,
    messages: Vec<ApiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ApiTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<ApiMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ApiThinking>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_config: Option<ApiOutputConfig>,
    stream: bool,
}

#[derive(Serialize, Clone, Copy)]
struct ApiThinking {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    budget_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    display: Option<&'static str>,
}

#[derive(Serialize, Clone, Copy)]
struct ApiOutputConfig {
    effort: &'static str,
}

#[derive(Serialize, Clone)]
struct ApiMetadata {
    user_id: String,
}

#[derive(Serialize, Clone)]
#[serde(untagged)]
enum ApiSystem {
    Blocks(Vec<ApiSystemBlock>),
}

/// Cache control for prompt caching
#[derive(Serialize, Clone)]
struct CacheControlParam {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    ttl: Option<&'static str>,
}

impl CacheControlParam {
    fn ephemeral() -> Self {
        if is_cache_ttl_1h() {
            Self::ephemeral_1h()
        } else {
            Self {
                kind: "ephemeral",
                ttl: None,
            }
        }
    }

    fn ephemeral_1h() -> Self {
        Self {
            kind: "ephemeral",
            ttl: Some("1h"),
        }
    }
}

#[derive(Serialize, Clone)]
struct ApiSystemBlock {
    #[serde(rename = "type")]
    block_type: &'static str,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControlParam>,
}

fn build_system_param(system: &str, is_oauth: bool) -> Option<ApiSystem> {
    build_system_param_split(system, "", is_oauth)
}

/// Build system param with split static/dynamic content for better caching
fn build_system_param_split(
    static_part: &str,
    dynamic_part: &str,
    is_oauth: bool,
) -> Option<ApiSystem> {
    if is_oauth {
        let mut blocks = Vec::new();
        blocks.push(ApiSystemBlock {
            block_type: "text",
            text: format!("x-anthropic-billing-header: {}", OAUTH_BILLING_HEADER),
            cache_control: None,
        });
        blocks.push(ApiSystemBlock {
            block_type: "text",
            text: CLAUDE_CODE_IDENTITY.to_string(),
            cache_control: None,
        });
        // Static content - CACHED (instruction files, base prompt, skills)
        if !static_part.is_empty() {
            blocks.push(ApiSystemBlock {
                block_type: "text",
                text: static_part.to_string(),
                cache_control: Some(CacheControlParam::ephemeral()),
            });
        }
        // Dynamic content - NOT cached (date, git status, memory)
        if !dynamic_part.is_empty() {
            blocks.push(ApiSystemBlock {
                block_type: "text",
                text: dynamic_part.to_string(),
                cache_control: None,
            });
        }
        return Some(ApiSystem::Blocks(blocks));
    }

    // Non-OAuth: use block format with cache control for static part only
    let has_static = !static_part.is_empty();
    let has_dynamic = !dynamic_part.is_empty();

    if !has_static && !has_dynamic {
        None
    } else {
        let mut blocks = Vec::new();
        if has_static {
            blocks.push(ApiSystemBlock {
                block_type: "text",
                text: static_part.to_string(),
                cache_control: Some(CacheControlParam::ephemeral()),
            });
        }
        if has_dynamic {
            blocks.push(ApiSystemBlock {
                block_type: "text",
                text: dynamic_part.to_string(),
                cache_control: None,
            });
        }
        Some(ApiSystem::Blocks(blocks))
    }
}

fn format_messages_with_identity(messages: Vec<ApiMessage>, _is_oauth: bool) -> Vec<ApiMessage> {
    let mut out = messages;

    // Add cache breakpoints for both OAuth and non-OAuth paths
    add_message_cache_breakpoint(&mut out);

    out
}

/// Add cache_control to messages for conversation caching.
///
/// Strategy: sliding two-marker window
///   - Second-to-last assistant message → READ marker (re-uses cache snapshot from previous turn)
///   - Last assistant message           → WRITE marker (creates new snapshot for the next turn)
///
/// This ensures each turn N+1 reads from turn N's conversation cache, paying only
/// cache_read_input_tokens for the already-cached history instead of full input tokens.
///
/// Budget: system (1) + tools (1) + messages (up to 2) = 4 total, within Anthropic's limit.
fn add_message_cache_breakpoint(messages: &mut [ApiMessage]) {
    crate::logging::info(&format!(
        "Conversation caching: {} messages to process",
        messages.len()
    ));

    if messages.len() < 3 {
        // Need at least: user + assistant + user to be worth caching
        crate::logging::info("Conversation caching: too few messages, skipping");
        return;
    }

    // Collect indices of up to 2 most recent assistant messages (newest first)
    let mut assistant_indices: Vec<usize> = Vec::with_capacity(2);
    for (i, msg) in messages.iter().enumerate().rev() {
        if msg.role == "assistant" {
            assistant_indices.push(i);
            if assistant_indices.len() == 2 {
                break;
            }
        }
    }

    if assistant_indices.is_empty() {
        crate::logging::info("Conversation caching: no assistant message found");
        return;
    }

    // Place cache_control on both (newest = WRITE for next turn, older = READ from prev turn)
    let total = assistant_indices.len();
    for (slot, &idx) in assistant_indices.iter().enumerate() {
        let label = if slot == 0 {
            "WRITE (newest)"
        } else {
            "READ (prev-turn)"
        };
        let mut added = false;
        if let Some(msg) = messages.get_mut(idx) {
            for block in msg.content.iter_mut().rev() {
                match block {
                    ApiContentBlock::Text { cache_control, .. }
                    | ApiContentBlock::ToolUse { cache_control, .. } => {
                        *cache_control = Some(CacheControlParam::ephemeral());
                        added = true;
                        break;
                    }
                    _ => {}
                }
            }
        }
        if added {
            crate::logging::info(&format!(
                "Conversation caching: breakpoint {}/{} at message {} [{}]",
                slot + 1,
                total,
                idx,
                label
            ));
        } else {
            crate::logging::info(&format!(
                "Conversation caching: no cacheable block in assistant message {} [{}]",
                idx, label
            ));
        }
    }
}

#[derive(Serialize, Clone)]
struct ApiMessage {
    role: String,
    content: Vec<ApiContentBlock>,
}

#[derive(Serialize, Clone)]
#[serde(tag = "type")]
enum ApiContentBlock {
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControlParam>,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControlParam>,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: ToolResultContent,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        is_error: bool,
    },
    #[serde(rename = "image")]
    Image { source: ApiImageSource },
}

#[derive(Serialize, Clone)]
#[serde(untagged)]
enum ToolResultContent {
    Text(String),
    Blocks(Vec<ToolResultContentBlock>),
}

#[derive(Serialize, Clone)]
#[serde(tag = "type")]
enum ToolResultContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { source: ApiImageSource },
}

#[derive(Serialize, Clone)]
struct ApiImageSource {
    #[serde(rename = "type")]
    kind: String,
    media_type: String,
    data: String,
}

#[derive(Serialize, Clone)]
struct ApiTool {
    name: String,
    description: String,
    input_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControlParam>,
}

// Response types for SSE parsing

#[derive(Deserialize)]
struct MessageStartEvent {
    message: MessageStartMessage,
}

#[derive(Deserialize)]
struct MessageStartMessage {
    usage: Option<UsageInfo>,
}

#[derive(Deserialize)]
struct ContentBlockStartEvent {
    #[serde(rename = "index")]
    _index: u32,
    content_block: ApiContentBlockStart,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ApiContentBlockStart {
    #[serde(rename = "text")]
    Text {
        #[serde(rename = "text")]
        _text: String,
    },
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String },
}

#[derive(Deserialize)]
struct ContentBlockDeltaEvent {
    #[serde(rename = "index")]
    _index: u32,
    delta: ApiDelta,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ApiDelta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
}

#[derive(Deserialize)]
struct MessageDeltaEvent {
    delta: MessageDeltaDelta,
    usage: Option<UsageInfo>,
}

#[derive(Deserialize)]
struct MessageDeltaDelta {
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct UsageInfo {
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
    cache_read_input_tokens: Option<u32>,
    cache_creation_input_tokens: Option<u32>,
}

#[cfg(test)]
#[path = "anthropic_tests.rs"]
mod tests;
