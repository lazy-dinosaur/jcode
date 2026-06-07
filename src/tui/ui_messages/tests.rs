use super::*;

fn extract_line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

fn leading_spaces(text: &str) -> usize {
    text.chars().take_while(|c| *c == ' ').count()
}

fn system_glyph_env_lock() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};

    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn render_system_message_forces_system_color_on_all_spans() {
    let msg = DisplayMessage::system("**Reload complete** — continuing.");

    let lines = render_system_message(&msg, 80, crate::config::DiffDisplayMode::Off);

    assert!(!lines.is_empty(), "expected rendered system message lines");
    for line in lines {
        for span in line.spans {
            assert_eq!(span.style.fg, Some(system_message_color()));
        }
    }
}

#[test]
fn render_system_message_does_not_fall_back_to_raw_markdown_when_wide() {
    let long_value = "x".repeat(120);
    let msg = DisplayMessage::system(format!(
        "System note:\n\n```json\n{{\"long\":\"{}\"}}\n```\n\n| A | B |\n|---|---|\n| value | `{}` |",
        long_value, long_value
    ));

    let lines = render_system_message(&msg, 64, crate::config::DiffDisplayMode::Off);
    let plain = lines
        .iter()
        .map(extract_line_text)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        plain.contains("┌─ json"),
        "code block should stay rendered: {plain}"
    );
    assert!(
        plain.contains("A") && plain.contains('│'),
        "table should stay rendered: {plain}"
    );
    assert!(
        !plain.contains("```json"),
        "raw code fence leaked after wide markdown fallback: {plain}"
    );
    assert!(
        !plain.contains("|---|"),
        "raw table separator leaked after wide markdown fallback: {plain}"
    );
    assert!(
        lines.iter().all(|line| line.width() <= 64),
        "rendered markdown lines should wrap instead of overflowing: {:?}",
        lines.iter().map(extract_line_text).collect::<Vec<_>>()
    );
}

#[test]
fn render_system_message_wraps_parsed_long_lines_without_raw_fallback() {
    let msg = DisplayMessage::system(
        "점검 결과: 많이 쌓였습니다. 이제 `thin data`는 아닙니다. Medivance: hook timings `17176`, route decisions `452`, graph `254`, host-owned/changed records `183`",
    );

    let lines = render_system_message(&msg, 72, crate::config::DiffDisplayMode::Off);
    let plain = lines
        .iter()
        .map(extract_line_text)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(plain.contains("thin data"));
    assert!(
        lines.iter().all(|line| line.width() <= 72),
        "system markdown should wrap parsed spans within viewport: {:?}",
        lines.iter().map(extract_line_text).collect::<Vec<_>>()
    );
    assert!(!plain.contains("```"));
    assert!(!plain.contains("|---|"));
}

#[test]
fn render_assistant_message_wraps_parsed_long_lines() {
    let msg = DisplayMessage::assistant(
        "네, 둘 다 확인했습니다. `~/dev/medivance`: record-audit ok=true, hook timings `17,176`, route decisions `452`, host-owned/changed `183`\n\n- `~/dev/medivance-pwa`: record-audit ok=true, hook timings `3,649`, route decisions `68`, host-owned/changed `67`",
    );

    let lines = render_assistant_message(&msg, 72, crate::config::DiffDisplayMode::Off);
    let plain = lines
        .iter()
        .map(extract_line_text)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(plain.contains("medivance"));
    assert!(plain.contains("medivance-pwa"));
    assert!(
        lines.iter().all(|line| line.width() <= 72),
        "assistant markdown should wrap parsed spans within viewport: {:?}",
        lines.iter().map(extract_line_text).collect::<Vec<_>>()
    );
}

#[test]
fn render_assistant_message_keeps_nested_summary_bullets_on_separate_lines() {
    let msg = DisplayMessage::assistant(
        "1. **Markdown/list/option 렌더링 문제들**\n   - glued markdown list marker 복구\n   - markdown list continuation 줄바꿈 보존\n   - A., B. 선택지 앞 줄바꿈 보존\n   - 다음은 선택지입니다:A.처럼 콜론 뒤에 바로 붙은 A. 분리\n   - markdown table boundary 보존\n   - CJK wrap에서 한 글자/토큰이 이상하게 고아처럼 남는 문제\n\n2. **큐/백그라운드 prompt 문제**\n   - tool backgrounding 후 queued prompt가 dispatch 안 되던 문제\n   - queued prompt에 붙은 이미지가 전송에서 빠지던 문제\n   - reload/recovery/remote follow-up에서도 queued image metadata가 안 어긋나게 수정",
    );

    let lines = render_assistant_message(&msg, 96, crate::config::DiffDisplayMode::Off);
    let plain_lines = lines.iter().map(extract_line_text).collect::<Vec<_>>();

    assert!(
        plain_lines.iter().any(|line| line
            .trim_start()
            .starts_with("1. Markdown/list/option 렌더링 문제들")),
        "first numbered heading should render: {plain_lines:?}"
    );
    assert!(
        plain_lines.iter().any(|line| line
            .trim_start()
            .starts_with("• glued markdown list marker 복구")),
        "nested bullet should render on its own line: {plain_lines:?}"
    );
    assert!(
        plain_lines.iter().any(|line| line
            .trim_start()
            .starts_with("2. 큐/백그라운드 prompt 문제")),
        "second numbered heading should render separately: {plain_lines:?}"
    );
    assert!(
        plain_lines
            .iter()
            .all(|line| !line.contains("문제들 -") && !line.contains("문제2.")),
        "nested bullet and next marker must not be glued: {plain_lines:?}"
    );
}

