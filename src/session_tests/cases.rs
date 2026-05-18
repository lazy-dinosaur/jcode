use super::*;
use anyhow::{Result, anyhow};

#[test]
fn test_session_exists_roundtrip() -> Result<()> {
    let tmp_dir = std::env::temp_dir().join(format!(
        "jcode-session-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| anyhow!(e))?
            .as_nanos()
    ));
    std::fs::create_dir_all(tmp_dir.join("sessions"))?;

    assert!(!session_path_in_dir(&tmp_dir, "missing-session").exists());

    let session_path = session_path_in_dir(&tmp_dir, "exists-session");
    std::fs::write(&session_path, "{}")?;
    assert!(session_path.exists());

    let random_id = format!(
        "missing-session-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| anyhow!(e))?
            .as_nanos()
    );
    assert!(!session_exists(&random_id));
    Ok(())
}

#[test]
fn derive_session_provider_key_prefers_runtime_identity_over_transport() {
    let _lock = lock_env();
    let _runtime = EnvVarGuard::set("JCODE_RUNTIME_PROVIDER", "azure-openai");
    let _namespace = EnvVarGuard::set("JCODE_OPENROUTER_CACHE_NAMESPACE", "azure-cache");
    let _active = EnvVarGuard::set("JCODE_ACTIVE_PROVIDER", "openrouter");

    assert_eq!(
        derive_session_provider_key("openrouter").as_deref(),
        Some("azure-openai")
    );
}

#[test]
fn derive_session_provider_key_falls_back_to_openrouter_namespace() {
    let _lock = lock_env();
    let _runtime = EnvVarGuard::remove("JCODE_RUNTIME_PROVIDER");
    let _namespace = EnvVarGuard::set("JCODE_OPENROUTER_CACHE_NAMESPACE", "azure-openai");
    let _active = EnvVarGuard::set("JCODE_ACTIVE_PROVIDER", "openrouter");

    assert_eq!(
        derive_session_provider_key("openrouter").as_deref(),
        Some("azure-openai")
    );
}

#[test]
fn derive_session_provider_key_keeps_openai_compatible_profile_namespace() {
    let _lock = lock_env();
    let _runtime = EnvVarGuard::set("JCODE_RUNTIME_PROVIDER", "openai-compatible");
    let _namespace = EnvVarGuard::set("JCODE_OPENROUTER_CACHE_NAMESPACE", "zai");
    let _active = EnvVarGuard::set("JCODE_ACTIVE_PROVIDER", "openrouter");

    assert_eq!(
        derive_session_provider_key("openrouter").as_deref(),
        Some("zai")
    );
}

#[test]
fn rename_title_preserves_generated_title_for_clear() {
    let mut session = Session::create_with_id(
        "session_rename_clear_123".to_string(),
        None,
        Some("Generated first prompt title".to_string()),
    );

    assert_eq!(
        session.display_title(),
        Some("Generated first prompt title")
    );
    session.rename_title(Some("Custom planning name".to_string()));
    assert_eq!(
        session.title.as_deref(),
        Some("Generated first prompt title")
    );
    assert_eq!(
        session.custom_title.as_deref(),
        Some("Custom planning name")
    );
    assert_eq!(session.display_title(), Some("Custom planning name"));

    session.rename_title(None);
    assert_eq!(
        session.title.as_deref(),
        Some("Generated first prompt title")
    );
    assert!(session.custom_title.is_none());
    assert_eq!(
        session.display_title(),
        Some("Generated first prompt title")
    );

    session.custom_title = Some("   ".to_string());
    assert_eq!(
        session.display_title(),
        Some("Generated first prompt title")
    );
}

#[test]
fn test_debug_memory_profile_reports_messages_and_provider_cache() {
    let mut session = Session::create_with_id(
        "session_memory_profile_test".to_string(),
        None,
        Some("Memory profile".to_string()),
    );
    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "hello world".to_string(),
            cache_control: None,
        }],
    );
    session.add_message(
        Role::Assistant,
        vec![
            ContentBlock::ToolUse {
                id: "tool_1".to_string(),
                name: "bash".to_string(),
                input: serde_json::json!({"command": "echo hi"}),
            },
            ContentBlock::ToolResult {
                tool_use_id: "tool_1".to_string(),
                content: "hi".to_string(),
                is_error: None,
            },
        ],
    );

    session.compaction = Some(StoredCompactionState {
        summary_text: "summary".to_string(),
        openai_encrypted_content: None,
        covers_up_to_turn: 7,
        original_turn_count: 9,
        compacted_count: 7,
    });

    let _ = session.provider_messages();
    let profile = session.debug_memory_profile();

    assert_eq!(profile["messages"]["count"], 2);
    assert_eq!(profile["messages"]["memory"]["text_blocks"], 1);
    assert_eq!(profile["messages"]["memory"]["tool_use_blocks"], 1);
    assert_eq!(profile["messages"]["memory"]["tool_result_blocks"], 1);
    assert!(profile["messages"]["json_bytes"].as_u64().unwrap_or(0) > 0);
    assert_eq!(profile["provider_messages_cache"]["count"], 2);
    assert_eq!(profile["compaction"]["present"], true);
    assert_eq!(profile["compaction"]["covers_up_to_turn"], 7);
    assert_eq!(profile["compaction"]["original_turn_count"], 9);
    assert_eq!(profile["compaction"]["compacted_count"], 7);
    assert!(
        profile["provider_messages_cache"]["json_bytes"]
            .as_u64()
            .unwrap_or(0)
            > 0
    );
}

#[test]
fn initial_session_context_is_persisted_once_and_not_overwritten() {
    let mut session = Session::create_with_id(
        "session_context_test".to_string(),
        None,
        Some("Session context".to_string()),
    );

    assert!(session.ensure_initial_session_context_message());
    assert_eq!(session.messages.len(), 1);
    let first = session.messages[0].content_preview();
    assert!(first.contains("# Session Context"));
    assert!(first.contains("OS:"));
    assert_eq!(
        session.messages[0].display_role,
        Some(StoredDisplayRole::System)
    );

    assert!(!session.ensure_initial_session_context_message());
    assert_eq!(session.messages.len(), 1);

    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "hello".to_string(),
            cache_control: None,
        }],
    );
    assert!(!session.ensure_initial_session_context_message());
    assert_eq!(session.messages.len(), 2);
}

