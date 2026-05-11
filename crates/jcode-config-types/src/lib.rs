use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Compaction mode
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum CompactionMode {
    /// Compact when context hits a fixed threshold (default)
    #[default]
    Reactive,
    /// Compact early based on predicted token growth rate
    Proactive,
    /// Compact based on semantic topic shifts and relevance scoring
    Semantic,
}

impl CompactionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Reactive => "reactive",
            Self::Proactive => "proactive",
            Self::Semantic => "semantic",
        }
    }

    pub fn parse(input: &str) -> Option<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "reactive" => Some(Self::Reactive),
            "proactive" => Some(Self::Proactive),
            "semantic" => Some(Self::Semantic),
            _ => None,
        }
    }
}

/// Session picker Enter action: "new-terminal" (default) or "current-terminal".
/// Ctrl+Enter performs the alternate action.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SessionPickerResumeAction {
    #[default]
    NewTerminal,
    CurrentTerminal,
}

impl SessionPickerResumeAction {
    pub fn alternate(self) -> Self {
        match self {
            Self::NewTerminal => Self::CurrentTerminal,
            Self::CurrentTerminal => Self::NewTerminal,
        }
    }
}

/// How to display file diffs from edit/write tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffDisplayMode {
    /// Don't show diffs at all.
    Off,
    /// Show diffs inline in the chat (default).
    #[default]
    Inline,
    /// Show the full inline diff in the chat without preview truncation.
    #[serde(
        rename = "full-inline",
        alias = "full_inline",
        alias = "fullinline",
        alias = "inline-full",
        alias = "inline_full",
        alias = "inlinefull",
        alias = "full"
    )]
    FullInline,
    /// Show diffs in a dedicated pinned pane.
    Pinned,
    /// Show full file with diff highlights in side panel, synced to scroll position.
    File,
}

impl DiffDisplayMode {
    pub fn is_inline(&self) -> bool {
        matches!(self, Self::Inline | Self::FullInline)
    }

    pub fn is_full_inline(&self) -> bool {
        matches!(self, Self::FullInline)
    }

    pub fn is_pinned(&self) -> bool {
        matches!(self, Self::Pinned)
    }

    pub fn is_file(&self) -> bool {
        matches!(self, Self::File)
    }

    pub fn has_side_pane(&self) -> bool {
        matches!(self, Self::Pinned | Self::File)
    }

    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::Inline,
            Self::Inline => Self::FullInline,
            Self::FullInline => Self::Pinned,
            Self::Pinned => Self::File,
            Self::File => Self::Off,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::Inline => "Inline",
            Self::FullInline => "Inline Full",
            Self::Pinned => "Pinned",
            Self::File => "File",
        }
    }
}

/// How to display mermaid diagrams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagramDisplayMode {
    /// Don't show diagrams in dedicated widgets (only inline in messages).
    #[default]
    None,
    /// Show diagrams in info widget margins (opportunistic, if space available).
    Margin,
    /// Show diagrams in a dedicated pinned pane (forces space allocation).
    Pinned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagramPanePosition {
    #[default]
    Side,
    Top,
}

/// How much vertical spacing to use when rendering markdown blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkdownSpacingMode {
    /// Compact chat/TUI-oriented spacing.
    #[default]
    Compact,
    /// Document-style spacing between top-level blocks.
    Document,
}

impl MarkdownSpacingMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Document => "Document",
        }
    }
}

/// Update channel: how aggressively to receive updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    /// Only update from tagged GitHub Releases (default).
    #[default]
    Stable,
    /// Update from latest commit on main branch (bleeding edge).
    Main,
}

impl std::fmt::Display for UpdateChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stable => write!(f, "stable"),
            Self::Main => write!(f, "main"),
        }
    }
}

/// Cross-provider failover behavior when the same input would be resent elsewhere.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum CrossProviderFailoverMode {
    /// Show a 3-second cancelable countdown, then resend on another provider.
    #[default]
    Countdown,
    /// Do not resend the prompt to another provider automatically.
    Manual,
}

impl CrossProviderFailoverMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Countdown => "countdown",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "manual" => Some(Self::Manual),
            "countdown" | "auto" | "automatic" => Some(Self::Countdown),
            _ => None,
        }
    }
}

