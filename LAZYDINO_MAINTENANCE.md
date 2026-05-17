# Lazydino jcode Custom Harness Maintenance

This repository is a local checkout of upstream `1jehuang/jcode` with a personal custom branch for Lazydino-specific patches.

## Repository locations

Local checkout:

```text
/home/lazydino/dev/jcode
```

Remotes:

```text
origin = https://github.com/1jehuang/jcode.git
fork   = https://github.com/lazy-dinosaur/jcode.git
```

Branch roles:

```text
origin/master                  upstream jcode source of truth
fork/master                    Lazydino fork mirror of upstream master
custom/lazydino-harness        personal custom jcode branch
```

## Current custom branch

Use this branch for all personal jcode modifications:

```bash
git switch custom/lazydino-harness
```

Do not put personal custom commits directly on `master`. Keep `master` clean so it can mirror upstream.

## Custom patch philosophy

The maintenance model is:

```text
latest upstream jcode
        +
Lazydino custom patch stack
        =
custom/lazydino-harness
```

When upstream updates, rebase the custom branch on top of the latest upstream code. This reapplies the personal patches onto the newest jcode source.

Custom patch ledger rules:

- Any feature that does not exist in upstream jcode must be explicitly recorded in this file before or immediately after commit.
- Each custom patch should state its purpose, config surface, touched behavior, validation command, and whether the installed binary must be rebuilt/replaced.
- After upstream updates, replay/rebase the custom patch stack and verify each patch with its documented validation command.
- Do not silently add local-only behavior. If it changes harness behavior, document it here.

## Update workflow

### 1. Fetch upstream

```bash
cd /home/lazydino/dev/jcode
git fetch origin
```

### 2. Update fork master to match upstream

Warning: this intentionally resets local `master` to upstream.

```bash
git switch master
git reset --hard origin/master
git push fork master --force-with-lease
```

### 3. Rebase custom branch on latest upstream

```bash
git switch custom/lazydino-harness
git rebase origin/master
```

If conflicts occur, resolve them while preserving Lazydino-specific behavior, then continue:

```bash
git status
# edit conflicted files
git add <resolved-files>
git rebase --continue
```

### 4. Validate

Run at least:

```bash
cargo check
```

Prefer targeted tests for touched areas, for example:

```bash
cargo test -p jcode-config-types
cargo test
```

### 5. Push custom branch to fork

Because rebasing rewrites commit history, use `--force-with-lease`:

```bash
git push fork custom/lazydino-harness --force-with-lease
```


## Safe custom patch replay script

A conservative replay helper exists for upstream updates:

```text
/home/lazydino/dev/jcode/scripts/lazydino/reapply-custom-stack.sh
```

Purpose:

- Replay Lazydino's documented `patch/*` stack onto a fresh upstream base.
- Stop safely on dirty working trees, missing patch refs, cherry-pick conflicts, or validation failures.
- Never force-push automatically.
- Never update `custom/lazydino-harness` unless `--update-target` is explicitly passed.

Default dry-run:

```bash
cd /home/lazydino/dev/jcode
scripts/lazydino/reapply-custom-stack.sh
```

Actual replay onto latest upstream:

```bash
cd /home/lazydino/dev/jcode
scripts/lazydino/reapply-custom-stack.sh --apply --validate
```

Replay onto a specific base:

```bash
scripts/lazydino/reapply-custom-stack.sh --apply --base origin/master --validate
```

If and only if the resulting work branch is correct, update the local custom branch:

```bash
scripts/lazydino/reapply-custom-stack.sh --apply --validate --update-target
```

Push remains manual:

```bash
git push fork custom/lazydino-harness --force-with-lease
```

Design principle:

```text
small upstream drift  -> script can cherry-pick the stack
large upstream drift  -> script stops at first conflict
conflict/test failure -> use LAZYDINO_MAINTENANCE.md + patch/* branch as source of truth and ask an AI agent to re-implement against the new code structure
```

Ordered patch refs are embedded in the script. When a new custom patch is added, also add its `patch/<name>` ref to the script's `PATCH_REFS` list and update this document.

## Custom binary install helper

Use this after changing any runtime behavior or client/server protocol:

```bash
cd /home/lazydino/dev/jcode
scripts/lazydino/install-custom-jcode.sh
```

What it updates:

- `~/.local/bin/jcode`
- `~/.jcode/builds/stable/jcode`
- `~/.jcode/builds/current/jcode`
- a versioned custom slot such as `~/.jcode/builds/versions/lazydino-<sha>/jcode`

Why this exists:

- The foreground client can use `~/.local/bin/jcode` while the shared daemon/server can still be running from `~/.jcode/builds/stable/jcode`.
- If only `~/.local/bin/jcode` is replaced, the old daemon can keep applying old defaults such as `gpt-5.5` and miss new project skill/hook behavior.
- Updating the Jcode-managed stable/current symlinks makes new daemon starts use the custom build.

To also terminate the old Jcode-managed daemon so the next client starts the new build:

```bash
scripts/lazydino/install-custom-jcode.sh --restart-server
```

The restart option only targets daemon-style `jcode ... serve` processes under `~/.jcode/builds/*`; it does not intentionally kill foreground TUI client processes.

## AI agent maintenance prompt

Use this prompt when asking an AI agent to maintain the branch:

```text
Update /home/lazydino/dev/jcode. Fetch origin/master, update fork/master to mirror upstream, then rebase custom/lazydino-harness onto origin/master. Resolve conflicts while preserving Lazydino custom patches, especially mermaid label rendering fixes and jcode hook support. Run cargo check and relevant tests. Do not discard custom behavior. Push custom/lazydino-harness to fork with --force-with-lease only after validation succeeds.
```

## Existing custom patches

Track each custom patch as a small commit. Current known customizations:

1. Mermaid label rendering fix
   - Commit: `fix: restore mermaid label rendering on v0.12`
   - Purpose: restore visible Mermaid node/edge labels in the TUI renderer.

2. Hook support
   - Goal: add Claude Code-style `PreToolUse` / `PostToolUse` MVP with opencode-style event names:
     - `tool.execute.before`
     - `tool.execute.after`
   - Commit: `feat: add command hooks for tool lifecycle`
   - Current implementation uses command hooks configured from jcode config.
   - Config surface: `[hooks]` and `[[hooks.commands]]`.
   - Validation: `cargo test hook --lib` and `cargo check`.
   - Binary reinstall required: yes, because this changes runtime behavior.

3. Project-local hook config
   - Goal: allow project-specific hook configuration in addition to global `~/.jcode/config.toml`.
   - Config files:
     - `<project>/.jcode/config.toml`
     - `<project>/.jcode/config.local.toml`
   - Merge behavior:
     - global hooks first
     - project shared hooks second
     - project local/private hooks third
   - Current scope: hook config only. Full config merging is intentionally deferred.
   - Validation: `cargo test project_local --lib`.
   - Binary reinstall required: yes, because this changes runtime behavior.

4. Jcode-style agent profiles
   - Goal: route subagents to practical named profiles by `subagent_type`, using jcode's existing subagent/task execution model without copying all oh-my-opencode mythology into the default workflow.
   - Official config surface:
     - `[agents.profiles.<name>]` defines a callable agent profile. The `<name>` becomes a valid `subagent_type` exposed in the `subagent` tool schema.
     - Each profile supports `model`, `effort` / oh-my-opencode `variant`, `description`, `when`, and short profile `prompt` instructions.
     - Profile metadata is prepended to the subagent prompt as an `<agent_profile>` block so the child agent knows its role and when-to-use intent.
   - Deprecated compatibility surface:
     - `[agents.routes.<name>]` and `[agents.routing]` are still accepted for older local configs, but new configs should use `[agents.profiles.<name>]` only.
   - Main orchestrator policy: global `[provider]` should default to `claude` + `claude-opus-4-7[1m]` when the user wants Opus as the planning/orchestration brain.
   - Practical default profile set: `planner`, `coder`, `executor`, `searcher`, `reviewer`, `quick`, `visual`, `writer`, plus `sisyphus` / `sysiphus` for hard debugging.
   - Opus planner/reviewer/sisyphus profiles should use `claude-opus-4-7`; Sonnet support profiles should use `claude-sonnet-4-6`.
   - oh-my-opencode `variant` mapping:
     - `medium` -> OpenAI `medium`
     - `high` -> OpenAI `high`
     - `xhigh` -> OpenAI `xhigh`
     - `max` on GPT/OpenAI -> OpenAI `xhigh`
     - `max` on supported Claude routes -> append `[1m]` for Claude Max / long-context route, e.g. `claude-opus-4-7[1m]`
   - Sonnet 4.6 stays `claude-sonnet-4-6`; Opus planner/reviewer routes should use `claude-opus-4-7`, with `variant = "max"` when the Max route is intended.
   - Resolution order:
     1. explicit `model` argument in the `subagent` tool
     2. existing reused session model
     3. `[agents.profiles].<subagent_type>` / deprecated `[agents.routes].<subagent_type>` / deprecated `[agents.routing].<subagent_type>`
     4. parent session preferred subagent model
     5. `agents.swarm_model`
     6. current provider model
   - Recommended high-level policy:
     - Opus = main orchestrator plus planner/reviewer/sisyphus brain
     - GPT = coder/executor/searcher/tool-loop runner
     - Gemini = visual/multimodal specialist
     - Haiku quick/explore routes should use the live catalog ID `claude-haiku-4-5-20251001` when available, rather than the shorter alias, to match the model picker exactly.
   - Active local aliases: both `sisyphus` and `sysiphus` route to Opus Max so the agent can be called even if the name is typed the user's common way.
   - Validation: `cargo test configured_subagent_types --lib`, `cargo test prompt_with_profile --lib`, `cargo test test_agents_routing_deserializes_from_config --lib`, and `cargo check`.
   - Binary reinstall required: yes, because this changes runtime behavior.

5. Private `.jcode/` harness prompt loading
   - Goal: allow a local, gitignored `.jcode/` directory to act as the user's primary harness without modifying team `AGENTS.md`.
   - Config surface: `[prompt]` in `~/.jcode/config.toml` or `<project>/.jcode/config.toml`.
   - Supported prompt files:
     - `<project>/.jcode/AGENTS.md`
     - `<project>/.jcode/harness/*.md` loaded in sorted filename order
     - `<project>/.jcode/prompt-overlay.md`
   - Important options:
     - `ignore_project_agents = true` skips team `<project>/AGENTS.md`.
     - `load_jcode_agents = true` loads private `.jcode/AGENTS.md`.
     - `load_harness_dir = true` loads private `.jcode/harness/*.md`.
   - Priority: private `.jcode` instructions load after team/global AGENTS instructions so they have higher prompt priority. If the team harness should be fully ignored, set `ignore_project_agents = true`.
   - Recommended `.gitignore` for projects where `.jcode` is personal only: `.jcode/`.
   - Validation: `cargo test private_jcode --lib`, `cargo test prompt_config --lib`, and `cargo check`.
   - Binary reinstall required: yes, because this changes runtime behavior.

6. Native `jcode init` project onboarding
   - Goal: make project-local `.jcode/` onboarding an actual Jcode command, not a passive skill instruction bundle.
   - Command surface:
     - `jcode init [target]`
     - `jcode init --ignore-team-agents [target]`
     - `jcode init --gitignore [target]`
     - `jcode init --force [target]`
   - Generated files:
     - `.jcode/config.toml`
     - `.jcode/AGENTS.md`
     - `.jcode/harness/10-routing-policy.md`
     - `.jcode/harness/20-project-rules.md`
     - `.jcode/hooks/check-bash.sh`
     - `.jcode/hooks/log-tool.sh`
   - Default privacy behavior: add `.jcode/` to `.git/info/exclude` so private harness files are not committed. Use `--gitignore` only when the user explicitly wants to modify the shared project `.gitignore`.
   - Default prompt behavior: keep team `AGENTS.md` active. Use `--ignore-team-agents` when the private harness should become primary.
   - Validation: `cargo test project_init --lib`, `cargo check`, and temp-project CLI smoke via `cargo run --bin jcode -- init <tmp-project>`.
   - Binary reinstall required: yes, because this adds a user-facing CLI command.

7. Ambient numeric argument serde compatibility
   - Upstream source: adapted from PR `#173` (`Fix ambient serde bug: handle string or u32 for Claude tool parameters`).
   - Commit: `fix: accept stringified ambient numeric args`.
   - Patch branch: `patch/ambient-serde-args`.
   - Goal: make ambient tools accept Claude-style stringified numeric tool arguments such as `"0"`, `"15"`, and normal JSON numbers.
   - Touched fields:
     - `EndCycleInput.memories_modified`
     - `EndCycleInput.compactions`
     - `NextScheduleInput.wake_in_minutes`
     - `ScheduleInput.wake_in_minutes`
     - `ScheduleToolInput.wake_in_minutes`
   - Why: ambient cycles can fail when a provider emits numeric tool parameters as strings. This blocks scheduled/idle ambient work and memory consolidation.
   - Validation: `cargo test tool::ambient --lib`.
   - Binary reinstall required: yes, because this changes ambient tool runtime behavior.

8. Project skill sync and slash activation for remote sessions
   - Upstream inspiration: adapted from the purposes behind PR `#166` and `#162`, with `#151` treated as a larger design reference.
   - Commit: `feat: sync project skills in remote sessions`.
   - Patch branch: `patch/project-skill-sync`.
   - Goal: make project-local skills behave like real active project capabilities, not only passive UI suggestions.
   - Skill directories loaded from the active working directory:
     - `.jcode/skills/<skill>/SKILL.md`
     - `.claude/skills/<skill>/SKILL.md`
     - `.agents/skills/<skill>/SKILL.md`
     - `.opencode/skills/<skill>/SKILL.md`
   - Runtime behavior:
     - Remote clients send `activate_skill` to the server when the user enters `/skill-name`.
     - The server reloads project-local skill directories after session subscribe/working-dir changes.
     - The server sets the agent's active skill, then emits `skill_activated` so the UI and model prompt state stay in sync.
     - The `skill_manage` tool accepts both `name` and public-style `skill` parameters.
   - Why: the UI could show project skills as active/available while the daemon-side agent did not reliably load or activate the same project-local skill registry.
   - Validation: `cargo test skill --lib` and `cargo check`.
   - Binary reinstall required: yes, because this changes client/server protocol and runtime behavior.

