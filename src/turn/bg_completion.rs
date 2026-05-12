use crate::logging;
use crate::turn::injected_context::{
    InjectedContext, InjectedSource, InjectionFormat, enqueue_injection,
};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;
use tokio::sync::mpsc;

static BG_COMPLETION_SENDERS: LazyLock<
    Mutex<HashMap<String, mpsc::UnboundedSender<BackgroundCompletion>>>,
> = LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundAutoInjectConfig {
    pub enabled: bool,
    pub format: InjectionFormat,
    pub max_bytes: Option<usize>,
}

impl Default for BackgroundAutoInjectConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            format: InjectionFormat::SystemReminder,
            max_bytes: None,
        }
    }
}

pub fn parse_injection_format(raw: Option<&str>) -> InjectionFormat {
    match raw.unwrap_or_default().trim().to_ascii_lowercase().as_str() {
        "user_message" | "user" => InjectionFormat::UserMessage,
        _ => InjectionFormat::SystemReminder,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundCompletion {
    pub task_id: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub session_id: String,
    pub notify: bool,
    pub auto_inject: bool,
    pub auto_inject_format: InjectionFormat,
    pub auto_inject_max_bytes: Option<usize>,
}

fn truncate_for_inject(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }

    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

pub fn enqueue_bg_completion_injection(
    session_id: &str,
    completion: &BackgroundCompletion,
) -> anyhow::Result<bool> {
    if !completion.auto_inject {
        return Ok(false);
    }

    let body_cap = completion
        .auto_inject_max_bytes
        .unwrap_or(InjectedContext::MAX_BODY_BYTES);
    let ctx = InjectedContext {
        source: InjectedSource::BackgroundTask {
            task_id: completion.task_id.clone(),
            exit_code: completion.exit_code,
            stdout: truncate_for_inject(&completion.stdout, body_cap),
            stderr: truncate_for_inject(&completion.stderr, body_cap.min(2048)),
            duration_ms: completion.duration_ms,
        },
        timestamp: SystemTime::now(),
        format: completion.auto_inject_format,
        dedupe_key: format!("bg:{}", completion.task_id),
    };
    enqueue_injection(session_id, ctx)?;
    Ok(true)
}

pub fn register_bg_completion_receiver(
    session_id: &str,
) -> anyhow::Result<mpsc::UnboundedReceiver<BackgroundCompletion>> {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut senders = BG_COMPLETION_SENDERS
        .lock()
        .map_err(|err| anyhow::anyhow!("background completion sender lock poisoned: {err}"))?;
    senders.insert(session_id.to_string(), tx);
    logging::info(&format!(
        "[bg-completion] registered receiver session_id={}",
        session_id
    ));
    Ok(rx)
}

pub fn unregister_bg_completion_receiver(session_id: &str) {
    if let Ok(mut senders) = BG_COMPLETION_SENDERS.lock() {
        senders.remove(session_id);
    }
}

pub fn send_bg_completion(completion: BackgroundCompletion) -> bool {
    if !completion.notify {
        return false;
    }

    let tx = BG_COMPLETION_SENDERS
        .lock()
        .ok()
        .and_then(|senders| senders.get(&completion.session_id).cloned());

    let Some(tx) = tx else {
        logging::info(&format!(
            "[bg-completion] no receiver for task_id={} session_id={}",
            completion.task_id, completion.session_id
        ));
        return false;
    };

    match tx.send(completion) {
        Ok(()) => true,
        Err(err) => {
            logging::warn(&format!(
                "[bg-completion] failed to send completion task_id={} session_id={}",
                err.0.task_id, err.0.session_id
            ));
            false
        }
    }
}

#[cfg(test)]
#[path = "bg_completion_tests.rs"]
mod tests;
