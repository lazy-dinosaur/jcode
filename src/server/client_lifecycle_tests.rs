use super::*;
use crate::message::{Message, ToolDefinition};
use crate::provider::{EventStream, Provider};
use async_trait::async_trait;
use jcode_agent_runtime::TurnControl;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

struct IsolatedRuntimeDir {
    _prev_runtime: Option<std::ffi::OsString>,
    _temp: tempfile::TempDir,
}

#[tokio::test]
async fn session_control_handle_does_not_wait_for_busy_agent_lock() {
    let provider: Arc<dyn Provider> = Arc::new(PanicOnForkProvider {
        forked: Arc::new(AtomicBool::new(false)),
    });
    let registry = Registry::new(Arc::clone(&provider)).await;
    let agent = Arc::new(Mutex::new(Agent::new(provider, registry)));

    let queue = Arc::new(std::sync::Mutex::new(Vec::new()));
    let background_signal = InterruptSignal::new();
    let turn_control = TurnControl::new();
    let stop_signal = turn_control.stop_signal();
    let control = SessionControlHandle::new(
        "session_control_test",
        Arc::clone(&queue),
        background_signal.clone(),
        turn_control,
    );

    let _busy_agent_lock = agent.lock().await;

    tokio::time::timeout(Duration::from_millis(100), async {
        assert!(control.queue_soft_interrupt(
            "please stop".to_string(),
            true,
            SoftInterruptSource::User,
        ));
        control.request_cancel();
        assert!(control.request_background_current_tool());
        control.clear_soft_interrupts();
    })
    .await
    .expect("lock-free control operations should not wait for the agent mutex");

    assert!(stop_signal.is_set());
    assert!(background_signal.is_set());
    assert!(queue.lock().expect("queue lock").is_empty());
}

#[tokio::test]
async fn session_control_interrupt_diagnostics_report_signal_state() {
    let queue = Arc::new(std::sync::Mutex::new(Vec::new()));
    let background_signal = InterruptSignal::new();
    let turn_control = TurnControl::new();
    let control = SessionControlHandle::new(
        "session_interrupt_diag",
        Arc::clone(&queue),
        background_signal.clone(),
        turn_control,
    );

    let initial = control.interrupt_diagnostics();
    assert_eq!(initial.session_id, "session_interrupt_diag");
    assert_eq!(initial.soft_interrupt_count, 0);
    assert_eq!(initial.urgent_soft_interrupt_count, 0);
    assert!(initial.background_tool_signal_registered);
    assert!(!initial.background_tool_signal_set);
    assert!(!initial.stop_current_turn_signal_set);

    assert!(
        control.queue_soft_interrupt("soft stop".to_string(), true, SoftInterruptSource::User,)
    );
    assert!(control.request_background_current_tool());
    control.request_cancel();

    let after = control.interrupt_diagnostics();
    assert_eq!(after.soft_interrupt_count, 1);
    assert_eq!(after.urgent_soft_interrupt_count, 1);
    assert!(after.background_tool_signal_set);
    assert!(after.stop_current_turn_signal_set);

    control.reset_cancel();
    assert!(!control.interrupt_diagnostics().stop_current_turn_signal_set);
}

#[tokio::test]
async fn user_cancel_turn_control_does_not_set_graceful_reload_signal() {
    let provider: Arc<dyn Provider> = Arc::new(PanicOnForkProvider {
        forked: Arc::new(AtomicBool::new(false)),
    });
    let registry = Registry::new(Arc::clone(&provider)).await;
    let agent = Agent::new(provider, registry);
    let reload_signal = agent.graceful_shutdown_signal();
    let control = SessionControlHandle::new(
        agent.session_id().to_string(),
        agent.soft_interrupt_queue(),
        agent.background_tool_signal(),
        agent.turn_control(),
    );

    control.request_cancel();

    let diagnostics = control.interrupt_diagnostics();
    assert!(diagnostics.stop_current_turn_signal_set);
    assert_eq!(diagnostics.stop_reason.as_deref(), Some("user_interrupt"));
    assert!(control.stop_current_turn_signal().is_set());
    assert!(
        !reload_signal.is_set(),
        "user cancel must not reuse graceful reload shutdown signal"
    );

    agent.request_graceful_shutdown();
    assert!(
        reload_signal.is_set(),
        "reload signal remains independently usable"
    );
}

