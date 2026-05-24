# Upstream Catchup Execution Plan — 2026-05-24

This plan is for catching `deploy/m9-m27-catchup` up to `upstream/master` without losing the Lazydino custom stack.

Companion guardrail: `docs/lazydino/CUSTOM_STACK_INVENTORY.md`.

## Current divergence

- Base: `50d2c68b`
- Current deploy branch tip at planning time: `efde7d65`
- Upstream tip at planning time: `2913e25c`
- Lazydino-only commits: 313 after inventory doc, 312 before it
- Upstream-only commits: 290
- Overlapping changed files since base: 136
- Patch-id equivalent upstream commits: 0

## Non-negotiable rule

Do not run a blind merge or a long cherry-pick chain onto `deploy/m9-m27-catchup`. This branch is a custom distribution. Use 3-way intent review for every batch:

1. Upstream intent
2. Lazydino/custom intent
3. Decision: `KEEP_OURS`, `PORT_UPSTREAM`, `CHERRY_PICK_OK`, `SKIP_UPSTREAM`, or `NEEDS_REVIEW`

## Branching strategy

Use integration branches, not the deploy branch directly:

```bash
git checkout deploy/m9-m27-catchup
git pull --ff-only origin deploy/m9-m27-catchup
git fetch upstream master
git switch -c sync/upstream-2026-05-24
```

Each completed stage/batch gets its own commit and push. Merge back only after validation.

## Recommended execution order

### Stage 0 — guardrails only, already done

- Create and commit custom stack inventory.
- Create this execution plan.
- No upstream code applied yet.

### Stage 1 — low-overlap desktop tail first

Apply B14 and B15 first on the integration branch because they have zero overlap with Lazydino changed files and are mostly new desktop worker-host code.

Why first:

- Reduces upstream tail risk without touching custom TUI/provider/hook stack.
- Proves the branch can absorb upstream Cargo/desktop additions separately.
- Keeps failures isolated from hook/provider/interrupt areas.

Expected decisions:

- B14: mostly `CHERRY_PICK_OK`, but verify Cargo feature/dependency impacts.
- B15: mostly `CHERRY_PICK_OK`, desktop smoke scripts only.

Validation after Stage 1:

```bash
cargo fmt --check
cargo check -q -p jcode
cargo check -q -p jcode-desktop
```

### Stage 2 — high-risk upstream core in smaller sub-batches

B01-B13 are all high-risk by overlap. Do not apply each 20-commit batch as a single blind cherry-pick. For each Bxx:

1. Split into 5-10 commit sub-batches by theme.
2. For each sub-batch, classify every commit.
3. Cherry-pick only `CHERRY_PICK_OK`.
4. Manually port `PORT_UPSTREAM` commits.
5. Record `KEEP_OURS` and `SKIP_UPSTREAM` in a batch note.

Sub-batch commit message pattern:

```text
Catch up upstream B03 provider picker slice

Upstream range: <sha>..<sha>
Decisions: CHERRY_PICK_OK=3 PORT_UPSTREAM=2 KEEP_OURS=1 SKIP_UPSTREAM=0
Validation: cargo check -p jcode; cargo test ...
```

### Stage 3 — full validation and selfdev reload

After all selected upstream changes are applied:

```bash
cargo fmt --check
cargo check -q -p jcode
cargo test -q -p jcode-provider-gemini
cargo test -q -p jcode antigravity
cargo test -q -p jcode client_lifecycle
cargo test -q -p jcode mcp
cargo test -q -p jcode comm_control
cargo test -q -p jcode compaction
```

Then:

```bash
# preferred
selfdev build --target tui
selfdev reload
```

## Batch risk table

