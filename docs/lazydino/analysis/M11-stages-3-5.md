# M11 Analysis: Stages 3-5

## Stage 3: `stop_hook_active` loop guard

### Current payload

- File: `src/hooks.rs:35` — `SessionStopHookPayload`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct SessionStopHookPayload<'a> {
    pub event: &'a str,
    pub session_id: &'a str,
    pub working_dir: Option<String>,
    pub reason: &'a str,
    pub message_count: usize,
}
```

- File: `src/hooks.rs:44` — `ResponseCompletedHookPayload`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct ResponseCompletedHookPayload<'a> {
    pub event: &'a str,
    pub session_id: &'a str,
    pub message_id: &'a str,
    pub working_dir: Option<String>,
    pub stop_reason: Option<&'a str>,
    pub tool_calls_count: usize,
    pub output_chars: usize,
}
```

- Re-entrance flag exists? **No.** No `stop_hook_active`, `hook_active`, or lifecycle re-entry state exists in payloads or dispatch.

### Dispatch path

- File: `src/hooks.rs:100`

```rust
pub async fn run_session_hooks(payload: SessionStopHookPayload<'_>) -> Result<()> {
    run_lifecycle_hooks(SESSION_STOP, payload.working_dir.as_deref(), &payload).await
}

pub async fn run_response_hooks(payload: ResponseCompletedHookPayload<'_>) -> Result<()> {
    run_lifecycle_hooks(RESPONSE_COMPLETED, payload.working_dir.as_deref(), &payload).await
}
```

- File: `src/hooks.rs:182`

```rust
async fn run_lifecycle_hooks<T>(event: &'static str, cwd: Option<&str>, payload: &T) -> Result<()>
where
    T: Serialize + ?Sized,
{
    let hooks = config().hooks_for_working_dir(cwd.map(std::path::Path::new));
```

- File: `src/hooks.rs:208`

```rust
async fn run_lifecycle_hook_commands<T>(
    matching: Vec<crate::config::HookCommandConfig>,
    cwd: Option<&str>,
    payload: &T,
) -> Result<()>
```

### Recursion guards

- There is **no explicit recursion guard** in `run_session_hooks`, `run_response_hooks`, `run_lifecycle_hooks`, or `run_lifecycle_hook_commands`.
- Lifecycle hooks differ from tool hooks: blocking lifecycle hook stdout is ignored, not parsed as a decision.
- File: `src/hooks.rs:302`

```rust
async fn run_blocking_lifecycle_hook(
    command: &str,
    timeout_ms: u64,
    cwd: Option<&str>,
    payload_json: &[u8],
) -> Result<()> {
```

- File: `src/hooks.rs:312`

```rust
if !output.status.success() {
    return Err(anyhow!(
        "hook command exited with status {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    ));
}
Ok(())
```

- Therefore, **non-empty stdout alone cannot trigger turns in this implementation**. The risk is via hook side effects, such as invoking jcode externally, enqueueing messages, causing disconnects, or closing sessions.

### Can re-trigger?

- `response.completed` fires at logical end of a turn.
- File: `src/agent/turn_loops.rs:652`

```rust
self.fire_response_completed_hook(
    assistant_message_id.as_deref(),
    stop_reason.as_deref(),
    assistant_tool_calls_count,
    text_content.chars().count(),
)
.await;
```

- File: `src/agent/turn_streaming_mpsc.rs:665`

```rust
self.fire_response_completed_hook(
    assistant_message_id.as_deref(),
    stop_reason.as_deref(),
    assistant_tool_calls_count,
    text_content.chars().count(),
)
.await;
```

- Multiple chained turns can later re-enter `run_lifecycle_hook_commands` because every completed turn fires `response.completed`.
- There is no per-session “currently in stop/response hook” state, so a hook script that causes another completed response can observe no built-in re-entry signal.

### Recursion risk scenarios

- A `response.completed` hook calls back into the server and posts a message, producing another response, then another `response.completed`.
- A `session.stop` hook causes reconnect/disconnect or session-close side effects that re-enter disconnect cleanup for another connection/session.
- Non-blocking lifecycle hooks are spawned and can overlap with later turns because `tokio::spawn` is used at `src/hooks.rs:235`.
- Blocking lifecycle hooks do not parse stdout, but they still can perform external side effects.

### Patch design, terse