9. Custom install helper for client and daemon binary paths
   - Commit: `chore: add custom jcode install helper`.
   - Patch branch: `patch/custom-install-server-paths`.
   - Goal: prevent split-brain installs where `~/.local/bin/jcode` is custom/new but the shared daemon still runs `~/.jcode/builds/stable/jcode` from an old upstream build.
   - Script: `scripts/lazydino/install-custom-jcode.sh`.
   - Install targets:
     - `~/.local/bin/jcode`
     - `~/.jcode/builds/stable/jcode`
     - `~/.jcode/builds/current/jcode`
     - `~/.jcode/builds/versions/lazydino-<sha>/jcode`
   - Optional runtime action: `--restart-server` terminates only daemon-style `jcode ... serve` processes launched from `~/.jcode/builds/*` so the next client starts the new build.
   - Validation: `bash -n scripts/lazydino/install-custom-jcode.sh`, `scripts/lazydino/install-custom-jcode.sh --help`, and a release install smoke.
   - Binary reinstall required: no for the script itself, but this script exists to do binary reinstalls correctly.

10. OpenAI usage percent normalization
   - Upstream source: adapted from PR `#178` (`Fix OpenAI usage percent normalization for low values`).
   - Commit: `fix: normalize OpenAI usage percentages`.
   - Patch branch: `patch/openai-usage-percent-normalization`.
   - Goal: fix `jcode usage`, `/usage`, and compact quota widgets incorrectly showing `used_percent: 1` as 100% exhausted.
   - Root cause: OpenAI `wham/usage` returns `used_percent` in `[0, 100]`, but the old helper treated values `<= 1.0` as ratios, so `1` became `1.0` instead of `0.01`.
   - Runtime behavior: `normalize_ratio(raw)` now always treats OpenAI usage values as percentages and returns `(raw / 100.0).clamp(0.0, 1.0)`.
   - Touched paths:
     - `src/usage/openai_helpers.rs`
     - `src/usage_openai.rs`
     - `src/usage/tests.rs`
   - Validation: `cargo test usage::tests --lib` and `cargo check`.
   - Binary reinstall required: yes, because this changes usage/quota runtime behavior.

11. Immediate session message journaling
   - Commit: `feat: journal new session messages immediately`.
   - Patch branch: `patch/journal-on-message`.
   - Purpose: append each newly stored session message to `<session>.journal.jsonl` immediately after it enters `Session.messages`, so daemon crashes, SIGTERM/SIGKILL, or install-helper restarts cannot lose messages that were already accepted in memory.
   - Runtime behavior:
     - New messages are journaled only after a snapshot baseline exists.
     - Immediate journal success advances `persist_state.messages_len`, so the next `Session::save()` does not duplicate the message delta.
     - Forced full snapshots or metadata changes that require checkpointing skip immediate journaling and let the next save write the snapshot.
     - Immediate journal failures are logged as best-effort warnings and do not break the in-memory session.
   - Touched paths:
     - `src/session.rs`
     - `src/session/journal.rs`
     - `src/session/persistence.rs`
     - `src/session_tests/cases.rs`
   - Validation: `cargo check`, `cargo test session::tests::cases --lib`, `cargo test immediate_journal --lib`, and `cargo test --lib --no-run`.
   - Binary reinstall required: yes, because this changes session persistence runtime behavior.

12. Safe server restart session drain
   - Commit: `feat: drain and flush sessions on daemon shutdown`.
   - Patch branch: `patch/safe-server-restart`.
   - Purpose: make daemon shutdown preserve in-memory session state and avoid active-session ghosts when the shared server is restarted by SIGTERM or the custom install helper.
   - Runtime behavior:
     - SIGTERM/SIGINT now runs a bounded best-effort drain before unregistering the server and exiting.
     - Each live agent saves its session and marks it `Closed`, or `Crashed` with `server shutdown drain` when the last visible conversation message is a pending user turn.
     - The drain skips when a reload marker is already active so it does not conflict with `reload::graceful_shutdown_sessions`.
     - Debug socket command `shutdown:drain` initiates the same clean drain/unregister/exit path and returns a JSON acknowledgement before exiting.
     - `scripts/lazydino/install-custom-jcode.sh --restart-server` tries `jcode debug shutdown drain` before falling back to SIGTERM/SIGKILL.
   - Touched paths:
     - `src/agent.rs`
     - `src/server.rs`
     - `src/server/reload.rs`
     - `src/server/debug.rs`
     - `src/server/debug_command_exec.rs`
     - `src/server/debug_help.rs`
     - `src/server/drain_tests.rs`
     - `scripts/lazydino/install-custom-jcode.sh`
   - Validation: `cargo check`, `cargo test drain --lib --no-fail-fast`, `cargo test reload --lib --no-fail-fast`, `cargo test session --lib --no-fail-fast`, `bash -n scripts/lazydino/install-custom-jcode.sh`, and `scripts/lazydino/install-custom-jcode.sh --help`.
   - Binary reinstall required: yes, because this changes daemon shutdown behavior.

13. Reload handoff hard timeout
   - Commit: `feat: cap AwaitingHistory wait with hard timeout`.
   - Patch branch: `patch/reload-handoff-hard-timeout`.
   - Upstream inspiration: PR `#151` slice `097 Add hard timeout for reload handoff`, reimplemented for this stack.
   - Purpose: prevent the TUI from staying forever in reload recovery / `AwaitingHistory` after a daemon restart when the new server never sends the expected `History` event.
   - Runtime behavior:
     - `AwaitingHistory` now records a start time, deadline, and configured timeout.
     - Default timeout is 10 seconds, configurable through `[reload].awaiting_history_timeout_secs`.
     - On timeout, the client preserves local display messages, marks history as usable, clears the loading-session startup phase, shows a terse warning, and resumes normal input/queued follow-up dispatch.
     - `Esc` or `Ctrl+C`/`Ctrl+D` while waiting aborts the history check early and falls back to the same locally cached messages instead of quitting or freezing.
     - If the server `History` event arrives first, the normal history handler still wins and clears the pending reload status.
   - Touched paths:
     - `crates/jcode-config-types/src/lib.rs`
     - `src/config.rs`
     - `src/config/default_file.rs`
     - `src/tui/app.rs`
     - `src/tui/app/remote.rs`
     - `src/tui/app/remote/key_handling.rs`
     - `src/tui/app/remote/reconnect.rs`
     - `src/tui/app/remote/server_events.rs`
     - `src/tui/app/tests/remote_events_reload_03/part_01.rs`
     - `src/tui/app/tui_lifecycle.rs`
   - Validation: `cargo fmt --check`, `cargo check`, `cargo test awaiting_history --lib --no-fail-fast`, `cargo test reload --lib --no-fail-fast -- --test-threads=1`, and `cargo test remote --lib --no-fail-fast` (known unrelated remote-filter failures remain).
   - Binary reinstall required: yes, because this changes TUI reload recovery behavior and config surface.

14. Mermaid input non-blocking render queue
   - Commit: `feat: render mermaid diagrams off the TUI render thread`.
   - Patch branch: `patch/mermaid-input-non-blocking`.
   - Purpose: keep keyboard input responsive while Mermaid diagrams are first rendered or re-rendered at a new size.
   - Root cause found: the full markdown path already had a deferred Mermaid worker under the TUI draw context, but the lazy markdown path still called synchronous Mermaid rendering on cache miss. The deferred path also had rare fallback branches that could run the synchronous render if the queue/pending state failed.
   - Runtime behavior:
     - Mermaid cache hits still return the cached image immediately.
     - Deferred cache misses enqueue the existing background worker and return a placeholder instead of running parse/layout/SVG/PNG/font work on the render frame.
     - Lazy markdown rendering now honors the same deferred Mermaid context as the full renderer.
     - If the deferred pending map or worker channel is unavailable, the UI keeps the placeholder and logs a warning instead of freezing the input loop with a synchronous fallback render.
     - Background completion still populates the existing cache, bumps the deferred render epoch, and requests redraw through the installed render-completed hook.
   - User-visible placeholder: `↻ rendering mermaid diagram...` inline, or `↻ mermaid diagram rendering in sidebar...` when the diagram is side-panel only.
   - Touched paths:
     - `crates/jcode-tui-mermaid/src/mermaid_cache_render.rs`
     - `crates/jcode-tui-mermaid/src/mermaid_tests/part_01.rs`
     - `crates/jcode-tui-mermaid/src/mermaid_tests/part_02.rs`
     - `crates/jcode-tui-markdown/src/markdown_render_lazy.rs`
     - `crates/jcode-tui-markdown/src/markdown_tests/cases/wrapping_currency.rs`
   - Validation: `cargo check -p jcode-tui-mermaid`, `cargo check`, `cargo test -p jcode-tui-mermaid --no-fail-fast`, `cargo test -p jcode-tui-markdown test_lazy_renderer_deferred_mermaid_returns_placeholder_on_cache_miss --no-fail-fast`, and `cargo test mermaid --lib --no-fail-fast` (known unrelated filtered failure: `side_panel_mermaid_probe_reports_viewport_fill_for_underutilized_fit` expected `127%` but got `129%`).
   - Binary reinstall required: yes, because this changes TUI rendering/runtime behavior.

15. Ecosystem paths policy
   - Commit: `feat: enforce ecosystem path policy (global=jcode-only, project=4-way)`.
   - Patch branch: `patch/ecosystem-paths-policy`.
   - Purpose: codify the resource discovery rule that global resources are jcode-only while project-local resources can be discovered from the four supported ecosystem directories.
   - Policy:
     - Global: read only jcode-owned `~/.jcode/...` resources.
     - Project-local: read `.jcode`, `.claude`, `.agents`, and `.opencode` resources from the project working directory.
   - Runtime behavior:
     - First-run global skill import from `~/.claude/skills`, `~/.codex/skills`, and `~/.opencode/skills` is disabled.
     - First-run global MCP import from `~/.claude/mcp.json` and `~/.codex/config.toml` is disabled.
     - Project-local MCP discovery now includes `.agents/mcp.json` and `.opencode/mcp.json`, loaded before `.claude/mcp.json` and `.jcode/mcp.json` so `.jcode` has highest priority on duplicate server names.
     - Project-local skill discovery remains unchanged and continues to cover `.jcode/skills`, `.claude/skills`, `.agents/skills`, and `.opencode/skills`.
   - Rationale: installing Claude Code, Codex, or opencode globally should not silently feed their global resources into jcode. Cross-tool compatibility is opt-in at project scope through checked-in or local project directories.
   - Validation: `cargo check`, `cargo test skill --lib --no-fail-fast`, `cargo test mcp --lib --no-fail-fast`, `cargo test --lib --no-run`, and the 12-test known-failure smoke.
   - Binary reinstall required: yes, because this changes startup/config resource loading behavior.

16. Project-local agent profile config
   - Commit: `feat: merge agent profiles from project-local config`.
   - Patch branch: `patch/agent-profiles-project-merge`.
   - Purpose: make `[agents.profiles]` and deprecated `[agents.routes]` / `[agents.routing]` usable from self-contained project harness installs, not only from global `~/.jcode/config.toml`.
   - Config files:
     - global `~/.jcode/config.toml`
     - project shared `<project>/.jcode/config.toml`
     - project local/private `<project>/.jcode/config.local.toml`
   - Merge behavior:
     - global agents first
     - project shared agents second
     - project local/private agents third
     - map fields (`routing`, `routes`, `profiles`) merge by key, with later layers overriding earlier layers
     - scalar fields (`swarm_model`, `memory_model`, `memory_sidecar_enabled`) use project values when the project agents section supplies them
   - Runtime behavior: the `subagent`/`task` tool resolves its callable profile, deprecated route, and swarm model from the active session working directory when available; schema-only/global introspection keeps the global-only fallback.
   - Validation: `cargo check`, `cargo test agents_for_working_dir --lib --no-fail-fast`, `cargo test config_tests --lib --no-fail-fast`, `cargo test task --lib --no-fail-fast`, `cargo test --lib --no-run`, and the 13-test known-failure smoke.
   - Binary reinstall required: yes, because this changes runtime subagent routing/config behavior.

17. Project-local markdown agent profiles
   - Commit: `feat: load agent profiles from project-local markdown files`.
   - Patch branch: `patch/agent-profiles-md-files`.
   - Purpose: let project harnesses ship callable agent profiles as Claude-style markdown files instead of requiring users to edit `[agents.profiles]` TOML.
   - Agent profile directories:
     - `<project>/.jcode/agents/*.md`
     - `<project>/.claude/agents/*.md`
     - `<project>/.agents/agents/*.md`
     - `<project>/.opencode/agents/*.md`
   - Frontmatter behavior:
     - `name` is optional; if absent, the filename stem becomes the profile key.
     - Accepted aliases include `model`, `effort` / `reasoning-effort` / `reasoning_effort`, `description` / `desc`, `when` / `when_to_use`, and `system-prompt` / `system_prompt`.
     - `allowed-tools` / `allowed_tools` / `tools` are tolerated for ecosystem compatibility but currently ignored because jcode does not enforce per-profile tool gates.
     - Files with no frontmatter still load, using the entire markdown body as the profile prompt.
   - Merge behavior:
     - global `~/.jcode/config.toml` agents first
     - project markdown agents second, with markdown source precedence `.jcode > .claude > .agents > .opencode`
     - project `.jcode/config.toml` and `.jcode/config.local.toml` agents last so users can override framework-shipped markdown profiles with TOML
   - Runtime behavior: `agents_for_working_dir(Some(project))` includes project-local markdown profiles in the effective `AgentsConfig`; `agents_for_working_dir(None)` remains global-only.
   - Validation: `cargo check`, `cargo test agents_for_working_dir --lib --no-fail-fast`, `cargo test agent_profiles_md --lib --no-fail-fast`, `cargo test config_tests --lib --no-fail-fast`, `cargo test task --lib --no-fail-fast` (known baseline `spawn_target_creates_one_child_session_and_runs_task` failure remains), `cargo test --lib --no-run`, and the 13-test known-failure smoke.
   - Binary reinstall required: yes, because this changes runtime subagent profile discovery behavior.

