use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use tokio::task::JoinHandle;

static PENDING_ASYNC_HOOKS: LazyLock<Mutex<HashMap<String, Vec<PendingAsyncHook>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub struct PendingAsyncHook {
    pub id: String,
    pub handle: JoinHandle<PendingAsyncHookOutput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAsyncHookOutput {
    pub session_id: String,
    pub hook_kind: String,
    pub tool_name: Option<String>,
    pub command: String,
    pub stdout: String,
}

pub fn register_pending_async_hook(
    session_id: &str,
    id: String,
    handle: JoinHandle<PendingAsyncHookOutput>,
) {
    let hook = PendingAsyncHook { id, handle };
    match PENDING_ASYNC_HOOKS.lock() {
        Ok(mut pending) => pending
            .entry(session_id.to_string())
            .or_default()
            .push(hook),
        Err(err) => crate::logging::warn(&format!(
            "pending async hook registry lock poisoned while registering hook: {err}"
        )),
    }
}

pub async fn drain_completed_within(
    session_id: &str,
    timeout: Duration,
) -> Vec<(String, PendingAsyncHookOutput)> {
    let Some(has_finished) = pending_status(session_id) else {
        return Vec::new();
    };
    if timeout > Duration::ZERO && !has_finished {
        tokio::time::sleep(timeout).await;
    }

    let completed = take_finished(session_id);
    let mut outputs = Vec::with_capacity(completed.len());
    for hook in completed {
        let id = hook.id;
        match hook.handle.await {
            Ok(output) => outputs.push((id, output)),
            Err(err) => crate::logging::warn(&format!(
                "pending async hook task failed before injection drain: {err}"
            )),
        }
    }
    outputs
}

pub async fn drain_all_within(timeout: Duration) -> usize {
    let completed = take_all();
    if completed.is_empty() {
        return 0;
    }

    let join_all = async move {
        let mut count = 0;
        for hook in completed {
            if hook.handle.await.is_ok() {
                count += 1;
            }
        }
        count
    };

    if timeout == Duration::ZERO {
        return join_all.await;
    }

    match tokio::time::timeout(timeout, join_all).await {
        Ok(count) => count,
        Err(_) => 0,
    }
}

fn pending_status(session_id: &str) -> Option<bool> {
    PENDING_ASYNC_HOOKS.lock().ok().and_then(|pending| {
        pending
            .get(session_id)
            .map(|hooks| hooks.iter().any(|hook| hook.handle.is_finished()))
    })
}

#[cfg(test)]
pub fn pending_count(session_id: &str) -> usize {
    PENDING_ASYNC_HOOKS
        .lock()
        .ok()
        .and_then(|pending| pending.get(session_id).map(Vec::len))
        .unwrap_or(0)
}

#[cfg(test)]
pub fn clear_pending_async_hooks(session_id: &str) {
    if let Ok(mut pending) = PENDING_ASYNC_HOOKS.lock() {
        pending.remove(session_id);
    }
}

fn take_finished(session_id: &str) -> Vec<PendingAsyncHook> {
    let Ok(mut pending) = PENDING_ASYNC_HOOKS.lock() else {
        return Vec::new();
    };
    let Some(hooks) = pending.get_mut(session_id) else {
        return Vec::new();
    };

    let mut completed = Vec::new();
    let mut unfinished = Vec::with_capacity(hooks.len());
    for hook in std::mem::take(hooks) {
        if hook.handle.is_finished() {
            completed.push(hook);
        } else {
            unfinished.push(hook);
        }
    }
    *hooks = unfinished;
    if hooks.is_empty() {
        pending.remove(session_id);
    }
    completed
}

fn take_all() -> Vec<PendingAsyncHook> {
    PENDING_ASYNC_HOOKS
        .lock()
        .map(|mut pending| pending.drain().flat_map(|(_, hooks)| hooks).collect())
        .unwrap_or_default()
}
