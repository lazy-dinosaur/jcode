//! Project-local slash command discovery and dispatch.
//!
//! Loads markdown files from `<project>/.jcode/commands/*.md` and the parallel
//! ecosystem dirs (`.claude/commands`, `.agents/commands`, `.opencode/commands`),
//! exposing them as slash commands the user can invoke as `/<name>`.
//!
//! When invoked, the .md body becomes the user's prompt (with `$ARGUMENTS`
//! substitution if the user passed args).
//!
//! Per policy: project-local only, no global discovery.

use serde_yaml::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(not(test))]
use std::sync::OnceLock;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct ProjectCommand {
    pub name: String,
    pub description: Option<String>,
    pub argument_hint: Option<String>,
    pub model: Option<String>,
    pub allowed_tools: Vec<String>,
    pub body: String,
    pub source_path: PathBuf,
}

impl ProjectCommand {
    /// Render the body with `$ARGUMENTS` substitution.
    /// If the body does not contain `$ARGUMENTS` and args is non-empty,
    /// append the args to the end as a separate line.
    pub fn render(&self, args: &str) -> String {
        if self.body.contains("$ARGUMENTS") {
            self.body.replace("$ARGUMENTS", args)
        } else if args.is_empty() {
            self.body.clone()
        } else {
            format!("{}\n\n{}", self.body, args)
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ProjectCommandRegistry {
    commands: BTreeMap<String, ProjectCommand>,
}

impl ProjectCommandRegistry {
    pub fn shared_registry() -> Arc<RwLock<Self>> {
        #[cfg(test)]
        {
            Arc::new(RwLock::new(Self::load_for_working_dir(None)))
        }

        #[cfg(not(test))]
        {
            static SHARED: OnceLock<Arc<RwLock<ProjectCommandRegistry>>> = OnceLock::new();
            SHARED
                .get_or_init(|| Arc::new(RwLock::new(Self::load_for_working_dir(None))))
                .clone()
        }
    }

    pub fn shared_snapshot() -> Arc<Self> {
        #[cfg(test)]
        {
            Arc::new(Self::load_for_working_dir(None))
        }

        #[cfg(not(test))]
        {
            if let Ok(commands) = Self::shared_registry().try_read() {
                Arc::new(commands.clone())
            } else {
                Arc::new(Self::load_for_working_dir(None))
            }
        }
    }

    pub fn load_for_working_dir(working_dir: Option<&Path>) -> Self {
        Self {
            commands: load_project_local_commands(working_dir),
        }
    }

    pub fn reload_all_for_working_dir(&mut self, working_dir: Option<&Path>) -> usize {
        self.commands = load_project_local_commands(working_dir);
        self.commands.len()
    }

    pub fn get(&self, name: &str) -> Option<&ProjectCommand> {
        self.commands.get(name)
    }

    pub fn list(&self) -> Vec<&ProjectCommand> {
        self.commands.values().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

/// Parse a single command .md file. Tolerant: malformed frontmatter falls back
/// to no frontmatter, body only. Returns None if file is unreadable or the file
/// stem is not a valid slash command name.
pub fn parse_command_md_file(path: &Path) -> Option<ProjectCommand> {
    let name = file_stem_name(path)?;
    if !is_valid_command_name(&name) {
        crate::logging::warn(&format!(
            "Skipping project command with invalid filename {}: command names may only contain ASCII letters, digits, '-' and '_'",
            path.display()
        ));
        return None;
    }

    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) => {
            crate::logging::warn(&format!(
                "Failed to read project command markdown file {}: {err}",
                path.display()
            ));
            return None;
        }
    };

    let (frontmatter, body) = match crate::agent_profiles_md::split_frontmatter(&content) {
        Ok(Some((yaml, body))) => match serde_yaml::from_str::<Value>(yaml) {
            Ok(value) => (Some(value), body.trim().to_string()),
            Err(err) => {
                crate::logging::warn(&format!(
                    "Failed to parse project command markdown frontmatter {}: {err}; loading as plain markdown",
                    path.display()
                ));
                (None, content.trim().to_string())
            }
        },
        Ok(None) => (None, content.trim().to_string()),
        Err(err) => {
            crate::logging::warn(&format!(
                "Failed to split project command markdown frontmatter {}: {err}; loading as plain markdown",
                path.display()
            ));
            (None, content.trim().to_string())
        }
    };

    let description = frontmatter
        .as_ref()
        .and_then(|value| crate::agent_profiles_md::string_field(value, &["description", "desc"]));
    let argument_hint = frontmatter.as_ref().and_then(|value| {
        crate::agent_profiles_md::string_field(value, &["argument-hint", "argument_hint", "args"])
    });
    let model = frontmatter
        .as_ref()
        .and_then(|value| crate::agent_profiles_md::string_field(value, &["model"]));
    let allowed_tools = frontmatter
        .as_ref()
        .map(|value| {
            crate::agent_profiles_md::string_list_field(
                value,
                &["allowed-tools", "allowed_tools", "tools"],
            )
        })
        .unwrap_or_default();

    Some(ProjectCommand {
        name,
        description,
        argument_hint,
        model,
        allowed_tools,
        body,
        source_path: path.to_path_buf(),
    })
}

/// Load all *.md files from a directory into a name->command map.
/// Per-file errors are logged at warn and skipped.
pub fn load_commands_from_dir(dir: &Path) -> BTreeMap<String, ProjectCommand> {
    let mut commands = BTreeMap::new();
    if !dir.is_dir() {
        return commands;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            crate::logging::warn(&format!(
                "Failed to read project command directory {}: {err}",
                dir.display()
            ));
            return commands;
        }
    };

    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| match entry {
            Ok(entry) => Some(entry.path()),
            Err(err) => {
                crate::logging::warn(&format!(
                    "Failed to read project command directory entry {}: {err}",
                    dir.display()
                ));
                None
            }
        })
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        })
        .collect();
    files.sort();

    for path in files {
        if let Some(command) = parse_command_md_file(&path) {
            commands.insert(command.name.clone(), command);
        }
    }

    commands
}

