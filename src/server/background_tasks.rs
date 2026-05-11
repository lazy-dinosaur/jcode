use super::{
    SessionAgents, SessionInterruptQueues, SwarmMember, fanout_session_event,
    queue_soft_interrupt_for_session, session_event_fanout_sender,
};
use crate::message::{
    format_background_task_notification_markdown, format_background_task_progress_markdown,
};
use crate::protocol::{NotificationType, ServerEvent};
use crate::session::Session;
use jcode_agent_runtime::SoftInterruptSource;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

const MAX_DELIVERY_PARENT_DEPTH: usize = 10;

pub(super) async fn run_background_task_message_in_live_session_if_idle(
    session_id: &str,
    message: &str,
    sessions: &SessionAgents,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
) -> bool {
    let agent = {
        let guard = sessions.read().await;
        guard.get(session_id).cloned()
    };
    let Some(agent) = agent else {
        return false;
    };

    let has_live_attachments = {
        let members = swarm_members.read().await;
        members
            .get(session_id)
            .map(|member| member.event_txs.values().any(|tx| !tx.is_closed()))
            .unwrap_or(false)
    };
    if !has_live_attachments {
        return false;
    }

    let is_idle = match agent.try_lock() {
        Ok(guard) => {
            drop(guard);
            true
        }
        Err(_) => false,
    };

    if !is_idle {
        return false;
    }

    let session_id = session_id.to_string();
    let message = message.to_string();
    let event_tx = session_event_fanout_sender(session_id.clone(), Arc::clone(swarm_members));
    tokio::spawn(async move {
        if let Err(err) = super::client_lifecycle::process_message_streaming_mpsc(
            agent,
            &message,
            vec![],
            Some(
                "A background task for this session just finished. Review the completion message and continue if useful."
                    .to_string(),
            ),
            event_tx,
        )
        .await
        {
            crate::logging::error(&format!(
                "Failed to run background task completion immediately for live session {}: {}",
                session_id, err
            ));
        }
    });

    true
}

/// Walk report-back / parent_id chains from `start_session_id` upward and return
/// the first session that has a live attached client. Falls back to the start id
/// when no live ancestor is found or a loop/depth limit is encountered.
pub(super) async fn resolve_background_delivery_target(
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    start_session_id: &str,
) -> String {
    let mut current = next_delivery_ancestor(swarm_members, start_session_id)
        .await
        .unwrap_or_else(|| start_session_id.to_string());
    let mut visited = HashSet::new();

    for _ in 0..MAX_DELIVERY_PARENT_DEPTH {
        if !visited.insert(current.clone()) {
            break;
        }

        let has_live_attachment = {
            let members = swarm_members.read().await;
            members
                .get(&current)
                .map(|member| member.event_txs.values().any(|tx| !tx.is_closed()))
                .unwrap_or(false)
        };

        if has_live_attachment {
            return current;
        }

        let next = next_delivery_ancestor(swarm_members, &current).await;

        let Some(next) = next else {
            break;
        };
        current = next;
    }

    start_session_id.to_string()
}

async fn next_delivery_ancestor(
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    session_id: &str,
) -> Option<String> {
    if let Some(parent_id) = Session::load_startup_stub(session_id)
        .ok()
        .and_then(|session| session.parent_id)
        .filter(|parent_id| parent_id != session_id)
    {
        return Some(parent_id);
    }

    let members = swarm_members.read().await;
    members
        .get(session_id)
        .and_then(|member| member.report_back_to_session_id.clone())
        .filter(|report_back_id| report_back_id != session_id)
}

pub(super) async fn dispatch_background_task_completion(
    task: &crate::bus::BackgroundTaskCompleted,
    sessions: &SessionAgents,
    soft_interrupt_queues: &SessionInterruptQueues,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
) {
    let notification = format_background_task_notification_markdown(task);
    let delivery_session_id =
        resolve_background_delivery_target(swarm_members, task.delivery_session_id_or_owner())
            .await;

    if task.notify
        && fanout_session_event(
            swarm_members,
            &delivery_session_id,
            ServerEvent::Notification {
                from_session: "background_task".to_string(),
                from_name: Some("background task".to_string()),
                notification_type: NotificationType::Message {
                    scope: Some("background_task".to_string()),
                    channel: None,
                },
                message: notification.clone(),
            },
        )
        .await
            == 0
    {
        crate::logging::warn(&format!(
            "Failed to notify attached clients for background task completion on session {} (owner {})",
            delivery_session_id, task.session_id
        ));
    }

    if task.wake
        && !run_background_task_message_in_live_session_if_idle(
            &delivery_session_id,
            &notification,
            sessions,
            swarm_members,
        )
        .await
        && !queue_soft_interrupt_for_session(
            &delivery_session_id,
            notification.clone(),
            false,
            SoftInterruptSource::BackgroundTask,
            soft_interrupt_queues,
            sessions,
        )
        .await
    {
        crate::logging::warn(&format!(
            "Failed to deliver background task completion to session {} (owner {})",
            delivery_session_id, task.session_id
        ));
    }
}

pub(super) async fn dispatch_background_task_progress(
    task: &crate::bus::BackgroundTaskProgressEvent,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
) {
    let notification = format_background_task_progress_markdown(task);
    let delivery_session_id =
        resolve_background_delivery_target(swarm_members, task.delivery_session_id_or_owner())
            .await;
    if fanout_session_event(
        swarm_members,
        &delivery_session_id,
        ServerEvent::Notification {
            from_session: "background_task".to_string(),
            from_name: Some("background task".to_string()),
            notification_type: NotificationType::Message {
                scope: Some("background_task".to_string()),
                channel: None,
            },
            message: notification,
        },
    )
    .await
        == 0
    {
        crate::logging::warn(&format!(
            "Failed to notify attached clients for background task progress on session {} (owner {})",
            delivery_session_id, task.session_id
        ));
    }
}

pub(super) async fn dispatch_ui_activity(
    activity: &crate::bus::UiActivity,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
) {
    let Some(session_id) = activity.session_id.as_deref() else {
        return;
    };

    if fanout_session_event(
        swarm_members,
        session_id,
        ServerEvent::Notification {
            from_session: "jcode".to_string(),
            from_name: Some("Jcode".to_string()),
            notification_type: NotificationType::Message {
                scope: Some(activity.kind.scope().to_string()),
                channel: None,
            },
            message: activity.message.clone(),
        },
    )
    .await
        == 0
    {
        crate::logging::warn(&format!(
            "Failed to notify attached clients for UI activity on session {}",
            session_id
        ));
    }
}
