//! MCP management tool - connect, disconnect, list, reload MCP servers

use crate::mcp::{McpConfig, McpManager, McpServerConfig};
use crate::tool::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Deserialize)]
struct McpToolInput {
    action: String,
    #[serde(default)]
    server: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Option<Vec<String>>,
    #[serde(default)]
    env: Option<HashMap<String, String>>,
    #[serde(default)]
    no_browser: bool,
    #[serde(default)]
    callback_url: Option<String>,
}

pub struct McpManagementTool {
    manager: Arc<RwLock<McpManager>>,
    registry: Option<crate::tool::Registry>,
}

impl McpManagementTool {
    pub fn new(manager: Arc<RwLock<McpManager>>) -> Self {
        Self {
            manager,
            registry: None,
        }
    }

    pub fn with_registry(mut self, registry: crate::tool::Registry) -> Self {
        self.registry = Some(registry);
        self
    }
}

#[async_trait]
impl Tool for McpManagementTool {
    fn name(&self) -> &str {
        "mcp"
    }

    fn description(&self) -> &str {
        "Manage MCP (Model Context Protocol) servers."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "intent": super::intent_schema_property(),
                "action": {
                    "type": "string",
                    "enum": ["list", "connect", "disconnect", "reload", "reconcile", "login"],
                    "description": "Action."
                },
                "server": {
                    "type": "string",
                    "description": "Server name."
                },
                "command": {
                    "type": "string",
                    "description": "Server command."
                },
                "args": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Command args."
                },
                "env": {
                    "type": "object",
                    "additionalProperties": {"type": "string"},
                    "description": "Server env."
                },
                "no_browser": {
                    "type": "boolean",
                    "description": "Print auth URL without opening a browser for OAuth login."
                },
                "callback_url": {
                    "type": "string",
                    "description": "For MCP OAuth login, paste the full callback URL or query string from a previously started no-browser login to finish the pending login."
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: McpToolInput = serde_json::from_value(input)?;

        match params.action.as_str() {
            "list" => self.list_servers().await,
            "connect" => self.connect_server(params, &ctx.session_id).await,
            "disconnect" => self.disconnect_server(params).await,
            "reload" => self.reload_config(&ctx.session_id).await,
            "reconcile" => self.reconcile_registry().await,
            "login" => self.login_server(params).await,
            _ => Ok(ToolOutput::new(format!(
                "Unknown action: {}. Use 'list', 'connect', 'disconnect', 'reload', 'reconcile', or 'login'.",
                params.action
            ))),
        }
    }
}

// Helper for tests to update cached server names
impl McpManagementTool {
    pub fn manager(&self) -> &Arc<RwLock<McpManager>> {
        &self.manager
    }
}

impl McpManagementTool {
    async fn list_servers(&self) -> Result<ToolOutput> {
        self.auto_reconcile_registry("list").await;
        let manager = self.manager.read().await;
        let servers = manager.connected_servers().await;
        let all_tools = manager.all_tools().await;

        if servers.is_empty() {
            let mut output = String::from(
                "No MCP servers connected.\n\n\
                To connect a server, use:\n\
                {\"action\": \"connect\", \"server\": \"name\", \"command\": \"/path/to/server\", \"args\": []}\n\n\
                Or add servers to ~/.jcode/mcp.json or .jcode/mcp.json and use {\"action\": \"reload\"}.\n\
                .claude/mcp.json is also supported for compatibility.",
            );
            self.append_registry_diagnostics(&mut output).await;
            return Ok(ToolOutput::new(output).with_title("MCP: No servers"));
        }

        let mut output = String::new();
        output.push_str(&format!("Connected MCP servers: {}\n\n", servers.len()));

        for server in &servers {
            output.push_str(&format!("## {}\n", server));
            let server_tools: Vec<_> = all_tools.iter().filter(|(s, _)| s == server).collect();

            if server_tools.is_empty() {
                output.push_str("  (no tools)\n");
            } else {
                for (_, tool) in server_tools {
                    output.push_str(&format!(
                        "  - mcp__{}__{}: {}\n",
                        server,
                        tool.name,
                        tool.description.as_deref().unwrap_or("(no description)")
                    ));
                }
            }
            output.push('\n');
        }

        self.append_registry_diagnostics(&mut output).await;

        Ok(ToolOutput::new(output).with_title("MCP: Server list"))
    }

