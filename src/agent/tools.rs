use super::Agent;
use crate::message::{ContentBlock, ToolCall};
use crate::tool::ToolOutput;
use std::path::PathBuf;

pub(super) fn tool_output_to_content_blocks(
    tool_use_id: String,
    output: ToolOutput,
) -> Vec<ContentBlock> {
    let mut blocks = vec![ContentBlock::ToolResult {
        tool_use_id,
        content: output.output,
        is_error: None,
    }];
    for img in output.images {
        blocks.push(ContentBlock::Image {
            media_type: img.media_type,
            data: img.data,
        });
        if let Some(label) = img.label.filter(|label| !label.trim().is_empty()) {
            blocks.push(ContentBlock::Text {
                text: format!(
                    "[Attached image associated with the preceding tool result: {}]",
                    label
                ),
                cache_control: None,
            });
        }
    }
    blocks
}

pub(super) fn print_tool_summary(tool: &ToolCall) {
    match tool.name.as_str() {
        "bash" => {
            if let Some(cmd) = tool.input.get("command").and_then(|v| v.as_str()) {
                let short = if cmd.len() > 60 {
                    format!("{}...", crate::util::truncate_str(cmd, 60))
                } else {
                    cmd.to_string()
                };
                println!("$ {}", short);
            }
        }
        "read" | "write" | "edit" => {
            if let Some(path) = tool.input.get("file_path").and_then(|v| v.as_str()) {
                println!("{}", path);
            }
        }
        "glob" | "grep" => {
            if let Some(pattern) = tool.input.get("pattern").and_then(|v| v.as_str()) {
                println!("'{}'", pattern);
            }
        }
        "ls" => {
            let path = tool
                .input
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            println!("{}", path);
        }
        _ => {}
    }
}

impl Agent {
    pub(super) fn apply_tool_output_side_effects(
        &mut self,
        tool_name: &str,
        output: &ToolOutput,
    ) -> anyhow::Result<()> {
        if !matches!(tool_name, "cwd" | "pwd" | "cd") {
            return Ok(());
        }

        let Some(session_cwd) = output
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("session_cwd"))
        else {
            return Ok(());
        };
        let Some(working_dir) = session_cwd
            .get("working_dir")
            .and_then(|value| value.as_str())
        else {
            return Ok(());
        };

        self.set_working_dir_and_save(working_dir)?;
        if session_cwd
            .get("refresh_skills")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            let _ = self.refresh_skills_for_working_dir()?;
        }
        crate::tui::session_picker::invalidate_session_list_cache();
        Ok(())
    }

    pub(super) fn inject_nested_instructions_for_tool_calls(
        &mut self,
        tool_calls: &[ToolCall],
    ) -> bool {
        let touched_paths = nested_instruction_touched_paths(tool_calls);
        if touched_paths.is_empty() {
            return false;
        }

        let instructions = crate::prompt::load_nested_instructions_for_paths(
            self.working_dir().map(std::path::Path::new),
            touched_paths,
        );
        if instructions.is_empty() {
            return false;
        }

        let instructions: Vec<_> = instructions
            .into_iter()
            .filter(|instruction| {
                let key = std::fs::canonicalize(&instruction.path)
                    .unwrap_or_else(|_| instruction.path.clone());
                if self.nested_private_instruction_keys.contains(&key) {
                    false
                } else {
                    self.nested_private_instruction_keys.insert(key);
                    true
                }
            })
            .collect();
        if instructions.is_empty() {
            return false;
        }

        let mut text = String::from(
            "# Nested Instructions\n\nThe following AGENTS.md/agents.md and private `.jcode/` instructions are relevant to files just read/searched/edited in this turn. Read and follow them for the next steps. Private `.jcode/` instructions take priority over public AGENTS instructions when they conflict.\n",
        );
        for instruction in instructions {
            text.push_str("\n## ");
            text.push_str(&instruction.label);
            text.push_str("\n\n");
            text.push_str(&instruction.content);
            text.push('\n');
        }

        self.add_message(
            crate::message::Role::User,
            vec![ContentBlock::Text {
                text,
                cache_control: None,
            }],
        );
        true
    }
}

fn nested_instruction_touched_paths(tool_calls: &[ToolCall]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for tool in tool_calls {
        match tool.name.as_str() {
            "read" | "write" | "edit" | "multiedit" => {
                if let Some(path) = tool.input.get("file_path").and_then(|value| value.as_str()) {
                    paths.push(PathBuf::from(path));
                }
            }
            "grep" | "glob" | "ls" => {
                if let Some(path) = tool.input.get("path").and_then(|value| value.as_str()) {
                    paths.push(PathBuf::from(path));
                }
            }
            "agentgrep" => {
                if let Some(file) = tool.input.get("file").and_then(|value| value.as_str()) {
                    paths.push(PathBuf::from(file));
                } else if let Some(path) = tool.input.get("path").and_then(|value| value.as_str()) {
                    paths.push(PathBuf::from(path));
                }
            }
            _ => {}
        }
    }
    paths
}
