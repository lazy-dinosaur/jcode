#[tokio::test]
#[ignore = "requires real OpenAI OAuth credentials"]
async fn live_openai_catalog_lists_gpt_5_4_family() -> Result<()> {
    let Some(catalog) = live_openai_catalog().await? else {
        eprintln!("skipping live OpenAI catalog test: no real OAuth credentials");
        return Ok(());
    };

    crate::provider::populate_context_limits(catalog.context_limits.clone());
    crate::provider::populate_account_models(catalog.available_models.clone());

    assert!(
        catalog
            .available_models
            .iter()
            .any(|model| model.starts_with("gpt-5.4")),
        "expected GPT-5.4 family in live catalog, got {:?}",
        catalog.available_models
    );
    assert!(
        crate::provider::known_openai_model_ids()
            .iter()
            .any(|model| model == "gpt-5.4"),
        "expected GPT-5.4 in display model list"
    );

    let reports_long_context = catalog
        .context_limits
        .get("gpt-5.4")
        .copied()
        .unwrap_or_default()
        >= 1_000_000;
    assert_eq!(
        crate::provider::known_openai_model_ids()
            .iter()
            .any(|model| model == "gpt-5.4[1m]"),
        reports_long_context,
        "displayed 1m alias should follow the live catalog"
    );

    Ok(())
}

#[tokio::test]
#[ignore = "requires real OpenAI OAuth credentials"]
async fn live_openai_gpt_5_4_and_fast_requests_succeed() -> Result<()> {
    let Some(catalog) = live_openai_catalog().await? else {
        eprintln!("skipping live OpenAI response test: no real OAuth credentials");
        return Ok(());
    };
    crate::provider::populate_context_limits(catalog.context_limits.clone());
    crate::provider::populate_account_models(catalog.available_models.clone());

    let Some(plain_response) = live_openai_smoke("gpt-5.4", "JCODE_GPT54_OK").await? else {
        eprintln!("skipping live OpenAI response test: no real OAuth credentials");
        return Ok(());
    };
    assert!(
        plain_response.contains("JCODE_GPT54_OK"),
        "unexpected GPT-5.4 response: {}",
        plain_response
    );

    if catalog
        .available_models
        .iter()
        .any(|model| model == "gpt-5.3-codex-spark")
    {
        let Some(fast_response) =
            live_openai_smoke("gpt-5.3-codex-spark", "JCODE_GPT53_SPARK_OK").await?
        else {
            eprintln!("skipping live OpenAI fast-model test: no real OAuth credentials");
            return Ok(());
        };
        assert!(
            fast_response.contains("JCODE_GPT53_SPARK_OK"),
            "unexpected gpt-5.3-codex-spark response: {}",
            fast_response
        );
    }

    if crate::provider::known_openai_model_ids()
        .iter()
        .any(|model| model == "gpt-5.4[1m]")
    {
        let Some(long_context_response) =
            live_openai_smoke("gpt-5.4[1m]", "JCODE_GPT54_1M_OK").await?
        else {
            eprintln!("skipping live OpenAI 1m test: no real OAuth credentials");
            return Ok(());
        };
        assert!(
            long_context_response.contains("JCODE_GPT54_1M_OK"),
            "unexpected GPT-5.4[1m] response: {}",
            long_context_response
        );
    }

    Ok(())
}

#[test]
fn test_should_prefer_websocket_enabled_for_named_models() {
    assert!(OpenAIProvider::should_prefer_websocket(
        "gpt-5.3-codex-spark"
    ));
    assert!(OpenAIProvider::should_prefer_websocket("gpt-5.3-codex"));
    assert!(OpenAIProvider::should_prefer_websocket("gpt-5"));
    assert!(OpenAIProvider::should_prefer_websocket("codex-mini"));
    assert!(!OpenAIProvider::should_prefer_websocket(""));
}

#[test]
fn test_openai_transport_mode_defaults_to_auto() {
    let mode = OpenAITransportMode::from_config(None);
    assert_eq!(mode.as_str(), "auto");
}

#[test]
fn test_openai_transport_mode_auto_prefers_websocket_for_openai_models() {
    let mode = OpenAITransportMode::from_config(Some("auto"));
    assert_eq!(mode.as_str(), "auto");
    assert!(OpenAIProvider::should_prefer_websocket("gpt-5.4"));
}

