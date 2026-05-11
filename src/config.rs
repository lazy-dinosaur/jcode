//! Configuration file support for jcode
//!
//! Config is loaded from `~/.jcode/config.toml` (or `$JCODE_HOME/config.toml`)
//! Environment variables override config file settings.

pub use jcode_config_types::{
    AgentRouteConfig, AgentsConfig, AmbientConfig, AuthConfig, AutoJudgeConfig, AutoReviewConfig,
    BashToolConfig, CompactionConfig, CompactionMode, CrossProviderFailoverMode,
    DiagramDisplayMode, DiagramPanePosition, DiffDisplayMode, DisplayConfig, FeatureConfig,
    GatewayConfig, HookCommandConfig, HooksConfig, KeybindingsConfig, MarkdownSpacingMode,
    NamedProviderAuth, NamedProviderConfig, NamedProviderModelConfig, NamedProviderType,
    NativeScrollbarConfig, PromptConfig, ProviderConfig, ReloadConfig, SafetyConfig,
    SessionPickerResumeAction, SwarmConfig, ToolConfig, UpdateChannel,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

/// M19: minimum interval between mtime stat checks. Without this, every
/// `config()` call would stat the file (`config()` is hot — called from render
/// paths, hook dispatch, etc.). 500 ms is fast enough that human edits feel
/// "instant" but slow enough that bursts of accesses don't pound the FS.
const CONFIG_RELOAD_DEBOUNCE: Duration = Duration::from_millis(500);

/// M19 cached config state behind a `Mutex`. The `current` reference is leaked
/// into a `&'static Config` so that `pub fn config() -> &'static Config` can
/// stay stable for the (84+) call sites without forcing every consumer to
/// switch to `Arc<Config>`. Memory cost is one leaked snapshot per accepted
/// reload — bounded by human edit frequency, well within budget for a
/// long-running server.
struct ConfigCache {
    /// Currently active config. Always points to a leaked, immutable `Config`.
    current: &'static Config,
    /// Path being watched (`Config::path()` at init time). `None` if no jcode
    /// dir was resolvable; in that case we never attempt reload.
    path: Option<PathBuf>,
    /// Last observed `mtime` of `path`. `None` means "file did not exist at
    /// last check" (so a future creation triggers reload).
    observed_modified: Option<SystemTime>,
    /// Last time we ran the stat-check (for debounce).
    last_checked: Instant,
}

static CONFIG_CACHE: OnceLock<Mutex<ConfigCache>> = OnceLock::new();

fn cache() -> &'static Mutex<ConfigCache> {
    CONFIG_CACHE.get_or_init(|| {
        let initial = Box::leak(Box::new(Config::load()));
        let path = Config::path();
        let observed_modified = path
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok());
        Mutex::new(ConfigCache {
            current: initial,
            path,
            observed_modified,
            last_checked: Instant::now(),
        })
    })
}

/// Get the global config instance.
///
/// M19: returns a `&'static Config` that is hot-reloaded whenever
/// `~/.jcode/config.toml` (or `$JCODE_HOME/config.toml`) is modified on disk.
/// The returned reference remains valid forever — older snapshots are never
/// reclaimed, so callers may freely hold this reference across reloads.
///
/// Reload policy:
/// - Stat the config file at most once every `CONFIG_RELOAD_DEBOUNCE`.
/// - If `mtime` changed (or the file just appeared / disappeared), reparse.
/// - On parse error, keep the previous snapshot (last-good).
/// - On parse success, replace `current` and observed mtime.
pub fn config() -> &'static Config {
    let cache = cache();
    let now = Instant::now();
    let mut guard = match cache.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };

    if now.duration_since(guard.last_checked) >= CONFIG_RELOAD_DEBOUNCE {
        guard.last_checked = now;
        maybe_reload(&mut guard);
    }

    guard.current
}

/// Force an immediate reload check, bypassing the debounce window.
///
/// Intended for tests and for explicit user-initiated reloads (future
/// `/reload-config` slash command). Returns `true` if a new snapshot was
/// installed.
pub fn force_reload_config() -> bool {
    let cache = cache();
    let mut guard = match cache.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.last_checked = Instant::now();
    let before_ptr = guard.current as *const Config;
    maybe_reload(&mut guard);
    let after_ptr = guard.current as *const Config;
    !std::ptr::eq(before_ptr, after_ptr)
}

