use super::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;

pub struct CwdTool;

impl CwdTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct CwdInput {
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    intent: Option<String>,
}

fn normalized_action(input: &CwdInput) -> &str {
    input
        .action
        .as_deref()
        .unwrap_or_else(|| if input.path.is_some() { "set" } else { "show" })
}

fn current_dir(ctx: &ToolContext) -> Option<&Path> {
    ctx.working_dir.as_deref()
}

#[async_trait]
impl Tool for CwdTool {
    fn name(&self) -> &str {
        "cwd"
    }

    fn description(&self) -> &str {
        "Show or change the current session working directory. Use action='show' like /pwd or /cwd, and action='set' with path like /cwd <path> or /cd <path>. Changing cwd affects subsequent file, shell, subagent, and swarm tool calls in this session."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "intent": super::intent_schema_property(),
                "action": {
                    "type": "string",
                    "enum": ["show", "set", "pwd", "cwd", "cd"],
                    "description": "Action. show/pwd/cwd displays the session cwd. set/cd changes it."
                },
                "path": {
                    "type": "string",
                    "description": "Directory to switch to for action=set or action=cd. Relative paths resolve from the current session cwd. Supports ~ and ~/... ."
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: CwdInput = serde_json::from_value(input)?;
        let _intent = params.intent.as_deref();
        match normalized_action(&params) {
            "show" | "pwd" | "cwd" => {
                Ok(ToolOutput::new(crate::cwd::format_cwd(current_dir(&ctx)))
                    .with_title("cwd")
                    .with_metadata(json!({
                        "working_dir": current_dir(&ctx).map(|dir| dir.display().to_string())
                    })))
            }
            "set" | "cd" => {
                let raw = params
                    .path
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("'path' is required for cwd action=set/cd"))?;
                let dir = crate::cwd::resolve_cwd_path(current_dir(&ctx), raw)?;
                let dir_string = dir.display().to_string();
                Ok(ToolOutput::new(format!(
                    "✓ Session cwd switched to `{}`. Conversation context was preserved.",
                    dir_string
                ))
                .with_title("cwd set")
                .with_metadata(json!({
                    "session_cwd": {
                        "working_dir": dir_string,
                        "refresh_skills": true
                    }
                })))
            }
            other => Err(anyhow::anyhow!(
                "Unknown cwd action '{}'. Use action='show' or action='set'.",
                other
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ToolExecutionMode;

    fn ctx(dir: &Path) -> ToolContext {
        ToolContext {
            session_id: "session-test".to_string(),
            message_id: "msg-test".to_string(),
            tool_call_id: "call-test".to_string(),
            working_dir: Some(dir.to_path_buf()),
            stdin_request_tx: None,
            graceful_shutdown_signal: None,
            execution_mode: ToolExecutionMode::Direct,
        }
    }

    #[tokio::test]
    async fn cwd_tool_shows_current_session_dir() {
        let temp = tempfile::tempdir().unwrap();
        let output = CwdTool::new()
            .execute(json!({"action":"show"}), ctx(temp.path()))
            .await
            .unwrap();
        assert!(output.output.contains("Session cwd:"));
        assert_eq!(
            output.metadata.unwrap()["working_dir"].as_str(),
            Some(temp.path().to_str().unwrap())
        );
    }

    #[tokio::test]
    async fn cwd_tool_set_returns_session_side_effect_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let child = temp.path().join("child");
        std::fs::create_dir(&child).unwrap();
        let output = CwdTool::new()
            .execute(json!({"action":"set", "path":"child"}), ctx(temp.path()))
            .await
            .unwrap();
        assert!(output.output.contains("Session cwd switched"));
        assert_eq!(
            output.metadata.unwrap()["session_cwd"]["working_dir"].as_str(),
            Some(child.canonicalize().unwrap().to_str().unwrap())
        );
    }
}
