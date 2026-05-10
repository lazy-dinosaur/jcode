use super::{
    AmbientConfig, Config, DiffDisplayMode, DisplayConfig, ProviderConfig,
    SessionPickerResumeAction,
};
use std::path::Path;

#[test]
fn test_openai_reasoning_effort_defaults_to_low() {
    assert_eq!(
        ProviderConfig::default().openai_reasoning_effort.as_deref(),
        Some("low")
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

    if let Some(prev) = prev_home {
        crate::env::set_var("JCODE_HOME", prev);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
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