/// M19: test-only helper that resets the config cache so the next `config()`
/// call re-reads from `Config::path()`. Required because `JCODE_HOME` may
/// change between tests, and the cache binds its watched path at init time.
///
/// Not exposed outside `cfg(test)` so production code cannot accidentally
/// invalidate `&'static Config` references that other code may hold.
#[cfg(test)]
pub fn reset_config_cache_for_tests() {
    let cache = cache();
    let mut guard = match cache.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let fresh_path = Config::path();
    let fresh_modified = fresh_path
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok());
    let fresh_config: &'static Config = Box::leak(Box::new(Config::load()));
    guard.current = fresh_config;
    guard.path = fresh_path;
    guard.observed_modified = fresh_modified;
    guard.last_checked = Instant::now();
}

fn maybe_reload(guard: &mut ConfigCache) {
    let Some(path) = guard.path.as_ref() else {
        return;
    };

    // Stat the file. If it doesn't exist *and* we previously observed it not
    // existing, no work to do. Otherwise, reload (file appeared, disappeared,
    // or mtime changed).
    let current_modified = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());

    if current_modified == guard.observed_modified {
        return;
    }

    // Something changed. Try to load. On any failure, keep last-good config
    // but record the new observed mtime so we don't retry every debounce
    // window for the same broken file.
    match Config::try_load() {
        Ok(new_config) => {
            let leaked: &'static Config = Box::leak(Box::new(new_config));
            guard.current = leaked;
            guard.observed_modified = current_modified;
            crate::logging::info(&format!(
                "config reloaded from {} (mtime changed)",
                path.display()
            ));
        }
        Err(e) => {
            // Keep last-good. Update observed mtime so a single broken save
            // doesn't make us retry-and-fail every debounce window — we only
            // try again when the user *changes* the file again.
            guard.observed_modified = current_modified;
            crate::logging::warn(&format!(
                "config reload skipped (parse error, keeping previous): {} ({})",
                e,
                path.display()
            ));
        }
    }
}

/// Main configuration struct
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    /// Keybinding configuration
    pub keybindings: KeybindingsConfig,

    /// External dictation / speech-to-text integration
    pub dictation: DictationConfig,

    /// Display/UI configuration
    pub display: DisplayConfig,

    /// Feature toggles
    pub features: FeatureConfig,

    /// Auth trust / consent configuration
    pub auth: AuthConfig,

    /// Provider configuration
    pub provider: ProviderConfig,

    /// Named provider profiles, keyed by profile name.
    ///
    /// Example:
    /// [providers.my-gateway]
    /// type = "openai-compatible"
    /// base_url = "https://llm.example.com/v1"
    /// api_key_env = "MY_GATEWAY_API_KEY"
    pub providers: BTreeMap<String, NamedProviderConfig>,

    /// Agent-specific model defaults
    pub agents: AgentsConfig,

    /// Swarm coordination safety configuration
    pub swarm: SwarmConfig,

    /// Prompt and project instruction loading configuration
    pub prompt: PromptConfig,

    /// Ambient mode configuration
    pub ambient: AmbientConfig,

    /// Safety / notification configuration
    pub safety: SafetyConfig,

    /// WebSocket gateway configuration (for iOS/web clients)
    pub gateway: GatewayConfig,

    /// Compaction configuration
    pub compaction: CompactionConfig,

    /// Reload/reconnect recovery configuration
    pub reload: ReloadConfig,

    /// Auto-review configuration
    pub autoreview: AutoReviewConfig,

    /// Auto-judge configuration
    pub autojudge: AutoJudgeConfig,

    /// Hook configuration for tool lifecycle events
    pub hooks: HooksConfig,

    /// Per-tool configuration (M20: bash timeouts, etc.)
    pub tool: ToolConfig,
}

/// External dictation / speech-to-text integration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DictationConfig {
    /// Shell command to run. Must print the transcript to stdout.
    pub command: String,
    /// How to apply the resulting transcript.
    pub mode: crate::protocol::TranscriptMode,
    /// Optional in-app hotkey to trigger dictation.
    pub key: String,
    /// Maximum time to wait for the command to finish (0 = no timeout).
    pub timeout_secs: u64,
}

impl Default for DictationConfig {
    fn default() -> Self {
        Self {
            command: String::new(),
            mode: crate::protocol::TranscriptMode::Send,
            key: "off".to_string(),
            timeout_secs: 90,
        }
    }
}

mod config_file;
mod default_file;
mod display_summary;
mod env_overrides;

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