- Add `stop_hook_active: bool` to `SessionStopHookPayload`.
- Consider also `hook_active` or `response_hook_active` for `ResponseCompletedHookPayload` if M11 wants parity.
- Default false at normal call sites.
- Track lifecycle activity with a small per-task or per-session guard around `run_lifecycle_hooks`.
- When dispatching `session.stop` or `response.completed` while already active, set payload flag true instead of suppressing.
- Approx: 20-40 lines plus golden serialization test updates.

---

## Stage 4: `session.stop` vs `client.disconnect`

### Call sites of `SESSION_STOP`

1. File: `src/hooks.rs:13`

```rust
pub const SESSION_STOP: &str = "session.stop";
pub const RESPONSE_COMPLETED: &str = "response.completed";
```

2. File: `src/hooks.rs:100`

```rust
pub async fn run_session_hooks(payload: SessionStopHookPayload<'_>) -> Result<()> {
    run_lifecycle_hooks(SESSION_STOP, payload.working_dir.as_deref(), &payload).await
}
```

3. File: `src/server/client_disconnect_cleanup.rs:54`

```rust
let payload = crate::hooks::SessionStopHookPayload {
    event: crate::hooks::SESSION_STOP,
    session_id,
    working_dir,
    reason,
    message_count,
};
```

4. Tests only:
   - `src/hooks.rs:388` golden `session.stop` payload test.
   - `src/server/client_disconnect_cleanup.rs:306` tests disconnect disposition maps to hook reason.

### Client disconnect handling

- File: `src/server/client_lifecycle.rs:1043`

```rust
n = reader.read_line(&mut line) => {
    let n = match n {
        Ok(n) => n,
        Err(e) => {
            crate::logging::error(&format!("Client read error: {}", e));
            break;
        }
    };
    if n == 0 {
        break; // Client disconnected
    }
```

- File: `src/server/client_lifecycle.rs:1140`

```rust
disconnect_signal = disconnect_rx.recv() => {
    if disconnect_signal.is_some() {
        crate::logging::info(&format!(
            "Client connection {} was superseded; disconnecting old owner of session {}",
            client_connection_id, client_session_id
        ));
        break;
    }
```

- File: `src/server/client_lifecycle.rs:2575`

```rust
cleanup_client_connection(
    &sessions,
    &client_session_id,
    client_is_processing,
    &mut processing_task,
    event_handle,
```

- File: `src/server/client_disconnect_cleanup.rs:40`

```rust
fn session_stop_hook_reason(disposition: DisconnectDisposition) -> Option<&'static str> {
    match disposition {
        DisconnectDisposition::Closed => Some("disconnect"),
        DisconnectDisposition::Crashed => Some("disconnect"),
        DisconnectDisposition::Reloading => None,
    }
}
```

- File: `src/server/client_disconnect_cleanup.rs:173`

```rust
if let Some(reason) = stop_hook_reason {
    fire_session_stop_hook(&sid, working_dir.clone(), reason, message_count)
        .await;
}
```

### Current split?

- **No split exists.**
- `session.stop` is only fired from client disconnect cleanup.
- There is no `CLIENT_DISCONNECT` constant and no `client.disconnect` event.
- Logical agent completion does **not** fire `session.stop`; normal turn completion fires `response.completed`.
- Client disconnects currently emit `session.stop` with `reason: "disconnect"`.
- Server reload disconnects intentionally skip hook firing via `DisconnectDisposition::Reloading => None`.

### Payload per call site

- Disconnect cleanup payload:
  - `event`: always `"session.stop"`.
  - `session_id`: `client_session_id`.
  - `working_dir`: from `agent.working_dir().map(|dir| dir.to_string())`, populated if agent lock succeeds.
  - `reason`: `"disconnect"` for closed or crashed disconnect.
  - `message_count`: `agent.messages().len()`.

### Patch design, terse

- Introduce `pub const CLIENT_DISCONNECT: &str = "client.disconnect";`.
- Add `ClientDisconnectHookPayload` or reuse lifecycle payload with distinct event and reason/disposition fields.
- Change `client_disconnect_cleanup.rs` to fire `CLIENT_DISCONNECT`, not `SESSION_STOP`.
- Reserve `SESSION_STOP` for explicit logical session stop/end events.
- Preserve reload skip behavior unless M11 wants `client.disconnect` for reloads too.
- Approx: 30-60 lines plus tests and config docs.

