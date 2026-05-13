use super::{
    AgentRouteConfig, AmbientConfig, Config, DiffDisplayMode, DisplayConfig, HookCommandConfig,
    ProviderConfig, SessionPickerResumeAction,
};
use std::ffi::{OsStr, OsString};
use std::path::Path;

fn restore_env_var(key: &str, previous: Option<OsString>) {
    if let Some(previous) = previous {
        crate::env::set_var(key, previous);
    } else {
        crate::env::remove_var(key);
    }
}

struct EnvVarGuard {
    key: &'static str,
    prev: Option<OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
        let prev = std::env::var_os(key);
        crate::env::set_var(key, value.as_ref());
        Self { key, prev }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(prev) = self.prev.as_ref() {
            crate::env::set_var(self.key, prev);
        } else {
            crate::env::remove_var(self.key);
        }
    }
}

fn isolated_jcode_home() -> (
    std::sync::MutexGuard<'static, ()>,
    EnvVarGuard,
    tempfile::TempDir,
) {
    let lock = crate::storage::lock_test_env();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let guard = EnvVarGuard::set("JCODE_HOME", dir.path());
    (lock, guard, dir)
}

#[test]
fn test_openai_reasoning_effort_defaults_to_low() {
    assert_eq!(
        ProviderConfig::default().openai_reasoning_effort.as_deref(),
        Some("low")
    );
}

#[test]
fn test_openai_fast_mode_defaults_to_off() {
    // lazydino fork (Round 24): default OFF to avoid accidental fast usage.
    // Users can still opt in via /fast or by setting openai_service_tier in config.
    assert_eq!(
        ProviderConfig::default().openai_service_tier.as_deref(),
        None
    );
}

