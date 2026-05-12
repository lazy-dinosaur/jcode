use crate::logging;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use tokio::sync::mpsc;

static INJECT_WAKE_SENDERS: LazyLock<Mutex<HashMap<String, mpsc::UnboundedSender<InjectWake>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectWake {
    pub session_id: String,
    pub source_kind: &'static str,
}

pub fn register_inject_wake_receiver(
    session_id: &str,
) -> anyhow::Result<mpsc::UnboundedReceiver<InjectWake>> {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut senders = INJECT_WAKE_SENDERS
        .lock()
        .map_err(|err| anyhow::anyhow!("inject wake sender lock poisoned: {err}"))?;
    senders.insert(session_id.to_string(), tx);
    logging::info(&format!(
        "[inject-wake] registered receiver session_id={}",
        session_id
    ));
    Ok(rx)
}

pub fn unregister_inject_wake_receiver(session_id: &str) {
    if let Ok(mut senders) = INJECT_WAKE_SENDERS.lock() {
        senders.remove(session_id);
    }
}

pub fn send_inject_wake(session_id: &str, source_kind: &'static str) -> bool {
    let tx = INJECT_WAKE_SENDERS
        .lock()
        .ok()
        .and_then(|senders| senders.get(session_id).cloned());

    let Some(tx) = tx else {
        logging::info(&format!(
            "[inject-wake] no receiver session_id={} source={}",
            session_id, source_kind
        ));
        return false;
    };

    let wake = InjectWake {
        session_id: session_id.to_string(),
        source_kind,
    };

    match tx.send(wake) {
        Ok(()) => true,
        Err(err) => {
            logging::warn(&format!(
                "[inject-wake] failed to send wake session_id={} source={}",
                err.0.session_id, err.0.source_kind
            ));
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_session(prefix: &str) -> String {
        static NEXT_SESSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        format!(
            "{prefix}-{}",
            NEXT_SESSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )
    }

    #[tokio::test]
    async fn inject_wake_register_and_send_delivers_message() {
        let session_id = unique_session("inject-wake");
        let mut rx = register_inject_wake_receiver(&session_id).expect("register receiver");

        assert!(send_inject_wake(&session_id, "hook"));
        let received = rx.recv().await.expect("wake message");

        assert_eq!(received.session_id, session_id);
        assert_eq!(received.source_kind, "hook");
        unregister_inject_wake_receiver(&received.session_id);
    }

    #[test]
    fn send_inject_wake_without_receiver_is_noop() {
        let session_id = unique_session("inject-wake-missing");
        unregister_inject_wake_receiver(&session_id);

        assert!(!send_inject_wake(&session_id, "hook"));
    }
}
