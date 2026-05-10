# M11 Analysis: Stages 1-2

(searcher subagent, 1m40s, jcode-m11 worktree)

## Stage 1: Decision JSON Parsing

### Current code
- File: `src/hooks.rs:248-283` — `run_blocking_hook`
  - Executes blocking tool hook via `run_hook_command`.
  - Reads `output.stdout`.
  - Empty stdout means allow.
  - Non-empty stdout is parsed as decision JSON.
- Decision struct: `src/hooks.rs:54-68`
```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct HookDecision {
    action: String,
    reason: Option<String>,
}

impl Default for HookDecision {
    fn default() -> Self {
        Self {
            action: "allow".to_string(),
            reason: None,
        }
    }
}
```
- JSON parse call site: `src/hooks.rs:266-272`
```rust
let stdout = String::from_utf8_lossy(&output.stdout);
if stdout.trim().is_empty() {
    return Ok(());
}

let decision: HookDecision = serde_json::from_str(stdout.trim())
    .with_context(|| format!("invalid hook decision JSON from command: {command}"))?;
```
- Decision handling: `src/hooks.rs:273-282`
```rust
match decision.action.as_str() {
    "allow" | "" => Ok(()),
    "deny" => Err(anyhow!(
        "tool call denied by hook: {}",
        decision.reason.unwrap_or_else(|| "no reason provided".to_string())
    )),
    other => Err(anyhow!("unsupported hook action: {other}")),
}
```

### Callers of the blocking hook decision flow
1. `src/hooks.rs:70-72` — `pre_tool_use(...)`
   - Calls `run_tool_hooks(TOOL_EXECUTE_BEFORE, ...)`.
2. `src/hooks.rs:108-180` — `run_tool_hooks(...)`
   - Filters matching hook commands by event, non-empty command, and optional `tool` filter.
   - For `hook.blocking`, calls `run_blocking_hook(...)` at `src/hooks.rs:157-163`.
3. `src/tool/mod.rs:349-374` — `Registry::execute(...)`
   - Calls `crate::hooks::pre_tool_use(resolved_name, &input, &ctx).await?` at `src/tool/mod.rs:361`.
   - A deny/error aborts before actual tool execution and before `post_tool_use`.

### Existing tests
- `src/hooks.rs:372-376` — `hook_decision_defaults_to_allow`
  - Verifies `{}` defaults to `action == "allow"`.
- `src/hooks.rs:378-384` — `hook_decision_parses_deny_reason`
  - Verifies `{"action":"deny","reason":"blocked"}` parses.
- `src/hooks.rs:467-472` — `blocking_hook_allows_empty_stdout`
  - Verifies empty stdout allows.
- `src/hooks.rs:474-485` — `blocking_hook_denies_from_json_stdout`
  - Verifies blocking hook stdout deny returns an error containing `blocked`.
- `src/hooks.rs:519-529` — `lifecycle_blocking_hook_ignores_deny_decision`
  - Verifies lifecycle blocking hooks do not parse/deny on stdout JSON.
- No matching integration tests under `tests/` for hook deny/allow decisions were found.

### Gaps for M11 stage 1
- `HookDecision` is private and `action` is a raw `String`, not an enum.
- Parsing is lenient:
  - `#[serde(default)]` allows missing `action`.
  - Unknown JSON fields are accepted because `deny_unknown_fields` is not used.
  - Empty action string is treated as allow.
- Unsupported actions currently become hard errors: `unsupported hook action: {other}`.
- Action matching is case-sensitive and whitespace-sensitive after JSON parsing.
- No explicit tests for invalid JSON stdout, unknown fields, unsupported action, `{"action":"allow","reason":"..."}`, empty or missing `reason` on deny, non-string `reason`, multi-line/noisy stdout.

## Stage 2: Reason Injection

### Current flow
- Hook deny reason captured at: `src/hooks.rs:275-280`
  - Deny returns `anyhow!("tool call denied by hook: {reason}")`.
  - Missing reason becomes `"no reason provided"`.
- Surfaces to caller as:
  - `run_blocking_hook(...) -> Result<()>`
  - `run_tool_hooks(...) -> Result<()>`
  - `pre_tool_use(...) -> Result<()>`
  - `Registry::execute(...) -> Result<ToolOutput>`
- Tool layer consumes at: `src/tool/mod.rs:361`
```rust
crate::hooks::pre_tool_use(resolved_name, &input, &ctx).await?;
```
  - The `?` propagates the deny as an error.
  - The denied tool is not executed.
  - `post_tool_use` is not called.

### Reason → LLM message path
- Headless/non-streaming turn:
  - `src/agent/turn_loops.rs:823` calls `self.registry.execute(...)`.
  - `src/agent/turn_loops.rs:867-898` handles `Err(e)`.
  - `src/agent/turn_loops.rs:878` creates `let error_msg = format!("Error: {}", e);`.
  - `src/agent/turn_loops.rs:888-896` appends a user `ContentBlock::ToolResult`:
```rust
ContentBlock::ToolResult {
    tool_use_id: tc.id,
    content: error_msg,
    is_error: Some(true),
}
```
- Streaming broadcast turn: `src/agent/turn_streaming_broadcast.rs:820,830-833` (same pattern)
- Streaming mpsc turn: `src/agent/turn_streaming_mpsc.rs:921,931-934` (same pattern)
- Provider serialization: `src/provider/anthropic.rs:802-811` converts `ContentBlock::ToolResult` into Anthropic `ApiContentBlock::ToolResult`.
- Native provider tool calls: `src/agent/turn_loops.rs:471-474`, `turn_streaming_broadcast.rs:458-460`, `turn_streaming_mpsc.rs:465-468` send `NativeToolResult::error(request_id, e.to_string())`.

### Existing tests
- `src/hooks.rs:474-485` only verifies the lower-level hook error contains the deny reason.
- No tests verifying the deny reason is appended as a `ContentBlock::ToolResult`.
- No tests verifying the provider/LLM receives or sees the deny reason in context.
- No tests for streaming paths or native tool call paths.

### Gaps for M11 stage 2
- Reason is currently surfaced indirectly through a generic `anyhow::Error`.
- There is no typed `ToolError` / `ToolDenied` variant carrying `reason`.
- User/model-visible text is `"Error: tool call denied by hook: {reason}"`, not a dedicated denial message.
- Tests do not cover the full hook decision → tool error → message stream → provider context path.
- Direct/debug `Agent::execute_tool` at `src/agent/turn_execution.rs:319-340` returns the error to caller; it does not itself inject a tool-result message unless caller separately calls `add_manual_tool_error`.

## Patch design suggestion (terse)
- **Stage 1 patch**: ~40-70 lines, mostly `src/hooks.rs`.
  - Introduce `HookDecisionAction` enum or normalize action parsing.
  - Add explicit parser helper, e.g. `parse_hook_decision(stdout: &str) -> Result<HookDecision>`.
  - Add unit tests for invalid JSON, unsupported action, unknown fields policy, and missing reason.
- **Stage 2 patch**: ~50-100 lines, mostly `src/hooks.rs`, `src/tool/mod.rs`, and agent turn tests.
  - Prefer a typed denial error or helper predicate so denial is not just stringly typed.
  - Ensure all turn paths produce a `ToolResult { is_error: Some(true), content }` containing the reason.
- Suggested test additions:
  - `src/hooks.rs`: parser edge cases.
  - `src/agent_tests.rs` or turn-loop tests: configured `tool.execute.before` deny causes the next provider call/session messages to include the deny reason.
  - Streaming equivalent test if existing harness makes that cheap.
