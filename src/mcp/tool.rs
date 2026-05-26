//! MCP Tool - wraps MCP server tools for jcode's tool system

use super::manager::McpManager;
use super::protocol::{ContentBlock, McpToolDef};
use crate::tool::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A tool that proxies to an MCP server
pub struct McpTool {
    server_name: String,
    tool_def: McpToolDef,
    manager: Arc<RwLock<McpManager>>,
}

impl McpTool {
    pub fn new(
        server_name: String,
        tool_def: McpToolDef,
        manager: Arc<RwLock<McpManager>>,
    ) -> Self {
        Self {
            server_name,
            tool_def,
            manager,
        }
    }

    fn is_unknown_tool_error(error: &anyhow::Error) -> bool {
        let text = format!("{:#}", error).to_ascii_lowercase();
        text.contains("unknowntool") || text.contains("unknown tool")
    }

    fn normalize_input(&self, input: Value, ctx: &ToolContext) -> Value {
        normalize_mcp_input(&self.server_name, &self.tool_def.name, input, ctx)
    }
}

fn normalize_filesystem_path_value(value: Value, ctx: &ToolContext) -> Value {
    let Value::String(path) = value else {
        return value;
    };
    if path.trim().is_empty() {
        return Value::String(path);
    }
    let resolved = ctx.resolve_path(Path::new(&path));
    Value::String(resolved.to_string_lossy().into_owned())
}

fn normalize_filesystem_paths_in_object(
    obj: &mut serde_json::Map<String, Value>,
    ctx: &ToolContext,
) {
    if let Some(path) = obj.remove("path") {
        obj.insert(
            "path".to_string(),
            normalize_filesystem_path_value(path, ctx),
        );
    }

    if let Some(Value::Array(paths)) = obj.get_mut("paths") {
        for path in paths {
            let current = std::mem::take(path);
            *path = normalize_filesystem_path_value(current, ctx);
        }
    }
}

fn normalize_mcp_input(
    server_name: &str,
    tool_name: &str,
    input: Value,
    ctx: &ToolContext,
) -> Value {
    if server_name != "filesystem" {
        return input;
    }

    match input {
        Value::String(value) if tool_name == "search_files" => {
            let mut obj = serde_json::Map::new();
            obj.insert("path".to_string(), Value::String(".".to_string()));
            obj.insert("pattern".to_string(), Value::String(value));
            normalize_filesystem_paths_in_object(&mut obj, ctx);
            Value::Object(obj)
        }
        Value::String(value) => {
            json!({ "path": normalize_filesystem_path_value(Value::String(value), ctx) })
        }
        Value::Object(mut obj) => {
            if !obj.contains_key("path") {
                for alias in ["file_path", "file", "filename", "dir", "directory"] {
                    if let Some(value) = obj.remove(alias) {
                        obj.insert("path".to_string(), value);
                        break;
                    }
                }
            }
            if tool_name == "search_files" && !obj.contains_key("pattern") {
                for alias in ["query", "glob", "include", "name"] {
                    if let Some(value) = obj.remove(alias) {
                        obj.insert("pattern".to_string(), value);
                        break;
                    }
                }
            }
            if tool_name == "search_files" && !obj.contains_key("path") {
                obj.insert("path".to_string(), Value::String(".".to_string()));
            }
            normalize_filesystem_paths_in_object(&mut obj, ctx);
            Value::Object(obj)
        }
        other => other,
    }
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        // This will be overridden in registration with prefixed name
        &self.tool_def.name
    }

    fn description(&self) -> &str {
        self.tool_def.description.as_deref().unwrap_or("MCP tool")
    }

    fn parameters_schema(&self) -> Value {
        self.tool_def.input_schema.clone()
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let input = if input.is_null() {
            Value::Object(serde_json::Map::new())
        } else {
            input
        };
        let input = self.normalize_input(input, &ctx);
        let manager = self.manager.read().await;
        let result = match manager
            .call_tool(&self.server_name, &self.tool_def.name, input.clone())
            .await
        {
            Ok(result) => result,
            Err(err) if Self::is_unknown_tool_error(&err) => {
                crate::logging::warn(&format!(
                    "MCP tool '{}:{}' returned UnknownTool; refreshing tools and retrying once",
                    self.server_name, self.tool_def.name
                ));
                let _ = manager.refresh_server_tools(&self.server_name).await;
                match manager
                    .call_tool(&self.server_name, &self.tool_def.name, input)
                    .await
                {
                    Ok(result) => result,
                    Err(retry_err) if Self::is_unknown_tool_error(&retry_err) => {
                        let title = format!("mcp:{}:{}", self.server_name, self.tool_def.name);
                        return Ok(ToolOutput::new(format!(
                            "Error: MCP tool '{}' is no longer available on server '{}'. The server reported UnknownTool even after refreshing the tool list. Try reconnecting or reloading MCP tools before calling it again.",
                            self.tool_def.name, self.server_name
                        ))
                        .with_title(title));
                    }
                    Err(retry_err) => return Err(retry_err),
                }
            }
            Err(err) => return Err(err),
        };

        // Convert MCP content blocks to output string
        let mut output_parts = Vec::new();
        for block in result.content {
            match block {
                ContentBlock::Text { text } => {
                    output_parts.push(text);
                }
                ContentBlock::Image { data, mime_type } => {
                    output_parts.push(format!("[Image: {} ({} bytes)]", mime_type, data.len()));
                }
                ContentBlock::Resource { resource } => {
                    if let Some(text) = resource.text {
                        output_parts.push(text);
                    } else if let Some(blob) = resource.blob {
                        output_parts.push(format!(
                            "[Resource: {} ({} bytes)]",
                            resource.uri,
                            blob.len()
                        ));
                    } else {
                        output_parts.push(format!("[Resource: {}]", resource.uri));
                    }
                }
                ContentBlock::ResourceLink(resource) => {
                    let label = resource
                        .title
                        .as_deref()
                        .or(resource.name.as_deref())
                        .unwrap_or("Resource link");
                    let mime = resource
                        .mime_type
                        .as_deref()
                        .map(|mime| format!(" ({mime})"))
                        .unwrap_or_default();
                    if let Some(description) = resource.description.as_deref() {
                        output_parts.push(format!(
                            "[{label}{mime}: {}]\n{}",
                            resource.uri, description
                        ));
                    } else {
                        output_parts.push(format!("[{label}{mime}: {}]", resource.uri));
                    }
                }
            }
        }

        let output = output_parts.join("\n");
        let title = format!("mcp:{}:{}", self.server_name, self.tool_def.name);

        if result.is_error {
            Ok(ToolOutput::new(format!("Error: {}", output)).with_title(title))
        } else {
            Ok(ToolOutput::new(output).with_title(title))
        }
    }
}

