use super::*;
use crate::storage::jcode_dir;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PartialConfig {
    prompt: PartialPromptConfig,
    swarm: PartialSwarmConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PartialPromptConfig {
    ignore_project_agents: Option<bool>,
    ignore_global_agents: Option<bool>,
    load_jcode_agents: Option<bool>,
    load_harness_dir: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PartialAgentsConfig {
    swarm_model: Option<String>,
    routing: std::collections::BTreeMap<String, String>,
    routes: std::collections::BTreeMap<String, AgentRouteConfig>,
    profiles: std::collections::BTreeMap<String, AgentRouteConfig>,
    memory_model: Option<String>,
    memory_sidecar_enabled: bool,
    swarm_spawn_visible: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PartialSwarmConfig {
    max_active_spawns_per_coordinator: Option<u32>,
    max_active_spawns_per_run: Option<u32>,
}

impl PartialAgentsConfig {
    fn apply_to(self, agents: &mut AgentsConfig) {
        if let Some(value) = self.swarm_model {
            agents.swarm_model = Some(value);
        }
        agents.routing.extend(self.routing);
        agents.routes.extend(self.routes);
        agents.profiles.extend(self.profiles);
        if let Some(value) = self.memory_model {
            agents.memory_model = Some(value);
        }
        agents.memory_sidecar_enabled = self.memory_sidecar_enabled;
        if self.swarm_spawn_visible.is_some() {
            agents.swarm_spawn_visible = self.swarm_spawn_visible;
        }
    }
}

impl PartialPromptConfig {
    fn apply_to(self, prompt: &mut PromptConfig) {
        if let Some(value) = self.ignore_project_agents {
            prompt.ignore_project_agents = value;
        }
        if let Some(value) = self.ignore_global_agents {
            prompt.ignore_global_agents = value;
        }
        if let Some(value) = self.load_jcode_agents {
            prompt.load_jcode_agents = value;
        }
        if let Some(value) = self.load_harness_dir {
            prompt.load_harness_dir = value;
        }
    }
}

impl PartialSwarmConfig {
    fn apply_to(self, swarm: &mut SwarmConfig) {
        if self.max_active_spawns_per_coordinator.is_some() {
            swarm.max_active_spawns_per_coordinator = self.max_active_spawns_per_coordinator;
        }
        if self.max_active_spawns_per_run.is_some() {
            swarm.max_active_spawns_per_run = self.max_active_spawns_per_run;
        }
    }
}

impl Config {
    /// Get the config file path
    pub fn path() -> Option<PathBuf> {
        jcode_dir().ok().map(|d| d.join("config.toml"))
    }

    /// Load config from file, with environment variable overrides
    pub fn load() -> Self {
        let mut config = Self::load_from_file().unwrap_or_default();
        config.apply_env_overrides();
        config
    }

    /// M19: like [`Self::load`] but propagates parse errors instead of falling
    /// back to default. Used by hot-reload (`force_reload_config` /
    /// `maybe_reload`) so a transiently invalid TOML file (e.g. mid-edit save)
    /// does not silently wipe the user's effective config back to defaults —
    /// callers can keep the previous snapshot instead.
    ///
    /// Behaviour:
    /// - returns `Ok(default + env overrides)` if no config file exists
    ///   (matches `load()` for that case).
    /// - returns `Ok(parsed + env overrides)` on successful parse.
    /// - returns `Err(...)` on read or parse failure.
    pub fn try_load() -> anyhow::Result<Self> {
        let path = Self::path();
        let mut config = match path {
            Some(ref p) if p.exists() => {
                let content = std::fs::read_to_string(p)
                    .map_err(|e| anyhow::anyhow!("failed to read {}: {}", p.display(), e))?;
                let mut parsed: Self = toml::from_str(&content)
                    .map_err(|e| anyhow::anyhow!("failed to parse {}: {}", p.display(), e))?;
                parsed.display.apply_legacy_compat();
                parsed
            }
            _ => Self::default(),
        };
        config.apply_env_overrides();
        Ok(config)
    }

    /// Load config from file only (no env overrides)
    fn load_from_file() -> Option<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return None;
        }

        let content = std::fs::read_to_string(&path).ok()?;
        match toml::from_str::<Self>(&content) {
            Ok(mut config) => {
                config.display.apply_legacy_compat();
                Some(config)
            }
            Err(e) => {
                crate::logging::error(&format!("Failed to parse config file: {}", e));
                None
            }
        }
    }

    /// Build the hook config effective for a working directory.
    ///
    /// Global hooks are loaded from `~/.jcode/config.toml`. Project hooks are appended from
    /// `<project>/.jcode/config.toml` and `<project>/.jcode/config.local.toml`, where `<project>`
    /// is the nearest ancestor containing a `.jcode` directory/config. This intentionally keeps
    /// the initial project-local merge narrow: hooks append, and `enabled` is true if any layer
    /// enables hooks.
    ///
    /// M9 fix: when the working_dir's nearest project-config path resolves to the same file as
    /// the global config (typical for `jcode` invocations launched from `~`), skip the project
    /// merge so each hook command is only registered once. Without this, every lifecycle / tool
    /// hook fires twice.
    pub fn hooks_for_working_dir(&self, working_dir: Option<&Path>) -> HooksConfig {
        let mut hooks = self.hooks.clone();
        if let Some(project_dir) = working_dir.and_then(Self::find_project_config_dir) {
            let global_path = Self::path();
            for config_path in [
                project_dir.join(".jcode").join("config.toml"),
                project_dir.join(".jcode").join("config.local.toml"),
            ] {
                // M9: skip if this project-config path is literally the global path
                // (canonical equality). Without this guard, running jcode from `~` or any
                // ancestor of `~/.jcode` re-loads the same hooks file twice.
                if Self::paths_resolve_to_same_file(global_path.as_deref(), &config_path) {
                    continue;
                }
                if let Some(project_hooks) = Self::load_hooks_from_file(&config_path) {
                    hooks.enabled |= project_hooks.enabled || !project_hooks.commands.is_empty();
                    hooks.commands.extend(project_hooks.commands);
                }
            }
        }
        hooks
    }

    /// True when both paths exist and resolve (via `canonicalize`, falling back to
    /// lexical normalization on error) to the same filesystem location. Used by M9
    /// hook-merge dedupe to detect "global config == project-discovered config".
    fn paths_resolve_to_same_file(a: Option<&Path>, b: &Path) -> bool {
        let Some(a) = a else { return false };
        let canon_a = std::fs::canonicalize(a).ok();
        let canon_b = std::fs::canonicalize(b).ok();
        match (canon_a, canon_b) {
            (Some(ca), Some(cb)) => ca == cb,
            _ => a == b,
        }
    }

    /// Build the prompt config effective for a working directory.
    ///
    /// Global prompt settings come from `~/.jcode/config.toml`. Project-local settings in
    /// `<project>/.jcode/config.toml` and `<project>/.jcode/config.local.toml` override only the
    /// prompt fields they explicitly set.
    pub fn prompt_for_working_dir(&self, working_dir: Option<&Path>) -> PromptConfig {
        let mut prompt = self.prompt.clone();
        if let Some(project_dir) = working_dir.and_then(Self::find_project_config_dir) {
            for config_path in [
                project_dir.join(".jcode").join("config.toml"),
                project_dir.join(".jcode").join("config.local.toml"),
            ] {
                if let Some(partial) = Self::load_partial_prompt_from_file(&config_path) {
                    partial.apply_to(&mut prompt);
                }
            }
        }
        prompt
    }

    /// Build the agent config effective for a working directory.
    ///
    /// Global agent settings come from `~/.jcode/config.toml`. Project-local settings in
    /// `<project>/.jcode/config.toml` and `<project>/.jcode/config.local.toml` overlay the global
    /// config. Map fields merge by key, with later layers overriding earlier ones. Scalar fields
    /// override only when explicitly set by the project config.
    pub fn agents_for_working_dir(&self, working_dir: Option<&Path>) -> AgentsConfig {
        let mut agents = self.agents.clone();

        for (name, profile) in crate::agent_profiles_md::load_global_jcode_agent_md() {
            agents.profiles.insert(name, profile);
        }

        for (name, profile) in crate::agent_profiles_md::load_project_local_agent_md(working_dir) {
            agents.profiles.insert(name, profile);
        }

        if let Some(project_dir) = working_dir.and_then(Self::find_project_config_dir) {
            for config_path in [
                project_dir.join(".jcode").join("config.toml"),
                project_dir.join(".jcode").join("config.local.toml"),
            ] {
                if let Some(project_agents) = Self::load_agents_from_file(&config_path) {
                    project_agents.apply_to(&mut agents);
                }
            }
        }
        agents
    }

    /// Build the swarm safety config effective for a working directory.
    ///
    /// Global swarm settings come from `~/.jcode/config.toml`. Project-local
    /// settings in `<project>/.jcode/config.toml` and
    /// `<project>/.jcode/config.local.toml` override only fields they explicitly
    /// set. Env vars are applied by the caller so they can stay highest priority.
    pub fn swarm_for_working_dir(&self, working_dir: Option<&Path>) -> SwarmConfig {
        let mut swarm = self.swarm.clone();
        if let Some(project_dir) = working_dir.and_then(Self::find_project_config_dir) {
            for config_path in [
                project_dir.join(".jcode").join("config.toml"),
                project_dir.join(".jcode").join("config.local.toml"),
            ] {
                if let Some(project_swarm) = Self::load_partial_swarm_from_file(&config_path) {
                    project_swarm.apply_to(&mut swarm);
                }
            }
        }
        swarm
    }

    fn find_project_config_dir(working_dir: &Path) -> Option<PathBuf> {
        let start = if working_dir.is_file() {
            working_dir.parent()?
        } else {
            working_dir
        };

        for ancestor in start.ancestors() {
            let project_config = ancestor.join(".jcode").join("config.toml");
            let local_config = ancestor.join(".jcode").join("config.local.toml");
            if project_config.exists() || local_config.exists() {
                return Some(ancestor.to_path_buf());
            }
        }
        None
    }

    fn load_hooks_from_file(path: &Path) -> Option<HooksConfig> {
        if !path.exists() {
            return None;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) => {
                crate::logging::warn(&format!(
                    "Failed to read project jcode config {}: {err}",
                    path.display()
                ));
                return None;
            }
        };

        match toml::from_str::<Config>(&content) {
            Ok(config) => Some(config.hooks),
            Err(err) => {
                crate::logging::warn(&format!(
                    "Failed to parse project jcode config {}: {err}",
                    path.display()
                ));
                None
            }
        }
    }

    fn load_partial_prompt_from_file(path: &Path) -> Option<PartialPromptConfig> {
        if !path.exists() {
            return None;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) => {
                crate::logging::warn(&format!(
                    "Failed to read project jcode config {}: {err}",
                    path.display()
                ));
                return None;
            }
        };

        match toml::from_str::<PartialConfig>(&content) {
            Ok(config) => Some(config.prompt),
            Err(err) => {
                crate::logging::warn(&format!(
                    "Failed to parse project jcode prompt config {}: {err}",
                    path.display()
                ));
                None
            }
        }
    }

    fn load_partial_swarm_from_file(path: &Path) -> Option<PartialSwarmConfig> {
        if !path.exists() {
            return None;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) => {
                crate::logging::warn(&format!(
                    "Failed to read project jcode config {}: {err}",
                    path.display()
                ));
                return None;
            }
        };

        match toml::from_str::<PartialConfig>(&content) {
            Ok(config) => Some(config.swarm),
            Err(err) => {
                crate::logging::warn(&format!(
                    "Failed to parse project jcode swarm config {}: {err}",
                    path.display()
                ));
                None
            }
        }
    }

    fn load_agents_from_file(path: &Path) -> Option<PartialAgentsConfig> {
        if !path.exists() {
            return None;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) => {
                crate::logging::warn(&format!(
                    "Failed to read project jcode config {}: {err}",
                    path.display()
                ));
                return None;
            }
        };

        let value = match toml::from_str::<toml::Value>(&content) {
            Ok(value) => value,
            Err(err) => {
                crate::logging::warn(&format!(
                    "Failed to parse project jcode agents config {}: {err}",
                    path.display()
                ));
                return None;
            }
        };
        let Some(agents) = value.get("agents").cloned() else {
            return None;
        };
        match agents.try_into::<PartialAgentsConfig>() {
            Ok(agents) => Some(agents),
            Err(err) => {
                crate::logging::warn(&format!(
                    "Failed to parse project jcode agents config {}: {err}",
                    path.display()
                ));
                None
            }
        }
    }

    /// Save config to file
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path().ok_or_else(|| anyhow::anyhow!("No config path"))?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// Update the copilot premium mode in the config file.
    /// Reloads, patches, and saves so it doesn't clobber other fields.
    pub fn set_copilot_premium(mode: Option<&str>) -> anyhow::Result<()> {
        let mut cfg = Self::load();
        cfg.provider.copilot_premium = mode.map(|s| s.to_string());
        cfg.save()?;
        crate::logging::info(&format!(
            "Saved copilot_premium to config: {}",
            mode.unwrap_or("(none)")
        ));
        Ok(())
    }

    /// Update just the default model and provider in the config file.
    /// This reloads, patches, and saves so it doesn't clobber other fields.
    pub fn set_default_model(model: Option<&str>, provider: Option<&str>) -> anyhow::Result<()> {
        let mut cfg = Self::load();
        cfg.provider.default_model = model.map(|s| s.to_string());
        cfg.provider.default_provider = provider.map(|s| s.to_string());
        cfg.save()?;

        // M19: previously this site had to live with the fact that the global
        // `OnceLock<Config>` could not be mutated, so the change only took
        // effect after restart. Now the global config hot-reloads on mtime
        // change; the `cfg.save()` above bumps mtime, so the next `config()`
        // call will pick up the new defaults automatically (within the
        // debounce window). Force an immediate reload so callers that read
        // the global config in the same turn see the new value.
        let _ = crate::config::force_reload_config();
        crate::logging::info(&format!(
            "Saved default model: {}, provider: {}",
            model.unwrap_or("(none)"),
            provider.unwrap_or("(auto)")
        ));
        Ok(())
    }

    /// Update just the default provider in the config file.
    pub fn set_default_provider(provider: Option<&str>) -> anyhow::Result<()> {
        let cfg = Self::load();
        Self::set_default_model(cfg.provider.default_model.as_deref(), provider)
    }

    /// Update just the default model in the config file.
    pub fn set_default_model_only(model: Option<&str>) -> anyhow::Result<()> {
        let cfg = Self::load();
        Self::set_default_model(model, cfg.provider.default_provider.as_deref())
    }

    /// Update the persisted OpenAI reasoning effort preference.
    pub fn set_openai_reasoning_effort(value: Option<&str>) -> anyhow::Result<()> {
        let mut cfg = Self::load();
        cfg.provider.openai_reasoning_effort = value.map(|s| s.to_string());
        cfg.save()?;
        crate::logging::info(&format!(
            "Saved openai_reasoning_effort to config: {}",
            value.unwrap_or("(none)")
        ));
        Ok(())
    }

    /// Update the persisted OpenAI transport preference.
    pub fn set_openai_transport(value: Option<&str>) -> anyhow::Result<()> {
        let mut cfg = Self::load();
        cfg.provider.openai_transport = value.map(|s| s.to_string());
        cfg.save()?;
        crate::logging::info(&format!(
            "Saved openai_transport to config: {}",
            value.unwrap_or("(none)")
        ));
        Ok(())
    }

    /// Update the persisted OpenAI service tier preference.
    pub fn set_openai_service_tier(value: Option<&str>) -> anyhow::Result<()> {
        let mut cfg = Self::load();
        cfg.provider.openai_service_tier = value.map(|s| s.to_string());
        cfg.save()?;
        crate::logging::info(&format!(
            "Saved openai_service_tier to config: {}",
            value.unwrap_or("(none)")
        ));
        Ok(())
    }

    /// Update the persisted default alignment preference.
    pub fn set_display_centered(centered: bool) -> anyhow::Result<()> {
        let mut cfg = Self::load();
        cfg.display.centered = centered;
        cfg.save()?;
        crate::logging::info(&format!("Saved display.centered to config: {}", centered));
        Ok(())
    }

    fn normalize_external_auth_source_id(source_id: &str) -> String {
        source_id.trim().to_ascii_lowercase()
    }

    pub(crate) fn trusted_external_auth_path_entry(
        source_id: &str,
        path: &std::path::Path,
    ) -> anyhow::Result<String> {
        let source_id = Self::normalize_external_auth_source_id(source_id);
        if source_id.is_empty() {
            anyhow::bail!("External auth source id cannot be empty");
        }
        let canonical = crate::storage::validate_external_auth_file(path)?;
        Ok(format!(
            "{}|{}",
            source_id,
            canonical.to_string_lossy().to_ascii_lowercase()
        ))
    }

    pub fn external_auth_source_allowed(source_id: &str) -> bool {
        let source_id = Self::normalize_external_auth_source_id(source_id);
        if source_id.is_empty() {
            return false;
        }

        let cfg = Self::load();
        cfg.auth
            .trusted_external_sources
            .iter()
            .any(|value| value.trim().eq_ignore_ascii_case(&source_id))
    }

    pub fn external_auth_source_allowed_for_path(source_id: &str, path: &std::path::Path) -> bool {
        let Ok(entry) = Self::trusted_external_auth_path_entry(source_id, path) else {
            return false;
        };

        let cfg = Self::load();
        cfg.auth
            .trusted_external_source_paths
            .iter()
            .any(|value| value.trim().eq_ignore_ascii_case(&entry))
    }

    /// Startup-sensitive variant that uses the process-cached config snapshot.
    ///
    /// This avoids reloading config.toml repeatedly during cold-start probes.
    pub fn external_auth_source_allowed_for_path_cached(
        source_id: &str,
        path: &std::path::Path,
    ) -> bool {
        let Ok(entry) = Self::trusted_external_auth_path_entry(source_id, path) else {
            return false;
        };

        config()
            .auth
            .trusted_external_source_paths
            .iter()
            .any(|value| value.trim().eq_ignore_ascii_case(&entry))
    }

    pub fn allow_external_auth_source(source_id: &str) -> anyhow::Result<()> {
        let source_id = Self::normalize_external_auth_source_id(source_id);
        if source_id.is_empty() {
            anyhow::bail!("External auth source id cannot be empty");
        }

        let mut cfg = Self::load();
        if !cfg
            .auth
            .trusted_external_sources
            .iter()
            .any(|value| value.trim().eq_ignore_ascii_case(&source_id))
        {
            cfg.auth.trusted_external_sources.push(source_id.clone());
            cfg.auth.trusted_external_sources.sort();
            cfg.auth.trusted_external_sources.dedup();
            cfg.save()?;
        }

        crate::logging::info(&format!(
            "Saved trusted external auth source to config: {}",
            source_id
        ));
        Ok(())
    }

    pub fn allow_external_auth_source_for_path(
        source_id: &str,
        path: &std::path::Path,
    ) -> anyhow::Result<()> {
        let entry = Self::trusted_external_auth_path_entry(source_id, path)?;
        let mut cfg = Self::load();
        if !cfg
            .auth
            .trusted_external_source_paths
            .iter()
            .any(|value| value.trim().eq_ignore_ascii_case(&entry))
        {
            cfg.auth.trusted_external_source_paths.push(entry.clone());
            cfg.auth.trusted_external_source_paths.sort();
            cfg.auth.trusted_external_source_paths.dedup();
            cfg.save()?;
        }
        crate::logging::info(&format!(
            "Saved trusted external auth source path: {}",
            entry
        ));
        Ok(())
    }

    pub fn revoke_external_auth_source_for_path(
        source_id: &str,
        path: &std::path::Path,
    ) -> anyhow::Result<()> {
        let entry = Self::trusted_external_auth_path_entry(source_id, path)?;
        let mut cfg = Self::load();
        let before = cfg.auth.trusted_external_source_paths.len();
        cfg.auth
            .trusted_external_source_paths
            .retain(|value| !value.trim().eq_ignore_ascii_case(&entry));
        if cfg.auth.trusted_external_source_paths.len() != before {
            cfg.save()?;
            crate::logging::info(&format!(
                "Removed trusted external auth source path: {}",
                entry
            ));
        }
        Ok(())
    }
}
