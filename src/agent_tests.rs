use super::*;
use crate::agent::environment::EnvSnapshotDetail;
use crate::message::{Message, StreamEvent, ToolDefinition};
use crate::provider::{EventStream, Provider};
use crate::tool::{Registry, Tool, ToolContext, ToolOutput};
use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::mpsc as tokio_mpsc;
use tokio_stream::wrappers::ReceiverStream;

struct DelayedProvider {
    open_delay: Duration,
    first_event_delay: Duration,
}

struct NativeAutoCompactionProvider;

struct SequentialProvider {
    responses: std::sync::Mutex<VecDeque<Vec<StreamEvent>>>,
    calls: Arc<AtomicUsize>,
}

struct GatedToolProvider {
    tool_started: Arc<tokio::sync::Notify>,
    release_tool_end: Arc<tokio::sync::Notify>,
    calls: Arc<AtomicUsize>,
}

struct SingleToolProvider {
    calls: Arc<AtomicUsize>,
}

struct DelayTestTool;

impl SequentialProvider {
    fn new(responses: Vec<Vec<StreamEvent>>) -> (Self, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        (
            Self {
                responses: std::sync::Mutex::new(responses.into()),
                calls: calls.clone(),
            },
            calls,
        )
    }
}

impl GatedToolProvider {
    fn new() -> (Self, Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>) {
        let tool_started = Arc::new(tokio::sync::Notify::new());
        let release_tool_end = Arc::new(tokio::sync::Notify::new());
        (
            Self {
                tool_started: tool_started.clone(),
                release_tool_end: release_tool_end.clone(),
                calls: Arc::new(AtomicUsize::new(0)),
            },
            tool_started,
            release_tool_end,
        )
    }
}

#[async_trait]
impl Provider for DelayedProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        tokio::time::sleep(self.open_delay).await;

        let first_event_delay = self.first_event_delay;
        let (tx, rx) = tokio_mpsc::channel::<Result<StreamEvent>>(8);
        tokio::spawn(async move {
            tokio::time::sleep(first_event_delay).await;
            let _ = tx
                .send(Ok(StreamEvent::TextDelta("hello".to_string())))
                .await;
            let _ = tx
                .send(Ok(StreamEvent::MessageEnd {
                    stop_reason: Some("end_turn".to_string()),
                }))
                .await;
        });

        Ok(Box::pin(ReceiverStream::new(rx)))
    }

    fn name(&self) -> &str {
        "delayed"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self {
            open_delay: self.open_delay,
            first_event_delay: self.first_event_delay,
        })
    }
}

#[async_trait]
impl Provider for NativeAutoCompactionProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        let (_tx, rx) = tokio_mpsc::channel::<Result<StreamEvent>>(1);
        Ok(Box::pin(ReceiverStream::new(rx)))
    }

    fn name(&self) -> &str {
        "openai"
    }

    fn supports_compaction(&self) -> bool {
        true
    }

    fn uses_jcode_compaction(&self) -> bool {
        false
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Provider for SequentialProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let events = self
            .responses
            .lock()
            .expect("sequential provider responses lock poisoned")
            .pop_front()
            .unwrap_or_else(|| {
                vec![StreamEvent::MessageEnd {
                    stop_reason: Some("end_turn".to_string()),
                }]
            });
        let (tx, rx) = tokio_mpsc::channel::<Result<StreamEvent>>(8);
        tokio::spawn(async move {
            for event in events {
                let _ = tx.send(Ok(event)).await;
            }
        });
        Ok(Box::pin(ReceiverStream::new(rx)))
    }

    fn name(&self) -> &str {
        "sequential"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        let responses = self
            .responses
            .lock()
            .expect("sequential provider responses lock poisoned")
            .clone();
        Arc::new(Self {
            responses: std::sync::Mutex::new(responses),
            calls: self.calls.clone(),
        })
    }
}

#[async_trait]
impl Provider for GatedToolProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = tokio_mpsc::channel::<Result<StreamEvent>>(8);
        if call == 0 {
            let tool_started = self.tool_started.clone();
            let release_tool_end = self.release_tool_end.clone();
            tokio::spawn(async move {
                let _ = tx
                    .send(Ok(StreamEvent::ToolUseStart {
                        id: "call_delay".to_string(),
                        name: "delay_test".to_string(),
                    }))
                    .await;
                tool_started.notify_waiters();
                release_tool_end.notified().await;
                let _ = tx
                    .send(Ok(StreamEvent::ToolInputDelta(
                        serde_json::json!({"delay_ms": 50}).to_string(),
                    )))
                    .await;
                let _ = tx.send(Ok(StreamEvent::ToolUseEnd)).await;
                let _ = tx
                    .send(Ok(StreamEvent::MessageEnd {
                        stop_reason: Some("tool_use".to_string()),
                    }))
                    .await;
            });
        } else {
            tokio::spawn(async move {
                let _ = tx
                    .send(Ok(StreamEvent::TextDelta("done".to_string())))
                    .await;
                let _ = tx
                    .send(Ok(StreamEvent::MessageEnd {
                        stop_reason: Some("end_turn".to_string()),
                    }))
                    .await;
            });
        }
        Ok(Box::pin(ReceiverStream::new(rx)))
    }

    fn name(&self) -> &str {
        "gated-tool"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self {
            tool_started: self.tool_started.clone(),
            release_tool_end: self.release_tool_end.clone(),
            calls: self.calls.clone(),
        })
    }
}