#[test]
fn test_generated_default_config_uses_low_openai_reasoning_effort() {
    let _guard = crate::storage::lock_test_env();
    let prev_home = std::env::var_os("JCODE_HOME");
    let dir = tempfile::TempDir::new().expect("tempdir");
    crate::env::set_var("JCODE_HOME", dir.path());

    let path = Config::create_default_config_file().expect("create default config file");
    let content = std::fs::read_to_string(path).expect("read default config file");

    assert!(
        content.contains("openai_reasoning_effort = \"low\""),
        "generated default config should use low OpenAI reasoning effort"
    );
    assert!(
        content.contains("# openai_service_tier = \"priority\""),
        "generated default config should leave OpenAI fast mode commented out by default (off)"
    );

    if let Some(prev) = prev_home {
        crate::env::set_var("JCODE_HOME", prev);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}

#[test]
fn global_config_cache_reloads_after_manual_file_edit() {
    let _guard = crate::storage::lock_test_env();
    let prev_home = std::env::var_os("JCODE_HOME");
    let dir = tempfile::TempDir::new().expect("tempdir");
    crate::env::set_var("JCODE_HOME", dir.path());
    Config::invalidate_cache();

    let path = Config::path().expect("config path");
    std::fs::create_dir_all(path.parent().expect("config parent")).expect("create config parent");
    std::fs::write(&path, "[display]\ncentered = false\n").expect("write initial config");

    assert!(!crate::config::config().display.centered);

    // Different length as well as mtime so the metadata fingerprint notices the
    // manual edit even on filesystems with coarse timestamp resolution.
    std::fs::write(&path, "[display]\ncentered = true\n# edited\n").expect("edit config");

    // `config()` debounces stat() calls (500ms). Two calls fired back-to-back
    // in a test will skip the reload-check on the second one. Tests that need
    // to observe manual file edits must bypass the debounce explicitly. This
    // mirrors the production path a user would take (`/reload-config` slash
    // command or an explicit `force_reload_config()` call).
    assert!(crate::config::force_reload_config());
    assert!(crate::config::config().display.centered);

    restore_env_var("JCODE_HOME", prev_home);
    Config::invalidate_cache();
}

#[test]
fn config_save_invalidates_global_config_cache() {
    let _guard = crate::storage::lock_test_env();
    let prev_home = std::env::var_os("JCODE_HOME");
    let dir = tempfile::TempDir::new().expect("tempdir");
    crate::env::set_var("JCODE_HOME", dir.path());
    Config::invalidate_cache();

    let mut cfg = Config::default();
    cfg.display.centered = false;
    cfg.save().expect("save initial config");
    assert!(!crate::config::config().display.centered);

    cfg.display.centered = true;
    cfg.save().expect("save updated config");
    assert!(crate::config::config().display.centered);

    restore_env_var("JCODE_HOME", prev_home);
    Config::invalidate_cache();
}

#[test]
fn cached_external_auth_trust_observes_manual_revocation() {
    let _guard = crate::storage::lock_test_env();
    let prev_home = std::env::var_os("JCODE_HOME");
    let dir = tempfile::TempDir::new().expect("tempdir");
    crate::env::set_var("JCODE_HOME", dir.path());
    Config::invalidate_cache();

    let auth_file = dir.path().join("external-auth.json");
    std::fs::write(&auth_file, "{}\n").expect("write external auth file");
    Config::allow_external_auth_source_for_path("test_source", &auth_file)
        .expect("trust external auth path");
    assert!(Config::external_auth_source_allowed_for_path_cached(
        "test_source",
        &auth_file
    ));

    let path = Config::path().expect("config path");
    std::fs::write(
        &path,
        "[auth]\ntrusted_external_source_paths = []\n# manually revoked\n",
    )
    .expect("manually revoke external auth trust");

    // Bypass the 500ms debounce — see comment in
    // `global_config_cache_reloads_after_manual_file_edit` above.
    assert!(crate::config::force_reload_config());

    assert!(!Config::external_auth_source_allowed_for_path_cached(
        "test_source",
        &auth_file
    ));

    restore_env_var("JCODE_HOME", prev_home);
    Config::invalidate_cache();
}

#[test]
fn test_ambient_visible_defaults_to_true() {
    assert!(AmbientConfig::default().visible);
}

#[test]
fn test_display_auto_server_reload_defaults_to_true() {
    assert!(DisplayConfig::default().auto_server_reload);
}

#[test]
fn test_display_alignment_defaults_to_left() {
    assert!(!DisplayConfig::default().centered);
}

#[test]
fn test_provider_failover_defaults_match_new_behavior() {
    let provider = Config::default().provider;
    assert_eq!(
        provider.cross_provider_failover,
        super::CrossProviderFailoverMode::Countdown
    );
    assert!(provider.same_provider_account_failover);
}

#[test]
fn test_native_scrollbars_default_to_enabled() {
    let display = DisplayConfig::default();
    assert!(display.native_scrollbars.chat);
    assert!(display.native_scrollbars.side_panel);
}

#[test]
fn test_session_picker_resume_action_defaults_to_new_terminal() {
    assert_eq!(
        Config::default().keybindings.session_picker_enter,
        SessionPickerResumeAction::NewTerminal
    );
    assert_eq!(
        SessionPickerResumeAction::NewTerminal.alternate(),
        SessionPickerResumeAction::CurrentTerminal
    );
}

#[test]
fn test_session_picker_resume_action_deserializes_kebab_case() {
    let cfg: Config = toml::from_str(
        r#"
        [keybindings]
        session_picker_enter = "current-terminal"
        "#,
    )
    .expect("config should deserialize");

    assert_eq!(
        cfg.keybindings.session_picker_enter,
        SessionPickerResumeAction::CurrentTerminal
    );
}

#[test]
fn test_agents_routing_deserializes_from_config() {
    let cfg: Config = toml::from_str(
        r#"
        [agents.profiles.planner]
        model = "claude-opus-4-7"
        variant = "max"
        description = "Architecture and planning coordinator"
        when = ["the task is ambiguous", "the implementation needs decomposition"]
        prompt = "Prefer a concise plan before implementation."

        [agents.routing]
        planner = "claude-opus-4-7"
        coder = "gpt-5.5"

        [agents.routes.hephaestus]
        model = "gpt-5.5"
        variant = "medium"
        "#,
    )
    .expect("config should deserialize");

    assert_eq!(
        cfg.agents.routing.get("planner").map(String::as_str),
        Some("claude-opus-4-7")
    );
    let profile = cfg.agents.profiles.get("planner").expect("profile config");
    assert_eq!(profile.model.as_deref(), Some("claude-opus-4-7"));
    assert_eq!(profile.variant.as_deref(), Some("max"));
    assert_eq!(
        profile.description.as_deref(),
        Some("Architecture and planning coordinator")
    );
    assert_eq!(
        profile.when,
        vec![
            "the task is ambiguous".to_string(),
            "the implementation needs decomposition".to_string()
        ]
    );
    assert_eq!(
        profile.prompt.as_deref(),
        Some("Prefer a concise plan before implementation.")
    );
    assert_eq!(
        cfg.agents.routing.get("coder").map(String::as_str),
        Some("gpt-5.5")
    );
    let route = cfg.agents.routes.get("hephaestus").expect("route config");
    assert_eq!(route.model.as_deref(), Some("gpt-5.5"));
    assert_eq!(route.variant.as_deref(), Some("medium"));
}

#[test]
fn test_prompt_config_deserializes_from_config() {
    let cfg: Config = toml::from_str(
        r#"
        [prompt]
        ignore_project_agents = true
        ignore_global_agents = true
        load_jcode_agents = false
        load_harness_dir = false
        private_instructions = ["rules/*.md", "extra.md"]
        "#,
    )
    .expect("config should deserialize");

    assert!(cfg.prompt.ignore_project_agents);
    assert!(cfg.prompt.ignore_global_agents);
    assert!(!cfg.prompt.load_jcode_agents);
    assert!(!cfg.prompt.load_harness_dir);
    assert_eq!(
        cfg.prompt.private_instructions,
        vec!["rules/*.md".to_string(), "extra.md".to_string()]
    );
}

#[test]
fn test_project_local_prompt_config_overrides_only_set_fields() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join(".jcode")).expect("create .jcode");
    std::fs::write(
        project.join(".jcode").join("config.toml"),
        r#"
        [prompt]
        ignore_project_agents = true
        "#,
    )
    .expect("write project config");

    let mut cfg = Config::default();
    cfg.prompt.load_jcode_agents = false;
    cfg.prompt.private_instructions = vec!["global.md".to_string()];

    let prompt = cfg.prompt_for_working_dir(Some(&project));
    assert!(prompt.ignore_project_agents);
    assert!(
        !prompt.load_jcode_agents,
        "unset project fields should preserve global config"
    );
    assert!(prompt.load_harness_dir);
    assert_eq!(prompt.private_instructions, vec!["global.md".to_string()]);
}

#[test]
fn test_project_local_prompt_config_overrides_private_instructions() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join(".jcode")).expect("create .jcode");
    std::fs::write(
        project.join(".jcode").join("config.local.toml"),
        r#"
        [prompt]
        private_instructions = ["rules/*.md"]
        "#,
    )
    .expect("write project config");

    let mut cfg = Config::default();
    cfg.prompt.private_instructions = vec!["global.md".to_string()];

    let prompt = cfg.prompt_for_working_dir(Some(&project));
    assert_eq!(prompt.private_instructions, vec!["rules/*.md".to_string()]);
}