#[tokio::test]
async fn processing_interrupt_snapshot_tracks_active_task_without_agent_lock() {
    let mut client_is_processing = true;
    let mut processing_message_id = Some(777);
    let mut processing_session_id = Some("session_processing_diag".to_string());
    let mut processing_task = Some(tokio::spawn(async {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }));

    let snapshot = processing_interrupt_snapshot(&ProcessingState {
        client_is_processing: &mut client_is_processing,
        message_id: &mut processing_message_id,
        session_id: &mut processing_session_id,
        task: &mut processing_task,
        cancel_state: &mut ProcessingCancelState::Idle,
    });

    assert!(snapshot.client_is_processing);
    assert_eq!(snapshot.message_id, Some(777));
    assert_eq!(
        snapshot.session_id.as_deref(),
        Some("session_processing_diag")
    );
    assert!(snapshot.task_present);
    assert_eq!(snapshot.task_finished, Some(false));
}

#[tokio::test]
async fn cancel_processing_message_uses_cooperative_grace_then_abort_fallback() {
    let queue = Arc::new(std::sync::Mutex::new(Vec::new()));
    let background_signal = InterruptSignal::new();
    let turn_control = TurnControl::new();
    let stop_signal = turn_control.stop_signal();
    let control = SessionControlHandle::new(
        "session_cancel_cooperative",
        Arc::clone(&queue),
        background_signal,
        turn_control,
    );

    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel::<ServerEvent>();
    let mut client_is_processing = true;
    let mut processing_message_id = Some(4242);
    let mut processing_session_id = Some("session_cancel_cooperative".to_string());
    let mut processing_cancel_state = ProcessingCancelState::Idle;
    let stubborn_started = Arc::new(AtomicBool::new(false));
    let stubborn_started_task = Arc::clone(&stubborn_started);
    let mut processing_task = Some(tokio::spawn(async move {
        stubborn_started_task.store(true, Ordering::SeqCst);
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    }));
    while !stubborn_started.load(Ordering::SeqCst) {
        tokio::task::yield_now().await;
    }

    let swarm_members = Arc::new(RwLock::new(HashMap::new()));
    let swarms_by_id = Arc::new(RwLock::new(HashMap::new()));
    let event_history = Arc::new(RwLock::new(std::collections::VecDeque::new()));
    let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (swarm_event_tx, _) = broadcast::channel(8);

    cancel_processing_message(
        &mut ProcessingState {
            client_is_processing: &mut client_is_processing,
            message_id: &mut processing_message_id,
            session_id: &mut processing_session_id,
            task: &mut processing_task,
            cancel_state: &mut processing_cancel_state,
        },
        &control,
        &client_event_tx,
        &SwarmStatusRefs {
            members: &swarm_members,
            swarms_by_id: &swarms_by_id,
            event_history: &event_history,
            event_counter: &event_counter,
            event_tx: &swarm_event_tx,
        },
    )
    .await;

    assert!(!client_is_processing);
    assert!(processing_message_id.is_none());
    assert!(processing_session_id.is_none());
    assert!(processing_task.is_none());
    assert_eq!(processing_cancel_state, ProcessingCancelState::Idle);
    assert!(
        !stop_signal.is_set(),
        "cancel state resets after abort fallback"
    );
    assert!(matches!(
        client_event_rx.recv().await,
        Some(ServerEvent::Interrupted)
    ));
    assert!(matches!(
        client_event_rx.recv().await,
        Some(ServerEvent::Done { id: 4242 })
    ));
}