| Batch | Commits | Range | Risk | Files | Overlap | Protected areas | Span |
|---|---:|---|---|---:|---:|---|---|
| B01 | 20 | `ba7d37c2..95db8aee` | HIGH | 45 | 29 | hooks_lifecycle(1), interrupt_turn(5), provider_oauth(5), tui_render_remote(18), browser_tool_args(2), desktop(5) | Stop reconnect loop on remote protocol errors → Repair browser setup when native host is missing |
| B02 | 20 | `e354a3d8..4407509a` | HIGH | 48 | 33 | hooks_lifecycle(3), background_ambient(1), swarm_comm(3), compaction_session(2), interrupt_turn(2), provider_oauth(4), tui_render_remote(6), browser_tool_args(1), desktop(4) | Add swarm spawn mode config default → Optimize desktop resize rendering |
| B03 | 20 | `c06d52e9..9a356287` | HIGH | 47 | 24 | hooks_lifecycle(2), background_ambient(1), compaction_session(3), interrupt_turn(6), provider_oauth(7), selfdev_reload(1), tui_render_remote(12), desktop(4) | Improve desktop transcript text colors → Show live direct provider catalogs in model picker |
| B04 | 20 | `879f95ad..4ef3607e` | HIGH | 45 | 21 | hooks_lifecycle(1), background_ambient(1), interrupt_turn(1), provider_oauth(7), mcp(2), selfdev_reload(1), tui_render_remote(6), desktop(6) | Add client picker debug snapshot → Split overnight prompts and tests |
| B05 | 20 | `56efd93a..5b52a370` | HIGH | 46 | 26 | hooks_lifecycle(3), background_ambient(1), interrupt_turn(3), provider_oauth(4), selfdev_reload(1), tui_render_remote(22), desktop(5) | Extract mermaid debug APIs → Fix inline diff expand badge feedback |
| B06 | 20 | `828a3712..95087d57` | HIGH | 26 | 13 | hooks_lifecycle(1), background_ambient(1), interrupt_turn(4), selfdev_reload(1), tui_render_remote(8), desktop(12) | Synchronize spinner-only TUI redraws → fix(desktop): render whitespace inline code pills |
| B07 | 20 | `f85c2d59..9e7c68f2` | HIGH | 48 | 22 | hooks_lifecycle(1), background_ambient(2), swarm_comm(2), compaction_session(1), interrupt_turn(4), provider_oauth(3), mcp(1), tui_render_remote(7), browser_tool_args(1), desktop(18) | Mark fallback model picker routes → Fix desktop fractional text scrolling |
| B08 | 20 | `3a39411c..c5857d31` | HIGH | 31 | 18 | interrupt_turn(2), provider_oauth(5), mcp(1), tui_render_remote(9), desktop(8) | Preserve desktop session during self-dev reload → Update slash suggestion chrome tests |
| B09 | 20 | `71f2f9dc..4d20df15` | HIGH | 42 | 11 | hooks_lifecycle(2), background_ambient(1), swarm_comm(1), interrupt_turn(1), provider_oauth(2), tui_render_remote(5), desktop(16) | Preserve desktop workspace state on hot reload → fix(desktop): stabilize streaming text fade |
| B10 | 20 | `55803f6b..32924cc6` | HIGH | 35 | 15 | hooks_lifecycle(4), private_skills_prompt(1), interrupt_turn(1), selfdev_reload(3), tui_render_remote(11), desktop(7) | feat: add commit slash command → feat(desktop): show filtered session counts |
| B11 | 20 | `7b5753c9..d69fb2a5` | HIGH | 29 | 16 | compaction_session(1), interrupt_turn(1), provider_oauth(2), selfdev_reload(1), tui_render_remote(15), browser_tool_args(1), desktop(3) | feat(desktop): expose accepted slash aliases → docs: define resume picker behavior |
| B12 | 20 | `c3f63b38..ec0d5062` | HIGH | 61 | 31 | compaction_session(2), interrupt_turn(5), selfdev_reload(1), tui_render_remote(23), desktop(18) | feat(desktop): promote workspace sessions in place → Fix compacted history visible window |
| B13 | 20 | `318f9ca2..d415c401` | HIGH | 71 | 48 | hooks_lifecycle(3), background_ambient(2), swarm_comm(2), interrupt_turn(8), provider_oauth(7), tui_render_remote(16), browser_tool_args(2), desktop(9) | Add configurable tool profiles → Fix desktop resume switcher text rendering |
| B14 | 20 | `11f77cbb..2dfbc092` | MEDIUM-DESKTOP | 16 | 0 | desktop(16) | feat(desktop): handle runtime server events → desktop: initialize app worker over IPC |
| B15 | 10 | `1fa42b70..2913e25c` | LOW | 4 | 0 | desktop(4) | desktop: emit initial worker scene over IPC → desktop: add reload window e2e smoke |

