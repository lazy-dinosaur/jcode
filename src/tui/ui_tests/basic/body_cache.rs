fn test_hashes(values: &[u64]) -> Arc<[u64]> {
    values.to_vec().into()
}

fn display_message_hashes(messages: &[DisplayMessage]) -> Arc<[u64]> {
    messages
        .iter()
        .map(DisplayMessage::stable_cache_hash)
        .collect::<Vec<_>>()
        .into()
}

#[test]
fn test_body_cache_state_keeps_multiple_width_entries() {
    let key_a = BodyCacheKey {
        session_id: None,
        width: 40,
        diff_mode: crate::config::DiffDisplayMode::Off,
        messages_version: 1,
        diagram_mode: crate::config::DiagramDisplayMode::Pinned,
        centered: false,
    };
    let key_b = BodyCacheKey {
        session_id: None,
        width: 41,
        ..key_a.clone()
    };

    let prepared_a = Arc::new(PreparedMessages {
        wrapped_lines: vec![Line::from("a")],
        wrapped_plain_lines: Arc::new(vec!["a".to_string()]),
        wrapped_copy_offsets: Arc::new(vec![0]),
        raw_plain_lines: Arc::new(Vec::new()),
        wrapped_line_map: Arc::new(Vec::new()),
        wrapped_user_indices: Vec::new(),
        wrapped_user_prompt_starts: Vec::new(),
        wrapped_user_prompt_ends: Vec::new(),
        message_wrapped_starts: Vec::new(),
        user_prompt_texts: Vec::new(),
        image_regions: Vec::new(),
        edit_tool_ranges: Vec::new(),
        copy_targets: Vec::new(),
    });
    let prepared_b = Arc::new(PreparedMessages {
        wrapped_lines: vec![Line::from("b")],
        wrapped_plain_lines: Arc::new(vec!["b".to_string()]),
        wrapped_copy_offsets: Arc::new(vec![0]),
        raw_plain_lines: Arc::new(Vec::new()),
        wrapped_line_map: Arc::new(Vec::new()),
        wrapped_user_indices: Vec::new(),
        wrapped_user_prompt_starts: Vec::new(),
        wrapped_user_prompt_ends: Vec::new(),
        message_wrapped_starts: Vec::new(),
        user_prompt_texts: Vec::new(),
        image_regions: Vec::new(),
        edit_tool_ranges: Vec::new(),
        copy_targets: Vec::new(),
    });

    let mut cache = BodyCacheState::default();
    cache.insert(key_a.clone(), prepared_a.clone(), 3);
    cache.insert(key_b.clone(), prepared_b.clone(), 3);

    let hit_a = cache
        .get_exact(&key_a)
        .expect("expected width 40 cache hit");
    let hit_b = cache
        .get_exact(&key_b)
        .expect("expected width 41 cache hit");

    assert!(Arc::ptr_eq(&hit_a, &prepared_a));
    assert!(Arc::ptr_eq(&hit_b, &prepared_b));
    assert_eq!(cache.entries.len(), 2);
}

#[test]
fn test_body_cache_state_evicts_oldest_entries() {
    let mut cache = BodyCacheState::default();

    for idx in 0..(BODY_CACHE_MAX_ENTRIES + 2) {
        let key = BodyCacheKey {
            session_id: None,
            width: 40 + idx as u16,
            diff_mode: crate::config::DiffDisplayMode::Off,
            messages_version: 1,
            diagram_mode: crate::config::DiagramDisplayMode::Pinned,
            centered: false,
        };
        let prepared = Arc::new(PreparedMessages {
            wrapped_lines: vec![Line::from(format!("{idx}"))],
            wrapped_plain_lines: Arc::new(vec![format!("{idx}")]),
            wrapped_copy_offsets: Arc::new(vec![0]),
            raw_plain_lines: Arc::new(Vec::new()),
            wrapped_line_map: Arc::new(Vec::new()),
            wrapped_user_indices: Vec::new(),
            wrapped_user_prompt_starts: Vec::new(),
            wrapped_user_prompt_ends: Vec::new(),
            message_wrapped_starts: Vec::new(),
            user_prompt_texts: Vec::new(),
            image_regions: Vec::new(),
            edit_tool_ranges: Vec::new(),
            copy_targets: Vec::new(),
        });
        cache.insert(key, prepared, idx);
    }

    assert_eq!(cache.entries.len(), BODY_CACHE_MAX_ENTRIES);
    assert!(
        cache.entries.iter().all(|entry| entry.key.width >= 42),
        "oldest widths should be evicted"
    );
}