#[tokio::test]
async fn cancel_processing_message_ignores_repeated_cancel_while_cancelling() {
    let queue = Arc::new(std::sync::Mutex::new(Vec::new()));
    let background_signal = InterruptSignal::new();
    let turn_control = TurnControl::new();
    let stop_signal = turn_control.stop_signal();
    let control = SessionControlHandle::new(
        "session_cancel_idempotent",
        Arc::clone(&queue),
        background_signal,
        turn_control,
    );

    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel::<ServerEvent>();
    let mut client_is_processing = true;
    let mut processing_message_id = Some(9001);
    let mut processing_session_id = Some("session_cancel_idempotent".to_string());
    let mut processing_cancel_state = ProcessingCancelState::Cancelling;
    let mut processing_task = Some(tokio::spawn(async {
        tokio::time::sleep(Duration::from_secs(60)).await;
    }));

    let swarm_members = Arc::new(RwLock::new(HashMap::new()));
    let swarms_by_id = Arc::new(RwLock::new(HashMap::new()));
    let event_history = Arc::new(RwLock::new(std::collections::VecDeque::new()));
    let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (swarm_event_tx, _) = broadcast::channel(8);

    cancel_processing_message(
        &mut ProcessingState {
            client_is_processing: &mut client_is_processing,
            message_id: &mut processing_message_id,
            session_id: &mut processing_session_id,
            task: &mut processing_task,
            cancel_state: &mut processing_cancel_state,
        },
        &control,
        &client_event_tx,
        &SwarmStatusRefs {
            members: &swarm_members,
            swarms_by_id: &swarms_by_id,
            event_history: &event_history,
            event_counter: &event_counter,
            event_tx: &swarm_event_tx,
        },
    )
    .await;

    assert!(client_is_processing);
    assert_eq!(processing_message_id, Some(9001));
    assert_eq!(
        processing_session_id.as_deref(),
        Some("session_cancel_idempotent")
    );
    assert_eq!(processing_cancel_state, ProcessingCancelState::Cancelling);
    assert!(processing_task.is_some());
    assert!(!stop_signal.is_set());
    assert!(client_event_rx.try_recv().is_err());

    processing_task.take().unwrap().abort();
}

#[tokio::test]
async fn cancel_processing_message_waits_for_cooperative_completion_before_abort() {
    let queue = Arc::new(std::sync::Mutex::new(Vec::new()));
    let background_signal = InterruptSignal::new();
    let turn_control = TurnControl::new();
    let stop_signal = turn_control.stop_signal();
    let control = SessionControlHandle::new(
        "session_cancel_grace",
        Arc::clone(&queue),
        background_signal,
        turn_control,
    );

    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel::<ServerEvent>();
    let mut client_is_processing = true;
    let mut processing_message_id = Some(5150);
    let mut processing_session_id = Some("session_cancel_grace".to_string());
    let mut processing_cancel_state = ProcessingCancelState::Idle;
    let observed_cancel = Arc::new(AtomicBool::new(false));
    let observed_cancel_task = Arc::clone(&observed_cancel);
    let stop_signal_for_task = stop_signal.clone();
    let mut processing_task = Some(tokio::spawn(async move {
        stop_signal_for_task.notified().await;
        observed_cancel_task.store(true, Ordering::SeqCst);
    }));

    let swarm_members = Arc::new(RwLock::new(HashMap::new()));
    let swarms_by_id = Arc::new(RwLock::new(HashMap::new()));
    let event_history = Arc::new(RwLock::new(std::collections::VecDeque::new()));
    let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (swarm_event_tx, _) = broadcast::channel(8);

    let started = Instant::now();
    cancel_processing_message(
        &mut ProcessingState {
            client_is_processing: &mut client_is_processing,
            message_id: &mut processing_message_id,
            session_id: &mut processing_session_id,
            task: &mut processing_task,
            cancel_state: &mut processing_cancel_state,
        },
        &control,
        &client_event_tx,
        &SwarmStatusRefs {
            members: &swarm_members,
            swarms_by_id: &swarms_by_id,
            event_history: &event_history,
            event_counter: &event_counter,
            event_tx: &swarm_event_tx,
        },
    )
    .await;

    assert!(observed_cancel.load(Ordering::SeqCst));
    assert!(started.elapsed() < Duration::from_millis(1000));
    assert!(!client_is_processing);
    assert!(processing_message_id.is_none());
    assert!(processing_session_id.is_none());
    assert!(processing_task.is_none());
    assert_eq!(processing_cancel_state, ProcessingCancelState::Idle);
    assert!(!stop_signal.is_set());
    assert!(matches!(
        client_event_rx.recv().await,
        Some(ServerEvent::Interrupted)
    ));
    assert!(matches!(
        client_event_rx.recv().await,
        Some(ServerEvent::Done { id: 5150 })
    ));
}

impl IsolatedRuntimeDir {
    fn new() -> Self {
        let temp = tempfile::TempDir::new().expect("runtime dir");
        let prev_runtime = std::env::var_os("JCODE_RUNTIME_DIR");
        crate::env::set_var("JCODE_RUNTIME_DIR", temp.path());
        crate::server::clear_reload_marker();
        Self {
            _prev_runtime: prev_runtime,
            _temp: temp,
        }
    }
}

