use super::turn_execution::PresetToolResult;
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LifecycleHookOutcome {
    Stop,
    ContinueImmediate,
    ContinueImmediateWithInject(crate::hooks::HookInjectContinuation),
}

impl Agent {
    /// M35 Round 22: default 를 claude-code 호환 무제한 (0 = no cap) 으로 변경.
    /// claude-code 의 Stop hook spec 은 cap 없이 `stop_hook_active` 만 제공하고
    /// hook script 가 스스로 self-throttle 책임을 진다. jcode 도 동일 정책 채택.
    /// 사용자가 hardcap 을 원하면 `max_lifecycle_deny_streak = N` 으로 명시 설정.
    pub(super) const DEFAULT_MAX_LIFECYCLE_DENY_STREAK: u8 = 0;

    fn lifecycle_hook_reminder(reason: &str) -> String {
        format!(
            "A lifecycle hook denied completion of the previous turn. Follow this instruction before stopping again:\n\n{}",
            reason.trim()
        )
    }

    pub(super) fn resolve_max_lifecycle_deny_streak_for_current_session(&self) -> u8 {
        let configured = crate::config::config()
            .agents_for_working_dir(
                self.session
                    .working_dir
                    .as_deref()
                    .map(std::path::Path::new),
            )
            .max_lifecycle_deny_streak;
        Self::resolve_max_lifecycle_deny_streak_with_config(configured)
    }

    pub(crate) fn resolve_max_lifecycle_deny_streak_with_config(configured: Option<u8>) -> u8 {
        if let Ok(value) = std::env::var("JCODE_MAX_LIFECYCLE_DENY_STREAK")
            && let Ok(parsed) = value.trim().parse::<u8>()
        {
            return parsed;
        }
        configured.unwrap_or(Self::DEFAULT_MAX_LIFECYCLE_DENY_STREAK)
    }

    pub(super) fn set_pending_lifecycle_system_reminder(&mut self, reason: String) {
        let reason = reason.trim();
        if reason.is_empty() {
            return;
        }
        self.pending_lifecycle_system_reminder = Some(Self::lifecycle_hook_reminder(reason));
    }

    pub(super) fn take_pending_lifecycle_system_reminder(&mut self) -> Option<String> {
        self.pending_lifecycle_system_reminder.take()
    }

    /// M11 stage 6: a new user turn starts a fresh self-correction chain.
    pub(super) fn reset_lifecycle_deny_streak_for_user_turn(&mut self) {
        self.lifecycle_deny_streak = 0;
        self.nested_private_instruction_keys.clear();
    }

    /// M11 stage 6 fix: continuation requires the conversation to end with a
    /// `Role::User` message. The previous turn's last message is the assistant
    /// response that just got denied, so a bare `continue` would leave the
    /// conversation ending with assistant and Anthropic (and other providers)
    /// will reject the next call with "must end with user message".
    ///
    /// We inject the lifecycle reminder as a user-authored `<system-reminder>`
    /// block. This matches the claude-code stop-hook pattern: reminders live
    /// inline inside the conversation (not in the system prompt) so the model
    /// sees them as part of the dialogue. The system-prompt-area
    /// `current_turn_system_reminder` is intentionally left alone — that
    /// channel is for the "next user turn" pathway (Stage 1+2 fallback when
    /// the deny streak cap is hit), not for in-place continuations.
    pub(super) fn inject_lifecycle_reminder_for_continuation(&mut self) {
        let Some(reminder) = self.take_pending_lifecycle_system_reminder() else {
            return;
        };
        let trimmed = reminder.trim();
        if trimmed.is_empty() {
            return;
        }
        self.add_message(
            Role::User,
            vec![ContentBlock::Text {
                text: format!("<system-reminder>\n{trimmed}\n</system-reminder>"),
                cache_control: None,
            }],
        );
        if let Err(err) = self.session.save() {
            logging::warn(&format!(
                "[m11-stage6] failed to save session after continuation reminder inject: {err:#}"
            ));
        }
    }

    pub(super) fn inject_hook_body_for_continuation(
        &mut self,
        inject: crate::hooks::HookInjectContinuation,
    ) {
        let trimmed = inject.body.trim();
        if trimmed.is_empty() {
            return;
        }
        let text = match inject.format {
            crate::turn::injected_context::InjectionFormat::SystemReminder => {
                format!("<system-reminder>\n{trimmed}\n</system-reminder>")
            }
            crate::turn::injected_context::InjectionFormat::UserMessage => trimmed.to_string(),
        };
        self.add_message(
            Role::User,
            vec![ContentBlock::Text {
                text,
                cache_control: None,
            }],
        );
        if let Err(err) = self.session.save() {
            logging::warn(&format!(
                "[m35] failed to save session after hook inject continuation: {err:#}"
            ));
        }
    }

    #[cfg(test)]
    pub(crate) fn inject_lifecycle_reminder_for_continuation_for_tests(&mut self) {
        self.inject_lifecycle_reminder_for_continuation();
    }

    #[cfg(test)]
    pub(crate) fn lifecycle_deny_streak_for_tests(&self) -> u8 {
        self.lifecycle_deny_streak
    }

    #[cfg(test)]
    pub(crate) fn handle_lifecycle_hook_deny_with_cap_for_tests(
        &mut self,
        reason: String,
        cap: u8,
    ) -> LifecycleHookOutcome {
        self.handle_lifecycle_hook_deny(reason, cap)
    }

    #[cfg(test)]
    pub(crate) fn handle_lifecycle_hook_inject_with_cap_for_tests(
        &mut self,
        inject: crate::hooks::HookInjectContinuation,
        cap: u8,
    ) -> LifecycleHookOutcome {
        self.handle_lifecycle_hook_inject(inject, cap)
    }