#[test]
fn test_project_local_hooks_append_to_global_hooks() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let project = dir.path().join("project");
    let nested = project.join("src");
    std::fs::create_dir_all(project.join(".jcode")).expect("create .jcode");
    std::fs::create_dir_all(&nested).expect("create nested");
    std::fs::write(
        project.join(".jcode").join("config.toml"),
        r#"
        [hooks]
        enabled = true

        [[hooks.commands]]
        event = "tool.execute.before"
        tool = "bash"
        command = ".jcode/hooks/project-check.sh"
        blocking = true
        timeout_ms = 1234
        "#,
    )
    .expect("write project config");

    let mut cfg = Config::default();
    cfg.hooks.enabled = true;
    cfg.hooks.commands.push(HookCommandConfig {
        event: "tool.execute.after".to_string(),
        tool: Some("*".to_string()),
        command: "~/.jcode/hooks/log-tool.sh".to_string(),
        blocking: false,
        timeout_ms: 3000,
    });

    let hooks = cfg.hooks_for_working_dir(Some(&nested));
    assert!(hooks.enabled);
    assert_eq!(hooks.commands.len(), 2);
    assert_eq!(hooks.commands[0].command, "~/.jcode/hooks/log-tool.sh");
    assert_eq!(hooks.commands[1].command, ".jcode/hooks/project-check.sh");
    assert_eq!(hooks.commands[1].timeout_ms, 1234);
}

#[test]
fn test_project_local_config_local_appends_after_shared_config() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join(".jcode")).expect("create .jcode");
    std::fs::write(
        project.join(".jcode").join("config.toml"),
        r#"
        [hooks]
        enabled = true

        [[hooks.commands]]
        event = "tool.execute.before"
        command = "shared"
        "#,
    )
    .expect("write shared config");
    std::fs::write(
        project.join(".jcode").join("config.local.toml"),
        r#"
        [hooks]

        [[hooks.commands]]
        event = "tool.execute.after"
        command = "local"
        "#,
    )
    .expect("write local config");

    let cfg = Config::default();
    let hooks = cfg.hooks_for_working_dir(Some(&project));
    assert!(hooks.enabled);
    assert_eq!(
        hooks
            .commands
            .iter()
            .map(|hook| hook.command.as_str())
            .collect::<Vec<_>>(),
        vec!["shared", "local"]
    );
}

fn agent_route(model: &str) -> AgentRouteConfig {
    AgentRouteConfig {
        model: Some(model.to_string()),
        ..Default::default()
    }
}

#[test]
fn test_agents_for_working_dir_uses_global_when_no_project_config() {
    let _home = isolated_jcode_home();
    let mut cfg = Config::default();
    cfg.agents
        .profiles
        .insert("reviewer".to_string(), agent_route("opus"));

    let agents = cfg.agents_for_working_dir(None);

    assert_eq!(agents.profiles.len(), 1);
    assert_eq!(agents.profiles["reviewer"].model.as_deref(), Some("opus"));
}

#[test]
fn test_agents_for_working_dir_merges_project_profiles() {
    let _home = isolated_jcode_home();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join(".jcode")).expect("create .jcode");
    std::fs::write(
        project.join(".jcode").join("config.toml"),
        r#"
        [agents.profiles.coder]
        model = "gpt-5.5"
        "#,
    )
    .expect("write project config");

    let mut cfg = Config::default();
    cfg.agents
        .profiles
        .insert("reviewer".to_string(), agent_route("opus"));

    let agents = cfg.agents_for_working_dir(Some(&project));

    assert_eq!(agents.profiles["reviewer"].model.as_deref(), Some("opus"));
    assert_eq!(agents.profiles["coder"].model.as_deref(), Some("gpt-5.5"));
}

#[test]
fn test_agents_for_working_dir_project_overrides_global_same_key() {
    let _home = isolated_jcode_home();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join(".jcode")).expect("create .jcode");
    std::fs::write(
        project.join(".jcode").join("config.toml"),
        r#"
        [agents.profiles.reviewer]
        model = "haiku"
        "#,
    )
    .expect("write project config");

    let mut cfg = Config::default();
    cfg.agents
        .profiles
        .insert("reviewer".to_string(), agent_route("opus"));

    let agents = cfg.agents_for_working_dir(Some(&project));

    assert_eq!(agents.profiles["reviewer"].model.as_deref(), Some("haiku"));
}

#[test]
fn test_agents_for_working_dir_local_overrides_project_overrides_global() {
    let _home = isolated_jcode_home();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join(".jcode")).expect("create .jcode");
    std::fs::write(
        project.join(".jcode").join("config.toml"),
        r#"
        [agents.profiles.reviewer]
        model = "haiku"
        "#,
    )
    .expect("write project config");
    std::fs::write(
        project.join(".jcode").join("config.local.toml"),
        r#"
        [agents.profiles.reviewer]
        model = "sonnet"
        "#,
    )
    .expect("write local config");

    let mut cfg = Config::default();
    cfg.agents
        .profiles
        .insert("reviewer".to_string(), agent_route("opus"));

    let agents = cfg.agents_for_working_dir(Some(&project));

    assert_eq!(agents.profiles["reviewer"].model.as_deref(), Some("sonnet"));
}