#[test]
fn test_body_cache_state_accepts_large_single_entry_within_total_budget() {
    let key = BodyCacheKey {
        session_id: None,
        width: 120,
        diff_mode: crate::config::DiffDisplayMode::Off,
        messages_version: 99,
        diagram_mode: crate::config::DiagramDisplayMode::Pinned,
        centered: false,
    };
    let prepared = make_prepared_messages_with_content_bytes(3 * 1024 * 1024, "body-large-");

    assert!(estimate_prepared_messages_bytes(&prepared) > 4 * 1024 * 1024);
    assert!(estimate_prepared_messages_bytes(&prepared) < BODY_CACHE_MAX_BYTES);

    let mut cache = BodyCacheState::default();
    cache.insert(key.clone(), prepared.clone(), 60);

    let hit = cache
        .get_exact(&key)
        .expect("expected large body cache entry to be retained");
    assert!(Arc::ptr_eq(&hit, &prepared));
}

#[test]
fn test_body_cache_state_reuses_prefix_when_last_message_updates() {
    let key_v1 = BodyCacheKey {
        session_id: Some("session-a".to_string()),
        width: 120,
        diff_mode: crate::config::DiffDisplayMode::Off,
        messages_version: 1,
        diagram_mode: crate::config::DiagramDisplayMode::Pinned,
        centered: false,
    };
    let key_v2 = BodyCacheKey {
        messages_version: 2,
        ..key_v1.clone()
    };
    let prepared = Arc::new(PreparedMessages {
        wrapped_lines: vec![Line::from("first"), Line::from("tail")],
        wrapped_plain_lines: Arc::new(vec!["first".to_string(), "tail".to_string()]),
        wrapped_copy_offsets: Arc::new(vec![0, 0]),
        raw_plain_lines: Arc::new(vec!["first".to_string(), "tail".to_string()]),
        wrapped_line_map: Arc::new(vec![
            WrappedLineMap {
                raw_line: 0,
                start_col: 0,
                end_col: 5,
            },
            WrappedLineMap {
                raw_line: 1,
                start_col: 0,
                end_col: 4,
            },
        ]),
        wrapped_user_indices: Vec::new(),
        wrapped_user_prompt_starts: Vec::new(),
        wrapped_user_prompt_ends: Vec::new(),
        message_wrapped_starts: vec![0, 1, 2],
        user_prompt_texts: Vec::new(),
        image_regions: Vec::new(),
        edit_tool_ranges: Vec::new(),
        copy_targets: Vec::new(),
    });

    let mut cache = BodyCacheState::default();
    cache.insert_with_message_hashes(key_v1, prepared, 2, test_hashes(&[11, 22]));

    let (prefix, prefix_count) = cache
        .take_tail_update_base(&key_v2, 2, &[11, 33])
        .expect("same-count entry should provide prefix base");
    assert_eq!(prefix_count, 1);
    assert_eq!(prefix.wrapped_lines.len(), 1);
    assert_eq!(prefix.raw_plain_lines.len(), 1);
    assert_eq!(prefix.message_wrapped_starts, vec![0, 1]);
    assert_eq!(super::line_plain_text(&prefix.wrapped_lines[0]), "first");
}

