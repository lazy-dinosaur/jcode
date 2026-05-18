use jcode_message_types::{ContentBlock, Message, Role};
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

/// Default token budget (200k tokens - matches Claude's actual context limit)
pub const DEFAULT_TOKEN_BUDGET: usize = 200_000;

/// Trigger compaction at this percentage of budget
pub const COMPACTION_THRESHOLD: f32 = 0.80;

/// If context is above this threshold when compaction starts, do a synchronous
/// hard-compact (drop old messages) so the API call doesn't fail.
pub const CRITICAL_THRESHOLD: f32 = 0.95;

/// Minimum threshold for manual compaction (can compact at any time above this)
pub const MANUAL_COMPACT_MIN_THRESHOLD: f32 = 0.10;

/// Keep this many recent turns verbatim (not summarized)
pub const RECENT_TURNS_TO_KEEP: usize = 10;

/// Absolute minimum turns to keep during emergency compaction
pub const MIN_TURNS_TO_KEEP: usize = 2;

/// Max chars for a single tool result during emergency truncation
pub const EMERGENCY_TOOL_RESULT_MAX_CHARS: usize = 4000;

/// Approximate chars per token for estimation
pub const CHARS_PER_TOKEN: usize = 4;

/// Fixed token overhead for system prompt + tool definitions.
/// These are not counted in message content but do count toward the context limit.
/// Estimated conservatively: ~8k tokens for system prompt + ~10k for 50+ tools.
pub const SYSTEM_OVERHEAD_TOKENS: usize = 18_000;

/// Rolling window size for token history (proactive/semantic modes)
pub const TOKEN_HISTORY_WINDOW: usize = 20;

/// Maximum characters to embed per message (first N chars capture semantic content)
pub const EMBED_MAX_CHARS_PER_MSG: usize = 512;

/// Rolling window of per-turn embeddings used for topic-shift detection
pub const EMBEDDING_HISTORY_WINDOW: usize = 10;

/// Per-manager semantic embedding cache capacity.
pub const SEMANTIC_EMBED_CACHE_CAPACITY: usize = 256;

/// M14/M14a safety: maximum consecutive compaction failures (background task
/// errors, panics, hard-compact rejects) before triggers short-circuit and
/// stop calling the summarizer / hard-compact path. Reset to 0 on any
/// successful compaction.
///
/// The user observed two related runaway loops:
///   * proactive compaction firing on every new turn after the summarizer
///     errored out (no cooldown applied on failure)
///   * 22 consecutive emergency hard-compactions inside a single turn loop,
///     because per-turn `MAX_CONTEXT_LIMIT_RETRIES` does not see the
///     session-wide repetition.
///
/// 3 attempts is enough to recover from a transient failure but small enough
/// to stop billing the user for an unrecoverable summarizer state.
pub const MAX_CONSECUTIVE_COMPACTION_FAILURES: usize = 3;

pub const SUMMARY_PROMPT: &str = r#"Summarize our conversation so you can continue this work later.

Write in natural language with these sections:
- **Context:** What we're working on and why (1-2 sentences)
- **What we did:** Key actions taken, files changed, problems solved
- **Current state:** What works, what's broken, what's next
- **User preferences:** Specific requirements or decisions they made

Be concise but preserve important details. You can search the full conversation later if you need exact error messages or code snippets."#;

/// A completed summary covering turns up to a certain point
#[derive(Debug, Clone)]
pub struct Summary {
    pub text: String,
    pub openai_encrypted_content: Option<String>,
    pub covers_up_to_turn: usize,
    pub original_turn_count: usize,
}

/// Event emitted when compaction is applied
#[derive(Debug, Clone)]
pub struct CompactionEvent {
    pub trigger: String,
    pub pre_tokens: Option<u64>,
    pub post_tokens: Option<u64>,
    pub tokens_saved: Option<u64>,
    pub duration_ms: Option<u64>,
    pub messages_dropped: Option<usize>,
    pub messages_compacted: Option<usize>,
    pub summary_chars: Option<usize>,
    pub active_messages: Option<usize>,
}

/// What happened when ensure_context_fits was called
#[derive(Debug, Clone, PartialEq)]
pub enum CompactionAction {
    /// Nothing needed, context is fine.
    None,
    /// Background summarization started.
    BackgroundStarted { trigger: String },
    /// Emergency hard compact performed. Contains number of messages dropped.
    HardCompacted(usize),
}

/// Stats about compaction state
#[derive(Debug, Clone)]
pub struct CompactionStats {
    pub total_turns: usize,
    pub active_messages: usize,
    pub has_summary: bool,
    pub is_compacting: bool,
    pub token_estimate: usize,
    pub effective_tokens: usize,
    pub observed_input_tokens: Option<u64>,
    pub context_usage: f32,
}

pub fn compacted_summary_text_block(summary: &str) -> String {
    format!("## Previous Conversation Summary\n\n{}\n\n---\n\n", summary)
}

pub fn build_compaction_prompt(
    messages: &[Message],
    existing_summary: Option<&Summary>,
    max_prompt_chars: usize,
) -> String {
    let mut conversation_text = build_compaction_conversation_text(messages, existing_summary);
    let overhead = SUMMARY_PROMPT.len() + 50;
    if conversation_text.len() + overhead > max_prompt_chars && max_prompt_chars > overhead {
        let budget = max_prompt_chars - overhead;
        conversation_text = truncate_str_boundary(&conversation_text, budget).to_string();
        conversation_text
            .push_str("\n\n... [earlier conversation truncated to fit context window]\n");
    }
    format!("{}\n\n---\n\n{}", conversation_text, SUMMARY_PROMPT)
}

pub fn build_compaction_conversation_text(
    messages: &[Message],
    existing_summary: Option<&Summary>,
) -> String {
    let mut conversation_text = String::new();
    if let Some(summary) = existing_summary {
        conversation_text.push_str("## Previous Summary\n\n");
        conversation_text.push_str(&summary.text);
        conversation_text.push_str("\n\n## New Conversation\n\n");
    }

    for msg in messages {
        let role_str = match msg.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
        };
        conversation_text.push_str(&format!("**{}:**\n", role_str));
        for block in &msg.content {
            match block {
                ContentBlock::Text { text, .. } => {
                    conversation_text.push_str(text);
                    conversation_text.push('\n');
                }
                ContentBlock::ToolUse { name, input, .. } => {
                    conversation_text.push_str(&format!("[Tool: {} - {}]\n", name, input));
                }
                ContentBlock::ToolResult { content, .. } => {
                    let truncated = if content.len() > 500 {
                        format!("{}... (truncated)", truncate_str_boundary(content, 500))
                    } else {
                        content.clone()
                    };
                    conversation_text.push_str(&format!("[Result: {}]\n", truncated));
                }
                ContentBlock::Reasoning { .. } => {}
                ContentBlock::Image { .. } => conversation_text.push_str("[Image]\n"),
                ContentBlock::OpenAICompaction { .. } => {
                    conversation_text.push_str("[OpenAI native compaction]\n")
                }
            }
        }
        conversation_text.push('\n');
    }
    conversation_text
}

pub fn truncate_str_boundary(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

pub fn mean_embedding(embeddings: &[&Vec<f32>], dim: usize) -> Vec<f32> {
    let mut mean = vec![0f32; dim];
    for emb in embeddings {
        for (i, v) in emb.iter().enumerate() {
            if i < dim {
                mean[i] += v;
            }
        }
    }
    let n = embeddings.len().max(1) as f32;
    for v in &mut mean {
        *v /= n;
    }
    let norm: f32 = mean.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in &mut mean {
            *v /= norm;
        }
    }
    mean
}

/// Find a safe compaction cutoff that does not leave kept tool results without
/// their corresponding tool calls.
pub fn safe_compaction_cutoff(messages: &[Message], initial_cutoff: usize) -> usize {
    let mut cutoff = initial_cutoff.min(messages.len());

    // Track tool call/result ids in the kept portion.
    let mut available_tool_ids = HashSet::new();
    let mut missing_tool_ids = HashSet::new();

    for msg in &messages[cutoff..] {
        for block in &msg.content {
            match block {
                ContentBlock::ToolUse { id, .. } => {
                    available_tool_ids.insert(id.clone());
                    missing_tool_ids.remove(id);
                }
                ContentBlock::ToolResult { tool_use_id, .. }
                    if !available_tool_ids.contains(tool_use_id) =>
                {
                    missing_tool_ids.insert(tool_use_id.clone());
                }
                _ => {}
            }
        }
    }

    if missing_tool_ids.is_empty() {
        return cutoff;
    }

    // Walk backward once, progressively growing the kept suffix until every
    // kept tool result has its matching tool use in the same suffix.
    for (idx, msg) in messages[..cutoff].iter().enumerate().rev() {
        for block in &msg.content {
            match block {
                ContentBlock::ToolUse { id, .. } => {
                    available_tool_ids.insert(id.clone());
                    missing_tool_ids.remove(id);
                }
                ContentBlock::ToolResult { tool_use_id, .. }
                    if !available_tool_ids.contains(tool_use_id) =>
                {
                    missing_tool_ids.insert(tool_use_id.clone());
                }
                _ => {}
            }
        }
        if missing_tool_ids.is_empty() {
            cutoff = idx;
            return cutoff;
        }
    }

    // If we couldn't find every matching tool call, don't compact at all.
    0
}

pub fn message_char_count(msg: &Message) -> usize {
    content_char_count(&msg.content)
}

pub fn content_char_count(content: &[ContentBlock]) -> usize {
    content
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text, .. } => text.len(),
            ContentBlock::Reasoning { text } => text.len(),
            ContentBlock::ToolUse { input, .. } => input.to_string().len() + 50,
            ContentBlock::ToolResult { content, .. } => content.len() + 20,
            ContentBlock::Image { data, .. } => data.len(),
            ContentBlock::OpenAICompaction { encrypted_content } => encrypted_content.len(),
        })
        .sum()
}

pub fn summary_payload_char_count(summary: &Summary) -> usize {
    // OpenAI native compaction's `encrypted_content` is a provider replay blob,
    // not prompt-visible text. Counting its byte length as prompt tokens makes a
    // successful native compaction look like it is still far above the context
    // window (for example an ~8 MiB encrypted blob becomes ~2M estimated
    // tokens), which can trigger an endless emergency-compaction loop.
    //
    // Use the visible summary text for token estimation. Payload sendability is
    // guarded separately by `openai_encrypted_content_is_sendable` before replay.
    summary.text.len()
}

pub fn estimate_compaction_tokens(
    summary: Option<&Summary>,
    active_message_chars: usize,
    token_budget: usize,
) -> usize {
    let summary_chars = summary.map(summary_payload_char_count).unwrap_or(0);
    estimate_compaction_tokens_from_chars(summary_chars + active_message_chars, token_budget)
}

pub fn estimate_compaction_tokens_from_chars(total_chars: usize, token_budget: usize) -> usize {
    let msg_tokens = total_chars / CHARS_PER_TOKEN;
    // Add overhead for system prompt + tool definitions, which are not in the
    // message list but do count toward the context limit. Scale the overhead to
    // the budget so tests with tiny budgets aren't affected.
    let overhead = if token_budget >= DEFAULT_TOKEN_BUDGET / 2 {
        SYSTEM_OVERHEAD_TOKENS
    } else {
        0
    };
    msg_tokens + overhead
}

pub fn semantic_goal_text(messages: &[Message]) -> String {
    let mut text = String::new();
    for msg in messages {
        for block in &msg.content {
            match block {
                ContentBlock::Text {
                    text: block_text, ..
                } => push_semantic_excerpt(&mut text, block_text, 200),
                ContentBlock::ToolResult { content, .. } => {
                    push_semantic_excerpt(&mut text, content, 100)
                }
                _ => {}
            }
        }
    }
    text
}

pub fn semantic_message_text(msg: &Message) -> String {
    let mut text = String::new();
    for block in &msg.content {
        if let ContentBlock::Text {
            text: block_text, ..
        } = block
        {
            push_semantic_excerpt(&mut text, block_text, EMBED_MAX_CHARS_PER_MSG);
        }
    }
    text
}

pub fn push_semantic_excerpt(target: &mut String, source: &str, max_chars: usize) {
    if source.is_empty() {
        return;
    }
    if !target.is_empty() {
        target.push(' ');
    }
    target.extend(source.chars().take(max_chars));
}