18. Project-local markdown slash commands
   - Commit: `feat: load slash commands from project-local markdown files`.
   - Patch branch: `patch/project-slash-commands`.
   - Purpose: let project harnesses ship Claude-style slash commands as markdown files that expand into normal user prompts in the TUI.
   - Command directories:
     - `<project>/.jcode/commands/*.md`
     - `<project>/.claude/commands/*.md`
     - `<project>/.agents/commands/*.md`
     - `<project>/.opencode/commands/*.md`
   - Frontmatter behavior:
     - Frontmatter is optional; files without frontmatter use the entire markdown body as the prompt and the filename stem as the command name.
     - Accepted aliases include `description` / `desc`, `argument-hint` / `argument_hint` / `args`, `allowed-tools` / `allowed_tools` / `tools`, and `model`.
     - `allowed-tools` and `model` are parsed for ecosystem compatibility but are informational only in this patch.
   - Runtime behavior:
     - Built-in slash commands win over project commands.
     - Installed skills win over project commands.
     - Matching project commands render their markdown body with `$ARGUMENTS` substitution; if no placeholder exists, non-empty user args are appended as a separate paragraph.
     - Rendered text is submitted as a normal user message. Project commands do not execute external code.
     - Discovery is project-local only; no global `~/.claude/commands` or `~/.jcode/commands` are loaded.
   - Precedence: `.jcode > .claude > .agents > .opencode`.
   - Validation: `cargo check`, `cargo test project_commands --lib --no-fail-fast`, `cargo test project_command --lib --no-fail-fast`, `cargo test agents_for_working_dir --lib --no-fail-fast`, `cargo test skill --lib --no-fail-fast`, `cargo test --lib --no-run`, and the 13-test known-failure smoke.
   - Binary reinstall required: yes, because this changes TUI slash command discovery and dispatch behavior.

19. Global jcode markdown agents and slash commands
   - Commit: `feat: load global jcode .md agents and slash commands`.
   - Patch branch: `patch/global-jcode-md-resources`.
   - Purpose: close the consistency gaps left by patches #17 and #18 by discovering jcode-owned global markdown resources alongside existing global `~/.jcode/skills/`, `~/.jcode/config.toml`, and `~/.jcode/mcp.json` support.
   - Global resource directories:
     - `~/.jcode/agents/*.md`
     - `~/.jcode/commands/*.md`
   - Policy:
     - Global discovery remains jcode-only per patch #15.
     - `~/.claude/agents`, `~/.claude/commands`, `~/.codex`, `~/.opencode`, and `~/.agents` global directories are not read.
     - Project-local discovery remains 4-way for `.jcode`, `.claude`, `.agents`, and `.opencode`.
   - Merge behavior:
     - agents: global TOML first, global `~/.jcode/agents/*.md` second, project markdown agents third, project `.jcode/config.toml` / `.jcode/config.local.toml` last.
     - slash commands: global `~/.jcode/commands/*.md` is loaded into the project command registry, then project markdown commands override duplicate names. Built-in commands and installed skills still take precedence at dispatch/autocomplete time.
   - Validation: `cargo check`, `cargo test agents_for_working_dir --lib --no-fail-fast`, `cargo test load_global_jcode --lib --no-fail-fast`, `cargo test project_commands --lib --no-fail-fast`, `cargo test agent_profiles_md --lib --no-fail-fast`, `cargo test --lib --no-run`, and the 13-test known-failure smoke.
   - Binary reinstall required: yes, because this changes runtime subagent profile and TUI slash command discovery behavior.

20. Full harness doctor CLI
   - Commit: `feat: add jcode doctor command for full harness diagnosis`.
   - Patch branch: `patch/jcode-doctor`.
   - Purpose: add a top-level `jcode doctor` command, distinct from `jcode auth doctor`, so users can verify a project harness install in one pass.
   - Coverage:
     - global, project, and local `.jcode` TOML configuration parse status
     - configured lifecycle hooks and executable-path linting
     - loaded skills from global/project ecosystem directories
     - merged agent profiles with source attribution and collision warnings
     - loaded slash commands with built-in/skill collision warnings
     - declared MCP servers with config-only command linting, never starting servers
     - auth handoff note pointing to `jcode auth doctor`
   - CLI surface:
     - `jcode doctor` for human output
     - `jcode doctor --json` for pretty serde JSON
     - `jcode doctor --quiet` to hide healthy/info items in human output
   - Exit codes: `0` healthy, `1` warnings, `2` errors.
   - Validation: `cargo check`, `cargo test doctor --lib --no-fail-fast`, `cargo test --lib --no-run`, direct CLI smoke for human/json/quiet output, and the 13-test known-failure smoke.
   - Binary reinstall required: yes, because this adds a new top-level CLI command.

21. Swarm stability core fixes
   - Commit stack: `feat: auto-promote current session to swarm coordinator before cleanup`, `feat: add owned_only scope to swarm await_members`, `feat: include stale workers in default swarm cleanup target statuses`, and `feat: retry swarm spawn after coordinator self-promotion`.
   - Patch branch: `patch/swarm-stability-core`.
   - Source: adapted from PR #151 slices 089 (partial), 090, `b57ff273`, and `ad422ee9`.
   - Purpose: adopt the core stability fixes that prevent multi-agent flows from blocking on stale/unrelated workers and recover from coordinator role drift after reload/crash scenarios.
   - Scope:
     - `swarm cleanup` self-promotes the requester to coordinator before stopping owned cleanup candidates.
     - `swarm await_members` supports protocol field `owned_only: Option<bool>` and defaults to owned-only server-side when no explicit `session_ids` / `target_session` are provided.
     - owned-only await snapshots only non-terminal workers that report back to the requester and excludes stale statuses (`crashed`, `closed`, `disconnected`, `running_stale`).
     - default cleanup statuses include stale workers so Closed/Crashed drained sessions are removed by ordinary cleanup.
     - coordinator self-promotion eagerly demotes other coordinators in the same swarm, and spawn retries once after automatic self-promotion when the server denies spawn due to coordinator drift.
   - Explicitly deferred: PR #151 run-id infrastructure, swarm health/reconcile diagnostics, dry-run support, idempotency operation IDs, and other Phase B/C work.
   - Validation: `cargo check`, `cargo test communicate --lib --no-fail-fast`, `cargo test comm_await --lib --no-fail-fast`, `cargo test comm_control --lib --no-fail-fast`, `cargo test swarm --lib --no-fail-fast -- --test-threads=1`, `cargo test --lib --no-run`, and the 13-test known-failure smoke.
   - Binary reinstall required: yes, because this changes protocol and runtime swarm coordination behavior.

22. Swarm run-id infrastructure
   - Commit stack: `feat: tag swarm workers with run ids`, `feat: scope swarm await and cleanup by run id`, and `feat: scope swarm list output by run id`.
   - Patch branch: `patch/swarm-run-id`.
   - Source: adapted from PR #151 slices 092, 093, and 094 list portion. The health portion of 094 is deferred to Phase C.
   - Purpose: tag workers spawned by one orchestration run with a shared run id, then use that id to safely scope multi-run await, cleanup, and list operations.
   - Scope:
     - protocol and persisted swarm records gain optional `run_id` fields with backward-compatible `None` defaults.
     - `swarm spawn` and `swarm assign_next` accept explicit `run_id`; `swarm run_plan` and `swarm fill_slots` generate a fresh run id when omitted and propagate it to spawned workers.
     - `swarm await_members` and `swarm cleanup` accept optional `run_id` filters, and `run_plan` scopes its internal await/cleanup calls to its generated run id.
     - persisted await keys include run id so reload/reattach resumes the same scoped await.
     - `swarm list run_id=<id>` filters output to members tagged with that run id while unscoped list output remains unchanged.
   - Explicitly deferred: `swarm health`, `swarm reconcile`, `swarm cleanup dry_run`, and `operation_id` from later PR #151 slices.
   - Validation: `cargo check`, `cargo test communicate --lib --no-fail-fast`, `cargo test comm_await --lib --no-fail-fast`, `cargo test comm_control --lib --no-fail-fast`, `cargo test swarm --lib --no-fail-fast`, `cargo test --lib --no-run`, and the 13-test known-failure smoke.
   - Binary reinstall required: yes, because this changes protocol and runtime swarm coordination behavior.

23. Empty assistant response retry after tool result
   - Commit: `fix: retry empty assistant response after tool result`.
   - Patch branch: `patch/empty-response-retry`.
   - Purpose: prevent sessions from appearing silently idle when a provider returns an empty assistant response with no text and no tool calls immediately after a `tool_result` message.
   - Root cause: the turn loop persisted no assistant message when generated content blocks were empty, then treated the no-tool-call response as a successful turn end. This was observed with large/cold Claude prompts after `repair_missing_tool_outputs()` reset cache state and relocked a large tool list.
   - Runtime behavior:
     - If the latest stored message is a user `ToolResult` and the provider response has no text, no persisted assistant message, and no tool calls, the agent appends a brief system reminder asking the model to read the tool result and continue.
     - The retry quota is per-turn and bounded to one continuation to avoid loops.
     - Plain empty responses that do not follow a tool result keep the prior behavior and are not retried.
   - Touched paths:
     - `src/agent/response_recovery.rs`
     - `src/agent/turn_streaming_mpsc.rs`
     - `src/agent/turn_streaming_broadcast.rs`
     - `src/agent/turn_loops.rs`
     - `src/agent_tests.rs`
   - Validation: `cargo check`, `cargo test response_recovery --lib --no-fail-fast`, `cargo test empty_response --lib --no-fail-fast`, `cargo test turn_streaming --lib --no-fail-fast`, `cargo test agent --lib --no-fail-fast`, `cargo test --lib --no-run`, and the 13-test known-failure smoke.
   - Binary reinstall required: yes, because this changes agent turn-loop runtime behavior.

24. Alt+B early background race fix
   - Commit: `fix: preserve early Alt+B fire by moving background signal reset before ToolStart`.
   - Patch branch: `patch/altb-early-race`.
   - Purpose: make Alt+B reliable when the user presses it immediately after a tool becomes visible in the UI.
   - Root cause: `BackgroundToolSignal` is an `InterruptSignal` latch, but `run_turn_streaming_mpsc` reset the latch after `ToolStart` had already been emitted and just before entering the tool execution `select!`. An Alt+B fire in that window was cleared before the select could observe it.
   - Runtime behavior:
     - The mpsc turn loop now clears stale background-tool requests at the start of each provider turn, before any `ToolStart` can be emitted.
     - Once `ToolStart` is visible to the UI, an Alt+B fire remains latched until the tool execution select sees it and detaches the running tool.
     - A stale background request from a previous turn is cleared before the next tool can start, preventing false-positive auto-backgrounding.
     - `move_tool_to_background` now returns an explicit error event, plus a debug log, when no active background-tool signal is registered instead of silently acknowledging a no-op.
     - `turn_streaming_broadcast` was inspected and left unchanged because it does not have the async Alt+B detach/select pattern.
   - Touched paths:
     - `crates/jcode-agent-runtime/src/lib.rs`
     - `src/agent/turn_streaming_mpsc.rs`
     - `src/agent_tests.rs`
     - `src/server/client_lifecycle.rs`
   - Validation: `cargo check --all-targets`, `cargo test -p jcode-agent-runtime background_tool_signal`, `cargo test --lib --no-fail-fast altb_early_race`, `cargo test --lib --no-fail-fast interrupt_signal`, and `cargo test --lib --no-fail-fast turn_streaming`. A full `cargo test --lib --no-fail-fast` was attempted on this workstation but hit the local 10-minute harness timeout before completion; focused coverage for this patch passed.
   - Binary reinstall required: yes, because this changes turn-loop runtime behavior.


25. Background task delivery target routing
   - Commit: `fix: route background task notifications to parent/report-back delivery target`.
   - Patch branch: `patch/bg-delivery-target`.
   - Purpose: make background task completion/progress notifications route to the user-attached parent/report-back session instead of being absorbed by headless or detached child sessions.
   - Root cause:
     - `fanout_session_event` and background dispatch targeted only the task owner session id and did not follow `Session.parent_id` or `SwarmMember.report_back_to_session_id`.
     - `run_background_task_message_in_live_session_if_idle` treated the headless drain `event_tx` as a live client, so headless workers could consume their own completion path.
     - Alt+B adopted tools recorded only the owner session id, losing the parent delivery hint. `wake=true` policy remains deferred to a follow-up patch.
   - Runtime behavior:
     - `BackgroundTaskCompleted` and `BackgroundTaskProgressEvent` carry both `session_id` (owner/executor) and `delivery_session_id` (notification/wake target hint).
     - Dispatch resolves `delivery_session_id` through `SwarmMember.report_back_to_session_id` and persisted `Session.parent_id`, walking up to 10 ancestors and selecting the first session with live attached clients.
     - Live client detection now trusts only non-closed `event_txs` attachments, not headless drain channels.
     - Alt+B adopt preserves current `wake=false` behavior but stores the parent session as the delivery hint when available.
   - Touched paths:
     - `crates/jcode-background-types/src/lib.rs`
     - `src/bus.rs`
     - `src/background.rs`
     - `src/background/model.rs`
     - `src/background/tests.rs`
     - `src/server.rs`
     - `src/server/background_tasks.rs`
     - `src/server/tests.rs`
     - `src/agent/turn_streaming_mpsc.rs`
     - `src/tui/app/local.rs`
     - `src/message/tests.rs`
     - `src/tui/app/tests/remote_startup_input_02/part_02.rs`
     - `src/tool/selfdev/tests.rs`
   - Validation: `cargo check --all-targets`, `cargo test --lib server::tests::background_ --no-fail-fast`, `cargo test --lib background --no-fail-fast`, plus requested broader sweeps. Known failures remained limited to the documented inventory; focused new regressions passed.
   - Binary reinstall required: yes, because this changes runtime delivery routing and bus event payloads.

