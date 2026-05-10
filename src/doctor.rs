use crate::agent_profiles_md;
use crate::config::{AgentRouteConfig, Config};
use crate::mcp::McpConfig;
use crate::project_commands;
use crate::skill::SkillRegistry;
use anyhow::Context;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DoctorOptions {
    pub json: bool,
    pub quiet: bool,
    pub working_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub project_root: Option<PathBuf>,
    pub mode: String,
    pub sections: Vec<Section>,
    pub summary: Summary,
}

#[derive(Debug, Clone, Serialize)]
pub struct Section {
    pub name: String,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub status: String,
    pub label: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Summary {
    pub ok: u32,
    pub warn: u32,
    pub error: u32,
}

impl Report {
    pub fn exit_code(&self) -> i32 {
        if self.summary.error > 0 {
            2
        } else if self.summary.warn > 0 {
            1
        } else {
            0
        }
    }
}

pub async fn run(opts: DoctorOptions) -> anyhow::Result<i32> {
    let report = build_report(&opts).await;
    let rendered = if opts.json {
        render_json(&report)?
    } else {
        render_human(&report, std::io::stdout().is_terminal(), opts.quiet)
    };
    println!("{rendered}");
    Ok(report.exit_code())
}

pub(crate) async fn build_report(opts: &DoctorOptions) -> Report {
    let working_dir = opts
        .working_dir
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let project_root = detect_project_root(&working_dir);
    let mode = if project_root.is_some() {
        "project"
    } else {
        "global"
    }
    .to_string();

    let mut sections = vec![
        section_configuration(&working_dir),
        section_hooks(&working_dir),
        section_skills(&working_dir),
        section_agent_profiles(&working_dir),
        section_slash_commands(&working_dir),
        section_mcp_servers(&working_dir),
        section_authentication(),
    ];

    let summary = summarize(&sections);
    Report {
        project_root: project_root.or_else(|| canonicalize_best_effort(&working_dir).ok()),
        mode,
        sections: std::mem::take(&mut sections),
        summary,
    }
}

pub(crate) fn render_json(report: &Report) -> anyhow::Result<String> {
    serde_json::to_string_pretty(report).context("serialize doctor report")
}

pub(crate) fn render_human(report: &Report, tty: bool, quiet: bool) -> String {
    let mut out = String::new();
    let project = report
        .project_root
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<none>".to_string());
    out.push_str(&format!("Project: {project}\n"));
    out.push_str(&format!("Mode: {}\n", report.mode));

    for section in &report.sections {
        let visible: Vec<&Item> = section
            .items
            .iter()
            .filter(|item| !quiet || matches!(item.status.as_str(), "warn" | "error"))
            .collect();
        if quiet && visible.is_empty() {
            continue;
        }
        out.push('\n');
        if tty {
            out.push_str(&format!("\x1b[1m{}\x1b[0m\n", section.name));
        } else {
            out.push_str(&format!("{}\n", section.name));
        }
        for item in visible {
            let marker = status_marker(&item.status, tty);
            out.push_str("  ");
            out.push_str(&marker);
            out.push(' ');
            out.push_str(&item.label);
            if let Some(detail) = &item.detail {
                if !detail.is_empty() {
                    out.push_str(": ");
                    out.push_str(detail);
                }
            }
            out.push('\n');
        }
    }

    out.push_str(&format!(
        "\nSummary: {} warning{}, {} error{}. {}\n",
        report.summary.warn,
        plural(report.summary.warn),
        report.summary.error,
        plural(report.summary.error),
        if report.summary.error > 0 {
            "Harness has errors."
        } else if report.summary.warn > 0 {
            "Harness is functional with warnings."
        } else {
            "Harness is functional."
        }
    ));
    out.trim_end().to_string()
}

fn status_marker(status: &str, tty: bool) -> String {
    match (status, tty) {
        ("ok", true) => "\x1b[32m✓\x1b[0m".to_string(),
        ("warn", true) => "\x1b[33m⚠\x1b[0m".to_string(),
        ("error", true) => "\x1b[31m✗\x1b[0m".to_string(),
        ("info", true) => "\x1b[36m→\x1b[0m".to_string(),
        ("ok", false) => "[OK]".to_string(),
        ("warn", false) => "[WARN]".to_string(),
        ("error", false) => "[ERROR]".to_string(),
        _ => "[INFO]".to_string(),
    }
}

fn plural(n: u32) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn summarize(sections: &[Section]) -> Summary {
    let mut summary = Summary::default();
    for item in sections.iter().flat_map(|section| &section.items) {
        match item.status.as_str() {
            "ok" => summary.ok += 1,
            "warn" => summary.warn += 1,
            "error" => summary.error += 1,
            _ => {}
        }
    }
    summary
}

fn section_configuration(working_dir: &Path) -> Section {
    let mut items = Vec::new();

    match Config::path() {
        Some(path) if path.exists() => match parse_config_file(&path) {
            Ok(_) => items.push(ok("Global config", format!("{} (valid)", path.display()))),
            Err(err) => items.push(error(
                "Global config",
                format!("{} ({err})", path.display()),
            )),
        },
        Some(path) => items.push(warn(
            "Global config",
            format!("{} missing (optional)", path.display()),
        )),
        None => items.push(warn(
            "Global config",
            "could not resolve ~/.jcode/config.toml",
        )),
    }

    let project_config = working_dir.join(".jcode/config.toml");
    if project_config.exists() {
        match parse_config_file(&project_config) {
            Ok(_) => items.push(ok(
                "Project config",
                format!("{} (valid)", relative_display(working_dir, &project_config)),
            )),
            Err(err) => items.push(error(
                "Project config",
                format!("{} ({err})", relative_display(working_dir, &project_config)),
            )),
        }
    } else {
        items.push(warn(
            "Project config",
            format!("{} missing", relative_display(working_dir, &project_config)),
        ));
    }

    let project_local_config = working_dir.join(".jcode/config.local.toml");
    if project_local_config.exists() {
        match parse_config_file(&project_local_config) {
            Ok(_) => items.push(ok(
                "Project local config",
                format!(
                    "{} (valid)",
                    relative_display(working_dir, &project_local_config)
                ),
            )),
            Err(err) => items.push(error(
                "Project local config",
                format!(
                    "{} ({err})",
                    relative_display(working_dir, &project_local_config)
                ),
            )),
        }
    } else {
        items.push(info(
            "Project local config",
            format!(
                "{} missing (optional)",
                relative_display(working_dir, &project_local_config)
            ),
        ));
    }

    Section {
        name: "Configuration".to_string(),
        items,
    }
}

fn section_hooks(working_dir: &Path) -> Section {
    let mut items = Vec::new();
    match std::panic::catch_unwind(|| {
        load_global_config_for_doctor().hooks_for_working_dir(Some(working_dir))
    }) {
        Ok(hooks) => {
            if hooks.commands.is_empty() {
                items.push(info("Hooks", "0 commands declared"));
            } else {
                for hook in hooks.commands {
                    let tool = hook.tool.as_deref().unwrap_or("*");
                    let label = format!("{} [{}]", hook.event, tool);
                    let command = hook.command.trim();
                    if command.is_empty() {
                        items.push(warn(label, "empty command"));
                    } else if should_skip_executable_check(command) {
                        items.push(info(
                            label,
                            format!("{command} (shell command, not linted)"),
                        ));
                    } else {
                        let path = resolve_command_path(working_dir, command);
                        if path.is_file() && is_executable(&path) {
                            items.push(ok(label, format!("{} (executable)", path.display())));
                        } else if path.is_file() {
                            items
                                .push(warn(label, format!("{} is not executable", path.display())));
                        } else {
                            items.push(warn(label, format!("{} not found", path.display())));
                        }
                    }
                }
            }
        }
        Err(_) => items.push(error("Hooks", "failed to load hook configuration")),
    }
    Section {
        name: "Hooks".to_string(),
        items,
    }
}

fn section_skills(working_dir: &Path) -> Section {
    let mut items = Vec::new();
    match SkillRegistry::load_for_working_dir(Some(working_dir)) {
        Ok(registry) => {
            let mut skills = registry.list();
            skills.sort_by(|a, b| a.name.cmp(&b.name));
            if skills.is_empty() {
                if has_any_dir(
                    working_dir,
                    &[
                        ".jcode/skills",
                        ".claude/skills",
                        ".agents/skills",
                        ".opencode/skills",
                    ],
                ) {
                    items.push(warn("Skills", "0 loaded despite project skill directories"));
                } else {
                    items.push(info("Skills", "0 loaded"));
                }
            } else {
                for skill in skills {
                    items.push(ok(
                        skill.name.clone(),
                        format!("{}", relative_display(working_dir, &skill.path)),
                    ));
                }
            }
        }
        Err(err) => items.push(error("Skills", format!("failed to load skills: {err}"))),
    }
    Section {
        name: "Skills".to_string(),
        items,
    }
}

fn section_agent_profiles(working_dir: &Path) -> Section {
    let mut items = Vec::new();
    let sources = agent_profile_sources(working_dir);
    let mut definitions: BTreeMap<String, Vec<AgentDefinition>> = BTreeMap::new();
    for source in sources {
        for (name, profile) in source.profiles {
            definitions.entry(name).or_default().push(AgentDefinition {
                source: source.label.clone(),
                profile,
            });
        }
    }

    if definitions.is_empty() {
        items.push(info("Agent profiles", "0 loaded"));
    } else {
        for (name, defs) in &definitions {
            let winner = defs.last().expect("definition exists");
            items.push(ok(name.clone(), format!("origin: {}", winner.source)));
            if defs.len() > 1 {
                let all = defs
                    .iter()
                    .map(|d| d.source.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                items.push(warn(
                    format!("Conflict resolved: \"{name}\""),
                    format!("{} overrides {all}", winner.source),
                ));
            }
            if winner.profile.model.is_none() && winner.profile.prompt.is_none() {
                items.push(warn(
                    format!("Profile \"{name}\""),
                    "has neither model nor prompt",
                ));
            }
        }
    }

    Section {
        name: "Agent profiles".to_string(),
        items,
    }
}

fn section_slash_commands(working_dir: &Path) -> Section {
    let mut items = Vec::new();
    let commands = project_commands::load_all_commands(Some(working_dir));
    let mut names: Vec<_> = commands.keys().cloned().collect();
    names.sort();
    if names.is_empty() {
        items.push(info("Slash commands", "0 loaded"));
    } else {
        for name in &names {
            if let Some(command) = commands.get(name) {
                items.push(ok(
                    format!("/{name}"),
                    format!(
                        "origin: {}",
                        relative_display(working_dir, &command.source_path)
                    ),
                ));
            }
        }
    }

    let builtins = crate::tui::registered_command_names();
    for name in &names {
        let slash = format!("/{name}");
        if builtins.contains(&slash.as_str()) {
            items.push(warn(
                format!("/{name}"),
                "collides with built-in slash command; built-in wins",
            ));
        }
    }

    if let Ok(registry) = SkillRegistry::load_for_working_dir(Some(working_dir)) {
        let skill_names: BTreeSet<_> = registry
            .list()
            .into_iter()
            .map(|s| s.name.clone())
            .collect();
        for name in &names {
            if skill_names.contains(name) {
                items.push(warn(
                    format!("/{name}"),
                    "collides with skill name; skill wins",
                ));
            }
        }
    }

    Section {
        name: "Slash commands".to_string(),
        items,
    }
}

fn section_mcp_servers(working_dir: &Path) -> Section {
    let mut items = Vec::new();
    let _guard = CurrentDirGuard::enter(working_dir);
    let mcp = McpConfig::load();
    if mcp.servers.is_empty() {
        items.push(info("MCP servers", "0 declared"));
    } else {
        let mut servers: Vec<_> = mcp.servers.into_iter().collect();
        servers.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, server) in servers {
            let command = server.command.trim();
            if command.contains("${") {
                items.push(info(
                    format!("\"{name}\""),
                    format!("{command} uses template var, will resolve at runtime"),
                ));
            } else {
                let path = Path::new(command);
                if path.is_absolute() {
                    if path.is_file() && is_executable(path) {
                        items.push(ok(format!("\"{name}\""), format!("{command} executable")));
                    } else if path.exists() {
                        items.push(warn(
                            format!("\"{name}\""),
                            format!("{command} is not executable"),
                        ));
                    } else {
                        items.push(warn(format!("\"{name}\""), format!("{command} not found")));
                    }
                } else {
                    items.push(info(
                        format!("\"{name}\""),
                        format!("{command} relies on PATH (config lint only, not runtime check)"),
                    ));
                }
            }
        }
    }
    Section {
        name: "MCP servers".to_string(),
        items,
    }
}

fn section_authentication() -> Section {
    Section {
        name: "Authentication".to_string(),
        items: vec![info(
            "Authentication",
            "run `jcode auth doctor` for detailed auth diagnosis",
        )],
    }
}

#[derive(Clone)]
struct AgentSource {
    label: String,
    profiles: BTreeMap<String, AgentRouteConfig>,
}

struct AgentDefinition {
    source: String,
    profile: AgentRouteConfig,
}

fn agent_profile_sources(working_dir: &Path) -> Vec<AgentSource> {
    let mut sources = Vec::new();
    sources.push(AgentSource {
        label: "Global TOML".to_string(),
        profiles: load_global_config_for_doctor()
            .agents
            .profiles
            .into_iter()
            .collect(),
    });
    sources.push(AgentSource {
        label: "Global .md (~/.jcode/agents)".to_string(),
        profiles: agent_profiles_md::load_global_jcode_agent_md(),
    });

    for (label, dir) in [
        ("Project .md (.opencode/agents)", ".opencode/agents"),
        ("Project .md (.agents/agents)", ".agents/agents"),
        ("Project .md (.claude/agents)", ".claude/agents"),
        ("Project .md (.jcode/agents)", ".jcode/agents"),
    ] {
        let path = working_dir.join(dir);
        sources.push(AgentSource {
            label: label.to_string(),
            profiles: agent_profiles_md::load_agents_from_dir(&path),
        });
    }

    for (label, path) in [
        (
            "Project TOML (.jcode/config.toml)",
            working_dir.join(".jcode/config.toml"),
        ),
        (
            "Project local TOML (.jcode/config.local.toml)",
            working_dir.join(".jcode/config.local.toml"),
        ),
    ] {
        if let Ok(cfg) = parse_config_file(&path) {
            sources.push(AgentSource {
                label: label.to_string(),
                profiles: cfg.agents.profiles.into_iter().collect(),
            });
        }
    }

    sources
}

fn load_global_config_for_doctor() -> Config {
    Config::path()
        .filter(|path| path.exists())
        .and_then(|path| parse_config_file(&path).ok())
        .unwrap_or_default()
}

fn parse_config_file(path: &Path) -> anyhow::Result<Config> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str::<Config>(&content).with_context(|| "failed to parse TOML")
}