/// Create tools from an MCP manager
pub async fn create_mcp_tools(manager: Arc<RwLock<McpManager>>) -> Vec<(String, Arc<dyn Tool>)> {
    let mgr = manager.read().await;
    let all_tools = mgr.all_tools().await;
    drop(mgr);

    let mut tools = Vec::new();
    for (server_name, tool_def) in all_tools {
        let prefixed_name = format!("mcp__{}__{}", server_name, tool_def.name);
        let mcp_tool = McpTool::new(server_name, tool_def, Arc::clone(&manager));
        tools.push((prefixed_name, Arc::new(mcp_tool) as Arc<dyn Tool>));
    }
    tools
}

#[cfg(test)]
mod tests {
    use super::*;
    use jcode_tool_core::ToolExecutionMode;
    use std::path::PathBuf;

    fn test_ctx() -> ToolContext {
        ToolContext {
            session_id: "session".to_string(),
            message_id: "message".to_string(),
            tool_call_id: "tool".to_string(),
            working_dir: Some(PathBuf::from("/workspace/project")),
            stdin_request_tx: None,
            graceful_shutdown_signal: None,
            turn_cancel_signal: None,
            execution_mode: ToolExecutionMode::Direct,
        }
    }

    #[test]
    fn detects_common_unknown_tool_errors() {
        assert!(McpTool::is_unknown_tool_error(&anyhow::anyhow!(
            "MCP error -32602: UnknownTool: inspect_frame"
        )));
        assert!(McpTool::is_unknown_tool_error(&anyhow::anyhow!(
            "unknown tool: figma_get_node"
        )));
        assert!(!McpTool::is_unknown_tool_error(&anyhow::anyhow!(
            "MCP error -32000: timeout"
        )));
    }

    #[test]
    fn normalizes_filesystem_path_aliases() {
        assert_eq!(
            normalize_mcp_input(
                "filesystem",
                "read_text_file",
                json!({"file_path": "README.md"}),
                &test_ctx(),
            ),
            json!({"path": "/workspace/project/README.md"})
        );
        assert_eq!(
            normalize_mcp_input("filesystem", "list_directory", json!("src"), &test_ctx()),
            json!({"path": "/workspace/project/src"})
        );
    }

    #[test]
    fn normalizes_filesystem_search_aliases() {
        assert_eq!(
            normalize_mcp_input("filesystem", "search_files", json!("*.tsx"), &test_ctx()),
            json!({"path": "/workspace/project/.", "pattern": "*.tsx"})
        );
        assert_eq!(
            normalize_mcp_input(
                "filesystem",
                "search_files",
                json!({"query": "*.md"}),
                &test_ctx(),
            ),
            json!({"path": "/workspace/project/.", "pattern": "*.md"})
        );
    }

    #[test]
    fn normalizes_filesystem_paths_array() {
        assert_eq!(
            normalize_mcp_input(
                "filesystem",
                "read_multiple_files",
                json!({"paths": ["README.md", "/tmp/absolute.md"]}),
                &test_ctx(),
            ),
            json!({"paths": ["/workspace/project/README.md", "/tmp/absolute.md"]})
        );
    }
}
