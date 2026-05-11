# M19 Analysis: Config Hot-Reload

(searcher subagent, 2m4s, deploy/m9-m10 worktree)

## Current architecture

### Config::load() and storage

- `src/config.rs:17-24`

```rust
use std::sync::OnceLock;

static CONFIG: OnceLock<Config> = OnceLock::new();

/// Get the global config instance (loaded once on first access)
pub fn config() -> &'static Config {
    CONFIG.get_or_init(Config::load)
}
```

- `src/config/config_file.rs:64-75`

```rust
impl Config {
    pub fn path() -> Option<PathBuf> {
        jcode_dir().ok().map(|d| d.join("config.toml"))
    }

    pub fn load() -> Self {
        let mut config = Self::load_from_file().unwrap_or_default();
        config.apply_env_overrides();
        config
    }
```

- OnceLock type: `OnceLock<Config>`
- Result: first `config()` call permanently snapshots `$JCODE_HOME/config.toml`.

### Accessor pattern

- `crate::config::config()` returns `&'static Config`.
- 84 occurrences across 43 files.
- 6 additional files import `use crate::config::config;`.

### Existing file watcher code

- No Rust config watcher.
- No `notify` crate.
- Only `scripts/screenshot_watcher.sh` uses inotifywait (shell only).

### Cargo.toml relevant crates

- `notify`: Not present.
- `tokio`, `toml`, `serde`, `dirs`, `anyhow`: present.
- Standard library `std::fs::metadata().modified()` is enough for lazy mtime reload.

## Option A: notify-based file watcher

### Changes
- `Cargo.toml`: add `notify`
- `src/config.rs`: replace `OnceLock<Config>` with `OnceLock<ArcSwap/RwLock<Arc<Config>>>`
- Spawn watcher thread on first `config()` or on `serve` startup
- ~80-120 lines

### Backward compat
- `config() -> Arc<Config>` would break ~84 call sites.
- Preserving `&'static Config` requires leak strategy.

### Risk
- Higher dependency cost.
- Editor atomic-write (rename) requires watching parent dir.
- Partial writes / invalid TOML need fallback-to-last-good.

## Option B: lazy mtime check

### Changes
- `src/config.rs`: cache state with leaked `&'static Config` + last mtime + last check time
- `config()` calls `maybe_reload()` before returning
- ~30-50 lines

### Backward compat
- Can preserve `pub fn config() -> &'static Config` via `Box::leak`
- No call site changes

### Risk
- Per-call stat() — needs debounce
- mtime granularity could miss rapid edits

## Option C: explicit reload command

### Changes
- Add `Config::reload_global()` + slash command
- ~10-20 lines

### Backward compat
- Same storage issue as A/B

### Risk
- User must remember to run it

## Option D: hybrid (lazy mtime + debounce + leak)

### Changes
- Same as B + debounce timestamp
- Stat at most every CONFIG_RELOAD_CHECK_INTERVAL (e.g. 500ms)
- Expose `force_reload_config()` for tests / future `/reload-config`

### Backward compat
- Best — preserves `config() -> &'static Config`
- 84 accessor call sites untouched

### Risk
- Leaked snapshots on each reload (bounded by human edit frequency, acceptable)
- Need fallback-to-last-good on parse error
- Locking care to avoid deadlock if reload logs (and logging consults config)

## Recommendation: Option D

Smallest focused patch fixing long-running-server stale config without
touching dozens of call sites. Stdlib metadata only, no `notify` dep.
Memory leak is bounded by human edit frequency. Add fallback-to-last-good
so transient invalid TOML does not wipe config to defaults.

## Patch design

### Files changed
- `src/config.rs` — replace OnceLock<Config> with reloadable cache state
- `src/config/config_file.rs` — expose `try_load() -> anyhow::Result<Config>` to keep last-good on parse error
- `src/config_tests.rs` — hot-reload tests using temporary `JCODE_HOME`

### Internal shape

```rust
static CONFIG: OnceLock<std::sync::Mutex<CachedConfig>> = OnceLock::new();

struct CachedConfig {
    current: &'static Config,
    path: Option<PathBuf>,
    observed_modified: Option<SystemTime>,
    observed_exists: bool,
    last_checked: Instant,
}
```

### `config()` flow
1. initialize with leaked `Config::load()`
2. if debounce elapsed (e.g. 500ms), stat `Config::path()`
3. if mtime/existence changed, parse config
4. on success: leak and swap `current`
5. on failure: keep `current`, update `last_checked`, log warning
6. return `current`

### Tests
- `config_reloads_when_global_config_mtime_changes`
- `config_keeps_last_good_on_invalid_toml`
- `hooks_for_working_dir_uses_reloaded_global_hooks`

### Estimated lines
- `src/config.rs`: +80 to +130
- `src/config/config_file.rs`: +20 to +50
- tests: +80 to +140
- total: ~180 to 320 LOC

### Open questions
- Reload all sections or only hooks initially? (Recommend: all)
- Keep last-good vs default fallback? (Recommend: last-good)
- Debounce interval? (Recommend: 500 ms)
- Should `set_default_model*` force-update cache immediately? (Nice bonus)
