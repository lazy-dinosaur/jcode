use super::drain_and_flush_sessions;
use crate::agent::Agent;
use crate::message::{ContentBlock, Message, Role, StreamEvent, ToolDefinition};
use crate::provider::{EventStream, Provider};
use crate::session::{Session, SessionStatus};
use crate::tool::Registry;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc as tokio_mpsc;
use tokio::sync::{Mutex, RwLock};
use tokio_stream::wrappers::ReceiverStream;

struct TestProvider;

#[async_trait]
impl Provider for TestProvider {
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
        "test"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self)
    }
}

struct TestHome {
    _temp_home: tempfile::TempDir,
    previous_home: Option<std::ffi::OsString>,
}

impl TestHome {
    fn new() -> Result<Self> {
        let temp_home = tempfile::TempDir::new()?;
        let previous_home = std::env::var_os("JCODE_HOME");
        crate::env::set_var("JCODE_HOME", temp_home.path());
        Ok(Self {
            _temp_home: temp_home,
            previous_home,
        })
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        if let Some(previous_home) = self.previous_home.as_ref() {
            crate::env::set_var("JCODE_HOME", previous_home);
        } else {
            crate::env::remove_var("JCODE_HOME");
        }
    }
}

fn text_block(text: &str) -> Vec<ContentBlock> {
    vec![ContentBlock::Text {
        text: text.to_string(),
        cache_control: None,
    }]
}

fn test_agent(session_id: &str) -> Arc<Mutex<Agent>> {
    let session = Session::create_with_id(session_id.to_string(), None, None);
    let provider: Arc<dyn Provider> = Arc::new(TestProvider);
    let mut agent = Agent::new_with_session(provider, Registry::empty(), session, None);
    agent.session_mut_for_test().save().expect("initial save");
    Arc::new(Mutex::new(agent))
}

fn session_map(
    entries: Vec<(&str, Arc<Mutex<Agent>>)>,
) -> Arc<RwLock<HashMap<String, Arc<Mutex<Agent>>>>> {
    Arc::new(RwLock::new(
        entries
            .into_iter()
            .map(|(id, agent)| (id.to_string(), agent))
            .collect(),
    ))
}

#[tokio::test(flavor = "current_thread")]
async fn test_drain_persists_idle_session() -> Result<()> {
    let _env_guard = crate::storage::lock_test_env();
    let _home = TestHome::new()?;
    let agent = test_agent("drain-idle");
    {
        let mut agent = agent.lock().await;
        agent
            .session_mut_for_test()
            .add_message(Role::User, text_block("unsaved user"));
        agent
            .session_mut_for_test()
            .add_message(Role::Assistant, text_block("unsaved assistant"));
    }

    let drained = drain_and_flush_sessions(
        &session_map(vec![("drain-idle", Arc::clone(&agent))]),
        Duration::from_secs(2),
    )
    .await;

    let restored = Session::load("drain-idle")?;
    assert_eq!(drained, 1);
    assert!(matches!(restored.status, SessionStatus::Closed));
    assert!(restored.messages.iter().any(|message| {
        message.role == Role::User
            && message.content.iter().any(|block| {
                matches!(
                    block,
                    ContentBlock::Text { text, .. } if text == "unsaved user"
                )
            })
    }));
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn test_drain_marks_inflight_session_crashed() -> Result<()> {
    let _env_guard = crate::storage::lock_test_env();
    let _home = TestHome::new()?;
    let agent = test_agent("drain-inflight");
    {
        let mut agent = agent.lock().await;
        agent
            .session_mut_for_test()
            .add_message(Role::User, text_block("pending user turn"));
    }

    let drained = drain_and_flush_sessions(
        &session_map(vec![("drain-inflight", Arc::clone(&agent))]),
        Duration::from_secs(2),
    )
    .await;

    let restored = Session::load("drain-inflight")?;
    assert_eq!(drained, 1);
    assert!(matches!(
        restored.status,
        SessionStatus::Crashed { ref message }
            if message.as_deref() == Some("server shutdown drain")
    ));
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn test_drain_per_session_timeout_does_not_block_others() -> Result<()> {
    let _env_guard = crate::storage::lock_test_env();
    let _home = TestHome::new()?;
    let locked_agent = test_agent("drain-locked");
    let ready_agent = test_agent("drain-ready");
    {
        let mut agent = ready_agent.lock().await;
        agent
            .session_mut_for_test()
            .add_message(Role::User, text_block("ready user"));
        agent
            .session_mut_for_test()
            .add_message(Role::Assistant, text_block("ready assistant"));
    }

    let _held_lock = locked_agent.lock().await;
    let started = Instant::now();
    let drained = drain_and_flush_sessions(
        &session_map(vec![
            ("drain-locked", Arc::clone(&locked_agent)),
            ("drain-ready", Arc::clone(&ready_agent)),
        ]),
        Duration::from_millis(40),
    )
    .await;

    assert_eq!(drained, 1);
    assert!(started.elapsed() < Duration::from_secs(1));
    let restored = Session::load("drain-ready")?;
    assert!(matches!(restored.status, SessionStatus::Closed));
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn test_drain_returns_count() -> Result<()> {
    let _env_guard = crate::storage::lock_test_env();
    let _home = TestHome::new()?;
    let first = test_agent("drain-count-1");
    let second = test_agent("drain-count-2");
    for agent in [&first, &second] {
        let mut agent = agent.lock().await;
        agent
            .session_mut_for_test()
            .add_message(Role::User, text_block("count user"));
        agent
            .session_mut_for_test()
            .add_message(Role::Assistant, text_block("count assistant"));
    }

    let drained = drain_and_flush_sessions(
        &session_map(vec![
            ("drain-count-1", Arc::clone(&first)),
            ("drain-count-2", Arc::clone(&second)),
        ]),
        Duration::from_secs(2),
    )
    .await;

    assert_eq!(drained, 2);
    Ok(())
}