#[test]
#[allow(clippy::redundant_closure_call)]
fn initial_session_context_uses_current_cwd_when_inserted() -> Result<()> {
    let _env_lock = lock_env();
    let original_cwd = std::env::current_dir().map_err(|e| anyhow!(e))?;
    let first_dir = tempfile::Builder::new()
        .prefix("jcode-session-context-first-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let second_dir = tempfile::Builder::new()
        .prefix("jcode-session-context-second-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;

    std::env::set_current_dir(first_dir.path()).map_err(|e| anyhow!(e))?;
    let mut session = Session::create_with_id(
        "session_context_cwd_refresh_test".to_string(),
        None,
        Some("Session context cwd refresh".to_string()),
    );
    assert_eq!(
        session.working_dir.as_deref(),
        Some(first_dir.path().to_str().unwrap())
    );

    std::env::set_current_dir(second_dir.path()).map_err(|e| anyhow!(e))?;
    let result: std::result::Result<(), anyhow::Error> = (|| {
        assert!(session.ensure_initial_session_context_message());
        let first = session.messages[0].content_preview();
        assert!(
            first.contains(&format!(
                "Working directory: {}",
                second_dir.path().display()
            )),
            "session context should use cwd at insertion time, got: {first}"
        );
        assert_eq!(
            session.working_dir.as_deref(),
            Some(second_dir.path().to_str().unwrap())
        );
        Ok(())
    })();
    std::env::set_current_dir(original_cwd).map_err(|e| anyhow!(e))?;
    result?;

    Ok(())
}

#[test]
#[allow(clippy::redundant_closure_call)]
fn initial_session_context_can_refresh_before_real_conversation() -> Result<()> {
    let _env_lock = lock_env();
    let original_cwd = std::env::current_dir().map_err(|e| anyhow!(e))?;
    let first_dir = tempfile::Builder::new()
        .prefix("jcode-session-context-stale-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let second_dir = tempfile::Builder::new()
        .prefix("jcode-session-context-real-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;

    std::env::set_current_dir(first_dir.path()).map_err(|e| anyhow!(e))?;
    let result: std::result::Result<(), anyhow::Error> = (|| {
        let mut session = Session::create_with_id(
            "session_context_remote_cwd_refresh_test".to_string(),
            None,
            Some("Remote cwd refresh".to_string()),
        );
        assert!(session.ensure_initial_session_context_message());
        assert!(session.messages[0].content_preview().contains(&format!(
            "Working directory: {}",
            first_dir.path().display()
        )));

        session.working_dir = Some(second_dir.path().display().to_string());
        assert!(session.refresh_initial_session_context_message());
        let refreshed = session.messages[0].content_preview();
        assert!(
            refreshed.contains(&format!(
                "Working directory: {}",
                second_dir.path().display()
            )),
            "session context should refresh to subscribed cwd, got: {refreshed}"
        );
        assert!(!refreshed.contains(&format!(
            "Working directory: {}",
            first_dir.path().display()
        )));
        Ok(())
    })();
    std::env::set_current_dir(original_cwd).map_err(|e| anyhow!(e))?;
    result?;

    Ok(())
}

#[test]
#[allow(clippy::redundant_closure_call)]
fn initial_session_context_does_not_refresh_after_real_conversation() -> Result<()> {
    let _env_lock = lock_env();
    let original_cwd = std::env::current_dir().map_err(|e| anyhow!(e))?;
    let first_dir = tempfile::Builder::new()
        .prefix("jcode-session-context-original-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let second_dir = tempfile::Builder::new()
        .prefix("jcode-session-context-late-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;

    std::env::set_current_dir(first_dir.path()).map_err(|e| anyhow!(e))?;
    let result: std::result::Result<(), anyhow::Error> = (|| {
        let mut session = Session::create_with_id(
            "session_context_late_cwd_refresh_test".to_string(),
            None,
            Some("Late cwd refresh".to_string()),
        );
        assert!(session.ensure_initial_session_context_message());
        session.add_message(
            Role::User,
            vec![ContentBlock::Text {
                text: "hello".to_string(),
                cache_control: None,
            }],
        );

        session.working_dir = Some(second_dir.path().display().to_string());
        assert!(!session.refresh_initial_session_context_message());
        let original = session.messages[0].content_preview();
        assert!(original.contains(&format!(
            "Working directory: {}",
            first_dir.path().display()
        )));
        assert!(!original.contains(&format!(
            "Working directory: {}",
            second_dir.path().display()
        )));
        Ok(())
    })();
    std::env::set_current_dir(original_cwd).map_err(|e| anyhow!(e))?;
    result?;

    Ok(())
}

#[test]
fn existing_non_empty_session_does_not_get_retroactive_session_context() {
    let mut session = Session::create_with_id(
        "session_context_existing_test".to_string(),
        None,
        Some("Existing".to_string()),
    );
    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "already started".to_string(),
            cache_control: None,
        }],
    );

    assert!(!session.ensure_initial_session_context_message());
    assert_eq!(session.messages.len(), 1);
    assert!(
        !session.messages[0]
            .content_preview()
            .contains("# Session Context")
    );
}

#[test]
fn load_startup_stub_preserves_metadata_but_skips_heavy_vectors() -> Result<()> {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-startup-stub-test-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let session_id = "session_startup_stub_roundtrip";
    let mut session = Session::create_with_id(
        session_id.to_string(),
        Some("parent_123".to_string()),
        Some("startup stub".to_string()),
    );
    session.model = Some("gpt-5.4".to_string());
    session.reasoning_effort = Some("high".to_string());
    session.provider_key = Some("openai".to_string());
    session.set_canary("self-dev");
    session.append_stored_message(StoredMessage {
        id: "msg_1".to_string(),
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: "hello world".to_string(),
            cache_control: None,
        }],
        display_role: None,
        timestamp: Some(Utc::now()),
        tool_duration_ms: None,
        token_usage: None,
    });
    session.record_env_snapshot(EnvSnapshot {
        captured_at: Utc::now(),
        reason: "resume".to_string(),
        session_id: session_id.to_string(),
        working_dir: Some(temp_home.path().to_string_lossy().to_string()),
        provider: "openai".to_string(),
        model: "gpt-5.4".to_string(),
        jcode_version: "test".to_string(),
        jcode_git_hash: Some("abc123".to_string()),
        jcode_git_dirty: Some(false),
        os: "linux".to_string(),
        arch: "x86_64".to_string(),
        pid: 123,
        is_selfdev: true,
        is_debug: false,
        is_canary: true,
        testing_build: Some("self-dev".to_string()),
        working_git: None,
    });
    session.record_memory_injection(
        "summary".to_string(),
        "content".to_string(),
        1,
        5,
        Vec::new(),
    );
    session.record_replay_display_message("system", Some("Launch".to_string()), "boot");
    session.save()?;

    let stub = Session::load_startup_stub(session_id)?;
    assert_eq!(stub.id, session_id);
    assert_eq!(stub.parent_id.as_deref(), Some("parent_123"));
    assert_eq!(stub.title.as_deref(), Some("startup stub"));
    assert_eq!(stub.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(stub.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(stub.provider_key.as_deref(), Some("openai"));
    assert!(stub.is_canary);
    assert!(stub.messages.is_empty());
    assert!(stub.env_snapshots.is_empty());
    assert!(stub.memory_injections.is_empty());
    assert!(stub.replay_events.is_empty());
    Ok(())
}

#[test]
fn load_for_remote_startup_preserves_messages_and_replay_but_skips_heavy_vectors() -> Result<()> {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-remote-startup-test-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let session_id = "session_remote_startup_roundtrip";
    let mut session = Session::create_with_id(
        session_id.to_string(),
        Some("parent_remote".to_string()),
        Some("remote startup".to_string()),
    );
    session.model = Some("gpt-5.4".to_string());
    session.reasoning_effort = Some("medium".to_string());
    session.append_stored_message(StoredMessage {
        id: "msg_remote_1".to_string(),
        role: Role::Assistant,
        content: vec![ContentBlock::Text {
            text: "hello remote startup".to_string(),
            cache_control: None,
        }],
        display_role: None,
        timestamp: Some(Utc::now()),
        tool_duration_ms: None,
        token_usage: None,
    });
    session.record_env_snapshot(EnvSnapshot {
        captured_at: Utc::now(),
        reason: "resume".to_string(),
        session_id: session_id.to_string(),
        working_dir: Some(temp_home.path().to_string_lossy().to_string()),
        provider: "openai".to_string(),
        model: "gpt-5.4".to_string(),
        jcode_version: "test".to_string(),
        jcode_git_hash: Some("abc123".to_string()),
        jcode_git_dirty: Some(false),
        os: "linux".to_string(),
        arch: "x86_64".to_string(),
        pid: 123,
        is_selfdev: false,
        is_debug: false,
        is_canary: false,
        testing_build: None,
        working_git: None,
    });
    session.record_memory_injection(
        "summary".to_string(),
        "content".to_string(),
        1,
        5,
        Vec::new(),
    );
    session.record_replay_display_message("system", Some("Launch".to_string()), "boot");
    session.save()?;

    let loaded = Session::load_for_remote_startup(session_id)?;
    assert_eq!(loaded.id, session_id);
    assert_eq!(loaded.parent_id.as_deref(), Some("parent_remote"));
    assert_eq!(loaded.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(loaded.reasoning_effort.as_deref(), Some("medium"));
    assert_eq!(loaded.messages.len(), 1);
    assert!(loaded.replay_events.is_empty());
    assert!(loaded.env_snapshots.is_empty());
    assert!(loaded.memory_injections.is_empty());
    Ok(())
}

/// Regression test for M7 (data loss on abnormal shutdown).
///
/// Scenario: server crashes / SIGKILL after journal append but before the
/// next snapshot checkpoint. Then on restart `load_for_remote_startup` could
/// fail (e.g. snapshot file got truncated mid-write, or contains an unknown
/// message variant) and the caller falls back to `load_startup_stub`. Before
/// the M7 fix that fallback returned an empty `messages` vector, silently
/// dropping every message that was only present in the journal. With the
/// fix, the stub fallback now replays the journal so messages survive.
#[test]
fn load_startup_stub_recovers_messages_from_journal() -> Result<()> {
    use crate::session::{session_journal_path, session_path};

    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-startup-stub-journal-recovery-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let session_id = "session_m7_journal_recovery";
    let mut session = Session::create_with_id(
        session_id.to_string(),
        None,
        Some("M7 recovery".to_string()),
    );
    session.model = Some("gpt-5.4".to_string());
    // Initial save -> writes a snapshot, journal stays empty.
    session.append_stored_message(StoredMessage {
        id: "msg_pre_snapshot".to_string(),
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: "pre-snapshot message".to_string(),
            cache_control: None,
        }],
        display_role: None,
        timestamp: Some(Utc::now()),
        tool_duration_ms: None,
        token_usage: None,
    });
    session.save()?;

    // Two more messages -> these go to the journal (append, not snapshot).
    session.append_stored_message(StoredMessage {
        id: "msg_journal_1".to_string(),
        role: Role::Assistant,
        content: vec![ContentBlock::Text {
            text: "journal-only message 1".to_string(),
            cache_control: None,
        }],
        display_role: None,
        timestamp: Some(Utc::now()),
        tool_duration_ms: None,
        token_usage: None,
    });
    session.append_stored_message(StoredMessage {
        id: "msg_journal_2".to_string(),
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: "journal-only message 2".to_string(),
            cache_control: None,
        }],
        display_role: None,
        timestamp: Some(Utc::now()),
        tool_duration_ms: None,
        token_usage: None,
    });
    session.save()?;

    // Sanity check: journal file exists with at least one entry.
    let journal_path = session_journal_path(session_id)?;
    assert!(
        journal_path.exists(),
        "journal must exist after appending after snapshot"
    );
    let journal_contents = std::fs::read_to_string(&journal_path)?;
    assert!(
        !journal_contents.trim().is_empty(),
        "journal must have content (post-snapshot appends)"
    );

    // Drop the live `Session` so we exercise the disk-only load paths.
    drop(session);

    // Confirm `load_for_remote_startup` (the primary read path) sees all 3
    // messages — this exercises the journal-replay logic that already worked.
    let primary = Session::load_for_remote_startup(session_id)?;
    assert_eq!(
        primary.messages.len(),
        3,
        "primary load_for_remote_startup should restore snapshot + journal messages"
    );

    // Simulate the snapshot being unrecoverable: truncate to zero bytes so
    // any deserialize attempt fails. The journal stays intact on disk.
    let snapshot_path = session_path(session_id)?;
    std::fs::write(&snapshot_path, b"")?;

    // The primary path now fails because the snapshot can't be parsed.
    assert!(
        Session::load_for_remote_startup(session_id).is_err(),
        "primary should fail when snapshot is corrupt"
    );

    // Stub fallback also fails on a fully-empty snapshot (need at least the
    // metadata fields). Restore a minimal but valid stub-shaped snapshot so
    // we can exercise the fallback recovery path. Snapshot has no messages
    // (simulating a stale checkpoint or incompatible variant), but the
    // journal still holds all post-snapshot appends.
    let minimal_stub = serde_json::json!({
        "id": session_id,
        "title": "M7 recovery",
        "created_at": Utc::now(),
        "updated_at": Utc::now(),
        "model": "gpt-5.4",
    });
    std::fs::write(&snapshot_path, serde_json::to_string(&minimal_stub)?)?;

    // With the fix: stub fallback replays the journal, so journal-only
    // messages are recovered even when the snapshot lost its message vector.
    let recovered = Session::load_startup_stub(session_id)?;
    assert_eq!(recovered.id, session_id);
    assert_eq!(recovered.model.as_deref(), Some("gpt-5.4"));
    assert!(
        recovered.messages.len() >= 2,
        "stub fallback must recover the 2 journal-only messages (got {})",
        recovered.messages.len()
    );
    let ids: Vec<_> = recovered.messages.iter().map(|m| m.id.as_str()).collect();
    assert!(
        ids.contains(&"msg_journal_1"),
        "msg_journal_1 should be recovered from journal: ids={:?}",
        ids
    );
    assert!(
        ids.contains(&"msg_journal_2"),
        "msg_journal_2 should be recovered from journal: ids={:?}",
        ids
    );
    Ok(())
}