#[test]
fn render_assistant_message_keeps_nested_summary_bullets_in_centered_mode() {
    let saved = crate::tui::markdown::center_code_blocks();
    crate::tui::markdown::set_center_code_blocks(true);
    let msg = DisplayMessage::assistant(
        "1. **Markdown/list/option 렌더링 문제들**\n   - glued markdown list marker 복구\n   - markdown list continuation 줄바꿈 보존\n   - A., B. 선택지 앞 줄바꿈 보존\n   - 다음은 선택지입니다:A.처럼 콜론 뒤에 바로 붙은 A. 분리\n   - markdown table boundary 보존\n   - CJK wrap에서 한 글자/토큰이 이상하게 고아처럼 남는 문제\n\n2. **큐/백그라운드 prompt 문제**\n   - tool backgrounding 후 queued prompt가 dispatch 안 되던 문제\n   - queued prompt에 붙은 이미지가 전송에서 빠지던 문제\n   - reload/recovery/remote follow-up에서도 queued image metadata가 안 어긋나게 수정",
    );

    let lines = render_assistant_message(&msg, 140, crate::config::DiffDisplayMode::Off);
    crate::tui::markdown::set_center_code_blocks(saved);
    let plain_lines = lines.iter().map(extract_line_text).collect::<Vec<_>>();

    assert!(
        plain_lines.iter().any(|line| line
            .trim_start()
            .starts_with("1. Markdown/list/option 렌더링 문제들")),
        "first numbered heading should render: {plain_lines:?}"
    );
    assert!(
        plain_lines.iter().any(|line| line
            .trim_start()
            .starts_with("• glued markdown list marker 복구")),
        "nested bullet should render on its own line in centered mode: {plain_lines:?}"
    );
    assert!(
        plain_lines.iter().any(|line| line
            .trim_start()
            .starts_with("2. 큐/백그라운드 prompt 문제")),
        "second numbered heading should render separately: {plain_lines:?}"
    );
    assert!(
        plain_lines
            .iter()
            .all(|line| !line.contains("문제들 -") && !line.contains("문제2.")),
        "nested bullet and next marker must not be glued in centered mode: {plain_lines:?}"
    );
}

#[test]
fn render_assistant_message_avoids_orphaned_korean_token_before_option_label() {
    let msg = DisplayMessage::assistant(
        "맞아요. record 기준으로 **Phase 3 hook replacement 말고 남은 큰 축이 하나 더 있습니다.**가장 가능성 높은 건 A. Capability Registry dogfood evaluation (Recommended next check)\n\n- lazy capability audit/list/resolve 돌려서\n- Medivance 실제 workflow에서 capability가 잘 잡히는지 봅니다.",
    );

    let lines = render_assistant_message(&msg, 96, crate::config::DiffDisplayMode::Off);
    let plain_lines = lines.iter().map(extract_line_text).collect::<Vec<_>>();

    assert!(
        plain_lines
            .iter()
            .any(|line| line.contains("A. Capability")),
        "expected option label line in rendered output: {plain_lines:?}"
    );
    assert!(
        plain_lines
            .iter()
            .all(|line| !line.trim_end().ends_with("가장")),
        "wrap should not orphan `가장` at the end of a line before `A. Capability`: {plain_lines:?}"
    );
    assert!(
        lines.iter().all(|line| line.width() <= 96),
        "assistant markdown should remain within width after de-orphaning: {plain_lines:?}"
    );
}

#[test]
fn render_assistant_message_splits_recommended_colon_options() {
    let msg = DisplayMessage::assistant(
        "이건 분석+결정 게이트입니다. 슬롯 너비를 키우는 방향 옵션:\n\nA (Recommended): MIN_ROOM_SLOT_WIDTH = 34 → 38. inset(4px)을 더해 칩이 정확히 Figma 34px가 되게 함. 가장 정확. B: MIN_ROOM_SLOT_WIDTH는 34 유지하되 셀 좌우 inset(2px씩)을 줄이거나 칩 px-2를 px-1로. C: 직접 입력",
    );

    let lines = render_assistant_message(&msg, 120, crate::config::DiffDisplayMode::Off);
    let plain_lines = lines.iter().map(extract_line_text).collect::<Vec<_>>();

    assert!(
        plain_lines
            .iter()
            .any(|line| line.trim_start().starts_with("A (Recommended): MIN_ROOM")),
        "A recommended option should render as a distinct line: {plain_lines:?}"
    );
    assert!(
        plain_lines
            .iter()
            .any(|line| line.trim_start().starts_with("B: MIN_ROOM_SLOT_WIDTH")),
        "B colon option should render as a distinct line: {plain_lines:?}"
    );
    assert!(
        plain_lines
            .iter()
            .any(|line| line.trim_start().starts_with("C: 직접 입력")),
        "C colon option should render as a distinct line: {plain_lines:?}"
    );
    assert!(
        plain_lines
            .iter()
            .all(|line| !line.contains("정확. B:") && !line.contains("px-1로. C:")),
        "B/C options must not remain glued inside the A option line: {plain_lines:?}"
    );
}

#[test]
fn render_assistant_message_keeps_wrapped_table_cell_continuation_in_table() {
    let msg = DisplayMessage::assistant(
        "확인하고 싶습니다. 어느 방향인가요?\n\n| 옵션 | 작업 |\n|---|---|\n| A (Recommended) | 일정 알림 토스트를 상단 중앙에 표시 (위치만 변경, 설정 기능 없이) |\n| B | 토스트 위치 선택 설정 기능 신설 (상단중앙/우상단/우하단 등)\n- 그 값 따름 |\n| C | Figma에 토스트 위치 명시가 있는지 다시 확인 (다른 노드/화면) |\n| D | 직접 알려주기 (위치/설정 범위) |\n\n어떻게 할까요?",
    );

    let lines = render_assistant_message(&msg, 120, crate::config::DiffDisplayMode::Off);
    let plain_lines = lines.iter().map(extract_line_text).collect::<Vec<_>>();

    assert!(
        plain_lines
            .iter()
            .any(|line| line.contains("B") && line.contains("그 값 따름")),
        "B continuation should stay in the table row: {plain_lines:?}"
    );
    assert!(
        plain_lines
            .iter()
            .any(|line| line.contains("C") && line.contains("Figma")),
        "C row should render as table output: {plain_lines:?}"
    );
    assert!(
        plain_lines
            .iter()
            .any(|line| line.contains("D") && line.contains("직접 알려주기")),
        "D row should render as table output: {plain_lines:?}"
    );
    assert!(
        plain_lines
            .iter()
            .all(|line| !line.trim_start().starts_with("• 그 값 따름")
                && !line.trim_start().starts_with("| C |")
                && !line.trim_start().starts_with("| D |")),
        "wrapped table continuation must not leak as a bullet/raw pipe rows: {plain_lines:?}"
    );
}

#[test]
fn render_assistant_message_splits_glued_blockquote_marker() {
    let msg = DisplayMessage::assistant(
        "즉 목표를 이렇게 바꾸면 좋겠습니다:> “lazy-harness는 느린 감시자가 아니라, 빠른 분류기 + 강제 실행 경계다.”\n이 방향으로 계획을 수정하면 됩니다.",
    );

    let lines = render_assistant_message(&msg, 120, crate::config::DiffDisplayMode::Off);
    let plain_lines = lines.iter().map(extract_line_text).collect::<Vec<_>>();

    assert_eq!(
        plain_lines.first().map(String::as_str),
        Some("즉 목표를 이렇게 바꾸면 좋겠습니다:")
    );
    assert!(
        plain_lines
            .iter()
            .any(|line| line.trim_start().starts_with("│ “lazy-harness는")),
        "glued blockquote should render as a distinct quoted line: {plain_lines:?}"
    );
    assert!(
        plain_lines
            .iter()
            .all(|line| !line.contains("좋겠습니다:>") && !line.contains(":> “lazy")),
        "blockquote marker must not remain glued in TUI render: {plain_lines:?}"
    );
}