impl Drop for IsolatedRuntimeDir {
    fn drop(&mut self) {
        crate::server::clear_reload_marker();
        if let Some(prev_runtime) = self._prev_runtime.take() {
            crate::env::set_var("JCODE_RUNTIME_DIR", prev_runtime);
        } else {
            crate::env::remove_var("JCODE_RUNTIME_DIR");
        }
    }
}

struct PanicOnForkProvider {
    forked: Arc<AtomicBool>,
}

#[async_trait]
impl Provider for PanicOnForkProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        panic!("complete should never run in lightweight control test")
    }

    fn name(&self) -> &str {
        "panic-on-fork"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        self.forked.store(true, Ordering::SeqCst);
        panic!("fork should not run for lightweight control requests")
    }
}

#[test]
fn ping_request_is_lightweight_control_request() {
    assert!((Request::Ping { id: 1 }).is_lightweight_control_request());
}

#[test]
fn server_reload_starting_is_true_only_for_recent_starting_marker() {
    let _guard = crate::storage::lock_test_env();
    let _runtime = IsolatedRuntimeDir::new();

    assert!(!server_reload_starting());

    crate::server::write_reload_state(
        "reload-lifecycle-test",
        "test-hash",
        crate::server::ReloadPhase::Starting,
        Some("session_test_reload".to_string()),
    );
    assert!(server_reload_starting());

    crate::server::write_reload_state(
        "reload-lifecycle-test",
        "test-hash",
        crate::server::ReloadPhase::SocketReady,
        Some("session_test_reload".to_string()),
    );
    assert!(!server_reload_starting());
}

#[test]
fn reload_starting_rejects_new_turn_without_spawning_processing_task() {
    let _guard = crate::storage::lock_test_env();
    let _runtime = IsolatedRuntimeDir::new();
    crate::server::write_reload_state(
        "reload-lifecycle-starting",
        "test-hash",
        crate::server::ReloadPhase::Starting,
        Some("session_guard".to_string()),
    );

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let forked = Arc::new(AtomicBool::new(false));
        let provider: Arc<dyn Provider> = Arc::new(PanicOnForkProvider {
            forked: Arc::clone(&forked),
        });
        let registry = Registry::new(Arc::clone(&provider)).await;
        let mut session =
            crate::session::Session::create_with_id("session_guard".to_string(), None, None);
        session.model = Some("panic-on-fork".to_string());
        let agent = Arc::new(Mutex::new(Agent::new_with_session(
            provider, registry, session, None,
        )));

        let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel::<ServerEvent>();
        let (processing_done_tx, mut processing_done_rx) = mpsc::unbounded_channel();
        let mut client_is_processing = false;
        let mut processing_message_id = None;
        let mut processing_session_id = None;
        let mut processing_task = None;
        let mut processing_cancel_state = ProcessingCancelState::Idle;
        let swarm_members = Arc::new(RwLock::new(HashMap::new()));
        let swarms_by_id = Arc::new(RwLock::new(HashMap::new()));
        let event_history = Arc::new(RwLock::new(std::collections::VecDeque::new()));
        let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let (swarm_event_tx, _) = broadcast::channel(8);

        start_processing_message(
            ProcessingMessage {
                id: 42,
                content: "do not start during reload".to_string(),
                images: Vec::new(),
                system_reminder: None,
            },
            "session_guard",
            "conn_test_guard",
            &mut ProcessingState {
                client_is_processing: &mut client_is_processing,
                message_id: &mut processing_message_id,
                session_id: &mut processing_session_id,
                task: &mut processing_task,
                cancel_state: &mut processing_cancel_state,
            },
            &agent,
            &client_event_tx,
            &processing_done_tx,
            &SwarmStatusRefs {
                members: &swarm_members,
                swarms_by_id: &swarms_by_id,
                event_history: &event_history,
                event_counter: &event_counter,
                event_tx: &swarm_event_tx,
            },
        )
        .await;

        let event = client_event_rx
            .recv()
            .await
            .expect("reload event should be sent to client");
        assert!(matches!(event, ServerEvent::Reloading { new_socket: None }));
        assert!(
            client_event_rx.try_recv().is_err(),
            "reload guard should only emit the reload notification"
        );
        assert!(!client_is_processing);
        assert_eq!(processing_message_id, None);
        assert_eq!(processing_session_id, None);
        assert!(processing_task.is_none());
        assert!(processing_done_rx.try_recv().is_err());
        assert!(
            !forked.load(Ordering::SeqCst),
            "rejecting during reload should not fork or invoke provider work"
        );
    });
}

