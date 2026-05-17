use super::*;

impl Agent {
    pub fn set_premium_mode(&self, mode: crate::provider::copilot::PremiumMode) {
        self.provider.set_premium_mode(mode);
    }

    pub fn premium_mode(&self) -> crate::provider::copilot::PremiumMode {
        self.provider.premium_mode()
    }

    pub fn provider_fork(&self) -> Arc<dyn Provider> {
        self.provider.fork()
    }

    pub fn provider_handle(&self) -> Arc<dyn Provider> {
        Arc::clone(&self.provider)
    }

    pub fn available_models(&self) -> Vec<&'static str> {
        self.provider.available_models()
    }

    pub fn available_models_for_switching(&self) -> Vec<String> {
        self.provider.available_models_for_switching()
    }

    pub fn available_models_display(&self) -> Vec<String> {
        self.provider.available_models_display()
    }

    pub fn model_routes(&self) -> Vec<crate::provider::ModelRoute> {
        self.provider.model_routes()
    }

    pub fn registry(&self) -> Registry {
        self.registry.clone()
    }

    pub async fn compaction_mode(&self) -> crate::config::CompactionMode {
        self.registry.compaction().read().await.mode()
    }

    pub async fn set_compaction_mode(&self, mode: crate::config::CompactionMode) -> Result<()> {
        let compaction = self.registry.compaction();
        let mut manager = compaction.write().await;
        manager.set_mode(mode);
        Ok(())
    }

    pub fn provider_messages(&mut self) -> Vec<Message> {
        self.session.messages_for_provider()
    }

    pub fn set_model(&mut self, model: &str) -> Result<()> {
        self.set_model_from_provider_state_event(
            model,
            crate::provider::ProviderModelSelectionSource::User,
        )
    }

    pub(crate) fn set_model_from_auth(&mut self, model: &str) -> Result<()> {
        self.set_model_from_provider_state_event(
            model,
            crate::provider::ProviderModelSelectionSource::Auth,
        )
    }

    fn set_model_from_provider_state_event(
        &mut self,
        model: &str,
        source: crate::provider::ProviderModelSelectionSource,
    ) -> Result<()> {
        crate::provider::set_model_with_auth_refresh(self.provider.as_ref(), model)?;
        let resolved_model = self.provider.model();
        self.session.model = Some(resolved_model.clone());
        let event = crate::provider::ProviderStateEvent::selected_model(source, resolved_model);
        self.provider_runtime_state.apply(event);
        self.log_env_snapshot("set_model");
        Ok(())
    }

    pub(crate) fn provider_model_selection_generation(&self) -> u64 {
        self.provider_runtime_state.selection_generation()
    }

    pub(crate) fn user_selected_provider_model_after(&self, generation: u64) -> bool {
        self.provider_runtime_state.user_selected_after(generation)
    }

    /// M47-C6: Restore all provider preferences persisted on the session
    /// (effort / context_preference / thinking_enabled) onto the live provider
    /// when the session is loaded or activated. Each dimension is independent:
    /// the active provider may support some and silently skip the rest
    /// (M47-C1/C-4 silent-skip semantics). When the session has no persisted
    /// preference for a dimension, the current provider value is captured back
    /// into the session so reconnects to a new account preserve the user's
    /// intent.
    pub fn restore_provider_preferences_from_session(&mut self) {
        // ---- effort (M47-C1 silent skip semantics) ----
        if let Some(effort) = self.session.reasoning_effort.clone() {
            if let Err(e) = self.provider.set_reasoning_effort(&effort) {
                crate::logging::debug(&format!(
                    "Skipped restoring reasoning effort '{}': {}",
                    effort, e
                ));
            }
        } else {
            self.session.reasoning_effort = self.provider.reasoning_effort();
        }

        // ---- context preference (M47-C4 Anthropic [1m] toggle) ----
        if let Some(context) = self.session.context_preference.clone() {
            if let Err(e) = self.provider.set_context_preference(&context) {
                crate::logging::debug(&format!(
                    "Skipped restoring context preference '{}': {}",
                    context, e
                ));
            }
        } else if !self.provider.available_contexts().is_empty() {
            self.session.context_preference = self.provider.context_preference();
        }

        // ---- thinking toggle (M47-C4 Gemini / Anthropic / OpenRouter) ----
        if let Some(thinking) = self.session.thinking_enabled {
            if let Err(e) = self.provider.set_thinking(thinking) {
                crate::logging::debug(&format!(
                    "Skipped restoring thinking={}: {}",
                    thinking, e
                ));
            }
        } else if self.provider.supports_thinking() {
            self.session.thinking_enabled = self.provider.thinking_enabled();
        }
    }

    /// M47-C6 back-compat alias: the historical entry point used by
    /// `Agent::new_with_session` and `Agent::restore_session`. Forwards to
    /// the generalized restorer so all five session-level provider
    /// preferences (effort / context / thinking) round-trip on session load.
    pub fn restore_reasoning_effort_from_session(&mut self) {
        self.restore_provider_preferences_from_session();
    }

    pub fn set_reasoning_effort(&mut self, effort: &str) -> Result<Option<String>> {
        self.provider.set_reasoning_effort(effort)?;
        let current = self.provider.reasoning_effort();
        self.session.reasoning_effort = current.clone();
        self.log_env_snapshot("set_reasoning_effort");
        self.session.save()?;
        Ok(current)
    }

    pub fn subagent_model(&self) -> Option<String> {
        self.session.subagent_model.clone()
    }

    pub fn set_subagent_model(&mut self, model: Option<String>) -> Result<()> {
        self.session.subagent_model = model;
        self.log_env_snapshot("set_subagent_model");
        self.session.save()?;
        Ok(())
    }

    pub fn session_provider_key(&self) -> Option<String> {
        self.session.provider_key.clone()
    }

    pub fn set_session_provider_key(&mut self, provider_key: Option<String>) {
        self.session.provider_key = provider_key;
    }

    pub fn rename_session_title(&mut self, title: Option<String>) -> Result<String> {
        self.session.rename_title(title);
        self.log_env_snapshot("rename_session");
        self.session.save()?;
        Ok(self.session.display_title_or_name().to_string())
    }

    pub fn autoreview_enabled(&self) -> Option<bool> {
        self.session.autoreview_enabled
    }

    pub fn set_autoreview_enabled(&mut self, enabled: bool) -> Result<()> {
        self.session.autoreview_enabled = Some(enabled);
        self.log_env_snapshot("set_autoreview_enabled");
        self.session.save()?;
        Ok(())
    }

    pub fn autojudge_enabled(&self) -> Option<bool> {
        self.session.autojudge_enabled
    }

    pub fn set_autojudge_enabled(&mut self, enabled: bool) -> Result<()> {
        self.session.autojudge_enabled = Some(enabled);
        self.log_env_snapshot("set_autojudge_enabled");
        self.session.save()?;
        Ok(())
    }

    /// Set the working directory for this session
    pub fn set_working_dir(&mut self, dir: &str) {
        if self.session.working_dir.as_deref() == Some(dir) {
            return;
        }
        self.session.working_dir = Some(dir.to_string());
        self.session.refresh_initial_session_context_message();
        self.log_env_snapshot("working_dir");
    }

    /// Set and persist the working directory for this session.
    pub fn set_working_dir_and_save(&mut self, dir: &str) -> Result<()> {
        self.set_working_dir(dir);
        self.session.updated_at = chrono::Utc::now();
        self.session.save()?;
        Ok(())
    }

    /// Get the working directory for this session
    pub fn working_dir(&self) -> Option<&str> {
        self.session.working_dir.as_deref()
    }

    /// Get the stored messages (for transcript export)
    pub fn messages(&self) -> &[StoredMessage] {
        &self.session.messages
    }
}
