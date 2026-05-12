use crate::config::config;
use crate::tool::{ToolContext, ToolOutput};
use crate::turn::bg_completion::parse_injection_format;
use crate::turn::injected_context::{
    InjectedContext, InjectedSource, InjectionFormat, enqueue_injection,
};
use crate::turn::pending_async_hooks::{
    PendingAsyncHookOutput, drain_all_within, drain_completed_within, register_pending_async_hook,
};
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

pub const TOOL_EXECUTE_BEFORE: &str = "tool.execute.before";
pub const TOOL_EXECUTE_AFTER: &str = "tool.execute.after";
/// Deprecated since M11 stage 4. The only call site that currently emits
/// this event is the client-disconnect cleanup path, which now also emits
/// the more specific `client.disconnect`. Existing user configurations
/// listening for `session.stop` continue to work, but new hooks should
/// listen for `client.disconnect` if they want to react to client teardown,
/// and `session.stop` will be reserved for a future explicit logical
/// session-end signal (no producer yet).
pub const SESSION_STOP: &str = "session.stop";
/// M11 stage 4: emitted when a client connection is torn down (closed or
/// crashed). The payload mirrors `SessionStopHookPayload` so users can
/// migrate by changing only the `event` field they filter on.
pub const CLIENT_DISCONNECT: &str = "client.disconnect";
pub const RESPONSE_COMPLETED: &str = "response.completed";

/// M10: tracker for non-blocking hook tasks so single-shot CLI commands
/// (`jcode run`, etc) can await pending lifecycle/tool hooks before the
/// tokio runtime is dropped. Without this, fire-and-forget `tokio::spawn`
/// calls in `run_tool_hooks` and `run_lifecycle_hook_commands` race against
/// process exit and the hook child is killed via `kill_on_drop(true)` before
/// it finishes.
fn pending_nonblocking_hooks() -> &'static Mutex<Vec<JoinHandle<()>>> {
    static SLOT: OnceLock<Mutex<Vec<JoinHandle<()>>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(Vec::new()))
}

#[cfg(test)]
fn spawn_tracked_nonblocking_hook<F>(future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let handle = tokio::spawn(future);
    let pending = pending_nonblocking_hooks();
    if let Ok(mut guard) = pending.try_lock() {
        guard.push(handle);
    } else {
        tokio::spawn(async move {
            pending_nonblocking_hooks().lock().await.push(handle);
        });
    }
}