    async fn append_registry_diagnostics(&self, output: &mut String) {
        let Some(registry) = self.registry.as_ref() else {
            return;
        };
        let diagnostics = registry.mcp_registry_diagnostics().await;
        output.push_str("\n## Registry diagnostics\n");
        output.push_str(&format!(
            "  - mcp management tool registered: {}\n",
            diagnostics.mcp_management_registered
        ));
        output.push_str(&format!(
            "  - registered MCP server tools: {}\n",
            diagnostics.mcp_server_tool_count
        ));
        output.push_str(&format!(
            "  - total registered tools: {}\n",
            diagnostics.total_tool_count
        ));
        if !diagnostics.mcp_server_tool_names.is_empty() {
            output.push_str("  - registered MCP tool names:\n");
            for name in diagnostics.mcp_server_tool_names {
                output.push_str(&format!("    - {}\n", name));
            }
        }
    }

    async fn auto_reconcile_registry(&self, reason: &str) {
        let Some(registry) = self.registry.as_ref() else {
            return;
        };
        let report = registry
            .reconcile_mcp_tools_from_manager(Arc::clone(&self.manager))
            .await;
        if !report.repaired_tool_names.is_empty() {
            crate::logging::warn(&format!(
                "MCP: repaired {} missing registry tool(s) during {}: {:?}",
                report.repaired_tool_names.len(),
                reason,
                report.repaired_tool_names
            ));
        }
    }

    async fn reconcile_registry(&self) -> Result<ToolOutput> {
        let Some(registry) = self.registry.as_ref() else {
            return Ok(ToolOutput::new(
                "MCP registry reconciliation is unavailable: management tool has no registry handle.",
            )
            .with_title("MCP: Reconcile unavailable"));
        };

        let report = registry
            .reconcile_mcp_tools_from_manager(Arc::clone(&self.manager))
            .await;
        let mut output = format!(
            "Reconciled MCP registry\n\nExpected MCP server tools: {}\nAlready registered: {}\nRepaired missing tools: {}\n",
            report.expected_mcp_server_tool_count,
            report.already_registered_count,
            report.repaired_tool_names.len()
        );
        if !report.repaired_tool_names.is_empty() {
            output.push_str("\nRepaired tools:\n");
            for name in &report.repaired_tool_names {
                output.push_str(&format!("  - {}\n", name));
            }
        }
        self.append_registry_diagnostics(&mut output).await;
        Ok(ToolOutput::new(output).with_title("MCP: Reconciled"))
    }

    async fn connect_server(&self, params: McpToolInput, session_id: &str) -> Result<ToolOutput> {
        let server_name = params
            .server
            .ok_or_else(|| anyhow::anyhow!("'server' is required for connect action"))?;
        let command = params
            .command
            .ok_or_else(|| anyhow::anyhow!("'command' is required for connect action"))?;

        let config = McpServerConfig {
            command,
            args: params.args.unwrap_or_default(),
            env: params.env.unwrap_or_default(),
            transport: None,
            url: None,
            headers: std::collections::HashMap::new(),
            auth: None,
            shared: true,
        };

        let manager = self.manager.read().await;

        // Check if already connected
        let connected = manager.connected_servers().await;
        if connected.contains(&server_name) {
            return Ok(ToolOutput::new(format!(
                "Server '{}' is already connected. Use 'disconnect' first to reconnect.",
                server_name
            ))
            .with_title("MCP: Already connected"));
        }
        drop(manager);

        // Connect
        let manager = self.manager.read().await;
        match manager.connect(&server_name, &config).await {
            Ok(()) => {
                let tools = manager.all_tools().await;
                let server_tools: Vec<_> =
                    tools.iter().filter(|(s, _)| s == &server_name).collect();

                let mut output = format!(
                    "Connected to MCP server '{}'\n\nAvailable tools ({}):\n",
                    server_name,
                    server_tools.len()
                );
                for (_, tool) in &server_tools {
                    output.push_str(&format!(
                        "  - mcp__{}__{}: {}\n",
                        server_name,
                        tool.name,
                        tool.description.as_deref().unwrap_or("(no description)")
                    ));
                }
                drop(manager);

                // Register the new tools in the registry
                if let Some(ref registry) = self.registry {
                    let mcp_tools = crate::mcp::create_mcp_tools(Arc::clone(&self.manager)).await;
                    for (name, tool) in mcp_tools {
                        if name.starts_with(&format!("mcp__{}__", server_name)) {
                            registry.register(name, tool).await;
                        }
                    }
                }

                Ok(ToolOutput::new(output).with_title(format!("MCP: Connected {}", server_name)))
            }
            Err(e) => {
                crate::logging::warn(&format!(
                    "[tool:mcp] connect failed server={} session_id={} error={}",
                    server_name, session_id, e
                ));
                Ok(
                    ToolOutput::new(format!("Failed to connect to '{}': {}", server_name, e))
                        .with_title("MCP: Connection failed"),
                )
            }
        }
    }