#[test]
fn test_create_marks_debug_when_test_session_env_enabled() {
    let _env_lock = lock_env();
    let _test_flag = EnvVarGuard::set("JCODE_TEST_SESSION", "1");

    let s1 = Session::create(None, None);
    assert!(s1.is_debug);

    let s2 = Session::create_with_id("session_test_1".to_string(), None, None);
    assert!(s2.is_debug);
}

#[test]
fn test_create_not_debug_when_test_session_env_disabled() {
    let _env_lock = lock_env();
    let _test_flag = EnvVarGuard::set("JCODE_TEST_SESSION", "0");

    let s = Session::create(None, None);
    assert!(!s.is_debug);
}

#[test]
fn test_recover_crashed_sessions_preserves_debug_flag() -> Result<()> {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-recover-debug-test-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());
    let _test_flag = EnvVarGuard::set("JCODE_TEST_SESSION", "0");

    let mut crashed = Session::create_with_id(
        "session_recover_debug_source".to_string(),
        None,
        Some("debug source".to_string()),
    );
    crashed.is_debug = true;
    crashed.mark_crashed(Some("test crash".to_string()));
    crashed.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "hello".to_string(),
            cache_control: None,
        }],
    );
    crashed.save()?;

    let recovered_ids = recover_crashed_sessions()?;
    assert_eq!(recovered_ids.len(), 1);

    let recovered = Session::load(&recovered_ids[0])?;
    assert!(recovered.is_debug);
    Ok(())
}

