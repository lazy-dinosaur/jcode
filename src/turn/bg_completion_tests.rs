use super::*;
use tokio::time::{Duration, timeout};

fn completion(session_id: &str, task_id: &str, notify: bool) -> BackgroundCompletion {
    BackgroundCompletion {
        task_id: task_id.to_string(),
        exit_code: 0,
        stdout: format!("stdout for {task_id}"),
        stderr: String::new(),
        duration_ms: 42,
        session_id: session_id.to_string(),
        notify,
    }
}

#[tokio::test]
async fn bg_completion_with_notify_true_sends_to_channel() {
    let session_id = format!("bg_completion_true_{}", crate::id::new_id("test"));
    let mut rx = register_bg_completion_receiver(&session_id).expect("register receiver");

    assert!(send_bg_completion(completion(&session_id, "task_true", true)));

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

    assert!(!send_bg_completion(completion(&session_id, "task_false", false)));

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

    assert!(send_bg_completion(completion(&session_id, "task_wake", true)));

    let mut woke = false;
    tokio::select! {
        received = rx.recv() => {
            let received = received.expect("completion should be received");
            assert_eq!(received.task_id, "task_wake");
            woke = true;
        }
        _ = tokio::time::sleep(Duration::from_secs(1)) => {}
    }

    assert!(woke, "session loop receiver branch should wake on completion recv");
    unregister_bg_completion_receiver(&session_id);
}
