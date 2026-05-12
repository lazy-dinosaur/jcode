use crate::logging;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use tokio::sync::mpsc;

static BG_COMPLETION_SENDERS: LazyLock<Mutex<HashMap<String, mpsc::UnboundedSender<BackgroundCompletion>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundCompletion {
    pub task_id: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub session_id: String,
    pub notify: bool,
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
