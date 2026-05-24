# Lazydino Jcode Custom Stack Inventory

Last updated: 2026-05-24
Baseline for upstream catchup planning.

## Why this exists

This branch is not a small fork. It is a custom Jcode distribution with hundreds of branch-only commits. Upstream catchup must preserve the custom stack and port upstream intent selectively. Do not treat upstream as automatically authoritative in overlapping areas.

Current divergence snapshot when this file was created:

- Merge base with `upstream/master`: `50d2c68b`
- Branch-only commits: 312
- Upstream-only commits: 290
- Overlapping changed files since merge base: 136
- Patch-id equivalent upstream commits: 0

## Catchup rule

For every upstream batch, use a 3-way intent review:

1. What did upstream change and why?
2. What did Lazydino/Jcode already change in the same area?
3. Should we keep ours, port upstream manually, cherry-pick, or skip?

Never blindly overwrite the protected areas below.

## Protected custom areas

### 1. Hooks and lifecycle automation

Purpose:

- Tool lifecycle hooks
- Session lifecycle hooks
- `response.completed` and `session.stop` events
- Non-blocking hook flush before CLI exit
- Hook output injection into turns
- ContinueImmediate / lifecycle continuation wiring
- Global/project hook dedupe

Key files:

- `src/hooks.rs`
- `src/auth/lifecycle.rs`
- `src/auth/lifecycle_driver.rs`
- `src/cli/dispatch.rs`
- `src/cli/startup.rs`
- `src/cli/commands.rs`
- `src/server/client_lifecycle.rs`
- `src/prompt.rs`
- `docs/HOOKS_USER_GUIDE.md`
- `docs/M35_LIFECYCLE_HOOK_INJECT.md`
- `LAZYDINO_LIVE_VERIFICATION_M9_M14a.md`

Representative commits/milestones:

- M9 hook dedupe
- M10 non-blocking hook race / CLI flush
- M11 lifecycle continuation
- M35 lifecycle hook stdout injection

Catchup policy:

- Manual review required for all upstream changes touching these files.
- Preserve Lazydino hook semantics unless upstream provides a strictly better equivalent.
- If upstream refactors lifecycle flow, port our hook emission and injection points into the new structure.

### 2. Private instructions, prompts, skills, project harness

Purpose:

- Private `.jcode` instruction discovery
- Nested private instruction globs
- AGENTS/private instruction priority model
- Project-local skills and slash command support
- Lazy harness init/update/sync skills
- Agent profile markdown generation and profile routing

Key files:

- `.jcode/skills/**`
- `src/prompt.rs`
- `src/skill.rs`
- `src/project_commands.rs`
- `src/project_init.rs`
- `src/agent_profiles_md.rs`
- `src/cli/commands.rs`
- `docs/PRIVATE_INSTRUCTIONS.md`
- `docs/lazydino/**`

Representative commits/milestones:

- M45 private instructions and AGENTS priority
- project-local hook/config/skill sync
- agent profiles and subagent recursion controls

Catchup policy:

- Keep private/project-local instruction priority intact.
- Upstream prompt changes should be merged around, not over, the Lazydino prompt overlay behavior.

### 3. Background tasks, auto-inject, ambient delivery

Purpose:

- Background completion wake channel
- Auto-inject completed task output into the parent session
- User-message delivery for idle parent wakeups
- Ambient/background task delivery target routing
- Output caps and reliable progress/checkpoint handling

Key files:

- `src/background.rs`
- `src/background/model.rs`
- `src/server/background_tasks.rs`
- `src/server/client_lifecycle.rs`
- `src/server/client_actions.rs`
- `src/tool/ambient.rs`
- `src/tool/bash.rs`
- `src/tool/task.rs`
- `docs/M31_BG_AUTO_INJECT.md`
- `LAZYDINO_TEST_PLAN.md`

Representative commits/milestones:

- M1 background delivery target routing
- M5 Alt+B early race
- M30/M31 background auto-inject
- M34 schedule/context aliases

Catchup policy:

- Preserve parent wake and auto-inject semantics.
- Review upstream background/tool changes for duplicate or incompatible delivery paths.

### 4. Swarm, communication, task orchestration

Purpose:

- Swarm task assignment, status reporting, retry/resume
- Worker report delivery even when status does not change
- Await-members behavior and reload deadlines
- Communication channel/session persistence

Key files:

- `src/server/comm_control.rs`
- `src/server/comm_await.rs`
- `src/server/comm_session.rs`
- `src/server/swarm.rs`
- `src/server/await_members_state.rs`
- `src/server/comm_control_tests/**`
- `src/tool/communicate.rs`
- `src/tool/task.rs`

