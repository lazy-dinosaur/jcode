use super::*;
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

struct CwdGuard {
    previous: std::path::PathBuf,
}

impl CwdGuard {
    fn set(path: &Path) -> Self {
        let previous = std::env::current_dir().expect("read current dir");
        std::env::set_current_dir(path).expect("set current dir");
        Self { previous }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.previous).expect("restore current dir");
    }
}

fn write_mcp_server(root: &Path, config_path: &str, name: &str, command: &str) {
    let path = root.join(config_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create mcp config dir");
    }
    let json = format!(
        r#"{{
            "servers": {{
                "{name}": {{
                    "command": "{command}",
                    "args": [],
                    "env": {{}}
                }}
            }}
        }}"#
    );
    std::fs::write(path, json).expect("write mcp config");
}

#[test]
fn test_json_rpc_request_serialization() {
    let request = JsonRpcRequest::new(1, "tools/list", None);
    let json = serde_json::to_string(&request).unwrap();
    assert!(json.contains("\"jsonrpc\":\"2.0\""));
    assert!(json.contains("\"id\":1"));
    assert!(json.contains("\"method\":\"tools/list\""));
}

#[test]
fn test_json_rpc_response_deserialization() {
    let json = r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#;
    let response: JsonRpcResponse = serde_json::from_str(json).unwrap();
    assert_eq!(response.id, Some(1));
    assert!(response.result.is_some());
    assert!(response.error.is_none());
}

#[test]
fn test_json_rpc_error_response() {
    let json = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32600,"message":"Invalid Request"}}"#;
    let response: JsonRpcResponse = serde_json::from_str(json).unwrap();
    assert!(response.error.is_some());
    let err = response.error.unwrap();
    assert_eq!(err.code, -32600);
    assert_eq!(err.message, "Invalid Request");
}

#[test]
fn test_mcp_config_deserialization() {
    let json = r#"{
            "servers": {
                "test-server": {
                    "command": "/usr/bin/test-mcp",
                    "args": ["--port", "8080"],
                    "env": {"API_KEY": "secret"}
                }
            }
        }"#;
    let config: McpConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.servers.len(), 1);
    let server = config.servers.get("test-server").unwrap();
    assert_eq!(server.command, "/usr/bin/test-mcp");
    assert_eq!(server.args, vec!["--port", "8080"]);
    assert_eq!(server.env.get("API_KEY"), Some(&"secret".to_string()));
    assert_eq!(server.resolved_transport(), McpTransport::Stdio);
}

#[test]
fn test_m44_remote_mcp_config_deserialization() {
    let json = r#"{
            "servers": {
                "figma": {
                    "transport": "streamable_http",
                    "url": "https://example.com/mcp",
                    "headers": {"X-Workspace": "design"},
                    "auth": {
                        "type": "bearer",
                        "token_env": "FIGMA_MCP_TOKEN"
                    },
                    "shared": true
                }
            }
        }"#;

    let config: McpConfig = serde_json::from_str(json).unwrap();
    let server = config.servers.get("figma").unwrap();

    assert_eq!(server.command, "");
    assert_eq!(server.resolved_transport(), McpTransport::StreamableHttp);
    assert_eq!(server.url.as_deref(), Some("https://example.com/mcp"));
    assert_eq!(
        server.headers.get("X-Workspace"),
        Some(&"design".to_string())
    );
    match server.auth.as_ref().unwrap() {
        McpAuthConfig::Bearer { token_env, token } => {
            assert_eq!(token_env.as_deref(), Some("FIGMA_MCP_TOKEN"));
            assert!(token.is_none());
        }
        McpAuthConfig::OAuth { .. } => panic!("expected bearer auth"),
    }
}

#[test]
fn test_m44_remote_mcp_config_aliases_and_redaction() {
    let json = r#"{
            "command": "",
            "transport": "http",
            "url": "https://example.com/mcp",
            "auth": {
                "type": "bearer",
                "bearer_token_env": "REMOTE_TOKEN",
                "bearer_token": "super-secret"
            }
        }"#;

    let server: McpServerConfig = serde_json::from_str(json).unwrap();
    assert_eq!(server.resolved_transport(), McpTransport::Http);

    let summary = server.redacted_summary();
    assert!(summary.contains("Http"));
    assert!(summary.contains("auth=configured"));
    assert!(!summary.contains("super-secret"));
    assert!(!summary.contains("REMOTE_TOKEN"));
}

#[test]
fn test_m44_remote_mcp_config_roundtrip() {
    let json = r#"{
            "servers": {
                "remote": {
                    "transport": "sse",
                    "url": "https://example.com/sse",
                    "auth": {"type": "oauth", "client_id": "abc", "scopes": ["files:read"]}
                }
            }
        }"#;

    let config: McpConfig = serde_json::from_str(json).unwrap();
    let serialized = serde_json::to_string(&config).unwrap();
    let reparsed: McpConfig = serde_json::from_str(&serialized).unwrap();
    let server = reparsed.servers.get("remote").unwrap();

    assert_eq!(server.resolved_transport(), McpTransport::Sse);
    assert_eq!(server.url.as_deref(), Some("https://example.com/sse"));
    match server.auth.as_ref().unwrap() {
        McpAuthConfig::OAuth {
            client_id, scopes, ..
        } => {
            assert_eq!(client_id.as_deref(), Some("abc"));
            assert_eq!(scopes, &vec!["files:read".to_string()]);
        }
        McpAuthConfig::Bearer { .. } => panic!("expected oauth auth"),
    }
}