## Batch notes and recommended handling

### B01 — HIGH

- Range: `ba7d37c2..95db8aee`
- Commits: 20
- Unique files touched: 45
- Overlap with Lazydino files: 29
- Protected areas: hooks_lifecycle(1), interrupt_turn(5), provider_oauth(5), tui_render_remote(18), browser_tool_args(2), desktop(5)
- Top paths: src:46, crates:9, scripts:2, README.md:1, Cargo.lock:1, packaging:1
- First commit: Stop reconnect loop on remote protocol errors
- Last commit: Repair browser setup when native host is missing
- Handling: Remote protocol, prompt cache warnings, huge tool outputs, model/login/provider startup, browser repair. High overlap with provider, TUI, browser, interrupt. Start with browser repair comparison because Lazydino already has browser-host repair. Likely mix of KEEP_OURS and PORT_UPSTREAM.
- Overlap sample: `Cargo.lock`, `README.md`, `crates/jcode-provider-metadata/src/lib.rs`, `src/agent.rs`, `src/agent/tools.rs`, `src/agent/turn_loops.rs`, `src/agent/turn_streaming_broadcast.rs`, `src/agent/turn_streaming_mpsc.rs`, `src/browser.rs`, `src/browser_tests.rs`

### B02 — HIGH

- Range: `e354a3d8..4407509a`
- Commits: 20
- Unique files touched: 48
- Overlap with Lazydino files: 33
- Protected areas: hooks_lifecycle(3), background_ambient(1), swarm_comm(3), compaction_session(2), interrupt_turn(2), provider_oauth(4), tui_render_remote(6), browser_tool_args(1), desktop(4)
- Top paths: src:31, crates:18, scripts:7, .github:2, packaging:1, Cargo.lock:1
- First commit: Add swarm spawn mode config default
- Last commit: Optimize desktop resize rendering
- Handling: Swarm config defaults, ChatGPT OAuth prompt, desktop reverts/additions, tool events and resize. High overlap with swarm/background/provider/TUI. Must preserve Lazydino swarm defaults and hook/background behavior.
- Overlap sample: `Cargo.lock`, `Cargo.toml`, `crates/jcode-compaction-core/src/lib.rs`, `crates/jcode-config-types/src/lib.rs`, `crates/jcode-import-core/src/lib.rs`, `crates/jcode-protocol/src/lib.rs`, `crates/jcode-protocol/src/protocol_tests/comm_requests.rs`, `crates/jcode-tui-mermaid/src/mermaid_viewport.rs`, `src/auth/lifecycle.rs`, `src/cli/args.rs`

### B03 — HIGH

- Range: `c06d52e9..9a356287`
- Commits: 20
- Unique files touched: 47
- Overlap with Lazydino files: 24
- Protected areas: hooks_lifecycle(2), background_ambient(1), compaction_session(3), interrupt_turn(6), provider_oauth(7), selfdev_reload(1), tui_render_remote(12), desktop(4)
- Top paths: src:50, crates:19, scripts:2
- First commit: Improve desktop transcript text colors
- Last commit: Show live direct provider catalogs in model picker
- Handling: Desktop rendering wave plus live direct provider catalogs. High overlap with provider model picker and selfdev/TUI. Provider catalog pieces may be valuable but must be reconciled with Antigravity/Kimi/custom model picker.
- Overlap sample: `crates/jcode-config-types/src/lib.rs`, `src/agent/turn_loops.rs`, `src/agent/turn_streaming_broadcast.rs`, `src/agent/turn_streaming_mpsc.rs`, `src/cli/auth_test.rs`, `src/config/default_file.rs`, `src/config_tests.rs`, `src/lib.rs`, `src/message/tests.rs`, `src/provider/mod.rs`

### B04 — HIGH