#[test]
fn test_save_persists_full_session_content() -> Result<()> {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-save-test-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let mut session = Session::create_with_id(
        "session_save_persist_test".to_string(),
        None,
        Some("save fidelity test".to_string()),
    );

    session.add_message(
        Role::User,
        vec![ContentBlock::ToolResult {
            tool_use_id: "tool_1".to_string(),
            content: "OPENROUTER_API_KEY=sk-or-v1-abcdefghijklmnopqrstuvwxyz0123456789".to_string(),
            is_error: None,
        }],
    );

    session.add_message(
        Role::Assistant,
        vec![ContentBlock::ToolUse {
            id: "tool_2".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({
                "command": "echo ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123"
            }),
        }],
    );

    session.save()?;

    let loaded = Session::load("session_save_persist_test")?;

    let ContentBlock::ToolResult { content, .. } = &loaded.messages[0].content[0] else {
        return Err(anyhow!("expected tool result block"));
    };
    assert!(content.contains("sk-or-v1-abcdefghijklmnopqrstuvwxyz0123456789"));
    assert!(!content.contains("[REDACTED_SECRET]"));

    let ContentBlock::ToolUse { input, .. } = &loaded.messages[1].content[0] else {
        return Err(anyhow!("expected tool use block"));
    };
    let input_str = input.to_string();
    assert!(input_str.contains("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123"));
    assert!(!input_str.contains("[REDACTED_SECRET]"));
    Ok(())
}

#[test]
fn test_save_persists_compaction_state() -> Result<()> {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-compaction-save-test-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let mut session = Session::create_with_id(
        "session_compaction_persist_test".to_string(),
        None,
        Some("compaction persistence test".to_string()),
    );
    session.compaction = Some(StoredCompactionState {
        summary_text: "saved summary".to_string(),
        openai_encrypted_content: None,
        covers_up_to_turn: 8,
        original_turn_count: 8,
        compacted_count: 8,
    });

    session.save()?;

    let loaded = Session::load("session_compaction_persist_test")?;
    assert_eq!(loaded.compaction, session.compaction);
    Ok(())
}

// M48-C1: durable compaction-turn schema persistence + backfill.

#[test]
fn test_save_persists_compaction_turns_round_trip() -> Result<()> {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-compaction-turns-save-test-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let mut session = Session::create_with_id(
        "session_compaction_turns_persist_test".to_string(),
        None,
        Some("compaction turns persistence test".to_string()),
    );
    session
        .compaction_turns
        .push(crate::session::StoredCompactionTurn {
            id: "turn-1".to_string(),
            marker_message_id: "msg-marker-1".to_string(),
            summary_message_id: "msg-summary-1".to_string(),
            auto: true,
            overflow: false,
            tail_start_id: Some("msg-tail-7".to_string()),
            previous_summary_id: None,
            summary_of_message_ids: vec!["msg-1".to_string(), "msg-2".to_string()],
            backfilled_from_legacy: false,
            created_at: Some(chrono::Utc::now()),
        });

    session.save()?;

    let loaded = Session::load("session_compaction_turns_persist_test")?;
    assert_eq!(loaded.compaction_turns.len(), 1);
    let loaded_turn = &loaded.compaction_turns[0];
    assert_eq!(loaded_turn.id, "turn-1");
    assert_eq!(loaded_turn.marker_message_id, "msg-marker-1");
    assert_eq!(loaded_turn.summary_message_id, "msg-summary-1");
    assert!(loaded_turn.auto);
    assert!(!loaded_turn.overflow);
    assert_eq!(loaded_turn.tail_start_id.as_deref(), Some("msg-tail-7"));
    assert_eq!(
        loaded_turn.summary_of_message_ids,
        vec!["msg-1".to_string(), "msg-2".to_string()]
    );
    assert!(!loaded_turn.is_legacy_backfill());
    assert!(loaded_turn.has_durable_messages());
    Ok(())
}

