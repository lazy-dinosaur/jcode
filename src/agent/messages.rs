use super::*;

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