#[test]
fn test_mcp_config_empty() {
    let json = r#"{}"#;
    let config: McpConfig = serde_json::from_str(json).unwrap();
    assert!(config.servers.is_empty());
}

#[test]
fn test_tool_def_deserialization() {
    let json = r#"{
            "name": "read_file",
            "description": "Read a file from disk",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                },
                "required": ["path"]
            }
        }"#;
    let tool: McpToolDef = serde_json::from_str(json).unwrap();
    assert_eq!(tool.name, "read_file");
    assert_eq!(tool.description, Some("Read a file from disk".to_string()));
}

#[test]
fn test_tool_call_result_text() {
    let json = r#"{
            "content": [{"type": "text", "text": "File contents here"}],
            "isError": false
        }"#;
    let result: ToolCallResult = serde_json::from_str(json).unwrap();
    assert!(!result.is_error);
    assert_eq!(result.content.len(), 1);
    match &result.content[0] {
        ContentBlock::Text { text, .. } => assert_eq!(text, "File contents here"),
        _ => panic!("Expected text block"),
    }
}

#[test]
fn test_tool_call_result_error() {
    let json = r#"{
            "content": [{"type": "text", "text": "File not found"}],
            "isError": true
        }"#;
    let result: ToolCallResult = serde_json::from_str(json).unwrap();
    assert!(result.is_error);
}

#[test]
fn test_initialize_result() {
    let json = r#"{
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {"listChanged": true}
            },
            "serverInfo": {
                "name": "test-server",
                "version": "1.0.0"
            }
        }"#;
    let result: InitializeResult = serde_json::from_str(json).unwrap();
    assert_eq!(result.protocol_version, "2024-11-05");
    assert!(result.server_info.is_some());
}

#[test]
fn test_global_external_mcp_import_is_disabled() {
    let _env_lock = crate::storage::lock_test_env();
    let home = tempfile::tempdir().expect("home tempdir");
    let project = tempfile::tempdir().expect("project tempdir");
    let _home_guard = EnvVarGuard::set_path("JCODE_HOME", home.path());
    let _cwd_guard = CwdGuard::set(project.path());

    write_mcp_server(
        home.path(),
        "external/.claude/mcp.json",
        "claude_srv",
        "claude-command",
    );

    let config = McpConfig::load();

    assert!(!config.servers.contains_key("claude_srv"));
    assert!(!home.path().join("mcp.json").exists());
}

#[test]
fn test_project_local_agents_mcp_is_loaded() {
    let _env_lock = crate::storage::lock_test_env();
    let home = tempfile::tempdir().expect("home tempdir");
    let project = tempfile::tempdir().expect("project tempdir");
    let _home_guard = EnvVarGuard::set_path("JCODE_HOME", home.path());
    let _cwd_guard = CwdGuard::set(project.path());

    write_mcp_server(
        project.path(),
        ".agents/mcp.json",
        "agents_srv",
        "agents-command",
    );

    let config = McpConfig::load();

    assert_eq!(
        config
            .servers
            .get("agents_srv")
            .map(|server| server.command.as_str()),
        Some("agents-command")
    );
}

#[test]
fn test_project_local_opencode_mcp_is_loaded() {
    let _env_lock = crate::storage::lock_test_env();
    let home = tempfile::tempdir().expect("home tempdir");
    let project = tempfile::tempdir().expect("project tempdir");
    let _home_guard = EnvVarGuard::set_path("JCODE_HOME", home.path());
    let _cwd_guard = CwdGuard::set(project.path());

    write_mcp_server(
        project.path(),
        ".opencode/mcp.json",
        "opencode_srv",
        "opencode-command",
    );

    let config = McpConfig::load();

    assert_eq!(
        config
            .servers
            .get("opencode_srv")
            .map(|server| server.command.as_str()),
        Some("opencode-command")
    );
}

#[test]
fn test_project_local_priority_order() {
    let _env_lock = crate::storage::lock_test_env();
    let home = tempfile::tempdir().expect("home tempdir");
    let project = tempfile::tempdir().expect("project tempdir");
    let _home_guard = EnvVarGuard::set_path("JCODE_HOME", home.path());
    let _cwd_guard = CwdGuard::set(project.path());

    write_mcp_server(
        project.path(),
        ".opencode/mcp.json",
        "name",
        "opencode-command",
    );
    write_mcp_server(project.path(), ".agents/mcp.json", "name", "agents-command");
    write_mcp_server(project.path(), ".claude/mcp.json", "name", "claude-command");
    write_mcp_server(project.path(), ".jcode/mcp.json", "name", "jcode-command");

    let config = McpConfig::load();

    assert_eq!(
        config
            .servers
            .get("name")
            .map(|server| server.command.as_str()),
        Some("jcode-command")
    );
}