#[test]
fn test_agents_for_working_dir_swarm_model_project_override() {
    let _home = isolated_jcode_home();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join(".jcode")).expect("create .jcode");
    std::fs::write(
        project.join(".jcode").join("config.toml"),
        r#"
        [agents]
        swarm_model = "haiku"
        "#,
    )
    .expect("write project config");

    let mut cfg = Config::default();
    cfg.agents.swarm_model = Some("opus".to_string());

    let agents = cfg.agents_for_working_dir(Some(&project));

    assert_eq!(agents.swarm_model.as_deref(), Some("haiku"));
}

#[test]
fn test_agents_for_working_dir_max_lifecycle_deny_streak_project_override() {
    let _home = isolated_jcode_home();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join(".jcode")).expect("create .jcode");
    std::fs::write(
        project.join(".jcode").join("config.toml"),
        r#"
        [agents]
        max_lifecycle_deny_streak = 1
        "#,
    )
    .expect("write project config");

    let mut cfg = Config::default();
    cfg.agents.max_lifecycle_deny_streak = Some(10);

    let agents = cfg.agents_for_working_dir(Some(&project));

    assert_eq!(agents.max_lifecycle_deny_streak, Some(1));
}

#[test]
fn test_agents_default_allow_subagent_recursion_is_false() {
    let cfg = Config::default();
    assert!(!cfg.agents.allow_subagent_recursion);
}

#[test]
fn test_agents_for_working_dir_allow_subagent_recursion_project_override() {
    let _home = isolated_jcode_home();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join(".jcode")).expect("create .jcode");
    std::fs::write(
        project.join(".jcode").join("config.toml"),
        r#"
        [agents]
        allow_subagent_recursion = true
        "#,
    )
    .expect("write project config");

    let cfg = Config::default();
    assert!(!cfg.agents.allow_subagent_recursion);

    let agents = cfg.agents_for_working_dir(Some(&project));
    assert!(agents.allow_subagent_recursion);
}

#[test]
fn test_provider_default_openai_parallel_tool_calls_is_true() {
    let cfg = Config::default();
    assert!(cfg.provider.openai_parallel_tool_calls);
}

#[test]
fn test_provider_openai_parallel_tool_calls_env_override_false() {
    use std::sync::Mutex;
    static GUARD: Mutex<()> = Mutex::new(());
    let _lock = GUARD.lock().unwrap_or_else(|p| p.into_inner());

    let _home = isolated_jcode_home();
    let _env = EnvVarGuard::set("JCODE_OPENAI_PARALLEL_TOOL_CALLS", "0");

    let mut cfg = Config::default();
    assert!(cfg.provider.openai_parallel_tool_calls); // baseline before env apply
    cfg.apply_env_overrides();
    assert!(!cfg.provider.openai_parallel_tool_calls);
}

#[test]
fn test_agents_for_working_dir_routes_and_routing_also_merge() {
    let _home = isolated_jcode_home();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join(".jcode")).expect("create .jcode");
    std::fs::write(
        project.join(".jcode").join("config.toml"),
        r#"
        [agents.routing]
        local_legacy = "haiku"

        [agents.routes.local_rich]
        model = "sonnet"
        "#,
    )
    .expect("write project config");

    let mut cfg = Config::default();
    cfg.agents
        .routing
        .insert("global_legacy".to_string(), "opus".to_string());
    cfg.agents
        .routes
        .insert("global_rich".to_string(), agent_route("opus"));

    let agents = cfg.agents_for_working_dir(Some(&project));

    assert_eq!(
        agents.routing.get("global_legacy").map(String::as_str),
        Some("opus")
    );
    assert_eq!(
        agents.routing.get("local_legacy").map(String::as_str),
        Some("haiku")
    );
    assert_eq!(agents.routes["global_rich"].model.as_deref(), Some("opus"));
    assert_eq!(agents.routes["local_rich"].model.as_deref(), Some("sonnet"));
}

#[test]
fn test_agents_for_working_dir_missing_project_files_fallback_to_global() {
    let _home = isolated_jcode_home();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).expect("create project");

    let mut cfg = Config::default();
    cfg.agents.swarm_model = Some("opus".to_string());
    cfg.agents
        .profiles
        .insert("reviewer".to_string(), agent_route("opus"));

    let agents = cfg.agents_for_working_dir(Some(&project));

    assert_eq!(agents.swarm_model.as_deref(), Some("opus"));
    assert_eq!(agents.profiles["reviewer"].model.as_deref(), Some("opus"));
}

#[test]
fn test_agents_for_working_dir_invalid_project_toml_logs_and_falls_back() {
    let _home = isolated_jcode_home();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join(".jcode")).expect("create .jcode");
    std::fs::write(
        project.join(".jcode").join("config.toml"),
        "[agents.profiles.reviewer\nmodel = \"haiku\"\n",
    )
    .expect("write malformed project config");

    let mut cfg = Config::default();
    cfg.agents
        .profiles
        .insert("reviewer".to_string(), agent_route("opus"));

    let agents = cfg.agents_for_working_dir(Some(&project));

    assert_eq!(agents.profiles["reviewer"].model.as_deref(), Some("opus"));
}

fn write_agent_md(project: &std::path::Path, relative_dir: &str, file: &str, content: &str) {
    let dir = project.join(relative_dir);
    std::fs::create_dir_all(&dir).expect("create agent dir");
    std::fs::write(dir.join(file), content).expect("write agent md");
}

#[test]
fn test_agents_for_working_dir_loads_jcode_md_agents() {
    let _home = isolated_jcode_home();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let project = dir.path().join("project");
    write_agent_md(
        &project,
        ".jcode/agents",
        "reviewer.md",
        "---\nname: reviewer\nmodel: opus\n---\nReview code.",
    );

    let cfg = Config::default();
    let agents = cfg.agents_for_working_dir(Some(&project));

    assert_eq!(agents.profiles["reviewer"].model.as_deref(), Some("opus"));
}

