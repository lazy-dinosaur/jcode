use super::protocol::{McpAuthConfig, McpServerConfig};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use url::Url;

const TOKEN_REFRESH_SKEW_MS: i64 = 60_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpOAuthTokens {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    pub expires_at: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
}

impl McpOAuthTokens {
    pub fn is_expired(&self) -> bool {
        chrono::Utc::now().timestamp_millis() + TOKEN_REFRESH_SKEW_MS >= self.expires_at
    }
}

#[derive(Debug, Clone)]
pub struct McpOAuthEndpoints {
    pub authorization_url: String,
    pub token_url: String,
}

#[derive(Debug, Deserialize)]
struct ProtectedResourceMetadata {
    #[serde(default)]
    authorization_servers: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AuthorizationServerMetadata {
    authorization_endpoint: String,
    token_endpoint: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    scope: Option<String>,
}

pub fn tokens_path(server_name: &str) -> Result<std::path::PathBuf> {
    let safe_name: String = server_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    Ok(crate::storage::jcode_dir()?
        .join("mcp_oauth")
        .join(format!("{safe_name}.json")))
}

pub fn load_tokens(server_name: &str) -> Result<McpOAuthTokens> {
    let path = tokens_path(server_name)?;
    if !path.exists() {
        anyhow::bail!(
            "No MCP OAuth tokens found for '{server_name}'. Run mcp action=\"login\" first."
        );
    }
    crate::storage::harden_secret_file_permissions(&path);
    crate::storage::read_json(&path)
        .with_context(|| format!("Failed to read MCP OAuth tokens from {}", path.display()))
}

pub fn save_tokens(server_name: &str, tokens: &McpOAuthTokens) -> Result<()> {
    let path = tokens_path(server_name)?;
    crate::storage::write_json_secret(&path, tokens)
}

fn oauth_config(config: &McpServerConfig) -> Result<&McpAuthConfig> {
    config
        .auth
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("MCP server is not configured for OAuth"))
}

fn oauth_fields(config: &McpServerConfig) -> Result<(&str, Option<&str>, &[String], Option<&str>)> {
    match oauth_config(config)? {
        McpAuthConfig::OAuth {
            client_id,
            client_secret,
            scopes,
            redirect_uri,
            ..
        } => Ok((
            client_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("MCP OAuth config requires client_id"))?,
            client_secret.as_deref(),
            scopes,
            redirect_uri.as_deref(),
        )),
        McpAuthConfig::Bearer { .. } => {
            anyhow::bail!("MCP server is configured for bearer auth, not OAuth")
        }
    }
}

pub async fn discover_endpoints(config: &McpServerConfig) -> Result<McpOAuthEndpoints> {
    let auth = match oauth_config(config)? {
        McpAuthConfig::OAuth {
            authorization_url,
            token_url,
            authorization_server_metadata_url,
            resource_metadata_url,
            ..
        } => {
            if let (Some(authorization_url), Some(token_url)) = (authorization_url, token_url) {
                return Ok(McpOAuthEndpoints {
                    authorization_url: authorization_url.clone(),
                    token_url: token_url.clone(),
                });
            }
            (
                authorization_server_metadata_url.clone(),
                resource_metadata_url.clone(),
            )
        }
        McpAuthConfig::Bearer { .. } => {
            anyhow::bail!("MCP bearer auth does not use OAuth discovery")
        }
    };

    let client = crate::provider::shared_http_client();
    let metadata_url = if let Some(metadata_url) = auth.0 {
        metadata_url
    } else {
        let resource_metadata_url = if let Some(resource_metadata_url) = auth.1 {
            resource_metadata_url
        } else {
            default_resource_metadata_url(config)?
        };
        let resource: ProtectedResourceMetadata = client
            .get(&resource_metadata_url)
            .send()
            .await
            .with_context(|| format!("Failed to fetch MCP OAuth protected resource metadata from {resource_metadata_url}"))?
            .error_for_status()
            .with_context(|| format!("MCP OAuth protected resource metadata request failed: {resource_metadata_url}"))?
            .json()
            .await
            .with_context(|| format!("Failed to parse MCP OAuth protected resource metadata from {resource_metadata_url}"))?;
        resource
            .authorization_servers
            .first()
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "MCP protected resource metadata did not include authorization_servers"
                )
            })?
    };

    let metadata_url = normalize_authorization_server_metadata_url(&metadata_url)?;
    let metadata: AuthorizationServerMetadata = client
        .get(&metadata_url)
        .send()
        .await
        .with_context(|| {
            format!("Failed to fetch MCP OAuth authorization metadata from {metadata_url}")
        })?
        .error_for_status()
        .with_context(|| {
            format!("MCP OAuth authorization metadata request failed: {metadata_url}")
        })?
        .json()
        .await
        .with_context(|| {
            format!("Failed to parse MCP OAuth authorization metadata from {metadata_url}")
        })?;

    Ok(McpOAuthEndpoints {
        authorization_url: metadata.authorization_endpoint,
        token_url: metadata.token_endpoint,
    })
}