#[test]
fn test_body_cache_state_rejects_tail_update_when_prefix_hash_changes() {
    let key_v1 = BodyCacheKey {
        session_id: Some("session-a".to_string()),
        width: 120,
        diff_mode: crate::config::DiffDisplayMode::Off,
        messages_version: 1,
        diagram_mode: crate::config::DiagramDisplayMode::Pinned,
        centered: false,
    };
    let key_v2 = BodyCacheKey {
        messages_version: 2,
        ..key_v1.clone()
    };
    let prepared = Arc::new(PreparedMessages {
        wrapped_lines: vec![Line::from("first"), Line::from("tail")],
        wrapped_plain_lines: Arc::new(vec!["first".to_string(), "tail".to_string()]),
        wrapped_copy_offsets: Arc::new(vec![0, 0]),
        raw_plain_lines: Arc::new(vec!["first".to_string(), "tail".to_string()]),
        wrapped_line_map: Arc::new(vec![
            WrappedLineMap {
                raw_line: 0,
                start_col: 0,
                end_col: 5,
            },
            WrappedLineMap {
                raw_line: 1,
                start_col: 0,
                end_col: 4,
            },
        ]),
        wrapped_user_indices: Vec::new(),
        wrapped_user_prompt_starts: Vec::new(),
        wrapped_user_prompt_ends: Vec::new(),
        message_wrapped_starts: vec![0, 1, 2],
        user_prompt_texts: Vec::new(),
        image_regions: Vec::new(),
        edit_tool_ranges: Vec::new(),
        copy_targets: Vec::new(),
    });

    let mut cache = BodyCacheState::default();
    cache.insert_with_message_hashes(key_v1, prepared, 2, test_hashes(&[11, 22]));

    assert!(cache.take_tail_update_base(&key_v2, 2, &[99, 22]).is_none());
}

#[test]
fn test_body_cache_state_retains_oversized_hot_entry() {
    let key = BodyCacheKey {
        session_id: None,
        width: 140,
        diff_mode: crate::config::DiffDisplayMode::Off,
        messages_version: 120,
        diagram_mode: crate::config::DiagramDisplayMode::Pinned,
        centered: false,
    };
    let prepared = make_oversized_prepared_messages("body-oversized-");
    let oversized_budget = 4 * 1024 * 1024;

    assert!(estimate_prepared_messages_bytes(&prepared) > oversized_budget);

    let mut cache = BodyCacheState::default();
    cache.insert_with_budget(key.clone(), prepared.clone(), 120, oversized_budget);

    let hit = cache
        .get_exact(&key)
        .expect("expected oversized body cache entry to be retained as hot entry");
    assert!(Arc::ptr_eq(&hit, &prepared));
    assert!(cache.entries.is_empty());
    assert_eq!(cache.oversized_entries.len(), 1);
}

#[test]
fn test_body_cache_state_keeps_two_oversized_width_entries_hot() {
    let key_a = BodyCacheKey {
        session_id: None,
        width: 140,
        diff_mode: crate::config::DiffDisplayMode::Off,
        messages_version: 120,
        diagram_mode: crate::config::DiagramDisplayMode::Pinned,
        centered: false,
    };
    let key_b = BodyCacheKey {
        session_id: None,
        width: 139,
        ..key_a.clone()
    };
    let prepared_a = make_oversized_prepared_messages("body-oversized-a-");
    let prepared_b = make_oversized_prepared_messages("body-oversized-b-");
    let oversized_budget = 4 * 1024 * 1024;

    let mut cache = BodyCacheState::default();
    cache.insert_with_budget(key_a.clone(), prepared_a.clone(), 120, oversized_budget);
    cache.insert_with_budget(key_b.clone(), prepared_b.clone(), 120, oversized_budget);

    let hit_a = cache
        .get_exact(&key_a)
        .expect("expected first oversized body width to remain hot");
    let hit_b = cache
        .get_exact(&key_b)
        .expect("expected second oversized body width to remain hot");
    assert!(Arc::ptr_eq(&hit_a, &prepared_a));
    assert!(Arc::ptr_eq(&hit_b, &prepared_b));
    assert_eq!(cache.oversized_entries.len(), 2);
}

#[test]
fn test_body_cache_state_uses_oversized_hot_entry_as_incremental_base() {
    let key = BodyCacheKey {
        session_id: None,
        width: 140,
        diff_mode: crate::config::DiffDisplayMode::Off,
        messages_version: 120,
        diagram_mode: crate::config::DiagramDisplayMode::Pinned,
        centered: false,
    };
    let prepared = make_oversized_prepared_messages("body-oversized-base-");
    let oversized_budget = 4 * 1024 * 1024;

    assert!(estimate_prepared_messages_bytes(&prepared) > oversized_budget);

    let mut cache = BodyCacheState::default();
    cache.insert_with_budget(key.clone(), prepared.clone(), 120, oversized_budget);

    let base = cache
        .best_incremental_base(
            &BodyCacheKey {
                messages_version: 121,
                ..key.clone()
            },
            121,
        )
        .expect("expected oversized hot entry to remain eligible as incremental base");
    assert!(Arc::ptr_eq(&base.0, &prepared));
    assert_eq!(base.1, 120);
}

