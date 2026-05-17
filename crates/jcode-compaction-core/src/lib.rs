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
