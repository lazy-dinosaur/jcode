use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CwdCommand {
    Show,
    Set { path: String },
}

pub fn parse_cwd_command(trimmed: &str) -> Option<Result<CwdCommand, String>> {
    if trimmed == "/pwd" || trimmed == "/cwd" || trimmed == "/cd" {
        return Some(Ok(CwdCommand::Show));
    }

    for prefix in ["/cwd", "/cd"] {
        let Some(rest) = trimmed.strip_prefix(prefix) else {
            continue;
        };
        if !rest.chars().next().is_some_and(char::is_whitespace) {
            continue;
        }
        let path = rest.trim();
        return Some(if path.is_empty() {
            Err(cwd_usage())
        } else {
            Ok(CwdCommand::Set {
                path: path.to_string(),
            })
        });
    }

    if trimmed.starts_with("/pwd ") {
        return Some(Err("Usage: `/pwd`".to_string()));
    }

    None
}

pub fn cwd_usage() -> String {
    "Usage: `/pwd`, `/cwd`, `/cwd <path>`, or `/cd <path>`".to_string()
}

pub fn canonical_existing_dir(path: &Path) -> Result<PathBuf> {
    if !path.exists() {
        bail!("directory does not exist: {}", path.display());
    }
    if !path.is_dir() {
        bail!("not a directory: {}", path.display());
    }
    path.canonicalize()
        .with_context(|| format!("failed to canonicalize {}", path.display()))
}

pub fn resolve_cwd_path(current_dir: Option<&Path>, raw: &str) -> Result<PathBuf> {
    let expanded;
    let raw = if raw == "~" {
        expanded = std::env::var("HOME").context("HOME is not set")?;
        expanded.as_str()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        let home = std::env::var("HOME").context("HOME is not set")?;
        expanded = format!("{}/{}", home.trim_end_matches('/'), rest);
        expanded.as_str()
    } else {
        raw
    };
    let raw_path = Path::new(raw);
    let candidate = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        current_dir.unwrap_or_else(|| Path::new(".")).join(raw_path)
    };
    canonical_existing_dir(&candidate)
}

pub fn format_cwd(current_dir: Option<&Path>) -> String {
    match current_dir {
        Some(dir) => format!("Session cwd: `{}`", dir.display()),
        None => "Session cwd: not recorded".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cwd_commands() {
        assert_eq!(
            parse_cwd_command("/pwd").unwrap().unwrap(),
            CwdCommand::Show
        );
        assert_eq!(
            parse_cwd_command("/cwd").unwrap().unwrap(),
            CwdCommand::Show
        );
        assert_eq!(parse_cwd_command("/cd").unwrap().unwrap(), CwdCommand::Show);
        assert_eq!(
            parse_cwd_command("/cwd ../foo").unwrap().unwrap(),
            CwdCommand::Set {
                path: "../foo".to_string()
            }
        );
        assert_eq!(
            parse_cwd_command("/cd /tmp").unwrap().unwrap(),
            CwdCommand::Set {
                path: "/tmp".to_string()
            }
        );
        assert!(parse_cwd_command("/cdata").is_none());
    }
}