26. Alt+B background task parent wake
   - Commit: `feat: wake parent turn on Alt+B background task completion`.
   - Patch branch: `patch/altb-wake-parent`.
   - Purpose: M1 follow-up that makes an Alt+B-detached tool wake the parent/report-back turn when the adopted background task completes, so the model receives the real completion result instead of only the synthetic detached ToolResult.
   - Runtime behavior:
     - `BackgroundTaskManager::adopt_with_delivery` now accepts an explicit `wake_on_completion` policy.
     - The generic `adopt(...)` helper keeps the prior `wake=false` default for compatibility.
     - The MPSC Alt+B detach path passes `wake=true` while preserving the parent delivery session hint added by `patch/bg-delivery-target`.
     - `turn_streaming_broadcast` was inspected and left unchanged because it has no `adopt_with_delivery` Alt+B detach path in this branch.
   - Touched paths:
     - `src/background.rs`
     - `src/background/tests.rs`
     - `src/agent/turn_streaming_mpsc.rs`
   - Validation: `cargo check --all-targets`, `cargo test --lib background --no-fail-fast`, `cargo test --lib agent::tests --no-fail-fast`, and `cargo test --lib server::background_tasks --no-fail-fast`.
   - Binary reinstall required: yes, because this changes turn-loop wake behavior after Alt+B background completion.

27. Lifecycle hook events for session/response boundaries
   - Commit: `feat: add session.stop and response.completed lifecycle hooks`.
   - Patch branch: `patch/lifecycle-hooks`.
   - Purpose: extend command hooks beyond tool execution with `session.stop` and `response.completed` so external harness automation can observe true session termination and final assistant turn completion.
   - Runtime behavior:
     - `session.stop` fires after a session is removed from live server state for real close/crash disconnect cleanup, with `reason = "disconnect"`, working directory, and message count.
     - Reload-triggered temporary detach maps to the existing `Reloading` disposition and is explicitly skipped, preventing duplicate external stop notifications during daemon reload handoff.
     - `response.completed` fires from the actual turn-loop exit path after empty-response retry and incomplete-response continuation checks, once per final assistant response, with assistant message id, stop reason, tool-call count, and output char count.
     - Lifecycle hooks match by `event` only; any configured `tool` field is ignored. Blocking lifecycle hooks ignore deny decisions and failures are logged as warnings without stopping the turn/session.
   - Touched paths:
     - `src/hooks.rs`
     - `src/agent/turn_loops.rs`
     - `src/agent/turn_streaming_mpsc.rs`
     - `src/server/client_disconnect_cleanup.rs`
     - `src/config/default_file.rs`
     - `crates/jcode-config-types/src/lib.rs`
   - Validation: `cargo check --all-targets`, `cargo test --lib hooks --no-fail-fast`, and `cargo test --lib client_disconnect_cleanup --no-fail-fast`. Broader requested sweeps `cargo test --lib agent::tests --no-fail-fast` and `cargo test --lib server --no-fail-fast` still show pre-existing environment/order-sensitive failures unrelated to this patch (`env_snapshot_detail_is_minimal_for_empty_sessions_and_full_after_history`, spawn working-dir/history busy-agent assertions).
   - Binary reinstall required: yes, because this changes runtime hook behavior.

28. Anthropic OAuth tool advertisement / dispatch alignment (M12, M13)
   - Commit: `fix(m12,m13): align anthropic OAuth tool advertisements with dispatch handlers`.
   - Patch branch: `patch/anthropic-oauth-tool-schema-align`.
   - Purpose: fix two stale OAuth tool advertisements that did not match local dispatch handlers, causing runtime errors whenever the model called the advertised name.
     - M13 (`schedule` / `ScheduleWakeup`): advertised schema required `delaySeconds`/`reason`/`prompt` for an unimplemented `/loop` dynamic mode, but the real dispatcher (`ScheduleTool` in `src/tool/ambient.rs`) requires `task`. Calls returned `missing field 'task'`.
     - M12 (`ToolSearch`): advertised name had no incoming mapping in `crates/jcode-provider-core/src/anthropic.rs::anthropic_map_tool_name_from_oauth` (comment claimed no local analogue), so calls returned `Unknown tool: ToolSearch`. The Claude provider already routes `ToolSearch <-> codesearch`; this patch mirrors that mapping for the Anthropic OAuth provider.
   - Runtime behavior:
     - `ToolSearch <-> codesearch` mapping added in both directions in the provider-core helper, with a corresponding round-trip case in `oauth_tool_name_mapping_is_reversible_for_known_tools`.
     - Advertised `ScheduleWakeup` schema now matches `ScheduleTool::parameters_schema` (required `task`, optional `wake_in_minutes`/`wake_at`/`priority`/`relevant_files`/`background_context`/`success_criteria`/`target`).
     - Advertised `ToolSearch` schema now matches `CodeSearchTool::parameters_schema` (required `query`, optional `max_tokens`).
     - Wire-side OAuth tool names are unchanged (`ScheduleWakeup`, `ToolSearch`); only schemas and the missing incoming mapping changed, so external prompt-cache breakpoints stay stable.
   - Background: this is the only provider with this risk. OpenAI/Gemini/Copilot/Cursor/Bedrock/Antigravity all serialize `tool.input_schema` from the `ToolRegistry` directly. Anthropic OAuth alone uses a hand-rolled JSON whitelist in `format_tools` (is_oauth=true branch). The structural fix (advertise from `ToolDefinition`) is tracked separately as milestone M16.
   - Touched paths:
     - `crates/jcode-provider-core/src/anthropic.rs`
     - `src/provider/anthropic.rs`
     - `src/provider/anthropic_tests.rs`
   - Validation: `cargo build --release --bin jcode`, focused `cargo test --release --lib provider::anthropic_tests::test_oauth_schedule_tool_advertised_schema_matches_dispatch` and `... ::test_oauth_tool_search_advertised_schema_matches_codesearch_dispatch`, plus `cargo test --release --lib -p jcode-provider-core anthropic`.
   - Binary reinstall required: yes, because this changes the advertised tool schemas the model sees.

29. Compaction failure cooldown + streak gate (M14, M14a)
   - Commit: `e71713ba` `fix(compaction): cooldown + failure-streak gate to stop runaway loops (M14/M14a)`.
   - Patch branch: `patch/compaction-failure-cooldown`.
   - Purpose: stop two related runaway loops the user observed in production.
     - M14 ("`/compact` 동작 안 함"): proactive/semantic auto-compaction kept firing on every new turn after the background summarizer errored once. Root cause: the failure paths in `CompactionManager::check_and_apply_compaction_with` did not reset `turns_since_last_compact`, so the cooldown anti-signal (`min_turns_between_compactions`) was effectively only enforced when the previous compaction succeeded. Once any failure happened the counter monotonically grew and re-triggered on every new turn forever.
     - M14a ("emergency compaction 22회 연속"): the per-turn `MAX_CONTEXT_LIMIT_RETRIES` retry budget cannot see session-wide repetition, so the same wedged turn fired emergency hard-compactions over and over.
   - Runtime behavior:
     - New session-wide counter `consecutive_compaction_failures` (saturating, 0..N) on `CompactionManager`.
     - New constant `jcode_compaction_core::MAX_CONSECUTIVE_COMPACTION_FAILURES = 3` (small enough to stop billing the user fast, large enough to recover from a transient error).
     - New helpers `note_compaction_success()` / `note_compaction_failure()` / `consecutive_compaction_failures()` so all paths use one place to bookkeep both the cooldown counter and the failure streak.
     - Three short-circuit points read the streak and refuse further attempts once the cap is hit:
       1. `should_compact_with` (proactive/semantic/reactive auto-trigger) returns `false`.
       2. `Agent::try_auto_compact_after_context_limit` returns `false` so the per-turn retry budget rejects further attempts and the context-limit error propagates to the model normally.
       3. `ensure_context_fits` synchronous critical-threshold hard-compact records success/failure into the same counter.
     - All failure paths now also zero `turns_since_last_compact` so the cooldown anti-signal stays effective even when the previous attempt errored out.
   - Side change: `tool::codesearch` is now `pub(crate)` so the M12 OAuth tool-schema regression test added in #28 can reach `CodeSearchTool::parameters_schema`. Without this `cargo test --lib` on this branch fails to compile.
   - Touched paths:
     - `crates/jcode-compaction-core/src/lib.rs`
     - `src/compaction.rs`
     - `src/agent/compaction.rs`
     - `src/compaction_tests.rs`
     - `src/tool/mod.rs`
   - Validation: `cargo build --release --bin jcode` (1m 33s, ok), focused `cargo test --release --lib compaction::tests::` (30 tests, all passed including 4 new ones: `test_note_compaction_success_resets_cooldown_and_streak`, `test_note_compaction_failure_zeros_cooldown_counter`, `test_should_compact_with_short_circuits_after_failure_streak`, `test_note_compaction_failure_saturates`), and re-ran #28 OAuth tests for regression (4/4 passed).
   - Binary reinstall required: yes (changes runtime compaction trigger gating and emergency recovery behaviour).
   - Deployment note (2026-05-10): the previous deployment pattern of overwriting only `~/.local/bin/jcode` left the active TUI server (`~/.jcode/builds/stable/jcode`) on an old binary. Going forward, every fix that changes runtime behaviour should also re-link `~/.jcode/builds/stable/jcode` and `~/.jcode/builds/current/jcode` to a new `versions/lazydino-<sha>/jcode` directory so the next `/restart` actually loads the patched build. Consider folding this into a deploy helper script.

30. Hook merge dedupe when global config path == project-discovered path (M9)
   - Commit: `82b7c81f` (deploy/m9-m10) / source `9774d9a6` `fix(m9): dedupe hooks_for_working_dir when global path == project path`.
   - Patch branch: `patch/hook-config-dedupe` (based on `patch/project-local-hook-config`).
   - Purpose: stop every lifecycle and tool hook from firing twice when `jcode` is launched from `~` or any ancestor of `~/.jcode`. Root cause was introduced by `ed918aeb` ("feat: load project-local hook config"): `Config::hooks_for_working_dir` unconditionally appends hooks discovered under `<cwd>/.jcode/` to the already-loaded global hooks. When the working_dir's nearest project-config search resolved to `~/.jcode/config.toml` itself, the same file got read twice and each hook command got registered twice.
   - Fix: new private helper `Config::paths_resolve_to_same_file` (canonicalize both paths, fall back to lexical equality if either canonicalize fails). `hooks_for_working_dir` now skips a project candidate path when it resolves to the same filesystem location as `Config::path()`. Distinct project paths still merge as before.
   - Touched paths:
     - `src/config/config_file.rs`
     - `src/config_tests.rs`
   - Validation: 2 new regression tests pass: `test_hooks_for_working_dir_dedupes_when_global_path_equals_project_path` proves a same-path setup yields 1 command instead of 2; `test_hooks_for_working_dir_still_merges_distinct_project_path` proves legitimate project-local hooks are not suppressed by the dedupe. Pre-existing `test_project_local_hooks_*` tests (4) still pass.
   - Binary reinstall required: yes (runtime hook registration path).

31. Track and flush non-blocking hooks before single-shot CLI exit (M10)
   - Commit: `13dd3132` (deploy/m9-m10) / source `e74791df` `fix(m10): track and flush non-blocking hooks before single-shot CLI exit`.
   - Patch branch: `patch/lifecycle-hook-cli-flush` (based on `patch/lifecycle-hooks`).
   - Purpose: stop lifecycle hooks (`response.completed`, `session.stop`) and `blocking=false` tool hooks from being silently dropped when non-server `jcode` entrypoints (e.g. `jcode run`, oneshot `jcode --version`-like flows that still trigger hooks) exit. Root cause: `run_tool_hooks` and `run_lifecycle_hook_commands` used fire-and-forget `tokio::spawn`. The `JoinHandle` was dropped immediately, so when `runtime.block_on(jcode::run())` returned, the runtime was dropped before the hook task could finish. Because `run_hook_command` sets `kill_on_drop(true)` on the child process, the spawned shell got killed mid-execution. `jcode serve` did not show the bug only because its event loop runs forever.
   - Fix: process-global registry `OnceLock<Mutex<Vec<JoinHandle<()>>>>` exposed via:
     - `pending_nonblocking_hooks()` (lazy-init).
     - `spawn_tracked_nonblocking_hook(future)` (replaces both `tokio::spawn` call sites in `src/hooks.rs`).
     - `pub async fn flush_nonblocking_hooks(timeout: Duration) -> usize` drains the registry and awaits each handle, bounded by the timeout so a slow/hung hook cannot wedge process exit. Returns the count of completed handles for observability.
   - `src/cli/startup.rs::run` calls `flush_nonblocking_hooks(Duration::from_secs(5))` between `dispatch::run_main(args).await` and the `Err` propagation, so the flush runs whether the dispatch succeeded or failed. The 5s budget matches the existing tool-hook timeout for well-behaved hooks.
   - Touched paths:
     - `src/hooks.rs`
     - `src/cli/startup.rs`
   - Validation: 3 new regression tests pass (serial mutex `M10_GLOBAL` because the registry is process-global): `flush_nonblocking_hooks_awaits_tracked_handle` (tracked side effect runs and is reported), `flush_nonblocking_hooks_returns_zero_when_empty` (no-hook fast path short-circuits regardless of timeout, required for hot path), `flush_nonblocking_hooks_bounded_by_timeout` (a never-returning hook is dropped after the deadline, flush returns 0).
   - Long-running `jcode serve` is unaffected: the flush still runs at shutdown but typically finds the slot empty (each `tokio::spawn` task has long since completed). Server graceful-shutdown semantics are unchanged.
   - Binary reinstall required: yes (hook spawn/flush plumbing on a hot path).