#[tokio::test]
async fn test_record_websocket_fallback_sets_cooldown_for_auto_default_models() {
    let cooldowns = Arc::new(RwLock::new(HashMap::new()));
    let streaks = Arc::new(RwLock::new(HashMap::new()));
    let model = "gpt-5.4";

    let (streak, cooldown) = record_websocket_fallback(
        &cooldowns,
        &streaks,
        model,
        WebsocketFallbackReason::StreamTimeout,
    )
    .await;
    assert_eq!(streak, 1);
    assert_eq!(
        cooldown,
        Duration::from_secs(WEBSOCKET_MODEL_COOLDOWN_BASE_SECS)
    );
    assert!(
        websocket_cooldown_remaining(&cooldowns, model)
            .await
            .is_some(),
        "auto websocket default must still be guarded by cooldown after fallback"
    );
}

#[tokio::test]
async fn test_websocket_cooldown_helpers_set_clear_and_expire() {
    let cooldowns = Arc::new(RwLock::new(HashMap::new()));
    let model = "gpt-5.3-codex";

    assert!(
        websocket_cooldown_remaining(&cooldowns, model)
            .await
            .is_none()
    );

    set_websocket_cooldown(&cooldowns, model).await;
    let remaining = websocket_cooldown_remaining(&cooldowns, model).await;
    assert!(remaining.is_some());

    clear_websocket_cooldown(&cooldowns, model).await;
    assert!(
        websocket_cooldown_remaining(&cooldowns, model)
            .await
            .is_none()
    );

    {
        let mut guard = cooldowns.write().await;
        guard.insert(model.to_string(), Instant::now() - Duration::from_secs(1));
    }
    assert!(
        websocket_cooldown_remaining(&cooldowns, model)
            .await
            .is_none()
    );
    assert!(!cooldowns.read().await.contains_key(model));
}

#[test]
fn test_websocket_cooldown_for_streak_scales_and_caps() {
    assert_eq!(
        websocket_cooldown_for_streak(1, WebsocketFallbackReason::StreamTimeout),
        Duration::from_secs(WEBSOCKET_MODEL_COOLDOWN_BASE_SECS)
    );
    assert_eq!(
        websocket_cooldown_for_streak(2, WebsocketFallbackReason::StreamTimeout),
        Duration::from_secs(WEBSOCKET_MODEL_COOLDOWN_BASE_SECS * 2)
    );
    assert_eq!(
        websocket_cooldown_for_streak(3, WebsocketFallbackReason::StreamTimeout),
        Duration::from_secs(WEBSOCKET_MODEL_COOLDOWN_BASE_SECS * 4)
    );
    assert_eq!(
        websocket_cooldown_for_streak(32, WebsocketFallbackReason::StreamTimeout),
        Duration::from_secs(WEBSOCKET_MODEL_COOLDOWN_MAX_SECS)
    );
}

#[test]
fn test_websocket_cooldown_for_reason_adjusts_by_failure_type() {
    assert_eq!(
        websocket_cooldown_for_streak(1, WebsocketFallbackReason::ConnectTimeout),
        Duration::from_secs((WEBSOCKET_MODEL_COOLDOWN_BASE_SECS / 2).max(1))
    );
    assert_eq!(
        websocket_cooldown_for_streak(1, WebsocketFallbackReason::ServerRequestedHttps),
        Duration::from_secs(WEBSOCKET_MODEL_COOLDOWN_BASE_SECS * 5)
    );
    assert_eq!(
        websocket_cooldown_for_streak(32, WebsocketFallbackReason::ServerRequestedHttps),
        Duration::from_secs(WEBSOCKET_MODEL_COOLDOWN_MAX_SECS * 3)
    );
}