/// Load project-local commands from the four ecosystem dirs.
/// Priority: `.jcode > .claude > .agents > .opencode`.
pub fn load_project_local_commands(working_dir: Option<&Path>) -> BTreeMap<String, ProjectCommand> {
    let Some(working_dir) = working_dir else {
        return BTreeMap::new();
    };

    let start = if working_dir.is_file() {
        working_dir.parent().unwrap_or(working_dir)
    } else {
        working_dir
    };
    let project_dir = find_project_command_dir(start).unwrap_or(start);

    let mut commands = BTreeMap::new();
    for relative in [
        Path::new(".opencode").join("commands"),
        Path::new(".agents").join("commands"),
        Path::new(".claude").join("commands"),
        Path::new(".jcode").join("commands"),
    ] {
        commands.extend(load_commands_from_dir(&project_dir.join(relative)));
    }
    commands
}

pub fn parse_project_command_invocation(input: &str) -> Option<(&str, &str)> {
    let trimmed = input.trim();
    let rest = trimmed.strip_prefix('/')?;
    let mut parts = rest.splitn(2, char::is_whitespace);
    let name = parts.next()?.trim();
    if name.is_empty() || !is_valid_command_name(name) {
        return None;
    }
    let args = parts.next().unwrap_or("").trim();
    Some((name, args))
}

fn find_project_command_dir(start: &Path) -> Option<&Path> {
    start.ancestors().find(|ancestor| {
        command_dirs_for_project(ancestor)
            .iter()
            .any(|dir| dir.is_dir())
    })
}

fn command_dirs_for_project(project_dir: &Path) -> [PathBuf; 4] {
    [
        project_dir.join(".opencode").join("commands"),
        project_dir.join(".agents").join("commands"),
        project_dir.join(".claude").join("commands"),
        project_dir.join(".jcode").join("commands"),
    ]
}

fn file_stem_name(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .map(str::to_string)
}

