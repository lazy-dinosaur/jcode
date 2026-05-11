use crate::doctor::{self, DoctorOptions, Report};
use serde_json::Value;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock};

fn doctor_test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn valid_skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: {name} skill\n---\nUse {name}.\n")
}

async fn report_for(path: &Path, json: bool, quiet: bool) -> Report {
    doctor::build_report(&DoctorOptions {
        json,
        quiet,
        working_dir: Some(path.to_path_buf()),
    })
    .await
}

fn section<'a>(report: &'a Report, name: &str) -> &'a crate::doctor::Section {
    report
        .sections
        .iter()
        .find(|section| section.name == name)
        .unwrap_or_else(|| panic!("missing section {name}"))
}

#[tokio::test]
async fn test_doctor_runs_on_empty_project_no_panic() {
    let _guard = doctor_test_lock();
    let tmp = tempfile::TempDir::new().unwrap();
    let report = report_for(tmp.path(), true, false).await;
    assert!((0..=2).contains(&report.exit_code()));
    let json = doctor::render_json(&report).unwrap();
    serde_json::from_str::<Value>(&json).unwrap();
}

#[tokio::test]
async fn test_doctor_detects_invalid_project_config() {
    let _guard = doctor_test_lock();
    let tmp = tempfile::TempDir::new().unwrap();
    write(&tmp.path().join(".jcode/config.toml"), "not [valid toml");
    let report = report_for(tmp.path(), true, false).await;
    assert!(
        section(&report, "Configuration")
            .items
            .iter()
            .any(|item| item.status == "error")
    );
}

#[tokio::test]
async fn test_doctor_detects_non_executable_hook() {
    let _guard = doctor_test_lock();
    let tmp = tempfile::TempDir::new().unwrap();
    write(
        &tmp.path().join(".jcode/hooks/test.sh"),
        "#!/bin/sh\nexit 0\n",
    );
    write(
        &tmp.path().join(".jcode/config.toml"),
        r#"
[hooks]
enabled = true

[[hooks.commands]]
event = "tool.execute.before"
tool = "bash"
command = ".jcode/hooks/test.sh"
"#,
    );
    let report = report_for(tmp.path(), true, false).await;
    assert!(section(&report, "Hooks").items.iter().any(|item| {
        item.status == "warn"
            && item
                .detail
                .as_deref()
                .unwrap_or_default()
                .contains("not executable")
    }));
}

#[tokio::test]
async fn test_doctor_lists_loaded_skills() {
    let _guard = doctor_test_lock();
    let tmp = tempfile::TempDir::new().unwrap();
    write(
        &tmp.path().join(".jcode/skills/foo/SKILL.md"),
        &valid_skill_md("foo"),
    );
    let report = report_for(tmp.path(), true, false).await;
    assert!(
        section(&report, "Skills")
            .items
            .iter()
            .any(|item| item.label.contains("foo")
                || item.detail.as_deref().unwrap_or_default().contains("foo"))
    );
}

#[tokio::test]
async fn test_doctor_lists_agent_profiles_with_origin() {
    let _guard = doctor_test_lock();
    let tmp = tempfile::TempDir::new().unwrap();
    write(
        &tmp.path().join(".jcode/config.toml"),
        r#"
[agents.profiles.x]
model = "gpt-test"
prompt = "profile x"
"#,
    );
    write(
        &tmp.path().join(".jcode/agents/y.md"),
        "---\nname: y\nmodel: claude-test\n---\nprofile y\n",
    );
    let report = report_for(tmp.path(), true, false).await;
    let agent_text = section(&report, "Agent profiles")
        .items
        .iter()
        .map(|item| format!("{} {}", item.label, item.detail.clone().unwrap_or_default()))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(agent_text.contains("x"));
    assert!(agent_text.contains("Project TOML"));
    assert!(agent_text.contains("y"));
    assert!(agent_text.contains("Project .md (.jcode/agents)"));
}

#[tokio::test]
async fn test_doctor_detects_command_skill_collision() {
    let _guard = doctor_test_lock();
    let tmp = tempfile::TempDir::new().unwrap();
    write(
        &tmp.path().join(".jcode/skills/foo/SKILL.md"),
        &valid_skill_md("foo"),
    );
    write(&tmp.path().join(".jcode/commands/foo.md"), "Do foo\n");
    let report = report_for(tmp.path(), true, false).await;
    assert!(section(&report, "Slash commands").items.iter().any(|item| {
        item.status == "warn"
            && item.label.contains("foo")
            && item.detail.as_deref().unwrap_or_default().contains("skill")
    }));
}

#[tokio::test]
async fn test_doctor_detects_command_builtin_collision() {
    let _guard = doctor_test_lock();
    let tmp = tempfile::TempDir::new().unwrap();
    write(&tmp.path().join(".jcode/commands/help.md"), "Do help\n");
    let report = report_for(tmp.path(), true, false).await;
    assert!(section(&report, "Slash commands").items.iter().any(|item| {
        item.status == "warn"
            && item.label.contains("help")
            && item
                .detail
                .as_deref()
                .unwrap_or_default()
                .contains("built-in")
    }));
}

#[tokio::test]
async fn test_doctor_json_output_is_valid_json() {
    let _guard = doctor_test_lock();
    let tmp = tempfile::TempDir::new().unwrap();
    let report = report_for(tmp.path(), true, false).await;
    let json = doctor::render_json(&report).unwrap();
    let value: Value = serde_json::from_str(&json).unwrap();
    assert!(value.get("project_root").is_some());
    assert!(value.get("sections").is_some());
    assert!(value.get("summary").is_some());
}

#[tokio::test]
async fn test_doctor_quiet_omits_ok_items() {
    let _guard = doctor_test_lock();
    let tmp = tempfile::TempDir::new().unwrap();
    write(&tmp.path().join(".jcode/config.toml"), "");
    let report = report_for(tmp.path(), false, true).await;
    let rendered = doctor::render_human(&report, false, true);
    assert!(!rendered.contains("✓"));
    assert!(!rendered.contains("[OK]"));
}

#[tokio::test]
async fn test_doctor_exit_code_reflects_severity() {
    let _guard = doctor_test_lock();
    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("home");
    write(&home.join("config.toml"), "");
    let old_home = std::env::var_os("JCODE_HOME");
    crate::env::set_var("JCODE_HOME", &home);

    let healthy = tmp.path().join("healthy");
    write(&healthy.join(".jcode/config.toml"), "");
    let healthy_report = report_for(&healthy, false, false).await;
    assert_eq!(healthy_report.exit_code(), 0);

    let warned = tmp.path().join("warned");
    write(&warned.join(".jcode/hooks/test.sh"), "#!/bin/sh\n");
    write(
        &warned.join(".jcode/config.toml"),
        r#"
[hooks]
enabled = true
[[hooks.commands]]
event = "tool.execute.before"
command = ".jcode/hooks/test.sh"
"#,
    );
    let warned_report = report_for(&warned, false, false).await;
    assert_eq!(warned_report.exit_code(), 1);

    let errored = tmp.path().join("errored");
    write(&errored.join(".jcode/config.toml"), "not [valid toml");
    let errored_report = report_for(&errored, false, false).await;
    assert_eq!(errored_report.exit_code(), 2);

    if let Some(old_home) = old_home {
        crate::env::set_var("JCODE_HOME", old_home);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}
