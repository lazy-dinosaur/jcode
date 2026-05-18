//! MCP Tool - wraps MCP server tools for jcode's tool system

use super::manager::McpManager;
use super::protocol::{ContentBlock, McpToolDef};
use crate::tool::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
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

    async fn execute(&self, input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let input = if input.is_null() {
            Value::Object(serde_json::Map::new())
        } else {
            input
        };
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
}
