use super::{external_tool_manual_command, parse_manual_subagent_spec, resolve_nvim_target};

#[test]
fn parse_manual_subagent_spec_accepts_flags_and_prompt() {
    let spec = parse_manual_subagent_spec(
        "--type research --model gpt-5.4 --continue session_123 investigate this bug",
    )
    .expect("parse manual subagent spec");

    assert_eq!(spec.subagent_type, "research");
    assert_eq!(spec.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(spec.session_id.as_deref(), Some("session_123"));
    assert_eq!(spec.prompt, "investigate this bug");
}

#[test]
fn parse_manual_subagent_spec_rejects_missing_prompt() {
    let err = parse_manual_subagent_spec("--model gpt-5.4")
        .expect_err("missing prompt should be rejected");
    assert!(err.contains("Missing prompt"));
}

#[test]
fn resolve_nvim_target_accepts_existing_and_new_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let existing = dir.path().join("existing.rs");
    std::fs::write(&existing, "fn main() {}\n").expect("write fixture");

    let resolved = resolve_nvim_target(dir.path(), "existing.rs").expect("resolve existing file");
    assert_eq!(resolved, existing.canonicalize().expect("canonical file"));

    let new_file = resolve_nvim_target(dir.path(), "new.rs").expect("resolve new file");
    assert_eq!(new_file, dir.path().join("new.rs"));

    let missing_parent = resolve_nvim_target(dir.path(), "missing/new.rs")
        .expect_err("missing parent should be rejected");
    assert!(missing_parent.contains("parent directory does not exist"));
}

#[test]
#[cfg(unix)]
fn external_tool_manual_command_quotes_cwd_and_args() {
    let command = external_tool_manual_command(
        "nvim",
        &["it's ok.rs".to_string()],
        std::path::Path::new("/tmp/jcode cwd"),
    );

    assert_eq!(command, "cd '/tmp/jcode cwd' && 'nvim' 'it'\"'\"'s ok.rs'");
}
