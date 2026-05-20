use super::{
    CompletionOptions, EventStream, ModelRoute, MultiProvider, NativeToolResultSender, Provider,
    copilot,
};
use crate::message::{Message, ToolDefinition};
use crate::provider::models::{
    ensure_model_allowed_for_subscription, filtered_display_models, filtered_model_routes,
};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::{Arc, RwLock};

pub struct JcodeProvider {
    inner: MultiProvider,
    selected_model: Arc<RwLock<String>>,
}

impl JcodeProvider {
    fn has_explicit_config_default_provider() -> bool {
        crate::config::config()
            .provider
            .default_provider
            .as_deref()
            .map(str::trim)
            .is_some_and(|provider| !provider.is_empty() && provider != "jcode")
    }

    pub fn new() -> Self {
        let has_explicit_config_default = Self::has_explicit_config_default_provider();
        if !has_explicit_config_default {
            crate::subscription_catalog::apply_runtime_env();
            Self::apply_runtime_profile();
        }
        let inner = MultiProvider::new_fast();
        let default_model = crate::subscription_catalog::default_model().id.to_string();
        if !has_explicit_config_default {
            let _ = inner.set_model(&default_model);
        }
        Self {
            inner,
            selected_model: Arc::new(RwLock::new(if has_explicit_config_default {
                String::new()
            } else {
                default_model
            })),
        }
    }

    fn apply_runtime_profile() {
        let _ = crate::provider::activation::ProviderActivation::jcode_subscription(
            crate::subscription_catalog::default_model().id,
        )
        .apply_env();
    }

    fn ensure_runtime_mode(&self) {
        if Self::has_explicit_config_default_provider() {
            return;
        }
        if !crate::subscription_catalog::is_runtime_mode_enabled() {
            crate::subscription_catalog::apply_runtime_env();
        }
        Self::apply_runtime_profile();
    }
}