#[test]
fn test_prepare_body_incremental_reuses_unique_prepared_arc() {
    let width = 80;
    let base_state = TestState {
        display_messages: vec![
            DisplayMessage::user("first prompt"),
            DisplayMessage::assistant("initial answer"),
        ],
        messages_version: 1,
        ..Default::default()
    };
    let grown_state = TestState {
        display_messages: vec![
            DisplayMessage::user("first prompt"),
            DisplayMessage::assistant("initial answer"),
            DisplayMessage::user("second prompt"),
            DisplayMessage::assistant("follow-up answer"),
        ],
        messages_version: 2,
        ..Default::default()
    };

    let prepared = Arc::new(super::prepare::prepare_body(&base_state, width, false));
    let base_ptr = Arc::as_ptr(&prepared) as usize;
    let incremented = super::prepare::prepare_body_incremental(&grown_state, width, prepared, 2);

    assert_eq!(Arc::as_ptr(&incremented) as usize, base_ptr);
    assert!(
        incremented.wrapped_lines.len() >= 4,
        "expected incremental prep to append new wrapped content"
    );
    assert_eq!(
        incremented.message_wrapped_starts.len(),
        grown_state.display_messages.len() + 1,
        "incremental prep should retain message boundaries for later tail updates"
    );
    assert_eq!(
        incremented.message_wrapped_starts.last().copied(),
        Some(incremented.wrapped_lines.len())
    );
}

#[test]
fn test_prepare_body_renders_glued_assistant_ordered_list_on_separate_lines() {
    let state = TestState {
        display_messages: vec![DisplayMessage::assistant(
            "다음은 이 순서가 제일 좋아요.1. **실제 긴 세션 런타임 검증** - 화면 확인.2. **scroll/input 밀림 전용 진단 추가** - geometry invariant.",
        )],
        messages_version: 1,
        ..Default::default()
    };

    let prepared = super::prepare::prepare_body(&state, 180, false);
    let lines = prepared.wrapped_plain_lines.as_ref();

    assert_eq!(
        lines.first().map(String::as_str),
        Some("다음은 이 순서가 제일 좋아요.")
    );
    assert!(
        lines
            .iter()
            .any(|line| line.trim_start().starts_with("1. 실제 긴 세션 런타임 검증")),
        "first ordered item should render separately: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.trim_start().starts_with("2. scroll/input 밀림 전용 진단 추가")),
        "second ordered item should render separately: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .all(|line| !line.contains("좋아요.1.") && !line.contains("확인.2.")),
        "TUI body wrapping must not re-glue ordered markers: {lines:?}"
    );
}

#[test]
fn test_prepare_body_renders_glued_alphabetic_options_on_separate_lines() {
    let state = TestState {
        display_messages: vec![DisplayMessage::assistant(
            "선택지: A. 내가 중복 함수 하나만 제거 (Recommended) B. 네가 직접 getRecipientChatBadge 중복 블록 하나 삭제 C. 우선 그대로 둠",
        )],
        messages_version: 1,
        ..Default::default()
    };

    let prepared = super::prepare::prepare_body(&state, 180, false);
    let lines = prepared.wrapped_plain_lines.as_ref();

    assert_eq!(lines.first().map(String::as_str), Some("선택지:"));
    assert!(
        lines
            .iter()
            .any(|line| line.trim_start().starts_with("A. 내가 중복 함수")),
        "A option should render separately: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.trim_start().starts_with("B. 네가 직접")),
        "B option should render separately: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .all(|line| !line.contains("선택지: A.") && !line.contains("Recommended) B.")),
        "TUI body wrapping must not glue alphabetic option markers: {lines:?}"
    );
}

#[test]
fn test_prepare_body_preserves_source_newlines_before_alphabetic_options() {
    let state = TestState {
        display_messages: vec![DisplayMessage::assistant(
            "선택지:\nA. 내가 중복 함수 하나만 제거 (Recommended)\nB. 네가 직접 getRecipientChatBadge 중복 블록 하나 삭제\nC. 우선 그대로 둠",
        )],
        messages_version: 1,
        ..Default::default()
    };

    let prepared = super::prepare::prepare_body(&state, 180, false);
    let lines = prepared.wrapped_plain_lines.as_ref();

    assert_eq!(lines.first().map(String::as_str), Some("선택지:"));
    assert!(
        lines
            .iter()
            .any(|line| line.trim_start().starts_with("A. 내가 중복 함수")),
        "A option should preserve source newline: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.trim_start().starts_with("B. 네가 직접")),
        "B option should preserve source newline: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .all(|line| !line.contains("선택지: A.") && !line.contains("Recommended) B.")),
        "TUI body wrapping must not collapse source newlines before options: {lines:?}"
    );
}

