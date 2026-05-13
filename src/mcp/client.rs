//! MCP Client - handles communication with a single MCP server

use super::protocol::*;
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, mpsc, oneshot};

fn first_sse_json_data(body: &str) -> Option<&str> {
    body.lines().find_map(|line| {
        let data = line.strip_prefix("data:")?.trim();
        if data.is_empty() || data == "[DONE]" {
            None
        } else {
            Some(data)
        }
    })
}

fn build_http_headers(config: &McpServerConfig) -> Result<reqwest::header::HeaderMap> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static("application/json, text/event-stream"),
    );
    headers.insert(
        reqwest::header::CONTENT_TYPE,
        reqwest::header::HeaderValue::from_static("application/json"),
    );

    for (name, value) in &config.headers {
        let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .with_context(|| format!("Invalid MCP HTTP header name: {}", name))?;
        let value = reqwest::header::HeaderValue::from_str(value)
            .with_context(|| format!("Invalid MCP HTTP header value for {}", name))?;
        headers.insert(name, value);
    }

    match &config.auth {
        Some(McpAuthConfig::Bearer { token_env, token }) => {
            let bearer = if let Some(env_name) = token_env {
                std::env::var(env_name).with_context(|| {
                    format!("MCP bearer token env var '{}' is not set", env_name)
                })?
            } else if let Some(token) = token {
                token.clone()
            } else {
                anyhow::bail!("MCP bearer auth requires token_env or token");
            };

            headers.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("Bearer {}", bearer))
                    .context("Invalid MCP bearer token")?,
            );
        }
        Some(McpAuthConfig::OAuth { .. }) => {
            anyhow::bail!("MCP OAuth auth is configured but login/refresh is not implemented yet")
        }
        None => {}
    }

    Ok(headers)
}

#[derive(Clone)]
enum McpHandleTransport {
    Stdio(mpsc::Sender<String>),
    Http {
        client: reqwest::Client,
        url: String,
        headers: reqwest::header::HeaderMap,
    },
}

/// Shared communication handle for an MCP server.
/// Multiple sessions can hold clones of this and send concurrent requests.
/// Request/response correlation by ID ensures no interference.
#[derive(Clone)]
pub struct McpHandle {
    pub(crate) name: String,
    request_id: Arc<AtomicU64>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>>,
    transport: McpHandleTransport,
    server_info: Arc<std::sync::RwLock<Option<ServerInfo>>>,
    capabilities: Arc<std::sync::RwLock<ServerCapabilities>>,
    tools: Arc<std::sync::RwLock<Vec<McpToolDef>>>,
}

impl McpHandle {
    /// Send a request and wait for response
    pub async fn request(&self, method: &str, params: Option<Value>) -> Result<JsonRpcResponse> {
        let id = self.request_id.fetch_add(1, Ordering::SeqCst);
        let request = JsonRpcRequest::new(id, method, params);

        if let McpHandleTransport::Http {
            client,
            url,
            headers,
        } = &self.transport
        {
            return self
                .request_http(client, url, headers, &request)
                .await
                .with_context(|| format!("HTTP MCP request '{}' failed", method));
        }

        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(id, tx);
        }

        let msg = serde_json::to_string(&request)? + "\n";
        match &self.transport {
            McpHandleTransport::Stdio(writer_tx) => writer_tx
                .send(msg)
                .await
                .context("Failed to send request")?,
            McpHandleTransport::Http { .. } => unreachable!("HTTP handled above"),
        }

        let response = tokio::time::timeout(std::time::Duration::from_secs(30), rx)
            .await
            .context("Request timeout")?
            .context("Channel closed")?;

        if let Some(err) = &response.error {
            anyhow::bail!("MCP error {}: {}", err.code, err.message);
        }

