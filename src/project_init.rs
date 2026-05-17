use anyhow::{Context, Result, anyhow};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ProjectInitOptions {
    pub target_dir: PathBuf,
    pub force: bool,
    pub gitignore: bool,
    pub ignore_team_agents: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectInitAction {
    Wrote(PathBuf),
    Kept(PathBuf),
    AddedGitExclude(PathBuf),
    GitExcludeAlreadyPresent(PathBuf),
    AddedGitignore(PathBuf),
    GitignoreAlreadyPresent(PathBuf),
    SkippedGitIgnore,
}

#[derive(Debug, Clone)]
pub struct ProjectInitReport {
    pub jcode_dir: PathBuf,
    pub actions: Vec<ProjectInitAction>,
}

impl ProjectInitReport {
    pub fn print_human(&self) {
        for action in &self.actions {
            match action {
                ProjectInitAction::Wrote(path) => println!("wrote: {}", path.display()),
                ProjectInitAction::Kept(path) => println!("keep existing: {}", path.display()),
                ProjectInitAction::AddedGitExclude(path) => {
                    println!("added .jcode/ to {}", path.display())
                }
                ProjectInitAction::GitExcludeAlreadyPresent(path) => {
                    println!(".jcode/ already in {}", path.display())
                }
                ProjectInitAction::AddedGitignore(path) => {
                    println!("added .jcode/ to {}", path.display())
                }
                ProjectInitAction::GitignoreAlreadyPresent(path) => {
                    println!(".jcode/ already in {}", path.display())
                }
                ProjectInitAction::SkippedGitIgnore => {
                    println!("not a git repository: skipped git ignore/exclude setup")
                }
            }
        }
        println!();
        println!(
            "Initialized project-local Jcode harness at: {}",
            self.jcode_dir.display()
        );
        for file in generated_paths(&self.jcode_dir) {
            println!("{}", file.display());
        }
    }
}

pub fn init_project(options: ProjectInitOptions) -> Result<ProjectInitReport> {
    let target_dir = normalize_existing_dir(&options.target_dir)?;
    let jcode_dir = target_dir.join(".jcode");
    fs::create_dir_all(jcode_dir.join("hooks"))?;
    fs::create_dir_all(jcode_dir.join("harness"))?;

    let mut actions = Vec::new();
    write_generated_file(
        &jcode_dir.join("config.toml"),
        &config_toml(options.ignore_team_agents),
        false,
        options.force,
        &mut actions,
    )?;
    write_generated_file(
        &jcode_dir.join("AGENTS.md"),
        AGENTS_MD,
        false,
        options.force,
        &mut actions,
    )?;
    write_generated_file(
        &jcode_dir.join("harness/10-routing-policy.md"),
        ROUTING_POLICY_MD,
        false,
        options.force,
        &mut actions,
    )?;
    write_generated_file(
        &jcode_dir.join("harness/20-project-rules.md"),
        PROJECT_RULES_MD,
        false,
        options.force,
        &mut actions,
    )?;
    write_generated_file(
        &jcode_dir.join("hooks/check-bash.sh"),
        CHECK_BASH_SH,
        true,
        options.force,
        &mut actions,
    )?;
    write_generated_file(
        &jcode_dir.join("hooks/log-tool.sh"),
        LOG_TOOL_SH,
        true,
        options.force,
        &mut actions,
    )?;

    // M47-C9: ship 4 sample agent profiles so a freshly initialized project
    // demonstrates the 5-dimension provider-aware schema. Each persona targets
    // a different backend so users can read them as concrete documentation of
    // how `model` / `variant` / `effort` / `context` / `thinking` interact.
    write_generated_file(
        &jcode_dir.join("agents/claude-strategist.md"),
        SAMPLE_AGENT_CLAUDE_STRATEGIST_MD,
        false,
        options.force,
        &mut actions,
    )?;
    write_generated_file(
        &jcode_dir.join("agents/gpt-coder.md"),
        SAMPLE_AGENT_GPT_CODER_MD,
        false,
        options.force,
        &mut actions,
    )?;
    write_generated_file(
        &jcode_dir.join("agents/gemini-visual.md"),
        SAMPLE_AGENT_GEMINI_VISUAL_MD,
        false,
        options.force,
        &mut actions,
    )?;
    write_generated_file(
        &jcode_dir.join("agents/glm-worker.md"),
        SAMPLE_AGENT_GLM_WORKER_MD,
        false,
        options.force,
        &mut actions,
    )?;

    update_ignore_files(&target_dir, options.gitignore, &mut actions)?;

    Ok(ProjectInitReport { jcode_dir, actions })
}

fn normalize_existing_dir(path: &Path) -> Result<PathBuf> {
    let path = if path.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        path.to_path_buf()
    };
    let canonical = path
        .canonicalize()
        .with_context(|| format!("target directory does not exist: {}", path.display()))?;
    if !canonical.is_dir() {
        return Err(anyhow!(
            "target is not a directory: {}",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn write_generated_file(
    path: &Path,
    content: &str,
    executable: bool,
    force: bool,
    actions: &mut Vec<ProjectInitAction>,
) -> Result<()> {
    if path.exists() && !force {
        actions.push(ProjectInitAction::Kept(path.to_path_buf()));
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    set_mode(path, executable)?;
    actions.push(ProjectInitAction::Wrote(path.to_path_buf()));
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, executable: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if executable { 0o755 } else { 0o644 };
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(mode);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _executable: bool) -> Result<()> {
    Ok(())
}

fn update_ignore_files(
    target_dir: &Path,
    use_gitignore: bool,
    actions: &mut Vec<ProjectInitAction>,
) -> Result<()> {
    let Some(git_root) = find_git_root(target_dir)? else {
        actions.push(ProjectInitAction::SkippedGitIgnore);
        return Ok(());
    };

    if use_gitignore {
        let path = git_root.join(".gitignore");
        append_ignore_entry(
            &path,
            ProjectInitAction::AddedGitignore,
            ProjectInitAction::GitignoreAlreadyPresent,
            actions,
        )
    } else {
        let git_dir = git_dir_for_root(&git_root)?;
        let path = git_dir.join("info/exclude");
        append_ignore_entry(
            &path,
            ProjectInitAction::AddedGitExclude,
            ProjectInitAction::GitExcludeAlreadyPresent,
            actions,
        )
    }
}

fn append_ignore_entry(
    path: &Path,
    added: fn(PathBuf) -> ProjectInitAction,
    present: fn(PathBuf) -> ProjectInitAction,
    actions: &mut Vec<ProjectInitAction>,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let existing = fs::read_to_string(path).unwrap_or_default();
    if existing.lines().any(|line| line.trim() == ".jcode/") {
        actions.push(present(path.to_path_buf()));
        return Ok(());
    }
    let prefix = if existing.ends_with('\n') || existing.is_empty() {
        ""
    } else {
        "\n"
    };
    fs::write(
        path,
        format!("{existing}{prefix}\n# Private Jcode harness\n.jcode/\n"),
    )?;
    actions.push(added(path.to_path_buf()));
    Ok(())
}

fn find_git_root(start: &Path) -> Result<Option<PathBuf>> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join(".git").exists() {
            return Ok(Some(dir));
        }
        if !dir.pop() {
            return Ok(None);
        }
    }
}

fn git_dir_for_root(root: &Path) -> Result<PathBuf> {
    let git_path = root.join(".git");
    if git_path.is_dir() {
        return Ok(git_path);
    }
    let content = fs::read_to_string(&git_path)
        .with_context(|| format!("failed to read git file: {}", git_path.display()))?;
    let rel = content
        .strip_prefix("gitdir:")
        .ok_or_else(|| anyhow!("unsupported .git file format in {}", git_path.display()))?
        .trim();
    let path = PathBuf::from(rel);
    Ok(if path.is_absolute() {
        path
    } else {
        root.join(path)
    })
}

fn generated_paths(jcode_dir: &Path) -> Vec<PathBuf> {
    [
        "AGENTS.md",
        "config.toml",
        "harness/10-routing-policy.md",
        "harness/20-project-rules.md",
        "hooks/check-bash.sh",
        "hooks/log-tool.sh",
    ]
    .into_iter()
    .map(|path| jcode_dir.join(path))
    .collect()
}

fn config_toml(ignore_team_agents: bool) -> String {
    format!(
        r#"# Project-local Jcode harness config.
# This file is intended to live in a private, gitignored .jcode/ directory.

[prompt]
# Set true when you want to bypass the team's project AGENTS.md for this checkout.
ignore_project_agents = {ignore_team_agents}
ignore_global_agents = false
load_jcode_agents = true
load_harness_dir = true
# Relative paths resolve under this private .jcode/ directory.
private_instructions = ["private_instructions", "private_instructions.md", "rules/*.md"]

[hooks]
enabled = true

[[hooks.commands]]
event = "tool.execute.before"
tool = "bash"
command = ".jcode/hooks/check-bash.sh"
blocking = true
timeout_ms = 3000

[[hooks.commands]]
event = "tool.execute.after"
tool = "*"
command = ".jcode/hooks/log-tool.sh"
blocking = false
timeout_ms = 3000
"#
    )
}

const AGENTS_MD: &str = r#"# Private Jcode Harness

This directory is Lazydino's private project-local harness for Jcode.

## Intent

- Preserve project/team instructions unless `.jcode/config.toml` sets `ignore_project_agents = true`.
- Prefer this private harness for personal workflow details, local hooks, and routing preferences.
- Do not assume `.jcode/` is committed. Treat it as local/private by default.

## Working style

- Be proactive and finish natural next steps.
- Run focused validation after code changes.
- Avoid destructive actions unless explicitly requested.
- Document project-specific discoveries in `.jcode/harness/20-project-rules.md` when useful.
"#;

const ROUTING_POLICY_MD: &str = r#"# Jcode Agent Routing Policy

Use the configured Jcode agent profiles intentionally.

## Model/persona guidance

- Human intent, planning, architecture, frontend/product judgment, and many-case reasoning: prefer `metis`, `planner`, `prometheus`, or `reviewer`.
- Concrete implementation, backend edits, command execution, and validation loops: prefer `hephaestus`, `coder`, `executor`, or `oracle`.
- Unknown codebase area: use `searcher`, `librarian`, `atlas`, or `explore` depending on depth.
- Difficult debugging: use `sisyphus`; optionally use `oracle` for a GPT second opinion.
- Visual/UI inspection: use `visual` or `multimodal-looker`.

## Delegation rule

Only delegate when it reduces risk or speeds up real progress. For small tasks, solve directly.
"#;

const PROJECT_RULES_MD: &str = r#"# Project-specific Jcode Notes

Fill this file with local project conventions discovered during work.

## Validation

- TODO: add primary test/check command.
- TODO: add lint/typecheck command.
- TODO: add build command if relevant.

## Architecture notes

- TODO: summarize important directories and boundaries.

## Safety notes

- TODO: list commands, migrations, or services that require extra care.
"#;

const CHECK_BASH_SH: &str = r#"#!/usr/bin/env bash
set -euo pipefail

payload=$(cat || true)

python3 - "$payload" <<'PY'
import json
import re
import sys

payload = sys.argv[1] if len(sys.argv) > 1 else ""
try:
    data = json.loads(payload) if payload.strip() else {}
except Exception:
    data = {"raw": payload}

blob = json.dumps(data, ensure_ascii=False)
blocked = [
    (r"\brm\s+-rf\s+/(?:[\s\\\"\x27}\]]|$)", "Refusing rm -rf /"),
    (r"\bsudo\s+rm\s+-rf\s+/(?:[\s\\\"\x27}\]]|$)", "Refusing rm -rf /"),
    (r"\bdd\s+.*\bof=/dev/(sd|nvme|vd)", "Refusing raw disk overwrite"),
    (r"\bmkfs(?:\.[a-z0-9]+)?\s+/dev/", "Refusing filesystem creation on block device"),
]

for pattern, reason in blocked:
    if re.search(pattern, blob, re.IGNORECASE | re.DOTALL):
        print(json.dumps({"action": "deny", "reason": reason}))
        sys.exit(0)

print(json.dumps({"action": "allow"}))
PY
"#;

const LOG_TOOL_SH: &str = r#"#!/usr/bin/env bash
set -euo pipefail

mkdir -p .jcode/hooks
payload=$(cat || true)
printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$payload" >> .jcode/hooks/tool-events.jsonl
exit 0
"#;

// ---- M47-C9: sample agent profiles ----
//
// Each sample targets a different provider (Claude / OpenAI / Gemini /
// OpenRouter-GLM) so users can see how the 5-dimension schema actually
// maps to provider channels. The frontmatter keys mirror the M47-C3
// schema and the M47-C5 variant resolver behavior:
//
//   model    -> the model id
//   variant  -> provider-aware "max" alias (Claude=1m, GPT=xhigh effort,
//               Gemini=thinking on, OpenRouter=effort+thinking)
//   effort   -> explicit reasoning effort, applied where supported
//   context  -> "200k"|"1m"; Anthropic uses [1m] suffix, others ignore
//   thinking -> bool toggle; Anthropic/Gemini/OpenRouter consume
//
// All four files land in `.jcode/agents/*.md` so the `subagent` tool can
// invoke them by name (e.g. `subagent_type="claude-strategist"`).

const SAMPLE_AGENT_CLAUDE_STRATEGIST_MD: &str = r#"---
name: claude-strategist
model: claude-opus-4-7
variant: max
description: Strategy and architecture lead — multi-step planning, user-intent inference, large-context reasoning.
when:
  - the task is ambiguous or multi-step
  - architecture or sequencing matters
  - many user cases / edge cases need consideration
---
You are claude-strategist. Read the user's request carefully, infer the
underlying intent, and produce a concise plan that prioritizes the
fewest reversible steps needed to validate the right direction. Prefer
research → small experiments → broad changes.

When delegating, hand off concrete code edits to gpt-coder, mechanical
work to executor-style agents, and visual inspection to gemini-visual.

variant=max on Anthropic routes you through the [1m] long-context window
so multi-file synthesis is safe.
"#;

const SAMPLE_AGENT_GPT_CODER_MD: &str = r#"---
name: gpt-coder
model: gpt-5.5
effort: medium
description: Implementation agent — concrete code changes, focused tests, validation loops.
when:
  - the plan is clear
  - files need editing
  - tests or focused validation should be run after changes
---
You are gpt-coder. Implement the change exactly as planned. Run focused
validation (cargo check, targeted tests, or the project's quick smoke)
after each meaningful edit. Report file diffs and validation results,
not narrative explanations.

effort=medium balances throughput against debugging quality. Bump to
high only for code that is hard to validate purely by tests.
"#;

const SAMPLE_AGENT_GEMINI_VISUAL_MD: &str = r#"---
name: gemini-visual
model: gemini-3.1-pro-preview
thinking: true
description: Visual / UI / multimodal specialist — screenshots, layouts, diagrams, design critique.
when:
  - the work involves screenshots, UI, or visual quality
  - frontend layout or design needs review
  - a diagram or visual artifact is the deliverable
---
You are gemini-visual. Inspect visual artifacts carefully, describe what
you see in a way the coordinator can act on, and recommend concrete
visual changes (alignment, spacing, color, hierarchy). When generating
content, prefer compact descriptions plus a single representative image
over verbose prose.

thinking=true enables Gemini thinking_budget so layout reasoning gets
extra compute when needed.
"#;

const SAMPLE_AGENT_GLM_WORKER_MD: &str = r#"---
name: glm-worker
model: zhipu/glm-4-6
variant: max
description: Worker agent for OpenRouter-served reasoning models (GLM family).
when:
  - mechanical edits or repetitive work
  - secondary opinion on backend correctness
  - cost-sensitive long runs where a reasoning-capable smaller model is OK
---
You are glm-worker. Execute the requested change directly and report
back. Treat ambiguity as a signal to ask one focused clarifying question
rather than guess.

variant=max on OpenRouter routes both reasoning_effort=xhigh and the
thinking channel (where the model family supports it), so you can lean
on the provider-side reasoning surface for harder cases.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_project_writes_private_harness_and_git_exclude() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        fs::create_dir(&project).unwrap();
        fs::create_dir(project.join(".git")).unwrap();
        fs::create_dir_all(project.join(".git/info")).unwrap();

        let report = init_project(ProjectInitOptions {
            target_dir: project.clone(),
            force: false,
            gitignore: false,
            ignore_team_agents: false,
        })
        .unwrap();

        assert_eq!(report.jcode_dir, project.join(".jcode"));
        assert!(project.join(".jcode/config.toml").exists());
        assert!(project.join(".jcode/hooks/check-bash.sh").exists());
        // M47-C9: 4 sample agent profiles land alongside the harness files.
        assert!(project.join(".jcode/agents/claude-strategist.md").exists());
        assert!(project.join(".jcode/agents/gpt-coder.md").exists());
        assert!(project.join(".jcode/agents/gemini-visual.md").exists());
        assert!(project.join(".jcode/agents/glm-worker.md").exists());
        let config = fs::read_to_string(project.join(".jcode/config.toml")).unwrap();
        assert!(config.contains("ignore_project_agents = false"));
        let exclude = fs::read_to_string(project.join(".git/info/exclude")).unwrap();
        assert!(exclude.contains(".jcode/"));
    }

    // M47-C9: sample agent profiles parse back into AgentRouteConfig with the
    // dimensions advertised by the M47 plan (variant=max routes per provider,
    // explicit effort, explicit thinking). This is the end-to-end sanity check
    // that the shipped templates remain in sync with the parser.
    #[test]
    fn m47_c9_sample_agents_parse_with_expected_dimensions() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        fs::create_dir(&project).unwrap();
        init_project(ProjectInitOptions {
            target_dir: project.clone(),
            force: false,
            gitignore: false,
            ignore_team_agents: false,
        })
        .unwrap();

        let agents_dir = project.join(".jcode/agents");
        let by_name = crate::agent_profiles_md::load_agents_from_dir(&agents_dir);

        let strategist = by_name
            .get("claude-strategist")
            .expect("claude-strategist sample loaded");
        assert_eq!(strategist.model.as_deref(), Some("claude-opus-4-7"));
        assert_eq!(strategist.variant.as_deref(), Some("max"));

        let coder = by_name.get("gpt-coder").expect("gpt-coder sample loaded");
        assert_eq!(coder.model.as_deref(), Some("gpt-5.5"));
        assert_eq!(coder.effort.as_deref(), Some("medium"));

        let visual = by_name
            .get("gemini-visual")
            .expect("gemini-visual sample loaded");
        assert_eq!(visual.model.as_deref(), Some("gemini-3.1-pro-preview"));
        assert_eq!(visual.thinking, Some(true));

        let glm = by_name.get("glm-worker").expect("glm-worker sample loaded");
        assert_eq!(glm.model.as_deref(), Some("zhipu/glm-4-6"));
        assert_eq!(glm.variant.as_deref(), Some("max"));
    }

    #[test]
    fn init_project_keeps_existing_files_without_force() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join(".jcode/config.toml");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        fs::write(&config, "custom").unwrap();

        init_project(ProjectInitOptions {
            target_dir: temp.path().to_path_buf(),
            force: false,
            gitignore: false,
            ignore_team_agents: true,
        })
        .unwrap();

        assert_eq!(fs::read_to_string(config).unwrap(), "custom");
    }

    #[test]
    fn init_project_can_write_gitignore_and_ignore_team_agents() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join(".git")).unwrap();

        init_project(ProjectInitOptions {
            target_dir: temp.path().to_path_buf(),
            force: false,
            gitignore: true,
            ignore_team_agents: true,
        })
        .unwrap();

        let config = fs::read_to_string(temp.path().join(".jcode/config.toml")).unwrap();
        assert!(config.contains("ignore_project_agents = true"));
        let gitignore = fs::read_to_string(temp.path().join(".gitignore")).unwrap();
        assert!(gitignore.contains(".jcode/"));
    }
}