32. Deep-merge agent profiles per key (M47-C0)
   - Commit: `539c8f47` `agents: deep-merge profiles per key so host configs can override one field`.
   - Patch branch: `patch/m47-c0-deep-merge-profiles`.
   - Purpose: previously `[agents.profiles.<name>]` merges across layers (global TOML, global md, project md, project TOML) used `BTreeMap::extend`, which silently replaced the entire profile when a host file mentioned the same key. A host that adjusted only `model` would wipe inherited `description`/`when`/`prompt`/`effort`/`variant` from the global definition. Deep-merge keeps framework defaults intact while letting host configs adjust one field at a time.
   - Implementation:
     - New `AgentRouteConfig::merge_from(other)` in `crates/jcode-config-types/src/lib.rs`: per-field override, `Option<String>` fields only overwrite when `Some(non-empty)` in `other`; `when: Vec<String>` is replaced wholesale only when `other` supplies a non-empty list.
     - `PartialAgentsConfig::apply_to` and the two md-layer loops in `Config::agents_for_working_dir` (`src/config/config_file.rs`) all switched from `.extend()` / `.insert()` to `entry(name).and_modify(|e| e.merge_from(p.clone())).or_insert(p)`.
   - Host-wins ordering preserved (global TOML < global md < project md < project TOML); the change is that each layer overrides only the fields it sets instead of replacing the whole profile.
   - Touched paths:
     - `crates/jcode-config-types/src/lib.rs`
     - `src/config/config_file.rs`
     - `src/agent_profiles_md.rs` (2 new regression tests)
   - Validation: 24 `agents_for_working_dir*` tests pass, including 2 new ones: `agents_for_working_dir_project_toml_deep_merges_into_global_md`, `agents_for_working_dir_deep_merge_replaces_when_list_when_set`. Existing `test_agents_for_working_dir_project_overrides_global_same_key` still passes (host-wins semantics maintained).
   - Binary reinstall required: yes (runtime profile merge behavior).

33. Silent-skip `set_reasoning_effort` on providers without an effort surface (M47-C1)
   - Commit: TBD `provider: silently skip set_reasoning_effort on non-OpenAI providers (M47-C1)`.
   - Patch branch: `patch/m47-c1-effort-silent-skip` (parent: `patch/m47-c0-deep-merge-profiles`).
   - Purpose: stop noisy `error!` log on every Claude/Gemini/Bedrock/Copilot/Cursor/Antigravity session whose persisted state still carries an OpenAI-style `reasoning_effort` value. The historical hard error here surfaced as `"Failed to set effort: Reasoning effort is only supported for OpenAI models"` every time `restore_reasoning_effort_from_session` ran on a non-OpenAI provider. The effort dimension is provider-specific (M47 plan); missing on these backends is "not applicable", not a failure.
   - Implementation:
     - `src/provider/mod.rs::MultiProvider::set_reasoning_effort`: the catch-all `_ => Err(...)` arm becomes `other => { logging::debug(...); Ok(()) }`. OpenAI and OpenRouter arms unchanged so DeepSeek/GLM reasoning paths keep working.
     - `src/agent/provider.rs::restore_reasoning_effort_from_session`: error branch downgraded from `logging::error` to `logging::debug` because a real error here now means the active provider supports effort but rejected the value (malformed level), which is non-critical.
   - Touched paths:
     - `src/provider/mod.rs`
     - `src/agent/provider.rs`
     - `src/provider/tests/model_resolution.rs` (6 new regression tests)
   - Validation: 6 new tests pass — `set_reasoning_effort_silently_skips_on_{claude,gemini,bedrock,cursor,copilot,antigravity}`. Existing 320 `provider::*` tests still pass, including the OpenRouter DeepSeek reasoning_effort suite (`direct_deepseek_profile_exposes_max_reasoning_effort`, `direct_deepseek_chat_request_sends_reasoning_effort`, `non_deepseek_compatible_profile_does_not_expose_reasoning_effort`).
   - Binary reinstall required: yes (runtime log noise + future effort-setting paths).

34. Apply route effort via provider classification, not raw `gpt-`/`openai/` prefix (M47-C2)
   - Commit: TBD `task: apply route effort whenever provider supports reasoning_effort (M47-C2)`.
   - Patch branch: `patch/m47-c2-effort-apply-via-provider` (parent: `patch/m47-c1-effort-silent-skip`).
   - Purpose: align `SubagentTool::should_apply_route_effort` with the actual provider acceptance rule in `MultiProvider::set_reasoning_effort`. Previously the spawn path tested `model.starts_with("gpt-") || model.starts_with("openai/")`, which silently dropped route effort for OpenRouter reasoning models (DeepSeek direct profile, GLM, Kimi, etc.) — even though the provider would accept and use the value. After M47-C1 silent-skip, this internal mismatch became the only barrier between agent profile `effort:` keys and OpenRouter-served reasoning models.
   - Implementation:
     - `src/tool/task.rs::SubagentTool::should_apply_route_effort`: replace the string prefix check with `provider_for_model(model)` matching `Some("openai") | Some("openrouter")`. Non-OpenAI providers (Claude/Gemini/Bedrock/...) continue to skip in two places: here (so `session.reasoning_effort` is never populated with an ignored value) and in `MultiProvider::set_reasoning_effort` (silent skip from M47-C1). Unknown / empty models also skip.
   - Touched paths:
     - `src/tool/task.rs`
   - Validation: 14 `tool::task::tests::*` pass; the broadened `route_effort_applies_only_to_openai_style_models` now also asserts effort applies to `deepseek/deepseek-r1`, `zhipu/glm-4-6`, `moonshot/kimi-k2` (DeepSeek/GLM/Kimi OpenRouter reasoning profiles) and skips for `claude-opus-4-7[1m]`, `gemini-2.5-flash`, unknown models, and the empty string.
   - Behavior change: GLM/DeepSeek/Kimi agent profiles with `effort:` now route the value into `session.reasoning_effort`, which `MultiProvider::set_reasoning_effort` already accepts for OpenRouter. No change for OpenAI direct or for non-reasoning providers.
   - Binary reinstall required: yes (subagent spawn path).

35. Agent profile schema gains `context` and `thinking` dimensions (M47-C3)
   - Commit: `0865781c` `agents: add context and thinking dimensions to AgentRouteConfig (M47-C3)`.
   - Patch branch: `patch/m47-c3-route-config-context-thinking` (parent: `patch/m47-c0-deep-merge-profiles`, independent of C-1/C-2).
   - Purpose: profile schema in 2026-05 carried only `model`, `effort`, and `variant`, which forced Claude long-context, Gemini thinking, and OpenRouter Kimi/GLM thinking to ride on `variant="max"`'s provider-aware fallback. M47-C3 introduces two first-class optional fields so a single SSOT can target every persona explicitly. The lazy-harness 4-provider goal (Claude/GPT/Gemini/GLM) is the immediate consumer.
   - Implementation:
     - `crates/jcode-config-types/src/lib.rs::AgentRouteConfig`: add `context: Option<String>` and `thinking: Option<bool>` with rustdoc covering provider-specific behavior (Claude `[1m]` mapping for `context = "1m"`, Anthropic/Gemini/OpenRouter thinking surfaces, OpenAI ignores).
     - Same file `AgentRouteConfig::merge_from`: deep-merge the two new fields. `context` follows the existing `Option<String>` rule (overwrite only when `Some(non-empty)`); `thinking` overwrites whenever `other.thinking.is_some()` so `thinking = false` correctly turns off an inherited `thinking: true`.
     - `src/agent_profiles_md.rs::parse_agent_md_file`: read `context` / `context-window` / `context_window` / `context-length` as the string field, and `thinking` / `extended-thinking` / `extended_thinking` / `thinking-budget` / `thinking_budget` as the bool field.
     - New `bool_field` helper alongside `string_field`: accepts real YAML booleans, the strings `"true"/"false"/"yes"/"no"/"on"/"off"/"enabled"/"disabled"` (case insensitive), and treats positive integer budgets as `Some(true)` so ecosystem profiles shipping `thinking-budget: 8192` map cleanly. A future milestone may add a dedicated numeric budget field.
   - Touched paths:
     - `crates/jcode-config-types/src/lib.rs`
     - `src/agent_profiles_md.rs` (helper + 6 new regression tests)
   - Validation: 15 `agent_profiles_md::tests::*` pass, including 6 new ones: `parse_agent_md_file_reads_context_and_thinking_fields`, `parse_agent_md_file_accepts_context_window_and_extended_thinking_aliases`, `parse_agent_md_file_thinking_budget_integer_maps_to_true`, `parse_agent_md_file_thinking_zero_maps_to_false`, `agents_for_working_dir_deep_merge_inherits_thinking_when_context_only_in_host`, `agents_for_working_dir_deep_merge_explicit_thinking_false_overrides_global`. `config::tests::*` (54) and `tool::task::tests::*` (14) still pass.
   - Behavior change: the new fields are optional, deserialize-default, and ignored by downstream code until later M47 stages (C-4/C-5) wire them into provider behavior. Existing TOML and markdown profiles parse unchanged.
   - Binary reinstall required: yes (config schema change; rust types and frontmatter parsing must match the live binary).

36. Provider trait gains context-window and thinking dimensions (M47-C4)
   - Commits:
     - `2781d68a` `provider: add context and thinking dimensions to Provider trait (M47-C4 step 1)` (trait + MultiProvider/JcodeProvider dispatch)
     - `c158bacb` `provider: anthropic implements context_preference and supports_thinking (M47-C4 step 2)` (Anthropic [1m] suffix wiring + tests)
     - `d367dbdb` `provider: gemini and openrouter declare thinking surface (M47-C4 step 3)` (Gemini preference field + OpenRouter capability declaration)
   - Patch branch: `patch/m47-c4-provider-trait-dimensions` (parent: `patch/m47-c3-route-config-context-thinking`).
   - Purpose: expose declarative context-window and thinking dimensions on every provider so the M47-C5 variant resolver (and a future TUI picker, M48) can route `variant = "max"` and explicit `context:` / `thinking:` profile keys to the right channel per backend. Currently `variant="max"` only routes to Anthropic 1M (via `apply_route_variant_to_model`) and OpenAI reasoning_effort=xhigh (via `normalize_route_effort`); Gemini thinking and OpenRouter Kimi/GLM thinking have no first-class surface.
   - Surface added on `Provider`:
     - `available_contexts() -> Vec<&'static str>` (default empty = not exposed)
     - `context_preference() -> Option<String>`
     - `set_context_preference(&str) -> Result<()>` (default Ok = silent skip)
     - `supports_thinking() -> bool` (default false)
     - `thinking_enabled() -> Option<bool>`
     - `set_thinking(bool) -> Result<()>` (default Ok = silent skip)
   - Implementations:
     - Anthropic: `available_contexts() = ["200k","1m"]`, `context_preference()` reads `model.ends_with("[1m]")`, `set_context_preference("1m" | "1m-context" | "long" | "long-context")` appends the `[1m]` suffix idempotently, `set_context_preference("200k" | "default" | "short" | "short-context")` strips it, unknown values are debug-logged no-ops. `supports_thinking() = true` (declarative; the interleaved-thinking-2025-05-14 beta is already in `ANTHROPIC_OAUTH_BETA_HEADERS`).
     - Gemini: new `thinking_enabled: Arc<RwLock<Option<bool>>>` field threaded through `new()`/`fork()`/`Clone`/per-stream snapshot. `supports_thinking() = true`. `set_thinking(b)` stores `Some(b)`. The Gemini request builder may consume it via `thinkingConfig.thinkingBudget` in a follow-up commit; M47-C4 only adds the declarative surface.
     - OpenRouter: `supports_thinking() = true`. The existing `JCODE_OPENROUTER_THINKING` env override (`OpenRouterProvider::thinking_override`) remains the authoritative request-time switch; M47-C4 leaves the env path intact and only declares the capability so the variant resolver recognizes thinking-on as intentional.
     - OpenAI, Bedrock, Cursor, Copilot, Antigravity: keep default impls (no surface exposed).
   - MultiProvider dispatch routes each dimension to the active backend. Claude prefers `anthropic_provider()` (HTTPS API with `[1m]` routing) and falls back to `claude_provider()` (CLI). Non-implementing branches silent-skip with a debug log, matching the M47-C1 semantic so a single SSOT can carry every field for every persona without raising on unrelated backends.
   - JcodeProvider (desktop/UI wrapper) forwards all six methods to its inner `MultiProvider` so runtime profile application keeps full surface parity.
   - Touched paths:
     - `crates/jcode-provider-core/src/lib.rs` (trait surface)
     - `src/provider/mod.rs` (MultiProvider dispatch)
     - `src/provider/jcode.rs` (wrapper forwards)
     - `src/provider/anthropic.rs` (context_preference + supports_thinking)
     - `src/provider/anthropic_tests.rs` (5 new regression tests)
     - `src/provider/gemini.rs` (thinking_enabled field + accessors)
     - `src/provider/openrouter_provider_impl.rs` (supports_thinking declaration)
   - Validation: `cargo check -p jcode` clean; 55 `provider::anthropic::tests::*` pass (including 5 new `set_context_preference_*` / `anthropic_available_contexts_*` / `anthropic_supports_thinking_*` cases); 470 of 471 `provider::*` tests pass — the single failure `provider_catalog::provider_catalog_tests::auth_profile_env_application_flushes_stale_openrouter_catalog_state` is a pre-existing flaky env-lock test that passes on solo invocation.
   - Binary reinstall required: yes (provider trait surface change + Anthropic runtime context-preference behavior).