#[test]
fn test_record_durable_compaction_turn_persists_artifacts_but_hides_from_provider() -> Result<()> {
    let mut session = Session::create_with_id(
        "session_durable_compaction_runtime_test".to_string(),
        None,
        Some("durable compaction runtime test".to_string()),
    );
    let msg1 = session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "first".to_string(),
            cache_control: None,
        }],
    );
    let msg2 = session.add_message(
        Role::Assistant,
        vec![ContentBlock::Text {
            text: "second".to_string(),
            cache_control: None,
        }],
    );
    let msg3 = session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "tail".to_string(),
            cache_control: None,
        }],
    );

    let state = StoredCompactionState {
        summary_text: "durable summary".to_string(),
        openai_encrypted_content: None,
        covers_up_to_turn: 2,
        original_turn_count: 2,
        compacted_count: 2,
    };

    assert!(session.record_durable_compaction_turn(None, &state, true, false));
    assert_eq!(session.compaction_turns.len(), 1);
    let turn = &session.compaction_turns[0];
    assert!(turn.has_durable_messages());
    assert!(turn.auto);
    assert!(!turn.overflow);
    assert_eq!(turn.tail_start_id.as_deref(), Some(msg3.as_str()));
    assert_eq!(turn.summary_of_message_ids, vec![msg1, msg2]);
    assert_eq!(session.messages.len(), 5);

    let uncached_provider_messages = session.messages_for_provider_uncached();
    assert_eq!(uncached_provider_messages.len(), 3);
    assert!(!uncached_provider_messages.iter().any(|message| {
        message.content.iter().any(
            |block| matches!(block, ContentBlock::Text { text, .. } if text == "durable summary"),
        )
    }));

    let provider_messages = session.messages_for_provider();
    assert_eq!(provider_messages.len(), 3);
    assert!(provider_messages.iter().any(|message| {
        message
            .content
            .iter()
            .any(|block| matches!(block, ContentBlock::Text { text, .. } if text == "tail"))
    }));
    assert!(!provider_messages.iter().any(|message| {
        message.content.iter().any(
            |block| matches!(block, ContentBlock::Text { text, .. } if text == "durable summary"),
        )
    }));
    Ok(())
}

#[test]
fn test_record_durable_compaction_turn_marks_overflow_recovery() -> Result<()> {
    let mut session = Session::create_with_id(
        "session_overflow_compaction_runtime_test".to_string(),
        None,
        Some("overflow compaction runtime test".to_string()),
    );
    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "overflowing prompt".to_string(),
            cache_control: None,
        }],
    );
    session.add_message(
        Role::Assistant,
        vec![ContentBlock::Text {
            text: "large answer".to_string(),
            cache_control: None,
        }],
    );

    let state = StoredCompactionState {
        summary_text: "overflow summary".to_string(),
        openai_encrypted_content: None,
        covers_up_to_turn: 2,
        original_turn_count: 2,
        compacted_count: 2,
    };

    assert!(session.record_durable_compaction_turn(None, &state, true, true));
    assert_eq!(session.compaction_turns.len(), 1);
    let turn = &session.compaction_turns[0];
    assert!(turn.auto);
    assert!(turn.overflow);
    assert!(turn.has_durable_messages());
    Ok(())
}

#[test]
fn test_load_backfills_compaction_turns_from_legacy_state() -> Result<()> {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-compaction-backfill-test-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let mut session = Session::create_with_id(
        "session_legacy_compaction_backfill_test".to_string(),
        None,
        Some("legacy compaction backfill test".to_string()),
    );
    // Simulate a pre-M48-C1 session: legacy compaction state set, no durable
    // compaction_turns persisted.
    session.compaction = Some(StoredCompactionState {
        summary_text: "legacy session summary".to_string(),
        openai_encrypted_content: None,
        covers_up_to_turn: 12,
        original_turn_count: 12,
        compacted_count: 4,
    });
    assert!(session.compaction_turns.is_empty());
    session.save()?;

    // Loading the session must synthesize exactly one backfill turn while
    // leaving the legacy `compaction` field intact.
    let loaded = Session::load("session_legacy_compaction_backfill_test")?;
    assert_eq!(loaded.compaction_turns.len(), 1);
    let turn = &loaded.compaction_turns[0];
    assert!(turn.is_legacy_backfill());
    assert!(!turn.has_durable_messages());
    assert!(turn.marker_message_id.is_empty());
    assert!(turn.summary_message_id.is_empty());
    assert_eq!(turn.id, "legacy-session_legacy_compaction_backfill_test-4");
    // The legacy field must survive untouched.
    assert!(loaded.compaction.is_some());
    assert_eq!(
        loaded.compaction.as_ref().unwrap().summary_text,
        "legacy session summary"
    );
    Ok(())
}

#[test]
fn test_load_does_not_backfill_when_compaction_turns_already_present() -> Result<()> {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-compaction-no-backfill-test-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let mut session = Session::create_with_id(
        "session_no_backfill_test".to_string(),
        None,
        Some("no backfill needed".to_string()),
    );
    // Both legacy compaction and durable turn present.
    session.compaction = Some(StoredCompactionState {
        summary_text: "legacy".to_string(),
        openai_encrypted_content: None,
        covers_up_to_turn: 1,
        original_turn_count: 1,
        compacted_count: 1,
    });
    session
        .compaction_turns
        .push(crate::session::StoredCompactionTurn {
            id: "turn-real".to_string(),
            marker_message_id: "msg-a".to_string(),
            summary_message_id: "msg-b".to_string(),
            auto: false,
            overflow: false,
            tail_start_id: None,
            previous_summary_id: None,
            summary_of_message_ids: Vec::new(),
            backfilled_from_legacy: false,
            created_at: None,
        });
    session.save()?;

    // Backfill must be idempotent: the existing turn stays, no synthetic
    // legacy turn is appended.
    let loaded = Session::load("session_no_backfill_test")?;
    assert_eq!(loaded.compaction_turns.len(), 1);
    assert_eq!(loaded.compaction_turns[0].id, "turn-real");
    assert!(!loaded.compaction_turns[0].is_legacy_backfill());
    Ok(())
}

#[test]
fn test_save_omits_compaction_turns_when_empty() -> Result<()> {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-compaction-empty-save-test-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let mut session = Session::create_with_id(
        "session_compaction_empty_test".to_string(),
        None,
        Some("empty compaction turns".to_string()),
    );
    session.save()?;

    // No compaction, no turns: round-trips clean.
    let loaded = Session::load("session_compaction_empty_test")?;
    assert!(loaded.compaction.is_none());
    assert!(loaded.compaction_turns.is_empty());
    Ok(())
}

#[test]
fn test_save_persists_provider_key() -> Result<()> {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-provider-key-save-test-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let mut session = Session::create_with_id(
        "session_provider_key_persist_test".to_string(),
        None,
        Some("provider key persistence test".to_string()),
    );
    session.provider_key = Some("opencode".to_string());
    session.model = Some("anthropic/claude-sonnet-4".to_string());

    session.save()?;

    let loaded = Session::load("session_provider_key_persist_test")?;
    assert_eq!(loaded.provider_key.as_deref(), Some("opencode"));
    assert_eq!(loaded.model.as_deref(), Some("anthropic/claude-sonnet-4"));
    Ok(())
}