        Ok(response)
    }

    async fn request_http(
        &self,
        client: &reqwest::Client,
        url: &str,
        headers: &reqwest::header::HeaderMap,
        request: &JsonRpcRequest,
    ) -> Result<JsonRpcResponse> {
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            client
                .post(url)
                .headers(headers.clone())
                .json(request)
                .send(),
        )
        .await
        .context("HTTP MCP request timeout")??;

        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let body = response.text().await?;

        if !status.is_success() {
            anyhow::bail!("HTTP MCP server returned {}: {}", status, body);
        }

        let response_text = if content_type.contains("text/event-stream") {
            first_sse_json_data(&body).context("HTTP MCP SSE response had no JSON data event")?
        } else {
            body.as_str()
        };

        let response: JsonRpcResponse = serde_json::from_str(response_text)?;
        if let Some(err) = &response.error {
            anyhow::bail!("MCP error {}: {}", err.code, err.message);
        }

        Ok(response)
    }

    async fn notify(&self, method: &str, params: Option<Value>) -> Result<()> {
        let notif = JsonRpcRequest::new(0, method, params);
        match &self.transport {
            McpHandleTransport::Stdio(writer_tx) => {
                let msg = serde_json::to_string(&notif)? + "\n";
                writer_tx.send(msg).await?;
            }
            McpHandleTransport::Http {
                client,
                url,
                headers,
            } => {
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    client
                        .post(url)
                        .headers(headers.clone())
                        .json(&notif)
                        .send(),
                )
                .await
                .context("HTTP MCP notification timeout")??;
            }
        }
        Ok(())
    }

    /// Call a tool
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<ToolCallResult> {
        let arguments = if arguments.is_null() {
            Value::Object(serde_json::Map::new())
        } else {
            arguments
        };
        let params = ToolCallParams {
            name: name.to_string(),
            arguments,
        };

        let response = self
            .request("tools/call", Some(serde_json::to_value(params)?))
            .await?;

        let result = response.result.context("No result from tool call")?;
        let tool_result: ToolCallResult = serde_json::from_value(result)?;

        Ok(tool_result)
    }

    /// Get the server name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get server info
    pub fn server_info(&self) -> Option<ServerInfo> {
        self.server_info
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Get available tools
    pub fn tools(&self) -> Vec<McpToolDef> {
        self.tools
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Refresh the list of available tools
    pub async fn refresh_tools(&self) -> Result<()> {
        let response = self.request("tools/list", None).await?;

        if let Some(result) = response.result {
            let tools_result: ToolsListResult = serde_json::from_value(result)?;
            *self
                .tools
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = tools_result.tools;
        }

        Ok(())
    }
}

/// MCP Client - owns the child process and provides shared handles.
/// Only one McpClient exists per MCP server process, but many McpHandle
/// clones can be distributed to different sessions.
pub struct McpClient {
    handle: McpHandle,
    child: Option<Child>,
}

impl McpClient {
    /// Connect to an MCP server
    pub async fn connect(name: String, config: &McpServerConfig) -> Result<Self> {
        if !config.is_stdio() {
            return Self::connect_http(name, config).await;
        }

        crate::logging::info(&format!(
            "MCP: Connecting to '{}' ({})",
            name,
            config.redacted_summary()
        ));

        let mut env: HashMap<String, String> = std::env::vars().collect();
        env.extend(config.env.clone());

        let mut child = Command::new(&config.command)
            .args(&config.args)
            .envs(&env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("Failed to spawn MCP server: {}", config.command))?;

        let stdin = child.stdin.take().context("No stdin")?;
        let stdout = child.stdout.take().context("No stdout")?;
        let stderr = child.stderr.take().context("No stderr")?;

        // Spawn stderr reader
        let server_name = name.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            crate::logging::warn(&format!(
                                "MCP [{}] stderr: {}",
                                server_name, trimmed
                            ));
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        // Setup channels
        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let (writer_tx, mut writer_rx) = mpsc::channel::<String>(32);

        // Spawn writer task
        let mut stdin = stdin;
        tokio::spawn(async move {
            while let Some(msg) = writer_rx.recv().await {
                if stdin.write_all(msg.as_bytes()).await.is_err() {
                    break;
                }
                if stdin.flush().await.is_err() {
                    break;
                }
            }
        });

        // Spawn reader task
        let pending_clone = Arc::clone(&pending);
        let reader_name = name.clone();
        let mut reader = BufReader::new(stdout);
        tokio::spawn(async move {
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        crate::logging::debug(&format!("MCP [{}]: stdout EOF", reader_name));
                        break;
                    }
                    Ok(_) => {
                        if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(&line) {
                            if let Some(id) = response.id {
                                let mut pending = pending_clone.lock().await;
                                if let Some(tx) = pending.remove(&id) {
                                    let _ = tx.send(response);
                                }
                            }
                        } else {
                            let trimmed = line.trim();
                            if !trimmed.is_empty() {
                                crate::logging::debug(&format!(
                                    "MCP [{}] non-JSON output: {}",
                                    reader_name, trimmed
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        crate::logging::warn(&format!("MCP [{}] read error: {}", reader_name, e));
                        break;
                    }
                }
            }
        });

        let handle = McpHandle {
            name: name.clone(),
            request_id: Arc::new(AtomicU64::new(1)),
            pending,
            transport: McpHandleTransport::Stdio(writer_tx),
            server_info: Arc::new(std::sync::RwLock::new(None)),
            capabilities: Arc::new(std::sync::RwLock::new(ServerCapabilities::default())),
            tools: Arc::new(std::sync::RwLock::new(Vec::new())),
        };

        let mut client = Self {
            handle,
            child: Some(child),
        };

        client
            .initialize()
            .await
            .with_context(|| format!("MCP server '{}' failed to initialize", name))?;

        client
            .handle
            .refresh_tools()
            .await
            .with_context(|| format!("MCP server '{}' failed to list tools", name))?;

        crate::logging::info(&format!(
            "MCP: Connected to '{}' with {} tools",
            name,
            client.handle.tools().len()
        ));

        Ok(client)
    }

    async fn connect_http(name: String, config: &McpServerConfig) -> Result<Self> {
        let url = config
            .url
            .clone()
            .with_context(|| format!("MCP remote server '{}' is missing url", name))?;
        let headers = build_http_headers(config)?;
        crate::logging::info(&format!(
            "MCP: Connecting to '{}' ({})",
            name,
            config.redacted_summary()
        ));

        let handle = McpHandle {
            name: name.clone(),
            request_id: Arc::new(AtomicU64::new(1)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            transport: McpHandleTransport::Http {
                client: reqwest::Client::new(),
                url,
                headers,
            },
            server_info: Arc::new(std::sync::RwLock::new(None)),
            capabilities: Arc::new(std::sync::RwLock::new(ServerCapabilities::default())),
            tools: Arc::new(std::sync::RwLock::new(Vec::new())),
        };

        let mut client = Self {
            handle,
            child: None,
        };

        client
            .initialize()
            .await
            .with_context(|| format!("MCP remote server '{}' failed to initialize", name))?;

        client
            .handle
            .refresh_tools()
            .await
            .with_context(|| format!("MCP remote server '{}' failed to list tools", name))?;

        crate::logging::info(&format!(
            "MCP: Connected to remote '{}' with {} tools",
            name,
            client.handle.tools().len()
        ));

        Ok(client)
    }

    /// Get a shareable handle to this client
    pub fn handle(&self) -> McpHandle {
        self.handle.clone()
    }

    /// Initialize the MCP connection
    async fn initialize(&mut self) -> Result<()> {
        let params = InitializeParams {
            protocol_version: "2024-11-05".to_string(),
            capabilities: ClientCapabilities::default(),
            client_info: ClientInfo {
                name: "jcode".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        };

        let response = self
            .handle
            .request("initialize", Some(serde_json::to_value(params)?))
            .await?;

        if let Some(result) = response.result {
            let init_result: InitializeResult = serde_json::from_value(result)?;
            *self
                .handle
                .server_info
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = init_result.server_info;
            *self
                .handle
                .capabilities
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = init_result.capabilities;
        }

        // Send initialized notification
        self.handle
            .notify("notifications/initialized", None)
            .await?;

        Ok(())
    }

    /// Check if server is still running
    pub fn is_running(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(None) => true,
                Ok(Some(_)) => false,
                Err(_) => false,
            },
            None => true,
        }
    }

    /// Shutdown the server
    pub async fn shutdown(&mut self) {
        let _ = self.handle.notify("shutdown", None).await;

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        if let Some(child) = self.child.as_mut() {
            let _ = child.kill().await;
        }
    }

    // === Legacy compatibility methods that delegate to handle ===

    pub fn name(&self) -> &str {
        &self.handle.name
    }

    pub fn server_info(&self) -> Option<ServerInfo> {
        self.handle.server_info()
    }

    pub fn tools(&self) -> Vec<McpToolDef> {
        self.handle.tools()
    }

    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<ToolCallResult> {
        self.handle.call_tool(name, arguments).await
    }

    pub async fn refresh_tools(&self) -> Result<()> {
        self.handle.refresh_tools().await
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
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

    async fn read_http_request(
        stream: &mut tokio::net::TcpStream,
    ) -> anyhow::Result<(String, serde_json::Value)> {
        let mut buffer = Vec::new();
        let header_end = loop {
            let mut chunk = [0u8; 1024];
            let n = stream.read(&mut chunk).await?;
            anyhow::ensure!(n > 0, "connection closed before headers");
            buffer.extend_from_slice(&chunk[..n]);
            if let Some(pos) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                break pos + 4;
            }
        };

        let headers = String::from_utf8_lossy(&buffer[..header_end]).to_string();
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);

        while buffer.len() < header_end + content_length {
            let mut chunk = vec![0u8; header_end + content_length - buffer.len()];
            let n = stream.read(&mut chunk).await?;
            anyhow::ensure!(n > 0, "connection closed before body");
            buffer.extend_from_slice(&chunk[..n]);
        }

        let body = &buffer[header_end..header_end + content_length];
        Ok((headers, serde_json::from_slice(body)?))
    }

    async fn write_json_response(
        stream: &mut tokio::net::TcpStream,
        body: serde_json::Value,
    ) -> anyhow::Result<()> {
        let body = serde_json::to_string(&body)?;
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .as_bytes(),
            )
            .await?;
        Ok(())
    }

    #[tokio::test]
    async fn m44_http_mcp_connect_uses_bearer_env_and_lists_tools() {
        let _env = EnvVarGuard::set("JCODE_TEST_MCP_TOKEN", "test-token");
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let seen_server = Arc::clone(&seen);

        let server = tokio::spawn(async move {
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let (headers, body) = read_http_request(&mut stream).await.unwrap();
                seen_server.lock().unwrap().push(headers);
                let id = body.get("id").and_then(|value| value.as_u64()).unwrap_or(0);
                let method = body.get("method").and_then(|value| value.as_str()).unwrap();
                let response = match method {
                    "initialize" => serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "protocolVersion": "2024-11-05",
                            "capabilities": {"tools": {"listChanged": true}},
                            "serverInfo": {"name": "mock-http", "version": "1.0.0"}
                        }
                    }),
                    "notifications/initialized" => serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {}
                    }),
                    "tools/list" => serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "tools": [{
                                "name": "inspect_frame",
                                "description": "Inspect a design frame",
                                "inputSchema": {"type": "object", "properties": {}}
                            }]
                        }
                    }),
                    other => panic!("unexpected method {other}"),
                };
                write_json_response(&mut stream, response).await.unwrap();
            }
        });

        let config = McpServerConfig {
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            transport: Some(McpTransport::StreamableHttp),
            url: Some(url),
            headers: HashMap::new(),
            auth: Some(McpAuthConfig::Bearer {
                token_env: Some("JCODE_TEST_MCP_TOKEN".to_string()),
                token: None,
            }),
            shared: true,
        };

        let client = McpClient::connect("figma".to_string(), &config)
            .await
            .unwrap();
        assert_eq!(client.tools().len(), 1);
        assert_eq!(client.tools()[0].name, "inspect_frame");

        server.await.unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        assert!(
            seen.iter()
                .all(|headers| headers.contains("authorization: Bearer test-token"))
        );
    }

    #[test]
    fn m44_http_mcp_oauth_auth_is_explicitly_unsupported_until_stage_3() {
        let config = McpServerConfig {
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            transport: Some(McpTransport::StreamableHttp),
            url: Some("https://example.com/mcp".to_string()),
            headers: HashMap::new(),
            auth: Some(McpAuthConfig::OAuth {
                client_id: Some("client".to_string()),
                scopes: Vec::new(),
            }),
            shared: true,
        };

        let error = build_http_headers(&config).unwrap_err().to_string();
        assert!(error.contains("OAuth auth is configured"));
    }

    #[test]
    fn m44_http_mcp_parses_sse_json_data() {
        let body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n\n";
        assert_eq!(
            first_sse_json_data(body),
            Some("{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}")
        );
    }
}
