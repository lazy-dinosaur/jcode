use serde::{Deserialize, Serialize};

/// Status of a background task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BackgroundTaskStatus {
    Running,
    Completed,
    Superseded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundTaskProgressKind {
    Determinate,
    Indeterminate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundTaskProgressSource {
    Reported,
    ParsedOutput,
    Heuristic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackgroundTaskProgress {
    pub kind: BackgroundTaskProgressKind,
    pub percent: Option<f32>,
    pub message: Option<String>,
    pub current: Option<u64>,
    pub total: Option<u64>,
    pub unit: Option<String>,
    pub eta_seconds: Option<u64>,
    pub updated_at: String,
    pub source: BackgroundTaskProgressSource,
}

impl BackgroundTaskProgress {
    pub fn normalize(mut self) -> Self {
        if let (Some(current), Some(total)) = (self.current, self.total)
            && total > 0
            && self.percent.is_none()
        {
            let computed = (current as f64 / total as f64) * 100.0;
            self.percent = Some(((computed * 100.0).round() / 100.0) as f32);
        }

        self.percent = self
            .percent
            .map(|percent| ((percent.clamp(0.0, 100.0) * 100.0).round()) / 100.0);

        if matches!(self.kind, BackgroundTaskProgressKind::Indeterminate)
            && (self.percent.is_some()
                || matches!((self.current, self.total), (_, Some(total)) if total > 0))
        {
            self.kind = BackgroundTaskProgressKind::Determinate;
        }

        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackgroundTaskProgressEvent {
    pub task_id: String,
    pub tool_name: String,
    pub display_name: Option<String>,
    /// Session id where the task was owned/executed.
    pub session_id: String,
    /// Session id where progress should be delivered. Defaults to `session_id`
    /// at construction sites for backwards-compatible owner-local delivery.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub delivery_session_id: String,
    pub progress: BackgroundTaskProgress,
}

impl BackgroundTaskProgressEvent {
    pub fn delivery_session_id_or_owner(&self) -> &str {
        if self.delivery_session_id.is_empty() {
            &self.session_id
        } else {
            &self.delivery_session_id
        }
    }
}
