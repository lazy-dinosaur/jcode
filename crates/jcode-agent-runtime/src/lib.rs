use std::sync::Arc;

/// A soft interrupt message queued for injection at the next safe point.
#[derive(Debug, Clone)]
pub struct SoftInterruptMessage {
    pub content: String,
    /// If true, can skip remaining tools when injected at point C.
    pub urgent: bool,
    pub source: SoftInterruptSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoftInterruptSource {
    User,
    System,
    BackgroundTask,
}

/// Thread-safe soft interrupt queue that can be accessed without holding the agent lock.
pub type SoftInterruptQueue = Arc<std::sync::Mutex<Vec<SoftInterruptMessage>>>;

/// Signal to move the currently executing tool to background.
/// Uses std::sync so it can be set without async from outside the agent lock.
pub type BackgroundToolSignal = Arc<std::sync::atomic::AtomicBool>;

/// Signal to gracefully stop generation.
pub type GracefulShutdownSignal = Arc<std::sync::atomic::AtomicBool>;

/// Async-aware interrupt signal that combines AtomicBool (sync read) with
/// tokio::Notify (async wake). Eliminates spin-loops during tool execution.
#[derive(Clone)]
pub struct InterruptSignal {
    flag: Arc<std::sync::atomic::AtomicBool>,
    notify: Arc<tokio::sync::Notify>,
}

impl InterruptSignal {
    pub fn new() -> Self {
        Self {
            flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            notify: Arc::new(tokio::sync::Notify::new()),
        }
    }

    pub fn fire(&self) {
        self.flag.store(true, std::sync::atomic::Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_set(&self) -> bool {
        self.flag.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn reset(&self) {
        self.flag.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    pub async fn notified(&self) {
        let notified = self.notify.notified();
        if self.is_set() {
            return;
        }
        notified.await;
    }

    pub fn as_atomic(&self) -> Arc<std::sync::atomic::AtomicBool> {
        Arc::clone(&self.flag)
    }
}

impl Default for InterruptSignal {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnStopReason {
    UserInterrupt,
    ClientDisconnect,
    ServerReload,
    BackgroundCurrentTool,
    Superseded,
}

impl TurnStopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserInterrupt => "user_interrupt",
            Self::ClientDisconnect => "client_disconnect",
            Self::ServerReload => "server_reload",
            Self::BackgroundCurrentTool => "background_current_tool",
            Self::Superseded => "superseded",
        }
    }
}

#[derive(Clone)]
pub struct TurnControl {
    stop: InterruptSignal,
    reason: Arc<std::sync::RwLock<Option<TurnStopReason>>>,
}

impl TurnControl {
    pub fn new() -> Self {
        Self {
            stop: InterruptSignal::new(),
            reason: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    pub fn from_stop_signal(stop: InterruptSignal) -> Self {
        Self {
            stop,
            reason: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    pub fn request_stop(&self, reason: TurnStopReason) {
        if let Ok(mut guard) = self.reason.write() {
            *guard = Some(reason);
        }
        self.stop.fire();
    }

    pub fn reset(&self) {
        if let Ok(mut guard) = self.reason.write() {
            *guard = None;
        }
        self.stop.reset();
    }

    pub fn stop_signal(&self) -> InterruptSignal {
        self.stop.clone()
    }

    pub fn is_stopped(&self) -> bool {
        self.stop.is_set()
    }

    pub fn reason(&self) -> Option<TurnStopReason> {
        self.reason.read().ok().and_then(|guard| *guard)
    }

    pub fn reason_label(&self) -> Option<&'static str> {
        self.reason().map(TurnStopReason::as_str)
    }
}

impl Default for TurnControl {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::InterruptSignal;
    use std::time::Duration;

    #[tokio::test]
    async fn background_tool_signal_fire_before_notified_survives_until_reset() {
        let signal = InterruptSignal::new();
        signal.fire();

        tokio::time::timeout(Duration::from_millis(100), signal.notified())
            .await
            .expect("fire before notified() should wake immediately while the flag is set");

        signal.reset();
        tokio::time::timeout(Duration::from_millis(25), signal.notified())
            .await
            .expect_err("reset signal should not wake without another fire");
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct StreamError {
    pub message: String,
    pub retry_after_secs: Option<u64>,
}

impl StreamError {
    pub fn new(message: String, retry_after_secs: Option<u64>) -> Self {
        Self {
            message,
            retry_after_secs,
        }
    }
}