#[async_trait]
impl Provider for SingleToolProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let events = if call == 0 {
            vec![
                StreamEvent::ToolUseStart {
                    id: "call_delay".to_string(),
                    name: "delay_test".to_string(),
                },
                StreamEvent::ToolInputDelta(serde_json::json!({"delay_ms": 50}).to_string()),
                StreamEvent::ToolUseEnd,
                StreamEvent::MessageEnd {
                    stop_reason: Some("tool_use".to_string()),
                },
            ]
        } else {
            vec![
                StreamEvent::TextDelta("done".to_string()),
                StreamEvent::MessageEnd {
                    stop_reason: Some("end_turn".to_string()),
                },
            ]
        };
        let (tx, rx) = tokio_mpsc::channel::<Result<StreamEvent>>(8);
        tokio::spawn(async move {
            for event in events {
                let _ = tx.send(Ok(event)).await;
            }
        });
        Ok(Box::pin(ReceiverStream::new(rx)))
    }

    fn name(&self) -> &str {
        "single-tool"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self {
            calls: self.calls.clone(),
        })
    }
}

#[async_trait]
impl Tool for DelayTestTool {
    fn name(&self) -> &str {
        "delay_test"
    }

    fn description(&self) -> &str {
        "Test-only delay tool"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {"delay_ms": {"type": "integer"}}
        })
    }

    async fn execute(&self, input: serde_json::Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let delay_ms = input
            .get("delay_ms")
            .and_then(|value| value.as_u64())
            .unwrap_or(50);
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        Ok(ToolOutput::new("delay done"))
    }
}

#[test]
fn tool_output_to_content_blocks_preserves_labeled_images() {
    let output = ToolOutput::new("Image ready").with_labeled_image(
        "image/png",
        "ZmFrZQ==",
        "screenshots/example.png",
    );

    let blocks = tool_output_to_content_blocks("call_1".to_string(), output);
    assert_eq!(blocks.len(), 3);

    match &blocks[0] {
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_use_id, "call_1");
            assert_eq!(content, "Image ready");
            assert_eq!(*is_error, None);
        }
        other => panic!("expected tool result, got {other:?}"),
    }

    match &blocks[1] {
        ContentBlock::Image { media_type, data } => {
            assert_eq!(media_type, "image/png");
            assert_eq!(data, "ZmFrZQ==");
        }
        other => panic!("expected image block, got {other:?}"),
    }

    match &blocks[2] {
        ContentBlock::Text { text, .. } => {
            assert!(text.contains("screenshots/example.png"));
            assert!(text.contains("preceding tool result"));
        }
        other => panic!("expected trailing label text, got {other:?}"),
    }
}

#[tokio::test]
async fn run_turn_streaming_mpsc_emits_keepalive_while_provider_is_quiet() {
    let _guard = crate::storage::lock_test_env();
    let provider: Arc<dyn Provider> = Arc::new(DelayedProvider {
        open_delay: Duration::from_secs(2),
        first_event_delay: Duration::from_secs(2),
    });
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);
    agent.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "test".to_string(),
            cache_control: None,
        }],
    );

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(async move { agent.run_turn_streaming_mpsc(tx).await });

    let mut saw_keepalive = false;
    let keepalive_deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < keepalive_deadline {
        match tokio::time::timeout(Duration::from_secs(1), rx.recv()).await {
            Ok(Some(ServerEvent::Pong { id })) => {
                assert_eq!(id, STREAM_KEEPALIVE_PONG_ID);
                saw_keepalive = true;
                break;
            }
            Ok(Some(ServerEvent::TextDelta { text })) => {
                panic!("expected keepalive before text delta, got: {text}");
            }
            Ok(Some(_)) => {}
            Ok(None) => panic!("channel closed before keepalive"),
            Err(_) => {
                assert!(
                    !task.is_finished(),
                    "streaming task finished before keepalive arrived"
                );
            }
        }
    }
    assert!(saw_keepalive, "expected keepalive before provider response");

    let mut saw_text = false;
    let text_deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < text_deadline {
        match tokio::time::timeout(Duration::from_secs(1), rx.recv()).await {
            Ok(Some(ServerEvent::TextDelta { text })) => {
                assert_eq!(text, "hello");
                saw_text = true;
                break;
            }
            Ok(Some(ServerEvent::Pong { id })) => {
                assert_eq!(id, STREAM_KEEPALIVE_PONG_ID);
            }
            Ok(Some(_)) => {}
            Ok(None) => panic!("channel closed before text delta"),
            Err(_) => {
                assert!(
                    !task.is_finished(),
                    "streaming task finished before text delta arrived"
                );
            }
        }
    }

    assert!(saw_text, "expected delayed provider text after keepalive");
    task.await.unwrap().unwrap();
}

fn empty_response_events() -> Vec<StreamEvent> {
    vec![StreamEvent::MessageEnd {
        stop_reason: Some("end_turn".to_string()),
    }]
}

fn text_response_events(text: &str) -> Vec<StreamEvent> {
    vec![
        StreamEvent::TextDelta(text.to_string()),
        StreamEvent::MessageEnd {
            stop_reason: Some("end_turn".to_string()),
        },
    ]
}

fn add_tool_result_message(agent: &mut Agent) {
    agent.add_message(
        Role::User,
        vec![ContentBlock::ToolResult {
            tool_use_id: "call_empty_retry".to_string(),
            content: "tool output that needs a response".to_string(),
            is_error: None,
        }],
    );
}

fn assistant_text_messages(agent: &Agent) -> Vec<String> {
    agent
        .session
        .messages
        .iter()
        .filter(|message| message.role == Role::Assistant)
        .flat_map(|message| message.content.iter())
        .filter_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn empty_response_after_tool_result_is_retried_once_via_streaming_mpsc() {
    let _guard = crate::storage::lock_test_env();
    let retry_text = "Here is the concise follow-up.";
    let (provider, calls) = SequentialProvider::new(vec![
        empty_response_events(),
        text_response_events(retry_text),
    ]);
    let provider: Arc<dyn Provider> = Arc::new(provider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);
    add_tool_result_message(&mut agent);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    agent.run_turn_streaming_mpsc(tx).await.unwrap();

    let text_events: Vec<String> = std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|event| match event {
            ServerEvent::TextDelta { text } => Some(text),
            _ => None,
        })
        .collect();

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(text_events.iter().any(|text| text == retry_text));
    assert!(
        assistant_text_messages(&agent)
            .iter()
            .any(|text| text == retry_text)
    );
}

