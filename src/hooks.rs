use crate::config::config;
use crate::tool::{ToolContext, ToolOutput};
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

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

#[derive(Debug, Clone, Serialize)]
pub struct SessionStopHookPayload<'a> {
    pub event: &'a str,
    pub session_id: &'a str,
    pub working_dir: Option<String>,
    pub reason: &'a str,
    pub message_count: usize,
}

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
    run_lifecycle_hooks(SESSION_STOP, payload.working_dir.as_deref(), &payload).await
}

/// M11 stage 4: fire `client.disconnect` lifecycle hooks. The payload type
/// is shared with `session.stop` (only the `event` field differs) so users
/// can filter on either or both event names.
pub async fn run_client_disconnect_hooks(
    payload: SessionStopHookPayload<'_>,
) -> Result<Option<String>> {
    run_lifecycle_hooks(CLIENT_DISCONNECT, payload.working_dir.as_deref(), &payload).await
}

pub async fn run_response_hooks(
    payload: ResponseCompletedHookPayload<'_>,
) -> Result<Option<String>> {
    run_lifecycle_hooks(RESPONSE_COMPLETED, payload.working_dir.as_deref(), &payload).await
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
            tokio::spawn(async move {
                if let Err(err) =
                    run_nonblocking_hook(&command, timeout_ms, cwd.as_deref(), &payload_json).await
                {
                    crate::logging::warn(&format!("non-blocking hook failed: {err:#}"));
                }
            });
        }
    }

    Ok(())
}

async fn run_lifecycle_hooks<T>(
    event: &'static str,
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

    run_lifecycle_hook_commands(matching, cwd, payload).await
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
            match run_blocking_lifecycle_hook(&hook.command, hook.timeout_ms, cwd, &payload_json)
                .await
            {
                Ok(Some(reason)) => return Ok(Some(reason)),
                Ok(None) => {}
                Err(err) => {
                    crate::logging::warn(&format!("blocking lifecycle hook failed: {err:#}"));
                }
            }
        } else {
            let command = hook.command.clone();
            let timeout_ms = hook.timeout_ms;
            let cwd = cwd.map(str::to_string);
            let payload_json = payload_json.clone();
            tokio::spawn(async move {
                if let Err(err) =
                    run_nonblocking_hook(&command, timeout_ms, cwd.as_deref(), &payload_json).await
                {
                    crate::logging::warn(&format!("non-blocking lifecycle hook failed: {err:#}"));
                }
            });
        }
    }

    Ok(None)
}

async fn run_blocking_hook(
    command: &str,
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

    let Some(decision) = parse_hook_decision_stdout(&output.stdout, command)? else {
        return Ok(());
    };

    match hook_decision_denial_reason(decision)? {
        Some(reason) => Err(anyhow!("tool call denied by hook: {reason}")),
        None => Ok(()),
    }
}

fn parse_hook_decision_stdout(output_stdout: &[u8], command: &str) -> Result<Option<HookDecision>> {
    let stdout = String::from_utf8_lossy(output_stdout);
    let stdout = stdout.trim();
    if stdout.is_empty() {
        return Ok(None);
    }

    let decision: HookDecision = serde_json::from_str(stdout)
        .with_context(|| format!("invalid hook decision JSON from command: {command}"))?;
    Ok(Some(decision))
}

fn hook_decision_denial_reason(decision: HookDecision) -> Result<Option<String>> {
    match decision.action.as_str() {
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

async fn run_nonblocking_hook(
    command: &str,
    timeout_ms: u64,
    cwd: Option<&str>,
    payload_json: &[u8],
) -> Result<()> {
    let output = run_hook_command(command, timeout_ms, cwd, payload_json).await?;
    if !output.status.success() {
        return Err(anyhow!(
            "hook command exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

async fn run_blocking_lifecycle_hook(
    command: &str,
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

    let Some(decision) = parse_hook_decision_stdout(&output.stdout, command)? else {
        return Ok(None);
    };
    hook_decision_denial_reason(decision)
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
        assert_eq!(decision.action, "allow");
    }

    #[test]
    fn hook_decision_parses_deny_reason() {
        let decision: HookDecision =
            serde_json::from_str(r#"{"action":"deny","reason":"blocked"}"#).unwrap();
        assert_eq!(decision.action, "deny");
        assert_eq!(decision.reason.as_deref(), Some("blocked"));
    }

    #[test]
    fn hook_decision_stdout_empty_means_no_decision() {
        assert!(
            parse_hook_decision_stdout(b"  \n", "cmd")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn hook_decision_rejects_noisy_multiline_stdout() {
        let err = parse_hook_decision_stdout(
            b"log line\n{\"action\":\"deny\",\"reason\":\"blocked\"}",
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

    #[test]
    fn session_stop_payload_serializes_golden() {
        let payload = SessionStopHookPayload {
            event: SESSION_STOP,
            session_id: "sess-1",
            working_dir: Some("/tmp/work".to_string()),
            reason: "disconnect",
            message_count: 3,
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
        };

        let denial = run_lifecycle_hook_commands(
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
            })
        );
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
        run_blocking_hook("cat >/dev/null", 1000, None, br#"{"ok":true}"#)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn blocking_hook_denies_from_json_stdout() {
        let err = run_blocking_hook(
            "cat >/dev/null; printf '%s' '{\"action\":\"deny\",\"reason\":\"blocked\"}'",
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
        };

        let denial = run_lifecycle_hook_commands(commands, None, &payload)
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
        };

        let denial = run_lifecycle_hook_commands(commands, None, &payload)
            .await
            .unwrap();

        assert_eq!(denial.as_deref(), Some("blocked"));
        assert!(!marker.exists());
    }
}
