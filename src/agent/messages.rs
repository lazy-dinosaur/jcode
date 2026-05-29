use super::*;

fn is_count_wrapper_noise_line(line: &str) -> bool {
    line.trim().eq_ignore_ascii_case("count")
}

fn strip_count_wrapper_noise_edges(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    let mut start = 0usize;
    while start < lines.len()
        && (lines[start].trim().is_empty() || is_count_wrapper_noise_line(lines[start]))
    {
        start += 1;
    }

    let mut end = lines.len();
    while end > start
        && (lines[end - 1].trim().is_empty() || is_count_wrapper_noise_line(lines[end - 1]))
    {
        end -= 1;
    }

    lines[start..end].join("\n").trim().to_string()
}

fn normalize_assistant_tool_call_noise(
    role: &Role,
    content: Vec<ContentBlock>,
) -> Vec<ContentBlock> {
    if !matches!(role, Role::Assistant)
        || !content
            .iter()
            .any(|block| matches!(block, ContentBlock::ToolUse { .. }))
    {
        return content;
    }

    content
        .into_iter()
        .filter_map(|block| match block {
            ContentBlock::Text {
                text,
                cache_control,
            } => {
                let text = strip_count_wrapper_noise_edges(&text);
                if text.is_empty() {
                    None
                } else {
                    Some(ContentBlock::Text {
                        text,
                        cache_control,
                    })
                }
            }
            other => Some(other),
        })
        .collect()
}