#[tokio::test]
async fn empty_response_without_prior_tool_result_does_not_retry() {
    let _guard = crate::storage::lock_test_env();
    let (provider, calls) = SequentialProvider::new(vec![
        empty_response_events(),
        text_response_events("should not be requested"),
    ]);
    let provider: Arc<dyn Provider> = Arc::new(provider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);
    agent.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "plain prompt".to_string(),
            cache_control: None,
        }],
    );

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    agent.run_turn_streaming_mpsc(tx).await.unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(assistant_text_messages(&agent).is_empty());
}

#[tokio::test]
async fn second_empty_response_after_tool_result_does_not_retry_again() {
    let _guard = crate::storage::lock_test_env();
    let (provider, calls) = SequentialProvider::new(vec![
        empty_response_events(),
        empty_response_events(),
        text_response_events("should not be requested"),
    ]);
    let provider: Arc<dyn Provider> = Arc::new(provider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);
    add_tool_result_message(&mut agent);

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    agent.run_turn_streaming_mpsc(tx).await.unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(assistant_text_messages(&agent).is_empty());
}

#[tokio::test]
async fn messages_for_provider_replays_persisted_native_compaction_in_auto_mode() {
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);

    agent.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "first".to_string(),
            cache_control: None,
        }],
    );
    agent.add_message(
        Role::Assistant,
        vec![ContentBlock::Text {
            text: "second".to_string(),
            cache_control: None,
        }],
    );

    agent
        .apply_openai_native_compaction("enc_auto".to_string(), 1)
        .expect("persist native compaction");

    let (messages, event) = agent.messages_for_provider();
    assert!(event.is_none());
    assert!(!messages.is_empty());
    match &messages[0].content[0] {
        ContentBlock::OpenAICompaction { encrypted_content } => {
            assert_eq!(encrypted_content, "enc_auto");
        }
        other => panic!("expected OpenAI compaction block, got {other:?}"),
    }
    assert!(
        messages
            .iter()
            .any(|message| message.role == Role::Assistant)
    );
}

#[tokio::test]
async fn oversized_openai_native_compaction_is_persisted_as_text_fallback() {
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);

    agent.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "first".to_string(),
            cache_control: None,
        }],
    );
    agent.add_message(
        Role::Assistant,
        vec![ContentBlock::Text {
            text: "second".to_string(),
            cache_control: None,
        }],
    );

    let oversized =
        "x".repeat(crate::provider::openai_request::OPENAI_ENCRYPTED_CONTENT_SAFE_MAX_CHARS + 1);
    agent
        .apply_openai_native_compaction(oversized, 1)
        .expect("persist fallback compaction");

    let state = agent
        .session
        .compaction
        .as_ref()
        .expect("compaction should be persisted");
    assert!(state.openai_encrypted_content.is_none());
    assert!(
        state
            .summary_text
            .contains("OpenAI native compaction state was discarded")
    );

    let (messages, event) = agent.messages_for_provider();
    assert!(event.is_none());
    assert!(!messages.is_empty());
    assert!(messages.iter().all(|message| {
        message
            .content
            .iter()
            .all(|block| !matches!(block, ContentBlock::OpenAICompaction { .. }))
    }));
    match &messages[0].content[0] {
        ContentBlock::Text { text, .. } => {
            assert!(text.contains("Previous Conversation Summary"));
            assert!(text.contains("OpenAI native compaction state was discarded"));
        }
        other => panic!("expected text fallback summary, got {other:?}"),
    }
    assert!(
        messages
            .iter()
            .any(|message| message.role == Role::Assistant)
    );
}

// ── InterruptSignal tests ────────────────────────────────────────────────

#[tokio::test]
async fn interrupt_signal_fire_before_notified_does_not_hang() {
    // Regression test: fire() called BEFORE notified().await must not hang.
    // The old code called notify_waiters() which drops the notification if
    // nobody is waiting yet. The flag is still set so the fast path catches it,
    // but only if the future is created before the flag check.
    let sig = InterruptSignal::new();
    sig.fire(); // fire before anyone is waiting
    tokio::time::timeout(std::time::Duration::from_millis(100), sig.notified())
        .await
        .expect("notified() hung when signal was already set before call");
}

#[tokio::test]
async fn interrupt_signal_fire_concurrent_with_notified() {
    // Regression test for the race window: fire() is called concurrently while
    // notified() is being set up. The fix (create future before flag check) ensures
    // the notify_waiters() in fire() wakes the registered future.
    let sig = Arc::new(InterruptSignal::new());
    let sig2 = Arc::clone(&sig);

    // Spawn a task that fires after a tiny delay, giving the main task time to
    // enter notified() but before it reaches notified().await.
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        sig2.fire();
    });

    tokio::time::timeout(std::time::Duration::from_millis(500), sig.notified())
        .await
        .expect("notified() hung during concurrent fire()");
}

#[tokio::test]
async fn interrupt_signal_is_set_false_initially() {
    let sig = InterruptSignal::new();
    assert!(!sig.is_set());
}

#[tokio::test]
async fn interrupt_signal_is_set_true_after_fire() {
    let sig = InterruptSignal::new();
    sig.fire();
    assert!(sig.is_set());
}

#[tokio::test]
async fn interrupt_signal_reset_clears_flag() {
    let sig = InterruptSignal::new();
    sig.fire();
    assert!(sig.is_set());
    sig.reset();
    assert!(!sig.is_set());
}

#[tokio::test]
async fn interrupt_signal_altb_early_race_fire_survives_until_reset() {
    let sig = InterruptSignal::new();
    sig.fire();

    tokio::time::timeout(Duration::from_millis(100), sig.notified())
        .await
        .expect("early fire should wake notified() immediately while the flag remains set");

    sig.reset();
    tokio::time::timeout(Duration::from_millis(25), sig.notified())
        .await
        .expect_err("reset signal should not wake notified() without a new fire");
}