    fn handle_lifecycle_hook_deny(&mut self, reason: String, cap: u8) -> LifecycleHookOutcome {
        self.set_pending_lifecycle_system_reminder(reason);
        if cap == 0 || self.lifecycle_deny_streak < cap {
            self.lifecycle_deny_streak = self.lifecycle_deny_streak.saturating_add(1);
            logging::info(&format!(
                "[m11-stage6] lifecycle deny #{} (cap={}), continuing turn",
                self.lifecycle_deny_streak,
                if cap == 0 {
                    "unlimited".to_string()
                } else {
                    cap.to_string()
                }
            ));
            LifecycleHookOutcome::ContinueImmediate
        } else {
            logging::warn(&format!(
                "[m11-stage6] lifecycle deny streak cap ({cap}) reached, falling back to next-prompt reminder"
            ));
            LifecycleHookOutcome::Stop
        }
    }

    fn handle_lifecycle_hook_inject(
        &mut self,
        inject: crate::hooks::HookInjectContinuation,
        cap: u8,
    ) -> LifecycleHookOutcome {
        if cap == 0 || self.lifecycle_deny_streak < cap {
            self.lifecycle_deny_streak = self.lifecycle_deny_streak.saturating_add(1);
            logging::info(&format!(
                "[m35] lifecycle inject #{} (cap={}), continuing turn",
                self.lifecycle_deny_streak,
                if cap == 0 {
                    "unlimited".to_string()
                } else {
                    cap.to_string()
                }
            ));
            LifecycleHookOutcome::ContinueImmediateWithInject(inject)
        } else {
            logging::warn(&format!(
                "[m35] lifecycle inject streak cap ({cap}) reached, stopping turn"
            ));
            LifecycleHookOutcome::Stop
        }
    }