#[tokio::test]
async fn test_record_websocket_fallback_tracks_streak_and_cooldown() {
    let cooldowns = Arc::new(RwLock::new(HashMap::new()));
    let streaks = Arc::new(RwLock::new(HashMap::new()));
    let model = "gpt-5.3-codex-spark";

    let (streak1, cooldown1) = record_websocket_fallback(
        &cooldowns,
        &streaks,
        model,
        WebsocketFallbackReason::StreamTimeout,
    )
    .await;
    assert_eq!(streak1, 1);
    assert_eq!(
        cooldown1,
        Duration::from_secs(WEBSOCKET_MODEL_COOLDOWN_BASE_SECS)
    );
    let remaining1 = websocket_cooldown_remaining(&cooldowns, model)
        .await
        .expect("cooldown should be set");
    assert!(remaining1 <= cooldown1);

    let (streak2, cooldown2) = record_websocket_fallback(
        &cooldowns,
        &streaks,
        model,
        WebsocketFallbackReason::StreamTimeout,
    )
    .await;
    assert_eq!(streak2, 2);
    assert_eq!(
        cooldown2,
        Duration::from_secs(WEBSOCKET_MODEL_COOLDOWN_BASE_SECS * 2)
    );
    let remaining2 = websocket_cooldown_remaining(&cooldowns, model)
        .await
        .expect("cooldown should be set");
    assert!(remaining2 <= cooldown2);

    record_websocket_success(&cooldowns, &streaks, model).await;
    assert!(
        websocket_cooldown_remaining(&cooldowns, model)
            .await
            .is_none()
    );
    let normalized = normalize_transport_model(model).expect("normalized model");
    assert!(!streaks.read().await.contains_key(&normalized));
}