/// Compaction configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CompactionConfig {
    /// Compaction mode: reactive (default), proactive, or semantic
    pub mode: CompactionMode,

    /// [proactive] Number of turns to look ahead when projecting token growth
    pub lookahead_turns: usize,

    /// [proactive] EWMA alpha for token growth smoothing (0.0-1.0, higher = more recency bias)
    pub ewma_alpha: f32,

    /// [proactive/semantic] Minimum context fill level before any proactive check fires (0.0-1.0)
    pub proactive_floor: f32,

    /// [proactive/semantic] Minimum number of token snapshots needed before proactive check
    pub min_samples: usize,

    /// [proactive/semantic] Number of stable turns (no growth) before suppressing proactive compact
    pub stall_window: usize,

    /// [proactive/semantic] Minimum turns between two compactions (cooldown)
    pub min_turns_between_compactions: usize,

    /// [semantic] Cosine similarity threshold below which a topic shift is detected (0.0-1.0)
    pub topic_shift_threshold: f32,

    /// [semantic] Cosine similarity above which a message is kept verbatim (0.0-1.0)
    pub relevance_keep_threshold: f32,

    /// [semantic] Number of recent turns to look at for building the "current goal" embedding
    pub goal_window_turns: usize,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            mode: CompactionMode::Reactive,
            lookahead_turns: 15,
            ewma_alpha: 0.3,
            proactive_floor: 0.40,
            min_samples: 3,
            stall_window: 5,
            min_turns_between_compactions: 10,
            topic_shift_threshold: 0.45,
            relevance_keep_threshold: 0.65,
            goal_window_turns: 5,
        }
    }
}

/// Reload/reconnect recovery configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReloadConfig {
    /// Maximum time to wait for the new server to deliver session History
    /// after a reload/reconnect before falling back to locally cached messages.
    /// Default: 10 seconds.
    pub awaiting_history_timeout_secs: Option<u64>,
}