#[test]
fn test_save_persists_reasoning_effort() -> Result<()> {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-reasoning-effort-save-test-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let mut session = Session::create_with_id(
        "session_reasoning_effort_persist_test".to_string(),
        None,
        Some("reasoning effort persistence test".to_string()),
    );
    session.model = Some("gpt-5.4".to_string());
    session.reasoning_effort = Some("xhigh".to_string());

    session.save()?;

    let loaded = Session::load("session_reasoning_effort_persist_test")?;
    assert_eq!(loaded.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(loaded.reasoning_effort.as_deref(), Some("xhigh"));
    Ok(())
}

// M47-C6: context_preference and thinking_enabled must round-trip through
// session save/load just like reasoning_effort does. These three preferences
// together form the persisted half of the M47 5-dimension agent profile
// schema; restore_provider_preferences_from_session applies them all on
// session activation via the M47-C4 Provider trait surface.

#[test]
fn test_save_persists_context_preference() -> Result<()> {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-context-preference-save-test-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let mut session = Session::create_with_id(
        "session_context_preference_persist_test".to_string(),
        None,
        Some("context preference persistence test".to_string()),
    );
    session.model = Some("claude-opus-4-7[1m]".to_string());
    session.context_preference = Some("1m".to_string());

    session.save()?;

    let loaded = Session::load("session_context_preference_persist_test")?;
    assert_eq!(loaded.model.as_deref(), Some("claude-opus-4-7[1m]"));
    assert_eq!(loaded.context_preference.as_deref(), Some("1m"));
    assert_eq!(loaded.thinking_enabled, None);
    Ok(())
}

#[test]
fn test_save_persists_thinking_enabled_true_and_false() -> Result<()> {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-thinking-save-test-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    // explicit-on
    let mut on = Session::create_with_id(
        "session_thinking_on_test".to_string(),
        None,
        Some("thinking on".to_string()),
    );
    on.model = Some("gemini-3.1-pro-preview".to_string());
    on.thinking_enabled = Some(true);
    on.save()?;

    let loaded_on = Session::load("session_thinking_on_test")?;
    assert_eq!(loaded_on.thinking_enabled, Some(true));

    // explicit-off
    let mut off = Session::create_with_id(
        "session_thinking_off_test".to_string(),
        None,
        Some("thinking off".to_string()),
    );
    off.model = Some("gemini-3.1-pro-preview".to_string());
    off.thinking_enabled = Some(false);
    off.save()?;

    let loaded_off = Session::load("session_thinking_off_test")?;
    assert_eq!(loaded_off.thinking_enabled, Some(false));

    Ok(())
}

#[test]
fn test_save_omits_unset_context_and_thinking_dimensions() -> Result<()> {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-unset-dims-save-test-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let mut session = Session::create_with_id(
        "session_unset_dims_test".to_string(),
        None,
        Some("unset dims".to_string()),
    );
    session.model = Some("gpt-5.5".to_string());
    // context_preference, thinking_enabled intentionally left None.
    session.save()?;

    let loaded = Session::load("session_unset_dims_test")?;
    assert_eq!(loaded.context_preference, None);
    assert_eq!(loaded.thinking_enabled, None);
    Ok(())
}

fn read_journal_entries(path: &std::path::Path) -> Result<Vec<serde_json::Value>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    std::fs::read_to_string(path)?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(Into::into))
        .collect()
}

#[test]
fn test_append_stored_message_writes_immediate_journal_entry() -> Result<()> {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-immediate-journal-test-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let mut session = Session::create_with_id(
        "session_immediate_journal_test".to_string(),
        None,
        Some("immediate journal test".to_string()),
    );
    session.save()?;

    let snapshot_path = session_path("session_immediate_journal_test")?;
    let journal_path = session_journal_path("session_immediate_journal_test")?;
    assert!(snapshot_path.exists());
    assert!(!journal_path.exists());

    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "immediate".to_string(),
            cache_control: None,
        }],
    );

    let entries = read_journal_entries(&journal_path)?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["append_messages"].as_array().unwrap().len(), 1);
    assert_eq!(
        entries[0]["append_messages"][0]["content"][0]["text"],
        "immediate"
    );
    assert_eq!(
        session.persisted_messages_len_for_test(),
        session.messages.len()
    );
    Ok(())
}

#[test]
fn test_immediate_journal_does_not_duplicate_on_next_save() -> Result<()> {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-immediate-journal-dedupe-test-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let mut session = Session::create_with_id(
        "session_immediate_journal_dedupe_test".to_string(),
        None,
        Some("immediate journal dedupe test".to_string()),
    );
    session.save()?;
    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "dedupe".to_string(),
            cache_control: None,
        }],
    );

    let journal_path = session_journal_path("session_immediate_journal_dedupe_test")?;
    assert_eq!(read_journal_entries(&journal_path)?.len(), 1);

    session.save()?;

    let entries = read_journal_entries(&journal_path)?;
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0]["append_messages"][0]["content"][0]["text"],
        "dedupe"
    );
    Ok(())
}

#[test]
fn test_immediate_journal_skipped_before_first_snapshot() -> Result<()> {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-immediate-journal-no-snapshot-test-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let mut session = Session::create_with_id(
        "session_immediate_journal_no_snapshot_test".to_string(),
        None,
        Some("immediate journal no snapshot test".to_string()),
    );
    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "before snapshot".to_string(),
            cache_control: None,
        }],
    );

    let snapshot_path = session_path("session_immediate_journal_no_snapshot_test")?;
    let journal_path = session_journal_path("session_immediate_journal_no_snapshot_test")?;
    assert!(!snapshot_path.exists());
    assert!(!journal_path.exists());

    session.save()?;

    assert!(snapshot_path.exists());
    assert!(!journal_path.exists());
    let loaded = Session::load("session_immediate_journal_no_snapshot_test")?;
    assert_eq!(loaded.messages.len(), 1);
    assert_eq!(loaded.messages[0].content_preview(), "before snapshot");
    Ok(())
}

#[test]
fn test_immediate_journal_recovered_on_load() -> Result<()> {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-immediate-journal-recover-test-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let mut session = Session::create_with_id(
        "session_immediate_journal_recover_test".to_string(),
        None,
        Some("immediate journal recover test".to_string()),
    );
    session.save()?;
    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "recovered".to_string(),
            cache_control: None,
        }],
    );

    let loaded = Session::load_from_path(&session_path("session_immediate_journal_recover_test")?)?;
    assert_eq!(loaded.messages.len(), 1);
    assert_eq!(loaded.messages[0].content_preview(), "recovered");
    Ok(())
}

#[test]
fn test_save_appends_journal_and_load_replays_it() -> Result<()> {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-journal-test-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let mut session = Session::create_with_id(
        "session_journal_append_test".to_string(),
        None,
        Some("journal append test".to_string()),
    );
    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "first".to_string(),
            cache_control: None,
        }],
    );
    session.save()?;

    let snapshot_path = session_path("session_journal_append_test")?;
    let journal_path = session_journal_path("session_journal_append_test")?;
    assert!(snapshot_path.exists());
    assert!(!journal_path.exists());

    session.add_message(
        Role::Assistant,
        vec![ContentBlock::Text {
            text: "second".to_string(),
            cache_control: None,
        }],
    );
    session.save()?;

    assert!(journal_path.exists());
    let journal = std::fs::read_to_string(&journal_path)?;
    assert!(journal.contains("second"));

    let loaded = Session::load("session_journal_append_test")?;
    assert_eq!(loaded.messages.len(), 2);
    assert_eq!(loaded.messages[1].content_preview(), "second");
    Ok(())
}