#[test]
fn test_websocket_activity_payload_detection() {
    assert!(is_websocket_activity_payload(
        r#"{"type":"response.created","response":{"id":"resp_1"}}"#
    ));
    assert!(is_websocket_activity_payload(
        r#"{"type":"response.reasoning.delta","delta":"thinking"}"#
    ));
    assert!(!is_websocket_activity_payload("not json"));
    assert!(!is_websocket_activity_payload(r#"{"foo":"bar"}"#));
}

#[test]
fn test_websocket_first_activity_payload_counts_typed_control_events() {
    assert!(is_websocket_first_activity_payload(
        r#"{"type":"rate_limits.updated"}"#
    ));
    assert!(is_websocket_first_activity_payload(
        r#"{"type":"session.created","session":{}}"#
    ));
    assert!(!is_websocket_first_activity_payload(r#"{"foo":"bar"}"#));
    assert!(!is_websocket_first_activity_payload("not json"));
}

#[test]
fn test_websocket_completion_timeout_is_long_enough_for_reasoning() {
    let timeout = std::hint::black_box(WEBSOCKET_COMPLETION_TIMEOUT_SECS);
    assert!(
        timeout >= 120,
        "completion timeout regressed to {}s; reasoning models may need several minutes",
        timeout
    );
    assert!(
        timeout <= 180,
        "completion timeout regressed to {}s; silent websocket waits look like stuck thinking before HTTPS fallback",
        timeout
    );
}

#[test]
fn test_stream_activity_event_treats_any_stream_event_as_activity() {
    assert!(is_stream_activity_event(&StreamEvent::ThinkingStart));
    assert!(is_stream_activity_event(&StreamEvent::ThinkingDelta(
        "working".to_string()
    )));
    assert!(is_stream_activity_event(&StreamEvent::TextDelta(
        "hello".to_string()
    )));
    assert!(is_stream_activity_event(&StreamEvent::MessageEnd {
        stop_reason: None
    }));
}

#[test]
fn test_stale_persistent_continuation_errors_trigger_fallback() {
    assert!(super::openai_stream_runtime::is_stale_persistent_continuation_error(
        "invalid_request_error (previous_response_not_found): Previous response with id 'resp_abc' not found."
    ));
    assert!(super::openai_stream_runtime::is_stale_persistent_continuation_error(
        "No tool output found for function call call_123."
    ));
    assert!(!super::openai_stream_runtime::is_stale_persistent_continuation_error(
        "rate limit exceeded"
    ));
}

#[tokio::test]
async fn test_persistent_ws_continuation_rejects_changed_input_prefix() {
    let (state, server) = test_persistent_ws_state().await;
    let persistent_ws = Arc::new(tokio::sync::Mutex::new(Some(state)));
    let (tx, _rx) = tokio::sync::mpsc::channel(4);
    let request = serde_json::json!({
        "model": "gpt-test",
        "input": [],
        "tools": [],
    });
    let input = vec![
        serde_json::json!({
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": "changed prefix"}],
        }),
        serde_json::json!({
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": "new item"}],
        }),
    ];

    let result = super::openai_stream_runtime::try_persistent_ws_continuation(
        &persistent_ws,
        &request,
        &input,
        input.len(),
        &tx,
    )
    .await;

    assert!(matches!(result, PersistentWsResult::NotAvailable));
    assert!(persistent_ws.lock().await.is_none());
    server.abort();
}

#[tokio::test]
async fn test_persistent_ws_continuation_finalizes_after_assistant_message_done_without_response_completed()
{
    let prefix_item = serde_json::json!({
        "type": "message",
        "role": "user",
        "content": [{"type": "input_text", "text": "old prefix"}],
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test websocket listener");
    let addr = listener.local_addr().expect("listener local addr");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept websocket client");
        let mut ws = tokio_tungstenite::accept_async(stream)
            .await
            .expect("accept websocket handshake");
        let _request = ws.next().await.expect("continuation request").expect("request frame");
        ws.send(WsMessage::Text(
            serde_json::json!({
                "type": "response.created",
                "response": {"id": "resp_after_tool"}
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send response.created");
        ws.send(WsMessage::Text(
            serde_json::json!({
                "type": "response.output_text.delta",
                "delta": "final text"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send text delta");
        ws.send(WsMessage::Text(
            serde_json::json!({
                "type": "response.output_item.done",
                "item": {
                    "id": "msg_final",
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "final text"}]
                }
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send assistant message done");
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if ws
                .send(WsMessage::Text(
                    serde_json::json!({
                        "type": "response.in_progress",
                        "response": {"status": "in_progress"}
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    let (client_ws, _) = connect_async(format!("ws://{}", addr))
        .await
        .expect("connect websocket client");
    let persistent_ws = Arc::new(tokio::sync::Mutex::new(Some(PersistentWsState {
        ws_stream: client_ws,
        last_response_id: "resp_test".to_string(),
        connected_at: Instant::now(),
        last_activity_at: Instant::now(),
        message_count: 1,
        last_input_item_count: 1,
        last_input_item_hashes: crate::provider::fingerprint::item_hashes(&[prefix_item.clone()]),
    })));
    let input = vec![
        prefix_item,
        serde_json::json!({
            "type": "function_call_output",
            "call_id": "call_123",
            "output": "tool done",
        }),
    ];
    let request = serde_json::json!({
        "model": "gpt-test",
        "input": input,
        "tools": [],
    });
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);

    let result = tokio::time::timeout(
        Duration::from_secs(3),
        super::openai_stream_runtime::try_persistent_ws_continuation(
            &persistent_ws,
            &request,
            request.get("input").unwrap().as_array().unwrap(),
            2,
            &tx,
        ),
    )
    .await
    .expect("assistant message done should drain instead of hanging");

    assert!(matches!(result, PersistentWsResult::Success));
    assert!(
        persistent_ws.lock().await.is_none(),
        "synthetic completion must discard the still in-flight persistent socket"
    );

    let mut saw_text = false;
    let mut saw_message_end = false;
    while let Ok(event) = rx.try_recv() {
        match event.expect("stream event should be ok") {
            StreamEvent::TextDelta(text) if text == "final text" => saw_text = true,
            StreamEvent::MessageEnd { stop_reason } => {
                saw_message_end = stop_reason.as_deref() == Some("assistant_message_done")
            }
            _ => {}
        }
    }
    assert!(saw_text, "final text delta should be forwarded before synthetic completion");
    assert!(saw_message_end, "synthetic MessageEnd should be emitted after drain");
    server.abort();
}

#[test]
fn test_websocket_activity_payload_counts_response_completed() {
    assert!(is_websocket_activity_payload(
        r#"{"type":"response.completed","response":{"status":"completed"}}"#
    ));
}

#[test]
fn test_websocket_activity_payload_counts_in_progress_events() {
    assert!(is_websocket_activity_payload(
        r#"{"type":"response.in_progress","response":{"status":"in_progress"}}"#
    ));
}

#[test]
fn test_websocket_activity_payload_ignores_non_response_events() {
    assert!(!is_websocket_activity_payload(
        r#"{"type":"session.created","session":{}}"#
    ));
    assert!(!is_websocket_activity_payload(
        r#"{"type":"rate_limits.updated"}"#
    ));
    assert!(!is_websocket_activity_payload(r#"not json at all"#));
}

#[test]
fn test_websocket_remaining_timeout_secs_uses_idle_time_budget() {
    let recent = Instant::now() - Duration::from_secs(2);
    let remaining = websocket_remaining_timeout_secs(recent, 8).expect("still within budget");
    assert!(
        (6..=7).contains(&remaining),
        "expected remaining idle budget near 6-7s, got {remaining}"
    );
}

#[test]
fn test_websocket_remaining_timeout_secs_expires_after_budget() {
    let expired = Instant::now() - Duration::from_secs(9);
    assert!(websocket_remaining_timeout_secs(expired, 8).is_none());
}

#[test]
fn test_websocket_next_activity_timeout_uses_request_start_before_first_event() {
    let ws_started_at = Instant::now() - Duration::from_secs(3);
    let last_api_activity_at = Instant::now() - Duration::from_secs(1);
    let remaining =
        websocket_next_activity_timeout_secs(ws_started_at, last_api_activity_at, false)
            .expect("first-event timeout should still be active");
    assert!(
        (5..=6).contains(&remaining),
        "expected first-event timeout near 5-6s, got {remaining}"
    );
}

#[test]
fn test_websocket_next_activity_timeout_resets_after_api_activity() {
    let ws_started_at = Instant::now() - Duration::from_secs(299);
    let last_api_activity_at = Instant::now() - Duration::from_secs(2);
    let remaining = websocket_next_activity_timeout_secs(ws_started_at, last_api_activity_at, true)
        .expect("idle timeout should use last activity, not total request age");
    assert!(
        remaining >= WEBSOCKET_COMPLETION_TIMEOUT_SECS.saturating_sub(3),
        "expected full idle budget to reset after activity, got {remaining}"
    );
}

#[test]
fn test_websocket_activity_timeout_kind_labels_first_and_next() {
    assert_eq!(websocket_activity_timeout_kind(false), "first");
    assert_eq!(websocket_activity_timeout_kind(true), "next");
}

#[test]
fn test_format_status_duration_uses_compact_human_labels() {
    assert_eq!(format_status_duration(Duration::from_secs(9)), "9s");
    assert_eq!(format_status_duration(Duration::from_secs(125)), "2m 5s");
    assert_eq!(format_status_duration(Duration::from_secs(7260)), "2h 1m");
}

#[test]
fn test_summarize_websocket_fallback_reason_classifies_common_failures() {
    assert_eq!(
        summarize_websocket_fallback_reason("WebSocket connect timed out after 8s"),
        "connect timeout"
    );
    assert_eq!(
        summarize_websocket_fallback_reason(
            "WebSocket stream timed out waiting for first websocket activity (8s)"
        ),
        "first response timeout"
    );
    assert_eq!(
        summarize_websocket_fallback_reason(
            "WebSocket stream timed out waiting for next websocket activity (300s)"
        ),
        "stream timeout"
    );
    assert_eq!(
        summarize_websocket_fallback_reason("server requested fallback"),
        "server requested https"
    );
    assert_eq!(
        summarize_websocket_fallback_reason("WebSocket stream closed before response.completed"),
        "stream closed early"
    );
}

#[test]
fn test_normalize_transport_model_trims_and_lowercases() {
    assert_eq!(
        normalize_transport_model("  GPT-5.4  "),
        Some("gpt-5.4".to_string())
    );
    assert_eq!(normalize_transport_model("   \t\n  "), None);
}

#[tokio::test]
async fn test_record_websocket_success_clears_normalized_keys() {
    let cooldowns = Arc::new(RwLock::new(HashMap::new()));
    let streaks = Arc::new(RwLock::new(HashMap::new()));
    let canonical = "gpt-5.4";

    record_websocket_fallback(
        &cooldowns,
        &streaks,
        canonical,
        WebsocketFallbackReason::StreamTimeout,
    )
    .await;
    assert!(
        websocket_cooldown_remaining(&cooldowns, canonical)
            .await
            .is_some()
    );

    record_websocket_success(&cooldowns, &streaks, " GPT-5.4 ").await;

    assert!(
        websocket_cooldown_remaining(&cooldowns, canonical)
            .await
            .is_none(),
        "success should clear normalized cooldown entries"
    );
    assert!(
        !streaks.read().await.contains_key(canonical),
        "success should clear normalized failure streak entries"
    );
}