impl Default for ReloadConfig {
    fn default() -> Self {
        Self {
            awaiting_history_timeout_secs: Some(10),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum NamedProviderType {
    #[serde(alias = "openai-compatible", alias = "openai_compatible")]
    #[default]
    OpenAiCompatible,
    OpenRouter,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum NamedProviderAuth {
    #[default]
    Bearer,
    Header,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct NamedProviderModelConfig {
    pub id: String,
    #[serde(
        default,
        alias = "context_limit",
        alias = "context-length",
        alias = "context-window",
        alias = "context_length",
        skip_serializing_if = "Option::is_none"
    )]
    pub context_window: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct NamedProviderConfig {
    #[serde(rename = "type")]
    pub provider_type: NamedProviderType,
    pub base_url: String,
    pub api: Option<String>,
    pub auth: NamedProviderAuth,
    pub auth_header: Option<String>,
    pub api_key_env: Option<String>,
    pub api_key: Option<String>,
    pub env_file: Option<String>,
    pub default_model: Option<String>,
    pub requires_api_key: Option<bool>,
    #[serde(default)]
    pub provider_routing: bool,
    #[serde(default)]
    pub model_catalog: bool,
    #[serde(default)]
    pub allow_provider_pinning: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<NamedProviderModelConfig>,
}

impl Default for NamedProviderConfig {
    fn default() -> Self {
        Self {
            provider_type: NamedProviderType::OpenAiCompatible,
            base_url: String::new(),
            api: None,
            auth: NamedProviderAuth::Bearer,
            auth_header: None,
            api_key_env: None,
            api_key: None,
            env_file: None,
            default_model: None,
            requires_api_key: None,
            provider_routing: false,
            model_catalog: false,
            allow_provider_pinning: false,
            models: Vec::new(),
        }
    }
}

/// Remembered trust decisions for external auth sources managed by other tools.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AuthConfig {
    /// External auth source ids that the user has approved jcode to read/use.
    pub trusted_external_sources: Vec<String>,
    /// Path-bound approvals for external auth sources managed by other tools.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trusted_external_source_paths: Vec<String>,
}

/// Agent-specific model defaults.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AgentsConfig {
    /// Optional default model override for spawned swarm/subagent sessions.
    pub swarm_model: Option<String>,
    /// Deprecated simple model routing by subagent type/name, e.g. planner -> claude-opus-4-7.
    pub routing: BTreeMap<String, String>,
    /// Deprecated rich routing by subagent type/name. Prefer `profiles` for new configs.
    pub routes: BTreeMap<String, AgentRouteConfig>,
    /// Callable agent profiles by subagent type/name, including model, effort, variant, and usage guidance.
    pub profiles: BTreeMap<String, AgentRouteConfig>,
    /// Optional default model override for the memory sidecar.
    pub memory_model: Option<String>,
    /// Whether memory should use the sidecar for relevance/extraction.
    pub memory_sidecar_enabled: bool,
    /// Lazydino M2 stage 2 — controls whether `swarm spawn` opens a visible
    /// terminal window for each new worker. `None` keeps upstream behavior
    /// (try visible, fall back to headless). `Some(false)` forces
    /// headless-only spawn even when a terminal emulator is available, which
    /// avoids the "10+ terminals open" failure mode from upstream issue #76
    /// and gives a clean reproduction surface. Env var
    /// `JCODE_SWARM_NO_TERMINAL=1` overrides the config to force headless.
    pub swarm_spawn_visible: Option<bool>,
    /// Maximum consecutive lifecycle hook denies that may trigger immediate
    /// continuation turns. `None` uses the default (3). `Some(0)` means
    /// unlimited / trust hook scripts to self-throttle.
    pub max_lifecycle_deny_streak: Option<u8>,
}

/// Swarm coordination safety limits.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SwarmConfig {
    /// Maximum non-terminal workers one coordinator may own concurrently.
    /// `None` uses the built-in default; `Some(0)` disables the limit.
    pub max_active_spawns_per_coordinator: Option<u32>,
    /// Maximum non-terminal workers one orchestration run may own concurrently.
    /// `None` uses the built-in default; `Some(0)` disables the limit.
    pub max_active_spawns_per_run: Option<u32>,
    /// Seconds without member heartbeat before a running worker is surfaced as running_stale.
    /// `None` uses the built-in default of 180 seconds.
    pub heartbeat_stale_secs: Option<u32>,
    /// Optional hard timeout for assigned task execution. `None` means unlimited.
    pub default_task_timeout_minutes: Option<u32>,
}

/// Rich subagent routing configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AgentRouteConfig {
    /// Model to use for this subagent type.
    pub model: Option<String>,
    /// Reasoning effort for providers that support it, e.g. OpenAI `medium`, `high`, `xhigh`.
    pub effort: Option<String>,
    /// oh-my-opencode-compatible variant. For GPT/OpenAI this maps to reasoning effort; for
    /// supported Claude models, `max` selects the `[1m]` Claude Max / long-context route.
    pub variant: Option<String>,
    /// Human-readable role description for this callable agent profile.
    pub description: Option<String>,
    /// Guidance for when the coordinator should use this agent profile.
    pub when: Vec<String>,
    /// Optional extra instructions prepended to the subagent prompt when this profile is used.
    pub prompt: Option<String>,
}

/// Prompt and project instruction loading configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PromptConfig {
    /// Ignore `<project>/AGENTS.md` when building the system prompt.
    pub ignore_project_agents: bool,
    /// Ignore `~/.AGENTS.md` when building the system prompt.
    pub ignore_global_agents: bool,
    /// Load private project harness instructions from `<project>/.jcode/AGENTS.md`.
    pub load_jcode_agents: bool,
    /// Load private project harness modules from `<project>/.jcode/harness/*.md`.
    pub load_harness_dir: bool,
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self {
            ignore_project_agents: false,
            ignore_global_agents: false,
            load_jcode_agents: true,
            load_harness_dir: true,
        }
    }
}

/// Automatic end-of-turn code review configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AutoReviewConfig {
    /// Enable autoreview by default for new/resumed sessions (default: false)
    pub enabled: bool,
    /// Optional model override for autoreview reviewer sessions.
    pub model: Option<String>,
}

/// Automatic end-of-turn execution judging configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AutoJudgeConfig {
    /// Enable autojudge by default for new/resumed sessions (default: false)
    pub enabled: bool,
    /// Optional model override for autojudge sessions.
    pub model: Option<String>,
}

