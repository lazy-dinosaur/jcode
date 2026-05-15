use super::{
    SpawnCapError, count_active_owned_workers, enforce_spawn_caps, ensure_spawn_coordinator_swarm,
    prepare_visible_spawn_session, register_visible_spawned_member, require_coordinator_swarm,
    resolve_max_active_spawns_per_coordinator, resolve_spawn_working_dir,
    resolve_stop_target_session, swarm_force_headless_spawn, swarm_stop_allowed_by_owner,
    validate_spawn_working_dir,
};
use crate::agent::Agent;
use crate::message::{Message, ToolDefinition};
use crate::protocol::{NotificationType, ServerEvent};
use crate::provider::{EventStream, Provider};
use crate::server::{SwarmEventType, SwarmMember, VersionedPlan};
use crate::tool::Registry;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Instant;
use tokio::sync::{Mutex, RwLock, broadcast, mpsc};

struct MockProvider;

#[async_trait]
impl Provider for MockProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        Err(anyhow::anyhow!("mock provider should not be called"))
    }

    fn name(&self) -> &str {
        "mock"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(MockProvider)
    }
}

fn member(
    session_id: &str,
    swarm_id: Option<&str>,
    role: &str,
) -> (SwarmMember, mpsc::UnboundedReceiver<ServerEvent>) {
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    (
        SwarmMember {
            session_id: session_id.to_string(),
            event_tx,
            event_txs: HashMap::new(),
            working_dir: None,
            swarm_id: swarm_id.map(|id| id.to_string()),
            swarm_enabled: true,
            status: "ready".to_string(),
            detail: None,
            friendly_name: Some(session_id.to_string()),
            report_back_to_session_id: None,
            run_id: None,
            latest_completion_report: None,
            role: role.to_string(),
            joined_at: Instant::now(),
            last_status_change: Instant::now(),
            last_heartbeat_at: Some(Instant::now()),
            last_tool: None,
            last_checkpoint: None,
            is_headless: false,
        },
        event_rx,
    )
}

async fn test_agent_with_working_dir(session_id: &str, working_dir: &str) -> Arc<Mutex<Agent>> {
    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut session = crate::session::Session::create_with_id(session_id.to_string(), None, None);
    session.model = Some("mock".to_string());
    session.working_dir = Some(working_dir.to_string());
    let mut agent = Agent::new_with_session(provider, registry, session, None);
    agent.set_working_dir(working_dir);
    Arc::new(Mutex::new(agent))
}

#[tokio::test]
async fn resolve_spawn_working_dir_prefers_explicit_dir() {
    let sessions = Arc::new(RwLock::new(HashMap::new()));
    let spawner_dir = tempfile::TempDir::new().expect("spawner dir");
    let spawner_dir_string = spawner_dir.path().display().to_string();
    sessions.write().await.insert(
        "req".to_string(),
        test_agent_with_working_dir("req", &spawner_dir_string).await,
    );
    let swarm_members = Arc::new(RwLock::new(HashMap::new()));

    assert_eq!(
        resolve_spawn_working_dir(
            Some("/tmp/explicit".to_string()),
            "req",
            &sessions,
            &swarm_members,
        )
        .await
        .as_deref(),
        Some("/tmp/explicit")
    );
}

#[tokio::test]
async fn resolve_spawn_working_dir_falls_back_to_member_dir() {
    let sessions = Arc::new(RwLock::new(HashMap::new()));
    let swarm_members = Arc::new(RwLock::new(HashMap::new()));
    let (mut req_member, _rx) = member("req", Some("swarm-1"), "coordinator");
    req_member.working_dir = Some(std::path::PathBuf::from("/tmp/member-dir"));
    swarm_members
        .write()
        .await
        .insert("req".to_string(), req_member);

    assert_eq!(
        resolve_spawn_working_dir(None, "req", &sessions, &swarm_members)
            .await
            .as_deref(),
        Some("/tmp/member-dir")
    );
}