#[test]
fn render_assistant_message_de_orphans_cjk_after_centered_indent() {
    let saved = crate::tui::markdown::center_code_blocks();
    crate::tui::markdown::set_center_code_blocks(true);
    let msg = DisplayMessage::assistant(
        "맞아요, 이제부터는 계속 사용하면서 자동 후보는 쌓이고, 확정 record는 내가/우리가 확인 후 승격하는 방식입니다. 정확히는:\n\n- 자동으로 쌓이는 것\n  - `.lazy-harness/knowledge/candidates.jsonl`\n  - BDD/user-flow 같은 ‘후보’는 hook이 조용히 dedupe 저장할 수 있음\n  - 현재도 candidate queue에 기록 있음 – 자동으로 확정되면 안 되는 것\n  - `.lazy-harness/domain/`, `spec/`, `behavior/`, `tests/`, `decisions/`, `ssot/`의 canonical record – `.lazy-harness/knowledge/graph.jsonl`의 confirmed fact – 이건 사용자 확인, 코드/테스트 근거, source read, implementation map이 필요함– 운영 방식 1. 평소처럼 사용2. 내가 작업/검수 중 발견한 지식은 candidate 또는 planning에 남김3. 사용자가 “맞다/이게 source of truth다/이렇게 하자”라고 확인하면 정식 record로 승격 4. `lazy record-audit`, `lazy impl-map`, graph-hygiene로 누락/품질을 주기적으로 확인한 줄로 말하면: 자동 기록 = 후보까지, canonical memory = 확인 후 축적입니다. 이 방향이 지금 만든 harness 원칙이랑 맞습니다.",
    );
    let lines = render_assistant_message(&msg, 112, crate::config::DiffDisplayMode::Off);
    crate::tui::markdown::set_center_code_blocks(saved);
    let plain_lines = lines.iter().map(extract_line_text).collect::<Vec<_>>();

    assert!(
        plain_lines
            .iter()
            .any(|line| line.contains("후 축적입니다")),
        "moved CJK token should be inserted after centered/list indentation: {plain_lines:?}"
    );
    assert!(
        plain_lines.iter().all(|line| {
            !line.contains("후     축적")
                && !line.contains("후    축적")
                && !line.contains("중     발견한")
                && !line.contains("중    발견한")
        }),
        "de-orphaning must not prepend CJK tokens before indentation padding: {plain_lines:?}"
    );
    assert!(
        lines.iter().all(|line| line.width() <= 112),
        "assistant markdown should stay within viewport: {plain_lines:?}"
    );
}

#[test]
fn render_assistant_message_preserves_lettered_option_source_lines() {
    let saved = crate::tui::markdown::center_code_blocks();
    crate::tui::markdown::set_center_code_blocks(true);
    let msg = DisplayMessage::assistant(
        "맞아요. 확인해보니 지금 seed는 제가 만든 가짜 SVG data URL이라 실제 업로드 preview 검증으로 부적절합니다. 수정 방향 옵션입니다.
A. (Recommended) public/dev-seed/pending-license-sample.pdf 예시
PDF를 추가하고, seed가 그 PDF URL을 보여주게 수정
B. 예시 이미지를 추가해서 이미지 preview 기준으로 검증
C. seed에는 파일 없음 상태를 보여주고, 실제 회원가입 업로드에서만 preview 표시
D. 직접 넣을 PDF/이미지 파일을 받아서 그걸 seed fixture로 사용추천은 A입니다. 없으면 “첨부파일 없음” empty state를 보여주고, PDF/이미지는 실제 파일 URL 또는 data URL을 <object>/img로 preview 하게 고치겠습니다.
A로 바로 진행해도 될까요?"
    );
    let lines = render_assistant_message(&msg, 112, crate::config::DiffDisplayMode::Off);
    crate::tui::markdown::set_center_code_blocks(saved);
    let plain_lines = lines.iter().map(extract_line_text).collect::<Vec<_>>();

    assert!(
        plain_lines.iter().any(|line| line
            .trim_start()
            .starts_with("A. (Recommended) public/dev-seed")),
        "A option should render on its source line: {plain_lines:?}"
    );
    assert!(
        plain_lines
            .iter()
            .any(|line| line.trim_start().starts_with("PDF를 추가하고")),
        "A option continuation should render on its source line: {plain_lines:?}"
    );
    assert!(
        plain_lines
            .iter()
            .any(|line| line.trim_start().starts_with("B. 예시 이미지를")),
        "B option should render on its source line: {plain_lines:?}"
    );
    assert!(
        plain_lines.iter().all(|line| {
            !line.contains("옵션입니다. A.") && !line.contains("수정 B. 예시")
        }),
        "lettered option lines should not collapse into adjacent prose: {plain_lines:?}"
    );
    assert!(
        lines.iter().all(|line| line.width() <= 112),
        "assistant markdown should stay within viewport: {plain_lines:?}"
    );
}

#[test]
fn render_system_message_centered_mode_left_aligns_with_padding() {
    let saved = crate::tui::markdown::center_code_blocks();
    crate::tui::markdown::set_center_code_blocks(true);
    let msg = DisplayMessage::system("Reload complete — continuing.");

    let lines = render_system_message(&msg, 80, crate::config::DiffDisplayMode::Off);

    assert!(!lines.is_empty(), "expected rendered system message lines");
    for line in &lines {
        assert_eq!(
            line.alignment,
            Some(ratatui::layout::Alignment::Left),
            "centered system lines should be left-aligned with padding"
        );
        assert!(
            line.spans
                .first()
                .is_some_and(|span| span.content.starts_with(' ')),
            "centered system lines should start with padding"
        );
    }
    crate::tui::markdown::set_center_code_blocks(saved);
}