    async fn disconnect_server(&self, params: McpToolInput) -> Result<ToolOutput> {
        let server_name = params
            .server
            .ok_or_else(|| anyhow::anyhow!("'server' is required for disconnect action"))?;

        let manager = self.manager.read().await;
        let connected = manager.connected_servers().await;

        if !connected.contains(&server_name) {
            return Ok(ToolOutput::new(format!(
                "Server '{}' is not connected.\n\nConnected servers: {}",
                server_name,
                if connected.is_empty() {
                    "(none)".to_string()
                } else {
                    connected.join(", ")
                }
            ))
            .with_title("MCP: Not connected"));
        }
        drop(manager);

        let manager = self.manager.read().await;
        manager.disconnect(&server_name).await?;
        drop(manager);

        // Unregister tools for this server
        if let Some(ref registry) = self.registry {
            let removed = registry
                .unregister_prefix(&format!("mcp__{}__", server_name))
                .await;
            crate::logging::info(&format!(
                "MCP: Unregistered {} tools for '{}'",
                removed.len(),
                server_name
            ));
        }

        Ok(
            ToolOutput::new(format!("Disconnected from MCP server '{}'", server_name))
                .with_title(format!("MCP: Disconnected {}", server_name)),
        )
    }

    async fn reload_config(&self, session_id: &str) -> Result<ToolOutput> {
        // Load fresh config
        let config = McpConfig::load();

        if config.servers.is_empty() {
            // Unregister all existing MCP tools before reporting empty
            if let Some(ref registry) = self.registry {
                registry.unregister_prefix("mcp__").await;
            }
            return Ok(ToolOutput::new(
                "No servers found in config.\n\n\
                Add servers to ~/.jcode/mcp.json (global) or .jcode/mcp.json (project):\n\
                {\n  \"servers\": {\n    \"server-name\": {\n      \"command\": \"/path/to/server\",\n      \"args\": [],\n      \"env\": {},\n      \"shared\": true\n    }\n  }\n}\n\n\
                .claude/mcp.json is also supported for compatibility."
            ).with_title("MCP: Empty config"));
        }

        let mut manager = self.manager.write().await;
        let (successes, failures, reload_applied) =
            manager.reload_atomic_preserving_existing().await?;

        let servers = manager.connected_servers().await;
        let all_tools = manager.all_tools().await;
        drop(manager);

        // Re-register tools only after the candidate reload has been accepted.
        // If the reload failed before any new server connected, keep old
        // registry entries so a transient config/server failure does not make a
        // previously usable tool list worse.
        if reload_applied {
            if let Some(ref registry) = self.registry {
                registry.unregister_prefix("mcp__").await;
                let mcp_tools = crate::mcp::create_mcp_tools(Arc::clone(&self.manager)).await;
                for (name, tool) in mcp_tools {
                    registry.register(name, tool).await;
                }
            }
        }

        let mut output = format!(
            "Reloaded MCP config. Connected: {}/{}\n\n",
            successes,
            config.servers.len()
        );

        // Show failures first
        if !failures.is_empty() {
            crate::logging::warn(&format!(
                "[tool:mcp] reload had {} connection failure(s) in session {}: {}",
                failures.len(),
                session_id,
                failures
                    .iter()
                    .map(|(name, error)| format!("{}={}", name, error))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            output.push_str("## Connection Failures\n");
            for (name, error) in &failures {
                output.push_str(&format!("  - {}: {}\n", name, error));
            }
            output.push('\n');
        }

        for server in &servers {
            output.push_str(&format!("## {}\n", server));
            let server_tools: Vec<_> = all_tools.iter().filter(|(s, _)| s == server).collect();

            for (_, tool) in server_tools {
                output.push_str(&format!("  - {}\n", tool.name));
            }
            output.push('\n');
        }

        if !reload_applied {
            output.push_str(
                "## Previous MCP registry preserved\n  Reload did not connect any new servers, so existing registered MCP tools were left in place.\n\n",
            );
        }

        self.append_registry_diagnostics(&mut output).await;

        Ok(ToolOutput::new(output).with_title("MCP: Reloaded"))
    }

    async fn login_server(&self, params: McpToolInput) -> Result<ToolOutput> {
        let server_name = params
            .server
            .ok_or_else(|| anyhow::anyhow!("'server' is required for login action"))?;
        let config = McpConfig::load();
        let server_config = config
            .servers
            .get(&server_name)
            .ok_or_else(|| anyhow::anyhow!("MCP server '{}' not found in config", server_name))?;

        let result = if let Some(callback_url) = params.callback_url.as_deref() {
            crate::mcp::oauth::complete_pending_login(&server_name, callback_url).await
        } else {
            crate::mcp::oauth::login(&server_name, server_config, params.no_browser).await
        };

        match result {
            Ok(tokens) => Ok(ToolOutput::new(format!(
                "Logged in to MCP server '{}'. Token expires at {}. Use {{\"action\": \"reload\"}} to reconnect with OAuth.",
                server_name, tokens.expires_at
            ))
            .with_title(format!("MCP: OAuth login {}", server_name))),
            Err(err) => {
                let message = err.to_string();
                if message.contains("Pending login saved") {
                    Ok(ToolOutput::new(format!(
                        "MCP OAuth login for '{}' is pending. {}",
                        server_name, message
                    ))
                    .with_title("MCP: OAuth login pending"))
                } else {
                    Ok(ToolOutput::new(format!(
                        "Failed to login to MCP server '{}': {}",
                        server_name, message
                    ))
                    .with_title("MCP: OAuth login failed"))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Tool;
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;

    fn create_test_tool() -> McpManagementTool {
        let manager = Arc::new(RwLock::new(McpManager::new()));
        McpManagementTool::new(manager)
    }

    fn create_test_context() -> ToolContext {
        ToolContext {
            session_id: "test-session".to_string(),
            message_id: "test-message".to_string(),
            tool_call_id: "test-tool-call".to_string(),
            working_dir: None,
            stdin_request_tx: None,
            graceful_shutdown_signal: None,
            turn_cancel_signal: None,
            execution_mode: crate::tool::ToolExecutionMode::Direct,
        }
    }

    fn test_mcp_server_config() -> (tempfile::TempDir, McpConfig) {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let server_script = temp_dir.path().join("test-mcp.sh");
        std::fs::write(
            &server_script,
            r#"#!/usr/bin/env bash
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *initialize*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"test","version":"1.0.0"}}}\n' "$id"
      ;;
    *tools/list*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"tools":[{"name":"hello","description":"Test hello","inputSchema":{"type":"object","properties":{}}}]}}\n' "$id"
      ;;
  esac
done
"#,
        )
        .expect("write MCP test script");

        let mut servers = HashMap::new();
        servers.insert(
            "atomic".to_string(),
            McpServerConfig {
                command: "bash".to_string(),
                args: vec![server_script.to_string_lossy().to_string()],
                env: HashMap::new(),
                transport: None,
                url: None,
                headers: HashMap::new(),
                auth: None,
                shared: false,
            },
        );
        (temp_dir, McpConfig { servers })
    }

    struct LocalMcpConfigGuard {
        path: PathBuf,
        backup: Option<String>,
        created_dir: bool,
    }

    impl LocalMcpConfigGuard {
        fn new(content: &str) -> std::io::Result<Self> {
            let path = PathBuf::from(".jcode/mcp.json");
            let dir = path
                .parent()
                .ok_or_else(|| std::io::Error::other("missing parent"))?;
            let created_dir = if !dir.exists() {
                fs::create_dir_all(dir)?;
                true
            } else {
                false
            };
            let backup = if path.exists() {
                Some(fs::read_to_string(&path)?)
            } else {
                None
            };
            fs::write(&path, content)?;
            Ok(Self {
                path,
                backup,
                created_dir,
            })
        }
    }

    impl Drop for LocalMcpConfigGuard {
        fn drop(&mut self) {
            match &self.backup {
                Some(content) => {
                    let _ = fs::write(&self.path, content);
                }
                None => {
                    let _ = fs::remove_file(&self.path);
                    if self.created_dir
                        && let Some(dir) = self.path.parent()
                    {
                        let _ = fs::remove_dir(dir);
                    }
                }
            }
        }
    }

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set_path(key: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var_os(key);
            crate::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => crate::env::set_var(self.key, value),
                None => crate::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn test_tool_name() {
        let tool = create_test_tool();
        assert_eq!(tool.name(), "mcp");
    }

    #[test]
    fn test_tool_description() {
        let tool = create_test_tool();
        assert!(tool.description().contains("MCP"));
        assert!(tool.description().contains("Model Context Protocol"));
    }

    #[test]
    fn test_parameters_schema() {
        let tool = create_test_tool();
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["action"].is_object());
        assert!(schema["properties"]["server"].is_object());
        assert!(schema["properties"]["command"].is_object());
    }

    #[tokio::test]
    async fn test_list_empty() {
        let tool = create_test_tool();
        let ctx = create_test_context();
        let input = json!({"action": "list"});

        let result = tool.execute(input, ctx).await.unwrap();
        assert!(result.output.contains("No MCP servers connected"));
    }

    #[tokio::test]
    async fn test_list_includes_registry_diagnostics_when_registry_is_available() {
        let registry = crate::tool::Registry::empty();
        let manager = Arc::new(RwLock::new(McpManager::new()));
        let tool = McpManagementTool::new(manager).with_registry(registry.clone());
        let ctx = create_test_context();
        let input = json!({"action": "list"});

        let result = tool.execute(input, ctx).await.unwrap();
        assert!(result.output.contains("## Registry diagnostics"));
        assert!(
            result
                .output
                .contains("mcp management tool registered: false")
        );
        assert!(result.output.contains("registered MCP server tools: 0"));
    }

    #[tokio::test]
    async fn test_reload_preserves_existing_registry_tools_when_candidate_fails() {
        let isolated_home = tempfile::tempdir().expect("isolated JCODE_HOME");
        let _home = EnvVarGuard::set_path("JCODE_HOME", isolated_home.path());
        let registry = crate::tool::Registry::empty();
        let (_temp_dir, initial_config) = test_mcp_server_config();
        let manager = Arc::new(RwLock::new(McpManager::with_config(initial_config)));
        let tool = McpManagementTool::new(Arc::clone(&manager)).with_registry(registry.clone());

        {
            let manager_guard = manager.write().await;
            let (successes, failures) = manager_guard.connect_all().await.unwrap();
            assert_eq!(successes, 1);
            assert!(failures.is_empty());
        }
        let report = registry
            .reconcile_mcp_tools_from_manager(Arc::clone(&manager))
            .await;
        assert_eq!(report.repaired_tool_names, vec!["mcp__atomic__hello"]);

        let _guard = LocalMcpConfigGuard::new(
            r#"{
  "servers": {
    "broken": {
      "command": "/definitely/missing/mcp-server",
      "args": [],
      "env": {},
      "shared": false
    }
  }
}"#,
        )
        .expect("write broken reload config");

        let result = tool
            .execute(json!({"action": "reload"}), create_test_context())
            .await
            .unwrap();
        assert!(result.output.contains("Previous MCP registry preserved"));
        assert_eq!(
            registry
                .mcp_registry_diagnostics()
                .await
                .mcp_server_tool_names,
            vec!["mcp__atomic__hello"]
        );
        assert_eq!(manager.read().await.connected_servers().await, vec!["atomic"]);
    }