pub fn semantic_cache_key(text: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

pub fn build_emergency_summary_text(
    existing_summary: Option<&str>,
    dropped_count: usize,
    pre_tokens: u64,
    token_budget: usize,
    dropped_messages: &[Message],
) -> String {
    let mut summary_parts: Vec<String> = Vec::new();

    if let Some(existing) = existing_summary
        && !existing.is_empty()
    {
        summary_parts.push(existing.to_string());
    }

    summary_parts.push(format!(
        "**[Emergency compaction]**: {} messages were dropped to recover from context overflow. \
         The conversation had ~{}k tokens which exceeded the {}k limit.",
        dropped_count,
        pre_tokens / 1000,
        token_budget / 1000,
    ));

    let mut file_mentions = Vec::new();
    let mut tool_names = HashSet::new();
    for msg in dropped_messages {
        collect_emergency_summary_hints(msg, &mut tool_names, &mut file_mentions);
    }

    if !tool_names.is_empty() {
        let mut tools: Vec<_> = tool_names.into_iter().collect();
        tools.sort();
        summary_parts.push(format!("Tools used: {}", tools.join(", ")));
    }

    file_mentions.sort();
    file_mentions.dedup();
    if !file_mentions.is_empty() {
        file_mentions.truncate(30);
        summary_parts.push(format!("Files referenced: {}", file_mentions.join(", ")));
    }

    summary_parts.join("\n\n")
}

fn collect_emergency_summary_hints(
    msg: &Message,
    tool_names: &mut HashSet<String>,
    file_mentions: &mut Vec<String>,
) {
    for block in &msg.content {
        match block {
            ContentBlock::ToolUse { name, .. } => {
                tool_names.insert(name.clone());
            }
            ContentBlock::Text { text, .. } => {
                extract_file_mentions(text, file_mentions);
            }
            _ => {}
        }
    }
}

pub fn extract_file_mentions(text: &str, file_mentions: &mut Vec<String>) {
    for word in text.split_whitespace() {
        if looks_like_file_reference(word) {
            let cleaned = clean_file_reference(word);
            if !cleaned.is_empty() {
                file_mentions.push(cleaned.to_string());
            }
        }
    }
}

pub fn looks_like_file_reference(word: &str) -> bool {
    (word.contains('/') || word.contains('.'))
        && word.len() > 3
        && word.len() < 120
        && !word.starts_with("http")
        && (word.contains(".rs")
            || word.contains(".ts")
            || word.contains(".py")
            || word.contains(".toml")
            || word.contains(".json")
            || word.starts_with("src/")
            || word.starts_with("./"))
}

pub fn clean_file_reference(word: &str) -> &str {
    word.trim_matches(|c: char| {
        !c.is_alphanumeric() && c != '/' && c != '.' && c != '_' && c != '-'
    })
}

pub fn emergency_truncate_tool_results(messages: &mut [Message], max_chars: usize) -> usize {
    let mut truncated = 0;

    for msg in messages.iter_mut() {
        for block in msg.content.iter_mut() {
            if let ContentBlock::ToolResult { content, .. } = block
                && content.len() > max_chars
            {
                *content = emergency_truncated_tool_result(content, max_chars);
                truncated += 1;
            }
        }
    }

    truncated
}

pub fn emergency_truncated_tool_result(content: &str, max_chars: usize) -> String {
    let original_len = content.len();
    let keep_head = max_chars / 2;
    let keep_tail = max_chars / 4;
    let head = truncate_str_boundary(content, keep_head);
    let tail = tail_str_boundary(content, keep_tail);
    let truncated_len = original_len.saturating_sub(head.len() + tail.len());
    format!(
        "{}\n\n... [{} chars truncated for context recovery] ...\n\n{}",
        head, truncated_len, tail,
    )
}

pub fn tail_str_boundary(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut start = value.len().saturating_sub(max_bytes);
    while start < value.len() && !value.is_char_boundary(start) {
        start += 1;
    }
    &value[start..]
}

/// M48-C0: deterministic fixtures used by compaction unit tests.
///
/// These shapes drive every later M48 stage so that selection / prune /
/// summary changes can be diffed against a stable baseline. The fixtures are
/// intentionally simple: they only use `Message`/`ContentBlock`, no provider
/// state, no async, no token counter. Real-shape edge cases (e.g. mixed
/// `Reasoning` blocks, OpenAI native compaction encrypted content) live here
/// so later stages do not need to re-invent them.
pub mod m48_fixtures {
    use super::{ContentBlock, Message, Role};
    use chrono::TimeZone;

    /// Build a fixed timestamp so message serialization is reproducible.
    fn ts(turn: u32) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc
            .with_ymd_and_hms(2026, 5, 17, 12, 0, turn as u32 % 60)
            .single()
            .expect("valid timestamp")
    }

    /// User text message with a known turn id so tests can locate it.
    pub fn user_text(turn: u32, text: &str) -> Message {
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: format!("[turn {}] {}", turn, text),
                cache_control: None,
            }],
            timestamp: Some(ts(turn)),
            tool_duration_ms: None,
        }
    }

    /// Assistant text reply (no tools).
    pub fn assistant_text(turn: u32, text: &str) -> Message {
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: format!("[turn {}] {}", turn, text),
                cache_control: None,
            }],
            timestamp: Some(ts(turn)),
            tool_duration_ms: None,
        }
    }

    /// Assistant message with a single tool_use block.
    pub fn assistant_tool_use(turn: u32, tool_id: &str, tool_name: &str, input: &str) -> Message {
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: tool_id.to_string(),
                name: tool_name.to_string(),
                input: serde_json::json!({ "command": input }),
            }],
            timestamp: Some(ts(turn)),
            tool_duration_ms: None,
        }
    }

    /// User message carrying a tool_result block paired with a prior tool_use.
    pub fn user_tool_result(turn: u32, tool_use_id: &str, result: &str) -> Message {
        Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: result.to_string(),
                is_error: None,
            }],
            timestamp: Some(ts(turn)),
            tool_duration_ms: Some(50),
        }
    }

    /// User message with an attached image block.
    pub fn user_image(turn: u32, media_type: &str) -> Message {
        Message {
            role: Role::User,
            content: vec![
                ContentBlock::Text {
                    text: format!("[turn {}] please describe this image", turn),
                    cache_control: None,
                },
                ContentBlock::Image {
                    media_type: media_type.to_string(),
                    data: "AAECAwQFBgcICQ==".to_string(),
                },
            ],
            timestamp: Some(ts(turn)),
            tool_duration_ms: None,
        }
    }

    /// Hidden OpenAI native compaction artifact preserved across turns.
    pub fn assistant_openai_compaction(turn: u32, encrypted: &str) -> Message {
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::OpenAICompaction {
                encrypted_content: encrypted.to_string(),
            }],
            timestamp: Some(ts(turn)),
            tool_duration_ms: None,
        }
    }

    /// Scenario 1: short session safely below any compaction threshold.
    ///
    /// 4 messages, all small text. Used as a "should never compact" baseline.
    pub fn short_session() -> Vec<Message> {
        vec![
            user_text(1, "hi"),
            assistant_text(1, "hello, how can I help?"),
            user_text(2, "what is 2+2?"),
            assistant_text(2, "4"),
        ]
    }

    /// Scenario 2: long text-only session that exceeds 0.8 of a small budget.
    ///
    /// 20 turns of varied prose. Stresses the recent-tail selection path
    /// without exercising tools or media. Total chars: about 40_000.
    pub fn long_text_only_session() -> Vec<Message> {
        let mut out = Vec::new();
        for turn in 1..=20u32 {
            let user_body = "describe one architectural tradeoff in distributed systems and explain it in roughly three sentences with concrete examples from past projects you have seen. ".repeat(2);
            out.push(user_text(turn, &user_body));
            let asst_body = "tradeoff: consistency versus availability. example: a leader-based replicated log offers serializable reads but stalls when the leader is partitioned; example: a multi-master CRDT store keeps writes flowing but exposes weakly-consistent reads to clients. mitigation: route latency-sensitive reads to followers with explicit staleness bounds. ".repeat(3);
            out.push(assistant_text(turn, &asst_body));
        }
        out
    }

    /// Scenario 3: tool-output heavy session.
    ///
    /// 6 turns, each paired with a large tool_result. The result content is
    /// 4_000 chars so the cumulative tool body alone exceeds typical small
    /// budgets, which is what the future prune pass needs to handle.
    pub fn tool_heavy_session() -> Vec<Message> {
        let mut out = Vec::new();
        for turn in 1..=6u32 {
            out.push(user_text(turn, "run the test suite please"));
            let tool_id = format!("call_{:03}", turn);
            out.push(assistant_tool_use(turn, &tool_id, "bash", "cargo test --all"));
            // 4_000 char tool result of varied lines.
            let mut result = String::with_capacity(4_000);
            for line in 0..200u32 {
                result.push_str(&format!(
                    "  test crates::module{:02}::case{:02} ... ok\n",
                    turn, line
                ));
            }
            out.push(user_tool_result(turn, &tool_id, &result));
            out.push(assistant_text(turn, "tests passed, nothing else to do"));
        }
        out
    }

    /// Scenario 4: session that includes an image block.
    ///
    /// Used by the future stripMedia path to verify summary input drops the
    /// image and keeps only the text shell.
    pub fn image_session() -> Vec<Message> {
        vec![
            user_image(1, "image/png"),
            assistant_text(1, "this image looks like a system diagram with three nodes"),
            user_text(2, "explain the arrows"),
            assistant_text(2, "the arrows show message direction between leader and follower replicas"),
        ]
    }

    /// Scenario 5: session that already had an earlier OpenAI native compaction.
    ///
    /// The encrypted_content placeholder marks where a real provider blob
    /// would live. Later stages must keep this artifact intact when running
    /// durable compaction on top of native compaction.
    pub fn openai_native_compacted_session() -> Vec<Message> {
        vec![
            user_text(1, "lets continue from where we left off"),
            assistant_openai_compaction(1, "BASE64_ENCRYPTED_NATIVE_COMPACTION_PLACEHOLDER"),
            user_text(2, "next, summarize the open todos"),
            assistant_text(2, "open todos: 1) finish M48 C-0 fixtures 2) port opencode select() 3) wire prune pass"),
        ]
    }

    #[cfg(test)]
    mod fixture_self_tests {
        use super::*;

        #[test]
        fn short_session_message_count_is_stable() {
            assert_eq!(short_session().len(), 4);
        }

        #[test]
        fn long_text_only_session_has_paired_user_assistant_turns() {
            let msgs = long_text_only_session();
            assert_eq!(msgs.len(), 40);
            assert!(matches!(msgs[0].role, Role::User));
            assert!(matches!(msgs[1].role, Role::Assistant));
        }

        #[test]
        fn tool_heavy_session_pairs_each_tool_use_with_a_result() {
            let msgs = tool_heavy_session();
            // 6 turns x (user + tool_use + tool_result + assistant) = 24 messages.
            assert_eq!(msgs.len(), 24);
            let tool_uses: Vec<_> = msgs
                .iter()
                .flat_map(|m| m.content.iter())
                .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
                .collect();
            let tool_results: Vec<_> = msgs
                .iter()
                .flat_map(|m| m.content.iter())
                .filter(|b| matches!(b, ContentBlock::ToolResult { .. }))
                .collect();
            assert_eq!(tool_uses.len(), tool_results.len());
        }

        #[test]
        fn image_session_carries_image_block() {
            let msgs = image_session();
            assert!(
                msgs[0]
                    .content
                    .iter()
                    .any(|b| matches!(b, ContentBlock::Image { .. }))
            );
        }

        #[test]
        fn openai_native_compacted_session_preserves_encrypted_block() {
            let msgs = openai_native_compacted_session();
            assert!(msgs.iter().flat_map(|m| m.content.iter()).any(|b| matches!(
                b,
                ContentBlock::OpenAICompaction { encrypted_content }
                if encrypted_content.contains("PLACEHOLDER")
            )));
        }
    }
}

/// M48-C0: lightweight per-message token trace.
///
/// Existing token estimation in this crate operates on aggregate char counts.
/// The opencode-style selection + prune algorithm needs per-message and per
/// block visibility to decide where to split a turn. This module supplies a
/// pure, allocation-free counter so later stages can build budget-based
/// decisions on top of it without depending on a real tokenizer.
pub mod m48_trace {
    use super::{CHARS_PER_TOKEN, ContentBlock, Message};

    /// Token contribution of a single content block.
    ///
    /// Uses the same `CHARS_PER_TOKEN` approximation as the rest of this
    /// crate. The numbers are deterministic for the same input so unit tests
    /// can assert exact values.
    pub fn block_tokens(block: &ContentBlock) -> usize {
        match block {
            ContentBlock::Text { text, .. } => text.len() / CHARS_PER_TOKEN,
            ContentBlock::Reasoning { text } => text.len() / CHARS_PER_TOKEN,
            ContentBlock::ToolUse { name, input, .. } => {
                let serialized = serde_json::to_string(input)
                    .map(|s| s.len())
                    .unwrap_or(0);
                (name.len() + serialized) / CHARS_PER_TOKEN
            }
            ContentBlock::ToolResult { content, .. } => content.len() / CHARS_PER_TOKEN,
            ContentBlock::Image { data, .. } => data.len() / CHARS_PER_TOKEN,
            ContentBlock::OpenAICompaction { encrypted_content } => {
                encrypted_content.len() / CHARS_PER_TOKEN
            }
        }
    }

    /// Sum of every block's token contribution in a message.
    pub fn message_tokens(message: &Message) -> usize {
        message.content.iter().map(block_tokens).sum()
    }

    /// Detailed per-message trace describing token cost and block makeup.
    ///
    /// Useful as a debug aid in tests so failure output is human-readable.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct MessageTrace {
        pub index: usize,
        pub role: &'static str,
        pub tokens: usize,
        pub blocks: Vec<BlockTrace>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct BlockTrace {
        pub kind: &'static str,
        pub tokens: usize,
    }