#[test]
fn reload_starting_rejects_new_turns_for_multiple_sessions() {
    let _guard = crate::storage::lock_test_env();
    let _runtime = IsolatedRuntimeDir::new();
    crate::server::write_reload_state(
        "reload-lifecycle-multi-starting",
        "test-hash",
        crate::server::ReloadPhase::Starting,
        Some("session_alpha".to_string()),
    );

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let forked = Arc::new(AtomicBool::new(false));
        let provider: Arc<dyn Provider> = Arc::new(PanicOnForkProvider {
            forked: Arc::clone(&forked),
        });
        let registry = Registry::new(Arc::clone(&provider)).await;
        let swarm_members = Arc::new(RwLock::new(HashMap::new()));
        let swarms_by_id = Arc::new(RwLock::new(HashMap::new()));
        let event_history = Arc::new(RwLock::new(std::collections::VecDeque::new()));
        let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let (swarm_event_tx, _) = broadcast::channel(8);

        for (message_id, session_id) in [
            (101, "session_alpha"),
            (102, "session_beta"),
            (103, "session_gamma"),
        ] {
            let mut session =
                crate::session::Session::create_with_id(session_id.to_string(), None, None);
            session.model = Some("panic-on-fork".to_string());
            let agent = Arc::new(Mutex::new(Agent::new_with_session(
                Arc::clone(&provider),
                registry.clone(),
                session,
                None,
            )));

            let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel::<ServerEvent>();
            let (processing_done_tx, mut processing_done_rx) = mpsc::unbounded_channel();
            let mut client_is_processing = false;
            let mut processing_message_id = None;
            let mut processing_session_id = None;
            let mut processing_task = None;
            let mut processing_cancel_state = ProcessingCancelState::Idle;

            start_processing_message(
                ProcessingMessage {
                    id: message_id,
                    content: format!("do not start {session_id} during reload"),
                    images: Vec::new(),
                    system_reminder: None,
                },
                session_id,
                "conn_test_burst",
                &mut ProcessingState {
                    client_is_processing: &mut client_is_processing,
                    message_id: &mut processing_message_id,
                    session_id: &mut processing_session_id,
                    task: &mut processing_task,
                    cancel_state: &mut processing_cancel_state,
                },
                &agent,
                &client_event_tx,
                &processing_done_tx,
                &SwarmStatusRefs {
                    members: &swarm_members,
                    swarms_by_id: &swarms_by_id,
                    event_history: &event_history,
                    event_counter: &event_counter,
                    event_tx: &swarm_event_tx,
                },
            )
            .await;

            let event = tokio::time::timeout(
                std::time::Duration::from_millis(250),
                client_event_rx.recv(),
            )
            .await
            .expect("reload guard should emit promptly for every session")
            .expect("reload event should be sent to client");
            assert!(
                matches!(event, ServerEvent::Reloading { new_socket: None }),
                "expected Reloading event for {session_id}, got {event:?}"
            );
            assert!(
                client_event_rx.try_recv().is_err(),
                "reload guard should only emit one reload notification for {session_id}"
            );
            assert!(
                !client_is_processing,
                "{session_id} should not enter processing during reload"
            );
            assert_eq!(processing_message_id, None);
            assert_eq!(processing_session_id, None);
            assert!(
                processing_task.is_none(),
                "{session_id} should not spawn a processing task during reload"
            );
            assert!(processing_done_rx.try_recv().is_err());
        }

        assert!(
            !forked.load(Ordering::SeqCst),
            "rejecting multiple sessions during reload should not fork or invoke provider work"
        );
    });
}