    #[tokio::test]
    async fn test_connect_missing_server() {
        let tool = create_test_tool();
        let ctx = create_test_context();
        let input = json!({"action": "connect", "command": "/bin/test"});

        let result = tool.execute(input, ctx).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("server"));
    }

    #[tokio::test]
    async fn test_connect_missing_command() {
        let tool = create_test_tool();
        let ctx = create_test_context();
        let input = json!({"action": "connect", "server": "test"});

        let result = tool.execute(input, ctx).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("command"));
    }

    #[tokio::test]
    async fn test_disconnect_not_connected() {
        let tool = create_test_tool();
        let ctx = create_test_context();
        let input = json!({"action": "disconnect", "server": "nonexistent"});

        let result = tool.execute(input, ctx).await.unwrap();
        assert!(result.output.contains("not connected"));
    }

    #[tokio::test]
    async fn test_unknown_action() {
        let tool = create_test_tool();
        let ctx = create_test_context();
        let input = json!({"action": "invalid_action"});

        let result = tool.execute(input, ctx).await.unwrap();
        assert!(result.output.contains("Unknown action"));
    }

    #[tokio::test]
    async fn test_reload_empty_config() {
        let _guard =
            LocalMcpConfigGuard::new("{\"servers\":{}}").expect("create temporary .jcode/mcp.json");
        let tool = create_test_tool();
        let ctx = create_test_context();
        let input = json!({"action": "reload"});

        let result = tool.execute(input, ctx).await.unwrap();
        // With config merging, global config may have servers.
        // If both are empty: "No servers found in config"
        // If global has servers: "Reloaded MCP config" (may show connection failures)
        assert!(
            result.output.contains("No servers")
                || result.output.contains("Empty config")
                || result.output.contains("Connected servers: 0")
                || result.output.contains("Reloaded MCP config")
        );
    }
}