#[test]
fn test_agents_for_working_dir_loads_md_agents_from_all_four_ecosystems() {
    let _home = isolated_jcode_home();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let project = dir.path().join("project");
    write_agent_md(
        &project,
        ".jcode/agents",
        "jcode.md",
        "---\nmodel: opus\n---\nJ",
    );
    write_agent_md(
        &project,
        ".claude/agents",
        "claude.md",
        "---\nmodel: sonnet\n---\nC",
    );
    write_agent_md(
        &project,
        ".agents/agents",
        "agents.md",
        "---\nmodel: haiku\n---\nA",
    );
    write_agent_md(
        &project,
        ".opencode/agents",
        "opencode.md",
        "---\nmodel: gpt\n---\nO",
    );

    let cfg = Config::default();
    let agents = cfg.agents_for_working_dir(Some(&project));

    assert!(agents.profiles.contains_key("jcode"));
    assert!(agents.profiles.contains_key("claude"));
    assert!(agents.profiles.contains_key("agents"));
    assert!(agents.profiles.contains_key("opencode"));
}

#[test]
fn test_agents_for_working_dir_md_overridden_by_project_toml() {
    let _home = isolated_jcode_home();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let project = dir.path().join("project");
    write_agent_md(
        &project,
        ".jcode/agents",
        "x.md",
        "---\nname: x\nmodel: opus\n---\nX",
    );
    std::fs::write(
        project.join(".jcode").join("config.toml"),
        r#"
        [agents.profiles.x]
        model = "haiku"
        "#,
    )
    .expect("write project config");

    let cfg = Config::default();
    let agents = cfg.agents_for_working_dir(Some(&project));

    assert_eq!(agents.profiles["x"].model.as_deref(), Some("haiku"));
}

#[test]
fn test_agents_for_working_dir_md_filename_stem_becomes_name_when_no_frontmatter_name() {
    let _home = isolated_jcode_home();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let project = dir.path().join("project");
    write_agent_md(
        &project,
        ".jcode/agents",
        "coder.md",
        "---\ndescription: Coder\n---\nCode things.",
    );

    let cfg = Config::default();
    let agents = cfg.agents_for_working_dir(Some(&project));

    assert!(agents.profiles.contains_key("coder"));
    assert_eq!(
        agents.profiles["coder"].description.as_deref(),
        Some("Coder")
    );
}

#[test]
fn test_agents_for_working_dir_md_no_frontmatter_uses_body_as_prompt() {
    let _home = isolated_jcode_home();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let project = dir.path().join("project");
    write_agent_md(&project, ".jcode/agents", "bare.md", "You are a bare agent");

    let cfg = Config::default();
    let agents = cfg.agents_for_working_dir(Some(&project));

    assert_eq!(
        agents.profiles["bare"].prompt.as_deref(),
        Some("You are a bare agent")
    );
}

#[test]
fn test_agents_for_working_dir_md_aliases_resolve() {
    let _home = isolated_jcode_home();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let project = dir.path().join("project");
    write_agent_md(
        &project,
        ".jcode/agents",
        "reasoner.md",
        "---\nreasoning-effort: high\nwhen_to_use: use when needed\ndesc: Reasoner\n---\nThink.",
    );

    let cfg = Config::default();
    let agents = cfg.agents_for_working_dir(Some(&project));

    assert_eq!(agents.profiles["reasoner"].effort.as_deref(), Some("high"));
    assert_eq!(agents.profiles["reasoner"].when, vec!["use when needed"]);
    assert_eq!(
        agents.profiles["reasoner"].description.as_deref(),
        Some("Reasoner")
    );
}

#[test]
fn test_agents_for_working_dir_md_unknown_fields_ignored() {
    let _home = isolated_jcode_home();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let project = dir.path().join("project");
    write_agent_md(
        &project,
        ".jcode/agents",
        "tools.md",
        "---\nmodel: opus\nallowed-tools: [read, bash]\n---\nUse tools.",
    );

    let cfg = Config::default();
    let agents = cfg.agents_for_working_dir(Some(&project));

    assert_eq!(agents.profiles["tools"].model.as_deref(), Some("opus"));
}

#[test]
fn test_agents_for_working_dir_md_invalid_file_skipped_other_files_loaded() {
    let _home = isolated_jcode_home();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let project = dir.path().join("project");
    write_agent_md(
        &project,
        ".jcode/agents",
        "bad.md",
        "---\nname: bad\nmodel: opus\nBody without closing marker",
    );
    write_agent_md(
        &project,
        ".jcode/agents",
        "good.md",
        "---\nname: good\nmodel: haiku\n---\nGood.",
    );

    let cfg = Config::default();
    let agents = cfg.agents_for_working_dir(Some(&project));

    assert!(!agents.profiles.contains_key("bad"));
    assert_eq!(agents.profiles["good"].model.as_deref(), Some("haiku"));
}

#[test]
fn test_agents_for_working_dir_md_within_ecosystem_priority() {
    let _home = isolated_jcode_home();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let project = dir.path().join("project");
    write_agent_md(
        &project,
        ".jcode/agents",
        "x.md",
        "---\nname: x\nmodel: opus\n---\nJ",
    );
    write_agent_md(
        &project,
        ".opencode/agents",
        "x.md",
        "---\nname: x\nmodel: haiku\n---\nO",
    );

    let cfg = Config::default();
    let agents = cfg.agents_for_working_dir(Some(&project));

    assert_eq!(agents.profiles["x"].model.as_deref(), Some("opus"));
}

