#[allow(dead_code)]
#[path = "e2e/mock_provider.rs"]
mod mock_provider;
use anyhow::Result;
use jcode::agent::{Agent, SoftInterruptMessage, SoftInterruptSource};
use jcode::message::{ContentBlock, StreamEvent};
use jcode::protocol::ServerEvent;
use jcode::session::Session;
use jcode::tool::{Registry, Tool, ToolContext, ToolOutput};
use mock_provider::MockProvider;
use serde_json::json;
use std::ffi::OsString;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};

static JCODE_HOME_LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();

struct TestEnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    prev_home: Option<OsString>,
    prev_runtime_dir: Option<OsString>,
    prev_test_session: Option<OsString>,
    _temp_home: tempfile::TempDir,
}

impl TestEnvGuard {
    fn new() -> Result<Self> {
        let lock = JCODE_HOME_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_home = tempfile::Builder::new()
            .prefix("jcode-m22-stage2-home-")
            .tempdir()?;
        let runtime_dir = temp_home.path().join("runtime");
        std::fs::create_dir_all(&runtime_dir)?;
        let prev_home = std::env::var_os("JCODE_HOME");
        let prev_runtime_dir = std::env::var_os("JCODE_RUNTIME_DIR");
        let prev_test_session = std::env::var_os("JCODE_TEST_SESSION");
        jcode::env::set_var("JCODE_HOME", temp_home.path());
        jcode::env::set_var("JCODE_RUNTIME_DIR", &runtime_dir);
        jcode::env::set_var("JCODE_TEST_SESSION", "1");
        Ok(Self {
            _lock: lock,
            prev_home,
            prev_runtime_dir,
            prev_test_session,
            _temp_home: temp_home,
        })
    }
}

impl Drop for TestEnvGuard {
    fn drop(&mut self) {
        if let Some(prev) = &self.prev_home {
            jcode::env::set_var("JCODE_HOME", prev);
        } else {
            jcode::env::remove_var("JCODE_HOME");
        }
        if let Some(prev) = &self.prev_runtime_dir {
            jcode::env::set_var("JCODE_RUNTIME_DIR", prev);
        } else {
            jcode::env::remove_var("JCODE_RUNTIME_DIR");
        }
        if let Some(prev) = &self.prev_test_session {
            jcode::env::set_var("JCODE_TEST_SESSION", prev);
        } else {
            jcode::env::remove_var("JCODE_TEST_SESSION");
        }
    }
}

fn setup_test_env() -> Result<TestEnvGuard> {
    TestEnvGuard::new()
}

fn tool_call(id: &str, name: &str, input: serde_json::Value) -> Vec<StreamEvent> {
    vec![
        StreamEvent::ToolUseStart {
            id: id.to_string(),
            name: name.to_string(),
        },
        StreamEvent::ToolInputDelta(input.to_string()),
        StreamEvent::ToolUseEnd,
    ]
}

fn tool_call_raw(id: &str, name: &str, raw_input: &str) -> Vec<StreamEvent> {
    vec![
        StreamEvent::ToolUseStart {
            id: id.to_string(),
            name: name.to_string(),
        },
        StreamEvent::ToolInputDelta(raw_input.to_string()),
        StreamEvent::ToolUseEnd,
    ]
}

fn tool_turn(calls: Vec<Vec<StreamEvent>>) -> Vec<StreamEvent> {
    let mut events = Vec::new();
    for call in calls {
        events.extend(call);
    }
    events.push(StreamEvent::MessageEnd {
        stop_reason: Some("tool_use".to_string()),
    });
    events
}

async fn run_tool_turn(events: Vec<StreamEvent>) -> Result<(Agent, Duration)> {
    let provider = MockProvider::new();
    provider.queue_response(events);
    provider.queue_response(vec![
        StreamEvent::TextDelta("done".to_string()),
        StreamEvent::MessageEnd {
            stop_reason: Some("end_turn".to_string()),
        },
    ]);
    let provider: Arc<dyn jcode::provider::Provider> = Arc::new(provider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);
    let started = Instant::now();
    let _ = agent.run_once_capture("run tools").await?;
    Ok((agent, started.elapsed()))
}

async fn new_agent_with_mock_provider(events: Vec<StreamEvent>) -> Result<Agent> {
    let provider = MockProvider::new();
    provider.queue_response(events);
    provider.queue_response(vec![StreamEvent::MessageEnd {
        stop_reason: Some("end_turn".to_string()),
    }]);
    let provider: Arc<dyn jcode::provider::Provider> = Arc::new(provider);
    let registry = Registry::new(provider.clone()).await;
    Ok(Agent::new(provider, registry))
}

