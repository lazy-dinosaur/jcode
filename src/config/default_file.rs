use super::*;
use std::path::PathBuf;

impl Config {
    /// Create a default config file with comments
    pub fn create_default_config_file() -> anyhow::Result<PathBuf> {
        let path = Self::path().ok_or_else(|| anyhow::anyhow!("No config path"))?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let default_content = r#"# jcode configuration file
# Location: ~/.jcode/config.toml
#
# Environment variables override these settings.
# Run `/config` in jcode to see current settings.

[keybindings]
# Scroll keys (vim-style by default)
# Supports: ctrl, alt, shift modifiers + any key
# Examples: "ctrl+k", "alt+j", "ctrl+shift+up", "pageup"
scroll_up = "ctrl+k"
scroll_down = "ctrl+j"
scroll_page_up = "alt+u"
scroll_page_down = "alt+d"

# Model switching
model_switch_next = "ctrl+tab"
model_switch_prev = "ctrl+shift+tab"

# Reasoning effort switching (OpenAI models)
effort_increase = "alt+right"
effort_decrease = "alt+left"

# Centered mode toggle key
centered_toggle = "alt+c"

# Jump between user prompts
# Ctrl+1..4 resizes the pinned side panel to 25/50/75/100%.
# Ctrl+5..9 jumps by recency rank (5 = 5th most recent).
scroll_prompt_up = "ctrl+["
scroll_prompt_down = "ctrl+]"

# Scroll bookmark toggle (stash position, jump to bottom, press again to return)
scroll_bookmark = "ctrl+g"

# Optional fallback scroll bindings (useful on macOS terminals that forward Command)
scroll_up_fallback = "cmd+k"
scroll_down_fallback = "cmd+j"

# Workspace navigation (Niri-style)
# Comma-separate multiple bindings to add aliases.
workspace_left = "alt+h"
workspace_down = "alt+j"
workspace_up = "alt+k"
workspace_right = "alt+l"

# /resume picker behavior. Options: "new-terminal" or "current-terminal".
# Ctrl+Enter performs the alternate action.
session_picker_enter = "new-terminal"

[dictation]
# External speech-to-text command.
# The command should record/transcribe speech and print the final transcript to stdout.
# You can include any tool-specific flags here too, for example a grammar target.
# Examples:
# command = "~/.local/bin/my-whisper-script"
# command = "~/.local/bin/my-whisper-script --grammar-target code"
command = ""

# How to apply the transcript inside jcode: insert|append|replace|send
mode = "send"

# Optional in-app hotkey to trigger dictation. Set to "off" to disable.
# Example: "alt+;"
key = "off"

# Max seconds to wait for the dictation command to finish (0 = no timeout)
timeout_secs = 90

[display]
# Diff display mode: "off", "inline" (default), "full-inline", "pinned" (dedicated pane), or "file"
diff_mode = "inline"

# Center all content by default (default: false)
centered = false

# Pin read images to a side pane (default: true)
pin_images = true

# Wrap long lines in the pinned diff pane (default: true)
# Set to false for horizontal scrolling instead of wrapping
diff_line_wrap = true

# Queue mode: wait until assistant is done before sending next message
queue_mode = false

# Automatically reload the remote server when a newer server binary is detected (default: true)
auto_server_reload = true

# Capture mouse events (enables scroll wheel; disables terminal text selection)
mouse_capture = true

# Enable debug socket for external control/testing (default: false)
debug_socket = false

# Show thinking/reasoning content (default: false)
show_thinking = false

# Markdown spacing style: "compact" (chat/TUI) or "document" (docs-like)
# markdown_spacing = "compact"

# Show idle animation before first prompt (default: true)
idle_animation = true

# Briefly animate a user prompt line when it enters the viewport (default: true)
prompt_entry_animation = true

# Disable specific animation variants by name.
# Examples: ["donut"] or ["donut", "orbit_rings"]
# Legacy aliases such as "three_rings" and "gyroscope" are still accepted.
# disabled_animations = []

# Performance tier: auto/full/reduced/minimal (default: auto)
# auto = detect system load, memory, terminal type, SSH, and apply extra caps for WSL/Windows Terminal
# full = all animations enabled
# reduced = skip idle animations, keep spinners
# minimal = disable all animations, slower redraw rate
# performance = "auto"

# Animation FPS (idle animation): 1-120 (default: 60)
# Runtime policy may cap this lower on slower environments such as WSL/Windows Terminal.
# animation_fps = 60

# Active redraw FPS (processing, streaming, spinners): 1-120 (default: 60)
# Runtime policy may cap this lower on slower environments such as WSL/Windows Terminal.
# redraw_fps = 60

[features]
# Memory: retrieval + extraction sidecar features
memory = true
# Swarm: multi-session coordination features
swarm = true
# Inject timestamps into user messages and tool results sent to the model
message_timestamps = true
# Update channel: "stable" (releases only) or "main" (latest commits on push)
# Set to "main" for bleeding edge updates every time code is pushed
update_channel = "stable"

[provider]
# Default model (optional, uses provider default if not set)
# Set via /model picker with Ctrl+D to save as default
# default_model = "claude-opus-4-7"
# Default provider (optional: claude|openai|copilot|openrouter)
# When set, this provider is preferred on startup if available
# default_provider = "copilot"
# OpenAI reasoning effort (none|low|medium|high|xhigh)
openai_reasoning_effort = "low"
# OpenAI transport mode (auto|websocket|https)
# openai_transport = "auto"
# OpenAI service tier override (priority|flex)
# Defaults to OFF (no service tier override). Set to "priority" if you want
# Codex /fast behavior (higher speed, higher usage). Set to "flex" for slower
# but cheaper. lazydino fork: default off to avoid accidental fast usage.
# openai_service_tier = "priority"
# Cross-provider failover when the same prompt would be resent elsewhere.
# countdown = 3-second countdown before retrying on another provider; press Esc to cancel (default)
# manual = show a notice and let you switch yourself
# cross_provider_failover = "manual"
# Try another account on the same provider before switching providers (default: true)
# same_provider_account_failover = false
cross_provider_failover = "countdown"
# Copilot premium mode: "normal" (default), "one" (first msg only), "zero" (all free)
# Set to "zero" if you have premium Copilot and want free requests
# copilot_premium = "zero"
# Lazydino: enable OpenAI Responses API `parallel_tool_calls`. When true, an
# OpenAI model (gpt-5, codex, o1/o3 family) may emit multiple tool calls in a
# single turn, matching Anthropic behavior and unlocking parallel sub-agent
# fan-out. Default true. Env override: JCODE_OPENAI_PARALLEL_TOOL_CALLS=0
# (also false/no/off) restores the historical single-call behavior.
openai_parallel_tool_calls = true

[prompt]
# Project prompt/instruction loading.
# If your repo has a team AGENTS.md but you want to use only your private .jcode harness,
# set this in <project>/.jcode/config.toml:
# ignore_project_agents = true
ignore_project_agents = false
# Ignore ~/.AGENTS.md if desired.
ignore_global_agents = false
# Load private project harness instructions from <project>/.jcode/AGENTS.md.
load_jcode_agents = true
# Load private project harness modules from <project>/.jcode/harness/*.md in sorted order.
load_harness_dir = true

[agents]
# Default model override for spawned swarm/subagent sessions when no profile matches.
# swarm_model = "gpt-5.5"
# Optional memory sidecar model override.
# memory_model = "claude-sonnet-4-6"
# Enable memory sidecar extraction/relevance model.
memory_sidecar_enabled = false

# Lazydino M2 stage 2 — control whether `swarm spawn` opens a visible terminal
# window for each new worker. Leaving this unset (or `true`) keeps upstream
# behavior: try a visible terminal first and fall back to headless if no
# emulator is available. Setting `false` forces every swarm worker to run
# headless even when a terminal emulator is installed. This avoids the
# upstream issue #76 failure mode where the coordinator opened 10+ visible
# windows that the user could not easily control.
#
# The `JCODE_SWARM_NO_TERMINAL=1` env var (also `true`/`yes`/`on`) overrides
# this setting at runtime without rebuilding. `JCODE_SWARM_NO_TERMINAL=0`
# (also `false`/`no`/`off`) forces visible-first even if config says
# otherwise. The spawn tool result string includes the active mode so the
# coordinator agent always knows which mode it spawned under.
#
# swarm_spawn_visible = false

# M11 lifecycle hooks: when a blocking response.completed hook prints
# {"action":"deny","reason":"..."}, jcode immediately continues the turn
# with that reason as a transient System Reminder. A cap can prevent a bad
# hook from creating an infinite self-correction loop.
#
# Default (M35 Round 22): 0 = no cap (claude-code compatible trust mode).
# Hook scripts must self-throttle using the response.completed payload field
# `stop_hook_active = true` on continuation turns, e.g.:
#   STOP_HOOK_ACTIVE=$(jq -r '.stop_hook_active' <&0)
#   [ "$STOP_HOOK_ACTIVE" = "true" ] && exit 0
# Set to N (e.g. 3) for a hardcap of N immediate continuation turns before
# stopping. Environment override wins over config:
#   JCODE_MAX_LIFECYCLE_DENY_STREAK=3 jcode
# max_lifecycle_deny_streak = 0

# Lazydino: allow recursive subagent calls (subagent inside subagent).
# Default `false` matches upstream — child subagents have `subagent`, `task`,
# and `todo*` tools removed from their allowed set to prevent unbounded
# recursion. Set `true` to let a child subagent spawn further subagents.
# Useful when you intentionally want deep delegation chains (e.g. a planner
# subagent that itself delegates to coder/reviewer subagents).
# Env override:
#   JCODE_ALLOW_SUBAGENT_RECURSION=1 jcode
# (also `true`/`yes`/`on`)
# allow_subagent_recursion = false

[swarm]
# Lazydino M2 stage 3 — opt-in hard caps for concurrently active workers
# owned by one coordinator. Active means any non-terminal worker status;
# completed, failed, crashed, closed, and disconnected workers do not count.
# `0` (the default) disables the cap entirely, matching upstream jcode
# behavior. Set a positive value to defend against runaway spawn patterns
# (see issue #76). Env vars override these values:
#   JCODE_MAX_ACTIVE_SPAWNS_PER_COORDINATOR
#   JCODE_MAX_ACTIVE_SPAWNS_PER_RUN
# Defaults when unset are 0 (unlimited) for both.
# max_active_spawns_per_coordinator = 0
# max_active_spawns_per_run = 0
#
# Lazydino M2 stage 4 — worker heartbeat stale surfacing. A running worker
# that emits no text/tool/status heartbeat for this many seconds is marked
# running_stale on status/await reads. This is reversible; a later heartbeat
# restores running. Env override: JCODE_WORKER_HEARTBEAT_STALE_SECS.
# Default when unset: 180 seconds.
# heartbeat_stale_secs = 180
#
# Optional hard timeout for assigned task execution. Default is unlimited for
# safety; set this only when you explicitly want long-silent workers failed.
# Per-request task_timeout_minutes on assign_task/start_task overrides this.
# Env override: JCODE_DEFAULT_TASK_TIMEOUT_MINUTES.
# default_task_timeout_minutes = 30
#
# Spawned worker cwd is pinned under the coordinator's cwd after canonicalizing
# symlinks. To intentionally bypass for a one-off run, set env var only:
#   JCODE_SWARM_ALLOW_ANY_CWD=1

# Practical callable agent profiles. Each [agents.profiles.<type>] name becomes a valid
# subagent_type exposed to the subagent tool. Profiles can carry model, variant/effort,
# description, when-to-use guidance, and optional prompt instructions.
#
# Explicit `model` in the subagent tool still wins over profiles.
# Reused session model also wins over profiles.
# Deprecated [agents.routes.<type>] and [agents.routing] are still accepted for compatibility.
#
# GPT/OpenAI `variant = "max"` maps to jcode effort `xhigh`.
# Supported Claude `variant = "max"` maps to the Claude Max / long-context `[1m]` route.
# Gemini routes currently ignore variant.
#
# [agents.profiles.planner]
# model = "claude-opus-4-7"
# variant = "max"
# description = "Planning and architecture agent for ambiguous or multi-step work."
# when = ["the request needs decomposition", "architecture or sequencing decisions matter"]
# prompt = "Create a concise plan, identify risks, and hand off implementation-ready steps."
#
# [agents.profiles.coder]
# model = "gpt-5.5"
# variant = "medium"
# description = "Implementation agent for concrete code changes."
# when = ["the plan is clear", "files need editing"]
#
# [agents.profiles.searcher]
# model = "gpt-5.5"
# variant = "medium"
# description = "Codebase research agent for finding files, symbols, and implementation patterns."
#
# [agents.profiles.reviewer]
# model = "claude-opus-4-7"
# variant = "max"
# description = "Review and risk analysis agent."
#
# [agents.profiles.quick]
# model = "claude-haiku-4-5-20251001"
# description = "Fast lightweight helper for tiny checks."

[ambient]
# Ambient mode: background agent that maintains your codebase
# Enable ambient mode (default: false)
enabled = false
# Provider override (default: auto-select based on available credentials)
# provider = "claude"
# Model override (default: provider's strongest)
# model = "claude-sonnet-4-20250514"
# Allow API key usage (default: false, only OAuth to avoid surprise costs)
allow_api_keys = false
# Daily token budget when using API keys (optional)
# api_daily_budget = 100000
# Minimum interval between cycles in minutes
min_interval_minutes = 5
# Maximum interval between cycles in minutes
max_interval_minutes = 120
# Pause ambient when user has active session
pause_on_active_session = true
# Enable proactive work (new features, refactoring) vs garden-only (lint, format, deps)
proactive_work = true
# Branch prefix for proactive work
work_branch_prefix = "ambient/"
# Show ambient cycle in a terminal window (default: true)
# visible = true

[reload]
# Max seconds to wait for server History after reload before using local cached messages.
awaiting_history_timeout_secs = 10

[gateway]
# Enable WebSocket gateway for iOS/web clients
enabled = false
# TCP port for gateway listener
port = 7643
# Bind address (0.0.0.0 for LAN/Tailscale reachability)
bind_addr = "0.0.0.0"

[hooks]
# Command hooks for tool/session/response lifecycle events (default: disabled).
# Global hooks live here. Project hooks may be added in:
#   <project>/.jcode/config.toml
#   <project>/.jcode/config.local.toml
# Project/local hook commands are appended to global hook commands.
# Hooks receive a JSON payload on stdin.
# Blocking tool.execute.before hooks may return {"action":"allow"} or {"action":"deny","reason":"..."}.
# Blocking lifecycle hooks (response.completed, session.stop, client.disconnect) may also return
# {"action":"deny","reason":"..."}; the reason is injected as a system reminder into the next user
# turn. After 3 consecutive denies a loop guard surfaces a single notice telling the model to stop
# and clears the streak (see M11 stage 3).
enabled = false

# Example: block or allow tool calls before execution.
# [[hooks.commands]]
# event = "tool.execute.before"
# tool = "bash" # optional, use "*" or omit for all tools
# command = ".jcode/hooks/check-bash.sh"
# blocking = true
# timeout_ms = 3000

# Example: log tool results after execution.
# [[hooks.commands]]
# event = "tool.execute.after"
# command = ".jcode/hooks/log-tool.sh"
# blocking = false
# timeout_ms = 3000

# Example: react to client teardown (M11 stage 4). Fires when a client connection
# closes or crashes. Preferred over `session.stop` for client-disconnect semantics.
# [[hooks.commands]]
# event = "client.disconnect"
# command = ".jcode/hooks/client-disconnect.sh"
# blocking = false
# timeout_ms = 3000

# Example: notify when a session truly stops. Currently `session.stop` is also
# emitted on client disconnect for backward compatibility; new hooks should
# prefer `client.disconnect` above. `session.stop` will be reserved for a
# future explicit logical session-end signal.
# [[hooks.commands]]
# event = "session.stop"
# command = ".jcode/hooks/session-stop.sh"
# blocking = false
# timeout_ms = 3000

# Example: log final assistant response completion once per turn.
# [[hooks.commands]]
# event = "response.completed"
# command = ".jcode/hooks/response-completed.sh"
# blocking = false
# timeout_ms = 3000

# M11 stage 5: lifecycle hook payloads (response.completed, session.stop,
# client.disconnect) carry these optional context fields. They are omitted
# from the JSON when empty so existing scripts keep working unchanged:
#
#   last_user_message     string  Most recent user-authored input
#                                 (system reminders skipped, max 500 chars)
#   recent_tool_calls     array   Up to 5 most-recent tool uses, each:
#                                   { "name": "bash",
#                                     "args_preview": "{\"command\":\"git status\"}" }
#                                 args_preview is one-line and ≤200 chars.
#   turn_count            number  Distinct user-authored turns so far
#   session_age_seconds   number  Seconds since session creation
#
# Example policy: block "commit and push" requests where the model didn't
# actually run git. Save as .jcode/hooks/require-git-on-commit.sh:
#
#   #!/usr/bin/env bash
#   read -r payload
#   msg=$(printf '%s' "$payload" | jq -r '.last_user_message // ""')
#   tools=$(printf '%s' "$payload" | jq -r '.recent_tool_calls[].name // ""')
#   if printf '%s' "$msg" | grep -qiE "commit|push|커밋"; then
#     if ! printf '%s' "$tools" | grep -qE '^bash$'; then
#       printf '{"action":"deny","reason":"commit was requested but no git tool ran"}'
#     fi
#   fi
#
# Wire it up with:
# [[hooks.commands]]
# event = "response.completed"
# command = ".jcode/hooks/require-git-on-commit.sh"
# blocking = true
# timeout_ms = 3000

[safety]
# Notification settings for ambient mode events

# ntfy.sh push notifications (free, phone app: https://ntfy.sh)
# ntfy_topic = "jcode-ambient-your-secret-topic"
# ntfy_server = "https://ntfy.sh"

# Desktop notifications via notify-send (default: true)
desktop_notifications = true

# Email notifications via SMTP
# email_enabled = false
# email_to = "you@example.com"
# email_from = "jcode@example.com"
# email_smtp_host = "smtp.gmail.com"
# email_smtp_port = 587
# Password via env: JCODE_SMTP_PASSWORD (preferred) or config below
# email_password = ""

# IMAP for email replies (reply to ambient emails to send directives)
# email_reply_enabled = false
# email_imap_host = "imap.gmail.com"
# email_imap_port = 993

# Telegram notifications via Bot API (free, https://telegram.org)
# telegram_enabled = false
# telegram_bot_token = ""  # From @BotFather (prefer JCODE_TELEGRAM_BOT_TOKEN env var)
# telegram_chat_id = ""    # Your user/chat ID
# telegram_reply_enabled = false  # Reply to bot messages to send directives

# Discord notifications via Bot API (https://discord.com/developers)
# discord_enabled = false
# discord_bot_token = ""     # From Discord Developer Portal (prefer JCODE_DISCORD_BOT_TOKEN env var)
# discord_channel_id = ""    # Channel ID to post in
# discord_bot_user_id = ""   # Bot's user ID (for filtering own messages)
# discord_reply_enabled = false  # Messages in channel become agent directives
"#;

        std::fs::write(&path, default_content)?;
        Ok(path)
    }
}