Representative commits/milestones:

- M2 swarm stability stages
- M38 swarm worker report delivery
- swarm run id and diagnostics

Catchup policy:

- Manual review any upstream changes in communication, server state, or task assignment.
- Preserve Lazydino task timeout, report delivery, and reload-aware await behavior.

### 5. Compaction, overflow recovery, transcript/session persistence

Purpose:

- Compaction failure cooldown
- Emergency compaction loop prevention
- Native/overflow runtime pruning
- Replay and transcript recovery
- Fresh-spawn resume restoration

Key files:

- `src/agent/compaction.rs`
- `src/compaction.rs`
- `crates/jcode-compaction-core/src/lib.rs`
- `src/session/journal.rs`
- `src/session/persistence.rs`
- `src/session_tests/**`
- `docs/lazydino/milestones/M48.md`
- `LAZYDINO_LIVE_VERIFICATION_M9_M14a.md`

Representative commits/milestones:

- M14/M14a compaction cooldown and loop prevention
- M48 overflow/runtime work
- compacted history visible window fixes

Catchup policy:

- Do not regress cooldown/loop-prevention.
- Upstream compaction refactors require targeted compaction and resume tests before acceptance.

### 6. Interrupt, cancel, queued prompt, turn-loop behavior

Purpose:

- TurnControl and provider cancellation propagation
- Tool cancel propagation
- Interrupted transcript finalization
- Two-step Esc interrupt UX
- Queued prompt retention/resubmission after interrupt
- Parallel tool dispatch and turn-loop fanout
- Cancel prefire before Ack writer lock

Key files:

- `crates/jcode-agent-runtime/src/lib.rs`
- `src/agent/interrupts.rs`
- `src/agent/turn_execution.rs`
- `src/agent/turn_loops.rs`
- `src/agent/turn_streaming_broadcast.rs`
- `src/agent/turn_streaming_mpsc.rs`
- `src/server/client_lifecycle.rs`
- `src/server/client_lifecycle_tests.rs`
- `src/tui/app/input.rs`
- `src/tui/app/remote/key_handling.rs`
- `src/tui/app/turn.rs`
- `docs/lazydino/milestones/M22.md`
- `docs/lazydino/milestones/M49.md`

Representative commits/milestones:

- M22 parallel tool dispatch/fanout
- M49 interrupt series
- provider cancellation propagation
- queued prompt independent item support
- `7e1f4adc` cancel prefire before Ack

Catchup policy:

- Manual review required.
- Preserve second-Esc UX and prefire cancel latency fix.
- Validate with interrupt/cancel focused tests after every related upstream batch.

### 7. Provider stack, routing, OAuth, model picker

Purpose:

- Antigravity OAuth provider
- Gemini 3.5 Flash / 3.5 models and backend ID mapping
- Kimi OAuth
- Gemini tool schema compatibility
- OpenAI/OpenRouter route controls and explicit provider selection
- Model picker compatible provider entries
- Disabling unwanted automatic model fallback

Key files:

- `src/provider/antigravity.rs`
- `src/provider/antigravity_tests.rs`
- `src/provider/gemini.rs`
- `src/provider/gemini_tests.rs`
- `src/auth/kimi.rs`
- `src/provider/openai*.rs`
- `src/provider/openrouter*.rs`
- `src/provider/routing.rs`
- `src/provider/selection.rs`
- `src/provider/models.rs`
- `crates/jcode-provider-gemini/src/lib.rs`
- `crates/jcode-provider-core/**`
- `crates/jcode-provider-metadata/**`

Representative commits:

- Antigravity Gemini 3.5 Flash variants
- Kimi OAuth and Gemini tool schemas
- Antigravity catalog model IDs
- Antigravity Gemini tool schema/tool call normalization
- `c980add0` Antigravity Gemini 3.5 Flash backend ID mapping

Catchup policy:

- Do not overwrite Antigravity/Kimi/Gemini routing with upstream defaults.
- Preserve explicit provider behavior and disabled automatic OpenAI fallback.
- Validate with provider-specific tests and model picker tests.

### 8. MCP management and reload repair

Purpose:

- MCP manager reload/reconcile
- Retry unknown tools after refresh
- OAuth support gap tracking
- Tool lock refresh/unlock after reload and debug repair
- Remote MCP reload command/autocomplete

Key files:

- `src/mcp/**`
- `src/tool/mcp.rs`
- `src/server/debug*.rs`
- `src/cli/commands.rs`
- `docs/lazydino/milestones/M44.md`
- `docs/lazydino/milestones/M50.md`