    /// M11 stage 5: extract the most recent user-authored message text from
    /// the session transcript. Internal `<system-reminder>` injections are
    /// skipped so the hook sees what the real human typed last.
    /// Returned string is truncated to `LIFECYCLE_HOOK_LAST_USER_MESSAGE_MAX`
    /// chars (with a `…` suffix on overflow). Returns None for empty
    /// transcripts or when no user-authored message is found.
    pub(crate) fn lifecycle_hook_last_user_message(&self) -> Option<String> {
        for stored in self.session.messages.iter().rev() {
            if !matches!(stored.role, Role::User) {
                continue;
            }
            // Find first plain text block; skip tool results, images, etc.
            let text = stored.content.iter().find_map(|block| match block {
                ContentBlock::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })?;
            let trimmed = text.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Skip injected system reminders (these aren't real user input).
            if trimmed.starts_with("<system-reminder>") {
                continue;
            }
            return Some(truncate_with_ellipsis(
                trimmed,
                crate::hooks::LIFECYCLE_HOOK_LAST_USER_MESSAGE_MAX,
            ));
        }
        None
    }

    /// M11 stage 5: build the last-N tool-call previews from the session
    /// transcript (oldest of the kept window first, newest last) so hook
    /// scripts can match on the tail of agent activity. Each entry's
    /// `args_preview` is a one-line, truncated rendering of the tool input.
    pub(crate) fn lifecycle_hook_recent_tool_calls(
        &self,
    ) -> Vec<crate::hooks::LifecycleHookToolCallPreview> {
        let mut collected: Vec<crate::hooks::LifecycleHookToolCallPreview> = Vec::new();
        for stored in self.session.messages.iter().rev() {
            for block in stored.content.iter().rev() {
                if let ContentBlock::ToolUse { name, input, .. } = block {
                    collected.push(crate::hooks::LifecycleHookToolCallPreview {
                        name: name.clone(),
                        args_preview: build_tool_args_preview(input),
                    });
                    if collected.len() >= crate::hooks::LIFECYCLE_HOOK_RECENT_TOOL_CALLS_MAX {
                        break;
                    }
                }
            }
            if collected.len() >= crate::hooks::LIFECYCLE_HOOK_RECENT_TOOL_CALLS_MAX {
                break;
            }
        }
        collected.reverse();
        collected
    }

    /// M11 stage 5: session_age_seconds derived from Session::created_at.
    /// Clamped to 0 if the system clock moved backwards.
    pub(crate) fn lifecycle_hook_session_age_seconds(&self) -> u64 {
        let now = chrono::Utc::now();
        let elapsed = now.signed_duration_since(self.session.created_at);
        elapsed.num_seconds().max(0) as u64
    }

    /// M11 stage 5: count user-authored turns observed so far. Each contiguous
    /// run of user messages (ignoring system reminders) counts as one turn.
    pub(crate) fn lifecycle_hook_turn_count(&self) -> usize {
        let mut count = 0usize;
        let mut last_was_user = false;
        for stored in &self.session.messages {
            if matches!(stored.role, Role::User) {
                let is_reminder = stored.content.iter().any(|block| match block {
                    ContentBlock::Text { text, .. } => {
                        text.trim_start().starts_with("<system-reminder>")
                    }
                    _ => false,
                });
                let is_tool_result_only = stored
                    .content
                    .iter()
                    .all(|block| matches!(block, ContentBlock::ToolResult { .. }));
                if is_reminder || is_tool_result_only {
                    continue;
                }
                if !last_was_user {
                    count += 1;
                }
                last_was_user = true;
            } else {
                last_was_user = false;
            }
        }
        count
    }

    pub(super) fn merge_current_and_pending_system_reminders(
        current: Option<String>,
        pending: Option<String>,
    ) -> Option<String> {
        let current = current.and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        });
        let pending = pending.and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        });

        match (current, pending) {
            (Some(current), Some(pending)) => Some(format!("{current}\n\n{pending}")),
            (Some(current), None) => Some(current),
            (None, Some(pending)) => Some(pending),
            (None, None) => None,
        }
    }

    /// Run turns until no more tool calls
    /// Maximum number of context-limit compaction retries before giving up.
    pub(super) const MAX_CONTEXT_LIMIT_RETRIES: u32 = 5;
    pub(super) const MAX_INCOMPLETE_CONTINUATION_ATTEMPTS: u32 = 3;

    pub(super) async fn fire_response_completed_hook(
        &mut self,
        message_id: Option<&str>,
        stop_reason: Option<&str>,
        tool_calls_count: usize,
        output_chars: usize,
    ) -> LifecycleHookOutcome {
        let Some(message_id) = message_id else {
            return LifecycleHookOutcome::Stop;
        };
        // M11 stage 5: enrich payload with last user message, recent tool
        // calls, turn count, and session age so hook scripts can write
        // meaningful policies. All fields are optional / omitted-when-empty
        // so existing stage 1-4 hook scripts keep working unchanged.
        let last_user_message = self.lifecycle_hook_last_user_message();
        let recent_tool_calls = self.lifecycle_hook_recent_tool_calls();
        let turn_count = Some(self.lifecycle_hook_turn_count());
        let session_age_seconds = Some(self.lifecycle_hook_session_age_seconds());
        let payload = crate::hooks::ResponseCompletedHookPayload {
            event: crate::hooks::RESPONSE_COMPLETED,
            session_id: &self.session.id,
            message_id,
            working_dir: self.session.working_dir.clone(),
            stop_reason,
            tool_calls_count,
            output_chars,
            stop_hook_active: self.lifecycle_deny_streak > 0,
            last_user_message,
            recent_tool_calls,
            turn_count,
            session_age_seconds,
        };
        match crate::hooks::run_response_hooks(payload).await {
            Ok(Some(crate::hooks::LifecycleHookDecision::Deny(reason))) => {
                let cap = self.resolve_max_lifecycle_deny_streak_for_current_session();
                self.handle_lifecycle_hook_deny(reason, cap)
            }
            Ok(Some(crate::hooks::LifecycleHookDecision::Inject(inject))) => {
                let cap = self.resolve_max_lifecycle_deny_streak_for_current_session();
                self.handle_lifecycle_hook_inject(inject, cap)
            }
            Ok(None) => LifecycleHookOutcome::Stop,
            Err(err) => {
                logging::warn(&format!("response.completed hook failed: {err:#}"));
                LifecycleHookOutcome::Stop
            }
        }
    }

    pub(super) async fn run_turn(&mut self, print_output: bool) -> Result<String> {
        self.set_log_context();
        let mut final_text = String::new();
        let trace = trace_enabled();
        let mut context_limit_retries = 0u32;
        let mut incomplete_continuations = 0u32;
        let mut empty_after_tool_continuations = 0u32;

        loop {
            let repaired = self.repair_missing_tool_outputs();
            if repaired > 0 {
                logging::warn(&format!(
                    "Recovered {} missing tool output(s) before API call",
                    repaired
                ));
            }
            let (messages, compaction_event) = self.messages_for_provider();
            if let Some(event) = compaction_event {
                // Reset cache tracker and tool lock on compaction since the message history changes
                self.cache_tracker.reset();
                self.locked_tools = None;
                if print_output {
                    let tokens_str = event
                        .pre_tokens
                        .map(|t| format!(" ({} tokens)", t))
                        .unwrap_or_default();
                    println!("📦 Context compacted ({}){}", event.trigger, tokens_str);
                }
            }

            let tools = self.tool_definitions().await;
            let messages: std::sync::Arc<[Message]> = messages.into();
            // Non-blocking memory: uses pending result from last turn, spawns check for next turn
            let memory_pending =
                self.build_memory_prompt_nonblocking_shared(std::sync::Arc::clone(&messages), None);
            // Use split prompt for better caching - static content cached, dynamic not
            let split_prompt = self.build_system_prompt_split(None);
            self.log_prompt_prefix_accounting(&split_prompt, &tools);

            // Check for client-side cache violations before memory injection.
            // Memory is an ephemeral suffix that changes each turn; tracking it would cause
            // false-positive violations every turn (prior turn's memory ≠ current history prefix).
            self.record_client_cache_request(&messages);

            // Inject memory as a user message at the end (preserves cache prefix)
            let mut messages_with_memory: Vec<Message> = messages.iter().cloned().collect();
            if let Some(memory) = memory_pending.as_ref() {
                let memory_count = memory.count.max(1);
                let age_ms = memory.computed_at.elapsed().as_millis() as u64;
                crate::memory::record_injected_prompt(&memory.prompt, memory_count, age_ms);
                self.record_memory_injection_in_session(memory);
                logging::info(&format!(
                    "Memory injected as message ({} chars)",
                    memory.prompt.len()
                ));
                let memory_msg =
                    format!("<system-reminder>\n{}\n</system-reminder>", memory.prompt);
                messages_with_memory.push(Message::user(&memory_msg));
            }

            logging::info(&format!(
                "API call starting: {} messages, {} tools",
                messages_with_memory.len(),
                tools.len()
            ));
            let api_start = Instant::now();

            // Publish status for TUI to show during Task execution
            Bus::global().publish(BusEvent::SubagentStatus(SubagentStatus {
                session_id: self.session.id.clone(),
                status: "calling API".to_string(),
                model: Some(self.provider.model()),
            }));

            let stamped;
            let send_messages: &[Message] = if crate::config::config().features.message_timestamps {
                stamped = Message::with_timestamps(&messages_with_memory);
                &stamped
            } else {
                &messages_with_memory
            };
            self.last_status_detail = None;
            let mut stream = match self
                .provider
                .complete_split(
                    send_messages,
                    &tools,
                    &split_prompt.static_part,
                    &split_prompt.dynamic_part,
                    self.provider_session_id.as_deref(),
                )
                .await
            {
                Ok(stream) => stream,
                Err(e) => {
                    if self.try_auto_compact_after_context_limit(&e.to_string()) {
                        context_limit_retries += 1;
                        if context_limit_retries > Self::MAX_CONTEXT_LIMIT_RETRIES {
                            logging::warn(
                                "Context-limit compaction retry limit reached; giving up",
                            );
                            return Err(anyhow::anyhow!(
                                "Context limit exceeded after {} compaction retries",
                                Self::MAX_CONTEXT_LIMIT_RETRIES
                            ));
                        }
                        continue;
                    }
                    return Err(e);
                }
            };

            // Successful API call - reset retry counter
            context_limit_retries = 0;

            logging::info(&format!(
                "API stream opened in {:.2}s",
                api_start.elapsed().as_secs_f64()
            ));

            Bus::global().publish(BusEvent::SubagentStatus(SubagentStatus {
                session_id: self.session.id.clone(),
                status: "streaming".to_string(),
                model: Some(self.provider.model()),
            }));

            let mut text_content = String::new();
            let mut tool_calls: Vec<ToolCall> = Vec::new();
            let mut current_tool: Option<ToolCall> = None;
            let mut current_tool_input = String::new();
            let mut generated_image_contexts: Vec<Vec<ContentBlock>> = Vec::new();
            let mut usage_input: Option<u64> = None;
            let mut usage_output: Option<u64> = None;
            let mut usage_cache_read: Option<u64> = None;
            let mut usage_cache_creation: Option<u64> = None;
            let mut saw_message_end = false;
            let mut stop_reason: Option<String> = None;
            let mut _thinking_start: Option<Instant> = None;
            let store_reasoning_content = self.provider.name() == "openrouter";
            let mut reasoning_content = String::new();
            // Track tool results from provider (already executed by Claude Code CLI)
            let mut sdk_tool_results: std::collections::HashMap<String, (String, bool)> =
                std::collections::HashMap::new();
            let mut openai_native_compaction: Option<(String, usize)> = None;

            let mut retry_after_compaction = false;
            while let Some(event) = stream.next().await {
                let event = match event {
                    Ok(event) => event,
                    Err(e) => {
                        let err_str = e.to_string();
                        if self.try_auto_compact_after_context_limit(&err_str) {
                            context_limit_retries += 1;
                            if context_limit_retries > Self::MAX_CONTEXT_LIMIT_RETRIES {
                                logging::warn(
                                    "Context-limit compaction retry limit reached; giving up",
                                );
                                return Err(anyhow::anyhow!(
                                    "Context limit exceeded after {} compaction retries",
                                    Self::MAX_CONTEXT_LIMIT_RETRIES
                                ));
                            }
                            retry_after_compaction = true;
                            break;
                        }
                        return Err(e);
                    }
                };

                match event {
                    StreamEvent::ThinkingStart => {
                        // Track start but don't print - wait for ThinkingDone
                        _thinking_start = Some(Instant::now());
                    }
                    StreamEvent::ThinkingDelta(thinking_text) => {
                        // Display reasoning content only if enabled
                        if print_output && crate::config::config().display.show_thinking {
                            println!("💭 {}", thinking_text);
                        }
                        if store_reasoning_content {
                            reasoning_content.push_str(&thinking_text);
                        }
                    }
                    StreamEvent::ThinkingEnd => {
                        // Don't print here - ThinkingDone has accurate timing
                        _thinking_start = None;
                    }
                    StreamEvent::ThinkingDone { duration_secs } => {
                        // Bridge provides accurate wall-clock timing
                        if print_output {
                            println!("Thought for {:.1}s\n", duration_secs);
                        }
                    }
                    StreamEvent::TextDelta(text) => {
                        if print_output {
                            print!("{}", text);
                            io::stdout().flush()?;
                        }
                        text_content.push_str(&text);
                    }
                    StreamEvent::ToolUseStart { id, name } => {
                        if trace {
                            eprintln!("\n[trace] tool_use_start name={} id={}", name, id);
                        }
                        if print_output {
                            print!("\n[{}] ", name);
                            io::stdout().flush()?;
                        }
                        current_tool = Some(ToolCall {
                            id,
                            name,
                            input: serde_json::Value::Null,
                            intent: None,
                        });
                        current_tool_input.clear();
                    }
                    StreamEvent::ToolInputDelta(delta) => {
                        current_tool_input.push_str(&delta);
                    }
                    StreamEvent::ToolUseEnd => {
                        if let Some(mut tool) = current_tool.take() {
                            // Parse the accumulated JSON
                            let tool_input =
                                serde_json::from_str::<serde_json::Value>(&current_tool_input)
                                    .unwrap_or(serde_json::Value::Null);
                            tool.input = tool_input.clone();
                            tool.intent = ToolCall::intent_from_input(&tool_input);

                            if trace {
                                if current_tool_input.trim().is_empty() {
                                    eprintln!("[trace] tool_input {} (empty)", tool.name);
                                } else if tool_input == serde_json::Value::Null {
                                    eprintln!(
                                        "[trace] tool_input {} (raw) {}",
                                        tool.name, current_tool_input
                                    );
                                } else {
                                    let pretty = serde_json::to_string_pretty(&tool_input)
                                        .unwrap_or_else(|_| tool_input.to_string());
                                    eprintln!("[trace] tool_input {} {}", tool.name, pretty);
                                }
                            }

                            if print_output {
                                // Show brief tool info
                                print_tool_summary(&tool);
                            }

                            tool_calls.push(tool);
                            current_tool_input.clear();
                        }
                    }
                    StreamEvent::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => {
                        // SDK already executed this tool, store the result
                        if trace {
                            eprintln!(
                                "[trace] sdk_tool_result id={} is_error={} content_len={}",
                                tool_use_id,
                                is_error,
                                content.len()
                            );
                        }
                        sdk_tool_results.insert(tool_use_id, (content, is_error));
                    }
                    StreamEvent::GeneratedImage {
                        id,
                        path,
                        metadata_path,
                        output_format,
                        revised_prompt,
                    } => {
                        if trace {
                            eprintln!(
                                "[trace] generated_image id={} format={} path={} metadata={}",
                                id,
                                output_format,
                                path,
                                metadata_path.as_deref().unwrap_or("none")
                            );
                        }
                        if print_output {
                            let summary = crate::message::generated_image_summary(
                                &path,
                                metadata_path.as_deref(),
                                &output_format,
                                revised_prompt.as_deref(),
                            );
                            eprintln!(
                                "\n[{}] {}",
                                crate::message::GENERATED_IMAGE_TOOL_NAME,
                                summary
                            );
                        }
                        if self.provider.supports_image_input() {
                            if let Some(blocks) =
                                crate::message::generated_image_visual_context_blocks(
                                    &path,
                                    metadata_path.as_deref(),
                                    &output_format,
                                    revised_prompt.as_deref(),
                                )
                            {
                                generated_image_contexts.push(blocks);
                            } else {
                                crate::logging::warn(&format!(
                                    "Generated image was not attached as visual context: {}",
                                    path
                                ));
                            }
                        }
                    }
                    StreamEvent::TokenUsage {
                        input_tokens,
                        output_tokens,
                        cache_read_input_tokens,
                        cache_creation_input_tokens,
                    } => {
                        if let Some(input) = input_tokens {
                            usage_input = Some(input);
                        }
                        if let Some(output) = output_tokens {
                            usage_output = Some(output);
                        }
                        if cache_read_input_tokens.is_some() {
                            usage_cache_read = cache_read_input_tokens;
                        }
                        if cache_creation_input_tokens.is_some() {
                            usage_cache_creation = cache_creation_input_tokens;
                        }
                        if let Some(input) = usage_input {
                            self.update_compaction_usage_from_stream(
                                input,
                                usage_cache_read,
                                usage_cache_creation,
                            );
                        }
                        if trace {
                            eprintln!(
                                "[trace] token_usage input={} output={} cache_read={} cache_write={}",
                                usage_input.unwrap_or(0),
                                usage_output.unwrap_or(0),
                                usage_cache_read.unwrap_or(0),
                                usage_cache_creation.unwrap_or(0)
                            );
                        }
                    }
                    StreamEvent::ConnectionType { connection } => {
                        if trace {
                            eprintln!("[trace] connection_type={}", connection);
                        }
                        crate::telemetry::record_connection_type(&connection);
                        self.last_connection_type = Some(connection);
                    }
                    StreamEvent::ConnectionPhase { phase } => {
                        if trace {
                            eprintln!("[trace] connection_phase={}", phase);
                        }
                    }
                    StreamEvent::StatusDetail { detail } => {
                        if trace {
                            eprintln!("[trace] status_detail={}", detail);
                        }
                        self.last_status_detail = Some(detail);
                    }
                    StreamEvent::MessageEnd {
                        stop_reason: reason,
                    } => {
                        saw_message_end = true;
                        if reason.is_some() {
                            stop_reason = reason;
                        }
                        // Don't break yet - wait for SessionId which comes after MessageEnd
                        // (but stream close will also end the loop for providers without SessionId)
                    }
                    StreamEvent::SessionId(sid) => {
                        if trace {
                            eprintln!("[trace] session_id {}", sid);
                        }
                        self.provider_session_id = Some(sid.clone());
                        self.session.provider_session_id = Some(sid);
                        // We've received session_id, can exit the loop now
                        if saw_message_end {
                            break;
                        }
                    }
                    StreamEvent::UpstreamProvider { provider } => {
                        // Log upstream provider for local trace output
                        if trace {
                            eprintln!("[trace] upstream_provider={}", provider);
                        }
                        self.last_upstream_provider = Some(provider);
                    }
                    StreamEvent::Compaction {
                        trigger,
                        pre_tokens,
                        openai_encrypted_content,
                    } => {
                        if let Some(encrypted_content) = openai_encrypted_content {
                            openai_native_compaction
                                .get_or_insert((encrypted_content, self.session.messages.len()));
                        }
                        if print_output {
                            let tokens_str = pre_tokens
                                .map(|t| format!(" ({} tokens)", t))
                                .unwrap_or_default();
                            println!("📦 Context compacted ({}){}", trigger, tokens_str);
                        }
                    }
                    StreamEvent::NativeToolCall {
                        request_id,
                        tool_name,
                        input,
                    } => {
                        // Execute native tool and send result back to SDK bridge
                        if trace {
                            eprintln!(
                                "[trace] native_tool_call request_id={} tool={}",
                                request_id, tool_name
                            );
                        }
                        let ctx = ToolContext {
                            session_id: self.session.id.clone(),
                            message_id: self.session.id.clone(),
                            tool_call_id: request_id.clone(),
                            working_dir: self.working_dir().map(PathBuf::from),
                            stdin_request_tx: self.stdin_request_tx.clone(),
                            graceful_shutdown_signal: Some(self.graceful_shutdown.clone()),
                            execution_mode: ToolExecutionMode::AgentTurn,
                        };
                        crate::telemetry::record_tool_call();
                        let tool_result = self.registry.execute(&tool_name, input, ctx).await;
                        if tool_result.is_err() {
                            crate::telemetry::record_tool_failure();
                        }
                        let native_result = match tool_result {
                            Ok(output) => NativeToolResult::success(request_id, output.output),
                            Err(e) => NativeToolResult::error(request_id, e.to_string()),
                        };
                        // Send result back to SDK bridge
                        if let Some(sender) = self.provider.native_result_sender() {
                            let _ = sender.send(native_result).await;
                        }
                    }
                    StreamEvent::Error {
                        message,
                        retry_after_secs,
                    } => {
                        if trace {
                            eprintln!("[trace] stream_error {}", message);
                        }
                        if self.try_auto_compact_after_context_limit(&message) {
                            context_limit_retries += 1;
                            if context_limit_retries > Self::MAX_CONTEXT_LIMIT_RETRIES {
                                logging::warn(
                                    "Context-limit compaction retry limit reached; giving up",
                                );
                                return Err(anyhow::anyhow!(
                                    "Context limit exceeded after {} compaction retries",
                                    Self::MAX_CONTEXT_LIMIT_RETRIES
                                ));
                            }
                            retry_after_compaction = true;
                            break;
                        }
                        return Err(StreamError::new(message, retry_after_secs).into());
                    }
                }
            }

            if retry_after_compaction {
                continue;
            }

            let api_elapsed = api_start.elapsed();
            logging::info(&format!(
                "API call complete in {:.2}s (input={} output={} cache_read={} cache_write={})",
                api_elapsed.as_secs_f64(),
                usage_input.unwrap_or(0),
                usage_output.unwrap_or(0),
                usage_cache_read.unwrap_or(0),
                usage_cache_creation.unwrap_or(0),
            ));

            if usage_input.is_some()
                || usage_output.is_some()
                || usage_cache_read.is_some()
                || usage_cache_creation.is_some()
            {
                crate::telemetry::record_token_usage(
                    usage_input.unwrap_or(0),
                    usage_output.unwrap_or(0),
                    usage_cache_read,
                    usage_cache_creation,
                );
            }

            if print_output
                && (usage_input.is_some()
                    || usage_output.is_some()
                    || usage_cache_read.is_some()
                    || usage_cache_creation.is_some())
            {
                let input = usage_input.unwrap_or(0);
                let output = usage_output.unwrap_or(0);
                let cache_read = usage_cache_read.unwrap_or(0);
                let cache_creation = usage_cache_creation.unwrap_or(0);
                let cache_str = if usage_cache_read.is_some() || usage_cache_creation.is_some() {
                    format!(
                        " cache_read: {} cache_write: {}",
                        cache_read, cache_creation
                    )
                } else {
                    String::new()
                };
                print!(
                    "\n[Tokens] upload: {} download: {}{}\n",
                    input, output, cache_str
                );
                io::stdout().flush()?;
            }

            // Store usage for debug queries
            self.last_usage = TokenUsage {
                input_tokens: usage_input.unwrap_or(0),
                output_tokens: usage_output.unwrap_or(0),
                cache_read_input_tokens: usage_cache_read,
                cache_creation_input_tokens: usage_cache_creation,
            };

            self.recover_text_wrapped_tool_call(&mut text_content, &mut tool_calls);

            // Add assistant message to history
            let mut content_blocks = Vec::new();
            if !text_content.is_empty() {
                content_blocks.push(ContentBlock::Text {
                    text: text_content.clone(),
                    cache_control: None,
                });
            }
            if store_reasoning_content && !reasoning_content.is_empty() {
                content_blocks.push(ContentBlock::Reasoning {
                    text: reasoning_content.clone(),
                });
            }
            for tc in &tool_calls {
                content_blocks.push(ContentBlock::ToolUse {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    input: tc.input.clone(),
                });
            }

            let assistant_message_id = if !content_blocks.is_empty() {
                crate::telemetry::record_assistant_response();
                let token_usage = Some(crate::session::StoredTokenUsage {
                    input_tokens: self.last_usage.input_tokens,
                    output_tokens: self.last_usage.output_tokens,
                    cache_read_input_tokens: self.last_usage.cache_read_input_tokens,
                    cache_creation_input_tokens: self.last_usage.cache_creation_input_tokens,
                });
                let message_id =
                    self.add_message_ext(Role::Assistant, content_blocks, None, token_usage);
                self.push_embedding_snapshot_if_semantic(&text_content);
                self.session.save()?;
                Some(message_id)
            } else {
                None
            };

            if let Some((encrypted_content, compacted_count)) = openai_native_compaction.take() {
                self.apply_openai_native_compaction(encrypted_content, compacted_count)?;
            }

            // If stop_reason indicates truncation (e.g. max_tokens), discard tool calls
            // with null/empty inputs since they were likely truncated mid-generation.
            // This prevents executing broken tool calls and instead requests a continuation.
            self.filter_truncated_tool_calls(
                stop_reason.as_deref(),
                &mut tool_calls,
                assistant_message_id.as_ref(),
            );
            let assistant_tool_calls_count = tool_calls.len();

            if tool_calls.is_empty() && !generated_image_contexts.is_empty() {
                for blocks in generated_image_contexts.drain(..) {
                    self.add_message(Role::User, blocks);
                }
                self.session.save()?;
                logging::info(
                    "Continuing turn so model can inspect generated image visual context",
                );
                continue;
            }

            if tool_calls.is_empty()
                && assistant_message_id.is_none()
                && text_content.trim().is_empty()
                && self
                    .maybe_continue_empty_after_tool_result(&mut empty_after_tool_continuations)?
            {
                continue;
            }

            // If no tool calls, we're done
            if tool_calls.is_empty() {
                if self.maybe_continue_incomplete_response(
                    stop_reason.as_deref(),
                    &mut incomplete_continuations,
                )? {
                    continue;
                }
                logging::info("Turn complete - no tool calls, returning");
                if print_output {
                    println!();
                }
                match self
                    .fire_response_completed_hook(
                        assistant_message_id.as_deref(),
                        stop_reason.as_deref(),
                        assistant_tool_calls_count,
                        text_content.chars().count(),
                    )
                    .await
                {
                    LifecycleHookOutcome::Stop => {
                        final_text = text_content;
                        break;
                    }
                    LifecycleHookOutcome::ContinueImmediate => {
                        self.inject_lifecycle_reminder_for_continuation();
                        continue;
                    }
                    LifecycleHookOutcome::ContinueImmediateWithInject(inject) => {
                        self.inject_hook_body_for_continuation(inject);
                        continue;
                    }
                }
            }

            logging::info(&format!(
                "Turn has {} tool calls to execute",
                tool_calls.len()
            ));

            // If provider handles tools internally (like Claude Code CLI), only run native tools locally
            if self.provider.handles_tools_internally() {
                tool_calls.retain(|tc| JCODE_NATIVE_TOOLS.contains(&tc.name.as_str()));
                if tool_calls.is_empty() {
                    if !generated_image_contexts.is_empty() {
                        for blocks in generated_image_contexts.drain(..) {
                            self.add_message(Role::User, blocks);
                        }
                        self.session.save()?;
                        logging::info(
                            "Continuing turn so model can inspect generated image visual context",
                        );
                        continue;
                    }
                    logging::info("Provider handles tools internally - task complete");
                    match self
                        .fire_response_completed_hook(
                            assistant_message_id.as_deref(),
                            stop_reason.as_deref(),
                            assistant_tool_calls_count,
                            text_content.chars().count(),
                        )
                        .await
                    {
                        LifecycleHookOutcome::Stop => break,
                        LifecycleHookOutcome::ContinueImmediate => {
                            self.inject_lifecycle_reminder_for_continuation();
                            continue;
                        }
                        LifecycleHookOutcome::ContinueImmediateWithInject(inject) => {
                            self.inject_hook_body_for_continuation(inject);
                            continue;
                        }
                    }
                }
                logging::info("Provider handles tools internally - executing native tools locally");
            }

            // Execute tools and add results
            let mut tool_results_dirty = false;
            let classified = self.classify_tool_calls(&tool_calls, &sdk_tool_results)?;
            let mut preset_results: HashMap<usize, PresetToolResult> =
                classified.presets.into_iter().collect();
            let to_execute = classified.to_execute;
            let mut dispatched_results = HashMap::new();

            if !to_execute.is_empty() {
                let message_id = assistant_message_id
                    .clone()
                    .unwrap_or_else(|| self.session.id.clone());
                let cancel_token = tokio_util::sync::CancellationToken::new();
                let ctx_factory = |tc: &ToolCall| ToolContext {
                    session_id: self.session.id.clone(),
                    message_id: message_id.clone(),
                    tool_call_id: tc.id.clone(),
                    working_dir: self.working_dir().map(PathBuf::from),
                    stdin_request_tx: self.stdin_request_tx.clone(),
                    graceful_shutdown_signal: Some(self.graceful_shutdown.clone()),
                    execution_mode: ToolExecutionMode::AgentTurn,
                };
                let per_tool_start = |tc: &ToolCall| {
                    if print_output {
                        print!("\n  → ");
                        let _ = io::stdout().flush();
                    }
                    if trace {
                        eprintln!("[trace] tool_exec_start name={} id={}", tc.name, tc.id);
                    }
                    Bus::global().publish(BusEvent::ToolUpdated(ToolEvent {
                        session_id: self.session.id.clone(),
                        message_id: message_id.clone(),
                        tool_call_id: tc.id.clone(),
                        tool_name: tc.name.clone(),
                        status: ToolStatus::Running,
                        title: None,
                    }));
                    logging::info(&format!("Tool starting: {}", tc.name));
                    Bus::global().publish(BusEvent::SubagentStatus(SubagentStatus {
                        session_id: self.session.id.clone(),
                        status: format!("running {}", tc.name),
                        model: Some(self.provider.model()),
                    }));
                };

                let results = self
                    .dispatch_tools_parallel(to_execute, ctx_factory, per_tool_start, &cancel_token)
                    .await;
                dispatched_results = results
                    .into_iter()
                    .map(|result| (result.index, result))
                    .collect();
            }

            for (tool_index, tc) in tool_calls.iter().enumerate() {
                let message_id = assistant_message_id
                    .clone()
                    .unwrap_or_else(|| self.session.id.clone());

                if let Some(preset) = preset_results.remove(&tool_index) {
                    match preset {
                        PresetToolResult::ValidationError(error_msg) => {
                            logging::warn(&error_msg);
                            Bus::global().publish(BusEvent::ToolUpdated(ToolEvent {
                                session_id: self.session.id.clone(),
                                message_id: message_id.clone(),
                                tool_call_id: tc.id.clone(),
                                tool_name: tc.name.clone(),
                                status: ToolStatus::Error,
                                title: None,
                            }));
                            if print_output {
                                println!("\n  → {}", error_msg);
                            }
                            self.add_message(
                                Role::User,
                                vec![ContentBlock::ToolResult {
                                    tool_use_id: tc.id.clone(),
                                    content: error_msg,
                                    is_error: Some(true),
                                }],
                            );
                            tool_results_dirty = true;
                        }
                        PresetToolResult::SdkProvided { content, is_error } => {
                            if trace {
                                eprintln!(
                                    "[trace] using_sdk_result name={} id={} is_error={}",
                                    tc.name, tc.id, is_error
                                );
                            }
                            if print_output {
                                print!("\n  → ");
                                let preview = if content.len() > 200 {
                                    format!("{}...", crate::util::truncate_str(&content, 200))
                                } else {
                                    content.clone()
                                };
                                println!("{}", preview.lines().next().unwrap_or("(done via SDK)"));
                            }
                            Bus::global().publish(BusEvent::ToolUpdated(ToolEvent {
                                session_id: self.session.id.clone(),
                                message_id: message_id.clone(),
                                tool_call_id: tc.id.clone(),
                                tool_name: tc.name.clone(),
                                status: if is_error {
                                    ToolStatus::Error
                                } else {
                                    ToolStatus::Completed
                                },
                                title: None,
                            }));
                            self.add_message(
                                Role::User,
                                vec![ContentBlock::ToolResult {
                                    tool_use_id: tc.id.clone(),
                                    content,
                                    is_error: if is_error { Some(true) } else { None },
                                }],
                            );
                            tool_results_dirty = true;
                        }
                    }
                    continue;
                }

                let Some(result) = dispatched_results.remove(&tool_index) else {
                    continue;
                };
                crate::telemetry::record_tool_call();
                self.unlock_tools_if_needed(&result.tc.name);
                logging::info(&format!(
                    "Tool finished: {} in {:.2}s",
                    result.tc.name,
                    result.elapsed.as_secs_f64()
                ));

                match result.result {
                    Ok(output) => {
                        Bus::global().publish(BusEvent::ToolUpdated(ToolEvent {
                            session_id: self.session.id.clone(),
                            message_id: message_id.clone(),
                            tool_call_id: result.tc.id.clone(),
                            tool_name: result.tc.name.clone(),
                            status: ToolStatus::Completed,
                            title: output.title.clone(),
                        }));

                        if trace {
                            eprintln!(
                                "[trace] tool_exec_done name={} id={}\n{}",
                                result.tc.name, result.tc.id, output.output
                            );
                        }
                        if print_output {
                            let preview = if output.output.len() > 200 {
                                format!("{}...", crate::util::truncate_str(&output.output, 200))
                            } else {
                                output.output.clone()
                            };
                            println!("{}", preview.lines().next().unwrap_or("(done)"));
                        }

                        let blocks = tool_output_to_content_blocks(result.tc.id.clone(), output);
                        self.add_message_with_duration(
                            Role::User,
                            blocks,
                            Some(result.elapsed.as_millis() as u64),
                        );
                        tool_results_dirty = true;
                    }
                    Err(e) => {
                        crate::telemetry::record_tool_failure();
                        Bus::global().publish(BusEvent::ToolUpdated(ToolEvent {
                            session_id: self.session.id.clone(),
                            message_id: message_id.clone(),
                            tool_call_id: result.tc.id.clone(),
                            tool_name: result.tc.name.clone(),
                            status: ToolStatus::Error,
                            title: None,
                        }));

                        let error_msg = format!("Error: {}", e);
                        if trace {
                            eprintln!(
                                "[trace] tool_exec_error name={} id={} {}",
                                result.tc.name, result.tc.id, error_msg
                            );
                        }
                        if print_output {
                            println!("{}", error_msg);
                        }
                        self.add_message_with_duration(
                            Role::User,
                            vec![ContentBlock::ToolResult {
                                tool_use_id: result.tc.id.clone(),
                                content: error_msg,
                                is_error: Some(true),
                            }],
                            Some(result.elapsed.as_millis() as u64),
                        );
                        tool_results_dirty = true;
                    }
                }
            }

            if tool_results_dirty {
                self.session.save()?;
                if self.inject_nested_instructions_for_tool_calls(&tool_calls) {
                    self.session.save()?;
                }
            }

            if !generated_image_contexts.is_empty() {
                for blocks in generated_image_contexts.drain(..) {
                    self.add_message(Role::User, blocks);
                }
                self.session.save()?;
            }

            if print_output {
                println!();
            }

            // Check for soft interrupts (e.g. Telegram messages) and inject them for the next turn
            let injected = self.inject_soft_interrupts();
            if !injected.is_empty() {
                let total_chars: usize = injected.iter().map(|item| item.content.len()).sum();
                logging::info(&format!(
                    "Soft interrupt injected into headless turn ({} message(s), {} chars)",
                    injected.len(),
                    total_chars
                ));
            }
        }

        Ok(final_text)
    }
}

/// M11 stage 5: truncate a string to `max_chars` Unicode chars and append
/// `…` if truncation occurred. Operates on char boundaries (NOT bytes) so
/// non-ASCII text such as Korean is never split mid-codepoint.
fn truncate_with_ellipsis(input: &str, max_chars: usize) -> String {
    let trimmed = input.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(max_chars).collect();
    out.push('…');
    out
}

/// M11 stage 5: render a tool-input JSON value as a compact, single-line
/// preview suitable for `args_preview`. Newlines collapse to spaces and the
/// string is truncated to `LIFECYCLE_HOOK_TOOL_ARGS_PREVIEW_MAX` chars.
fn build_tool_args_preview(input: &serde_json::Value) -> String {
    let raw = if input.is_null() {
        String::new()
    } else {
        serde_json::to_string(input).unwrap_or_default()
    };
    // Collapse all whitespace runs (including newlines) into a single space
    // so the preview reads as one line.
    let collapsed: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_with_ellipsis(
        &collapsed,
        crate::hooks::LIFECYCLE_HOOK_TOOL_ARGS_PREVIEW_MAX,
    )
}