#[test]
fn render_system_message_uses_width_stable_titles_on_kitty() {
    let _guard = system_glyph_env_lock();
    let prev_term_program = std::env::var("TERM_PROGRAM").ok();
    let prev_term = std::env::var("TERM").ok();
    crate::env::set_var("TERM_PROGRAM", "kitty");
    crate::env::set_var("TERM", "xterm-kitty");

    let msg = DisplayMessage::system(
        "⚡ Connection lost — retrying (attempt 2, 7s) — connection reset by server",
    )
    .with_title("Connection");

    let lines = render_system_message(&msg, 80, crate::config::DiffDisplayMode::Off);
    let plain = lines
        .iter()
        .map(extract_line_text)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(plain.contains("reconnecting"));
    assert!(!plain.contains("⚡ reconnecting"));

    match prev_term_program {
        Some(value) => crate::env::set_var("TERM_PROGRAM", value),
        None => crate::env::remove_var("TERM_PROGRAM"),
    }
    match prev_term {
        Some(value) => crate::env::set_var("TERM", value),
        None => crate::env::remove_var("TERM"),
    }
}

#[test]
fn render_background_task_message_uses_box_and_truncates_preview_lines() {
    let msg = DisplayMessage::background_task(
        "**Background task** `bg123` · `bash` · ✓ completed · 7.1s · exit 0\n\n```text\nline 1\nline 2\nline 3\nline 4\nline 5\n```\n\n_Full output:_ `bg action=\"output\" task_id=\"bg123\"`",
    );

    let lines = render_background_task_message(&msg, 80, crate::config::DiffDisplayMode::Off);
    let plain = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(plain.contains("✓ bg bash completed · bg123"));
    assert!(plain.contains("exit 0 · 7.1s"));
    assert!(plain.contains("line 1"));
    assert!(plain.contains("… +1 more line"));
    assert!(!plain.contains("task bg123 · bash"));
    assert!(!plain.contains("Preview"));
    assert!(!plain.contains("Full output"));
    assert!(!plain.contains("bg action=\"output\" task_id=\"bg123\""));
}

#[test]
fn render_background_task_progress_message_uses_box_with_progress_bar() {
    let msg = DisplayMessage::background_task(
        "**Background task progress** `bg123` · `bash`\n\n[#####-------] 42% · Running tests (reported)",
    );

    let lines = render_background_task_message(&msg, 80, crate::config::DiffDisplayMode::Off);
    let plain = lines
        .iter()
        .map(extract_line_text)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(plain.contains("◌ bg bash · bg123"));
    assert!(plain.contains("█"));
    assert!(plain.contains("░"));
    assert!(plain.contains("42%"));
    assert!(plain.contains("Running tests"));
    assert!(plain.contains("Latest status: bg action=\"status\" task_id=\"bg123\""));
    assert_eq!(
        plain.matches('│').count(),
        4,
        "expected compact progress row plus status hint:\n{plain}"
    );
    assert!(!plain.contains("Latest update"));
    assert!(!plain.contains("Source: reported"));
    assert!(!plain.contains("**Background task progress**"));
}

#[test]
fn render_overnight_message_uses_rounded_progress_card() {
    let card = crate::overnight::OvernightProgressCard {
        run_id: "overnight_1234567890abcdef".to_string(),
        status: "running".to_string(),
        phase: "running".to_string(),
        coordinator_session_id: "session_coord".to_string(),
        coordinator_session_name: "Overnight coordinator".to_string(),
        elapsed_label: "2h 15m".to_string(),
        target_duration_label: "7h".to_string(),
        progress_percent: 32.0,
        target_wake_at: "2026-05-01T15:00:00Z".to_string(),
        time_relation: "target in 4h 45m".to_string(),
        last_activity_label: "4m ago".to_string(),
        next_prompt_label: "handoff mode in 4h 15m or after current turn".to_string(),
        usage_risk: "medium".to_string(),
        usage_confidence: "low".to_string(),
        usage_projection: "projected 48% to 76%".to_string(),
        resources_summary: "RAM 62%, load 2.4/8, battery 80% discharging, disk 52.0 GB free"
            .to_string(),
        latest_event_kind: Some("coordinator_turn_completed".to_string()),
        latest_event_summary: Some("Coordinator turn completed".to_string()),
        task_summary: crate::overnight::OvernightTaskCardSummary {
            total: 4,
            counts: crate::overnight::OvernightTaskStatusCounts {
                completed: 2,
                active: 1,
                blocked: 0,
                deferred: 1,
                failed: 0,
                skipped: 0,
                unknown: 0,
            },
            validated: 2,
            high_risk: 0,
            latest_title: Some("Verify provider reload".to_string()),
            latest_status: Some("active".to_string()),
        },
        active_task_title: Some("Verify provider reload".to_string()),
        review_path: "/tmp/overnight/review.html".to_string(),
        log_path: "/tmp/overnight/run.log".to_string(),
        run_dir: "/tmp/overnight".to_string(),
        completed_at: None,
    };
    let msg = DisplayMessage::overnight(serde_json::to_string(&card).unwrap());

    let lines = render_overnight_message(&msg, 100, crate::config::DiffDisplayMode::Off);
    let plain = lines
        .iter()
        .map(extract_line_text)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(plain.contains("overnight · running"));
    assert!(plain.contains("█"));
    assert!(plain.contains("░"));
    assert!(plain.contains("32%"));
    assert!(plain.contains("2 complete, 1 active, 0 blocked, 1 deferred"));
    assert!(plain.contains("Verify provider reload"));
    assert!(plain.contains("medium risk"));
    assert!(plain.contains("review.html"));
}

#[test]
fn render_background_task_messages_prefer_display_name() {
    let completion = DisplayMessage::background_task(
        "**Background task** `bg123` · `Run integration tests` (`bash`) · ✓ completed · 7.1s · exit 0\n\n_No output captured._\n\n_Full output:_ `bg action=\"output\" task_id=\"bg123\"`",
    );
    let completion_plain =
        render_background_task_message(&completion, 100, crate::config::DiffDisplayMode::Off)
            .iter()
            .map(extract_line_text)
            .collect::<Vec<_>>()
            .join("\n");
    assert!(completion_plain.contains("✓ bg Run integration tests completed · bg123"));

    let progress = DisplayMessage::background_task(
        "**Background task progress** `bg123` · `Run integration tests` (`bash`)\n\n[#####-------] 42% · Running tests (reported)",
    );
    let progress_plain =
        render_background_task_message(&progress, 100, crate::config::DiffDisplayMode::Off)
            .iter()
            .map(extract_line_text)
            .collect::<Vec<_>>()
            .join("\n");
    assert!(progress_plain.contains("◌ bg Run integration tests · bg123"));
}