#[tokio::test]
async fn resolve_spawn_working_dir_prefers_live_agent_cwd_over_member_dir() {
    let sessions = Arc::new(RwLock::new(HashMap::new()));
    let parent_dir = tempfile::TempDir::new().expect("parent dir");
    let parent_dir_string = parent_dir.path().display().to_string();
    sessions.write().await.insert(
        "req".to_string(),
        test_agent_with_working_dir("req", &parent_dir_string).await,
    );

    let swarm_members = Arc::new(RwLock::new(HashMap::new()));
    let (mut req_member, _rx) = member("req", Some("swarm-1"), "coordinator");
    req_member.working_dir = Some(std::path::PathBuf::from("/tmp/stale-member-dir"));
    swarm_members
        .write()
        .await
        .insert("req".to_string(), req_member);

    assert_eq!(
        resolve_spawn_working_dir(None, "req", &sessions, &swarm_members)
            .await
            .as_deref(),
        Some(parent_dir_string.as_str())
    );
}

#[test]
fn stop_permission_defaults_to_sessions_spawned_by_requesting_coordinator() {
    let (mut owned, _owned_rx) = member("worker-owned", Some("swarm-1"), "agent");
    owned.report_back_to_session_id = Some("coord".to_string());
    let (mut user_created, _user_rx) = member("worker-user", Some("swarm-1"), "agent");
    user_created.report_back_to_session_id = None;
    let (mut other_owned, _other_rx) = member("worker-other", Some("swarm-1"), "agent");
    other_owned.report_back_to_session_id = Some("other-coord".to_string());

    assert!(swarm_stop_allowed_by_owner("coord", &owned, false));
    assert!(!swarm_stop_allowed_by_owner("coord", &user_created, false));
    assert!(!swarm_stop_allowed_by_owner("coord", &other_owned, false));
    assert!(swarm_stop_allowed_by_owner("coord", &user_created, true));
}

fn owned_worker(
    session_id: &str,
    coordinator: &str,
    status: &str,
    run_id: Option<&str>,
) -> SwarmMember {
    let (mut worker, _rx) = member(session_id, Some("swarm-1"), "agent");
    worker.report_back_to_session_id = Some(coordinator.to_string());
    worker.status = status.to_string();
    worker.run_id = run_id.map(str::to_string);
    worker
}

fn insert_owned_workers(
    members: &mut HashMap<String, SwarmMember>,
    coordinator: &str,
    count: usize,
    run_id: Option<&str>,
) {
    for index in 0..count {
        let id = format!("worker-{}-{index}", run_id.unwrap_or("none"));
        members.insert(id.clone(), owned_worker(&id, coordinator, "ready", run_id));
    }
}

struct EnvVarGuard {
    name: &'static str,
    previous: Option<String>,
}

impl EnvVarGuard {
    fn set(name: &'static str, value: &str) -> Self {
        let previous = std::env::var(name).ok();
        // SAFETY: each test holds `stage3_env_lock()` while mutating these vars.
        unsafe { std::env::set_var(name, value) };
        Self { name, previous }
    }