/// Command hook configuration.
///
/// Hook support covers tool and lifecycle boundaries:
/// `tool.execute.before`, `tool.execute.after`, `session.stop`, and `response.completed`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HooksConfig {
    /// Enable command hooks globally (default: false).
    pub enabled: bool,
    /// Command hooks to run for matching events/tools.
    pub commands: Vec<HookCommandConfig>,
}

impl Default for HooksConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            commands: Vec::new(),
        }
    }
}

/// A single command hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HookCommandConfig {
    /// Event name, e.g. `tool.execute.before`, `tool.execute.after`, `session.stop`, or `response.completed`.
    pub event: String,
    /// Optional tool name matcher. Empty means all tools.
    pub tool: Option<String>,
    /// Shell command to execute. Receives JSON payload on stdin.
    pub command: String,
    /// Whether jcode should wait for the hook. Deny decisions are only honored by tool.execute.before.
    pub blocking: bool,
    /// Maximum runtime in milliseconds.
    pub timeout_ms: u64,
}

impl Default for HookCommandConfig {
    fn default() -> Self {
        Self {
            event: String::new(),
            tool: None,
            command: String::new(),
            blocking: false,
            timeout_ms: 3000,
        }
    }
}

/// Keybinding configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindingsConfig {
    /// Scroll up key (default: "ctrl+k")
    pub scroll_up: String,
    /// Scroll down key (default: "ctrl+j")
    pub scroll_down: String,
    /// Page up key (default: "alt+u")
    pub scroll_page_up: String,
    /// Page down key (default: "alt+d")
    pub scroll_page_down: String,
    /// Model switch next key (default: "ctrl+tab")
    pub model_switch_next: String,
    /// Model switch previous key (default: "ctrl+shift+tab")
    pub model_switch_prev: String,
    /// Effort increase key (default: "alt+right")
    pub effort_increase: String,
    /// Effort decrease key (default: "alt+left")
    pub effort_decrease: String,
    /// Centered mode toggle key (default: "alt+c")
    pub centered_toggle: String,
    /// Scroll to previous prompt key (default: "ctrl+[")
    pub scroll_prompt_up: String,
    /// Scroll to next prompt key (default: "ctrl+]")
    pub scroll_prompt_down: String,
    /// Scroll bookmark toggle key (default: "ctrl+g")
    pub scroll_bookmark: String,
    /// Scroll up fallback key (default: "cmd+k")
    pub scroll_up_fallback: String,
    /// Scroll down fallback key (default: "cmd+j")
    pub scroll_down_fallback: String,
    /// Workspace navigation left key (default: "alt+h")
    pub workspace_left: String,
    /// Workspace navigation down key (default: "alt+j")
    pub workspace_down: String,
    /// Workspace navigation up key (default: "alt+k")
    pub workspace_up: String,
    /// Workspace navigation right key (default: "alt+l")
    pub workspace_right: String,
    /// Session picker Enter action: "new-terminal" (default) or "current-terminal".
    /// Ctrl+Enter performs the alternate action.
    pub session_picker_enter: SessionPickerResumeAction,
}

impl Default for KeybindingsConfig {
    fn default() -> Self {
        Self {
            scroll_up: "ctrl+k".to_string(),
            scroll_down: "ctrl+j".to_string(),
            scroll_page_up: "alt+u".to_string(),
            scroll_page_down: "alt+d".to_string(),
            model_switch_next: "ctrl+tab".to_string(),
            model_switch_prev: "ctrl+shift+tab".to_string(),
            effort_increase: "alt+right".to_string(),
            effort_decrease: "alt+left".to_string(),
            centered_toggle: "alt+c".to_string(),
            scroll_prompt_up: "ctrl+[".to_string(),
            scroll_prompt_down: "ctrl+]".to_string(),
            scroll_bookmark: "ctrl+g".to_string(),
            scroll_up_fallback: "cmd+k".to_string(),
            scroll_down_fallback: "cmd+j".to_string(),
            workspace_left: "alt+h".to_string(),
            workspace_down: "alt+j".to_string(),
            workspace_up: "alt+k".to_string(),
            workspace_right: "alt+l".to_string(),
            session_picker_enter: SessionPickerResumeAction::NewTerminal,
        }
    }
}

/// How to display file diffs from edit/write tools
/// Display/UI configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NativeScrollbarConfig {
    /// Show a native terminal scrollbar in the chat viewport (default: true)
    pub chat: bool,
    /// Show a native terminal scrollbar in the side panel (default: true)
    pub side_panel: bool,
}

