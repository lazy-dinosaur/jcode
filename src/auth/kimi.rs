use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

pub const KIMI_CLI_VERSION: &str = "1.41.0";
pub const USER_AGENT: &str = "KimiCLI/1.41.0";
pub const OAUTH_DEVICE_AUTH_URL: &str = "https://auth.kimi.com/api/oauth/device_authorization";
pub const OAUTH_TOKEN_URL: &str = "https://auth.kimi.com/api/oauth/token";
// Public OAuth client id shipped by the official Kimi CLI.
// gitleaks:allow - public device OAuth client id, safe to embed
pub const OAUTH_CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
pub const OAUTH_DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
pub const OAUTH_REFRESH_GRANT: &str = "refresh_token";
pub const API_BASE_URL: &str = "https://api.kimi.com/coding/v1";
const REQUEST_TIMEOUT_SECS: u64 = 120;
const REFRESH_SAFETY_WINDOW_MS: i64 = 60_000;
const DEVICE_ID_FILE: &str = ".kimi/device_id";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
}

impl KimiTokens {
    pub fn is_expiring(&self) -> bool {
        self.expires_at <= chrono::Utc::now().timestamp_millis() + REFRESH_SAFETY_WINDOW_MS
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceAuth {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[serde(default)]
    pub verification_uri_complete: Option<String>,
    pub expires_in: i64,
    #[serde(default = "default_poll_interval")]
    pub interval: u64,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
}

fn default_poll_interval() -> u64 {
    5
}

pub fn tokens_path() -> Result<PathBuf> {
    Ok(crate::storage::jcode_dir()?.join("kimi_oauth.json"))
}

pub fn load_tokens() -> Result<KimiTokens> {
    let path = tokens_path()?;
    if path.exists() {
        crate::storage::harden_secret_file_permissions(&path);
        return crate::storage::read_json(&path)
            .with_context(|| format!("Failed to read {}", path.display()));
    }
    anyhow::bail!("No Kimi OAuth tokens found. Run `jcode login --provider kimi`.")
}

pub fn save_tokens(tokens: &KimiTokens) -> Result<()> {
    crate::storage::write_json_secret(&tokens_path()?, tokens)
}

pub fn has_cached_auth() -> bool {
    load_tokens().is_ok()
}

fn token_response_to_tokens(response: TokenResponse) -> KimiTokens {
    KimiTokens {
        access_token: response.access_token,
        refresh_token: response.refresh_token,
        expires_at: chrono::Utc::now().timestamp_millis() + response.expires_in * 1000,
    }
}

fn device_id_path() -> Result<PathBuf> {
    crate::storage::user_home_path(DEVICE_ID_FILE)
}

pub fn device_id() -> Result<String> {
    let path = device_id_path()?;
    if path.exists() {
        if let Ok(existing) = fs::read_to_string(&path) {
            let trimmed = existing.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    fs::write(&path, &id)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(id)
}

fn ascii_header_value(value: impl AsRef<str>, fallback: &str) -> String {
    let sanitized: String = value
        .as_ref()
        .chars()
        .filter(|ch| matches!(*ch, '\u{20}'..='\u{7e}'))
        .collect::<String>()
        .trim()
        .to_string();
    if sanitized.is_empty() {
        fallback.to_string()
    } else {
        sanitized
    }
}

fn device_model() -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match os {
        "macos" => format!("macOS {arch}"),
        "windows" => format!("Windows {arch}"),
        "linux" => format!("Linux {arch}"),
        other => format!("{other} {arch}"),
    }
}

pub fn apply_kimi_headers(req: reqwest::RequestBuilder) -> Result<reqwest::RequestBuilder> {
    let hostname = std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string());
    let os_version = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    Ok(req
        .header("User-Agent", USER_AGENT)
        .header("X-Msh-Platform", "kimi_cli")
        .header("X-Msh-Version", KIMI_CLI_VERSION)
        .header("X-Msh-Device-Name", ascii_header_value(hostname, "unknown"))
        .header(
            "X-Msh-Device-Model",
            ascii_header_value(device_model(), "Unknown"),
        )
        .header("X-Msh-Device-Id", device_id()?)
        .header(
            "X-Msh-Os-Version",
            ascii_header_value(os_version, "unknown"),
        ))
}

async fn post_form<T: for<'de> Deserialize<'de>>(
    url: &str,
    params: &[(&str, String)],
) -> Result<T> {
    let client = crate::provider::shared_http_client();
    let req = client
        .post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .form(params);
    let resp = apply_kimi_headers(req)?.send().await?;
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        #[derive(Deserialize)]
        struct ErrorBody {
            error: Option<String>,
            error_description: Option<String>,
        }
        if let Ok(body) = serde_json::from_str::<ErrorBody>(&text) {
            let code = body.error.unwrap_or_else(|| status.to_string());
            let msg = body.error_description.unwrap_or(text);
            anyhow::bail!("kimi oauth {code}: {msg}");
        }
        anyhow::bail!("kimi oauth {}: {}", status, text);
    }
    serde_json::from_str(&text).with_context(|| format!("Kimi OAuth returned invalid JSON: {text}"))
}

pub async fn start_device_auth() -> Result<DeviceAuth> {
    post_form(
        OAUTH_DEVICE_AUTH_URL,
        &[("client_id", OAUTH_CLIENT_ID.to_string())],
    )
    .await
}

async fn poll_device_token_once(device_code: &str) -> Result<TokenResponse> {
    post_form(
        OAUTH_TOKEN_URL,
        &[
            ("client_id", OAUTH_CLIENT_ID.to_string()),
            ("device_code", device_code.to_string()),
            ("grant_type", OAUTH_DEVICE_GRANT.to_string()),
        ],
    )
    .await
}

pub async fn poll_device_token(device: &DeviceAuth) -> Result<KimiTokens> {
    let mut interval = std::cmp::max(1, device.interval);
    let deadline = chrono::Utc::now().timestamp_millis() + device.expires_in * 1000;
    while chrono::Utc::now().timestamp_millis() < deadline {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        match poll_device_token_once(&device.device_code).await {
            Ok(response) => return Ok(token_response_to_tokens(response)),
            Err(err) => {
                let msg = err.to_string();
                if msg.contains("authorization_pending") {
                    continue;
                }
                if msg.contains("slow_down") {
                    interval += 5;
                    continue;
                }
                if msg.contains("expired_token") {
                    anyhow::bail!("Kimi OAuth device code expired. Run login again.");
                }
                return Err(err);
            }
        }
    }
    anyhow::bail!("Kimi OAuth device code expired before approval.")
}

pub async fn refresh_tokens(refresh_token: &str) -> Result<KimiTokens> {
    let response: TokenResponse = post_form(
        OAUTH_TOKEN_URL,
        &[
            ("client_id", OAUTH_CLIENT_ID.to_string()),
            ("refresh_token", refresh_token.to_string()),
            ("grant_type", OAUTH_REFRESH_GRANT.to_string()),
        ],
    )
    .await?;
    Ok(token_response_to_tokens(response))
}

pub async fn load_or_refresh_tokens() -> Result<KimiTokens> {
    let tokens = load_tokens()?;
    if !tokens.is_expiring() {
        return Ok(tokens);
    }
    let refreshed = refresh_tokens(&tokens.refresh_token).await?;
    save_tokens(&refreshed)?;
    Ok(refreshed)
}

pub async fn bearer_token() -> Result<String> {
    Ok(load_or_refresh_tokens().await?.access_token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_header_value_strips_non_ascii_and_control() {
        assert_eq!(ascii_header_value("hi\n세계", "fallback"), "hi");
        assert_eq!(ascii_header_value("\n\t", "fallback"), "fallback");
    }

    #[test]
    fn kimi_oauth_constants_match_cli_shape() {
        assert_eq!(
            OAUTH_DEVICE_AUTH_URL,
            "https://auth.kimi.com/api/oauth/device_authorization"
        );
        assert_eq!(OAUTH_TOKEN_URL, "https://auth.kimi.com/api/oauth/token");
        assert_eq!(
            OAUTH_DEVICE_GRANT,
            "urn:ietf:params:oauth:grant-type:device_code"
        );
        assert_eq!(API_BASE_URL, "https://api.kimi.com/coding/v1");
        assert!(!OAUTH_CLIENT_ID.is_empty());
        assert_eq!(USER_AGENT, "KimiCLI/1.41.0");
    }
}