#[test]
fn render_system_message_uses_scheduled_task_card() {
    let msg = DisplayMessage::system(
        "[Scheduled task]\nA scheduled task for this session is now due.\n\nTask: Follow up on the scheduler test\nWorking directory: /home/jeremy/jcode\nRelevant files: src/tui/ui_messages.rs\nBranch: master\n\nBackground: Verify the scheduled task card styling\nSuccess criteria: The due task renders clearly\nScheduled by session: session_test",
    );

    let lines = render_system_message(&msg, 100, crate::config::DiffDisplayMode::Off);
    let plain = lines
        .iter()
        .map(extract_line_text)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(plain.contains(width_stable_system_title(
        "⏰ scheduled task due",
        "scheduled task due"
    )));
    assert!(plain.contains("This scheduled task is now active in this session."));
    assert!(plain.contains("Follow up on the scheduler test"));
    assert!(plain.contains("Verify the scheduled task card styling"));
    assert!(!plain.contains("[Scheduled task]"));
    assert!(!plain.contains("A scheduled task for this session is now due."));
}

#[test]
fn render_tool_message_uses_scheduled_card() {
    let msg = DisplayMessage {
        role: "tool".to_string(),
        content: "Scheduled task 'Follow up on the scheduler test' for in 1m (id: sched_abc123)\nWorking directory: /home/jeremy/jcode\nRelevant files: src/tui/ui_messages.rs\nTarget: resume session session_test".to_string(),
        tool_calls: Vec::new(),
        duration_secs: None,
        title: Some("scheduled: Follow up on the scheduler test".to_string()),
        tool_data: Some(crate::message::ToolCall {
            id: "call_schedule_card".to_string(),
            name: "schedule".to_string(),
            input: serde_json::json!({
                "task": "Follow up on the scheduler test",
                "wake_in_minutes": 1,
                "target": "resume"
            }),
            intent: None,
        }),
    };

    let lines = render_tool_message(&msg, 100, crate::config::DiffDisplayMode::Off);
    let plain = lines
        .iter()
        .map(extract_line_text)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(plain.contains(width_stable_system_title("⏰ scheduled", "scheduled")));
    assert!(plain.contains("Will run in 1m."));
    assert!(plain.contains("Follow up on the scheduler test"));
    assert!(plain.contains("session session_test"));
    assert!(plain.contains("sched_abc123"));
    assert!(!plain.contains("✓ schedule"));
}

#[test]
fn render_assistant_message_truncates_tool_calls_to_single_line() {
    let saved = crate::tui::markdown::center_code_blocks();
    crate::tui::markdown::set_center_code_blocks(false);
    let msg = DisplayMessage {
        role: "assistant".to_string(),
        content: "Done.".to_string(),
        tool_calls: vec![
            "read".to_string(),
            "grep".to_string(),
            "apply_patch".to_string(),
            "batch".to_string(),
        ],
        duration_secs: None,
        title: None,
        tool_data: None,
    };

    let lines = render_assistant_message(&msg, 20, crate::config::DiffDisplayMode::Off);
    assert_eq!(extract_line_text(&lines[1]), "");
    let tool_lines: Vec<String> = lines
        .iter()
        .skip(2)
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect();

    assert!(
        tool_lines.len() == 1,
        "expected single-line tool-call summary: {tool_lines:?}"
    );
    assert!(
        tool_lines[0].contains("tools:"),
        "expected tool summary label on first line: {tool_lines:?}"
    );
    assert!(
        tool_lines.iter().all(|line| line.width() <= 20),
        "tool-call summary line should respect available width: {tool_lines:?}"
    );
    crate::tui::markdown::set_center_code_blocks(saved);
}

#[test]
fn render_assistant_message_centers_single_line_tool_summary() {
    let saved = crate::tui::markdown::center_code_blocks();
    crate::tui::markdown::set_center_code_blocks(true);
    let msg = DisplayMessage {
        role: "assistant".to_string(),
        content: "Done.".to_string(),
        tool_calls: vec![
            "read".to_string(),
            "grep".to_string(),
            "apply_patch".to_string(),
            "batch".to_string(),
        ],
        duration_secs: None,
        title: None,
        tool_data: None,
    };

    let lines = render_assistant_message(&msg, 28, crate::config::DiffDisplayMode::Off);
    assert_eq!(extract_line_text(&lines[1]), "");
    let tool_lines: Vec<String> = lines
        .iter()
        .skip(2)
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect();

    assert!(
        tool_lines.len() == 1,
        "expected single-line tool-call summary: {tool_lines:?}"
    );
    let first_pad = tool_lines[0].chars().take_while(|c| *c == ' ').count();
    assert!(
        first_pad > 0,
        "tool summary should still be padded/centered as a block: {tool_lines:?}"
    );
    assert!(
        lines
            .iter()
            .skip(2)
            .all(|line| line.alignment == Some(ratatui::layout::Alignment::Left)),
        "centered tool summary should use a shared left-aligned block pad"
    );

    crate::tui::markdown::set_center_code_blocks(saved);
}

#[test]
fn render_assistant_message_without_body_does_not_add_extra_blank_line_before_tool_summary() {
    let saved = crate::tui::markdown::center_code_blocks();
    crate::tui::markdown::set_center_code_blocks(false);
    let msg = DisplayMessage {
        role: "assistant".to_string(),
        content: String::new(),
        tool_calls: vec!["read".to_string()],
        duration_secs: None,
        title: None,
        tool_data: None,
    };

    let lines = render_assistant_message(&msg, 28, crate::config::DiffDisplayMode::Off);
    let rendered: Vec<String> = lines.iter().map(extract_line_text).collect();

    assert_eq!(rendered.len(), 1, "rendered={rendered:?}");
    assert!(rendered[0].contains("tool:"), "rendered={rendered:?}");

    crate::tui::markdown::set_center_code_blocks(saved);
}

#[test]
fn render_assistant_message_hides_count_noise_around_tool_summary() {
    let msg = DisplayMessage {
        role: "assistant".to_string(),
        content: "count\n\ncount".to_string(),
        title: None,
        tool_calls: vec!["read src/lib.rs".to_string()],
        duration_secs: None,
        tool_data: None,
    };

    let lines = render_assistant_message(&msg, 80, crate::config::DiffDisplayMode::Off);
    let rendered = lines
        .iter()
        .map(crate::tui::ui::line_plain_text)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(!rendered.lines().any(|line| line.trim() == "count"));
    assert!(rendered.contains("read src/lib.rs"));
}

#[test]
fn render_assistant_message_strips_count_edges_but_preserves_body() {
    let msg = DisplayMessage {
        role: "assistant".to_string(),
        content: "count\n\nI will inspect it.\n\ncount".to_string(),
        title: None,
        tool_calls: vec!["read src/lib.rs".to_string()],
        duration_secs: None,
        tool_data: None,
    };

    let lines = render_assistant_message(&msg, 80, crate::config::DiffDisplayMode::Off);
    let rendered = lines
        .iter()
        .map(crate::tui::ui::line_plain_text)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("I will inspect it."));
    assert!(!rendered.lines().any(|line| line.trim() == "count"));
    assert!(rendered.contains("read src/lib.rs"));
}