impl Default for NativeScrollbarConfig {
    fn default() -> Self {
        Self {
            chat: true,
            side_panel: true,
        }
    }
}

/// Display/UI configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    /// How to display file diffs (off/inline/full-inline/pinned/file, default: inline)
    pub diff_mode: DiffDisplayMode,
    /// Legacy: "show_diffs = true/false" maps to diff_mode inline/off
    #[serde(default)]
    show_diffs: Option<bool>,
    /// Queue mode by default - wait until done before sending (default: false)
    pub queue_mode: bool,
    /// Automatically reload the remote server when a newer server binary is detected (default: true)
    pub auto_server_reload: bool,
    /// Capture mouse events (default: true). Enables scroll wheel but disables terminal selection.
    pub mouse_capture: bool,
    /// Enable debug socket for external control (default: false)
    pub debug_socket: bool,
    /// Center all content (default: false)
    pub centered: bool,
    /// Show thinking/reasoning content by default (default: false)
    pub show_thinking: bool,
    /// How to display mermaid diagrams (none/margin/pinned, default: none).
    /// Mermaid rendering is temporarily disabled for users unless JCODE_ENABLE_MERMAID=1.
    pub diagram_mode: DiagramDisplayMode,
    /// Markdown block spacing style (compact/document, default: compact)
    pub markdown_spacing: MarkdownSpacingMode,
    /// Pin read images to side pane (default: true)
    pub pin_images: bool,
    /// Show idle animation before first prompt (default: true)
    pub idle_animation: bool,
    /// Briefly animate user prompt line when it enters viewport (default: true)
    pub prompt_entry_animation: bool,
    /// Disable specific animation variants by name (e.g. ["donut", "orbit_rings"])
    pub disabled_animations: Vec<String>,
    /// Wrap long lines in the pinned diff pane (default: true)
    pub diff_line_wrap: bool,
    /// Performance tier override: auto/full/reduced/minimal (default: auto)
    pub performance: String,
    /// FPS for animations (startup, idle donut): 1-120 (default: 60)
    pub animation_fps: u32,
    /// FPS for active redraw (processing, streaming): 1-120 (default: 30)
    pub redraw_fps: u32,
    /// Show a truncated preview of the previous prompt at the top when it scrolls out of view (default: true)
    pub prompt_preview: bool,
    /// Native terminal scrollbar configuration for scrollable panes
    pub native_scrollbars: NativeScrollbarConfig,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            diff_mode: DiffDisplayMode::default(),
            show_diffs: None,
            pin_images: true,
            queue_mode: false,
            auto_server_reload: true,
            mouse_capture: true,
            debug_socket: false,
            centered: false,
            show_thinking: false,
            diagram_mode: DiagramDisplayMode::default(),
            markdown_spacing: MarkdownSpacingMode::default(),
            idle_animation: true,
            prompt_entry_animation: true,
            disabled_animations: Vec::new(),
            diff_line_wrap: true,
            performance: String::new(),
            animation_fps: 60,
            redraw_fps: 60,
            prompt_preview: true,
            native_scrollbars: NativeScrollbarConfig::default(),
        }
    }
}

impl DisplayConfig {
    pub fn apply_legacy_compat(&mut self) {
        if let Some(show) = self.show_diffs.take() {
            self.diff_mode = if show {
                DiffDisplayMode::Inline
            } else {
                DiffDisplayMode::Off
            };
        }
    }
}

/// Runtime feature toggles
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FeatureConfig {
    /// Enable memory retrieval/extraction features (default: true)
    pub memory: bool,
    /// Enable swarm coordination features (default: true)
    pub swarm: bool,
    /// Inject timestamps into user messages and tool results sent to the model (default: true)
    pub message_timestamps: bool,
    /// Update channel: "stable" (releases only) or "main" (latest commits)
    pub update_channel: UpdateChannel,
}