    fn unset(name: &'static str) -> Self {
        let previous = std::env::var(name).ok();
        // SAFETY: each test holds `stage3_env_lock()` while mutating these vars.
        unsafe { std::env::remove_var(name) };
        Self { name, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        // SAFETY: each test holds `stage3_env_lock()` while guards are dropped.
        unsafe {
            match self.previous.take() {
                Some(previous) => std::env::set_var(self.name, previous),
                None => std::env::remove_var(self.name),
            }
        }
    }
}

fn stage3_env_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

#[test]
fn count_active_owned_workers_excludes_terminal_states() {
    let mut members = HashMap::new();
    for (status, expected_active) in [
        ("ready", true),
        ("running", true),
        ("spawned", true),
        ("running_stale", true),
        ("headless", true),
        ("completed", false),
        ("crashed", false),
        ("failed", false),
        ("closed", false),
        ("disconnected", false),
    ] {
        let id = format!("{status}-{expected_active}");
        members.insert(id.clone(), owned_worker(&id, "coord", status, None));
    }
    members.insert(
        "other-owned".to_string(),
        owned_worker("other-owned", "other", "ready", None),
    );

    assert_eq!(count_active_owned_workers(&members, "coord", None), 5);
}

#[test]
fn count_active_owned_workers_filters_by_run_id() {
    let mut members = HashMap::new();
    members.insert(
        "a".to_string(),
        owned_worker("a", "coord", "ready", Some("run-a")),
    );
    members.insert(
        "b".to_string(),
        owned_worker("b", "coord", "running", Some("run-a")),
    );
    members.insert(
        "c".to_string(),
        owned_worker("c", "coord", "ready", Some("run-b")),
    );

    assert_eq!(
        count_active_owned_workers(&members, "coord", Some("run-a")),
        2
    );
    assert_eq!(
        count_active_owned_workers(&members, "coord", Some("run-b")),
        1
    );
}

#[test]
fn resolve_caps_env_overrides_config() {
    let _guard = stage3_env_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _coord = EnvVarGuard::set("JCODE_MAX_ACTIVE_SPAWNS_PER_COORDINATOR", "2");
    assert_eq!(resolve_max_active_spawns_per_coordinator(None), 2);
}

#[test]
fn resolve_caps_zero_means_unlimited() {
    let _guard = stage3_env_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _coord = EnvVarGuard::set("JCODE_MAX_ACTIVE_SPAWNS_PER_COORDINATOR", "0");
    let mut members = HashMap::new();
    insert_owned_workers(&mut members, "coord", 100, None);

    assert!(enforce_spawn_caps(&members, "coord", None, None).is_ok());
}

#[test]
fn enforce_spawn_caps_rejects_at_coordinator_limit() {
    let _guard = stage3_env_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _coord = EnvVarGuard::set("JCODE_MAX_ACTIVE_SPAWNS_PER_COORDINATOR", "6");
    let _run = EnvVarGuard::set("JCODE_MAX_ACTIVE_SPAWNS_PER_RUN", "0");
    let mut members = HashMap::new();
    insert_owned_workers(&mut members, "coord", 6, None);

    assert_eq!(
        enforce_spawn_caps(&members, "coord", None, None).unwrap_err(),
        SpawnCapError::Coordinator { active: 6, cap: 6 }
    );
}

#[test]
fn enforce_spawn_caps_rejects_at_run_limit() {
    let _guard = stage3_env_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _coord = EnvVarGuard::set("JCODE_MAX_ACTIVE_SPAWNS_PER_COORDINATOR", "0");
    let _run = EnvVarGuard::set("JCODE_MAX_ACTIVE_SPAWNS_PER_RUN", "4");
    let mut members = HashMap::new();
    insert_owned_workers(&mut members, "coord", 4, Some("run-1"));
    insert_owned_workers(&mut members, "coord", 10, Some("run-2"));

    assert_eq!(
        enforce_spawn_caps(&members, "coord", Some("run-1"), None).unwrap_err(),
        SpawnCapError::Run { active: 4, cap: 4 }
    );
}

#[test]
fn enforce_spawn_caps_allows_when_unlimited() {
    let _guard = stage3_env_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _coord = EnvVarGuard::set("JCODE_MAX_ACTIVE_SPAWNS_PER_COORDINATOR", "0");
    let _run = EnvVarGuard::set("JCODE_MAX_ACTIVE_SPAWNS_PER_RUN", "0");
    let mut members = HashMap::new();
    insert_owned_workers(&mut members, "coord", 100, Some("run-1"));

    assert!(enforce_spawn_caps(&members, "coord", Some("run-1"), None).is_ok());
}

#[test]
fn validate_spawn_working_dir_accepts_subdir() {
    let _guard = stage3_env_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _allow = EnvVarGuard::unset("JCODE_SWARM_ALLOW_ANY_CWD");
    let root = tempfile::TempDir::new().expect("root");
    let subdir = root.path().join("child");
    std::fs::create_dir(&subdir).expect("subdir");

    let validated = validate_spawn_working_dir(Some(root.path()), &subdir).expect("valid cwd");
    assert_eq!(validated, std::fs::canonicalize(&subdir).unwrap());
}

#[test]
fn validate_spawn_working_dir_rejects_sibling() {
    let _guard = stage3_env_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _allow = EnvVarGuard::unset("JCODE_SWARM_ALLOW_ANY_CWD");
    let temp = tempfile::TempDir::new().expect("temp");
    let root = temp.path().join("a");
    let sibling = temp.path().join("b");
    std::fs::create_dir(&root).expect("root");
    std::fs::create_dir(&sibling).expect("sibling");

    let error = validate_spawn_working_dir(Some(&root), &sibling).unwrap_err();
    assert!(error.to_string().contains("not under coordinator root"));
}

#[test]
#[cfg(unix)]
fn validate_spawn_working_dir_resolves_symlinks() {
    let _guard = stage3_env_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _allow = EnvVarGuard::unset("JCODE_SWARM_ALLOW_ANY_CWD");
    let temp = tempfile::TempDir::new().expect("temp");
    let root = temp.path().join("root");
    let outside = temp.path().join("outside");
    let inside = root.join("inside");
    let link_inside = temp.path().join("link-inside");
    let link_outside = root.join("link-outside");
    std::fs::create_dir_all(&inside).expect("inside");
    std::fs::create_dir(&outside).expect("outside");
    std::os::unix::fs::symlink(&inside, &link_inside).expect("symlink inside");
    std::os::unix::fs::symlink(&outside, &link_outside).expect("symlink outside");

    assert!(validate_spawn_working_dir(Some(&root), &link_inside).is_ok());
    assert!(validate_spawn_working_dir(Some(&root), &link_outside).is_err());
}

#[test]
fn validate_spawn_working_dir_skips_when_no_coord_root() {
    let _guard = stage3_env_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _allow = EnvVarGuard::unset("JCODE_SWARM_ALLOW_ANY_CWD");
    let missing = std::path::PathBuf::from("/definitely/not/required/to/exist/stage3");
    assert_eq!(validate_spawn_working_dir(None, &missing).unwrap(), missing);
}

#[test]
fn validate_spawn_working_dir_env_override() {
    let _guard = stage3_env_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _allow = EnvVarGuard::set("JCODE_SWARM_ALLOW_ANY_CWD", "1");
    let missing = std::path::PathBuf::from("/definitely/not/required/to/exist/stage3");
    assert_eq!(
        validate_spawn_working_dir(Some(std::path::Path::new("/also/missing")), &missing).unwrap(),
        missing
    );
}

#[tokio::test]
async fn stop_target_resolves_unique_friendly_name_and_suffix() {
    let swarm_members = Arc::new(RwLock::new(HashMap::new()));
    let (mut worker, _worker_rx) = member("session_jellyfish_1234_abcd", Some("swarm-1"), "agent");
    worker.friendly_name = Some("jellyfish".to_string());
    swarm_members
        .write()
        .await
        .insert(worker.session_id.clone(), worker);

    assert_eq!(
        resolve_stop_target_session("swarm-1", "jellyfish", &swarm_members)
            .await
            .as_deref(),
        Ok("session_jellyfish_1234_abcd")
    );
    assert_eq!(
        resolve_stop_target_session("swarm-1", "abcd", &swarm_members)
            .await
            .as_deref(),
        Ok("session_jellyfish_1234_abcd")
    );
}

#[tokio::test]
async fn stop_target_rejects_ambiguous_friendly_name() {
    let swarm_members = Arc::new(RwLock::new(HashMap::new()));
    let (mut first, _first_rx) = member("session_bear_1", Some("swarm-1"), "agent");
    first.friendly_name = Some("bear".to_string());
    let (mut second, _second_rx) = member("session_bear_2", Some("swarm-1"), "agent");
    second.friendly_name = Some("bear".to_string());
    let mut members = swarm_members.write().await;
    members.insert(first.session_id.clone(), first);
    members.insert(second.session_id.clone(), second);
    drop(members);

    let err = resolve_stop_target_session("swarm-1", "bear", &swarm_members)
        .await
        .expect_err("ambiguous friendly names should be rejected");
    assert!(err.contains("Ambiguous swarm session 'bear'"));
}

#[tokio::test]
async fn register_visible_spawned_member_marks_startup_as_running() {
    let swarm_members = Arc::new(RwLock::new(HashMap::new()));
    let swarms_by_id = Arc::new(RwLock::new(HashMap::new()));
    let event_history = Arc::new(RwLock::new(VecDeque::new()));
    let event_counter = Arc::new(AtomicU64::new(0));
    let (swarm_event_tx, _swarm_event_rx) = broadcast::channel(8);

    register_visible_spawned_member(
        "child-1",
        "swarm-1",
        Some("/tmp/worktree"),
        true,
        Some("owner"),
        Some("run-visible"),
        &swarm_members,
        &swarms_by_id,
        &event_history,
        &event_counter,
        &swarm_event_tx,
    )
    .await;

    let members = swarm_members.read().await;
    let member = members.get("child-1").expect("spawned member should exist");
    assert_eq!(member.status, "running");
    assert_eq!(member.detail.as_deref(), Some("startup queued"));
    assert_eq!(member.swarm_id.as_deref(), Some("swarm-1"));
    assert_eq!(member.run_id.as_deref(), Some("run-visible"));
    assert_eq!(
        member.working_dir.as_deref(),
        Some(std::path::Path::new("/tmp/worktree"))
    );
    drop(members);

    assert!(
        swarms_by_id
            .read()
            .await
            .get("swarm-1")
            .is_some_and(|members| members.contains("child-1"))
    );

    let history = event_history.read().await;
    assert!(history.iter().any(|event| {
            event.session_id == "child-1"
                && matches!(event.event, SwarmEventType::MemberChange { ref action } if action == "joined")
        }));
}

#[test]
fn prepare_visible_spawn_session_persists_startup_before_launch() {
    let _guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    crate::env::set_var("JCODE_HOME", temp_home.path());

    let worktree = tempfile::TempDir::new().expect("temp worktree");
    let startup = "Please start by auditing prompt delivery.";

    let (session_id, launched) = prepare_visible_spawn_session(
        Some(worktree.path().to_str().expect("utf8 worktree path")),
        None,
        None,
        false,
        Some(startup),
        |session_id, _cwd: &std::path::Path, _selfdev, provider_key| {
            assert_eq!(provider_key, None);
            let path = crate::storage::jcode_dir()
                .expect("jcode dir")
                .join(format!("client-input-{}", session_id));
            let data = std::fs::read_to_string(&path).expect("startup file should exist");
            assert!(
                data.contains(startup),
                "startup payload should be written before launch"
            );
            assert!(
                data.contains(r#""submit_on_restore":true"#),
                "startup payload should auto-submit on restore"
            );
            Ok(true)
        },
    )
    .expect("visible spawn preparation should succeed");

    assert!(launched);
    let path = crate::storage::jcode_dir()
        .expect("jcode dir")
        .join(format!("client-input-{}", session_id));
    assert!(
        path.exists(),
        "startup file should remain for launched visible session"
    );

    crate::env::remove_var("JCODE_HOME");
}

#[test]
fn prepare_visible_spawn_session_cleans_startup_when_launch_not_started() {
    let _guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    crate::env::set_var("JCODE_HOME", temp_home.path());

    let worktree = tempfile::TempDir::new().expect("temp worktree");

    let (session_id, launched) = prepare_visible_spawn_session(
        Some(worktree.path().to_str().expect("utf8 worktree path")),
        None,
        None,
        false,
        Some("Do the thing."),
        |_session_id, _cwd: &std::path::Path, _selfdev, _provider_key| Ok(false),
    )
    .expect("visible spawn preparation should succeed even when launch is skipped");

    assert!(!launched);
    let path = crate::storage::jcode_dir()
        .expect("jcode dir")
        .join(format!("client-input-{}", session_id));
    assert!(
        !path.exists(),
        "startup file should be removed when visible launch does not start"
    );
    assert!(
        !crate::session::session_exists(&session_id),
        "prepared session should be cleaned up when visible launch does not start"
    );

    crate::env::remove_var("JCODE_HOME");
}

#[test]
fn prepare_visible_spawn_session_cleans_session_when_launch_errors() {
    let _guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    crate::env::set_var("JCODE_HOME", temp_home.path());

    let worktree = tempfile::TempDir::new().expect("temp worktree");

    let error = prepare_visible_spawn_session(
        Some(worktree.path().to_str().expect("utf8 worktree path")),
        None,
        None,
        false,
        Some("Do the thing."),
        |_session_id, _cwd: &std::path::Path, _selfdev, _provider_key| {
            Err(anyhow::anyhow!("launch failed"))
        },
    )
    .expect_err("visible spawn preparation should surface launch error");

    assert!(error.to_string().contains("launch failed"));
    let sessions_dir = crate::storage::jcode_dir()
        .expect("jcode dir")
        .join("sessions");
    let remaining_sessions = std::fs::read_dir(&sessions_dir)
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert_eq!(
        remaining_sessions, 0,
        "failed visible launch should not leave orphan prepared sessions"
    );

    crate::env::remove_var("JCODE_HOME");
}

#[test]
fn prepare_visible_spawn_session_persists_and_launches_provider_key_for_openrouter_model() {
    let _guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    crate::env::set_var("JCODE_HOME", temp_home.path());

    let worktree = tempfile::TempDir::new().expect("temp worktree");
    let (session_id, launched) = prepare_visible_spawn_session(
        Some(worktree.path().to_str().expect("utf8 worktree path")),
        Some("openai/gpt-5.4@OpenAI"),
        None,
        false,
        None,
        |_session_id, _cwd: &std::path::Path, _selfdev, provider_key| {
            assert_eq!(provider_key, Some("openrouter"));
            Ok(true)
        },
    )
    .expect("visible spawn preparation should succeed");

    assert!(launched);
    let session = crate::session::Session::load(&session_id).expect("prepared session should save");
    assert_eq!(session.model.as_deref(), Some("openai/gpt-5.4@OpenAI"));
    assert_eq!(session.provider_key.as_deref(), Some("openrouter"));

    crate::env::remove_var("JCODE_HOME");
}

#[test]
fn prepare_visible_spawn_session_prefers_parent_provider_key_over_model_guess() {
    let _guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temp home");
    crate::env::set_var("JCODE_HOME", temp_home.path());

    let worktree = tempfile::TempDir::new().expect("temp worktree");
    let (session_id, launched) = prepare_visible_spawn_session(
        Some(worktree.path().to_str().expect("utf8 worktree path")),
        Some("gpt-5.4"),
        Some("ollama"),
        false,
        None,
        |_session_id, _cwd: &std::path::Path, _selfdev, provider_key| {
            assert_eq!(provider_key, Some("ollama"));
            Ok(true)
        },
    )
    .expect("visible spawn preparation should succeed");

    assert!(launched);
    let session = crate::session::Session::load(&session_id).expect("prepared session should save");
    assert_eq!(session.model.as_deref(), Some("gpt-5.4"));
    assert_eq!(session.provider_key.as_deref(), Some("ollama"));

    crate::env::remove_var("JCODE_HOME");
}

#[tokio::test]
async fn spawn_bootstraps_coordinator_when_swarm_has_none() {
    let swarm_members = Arc::new(RwLock::new(HashMap::new()));
    let swarms_by_id = Arc::new(RwLock::new(HashMap::from([(
        "swarm-1".to_string(),
        HashSet::from(["req".to_string()]),
    )])));
    let swarm_coordinators = Arc::new(RwLock::new(HashMap::new()));
    let swarm_plans = Arc::new(RwLock::new(HashMap::<String, VersionedPlan>::new()));
    let (req_member, _req_rx) = member("req", Some("swarm-1"), "agent");
    swarm_members
        .write()
        .await
        .insert("req".to_string(), req_member);
    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel();

    let swarm_id = ensure_spawn_coordinator_swarm(
        1,
        "req",
        "Only the coordinator can spawn new agents.",
        &client_event_tx,
        &swarm_members,
        &swarms_by_id,
        &swarm_coordinators,
        &swarm_plans,
    )
    .await;

    assert_eq!(swarm_id.as_deref(), Some("swarm-1"));
    assert_eq!(
        swarm_coordinators
            .read()
            .await
            .get("swarm-1")
            .map(String::as_str),
        Some("req")
    );
    assert_eq!(
        swarm_members
            .read()
            .await
            .get("req")
            .map(|member| member.role.as_str()),
        Some("coordinator")
    );
    assert!(matches!(
        client_event_rx.recv().await,
        Some(ServerEvent::Notification {
            notification_type: NotificationType::Message { .. },
            message,
            ..
        }) if message == "You are the coordinator for this swarm."
    ));
}

#[tokio::test]
async fn spawn_requires_existing_coordinator_when_one_is_set() {
    let swarm_members = Arc::new(RwLock::new(HashMap::new()));
    let swarms_by_id = Arc::new(RwLock::new(HashMap::from([(
        "swarm-1".to_string(),
        HashSet::from(["req".to_string(), "coord".to_string()]),
    )])));
    let swarm_coordinators = Arc::new(RwLock::new(HashMap::from([(
        "swarm-1".to_string(),
        "coord".to_string(),
    )])));
    let swarm_plans = Arc::new(RwLock::new(HashMap::<String, VersionedPlan>::new()));
    let (req_member, _req_rx) = member("req", Some("swarm-1"), "agent");
    let (coord_member, _coord_rx) = member("coord", Some("swarm-1"), "coordinator");
    let mut members = swarm_members.write().await;
    members.insert("req".to_string(), req_member);
    members.insert("coord".to_string(), coord_member);
    drop(members);
    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel();

    let swarm_id = ensure_spawn_coordinator_swarm(
        2,
        "req",
        "Only the coordinator can spawn new agents.",
        &client_event_tx,
        &swarm_members,
        &swarms_by_id,
        &swarm_coordinators,
        &swarm_plans,
    )
    .await;

    assert!(swarm_id.is_none());
    assert!(matches!(
        client_event_rx.recv().await,
        Some(ServerEvent::Error { message, .. })
            if message == "Only the coordinator can spawn new agents."
    ));
    assert_eq!(
        swarm_members
            .read()
            .await
            .get("req")
            .map(|member| member.role.as_str()),
        Some("agent")
    );
}

#[tokio::test]
async fn coordinator_actions_self_promote_when_recorded_coordinator_is_stale() {
    let swarm_members = Arc::new(RwLock::new(HashMap::new()));
    let swarm_coordinators = Arc::new(RwLock::new(HashMap::from([(
        "swarm-1".to_string(),
        "old-coord".to_string(),
    )])));
    let (req_member, _req_rx) = member("req", Some("swarm-1"), "agent");
    let (mut old_coord, _old_rx) = member("old-coord", Some("swarm-1"), "coordinator");
    old_coord.status = "crashed".to_string();
    let mut members = swarm_members.write().await;
    members.insert("req".to_string(), req_member);
    members.insert("old-coord".to_string(), old_coord);
    drop(members);
    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel();

    let swarm_id = require_coordinator_swarm(
        3,
        "req",
        "Only the coordinator can stop agents.",
        &client_event_tx,
        &swarm_members,
        &swarm_coordinators,
    )
    .await;

    assert_eq!(swarm_id.as_deref(), Some("swarm-1"));
    assert_eq!(
        swarm_coordinators
            .read()
            .await
            .get("swarm-1")
            .map(String::as_str),
        Some("req")
    );
    assert_eq!(
        swarm_members
            .read()
            .await
            .get("req")
            .map(|member| member.role.as_str()),
        Some("coordinator")
    );
    assert!(client_event_rx.try_recv().is_err());
}

mod swarm_force_headless_spawn_tests {
    //! Lazydino M2 stage 2 — verify env-var override for visible vs headless
    //! swarm spawn. We only test the env path here; config-path behavior
    //! depends on global `config::config()` which is process-wide and unsafe
    //! to mutate from tests. The env path takes precedence over config so
    //! covering it is sufficient for the M2 user-facing surface.

    use super::swarm_force_headless_spawn;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvGuard {
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(value: &str) -> Self {
            let previous = std::env::var("JCODE_SWARM_NO_TERMINAL").ok();
            // SAFETY: tests are serialized by `env_lock()`; the env var is
            // only read by `swarm_force_headless_spawn` and the inner test
            // body. No other thread observes the transient value.
            unsafe { std::env::set_var("JCODE_SWARM_NO_TERMINAL", value) };
            Self { previous }
        }

        fn unset() -> Self {
            let previous = std::env::var("JCODE_SWARM_NO_TERMINAL").ok();
            // SAFETY: same as `set`, serialized.
            unsafe { std::env::remove_var("JCODE_SWARM_NO_TERMINAL") };
            Self { previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: serialized; restoring to a value seen on entry.
            unsafe {
                match self.previous.take() {
                    Some(prev) => std::env::set_var("JCODE_SWARM_NO_TERMINAL", prev),
                    None => std::env::remove_var("JCODE_SWARM_NO_TERMINAL"),
                }
            }
        }
    }

    #[test]
    fn env_var_set_to_1_forces_headless() {
        let _guard = env_lock().lock().unwrap();
        let _env = EnvGuard::set("1");
        assert!(
            swarm_force_headless_spawn(),
            "JCODE_SWARM_NO_TERMINAL=1 must force headless spawn"
        );
    }

    #[test]
    fn env_var_set_to_true_yes_on_forces_headless() {
        let _guard = env_lock().lock().unwrap();
        for value in ["true", "TRUE", "True", "yes", "YES", "on", "ON"] {
            let _env = EnvGuard::set(value);
            assert!(
                swarm_force_headless_spawn(),
                "JCODE_SWARM_NO_TERMINAL={value:?} must force headless spawn"
            );
        }
    }

    #[test]
    fn env_var_set_to_0_keeps_visible_attempt() {
        let _guard = env_lock().lock().unwrap();
        for value in ["0", "false", "FALSE", "no", "NO", "off", "OFF", ""] {
            let _env = EnvGuard::set(value);
            assert!(
                !swarm_force_headless_spawn(),
                "JCODE_SWARM_NO_TERMINAL={value:?} must NOT force headless spawn"
            );
        }
    }

    #[test]
    fn env_var_unset_keeps_upstream_visible_first_default() {
        let _guard = env_lock().lock().unwrap();
        let _env = EnvGuard::unset();
        // Default config has `swarm_spawn_visible = None`, which keeps
        // upstream visible-first behavior. The function must return false.
        assert!(
            !swarm_force_headless_spawn(),
            "default behavior with no env override must keep visible-first"
        );
    }

    #[test]
    fn env_var_unknown_value_falls_through_to_config_default() {
        let _guard = env_lock().lock().unwrap();
        let _env = EnvGuard::set("maybe");
        // Unknown values neither force nor un-force; we fall through to
        // config. With default config (`None`), that means visible-first.
        assert!(
            !swarm_force_headless_spawn(),
            "unknown env value must fall through to config default"
        );
    }
}
