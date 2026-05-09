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