    fn block_kind(block: &ContentBlock) -> &'static str {
        match block {
            ContentBlock::Text { .. } => "text",
            ContentBlock::Reasoning { .. } => "reasoning",
            ContentBlock::ToolUse { .. } => "tool_use",
            ContentBlock::ToolResult { .. } => "tool_result",
            ContentBlock::Image { .. } => "image",
            ContentBlock::OpenAICompaction { .. } => "openai_compaction",
        }
    }

    pub fn trace_messages(messages: &[Message]) -> Vec<MessageTrace> {
        messages
            .iter()
            .enumerate()
            .map(|(index, msg)| {
                let blocks = msg
                    .content
                    .iter()
                    .map(|b| BlockTrace {
                        kind: block_kind(b),
                        tokens: block_tokens(b),
                    })
                    .collect();
                MessageTrace {
                    index,
                    role: match msg.role {
                        super::Role::User => "user",
                        super::Role::Assistant => "assistant",
                    },
                    tokens: message_tokens(msg),
                    blocks,
                }
            })
            .collect()
    }

    /// Total token count across a slice of messages.
    pub fn total_tokens(messages: &[Message]) -> usize {
        messages.iter().map(message_tokens).sum()
    }

    #[cfg(test)]
    mod trace_self_tests {
        use super::super::m48_fixtures;
        use super::*;

        #[test]
        fn short_session_under_token_budget() {
            let tokens = total_tokens(&m48_fixtures::short_session());
            assert!(tokens < 200, "short_session should be under 200 tokens, got {tokens}");
        }

        #[test]
        fn long_text_only_session_exceeds_small_budget() {
            let tokens = total_tokens(&m48_fixtures::long_text_only_session());
            // The 20-turn fixture is intentionally large enough to trip a 5k
            // token budget so future stages can exercise overflow logic with
            // it.
            assert!(
                tokens >= 5_000,
                "long_text_only_session should exceed 5k tokens for overflow tests, got {tokens}"
            );
        }

        #[test]
        fn tool_heavy_session_dominated_by_tool_results() {
            let msgs = m48_fixtures::tool_heavy_session();
            let trace = trace_messages(&msgs);
            let tool_result_tokens: usize = trace
                .iter()
                .flat_map(|t| t.blocks.iter())
                .filter(|b| b.kind == "tool_result")
                .map(|b| b.tokens)
                .sum();
            let total: usize = trace.iter().map(|t| t.tokens).sum();
            // The fixture should be dominated by tool_result content (>50%)
            // because later stages will prune those first.
            assert!(
                tool_result_tokens * 2 >= total,
                "tool_heavy_session tool_result tokens {} should dominate total {}",
                tool_result_tokens,
                total
            );
        }

        #[test]
        fn image_session_image_block_has_nonzero_tokens() {
            let msgs = m48_fixtures::image_session();
            let trace = trace_messages(&msgs);
            assert!(trace.iter().any(|t| t.blocks.iter().any(|b| b.kind == "image" && b.tokens > 0)));
        }

        #[test]
        fn openai_native_compaction_block_contributes_tokens() {
            let msgs = m48_fixtures::openai_native_compacted_session();
            let trace = trace_messages(&msgs);
            let native_tokens: usize = trace
                .iter()
                .flat_map(|t| t.blocks.iter())
                .filter(|b| b.kind == "openai_compaction")
                .map(|b| b.tokens)
                .sum();
            assert!(native_tokens > 0);
        }
    }
}

/// M48-C2: opencode-style token-budgeted recent-tail selection.
///
/// Given a message log and a token budget, decides which prefix to summarize
/// ("head") and which suffix to keep verbatim in the next provider payload
/// ("tail"). The algorithm mirrors `session/compaction.ts::select` and
/// `splitTurn` in opencode:
///
/// 1. Compute `usable_budget = context_window - reserved_output_tokens`.
/// 2. Compute `preserve_recent_tokens = config override or
///    clamp(usable_budget * 0.25, MIN, MAX)`.
/// 3. Identify user-led turns; ignore compaction marker messages.
/// 4. From the last `tail_turns` turns, walk backwards adding whole turns
///    while their token cost stays within the preserve budget.
/// 5. If the next whole turn would not fit, attempt to split that turn at
///    a message boundary so the remaining suffix fits.
/// 6. The resulting `tail_start` indexes into `messages` so callers can
///    summarize `messages[..tail_start]` and forward `messages[tail_start..]`
///    verbatim. `tail_start == 0` means everything is preserved (no
///    compaction needed).
pub mod m48_select {
    use super::{ContentBlock, Message, Role, m48_trace};

    /// Opencode `COMPACTION_BUFFER`: tokens reserved for the assistant
    /// output when no explicit `reserved_tokens` value is configured.
    pub const DEFAULT_RESERVED_TOKENS: usize = 20_000;

    /// Opencode `MIN_PRESERVE_RECENT_TOKENS` / `MAX_PRESERVE_RECENT_TOKENS`.
    pub const MIN_PRESERVE_RECENT_TOKENS: usize = 2_000;
    pub const MAX_PRESERVE_RECENT_TOKENS: usize = 8_000;

    /// Opencode `DEFAULT_TAIL_TURNS` (number of recent user-led turns
    /// preserved verbatim).
    pub const DEFAULT_TAIL_TURNS: usize = 2;

    /// Compute the effective input budget for compaction selection.
    ///
    /// Mirrors opencode `overflow.ts::usable`:
    /// - returns 0 when `context_window == 0` (provider lacks a known limit)
    /// - otherwise returns `max(0, context_window - reserved)`
    pub fn usable_budget(context_window: usize, reserved_tokens: Option<usize>) -> usize {
        if context_window == 0 {
            return 0;
        }
        let reserved = reserved_tokens.unwrap_or(DEFAULT_RESERVED_TOKENS);
        context_window.saturating_sub(reserved)
    }

    /// Compute the per-tail preservation budget.
    ///
    /// Mirrors opencode `compaction.ts::preserveRecentBudget`: explicit
    /// override wins, otherwise clamp `floor(usable * 0.25)` to
    /// `[MIN_PRESERVE_RECENT_TOKENS, MAX_PRESERVE_RECENT_TOKENS]`.
    pub fn preserve_recent_budget(usable: usize, override_tokens: Option<usize>) -> usize {
        if let Some(value) = override_tokens {
            return value;
        }
        let quarter = usable / 4;
        quarter
            .max(MIN_PRESERVE_RECENT_TOKENS)
            .min(MAX_PRESERVE_RECENT_TOKENS)
    }

    /// A contiguous span of messages owned by a single user-led turn.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Turn {
        /// Index of the user message that starts the turn.
        pub start: usize,
        /// Exclusive end index (start of next user message or message count).
        pub end: usize,
    }

    /// True when the message is a compaction marker that should be skipped
    /// when walking user-led turns. M48 C-1 introduced compaction markers as
    /// user-role messages carrying an `OpenAICompaction` block but never any
    /// human text. Future stages will add a dedicated marker block; the
    /// heuristic here treats any user message whose visible blocks are all
    /// `OpenAICompaction` as a marker.
    fn is_compaction_marker(msg: &Message) -> bool {
        if msg.role != Role::User || msg.content.is_empty() {
            return false;
        }
        msg.content
            .iter()
            .all(|b| matches!(b, ContentBlock::OpenAICompaction { .. }))
    }

    /// Enumerate user-led turns, ignoring compaction markers. Mirrors
    /// opencode `compaction.ts::turns`.
    ///
    /// Each `Turn.end` is set to the next user message's `start`, or the
    /// total length for the last turn. Assistant messages between user
    /// messages are folded into the preceding turn.
    pub fn turns(messages: &[Message]) -> Vec<Turn> {
        let mut result: Vec<Turn> = Vec::new();
        for (i, msg) in messages.iter().enumerate() {
            if msg.role != Role::User {
                continue;
            }
            if is_compaction_marker(msg) {
                continue;
            }
            result.push(Turn {
                start: i,
                end: messages.len(),
            });
        }
        for i in 0..result.len().saturating_sub(1) {
            result[i].end = result[i + 1].start;
        }
        result
    }

    /// Result of `select_tail`: where the verbatim tail begins.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct TailSelection {
        /// First message index that should be kept verbatim. Messages at
        /// `[..tail_start]` are candidates for summarization. `0` means the
        /// entire history fits and no compaction is required.
        pub tail_start: usize,
        /// Estimated token cost of the preserved tail.
        pub tail_tokens: usize,
        /// True when the tail was forced to start mid-turn because no
        /// whole-turn slice fit the budget. Callers may want to log this.
        pub split_turn: bool,
    }

    /// Decide where the verbatim tail begins.
    ///
    /// `tail_turns_limit` is the configured `tail_turns` value (defaults to
    /// `DEFAULT_TAIL_TURNS`). `budget` is `preserve_recent_budget` output.
    /// Mirrors opencode `compaction.ts::select`.
    pub fn select_tail(
        messages: &[Message],
        budget: usize,
        tail_turns_limit: Option<usize>,
    ) -> TailSelection {
        if messages.is_empty() || budget == 0 {
            return TailSelection {
                tail_start: 0,
                tail_tokens: 0,
                split_turn: false,
            };
        }

        let limit = tail_turns_limit.unwrap_or(DEFAULT_TAIL_TURNS);
        if limit == 0 {
            // Caller explicitly asked for no recent-tail preservation.
            return TailSelection {
                tail_start: messages.len(),
                tail_tokens: 0,
                split_turn: false,
            };
        }

        let all_turns = turns(messages);
        if all_turns.is_empty() {
            // No user turns at all: nothing to preserve, summarize the whole
            // history (e.g. only assistant boilerplate).
            return TailSelection {
                tail_start: 0,
                tail_tokens: 0,
                split_turn: false,
            };
        }

        let recent_start = all_turns.len().saturating_sub(limit);
        let recent: Vec<Turn> = all_turns[recent_start..].to_vec();

        // Walk backwards across the recent slice, keeping whole turns while
        // they fit. Stop on the first turn that does not fit; attempt a
        // mid-turn split there.
        let mut total = 0usize;
        let mut keep_start: Option<usize> = None;
        for turn in recent.iter().rev() {
            let size = m48_trace::total_tokens(&messages[turn.start..turn.end]);
            if total + size <= budget {
                total += size;
                keep_start = Some(turn.start);
                continue;
            }
            // Turn doesn't fit whole. Try to split it.
            let remaining = budget.saturating_sub(total);
            if let Some(split) = split_turn(messages, *turn, remaining) {
                let split_size = m48_trace::total_tokens(&messages[split..turn.end]);
                total += split_size;
                return TailSelection {
                    tail_start: split,
                    tail_tokens: total,
                    split_turn: true,
                };
            }
            break;
        }

        match keep_start {
            Some(start) if start > 0 => TailSelection {
                tail_start: start,
                tail_tokens: total,
                split_turn: false,
            },
            Some(_) => {
                // The recent window already starts at index 0: everything
                // fits and no compaction is needed.
                TailSelection {
                    tail_start: 0,
                    tail_tokens: total,
                    split_turn: false,
                }
            }
            None => {
                // Not even one whole recent turn fit and no split point was
                // found. Fall back to "summarize everything" so the caller
                // does not panic on an oversized turn.
                TailSelection {
                    tail_start: messages.len(),
                    tail_tokens: 0,
                    split_turn: false,
                }
            }
        }
    }

    /// Look inside a single turn for the smallest suffix that still fits in
    /// the remaining budget. Returns the absolute message index where that
    /// suffix begins, or `None` when no suffix fits or the turn has only one
    /// message. Mirrors opencode `compaction.ts::splitTurn`.
    pub fn split_turn(messages: &[Message], turn: Turn, budget: usize) -> Option<usize> {
        if budget == 0 {
            return None;
        }
        if turn.end.saturating_sub(turn.start) <= 1 {
            return None;
        }
        // Walk forward starting at the first non-anchor message in the
        // turn; the first index that fits the remaining budget is the
        // split point.
        for start in (turn.start + 1)..turn.end {
            let size = m48_trace::total_tokens(&messages[start..turn.end]);
            if size <= budget {
                return Some(start);
            }
        }
        None
    }

    #[cfg(test)]
    mod select_tests {
        use super::super::m48_fixtures;
        use super::*;

        #[test]
        fn usable_budget_subtracts_reserved_tokens() {
            assert_eq!(usable_budget(200_000, Some(20_000)), 180_000);
            assert_eq!(usable_budget(200_000, None), 180_000);
            assert_eq!(usable_budget(0, None), 0);
            // Saturating subtraction so we never underflow.
            assert_eq!(usable_budget(10_000, Some(50_000)), 0);
        }

        #[test]
        fn preserve_recent_budget_clamps_to_range() {
            // Below MIN -> clamped up
            assert_eq!(preserve_recent_budget(100, None), MIN_PRESERVE_RECENT_TOKENS);
            // Above MAX -> clamped down
            assert_eq!(preserve_recent_budget(40_000, None), MAX_PRESERVE_RECENT_TOKENS);
            // Inside range -> floor(usable/4)
            assert_eq!(preserve_recent_budget(20_000, None), 5_000);
            // Explicit override wins
            assert_eq!(preserve_recent_budget(20_000, Some(1_500)), 1_500);
        }

        #[test]
        fn turns_skips_assistant_only_runs() {
            let msgs = m48_fixtures::short_session();
            let t = turns(&msgs);
            // short_session: user, assistant, user, assistant
            assert_eq!(t.len(), 2);
            assert_eq!(t[0].start, 0);
            assert_eq!(t[0].end, 2);
            assert_eq!(t[1].start, 2);
            assert_eq!(t[1].end, 4);
        }

        #[test]
        fn turns_ignores_compaction_marker_messages() {
            // openai_native_compacted_session: user, assistant (compaction
            // block as user-role marker is fine to skip), user, assistant.
            let msgs = m48_fixtures::openai_native_compacted_session();
            let t = turns(&msgs);
            // Two real user turns (no OpenAICompaction marker among users in
            // this fixture; assistant_openai_compaction is an assistant).
            assert_eq!(t.len(), 2);
        }

        #[test]
        fn select_tail_short_session_returns_zero() {
            let msgs = m48_fixtures::short_session();
            let result = select_tail(&msgs, 10_000, Some(2));
            // Whole history fits in 10k tokens.
            assert_eq!(result.tail_start, 0);
            assert!(!result.split_turn);
        }

        #[test]
        fn select_tail_long_session_keeps_last_turns_under_budget() {
            let msgs = m48_fixtures::long_text_only_session();
            let result = select_tail(&msgs, 2_000, Some(2));
            // Budget too small for the whole 20-turn session. Tail must
            // start mid-history but never beyond the last 2 user turns.
            assert!(result.tail_start > 0);
            assert!(result.tail_tokens <= 2_000);
            // Verify tail does not exceed the last 2 user turns.
            let all_turns = turns(&msgs);
            let last_two_start = all_turns[all_turns.len() - 2].start;
            assert!(result.tail_start >= last_two_start);
        }

        #[test]
        fn select_tail_respects_zero_tail_turns_limit() {
            let msgs = m48_fixtures::long_text_only_session();
            let result = select_tail(&msgs, 10_000, Some(0));
            // tail_turns = 0 -> drop everything to summary.
            assert_eq!(result.tail_start, msgs.len());
        }

        #[test]
        fn select_tail_with_default_limit_keeps_last_two_turns_when_budget_large() {
            let msgs = m48_fixtures::long_text_only_session();
            let big_budget = m48_trace::total_tokens(&msgs);
            let result = select_tail(&msgs, big_budget, None);
            // With default tail_turns = 2, even a generous budget only keeps
            // the last two user turns verbatim. The rest of the 20-turn
            // history is candidate for summarization, so tail_start should
            // point to the start of the second-to-last turn.
            let all_turns = turns(&msgs);
            let expected_start = all_turns[all_turns.len() - 2].start;
            assert_eq!(result.tail_start, expected_start);
            assert!(!result.split_turn);
        }

        #[test]
        fn split_turn_finds_suffix_inside_oversized_turn() {
            // Build a 3-message turn whose total exceeds the budget but
            // whose final message alone fits.
            let msgs = vec![
                m48_fixtures::user_text(1, "header text occupying many tokens ".repeat(100).as_str()),
                m48_fixtures::assistant_text(1, "long assistant response ".repeat(100).as_str()),
                m48_fixtures::user_text(2, "follow up"),
            ];
            // Turn is [0..3) here because the second user message also
            // starts a new turn; pretend the caller passes the whole range.
            let turn = Turn { start: 0, end: 3 };
            let tiny_budget = m48_trace::total_tokens(&msgs[2..3]);
            let split = split_turn(&msgs, turn, tiny_budget);
            assert_eq!(split, Some(2));
        }

        #[test]
        fn split_turn_returns_none_for_single_message_turn() {
            let msgs = vec![m48_fixtures::user_text(1, "single")];
            let turn = Turn { start: 0, end: 1 };
            assert_eq!(split_turn(&msgs, turn, 10), None);
        }

        #[test]
        fn select_tail_falls_back_to_summarize_everything_when_no_suffix_fits() {
            // Single oversized turn with budget too small to admit even the
            // last message: caller should summarize the whole thing.
            let msgs = vec![
                m48_fixtures::user_text(
                    1,
                    "a very long single user message ".repeat(2_000).as_str(),
                ),
            ];
            let result = select_tail(&msgs, 1, Some(2));
            assert_eq!(result.tail_start, msgs.len());
            assert_eq!(result.tail_tokens, 0);
        }
    }
}