impl Agent {
    pub(crate) fn interruption_text_for_reason(reason: Option<TurnStopReason>) -> &'static str {
        match reason {
            Some(TurnStopReason::ServerReload) => "[Interrupted: server reloading]",
            Some(TurnStopReason::BackgroundCurrentTool) => "[Moved to background]",
            Some(TurnStopReason::ClientDisconnect) => "[Interrupted: client disconnected]",
            Some(TurnStopReason::Superseded) => "[Interrupted: superseded]",
            Some(TurnStopReason::UserInterrupt) | None => "[Interrupted: user cancelled]",
        }
    }

    pub(crate) fn add_interrupted_tool_results_for_calls(
        &mut self,
        tool_calls: &[ToolCall],
        reason: Option<TurnStopReason>,
        duration_ms: Option<u64>,
    ) -> usize {
        if tool_calls.is_empty() {
            return 0;
        }

        let content = Self::interruption_text_for_reason(reason).to_string();
        let blocks = tool_calls
            .iter()
            .map(|tc| ContentBlock::ToolResult {
                tool_use_id: tc.id.clone(),
                content: content.clone(),
                is_error: Some(!matches!(
                    reason,
                    Some(TurnStopReason::BackgroundCurrentTool)
                )),
            })
            .collect::<Vec<_>>();
        self.add_message_with_duration(Role::User, blocks, duration_ms);
        tool_calls.len()
    }

    pub(crate) fn persist_interrupted_assistant_turn(
        &mut self,
        text_content: &str,
        reasoning_content: &str,
        store_reasoning_content: bool,
        tool_calls: &[ToolCall],
        current_tool: Option<ToolCall>,
        current_tool_input: &str,
        reason: Option<TurnStopReason>,
    ) -> Result<bool> {
        let mut finalized_tool_calls = tool_calls.to_vec();
        if let Some(mut tool) = current_tool {
            tool.input = serde_json::from_str::<serde_json::Value>(current_tool_input)
                .unwrap_or(serde_json::Value::Null);
            tool.refresh_intent_from_input();
            finalized_tool_calls.push(tool);
        }

        let mut content_blocks = Vec::new();
        if !text_content.is_empty() {
            content_blocks.push(ContentBlock::Text {
                text: format!(
                    "{}\n\n{}",
                    text_content,
                    Self::interruption_text_for_reason(reason)
                ),
                cache_control: None,
            });
        }
        if store_reasoning_content && !reasoning_content.is_empty() {
            content_blocks.push(ContentBlock::Reasoning {
                text: reasoning_content.to_string(),
            });
        }
        for tc in &finalized_tool_calls {
            content_blocks.push(ContentBlock::ToolUse {
                id: tc.id.clone(),
                name: tc.name.clone(),
                input: tc.input.clone(),
            });
        }

        if content_blocks.is_empty() {
            return Ok(false);
        }

        self.add_message(Role::Assistant, content_blocks);
        self.add_interrupted_tool_results_for_calls(&finalized_tool_calls, reason, None);
        self.session.save()?;
        Ok(true)
    }

    pub(crate) fn add_message(&mut self, role: Role, content: Vec<ContentBlock>) -> String {
        let content = normalize_assistant_tool_call_noise(&role, content);
        let id = self.session.add_message(role, content);
        let compaction = self.registry.compaction();
        if let Ok(mut manager) = compaction.try_write() {
            if let Some(message) = self.session.messages.last() {
                manager.notify_message_added_blocks(&message.content);
            } else {
                manager.notify_message_added();
            }
        }
        id
    }

    pub(crate) fn add_message_with_display_role(
        &mut self,
        role: Role,
        content: Vec<ContentBlock>,
        display_role: Option<StoredDisplayRole>,
    ) -> String {
        let content = normalize_assistant_tool_call_noise(&role, content);
        let id = self
            .session
            .add_message_with_display_role(role, content, display_role);
        let compaction = self.registry.compaction();
        if let Ok(mut manager) = compaction.try_write() {
            if let Some(message) = self.session.messages.last() {
                manager.notify_message_added_blocks(&message.content);
            } else {
                manager.notify_message_added();
            }
        }
        id
    }

    pub(crate) fn add_message_with_duration(
        &mut self,
        role: Role,
        content: Vec<ContentBlock>,
        duration_ms: Option<u64>,
    ) -> String {
        let content = normalize_assistant_tool_call_noise(&role, content);
        let id = self
            .session
            .add_message_with_duration(role, content, duration_ms);
        let compaction = self.registry.compaction();
        if let Ok(mut manager) = compaction.try_write() {
            if let Some(message) = self.session.messages.last() {
                manager.notify_message_added_blocks(&message.content);
            } else {
                manager.notify_message_added();
            }
        }
        id
    }

    pub(crate) fn add_message_ext(
        &mut self,
        role: Role,
        content: Vec<ContentBlock>,
        duration_ms: Option<u64>,
        token_usage: Option<crate::session::StoredTokenUsage>,
    ) -> String {
        let content = normalize_assistant_tool_call_noise(&role, content);
        let id = self
            .session
            .add_message_ext(role, content, duration_ms, token_usage);
        let compaction = self.registry.compaction();
        if let Ok(mut manager) = compaction.try_write() {
            if let Some(message) = self.session.messages.last() {
                manager.notify_message_added_blocks(&message.content);
            } else {
                manager.notify_message_added();
            }
        }
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_assistant_tool_call_noise_drops_count_text_block() {
        let content = vec![
            ContentBlock::Text {
                text: "count\n\ncount".to_string(),
                cache_control: None,
            },
            ContentBlock::ToolUse {
                id: "tool_1".to_string(),
                name: "read".to_string(),
                input: serde_json::json!({"file_path":"Cargo.toml"}),
            },
        ];

        let normalized = normalize_assistant_tool_call_noise(&Role::Assistant, content);
        assert_eq!(normalized.len(), 1);
        assert!(matches!(normalized[0], ContentBlock::ToolUse { .. }));
    }

    #[test]
    fn normalize_assistant_tool_call_noise_strips_edges_only() {
        let content = vec![
            ContentBlock::Text {
                text: "count\n\nI will inspect it.\n\ncount".to_string(),
                cache_control: None,
            },
            ContentBlock::ToolUse {
                id: "tool_1".to_string(),
                name: "read".to_string(),
                input: serde_json::json!({"file_path":"Cargo.toml"}),
            },
        ];

        let normalized = normalize_assistant_tool_call_noise(&Role::Assistant, content);
        match &normalized[0] {
            ContentBlock::Text { text, .. } => assert_eq!(text, "I will inspect it."),
            other => panic!("expected text block, got {other:?}"),
        }
        assert!(matches!(normalized[1], ContentBlock::ToolUse { .. }));
    }

    #[test]
    fn normalize_assistant_tool_call_noise_does_not_touch_user_text() {
        let content = vec![ContentBlock::Text {
            text: "count".to_string(),
            cache_control: None,
        }];

        let normalized = normalize_assistant_tool_call_noise(&Role::User, content);
        match &normalized[0] {
            ContentBlock::Text { text, .. } => assert_eq!(text, "count"),
            other => panic!("expected text block, got {other:?}"),
        }
    }
}