#[test]
fn test_save_checkpoints_after_full_mutation_and_clears_journal() -> Result<()> {
    let _env_lock = lock_env();
    let temp_home = tempfile::Builder::new()
        .prefix("jcode-session-checkpoint-test-")
        .tempdir()
        .map_err(|e| anyhow!(e))?;
    let _home = EnvVarGuard::set("JCODE_HOME", temp_home.path().as_os_str());

    let mut session = Session::create_with_id(
        "session_journal_checkpoint_test".to_string(),
        None,
        Some("checkpoint test".to_string()),
    );
    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "one".to_string(),
            cache_control: None,
        }],
    );
    session.save()?;

    session.add_message(
        Role::Assistant,
        vec![ContentBlock::Text {
            text: "two".to_string(),
            cache_control: None,
        }],
    );
    session.save()?;

    let journal_path = session_journal_path("session_journal_checkpoint_test")?;
    assert!(journal_path.exists());

    session.truncate_messages(1);
    session.title = Some("checkpointed title".to_string());
    session.save()?;

    assert!(!journal_path.exists());

    let loaded = Session::load("session_journal_checkpoint_test")?;
    assert_eq!(loaded.title.as_deref(), Some("checkpointed title"));
    assert_eq!(loaded.messages.len(), 1);
    assert_eq!(loaded.messages[0].content_preview(), "one");
    Ok(())
}

#[test]
fn test_redacted_for_export_redacts_tool_result_and_tool_input() -> Result<()> {
    let mut session = Session::create_with_id(
        "session_redact_persist_test".to_string(),
        None,
        Some("redaction test".to_string()),
    );

    session.add_message(
        Role::User,
        vec![ContentBlock::ToolResult {
            tool_use_id: "tool_1".to_string(),
            content: "OPENROUTER_API_KEY=sk-or-v1-abcdefghijklmnopqrstuvwxyz0123456789".to_string(),
            is_error: None,
        }],
    );

    session.add_message(
        Role::Assistant,
        vec![ContentBlock::ToolUse {
            id: "tool_2".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({
                "command": "echo ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123"
            }),
        }],
    );

    let persisted = session.redacted_for_export();

    let first_content = &persisted.messages[0].content[0];
    let ContentBlock::ToolResult { content, .. } = first_content else {
        return Err(anyhow!("expected tool result block"));
    };
    assert!(content.contains("OPENROUTER_API_KEY=[REDACTED_SECRET]"));
    assert!(!content.contains("sk-or-v1-abcdefghijklmnopqrstuvwxyz0123456789"));

    let second_content = &persisted.messages[1].content[0];
    let ContentBlock::ToolUse { input, .. } = second_content else {
        return Err(anyhow!("expected tool use block"));
    };
    let input_str = input.to_string();
    assert!(input_str.contains("[REDACTED_SECRET]"));
    assert!(!input_str.contains("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123"));
    Ok(())
}

#[test]
fn test_redacted_for_export_redacts_replay_events() -> Result<()> {
    let mut session = Session::create_with_id(
        "session_redacted_replay_events_test".to_string(),
        None,
        Some("redacted replay events".to_string()),
    );

    session.record_replay_display_message(
        "swarm",
        Some("DM from fox".to_string()),
        "OPENROUTER_API_KEY=sk-or-v1-secret-value",
    );
    session.record_swarm_status_event(vec![crate::protocol::SwarmMemberStatus {
        session_id: "session_fox".to_string(),
        friendly_name: Some("fox".to_string()),
        status: "running".to_string(),
        detail: Some("ANTHROPIC_API_KEY=sk-ant-secret-value".to_string()),
        role: Some("agent".to_string()),
        is_headless: None,
        live_attachments: None,
        status_age_secs: None,
        last_heartbeat_secs_ago: None,
        last_tool: None,
        last_checkpoint: None,
    }]);
    session.record_swarm_plan_event(
        "swarm_test".to_string(),
        1,
        vec![crate::plan::PlanItem {
            content: "OPENROUTER_API_KEY=sk-or-v1-abcdefghijklmnopqrstuvwxyz0123456789".to_string(),
            status: "pending".to_string(),
            priority: "high".to_string(),
            id: "task-1".to_string(),
            subsystem: None,
            file_scope: Vec::new(),
            blocked_by: vec![],
            assigned_to: None,
        }],
        vec![],
        Some("ANTHROPIC_API_KEY=sk-ant-secret-value".to_string()),
    );

    let redacted = session.redacted_for_export();
    assert_eq!(redacted.replay_events.len(), 3);

    let StoredReplayEventKind::DisplayMessage { content, .. } = &redacted.replay_events[0].kind
    else {
        return Err(anyhow!("expected display message replay event"));
    };
    assert!(content.contains("OPENROUTER_API_KEY=[REDACTED_SECRET]"));
    assert!(!content.contains("sk-or-v1-secret-value"));

    let StoredReplayEventKind::SwarmStatus { members } = &redacted.replay_events[1].kind else {
        return Err(anyhow!("expected swarm status replay event"));
    };
    let detail = members[0].detail.as_deref().unwrap_or_default();
    assert!(detail.contains("ANTHROPIC_API_KEY=[REDACTED_SECRET]"));
    assert!(!detail.contains("sk-ant-secret-value"));

    let StoredReplayEventKind::SwarmPlan { items, reason, .. } = &redacted.replay_events[2].kind
    else {
        return Err(anyhow!("expected swarm plan replay event"));
    };
    assert!(
        items[0]
            .content
            .contains("OPENROUTER_API_KEY=[REDACTED_SECRET]")
    );
    assert!(
        !items[0]
            .content
            .contains("sk-or-v1-abcdefghijklmnopqrstuvwxyz0123456789")
    );
    let reason = reason.as_deref().unwrap_or_default();
    assert!(reason.contains("ANTHROPIC_API_KEY=[REDACTED_SECRET]"));
    assert!(!reason.contains("sk-ant-secret-value"));
    Ok(())
}

#[test]
fn test_summarize_tool_calls_includes_tool_only_assistant_messages() {
    let mut session = Session::create_with_id(
        "session_tool_summary_test".to_string(),
        None,
        Some("tool summary test".to_string()),
    );

    session.add_message(
        Role::Assistant,
        vec![ContentBlock::ToolUse {
            id: "tool_1".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({
                "command": "pwd"
            }),
        }],
    );

    let summaries = summarize_tool_calls(&session, 10);
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].tool_name, "bash");
    assert!(summaries[0].brief_output.contains("pwd"));
}