impl Default for FeatureConfig {
    fn default() -> Self {
        Self {
            memory: true,
            swarm: true,
            message_timestamps: true,
            update_channel: UpdateChannel::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    /// Default model to use (e.g. "claude-opus-4-7", "copilot:claude-opus-4.6")
    pub default_model: Option<String>,
    /// Default provider to use (claude|openai|copilot|openrouter)
    pub default_provider: Option<String>,
    /// Reasoning effort for OpenAI Responses API (none|low|medium|high|xhigh)
    pub openai_reasoning_effort: Option<String>,
    /// OpenAI transport mode (auto|websocket|https)
    pub openai_transport: Option<String>,
    /// OpenAI service tier override (priority|flex)
    pub openai_service_tier: Option<String>,
    /// OpenAI native compaction mode: "auto", "explicit", or "off".
    pub openai_native_compaction_mode: String,
    /// Token threshold at which OpenAI auto native compaction should trigger.
    pub openai_native_compaction_threshold_tokens: usize,
    /// How to handle cross-provider failover when the same input would be resent elsewhere.
    pub cross_provider_failover: CrossProviderFailoverMode,
    /// Whether jcode should automatically try another account on the same provider
    /// before falling back to a different provider.
    pub same_provider_account_failover: bool,
    /// Copilot premium request mode: "normal", "one", or "zero"
    /// "zero" means all requests are free (no premium requests consumed)
    pub copilot_premium: Option<String>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            default_model: None,
            default_provider: None,
            openai_reasoning_effort: Some("low".to_string()),
            openai_transport: None,
            openai_service_tier: Some("priority".to_string()),
            openai_native_compaction_mode: "auto".to_string(),
            openai_native_compaction_threshold_tokens: 200_000,
            cross_provider_failover: CrossProviderFailoverMode::Countdown,
            same_provider_account_failover: true,
            copilot_premium: None,
        }
    }
}

/// Ambient mode configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AmbientConfig {
    /// Enable ambient mode (default: false)
    pub enabled: bool,
    /// Provider override (default: auto-select)
    pub provider: Option<String>,
    /// Model override (default: provider's strongest)
    pub model: Option<String>,
    /// Allow API key usage (default: false, only OAuth)
    pub allow_api_keys: bool,
    /// Daily token budget when using API keys
    pub api_daily_budget: Option<u64>,
    /// Minimum interval between cycles in minutes (default: 5)
    pub min_interval_minutes: u32,
    /// Maximum interval between cycles in minutes (default: 120)
    pub max_interval_minutes: u32,
    /// Pause ambient when user has active session (default: true)
    pub pause_on_active_session: bool,
    /// Enable proactive work vs garden-only (default: true)
    pub proactive_work: bool,
    /// Proactive work branch prefix (default: "ambient/")
    pub work_branch_prefix: String,
    /// Show ambient cycle in a terminal window (default: true)
    pub visible: bool,
}

impl Default for AmbientConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: None,
            model: None,
            allow_api_keys: false,
            api_daily_budget: None,
            min_interval_minutes: 5,
            max_interval_minutes: 120,
            pause_on_active_session: true,
            proactive_work: true,
            work_branch_prefix: "ambient/".to_string(),
            visible: true,
        }
    }
}

/// Safety system & notification configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SafetyConfig {
    /// ntfy.sh topic name (required for push notifications)
    pub ntfy_topic: Option<String>,
    /// ntfy.sh server URL (default: https://ntfy.sh)
    pub ntfy_server: String,
    /// Enable desktop notifications via notify-send (default: true)
    pub desktop_notifications: bool,
    /// Enable email notifications (default: false)
    pub email_enabled: bool,
    /// Email recipient
    pub email_to: Option<String>,
    /// SMTP host (e.g. smtp.gmail.com)
    pub email_smtp_host: Option<String>,
    /// SMTP port (default: 587)
    pub email_smtp_port: u16,
    /// Email sender address
    pub email_from: Option<String>,
    /// SMTP password (prefer JCODE_SMTP_PASSWORD env var)
    pub email_password: Option<String>,
    /// IMAP host for receiving email replies (e.g. imap.gmail.com)
    pub email_imap_host: Option<String>,
    /// IMAP port (default: 993)
    pub email_imap_port: u16,
    /// Enable email reply → agent directive feature (default: false)
    pub email_reply_enabled: bool,
    /// Enable Telegram notifications (default: false)
    pub telegram_enabled: bool,
    /// Telegram bot token (from @BotFather)
    pub telegram_bot_token: Option<String>,
    /// Telegram chat ID to send messages to
    pub telegram_chat_id: Option<String>,
    /// Enable Telegram reply → agent directive feature (default: false)
    pub telegram_reply_enabled: bool,
    /// Enable Discord notifications (default: false)
    pub discord_enabled: bool,
    /// Discord bot token
    pub discord_bot_token: Option<String>,
    /// Discord channel ID to send messages to
    pub discord_channel_id: Option<String>,
    /// Discord bot user ID (for filtering own messages in polling)
    pub discord_bot_user_id: Option<String>,
    /// Enable Discord reply → agent directive feature (default: false)
    pub discord_reply_enabled: bool,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            ntfy_topic: None,
            ntfy_server: "https://ntfy.sh".to_string(),
            desktop_notifications: true,
            email_enabled: false,
            email_to: None,
            email_smtp_host: None,
            email_smtp_port: 587,
            email_from: None,
            email_password: None,
            email_imap_host: None,
            email_imap_port: 993,
            email_reply_enabled: false,
            telegram_enabled: false,
            telegram_bot_token: None,
            telegram_chat_id: None,
            telegram_reply_enabled: false,
            discord_enabled: false,
            discord_bot_token: None,
            discord_channel_id: None,
            discord_bot_user_id: None,
            discord_reply_enabled: false,
        }
    }
}