#[test]
fn render_assistant_message_strips_count_lines_before_interrupted_marker() {
    let msg = DisplayMessage {
        role: "assistant".to_string(),
        content: "회의 개별 칩 색을 확인합니다.\n\ncount\n\ncount\n\n[Interrupted: user cancelled]"
            .to_string(),
        title: None,
        tool_calls: Vec::new(),
        duration_secs: None,
        tool_data: None,
    };

    let lines = render_assistant_message(&msg, 80, crate::config::DiffDisplayMode::Off);
    let rendered = lines
        .iter()
        .map(crate::tui::ui::line_plain_text)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("회의 개별 칩 색을 확인합니다."));
    assert!(rendered.contains("[Interrupted: user cancelled]"));
    assert!(!rendered.lines().any(|line| line.trim() == "count"));
}

#[test]
fn render_assistant_message_centered_mode_keeps_markdown_unpadded_for_center_alignment() {
    let saved = crate::tui::markdown::center_code_blocks();
    crate::tui::markdown::set_center_code_blocks(true);
    let msg = DisplayMessage::assistant(
        "streaming-block streaming-block streaming-block streaming-block",
    );

    let lines = render_assistant_message(&msg, 120, crate::config::DiffDisplayMode::Off);
    let content_line = lines
        .iter()
        .find(|line| extract_line_text(line).contains("streaming-block"))
        .expect("expected assistant markdown line");

    let first_pad = extract_line_text(content_line)
        .chars()
        .take_while(|c| *c == ' ')
        .count();
    assert_eq!(
        first_pad, 0,
        "centered assistant markdown should not inject left padding: {lines:?}"
    );
    assert_eq!(
        content_line.alignment, None,
        "assistant render should leave centered prose alignment unset for outer centering"
    );

    crate::tui::markdown::set_center_code_blocks(saved);
}

#[test]
fn render_assistant_message_recenters_structured_markdown_to_actual_width() {
    let saved = crate::tui::markdown::center_code_blocks();
    crate::tui::markdown::set_center_code_blocks(true);
    let msg = DisplayMessage::assistant("- one\n- two");

    let lines = render_assistant_message(&msg, 140, crate::config::DiffDisplayMode::Off);
    let rendered: Vec<String> = lines.iter().map(extract_line_text).collect();
    let bullets: Vec<&String> = rendered.iter().filter(|line| line.contains("• ")).collect();

    assert_eq!(
        bullets.len(),
        2,
        "expected two rendered bullet lines: {rendered:?}"
    );
    let first_pad = leading_spaces(bullets[0]);
    let second_pad = leading_spaces(bullets[1]);
    assert_eq!(
        first_pad, second_pad,
        "simple list should share a block pad: {rendered:?}"
    );
    assert!(
        first_pad > 45,
        "list should be re-centered to the full display width: {rendered:?}"
    );
    assert!(
        bullets
            .iter()
            .all(|line| line[leading_spaces(line)..].starts_with("• ")),
        "bullet markers should remain flush-left within the centered block: {rendered:?}"
    );

    crate::tui::markdown::set_center_code_blocks(saved);
}

#[test]
fn render_assistant_message_centered_mode_uses_wide_prose_width() {
    let saved = crate::tui::markdown::center_code_blocks();
    crate::tui::markdown::set_center_code_blocks(true);
    let msg = DisplayMessage::assistant(
        "1. Custom overlay toast (Recommended)\n   - 우측 하단 커스텀 알림창, `overlay-toast.ts`\n2. OS native notification – Electron `Notification`, `notification.ts`\n3. OS taskbar overlay badge – Windows 작업표시줄 미읽음 숫자, `window-state.ts`일반 채팅/리마인드 쪽은 NotificationWatcher가 알림 payload 만들고, 채널 설정에 따라 custom overlay/native 쪽으로 dispatch하는 구조야.",
    );

    let lines = render_assistant_message(&msg, 180, crate::config::DiffDisplayMode::Off);
    crate::tui::markdown::set_center_code_blocks(saved);
    let rendered: Vec<String> = lines.iter().map(extract_line_text).collect();

    assert!(
        rendered.iter().any(|line| {
            line.contains("OS taskbar overlay badge") && line.contains("payload 만들고")
        }),
        "wide centered assistant prose should not be capped at the old 96-column wrap: {rendered:?}"
    );
    assert!(
        lines.iter().all(|line| line.width() <= 180),
        "assistant prose should stay within the available terminal width: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .filter(|line| !line.trim().is_empty())
            .all(|line| leading_spaces(line) >= 8),
        "structured assistant prose should still preserve a visible centered gutter: {rendered:?}"
    );
}

#[test]
fn render_system_message_centered_mode_caps_wrap_width_for_visible_gutters() {
    let saved = crate::tui::markdown::center_code_blocks();
    crate::tui::markdown::set_center_code_blocks(true);
    let msg = DisplayMessage::system(
        "This is a long centered-mode system notification that should keep visible side gutters instead of stretching nearly edge to edge in a wide terminal.",
    );

    let lines = render_system_message(&msg, 120, crate::config::DiffDisplayMode::Off);
    let rendered: Vec<String> = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect();

    assert!(
        rendered.iter().all(|line| line.starts_with("          ")),
        "centered system message should retain visible left padding in wide layouts: {rendered:?}"
    );

    crate::tui::markdown::set_center_code_blocks(saved);
}

#[test]
fn render_system_message_uses_reload_card_for_reload_title() {
    let msg = DisplayMessage::system("Reloading server with newer binary...").with_title("Reload");

    let lines = render_system_message(&msg, 80, crate::config::DiffDisplayMode::Off);
    let plain = lines
        .iter()
        .map(extract_line_text)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        plain.contains("reload"),
        "expected reload card title: {plain}"
    );
    assert!(plain.contains("Reloading server with newer binary"));
}

#[test]
fn render_system_message_uses_connection_card_for_reconnect_status() {
    let msg = DisplayMessage::system(
        "⚡ Connection lost — retrying (attempt 2, 7s) — connection reset by server · resume: jcode --resume koala",
    )
    .with_title("Connection");

    let lines = render_system_message(&msg, 80, crate::config::DiffDisplayMode::Off);
    let plain = lines
        .iter()
        .map(extract_line_text)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        plain.contains("reconnecting"),
        "expected reconnect card title: {plain}"
    );
    assert!(plain.contains("Retrying · attempt 2 · 7s"));
    assert!(plain.contains("connection reset by server"));
    assert!(plain.contains("jcode --resume koala"));
}

