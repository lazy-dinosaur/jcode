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



## Project-local harness initialization skill

A reusable Claude/Jcode skill exists to initialize project-local `.jcode/` harness directories on demand.

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
~/.claude/skills/jcode-init/scripts/init-jcode-project.sh <tmp-project>
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
