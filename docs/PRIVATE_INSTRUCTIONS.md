# Private Jcode Instructions

Jcode treats repository instruction files as workspace policy, not as optional notes.

## Load order and priority

When building the model prompt, Jcode loads instruction sources and appends an explicit priority reminder so the model knows how to resolve conflicts:

1. Explicit current user request
2. Nearest nested private `.jcode` instruction discovered from files touched by read/search/edit tools
3. Project private `.jcode` instructions
4. Repo/global `AGENTS.md` or `agents.md`
5. Default Jcode behavior

## Files

Jcode reads these at prompt construction time when available:

- `<project>/AGENTS.md` or `<project>/agents.md`
- `~/.AGENTS.md` or `~/.agents.md`
- `<project>/.jcode/AGENTS.md` or `<project>/.jcode/agents.md`
- `<project>/.jcode/harness/*.md`
- `[prompt].private_instructions` globs, resolved under `<project>/.jcode/` by default

Jcode also injects nearby private rules after file-local tools touch paths:

- `<dir>/.jcode/AGENTS.md` or `<dir>/.jcode/agents.md`
- `<dir>/.jcode/instructions.md`
- `<dir>/.jcode/rules/*.md`

These nested injections are nearest-first and deduped for the current user turn to avoid repeated token growth.

## Private-only mode

For private harness usage, put this in `<project>/.jcode/config.toml`:

```toml
[prompt]
ignore_project_agents = true
load_jcode_agents = true
load_harness_dir = true
private_instructions = ["rules/*.md", "monorepo/*/AGENTS.md"]
```

With `ignore_project_agents = true`, team repo `AGENTS.md` is skipped, while private `.jcode` instructions remain loaded and highest-priority.