#[tokio::test]
async fn interrupt_signal_notified_completes_after_fire() {
    let sig = Arc::new(InterruptSignal::new());
    let sig2 = Arc::clone(&sig);

    let handle = tokio::spawn(async move {
        sig2.notified().await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    sig.fire();

    tokio::time::timeout(std::time::Duration::from_millis(200), handle)
        .await
        .expect("notified() task timed out after fire()")
        .expect("task panicked");
}

#[tokio::test]
async fn turn_streaming_mpsc_altb_early_race_preserves_fire_after_tool_start() {
    let _guard = crate::storage::lock_test_env();
    let (provider, tool_started, release_tool_end) = GatedToolProvider::new();
    let provider: Arc<dyn Provider> = Arc::new(provider);
    let registry = Registry::empty();
    registry
        .register("delay_test".to_string(), Arc::new(DelayTestTool))
        .await;
    let mut agent = Agent::new(provider, registry);
    let background_signal = agent.background_tool_signal();
    agent.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "run the delay tool".to_string(),
            cache_control: None,
        }],
    );

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(async move { agent.run_turn_streaming_mpsc(tx).await });

    tokio::time::timeout(Duration::from_millis(500), tool_started.notified())
        .await
        .expect("provider should emit ToolStart");
    background_signal.fire();
    release_tool_end.notify_waiters();

    let mut saw_background_done = false;
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
            Ok(Some(ServerEvent::ToolDone { output, .. }))
                if output.contains("moved to background") =>
            {
                saw_background_done = true;
                break;
            }
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(_) => {}
        }
    }

    assert!(
        saw_background_done,
        "Alt+B fired after ToolStart should detach the running tool"
    );
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("turn should finish")
        .expect("turn task should not panic")
        .expect("turn should succeed");
}

/// Regression test for M8 (Alt+B leaves TUI processing-stuck because the
/// parent agent kept driving the turn loop after detach).
///
/// Without the M8 fix, after the user presses Alt+B and the running tool is
/// adopted into the background pool, the parent `run_turn_streaming_mpsc`
/// would fall through to "Point D" + the next provider iteration. That kept
/// `is_processing=true` on the TUI side and blocked client-side queued
/// messages from dispatching, so users perceived Alt+B as "having no effect".
///
/// With the fix, the Alt+B branch returns immediately after recording the
/// background ToolResult. We assert this by observing that the second
/// provider iteration (which `GatedToolProvider` would happily answer with
/// "done") never fires — `provider_calls` must stay at 1.
#[tokio::test]
async fn turn_streaming_mpsc_altb_ends_turn_immediately_after_detach() {
    let _guard = crate::storage::lock_test_env();
    let (provider, tool_started, release_tool_end) = GatedToolProvider::new();
    let provider_calls = provider.calls.clone();
    let provider: Arc<dyn Provider> = Arc::new(provider);
    let registry = Registry::empty();
    registry
        .register("delay_test".to_string(), Arc::new(DelayTestTool))
        .await;
    let mut agent = Agent::new(provider, registry);
    let background_signal = agent.background_tool_signal();
    agent.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "run the delay tool".to_string(),
            cache_control: None,
        }],
    );

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(async move { agent.run_turn_streaming_mpsc(tx).await });

    // Wait until provider has actually emitted ToolUseStart.
    tokio::time::timeout(Duration::from_millis(500), tool_started.notified())
        .await
        .expect("provider should emit ToolStart");
    // User presses Alt+B while the tool is running.
    background_signal.fire();
    // Simulate the underlying tool task finishing later in the background.
    release_tool_end.notify_waiters();

    // With the M8 fix the run_turn task returns Ok(()) right after the
    // detach branch records the synthetic ToolResult, instead of looping
    // back into another provider call.
    let task_result = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("turn must complete promptly after Alt+B detach")
        .expect("turn task must not panic");
    task_result.expect("turn must succeed");

    // Exactly one provider call (the original turn). Without the fix the
    // parent would have started a second LLM call to react to the synthetic
    // ToolResult — that's the behavior that left TUI stuck in
    // is_processing=true.
    assert_eq!(
        provider_calls.load(Ordering::SeqCst),
        1,
        "Alt+B detach must end the current turn instead of starting another provider call"
    );

    // Sanity: the moved-to-background ToolDone must actually have been
    // emitted, otherwise the test could pass for the wrong reason
    // (e.g. provider failed before detach branch ran).
    let mut events: Vec<ServerEvent> = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    let saw_background_done = events.iter().any(|e| {
        matches!(
            e,
            ServerEvent::ToolDone { output, .. } if output.contains("moved to background")
        )
    });
    assert!(
        saw_background_done,
        "expected a ToolDone with 'moved to background' marker"
    );
}

#[tokio::test]
async fn turn_streaming_mpsc_clears_stale_background_signal_before_next_tool_start() {
    let _guard = crate::storage::lock_test_env();
    let provider: Arc<dyn Provider> = Arc::new(SingleToolProvider {
        calls: Arc::new(AtomicUsize::new(0)),
    });
    let registry = Registry::empty();
    registry
        .register("delay_test".to_string(), Arc::new(DelayTestTool))
        .await;
    let mut agent = Agent::new(provider, registry);
    agent.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "run the delay tool".to_string(),
            cache_control: None,
        }],
    );
    agent.background_tool_signal().fire();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    agent.run_turn_streaming_mpsc(tx).await.unwrap();

    let tool_done_outputs: Vec<String> = std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|event| match event {
            ServerEvent::ToolDone { output, .. } => Some(output),
            _ => None,
        })
        .collect();

    assert!(
        tool_done_outputs
            .iter()
            .any(|output| output == "delay done"),
        "stale background signal should be cleared before the tool starts"
    );
    assert!(
        tool_done_outputs
            .iter()
            .all(|output| !output.contains("moved to background")),
        "stale background signal must not auto-background a later tool"
    );
}

