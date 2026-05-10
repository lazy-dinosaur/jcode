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
