use super::reconnect;
use super::{
    RemoteRunState, auth_provider_hint_for_login_provider, handle_post_connect,
    handle_server_event, process_remote_followups,
};
use crate::protocol::{
    MemoryActivitySnapshot, MemoryPipelineSnapshot, MemoryStateSnapshot, MemoryStepStatusSnapshot,
    ServerEvent,
};
use crate::provider::Provider;
use crate::tui::info_widget::{MemoryState, StepStatus};
use anyhow::Result;
use std::sync::Arc;

struct MockProvider;

#[async_trait::async_trait]
impl Provider for MockProvider {
    async fn complete(
        &self,
        _messages: &[crate::message::Message],
        _tools: &[crate::message::ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<crate::provider::EventStream> {
        Err(anyhow::anyhow!(
            "Mock provider should not be used for streaming completions in remote app tests"
        ))
    }

    fn name(&self) -> &str {
        "mock"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self)
    }
}

fn create_test_app() -> crate::tui::app::App {
    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let registry = rt.block_on(crate::tool::Registry::new(provider.clone()));
    let mut app = crate::tui::app::App::new_for_test_harness(provider, registry);
    app.queue_mode = false;
    app.diff_mode = crate::config::DiffDisplayMode::Inline;
    app
}

#[test]
fn reload_handoff_active_when_server_flag_is_set() {
    let state = RemoteRunState {
        server_reload_in_progress: true,
        ..RemoteRunState::default()
    };

    assert!(reconnect::reload_handoff_active(&state));
}

#[test]
fn auth_provider_hint_maps_openai_compatible_login_providers() {
    assert_eq!(
        auth_provider_hint_for_login_provider("Azure OpenAI"),
        Some("azure-openai")
    );
    assert_eq!(
        auth_provider_hint_for_login_provider("cerebras"),
        Some("cerebras")
    );
    assert_eq!(
        auth_provider_hint_for_login_provider("Cerebras"),
        Some("cerebras")
    );
    assert_eq!(
        auth_provider_hint_for_login_provider("minimax"),
        Some("minimax")
    );
    assert_eq!(
        auth_provider_hint_for_login_provider("not-a-provider"),
        None
    );
}

#[test]
fn auth_changed_event_for_cerebras_login_carries_runtime_and_catalog_identity() {
    let auth = super::auth_changed_event_for_login_provider("Cerebras")
        .expect("Cerebras login should produce typed auth event");

    assert_eq!(auth.provider.as_str(), "cerebras");
    assert_eq!(
        auth.credential_source,
        Some(crate::protocol::AuthCredentialSource::ApiKeyFile)
    );
    assert_eq!(
        auth.auth_method,
        Some(crate::protocol::AuthMethod::RemoteTuiPasteApiKey)
    );
    assert_eq!(
        auth.expected_runtime
            .as_ref()
            .map(crate::protocol::RuntimeProviderKey::as_str),
        Some("openai-compatible")
    );
    assert_eq!(
        auth.expected_catalog_namespace
            .as_ref()
            .map(crate::protocol::CatalogNamespace::as_str),
        Some("cerebras")
    );
}

#[test]
fn reload_handoff_inactive_without_flag_or_marker() {
    assert!(!reconnect::reload_handoff_active(&RemoteRunState::default()));
}

#[test]
fn reload_wait_status_message_uses_waiting_language() {
    let mut app = create_test_app();
    app.resume_session_id = Some("ses_test_reload_wait".to_string());
    let state = RemoteRunState::default();

    let message = reconnect::reload_wait_status_message(&app, &state, "server reload in progress");

    assert!(message.contains("waiting for handoff"));
    assert!(!message.contains("retrying"));
}

#[test]
fn process_remote_followups_auto_reloads_server_by_default() {
    let mut app = create_test_app();
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let _guard = rt.enter();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();
    remote.mark_history_loaded();

    app.pending_server_reload = true;
    app.auto_server_reload = true;

    rt.block_on(process_remote_followups(&mut app, &mut remote));

    assert!(!app.pending_server_reload);
    let last = app
        .display_messages()
        .last()
        .expect("missing reload message");
    assert_eq!(last.title.as_deref(), Some("Reload"));
    assert!(last.content.contains("Reloading server with newer binary"));
}

#[test]
fn process_remote_followups_respects_disabled_auto_server_reload() {
    let mut app = create_test_app();
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let _guard = rt.enter();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();
    remote.mark_history_loaded();

    app.pending_server_reload = true;
    app.auto_server_reload = false;

    rt.block_on(process_remote_followups(&mut app, &mut remote));

    assert!(!app.pending_server_reload);
    let last = app.display_messages().last().expect("missing info message");
    assert_eq!(last.role, "system");
    assert!(last.content.contains("display.auto_server_reload = false"));
}

#[test]
fn handle_post_connect_dispatches_reload_followup_even_if_history_snapshot_looks_busy() {
    let _guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("create temp home");
    let prev_home = std::env::var_os("JCODE_HOME");
    crate::env::set_var("JCODE_HOME", temp_home.path());

    let session_id = "session_reload_busy_snapshot";
    crate::tool::selfdev::ReloadContext {
        task_context: Some("Validate reload continuation after reconnect".to_string()),
        version_before: "old-build".to_string(),
        version_after: "new-build".to_string(),
        session_id: session_id.to_string(),
        timestamp: "2026-04-14T00:00:00Z".to_string(),
    }
    .save()
    .expect("save reload context");

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let mut app = crate::tui::app::App::new_for_remote(Some(session_id.to_string()));
    app.queue_mode = false;
    app.diff_mode = crate::config::DiffDisplayMode::Inline;
    app.is_processing = true;
    app.status = crate::tui::app::ProcessingStatus::RunningTool("batch".to_string());
    app.processing_started = Some(std::time::Instant::now());
    app.remote_resume_activity = Some(crate::tui::app::RemoteResumeActivity {
        session_id: session_id.to_string(),
        observed_at: std::time::Instant::now(),
        current_tool_name: Some("batch".to_string()),
    });

    let _enter = rt.enter();
    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = ratatui::Terminal::new(backend).expect("failed to create terminal");
    let mut remote = crate::tui::backend::RemoteConnection::dummy();
    remote.mark_history_loaded();
    let mut state = super::RemoteRunState {
        reconnect_attempts: 1,
        ..Default::default()
    };

    let outcome = rt
        .block_on(handle_post_connect(
            &mut app,
            &mut terminal,
            &mut remote,
            &mut state,
            Some(session_id),
        ))
        .expect("post connect should succeed");

    assert!(matches!(outcome, super::PostConnectOutcome::Ready));
    assert!(
        app.hidden_queued_system_messages.is_empty(),
        "reload continuation should dispatch instead of staying hidden"
    );
    assert!(matches!(
        app.status,
        crate::tui::app::ProcessingStatus::Sending
    ));
    assert!(app.current_message_id.is_some());
    assert!(app.rate_limit_pending_message.is_some());

    if let Ok(path) = crate::tool::selfdev::ReloadContext::path_for_session(session_id) {
        let _ = std::fs::remove_file(path);
    }
    if let Some(prev_home) = prev_home {
        crate::env::set_var("JCODE_HOME", prev_home);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}

#[test]
fn handle_server_event_applies_remote_memory_activity_snapshot() {
    crate::memory::clear_activity();

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let _guard = rt.enter();
    let mut app = create_test_app();
    app.memory_enabled = true;
    let mut remote = crate::tui::backend::RemoteConnection::dummy();

    handle_server_event(
        &mut app,
        ServerEvent::MemoryActivity {
            activity: MemoryActivitySnapshot {
                state: MemoryStateSnapshot::SidecarChecking { count: 3 },
                state_age_ms: 180,
                pipeline: Some(MemoryPipelineSnapshot {
                    search: MemoryStepStatusSnapshot::Done,
                    search_result: None,
                    verify: MemoryStepStatusSnapshot::Running,
                    verify_result: None,
                    verify_progress: Some((1, 3)),
                    inject: MemoryStepStatusSnapshot::Pending,
                    inject_result: None,
                    maintain: MemoryStepStatusSnapshot::Pending,
                    maintain_result: None,
                }),
            },
        },
        &mut remote,
    );

    let activity = crate::memory::get_activity().expect("memory activity should be populated");
    assert_eq!(activity.state, MemoryState::SidecarChecking { count: 3 });
    let pipeline = activity.pipeline.expect("pipeline should be restored");
    assert_eq!(pipeline.search, StepStatus::Done);
    assert_eq!(pipeline.verify, StepStatus::Running);
    assert_eq!(pipeline.verify_progress, Some((1, 3)));
    assert!(activity.state_since.elapsed().as_millis() >= 100);

    crate::memory::clear_activity();
}

// ---------------------------------------------------------------------------
// M41 — server-initiated turn wake-up regression tests
// ---------------------------------------------------------------------------

#[test]
fn m41_text_delta_on_idle_client_wakes_and_requests_redraw() {
    use crate::tui::app::ProcessingStatus;

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let _guard = rt.enter();
    let mut app = create_test_app();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();

    // Pre-conditions: client never called begin_remote_send(), so state is
    // the same as right after attach to a server already busy with a
    // background-initiated turn.
    assert!(!app.is_processing);
    assert!(matches!(app.status, ProcessingStatus::Idle));
    assert!(app.current_message_id.is_none());

    let needs_redraw = handle_server_event(
        &mut app,
        ServerEvent::TextDelta {
            text: "Hello".to_string(),
        },
        &mut remote,
    );

    // The wake-up path forces a redraw even on native full-tier terminals
    // where `eager_stream_redraw` is false.
    assert!(
        needs_redraw,
        "M41: first TextDelta on idle client must request a redraw"
    );
    assert!(
        app.is_processing,
        "M41: wake-up must flip is_processing to true"
    );
    assert!(
        !matches!(app.status, ProcessingStatus::Idle),
        "M41: wake-up must advance status out of Idle"
    );
    assert!(
        app.processing_started.is_some(),
        "M41: wake-up must record processing_started"
    );
}

#[test]
fn streaming_redraw_requests_are_coalesced_to_frame_interval() {
    let mut app = create_test_app();

    assert!(
        app.should_redraw_streaming_delta(),
        "first streaming chunk should paint immediately"
    );
    assert!(
        !app.should_redraw_streaming_delta(),
        "back-to-back streaming chunks should be coalesced"
    );

    std::thread::sleep(std::time::Duration::from_millis(17));
    assert!(
        app.should_redraw_streaming_delta(),
        "next frame interval should allow another streaming paint"
    );

    assert!(!app.should_redraw_streaming_delta());
    app.reset_streaming_redraw_coalescer();
    assert!(
        app.should_redraw_streaming_delta(),
        "lifecycle events reset the coalescer for immediate feedback"
    );
}

#[test]
fn m41_tool_start_on_idle_client_wakes_and_requests_redraw() {
    use crate::tui::app::ProcessingStatus;

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let _guard = rt.enter();
    let mut app = create_test_app();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();

    assert!(!app.is_processing);
    assert!(matches!(app.status, ProcessingStatus::Idle));

    let needs_redraw = handle_server_event(
        &mut app,
        ServerEvent::ToolStart {
            id: "tool-1".to_string(),
            name: "bash".to_string(),
        },
        &mut remote,
    );

    assert!(
        needs_redraw,
        "M41: first ToolStart on idle client must request a redraw"
    );
    assert!(app.is_processing);
    // ToolStart handler overrides status to RunningTool.
    assert!(matches!(app.status, ProcessingStatus::RunningTool(_)));
}

#[test]
fn m41_done_completes_server_initiated_turn_without_current_message_id() {
    use crate::tui::app::ProcessingStatus;

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let _guard = rt.enter();
    let mut app = create_test_app();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();

    // Drive a small server-initiated turn: TextDelta wakes the client,
    // then Done arrives with an id that the client never recorded.
    let _ = handle_server_event(
        &mut app,
        ServerEvent::TextDelta {
            text: "background result\n".to_string(),
        },
        &mut remote,
    );
    assert!(app.is_processing);
    assert!(app.current_message_id.is_none());

    let needs_redraw = handle_server_event(&mut app, ServerEvent::Done { id: 4242 }, &mut remote);

    assert!(
        needs_redraw,
        "M41: Done that closes a woken server-initiated turn must request a redraw"
    );
    assert!(
        !app.is_processing,
        "M41: Done cleanup must flip is_processing back to false"
    );
    assert!(
        matches!(app.status, ProcessingStatus::Idle),
        "M41: Done cleanup must return status to Idle"
    );
    assert!(
        app.streaming_text.is_empty(),
        "M41: streaming buffer should have been committed by Done cleanup"
    );
}

#[test]
fn m41_text_delta_on_active_local_turn_does_not_trigger_wake_logic() {
    use crate::tui::app::ProcessingStatus;
    use std::time::Instant;

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let _guard = rt.enter();
    let mut app = create_test_app();
    let mut remote = crate::tui::backend::RemoteConnection::dummy();

    // Simulate that begin_remote_send() already set up local state.
    app.is_processing = true;
    app.current_message_id = Some(7);
    app.status = ProcessingStatus::Sending;
    app.processing_started = Some(Instant::now());

    // The wake-up path should NOT fire (already processing), so the return
    // value reflects normal throttling rules. We don't assert a specific
    // bool here because it depends on terminal profile, but we assert that
    // the state machine was not perturbed.
    let _ = handle_server_event(
        &mut app,
        ServerEvent::TextDelta {
            text: "abc".to_string(),
        },
        &mut remote,
    );

    assert_eq!(app.current_message_id, Some(7));
    assert!(app.is_processing);
    // status should have advanced into Streaming via the existing branch,
    // not been touched by the wake-up code path.
    assert!(matches!(app.status, ProcessingStatus::Streaming));
}