#[tokio::test]
async fn new_agent_registers_active_pid_and_clear_swaps_it() {
    let _guard = crate::storage::lock_test_env();
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);

    let first_session_id = agent.session_id().to_string();
    assert!(
        crate::session::active_session_ids().contains(&first_session_id),
        "fresh agent session should be tracked as active"
    );

    agent.clear();

    let second_session_id = agent.session_id().to_string();
    let active = crate::session::active_session_ids();
    assert_ne!(first_session_id, second_session_id);
    assert!(
        active.contains(&second_session_id),
        "replacement session should be tracked as active"
    );
    assert!(
        !active.contains(&first_session_id),
        "cleared session should no longer be tracked as active"
    );
}

fn seed_transient_session_state(agent: &mut Agent) {
    agent.push_alert("pending alert".to_string());
    agent.queue_soft_interrupt(
        "queued interrupt".to_string(),
        true,
        SoftInterruptSource::User,
    );
    agent.background_tool_signal.fire();
    agent.request_graceful_shutdown();
    agent.tool_call_ids.insert("tool_call_old".to_string());
    agent.tool_result_ids.insert("tool_result_old".to_string());
    agent.tool_output_scan_index = 7;
    agent.last_upstream_provider = Some("upstream_old".to_string());
    agent.last_connection_type = Some("websocket".to_string());
    agent.current_turn_system_reminder = Some("reminder".to_string());
    agent.pending_lifecycle_system_reminder = Some("pending lifecycle".to_string());
    agent.last_usage = TokenUsage {
        input_tokens: 11,
        output_tokens: 17,
        cache_read_input_tokens: Some(3),
        cache_creation_input_tokens: Some(5),
    };
    agent.locked_tools = Some(vec![ToolDefinition {
        name: "test_tool".to_string(),
        description: "test tool".to_string(),
        input_schema: serde_json::json!({"type": "object"}),
    }]);
}

#[tokio::test]
async fn clear_resets_runtime_interrupt_and_queue_state() {
    let _guard = crate::storage::lock_test_env();
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);

    seed_transient_session_state(&mut agent);
    assert_eq!(agent.soft_interrupt_count(), 1);
    assert!(agent.background_tool_signal().is_set());
    assert!(agent.graceful_shutdown_signal().is_set());

    agent.clear();

    assert_eq!(agent.soft_interrupt_count(), 0);
    assert!(!agent.background_tool_signal().is_set());
    assert!(!agent.graceful_shutdown_signal().is_set());
    assert_eq!(agent.pending_alert_count(), 0);
    assert!(agent.tool_call_ids.is_empty());
    assert!(agent.tool_result_ids.is_empty());
    assert_eq!(agent.tool_output_scan_index, 0);
    assert!(agent.last_upstream_provider.is_none());
    assert!(agent.last_connection_type.is_none());
    assert!(agent.current_turn_system_reminder.is_none());
    assert!(agent.pending_lifecycle_system_reminder.is_none());
    assert_eq!(agent.last_usage.input_tokens, 0);
    assert_eq!(agent.last_usage.output_tokens, 0);
    assert!(agent.locked_tools.is_none());
}

#[tokio::test]
async fn restore_session_resets_runtime_interrupt_and_queue_state() {
    let _guard = crate::storage::lock_test_env();
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);

    let mut restored_session = crate::session::Session::create_with_id(
        "session_restore_resets_runtime_state".to_string(),
        None,
        None,
    );
    restored_session.save().expect("save restored session");

    seed_transient_session_state(&mut agent);
    assert_eq!(agent.soft_interrupt_count(), 1);
    assert!(agent.background_tool_signal().is_set());
    assert!(agent.graceful_shutdown_signal().is_set());

    let status = agent
        .restore_session(&restored_session.id)
        .expect("restore session should succeed");

    assert_eq!(status, crate::session::SessionStatus::Active);
    assert_eq!(agent.session_id(), restored_session.id);
    assert_eq!(agent.soft_interrupt_count(), 0);
    assert!(!agent.background_tool_signal().is_set());
    assert!(!agent.graceful_shutdown_signal().is_set());
    assert_eq!(agent.pending_alert_count(), 0);
    assert!(agent.tool_call_ids.is_empty());
    assert!(agent.tool_result_ids.is_empty());
    assert_eq!(agent.tool_output_scan_index, 0);
    assert!(agent.last_upstream_provider.is_none());
    assert!(agent.last_connection_type.is_none());
    assert!(agent.current_turn_system_reminder.is_none());
    assert!(agent.pending_lifecycle_system_reminder.is_none());
    assert_eq!(agent.last_usage.input_tokens, 0);
    assert_eq!(agent.last_usage.output_tokens, 0);
    assert!(agent.locked_tools.is_none());
}

#[tokio::test]
async fn lifecycle_hook_reason_becomes_next_turn_system_reminder() {
    let _guard = crate::storage::lock_test_env();
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);

    agent.set_pending_lifecycle_system_reminder("finish the checklist".to_string());
    let reminder = agent.take_pending_lifecycle_system_reminder().unwrap();

    assert!(reminder.contains("lifecycle hook denied completion"));
    assert!(reminder.contains("finish the checklist"));
    assert!(agent.pending_lifecycle_system_reminder.is_none());
}

#[test]
fn lifecycle_hook_reminder_merges_with_existing_turn_reminder() {
    let merged = Agent::merge_current_and_pending_system_reminders(
        Some("existing reminder".to_string()),
        Some("lifecycle reminder".to_string()),
    )
    .unwrap();

    assert_eq!(merged, "existing reminder\n\nlifecycle reminder");
}

// --- M11 stage 6: immediate continuation regression tests -------------------