fn default_resource_metadata_url(config: &McpServerConfig) -> Result<String> {
    let url = config
        .url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("MCP OAuth discovery requires server url"))?;
    let parsed = Url::parse(url)?;
    let origin = parsed.origin().ascii_serialization();
    Ok(format!("{origin}/.well-known/oauth-protected-resource"))
}

fn normalize_authorization_server_metadata_url(value: &str) -> Result<String> {
    let parsed = Url::parse(value)?;
    if parsed
        .path()
        .contains("/.well-known/oauth-authorization-server")
    {
        return Ok(value.to_string());
    }
    let origin = parsed.origin().ascii_serialization();
    Ok(format!("{origin}/.well-known/oauth-authorization-server"))
}

pub fn build_auth_url(
    endpoints: &McpOAuthEndpoints,
    config: &McpServerConfig,
    redirect_uri: &str,
    challenge: &str,
    state: &str,
) -> Result<String> {
    let (client_id, _client_secret, scopes, _configured_redirect_uri) = oauth_fields(config)?;
    let mut url = Url::parse(&endpoints.authorization_url)?;
    {
        let mut qp = url.query_pairs_mut();
        qp.append_pair("response_type", "code");
        qp.append_pair("client_id", client_id);
        qp.append_pair("redirect_uri", redirect_uri);
        qp.append_pair("code_challenge", challenge);
        qp.append_pair("code_challenge_method", "S256");
        qp.append_pair("state", state);
        if !scopes.is_empty() {
            qp.append_pair("scope", &scopes.join(" "));
        }
    }
    Ok(url.to_string())
}