async fn run_mpsc_tool_turn(mut agent: Agent, timeout: Duration) -> Result<(Agent, Duration)> {
    let (event_tx, _event_rx) = mpsc::unbounded_channel::<ServerEvent>();
    let started = Instant::now();
    tokio::time::timeout(
        timeout,
        agent.run_once_streaming_mpsc("run streaming mpsc tools", vec![], None, event_tx),
    )
    .await??;
    Ok((agent, started.elapsed()))
}

async fn run_broadcast_tool_turn(mut agent: Agent, timeout: Duration) -> Result<(Agent, Duration)> {
    let (event_tx, _event_rx) = broadcast::channel::<ServerEvent>(64);
    let started = Instant::now();
    tokio::time::timeout(
        timeout,
        agent.run_once_streaming("run streaming broadcast tools", event_tx),
    )
    .await??;
    Ok((agent, started.elapsed()))
}

fn tool_results(session: &Session) -> Vec<(String, String, Option<bool>)> {
    session
        .messages
        .iter()
        .flat_map(|message| message.content.iter())
        .filter_map(|block| match block {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => Some((tool_use_id.clone(), content.clone(), *is_error)),
            _ => None,
        })
        .collect()
}

fn text_blocks(session: &Session) -> Vec<String> {
    session
        .messages
        .iter()
        .flat_map(|message| message.content.iter())
        .filter_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

struct HangingSelfdevTool;

#[async_trait::async_trait]
impl Tool for HangingSelfdevTool {
    fn name(&self) -> &str {
        "selfdev"
    }

    fn description(&self) -> &str {
        "Test selfdev tool that remains in-flight until aborted."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({"type":"object","properties":{}})
    }

    async fn execute(&self, _input: serde_json::Value, _ctx: ToolContext) -> Result<ToolOutput> {
        tokio::time::sleep(Duration::from_secs(10)).await;
        Ok(ToolOutput::new("unexpected selfdev completion"))
    }
}

#[tokio::test]
async fn t1_parallel_timing_two_bash_tools() -> Result<()> {
    let _env = setup_test_env()?;
    let events = tool_turn(vec![
        tool_call(
            "t1-a",
            "bash",
            json!({"command":"python3 -c 'import time; time.sleep(0.45); print(\"a\")'","timeout":5000,"notify":false}),
        ),
        tool_call(
            "t1-b",
            "bash",
            json!({"command":"python3 -c 'import time; time.sleep(0.45); print(\"b\")'","timeout":5000,"notify":false}),
        ),
    ]);

    let (agent, elapsed) = run_tool_turn(events).await?;
    let session = Session::load(agent.session_id())?;
    let results = tool_results(&session);
    assert_eq!(
        results
            .iter()
            .filter(|(_, _, err)| err != &Some(true))
            .count(),
        2
    );
    assert!(
        elapsed < Duration::from_millis(800),
        "two 450ms tools should overlap; elapsed={elapsed:?}"
    );
    Ok(())
}

#[tokio::test]
async fn t2_validation_error_is_preset_and_other_tool_runs() -> Result<()> {
    let _env = setup_test_env()?;
    let events = tool_turn(vec![
        tool_call_raw("t2-invalid", "bash", "[]"),
        tool_call(
            "t2-ok",
            "bash",
            json!({"command":"printf valid","timeout":5000,"notify":false}),
        ),
    ]);

    let (agent, _) = run_tool_turn(events).await?;
    let session = Session::load(agent.session_id())?;
    let results = tool_results(&session);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].0, "t2-invalid");
    assert!(results[0].1.contains("arguments must be a JSON object"));
    assert_eq!(results[0].2, Some(true));
    assert_eq!(results[1].0, "t2-ok");
    assert!(results[1].1.contains("valid"));
    Ok(())
}