/// M48-C3: pre-summary tool-output pruning pass.
///
/// Mirrors opencode `session/compaction.ts::prune`: walks the message log
/// backwards, protects the last `protect_recent_turns` user-led turns, and
/// then replaces older `ToolResult` payloads with a short placeholder once
/// the rolling token budget exceeds `PRUNE_PROTECT`. Only fires when the
/// total bytes recovered would exceed `PRUNE_MINIMUM`, otherwise the input
/// is returned unchanged so we never burn IO for a trivial savings.
///
/// Differences from opencode:
/// - opencode mutates persistent `ToolPart.state.time.compacted`; jcode
///   `ContentBlock::ToolResult` has no such field yet, so this stage is
///   purely functional: it returns a new `Vec<Message>` with placeholder
///   `ToolResult { content }` payloads. Wiring this into session persistence
///   is M48-C4's job (alongside the anchored summary template).
/// - `protected_tools` is a slice argument rather than a global const so
///   tests can simulate skill-style protected tools without depending on
///   the runtime tool registry.
pub mod m48_prune {
    use super::{ContentBlock, Message, Role};

    /// Opencode `PRUNE_PROTECT`: tail-side budget of tool-output tokens
    /// that survive a prune pass even when they fall outside the protected
    /// turn window.
    pub const PRUNE_PROTECT: usize = 40_000;

    /// Opencode `PRUNE_MINIMUM`: minimum amount of token-equivalent bytes a
    /// prune pass must recover before it is allowed to mutate anything.
    pub const PRUNE_MINIMUM: usize = 20_000;

    /// Opencode `PRUNE_PROTECTED_TOOLS`: tool names whose outputs are never
    /// pruned. Callers pass this through; the default `prune` invocation
    /// uses the empty slice (no protected tools) so tests are deterministic
    /// without depending on the runtime tool registry.
    pub const DEFAULT_PROTECTED_TOOLS: &[&str] = &["skill"];

    /// Opencode `prune`'s implicit `turns < 2` guard: the last N user-led
    /// turns are skipped entirely so the most recent context is preserved
    /// verbatim.
    pub const DEFAULT_PROTECT_RECENT_TURNS: usize = 2;

    /// Placeholder content written in place of pruned tool output. Length
    /// is intentionally short so the prune savings are observable; the text
    /// matches opencode's `<tool result removed by compaction>` convention.
    pub const PRUNED_PLACEHOLDER: &str =
        "[tool output removed by compaction]";

    /// Summary of a prune pass.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct PruneReport {
        /// Number of `ToolResult` blocks rewritten to the placeholder.
        pub blocks_pruned: usize,
        /// Sum of original `content.len()` bytes across pruned blocks.
        pub bytes_recovered: usize,
        /// Whether the pass actually committed mutations. False when the
        /// would-be recovery was below `PRUNE_MINIMUM` (returned input
        /// untouched) or when nothing qualified.
        pub committed: bool,
    }

    /// Build a lookup from `tool_use_id` to tool name by scanning every
    /// `ToolUse` block in the messages. Needed because protection happens
    /// on the assistant-side `ToolUse.name`, not on the user-side
    /// `ToolResult.tool_use_id`.
    fn tool_use_id_to_name(messages: &[Message]) -> std::collections::HashMap<String, String> {
        let mut map = std::collections::HashMap::new();
        for msg in messages {
            for block in &msg.content {
                if let ContentBlock::ToolUse { id, name, .. } = block {
                    map.insert(id.clone(), name.clone());
                }
            }
        }
        map
    }

    /// True when a `ToolResult` looks like our prune placeholder. Used so
    /// re-prunes are idempotent (we never "recover" placeholder bytes).
    fn is_pruned_placeholder(content: &str) -> bool {
        content == PRUNED_PLACEHOLDER
    }

    /// True for "tool-result-only" user messages (e.g. provider-required
    /// continuation messages that carry only `ToolResult` blocks). These
    /// must NOT count as new conversational turns, otherwise the recent-
    /// turn protection window collapses because a single user prompt that
    /// triggered N tool calls would inflate the turn count by N+1.
    fn is_tool_result_only_user_message(msg: &Message) -> bool {
        if msg.role != Role::User || msg.content.is_empty() {
            return false;
        }
        msg.content
            .iter()
            .all(|b| matches!(b, ContentBlock::ToolResult { .. }))
    }

    /// Plan + execute a prune pass.
    ///
    /// Returns `(new_messages, report)`. When `report.committed == false`
    /// the returned `Vec<Message>` is a cheap clone of the input with no
    /// content changes. Callers can compare report.bytes_recovered against
    /// their threshold before persisting.
    pub fn prune(
        messages: &[Message],
        protected_tools: &[&str],
        protect_recent_turns: usize,
        prune_protect_tokens: usize,
        prune_minimum_tokens: usize,
    ) -> (Vec<Message>, PruneReport) {
        let name_map = tool_use_id_to_name(messages);
        let recent_limit = protect_recent_turns;

        // First pass: discover indices (msg_index, block_index, recovered_bytes)
        // walking backwards, tracking turn count and a rolling tool-output
        // budget. Mirrors the opencode `loop` block but skips tool-result-
        // only user messages when counting turns so the multi-message-per-
        // turn shape (user text + assistant tool_use + user tool_result)
        // collapses to a single turn boundary.
        let mut plan: Vec<(usize, usize, usize)> = Vec::new();
        let mut turns_seen = 0usize;
        let mut rolling = 0usize;
        for (msg_i, msg) in messages.iter().enumerate().rev() {
            if msg.role == Role::User && !is_tool_result_only_user_message(msg) {
                turns_seen += 1;
            }
            if turns_seen < recent_limit {
                continue;
            }
            for (block_i, block) in msg.content.iter().enumerate().rev() {
                let ContentBlock::ToolResult { content, tool_use_id, .. } = block else {
                    continue;
                };
                if is_pruned_placeholder(content) {
                    continue;
                }
                // Look up tool name via paired ToolUse and skip protected ones.
                if let Some(name) = name_map.get(tool_use_id) {
                    if protected_tools.iter().any(|t| *t == name.as_str()) {
                        continue;
                    }
                }
                let size = content.len();
                rolling += size;
                if rolling <= prune_protect_tokens {
                    continue;
                }
                plan.push((msg_i, block_i, size));
            }
        }

        let bytes_recovered: usize = plan.iter().map(|(_, _, s)| *s).sum();
        let mut report = PruneReport {
            blocks_pruned: 0,
            bytes_recovered,
            committed: false,
        };

        if bytes_recovered <= prune_minimum_tokens {
            // Threshold not reached -> bail without mutating.
            return (messages.to_vec(), report);
        }

        // Commit phase: clone messages, then rewrite tool-result contents.
        let mut new_messages = messages.to_vec();
        for (msg_i, block_i, _) in &plan {
            if let Some(block) = new_messages
                .get_mut(*msg_i)
                .and_then(|m| m.content.get_mut(*block_i))
            {
                if let ContentBlock::ToolResult { content, .. } = block {
                    *content = PRUNED_PLACEHOLDER.to_string();
                }
            }
        }
        report.blocks_pruned = plan.len();
        report.committed = true;
        (new_messages, report)
    }

    /// Convenience wrapper using the opencode defaults: `["skill"]`
    /// protected tools, last 2 turns protected, `PRUNE_PROTECT` rolling
    /// budget, `PRUNE_MINIMUM` recovery threshold.
    pub fn prune_with_defaults(messages: &[Message]) -> (Vec<Message>, PruneReport) {
        prune(
            messages,
            DEFAULT_PROTECTED_TOOLS,
            DEFAULT_PROTECT_RECENT_TURNS,
            PRUNE_PROTECT,
            PRUNE_MINIMUM,
        )
    }

    #[cfg(test)]
    mod prune_tests {
        use super::super::m48_fixtures;
        use super::*;
        use jcode_message_types::{ContentBlock, Role};

        fn count_tool_results(msgs: &[Message]) -> usize {
            msgs.iter()
                .flat_map(|m| m.content.iter())
                .filter(|b| matches!(b, ContentBlock::ToolResult { .. }))
                .count()
        }

        fn count_placeholders(msgs: &[Message]) -> usize {
            msgs.iter()
                .flat_map(|m| m.content.iter())
                .filter(|b| {
                    matches!(
                        b,
                        ContentBlock::ToolResult { content, .. } if is_pruned_placeholder(content)
                    )
                })
                .count()
        }

        #[test]
        fn small_session_does_not_meet_minimum_threshold() {
            // short_session has no tool results; recovery is 0.
            let msgs = m48_fixtures::short_session();
            let (out, report) = prune_with_defaults(&msgs);
            assert!(!report.committed);
            assert_eq!(report.blocks_pruned, 0);
            assert_eq!(report.bytes_recovered, 0);
            assert_eq!(out.len(), msgs.len());
        }

        #[test]
        fn large_tool_outputs_get_pruned_outside_protected_window() {
            // Build a session with 6 user turns of large tool output so
            // total bytes far exceed PRUNE_PROTECT + PRUNE_MINIMUM.
            let mut msgs: Vec<Message> = Vec::new();
            for turn in 1..=6 {
                msgs.push(m48_fixtures::user_text(turn, "run tests please"));
                let tool_id = format!("call-{turn}");
                msgs.push(m48_fixtures::assistant_tool_use(
                    turn,
                    &tool_id,
                    "bash",
                    "cargo test --all",
                ));
                let payload = "test output line\n".repeat(2_500); // ~42k bytes each
                msgs.push(m48_fixtures::user_tool_result(turn, &tool_id, &payload));
            }
            let (out, report) = prune_with_defaults(&msgs);
            assert!(report.committed, "should prune: {:?}", report);
            assert!(report.blocks_pruned > 0);
            assert!(report.bytes_recovered > PRUNE_MINIMUM);
            // The two most recent turns must keep their tool results intact.
            let total_results = count_tool_results(&out);
            let placeholders = count_placeholders(&out);
            assert_eq!(total_results, 6); // count stays the same; only content changes
            assert!(placeholders >= 1 && placeholders <= 4);
            // Last turn's ToolResult must not be a placeholder.
            let last_tool_result = out
                .iter()
                .rev()
                .find_map(|m| {
                    m.content.iter().rev().find_map(|b| match b {
                        ContentBlock::ToolResult { content, .. } => Some(content.clone()),
                        _ => None,
                    })
                })
                .expect("a tool result");
            assert!(!is_pruned_placeholder(&last_tool_result));
        }

        #[test]
        fn protected_tool_names_are_never_pruned() {
            // Same shape as the previous test but the tool name is "skill"
            // (the opencode-style protected tool).
            let mut msgs: Vec<Message> = Vec::new();
            for turn in 1..=6 {
                msgs.push(m48_fixtures::user_text(turn, "load skill"));
                let tool_id = format!("call-{turn}");
                msgs.push(m48_fixtures::assistant_tool_use(
                    turn,
                    &tool_id,
                    "skill",
                    "load-rules",
                ));
                let payload = "skill output line\n".repeat(2_500);
                msgs.push(m48_fixtures::user_tool_result(turn, &tool_id, &payload));
            }
            let (out, report) = prune_with_defaults(&msgs);
            // Skill outputs never accumulate into the rolling budget, so
            // nothing should be pruned.
            assert_eq!(report.blocks_pruned, 0);
            assert!(!report.committed);
            assert_eq!(count_placeholders(&out), 0);
        }

        #[test]
        fn prune_is_idempotent_on_already_pruned_content() {
            let mut msgs: Vec<Message> = Vec::new();
            for turn in 1..=6 {
                msgs.push(m48_fixtures::user_text(turn, "run"));
                let tool_id = format!("call-{turn}");
                msgs.push(m48_fixtures::assistant_tool_use(
                    turn,
                    &tool_id,
                    "bash",
                    "ls",
                ));
                let payload = "out\n".repeat(15_000);
                msgs.push(m48_fixtures::user_tool_result(turn, &tool_id, &payload));
            }
            let (first, r1) = prune_with_defaults(&msgs);
            assert!(r1.committed);
            let (second, r2) = prune_with_defaults(&first);
            // Second pass should find no new bytes to recover.
            assert_eq!(r2.bytes_recovered, 0);
            assert!(!r2.committed);
            assert_eq!(count_placeholders(&first), count_placeholders(&second));
        }

        #[test]
        fn protect_recent_turns_skips_last_n_turns() {
            // Two big tool results in two turns; protect_recent=2 means
            // neither is touched.
            let mut msgs: Vec<Message> = Vec::new();
            for turn in 1..=2 {
                msgs.push(m48_fixtures::user_text(turn, "go"));
                let tool_id = format!("c-{turn}");
                msgs.push(m48_fixtures::assistant_tool_use(
                    turn,
                    &tool_id,
                    "bash",
                    "ls",
                ));
                msgs.push(m48_fixtures::user_tool_result(
                    turn,
                    &tool_id,
                    &"x\n".repeat(30_000),
                ));
            }
            let (_, report) = prune_with_defaults(&msgs);
            // turns_seen < 2 -> nothing qualifies.
            assert_eq!(report.blocks_pruned, 0);
            assert!(!report.committed);
        }

        #[test]
        fn rolling_budget_keeps_first_recent_tail_intact() {
            // Construct: 5 turns where each tool_result is ~25k bytes
            // (total > PROTECT). Last 2 turns are protected; from turn 3
            // backwards the rolling sum will exceed PRUNE_PROTECT once we
            // accumulate enough, so the older turns get pruned.
            let mut msgs: Vec<Message> = Vec::new();
            for turn in 1..=5 {
                msgs.push(m48_fixtures::user_text(turn, "go"));
                let tool_id = format!("c-{turn}");
                msgs.push(m48_fixtures::assistant_tool_use(
                    turn,
                    &tool_id,
                    "bash",
                    "ls",
                ));
                msgs.push(m48_fixtures::user_tool_result(
                    turn,
                    &tool_id,
                    &"y\n".repeat(15_000),
                ));
            }
            let (out, report) = prune_with_defaults(&msgs);
            assert!(report.committed);
            // The two newest turns (turn 4, 5) must keep their tool results.
            // Find the user-tool_result messages for turns 4 and 5 by their
            // content's role and position.
            let user_results: Vec<&Message> = out
                .iter()
                .filter(|m| {
                    m.role == Role::User
                        && m.content
                            .iter()
                            .any(|b| matches!(b, ContentBlock::ToolResult { .. }))
                })
                .collect();
            assert_eq!(user_results.len(), 5);
            // The last one (turn 5) is always protected.
            let last = user_results.last().unwrap();
            let last_content = last
                .content
                .iter()
                .find_map(|b| match b {
                    ContentBlock::ToolResult { content, .. } => Some(content.clone()),
                    _ => None,
                })
                .unwrap();
            assert!(!is_pruned_placeholder(&last_content));
        }
    }
}

