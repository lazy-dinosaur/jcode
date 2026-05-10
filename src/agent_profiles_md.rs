use crate::config::AgentRouteConfig;
use anyhow::Result;
use serde_yaml::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Parse a single agent .md file into (name, AgentRouteConfig).
/// Returns Err if file is unreadable or has unclosed frontmatter.
pub fn parse_agent_md_file(path: &Path) -> Result<(String, AgentRouteConfig)> {
    let content = std::fs::read_to_string(path)?;
    let fallback_name = file_stem_name(path);

    let (frontmatter, body) = match split_frontmatter(&content) {
        Ok(Some((yaml, body))) => match serde_yaml::from_str::<Value>(yaml) {
            Ok(value) => (Some(value), body.trim().to_string()),
            Err(err) => {
                crate::logging::warn(&format!(
                    "Failed to parse agent markdown frontmatter {}: {err}; loading as plain markdown",
                    path.display()
                ));
                (None, content.trim().to_string())
            }
        },
        Ok(None) => (None, content.trim().to_string()),
        Err(err) => anyhow::bail!(err),
    };

    let mut config = AgentRouteConfig::default();
    let name = frontmatter
        .as_ref()
        .and_then(|value| string_field(value, &["name"]))
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(fallback_name);

    if let Some(value) = frontmatter.as_ref() {
        config.model = string_field(value, &["model"]);
        config.effort = string_field(value, &["effort", "reasoning-effort", "reasoning_effort"]);
        config.variant = string_field(value, &["variant"]);
        config.description = string_field(value, &["description", "desc"]);
        config.when = string_list_field(value, &["when", "when_to_use"]);
        config.prompt = string_field(value, &["system-prompt", "system_prompt"])
            .or_else(|| non_empty_string(body.clone()));
    } else {
        config.prompt = non_empty_string(body);
    }

    Ok((name, config))
}

/// Load all agent .md files from a directory.
/// Tolerant: per-file errors are logged at warn and the file is skipped.
/// Returns BTreeMap<String, AgentRouteConfig> in deterministic order.
pub fn load_agents_from_dir(dir: &Path) -> BTreeMap<String, AgentRouteConfig> {
    let mut agents = BTreeMap::new();
    if !dir.is_dir() {
        return agents;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            crate::logging::warn(&format!(
                "Failed to read agent markdown directory {}: {err}",
                dir.display()
            ));
            return agents;
        }
    };

    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| match entry {
            Ok(entry) => Some(entry.path()),
            Err(err) => {
                crate::logging::warn(&format!(
                    "Failed to read agent markdown directory entry {}: {err}",
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
        match parse_agent_md_file(&path) {
            Ok((name, config)) => {
                agents.insert(name, config);
            }
            Err(err) => {
                crate::logging::warn(&format!(
                    "Failed to load agent markdown file {}: {err}",
                    path.display()
                ));
            }
        }
    }

    agents
}

/// Load agent profiles from `~/.jcode/agents/*.md`.
/// Tolerant: missing dir -> empty map; per-file errors logged at warn.
pub fn load_global_jcode_agent_md() -> BTreeMap<String, AgentRouteConfig> {
    let Ok(jcode_dir) = crate::storage::jcode_dir() else {
        return BTreeMap::new();
    };
    let agents_dir = jcode_dir.join("agents");
    if !agents_dir.is_dir() {
        return BTreeMap::new();
    }
    load_agents_from_dir(&agents_dir)
}

/// Load project-local markdown agent profiles from the four ecosystem dirs.
/// Returns merged map with project precedence: .jcode > .claude > .agents > .opencode.
pub fn load_project_local_agent_md(
    working_dir: Option<&Path>,
) -> BTreeMap<String, AgentRouteConfig> {
    let Some(working_dir) = working_dir else {
        return BTreeMap::new();
    };

    let start = if working_dir.is_file() {
        working_dir.parent().unwrap_or(working_dir)
    } else {
        working_dir
    };
    let project_dir = find_project_agent_dir(start).unwrap_or(start);

    let mut agents = BTreeMap::new();
    for relative in [
        Path::new(".opencode").join("agents"),
        Path::new(".agents").join("agents"),
        Path::new(".claude").join("agents"),
        Path::new(".jcode").join("agents"),
    ] {
        agents.extend(load_agents_from_dir(&project_dir.join(relative)));
    }
    agents
}

fn find_project_agent_dir(start: &Path) -> Option<&Path> {
    start.ancestors().find(|ancestor| {
        agent_dirs_for_project(ancestor)
            .iter()
            .any(|dir| dir.is_dir())
    })
}

fn agent_dirs_for_project(project_dir: &Path) -> [PathBuf; 4] {
    [
        project_dir.join(".opencode").join("agents"),
        project_dir.join(".agents").join("agents"),
        project_dir.join(".claude").join("agents"),
        project_dir.join(".jcode").join("agents"),
    ]
}

pub(crate) fn split_frontmatter(content: &str) -> Result<Option<(&str, &str)>> {
    let Some(rest) = content.strip_prefix("---\n") else {
        return Ok(None);
    };
    let Some(end) = rest.find("\n---") else {
        anyhow::bail!("Unclosed YAML frontmatter");
    };
    let yaml = &rest[..end];
    let after_marker = &rest[end + "\n---".len()..];
    let body = after_marker
        .strip_prefix("\r\n")
        .or_else(|| after_marker.strip_prefix('\n'))
        .unwrap_or(after_marker);
    Ok(Some((yaml, body)))
}

fn file_stem_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("agent")
        .to_string()
}

pub(crate) fn string_field(value: &Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| value.get(*name))
        .and_then(value_to_string)
        .and_then(non_empty_string)
}