#[tokio::test]
async fn t3_urgent_interrupt_skips_not_started_tools() -> Result<()> {
    let _env = setup_test_env()?;
    let sentinel = tempfile::NamedTempFile::new()?.into_temp_path();
    let sentinel_path = sentinel.to_string_lossy().to_string();

    let provider = MockProvider::new();
    provider.queue_response(tool_turn(vec![
        tool_call(
            "t3-running",
            "bash",
            json!({"command":"python3 -c 'import time; time.sleep(0.2); print(\"running\")'","timeout":5000,"notify":false}),
        ),
        tool_call(
            "t3-cancelled",
            "bash",
            json!({"command":format!("python3 -c 'import time; time.sleep(2); open(\"{}\", \"w\").write(\"should-not-run\")'", sentinel_path),"timeout":5000,"notify":false}),
        ),
    ]));
    provider.queue_response(vec![StreamEvent::MessageEnd {
        stop_reason: Some("end_turn".to_string()),
    }]);
    let provider: Arc<dyn jcode::provider::Provider> = Arc::new(provider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);
    let queue = agent.soft_interrupt_queue();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        queue.lock().unwrap().push(SoftInterruptMessage {
            content: "stop now".to_string(),
            urgent: true,
            source: SoftInterruptSource::User,
        });
    });

    let _ = agent.run_once_capture("run interruptible tool").await?;
    let session = Session::load(agent.session_id())?;
    let results = tool_results(&session);
    assert!(
        results.iter().any(|(id, content, is_error)| {
            id == "t3-cancelled"
                && (content.contains("Cancelled: user interrupted")
                    || content.contains("Skipped: user interrupted"))
                && *is_error == Some(true)
        }),
        "urgent interrupt should cancel or skip pending work; results={results:?}"
    );
    Ok(())
}

#[tokio::test]
async fn t4_same_path_writes_finish_without_partial_race() -> Result<()> {
    let _env = setup_test_env()?;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("same.txt");
    let path_str = path.to_string_lossy().to_string();
    let a = "A".repeat(16_384);
    let b = "B".repeat(16_384);
    let events = tool_turn(vec![
        tool_call("t4-a", "write", json!({"file_path":path_str,"content":a})),
        tool_call("t4-b", "write", json!({"file_path":path_str,"content":b})),
    ]);

    let (agent, _) = run_tool_turn(events).await?;
    let session = Session::load(agent.session_id())?;
    let results = tool_results(&session);
    assert_eq!(results.len(), 2);
    let final_content = std::fs::read_to_string(path)?;
    assert!(
        final_content == "A".repeat(16_384) || final_content == "B".repeat(16_384),
        "final file should be one complete write, not mixed/partial"
    );
    Ok(())
}

#[tokio::test]
async fn t5_tool_results_preserve_tool_use_index_order() -> Result<()> {
    let _env = setup_test_env()?;
    let events = tool_turn(vec![
        tool_call(
            "t5-first",
            "bash",
            json!({"command":"python3 -c 'import time; time.sleep(0.35); print(\"first\")'","timeout":5000,"notify":false}),
        ),
        tool_call(
            "t5-second",
            "bash",
            json!({"command":"printf second","timeout":5000,"notify":false}),
        ),
    ]);

    let (agent, _) = run_tool_turn(events).await?;
    let session = Session::load(agent.session_id())?;
    let ids: Vec<String> = tool_results(&session)
        .into_iter()
        .map(|(id, _, _)| id)
        .collect();
    assert_eq!(ids, vec!["t5-first".to_string(), "t5-second".to_string()]);
    Ok(())
}

#[tokio::test]
async fn t6_mpsc_single_tool_alt_b_moves_to_background() -> Result<()> {
    let _env = setup_test_env()?;
    let agent = new_agent_with_mock_provider(tool_turn(vec![tool_call(
        "t6-bg",
        "bash",
        json!({"command":"python3 -c 'import time; time.sleep(1.5); print(\"late\")'","timeout":5000,"notify":false}),
    )]))
    .await?;
    let background_signal = agent.background_tool_signal();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        background_signal.fire();
    });

    let (agent, elapsed) = run_mpsc_tool_turn(agent, Duration::from_secs(2)).await?;
    assert!(
        elapsed < Duration::from_millis(800),
        "Alt+B handoff should return before the 1.5s tool completes; elapsed={elapsed:?}"
    );
    let session = Session::load(agent.session_id())?;
    let results = tool_results(&session);
    assert!(
        results.iter().any(|(id, content, is_error)| {
            id == "t6-bg"
                && content.contains("moved to background")
                && content.contains("task_id")
                && is_error.is_none()
        }),
        "expected background handoff ToolResult; results={results:?}"
    );
    Ok(())
}