#[test]
fn test_prepare_body_preserves_list_item_title_continuation_break() {
    let state = TestState {
        display_messages: vec![DisplayMessage::assistant(
            "5. **poll 메시지도 push 발송 유지**\npoll도 unread에 포함되므로 push가 나가야 합니다.",
        )],
        messages_version: 1,
        ..Default::default()
    };

    let prepared = super::prepare::prepare_body(&state, 180, false);
    let lines = prepared.wrapped_plain_lines.as_ref();

    assert!(
        lines
            .iter()
            .any(|line| line.trim_start().starts_with("5. poll 메시지도 push 발송 유지")),
        "list item title should render separately: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.trim_start().starts_with("poll도 unread에 포함")),
        "list item continuation should preserve visible line break: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .all(|line| !line.contains("유지 poll도") && !line.contains("유지poll도")),
        "TUI body wrapping must not glue list item continuation: {lines:?}"
    );
}

#[test]
fn test_prepare_body_does_not_glue_nested_bullet_to_next_numbered_item() {
    let state = TestState {
        display_messages: vec![DisplayMessage::assistant(
            "1. **실제 긴 세션에서 체감 확인**\n   - 스크롤 밀림\n   - input 밀림\n   - markdown 재깨짐 여부 - TPS/frame latency\n2. **cache 효과 계측**\n   - body cache hit/miss\n   - incremental reuse 비율\n   - markdown render 시간이 아직 큰지\n3. **RAM cap 조정**",
        )],
        messages_version: 1,
        ..Default::default()
    };

    let prepared = super::prepare::prepare_body(&state, 220, false);
    let lines = prepared.wrapped_plain_lines.as_ref();

    assert!(
        lines
            .iter()
            .any(|line| line.trim_start().starts_with("• markdown 재깨짐 여부 - TPS/frame latency")),
        "last nested bullet should render separately: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.trim_start().starts_with("2. cache 효과 계측")),
        "next numbered item should render separately: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.trim_start().starts_with("3. RAM cap 조정")),
        "third numbered item should render separately: {lines:?}"
    );
    assert!(
        lines.iter().all(|line| {
            !line.contains("latency2.")
                && !line.contains("큰지3.")
                && !line.contains("추가4.")
        }),
        "TUI body wrapping must not glue numbered markers after nested bullets: {lines:?}"
    );
}

#[test]
fn test_tail_update_incremental_body_matches_full_rebuild() {
    let width = 80;
    let old_state = TestState {
        display_messages: vec![
            DisplayMessage::user("first prompt"),
            DisplayMessage::system("tool still running"),
        ],
        messages_version: 1,
        ..Default::default()
    };
    let updated_state = TestState {
        display_messages: vec![
            DisplayMessage::user("first prompt"),
            DisplayMessage::system("tool finished"),
        ],
        messages_version: 2,
        ..Default::default()
    };
    let key_v1 = BodyCacheKey {
        session_id: None,
        width,
        diff_mode: crate::config::DiffDisplayMode::Off,
        messages_version: old_state.messages_version,
        diagram_mode: crate::config::DiagramDisplayMode::Pinned,
        centered: false,
    };
    let key_v2 = BodyCacheKey {
        messages_version: updated_state.messages_version,
        ..key_v1.clone()
    };

    let old_prepared = Arc::new(super::prepare::prepare_body(&old_state, width, false));
    let full_rebuild = super::prepare::prepare_body(&updated_state, width, false);
    let old_hashes = display_message_hashes(&old_state.display_messages);
    let updated_hashes = display_message_hashes(&updated_state.display_messages);
    let mut cache = BodyCacheState::default();
    cache.insert_with_message_hashes(
        key_v1,
        old_prepared,
        old_state.display_messages.len(),
        old_hashes,
    );

    let (prefix, prefix_count) = cache
        .take_tail_update_base(&key_v2, updated_state.display_messages.len(), &updated_hashes)
        .expect("expected same-count cache entry to provide reusable prefix");
    let incremented =
        super::prepare::prepare_body_incremental(&updated_state, width, prefix, prefix_count);

    assert_eq!(
        incremented.wrapped_plain_lines.as_ref(),
        full_rebuild.wrapped_plain_lines.as_ref()
    );
    assert_eq!(
        incremented.message_wrapped_starts,
        full_rebuild.message_wrapped_starts
    );
}