#[test]
fn test_env_override_auto_server_reload() {
    let _guard = crate::storage::lock_test_env();
    let prev = std::env::var_os("JCODE_AUTO_SERVER_RELOAD");
    crate::env::set_var("JCODE_AUTO_SERVER_RELOAD", "false");

    let mut cfg = Config::default();
    cfg.apply_env_overrides();

    assert!(!cfg.display.auto_server_reload);

    if let Some(prev) = prev {
        crate::env::set_var("JCODE_AUTO_SERVER_RELOAD", prev);
    } else {
        crate::env::remove_var("JCODE_AUTO_SERVER_RELOAD");
    }
}

#[test]
fn test_env_override_native_scrollbars() {
    let _guard = crate::storage::lock_test_env();
    let prev_chat = std::env::var_os("JCODE_CHAT_NATIVE_SCROLLBAR");
    let prev_side = std::env::var_os("JCODE_SIDE_PANEL_NATIVE_SCROLLBAR");
    crate::env::set_var("JCODE_CHAT_NATIVE_SCROLLBAR", "true");
    crate::env::set_var("JCODE_SIDE_PANEL_NATIVE_SCROLLBAR", "false");

    let mut cfg = Config::default();
    cfg.apply_env_overrides();

    assert!(cfg.display.native_scrollbars.chat);
    assert!(!cfg.display.native_scrollbars.side_panel);

    if let Some(prev) = prev_chat {
        crate::env::set_var("JCODE_CHAT_NATIVE_SCROLLBAR", prev);
    } else {
        crate::env::remove_var("JCODE_CHAT_NATIVE_SCROLLBAR");
    }
    if let Some(prev) = prev_side {
        crate::env::set_var("JCODE_SIDE_PANEL_NATIVE_SCROLLBAR", prev);
    } else {
        crate::env::remove_var("JCODE_SIDE_PANEL_NATIVE_SCROLLBAR");
    }
}

#[test]
fn test_env_override_diff_mode_full_inline() {
    let _guard = crate::storage::lock_test_env();
    let prev = std::env::var_os("JCODE_DIFF_MODE");
    crate::env::set_var("JCODE_DIFF_MODE", "full-inline");

    let mut cfg = Config::default();
    cfg.apply_env_overrides();

    assert_eq!(cfg.display.diff_mode, DiffDisplayMode::FullInline);

    if let Some(prev) = prev {
        crate::env::set_var("JCODE_DIFF_MODE", prev);
    } else {
        crate::env::remove_var("JCODE_DIFF_MODE");
    }
}

#[test]
fn test_env_override_trusted_external_auth_splits_source_and_path_entries() {
    let _guard = crate::storage::lock_test_env();
    let prev = std::env::var_os("JCODE_TRUSTED_EXTERNAL_AUTH_SOURCES");
    crate::env::set_var(
        "JCODE_TRUSTED_EXTERNAL_AUTH_SOURCES",
        "legacy_source,claude_code_credentials|/tmp/auth.json",
    );

    let mut cfg = Config::default();
    cfg.apply_env_overrides();

    assert_eq!(cfg.auth.trusted_external_sources, vec!["legacy_source"]);
    assert_eq!(
        cfg.auth.trusted_external_source_paths,
        vec!["claude_code_credentials|/tmp/auth.json"]
    );

    if let Some(prev) = prev {
        crate::env::set_var("JCODE_TRUSTED_EXTERNAL_AUTH_SOURCES", prev);
    } else {
        crate::env::remove_var("JCODE_TRUSTED_EXTERNAL_AUTH_SOURCES");
    }
}

#[test]
fn test_external_auth_source_allowed_for_path_matches_saved_entry() {
    let _guard = crate::storage::lock_test_env();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = dir.path().join("auth.json");
    std::fs::write(&path, "{}\n").expect("write auth file");

    let canonical = std::fs::canonicalize(&path).expect("canonical path");
    let mut cfg = Config::default();
    cfg.auth.trusted_external_source_paths = vec![format!(
        "test_source|{}",
        canonical.to_string_lossy().to_ascii_lowercase()
    )];

    assert!(cfg.external_auth_source_allowed_for_path_config("test_source", &path));
}

#[test]
fn test_external_auth_source_allowed_for_path_ignores_broad_legacy_entry() {
    let _guard = crate::storage::lock_test_env();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = dir.path().join("auth.json");
    std::fs::write(&path, "{}\n").expect("write auth file");

    let mut cfg = Config::default();
    cfg.auth.trusted_external_sources = vec!["test_source".to_string()];

    assert!(!cfg.external_auth_source_allowed_for_path_config("test_source", &path));
}

impl Config {
    fn external_auth_source_allowed_for_path_config(&self, source_id: &str, path: &Path) -> bool {
        let Ok(entry) = Self::trusted_external_auth_path_entry(source_id, path) else {
            return false;
        };
        self.auth
            .trusted_external_source_paths
            .iter()
            .any(|value| value.trim().eq_ignore_ascii_case(&entry))
    }
}

// ─────────────────────────────────────────────────────────────────────────
// M9 regression: hook double-fire when global config path equals project
// config path (typical for `jcode` invocations launched from `~`).
// ─────────────────────────────────────────────────────────────────────────