---

## Stage 5: payload context enrichment

### Current `ToolHookPayload` struct

File: `src/hooks.rs:16`

```rust
#[derive(Debug, Clone, Serialize)]
pub struct ToolHookPayload<'a> {
    pub event: &'a str,
    pub session_id: &'a str,
    pub message_id: &'a str,
    pub tool_call_id: &'a str,
    pub cwd: Option<String>,
    pub tool: ToolHookTool<'a>,
}
```

### Current `ToolHookTool` struct

File: `src/hooks.rs:26`

```rust
#[derive(Debug, Clone, Serialize)]
pub struct ToolHookTool<'a> {
    pub name: &'a str,
    pub args: &'a Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
}
```

### Current `SessionStopHookPayload` struct

File: `src/hooks.rs:34`

```rust
#[derive(Debug, Clone, Serialize)]
pub struct SessionStopHookPayload<'a> {
    pub event: &'a str,
    pub session_id: &'a str,
    pub working_dir: Option<String>,
    pub reason: &'a str,
    pub message_count: usize,
}
```

### Current `ResponseCompletedHookPayload` struct

File: `src/hooks.rs:43`

```rust
#[derive(Debug, Clone, Serialize)]
pub struct ResponseCompletedHookPayload<'a> {
    pub event: &'a str,
    pub session_id: &'a str,
    pub message_id: &'a str,
    pub working_dir: Option<String>,
    pub stop_reason: Option<&'a str>,
    pub tool_calls_count: usize,
    pub output_chars: usize,
}
```

### Fields populated at call sites

#### Tool hooks

- File: `src/hooks.rs:141`

```rust
let payload = ToolHookPayload {
    event,
    session_id: &ctx.session_id,
    message_id: &ctx.message_id,
    tool_call_id: &ctx.tool_call_id,
    cwd: cwd.clone(),
```

- File: `src/hooks.rs:147`

```rust
tool: ToolHookTool {
    name: tool_name,
    args: input,
    result,
},
```

- Populated:
  - `event`: before/after.
  - `session_id`: always from `ToolContext`.
  - `message_id`: always from `ToolContext`.
  - `tool_call_id`: always from `ToolContext`.
  - `cwd`: `Some` if `ctx.working_dir` exists.
  - `tool.result`: `None` before, `Some` after.

#### `response.completed`

- File: `src/agent/turn_loops.rs:19`

```rust
let payload = crate::hooks::ResponseCompletedHookPayload {
    event: crate::hooks::RESPONSE_COMPLETED,
    session_id: &self.session.id,
    message_id,
    working_dir: self.session.working_dir.clone(),
    stop_reason,
    tool_calls_count,
    output_chars,
};
```

- Populated:
  - `event`: always.
  - `session_id`: always.
  - `message_id`: only if `assistant_message_id` exists. Function returns early when absent.
  - `working_dir`: clone of session field, may be `None`.
  - `stop_reason`: provider stop reason, may be `None`.
  - counts: always passed by caller.

#### `session.stop`

- File: `src/server/client_disconnect_cleanup.rs:54`

```rust
let payload = crate::hooks::SessionStopHookPayload {
    event: crate::hooks::SESSION_STOP,
    session_id,
    working_dir,
    reason,
    message_count,
};
```

- Populated:
  - `event`: always `"session.stop"`.
  - `session_id`: always.
  - `working_dir`: from agent if agent lock succeeds, may be `None`.
  - `reason`: currently `"disconnect"`.
  - `message_count`: `agent.messages().len()`.

### Cheap additions

- `model` for response hooks:
  - Already in scope in `Agent`.
  - File: `src/agent/status.rs:137`

```rust
pub fn provider_model(&self) -> String {
    self.provider.model().to_string()
}
```

  - Also persisted as `self.session.model`.
  - File: `src/session.rs:84`

```rust
/// Model identifier for this session (e.g., "gpt-5.2-codex")
#[serde(default, skip_serializing_if = "Option::is_none")]
pub model: Option<String>,
```

- `provider` / provider profile key:
  - Runtime provider exists in `Agent`.
  - Env snapshot already uses it.
  - File: `src/agent/environment.rs:83`