- Range: `879f95ad..4ef3607e`
- Commits: 20
- Unique files touched: 45
- Overlap with Lazydino files: 21
- Protected areas: hooks_lifecycle(1), background_ambient(1), interrupt_turn(1), provider_oauth(7), mcp(2), selfdev_reload(1), tui_render_remote(6), desktop(6)
- Top paths: src:28, crates:21, scripts:1, tests:1, docs:1
- First commit: Add client picker debug snapshot
- Last commit: Split overnight prompts and tests
- Handling: Client picker debug, live compatible model cache, protocol wire extraction, overnight split. High overlap with provider/MCP/selfdev/TUI. Consider manual port for model cache and protocol extraction only after checking our protocol tests.
- Overlap sample: `crates/jcode-protocol/src/lib.rs`, `crates/jcode-provider-core/src/lib.rs`, `crates/jcode-provider-metadata/src/lib.rs`, `crates/jcode-tui-markdown/src/lib.rs`, `src/auth/lifecycle.rs`, `src/auth/tests.rs`, `src/cli/provider_init.rs`, `src/cli/provider_init_tests.rs`, `src/config/config_file.rs`, `src/config_tests.rs`

### B05 — HIGH

- Range: `56efd93a..5b52a370`
- Commits: 20
- Unique files touched: 46
- Overlap with Lazydino files: 26
- Protected areas: hooks_lifecycle(3), background_ambient(1), interrupt_turn(3), provider_oauth(4), selfdev_reload(1), tui_render_remote(22), desktop(5)
- Top paths: src:36, crates:19, Cargo.lock:1, Cargo.toml:1
- First commit: Extract mermaid debug APIs
- Last commit: Fix inline diff expand badge feedback
- Handling: Mermaid/markdown extraction and TUI inline fixes. High overlap with our mermaid, side/pinned, and TUI changes. Manual port strongly preferred over cherry-pick.
- Overlap sample: `Cargo.lock`, `Cargo.toml`, `crates/jcode-protocol/src/lib.rs`, `crates/jcode-tui-markdown/src/lib.rs`, `crates/jcode-tui-mermaid/src/lib.rs`, `src/auth/lifecycle.rs`, `src/auth/mod.rs`, `src/cli/startup.rs`, `src/config.rs`, `src/config_tests.rs`

### B06 — HIGH

- Range: `828a3712..95087d57`
- Commits: 20
- Unique files touched: 26
- Overlap with Lazydino files: 13
- Protected areas: hooks_lifecycle(1), background_ambient(1), interrupt_turn(4), selfdev_reload(1), tui_render_remote(8), desktop(12)
- Top paths: crates:43, src:16
- First commit: Synchronize spinner-only TUI redraws
- Last commit: fix(desktop): render whitespace inline code pills
- Handling: Spinner/redraw and desktop markdown fixes. High overlap but narrower. Port TUI redraw fixes cautiously, verify remote coalescing and scratchpad behavior.
- Overlap sample: `src/agent/interrupts.rs`, `src/cli/terminal.rs`, `src/server/client_lifecycle.rs`, `src/server/client_lifecycle_tests.rs`, `src/server/state.rs`, `src/tool/selfdev/build_queue.rs`, `src/tui/app/remote.rs`, `src/tui/app/remote/key_handling.rs`, `src/tui/app/remote/queue_recovery.rs`, `src/tui/app/remote/server_events.rs`

### B07 — HIGH

- Range: `f85c2d59..9e7c68f2`
- Commits: 20
- Unique files touched: 48
- Overlap with Lazydino files: 22
- Protected areas: hooks_lifecycle(1), background_ambient(2), swarm_comm(2), compaction_session(1), interrupt_turn(4), provider_oauth(3), mcp(1), tui_render_remote(7), browser_tool_args(1), desktop(18)
- Top paths: crates:58, src:27, Cargo.lock:2
- First commit: Mark fallback model picker routes
- Last commit: Fix desktop fractional text scrolling
- Handling: Fallback model picker routes, desktop tests, provider/MCP/browser mixed changes. High overlap with provider OAuth and MCP. Treat provider commits one by one.
- Overlap sample: `Cargo.lock`, `src/agent.rs`, `src/agent/turn_loops.rs`, `src/agent/turn_streaming_broadcast.rs`, `src/agent/turn_streaming_mpsc.rs`, `src/provider/openai.rs`, `src/provider/openai_provider_impl.rs`, `src/provider/openai_stream_runtime.rs`, `src/server/client_actions.rs`, `src/server/client_comm_channels.rs`

### B08 — HIGH