fn is_valid_command_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn parse_command_md_file_no_frontmatter_uses_body() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("release.md");
        write(&path, "Run release workflow.");

        let command = parse_command_md_file(&path).unwrap();
        assert_eq!(command.name, "release");
        assert_eq!(command.body, "Run release workflow.");
        assert_eq!(command.description, None);
    }

    #[test]
    fn parse_command_md_file_with_frontmatter() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("release.md");
        write(
            &path,
            "---\ndescription: Run release\nargument-hint: '<version>'\nallowed-tools:\n  - read\nmodel: opus\n---\n\nRun release workflow.",
        );

        let command = parse_command_md_file(&path).unwrap();
        assert_eq!(command.name, "release");
        assert_eq!(command.description.as_deref(), Some("Run release"));
        assert_eq!(command.argument_hint.as_deref(), Some("<version>"));
        assert_eq!(command.allowed_tools, vec!["read"]);
        assert_eq!(command.model.as_deref(), Some("opus"));
        assert_eq!(command.body, "Run release workflow.");
    }

    #[test]
    fn parse_command_md_file_aliases_resolve() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("release.md");
        write(
            &path,
            "---\ndesc: Run release\nargument_hint: '<version>'\ntools: bash\n---\n\nRun release workflow.",
        );

        let command = parse_command_md_file(&path).unwrap();
        assert_eq!(command.description.as_deref(), Some("Run release"));
        assert_eq!(command.argument_hint.as_deref(), Some("<version>"));
        assert_eq!(command.allowed_tools, vec!["bash"]);
    }

    #[test]
    fn parse_command_md_file_unknown_fields_ignored() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("release.md");
        write(
            &path,
            "---\nmodel: opus\nunknown-field: foo\n---\n\nRun release workflow.",
        );

        let command = parse_command_md_file(&path).unwrap();
        assert_eq!(command.model.as_deref(), Some("opus"));
        assert_eq!(command.body, "Run release workflow.");
    }

    #[test]
    fn parse_command_md_file_invalid_frontmatter_falls_back_to_body() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("release.md");
        let content = "---\ndescription: [unterminated\n---\n\nRun release workflow.";
        write(&path, content);

        let command = parse_command_md_file(&path).unwrap();
        assert_eq!(command.name, "release");
        assert_eq!(command.body, content.trim());
        assert_eq!(command.description, None);
    }

    #[test]
    fn load_commands_from_dir_loads_multiple() {
        let temp = tempdir().unwrap();
        write(&temp.path().join("a.md"), "A");
        write(&temp.path().join("b.md"), "B");
        write(&temp.path().join("c.md"), "C");

        let commands = load_commands_from_dir(temp.path());
        assert_eq!(commands.len(), 3);
        assert!(commands.contains_key("a"));
        assert!(commands.contains_key("b"));
        assert!(commands.contains_key("c"));
    }

    #[test]
    fn load_project_local_commands_priority() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(".opencode/commands/x.md"),
            "from-opencode",
        );
        write(&temp.path().join(".jcode/commands/x.md"), "from-jcode");

        let commands = load_project_local_commands(Some(temp.path()));
        assert_eq!(commands["x"].body, "from-jcode");
    }

    #[test]
    fn load_project_local_commands_only_from_project_dirs() {
        let home = tempdir().unwrap();
        write(&home.path().join(".claude/commands/leaked.md"), "leaked");
        let project = tempdir().unwrap();

        let commands = load_project_local_commands(Some(project.path()));
        assert!(!commands.contains_key("leaked"));
    }

    #[test]
    fn command_render_substitutes_arguments() {
        let command = ProjectCommand {
            name: "release".to_string(),
            description: None,
            argument_hint: None,
            model: None,
            allowed_tools: Vec::new(),
            body: "Build version $ARGUMENTS now.".to_string(),
            source_path: PathBuf::new(),
        };
        assert_eq!(command.render("v1.2.3"), "Build version v1.2.3 now.");
    }

    #[test]
    fn command_render_appends_args_when_no_placeholder() {
        let command = ProjectCommand {
            name: "release".to_string(),
            description: None,
            argument_hint: None,
            model: None,
            allowed_tools: Vec::new(),
            body: "Run release workflow.".to_string(),
            source_path: PathBuf::new(),
        };
        assert_eq!(command.render("v1.2.3"), "Run release workflow.\n\nv1.2.3");
    }

    #[test]
    fn command_render_no_args_no_placeholder() {
        let command = ProjectCommand {
            name: "release".to_string(),
            description: None,
            argument_hint: None,
            model: None,
            allowed_tools: Vec::new(),
            body: "Run release workflow.".to_string(),
            source_path: PathBuf::new(),
        };
        assert_eq!(command.render(""), "Run release workflow.");
    }

    #[test]
    fn command_name_filename_stem_only() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("electron-explorer.md");
        write(&path, "Explore Electron.");

        let command = parse_command_md_file(&path).unwrap();
        assert_eq!(command.name, "electron-explorer");
    }
}