/// M48-C4: anchored summary template + previousSummary chaining.
///
/// Mirrors opencode `session/compaction.ts::SUMMARY_TEMPLATE` and
/// `buildPrompt` so that subsequent compaction events update an
/// already-existing anchored summary instead of summarizing from scratch
/// every time. This is what gives long opencode sessions their cheap,
/// stable "memory" instead of progressively eroding into noise.
///
/// This stage is prompt + chain plumbing only. The LLM call that actually
/// produces the summary text is wired in M48-C5/C-6 alongside the
/// replay-on-overflow path and the OpenAI native compaction coexistence
/// logic. Keeping the template a `pub const &'static str` lets every
/// future caller share the same shape and lets us diff prompt drift in
/// reviews.
pub mod m48_summary {
    /// The shared 8-section markdown skeleton every anchored summary must
    /// fill. Keeping this in source (not a config file) so we can review
    /// drift in git history. Section order matches opencode 1:1.
    pub const SUMMARY_TEMPLATE: &str = r#"Output exactly the Markdown structure shown inside <template> and keep the section order unchanged. Do not include the <template> tags in your response.
<template>
## Goal
- [single-sentence task summary]

## Constraints & Preferences
- [user constraints, preferences, specs, or "(none)"]

## Progress
### Done
- [completed work or "(none)"]

### In Progress
- [current work or "(none)"]

### Blocked
- [blockers or "(none)"]

## Key Decisions
- [decision and why, or "(none)"]

## Next Steps
- [ordered next actions or "(none)"]

## Critical Context
- [important technical facts, errors, open questions, or "(none)"]

## Relevant Files
- [file or directory path: why it matters, or "(none)"]
</template>

Rules:
- Keep every section, even when empty.
- Use terse bullets, not prose paragraphs.
- Preserve exact file paths, commands, error strings, and identifiers when known.
- Do not mention the summary process or that context was compacted."#;

    /// Anchor prologue used when a `previous_summary` exists. We ask the
    /// model to refresh that summary rather than recreate it from scratch.
    /// Matches opencode `buildPrompt` exactly so prompt drift is easy to
    /// audit between the two codebases.
    pub const UPDATE_ANCHOR_PROLOGUE: &str = "Update the anchored summary below using the conversation history above.\nPreserve still-true details, remove stale details, and merge in the new facts.";

    /// Anchor prologue used when there is no prior summary in the chain.
    pub const CREATE_ANCHOR_PROLOGUE: &str =
        "Create a new anchored summary from the conversation history above.";

    /// XML-ish marker that wraps the previous summary inside the prompt so
    /// the model can find the anchor verbatim. Opencode uses literal
    /// `<previous-summary>` tags; we keep the exact bytes for parity.
    pub const PREVIOUS_SUMMARY_OPEN_TAG: &str = "<previous-summary>";
    pub const PREVIOUS_SUMMARY_CLOSE_TAG: &str = "</previous-summary>";

    /// Build the anchored summary prompt.
    ///
    /// `previous_summary` should carry the verbatim markdown of the most
    /// recent durable summary (or `None` on the first compaction event).
    /// `context` is the opencode `context` field: extra free-form bullets
    /// the caller wants to inject (system reminders, environment notes,
    /// etc.). When empty, no extra block is appended.
    ///
    /// Mirrors opencode `buildPrompt`:
    /// `[anchor, SUMMARY_TEMPLATE, ...context].join("\n\n")`.
    pub fn build_prompt(previous_summary: Option<&str>, context: &[&str]) -> String {
        let anchor = match previous_summary {
            Some(prev) => format!(
                "{}\n{}\n{}\n{}",
                UPDATE_ANCHOR_PROLOGUE,
                PREVIOUS_SUMMARY_OPEN_TAG,
                prev,
                PREVIOUS_SUMMARY_CLOSE_TAG,
            ),
            None => CREATE_ANCHOR_PROLOGUE.to_string(),
        };

        let mut parts: Vec<String> = vec![anchor, SUMMARY_TEMPLATE.to_string()];
        for ctx in context {
            parts.push((*ctx).to_string());
        }
        parts.join("\n\n")
    }

    /// Trait describing the minimal shape `resolve_previous_summary` needs.
    /// We define it here so consumers do not have to depend on the full
    /// `jcode-session-types` crate just to walk the chain. Concrete impl
    /// lives behind the `previous_summary_id` field of `StoredCompactionTurn`.
    pub trait CompactionTurnSlice {
        /// Unique id for this turn.
        fn id(&self) -> &str;
        /// Optional previous summary id for chain walking.
        fn previous_summary_id(&self) -> Option<&str>;
        /// True when the entry is a synthetic legacy backfill (no real
        /// marker/summary messages). These cannot serve as anchors.
        fn is_legacy_backfill(&self) -> bool;
        /// Message id of the assistant-role summary message that holds the
        /// verbatim summary text. Empty for legacy backfill entries.
        fn summary_message_id(&self) -> &str;
    }

    /// Walk the compaction-turn chain backwards from `current_id` to find
    /// the most recent **usable** anchored summary (one that has a real
    /// `summary_message_id` and is not a legacy backfill).
    ///
    /// Returns the `summary_message_id` of the resolved turn, or `None`
    /// when the chain ends before finding one (e.g. only legacy-backfill
    /// turns exist).
    ///
    /// The lookup is `O(chain length)` and tolerates cycles via a
    /// bounded visit cap (`MAX_CHAIN_DEPTH`) to defend against malformed
    /// sidecar data.
    pub const MAX_CHAIN_DEPTH: usize = 256;

