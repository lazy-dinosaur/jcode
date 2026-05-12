use super::*;
use tokio::time::{Duration, timeout};

fn completion(session_id: &str, task_id: &str, notify: bool) -> BackgroundCompletion {
    BackgroundCompletion {
        task_id: task_id.to_string(),
        exit_code: 0,
        stdout: format!("stdout for {task_id}"),
        stderr: format!("stderr for {task_id}"),
        duration_ms: 42,
        session_id: session_id.to_string(),
        notify,
        auto_inject: true,
        auto_inject_format: crate::turn::injected_context::InjectionFormat::SystemReminder,
        auto_inject_max_bytes: None,
    }
}

#[tokio::test]
async fn bg_completion_with_notify_true_sends_to_channel() {
    let session_id = format!("bg_completion_true_{}", crate::id::new_id("test"));
    let mut rx = register_bg_completion_receiver(&session_id).expect("register receiver");

    assert!(send_bg_completion(completion(
        &session_id,
        "task_true",
        true
    )));

    let received = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("completion should be delivered")
        .expect("channel should remain open");
    assert_eq!(received.task_id, "task_true");
    assert_eq!(received.session_id, session_id);
    unregister_bg_completion_receiver(&received.session_id);
}

#[tokio::test]
async fn bg_completion_with_notify_false_does_not_send() {
    let session_id = format!("bg_completion_false_{}", crate::id::new_id("test"));
    let mut rx = register_bg_completion_receiver(&session_id).expect("register receiver");

    assert!(!send_bg_completion(completion(
        &session_id,
        "task_false",
        false
    )));

    timeout(Duration::from_millis(100), rx.recv())
        .await
        .expect_err("notify=false should not deliver a completion");
    unregister_bg_completion_receiver(&session_id);
}

#[tokio::test]
async fn multiple_completions_queue_in_channel_order() {
    let session_id = format!("bg_completion_order_{}", crate::id::new_id("test"));
    let mut rx = register_bg_completion_receiver(&session_id).expect("register receiver");

    assert!(send_bg_completion(completion(&session_id, "task_1", true)));
    assert!(send_bg_completion(completion(&session_id, "task_2", true)));
    assert!(send_bg_completion(completion(&session_id, "task_3", true)));

    let mut task_ids = Vec::new();
    for _ in 0..3 {
        task_ids.push(
            timeout(Duration::from_secs(1), rx.recv())
                .await
                .expect("completion should be delivered")
                .expect("channel should remain open")
                .task_id,
        );
    }
    assert_eq!(task_ids, ["task_1", "task_2", "task_3"]);
    unregister_bg_completion_receiver(&session_id);
}

#[tokio::test]
async fn session_loop_wakes_on_bg_completion_recv() {
    let session_id = format!("bg_completion_wake_{}", crate::id::new_id("test"));
    let mut rx = register_bg_completion_receiver(&session_id).expect("register receiver");

    assert!(send_bg_completion(completion(
        &session_id,
        "task_wake",
        true
    )));

    let mut woke = false;
    tokio::select! {
        received = rx.recv() => {
            let received = received.expect("completion should be received");
            assert_eq!(received.task_id, "task_wake");
            woke = true;
        }
        _ = tokio::time::sleep(Duration::from_secs(1)) => {}
    }

    assert!(
        woke,
        "session loop receiver branch should wake on completion recv"
    );
    unregister_bg_completion_receiver(&session_id);
}

#[test]
fn bg_completion_with_auto_inject_true_enqueues_context() {
    let session_id = format!("bg_completion_inject_true_{}", crate::id::new_id("test"));
    let completion = completion(&session_id, "task_inject_true", true);

    assert!(enqueue_bg_completion_injection(&session_id, &completion).unwrap());

    let drained = crate::turn::injected_context::drain_injections(&session_id).unwrap();
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].dedupe_key, "bg:task_inject_true");
    match &drained[0].source {
        crate::turn::injected_context::InjectedSource::BackgroundTask {
            task_id,
            stdout,
            stderr,
            ..
        } => {
            assert_eq!(task_id, "task_inject_true");
            assert_eq!(stdout, "stdout for task_inject_true");
            assert_eq!(stderr, "stderr for task_inject_true");
        }
        other => panic!("expected background task source, got {other:?}"),
    }
}

#[test]
fn bg_completion_with_auto_inject_false_does_not_enqueue() {
    let session_id = format!("bg_completion_inject_false_{}", crate::id::new_id("test"));
    let mut completion = completion(&session_id, "task_inject_false", true);
    completion.auto_inject = false;

    assert!(!enqueue_bg_completion_injection(&session_id, &completion).unwrap());

    let drained = crate::turn::injected_context::drain_injections(&session_id).unwrap();
    assert!(drained.is_empty());
}

#[test]
fn inject_dedupe_key_is_bg_task_id() {
    let session_id = format!("bg_completion_dedupe_{}", crate::id::new_id("test"));
    let completion = completion(&session_id, "task_dedupe", true);

    enqueue_bg_completion_injection(&session_id, &completion).unwrap();
    enqueue_bg_completion_injection(&session_id, &completion).unwrap();

    let drained = crate::turn::injected_context::drain_injections(&session_id).unwrap();
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].dedupe_key, "bg:task_dedupe");
}

#[test]
fn inject_truncates_stdout_at_max_bytes() {
    let session_id = format!("bg_completion_stdout_cap_{}", crate::id::new_id("test"));
    let mut completion = completion(&session_id, "task_stdout_cap", true);
    completion.stdout = "a".repeat(64);
    completion.auto_inject_max_bytes = Some(10);

    enqueue_bg_completion_injection(&session_id, &completion).unwrap();

    let drained = crate::turn::injected_context::drain_injections(&session_id).unwrap();
    let crate::turn::injected_context::InjectedSource::BackgroundTask { stdout, .. } =
        &drained[0].source
    else {
        panic!("expected background task source");
    };
    assert_eq!(stdout.len(), 10);
}

#[test]
fn stderr_uses_smaller_cap_than_stdout() {
    let session_id = format!("bg_completion_stderr_cap_{}", crate::id::new_id("test"));
    let mut completion = completion(&session_id, "task_stderr_cap", true);
    completion.stdout = "o".repeat(4096);
    completion.stderr = "e".repeat(4096);
    completion.auto_inject_max_bytes = Some(4096);

    enqueue_bg_completion_injection(&session_id, &completion).unwrap();

    let drained = crate::turn::injected_context::drain_injections(&session_id).unwrap();
    let crate::turn::injected_context::InjectedSource::BackgroundTask { stdout, stderr, .. } =
        &drained[0].source
    else {
        panic!("expected background task source");
    };
    assert_eq!(stdout.len(), 4096);
    assert_eq!(stderr.len(), 2048);
}