37. Provider-aware variant resolver decomposes `variant=max` per 4-provider matrix (M47-C5)
   - Commit: `18b73c8b` `task: provider-aware variant resolver decomposes variant into effort/context/thinking (M47-C5)`.
   - Patch branch: `patch/m47-c5-variant-resolver` (parent: `patch/m47-c4-provider-trait-dimensions`).
   - Purpose: the historical `variant = "max"` shortcut routed to two channels via overlapping helpers (`apply_route_variant_to_model` → Claude `[1m]` suffix, `normalize_route_effort("max")` → `xhigh` effort applied only on `gpt-*`/`openai/*`). That worked for Claude long-context and OpenAI reasoning, but Gemini thinking and OpenRouter Kimi/GLM thinking had no first-class mapping even after M47-C4 exposed the declarative `supports_thinking()` surface on those backends. M47-C5 introduces a provider-aware resolver so a single `variant = "max"` (or explicit `context: / thinking:` profile fields) routes to the right channel per backend.
   - Implementation:
     - `src/tool/task.rs::ResolvedSubagentRoute`: gain `context: Option<String>` and `thinking: Option<bool>` so the spawn path can forward all five dimensions to the child session in M47-C6.
     - New `ResolvedVariantDimensions` struct (effort / context / thinking) and `SubagentTool::resolve_variant_dimensions_for_provider(model, variant)`. The resolver looks up `provider_for_model(model)` and maps `variant="max"` per the 4-provider matrix: Claude → `context = "1m"`; OpenAI → `effort = "xhigh"`; Gemini → `thinking = true`; OpenRouter → `effort = "xhigh"` + `thinking = true`; unknown → `effort = normalize_route_effort` fallback (back-compat). Other variants (`pro`/`fast`/unknown/empty) return `None`.
     - `route_for_subagent_type` now consults the resolver and merges with explicit profile fields. Explicit `effort` / `context` / `thinking` win over variant fallback so a SSOT can target a backend without provider-aware mapping.
     - New `apply_route_context_to_model` helper mirrors `set_context_preference`: an explicit `context = "1m"` on a Claude model normalizes the model id with `[1m]` so the child session sees a consistent `model + context` pair. Non-Claude models pass through unchanged.
   - Touched paths:
     - `src/tool/task.rs` (resolver + helper + 7 new regression tests)
   - Validation: 21 `tool::task::tests::*` pass (14 existing + 7 new) including the 4-provider matrix (`variant_max_on_{claude,openai,gemini,openrouter,unknown}_resolves_to_*`), `variant_resolver_returns_none_for_empty_or_unknown_variant`, and `apply_route_context_appends_1m_on_claude_and_strips_on_200k`. 15 `agent_profiles_md::tests::*` and 54 `config::tests::*` still pass.
   - Behavior change: the resolver is consumed inside `route_for_subagent_type` but the downstream `execute()` spawn path still only forwards `route.effort` to the child session. M47-C6 wires the new `context` and `thinking` dimensions into `Session` so `restore_provider_preferences_from_session` can apply them via the M47-C4 Provider trait surface. Until then the new fields are computed but unused at runtime — no observable behavior change for end users yet.
   - Binary reinstall required: yes (subagent spawn resolver change; downstream callers ready for next stage).

38. Session persists context_preference and thinking_enabled, restores all dims on load (M47-C6)
   - Commit: `533431e9` `session: persist context_preference and thinking, restore all dims on session load (M47-C6)`.
   - Patch branch: `patch/m47-c6-session-preferences` (parent: `patch/m47-c5-variant-resolver`).
   - Purpose: wires the M47 5-dimension agent profile schema through session persistence so subagent spawn → save → reload → restore round-trips all three provider preferences (effort / context / thinking) into the live provider via the M47-C4 Provider trait surface.
   - Implementation:
     - `src/session.rs::Session` adds `pub context_preference: Option<String>` and `pub thinking_enabled: Option<bool>`. Both use `serde(default, skip_serializing_if = "Option::is_none")` so on-disk session JSON stays backwards-compatible (pre-M47-C6 readers ignore the new keys, pre-M47-C6 sessions deserialize with `None` defaults). Both `create_with_id` and `create` constructors initialize them to `None`.
     - `src/agent/provider.rs` adds `restore_provider_preferences_from_session` generalizing the historical `restore_reasoning_effort_from_session`. Each dimension restored independently: a session may carry `context=1m` on a Claude run and `thinking=true` on a Gemini run, the active provider applies the ones it supports while silently skipping the rest (M47-C1/C-4 semantics). When the session has no persisted preference for a dimension, the current provider value is captured back into the session so account/route switches do not lose user intent.
     - `restore_reasoning_effort_from_session` is preserved as a back-compat alias forwarding to the generalized restorer, so existing call sites in `Agent::new_with_session` and `Agent::restore_session` pick up context+thinking restoration for free. Effort branch uses `debug` logging on a real provider reject (M47-C1 semantics).
     - `src/tool/task.rs::execute` forwards `route.context` and `route.thinking` from the M47-C5 variant resolver to the freshly-created child session, mirroring the existing `route.effort` handling. Existing session overrides are preserved (only set when the child session has the dimension unset).
   - Touched paths:
     - `src/session.rs` (schema + constructors)
     - `src/agent/provider.rs` (generalized restorer + back-compat alias)
     - `src/tool/task.rs` (spawn path forwards context/thinking to child session)
     - `src/session_tests/cases.rs` (3 new round-trip regression tests)
   - Validation: 3 new tests pass — `test_save_persists_context_preference`, `test_save_persists_thinking_enabled_true_and_false`, `test_save_omits_unset_context_and_thinking_dimensions`. Existing `test_save_persists_reasoning_effort` still passes. 46 `agent::tests::*` and 97 `session::*` tests still pass. `cargo check -p jcode` clean post-merge into deploy (one trivial whitespace/comment merge conflict in `restore_provider_preferences_from_session` resolved by preserving the M47-C1 docstring).
   - Behavior change: agent profiles with explicit `context:` / `thinking:` or `variant: max` on Claude/Gemini/OpenRouter now propagate through the subagent spawn → session restore cycle so the live provider applies them. End-to-end effect visible: a `~/.jcode/agents/prometheus.md` profile with `model: claude-opus-4-7` + `variant: max` now persists `context_preference = "1m"` on its child session, and `restore_provider_preferences_from_session` calls `AnthropicProvider::set_context_preference("1m")` on load. Backward-compatible: pre-existing sessions deserialize with `None` dimensions and never call the new setters.
   - Binary reinstall required: yes (session schema + restorer behavior).

39. ProviderConfig gains provider-agnostic default_reasoning_effort/context/thinking (M47-C7)
   - Commit: `99ecbda7` `config: provider-agnostic default_reasoning_effort/context/thinking (M47-C7)`.
   - Patch branch: `patch/m47-c7-provider-config-defaults` (parent: `patch/m47-c6-session-preferences`).
   - Purpose: give a global SSOT fallback for the M47 5-dimension agent profile schema so a single shell session can drive Claude/GPT/Gemini/GLM personas from one config file without having to repeat the same `effort:` / `context:` / `thinking:` field on every profile. The OpenAI-only `openai_reasoning_effort` key remains the authoritative fallback for direct OpenAI sessions (back-compat); the new keys are the cross-provider SSOT fallback that also reaches OpenRouter DeepSeek/GLM/Kimi.
   - Schema additions (jcode-config-types `ProviderConfig`):
     - `default_reasoning_effort: Option<String>` — none/low/medium/high/xhigh.
     - `default_context: Option<String>` — e.g. `"200k"` or `"1m"`. Currently only Anthropic consumes via `set_context_preference`; others silently skip.
     - `default_thinking: Option<bool>` — Anthropic / Gemini / OpenRouter Kimi+GLM consume; OpenAI direct ignores.
     - All default to `None` so existing installs stay unchanged.
   - Env overrides (src/config/env_overrides.rs):
     - `JCODE_DEFAULT_REASONING_EFFORT` → `default_reasoning_effort`
     - `JCODE_DEFAULT_CONTEXT` → `default_context`
     - `JCODE_DEFAULT_THINKING` → `default_thinking` (accepts 1/true/yes/on/enabled vs 0/false/no/off/disabled)
   - Generated default config (src/config/default_file.rs) gets commented-out sample entries that document the keys + env overrides so `jcode init` produces a config file that explains the SSOT fallback feature for new users.
   - Spawn path (src/tool/task.rs::execute): after consuming the M47-C5 variant resolver hints and the explicit agent profile fields, falls back to the new ProviderConfig defaults before creating the child session. Resolution order is `profile.explicit > variant resolver > ProviderConfig.default_*`. The effort fallback also runs through `should_apply_route_effort` (M47-C2) so it only persists on backends that consume effort.
   - Touched paths:
     - `crates/jcode-config-types/src/lib.rs`
     - `src/config/default_file.rs`
     - `src/config/env_overrides.rs`
     - `src/config_tests.rs` (2 new regression tests)
     - `src/tool/task.rs` (spawn path fallback chain)
   - Validation: 2 new tests pass — `m47_c7_provider_agnostic_defaults_default_to_none`, `m47_c7_generated_default_config_documents_provider_agnostic_keys`. 54 `config::tests::*` (now 56 with the new ones) and 21 `tool::task::tests::*` still pass.
   - Binary reinstall required: yes (config schema + spawn-path fallback).

40. Doctor renders effective 5-dimension summary per agent profile (M47-C8)
   - Commit: `fa1acf52` `doctor: render effective 5-dimension summary per agent profile (M47-C8)`.
   - Patch branch: `patch/m47-c8-doctor-effective-dimensions` (parent: `patch/m47-c7-provider-config-defaults`).
   - Purpose: after M47-C3 added `context`/`thinking` to `AgentRouteConfig` and M47-C5 wired them through the variant resolver, agent profiles now carry up to five provider-aware dimensions. `jcode doctor` previously printed only origin and warned when both model and prompt were missing, which left users guessing about routing.
   - Implementation:
     - `src/doctor.rs::section_agent_profiles`: after the conflict/empty warnings, append an info-level line per profile of the form `"<name>" dimensions  model=… · variant=… · effort=… · context=… · thinking=on|off`. Quiet mode hides it.
     - New helper `effective_profile_dimensions(profile)` renders each dimension the `AgentRouteConfig` actually carries. The rendering intentionally reports the file-level surface area (post-merge winning profile) rather than the M47-C5 variant-resolved end state — so users can read both alongside.
     - Empty profiles still skip the line silently to avoid noise.
   - Touched paths:
     - `src/doctor.rs`
     - `src/doctor_tests.rs` (1 new regression test)
   - Validation: 11 `doctor_tests::*` pass including new `test_doctor_renders_effective_profile_dimensions` which checks a Claude prometheus profile (model + variant + context + thinking) and a GPT coder profile (model + effort).
   - Binary reinstall required: yes (doctor output change).

41. project_init ships 4 sample agent profiles covering the 4 providers (M47-C9)
   - Commit: `6784854a` `project_init: ship 4 sample agent profiles covering the 4 providers (M47-C9)`.
   - Patch branch: `patch/m47-c9-sample-agent-md` (parent: `patch/m47-c8-doctor-effective-dimensions`).
   - Purpose: after M47-C0..C-8 wired the 5-dimension agent profile schema end-to-end (deep merge, silent skip, provider trait surface, variant resolver, session round-trip, provider-agnostic defaults, doctor visibility), fresh `jcode init` projects had no concrete example in `.jcode/agents/`. M47-C9 ships 4 sample profiles — one per supported provider — so a newly initialized project demonstrates the schema in real frontmatter.
   - Files added by `init_project`:
     - `.jcode/agents/claude-strategist.md` — `model = claude-opus-4-7` + `variant = max` (Anthropic routes to `context = "1m"` via M47-C5 variant resolver).
     - `.jcode/agents/gpt-coder.md` — `model = gpt-5.5` + `effort = medium` (OpenAI direct reasoning_effort).
     - `.jcode/agents/gemini-visual.md` — `model = gemini-3.1-pro-preview` + `thinking = true` (Gemini thinking_budget surface).
     - `.jcode/agents/glm-worker.md` — `model = zhipu/glm-4-6` + `variant = max` (OpenRouter routes to `effort = xhigh` + `thinking = true`).
   - Each sample doubles as documentation: the markdown body explains when to use the persona, how delegation flows, and which provider channel the variant alias routes to.
   - Touched paths:
     - `src/project_init.rs` (4 const string templates + 4 `write_generated_file` calls + 1 new regression test)
   - Validation: 4 `project_init::tests::*` pass including the new `m47_c9_sample_agents_parse_with_expected_dimensions` which parses each shipped sample back into `AgentRouteConfig` and asserts the dimensions match the M47 plan — catches drift between shipped templates and the parser/resolver.
   - Binary reinstall required: yes (project init writes new files).

## M47 milestone summary

The 10-stage M47 patch series (`patch/m47-c0-deep-merge-profiles` through `patch/m47-c9-sample-agent-md`) is complete. The lazy-harness 4-provider SSOT goal is unblocked: a single `~/.jcode/agents/<persona>.md` profile can carry `model` + `variant` + optional `effort` / `context` / `thinking` and the spawn-path will route the right combination per backend (Claude → 1M context, OpenAI → reasoning effort, Gemini → thinking budget, OpenRouter → effort + thinking). See `docs/lazydino/milestones/M47.md` for the dependency graph, validation matrix, and behavior change notes per stage.

