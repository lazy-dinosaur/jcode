#[test]
fn test_project_command_appears_in_autocomplete() {
    let temp = tempfile::tempdir().unwrap();
    let command_path = temp.path().join(".jcode/commands/release.md");
    std::fs::create_dir_all(command_path.parent().unwrap()).unwrap();
    std::fs::write(
        &command_path,
        "---\ndescription: Run release\n---\n\nRun release workflow.",
    )
    .unwrap();

    let mut app = create_test_app();
    app.session.working_dir = Some(temp.path().to_string_lossy().to_string());
    app.input = "/".to_string();

    let suggestions = app.command_suggestions();
    assert!(suggestions.iter().any(|(command, help)| {
        command == "/release" && *help == "Run release [project]"
    }));
}

#[test]
fn test_project_command_dispatched_as_user_message() {
    let temp = tempfile::tempdir().unwrap();
    let command_path = temp.path().join(".jcode/commands/release.md");
    std::fs::create_dir_all(command_path.parent().unwrap()).unwrap();
    std::fs::write(&command_path, "Run release workflow.").unwrap();

    let mut app = create_test_app();
    app.session.working_dir = Some(temp.path().to_string_lossy().to_string());
    app.input = "/release v1.2.3".to_string();
    app.submit_input();

    assert_eq!(app.active_skill, None);
    assert!(app.is_processing());
    let msg = app.session.messages.last().expect("missing project command prompt");
    assert!(matches!(
        &msg.content[0],
        ContentBlock::Text { text, .. } if text == "Run release workflow.\n\nv1.2.3"
    ));
}

#[test]
fn test_builtin_command_wins_over_project_command_with_same_name() {
    let temp = tempfile::tempdir().unwrap();
    let command_path = temp.path().join(".jcode/commands/help.md");
    std::fs::create_dir_all(command_path.parent().unwrap()).unwrap();
    std::fs::write(&command_path, "project help should not run").unwrap();

    let mut app = create_test_app();
    app.session.working_dir = Some(temp.path().to_string_lossy().to_string());
    app.input = "/help".to_string();
    app.submit_input();

    assert!(!app.is_processing());
    assert!(
        !app
            .session
            .messages
            .iter()
            .any(|message| matches!(&message.content[0], ContentBlock::Text { text, .. } if text.contains("project help should not run")))
    );
}

#[test]
fn test_skill_wins_over_project_command_with_same_name() {
    let temp = tempfile::tempdir().unwrap();
    let command_path = temp.path().join(".jcode/commands/x.md");
    std::fs::create_dir_all(command_path.parent().unwrap()).unwrap();
    std::fs::write(&command_path, "project x should not run").unwrap();

    let skill_path = temp.path().join(".jcode/skills/x/SKILL.md");
    std::fs::create_dir_all(skill_path.parent().unwrap()).unwrap();
    std::fs::write(
        &skill_path,
        "---\nname: x\ndescription: Skill x\n---\n\nSkill body.",
    )
    .unwrap();

    let mut app = create_test_app();
    app.session.working_dir = Some(temp.path().to_string_lossy().to_string());
    app.input = "/x".to_string();
    app.submit_input();

    assert_eq!(app.active_skill.as_deref(), Some("x"));
    assert!(!app.is_processing());
    assert!(app.display_messages().iter().any(|message| {
        message.content.contains("Activated skill: x - Skill x")
    }));
    assert!(
        !app
            .session
            .messages
            .iter()
            .any(|message| matches!(&message.content[0], ContentBlock::Text { text, .. } if text.contains("project x should not run")))
    );
}