#[tokio::test]
async fn lifecycle_deny_cap_three_allows_three_immediate_continuations_then_stops() {
    let _guard = crate::storage::lock_test_env();
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);

    for expected_streak in 1..=3 {
        let outcome = agent
            .handle_lifecycle_hook_deny_with_cap_for_tests(format!("deny {expected_streak}"), 3);
        assert!(matches!(
            outcome,
            super::turn_loops::LifecycleHookOutcome::ContinueImmediate
        ));
        assert_eq!(agent.lifecycle_deny_streak_for_tests(), expected_streak);
        assert!(agent.take_pending_lifecycle_system_reminder().is_some());
    }

    let outcome = agent.handle_lifecycle_hook_deny_with_cap_for_tests("deny 4".to_string(), 3);
    assert!(matches!(
        outcome,
        super::turn_loops::LifecycleHookOutcome::Stop
    ));
    assert_eq!(agent.lifecycle_deny_streak_for_tests(), 3);
    let pending = agent.take_pending_lifecycle_system_reminder().unwrap();
    assert!(pending.contains("deny 4"));
}

#[tokio::test]
async fn lifecycle_deny_cap_zero_allows_unlimited_immediate_continuations() {
    let _guard = crate::storage::lock_test_env();
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);

    for expected_streak in 1..=5 {
        let outcome = agent
            .handle_lifecycle_hook_deny_with_cap_for_tests(format!("deny {expected_streak}"), 0);
        assert!(matches!(
            outcome,
            super::turn_loops::LifecycleHookOutcome::ContinueImmediate
        ));
        assert_eq!(agent.lifecycle_deny_streak_for_tests(), expected_streak);
        assert!(agent.take_pending_lifecycle_system_reminder().is_some());
    }
}

#[tokio::test]
async fn lifecycle_deny_streak_resets_at_new_user_turn_start() {
    let _guard = crate::storage::lock_test_env();
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);

    agent.handle_lifecycle_hook_deny_with_cap_for_tests("first".to_string(), 3);
    agent.handle_lifecycle_hook_deny_with_cap_for_tests("second".to_string(), 3);
    assert_eq!(agent.lifecycle_deny_streak_for_tests(), 2);

    agent.reset_lifecycle_deny_streak_for_user_turn();

    assert_eq!(agent.lifecycle_deny_streak_for_tests(), 0);
}

/// M11 stage 6 fix: continuation must end the conversation with a user message
/// so the next LLM call is valid (Anthropic API rejects assistant-last). The
/// reminder is injected as a user-authored `<system-reminder>` block, matching
/// the claude-code stop-hook pattern.
#[tokio::test]
async fn lifecycle_continuation_appends_user_message_with_reminder() {
    let _guard = crate::storage::lock_test_env();
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);

    // Simulate a turn that ended with an assistant message (which is what a
    // real response.completed deny would see at the tail of the transcript).
    agent.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "say hello".to_string(),
            cache_control: None,
        }],
    );
    agent.add_message(
        Role::Assistant,
        vec![ContentBlock::Text {
            text: "Hello!".to_string(),
            cache_control: None,
        }],
    );
    let messages_before = agent.session.messages.len();

    // Stage 1+2 path: deny populates pending reminder.
    agent.handle_lifecycle_hook_deny_with_cap_for_tests(
        "Update handoff.md before stopping.".to_string(),
        3,
    );
    assert!(agent.pending_lifecycle_system_reminder.is_some());

    // Stage 6 fix: continuation injects reminder as user message.
    agent.inject_lifecycle_reminder_for_continuation_for_tests();

    assert_eq!(
        agent.session.messages.len(),
        messages_before + 1,
        "continuation must add exactly one user message"
    );

    let last = agent.session.messages.last().unwrap();
    assert!(
        matches!(last.role, Role::User),
        "last message after continuation injection must be Role::User, got {:?}",
        last.role
    );
    let text = match &last.content[0] {
        ContentBlock::Text { text, .. } => text.as_str(),
        other => panic!("expected ContentBlock::Text, got {other:?}"),
    };
    assert!(
        text.contains("<system-reminder>") && text.contains("</system-reminder>"),
        "injected message should wrap reminder in <system-reminder> tags, got: {text}"
    );
    assert!(
        text.contains("Update handoff.md before stopping."),
        "injected message should contain the reminder text from the deny reason, got: {text}"
    );
    assert!(
        text.contains("A lifecycle hook denied completion"),
        "injected message should contain the lifecycle_hook_reminder prefix, got: {text}"
    );

    // Reminder is consumed (take semantics).
    assert!(
        agent.pending_lifecycle_system_reminder.is_none(),
        "pending reminder must be drained after continuation injection"
    );

    // System prompt area (`current_turn_system_reminder`) is intentionally
    // untouched — continuation uses inline user-message channel only.
    assert!(
        agent.current_turn_system_reminder.is_none(),
        "current_turn_system_reminder must NOT be set by continuation path \
         (it is reserved for the next-user-turn fallback)"
    );
}

/// M11 stage 6 fix: guard against edge case where ContinueImmediate fires but
/// no pending reminder exists (defensive, shouldn't happen in practice).
#[tokio::test]
async fn lifecycle_continuation_is_noop_when_no_pending_reminder() {
    let _guard = crate::storage::lock_test_env();
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);

    let messages_before = agent.session.messages.len();
    assert!(agent.pending_lifecycle_system_reminder.is_none());

    agent.inject_lifecycle_reminder_for_continuation_for_tests();

    assert_eq!(
        agent.session.messages.len(),
        messages_before,
        "no message should be added when pending reminder is None"
    );
}

#[test]
fn lifecycle_deny_streak_env_override_beats_config() {
    let _guard = crate::storage::lock_test_env();
    let previous = std::env::var("JCODE_MAX_LIFECYCLE_DENY_STREAK").ok();
    crate::env::set_var("JCODE_MAX_LIFECYCLE_DENY_STREAK", "1");

    assert_eq!(
        Agent::resolve_max_lifecycle_deny_streak_with_config(Some(10)),
        1
    );

    match previous {
        Some(value) => crate::env::set_var("JCODE_MAX_LIFECYCLE_DENY_STREAK", value),
        None => crate::env::remove_var("JCODE_MAX_LIFECYCLE_DENY_STREAK"),
    }
}