impl Default for JcodeProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for JcodeProvider {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        system: &str,
        resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        self.ensure_runtime_mode();
        self.inner
            .complete(messages, tools, system, resume_session_id)
            .await
    }

    async fn complete_with_options(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        system: &str,
        resume_session_id: Option<&str>,
        options: CompletionOptions,
    ) -> Result<EventStream> {
        self.ensure_runtime_mode();
        self.inner
            .complete_with_options(messages, tools, system, resume_session_id, options)
            .await
    }

    async fn complete_split(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        system_static: &str,
        system_dynamic: &str,
        resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        self.ensure_runtime_mode();
        self.inner
            .complete_split(
                messages,
                tools,
                system_static,
                system_dynamic,
                resume_session_id,
            )
            .await
    }

    async fn complete_split_with_options(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        system_static: &str,
        system_dynamic: &str,
        resume_session_id: Option<&str>,
        options: CompletionOptions,
    ) -> Result<EventStream> {
        self.ensure_runtime_mode();
        self.inner
            .complete_split_with_options(
                messages,
                tools,
                system_static,
                system_dynamic,
                resume_session_id,
                options,
            )
            .await
    }

    fn name(&self) -> &str {
        "Jcode Subscription"
    }

    fn model(&self) -> String {
        let selected = self
            .selected_model
            .read()
            .map(|model| model.clone())
            .unwrap_or_default();
        if selected.trim().is_empty() {
            self.inner.model()
        } else {
            selected
        }
    }

    fn set_model(&self, model: &str) -> Result<()> {
        self.ensure_runtime_mode();
        ensure_model_allowed_for_subscription(model)?;
        self.inner.set_model(model)?;
        if let Ok(mut selected_model) = self.selected_model.write() {
            *selected_model = crate::subscription_catalog::canonical_model_id(model)
                .unwrap_or(model)
                .to_string();
        }
        Ok(())
    }

    fn available_models(&self) -> Vec<&'static str> {
        self.inner.available_models()
    }

    fn available_models_display(&self) -> Vec<String> {
        self.ensure_runtime_mode();
        filtered_display_models(self.inner.available_models_display())
    }

    fn available_models_for_switching(&self) -> Vec<String> {
        self.ensure_runtime_mode();
        filtered_display_models(self.inner.available_models_for_switching())
    }

    fn available_providers_for_model(&self, model: &str) -> Vec<String> {
        self.inner.available_providers_for_model(model)
    }

    fn provider_details_for_model(&self, model: &str) -> Vec<(String, String)> {
        self.inner.provider_details_for_model(model)
    }

    fn preferred_provider(&self) -> Option<String> {
        self.inner.preferred_provider()
    }

    fn model_routes(&self) -> Vec<ModelRoute> {
        self.ensure_runtime_mode();
        filtered_model_routes(self.inner.model_routes())
    }

    async fn prefetch_models(&self) -> Result<()> {
        self.ensure_runtime_mode();
        self.inner.prefetch_models().await
    }

    fn on_auth_changed(&self) {
        self.ensure_runtime_mode();
        self.inner.on_auth_changed();
        let selected_model = self.model();
        let _ = self.inner.set_model(&selected_model);
    }

    fn reasoning_effort(&self) -> Option<String> {
        self.inner.reasoning_effort()
    }

    fn set_reasoning_effort(&self, effort: &str) -> Result<()> {
        self.inner.set_reasoning_effort(effort)
    }

    fn available_efforts(&self) -> Vec<&'static str> {
        self.inner.available_efforts()
    }

    // M47-C4: forward provider-aware context / thinking dimensions to the
    // wrapped MultiProvider so any caller holding a `JcodeProvider` (e.g.
    // the desktop/UI layer that wraps the multi-provider for runtime profile
    // application) keeps full surface parity.
    fn available_contexts(&self) -> Vec<&'static str> {
        self.inner.available_contexts()
    }

    fn context_preference(&self) -> Option<String> {
        self.inner.context_preference()
    }

    fn set_context_preference(&self, context: &str) -> Result<()> {
        self.inner.set_context_preference(context)
    }

    fn supports_thinking(&self) -> bool {
        self.inner.supports_thinking()
    }

    fn thinking_enabled(&self) -> Option<bool> {
        self.inner.thinking_enabled()
    }

    fn set_thinking(&self, enabled: bool) -> Result<()> {
        self.inner.set_thinking(enabled)
    }

    fn native_compaction_mode(&self) -> Option<String> {
        self.inner.native_compaction_mode()
    }

    fn native_compaction_threshold_tokens(&self) -> Option<usize> {
        self.inner.native_compaction_threshold_tokens()
    }

    fn transport(&self) -> Option<String> {
        self.inner.transport()
    }

    fn set_transport(&self, transport: &str) -> Result<()> {
        self.inner.set_transport(transport)
    }

    fn available_transports(&self) -> Vec<&'static str> {
        self.inner.available_transports()
    }

    fn handles_tools_internally(&self) -> bool {
        self.inner.handles_tools_internally()
    }

    async fn invalidate_credentials(&self) {
        self.inner.invalidate_credentials().await;
    }

    fn set_premium_mode(&self, mode: copilot::PremiumMode) {
        self.inner.set_premium_mode(mode);
    }

    fn premium_mode(&self) -> copilot::PremiumMode {
        self.inner.premium_mode()
    }

    fn supports_compaction(&self) -> bool {
        self.inner.supports_compaction()
    }

    fn uses_jcode_compaction(&self) -> bool {
        self.inner.uses_jcode_compaction()
    }

    async fn native_compact(
        &self,
        messages: &[Message],
        existing_summary_text: Option<&str>,
        existing_openai_encrypted_content: Option<&str>,
    ) -> Result<crate::provider::NativeCompactionResult> {
        self.inner
            .native_compact(
                messages,
                existing_summary_text,
                existing_openai_encrypted_content,
            )
            .await
    }

    fn context_window(&self) -> usize {
        self.inner.context_window()
    }

    fn fork(&self) -> Arc<dyn Provider> {
        self.ensure_runtime_mode();
        let forked = Self::new();
        let selected_model = self.model();
        let _ = forked.set_model(&selected_model);
        Arc::new(forked)
    }

    fn native_result_sender(&self) -> Option<NativeToolResultSender> {
        self.inner.native_result_sender()
    }

    fn drain_startup_notices(&self) -> Vec<String> {
        self.inner.drain_startup_notices()
    }

    fn switch_active_provider_to(&self, provider: &str) -> Result<()> {
        self.ensure_runtime_mode();
        self.inner.switch_active_provider_to(provider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct JcodeHomeGuard {
        previous: Option<std::ffi::OsString>,
    }

    impl JcodeHomeGuard {
        fn new(path: &std::path::Path) -> Self {
            let previous = std::env::var_os("JCODE_HOME");
            crate::env::set_var("JCODE_HOME", path);
            crate::config::Config::invalidate_cache();
            Self { previous }
        }
    }

    impl Drop for JcodeHomeGuard {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.take() {
                crate::env::set_var("JCODE_HOME", previous);
            } else {
                crate::env::remove_var("JCODE_HOME");
            }
            crate::config::Config::invalidate_cache();
        }
    }

    #[test]
    fn jcode_provider_enables_subscription_runtime_mode() {
        let _guard = crate::storage::lock_test_env();
        let temp = tempfile::tempdir().expect("temp jcode home");
        let _home = JcodeHomeGuard::new(temp.path());
        crate::subscription_catalog::clear_runtime_env();
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");

        runtime.block_on(async {
            let provider = JcodeProvider::new();
            assert!(crate::subscription_catalog::is_runtime_mode_enabled());
            assert!(
                provider
                    .available_models_display()
                    .into_iter()
                    .all(|model| crate::subscription_catalog::is_curated_model(&model))
            );
        });

        crate::subscription_catalog::clear_runtime_env();
    }

    #[test]
    fn jcode_provider_name_and_default_model_are_curated() {
        let _guard = crate::storage::lock_test_env();
        let temp = tempfile::tempdir().expect("temp jcode home");
        let _home = JcodeHomeGuard::new(temp.path());
        crate::subscription_catalog::clear_runtime_env();
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");

        runtime.block_on(async {
            let provider = JcodeProvider::new();
            assert_eq!(provider.name(), "Jcode Subscription");
            let model = provider.model();
            assert!(
                crate::subscription_catalog::is_curated_model(&model),
                "expected curated model, got {model}"
            );
        });

        crate::subscription_catalog::clear_runtime_env();
    }

    #[test]
    fn jcode_provider_does_not_force_subscription_runtime_when_config_default_provider_is_explicit()
    {
        let _guard = crate::storage::lock_test_env();
        crate::subscription_catalog::clear_runtime_env();
        crate::env::set_var("JCODE_FORCE_PROVIDER", "0");
        crate::env::set_var("JCODE_ACTIVE_PROVIDER", "claude");
        crate::env::set_var("JCODE_OPENROUTER_MODEL", "before");
        let temp = tempfile::tempdir().expect("temp jcode home");
        let _home = JcodeHomeGuard::new(temp.path());
        std::fs::write(
            temp.path().join("config.toml"),
            "[provider]\ndefault_provider = \"claude\"\ndefault_model = \"claude-opus-4-7[1m]\"\n",
        )
        .expect("write config");
        crate::config::Config::invalidate_cache();

        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        runtime.block_on(async {
            let _provider = JcodeProvider::new();
            assert!(
                std::env::var("JCODE_FORCE_PROVIDER")
                    .ok()
                    .is_none_or(|value| !matches!(
                        value.trim().to_ascii_lowercase().as_str(),
                        "1" | "true" | "yes"
                    ))
            );
            assert_eq!(
                std::env::var("JCODE_ACTIVE_PROVIDER").as_deref(),
                Ok("claude")
            );
            assert!(std::env::var_os("JCODE_OPENROUTER_MODEL").is_none());
        });

        crate::env::remove_var("JCODE_FORCE_PROVIDER");
        crate::env::remove_var("JCODE_ACTIVE_PROVIDER");
        crate::env::remove_var("JCODE_OPENROUTER_MODEL");
        crate::subscription_catalog::clear_runtime_env();
    }
}