pub async fn login(
    server_name: &str,
    config: &McpServerConfig,
    no_browser: bool,
) -> Result<McpOAuthTokens> {
    let endpoints = discover_endpoints(config).await?;
    let (verifier, challenge) = crate::auth::oauth::generate_pkce_public();
    let state = crate::auth::oauth::generate_state_public();
    let configured_redirect_uri = match oauth_config(config)? {
        McpAuthConfig::OAuth { redirect_uri, .. } => redirect_uri.clone(),
        McpAuthConfig::Bearer { .. } => None,
    };

    let listener = if configured_redirect_uri.is_none() {
        crate::auth::oauth::bind_callback_listener(0).ok()
    } else {
        None
    };
    let redirect_uri = configured_redirect_uri
        .or_else(|| {
            listener
                .as_ref()
                .and_then(|listener| listener.local_addr().ok())
                .map(|addr| format!("http://127.0.0.1:{}", addr.port()))
        })
        .ok_or_else(|| {
            anyhow::anyhow!("MCP OAuth login requires redirect_uri or a local callback listener")
        })?;

    let auth_url = build_auth_url(&endpoints, config, &redirect_uri, &challenge, &state)?;
    eprintln!("\nOpen this MCP OAuth URL for '{server_name}':\n\n{auth_url}\n");
    let browser_opened = if crate::auth::browser_suppressed(no_browser) {
        false
    } else {
        open::that(&auth_url).is_ok()
    };

    let code = if browser_opened {
        if let Some(listener) = listener {
            match tokio::time::timeout(
                std::time::Duration::from_secs(300),
                crate::auth::oauth::wait_for_callback_async_on_listener(listener, &state),
            )
            .await
            {
                Ok(Ok(code)) => code,
                Ok(Err(err)) => anyhow::bail!("MCP OAuth callback failed: {err}"),
                Err(_) => anyhow::bail!("Timed out waiting for MCP OAuth callback"),
            }
        } else {
            anyhow::bail!(
                "MCP OAuth opened a browser but no local callback listener is available; configure a localhost redirect_uri or use no-browser/manual flow"
            )
        }
    } else {
        anyhow::bail!(
            "Browser launch suppressed or failed. Use the printed URL in a browser, then rerun with a supported local redirect flow."
        )
    };

    let tokens = exchange_code(config, &endpoints, &verifier, &code, &redirect_uri).await?;
    save_tokens(server_name, &tokens)?;
    Ok(tokens)
}

async fn exchange_code(
    config: &McpServerConfig,
    endpoints: &McpOAuthEndpoints,
    verifier: &str,
    code: &str,
    redirect_uri: &str,
) -> Result<McpOAuthTokens> {
    let (client_id, client_secret, scopes, _redirect_uri) = oauth_fields(config)?;
    let mut form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("client_id", client_id.to_string()),
        ("code", code.to_string()),
        ("code_verifier", verifier.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
    ];
    if let Some(client_secret) = client_secret {
        form.push(("client_secret", client_secret.to_string()));
    }

    let resp = crate::provider::shared_http_client()
        .post(&endpoints.token_url)
        .form(&form)
        .send()
        .await
        .context("Failed to exchange MCP OAuth code")?;
    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("MCP OAuth token exchange failed: {text}");
    }
    let token: TokenResponse = resp
        .json()
        .await
        .context("Failed to parse MCP OAuth token response")?;
    Ok(token_response_to_tokens(token, scopes))
}

pub async fn load_or_refresh_access_token(
    server_name: &str,
    config: &McpServerConfig,
) -> Result<String> {
    let mut tokens = load_tokens(server_name)?;
    if tokens.is_expired() {
        tokens = refresh_tokens(server_name, config, &tokens).await?;
    }
    Ok(tokens.access_token)
}

pub async fn refresh_tokens(
    server_name: &str,
    config: &McpServerConfig,
    tokens: &McpOAuthTokens,
) -> Result<McpOAuthTokens> {
    let refresh_token = tokens.refresh_token.as_deref().ok_or_else(|| {
        anyhow::anyhow!("MCP OAuth token for '{server_name}' has no refresh_token; rerun mcp login")
    })?;
    let endpoints = discover_endpoints(config).await?;
    let (client_id, client_secret, scopes, _redirect_uri) = oauth_fields(config)?;
    let mut form = vec![
        ("grant_type", "refresh_token".to_string()),
        ("client_id", client_id.to_string()),
        ("refresh_token", refresh_token.to_string()),
    ];
    if let Some(client_secret) = client_secret {
        form.push(("client_secret", client_secret.to_string()));
    }

    let resp = crate::provider::shared_http_client()
        .post(&endpoints.token_url)
        .form(&form)
        .send()
        .await
        .context("Failed to refresh MCP OAuth token")?;
    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("MCP OAuth token refresh failed: {text}");
    }
    let token: TokenResponse = resp
        .json()
        .await
        .context("Failed to parse MCP OAuth refresh response")?;
    let mut refreshed = token_response_to_tokens(token, scopes);
    if refreshed.refresh_token.is_none() {
        refreshed.refresh_token = tokens.refresh_token.clone();
    }
    save_tokens(server_name, &refreshed)?;
    Ok(refreshed)
}