/// M9 regression. When `JCODE_HOME` and the project root resolve to the same
/// directory, `hooks_for_working_dir` must NOT re-merge the same hooks file,
/// otherwise every lifecycle/tool hook command fires twice.
///
/// Repro before fix: 1 global hook + same-path "project" hook → 2 commands
/// in the merged result. After fix: 1 command (project merge skipped because
/// the discovered path canonicalizes to the global path).
#[test]
fn test_hooks_for_working_dir_dedupes_when_global_path_equals_project_path() {
    let _guard = crate::storage::lock_test_env();
    let prev_home = std::env::var_os("JCODE_HOME");

    let dir = tempfile::TempDir::new().expect("tempdir");
    let project_root = dir.path().to_path_buf();
    let jcode_home = project_root.join(".jcode");
    std::fs::create_dir_all(&jcode_home).expect("mkdir JCODE_HOME");
    crate::env::set_var("JCODE_HOME", &jcode_home);

    // Write the file at JCODE_HOME/config.toml. Because JCODE_HOME ==
    // <project_root>/.jcode, `find_project_config_dir(project_root)` discovers
    // the same `<project_root>/.jcode/config.toml` that `Config::path()` returns.
    std::fs::write(
        jcode_home.join("config.toml"),
        r#"
        [hooks]
        enabled = true

        [[hooks.commands]]
        event = "tool.execute.after"
        tool = "*"
        command = "log-tool.sh"
        blocking = false
        timeout_ms = 1000
        "#,
    )
    .expect("write JCODE_HOME/config.toml");

    // Build a Config that mirrors what Self::load() would yield from this file.
    let mut cfg = Config::default();
    cfg.hooks.enabled = true;
    cfg.hooks.commands.push(HookCommandConfig {
        event: "tool.execute.after".to_string(),
        tool: Some("*".to_string()),
        command: "log-tool.sh".to_string(),
        blocking: false,
        timeout_ms: 1000,
    });

    // working_dir == project_root so find_project_config_dir(project_root)
    // returns project_root and discovers `<project_root>/.jcode/config.toml`,
    // which canonically equals `Config::path()` (== JCODE_HOME/config.toml).
    let hooks = cfg.hooks_for_working_dir(Some(&project_root));

    assert_eq!(
        hooks.commands.len(),
        1,
        "M9: hooks must not be merged twice when project config path equals global config path; got commands: {:?}",
        hooks
            .commands
            .iter()
            .map(|c| &c.command)
            .collect::<Vec<_>>()
    );

    if let Some(prev) = prev_home {
        crate::env::set_var("JCODE_HOME", prev);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}

/// M9 sanity: a *different* project path must still merge (we don't want the
/// dedupe to suppress legitimate project-local hooks).
#[test]
fn test_hooks_for_working_dir_still_merges_distinct_project_path() {
    let _guard = crate::storage::lock_test_env();
    let prev_home = std::env::var_os("JCODE_HOME");

    let global_dir = tempfile::TempDir::new().expect("global tempdir");
    let project_dir = tempfile::TempDir::new().expect("project tempdir");
    let global_home = global_dir.path().join(".jcode");
    std::fs::create_dir_all(&global_home).expect("mkdir global .jcode");
    crate::env::set_var("JCODE_HOME", &global_home);

    std::fs::write(global_home.join("config.toml"), "[hooks]\nenabled = true\n")
        .expect("write global");
    std::fs::create_dir_all(project_dir.path().join(".jcode")).expect("mkdir .jcode");
    std::fs::write(
        project_dir.path().join(".jcode").join("config.toml"),
        r#"
        [hooks]
        enabled = true

        [[hooks.commands]]
        event = "tool.execute.before"
        tool = "bash"
        command = "project-only.sh"
        blocking = true
        timeout_ms = 1
        "#,
    )
    .expect("write project");

    let mut cfg = Config::default();
    cfg.hooks.enabled = true;
    cfg.hooks.commands.push(HookCommandConfig {
        event: "tool.execute.after".to_string(),
        tool: Some("*".to_string()),
        command: "global.sh".to_string(),
        blocking: false,
        timeout_ms: 0,
    });

    let hooks = cfg.hooks_for_working_dir(Some(project_dir.path()));
    assert_eq!(hooks.commands.len(), 2, "distinct paths must still merge");

    if let Some(prev) = prev_home {
        crate::env::set_var("JCODE_HOME", prev);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}

// =============================================================================
// M19: Config hot-reload tests
// =============================================================================

/// Helper: write a config.toml that sets a distinguishing field (here we use
/// `display.pin_images`, a plain bool that's easy to read back). Bumps
/// mtime by sleeping briefly so the watcher detects the change reliably even
/// on filesystems with low timestamp granularity.
#[cfg(test)]
fn m19_write_config(home: &Path, body: &str) {
    let path = home.join("config.toml");
    std::fs::write(&path, body).expect("write config.toml");
    // Some filesystems (e.g. tmpfs on Linux) have nanosecond mtime, but
    // others (notably macOS APFS in older kernels, and some CI runners) only
    // have second-level granularity. A 10ms sleep before each write is enough
    // for nanosecond-mtime FS; for second-level FS the test still passes
    // because we use `force_reload_config()` which bypasses debounce, and the
    // `observed_modified` comparison would yield "old vs new" on any whole-
    // second boundary cross. We don't sleep here; tests that need a guaranteed
    // mtime change can call `m19_bump_mtime` instead.
    let _ = path;
}

/// Helper: ensure mtime moves forward by at least one whole second, for FS
/// with second-level timestamp granularity. Prefer this when a test needs a
/// guaranteed mtime change between two writes.
#[cfg(test)]
fn m19_bump_mtime_then_write(home: &Path, body: &str) {
    std::thread::sleep(std::time::Duration::from_millis(1100));
    m19_write_config(home, body);
}

/// Sanity test: with a freshly-prepared `JCODE_HOME` and a config file that
/// turns `display.pin_images` on, the global `config()` returns true
/// after `reset_config_cache_for_tests()`.
#[test]
fn test_m19_config_initial_load_after_reset() {
    let _guard = crate::storage::lock_test_env();
    let prev_home = std::env::var_os("JCODE_HOME");
    let dir = tempfile::TempDir::new().expect("tempdir");
    crate::env::set_var("JCODE_HOME", dir.path());

    m19_write_config(dir.path(), "[display]\npin_images = true\n");
    crate::config::reset_config_cache_for_tests();

    let cfg = crate::config::config();
    assert!(
        cfg.display.pin_images,
        "expected pin_images=true after initial load"
    );

    if let Some(prev) = prev_home {
        crate::env::set_var("JCODE_HOME", prev);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}

/// Core M19 contract: editing the config file at runtime is reflected by a
/// subsequent `config()` call (after `force_reload_config` to bypass the
/// 500ms debounce window).
#[test]
fn test_m19_config_reloads_when_mtime_changes() {
    let _guard = crate::storage::lock_test_env();
    let prev_home = std::env::var_os("JCODE_HOME");
    let dir = tempfile::TempDir::new().expect("tempdir");
    crate::env::set_var("JCODE_HOME", dir.path());

    // Initial: feature off
    m19_write_config(dir.path(), "[display]\npin_images = false\n");
    crate::config::reset_config_cache_for_tests();
    assert!(
        !crate::config::config().display.pin_images,
        "expected pin_images=false initially"
    );

    // Edit on disk: feature on
    m19_bump_mtime_then_write(dir.path(), "[display]\npin_images = true\n");
    let did_reload = crate::config::force_reload_config();
    assert!(
        did_reload,
        "force_reload_config should report a swap on mtime change"
    );
    assert!(
        crate::config::config().display.pin_images,
        "expected pin_images=true after mtime change + force_reload"
    );

    if let Some(prev) = prev_home {
        crate::env::set_var("JCODE_HOME", prev);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}

/// Robustness: when the config file becomes invalid (e.g. mid-edit save with
/// broken TOML), the previous valid snapshot is retained — `config()` does
/// NOT silently fall back to defaults.
#[test]
fn test_m19_config_keeps_last_good_on_invalid_toml() {
    let _guard = crate::storage::lock_test_env();
    let prev_home = std::env::var_os("JCODE_HOME");
    let dir = tempfile::TempDir::new().expect("tempdir");
    crate::env::set_var("JCODE_HOME", dir.path());

    // Initial valid: feature on
    m19_write_config(dir.path(), "[display]\npin_images = true\n");
    crate::config::reset_config_cache_for_tests();
    assert!(
        crate::config::config().display.pin_images,
        "expected pin_images=true initially"
    );

    // Replace with garbage TOML (mid-edit corruption).
    m19_bump_mtime_then_write(dir.path(), "this is not valid toml = = = [[[\n");
    let _ = crate::config::force_reload_config();

    // Last-good preserved: feature still on, NOT default(false).
    assert!(
        crate::config::config().display.pin_images,
        "expected last-good config retained on invalid TOML; got default fallback instead"
    );

    if let Some(prev) = prev_home {
        crate::env::set_var("JCODE_HOME", prev);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}

/// After a corrupt write, fixing the file should make the new value take
/// effect on the next reload (i.e. the broken state isn't sticky).
#[test]
fn test_m19_config_recovers_after_invalid_then_valid_write() {
    let _guard = crate::storage::lock_test_env();
    let prev_home = std::env::var_os("JCODE_HOME");
    let dir = tempfile::TempDir::new().expect("tempdir");
    crate::env::set_var("JCODE_HOME", dir.path());

    // Start: feature off
    m19_write_config(dir.path(), "[display]\npin_images = false\n");
    crate::config::reset_config_cache_for_tests();
    assert!(!crate::config::config().display.pin_images);

    // Corrupt
    m19_bump_mtime_then_write(dir.path(), "broken =====\n");
    let _ = crate::config::force_reload_config();
    assert!(
        !crate::config::config().display.pin_images,
        "expected last-good (false) after corrupt write"
    );

    // Fix with a new value (true)
    m19_bump_mtime_then_write(dir.path(), "[display]\npin_images = true\n");
    let did_reload = crate::config::force_reload_config();
    assert!(
        did_reload,
        "force_reload should swap to the recovered config"
    );
    assert!(
        crate::config::config().display.pin_images,
        "expected fresh value (true) after recovery write"
    );

    if let Some(prev) = prev_home {
        crate::env::set_var("JCODE_HOME", prev);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}

/// Idempotence: when nothing changes on disk, `force_reload_config` reports
/// no swap and `config()` keeps returning the same snapshot.
#[test]
fn test_m19_force_reload_is_noop_when_unchanged() {
    let _guard = crate::storage::lock_test_env();
    let prev_home = std::env::var_os("JCODE_HOME");
    let dir = tempfile::TempDir::new().expect("tempdir");
    crate::env::set_var("JCODE_HOME", dir.path());

    m19_write_config(dir.path(), "[display]\npin_images = true\n");
    crate::config::reset_config_cache_for_tests();
    let first = crate::config::config() as *const Config;

    // No edit. force_reload should observe same mtime and report no swap.
    let did_reload = crate::config::force_reload_config();
    assert!(
        !did_reload,
        "expected force_reload to report no swap when mtime unchanged"
    );
    let second = crate::config::config() as *const Config;
    assert!(
        std::ptr::eq(first, second),
        "expected same &'static Config pointer when nothing changed"
    );

    if let Some(prev) = prev_home {
        crate::env::set_var("JCODE_HOME", prev);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}