42. Compaction baseline fixtures and per-message token trace (M48-C0)
   - Commit: `6aaa589e` `compaction: baseline fixtures and per-message token trace (M48-C0)`.
   - Patch branch: `patch/m48-c0-compaction-fixtures` (parent: `deploy/m9-m27-catchup`, kicks off M48).
   - Purpose: every later M48 stage (select, prune, anchored summary, replay-on-overflow, OpenAI coexistence) needs a stable input surface to diff behavior against. M48-C0 adds 5 deterministic fixtures plus a per-message token trace so subsequent stages can land focused PRs without re-inventing the test scaffolding each time.
   - Implementation:
     - New `jcode-compaction-core::m48_fixtures` module: `short_session`, `long_text_only_session` (20 turns, > 5k tokens), `tool_heavy_session` (4_000 char tool_result per turn), `image_session`, `openai_native_compacted_session`. Each fixture uses a fixed-timestamp builder so JSON round-trip remains reproducible.
     - New `jcode-compaction-core::m48_trace` module: `block_tokens`, `message_tokens`, `total_tokens`, and `trace_messages(...) -> Vec<MessageTrace>` carrying per-block `kind` + `tokens` for human-readable test failure output. Uses the existing `CHARS_PER_TOKEN` constant so numbers match the rest of the crate.
     - Added `chrono = "0.4"` (default-features=false, `clock` feature only) to `jcode-compaction-core/Cargo.toml` for fixture timestamps.
   - Touched paths:
     - `crates/jcode-compaction-core/Cargo.toml`
     - `crates/jcode-compaction-core/src/lib.rs` (+405 lines, two new `pub mod`s)
     - `Cargo.lock`
   - Validation: 19 `jcode-compaction-core` lib tests pass (9 pre-existing + 10 new self-tests). `cargo check -p jcode` clean.
   - Binary reinstall required: no (test-only modules; production binary is unchanged).

## Upstream PR triage notes

Last reviewed: 2026-05-10.

Use these notes when deciding whether to adopt upstream PR ideas into `custom/lazydino-harness`. The primary question is not "can we cherry-pick this exact diff?" but:

```text
What problem is this PR trying to solve?
Does that problem matter for Lazydino's harness?
If yes, should we cherry-pick, adapt, or reimplement the idea in our own architecture?
```

Decision policy:

- Prefer purpose-first adoption: problem -> desired behavior -> local design -> implementation.
- Cherry-pick only when the diff is small, isolated, current, and fits our custom stack.
- Reimplement when the PR direction is good but the code is stale, too broad, conflicting, or not aligned with our `.jcode`/hook/agent-profile architecture.
- Record every adopted behavior as its own `patch/*` branch so it can be replayed after upstream updates.

### Purpose-based roadmap

1. Skill and harness ergonomics
   - Related PRs: `#166`, `#162`, `#151`, `#113`.
   - Goal: make skills discoverable, callable, project-aware, and useful for private harness engineering.
   - Our direction:
     - Fix `skill_manage` / public `Skill` parameter confusion.
     - Ensure project skill dirs are loaded reliably from active working directory.
     - Add support for common ecosystem directories such as `.jcode/skills`, `.claude/skills`, `.agents/skills`, and `.opencode/skills` where appropriate.
     - Use selected skill content ideas like `verification-loop`, `search-first`, and `promptify` as private/local skills rather than source-tree baggage.
     - Treat `#151` as a design reference for embedded skills and deterministic routing, not as a merge candidate.

2. Agent reliability and provider hot-reload
   - Related PRs: `#75`, `#69`, `#139`, `#148`, `#95`, `#94`.
   - Goal: reduce broken sessions after login/model changes and make provider/model routing less fragile.
   - Our direction:
     - Adopt lazy provider/auth reinitialization if restored sessions or `/model` switching fail after login.
     - Keep canonical Claude model IDs accurate, especially Haiku and Opus Max routes.
     - Avoid sending provider-specific unsupported tools, e.g. OpenAI image generation to Codex models.
     - Defer custom provider/gateway support until the user actually needs that gateway.

3. Usage, cost, and failover correctness
   - Related PRs: `#178`, `#101` cheap-mode side note.
   - Goal: make quota/cost displays and failover decisions trustworthy.
   - Our direction:
     - Adopt the OpenAI usage-percent normalization idea because bad quota display can trigger wrong model/failover decisions.
     - Do not blindly add cheap-mode defaults because this user's harness intentionally uses premium Opus/GPT routing. If we add budget modes, make them explicit opt-in profiles, not global surprises.

4. Ambient and scheduled background work
   - Related PRs: `#173`, `#116`.
   - Goal: keep ambient cycles, schedule tools, and memory consolidation from failing on provider argument shape differences.
   - Our direction:
     - Already adopted string-or-number numeric deserialization as `patch/ambient-serde-args`.
     - Keep ambient conservative because it runs without a live user prompt.

5. Terminal and tmux workflow
   - Related PRs: `#78`, `#68`, `#101`, `#55` keyboard portion.
   - Goal: make jcode feel native inside the user's tmux/terminal workflow.
   - Our direction:
     - Already fixed tmux Ctrl+h/j/k/l passthrough in user tmux config.
     - Consider native tmux new-window spawning only if the current workflow needs spawning/resume panes.
     - Consider OSC52 clipboard fallback for SSH/tmux environments.
     - Consider recursive stdin detection if wrapper processes hang or stdin prompts are missed.

6. Safety and containment
   - Related PRs: `#138`, `#137`.
   - Goal: reduce accidental or unsafe file operations.
   - Our direction:
     - Treat sandboxing as a serious separate project, not a casual cherry-pick.
     - If adopted, reimplement carefully with tests for every file-touching tool and explicitly document that bash requires OS-level sandboxing for hard guarantees.
     - Continue using lifecycle hooks as the fast safety layer for bash/tool policy.

7. Native search/tool expansion
   - Related PRs: `#90`.
   - Goal: improve search/research capability.
   - Our direction:
     - Defer native Exa because this harness already has MCP Exa/websearch integrations.
     - Add native tools only when they reduce setup friction or improve reliability over MCP.

### Adopt / reimplement soon

- `#178` Fix OpenAI usage percent normalization for low values
  - Status: adapted locally as `patch/openai-usage-percent-normalization`.
  - Benefit: fixes `/usage` and info-widget bars that show 1% weekly usage as 100% exhausted.
  - Suggested action: keep covered by `cargo test usage::tests --lib` after rebases.
- `#173` Fix ambient serde bug
  - Status: already adapted locally as `patch/ambient-serde-args`.
  - Benefit: prevents ambient tool deserialization failure when numbers arrive as strings.
- `#166` Accept `skill` alias in skill tool
  - Status: adapted locally in `patch/project-skill-sync`.
  - Benefit: makes external/public Skill-style calls compatible with internal `skill_manage`.
  - Suggested action: keep covered by `cargo test skill --lib` after rebases.
- `#162` Skill alias plus Gemini schema sanitization
  - Status: useful, medium size.
  - Benefit: fixes skill tool confusion and Gemini failures on MCP tool schemas containing `$defs`, `$ref`, `$schema`.
  - Suggested action: skill alias/project-scope portion is adapted locally in `patch/project-skill-sync`; Gemini schema sanitizer remains a possible separate patch.
- `#139` Correct Claude Haiku 4.5 dated model id
  - Status: tiny.
  - Benefit: aligns with our current dated `claude-haiku-4-5-20251001` policy.
  - Suggested action: apply if upstream has not already fixed sidecar fallback.
- `#148` Disable OpenAI image generation tool for Codex models
  - Status: small.
  - Benefit: avoids sending unsupported native image generation to Codex-family models.
  - Suggested action: apply if GPT/Codex payload errors appear.
- `#68` OSC52 clipboard fallback
  - Status: useful for SSH/tmux/remote terminal work.
  - Benefit: copy-to-clipboard works even without Wayland/X11 clipboard tools.
  - Suggested action: apply after testing in the user's terminal setup.
- `#75` Lazy auth init on restored sessions
  - Status: medium, practical.
  - Benefit: prevents repeated login prompts after credentials were written but provider was initialized earlier.
  - Suggested action: reimplement small helper if the login/restore issue appears.
- `#69` Lazy OpenAI provider hot-init
  - Status: small but overlaps provider init code.
  - Benefit: `/model gpt-*` can recover after OpenAI login from another shell.
  - Suggested action: apply with `#75` as a provider hot-init bundle.
- `#101` Recursive Linux stdin detection
  - Status: useful for nested wrappers.
  - Benefit: better detects child/grandchild processes waiting for stdin.
  - Suggested action: apply if stdin prompts or wrapper processes misbehave.

### Consider later / partial extraction only

- `#151` jcode-harness embedded skills and LLM wiki memory loop
  - Status: very large, conflicting, fork/product-direction branch.
  - Useful ideas: embedded skills, deterministic skill router, skill doctor/import CLI, project init scaffolding, interview/wizard onboarding, wiki-memory safety prompts.
  - Suggested action: do not merge wholesale. Extract only small ideas after our local `.jcode` and skill-sync patches stabilize.
  - Interview mode direction: if adopted, make it explicit as `jcode init --interview` / `jcode init --wizard`, not the default chat behavior. It should ask project/harness questions, then generate durable `.jcode` config, prompt overlays, hooks, skills, and validation notes.
  - Detailed slice map from the 100-commit PR:
    - Already covered locally or mostly covered:
      - `096 Fix Skill tool alias for Anthropic OAuth` and `098 Fix slash skill invocation with context` -> covered by `patch/project-skill-sync`.
      - `048 Add project skill scope policy` -> covered by `.jcode/.claude/.agents/.opencode` project skill loading in `patch/project-skill-sync`.
      - `005 Add project init bootstrap`, `029 Queue swarm analysis from init`, and `030 Add jcode init swarm analysis` -> partially covered by native `jcode init`; swarm-analysis bootstrap remains optional future work.
    - High-value slices to reimplement next if useful:
      - `003 Start interactive mode by default in harness` -> rework as explicit `jcode init --interview` / `--wizard`, not default chat mode.
      - `002 Improve harness run and skills doctor`, `043 Add offline harness doctor diagnostics`, `045 Add offline skill validation gate`, and `046 Add safe skill import planner` -> good direction for `jcode doctor` / `jcode skills doctor`.
      - `014 Add CLI quality preflight gate`, `027 Add release gates and clean-code fixtures`, `060 Add opt-in live provider smoke`, and `061 Add CI-friendly harness smoke e2e` -> good direction for a local `jcode doctor --release` validation bundle.
      - `025 Add JSON output for skills commands`, `072-077 offline session JSON/NDJSON envelopes` -> useful if we need stable automation contracts for external harness tooling.
      - `089-095 swarm await/scope/health/run-id/cleanup/retry` -> potentially valuable for reliable multi-agent orchestration; inspect separately before changing current swarm behavior.
      - `097 Add hard timeout for reload handoff` -> adapted locally as `patch/reload-handoff-hard-timeout`.
      - `059 Fix selfdev reload repo discovery` -> relevant to our install/reload workflow; inspect before implementing if reload discovery misbehaves.
      - `082/085 user attention bell/background completion alerts` -> useful UX for long background builds/tests.
    - Medium-value or conditional slices:
      - `032 Add llmwiki memory skill`, `041 Add LLM wiki bridge preview`, `058 Document llmwiki bridge schema commands` -> adopt only as opt-in local memory/provenance skill. Do not make wiki memory a source-of-truth or sync secrets.
      - `068-071 offline demo runner/manifest/sandboxed demo` -> useful for reproducible harness demos, lower priority than doctor/init.
      - `078-080 ACP preview` and `086 ACP cancellation` -> interesting external protocol surface, defer unless ACP integration is needed.
      - `039 Support OpenRouter reasoning effort`, `040 Set OpenAI reasoning effort to max by default`, `100 Allow local Ollama HTTP endpoints` -> provider-specific; adopt only for an actual route/user need.
      - `012/013/015/038/055/056 security/error/OAuth hardening` -> inspect as separate small patches, but avoid importing broad churn blindly.
    - Skip or avoid wholesale:
      - `033 Rewrite README for harness fork`, `034 Add README engineering loop graphic`, `057 Polish JCode Harness branding` -> fork branding, not needed for Lazydino custom stack.
      - `.codex-harness/*`, `.context/*`, bulk generated contracts/gates/decisions -> useful as artifact pattern, but should not be copied into the jcode source tree.
      - `047/049-054 CI/security dependency churn` -> only adopt if upstream/base actually has that vulnerability or CI issue.
- `#138` Filesystem sandboxing with `--sandbox` / `JCODE_SANDBOX_ROOT`
  - Status: valuable but touches many file tools and has partial security boundary for bash.
  - Suggested action: consider later as a focused safety project. Must audit every file-touching tool and document bash limitations clearly.
- `#113` MAS-inspired project skills
  - Status: skill content pack, draft.
  - Useful ideas: `verification-loop`, `search-first`, `promptify` skills.
  - Suggested action: copy/adapt selected skills into private `~/.jcode/skills` or project `.jcode/skills`, not into upstream source.
- `#90` Exa search tool
  - Status: useful but we already have MCP Exa/websearch tools in this harness.
  - Suggested action: skip unless native built-in Exa without MCP becomes necessary.
- `#94` Anthropic-compatible provider and custom headers
  - Status: useful for custom gateways, nontrivial provider surface.
  - Suggested action: defer until a real Anthropic-compatible gateway is needed.
- `#95` Align OpenAI base URL with Codex config
  - Status: useful for Codex-compatible gateways, touches provider/session/auth.
  - Suggested action: defer unless the user needs Codex config gateway reuse.
- `#78` tmux pane spawning support
  - Status: directly relevant to tmux workflow but conflicts.
  - Suggested action: consider after current tmux key passthrough stabilizes. Reimplement small `tmux new-window` support if needed.
- `#55` System prompt config plus VSCode keyboard fix
  - Status: small but overlaps our private `.jcode` prompt-overlay approach.
  - Suggested action: do not add broad `provider.system_prompt` unless a hard override is required. The VSCode keyboard portion can be extracted separately if needed.

### Low priority / already upstream / not relevant now

- `#169`, `#168`, `#126`, `#56`: already merged upstream.
- `#172`: Windows docs only.
- `#150`, `#134`: MiniMax endpoint, not currently used.
- `#149`: Firefox browser host packaging, only relevant if browser setup fails.
- `#137`: broad audit cleanup, inspect only if security work starts.
- `#125`, `#123`: docs for compiler allows, low value.
- `#124`: deprecated flag cleanup, avoid until upstream direction is clear.
- `#103`, `#102`, `#92`, `#91`, `#83`, `#77`, `#72`, `#70`, `#47`, `#58`: situational provider/platform/release improvements. Revisit only if the matching issue appears.