- Range: `3a39411c..c5857d31`
- Commits: 20
- Unique files touched: 31
- Overlap with Lazydino files: 18
- Protected areas: interrupt_turn(2), provider_oauth(5), mcp(1), tui_render_remote(9), desktop(8)
- Top paths: crates:33, src:21, scripts:3, README.md:2
- First commit: Preserve desktop session during self-dev reload
- Last commit: Update slash suggestion chrome tests
- Handling: Desktop self-dev reload/session preservation, model picker fallback, slash suggestions. High overlap in provider/MCP/TUI. Preserve selfdev current-channel reload semantics.
- Overlap sample: `README.md`, `src/cli/login.rs`, `src/cli/provider_init.rs`, `src/cli/provider_init_tests.rs`, `src/provider/mod.rs`, `src/provider/openrouter.rs`, `src/provider_catalog.rs`, `src/provider_catalog_tests.rs`, `src/server/debug_help.rs`, `src/tui/app/debug_cmds.rs`

### B09 — HIGH

- Range: `71f2f9dc..4d20df15`
- Commits: 20
- Unique files touched: 42
- Overlap with Lazydino files: 11
- Protected areas: hooks_lifecycle(2), background_ambient(1), swarm_comm(1), interrupt_turn(1), provider_oauth(2), tui_render_remote(5), desktop(16)
- Top paths: crates:38, src:28
- First commit: Preserve desktop workspace state on hot reload
- Last commit: fix(desktop): stabilize streaming text fade
- Handling: Desktop workspace hot reload, streaming text animation, provider reselection. High overlap but many desktop-only commits. Separate desktop UI from provider reselection.
- Overlap sample: `crates/jcode-provider-metadata/src/lib.rs`, `src/cli/provider_init.rs`, `src/cli/provider_init_tests.rs`, `src/provider/openrouter_sse_stream.rs`, `src/server.rs`, `src/server/client_lifecycle.rs`, `src/server/swarm.rs`, `src/tui/app/navigation.rs`, `src/tui/app/run_shell.rs`, `src/tui/app/tests/scroll_copy_01/part_01.rs`

### B10 — HIGH

- Range: `55803f6b..32924cc6`
- Commits: 20
- Unique files touched: 35
- Overlap with Lazydino files: 15
- Protected areas: hooks_lifecycle(4), private_skills_prompt(1), interrupt_turn(1), selfdev_reload(3), tui_render_remote(11), desktop(7)
- Top paths: src:31, crates:27, tests:3
- First commit: feat: add commit slash command
- Last commit: feat(desktop): show filtered session counts
- Handling: Commit slash command, self-dev prompts, auth provider coverage, desktop session switcher. High overlap with private skills/prompts/selfdev/TUI. Upstream commit slash may conflict with our skills and hooks, manual review.
- Overlap sample: `src/cli/args.rs`, `src/cli/commands.rs`, `src/cli/dispatch.rs`, `src/cli/provider_init.rs`, `src/config.rs`, `src/prompt.rs`, `src/prompt_tests.rs`, `src/tui/app/commands.rs`, `src/tui/app/commands_improve.rs`, `src/tui/app/input_help.rs`

### B11 — HIGH

- Range: `7b5753c9..d69fb2a5`
- Commits: 20
- Unique files touched: 29
- Overlap with Lazydino files: 16
- Protected areas: compaction_session(1), interrupt_turn(1), provider_oauth(2), selfdev_reload(1), tui_render_remote(15), browser_tool_args(1), desktop(3)
- Top paths: src:25, crates:7, docs:2, scripts:1
- First commit: feat(desktop): expose accepted slash aliases
- Last commit: docs: define resume picker behavior
- Handling: Desktop slash aliases, resume behavior docs, browser/tool args. High overlap with compaction/session/provider/selfdev. Resume behavior must preserve Lazydino transcript recovery.
- Overlap sample: `src/browser_tests.rs`, `src/cli/commands_tests.rs`, `src/config/default_file.rs`, `src/provider/openai_tests/parsing_tools.rs`, `src/replay/tests.rs`, `src/tui/app/commands.rs`, `src/tui/app/inline_interactive.rs`, `src/tui/app/input_help.rs`, `src/tui/app/remote/key_handling.rs`, `src/tui/app/state_ui_input_helpers.rs`

### B12 — HIGH