    pub fn resolve_previous_summary_id<'a, T: CompactionTurnSlice>(
        turns: &'a [T],
        current_id: &str,
    ) -> Option<&'a str> {
        if turns.is_empty() {
            return None;
        }

        // Build an id -> &T index once.
        let by_id: std::collections::HashMap<&str, &T> =
            turns.iter().map(|t| (t.id(), t)).collect();

        let mut cursor: Option<&str> = by_id.get(current_id).and_then(|t| t.previous_summary_id());
        let mut hops = 0usize;
        while let Some(id) = cursor {
            if hops >= MAX_CHAIN_DEPTH {
                return None;
            }
            hops += 1;
            let Some(t) = by_id.get(id) else { return None };
            if !t.is_legacy_backfill() && !t.summary_message_id().is_empty() {
                return Some(t.summary_message_id());
            }
            cursor = t.previous_summary_id();
        }
        None
    }

    #[cfg(test)]
    mod summary_tests {
        use super::*;

        struct Fake {
            id: String,
            prev: Option<String>,
            legacy: bool,
            summary_msg_id: String,
        }

        impl CompactionTurnSlice for Fake {
            fn id(&self) -> &str {
                &self.id
            }
            fn previous_summary_id(&self) -> Option<&str> {
                self.prev.as_deref()
            }
            fn is_legacy_backfill(&self) -> bool {
                self.legacy
            }
            fn summary_message_id(&self) -> &str {
                &self.summary_msg_id
            }
        }

        fn turn(id: &str, prev: Option<&str>, legacy: bool, summary_msg: &str) -> Fake {
            Fake {
                id: id.to_string(),
                prev: prev.map(str::to_string),
                legacy,
                summary_msg_id: summary_msg.to_string(),
            }
        }

        #[test]
        fn create_prompt_has_no_anchor_block() {
            let prompt = build_prompt(None, &[]);
            assert!(prompt.starts_with(CREATE_ANCHOR_PROLOGUE));
            assert!(prompt.contains("## Goal"));
            assert!(!prompt.contains(PREVIOUS_SUMMARY_OPEN_TAG));
        }

        #[test]
        fn update_prompt_wraps_previous_summary() {
            let prev = "## Goal\n- demo summary";
            let prompt = build_prompt(Some(prev), &[]);
            assert!(prompt.starts_with(UPDATE_ANCHOR_PROLOGUE));
            assert!(prompt.contains(PREVIOUS_SUMMARY_OPEN_TAG));
            assert!(prompt.contains(prev));
            assert!(prompt.contains(PREVIOUS_SUMMARY_CLOSE_TAG));
            // Template still present.
            assert!(prompt.contains("## Critical Context"));
        }

        #[test]
        fn build_prompt_appends_context_blocks_in_order() {
            let prompt = build_prompt(None, &["env=dev", "tz=UTC"]);
            let env_pos = prompt.find("env=dev").unwrap();
            let tz_pos = prompt.find("tz=UTC").unwrap();
            assert!(env_pos < tz_pos);
            // Context must come AFTER the template.
            let template_pos = prompt.find("</template>").unwrap();
            assert!(template_pos < env_pos);
        }

        #[test]
        fn resolve_previous_summary_returns_none_on_empty_chain() {
            let turns: Vec<Fake> = vec![];
            assert_eq!(resolve_previous_summary_id(&turns, "x"), None);
        }

        #[test]
        fn resolve_previous_summary_returns_none_when_no_predecessor() {
            let turns = vec![turn("t1", None, false, "msg-1")];
            assert_eq!(resolve_previous_summary_id(&turns, "t1"), None);
        }

        #[test]
        fn resolve_previous_summary_returns_immediate_anchor() {
            let turns = vec![
                turn("t1", None, false, "msg-1"),
                turn("t2", Some("t1"), false, "msg-2"),
            ];
            assert_eq!(resolve_previous_summary_id(&turns, "t2"), Some("msg-1"));
        }

        #[test]
        fn resolve_previous_summary_skips_legacy_backfill_entries() {
            let turns = vec![
                turn("legacy", None, true, ""),
                turn("t1", Some("legacy"), false, "msg-1"),
                turn("t2", Some("t1"), false, "msg-2"),
            ];
            // t2 -> t1 (real) -> stop
            assert_eq!(resolve_previous_summary_id(&turns, "t2"), Some("msg-1"));
            // t1 -> legacy (skipped) -> None
            assert_eq!(resolve_previous_summary_id(&turns, "t1"), None);
        }

        #[test]
        fn resolve_previous_summary_walks_past_multiple_legacy_entries() {
            let turns = vec![
                turn("legacy1", None, true, ""),
                turn("legacy2", Some("legacy1"), true, ""),
                turn("real", Some("legacy2"), false, "msg-real"),
                turn("current", Some("real"), false, "msg-current"),
            ];
            assert_eq!(
                resolve_previous_summary_id(&turns, "current"),
                Some("msg-real")
            );
        }

        #[test]
        fn resolve_previous_summary_handles_broken_chain_pointer() {
            // current points to a non-existent prev id.
            let turns = vec![turn("current", Some("ghost"), false, "msg-current")];
            assert_eq!(resolve_previous_summary_id(&turns, "current"), None);
        }

        #[test]
        fn resolve_previous_summary_bounds_chain_depth_against_cycles() {
            // Cycle: a -> b -> a -> ... ; should bail at MAX_CHAIN_DEPTH.
            let turns = vec![
                turn("a", Some("b"), false, "msg-a"),
                turn("b", Some("a"), false, "msg-b"),
            ];
            // From "a" we walk: prev=b (real, summary "msg-b") -> return immediately.
            assert_eq!(resolve_previous_summary_id(&turns, "a"), Some("msg-b"));
            // If we artificially mark them legacy, the chain loops forever
            // until the cap kicks in and returns None.
            let turns_legacy = vec![
                turn("a", Some("b"), true, ""),
                turn("b", Some("a"), true, ""),
            ];
            assert_eq!(resolve_previous_summary_id(&turns_legacy, "a"), None);
        }
    }
}

/// M48-C5: overflow replay candidate selection + media-to-text fallback.
///
/// Mirrors the replay-prep portion of opencode `processCompaction`:
///
/// When a compaction event is triggered by a hard context-overflow error
/// (not a routine ratio check), the message that *caused* the overflow is
/// usually the most recent user prompt. Opencode captures that message
/// for replay so the user does not lose their question, then strips media
/// attachments down to text labels because oversized media is the most
/// common overflow cause.
///
/// This stage adds pure helpers only:
/// - `find_replay_candidate` returns the index + cloned content of the
///   most recent **non-compaction-marker** user message strictly before
///   the parent index. Returns `None` when no such message exists.
/// - `prepare_replay_blocks` strips compaction markers and rewrites
///   media blocks into `Text` placeholders so the replay payload no
///   longer carries the original attachment bytes.
/// - `is_replay_safe` checks the residual head (everything before the
///   captured replay) for at least one usable user-led turn. Mirrors
///   opencode's `hasContent` guard: when stripping out the replay would
///   leave nothing summarizable, the replay is dropped and the auto-
///   continue path is taken instead.
///
/// The actual session mutation (creating the synthetic replay user
/// message, persisting overflow=true on `StoredCompactionTurn`, calling
/// the compaction agent) is wired in C-4b/C-7. Keeping this layer pure
/// lets us unit-test the precise rule that controls whether a replay
/// happens at all.
pub mod m48_overflow {
    use super::{ContentBlock, Message, Role};

    /// Text placeholder for stripped image attachments. Format mirrors
    /// opencode `[Attached image/png: filename]` style so traces remain
    /// recognizable across the two codebases.
    pub fn media_text_label(media_type: &str) -> String {
        format!("[Attached {media_type}: replaced during compaction]")
    }

    /// True when this user message is *only* a compaction marker (no real
    /// human text). M48-C2 introduced the same heuristic via
    /// `is_compaction_marker`; we duplicate it here so this module does
    /// not need a back-reference into `m48_select`.
    fn is_compaction_marker(msg: &Message) -> bool {
        if msg.role != Role::User || msg.content.is_empty() {
            return false;
        }
        msg.content
            .iter()
            .all(|b| matches!(b, ContentBlock::OpenAICompaction { .. }))
    }

    /// Captured replay payload: the index in the original message log
    /// and a deep clone of the user message's content blocks so the
    /// caller can re-emit it under a fresh message id without aliasing
    /// the original.
    #[derive(Debug, Clone)]
    pub struct ReplayCandidate {
        /// Index of the source message in the original log.
        pub index: usize,
        /// Cloned content blocks ready for replay. Compaction markers
        /// are NOT yet stripped here so the caller can decide whether
        /// to skip them. Use `prepare_replay_blocks` to get the
        /// strip+rewrite output.
        pub content: Vec<ContentBlock>,
    }

    /// Find the most recent user message strictly before `parent_index`
    /// that is **not** a compaction marker. Walks backwards from
    /// `parent_index - 1`. Returns `None` when the head has no such
    /// message (first user prompt, or every prior user message is a
    /// compaction marker).
    pub fn find_replay_candidate(
        messages: &[Message],
        parent_index: usize,
    ) -> Option<ReplayCandidate> {
        if parent_index == 0 || parent_index > messages.len() {
            return None;
        }
        for i in (0..parent_index).rev() {
            let msg = &messages[i];
            if msg.role != Role::User {
                continue;
            }
            if is_compaction_marker(msg) {
                continue;
            }
            return Some(ReplayCandidate {
                index: i,
                content: msg.content.clone(),
            });
        }
        None
    }

    /// Rewrite content blocks for replay: drop compaction markers and
    /// replace media attachments (Image blocks) with short text labels.
    /// Pure function — does not consume the input. Mirrors opencode's
    /// per-part replay loop where `compaction` parts are skipped and
    /// `MessageV2.isMedia(part.mime)` parts become text labels.
    pub fn prepare_replay_blocks(blocks: &[ContentBlock]) -> Vec<ContentBlock> {
        let mut out: Vec<ContentBlock> = Vec::with_capacity(blocks.len());
        for block in blocks {
            match block {
                ContentBlock::OpenAICompaction { .. } => continue,
                ContentBlock::Image { media_type, .. } => {
                    out.push(ContentBlock::Text {
                        text: media_text_label(media_type),
                        cache_control: None,
                    });
                }
                other => out.push(other.clone()),
            }
        }
        out
    }

    /// True when the head slice that remains after extracting a replay
    /// candidate still contains at least one usable user-led turn that
    /// can be summarized. Mirrors opencode's `hasContent` guard.
    ///
    /// `head` is `messages[..replay.index]` (the messages BEFORE the
    /// replay candidate). When this returns false the caller should
    /// drop the replay and fall through to the auto-continue path.
    pub fn is_replay_safe(head: &[Message]) -> bool {
        head.iter().any(|m| m.role == Role::User && !is_compaction_marker(m))
    }

    /// One-shot helper: given the full message log and the parent index
    /// of the overflow-triggering user message, return the cloned blocks
    /// ready to be re-emitted as a new replay user message, plus the
    /// length of the residual head that should be fed to the compaction
    /// agent. Returns `None` when no safe replay exists.
    pub fn plan_overflow_replay(
        messages: &[Message],
        parent_index: usize,
    ) -> Option<(Vec<ContentBlock>, usize)> {
        let candidate = find_replay_candidate(messages, parent_index)?;
        let head = &messages[..candidate.index];
        if !is_replay_safe(head) {
            return None;
        }
        let prepared = prepare_replay_blocks(&candidate.content);
        Some((prepared, candidate.index))
    }

    #[cfg(test)]
    mod overflow_tests {
        use super::super::m48_fixtures;
        use super::*;
        use jcode_message_types::ContentBlock;

        #[test]
        fn media_text_label_is_human_readable() {
            assert_eq!(
                media_text_label("image/png"),
                "[Attached image/png: replaced during compaction]"
            );
        }

        #[test]
        fn find_replay_candidate_returns_previous_user_message() {
            // short_session: user, assistant, user, assistant
            let msgs = m48_fixtures::short_session();
            // parent_index = 2 (the second user message); replay should be
            // index 0 (the first user message).
            let cand = find_replay_candidate(&msgs, 2).unwrap();
            assert_eq!(cand.index, 0);
            assert!(matches!(cand.content[0], ContentBlock::Text { .. }));
        }

        #[test]
        fn find_replay_candidate_skips_compaction_markers() {
            let mut msgs = m48_fixtures::short_session();
            // Replace the first user message with a compaction marker.
            msgs[0] = Message {
                role: Role::User,
                content: vec![ContentBlock::OpenAICompaction {
                    encrypted_content: "x".to_string(),
                }],
                timestamp: msgs[0].timestamp,
                tool_duration_ms: None,
            };
            // No real prior user message exists -> None.
            assert!(find_replay_candidate(&msgs, 2).is_none());
        }

        #[test]
        fn find_replay_candidate_returns_none_at_index_zero() {
            let msgs = m48_fixtures::short_session();
            assert!(find_replay_candidate(&msgs, 0).is_none());
        }

        #[test]
        fn find_replay_candidate_returns_none_past_end() {
            let msgs = m48_fixtures::short_session();
            assert!(find_replay_candidate(&msgs, msgs.len() + 5).is_none());
        }

        #[test]
        fn prepare_replay_blocks_strips_compaction_markers() {
            let blocks = vec![
                ContentBlock::Text {
                    text: "hello".to_string(),
                    cache_control: None,
                },
                ContentBlock::OpenAICompaction {
                    encrypted_content: "x".to_string(),
                },
            ];
            let out = prepare_replay_blocks(&blocks);
            assert_eq!(out.len(), 1);
            assert!(matches!(out[0], ContentBlock::Text { .. }));
        }

        #[test]
        fn prepare_replay_blocks_rewrites_images_to_text_labels() {
            let blocks = vec![
                ContentBlock::Text {
                    text: "describe this".to_string(),
                    cache_control: None,
                },
                ContentBlock::Image {
                    media_type: "image/png".to_string(),
                    data: "AAA".to_string(),
                },
            ];
            let out = prepare_replay_blocks(&blocks);
            assert_eq!(out.len(), 2);
            // Second block should now be a text placeholder mentioning the
            // media type but NOT the original payload.
            match &out[1] {
                ContentBlock::Text { text, .. } => {
                    assert!(text.contains("image/png"));
                    assert!(!text.contains("AAA"));
                }
                other => panic!("expected text, got {other:?}"),
            }
        }

        #[test]
        fn is_replay_safe_requires_real_user_turn_in_head() {
            let msgs = m48_fixtures::short_session();
            // Head before index 2 has a real user turn at index 0.
            assert!(is_replay_safe(&msgs[..2]));
            // Empty head is unsafe.
            assert!(!is_replay_safe(&msgs[..0]));
        }

        #[test]
        fn is_replay_safe_treats_compaction_only_head_as_unsafe() {
            let head = vec![Message {
                role: Role::User,
                content: vec![ContentBlock::OpenAICompaction {
                    encrypted_content: "x".to_string(),
                }],
                timestamp: Some(chrono::Utc::now()),
                tool_duration_ms: None,
            }];
            assert!(!is_replay_safe(&head));
        }

        #[test]
        fn plan_overflow_replay_returns_prepared_blocks_and_head_len() {
            let msgs = m48_fixtures::short_session();
            // parent index = 2; replay candidate is index 0.
            // Head before index 0 is empty -> NOT safe; returns None.
            assert!(plan_overflow_replay(&msgs, 2).is_none());
        }

        #[test]
        fn plan_overflow_replay_succeeds_when_head_has_real_user_turn() {
            // Build: user0, assistant0, user1, assistant1, user2 (the
            // overflow-causing parent). Replay candidate = user1 (index 2),
            // head before index 2 = [user0, assistant0] which is safe.
            let mut msgs = m48_fixtures::short_session();
            msgs.push(m48_fixtures::user_text(3, "newest overflow prompt"));
            let parent_idx = msgs.len() - 1;
            let (blocks, head_len) = plan_overflow_replay(&msgs, parent_idx).unwrap();
            assert_eq!(head_len, 2);
            assert!(!blocks.is_empty());
            assert!(matches!(blocks[0], ContentBlock::Text { .. }));
        }
    }
}