#[test]
fn render_swarm_message_centered_mode_caps_wrap_width_for_long_notifications() {
    let saved = crate::tui::markdown::center_code_blocks();
    crate::tui::markdown::set_center_code_blocks(true);
    let msg = DisplayMessage::swarm(
        "File activity",
        "/home/jeremy/jcode/src/tui/ui_messages.rs — moss just edited this file while you were working nearby, so the notification should still read as centered in wide layouts.",
    );

    let lines = render_swarm_message(&msg, 120, crate::config::DiffDisplayMode::Off);
    let rendered: Vec<String> = lines.iter().map(extract_line_text).collect();
    let first_pad = rendered[0].chars().take_while(|c| *c == ' ').count();

    assert!(
        first_pad >= 8,
        "centered swarm notification should keep a clearly visible left gutter: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .all(|line| line.is_empty() || line.starts_with(&" ".repeat(first_pad))),
        "centered swarm notification should share one left pad across wrapped lines: {rendered:?}"
    );

    crate::tui::markdown::set_center_code_blocks(saved);
}

#[test]
fn render_tool_message_prefers_subagent_title_with_model() {
    let msg = DisplayMessage {
        role: "tool".to_string(),
        content: "done".to_string(),
        tool_calls: Vec::new(),
        duration_secs: None,
        title: Some("Verify subagent model (general · gpt-5.4)".to_string()),
        tool_data: Some(crate::message::ToolCall {
            id: "call_1".to_string(),
            name: "subagent".to_string(),
            input: serde_json::json!({
                "description": "Verify subagent model",
                "subagent_type": "general"
            }),
            intent: None,
        }),
    };

    let lines = render_tool_message(&msg, 80, crate::config::DiffDisplayMode::Off);
    let rendered: String = lines[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();

    assert!(rendered.contains("subagent Verify subagent model (general · gpt-5.4)"));
}

#[test]
fn render_tool_message_shows_intent_and_technical_preview_on_one_line() {
    let msg = DisplayMessage {
        role: "tool".to_string(),
        content: "ok".to_string(),
        tool_calls: Vec::new(),
        duration_secs: None,
        title: None,
        tool_data: Some(crate::message::ToolCall {
            id: "call_intent".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({
                "command": "cargo test -p jcode render_background_task --lib",
                "intent": "Verify compact progress card"
            }),
            intent: Some("Verify compact progress card".to_string()),
        }),
    };

    let lines = render_tool_message(&msg, 120, crate::config::DiffDisplayMode::Off);
    let rendered = extract_line_text(&lines[0]);

    assert!(rendered.contains("bash · Verify compact progress card · $ cargo test"));
    assert_eq!(
        lines.len(),
        1,
        "intent should not add vertical space: {rendered}"
    );
}

#[test]
fn render_tool_message_shows_token_badge() {
    let msg = DisplayMessage {
        role: "tool".to_string(),
        content: "x".repeat(7_600),
        tool_calls: Vec::new(),
        duration_secs: None,
        title: None,
        tool_data: Some(crate::message::ToolCall {
            id: "call_2".to_string(),
            name: "read".to_string(),
            input: serde_json::json!({"file_path": "src/main.rs"}),
            intent: None,
        }),
    };

    let lines = render_tool_message(&msg, 120, crate::config::DiffDisplayMode::Off);
    let badge_span = lines[0]
        .spans
        .iter()
        .find(|span| span.content.contains("1.9k tok"))
        .expect("missing token badge");

    assert_eq!(badge_span.style.fg, Some(rgb(118, 118, 118)));
}

#[test]
fn render_tool_message_colors_high_token_badge() {
    let msg = DisplayMessage {
        role: "tool".to_string(),
        content: "x".repeat(48_000),
        tool_calls: Vec::new(),
        duration_secs: None,
        title: None,
        tool_data: Some(crate::message::ToolCall {
            id: "call_3".to_string(),
            name: "read".to_string(),
            input: serde_json::json!({"file_path": "src/main.rs"}),
            intent: None,
        }),
    };

    let lines = render_tool_message(&msg, 120, crate::config::DiffDisplayMode::Off);
    let badge_span = lines[0]
        .spans
        .iter()
        .find(|span| span.content.contains("12k tok"))
        .expect("missing token badge");

    assert_eq!(badge_span.style.fg, Some(rgb(224, 118, 118)));
}

#[test]
fn render_tool_message_shows_inline_diff_for_pascal_case_multiedit() {
    let msg = DisplayMessage {
        role: "tool".to_string(),
        content: "Edited demo.txt\n\nApplied:\n  ✓ Edit 1: replaced 1 occurrence\n\nTotal: 1 applied, 0 failed\n"
            .to_string(),
        tool_calls: Vec::new(),
        duration_secs: None,
        title: Some("demo.txt".to_string()),
        tool_data: Some(crate::message::ToolCall {
            id: "call_multiedit_pascal".to_string(),
            name: "MultiEdit".to_string(),
            input: serde_json::json!({
                "file_path": "demo.txt",
                "edits": [
                    {"old_string": "old line\n", "new_string": "new line\n"}
                ]
            }),
            intent: None,
        }),
    };

    let lines = render_tool_message(&msg, 100, crate::config::DiffDisplayMode::Inline);
    let plain = lines
        .iter()
        .map(extract_line_text)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(plain.contains("┌─ diff"), "plain={plain}");
    assert!(plain.contains("old line"), "plain={plain}");
    assert!(plain.contains("new line"), "plain={plain}");
}

#[test]
fn render_tool_message_inline_mode_truncates_large_diffs() {
    let old = (1..=7)
        .map(|i| format!("old line {i}\n"))
        .collect::<String>();
    let new = (1..=7)
        .map(|i| format!("new line {i} suffix_{i}_abcdefghijklmnopqrstuvwxyz0123456789\n"))
        .collect::<String>();
    let msg = DisplayMessage {
        role: "tool".to_string(),
        content: "Edited demo.txt".to_string(),
        tool_calls: Vec::new(),
        duration_secs: None,
        title: Some("demo.txt".to_string()),
        tool_data: Some(crate::message::ToolCall {
            id: "call_edit_inline_truncated".to_string(),
            name: "edit".to_string(),
            input: serde_json::json!({
                "file_path": "demo.txt",
                "old_string": old,
                "new_string": new,
            }),
            intent: None,
        }),
    };

    let lines = render_tool_message(&msg, 40, crate::config::DiffDisplayMode::Inline);
    let plain = lines
        .iter()
        .map(extract_line_text)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(plain.contains("... 2 more changes ..."), "plain={plain}");
    assert!(plain.contains("old line 3"), "plain={plain}");
    assert!(!plain.contains("old line 7"), "plain={plain}");
    assert!(
        !plain.contains("new line 1 suffix_1_abcdefghijklmnopqrstuvwxyz0123456789"),
        "plain={plain}"
    );
    assert!(plain.contains("suffix_2_abcdefghijklm…"), "plain={plain}");
}

#[test]
fn render_tool_message_full_inline_mode_shows_full_diff() {
    let old = (1..=7)
        .map(|i| format!("old line {i}\n"))
        .collect::<String>();
    let new = (1..=7)
        .map(|i| format!("new line {i} suffix_{i}_abcdefghijklmnopqrstuvwxyz0123456789\n"))
        .collect::<String>();
    let msg = DisplayMessage {
        role: "tool".to_string(),
        content: "Edited demo.txt".to_string(),
        tool_calls: Vec::new(),
        duration_secs: None,
        title: Some("demo.txt".to_string()),
        tool_data: Some(crate::message::ToolCall {
            id: "call_edit_inline_full".to_string(),
            name: "edit".to_string(),
            input: serde_json::json!({
                "file_path": "demo.txt",
                "old_string": old,
                "new_string": new,
            }),
            intent: None,
        }),
    };

    let lines = render_tool_message(&msg, 40, crate::config::DiffDisplayMode::FullInline);
    let plain = lines
        .iter()
        .map(extract_line_text)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(!plain.contains("more changes"), "plain={plain}");
    assert!(plain.contains("old line 4"), "plain={plain}");
    assert!(
        plain.contains("new line 4 suffix_4_abcdefghijklmnopqrstuvwxyz0123456789"),
        "plain={plain}"
    );
    assert!(!plain.contains('…'), "plain={plain}");
}

#[test]
fn render_tool_message_memory_recall_centered_mode_left_aligns_with_padding() {
    let saved = crate::tui::markdown::center_code_blocks();
    crate::tui::markdown::set_center_code_blocks(true);
    let msg = DisplayMessage {
        role: "tool".to_string(),
        content: concat!(
            "- [fact] Centered mode should keep the recall card centered\n",
            "- [preference] The user likes visible side gutters"
        )
        .to_string(),
        tool_calls: Vec::new(),
        duration_secs: None,
        title: None,
        tool_data: Some(crate::message::ToolCall {
            id: "call_memory_recall_centered".to_string(),
            name: "memory".to_string(),
            input: serde_json::json!({
                "action": "recall",
                "query": "centered mode"
            }),
            intent: None,
        }),
    };

    let lines = render_tool_message(&msg, 120, crate::config::DiffDisplayMode::Off);
    let rendered: Vec<String> = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect();

    assert!(!rendered.is_empty(), "expected rendered recall card");
    assert!(
        rendered.iter().all(|line| line.starts_with("  ")),
        "centered recall card should include shared left padding: {rendered:?}"
    );
    assert_eq!(
        lines[0].alignment,
        Some(ratatui::layout::Alignment::Left),
        "centered recall card header should be left-aligned after padding"
    );
    assert!(
        rendered[0]
            .trim_start()
            .starts_with("🧠 recalled 2 memories"),
        "unexpected recall header: {rendered:?}"
    );

    crate::tui::markdown::set_center_code_blocks(saved);
}

#[test]
fn render_tool_message_memory_store_centered_mode_left_aligns_with_padding() {
    let saved = crate::tui::markdown::center_code_blocks();
    crate::tui::markdown::set_center_code_blocks(true);
    let msg = DisplayMessage {
        role: "tool".to_string(),
        content: "Saved memory".to_string(),
        tool_calls: Vec::new(),
        duration_secs: None,
        title: None,
        tool_data: Some(crate::message::ToolCall {
            id: "call_memory_store_centered".to_string(),
            name: "memory".to_string(),
            input: serde_json::json!({
                "action": "remember",
                "category": "fact",
                "content": "Centered mode should pad saved memory cards too"
            }),
            intent: None,
        }),
    };

    let lines = render_tool_message(&msg, 120, crate::config::DiffDisplayMode::Off);
    let rendered: Vec<String> = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect();

    assert!(!rendered.is_empty(), "expected rendered saved-memory card");
    assert!(
        rendered.iter().all(|line| line.starts_with("  ")),
        "centered saved-memory card should include shared left padding: {rendered:?}"
    );
    assert_eq!(
        lines[0].alignment,
        Some(ratatui::layout::Alignment::Left),
        "centered saved-memory card should be left-aligned after padding"
    );

    crate::tui::markdown::set_center_code_blocks(saved);
}

#[test]
fn render_tool_message_shows_swarm_spawn_prompt_summary() {
    let msg = DisplayMessage {
        role: "tool".to_string(),
        content: "spawned".to_string(),
        tool_calls: Vec::new(),
        duration_secs: None,
        title: None,
        tool_data: Some(crate::message::ToolCall {
            id: "call_swarm_spawn".to_string(),
            name: "swarm".to_string(),
            input: serde_json::json!({
                "action": "spawn",
                "prompt": "Extract the restart command cluster from cli commands and validate it"
            }),
            intent: None,
        }),
    };

    let lines = render_tool_message(&msg, 120, crate::config::DiffDisplayMode::Off);
    let rendered: String = lines[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();

    assert!(rendered.contains("swarm spawn"), "rendered={rendered}");
    assert!(
        rendered.contains("Extract the restart command cluster"),
        "rendered={rendered}"
    );
}

#[test]
fn render_tool_message_batch_subcall_shows_swarm_dm_details() {
    let msg = DisplayMessage {
        role: "tool".to_string(),
        content: "--- [1] swarm ---\nDone\n\nCompleted: 1 succeeded, 0 failed".to_string(),
        tool_calls: Vec::new(),
        duration_secs: None,
        title: None,
        tool_data: Some(crate::message::ToolCall {
            id: "call_batch_swarm".to_string(),
            name: "batch".to_string(),
            input: serde_json::json!({
                "tool_calls": [
                    {
                        "tool": "swarm",
                        "action": "dm",
                        "to_session": "shark",
                        "message": "Please validate the restart extraction and report back"
                    }
                ]
            }),
            intent: None,
        }),
    };

    let lines = render_tool_message(&msg, 120, crate::config::DiffDisplayMode::Off);
    let rendered = lines
        .iter()
        .map(extract_line_text)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("swarm dm → shark"), "rendered={rendered}");
    assert!(
        rendered.contains("Please validate the restart"),
        "rendered={rendered}"
    );
}