fn token_response_to_tokens(token: TokenResponse, configured_scopes: &[String]) -> McpOAuthTokens {
    let expires_at =
        chrono::Utc::now().timestamp_millis() + token.expires_in.unwrap_or(3600) * 1000;
    let scopes = token
        .scope
        .as_deref()
        .map(|scope| scope.split_whitespace().map(ToOwned::to_owned).collect())
        .unwrap_or_else(|| configured_scopes.to_vec());
    McpOAuthTokens {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at,
        scopes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::Path;

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set_path(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var_os(key);
            crate::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.take() {
                crate::env::set_var(self.key, previous);
            } else {
                crate::env::remove_var(self.key);
            }
        }
    }

    fn oauth_config() -> McpServerConfig {
        McpServerConfig {
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            transport: Some(super::super::protocol::McpTransport::StreamableHttp),
            url: Some("https://figma.example/mcp".to_string()),
            headers: HashMap::new(),
            auth: Some(McpAuthConfig::OAuth {
                client_id: Some("figma-client".to_string()),
                client_secret: None,
                scopes: vec!["files:read".to_string(), "comments:read".to_string()],
                authorization_url: Some("https://auth.example/authorize".to_string()),
                token_url: Some("https://auth.example/token".to_string()),
                resource_metadata_url: None,
                authorization_server_metadata_url: None,
                redirect_uri: None,
            }),
            shared: true,
        }
    }

    #[test]
    fn m44_mcp_oauth_builds_pkce_auth_url() {
        let endpoints = McpOAuthEndpoints {
            authorization_url: "https://auth.example/authorize".to_string(),
            token_url: "https://auth.example/token".to_string(),
        };
        let config = oauth_config();

        let url = build_auth_url(
            &endpoints,
            &config,
            "http://127.0.0.1:7777/callback",
            "challenge",
            "state",
        )
        .unwrap();
        let parsed = Url::parse(&url).unwrap();
        let pairs: HashMap<_, _> = parsed.query_pairs().into_owned().collect();

        assert_eq!(pairs.get("response_type").map(String::as_str), Some("code"));
        assert_eq!(
            pairs.get("client_id").map(String::as_str),
            Some("figma-client")
        );
        assert_eq!(
            pairs.get("redirect_uri").map(String::as_str),
            Some("http://127.0.0.1:7777/callback")
        );
        assert_eq!(
            pairs.get("code_challenge").map(String::as_str),
            Some("challenge")
        );
        assert_eq!(pairs.get("state").map(String::as_str), Some("state"));
        assert_eq!(
            pairs.get("scope").map(String::as_str),
            Some("files:read comments:read")
        );
    }

    #[test]
    fn m44_mcp_oauth_token_store_is_server_scoped_and_sanitized() {
        let _env_lock = crate::storage::lock_test_env();
        let home = tempfile::tempdir().expect("home tempdir");
        let _home_guard = EnvVarGuard::set_path("JCODE_HOME", home.path());
        let tokens = McpOAuthTokens {
            access_token: "access".to_string(),
            refresh_token: Some("refresh".to_string()),
            expires_at: chrono::Utc::now().timestamp_millis() + 3600_000,
            scopes: vec!["files:read".to_string()],
        };

        save_tokens("figma/team", &tokens).unwrap();
        let path = tokens_path("figma/team").unwrap();
        assert!(path.ends_with("mcp_oauth/figma_team.json"));
        let loaded = load_tokens("figma/team").unwrap();
        assert_eq!(loaded.access_token, "access");
        assert_eq!(loaded.refresh_token.as_deref(), Some("refresh"));
    }
}