pub(crate) fn string_list_field(value: &Value, names: &[&str]) -> Vec<String> {
    let Some(value) = names.iter().find_map(|name| value.get(*name)) else {
        return Vec::new();
    };

    match value {
        Value::Sequence(items) => items
            .iter()
            .filter_map(value_to_string)
            .filter_map(non_empty_string)
            .collect(),
        _ => value_to_string(value)
            .and_then(non_empty_string)
            .into_iter()
            .collect(),
    }
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn non_empty_string(value: String) -> Option<String> {
    let value = value.trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::ffi::{OsStr, OsString};

    struct EnvVarGuard {
        key: &'static str,
        prev: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
            let prev = std::env::var_os(key);
            crate::env::set_var(key, value.as_ref());
            Self { key, prev }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(prev) = self.prev.as_ref() {
                crate::env::set_var(self.key, prev);
            } else {
                crate::env::remove_var(self.key);
            }
        }
    }

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).expect("create parent");
        std::fs::write(path, content).expect("write file");
    }

    #[test]
    fn parse_agent_md_file_accepts_yaml_parse_error_as_plain_markdown() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("broken.md");
        std::fs::write(&path, "---\nwhen: [unterminated\n---\nBody").expect("write file");

        let (name, config) = parse_agent_md_file(&path).expect("parse as plain markdown");

        assert_eq!(name, "broken");
        assert_eq!(
            config.prompt.as_deref(),
            Some("---\nwhen: [unterminated\n---\nBody")
        );
    }

    #[test]
    fn parse_agent_md_file_uses_system_prompt_over_body() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("agent.md");
        std::fs::write(
            &path,
            "---\nname: x\nsystem-prompt: Use this prompt\n---\nIgnore this body",
        )
        .expect("write file");

        let (name, config) = parse_agent_md_file(&path).expect("parse agent");

        assert_eq!(name, "x");
        assert_eq!(config.prompt.as_deref(), Some("Use this prompt"));
    }

    #[test]
    fn load_global_jcode_agent_md_returns_empty_when_no_dir() {
        let _lock = crate::storage::lock_test_env();
        let dir = tempfile::TempDir::new().expect("tempdir");
        let _home = EnvVarGuard::set("JCODE_HOME", dir.path());

        let agents = load_global_jcode_agent_md();

        assert!(agents.is_empty());
    }

    #[test]
    fn load_global_jcode_agent_md_loads_md_files() {
        let _lock = crate::storage::lock_test_env();
        let dir = tempfile::TempDir::new().expect("tempdir");
        let _home = EnvVarGuard::set("JCODE_HOME", dir.path());
        write(
            &dir.path().join("agents/reviewer.md"),
            "---\ndescription: Reviews code\nmodel: haiku\nwhen:\n  - reviewing\n---\nReview carefully.",
        );

        let agents = load_global_jcode_agent_md();
        let reviewer = agents.get("reviewer").expect("reviewer agent");

        assert_eq!(reviewer.description.as_deref(), Some("Reviews code"));
        assert_eq!(reviewer.model.as_deref(), Some("haiku"));
        assert_eq!(reviewer.when, vec!["reviewing"]);
        assert_eq!(reviewer.prompt.as_deref(), Some("Review carefully."));
    }

    #[test]
    fn agents_for_working_dir_global_md_overrides_global_toml() {
        let _lock = crate::storage::lock_test_env();
        let dir = tempfile::TempDir::new().expect("tempdir");
        let _home = EnvVarGuard::set("JCODE_HOME", dir.path());
        write(
            &dir.path().join("config.toml"),
            "[agents.profiles.x]\nmodel = \"opus\"\n",
        );
        write(
            &dir.path().join("agents/x.md"),
            "---\nmodel: haiku\n---\nGlobal markdown prompt.",
        );

        let agents = Config::load().agents_for_working_dir(None);

        assert_eq!(agents.profiles["x"].model.as_deref(), Some("haiku"));
    }

    #[test]
    fn agents_for_working_dir_project_md_overrides_global_md() {
        let _lock = crate::storage::lock_test_env();
        let home = tempfile::TempDir::new().expect("tempdir");
        let _home = EnvVarGuard::set("JCODE_HOME", home.path());
        let project = tempfile::TempDir::new().expect("project tempdir");
        write(
            &home.path().join("agents/x.md"),
            "---\nmodel: global-md\n---\nGlobal prompt.",
        );
        write(
            &project.path().join(".jcode/agents/x.md"),
            "---\nmodel: project-md\n---\nProject prompt.",
        );

        let agents = Config::load().agents_for_working_dir(Some(project.path()));

        assert_eq!(agents.profiles["x"].model.as_deref(), Some("project-md"));
    }

    #[test]
    fn agents_for_working_dir_project_toml_overrides_global_md() {
        let _lock = crate::storage::lock_test_env();
        let home = tempfile::TempDir::new().expect("tempdir");
        let _home = EnvVarGuard::set("JCODE_HOME", home.path());
        let project = tempfile::TempDir::new().expect("project tempdir");
        write(
            &home.path().join("agents/x.md"),
            "---\nmodel: global-md\n---\nGlobal prompt.",
        );
        write(
            &project.path().join(".jcode/config.toml"),
            "[agents.profiles.x]\nmodel = \"project-toml\"\n",
        );

        let agents = Config::load().agents_for_working_dir(Some(project.path()));

        assert_eq!(agents.profiles["x"].model.as_deref(), Some("project-toml"));
    }
}