fn detect_project_root(working_dir: &Path) -> Option<PathBuf> {
    let start = if working_dir.is_file() {
        working_dir.parent().unwrap_or(working_dir)
    } else {
        working_dir
    };
    for ancestor in start.ancestors() {
        if [".jcode", ".claude", ".agents", ".opencode"]
            .iter()
            .any(|dir| ancestor.join(dir).exists())
        {
            return canonicalize_best_effort(ancestor).ok();
        }
    }
    None
}

fn canonicalize_best_effort(path: &Path) -> anyhow::Result<PathBuf> {
    path.canonicalize().or_else(|_| Ok(path.to_path_buf()))
}

fn has_any_dir(working_dir: &Path, rels: &[&str]) -> bool {
    rels.iter().any(|rel| working_dir.join(rel).is_dir())
}

fn resolve_command_path(working_dir: &Path, command: &str) -> PathBuf {
    let first = command.split_whitespace().next().unwrap_or(command);
    let path = Path::new(first);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        working_dir.join(path)
    }
}

fn should_skip_executable_check(command: &str) -> bool {
    let first = command.split_whitespace().next().unwrap_or(command);
    if first.contains('/') || first.starts_with('.') {
        return false;
    }
    matches!(
        first,
        "echo" | "printf" | "true" | "false" | "test" | "[" | "cd" | "pwd" | "export"
    )
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn relative_display(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn ok(label: impl Into<String>, detail: impl Into<String>) -> Item {
    Item {
        status: "ok".to_string(),
        label: label.into(),
        detail: Some(detail.into()),
    }
}

fn warn(label: impl Into<String>, detail: impl Into<String>) -> Item {
    Item {
        status: "warn".to_string(),
        label: label.into(),
        detail: Some(detail.into()),
    }
}

fn error(label: impl Into<String>, detail: impl Into<String>) -> Item {
    Item {
        status: "error".to_string(),
        label: label.into(),
        detail: Some(detail.into()),
    }
}

fn info(label: impl Into<String>, detail: impl Into<String>) -> Item {
    Item {
        status: "info".to_string(),
        label: label.into(),
        detail: Some(detail.into()),
    }
}

struct CurrentDirGuard {
    previous: PathBuf,
}

impl CurrentDirGuard {
    fn enter(path: &Path) -> Option<Self> {
        let previous = std::env::current_dir().ok()?;
        if std::env::set_current_dir(path).is_ok() {
            Some(Self { previous })
        } else {
            None
        }
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.previous);
    }
}