#[test]
fn test_render_messages_honors_system_display_role_override() {
    let mut session = Session::create_with_id(
        "session_display_role_test".to_string(),
        None,
        Some("display role test".to_string()),
    );

    session.add_message_with_display_role(
        Role::User,
        vec![ContentBlock::Text {
            text: "[Background Task Completed]\nTask: abc123 (bash)".to_string(),
            cache_control: None,
        }],
        Some(StoredDisplayRole::System),
    );

    let rendered = render_messages(&session);
    assert_eq!(rendered.len(), 1);
    assert_eq!(rendered[0].role, "system");
    assert!(rendered[0].content.contains("Background Task Completed"));
}

#[test]
fn test_render_messages_honors_background_task_display_role_override() {
    let mut session = Session::create_with_id(
        "session_background_task_role_test".to_string(),
        None,
        Some("background task role test".to_string()),
    );

    session.add_message_with_display_role(
            Role::User,
            vec![ContentBlock::Text {
                text: "**Background task** `abc123` · `bash` · ✓ completed · 7.1s · exit 0\n\n_No output captured._\n\n_Full output:_ `bg action=\"output\" task_id=\"abc123\"`".to_string(),
                cache_control: None,
            }],
            Some(StoredDisplayRole::BackgroundTask),
        );

    let rendered = render_messages(&session);
    assert_eq!(rendered.len(), 1);
    assert_eq!(rendered[0].role, "background_task");
    assert!(rendered[0].content.contains("**Background task**"));
}

#[test]
fn test_render_messages_hides_internal_system_reminders() {
    let mut session = Session::create_with_id(
        "session_hidden_system_reminder_test".to_string(),
        None,
        Some("hidden reminder test".to_string()),
    );

    assert!(session.ensure_initial_session_context_message());
    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "visible prompt".to_string(),
            cache_control: None,
        }],
    );

    let rendered = render_messages(&session);
    assert_eq!(rendered.len(), 1);
    assert_eq!(rendered[0].role, "user");
    assert_eq!(rendered[0].content, "visible prompt");
}

#[test]
fn test_render_messages_shows_recent_compacted_history_by_default() {
    let mut session = Session::create_with_id(
        "session_render_compacted_history_test".to_string(),
        None,
        Some("render compacted history test".to_string()),
    );

    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "old prompt".to_string(),
            cache_control: None,
        }],
    );
    session.add_message(
        Role::Assistant,
        vec![ContentBlock::Text {
            text: "old response".to_string(),
            cache_control: None,
        }],
    );
    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "current prompt".to_string(),
            cache_control: None,
        }],
    );
    session.compaction = Some(StoredCompactionState {
        summary_text: "old prompt and response".to_string(),
        openai_encrypted_content: None,
        covers_up_to_turn: 2,
        original_turn_count: 2,
        compacted_count: 2,
    });

    let rendered = render_messages(&session);
    assert_eq!(rendered.len(), 4);
    assert_eq!(rendered[0].role, "system");
    assert!(rendered[0].content.contains("showing all 2"));
    assert_eq!(rendered[1].role, "user");
    assert_eq!(rendered[1].content, "old prompt");
    assert_eq!(rendered[2].role, "assistant");
    assert_eq!(rendered[2].content, "old response");
    assert_eq!(rendered[3].role, "user");
    assert_eq!(rendered[3].content, "current prompt");
}

#[test]
fn test_render_messages_can_expand_compacted_history_window() {
    let mut session = Session::create_with_id(
        "session_render_compacted_history_expand_test".to_string(),
        None,
        Some("render compacted history expand test".to_string()),
    );

    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "old prompt".to_string(),
            cache_control: None,
        }],
    );
    session.add_message(
        Role::Assistant,
        vec![ContentBlock::Text {
            text: "old response".to_string(),
            cache_control: None,
        }],
    );
    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "current prompt".to_string(),
            cache_control: None,
        }],
    );
    session.compaction = Some(StoredCompactionState {
        summary_text: "old prompt and response".to_string(),
        openai_encrypted_content: None,
        covers_up_to_turn: 2,
        original_turn_count: 2,
        compacted_count: 2,
    });

    let (rendered, _images, info) = render_messages_and_images_with_compacted_history(&session, 1);
    assert_eq!(info.unwrap().total_messages, 2);
    assert_eq!(info.unwrap().visible_messages, 1);
    assert_eq!(info.unwrap().remaining_messages, 1);
    assert_eq!(rendered.len(), 3);
    assert!(rendered[0].content.contains("Showing 1 of 2"));
    assert_eq!(rendered[1].role, "assistant");
    assert_eq!(rendered[1].content, "old response");
    assert_eq!(rendered[2].content, "current prompt");

    let (rendered_all, _images, info_all) =
        render_messages_and_images_with_compacted_history(&session, usize::MAX);
    let info_all = info_all.expect("compacted info");
    assert_eq!(info_all.visible_messages, 2);
    assert_eq!(info_all.remaining_messages, 0);
    assert_eq!(rendered_all.len(), 4);
    assert!(rendered_all[0].content.contains("showing all 2"));
    assert_eq!(rendered_all[1].content, "old prompt");
    assert_eq!(rendered_all[2].content, "old response");
    assert_eq!(rendered_all[3].content, "current prompt");
}

#[test]
fn test_render_messages_and_images_share_tool_resolution_and_labels() {
    let mut session = Session::create_with_id(
        "session_render_bundle_test".to_string(),
        None,
        Some("render bundle test".to_string()),
    );

    session.add_message(
        Role::Assistant,
        vec![
            ContentBlock::ToolUse {
                id: "tool_img_1".to_string(),
                name: "view_image".to_string(),
                input: serde_json::json!({"file_path": "/tmp/screenshot.png"}),
            },
            ContentBlock::ToolResult {
                tool_use_id: "tool_img_1".to_string(),
                content: "rendered image".to_string(),
                is_error: None,
            },
            ContentBlock::Image {
                media_type: "image/png".to_string(),
                data: "abcd".to_string(),
            },
            ContentBlock::Text {
                text: "[Attached image associated with the preceding tool result: screenshot.png]"
                    .to_string(),
                cache_control: None,
            },
        ],
    );

    let (rendered, images) = render_messages_and_images(&session);
    assert_eq!(rendered.len(), 2);
    assert_eq!(rendered[0].role, "tool");
    assert_eq!(rendered[0].content, "rendered image");
    assert_eq!(
        rendered[0]
            .tool_data
            .as_ref()
            .map(|tool| tool.name.as_str()),
        Some("view_image")
    );

    assert_eq!(images.len(), 1);
    assert_eq!(images[0].label.as_deref(), Some("screenshot.png"));
    assert_eq!(images[0].media_type, "image/png");
    assert_eq!(
        images[0].source,
        RenderedImageSource::ToolResult {
            tool_name: "view_image".to_string(),
        }
    );
}
