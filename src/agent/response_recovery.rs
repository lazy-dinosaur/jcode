use super::*;

impl Agent {
    fn parse_text_wrapped_tool_call(
        text: &str,
    ) -> Option<(String, String, serde_json::Value, String)> {
        if let Some(parsed) = Self::parse_xml_wrapped_tool_call(text) {
            return Some(parsed);
        }

        let marker = "to=functions.";
        let marker_idx = text.find(marker)?;
        let after_marker = &text[marker_idx + marker.len()..];

        let mut tool_name_end = 0usize;
        for (idx, ch) in after_marker.char_indices() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                tool_name_end = idx + ch.len_utf8();
            } else {
                break;
            }
        }
        if tool_name_end == 0 {
            return None;
        }

        let tool_name = after_marker[..tool_name_end].to_string();
        let remaining = &after_marker[tool_name_end..];
        let mut fallback: Option<(String, String, serde_json::Value, String)> = None;

        for (brace_idx, ch) in remaining.char_indices() {
            if ch != '{' {
                continue;
            }
            let slice = &remaining[brace_idx..];
            let mut stream =
                serde_json::Deserializer::from_str(slice).into_iter::<serde_json::Value>();
            let parsed = match stream.next() {
                Some(Ok(value)) => value,
                Some(Err(_)) | None => continue,
            };
            let consumed = stream.byte_offset();
            if !parsed.is_object() {
                continue;
            }

            let prefix = text[..marker_idx].trim_end().to_string();
            let suffix = remaining[brace_idx + consumed..].trim().to_string();
            if suffix.is_empty() {
                return Some((prefix, tool_name.clone(), parsed, suffix));
            }
            if fallback.is_none() {
                fallback = Some((prefix, tool_name.clone(), parsed, suffix));
            }
        }

        fallback
    }

    fn parse_xml_wrapped_tool_call(
        text: &str,
    ) -> Option<(String, String, serde_json::Value, String)> {
        let invoke_start = text.find("<invoke")?;
        let tag_end_rel = text[invoke_start..].find('>')?;
        let open_tag_end = invoke_start + tag_end_rel + 1;
        let open_tag = &text[invoke_start..open_tag_end];
        let tool_name = Self::parse_xml_attr(open_tag, "name")?;
        let tool_name = tool_name
            .strip_prefix("functions.")
            .or_else(|| tool_name.strip_prefix("tools."))
            .unwrap_or(&tool_name)
            .to_string();
        if tool_name.trim().is_empty() {
            return None;
        }

        let close_tag = "</invoke>";
        let close_rel = text[open_tag_end..].find(close_tag);
        let close_start = close_rel
            .map(|rel| open_tag_end + rel)
            .unwrap_or_else(|| text.len());
        let inner = &text[open_tag_end..close_start];
        let after_close = close_rel
            .map(|_| close_start + close_tag.len())
            .unwrap_or(close_start);

        let arguments = Self::parse_xml_invoke_arguments(inner)?;
        let prefix = text[..invoke_start].trim_end().to_string();
        let suffix = text[after_close..].trim().to_string();
        Some((prefix, tool_name, arguments, suffix))
    }

    fn parse_xml_attr(tag: &str, attr: &str) -> Option<String> {
        let mut search_start = 0usize;
        while let Some(rel_idx) = tag[search_start..].find(attr) {
            let idx = search_start + rel_idx;
            let before_ok = idx == 0
                || tag[..idx]
                    .chars()
                    .next_back()
                    .is_some_and(|ch| ch.is_whitespace() || ch == '<');
            let after_attr = &tag[idx + attr.len()..];
            let after_eq = after_attr.trim_start();
            if !before_ok || !after_eq.starts_with('=') {
                search_start = idx + attr.len();
                continue;
            }
            let value = after_eq[1..].trim_start();
            let quote = value.chars().next()?;
            let quote = match quote {
                '"' | '\'' => quote,
                '“' | '”' => '”',
                '‘' | '’' => '’',
                _ => return None,
            };
            let value_body = &value[value.chars().next()?.len_utf8()..];
            let end = value_body.find(quote)?;
            return Some(Self::xml_unescape(&value_body[..end]));
        }

        None
    }

    fn parse_xml_invoke_arguments(inner: &str) -> Option<serde_json::Value> {
        let trimmed = inner.trim();
        if trimmed.starts_with('{') {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if value.is_object() {
                    return Some(value);
                }
            }
        }

        let mut map = serde_json::Map::new();
        let mut cursor = 0usize;
        while let Some(param_rel) = inner[cursor..].find("<parameter") {
            let param_start = cursor + param_rel;
            let tag_end_rel = inner[param_start..].find('>')?;
            let open_tag_end = param_start + tag_end_rel + 1;
            let open_tag = &inner[param_start..open_tag_end];
            let name = Self::parse_xml_attr(open_tag, "name")?;
            let close_tag = "</parameter>";
            let close_rel = inner[open_tag_end..].find(close_tag)?;
            let close_start = open_tag_end + close_rel;
            let raw_value = inner[open_tag_end..close_start].trim();
            map.insert(name, Self::xml_parameter_value(raw_value));
            cursor = close_start + close_tag.len();
        }

        if map.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(map))
        }
    }

    fn xml_parameter_value(raw: &str) -> serde_json::Value {
        let unescaped = Self::xml_unescape(raw);
        let trimmed = unescaped.trim();
        if trimmed.is_empty() {
            return serde_json::Value::String(String::new());
        }
        serde_json::from_str::<serde_json::Value>(trimmed)
            .unwrap_or_else(|_| serde_json::Value::String(unescaped))
    }

    fn xml_unescape(raw: &str) -> String {
        raw.replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&")
    }

    pub(super) fn recover_text_wrapped_tool_call(
        &self,
        text_content: &mut String,
        tool_calls: &mut Vec<ToolCall>,
    ) -> bool {
        if !tool_calls.is_empty() || text_content.trim().is_empty() {
            return false;
        }

        let Some((prefix, tool_name, arguments, suffix)) =
            Self::parse_text_wrapped_tool_call(text_content)
        else {
            return false;
        };

        let mut sanitized = String::new();
        if !prefix.is_empty() {
            sanitized.push_str(&prefix);
        }
        if !suffix.is_empty() {
            if !sanitized.is_empty() {
                sanitized.push('\n');
            }
            sanitized.push_str(&suffix);
        }
        *text_content = sanitized;

        let call_id = format!("fallback_text_call_{}", id::new_id("call"));
        let recovered_total = RECOVERED_TEXT_WRAPPED_TOOL_CALLS
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        logging::warn(&format!(
            "[agent] Recovered text-wrapped tool call for '{}' ({}, total={})",
            tool_name, call_id, recovered_total
        ));
        let intent = ToolCall::intent_from_input(&arguments);
        tool_calls.push(ToolCall {
            id: call_id,
            name: tool_name,
            input: arguments,
            intent,
        });

        true
    }

    pub(super) fn should_continue_after_stop_reason(stop_reason: &str) -> bool {
        let reason = stop_reason.trim().to_ascii_lowercase();
        if reason.is_empty() {
            return false;
        }

        if matches!(reason.as_str(), "stop" | "end_turn" | "tool_use") {
            return false;
        }

        reason.contains("incomplete")
            || reason.contains("max_output_tokens")
            || reason.contains("max_tokens")
            || reason.contains("length")
            || reason.contains("trunc")
            || reason.contains("commentary")
    }
    fn continuation_prompt_for_stop_reason(stop_reason: &str) -> String {
        format!(
            "[System reminder: your previous response ended before completion (stop_reason: {}). Continue exactly where you left off, do not repeat completed content, and if the next step is a tool call, emit the tool call now.]",
            stop_reason.trim()
        )
    }

    pub(crate) fn maybe_continue_incomplete_response(
        &mut self,
        stop_reason: Option<&str>,
        attempts: &mut u32,
    ) -> Result<bool> {
        let Some(stop_reason) = stop_reason
            .map(str::trim)
            .filter(|reason| !reason.is_empty())
        else {
            return Ok(false);
        };

        if !Self::should_continue_after_stop_reason(stop_reason) {
            return Ok(false);
        }

        if *attempts >= Self::MAX_INCOMPLETE_CONTINUATION_ATTEMPTS {
            logging::warn(&format!(
                "Response ended with stop_reason='{}' after {} continuation attempts; returning partial output",
                stop_reason, attempts
            ));
            return Ok(false);
        }

        *attempts += 1;
        logging::warn(&format!(
            "Response ended with stop_reason='{}'; requesting continuation (attempt {}/{})",
            stop_reason,
            attempts,
            Self::MAX_INCOMPLETE_CONTINUATION_ATTEMPTS
        ));

        self.add_message(
            Role::User,
            vec![ContentBlock::Text {
                text: Self::continuation_prompt_for_stop_reason(stop_reason),
                cache_control: None,
            }],
        );
        self.session.save()?;
        Ok(true)
    }

    pub(crate) fn maybe_continue_empty_after_tool_result(
        &mut self,
        attempts: &mut u32,
    ) -> Result<bool> {
        const MAX_EMPTY_AFTER_TOOL_CONTINUATIONS: u32 = 1;

        let last_is_tool_result = self.session.messages.last().is_some_and(|message| {
            message.role == Role::User
                && message
                    .content
                    .iter()
                    .any(|block| matches!(block, ContentBlock::ToolResult { .. }))
        });

        if !last_is_tool_result {
            return Ok(false);
        }

        if *attempts >= MAX_EMPTY_AFTER_TOOL_CONTINUATIONS {
            logging::warn(
                "Provider returned an empty assistant response after a tool result; retry limit reached",
            );
            return Ok(false);
        }

        *attempts += 1;
        logging::warn(
            "Provider returned an empty assistant response after a tool result; requesting continuation",
        );

        self.add_message(
            Role::User,
            vec![ContentBlock::Text {
                text: "[System reminder: your previous response after the tool result was empty. Read the tool result and continue with a concise useful response now.]".to_string(),
                cache_control: None,
            }],
        );
        self.session.save()?;
        Ok(true)
    }

    pub(super) fn filter_truncated_tool_calls(
        &mut self,
        stop_reason: Option<&str>,
        tool_calls: &mut Vec<ToolCall>,
        assistant_message_id: Option<&String>,
    ) {
        let stop_reason = stop_reason.unwrap_or("");
        if !Self::should_continue_after_stop_reason(stop_reason) {
            return;
        }

        let before = tool_calls.len();
        tool_calls.retain(|tc| !tc.input.is_null());
        let discarded = before - tool_calls.len();
        if discarded > 0 && tool_calls.is_empty() {
            logging::warn(&format!(
                "Discarded {} tool call(s) with null input (truncated by {}); requesting continuation",
                discarded,
                if stop_reason.is_empty() {
                    "unknown"
                } else {
                    stop_reason
                }
            ));
            if let Some(msg_id) = assistant_message_id {
                self.session.remove_tool_use_blocks(msg_id);
                self.persist_session_best_effort("truncated tool-call repair");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_xml_wrapped_bash_invoke_with_parameters() {
        let text = r#"count <invoke name="bash"> <parameter name="command">cd /tmp; echo “hello”; grep -n “foo|bar” file.txt</parameter> <parameter name="timeout">15000</parameter> </invoke> trailing"#;
        let (prefix, tool_name, arguments, suffix) =
            Agent::parse_text_wrapped_tool_call(text).expect("should recover invoke tool call");

        assert_eq!(prefix, "count");
        assert_eq!(tool_name, "bash");
        assert_eq!(
            arguments["command"],
            "cd /tmp; echo “hello”; grep -n “foo|bar” file.txt"
        );
        assert_eq!(arguments["timeout"], 15000);
        assert_eq!(suffix, "trailing");
    }

    #[test]
    fn parse_xml_wrapped_mcp_invoke_with_namespace_name() {
        let text = r#"<invoke name="mcp__electron-test__connect"><parameter name="port">9222</parameter></invoke>"#;
        let (prefix, tool_name, arguments, suffix) =
            Agent::parse_text_wrapped_tool_call(text).expect("should recover mcp invoke tool call");

        assert!(prefix.is_empty());
        assert_eq!(tool_name, "mcp__electron-test__connect");
        assert_eq!(arguments["port"], 9222);
        assert!(suffix.is_empty());
    }

    #[test]
    fn parse_xml_wrapped_read_invoke_with_parameters() {
        let text = r#"count <invoke name="read"> <parameter name="file_path">src/renderer/src/screens/Calendar/Header.tsx</parameter> <parameter name="limit">20</parameter> <parameter name="offset">42</parameter> </invoke>"#;
        let (prefix, tool_name, arguments, suffix) =
            Agent::parse_text_wrapped_tool_call(text).expect("should recover read invoke");

        assert_eq!(prefix, "count");
        assert_eq!(tool_name, "read");
        assert_eq!(
            arguments["file_path"],
            "src/renderer/src/screens/Calendar/Header.tsx"
        );
        assert_eq!(arguments["limit"], 20);
        assert_eq!(arguments["offset"], 42);
        assert!(suffix.is_empty());
    }

    #[test]
    fn parse_xml_wrapped_invoke_tolerates_missing_close_tag() {
        let text = r#"<invoke name="bash"><parameter name="command">echo ok</parameter> </in"#;
        let (prefix, tool_name, arguments, suffix) =
            Agent::parse_text_wrapped_tool_call(text).expect("should recover truncated invoke");

        assert!(prefix.is_empty());
        assert_eq!(tool_name, "bash");
        assert_eq!(arguments["command"], "echo ok");
        assert!(suffix.is_empty());
    }

    #[test]
    fn parse_xml_wrapped_invoke_accepts_json_body() {
        let text = r#"<invoke name="functions.bash">{"command":"pwd","timeout":5000}</invoke>"#;
        let (_prefix, tool_name, arguments, _suffix) =
            Agent::parse_text_wrapped_tool_call(text).expect("should recover json invoke");

        assert_eq!(tool_name, "bash");
        assert_eq!(arguments["command"], "pwd");
        assert_eq!(arguments["timeout"], 5000);
    }
}