#[test]
fn lifecycle_deny_streak_config_and_default_resolution() {
    let _guard = crate::storage::lock_test_env();
    let previous = std::env::var("JCODE_MAX_LIFECYCLE_DENY_STREAK").ok();
    crate::env::remove_var("JCODE_MAX_LIFECYCLE_DENY_STREAK");

    assert_eq!(
        Agent::resolve_max_lifecycle_deny_streak_with_config(Some(0)),
        0
    );
    assert_eq!(
        Agent::resolve_max_lifecycle_deny_streak_with_config(Some(9)),
        9
    );
    assert_eq!(
        Agent::resolve_max_lifecycle_deny_streak_with_config(None),
        Agent::DEFAULT_MAX_LIFECYCLE_DENY_STREAK
    );

    match previous {
        Some(value) => crate::env::set_var("JCODE_MAX_LIFECYCLE_DENY_STREAK", value),
        None => crate::env::remove_var("JCODE_MAX_LIFECYCLE_DENY_STREAK"),
    }
}

#[tokio::test]
async fn restore_session_rehydrates_injected_memory_ids() {
    let _guard = crate::storage::lock_test_env();
    crate::memory::clear_all_pending_memory();

    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);

    let mut restored_session = crate::session::Session::create_with_id(
        "session_restore_memory_dedup".to_string(),
        None,
        None,
    );
    restored_session.record_memory_injection(
        "🧠 auto-recalled 1 memory".to_string(),
        "persisted memory".to_string(),
        1,
        5,
        vec!["memory-persisted".to_string()],
    );
    restored_session.save().expect("save restored session");

    crate::memory::mark_memories_injected(&restored_session.id, &["memory-stale".to_string()]);

    agent
        .restore_session(&restored_session.id)
        .expect("restore session should succeed");

    assert!(crate::memory::is_memory_injected(
        &restored_session.id,
        "memory-persisted"
    ));
    assert!(
        !crate::memory::is_memory_injected(&restored_session.id, "memory-stale"),
        "restore should replace stale in-memory dedup state with persisted session data"
    );

    crate::memory::clear_all_pending_memory();
}

#[tokio::test]
async fn build_memory_prompt_nonblocking_defers_pending_memory_during_tool_loop() {
    let _guard = crate::storage::lock_test_env();
    crate::memory::clear_all_pending_memory();

    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let agent = Agent::new(provider, registry);
    let session_id = agent.session.id.clone();

    crate::memory::set_pending_memory_with_ids(
        &session_id,
        "remember this later".to_string(),
        1,
        vec!["memory-deferred".to_string()],
    );

    let tool_loop_messages = vec![
        Message::user("hello"),
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "call_1".to_string(),
                name: "bash".to_string(),
                input: serde_json::json!({}),
            }],
            timestamp: Some(chrono::Utc::now()),
            tool_duration_ms: None,
        },
        Message::tool_result("call_1", "ok", false),
    ];

    let pending = agent.build_memory_prompt_nonblocking(&tool_loop_messages, None);
    assert!(pending.is_none(), "memory should not inject mid tool loop");
    assert!(crate::memory::has_pending_memory(&session_id));

    let next_turn_messages = vec![Message::user("follow up")];
    let pending = agent.build_memory_prompt_nonblocking(&next_turn_messages, None);
    assert!(
        pending.is_some(),
        "memory should inject on the next real user turn"
    );
    assert!(!crate::memory::has_pending_memory(&session_id));

    crate::memory::clear_all_pending_memory();
}

#[tokio::test]
async fn mark_closed_persists_soft_interrupts_for_restore_after_reload() {
    let _guard = crate::storage::lock_test_env();
    let temp = tempfile::TempDir::new().expect("temp dir");
    let prev_home = std::env::var_os("JCODE_HOME");
    crate::env::set_var("JCODE_HOME", temp.path());

    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider.clone(), registry.clone());
    let session_id = agent.session_id().to_string();
    agent.session.save().expect("save active session");
    agent.queue_soft_interrupt(
        "resume me after reload".to_string(),
        true,
        SoftInterruptSource::System,
    );

    agent.mark_closed();

    let mut restored = Agent::new(provider, registry);
    restored
        .restore_session(&session_id)
        .expect("restore session with persisted interrupts");

    assert_eq!(restored.soft_interrupt_count(), 1);
    assert!(restored.has_urgent_interrupt());
    assert!(
        crate::soft_interrupt_store::load(&session_id)
            .expect("store should be readable after restore")
            .is_empty()
    );

    if let Some(prev_home) = prev_home {
        crate::env::set_var("JCODE_HOME", prev_home);
    } else {
        crate::env::remove_var("JCODE_HOME");
    }
}

#[tokio::test]
async fn env_snapshot_detail_is_minimal_for_empty_sessions_and_full_after_history() {
    let _guard = crate::storage::lock_test_env();
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);

    assert_eq!(agent.env_snapshot_detail(), EnvSnapshotDetail::Minimal);
    let minimal = agent.build_env_snapshot("create", agent.env_snapshot_detail());
    assert!(minimal.jcode_git_hash.is_none());
    assert!(minimal.jcode_git_dirty.is_none());
    assert!(minimal.working_git.is_none());

    agent
        .session
        .append_stored_message(crate::session::StoredMessage {
            id: "msg_env_snapshot_detail".to_string(),
            role: crate::message::Role::User,
            content: vec![ContentBlock::Text {
                text: "hello".to_string(),
                cache_control: None,
            }],
            display_role: None,
            timestamp: None,
            tool_duration_ms: None,
            token_usage: None,
        });

    assert_eq!(agent.env_snapshot_detail(), EnvSnapshotDetail::Full);
}

// --- M11 stage 5: lifecycle hook payload context enrichment ----------------

/// Helper for stage 5 tests: append a User text message to the session
/// without going through `add_message` (which auto-fills metadata that
/// doesn't matter for the context-extraction logic).
fn push_user_text(agent: &mut Agent, text: &str) {
    agent
        .session
        .append_stored_message(crate::session::StoredMessage {
            id: format!("msg_user_{}", agent.session.messages.len()),
            role: crate::message::Role::User,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
                cache_control: None,
            }],
            display_role: None,
            timestamp: None,
            tool_duration_ms: None,
            token_usage: None,
        });
}