#[tokio::test]
async fn t7_mpsc_single_bash_reload_preserves_background_handoff() -> Result<()> {
    let _env = setup_test_env()?;
    let agent = new_agent_with_mock_provider(tool_turn(vec![tool_call(
        "t7-bash",
        "bash",
        json!({"command":"python3 -c 'import time; time.sleep(0.15); print(\"reload-ok\")'","timeout":5000,"notify":false}),
    )]))
    .await?;
    let shutdown_signal = agent.graceful_shutdown_signal();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        shutdown_signal.fire();
    });

    let (agent, elapsed) = run_mpsc_tool_turn(agent, Duration::from_secs(2)).await?;
    assert!(
        elapsed < Duration::from_millis(900),
        "bash reload handoff should return promptly; elapsed={elapsed:?}"
    );
    let session = Session::load(agent.session_id())?;
    let results = tool_results(&session);
    assert!(
        results.iter().any(|(id, content, is_error)| {
            id == "t7-bash"
                && content.contains("Command continued in background due to reload")
                && content.contains("Task ID:")
                && is_error.is_none()
        }),
        "expected bash reload background handoff; results={results:?}"
    );
    Ok(())
}

#[tokio::test]
async fn t8_mpsc_single_selfdev_reload_uses_clean_non_error_message() -> Result<()> {
    let _env = setup_test_env()?;
    let provider = MockProvider::new();
    provider.queue_response(tool_turn(vec![tool_call(
        "t8-selfdev",
        "selfdev",
        json!({"action":"reload","context":"test reload handoff"}),
    )]));
    provider.queue_response(vec![StreamEvent::MessageEnd {
        stop_reason: Some("end_turn".to_string()),
    }]);
    let provider: Arc<dyn jcode::provider::Provider> = Arc::new(provider);
    let registry = Registry::empty();
    registry
        .register("selfdev".to_string(), Arc::new(HangingSelfdevTool))
        .await;
    let agent = Agent::new(provider, registry);
    let shutdown_signal = agent.graceful_shutdown_signal();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        shutdown_signal.fire();
    });

    let (agent, elapsed) = run_mpsc_tool_turn(agent, Duration::from_secs(2)).await?;
    assert!(
        elapsed < Duration::from_millis(900),
        "selfdev reload handoff should return promptly; elapsed={elapsed:?}"
    );
    let session = Session::load(agent.session_id())?;
    let results = tool_results(&session);
    assert!(
        results.iter().any(|(id, content, is_error)| {
            id == "t8-selfdev"
                && content == "Reload initiated. Process restarting..."
                && *is_error == Some(false)
        }),
        "expected clean selfdev reload ToolResult; results={results:?}"
    );
    Ok(())
}

#[tokio::test]
async fn t9_urgent_interrupt_remaining_count_mpsc_and_broadcast() -> Result<()> {
    async fn run_with_urgent_interrupt(streaming_kind: &str) -> Result<Vec<String>> {
        let events = tool_turn(vec![
            tool_call(
                &format!("{streaming_kind}-fast"),
                "bash",
                json!({"command":"python3 -c 'import time; time.sleep(0.1); print(\"fast\")'","timeout":5000,"notify":false}),
            ),
            tool_call(
                &format!("{streaming_kind}-slow-a"),
                "bash",
                json!({"command":"python3 -c 'import time; time.sleep(2); print(\"slow-a\")'","timeout":5000,"notify":false}),
            ),
            tool_call(
                &format!("{streaming_kind}-slow-b"),
                "bash",
                json!({"command":"python3 -c 'import time; time.sleep(2); print(\"slow-b\")'","timeout":5000,"notify":false}),
            ),
        ]);
        let agent = new_agent_with_mock_provider(events).await?;
        let interrupt_queue = agent.soft_interrupt_queue();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            interrupt_queue.lock().unwrap().push(SoftInterruptMessage {
                content: "stop remaining".to_string(),
                urgent: true,
                source: SoftInterruptSource::User,
            });
        });
        let agent = if streaming_kind == "mpsc" {
            run_mpsc_tool_turn(agent, Duration::from_secs(4)).await?.0
        } else {
            run_broadcast_tool_turn(agent, Duration::from_secs(4))
                .await?
                .0
        };
        Ok(text_blocks(&Session::load(agent.session_id())?))
    }

    let _env = setup_test_env()?;
    let mpsc_texts = run_with_urgent_interrupt("mpsc").await?;
    assert!(
        mpsc_texts
            .iter()
            .any(|text| text.contains("[User interrupted: 2 remaining tool(s) skipped]")),
        "mpsc should report exact remaining count; texts={mpsc_texts:?}"
    );

    let broadcast_texts = run_with_urgent_interrupt("broadcast").await?;
    assert!(
        broadcast_texts
            .iter()
            .any(|text| text.contains("[User interrupted: 2 remaining tool(s) skipped]")),
        "broadcast should report exact remaining count; texts={broadcast_texts:?}"
    );
    Ok(())
}