#[test]
fn test_full_prep_cache_state_keeps_multiple_width_entries() {
    let key_a = FullPrepCacheKey {
        session_id: None,
        width: 40,
        height: 20,
        diff_mode: crate::config::DiffDisplayMode::Off,
        messages_version: 1,
        diagram_mode: crate::config::DiagramDisplayMode::Pinned,
        centered: false,
        is_processing: false,
        streaming_text_len: 0,
        streaming_text_version: 0,
        batch_progress_hash: 0,
    };
    let key_b = FullPrepCacheKey {
        session_id: None,
        width: 39,
        ..key_a.clone()
    };

    let prepared_a = make_prepared_chat_frame(Arc::new(PreparedMessages {
        wrapped_lines: vec![Line::from("a")],
        wrapped_plain_lines: Arc::new(vec!["a".to_string()]),
        wrapped_copy_offsets: Arc::new(vec![0]),
        raw_plain_lines: Arc::new(Vec::new()),
        wrapped_line_map: Arc::new(Vec::new()),
        wrapped_user_indices: Vec::new(),
        wrapped_user_prompt_starts: Vec::new(),
        wrapped_user_prompt_ends: Vec::new(),
        message_wrapped_starts: Vec::new(),
        user_prompt_texts: Vec::new(),
        image_regions: Vec::new(),
        edit_tool_ranges: Vec::new(),
        copy_targets: Vec::new(),
    }));
    let prepared_b = make_prepared_chat_frame(Arc::new(PreparedMessages {
        wrapped_lines: vec![Line::from("b")],
        wrapped_plain_lines: Arc::new(vec!["b".to_string()]),
        wrapped_copy_offsets: Arc::new(vec![0]),
        raw_plain_lines: Arc::new(Vec::new()),
        wrapped_line_map: Arc::new(Vec::new()),
        wrapped_user_indices: Vec::new(),
        wrapped_user_prompt_starts: Vec::new(),
        wrapped_user_prompt_ends: Vec::new(),
        message_wrapped_starts: Vec::new(),
        user_prompt_texts: Vec::new(),
        image_regions: Vec::new(),
        edit_tool_ranges: Vec::new(),
        copy_targets: Vec::new(),
    }));

    let mut cache = FullPrepCacheState::default();
    cache.insert(key_a.clone(), prepared_a.clone());
    cache.insert(key_b.clone(), prepared_b.clone());

    let hit_a = cache
        .get_exact(&key_a)
        .expect("expected width 40 full prep cache hit");
    let hit_b = cache
        .get_exact(&key_b)
        .expect("expected width 39 full prep cache hit");

    assert!(Arc::ptr_eq(&hit_a, &prepared_a));
    assert!(Arc::ptr_eq(&hit_b, &prepared_b));
    assert_eq!(cache.entries.len(), 2);
}

#[test]
fn test_full_prep_cache_state_evicts_oldest_entries() {
    let mut cache = FullPrepCacheState::default();

    for idx in 0..(FULL_PREP_CACHE_MAX_ENTRIES + 2) {
        let key = FullPrepCacheKey {
            session_id: None,
            width: 40 + idx as u16,
            height: 20,
            diff_mode: crate::config::DiffDisplayMode::Off,
            messages_version: 1,
            diagram_mode: crate::config::DiagramDisplayMode::Pinned,
            centered: false,
            is_processing: false,
            streaming_text_len: 0,
            streaming_text_version: 0,
            batch_progress_hash: 0,
        };
        let prepared = make_prepared_chat_frame(Arc::new(PreparedMessages {
            wrapped_lines: vec![Line::from(format!("{idx}"))],
            wrapped_plain_lines: Arc::new(vec![format!("{idx}")]),
            wrapped_copy_offsets: Arc::new(vec![0]),
            raw_plain_lines: Arc::new(Vec::new()),
            wrapped_line_map: Arc::new(Vec::new()),
            wrapped_user_indices: Vec::new(),
            wrapped_user_prompt_starts: Vec::new(),
            wrapped_user_prompt_ends: Vec::new(),
            message_wrapped_starts: Vec::new(),
            user_prompt_texts: Vec::new(),
            image_regions: Vec::new(),
            edit_tool_ranges: Vec::new(),
            copy_targets: Vec::new(),
        }));
        cache.insert(key, prepared);
    }

    assert_eq!(cache.entries.len(), FULL_PREP_CACHE_MAX_ENTRIES);
    assert!(
        cache.entries.iter().all(|entry| entry.key.width >= 42),
        "oldest widths should be evicted"
    );
}

