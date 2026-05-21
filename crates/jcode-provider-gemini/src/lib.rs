use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

pub const DEFAULT_MODEL: &str = "gemini-2.5-pro";
pub const AVAILABLE_MODELS: &[&str] = &[
    "gemini-3.5-pro",
    "gemini-3.5-flash",
    "gemini-3.1-pro",
    "gemini-3.1-flash",
    "gemini-3-pro-preview",
    "gemini-3-flash-preview",
    "gemini-2.5-pro",
    "gemini-2.5-flash",
    "gemini-2.0-flash",
    "gemini-1.5-pro",
    "gemini-1.5-flash",
];
pub const FALLBACK_MODELS: &[&str] = &[
    "gemini-3.5-pro",
    "gemini-3.5-flash",
    "gemini-3.1-pro",
    "gemini-3.1-flash",
    "gemini-3-pro-preview",
    "gemini-2.5-pro",
    "gemini-3-flash-preview",
    "gemini-2.5-flash",
    "gemini-2.0-flash",
];
pub const CODE_ASSIST_ENDPOINT: &str = "https://cloudcode-pa.googleapis.com";
pub const CODE_ASSIST_API_VERSION: &str = "v1internal";
pub const USER_TIER_FREE: &str = "free-tier";
pub const USER_TIER_LEGACY: &str = "legacy-tier";

