#[allow(dead_code)]
#[path = "e2e/mock_provider.rs"]
mod mock_provider;
use anyhow::Result;
use jcode::agent::{Agent, SoftInterruptMessage, SoftInterruptSource};
use jcode::message::{ContentBlock, StreamEvent};
use jcode::session::Session;
use jcode::tool::Registry;
use mock_provider::MockProvider;
use serde_json::json;
use std::ffi::OsString;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
