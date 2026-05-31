use super::*;

impl App {
    pub(super) fn track_pending_soft_interrupt(&mut self, request_id: u64, content: String) {
        self.pending_soft_interrupt_requests
            .push((request_id, content.clone()));
        self.pending_soft_interrupts.push(content);
    }

    pub(super) fn acknowledge_pending_soft_interrupt(&mut self, request_id: u64) -> bool {
        if let Some(index) = self
            .pending_soft_interrupt_requests
            .iter()
            .position(|(id, _)| *id == request_id)
        {
            self.pending_soft_interrupt_requests.remove(index);
            true
        } else {
            false
        }
    }

    pub(super) fn clear_pending_soft_interrupt_tracking(&mut self) {
        self.pending_soft_interrupts.clear();
        self.pending_soft_interrupt_requests.clear();
    }

    pub(super) fn mark_soft_interrupt_injected(&mut self, content: &str) {
        if self.mark_combined_soft_interrupt_injected(content) {
            return;
        }

        if let Some(index) = self
            .pending_soft_interrupts
            .iter()
            .position(|pending| pending == content)
        {
            self.pending_soft_interrupts.remove(index);
        }

        if let Some(index) = self
            .pending_soft_interrupt_requests
            .iter()
            .position(|(_, pending)| pending == content)
        {
            self.pending_soft_interrupt_requests.remove(index);
        }
    }

    fn mark_combined_soft_interrupt_injected(&mut self, content: &str) -> bool {
        let mut combined = String::new();
        for (index, pending) in self.pending_soft_interrupts.iter().enumerate() {
            if index > 0 {
                combined.push_str("\n\n");
            }
            combined.push_str(pending);

            if combined == content {
                let count = index + 1;
                let removed: Vec<String> = self.pending_soft_interrupts.drain(..count).collect();
                for removed_content in removed {
                    if let Some(request_index) = self
                        .pending_soft_interrupt_requests
                        .iter()
                        .position(|(_, pending)| pending == &removed_content)
                    {
                        self.pending_soft_interrupt_requests.remove(request_index);
                    }
                }
                return true;
            }

            if !content.starts_with(&combined) {
                break;
            }
        }

        false
    }
}

pub(super) fn recover_local_interleave_to_queue(app: &mut App, reason: &str) -> bool {
    let Some(interleave) = app.interleave_message.take() else {
        return false;
    };
    if interleave.trim().is_empty() {
        return false;
    }

    crate::logging::info(&format!(
        "Recovering unsent interleave into queued follow-ups after {}",
        reason
    ));
    let inserted = app.prepend_recovered_user_followups_unique(vec![interleave]);
    if inserted {
        app.batch_recovered_soft_interrupts_with_queue = true;
    }
    inserted
}

pub(super) async fn recover_stranded_soft_interrupts(
    app: &mut App,
    remote: &mut RemoteConnection,
) -> bool {
    if app.is_processing || app.pending_soft_interrupts.is_empty() {
        return false;
    }

    let recovered_interrupts: Vec<String> = app
        .pending_soft_interrupt_requests
        .iter()
        .map(|(_, pending)| pending.clone())
        .collect();
    if recovered_interrupts.is_empty() {
        app.pending_soft_interrupts.clear();
        return false;
    }

    if let Err(err) = remote.cancel_soft_interrupts().await {
        app.push_display_message(DisplayMessage::error(format!(
            "Failed to recover queued interleave message: {}",
            err
        )));
        app.set_status_notice("Queued interleave recovery failed");
        return false;
    }

    crate::logging::info(&format!(
        "Recovering {} stranded soft interrupt(s) into queued follow-ups after turn boundary",
        recovered_interrupts.len()
    ));
    app.pending_soft_interrupts.clear();
    app.pending_soft_interrupt_requests.clear();

    if app.batch_recovered_soft_interrupts_with_queue {
        app.batch_recovered_soft_interrupts_with_queue = false;
        app.prepend_recovered_soft_interrupts_unique(recovered_interrupts);
    } else {
        let mut seen: std::collections::HashSet<String> =
            app.queued_messages.iter().cloned().collect();
        if let Some(existing) = app.interleave_message.as_ref()
            && !existing.trim().is_empty()
        {
            seen.insert(existing.clone());
        }
        let recovered_interrupts: Vec<String> = recovered_interrupts
            .into_iter()
            .filter(|interrupt| !interrupt.trim().is_empty() && seen.insert(interrupt.clone()))
            .collect();
        let recovered = recovered_interrupts.join("\n\n");
        if recovered.trim().is_empty() {
            return false;
        }
        app.interleave_message = Some(match app.interleave_message.take() {
            Some(existing) if !existing.trim().is_empty() => {
                format!("{}\n\n{}", recovered, existing)
            }
            _ => recovered,
        });
    }
    app.set_status_notice("Recovered queued interleave after turn finished");
    true
}