Representative commits/milestones:

- M44 MCP OAuth gap
- M50 MCP reload/reconcile series
- unknown-tool retry refresh
- remote MCP reload command

Catchup policy:

- Preserve reload repair paths and unknown-tool recovery.
- Any upstream MCP registry refactor must be adapted to keep Lazydino debug and remote reload commands.

### 9. Self-development build/reload and installation channels

Purpose:

- Coordinated selfdev build queue
- Current/stable/canary install channels
- Reload onto rebuilt binaries
- Remote build fallback
- Custom install scripts
- Selfdev tests and launcher path policy

Key files:

- `src/tool/selfdev/**`
- `src/cli/selfdev_tests.rs`
- `scripts/dev_cargo.sh`
- `scripts/install.sh`
- `scripts/lazydino/**`
- `src/server/reload.rs`
- `src/server/reload_tests.rs`
- `src/prompt/selfdev_*.txt`

Catchup policy:

- Preserve local active channel behavior: `~/.jcode/builds/current/jcode` and launcher symlink policy.
- Upstream desktop reload work should not break TUI selfdev reload flow.

### 10. TUI, remote mode, inline external tools, side/pinned panels

Purpose:

- Remote streaming redraw coalescing
- Remote compact/sending stall fixes
- Inline lazygit/nvim external tools
- Scratchpad foreground handoff
- Image/diagram pinned side panel
- Mermaid render fixes and optional mmdc renderer
- Leading-space slash escape behavior

Key files:

- `src/tui/**`
- `crates/jcode-tui-mermaid/**`
- `crates/jcode-tui-markdown/**`
- `src/tool/open*.rs`
- `src/tool/side_panel*.rs`
- `docs/lazydino/milestones/M40.md`
- `docs/MERMAID_RENDERING_REDESIGN.md`

Representative commits/milestones:

- M28 Mermaid rendering work
- M40 image attach/leading-space slash
- inline lazygit/nvim and scratchpad series
- side panel/pinned image improvements

Catchup policy:

- Upstream TUI rendering changes are high-risk because many files overlap.
- Validate remote mode, slash behavior, image paste, and pinned/side panel flows.

### 11. Browser host repair and web/tooling polish

Purpose:

- Browser native host setup repair
- Missing native host download/reinstall handling
- Search/tool malformed argument normalization
- Batch sibling argument preservation

Key files:

- `src/browser.rs`
- `src/browser_tests.rs`
- `src/tool/websearch.rs`
- `src/tool/agentgrep.rs`
- `src/tool/mod.rs`
- `src/tool/tests.rs`

Representative commits:

- browser host repair download
- malformed search input normalization
- batch subcall sibling argument preservation

Catchup policy:

- Preserve defensive normalization and repair behavior.

## Batch validation gates

Minimum gates for any upstream catchup batch:

```bash
cargo fmt --check
cargo check -q -p jcode
```

Additional targeted gates by touched area:

- Hooks/lifecycle: lifecycle hook tests, CLI flush tests
- Provider/OAuth: provider tests, Antigravity/Gemini/Kimi model route tests
- Interrupt/cancel: client lifecycle and interrupt tests
- MCP: MCP reload/reconcile tests
- Swarm/comm: comm_control and swarm persistence tests
- Compaction/session: compaction and session persistence tests
- TUI/rendering: relevant TUI state/remote/slash tests
- Selfdev: selfdev tests plus `selfdev build` and `selfdev reload`

## Upstream catchup decision labels

Use these labels in batch notes:

- `KEEP_OURS`: upstream conflicts with custom behavior, keep Lazydino code.
- `PORT_UPSTREAM`: upstream behavior is desirable but must be adapted manually.
- `CHERRY_PICK_OK`: upstream commit applies cleanly and does not touch protected behavior.
- `SKIP_UPSTREAM`: upstream commit is irrelevant or superseded by Lazydino implementation.
- `NEEDS_REVIEW`: cannot decide without deeper runtime or product judgment.

## Recommended catchup workflow

1. Create integration branch from current deploy branch.
2. For each 10-30 commit upstream batch:
   - Generate commit list and changed file list.
   - Mark protected overlaps.
   - Apply `CHERRY_PICK_OK` commits only.
   - Manually port `PORT_UPSTREAM` changes.
   - Keep explicit notes for `KEEP_OURS` and `SKIP_UPSTREAM`.
   - Run gates and commit the batch.
   - Push after each successful batch.
3. After all batches, selfdev build/reload and run smoke tests.
4. Merge integration branch back into deploy branch only after final validation.