/// WebSocket gateway configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewayConfig {
    /// Enable the WebSocket gateway (default: false)
    pub enabled: bool,
    /// TCP port to listen on (default: 7643)
    pub port: u16,
    /// Bind address (default: 0.0.0.0)
    pub bind_addr: String,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 7643,
            bind_addr: "0.0.0.0".to_string(),
        }
    }
}

/// Per-tool configuration (M20).
///
/// Lets users override default/cap timeouts for the `bash`/shell tool so
/// long-running commands like `cargo build`, `cargo test --release`, or
/// stress checks aren't killed by the historical 2-minute hard timeout.
///
/// Both `default_timeout_ms` and `max_timeout_ms` are clamped at runtime by
/// the absolute upper bound `BashToolConfig::HARD_CAP_MS` (20 minutes) to
/// avoid pathological values like `u64::MAX`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolConfig {
    /// Bash / shell tool defaults.
    pub bash: BashToolConfig,
}

impl Default for ToolConfig {
    fn default() -> Self {
        Self {
            bash: BashToolConfig::default(),
        }
    }
}

/// Bash / shell tool defaults (M20).
///
/// Historical jcode behaviour was a hardcoded `DEFAULT_TIMEOUT_MS = 120_000`
/// (2 min) and a hardcoded cap of 600_000 (10 min). M20 lifts both to
/// 5 min default / 20 min cap and makes them configurable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BashToolConfig {
    /// Default timeout (ms) when the model does not pass `timeout` on a
    /// `bash`/shell tool call. Clamped to `[1_000, max_timeout_ms]`.
    pub default_timeout_ms: u64,
    /// Maximum timeout (ms) the model is allowed to request via the
    /// `timeout` argument. Clamped to `[default_timeout_ms, HARD_CAP_MS]`.
    pub max_timeout_ms: u64,
}

impl BashToolConfig {
    /// Absolute upper bound (20 minutes). Applied even if config asks for more
    /// so we never end up with a runaway tool call.
    pub const HARD_CAP_MS: u64 = 20 * 60 * 1000;

    /// Resolve `default_timeout_ms` clamped into `[1s, HARD_CAP_MS]`.
    pub fn effective_default_ms(&self) -> u64 {
        self.default_timeout_ms.clamp(1_000, Self::HARD_CAP_MS)
    }

    /// Resolve `max_timeout_ms` clamped into `[effective_default_ms, HARD_CAP_MS]`.
    pub fn effective_max_ms(&self) -> u64 {
        self.max_timeout_ms
            .clamp(self.effective_default_ms(), Self::HARD_CAP_MS)
    }
}

impl Default for BashToolConfig {
    fn default() -> Self {
        Self {
            // 5 minutes is enough for most cargo test/build runs while still
            // surfacing genuine hangs early.
            default_timeout_ms: 5 * 60 * 1000,
            // 20 minutes covers full release builds, long stress checks, etc.
            max_timeout_ms: Self::HARD_CAP_MS,
        }
    }
}