fn push_assistant_tool_use(agent: &mut Agent, name: &str, input: serde_json::Value) {
    agent
        .session
        .append_stored_message(crate::session::StoredMessage {
            id: format!("msg_asst_{}", agent.session.messages.len()),
            role: crate::message::Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: format!("tool_use_{}", agent.session.messages.len()),
                name: name.to_string(),
                input,
            }],
            display_role: None,
            timestamp: None,
            tool_duration_ms: None,
            token_usage: None,
        });
}

#[tokio::test]
async fn lifecycle_hook_last_user_message_returns_most_recent_user_text() {
    let _guard = crate::storage::lock_test_env();
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);

    assert!(agent.lifecycle_hook_last_user_message().is_none());

    push_user_text(&mut agent, "first ask");
    push_assistant_tool_use(&mut agent, "bash", serde_json::json!({"command": "ls"}));
    push_user_text(&mut agent, "second ask");

    assert_eq!(
        agent.lifecycle_hook_last_user_message().as_deref(),
        Some("second ask")
    );
}

#[tokio::test]
async fn lifecycle_hook_last_user_message_skips_system_reminder_injections() {
    let _guard = crate::storage::lock_test_env();
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);

    push_user_text(&mut agent, "real user input");
    // <system-reminder> messages are internally injected (memory, lifecycle
    // hook denies, etc.) and must not be reported as user input.
    push_user_text(
        &mut agent,
        "<system-reminder>\nyou forgot to commit\n</system-reminder>",
    );

    assert_eq!(
        agent.lifecycle_hook_last_user_message().as_deref(),
        Some("real user input"),
        "system-reminder messages must be skipped"
    );
}

#[tokio::test]
async fn lifecycle_hook_last_user_message_truncates_long_text() {
    let _guard = crate::storage::lock_test_env();
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);

    let long = "a".repeat(crate::hooks::LIFECYCLE_HOOK_LAST_USER_MESSAGE_MAX + 50);
    push_user_text(&mut agent, &long);

    let extracted = agent.lifecycle_hook_last_user_message().unwrap();
    // Expect max_chars chars + a single '…' (1 char) appended.
    let char_count = extracted.chars().count();
    assert_eq!(
        char_count,
        crate::hooks::LIFECYCLE_HOOK_LAST_USER_MESSAGE_MAX + 1
    );
    assert!(extracted.ends_with('…'));
}

#[tokio::test]
async fn lifecycle_hook_recent_tool_calls_keeps_last_n_in_chronological_order() {
    let _guard = crate::storage::lock_test_env();
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);

    assert!(agent.lifecycle_hook_recent_tool_calls().is_empty());

    // Push more than LIFECYCLE_HOOK_RECENT_TOOL_CALLS_MAX tool uses; the
    // oldest must drop, the newest must remain, and ordering must be
    // oldest-of-kept-window first so hook scripts can read it as a tail.
    for i in 0..(crate::hooks::LIFECYCLE_HOOK_RECENT_TOOL_CALLS_MAX + 2) {
        push_assistant_tool_use(
            &mut agent,
            "bash",
            serde_json::json!({"command": format!("echo {i}")}),
        );
    }

    let recent = agent.lifecycle_hook_recent_tool_calls();
    assert_eq!(
        recent.len(),
        crate::hooks::LIFECYCLE_HOOK_RECENT_TOOL_CALLS_MAX
    );
    // First kept entry must be index=2 (the first 2 were dropped); last
    // must be the most recent one we pushed (max+1).
    assert!(recent.first().unwrap().args_preview.contains("echo 2"));
    let max_plus_one = crate::hooks::LIFECYCLE_HOOK_RECENT_TOOL_CALLS_MAX + 1;
    assert!(
        recent
            .last()
            .unwrap()
            .args_preview
            .contains(&format!("echo {max_plus_one}"))
    );
}

#[tokio::test]
async fn lifecycle_hook_recent_tool_calls_truncates_long_args_preview() {
    let _guard = crate::storage::lock_test_env();
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);

    let long_command = "x".repeat(crate::hooks::LIFECYCLE_HOOK_TOOL_ARGS_PREVIEW_MAX + 50);
    push_assistant_tool_use(
        &mut agent,
        "bash",
        serde_json::json!({"command": long_command}),
    );

    let recent = agent.lifecycle_hook_recent_tool_calls();
    assert_eq!(recent.len(), 1);
    let preview = &recent[0].args_preview;
    // Truncated previews end with `…` and are max+1 chars (max + ellipsis).
    assert!(preview.ends_with('…'));
    assert_eq!(
        preview.chars().count(),
        crate::hooks::LIFECYCLE_HOOK_TOOL_ARGS_PREVIEW_MAX + 1
    );
}

#[tokio::test]
async fn lifecycle_hook_turn_count_counts_distinct_user_turns() {
    let _guard = crate::storage::lock_test_env();
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);

    assert_eq!(agent.lifecycle_hook_turn_count(), 0);

    push_user_text(&mut agent, "turn 1");
    push_assistant_tool_use(&mut agent, "bash", serde_json::json!({"command": "true"}));
    push_user_text(&mut agent, "turn 2");
    // A reminder injection is not a real turn.
    push_user_text(
        &mut agent,
        "<system-reminder>\ninjected\n</system-reminder>",
    );

    assert_eq!(
        agent.lifecycle_hook_turn_count(),
        2,
        "system-reminder injections must not bump the user-turn count"
    );
}

#[tokio::test]
async fn lifecycle_hook_session_age_is_non_negative_and_reasonable() {
    let _guard = crate::storage::lock_test_env();
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let agent = Agent::new(provider, registry);

    // Freshly created session: age is < 2 seconds in practice but we only
    // need to verify it's a sane u64 (no panic on negative duration).
    let age = agent.lifecycle_hook_session_age_seconds();
    assert!(age < 120, "fresh session age should be tiny, got {age}");
}