## Project-local harness initialization

Preferred native command:

```bash
jcode init [target]
```

A Claude/Jcode skill also exists as an instruction wrapper, but it is not the execution surface. Unlike opencode-style command skills, Jcode/Claude skills activate instructions; they do not automatically mutate the filesystem. Use the native command for actual onboarding.

Skill location:

```text
/home/lazydino/.claude/skills/jcode-init/SKILL.md
/home/lazydino/.claude/skills/jcode-init/scripts/init-jcode-project.sh
```

Purpose:

- Create a private project-local `.jcode/` harness without editing shared team `AGENTS.md`.
- Generate project-local config, prompt files, routing policy notes, and basic tool hooks.
- Keep `.jcode/` private by adding it to `.git/info/exclude` by default.
- Support an explicit override mode for projects where the personal harness should ignore team instructions.

Generated files:

```text
.jcode/config.toml
.jcode/AGENTS.md
.jcode/harness/10-routing-policy.md
.jcode/harness/20-project-rules.md
.jcode/hooks/check-bash.sh
.jcode/hooks/log-tool.sh
```

Default command:

```bash
jcode init <target-project>
```

Legacy helper script, retained for compatibility:

```bash
~/.claude/skills/jcode-init/scripts/init-jcode-project.sh <target-project>
```

Useful options:

```bash
--force               overwrite existing generated files
--gitignore           add .jcode/ to project .gitignore instead of .git/info/exclude
--ignore-team-agents  set [prompt].ignore_project_agents = true
```

Default policy:

- Team/project `AGENTS.md` stays enabled.
- Private `.jcode/AGENTS.md` and `.jcode/harness/*.md` load after team/global instructions.
- `.jcode/` is ignored through `.git/info/exclude` so it remains local and private.

Validation performed:

```bash
# temp git project
jcode init <tmp-project>
python3 -c 'import tomllib, pathlib; tomllib.loads(pathlib.Path("<tmp-project>/.jcode/config.toml").read_text())'
printf '{"input":{"command":"rm -rf /"}}' | <tmp-project>/.jcode/hooks/check-bash.sh

git -C <tmp-project> check-ignore -v .jcode/config.toml
```

Expected hook behavior:

- safe bash commands return `{"action":"allow"}`.
- dangerous destructive patterns such as `rm -rf /` return `{"action":"deny", ...}`.

## Personal environment fixes

### tmux Ctrl+h/j/k/l passthrough for jcode

Problem:

```text
jcode runs inside tmux, but Ctrl+h/j/k/l are intercepted by tmux pane navigation instead of reaching jcode.
```

Root cause:

- The user's tmux setup uses `christoomey/vim-tmux-navigator`.
- That plugin binds `Ctrl+h/j/k/l` globally and only passes the keys through when the active pane command matches its vim/fzf regex.
- `jcode` was not in that regex, so tmux treated jcode like a normal shell pane and moved panes instead of sending the key to jcode.

Actual config location:

```text
/home/lazydino/.config/tmux/tmux.conf
```

Applied backup:

```text
/home/lazydino/.config/tmux/tmux.conf.bak-20260510T000423Z
```

Important correction:

- `/home/lazydino/.tmux.conf` was accidentally edited first, then immediately restored from `/home/lazydino/.tmux.conf.bak-20260510T000333Z`.
- The real active configuration is the XDG config file above.

Applied block, placed after TPM/plugin initialization so it overrides `vim-tmux-navigator` bindings:

```tmux
# LazyDino/Jcode: override vim-tmux-navigator to pass Ctrl+h/j/k/l into jcode.
# Must appear after TPM/plugin initialization because vim-tmux-navigator binds these keys too.
is_vim_fzf_or_jcode="ps -o state= -o comm= -t '#{pane_tty}' | grep -iqE '^[^TXZ ]+ +(\\S+/)?(jcode|g?\\.?(view|l?n?vim?x?|fzf)(diff)?(-wrapped)?)$'"
bind-key -n C-h if-shell "$is_vim_fzf_or_jcode" "send-keys C-h" "select-pane -L"
bind-key -n C-j if-shell "$is_vim_fzf_or_jcode" "send-keys C-j" "select-pane -D"
bind-key -n C-k if-shell "$is_vim_fzf_or_jcode" "send-keys C-k" "select-pane -U"
bind-key -n C-l if-shell "$is_vim_fzf_or_jcode" "send-keys C-l" "select-pane -R"
bind-key -n C-\\ if-shell "$is_vim_fzf_or_jcode" "send-keys C-\\" "select-pane -l"
```

Reload command:

```bash
tmux source-file ~/.config/tmux/tmux.conf
```

Validation command:

```bash
tmux list-keys -T root | grep -E 'C-(h|j|k|l|\\)'
```

Expected behavior:

- In `jcode`, vim/nvim, or fzf panes: `Ctrl+h/j/k/l` is passed through to the application.
- In normal shell panes: `Ctrl+h/j/k/l` still performs tmux pane navigation.

## Hook design note

The recommended hook strategy is:

```text
MVP: Claude Code-style tool boundary hooks
Long-term naming/extension: opencode-style event bus
jcode-specific future events: memory, swarm, background task, session, todo
```

MVP events:

```text
tool.execute.before  # PreToolUse
tool.execute.after   # PostToolUse
```

Current hook config example:

```toml
[hooks]
enabled = true

[[hooks.commands]]
event = "tool.execute.before"
tool = "bash"
command = "~/.jcode/hooks/check-bash.sh"
blocking = true
timeout_ms = 3000

[[hooks.commands]]
event = "tool.execute.after"
tool = "*"
command = "~/.jcode/hooks/log-tool.sh"
blocking = false
timeout_ms = 3000
```

Blocking hook stdout protocol:

```json
{"action":"allow"}
```

or:

```json
{"action":"deny","reason":"Dangerous command blocked"}
```

Empty stdout defaults to allow. `modify` is intentionally not implemented yet.

Recommended project-local config design:

```text
~/.jcode/config.toml           # global defaults
<project>/.jcode/config.toml   # project overrides, checked into repo if safe
<project>/.jcode/config.local.toml # private local overrides, gitignored
```

Merge policy should be:

```text
global hooks + project hooks + local hooks
```

Project config should be loaded from the active session working directory. This mirrors the useful parts of opencode's project-level `.opencode/opencode.json` and Claude Code's project/local settings split, while keeping jcode's TOML convention.

Future events can include:

```text
file.edited
session.created
session.idle
session.error
todo.updated
shell.env
memory.write.before
memory.write.after
swarm.task.started
swarm.task.completed
background.task.completed
model.request.before
model.response.after
```

## Known upstream test failures

These tests fail on `origin/master` (verified at upstream commit `4b97d322`) and on every Lazydino patch since `d2c9b046`. They are NOT regressions introduced by Lazydino patches. Last verified: 2026-05-10.

Verification recipe:
```bash
git checkout origin/master
/tmp/check-12-tests.sh   # or run the individual cargo test invocations below
```

Failure inventory (12 tests):

Environment-dependent (likely fine on a fresh CI machine, fail on this developer host):
- `auth::tests::cursor_status_is_available_for_authenticated_cli_session` — expects Cursor CLI to be authenticated locally.
- `server::comm_session::comm_session_tests::resolve_spawn_working_dir_prefers_explicit_then_spawner_agent_dir` — assertion compares `/tmp/spawner-agent` against current working dir resolution.
- `ambient::runner::runner_tests::spawn_target_creates_one_child_session_and_runs_task` — assertion against tempdir path resolution.

Stale expectation drift (single-line fixes if/when we adopt them):
- `tui::ui::pinned_ui::tests::side_panel_mermaid_probe_reports_viewport_fill_for_underutilized_fit` — expects `127%`, current code produces `130%`.
- `tui::app::helpers::helpers_tests::build_resume_command_uses_imported_jcode_session_for_codex` — expects no `--fresh-spawn` flag, current code adds it.

Suspected real upstream bugs (do not adopt blindly; investigate before fixing):
- `tui::app::tests::test_context_command_reports_session_context_snapshot` — `/context` summary text changed.
- `tui::app::tests::test_local_compacted_history_marker_scroll_expands_from_session` — compaction marker scroll-expand returns 0 elements where 2 are expected.
- `tui::app::tests::test_git_command_works_in_remote_mode_with_accessible_working_directory` — remote git command returns no response.
- `tui::app::tests::remote_add_provider_message_retains_remote_provider_copy` — remote provider message ends up duplicated.
- `server::client_session::tests::resume_tests::handle_resume_session_allows_live_attach_when_existing_agent_is_busy` — live attach to busy agent returns 0 instead of 1.
- `tui::app::helpers::helpers_tests::gather_ambient_info_filters_to_session_reminders_when_ambient_disabled` — panics with "ambient info" message.
- `agent::tests::env_snapshot_detail_is_minimal_for_empty_sessions_and_full_after_history` — expects `Minimal`, returns `Full`.
- `tool::mcp::tests::test_tool_description` — assertion expects `"Model Context Protocol"` substring; current code returns `"Manage MCP servers."` (verified failing on baseline `61028b2d` 2026-05-10).

Policy:
- Do NOT block Lazydino patches on these.
- When adding a new patch, run a focused subset (`cargo test <relevant-area> --lib`) and ignore matches against this list.
- If a patch you are working on actually starts passing one of these (genuine fix), promote it: remove from this list, add a `patch/upstream-test-fix-<area>` branch, and consider an upstream PR.
- Re-verify this list after every upstream rebase: any test that newly fails AND is not in this list IS a regression caused by the rebase or one of our patches and must be investigated.

Bisection summary (verified 2026-05-10):
- `origin/master` (`4b97d322`): 0/12 pass.
- `d2c9b046` (Lazydino baseline before recent patches): 0/12 pass.
- After `patch/journal-on-message`, `patch/safe-server-restart`, `patch/reload-handoff-hard-timeout`, `patch/mermaid-input-non-blocking`: 0/12 pass.
- Net regression introduced by Lazydino patches against this list: 0.

## Security note

A GitHub token was previously visible in a local URL rewrite. The local rewrite was removed, but the token should be revoked/rotated in GitHub because it may have been exposed in logs/chat.

Check remotes should look like this, without embedded credentials:

```bash
git remote -v
```

Expected:

```text
fork   https://github.com/lazy-dinosaur/jcode.git (fetch)
fork   https://github.com/lazy-dinosaur/jcode.git (push)
origin https://github.com/1jehuang/jcode.git (fetch)
origin https://github.com/1jehuang/jcode.git (push)
```

## Upstream sync state (2026-05-11, after M21 DONE)

- Correct remote mapping is **`origin=https://github.com/1jehuang/jcode.git`**
  and **`fork=https://github.com/lazy-dinosaur/jcode.git`**. Do **not** add/use
  `upstream=https://github.com/sst/jcode.git`; that remote is invalid for this
  repo and makes `git fetch --all` fail.
- ✅ **`fork/master` 동기화 완료**: 2026-05-11 `git push fork origin/master:master`
  로 226 commit fast-forward. 현재 `fork/master == origin/master`
  (`git rev-list --count fork/master..origin/master` → `0`).
- ✅ **deploy/m9-m10 + 48 patch branch 모두 fork 에 push 완료** (2026-05-11).
  dedupe rebase 는 불필요했음 — 우리 patch 들이 이미 origin/master 기준
  hash 로 깔끔히 얹혀있어서 path mismatch 충돌 없음.

## `.env` file (local, gitignored)

`/home/lazydino/dev/jcode/.env` 에 GitHub PAT 보관 (mode 0600,
`.gitignore` 의 `/.env` 패턴으로 보호). 누락 시 `~/dev/medivance/.env.local`
의 `GH_TOKEN` 에서 복사:

```bash
TOKEN=$(grep "^GH_TOKEN=" ~/dev/medivance/.env.local | head -1 | cut -d= -f2-)
cat > .env <<EOF
GH_TOKEN=$TOKEN
GH_USERNAME=lazy-dinosaur
EOF
chmod 600 .env
```

스크립트 `scripts/fork-push.sh` 가 이걸 읽어서 `GIT_ASKPASS` 로 push 함.
토큰 scope 검증:
```bash
curl -sI -H "Authorization: token $GH_TOKEN" https://api.github.com/user \
  | grep -i "x-oauth-scopes"
# 기대: repo, workflow
```

## fork push 절차 (일상)

```bash
# 코드 patch + deploy
./scripts/fork-push.sh deploy/m9-m10 patch/<name>

# fork/master 를 upstream 으로 동기화 (가끔)
./scripts/fork-push.sh master

# 모든 default ref (deploy + 3 main code patches: sdk-history-images,
# config-hot-reload, bash-tool-timeout)
./scripts/fork-push.sh
```

## Backup tag convention

위험한 작업 (rebase, force-push, mass branch reset) 전에 **항상**
보호 tag 만들기:

```bash
TS=$(date +%Y%m%d-%H%M)
PREFIX="backup/<reason>-${TS}"
git tag -a "${PREFIX}/deploy-m9-m10" deploy/m9-m10 -m "backup before <op>"
for b in $(git for-each-ref --format='%(refname:short)' refs/heads/ \
            | grep -E "^patch/"); do
  git tag -a "${PREFIX}/${b#patch/}" "$b" -m "backup before <op>"
done
```

복원:

```bash
git checkout -B <branch> "${PREFIX}/<branch-suffix>"
```

**주의**: 이전 시도에서 `backup/pre-upstream-rebase-20260511-0136`
파일-tag 와 `backup/pre-upstream-rebase-20260511-0136/<patch>`
디렉터리-tag 가 충돌해서 디렉터리 tag 들이 만들어지지 않은 적 있음
(silent fail). 한 prefix 안에서는 prefix 자체에 대한 file-tag 를
만들지 말 것.
