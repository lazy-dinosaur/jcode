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
}