#[derive(Debug, Clone)]
pub struct GeminiRuntimeState {
    pub project_id: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientMetadata {
    pub ide_type: &'static str,
    pub platform: &'static str,
    pub plugin_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duet_project: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadCodeAssistRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloudaicompanion_project: Option<String>,
    pub metadata: ClientMetadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<&'static str>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadCodeAssistResponse {
    #[serde(default)]
    pub current_tier: Option<GeminiUserTier>,
    #[serde(default)]
    pub allowed_tiers: Option<Vec<GeminiUserTier>>,
    #[serde(default)]
    pub ineligible_tiers: Option<Vec<IneligibleTier>>,
    #[serde(default)]
    pub cloudaicompanion_project: Option<String>,
    #[serde(default)]
    pub paid_tier: Option<GeminiUserTier>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiUserTier {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub is_default: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IneligibleTier {
    #[serde(default)]
    pub reason_code: Option<String>,
    #[serde(default)]
    pub reason_message: Option<String>,
    #[serde(default)]
    pub validation_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardUserRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloudaicompanion_project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ClientMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LongRunningOperationResponse {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub done: Option<bool>,
    #[serde(default)]
    pub response: Option<OnboardUserResponse>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardUserResponse {
    #[serde(default)]
    pub cloudaicompanion_project: Option<ProjectRef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectRef {
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodeAssistGenerateRequest {
    pub model: String,
    pub project: String,
    pub user_prompt_id: String,
    pub request: VertexGenerateContentRequest,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VertexGenerateContentRequest {
    pub contents: Vec<GeminiContent>,
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<GeminiContent>,
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GeminiGenerationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<GeminiTool>>,
    #[serde(rename = "toolConfig", skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<GeminiToolConfig>,
    #[serde(rename = "session_id", skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_config: Option<GeminiThinkingConfig>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiThinkingConfig {
    pub include_thoughts: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gemini3ModelConfig {
    pub model: String,
    pub thinking_level: Option<String>,
}

fn strip_gemini3_tier(model: &str) -> (&str, Option<&str>) {
    for tier in ["minimal", "low", "medium", "high"] {
        if let Some(base) = model.strip_suffix(&format!("-{tier}")) {
            return (base, Some(tier));
        }
    }
    (model, None)
}

fn is_gemini3_dotted_minor(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    lower.starts_with("gemini-3.")
}

fn is_gemini3_legacy_line(model: &str) -> bool {
    matches!(model, "gemini-3-pro" | "gemini-3-flash")
}

pub fn normalize_gemini_cli_model_for_api(model: &str) -> String {
    let mut normalized = model.trim().to_ascii_lowercase();
    if normalized.ends_with("-preview-customtools") {
        return normalized;
    }
    if is_gemini3_dotted_minor(&normalized)
        && let Some(base) = normalized.strip_suffix("-preview")
    {
        normalized = base.to_string();
    } else if is_gemini3_legacy_line(&normalized) {
        normalized.push_str("-preview");
    }
    normalized
}

pub fn normalize_antigravity_model_for_api(model: &str) -> Gemini3ModelConfig {
    let raw = model.trim().to_ascii_lowercase();
    let mut normalized = raw
        .strip_prefix("antigravity-")
        .or_else(|| raw.strip_prefix("agy-"))
        .unwrap_or(raw.as_str())
        .to_string();

    if normalized.ends_with("-preview-customtools") {
        normalized.truncate(normalized.len() - "-preview-customtools".len());
    } else if normalized.ends_with("-preview") {
        normalized.truncate(normalized.len() - "-preview".len());
    }

    let (base, tier) = strip_gemini3_tier(&normalized);
    let is_gemini3_pro = base.starts_with("gemini-3") && base.contains("-pro");
    let is_gemini3_flash = base.starts_with("gemini-3") && base.contains("-flash");

    if is_gemini3_flash {
        if base.starts_with("gemini-3.") && tier.is_some() {
            return Gemini3ModelConfig {
                model: normalized.clone(),
                thinking_level: tier.map(str::to_string),
            };
        }
        return Gemini3ModelConfig {
            model: base.to_string(),
            thinking_level: Some(tier.unwrap_or("low").to_string()),
        };
    }

    if is_gemini3_pro {
        let tier = tier.unwrap_or("low");
        return Gemini3ModelConfig {
            model: format!("{base}-{tier}"),
            thinking_level: Some(tier.to_string()),
        };
    }

    Gemini3ModelConfig {
        model: normalized,
        thinking_level: None,
    }
}

pub fn gemini3_thinking_generation_config(
    thinking_level: Option<&str>,
    include_thoughts: bool,
) -> Option<GeminiGenerationConfig> {
    thinking_level.map(|level| GeminiGenerationConfig {
        thinking_config: Some(GeminiThinkingConfig {
            include_thoughts,
            thinking_level: Some(level.to_string()),
            thinking_budget: None,
        }),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiContent {
    #[serde(default)]
    pub role: String,
    pub parts: Vec<GeminiPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiPart {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_data: Option<InlineData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_call: Option<GeminiFunctionCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_response: Option<GeminiFunctionResponse>,
    #[serde(rename = "thoughtSignature", skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineData {
    pub mime_type: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiFunctionCall {
    pub name: String,
    #[serde(default)]
    pub args: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiFunctionResponse {
    pub name: String,
    pub response: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GeminiTool {
    #[serde(rename = "functionDeclarations")]
    pub function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GeminiFunctionDeclaration {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiToolConfig {
    pub function_calling_config: GeminiFunctionCallingConfig,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiFunctionCallingConfig {
    pub mode: &'static str,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeAssistGenerateResponse {
    #[serde(default)]
    pub trace_id: Option<String>,
    #[serde(default)]
    pub response: Option<VertexGenerateContentResponse>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VertexGenerateContentResponse {
    #[serde(default)]
    pub candidates: Option<Vec<GeminiCandidate>>,
    #[serde(default)]
    pub prompt_feedback: Option<GeminiPromptFeedback>,
    #[serde(default)]
    pub usage_metadata: Option<GeminiUsageMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiCandidate {
    #[serde(default)]
    pub content: Option<GeminiContent>,
    #[serde(default)]
    pub finish_reason: Option<String>,
    #[serde(default)]
    pub finish_message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiPromptFeedback {
    #[serde(default)]
    pub block_reason: Option<String>,
    #[serde(default)]
    pub block_reason_message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiUsageMetadata {
    #[serde(default)]
    pub prompt_token_count: Option<u64>,
    #[serde(default)]
    pub candidates_token_count: Option<u64>,
    #[serde(default)]
    pub cached_content_token_count: Option<u64>,
}

pub fn gemini_fallback_models(current_model: &str) -> Vec<&'static str> {
    FALLBACK_MODELS
        .iter()
        .copied()
        .filter(|candidate| !candidate.eq_ignore_ascii_case(current_model))
        .collect()
}

pub fn google_cloud_project_from_env() -> Option<String> {
    std::env::var("GOOGLE_CLOUD_PROJECT")
        .ok()
        .or_else(|| std::env::var("GOOGLE_CLOUD_PROJECT_ID").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn load_code_assist_request(
    project_id: Option<String>,
    metadata: ClientMetadata,
) -> LoadCodeAssistRequest {
    LoadCodeAssistRequest {
        cloudaicompanion_project: project_id,
        metadata,
        mode: None,
    }
}

pub fn merge_gemini_model_lists(models: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut preferred = Vec::new();

    for known in AVAILABLE_MODELS {
        if models.iter().any(|model| model == known) && seen.insert((*known).to_string()) {
            preferred.push((*known).to_string());
        }
    }

    let mut extras: Vec<String> = models
        .into_iter()
        .map(|model| model.trim().to_string())
        .filter(|model| is_gemini_model_id(model) && seen.insert(model.clone()))
        .collect();
    extras.sort();
    preferred.extend(extras);
    preferred
}

pub fn extract_gemini_model_ids(value: &Value) -> Vec<String> {
    let mut found = HashSet::new();
    collect_gemini_model_ids(value, &mut found);
    merge_gemini_model_lists(found.into_iter().collect())
}

fn collect_gemini_model_ids(value: &Value, found: &mut HashSet<String>) {
    match value {
        Value::String(raw) => {
            let trimmed = raw.trim();
            if is_gemini_model_id(trimmed) {
                found.insert(trimmed.to_string());
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_gemini_model_ids(item, found);
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                collect_gemini_model_ids(item, found);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

pub fn is_gemini_model_id(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && trimmed.starts_with("gemini-")
        && trimmed
            .bytes()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_'))
}

pub fn client_metadata(project_id: Option<String>) -> ClientMetadata {
    ClientMetadata {
        ide_type: "IDE_UNSPECIFIED",
        platform: "PLATFORM_UNSPECIFIED",
        plugin_type: "GEMINI",
        duet_project: project_id,
    }
}

pub fn validate_load_code_assist_response(res: &LoadCodeAssistResponse) -> Result<()> {
    if res.current_tier.is_none()
        && let Some(validation) = res.ineligible_tiers.as_ref().and_then(|tiers| {
            tiers.iter().find(|tier| {
                tier.reason_code.as_deref() == Some("VALIDATION_REQUIRED")
                    && tier.validation_url.is_some()
            })
        })
    {
        let description = validation
            .reason_message
            .clone()
            .unwrap_or_else(|| "Account validation required".to_string());
        let url = validation.validation_url.clone().unwrap_or_default();
        anyhow::bail!("{description}. Complete account validation: {url}");
    }
    Ok(())
}

pub fn ineligible_or_project_error(res: &LoadCodeAssistResponse) -> anyhow::Error {
    if let Some(reasons) = res
        .ineligible_tiers
        .as_ref()
        .filter(|tiers| !tiers.is_empty())
    {
        let joined = reasons
            .iter()
            .filter_map(|tier| tier.reason_message.as_deref())
            .collect::<Vec<_>>()
            .join(", ");
        return anyhow::anyhow!(joined);
    }

    anyhow::anyhow!(
        "This Google account requires setting GOOGLE_CLOUD_PROJECT or GOOGLE_CLOUD_PROJECT_ID. See Gemini Code Assist Workspace auth docs."
    )
}

pub fn choose_onboard_tier(res: &LoadCodeAssistResponse) -> GeminiUserTier {
    if let Some(default_tier) = res.allowed_tiers.as_ref().and_then(|tiers| {
        tiers
            .iter()
            .find(|tier| tier.is_default.unwrap_or(false))
            .cloned()
    }) {
        return default_tier;
    }

    GeminiUserTier {
        id: Some(USER_TIER_LEGACY.to_string()),
        name: Some(String::new()),
        is_default: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fallback_models_skip_current_model() {
        assert_eq!(
            gemini_fallback_models("gemini-2.5-flash"),
            vec![
                "gemini-3.5-pro",
                "gemini-3.5-flash",
                "gemini-3.1-pro",
                "gemini-3.1-flash",
                "gemini-3-pro-preview",
                "gemini-2.5-pro",
                "gemini-3-flash-preview",
                "gemini-2.0-flash",
            ]
        );
    }

    #[test]
    fn extract_gemini_model_ids_discovers_nested_models() {
        let response = json!({
            "routing": {
                "manual": {
                    "models": [
                        {"id": "gemini-3-pro-preview"},
                        {"name": "gemini-3.1-pro"},
                        {"name": "gemini-3.5-flash"}
                    ]
                },
                "auto": ["gemini-3.1-flash", "gemini-3-flash-preview", "not-a-model"]
            }
        });

        assert_eq!(
            extract_gemini_model_ids(&response),
            vec![
                "gemini-3.5-flash".to_string(),
                "gemini-3.1-pro".to_string(),
                "gemini-3.1-flash".to_string(),
                "gemini-3-pro-preview".to_string(),
                "gemini-3-flash-preview".to_string(),
            ]
        );
    }

    #[test]
    fn normalizes_gemini_cli_dotted_minor_models_to_bare_names() {
        assert_eq!(
            normalize_gemini_cli_model_for_api("gemini-3.1-pro-preview"),
            "gemini-3.1-pro"
        );
        assert_eq!(
            normalize_gemini_cli_model_for_api("gemini-3.5-flash-preview"),
            "gemini-3.5-flash"
        );
        assert_eq!(
            normalize_gemini_cli_model_for_api("gemini-3-flash"),
            "gemini-3-flash-preview"
        );
        assert_eq!(
            normalize_gemini_cli_model_for_api("gemini-3.1-pro-preview-customtools"),
            "gemini-3.1-pro-preview-customtools"
        );
    }

    #[test]
    fn normalizes_antigravity_gemini3_models_to_backend_ids_and_thinking_levels() {
        assert_eq!(
            normalize_antigravity_model_for_api("antigravity-gemini-3.5-flash"),
            Gemini3ModelConfig {
                model: "gemini-3.5-flash".to_string(),
                thinking_level: Some("low".to_string()),
            }
        );
        assert_eq!(
            normalize_antigravity_model_for_api("agy-gemini-3.5-flash"),
            Gemini3ModelConfig {
                model: "gemini-3.5-flash".to_string(),
                thinking_level: Some("low".to_string()),
            }
        );
        assert_eq!(
            normalize_antigravity_model_for_api("gemini-3.5-flash-low"),
            Gemini3ModelConfig {
                model: "gemini-3.5-flash-low".to_string(),
                thinking_level: Some("low".to_string()),
            }
        );
        assert_eq!(
            normalize_antigravity_model_for_api("gemini-3.5-pro"),
            Gemini3ModelConfig {
                model: "gemini-3.5-pro-low".to_string(),
                thinking_level: Some("low".to_string()),
            }
        );
        assert_eq!(
            normalize_antigravity_model_for_api("gemini-3.1-pro-preview-customtools"),
            Gemini3ModelConfig {
                model: "gemini-3.1-pro-low".to_string(),
                thinking_level: Some("low".to_string()),
            }
        );
    }

    #[test]
    fn serializes_gemini3_thinking_generation_config() {
        let config = gemini3_thinking_generation_config(Some("low"), false)
            .expect("thinking config should be produced for Gemini 3 level");

        assert_eq!(
            serde_json::to_value(config).unwrap(),
            json!({
                "thinkingConfig": {
                    "includeThoughts": false,
                    "thinkingLevel": "low"
                }
            })
        );
        assert!(gemini3_thinking_generation_config(None, false).is_none());
    }
}
