use super::*;
#[cfg(feature = "renderer")]
use std::time::{Duration, Instant};

fn unique_mermaid(label: &str) -> String {
    format!(
        "flowchart LR\n  A[Start {label}] --> B[End {label}]\n",
        label = label
    )
}

#[test]
fn viewport_renderers_return_zero_for_empty_areas() {
    let area = ratatui::prelude::Rect::new(0, 0, 0, 0);
    let mut buf = ratatui::buffer::Buffer::empty(ratatui::prelude::Rect::new(0, 0, 1, 1));

    assert_eq!(
        super::render_image_widget_viewport(0xabc, area, &mut buf, 0, 0, 100, false),
        0
    );
    assert_eq!(
        super::render_image_widget_viewport_precise(0xabc, area, &mut buf, 0, 0, 1000, false),
        0
    );
}

#[test]
fn test_mermaid_render_queue_returns_placeholder_on_cache_miss() {
    clear_cache().ok();
    let content = unique_mermaid("placeholder_miss");

    let result = render_mermaid_deferred_with_registration(&content, Some(80), true);

    assert!(
        result.is_none(),
        "cache miss should enqueue background render and return placeholder signal"
    );
    assert!(
        debug_stats().deferred_enqueued >= 1,
        "expected the cache miss to enqueue a deferred render job"
    );
}

#[test]
#[cfg(feature = "renderer")]
fn test_mermaid_render_queue_caches_completed_render() {
    clear_cache().ok();
    let content = unique_mermaid("cache_completion");
    let hash = hash_content(&content);

    let result = render_mermaid_deferred_with_registration(&content, Some(80), false);
    assert!(
        result.is_none(),
        "initial cache miss should not render synchronously"
    );

    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if get_cached_path(hash).is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    panic!("background mermaid render did not populate the cache before timeout");
}