- Range: `c3f63b38..ec0d5062`
- Commits: 20
- Unique files touched: 61
- Overlap with Lazydino files: 31
- Protected areas: compaction_session(2), interrupt_turn(5), selfdev_reload(1), tui_render_remote(23), desktop(18)
- Top paths: src:56, crates:27, Cargo.lock:2, Cargo.toml:1, scripts:1, README.md:1
- First commit: feat(desktop): promote workspace sessions in place
- Last commit: Fix compacted history visible window
- Handling: Workspace session promotion, slash suggestions, compacted history visible window. Very high overlap. Manual port only, with session/compaction tests.
- Overlap sample: `Cargo.lock`, `Cargo.toml`, `README.md`, `crates/jcode-config-types/src/lib.rs`, `crates/jcode-session-types/src/lib.rs`, `src/agent.rs`, `src/agent/turn_loops.rs`, `src/agent/turn_streaming_broadcast.rs`, `src/agent/turn_streaming_mpsc.rs`, `src/agent_tests.rs`

### B13 — HIGH

- Range: `318f9ca2..d415c401`
- Commits: 20
- Unique files touched: 71
- Overlap with Lazydino files: 48
- Protected areas: hooks_lifecycle(3), background_ambient(2), swarm_comm(2), interrupt_turn(8), provider_oauth(7), tui_render_remote(16), browser_tool_args(2), desktop(9)
- Top paths: src:85, crates:15, docs:3, Cargo.toml:2
- First commit: Add configurable tool profiles
- Last commit: Fix desktop resume switcher text rendering
- Handling: Configurable tool profiles, fresh-spawn resume, runtime logging/debug, desktop resume switcher. Highest overlap. Must compare against Lazydino tool profiles, hooks, background, provider, TUI.
- Overlap sample: `Cargo.toml`, `crates/jcode-config-types/src/lib.rs`, `src/agent.rs`, `src/agent/interrupts.rs`, `src/agent/turn_execution.rs`, `src/agent/turn_loops.rs`, `src/agent/turn_streaming_broadcast.rs`, `src/agent/turn_streaming_mpsc.rs`, `src/agent_tests.rs`, `src/cli/args.rs`

### B14 — MEDIUM-DESKTOP

- Range: `11f77cbb..2dfbc092`
- Commits: 20
- Unique files touched: 16
- Overlap with Lazydino files: 0
- Protected areas: desktop(16)
- Top paths: crates:41, docs:1
- First commit: feat(desktop): handle runtime server events
- Last commit: desktop: initialize app worker over IPC
- Handling: Desktop runtime server events and worker host IPC. Zero Lazydino overlap. Good first candidate, but still check Cargo/desktop build.

### B15 — LOW

- Range: `1fa42b70..2913e25c`
- Commits: 10
- Unique files touched: 4
- Overlap with Lazydino files: 0
- Protected areas: desktop(4)
- Top paths: crates:12, docs:1, scripts:1
- First commit: desktop: emit initial worker scene over IPC
- Last commit: desktop: add reload window e2e smoke
- Handling: Desktop worker scene/input/reload smoke. Zero Lazydino overlap. Good first candidate after B14.

## Per-batch checklist

Before applying a batch:

- [ ] Create/confirm integration branch
- [ ] Save upstream commit list for batch
- [ ] Save changed-file list for batch
- [ ] Mark protected overlaps from `CUSTOM_STACK_INVENTORY.md`
- [ ] Classify commits with decision labels

During application:

- [ ] Cherry-pick only clean `CHERRY_PICK_OK` commits
- [ ] Manually port `PORT_UPSTREAM` commits
- [ ] Preserve explicit `KEEP_OURS` behavior
- [ ] Record `SKIP_UPSTREAM` rationale

After application:

- [ ] `cargo fmt --check`
- [ ] `cargo check -q -p jcode`
- [ ] Run targeted tests for touched protected areas
- [ ] Commit batch with decision summary
- [ ] Push integration branch

## Abort/revert policy

- If a sub-batch causes unclear behavior regression, abort the cherry-pick and move commits to `NEEDS_REVIEW`.
- If a committed sub-batch later fails, revert that sub-batch commit rather than rewriting previous successful batches.
- Never force-push `deploy/m9-m27-catchup`; only integration branches may be rewritten during local triage.