```rust
provider: self.provider.name().to_string(),
model: self.provider.model().to_string(),
```

  - Persisted provider key exists.
  - File: `src/session.rs:80`

```rust
/// Stable provider/profile key for session-source filtering (e.g. "openai",
/// "opencode", "opencode-go").
#[serde(default, skip_serializing_if = "Option::is_none")]
pub provider_key: Option<String>,
```

- `session_path` and `session_journal_path`:
  - Existing APIs are cheap from session id.
  - File: `src/session/storage_paths.rs:30`

```rust
pub fn session_path(session_id: &str) -> Result<PathBuf> {
    let base = storage::jcode_dir()?;
    Ok(session_path_in_dir(&base, session_id))
}
```

  - File: `src/session/storage_paths.rs:44`

```rust
pub fn session_journal_path(session_id: &str) -> Result<PathBuf> {
    Ok(session_journal_path_from_snapshot(&session_path(
        session_id,
    )?))
}
```

- `working_dir_status`:
  - Current session working dir initially comes from process cwd.
  - File: `src/session.rs:213`

```rust
fn current_working_dir_string() -> Option<String> {
    std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}
```

  - Cheap to add status enum/string at payload construction:
    - `session_working_dir` when `Some`.
    - `missing` when `None`.
    - `fallback_cwd` only if code explicitly falls back to `std::env::current_dir()`.

- Project config root:
  - Hooks already resolve project-local config using nearest `.jcode/config*.toml`.
  - File: `src/config/config_file.rs:177`

```rust
for ancestor in start.ancestors() {
    let project_config = ancestor.join(".jcode").join("config.toml");
    let local_config = ancestor.join(".jcode").join("config.local.toml");
    if project_config.exists() || local_config.exists() {
        return Some(ancestor.to_path_buf());
    }
}
```

### Hard additions

- `agent profile name`:
  - No obvious active agent profile field on `Agent` or `Session`.
  - Markdown/config profile loading exists, but selected profile name is not stored in current hook scopes.
  - Would require plumbing from agent/subagent creation or session metadata.

- Tool hook `model` / `provider`:
  - `ToolContext` lacks model/provider.
  - File: `crates/jcode-tool-core/src/lib.rs:30`

```rust
pub struct ToolContext {
    pub session_id: String,
    pub message_id: String,
    pub tool_call_id: String,
    pub working_dir: Option<PathBuf>,
    pub stdin_request_tx: Option<tokio::sync::mpsc::UnboundedSender<StdinInputRequest>>,
    pub graceful_shutdown_signal: Option<InterruptSignal>,
    pub execution_mode: ToolExecutionMode,
}
```

  - Requires adding fields to `ToolContext` and updating many constructors.

- Exact transcript path:
  - Session snapshot and journal paths are easy.
  - A separate “transcript path” does not appear as a first-class field.
  - If M11 means persisted session log, use `session_path`/`session_journal_path`.
  - If it means exported transcript, that requires new export/log plumbing.

- Reusable project root resolution status:
  - Existing detection helpers are private and inconsistent:
    - config root detects `.jcode/config*.toml`.
    - doctor detects `.jcode`, `.claude`, `.agents`, `.opencode`.
  - A shared public helper would be cleaner than duplicating.

### Patch design, terse

- Add optional fields to lifecycle payloads:
  - `provider: Option<String>`
  - `model: Option<String>`
  - `provider_key: Option<String>`
  - `session_path: Option<String>`
  - `session_journal_path: Option<String>`
  - `working_dir_status: Option<String>`
  - `project_root: Option<String>`
- Populate `response.completed` from `Agent` directly in `fire_response_completed_hook`.
- Populate disconnect lifecycle from locked `Agent` before `drop(agent)`.
- Add tool hook enrichment only for fields already in `ToolContext`, or extend `ToolContext` in a separate patch.
- Approx: 40-80 lines for lifecycle enrichment, more if touching tool context constructors.

## Cumulative estimate

- Stages 3-5 together: ~90-180 lines.
- Tests:
  - golden payload updates in `src/hooks.rs`.
  - disconnect event split tests in `src/server/client_disconnect_cleanup.rs`.
  - possibly ToolContext constructor updates if tool payload enriched.
- Plus tests: ~30-50 lines.

<subagent_metadata>
session_id: session_orangutan_1778426867467_c6d9a0439e971682
output_mode: answer
</subagent_metadata>