/// M48-C6: OpenAI native compaction coexistence helpers.
///
/// Jcode keeps two parallel summary representations once an OpenAI Responses
/// API session has been native-compacted:
/// 1. The provider-side opaque `encrypted_content` blob, which lets a
///    follow-up Responses turn skip resending the entire history.
/// 2. A plain-text Markdown anchored summary (M48-C4a), which is what
///    every non-OpenAI provider, the session search index, the export
///    tooling, and human reviewers actually read.
///
/// This module formalizes the *precedence rules* between those two so that
/// every caller (provider request builder, session export, replay path)
/// makes the same decision instead of re-inventing the logic. The rules
/// mirror the existing `discard_oversized_openai_native_compaction`
/// behavior in `src/compaction.rs` plus the safe / hard limits already
/// shipped in `jcode-provider-openai::request`:
///
/// - Active provider is OpenAI Responses AND encrypted blob length is
///   within the safe limit -> use the native blob, suppress the text
///   summary in the provider payload to save tokens.
/// - Active provider is OpenAI but the blob is oversized -> drop the
///   blob, fall back to the text summary, and emit a one-line
///   diagnostic so the caller can log it.
/// - Active provider is anything else (Anthropic, Gemini, OpenRouter,
///   etc.) -> always use the text summary; the encrypted blob is
///   meaningless to them.
/// - Search / export contexts -> always use the text summary; encrypted
///   bytes never leave the OpenAI request path.
///
/// The decision is exposed as a pure function returning a small enum so
/// callers can pattern-match without recomputing thresholds. Constants
/// are passed in (rather than reaching into `jcode-provider-openai`)
/// because `jcode-compaction-core` must stay provider-agnostic; the
/// provider crate is responsible for plugging its own limits in.
pub mod m48_native {
    /// Which representation of an anchored summary a caller should use
    /// for the current provider payload.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum SummaryRepresentation {
        /// Use the OpenAI provider-native `encrypted_content` blob; do
        /// not also include the text summary so the request stays
        /// compact. Carries the blob's length for telemetry only.
        Native { encrypted_content_len: usize },
        /// Use the plain-text summary. Set when the active provider is
        /// not OpenAI, or when the OpenAI blob would exceed the safe
        /// limit. `dropped_native_len` is `Some(n)` when we also had to
        /// discard an oversized blob (so the caller can log it).
        Text { dropped_native_len: Option<usize> },
        /// Neither representation is available. Callers should resend
        /// the verbatim head messages or trigger another compaction.
        None,
    }

    /// Whether the active provider can replay an OpenAI Responses
    /// `encrypted_content` payload. This is a tiny tagged enum so we
    /// do not have to import provider crates into `compaction-core`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ProviderKind {
        /// OpenAI Responses-compatible provider (gpt-5, gpt-5.5,
        /// gpt-5.1, etc). The only kind that can consume native blobs.
        OpenAIResponses,
        /// Anthropic Messages API.
        Anthropic,
        /// Google Gemini.
        Gemini,
        /// OpenRouter (Chat Completions; no native compaction support).
        OpenRouter,
        /// Any other provider; treated like a non-OpenAI provider.
        Other,
    }

    impl ProviderKind {
        /// True when this provider's request shape accepts
        /// `encrypted_content` items. Today only the OpenAI Responses
        /// API does; future expansion happens here.
        pub fn supports_native_encrypted_content(self) -> bool {
            matches!(self, ProviderKind::OpenAIResponses)
        }
    }

    /// Decide which summary representation to use for the current
    /// provider payload.
    ///
    /// `safe_max_chars` is the per-request safe ceiling for the
    /// encrypted blob (today: `OPENAI_ENCRYPTED_CONTENT_SAFE_MAX_CHARS`
    /// from `jcode-provider-openai::request`). Callers in non-OpenAI
    /// paths can pass any value; we only consult it when the provider
    /// is OpenAI and the blob is `Some`.
    pub fn decide_summary_representation(
        provider: ProviderKind,
        encrypted_content: Option<&str>,
        text_summary: Option<&str>,
        safe_max_chars: usize,
    ) -> SummaryRepresentation {
        let has_text = text_summary.map(|s| !s.trim().is_empty()).unwrap_or(false);

        // Non-OpenAI providers (and search/export) never get the blob.
        if !provider.supports_native_encrypted_content() {
            return if has_text {
                SummaryRepresentation::Text { dropped_native_len: None }
            } else {
                SummaryRepresentation::None
            };
        }

        // OpenAI path: prefer native when sendable.
        match encrypted_content {
            Some(blob) if blob.len() <= safe_max_chars => SummaryRepresentation::Native {
                encrypted_content_len: blob.len(),
            },
            Some(blob) => {
                // Oversized: drop blob, fall back to text.
                if has_text {
                    SummaryRepresentation::Text {
                        dropped_native_len: Some(blob.len()),
                    }
                } else {
                    SummaryRepresentation::None
                }
            }
            None => {
                // No blob at all (first compaction event, or already
                // discarded). Use text if present.
                if has_text {
                    SummaryRepresentation::Text { dropped_native_len: None }
                } else {
                    SummaryRepresentation::None
                }
            }
        }
    }

    /// True when the active provider can keep a previously stored
    /// encrypted blob across the upcoming request. Used by the
    /// compaction runtime to decide whether to retain or discard
    /// `Session.compaction.openai_encrypted_content` when the provider
    /// changes mid-session.
    ///
    /// Mirrors the precedence rule: only OpenAI Responses can keep it;
    /// switching to any other provider must drop it (callers may still
    /// retain it on disk for forensic export, but the active in-memory
    /// snapshot should treat it as unavailable for the next request).
    pub fn provider_can_consume_blob(provider: ProviderKind) -> bool {
        provider.supports_native_encrypted_content()
    }

    /// Convert a provider id string ("anthropic", "openai", "gemini",
    /// "openrouter", ...) into a `ProviderKind`. Case-insensitive.
    /// Unknown providers fall through to `Other`. Kept here so callers
    /// do not have to maintain duplicate match statements.
    pub fn classify_provider_id(provider_id: &str) -> ProviderKind {
        match provider_id.to_ascii_lowercase().as_str() {
            // The chat-completions transport never goes through the
            // Responses API; only the Responses path consumes blobs.
            "openai" | "openai-responses" => ProviderKind::OpenAIResponses,
            "anthropic" | "claude" => ProviderKind::Anthropic,
            "gemini" | "google" => ProviderKind::Gemini,
            "openrouter" => ProviderKind::OpenRouter,
            _ => ProviderKind::Other,
        }
    }

    #[cfg(test)]
    mod native_tests {
        use super::*;

        const SAFE: usize = 9_500_000;

        #[test]
        fn provider_kind_only_openai_supports_native_blob() {
            assert!(ProviderKind::OpenAIResponses.supports_native_encrypted_content());
            assert!(!ProviderKind::Anthropic.supports_native_encrypted_content());
            assert!(!ProviderKind::Gemini.supports_native_encrypted_content());
            assert!(!ProviderKind::OpenRouter.supports_native_encrypted_content());
            assert!(!ProviderKind::Other.supports_native_encrypted_content());
        }

        #[test]
        fn classify_provider_id_handles_known_aliases() {
            assert_eq!(classify_provider_id("openai"), ProviderKind::OpenAIResponses);
            assert_eq!(classify_provider_id("OpenAI"), ProviderKind::OpenAIResponses);
            assert_eq!(classify_provider_id("openai-responses"), ProviderKind::OpenAIResponses);
            assert_eq!(classify_provider_id("anthropic"), ProviderKind::Anthropic);
            assert_eq!(classify_provider_id("CLAUDE"), ProviderKind::Anthropic);
            assert_eq!(classify_provider_id("gemini"), ProviderKind::Gemini);
            assert_eq!(classify_provider_id("google"), ProviderKind::Gemini);
            assert_eq!(classify_provider_id("openrouter"), ProviderKind::OpenRouter);
            assert_eq!(classify_provider_id("groq"), ProviderKind::Other);
            assert_eq!(classify_provider_id(""), ProviderKind::Other);
        }

        #[test]
        fn openai_with_sendable_blob_returns_native() {
            let blob = "x".repeat(100_000);
            let result = decide_summary_representation(
                ProviderKind::OpenAIResponses,
                Some(&blob),
                Some("text summary"),
                SAFE,
            );
            assert_eq!(
                result,
                SummaryRepresentation::Native { encrypted_content_len: 100_000 }
            );
        }

        #[test]
        fn openai_with_oversized_blob_falls_back_to_text_with_dropped_len() {
            let blob = "x".repeat(SAFE + 1);
            let result = decide_summary_representation(
                ProviderKind::OpenAIResponses,
                Some(&blob),
                Some("text summary"),
                SAFE,
            );
            assert_eq!(
                result,
                SummaryRepresentation::Text { dropped_native_len: Some(SAFE + 1) }
            );
        }

        #[test]
        fn openai_with_oversized_blob_and_no_text_returns_none() {
            let blob = "x".repeat(SAFE + 1);
            let result = decide_summary_representation(
                ProviderKind::OpenAIResponses,
                Some(&blob),
                None,
                SAFE,
            );
            assert_eq!(result, SummaryRepresentation::None);
        }

        #[test]
        fn openai_without_blob_uses_text() {
            let result = decide_summary_representation(
                ProviderKind::OpenAIResponses,
                None,
                Some("text summary"),
                SAFE,
            );
            assert_eq!(
                result,
                SummaryRepresentation::Text { dropped_native_len: None }
            );
        }

        #[test]
        fn anthropic_with_blob_still_uses_text() {
            let blob = "x".repeat(100);
            let result = decide_summary_representation(
                ProviderKind::Anthropic,
                Some(&blob),
                Some("text summary"),
                SAFE,
            );
            assert_eq!(
                result,
                SummaryRepresentation::Text { dropped_native_len: None }
            );
        }

        #[test]
        fn anthropic_with_no_text_returns_none() {
            let result = decide_summary_representation(
                ProviderKind::Anthropic,
                None,
                None,
                SAFE,
            );
            assert_eq!(result, SummaryRepresentation::None);
        }

        #[test]
        fn whitespace_only_text_summary_is_ignored() {
            let result = decide_summary_representation(
                ProviderKind::Gemini,
                None,
                Some("   \n  "),
                SAFE,
            );
            assert_eq!(result, SummaryRepresentation::None);
        }

        #[test]
        fn provider_can_consume_blob_matches_supports_helper() {
            for p in [
                ProviderKind::OpenAIResponses,
                ProviderKind::Anthropic,
                ProviderKind::Gemini,
                ProviderKind::OpenRouter,
                ProviderKind::Other,
            ] {
                assert_eq!(provider_can_consume_blob(p), p.supports_native_encrypted_content());
            }
        }
    }
}

/// M48-C7: compaction-state diagnostics for TUI and debug overlays.
///
/// Provides one structured summary of the compaction subsystem that
/// every UI surface can render the same way. Without this layer, each
/// TUI component invents its own "% used / chars / turns" calculation
/// and they drift apart, which is what made the original emergency
/// compaction so hard to debug.
///
/// This module owns only the rendering shape. The numbers themselves
/// come from existing types (`CompactionStats` in `src/compaction.rs`,
/// `StoredCompactionTurn` from M48-C1, prune reports from M48-C3,
/// `SummaryRepresentation` from M48-C6a). C-7b will wire the actual
/// TUI panel + debug socket command.
pub mod m48_diagnostics {
    use super::m48_native::SummaryRepresentation;
    use super::m48_prune::PruneReport;