#[test]
fn test_full_prep_cache_state_accepts_large_single_entry_within_total_budget() {
    let key = FullPrepCacheKey {
        session_id: None,
        width: 120,
        height: 40,
        diff_mode: crate::config::DiffDisplayMode::Off,
        messages_version: 99,
        diagram_mode: crate::config::DiagramDisplayMode::Pinned,
        centered: false,
        is_processing: false,
        streaming_text_len: 0,
        streaming_text_version: 0,
        batch_progress_hash: 0,
    };
    let prepared = make_prepared_chat_frame_with_content_bytes(3 * 1024 * 1024, "full-large-");

    assert!(estimate_prepared_chat_frame_bytes(&prepared) < FULL_PREP_CACHE_MAX_BYTES);

    let mut cache = FullPrepCacheState::default();
    cache.insert(key.clone(), prepared.clone());

    let hit = cache
        .get_exact(&key)
        .expect("expected large full prep cache entry to be retained");
    assert!(Arc::ptr_eq(&hit, &prepared));
}

#[test]
fn test_full_prep_cache_state_retains_oversized_hot_entry() {
    let key = FullPrepCacheKey {
        session_id: None,
        width: 140,
        height: 42,
        diff_mode: crate::config::DiffDisplayMode::Off,
        messages_version: 120,
        diagram_mode: crate::config::DiagramDisplayMode::Pinned,
        centered: false,
        is_processing: true,
        streaming_text_len: 4096,
        streaming_text_version: 12345,
        batch_progress_hash: 0,
    };
    let prepared = make_oversized_prepared_chat_frame("full-oversized-");

    assert!(estimate_prepared_chat_frame_bytes(&prepared) > FULL_PREP_CACHE_MAX_BYTES);

    let mut cache = FullPrepCacheState::default();
    cache.insert(key.clone(), prepared.clone());

    let hit = cache
        .get_exact(&key)
        .expect("expected oversized full prep entry to be retained as hot entry");
    assert!(Arc::ptr_eq(&hit, &prepared));
    assert!(cache.entries.is_empty());
    assert_eq!(cache.oversized_entries.len(), 1);
}

#[test]
fn test_full_prep_cache_state_keeps_two_oversized_width_entries_hot() {
    let key_a = FullPrepCacheKey {
        session_id: None,
        width: 140,
        height: 42,
        diff_mode: crate::config::DiffDisplayMode::Off,
        messages_version: 120,
        diagram_mode: crate::config::DiagramDisplayMode::Pinned,
        centered: false,
        is_processing: true,
        streaming_text_len: 4096,
        streaming_text_version: 12345,
        batch_progress_hash: 0,
    };
    let key_b = FullPrepCacheKey {
        session_id: None,
        width: 139,
        ..key_a.clone()
    };
    let prepared_a = make_oversized_prepared_chat_frame("full-oversized-a-");
    let prepared_b = make_oversized_prepared_chat_frame("full-oversized-b-");

    let mut cache = FullPrepCacheState::default();
    cache.insert(key_a.clone(), prepared_a.clone());
    cache.insert(key_b.clone(), prepared_b.clone());

    let hit_a = cache
        .get_exact(&key_a)
        .expect("expected first oversized full-prep width to remain hot");
    let hit_b = cache
        .get_exact(&key_b)
        .expect("expected second oversized full-prep width to remain hot");
    assert!(Arc::ptr_eq(&hit_a, &prepared_a));
    assert!(Arc::ptr_eq(&hit_b, &prepared_b));
    assert_eq!(cache.oversized_entries.len(), 2);
}
