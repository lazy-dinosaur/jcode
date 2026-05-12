use crate::logging;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

static INJECTION_QUEUES: LazyLock<Mutex<HashMap<String, Vec<InjectedContext>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InjectedSource {
    BackgroundTask {
        task_id: String,
        exit_code: i32,
        stdout: String,
        stderr: String,
        duration_ms: u64,
    },
    LifecycleHookStdout {
        hook_kind: String,
        tool_name: Option<String>,
        stdout: String,
    },
    Custom {
        reason: String,
        body: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InjectionFormat {
    #[default]
    SystemReminder,
    UserMessage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectedContext {
    pub source: InjectedSource,
    pub timestamp: SystemTime,
    pub format: InjectionFormat,
    pub dedupe_key: String,
}

impl InjectedContext {
    pub const MAX_BODY_BYTES: usize = 16 * 1024;

    pub fn render(&self) -> String {
        match self.format {
            InjectionFormat::SystemReminder => self.render_system_reminder(),
            InjectionFormat::UserMessage => self.render_user_message(),
        }
    }

    fn render_system_reminder(&self) -> String {
        match &self.source {
            InjectedSource::BackgroundTask {
                task_id,
                exit_code,
                stdout,
                stderr,
                duration_ms,
            } => format!(
                "[system-reminder]\nBackground task completed.\n- task_id: {task_id}\n- exit_code: {exit_code}\n- duration_ms: {duration_ms}\n- stdout (truncated to 16384 bytes):\n{stdout}\n- stderr (truncated to 2048 bytes):\n{stderr}"
            ),
            InjectedSource::LifecycleHookStdout {
                hook_kind,
                tool_name,
                stdout,
            } => {
                let tool_line = tool_name
                    .as_ref()
                    .map(|tool_name| format!("\n- tool_name: {tool_name}"))
                    .unwrap_or_default();
                format!(
                    "[system-reminder]\nLifecycle hook output.\n- hook_kind: {hook_kind}{tool_line}\n- stdout (truncated to 16384 bytes):\n{stdout}"
                )
            }
            InjectedSource::Custom { reason, body } => format!(
                "[system-reminder]\nInjected context.\n- reason: {reason}\n- body (truncated to 16KB):\n{body}"
            ),
        }
    }

    fn render_user_message(&self) -> String {
        match &self.source {
            InjectedSource::BackgroundTask {
                task_id,
                exit_code,
                stdout,
                stderr,
                duration_ms,
            } => format!(
                "Background task completed.\n- task_id: {task_id}\n- exit_code: {exit_code}\n- duration_ms: {duration_ms}\n- stdout:\n{stdout}\n- stderr:\n{stderr}"
            ),
            InjectedSource::LifecycleHookStdout {
                hook_kind,
                tool_name,
                stdout,
            } => {
                let tool_line = tool_name
                    .as_ref()
                    .map(|tool_name| format!("\n- tool_name: {tool_name}"))
                    .unwrap_or_default();
                format!(
                    "Lifecycle hook produced stdout.\n- hook_kind: {hook_kind}{tool_line}\n- stdout:\n{stdout}"
                )
            }
            InjectedSource::Custom { reason, body } => {
                format!("Injected context.\n- reason: {reason}\n- body:\n{body}")
            }
        }
    }

    fn truncate_bodies(&mut self) {
        match &mut self.source {
            InjectedSource::BackgroundTask { stdout, stderr, .. } => {
                truncate_string_to_max_bytes(stdout, Self::MAX_BODY_BYTES);
                truncate_string_to_max_bytes(stderr, Self::MAX_BODY_BYTES);
            }
            InjectedSource::LifecycleHookStdout { stdout, .. } => {
                truncate_string_to_max_bytes(stdout, Self::MAX_BODY_BYTES);
            }
            InjectedSource::Custom { body, .. } => {
                truncate_string_to_max_bytes(body, Self::MAX_BODY_BYTES);
            }
        }
    }
}

pub fn enqueue_injection(session_id: &str, mut ctx: InjectedContext) -> Result<()> {
    ctx.truncate_bodies();
    let mut queues = INJECTION_QUEUES
        .lock()
        .map_err(|err| anyhow::anyhow!("injection queue lock poisoned: {err}"))?;
    let queue = queues.entry(session_id.to_string()).or_default();
    if queue
        .iter()
        .any(|existing| existing.dedupe_key == ctx.dedupe_key)
    {
        logging::info(&format!(
            "[injected-context] skipped duplicate injection session_id={} dedupe_key={}",
            session_id, ctx.dedupe_key
        ));
        return Ok(());
    }

    logging::info(&format!(
        "[injected-context] enqueued injection session_id={} dedupe_key={}",
        session_id, ctx.dedupe_key
    ));
    queue.push(ctx);
    Ok(())
}

pub fn drain_injections(session_id: &str) -> Result<Vec<InjectedContext>> {
    let mut queues = INJECTION_QUEUES
        .lock()
        .map_err(|err| anyhow::anyhow!("injection queue lock poisoned: {err}"))?;
    let mut drained = queues.remove(session_id).unwrap_or_default();
    drained.sort_by_key(|ctx| ctx.timestamp);
    Ok(drained)
}

fn truncate_string_to_max_bytes(value: &mut String, max_bytes: usize) {
    if value.len() <= max_bytes {
        return;
    }

    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
}

#[cfg(test)]
#[path = "injected_context_tests.rs"]
mod tests;