#[tokio::test]
async fn lightweight_comm_request_skips_full_session_initialization() {
    let (server_stream, client_stream) = crate::transport::Stream::pair().expect("socket pair");
    let forked = Arc::new(AtomicBool::new(false));
    let provider_template: Arc<dyn Provider> = Arc::new(PanicOnForkProvider {
        forked: Arc::clone(&forked),
    });

    let sessions: SessionAgents = Arc::new(RwLock::new(HashMap::new()));
    let global_session_id = Arc::new(RwLock::new(String::new()));
    let client_count = Arc::new(RwLock::new(0usize));
    let client_connections = Arc::new(RwLock::new(HashMap::new()));
    let swarm_members = Arc::new(RwLock::new(HashMap::new()));
    let swarms_by_id = Arc::new(RwLock::new(HashMap::new()));
    let shared_context = Arc::new(RwLock::new(HashMap::new()));
    let swarm_plans = Arc::new(RwLock::new(HashMap::new()));
    let swarm_coordinators = Arc::new(RwLock::new(HashMap::new()));
    let file_touches = Arc::new(RwLock::new(HashMap::new()));
    let files_touched_by_session = Arc::new(RwLock::new(HashMap::new()));
    let channel_subscriptions = Arc::new(RwLock::new(HashMap::new()));
    let channel_subscriptions_by_session = Arc::new(RwLock::new(HashMap::new()));
    let client_debug_state = Arc::new(RwLock::new(ClientDebugState::default()));
    let (_debug_response_tx, _) = broadcast::channel(8);
    let event_history = Arc::new(RwLock::new(std::collections::VecDeque::new()));
    let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (swarm_event_tx, _) = broadcast::channel(8);
    let (_global_event_tx, _) = broadcast::channel(8);
    let global_is_processing = Arc::new(RwLock::new(false));
    let shutdown_signals = Arc::new(RwLock::new(HashMap::new()));
    let soft_interrupt_queues: SessionInterruptQueues = Arc::new(RwLock::new(HashMap::new()));
    let mcp_pool = Arc::new(crate::mcp::SharedMcpPool::from_default_config());

    let server_task = tokio::spawn(handle_client(
        server_stream,
        Arc::clone(&sessions),
        _global_event_tx,
        provider_template,
        global_is_processing,
        global_session_id,
        client_count,
        Arc::clone(&client_connections),
        swarm_members,
        swarms_by_id,
        shared_context,
        swarm_plans,
        swarm_coordinators,
        file_touches,
        files_touched_by_session,
        channel_subscriptions,
        channel_subscriptions_by_session,
        client_debug_state,
        _debug_response_tx,
        event_history,
        event_counter,
        swarm_event_tx,
        "jcode-test".to_string(),
        "🧪".to_string(),
        mcp_pool,
        shutdown_signals,
        soft_interrupt_queues,
        Arc::new(RwLock::new(HashMap::new())),
        AwaitMembersRuntime::default(),
        SwarmMutationRuntime::default(),
    ));

    let (client_reader, mut client_writer) = client_stream.into_split();
    let mut client_reader = BufReader::new(client_reader);
    let request = Request::CommList {
        id: 7,
        session_id: "not-in-swarm".to_string(),
    };
    let payload = serde_json::to_string(&request).expect("serialize request") + "\n";
    client_writer
        .write_all(payload.as_bytes())
        .await
        .expect("write request");

    let mut line = String::new();
    client_reader
        .read_line(&mut line)
        .await
        .expect("read ack bytes");
    let ack = decode_request_or_event(&line);
    assert!(matches!(ack, ServerEvent::Ack { id: 7 }));

    line.clear();
    client_reader
        .read_line(&mut line)
        .await
        .expect("read terminal response");
    let response = decode_request_or_event(&line);
    match response {
        ServerEvent::Error { id, message, .. } => {
            assert_eq!(id, 7);
            assert!(message.contains("Not in a swarm"));
        }
        other => panic!("expected error response, got {other:?}"),
    }

    drop(client_writer);
    server_task
        .await
        .expect("server task join")
        .expect("server task result");

    assert!(
        !forked.load(Ordering::SeqCst),
        "lightweight control request should not fork a provider"
    );
    assert!(
        client_connections.read().await.is_empty(),
        "lightweight control request should not register a live client session"
    );
    assert!(
        sessions.read().await.is_empty(),
        "lightweight control request should not allocate a live agent session"
    );
}

fn decode_request_or_event(line: &str) -> ServerEvent {
    serde_json::from_str(line.trim()).expect("decode server event")
}