    /// One-line summary of a compaction marker / summary pair, suitable
    /// for the TUI context info popup. Each field maps to a concrete
    /// source so reviewers can grep for the origin.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct CompactionTurnDigest {
        /// `StoredCompactionTurn.id` (sidecar from M48-C1).
        pub turn_id: String,
        /// `StoredCompactionTurn.marker_message_id` or empty when this
        /// turn was reconstructed from the legacy `Session.compaction`
        /// field (backfill from M48-C1).
        pub marker_message_id: String,
        /// `StoredCompactionTurn.summary_message_id` or empty for legacy
        /// backfills.
        pub summary_message_id: String,
        /// `StoredCompactionTurn.tail_start_id` (the message id where
        /// the verbatim tail begins; M48-C2 produced this index).
        pub tail_start_id: Option<String>,
        /// Synthetic legacy backfill flag (from M48-C1).
        pub backfilled_from_legacy: bool,
        /// True when the compaction was triggered by a hard overflow
        /// rather than a routine ratio check (M48-C1 schema field).
        pub overflow: bool,
        /// Whether a chained `previous_summary_id` exists; useful for
        /// "anchored summary chain length" displays.
        pub has_previous_summary: bool,
    }

    /// One-line summary of the OpenAI native compaction state, suitable
    /// for the same popup. Derived from `m48_native::decide_summary_representation`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct NativeStateDigest {
        /// Provider id string from the active config (lowercased). UI
        /// shows this raw so users can compare against their config.
        pub provider_id: String,
        /// Result of the latest precedence decision. UI converts this
        /// into a short label: "native (N kB)", "text (dropped native NkB)",
        /// "text", or "none".
        pub representation: SummaryRepresentation,
    }

    /// Aggregate digest. UI panels render this struct directly so the
    /// numbers stay consistent across the TUI context popup, the debug
    /// memory profile, and any future export tooling.
    #[derive(Debug, Clone, PartialEq)]
    pub struct CompactionDiagnostics {
        /// Token usage ratio in `[0.0, 1.0]` (1.0 = full context).
        pub context_usage_ratio: f32,
        /// Effective (post-compaction) input token estimate.
        pub effective_tokens: usize,
        /// Total active (un-compacted) message count.
        pub active_messages: usize,
        /// All durable compaction turns currently known to the session.
        /// Empty when the session has never been compacted.
        pub turns: Vec<CompactionTurnDigest>,
        /// Last prune pass result, if any prune has run since session load.
        pub last_prune: Option<PruneReport>,
        /// OpenAI native vs text precedence for the *next* request.
        pub native_state: Option<NativeStateDigest>,
    }

    impl CompactionDiagnostics {
        /// Single-line header used by the TUI status bar. Format:
        /// `"ctx N% | M msgs | K turns compacted | native|text|none"`.
        /// All numbers are floor'd; ratios are shown as integer %.
        pub fn one_line_header(&self) -> String {
            let pct = (self.context_usage_ratio.clamp(0.0, 1.0) * 100.0) as u32;
            let kind = match self.native_state.as_ref().map(|s| &s.representation) {
                Some(SummaryRepresentation::Native { .. }) => "native",
                Some(SummaryRepresentation::Text { .. }) => "text",
                Some(SummaryRepresentation::None) => "none",
                None => "—",
            };
            format!(
                "ctx {pct}% | {} msgs | {} turns compacted | {kind}",
                self.active_messages,
                self.turns.len()
            )
        }

        /// Multi-line popup body. Mirrors what `/status` would show; one
        /// line per turn plus a tail block for the prune report and
        /// native state. UI components are free to truncate but should
        /// keep this exact ordering so the numbers do not drift.
        pub fn multi_line_body(&self) -> String {
            use std::fmt::Write as _;
            let mut out = String::new();
            let _ = writeln!(out, "{}", self.one_line_header());
            let _ = writeln!(out, "effective tokens: {}", self.effective_tokens);
            let _ = writeln!(out, "active messages: {}", self.active_messages);
            if self.turns.is_empty() {
                let _ = writeln!(out, "compaction turns: (none)");
            } else {
                let _ = writeln!(out, "compaction turns:");
                for (i, t) in self.turns.iter().enumerate() {
                    let _ = writeln!(
                        out,
                        "  [{i}] id={} marker={} summary={} tail_start={:?} legacy={} overflow={} chained={}",
                        t.turn_id,
                        if t.marker_message_id.is_empty() { "—" } else { &t.marker_message_id },
                        if t.summary_message_id.is_empty() { "—" } else { &t.summary_message_id },
                        t.tail_start_id,
                        t.backfilled_from_legacy,
                        t.overflow,
                        t.has_previous_summary,
                    );
                }
            }
            if let Some(p) = self.last_prune {
                let _ = writeln!(
                    out,
                    "last prune: blocks={} bytes={} committed={}",
                    p.blocks_pruned, p.bytes_recovered, p.committed
                );
            }
            if let Some(n) = &self.native_state {
                let label = match &n.representation {
                    SummaryRepresentation::Native { encrypted_content_len } => {
                        format!("native ({} bytes)", encrypted_content_len)
                    }
                    SummaryRepresentation::Text { dropped_native_len: Some(n) } => {
                        format!("text (dropped native {} bytes)", n)
                    }
                    SummaryRepresentation::Text { dropped_native_len: None } => {
                        "text".to_string()
                    }
                    SummaryRepresentation::None => "none".to_string(),
                };
                let _ = writeln!(out, "native state ({}): {}", n.provider_id, label);
            }
            out
        }
    }

    #[cfg(test)]
    mod diagnostics_tests {
        use super::super::m48_native::{ProviderKind, SummaryRepresentation};
        use super::super::m48_prune::PruneReport;
        use super::*;

        fn sample_turn(i: usize, legacy: bool, overflow: bool, chained: bool) -> CompactionTurnDigest {
            CompactionTurnDigest {
                turn_id: format!("turn-{i}"),
                marker_message_id: if legacy { String::new() } else { format!("marker-{i}") },
                summary_message_id: if legacy { String::new() } else { format!("summary-{i}") },
                tail_start_id: Some(format!("tail-{i}")),
                backfilled_from_legacy: legacy,
                overflow,
                has_previous_summary: chained,
            }
        }

        #[test]
        fn one_line_header_formats_percent_and_counts() {
            let diag = CompactionDiagnostics {
                context_usage_ratio: 0.42,
                effective_tokens: 12_000,
                active_messages: 18,
                turns: vec![sample_turn(1, false, false, false)],
                last_prune: None,
                native_state: None,
            };
            assert_eq!(
                diag.one_line_header(),
                "ctx 42% | 18 msgs | 1 turns compacted | —"
            );
        }

        #[test]
        fn one_line_header_clamps_ratio() {
            let diag = CompactionDiagnostics {
                context_usage_ratio: 1.7,
                effective_tokens: 0,
                active_messages: 0,
                turns: vec![],
                last_prune: None,
                native_state: None,
            };
            assert_eq!(
                diag.one_line_header(),
                "ctx 100% | 0 msgs | 0 turns compacted | —"
            );
        }

        #[test]
        fn one_line_header_labels_native_vs_text_vs_none() {
            for (rep, expected_suffix) in [
                (SummaryRepresentation::Native { encrypted_content_len: 1 }, "native"),
                (SummaryRepresentation::Text { dropped_native_len: None }, "text"),
                (SummaryRepresentation::None, "none"),
            ] {
                let diag = CompactionDiagnostics {
                    context_usage_ratio: 0.0,
                    effective_tokens: 0,
                    active_messages: 0,
                    turns: vec![],
                    last_prune: None,
                    native_state: Some(NativeStateDigest {
                        provider_id: "openai".to_string(),
                        representation: rep,
                    }),
                };
                let header = diag.one_line_header();
                assert!(
                    header.ends_with(expected_suffix),
                    "expected header to end with {expected_suffix:?}, got {header:?}"
                );
            }
        }

        #[test]
        fn multi_line_body_renders_no_turns_marker() {
            let diag = CompactionDiagnostics {
                context_usage_ratio: 0.0,
                effective_tokens: 0,
                active_messages: 0,
                turns: vec![],
                last_prune: None,
                native_state: None,
            };
            let body = diag.multi_line_body();
            assert!(body.contains("compaction turns: (none)"));
            assert!(!body.contains("last prune:"));
            assert!(!body.contains("native state"));
        }

        #[test]
        fn multi_line_body_renders_legacy_and_real_turns() {
            let diag = CompactionDiagnostics {
                context_usage_ratio: 0.5,
                effective_tokens: 5_000,
                active_messages: 7,
                turns: vec![
                    sample_turn(1, true, false, false),
                    sample_turn(2, false, true, true),
                ],
                last_prune: Some(PruneReport {
                    blocks_pruned: 3,
                    bytes_recovered: 25_000,
                    committed: true,
                }),
                native_state: Some(NativeStateDigest {
                    provider_id: "openai".to_string(),
                    representation: SummaryRepresentation::Text {
                        dropped_native_len: Some(10_485_760),
                    },
                }),
            };
            let body = diag.multi_line_body();
            // Legacy turn renders empty marker/summary as em-dash.
            assert!(body.contains("marker=— summary=—"));
            // Real turn renders ids.
            assert!(body.contains("marker=marker-2 summary=summary-2"));
            // Prune line.
            assert!(body.contains("last prune: blocks=3 bytes=25000 committed=true"));
            // Native fallback line with dropped len.
            assert!(body.contains("native state (openai): text (dropped native 10485760 bytes)"));
            // Overflow + chained flags are visible.
            assert!(body.contains("overflow=true"));
            assert!(body.contains("chained=true"));
        }

        #[test]
        fn multi_line_body_renders_native_in_use_label() {
            let diag = CompactionDiagnostics {
                context_usage_ratio: 0.0,
                effective_tokens: 0,
                active_messages: 0,
                turns: vec![],
                last_prune: None,
                native_state: Some(NativeStateDigest {
                    provider_id: "openai".to_string(),
                    representation: SummaryRepresentation::Native {
                        encrypted_content_len: 4096,
                    },
                }),
            };
            let body = diag.multi_line_body();
            assert!(body.contains("native state (openai): native (4096 bytes)"));
        }

        #[test]
        fn one_line_header_with_no_turns_and_no_native_state() {
            // Smoke test: defaults compile and produce a sane string.
            let diag = CompactionDiagnostics {
                context_usage_ratio: 0.0,
                effective_tokens: 0,
                active_messages: 0,
                turns: vec![],
                last_prune: None,
                native_state: None,
            };
            // Helps consumers depend on a stable empty-state header.
            assert_eq!(
                diag.one_line_header(),
                "ctx 0% | 0 msgs | 0 turns compacted | —"
            );
            // Make sure all referenced types still link.
            let _ = ProviderKind::Other;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_compaction_prompt_with_summary_and_truncated_tool_result() {
        let summary = Summary {
            text: "prior work".to_string(),
            openai_encrypted_content: None,
            covers_up_to_turn: 1,
            original_turn_count: 1,
        };
        let message = Message::user("hello");
        let prompt = build_compaction_prompt(&[message], Some(&summary), 10_000);
        assert!(prompt.contains("## Previous Summary"));
        assert!(prompt.contains("prior work"));
        assert!(prompt.contains("**User:**"));
        assert!(prompt.contains(SUMMARY_PROMPT));
    }

    #[test]
    fn truncates_on_utf8_boundary() {
        assert_eq!(truncate_str_boundary("éabc", 1), "");
        assert_eq!(truncate_str_boundary("éabc", 2), "é");
    }

    #[test]
    fn mean_embedding_is_normalized() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let mean = mean_embedding(&[&a, &b], 2);
        let norm = (mean[0] * mean[0] + mean[1] * mean[1]).sqrt();
        assert!((norm - 1.0).abs() < 0.0001);
    }

    #[test]
    fn safe_cutoff_keeps_tool_use_with_tool_result() {
        let tool_use = Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "call_1".to_string(),
                name: "read".to_string(),
                input: serde_json::json!({"file":"src/lib.rs"}),
            }],
            timestamp: None,
            tool_duration_ms: None,
        };
        let tool_result = Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "call_1".to_string(),
                content: "ok".to_string(),
                is_error: None,
            }],
            timestamp: None,
            tool_duration_ms: None,
        };
        let messages = vec![
            Message::user("old"),
            tool_use,
            tool_result,
            Message::user("new"),
        ];

        assert_eq!(safe_compaction_cutoff(&messages, 2), 1);
    }

    #[test]
    fn estimates_tokens_with_large_budget_overhead() {
        let summary = Summary {
            text: "abcd".repeat(100),
            openai_encrypted_content: None,
            covers_up_to_turn: 1,
            original_turn_count: 1,
        };

        assert_eq!(estimate_compaction_tokens(Some(&summary), 0, 1000), 100);
        assert_eq!(
            estimate_compaction_tokens(Some(&summary), 0, DEFAULT_TOKEN_BUDGET),
            100 + SYSTEM_OVERHEAD_TOKENS
        );
    }

    #[test]
    fn builds_semantic_text_from_relevant_content() {
        let message = Message {
            role: Role::User,
            content: vec![
                ContentBlock::Text {
                    text: "hello world".to_string(),
                    cache_control: None,
                },
                ContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: "tool output".to_string(),
                    is_error: None,
                },
            ],
            timestamp: None,
            tool_duration_ms: None,
        };

        assert_eq!(semantic_message_text(&message), "hello world");
        assert_eq!(semantic_goal_text(&[message]), "hello world tool output");
        assert_eq!(semantic_cache_key("stable"), semantic_cache_key("stable"));
    }

    #[test]
    fn builds_emergency_summary_with_tools_and_files() {
        let messages = vec![
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "read".to_string(),
                    input: serde_json::json!({"file":"src/lib.rs"}),
                }],
                timestamp: None,
                tool_duration_ms: None,
            },
            Message::user("Edited src/compaction.rs and Cargo.toml, ignored https://example.com"),
        ];

        let summary =
            build_emergency_summary_text(Some("previous"), 2, 201_000, 200_000, &messages);
        assert!(summary.contains("previous"));
        assert!(summary.contains("2 messages were dropped"));
        assert!(summary.contains("Tools used: read"));
        assert!(summary.contains("Files referenced: Cargo.toml, src/compaction.rs"));
        assert!(!summary.contains("https://example.com"));
    }

    #[test]
    fn native_openai_encrypted_payload_does_not_count_as_prompt_tokens() {
        let summary = Summary {
            text: "visible summary".repeat(300),
            openai_encrypted_content: Some("x".repeat(8_100_000)),
            covers_up_to_turn: 100,
            original_turn_count: 100,
        };

        let estimated = estimate_compaction_tokens(Some(&summary), 0, DEFAULT_TOKEN_BUDGET);

        assert!(
            estimated < 25_000,
            "encrypted provider replay payload must not be estimated as prompt tokens: {estimated}"
        );
        assert_eq!(
            estimated,
            summary.text.len() / CHARS_PER_TOKEN + SYSTEM_OVERHEAD_TOKENS
        );
    }

    #[test]
    fn emergency_truncation_is_utf8_safe() {
        let original = format!("{}middle{}", "é".repeat(20), "尾".repeat(20));
        let truncated = emergency_truncated_tool_result(&original, 25);
        assert!(truncated.contains("chars truncated for context recovery"));
        assert!(truncated.is_char_boundary(truncated.len()));
    }
}
