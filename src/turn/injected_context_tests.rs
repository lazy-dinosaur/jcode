use super::*;
use std::time::{Duration, UNIX_EPOCH};

fn custom_context(key: &str, body: &str, timestamp_ms: u64) -> InjectedContext {
    InjectedContext {
        source: InjectedSource::Custom {
            reason: "test".to_string(),
            body: body.to_string(),
        },
        timestamp: UNIX_EPOCH + Duration::from_millis(timestamp_ms),
        format: InjectionFormat::SystemReminder,
        dedupe_key: key.to_string(),
    }
}

fn session_id(test_name: &str) -> String {
    format!(
        "injected-context-test-{test_name}-{}",
        std::thread::current().name().unwrap_or("unnamed")
    )
}

#[test]
fn enqueue_then_drain_returns_same_context() {
    let session_id = session_id("enqueue_then_drain_returns_same_context");
    let ctx = custom_context("custom:one", "hello", 1);

    enqueue_injection(&session_id, ctx.clone()).unwrap();
    let drained = drain_injections(&session_id).unwrap();

    assert_eq!(drained, vec![ctx]);
}

#[test]
fn dedupe_by_key_skips_duplicates() {
    let session_id = session_id("dedupe_by_key_skips_duplicates");
    let first = custom_context("custom:dup", "first", 1);
    let duplicate = custom_context("custom:dup", "second", 2);

    enqueue_injection(&session_id, first.clone()).unwrap();
    enqueue_injection(&session_id, duplicate).unwrap();
    let drained = drain_injections(&session_id).unwrap();

    assert_eq!(drained, vec![first]);
}

#[test]
fn truncates_body_at_max_bytes() {
    let session_id = session_id("truncates_body_at_max_bytes");
    let oversized = "a".repeat(InjectedContext::MAX_BODY_BYTES + 32);
    let ctx = custom_context("custom:oversized", &oversized, 1);

    enqueue_injection(&session_id, ctx).unwrap();
    let drained = drain_injections(&session_id).unwrap();

    let InjectedSource::Custom { body, .. } = &drained[0].source else {
        panic!("expected custom source");
    };
    assert_eq!(body.len(), InjectedContext::MAX_BODY_BYTES);
}

#[test]
fn system_reminder_format_renders_expected_header() {
    let ctx = InjectedContext {
        source: InjectedSource::BackgroundTask {
            task_id: "297670ykyb".to_string(),
            exit_code: 0,
            stdout: "done".to_string(),
            stderr: String::new(),
            duration_ms: 42,
        },
        timestamp: UNIX_EPOCH,
        format: InjectionFormat::SystemReminder,
        dedupe_key: "bg:297670ykyb".to_string(),
    };

    let rendered = ctx.render();

    assert!(rendered.starts_with("[system-reminder]\nBackground task completed."));
    assert!(rendered.contains("- task_id: 297670ykyb"));
    assert!(rendered.contains("- stdout (truncated to 16KB):\ndone"));
}

#[test]
fn multiple_injections_drain_in_timestamp_order() {
    let session_id = session_id("multiple_injections_drain_in_timestamp_order");
    let later = custom_context("custom:later", "later", 20);
    let earlier = custom_context("custom:earlier", "earlier", 10);

    enqueue_injection(&session_id, later.clone()).unwrap();
    enqueue_injection(&session_id, earlier.clone()).unwrap();
    let drained = drain_injections(&session_id).unwrap();

    assert_eq!(drained, vec![earlier, later]);
}

#[test]
fn drain_clears_queue() {
    let session_id = session_id("drain_clears_queue");
    enqueue_injection(&session_id, custom_context("custom:one", "hello", 1)).unwrap();

    let first = drain_injections(&session_id).unwrap();
    let second = drain_injections(&session_id).unwrap();

    assert_eq!(first.len(), 1);
    assert!(second.is_empty());
}