/// M10: await all currently-tracked non-blocking hooks, then drop them.
///
/// Bounded by `timeout` so a slow / hung hook cannot wedge process exit
/// indefinitely. Any handle that does not complete within the timeout is
/// dropped (which triggers `kill_on_drop(true)` on the child process — same
/// behaviour as the pre-fix race, but at least the well-behaved hooks have a
/// chance to finish).
///
/// Safe to call multiple times. Safe to call when no hooks were ever
/// registered. Returns the number of hooks that completed within the timeout
/// (callers may surface this for diagnostics).
pub async fn flush_nonblocking_hooks(timeout: Duration) -> usize {
    let async_hook_count = drain_all_within(timeout).await;
    let handles: Vec<JoinHandle<()>> = {
        let mut guard = pending_nonblocking_hooks().lock().await;
        std::mem::take(&mut *guard)
    };
    if handles.is_empty() {
        return async_hook_count;
    }
    let total = handles.len();
    let join_all = async {
        for handle in handles {
            let _ = handle.await;
        }
    };
    match tokio::time::timeout(timeout, join_all).await {
        Ok(()) => total + async_hook_count,
        Err(_) => {
            crate::logging::warn(&format!(
                "flush_nonblocking_hooks: {total} hook(s) did not finish within {timeout:?}; remaining child processes will be killed on drop"
            ));
            // Outstanding handles are already moved out of the global slot;
            // dropping them here cancels the futures and kills the child via
            // kill_on_drop(true). That matches the pre-fix exit behaviour for
            // those specific slow hooks, but well-behaved hooks now finish
            // because we awaited them up to `timeout`.
            0
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolHookPayload<'a> {
    pub event: &'a str,
    pub session_id: &'a str,
    pub message_id: &'a str,
    pub tool_call_id: &'a str,
    pub cwd: Option<String>,
    pub tool: ToolHookTool<'a>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolHookTool<'a> {
    pub name: &'a str,
    pub args: &'a Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SessionStopHookPayload<'a> {
    pub event: &'a str,
    pub session_id: &'a str,
    pub working_dir: Option<String>,
    pub reason: &'a str,
    pub message_count: usize,
    /// M11 stage 5: optional context fields. Skipped from JSON when None so
    /// existing hook scripts continue to see the same payload shape they
    /// always did unless they opt in by reading these keys.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_user_message: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub recent_tool_calls: Vec<LifecycleHookToolCallPreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_age_seconds: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ResponseCompletedHookPayload<'a> {
    pub event: &'a str,
    pub session_id: &'a str,
    pub message_id: &'a str,
    pub working_dir: Option<String>,
    pub stop_reason: Option<&'a str>,
    pub tool_calls_count: usize,
    pub output_chars: usize,
    /// M11 stage 6: true when this response.completed hook is running from
    /// an immediate continuation turn caused by a previous lifecycle deny.
    /// Hook scripts can use this as the claude-code-compatible
    /// `stop_hook_active` self-throttle signal.
    #[serde(default)]
    pub stop_hook_active: bool,
    /// M11 stage 5: optional context fields. Skipped when not provided so
    /// the wire format stays backward compatible with stage 1-4 consumers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_user_message: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub recent_tool_calls: Vec<LifecycleHookToolCallPreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_age_seconds: Option<u64>,
}

/// M11 stage 5: compact tool-call summary embedded in lifecycle hook payloads.
/// `args_preview` is a one-line, truncated rendering of the tool input
/// (`LIFECYCLE_HOOK_TOOL_ARGS_PREVIEW_MAX` chars) so hook scripts can match
/// on what the agent actually did without inflating stdin.
#[derive(Debug, Clone, Serialize)]
pub struct LifecycleHookToolCallPreview {
    pub name: String,
    pub args_preview: String,
}

/// M11 stage 5: maximum number of recent tool calls embedded in a lifecycle
/// hook payload. Older entries are dropped first.
pub const LIFECYCLE_HOOK_RECENT_TOOL_CALLS_MAX: usize = 5;
/// M11 stage 5: maximum length (chars) of each `args_preview` string before
/// truncation. Keeps stdin small for blocking hooks with strict timeouts.
pub const LIFECYCLE_HOOK_TOOL_ARGS_PREVIEW_MAX: usize = 200;
/// M11 stage 5: maximum length (chars) of `last_user_message` before
/// truncation with a trailing ellipsis.
pub const LIFECYCLE_HOOK_LAST_USER_MESSAGE_MAX: usize = 500;

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct HookDecision {
    pub action: Option<String>,
    pub reason: Option<String>,
    pub inject: Option<HookInjectPayload>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct HookInjectPayload {
    pub body: String,
    #[serde(default)]
    pub format: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct HookOutputContext<'a> {
    session_id: &'a str,
    hook_kind: &'a str,
    tool_name: Option<&'a str>,
    command: &'a str,
}

pub async fn pre_tool_use(tool_name: &str, input: &Value, ctx: &ToolContext) -> Result<()> {
    run_tool_hooks(TOOL_EXECUTE_BEFORE, tool_name, input, None, ctx).await
}

pub async fn post_tool_use(
    tool_name: &str,
    input: &Value,
    output: &Result<ToolOutput>,
    ctx: &ToolContext,
) {
    let result = match output {
        Ok(output) => json!({
            "ok": true,
            "output": output.output,
            "title": output.title,
            "metadata": output.metadata,
            "image_count": output.images.len(),
        }),
        Err(err) => json!({
            "ok": false,
            "error": err.to_string(),
        }),
    };

    if let Err(err) = run_tool_hooks(TOOL_EXECUTE_AFTER, tool_name, input, Some(result), ctx).await
    {
        crate::logging::warn(&format!("post tool hook failed for {tool_name}: {err:#}"));
    }
}

pub async fn run_session_hooks(payload: SessionStopHookPayload<'_>) -> Result<Option<String>> {
    run_lifecycle_hooks(
        SESSION_STOP,
        payload.session_id,
        payload.working_dir.as_deref(),
        &payload,
    )
    .await
}

/// M11 stage 4: fire `client.disconnect` lifecycle hooks. The payload type
/// is shared with `session.stop` (only the `event` field differs) so users
/// can filter on either or both event names.
pub async fn run_client_disconnect_hooks(
    payload: SessionStopHookPayload<'_>,
) -> Result<Option<String>> {
    run_lifecycle_hooks(
        CLIENT_DISCONNECT,
        payload.session_id,
        payload.working_dir.as_deref(),
        &payload,
    )
    .await
}

pub async fn run_response_hooks(
    payload: ResponseCompletedHookPayload<'_>,
) -> Result<Option<String>> {
    run_lifecycle_hooks(
        RESPONSE_COMPLETED,
        payload.session_id,
        payload.working_dir.as_deref(),
        &payload,
    )
    .await
}

async fn run_tool_hooks(
    event: &'static str,
    tool_name: &str,
    input: &Value,
    result: Option<Value>,
    ctx: &ToolContext,
) -> Result<()> {
    let hooks = config().hooks_for_working_dir(ctx.working_dir.as_deref());
    if !hooks.enabled {
        return Ok(());
    }

    let matching: Vec<_> = hooks
        .commands
        .iter()
        .filter(|hook| hook.event == event)
        .filter(|hook| hook.command.trim().len() > 0)
        .filter(|hook| {
            hook.tool
                .as_deref()
                .is_none_or(|name| name == tool_name || name == "*")
        })
        .cloned()
        .collect();

    if matching.is_empty() {
        return Ok(());
    }

    let cwd = ctx
        .working_dir
        .as_ref()
        .map(|path| path.display().to_string());
    let payload = ToolHookPayload {
        event,
        session_id: &ctx.session_id,
        message_id: &ctx.message_id,
        tool_call_id: &ctx.tool_call_id,
        cwd: cwd.clone(),
        tool: ToolHookTool {
            name: tool_name,
            args: input,
            result,
        },
    };
    let payload_json = serde_json::to_vec(&payload)?;

    for hook in matching {
        if hook.blocking {
            run_blocking_hook(
                &hook.command,
                event,
                &ctx.session_id,
                Some(tool_name),
                hook.timeout_ms,
                cwd.as_deref(),
                &payload_json,
            )
            .await?;
        } else {
            let command = hook.command.clone();
            let timeout_ms = hook.timeout_ms;
            let cwd = cwd.clone();
            let payload_json = payload_json.clone();
            spawn_pending_nonblocking_hook(
                event,
                &ctx.session_id,
                Some(tool_name),
                command,
                timeout_ms,
                cwd,
                payload_json,
            );
        }
    }

    Ok(())
}

async fn run_lifecycle_hooks<T>(
    event: &'static str,
    session_id: &str,
    cwd: Option<&str>,
    payload: &T,
) -> Result<Option<String>>
where
    T: Serialize + ?Sized,
{
    let hooks = config().hooks_for_working_dir(cwd.map(std::path::Path::new));
    if !hooks.enabled {
        return Ok(None);
    }

    let matching = matching_lifecycle_hooks(&hooks.commands, event);

    run_lifecycle_hook_commands(event, session_id, matching, cwd, payload).await
}

fn matching_lifecycle_hooks(
    commands: &[crate::config::HookCommandConfig],
    event: &'static str,
) -> Vec<crate::config::HookCommandConfig> {
    commands
        .iter()
        .filter(|hook| hook.event == event)
        .filter(|hook| !hook.command.trim().is_empty())
        .cloned()
        .collect()
}

async fn run_lifecycle_hook_commands<T>(
    event: &'static str,
    session_id: &str,
    matching: Vec<crate::config::HookCommandConfig>,
    cwd: Option<&str>,
    payload: &T,
) -> Result<Option<String>>
where
    T: Serialize + ?Sized,
{
    if matching.is_empty() {
        return Ok(None);
    }

    let payload_json = serde_json::to_vec(payload)?;

    for hook in matching {
        if hook.blocking {
            match run_blocking_lifecycle_hook(
                &hook.command,
                event,
                session_id,
                hook.timeout_ms,
                cwd,
                &payload_json,
            )
            .await
            {
                Ok(Some(reason)) => return Ok(Some(reason)),
                Ok(None) => {}
                Err(err) => {
                    crate::logging::warn(&format!("blocking lifecycle hook failed: {err:#}"));
                }
            }
        } else {
            spawn_pending_nonblocking_hook(
                event,
                session_id,
                None,
                hook.command.clone(),
                hook.timeout_ms,
                cwd.map(str::to_string),
                payload_json.clone(),
            );
        }
    }

    Ok(None)
}

async fn run_blocking_hook(
    command: &str,
    hook_kind: &str,
    session_id: &str,
    tool_name: Option<&str>,
    timeout_ms: u64,
    cwd: Option<&str>,
    payload_json: &[u8],
) -> Result<()> {
    let output = run_hook_command(command, timeout_ms, cwd, payload_json)
        .await
        .with_context(|| format!("hook command failed: {command}"))?;

    if !output.status.success() {
        return Err(anyhow!(
            "hook command exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let Some(decision) = handle_hook_decision_stdout(
        HookOutputContext {
            session_id,
            hook_kind,
            tool_name,
            command,
        },
        &String::from_utf8_lossy(&output.stdout),
    )?
    else {
        return Ok(());
    };

    match hook_decision_denial_reason(decision)? {
        Some(reason) => Err(anyhow!("tool call denied by hook: {reason}")),
        None => Ok(()),
    }
}

fn parse_hook_decision_stdout(stdout: &str, command: &str) -> Result<Option<HookDecision>> {
    let stdout = stdout.trim();
    if stdout.is_empty() {
        return Ok(None);
    }

    let decision: HookDecision = serde_json::from_str(stdout)
        .with_context(|| format!("invalid hook decision JSON from command: {command}"))?;
    Ok(Some(decision))
}

fn hook_decision_denial_reason(decision: HookDecision) -> Result<Option<String>> {
    match decision.action.as_deref().unwrap_or("allow") {
        "allow" | "" => Ok(None),
        "deny" => Ok(Some(
            decision
                .reason
                .filter(|reason| !reason.trim().is_empty())
                .unwrap_or_else(|| "no reason provided".to_string()),
        )),
        other => Err(anyhow!("unsupported hook action: {other}")),
    }
}

fn handle_hook_decision_stdout(
    ctx: HookOutputContext<'_>,
    stdout: &str,
) -> Result<Option<HookDecision>> {
    let decision = parse_hook_decision_stdout(stdout, ctx.command)?;
    if let Some(decision) = decision.as_ref() {
        if let Some(payload) = decision.inject.as_ref() {
            enqueue_hook_injection(ctx, payload)?;
        }
    }
    Ok(decision)
}

fn enqueue_hook_injection(ctx: HookOutputContext<'_>, payload: &HookInjectPayload) -> Result<()> {
    let format = parse_hook_inject_format(payload.format.as_deref());
    let injection = InjectedContext {
        source: InjectedSource::LifecycleHookStdout {
            hook_kind: ctx.hook_kind.to_string(),
            tool_name: ctx.tool_name.map(ToString::to_string),
            stdout: payload.body.clone(),
        },
        timestamp: SystemTime::now(),
        format,
        dedupe_key: hook_inject_dedupe_key(ctx.hook_kind, ctx.tool_name, &payload.body),
    };
    enqueue_injection(ctx.session_id, injection)?;
    Ok(())
}

fn parse_hook_inject_format(raw: Option<&str>) -> InjectionFormat {
    parse_injection_format(raw)
}

fn hook_inject_dedupe_key(hook_kind: &str, tool_name: Option<&str>, body: &str) -> String {
    format!(
        "hook:{}:{}:{:016x}",
        hook_kind,
        tool_name.unwrap_or("_"),
        short_hash(body)
    )
}

fn short_hash(value: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

async fn run_blocking_lifecycle_hook(
    command: &str,
    hook_kind: &str,
    session_id: &str,
    timeout_ms: u64,
    cwd: Option<&str>,
    payload_json: &[u8],
) -> Result<Option<String>> {
    let output = run_hook_command(command, timeout_ms, cwd, payload_json)
        .await
        .with_context(|| format!("lifecycle hook command failed: {command}"))?;

    if !output.status.success() {
        return Err(anyhow!(
            "hook command exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let Some(decision) = handle_hook_decision_stdout(
        HookOutputContext {
            session_id,
            hook_kind,
            tool_name: None,
            command,
        },
        &String::from_utf8_lossy(&output.stdout),
    )?
    else {
        return Ok(None);
    };
    hook_decision_denial_reason(decision)
}

fn spawn_pending_nonblocking_hook(
    hook_kind: &'static str,
    session_id: &str,
    tool_name: Option<&str>,
    command: String,
    timeout_ms: u64,
    cwd: Option<String>,
    payload_json: Vec<u8>,
) {
    static NEXT_HOOK_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let hook_id = NEXT_HOOK_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let id = format!("hook-{hook_id}");
    let session_id_owned = session_id.to_string();
    let tool_name_owned = tool_name.map(ToString::to_string);
    let command_for_task = command.clone();
    let handle = tokio::spawn(async move {
        match run_hook_command(&command_for_task, timeout_ms, cwd.as_deref(), &payload_json).await {
            Ok(output) if output.status.success() => PendingAsyncHookOutput {
                session_id: session_id_owned,
                hook_kind: hook_kind.to_string(),
                tool_name: tool_name_owned,
                command: command_for_task,
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            },
            Ok(output) => {
                crate::logging::warn(&format!(
                    "non-blocking hook exited with status {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
                PendingAsyncHookOutput {
                    session_id: session_id_owned,
                    hook_kind: hook_kind.to_string(),
                    tool_name: tool_name_owned,
                    command: command_for_task,
                    stdout: String::new(),
                }
            }
            Err(err) => {
                crate::logging::warn(&format!("non-blocking hook failed: {err:#}"));
                PendingAsyncHookOutput {
                    session_id: session_id_owned,
                    hook_kind: hook_kind.to_string(),
                    tool_name: tool_name_owned,
                    command: command_for_task,
                    stdout: String::new(),
                }
            }
        }
    });
    register_pending_async_hook(session_id, id, handle);
}

pub async fn drain_pending_async_hook_injections(
    session_id: &str,
    timeout: Duration,
) -> Result<usize> {
    let completed = drain_completed_within(session_id, timeout).await;
    let count = completed.len();
    for (_id, output) in completed {
        handle_hook_decision_stdout(
            HookOutputContext {
                session_id: &output.session_id,
                hook_kind: &output.hook_kind,
                tool_name: output.tool_name.as_deref(),
                command: &output.command,
            },
            &output.stdout,
        )?;
    }
    Ok(count)
}

async fn run_hook_command(
    command: &str,
    timeout_ms: u64,
    cwd: Option<&str>,
    payload_json: &[u8],
) -> Result<std::process::Output> {
    let mut cmd = shell_command(command);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.kill_on_drop(true);

    let mut child = cmd.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload_json).await?;
    }

    let wait = child.wait_with_output();
    if timeout_ms == 0 {
        return wait.await.map_err(Into::into);
    }

    match tokio::time::timeout(Duration::from_millis(timeout_ms), wait).await {
        Ok(output) => output.map_err(Into::into),
        Err(_) => Err(anyhow!("hook command timed out after {timeout_ms}ms")),
    }
}

fn shell_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    }
    #[cfg(not(windows))]
    {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);
        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_decision_defaults_to_allow() {
        let decision: HookDecision = serde_json::from_str("{}").unwrap();
        assert!(decision.action.is_none());
        assert!(hook_decision_denial_reason(decision).unwrap().is_none());
    }

    #[test]
    fn hook_decision_parses_deny_reason() {
        let decision: HookDecision =
            serde_json::from_str(r#"{"action":"deny","reason":"blocked"}"#).unwrap();
        assert_eq!(decision.action.as_deref(), Some("deny"));
        assert_eq!(decision.reason.as_deref(), Some("blocked"));
    }

    #[test]
    fn hook_decision_stdout_empty_means_no_decision() {
        assert!(parse_hook_decision_stdout("  \n", "cmd").unwrap().is_none());
    }

    #[test]
    fn hook_decision_rejects_noisy_multiline_stdout() {
        let err = parse_hook_decision_stdout(
            "log line\n{\"action\":\"deny\",\"reason\":\"blocked\"}",
            "cmd",
        )
        .unwrap_err();

        assert!(err.to_string().contains("invalid hook decision JSON"));
    }

    #[test]
    fn hook_decision_unsupported_action_is_error() {
        let decision: HookDecision = serde_json::from_str(r#"{"action":"ask"}"#).unwrap();
        let err = hook_decision_denial_reason(decision).unwrap_err();

        assert_eq!(err.to_string(), "unsupported hook action: ask");
    }

    #[test]
    fn hook_decision_deny_without_reason_uses_fallback() {
        let decision: HookDecision = serde_json::from_str(r#"{"action":"deny"}"#).unwrap();

        assert_eq!(
            hook_decision_denial_reason(decision).unwrap().as_deref(),
            Some("no reason provided")
        );
    }

    fn unique_hook_session(prefix: &str) -> String {
        static NEXT_SESSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        format!(
            "{prefix}-{}",
            NEXT_SESSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )
    }

    #[test]
    fn hook_decision_parses_inject_field() {
        let decision: HookDecision = serde_json::from_str(
            r#"{"inject":{"body":"hello from hook","format":"system_reminder"}}"#,
        )
        .unwrap();
        let inject = decision.inject.unwrap();
        assert_eq!(inject.body, "hello from hook");
        assert_eq!(inject.format.as_deref(), Some("system_reminder"));
    }

    #[test]
    fn hook_decision_inject_format_defaults_to_system_reminder() {
        let decision: HookDecision =
            serde_json::from_str(r#"{"inject":{"body":"hello"}}"#).unwrap();
        let inject = decision.inject.unwrap();
        assert_eq!(
            parse_hook_inject_format(inject.format.as_deref()),
            InjectionFormat::SystemReminder
        );
    }

    #[test]
    fn hook_decision_inject_user_message_format_parses() {
        let decision: HookDecision =
            serde_json::from_str(r#"{"inject":{"body":"hello","format":"user_message"}}"#).unwrap();
        let inject = decision.inject.unwrap();
        assert_eq!(
            parse_hook_inject_format(inject.format.as_deref()),
            InjectionFormat::UserMessage
        );
    }

    #[test]
    fn hook_with_inject_enqueues_injected_context() {
        let session_id = unique_hook_session("hook-inject");
        handle_hook_decision_stdout(
            HookOutputContext {
                session_id: &session_id,
                hook_kind: TOOL_EXECUTE_AFTER,
                tool_name: Some("bash"),
                command: "cmd",
            },
            r#"{"inject":{"body":"hook body","format":"system_reminder"}}"#,
        )
        .unwrap();

        let drained = crate::turn::injected_context::drain_injections(&session_id).unwrap();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].format, InjectionFormat::SystemReminder);
        assert_eq!(
            drained[0].source,
            InjectedSource::LifecycleHookStdout {
                hook_kind: TOOL_EXECUTE_AFTER.to_string(),
                tool_name: Some("bash".to_string()),
                stdout: "hook body".to_string(),
            }
        );
    }

    #[test]
    fn hook_without_inject_does_not_enqueue_hook_inject() {
        let session_id = unique_hook_session("hook-no-inject");
        handle_hook_decision_stdout(
            HookOutputContext {
                session_id: &session_id,
                hook_kind: TOOL_EXECUTE_AFTER,
                tool_name: Some("bash"),
                command: "cmd",
            },
            r#"{"action":"allow"}"#,
        )
        .unwrap();

        let drained = crate::turn::injected_context::drain_injections(&session_id).unwrap();
        assert!(drained.is_empty());
    }

    #[tokio::test]
    async fn async_hook_completion_within_timeout_drains_and_injects_pending_async_hook() {
        let session_id = unique_hook_session("async-hook-done");
        crate::turn::pending_async_hooks::clear_pending_async_hooks(&session_id);
        let output_session = session_id.clone();
        let handle = tokio::spawn(async move {
            PendingAsyncHookOutput {
                session_id: output_session,
                hook_kind: RESPONSE_COMPLETED.to_string(),
                tool_name: None,
                command: "cmd".to_string(),
                stdout: r#"{"inject":{"body":"async body"}}"#.to_string(),
            }
        });
        register_pending_async_hook(&session_id, "hook-1".to_string(), handle);

        let drained_count =
            drain_pending_async_hook_injections(&session_id, Duration::from_millis(50))
                .await
                .unwrap();
        assert_eq!(drained_count, 1);

        let drained = crate::turn::injected_context::drain_injections(&session_id).unwrap();
        assert_eq!(drained.len(), 1);
        assert!(drained[0].render().contains("async body"));
    }

    #[tokio::test]
    async fn async_hook_completion_past_timeout_stays_pending_async_hook() {
        let session_id = unique_hook_session("async-hook-pending");
        crate::turn::pending_async_hooks::clear_pending_async_hooks(&session_id);
        let output_session = session_id.clone();
        let handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            PendingAsyncHookOutput {
                session_id: output_session,
                hook_kind: RESPONSE_COMPLETED.to_string(),
                tool_name: None,
                command: "cmd".to_string(),
                stdout: r#"{"inject":{"body":"late body"}}"#.to_string(),
            }
        });
        register_pending_async_hook(&session_id, "hook-1".to_string(), handle);

        let drained_count =
            drain_pending_async_hook_injections(&session_id, Duration::from_millis(1))
                .await
                .unwrap();
        assert_eq!(drained_count, 0);
        assert_eq!(
            crate::turn::pending_async_hooks::pending_count(&session_id),
            1
        );

        tokio::time::sleep(Duration::from_millis(120)).await;
        let _ = drain_pending_async_hook_injections(&session_id, Duration::from_millis(1)).await;
    }

    #[test]
    fn dedupe_key_uniqueness_for_repeated_hook_outputs_hook_inject() {
        let session_id = unique_hook_session("hook-dedupe");
        let stdout = r#"{"inject":{"body":"same body"}}"#;
        for _ in 0..2 {
            handle_hook_decision_stdout(
                HookOutputContext {
                    session_id: &session_id,
                    hook_kind: RESPONSE_COMPLETED,
                    tool_name: None,
                    command: "cmd",
                },
                stdout,
            )
            .unwrap();
        }

        let drained = crate::turn::injected_context::drain_injections(&session_id).unwrap();
        assert_eq!(drained.len(), 1);
        assert_eq!(
            drained[0].dedupe_key,
            hook_inject_dedupe_key(RESPONSE_COMPLETED, None, "same body")
        );
    }

    #[test]
    fn session_stop_payload_serializes_golden() {
        let payload = SessionStopHookPayload {
            event: SESSION_STOP,
            session_id: "sess-1",
            working_dir: Some("/tmp/work".to_string()),
            reason: "disconnect",
            message_count: 3,
            ..Default::default()
        };

        let value = serde_json::to_value(payload).unwrap();
        assert_eq!(
            value,
            json!({
                "event": "session.stop",
                "session_id": "sess-1",
                "working_dir": "/tmp/work",
                "reason": "disconnect",
                "message_count": 3,
            }),
            "M11 stage 5: optional context fields must be omitted (skip_serializing_if) when empty so existing hook scripts see the same wire format they always did"
        );
    }

    // M11 stage 5: golden test with context fields populated. Pins the new
    // wire format so hook authors know exactly what keys/types to expect.
    #[test]
    fn session_stop_payload_serializes_with_context_fields() {
        let payload = SessionStopHookPayload {
            event: CLIENT_DISCONNECT,
            session_id: "sess-1",
            working_dir: Some("/tmp/work".to_string()),
            reason: "disconnect",
            message_count: 3,
            last_user_message: Some("ship it".to_string()),
            recent_tool_calls: vec![LifecycleHookToolCallPreview {
                name: "bash".to_string(),
                args_preview: r#"{"command":"git status"}"#.to_string(),
            }],
            turn_count: Some(7),
            session_age_seconds: Some(123),
        };

        let value = serde_json::to_value(payload).unwrap();
        assert_eq!(
            value,
            json!({
                "event": "client.disconnect",
                "session_id": "sess-1",
                "working_dir": "/tmp/work",
                "reason": "disconnect",
                "message_count": 3,
                "last_user_message": "ship it",
                "recent_tool_calls": [
                    {"name": "bash", "args_preview": r#"{"command":"git status"}"#}
                ],
                "turn_count": 7,
                "session_age_seconds": 123,
            })
        );
    }

    // M11 stage 4 regression tests --------------------------------------------

    #[test]
    fn client_disconnect_const_has_expected_name() {
        // Pin the event name so user configurations don't break silently.
        assert_eq!(CLIENT_DISCONNECT, "client.disconnect");
        assert_eq!(SESSION_STOP, "session.stop");
    }

    #[test]
    fn client_disconnect_payload_uses_distinct_event_field() {
        // Same payload struct as session.stop but the event constant differs.
        let payload = SessionStopHookPayload {
            event: CLIENT_DISCONNECT,
            session_id: "sess-1",
            working_dir: Some("/tmp/work".to_string()),
            reason: "disconnect",
            message_count: 3,
            ..Default::default()
        };

        let value = serde_json::to_value(payload).unwrap();
        assert_eq!(value["event"], "client.disconnect");
        assert_eq!(value["reason"], "disconnect");
    }

    #[tokio::test]
    async fn client_disconnect_hooks_dispatch_under_correct_event_name() {
        // A hook registered for `client.disconnect` (and only for that event)
        // must fire when run_client_disconnect_hooks is called and must NOT
        // fire when run_session_hooks is called.
        let temp = tempfile::TempDir::new().unwrap();
        let log = temp.path().join("client-disconnect.log");
        let commands = vec![
            crate::config::HookCommandConfig {
                event: CLIENT_DISCONNECT.to_string(),
                tool: None,
                command: format!("cat >> {}", log.display()),
                blocking: true,
                timeout_ms: 1000,
            },
            crate::config::HookCommandConfig {
                event: SESSION_STOP.to_string(),
                tool: None,
                command: "true".to_string(),
                blocking: true,
                timeout_ms: 1000,
            },
        ];

        let payload = SessionStopHookPayload {
            event: CLIENT_DISCONNECT,
            session_id: "sess-disc",
            working_dir: None,
            reason: "disconnect",
            message_count: 1,
            ..Default::default()
        };

        let denial = run_lifecycle_hook_commands(
            CLIENT_DISCONNECT,
            payload.session_id,
            matching_lifecycle_hooks(&commands, CLIENT_DISCONNECT),
            None,
            &payload,
        )
        .await
        .unwrap();
        assert!(denial.is_none());

        let written = std::fs::read_to_string(&log).unwrap();
        let value: Value = serde_json::from_str(&written).unwrap();
        assert_eq!(value["event"], "client.disconnect");
        assert_eq!(value["session_id"], "sess-disc");
    }

    #[test]
    fn response_completed_payload_serializes_golden() {
        let payload = ResponseCompletedHookPayload {
            event: RESPONSE_COMPLETED,
            session_id: "sess-1",
            message_id: "msg-1",
            working_dir: None,
            stop_reason: Some("end_turn"),
            tool_calls_count: 0,
            output_chars: 42,
            ..Default::default()
        };

        let value = serde_json::to_value(payload).unwrap();
        assert_eq!(
            value,
            json!({
                "event": "response.completed",
                "session_id": "sess-1",
                "message_id": "msg-1",
                "working_dir": null,
                "stop_reason": "end_turn",
                "tool_calls_count": 0,
                "output_chars": 42,
                "stop_hook_active": false,
            }),
            "M11 stage 6: optional context fields stay omitted when empty, while stop_hook_active is always present so hook scripts can self-throttle continuation turns"
        );
    }

    // M11 stage 5: golden test with response.completed context fields
    // populated end-to-end. Pins the exact JSON keys/types hook authors
    // get when context is available.
    #[test]
    fn response_completed_payload_serializes_with_context_fields() {
        let payload = ResponseCompletedHookPayload {
            event: RESPONSE_COMPLETED,
            session_id: "sess-1",
            message_id: "msg-1",
            working_dir: Some("/tmp/work".to_string()),
            stop_reason: Some("end_turn"),
            tool_calls_count: 2,
            output_chars: 42,
            stop_hook_active: false,
            last_user_message: Some("commit and push".to_string()),
            recent_tool_calls: vec![
                LifecycleHookToolCallPreview {
                    name: "bash".to_string(),
                    args_preview: r#"{"command":"git add ."}"#.to_string(),
                },
                LifecycleHookToolCallPreview {
                    name: "bash".to_string(),
                    args_preview: r#"{"command":"git commit -m fix"}"#.to_string(),
                },
            ],
            turn_count: Some(3),
            session_age_seconds: Some(900),
        };

        let value = serde_json::to_value(payload).unwrap();
        assert_eq!(value["last_user_message"], "commit and push");
        assert_eq!(value["recent_tool_calls"].as_array().unwrap().len(), 2);
        assert_eq!(value["recent_tool_calls"][0]["name"], "bash");
        assert_eq!(value["turn_count"], 3);
        assert_eq!(value["session_age_seconds"], 900);
    }

    #[test]
    fn response_completed_payload_serializes_stop_hook_active() {
        let payload = ResponseCompletedHookPayload {
            event: RESPONSE_COMPLETED,
            session_id: "sess-1",
            message_id: "msg-1",
            working_dir: None,
            stop_reason: Some("end_turn"),
            tool_calls_count: 0,
            output_chars: 42,
            stop_hook_active: true,
            ..Default::default()
        };

        let value = serde_json::to_value(payload).unwrap();
        assert_eq!(value["stop_hook_active"], true);
    }

    #[test]
    fn lifecycle_hook_matching_ignores_tool_filter() {
        let commands = vec![
            crate::config::HookCommandConfig {
                event: RESPONSE_COMPLETED.to_string(),
                tool: Some("bash".to_string()),
                command: "true".to_string(),
                blocking: true,
                timeout_ms: 1000,
            },
            crate::config::HookCommandConfig {
                event: TOOL_EXECUTE_BEFORE.to_string(),
                tool: None,
                command: "true".to_string(),
                blocking: true,
                timeout_ms: 1000,
            },
            crate::config::HookCommandConfig {
                event: RESPONSE_COMPLETED.to_string(),
                tool: None,
                command: "   ".to_string(),
                blocking: true,
                timeout_ms: 1000,
            },
        ];

        let matching = matching_lifecycle_hooks(&commands, RESPONSE_COMPLETED);
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].tool.as_deref(), Some("bash"));
    }

    #[tokio::test]
    async fn blocking_hook_allows_empty_stdout() {
        run_blocking_hook(
            "cat >/dev/null",
            TOOL_EXECUTE_BEFORE,
            "sess-1",
            Some("bash"),
            1000,
            None,
            br#"{"ok":true}"#,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn blocking_hook_denies_from_json_stdout() {
        let err = run_blocking_hook(
            "cat >/dev/null; printf '%s' '{\"action\":\"deny\",\"reason\":\"blocked\"}'",
            TOOL_EXECUTE_BEFORE,
            "sess-1",
            Some("bash"),
            1000,
            None,
            br#"{"ok":true}"#,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("blocked"));
    }

    #[tokio::test]
    async fn lifecycle_hook_fires_matching_command() {
        let temp = tempfile::TempDir::new().unwrap();
        let out = temp.path().join("payload.json");
        let command = format!("cat > {}", out.display());
        let commands = vec![crate::config::HookCommandConfig {
            event: RESPONSE_COMPLETED.to_string(),
            tool: Some("ignored".to_string()),
            command,
            blocking: true,
            timeout_ms: 1000,
        }];
        let payload = ResponseCompletedHookPayload {
            event: RESPONSE_COMPLETED,
            session_id: "sess-1",
            message_id: "msg-1",
            working_dir: None,
            stop_reason: Some("end_turn"),
            tool_calls_count: 0,
            output_chars: 5,
            ..Default::default()
        };

        let denial = run_lifecycle_hook_commands(
            RESPONSE_COMPLETED,
            payload.session_id,
            commands,
            None,
            &payload,
        )
        .await
        .unwrap();
        assert!(denial.is_none());

        let written = std::fs::read_to_string(out).unwrap();
        let value: Value = serde_json::from_str(&written).unwrap();
        assert_eq!(value["event"], RESPONSE_COMPLETED);
        assert_eq!(value["message_id"], "msg-1");
    }

    #[tokio::test]
    async fn lifecycle_blocking_hook_returns_deny_reason() {
        let denial = run_blocking_lifecycle_hook(
            "cat >/dev/null; printf '%s' '{\"action\":\"deny\",\"reason\":\"ignored\"}'",
            RESPONSE_COMPLETED,
            "sess-1",
            1000,
            None,
            br#"{"ok":true}"#,
        )
        .await
        .unwrap();

        assert_eq!(denial.as_deref(), Some("ignored"));
    }

    #[tokio::test]
    async fn lifecycle_hook_commands_stop_after_deny() {
        let temp = tempfile::TempDir::new().unwrap();
        let marker = temp.path().join("ran-after-deny");
        let commands = vec![
            crate::config::HookCommandConfig {
                event: RESPONSE_COMPLETED.to_string(),
                tool: None,
                command:
                    "cat >/dev/null; printf '%s' '{\"action\":\"deny\",\"reason\":\"blocked\"}'"
                        .to_string(),
                blocking: true,
                timeout_ms: 1000,
            },
            crate::config::HookCommandConfig {
                event: RESPONSE_COMPLETED.to_string(),
                tool: None,
                command: format!("touch {}", marker.display()),
                blocking: true,
                timeout_ms: 1000,
            },
        ];
        let payload = ResponseCompletedHookPayload {
            event: RESPONSE_COMPLETED,
            session_id: "sess-1",
            message_id: "msg-1",
            working_dir: None,
            stop_reason: Some("end_turn"),
            tool_calls_count: 0,
            output_chars: 5,
            ..Default::default()
        };

        let denial = run_lifecycle_hook_commands(
            RESPONSE_COMPLETED,
            payload.session_id,
            commands,
            None,
            &payload,
        )
        .await
        .unwrap();

        assert_eq!(denial.as_deref(), Some("blocked"));
        assert!(!marker.exists());
    }

    // ─────────────────────────────────────────────────────────────────────
    // M10 regression: pending non-blocking hook flush.
    //
    // Tests use a serial mutex because `pending_nonblocking_hooks` is a
    // process-global singleton; concurrent tokio tests would otherwise see
    // each other's handles and `flush_nonblocking_hooks` returns counts that
    // depend on global state.
    // ─────────────────────────────────────────────────────────────────────

    static M10_GLOBAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// M10: a tracked non-blocking hook that finishes within the flush
    /// timeout must be awaited (return value reflects it as completed).
    /// Without the flush call, `tokio::spawn` would race against runtime drop
    /// in single-shot CLI commands and the hook child would be killed.
    #[tokio::test]
    async fn flush_nonblocking_hooks_awaits_tracked_handle() {
        let _serial = M10_GLOBAL.lock().await;
        // Drain any leftover handles from previous tests in the same process.
        let _ = flush_nonblocking_hooks(Duration::from_millis(50)).await;

        let marker = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let marker_for_task = marker.clone();
        spawn_tracked_nonblocking_hook(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            marker_for_task.store(true, std::sync::atomic::Ordering::SeqCst);
        });

        let completed = flush_nonblocking_hooks(Duration::from_secs(2)).await;
        assert_eq!(completed, 1, "flush should report 1 completed hook");
        assert!(
            marker.load(std::sync::atomic::Ordering::SeqCst),
            "M10: tracked hook side-effect must run before flush returns"
        );
    }

    /// M10: calling flush with no registered hooks returns 0 and does not
    /// block on the timeout. Required for the hot path inside the CLI exit
    /// hook, which runs on every invocation including ones with no hooks.
    #[tokio::test]
    async fn flush_nonblocking_hooks_returns_zero_when_empty() {
        let _serial = M10_GLOBAL.lock().await;
        let _ = flush_nonblocking_hooks(Duration::from_millis(50)).await;

        let started = std::time::Instant::now();
        let completed = flush_nonblocking_hooks(Duration::from_secs(60)).await;
        assert_eq!(completed, 0);
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "flush must short-circuit when no hooks are tracked (took {:?})",
            started.elapsed()
        );
    }

    /// M10: the timeout is a hard bound. A hook that never returns must not
    /// wedge process exit; flush returns 0 and logs a warning. The handle is
    /// dropped (which kills the child via `kill_on_drop(true)`).
    #[tokio::test]
    async fn flush_nonblocking_hooks_bounded_by_timeout() {
        let _serial = M10_GLOBAL.lock().await;
        let _ = flush_nonblocking_hooks(Duration::from_millis(50)).await;

        spawn_tracked_nonblocking_hook(async move {
            // Effectively forever for the purposes of this test.
            tokio::time::sleep(Duration::from_secs(60)).await;
        });

        let started = std::time::Instant::now();
        let completed = flush_nonblocking_hooks(Duration::from_millis(100)).await;
        assert_eq!(completed, 0, "slow hook must report 0 completed");
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "flush must respect the timeout (took {:?})",
            started.elapsed()
        );
    }
}
