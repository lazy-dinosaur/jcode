#[test]
fn test_simple_markdown() {
    let lines = render_markdown("Hello **world**");
    assert!(!lines.is_empty());
}

#[test]
fn test_code_block() {
    let lines = render_markdown("```rust\nfn main() {}\n```");
    assert!(!lines.is_empty());
}

#[test]
fn test_paragraph_to_ordered_list_starts_on_new_line_without_blank_source_line() {
    let md = "정리하면:\n1. **아이폰**에서는 새 서비스워커 적용 여부를 확인\n2. **안드로이드**도 badge API 결과를 확인";
    let rendered: Vec<String> = render_markdown_with_width(md, Some(96))
        .iter()
        .map(line_to_string)
        .collect();

    assert_eq!(rendered.first().map(String::as_str), Some("정리하면:"));
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("1. 아이폰")),
        "ordered list item should render on its own line: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("2. 안드로이드")),
        "second ordered list item should render on its own line: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .all(|line| !line.contains("정리하면: 1.")),
        "paragraph and ordered list marker must not be glued: {rendered:?}"
    );
}

#[test]
fn test_glued_ordered_list_marker_after_sentence_is_repaired() {
    let md = "다음은 이 순서가 제일 좋아요.1. **실제 긴 세션 런타임 검증** - 화면 확인.2. **scroll/input 밀림 전용 진단 추가** - geometry invariant.";
    let rendered: Vec<String> = render_markdown_with_width(md, Some(160))
        .iter()
        .map(line_to_string)
        .collect();

    assert_eq!(
        rendered.first().map(String::as_str),
        Some("다음은 이 순서가 제일 좋아요.")
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("1. 실제 긴 세션 런타임 검증")),
        "first glued ordered item should render on its own line: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("2. scroll/input 밀림 전용 진단 추가")),
        "second glued ordered item should render on its own line: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .all(|line| !line.contains("좋아요.1.") && !line.contains("확인.2.")),
        "ordered markers must not remain glued to prose: {rendered:?}"
    );
}

#[test]
fn test_glued_bullet_marker_after_sentence_is_repaired() {
    let md = "남은 작업은 이거예요. - body/input/status 영역 진단. • RAM cache cap 확인.";
    let rendered: Vec<String> = render_markdown_with_width(md, Some(140))
        .iter()
        .map(line_to_string)
        .collect();

    assert_eq!(
        rendered.first().map(String::as_str),
        Some("남은 작업은 이거예요.")
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("• body/input/status 영역 진단")),
        "hyphen bullet glued to prose should render as a bullet line: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("• RAM cache cap 확인")),
        "unicode bullet glued to prose should render as a bullet line: {rendered:?}"
    );
}

#[test]
fn test_glued_alphabetic_option_markers_after_choice_label_are_repaired() {
    let md = "선택지: A. 내가 중복 함수 하나만 제거 (Recommended) B. 네가 직접 getRecipientChatBadge 중복 블록 하나 삭제 C. 우선 그대로 둠";
    let rendered: Vec<String> = render_markdown_with_width(md, Some(160))
        .iter()
        .map(line_to_string)
        .collect();

    assert_eq!(rendered.first().map(String::as_str), Some("선택지:"));
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("A. 내가 중복 함수")),
        "A option should render on its own line: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("B. 네가 직접")),
        "B option should render on its own line: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("C. 우선 그대로")),
        "C option should render on its own line: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .all(|line| !line.contains("선택지: A.") && !line.contains("Recommended) B.")),
        "alphabetic options must not remain glued to prose: {rendered:?}"
    );
}

#[test]
fn test_alpha_option_marker_glued_directly_after_colon_is_repaired() {
    let md = "다음은 선택지입니다:A. Track B Phase 3 opt-in response.completed replacement plan/patch 작성 (Recommended) B. Track A workflow 설계 C. dogfood 관찰";
    let rendered: Vec<String> = render_markdown_with_width(md, Some(180))
        .iter()
        .map(line_to_string)
        .collect();

    assert_eq!(rendered.first().map(String::as_str), Some("다음은 선택지입니다:"));
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("A. Track B Phase 3")),
        "A option should render on its own line after a no-space colon: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("B. Track A workflow")),
        "B option should render on its own line: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .all(|line| !line.contains("선택지입니다:A.") && !line.contains("Recommended) B.")),
        "alphabetic options must not remain glued to prose: {rendered:?}"
    );
}

#[test]
fn test_alpha_option_markers_after_question_prompt_are_repaired() {
    let md = "어느 방향으로 갈까요? A. 큐 이미지 순서부터 고치기 B. A/B 줄바꿈부터 고치기 C. 둘 다 바로 고치기";
    let rendered: Vec<String> = render_markdown_with_width(md, Some(180))
        .iter()
        .map(line_to_string)
        .collect();

    assert_eq!(rendered.first().map(String::as_str), Some("어느 방향으로 갈까요?"));
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("A. 큐 이미지 순서")),
        "A option should render on its own line after a question prompt: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("B. A/B 줄바꿈")),
        "B option should render on its own line: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("C. 둘 다")),
        "C option should render on its own line: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .all(|line| !line.contains("갈까요? A.") && !line.contains("고치기 B.")),
        "alphabetic options must not remain glued to the question prompt: {rendered:?}"
    );
}

#[test]
fn test_alpha_option_words_in_plain_sentence_are_not_repaired() {
    let md = "We discussed plan A. Borrow time from B. Move the deadline if needed.";
    let rendered: Vec<String> = render_markdown_with_width(md, Some(180))
        .iter()
        .map(line_to_string)
        .collect();

    assert_eq!(rendered.len(), 1, "plain sentence should stay one line: {rendered:?}");
    assert_eq!(
        rendered.first().map(String::as_str),
        Some("We discussed plan A. Borrow time from B. Move the deadline if needed.")
    );
}

#[test]
fn test_alpha_option_labels_with_recommended_and_colons_are_repaired() {
    let md = "이어서 갈까요? A (Recommended): 전체보기 전용 스크롤 클래스를 만든다 - padding-right 0, margin-right만 유지. B: 전체보기에서 콘텐츠 width를 동적으로 맞춘다. C: 직접 입력";
    let rendered: Vec<String> = render_markdown_with_width(md, Some(180))
        .iter()
        .map(line_to_string)
        .collect();

    assert_eq!(rendered.first().map(String::as_str), Some("이어서 갈까요?"));
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("A (Recommended): 전체보기")),
        "A recommended option should render on its own line: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("B: 전체보기에서")),
        "B colon option should render on its own line: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("C: 직접 입력")),
        "C colon option should render on its own line: {rendered:?}"
    );
    assert!(
        rendered.iter().all(|line| !line.contains("갈까요? A")
            && !line.contains("유지. B:")
            && !line.contains("맞춘다. C:")),
        "recommended/colon options must not remain glued to prose: {rendered:?}"
    );
}

#[test]
fn test_source_newlines_before_alphabetic_option_markers_are_preserved() {
    let md = "선택지:\nA. 내가 중복 함수 하나만 제거 (Recommended)\nB. 네가 직접 getRecipientChatBadge 중복 블록 하나 삭제\nC. 우선 그대로 둠";
    let rendered: Vec<String> = render_markdown_with_width(md, Some(160))
        .iter()
        .map(line_to_string)
        .collect();

    assert_eq!(rendered.first().map(String::as_str), Some("선택지:"));
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("A. 내가 중복 함수")),
        "A option should preserve source newline: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("B. 네가 직접")),
        "B option should preserve source newline: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .all(|line| !line.contains("선택지: A.") && !line.contains("Recommended) B.")),
        "source newlines before options must not be collapsed: {rendered:?}"
    );
}

#[test]
fn test_terminal_alpha_option_marker_before_recommended_line_is_repaired() {
    let md = "버튼 동작 선택해주세요.A.\n(Recommended) 첫 화면 유지 구현\n/forgot-password를 Figma 그대로 구현하고, 휴대폰 인증 버튼은 placeholder로 둡니다. reset flow는 record/backlog로 남깁니다.B. 기존 본인인증 mock 연결\n버튼 클릭 시 기존 startPhoneIdentityVerification()을 호출합니다.C. 재설정 폼까지 임시 구현";
    let rendered: Vec<String> = render_markdown_with_width(md, Some(220))
        .iter()
        .map(line_to_string)
        .collect();

    assert_eq!(
        rendered.first().map(String::as_str),
        Some("버튼 동작 선택해주세요.")
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("A. (Recommended) 첫 화면 유지")),
        "terminal A marker should attach to following recommended text: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("B. 기존 본인인증 mock 연결")),
        "B option after multiline A description should render on its own line: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("C. 재설정 폼까지")),
        "C option after multiline B description should render on its own line: {rendered:?}"
    );
    assert!(
        rendered.iter().all(|line| {
            !line.contains("선택해주세요.A.")
                && !line.contains("남깁니다.B.")
                && !line.contains("호출합니다.C.")
        }),
        "glued terminal/cross-line option markers must be repaired: {rendered:?}"
    );
}

#[test]
fn test_cross_line_alpha_option_mode_does_not_split_plain_plan_b_sentence() {
    let md = "버튼 동작 선택해주세요.A.\n첫 번째 선택지는 설명만 이어집니다\nWe can keep plan B. Move forward without adding a choice list.";
    let rendered: Vec<String> = render_markdown_with_width(md, Some(220))
        .iter()
        .map(line_to_string)
        .collect();

    assert!(
        rendered
            .iter()
            .any(|line| line.contains("We can keep plan B. Move forward")),
        "plain plan B sentence on a later source line should stay intact: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .all(|line| !line.trim_start().starts_with("B. Move forward")),
        "cross-line alpha mode should require a sentence/list boundary before B: {rendered:?}"
    );
}

#[test]
fn test_alpha_abbreviation_is_not_split_as_option_marker() {
    let md = "참고 문장. U.S.A. 표기는 그대로 둡니다.";
    let rendered: Vec<String> = render_markdown_with_width(md, Some(120))
        .iter()
        .map(line_to_string)
        .collect();

    assert!(
        rendered.iter().any(|line| line.contains("U.S.A. 표기")),
        "abbreviations without choice context should not be split: {rendered:?}"
    );
}

#[test]
fn test_ordered_list_item_continuation_line_preserves_visible_break() {
    let md = "5. **poll 메시지도 push 발송 유지**\npoll도 unread에 포함되므로 push가 나가야 합니다.";
    let rendered: Vec<String> = render_markdown_with_width(md, Some(120))
        .iter()
        .map(line_to_string)
        .collect();

    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("5. poll 메시지도 push 발송 유지")),
        "list item title should render on its own line: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("poll도 unread에 포함")),
        "list item continuation should preserve the source line break: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .all(|line| !line.contains("유지 poll도") && !line.contains("유지poll도")),
        "list item title and continuation must not be glued: {rendered:?}"
    );
}

#[test]
fn test_nested_bullets_do_not_glue_to_next_numbered_item() {
    let md = "1. **실제 긴 세션에서 체감 확인**\n   - 스크롤 밀림\n   - input 밀림\n   - markdown 재깨짐 여부 - TPS/frame latency\n2. **cache 효과 계측**\n   - body cache hit/miss\n   - incremental reuse 비율\n   - markdown render 시간이 아직 큰지\n3. **RAM cap 조정**";
    let rendered: Vec<String> = render_markdown_with_width(md, Some(180))
        .iter()
        .map(line_to_string)
        .collect();

    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("1. 실제 긴 세션에서 체감 확인")),
        "first numbered item should render: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line
                .trim_start()
                .starts_with("• markdown 재깨짐 여부 - TPS/frame latency")),
        "last nested bullet should render separately: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("2. cache 효과 계측")),
        "second numbered item should not glue to previous bullet: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("3. RAM cap 조정")),
        "third numbered item should not glue to previous bullet: {rendered:?}"
    );
    assert!(
        rendered.iter().all(|line| {
            !line.contains("latency2.")
                && !line.contains("큰지3.")
                && !line.contains("추가4.")
        }),
        "numbered markers must not be glued after nested bullets: {rendered:?}"
    );
}

#[test]
fn test_nested_status_summary_bullets_do_not_render_inline() {
    let md = "1. **Markdown/list/option 렌더링 문제들**\n   - glued markdown list marker 복구\n   - markdown list continuation 줄바꿈 보존\n   - A., B. 선택지 앞 줄바꿈 보존\n   - 다음은 선택지입니다:A.처럼 콜론 뒤에 바로 붙은 A. 분리\n   - markdown table boundary 보존\n   - CJK wrap에서 한 글자/토큰이 이상하게 고아처럼 남는 문제\n\n2. **큐/백그라운드 prompt 문제**\n   - tool backgrounding 후 queued prompt가 dispatch 안 되던 문제\n   - queued prompt에 붙은 이미지가 전송에서 빠지던 문제\n   - reload/recovery/remote follow-up에서도 queued image metadata가 안 어긋나게 수정\n\n3. **Claude/OpenAI provider 설정 문제**\n   - Claude context / reasoning effort 분리\n   - Claude reasoning effort 복구";
    let rendered: Vec<String> = render_markdown_with_width(md, Some(96))
        .iter()
        .map(line_to_string)
        .collect();

    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("1. Markdown/list/option 렌더링 문제들")),
        "first numbered heading should render: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("• glued markdown list marker 복구")),
        "nested bullet should render on its own line: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("2. 큐/백그라운드 prompt 문제")),
        "second numbered heading should render separately: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .all(|line| !line.contains("문제들 -") && !line.contains("문제2.")),
        "nested bullet and next marker must not be glued into previous line: {rendered:?}"
    );
}

#[test]
fn test_paragraph_to_fenced_code_block_starts_on_new_line_without_blank_source_line() {
    let md = "로그 추가:\n```js\nconsole.log('badge')\n```";
    let rendered: Vec<String> = render_markdown_with_width(md, Some(96))
        .iter()
        .map(line_to_string)
        .collect();

    assert_eq!(rendered.first().map(String::as_str), Some("로그 추가:"));
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("┌─ js")),
        "fenced code block should render as a block: {rendered:?}"
    );
}

#[test]
fn test_glued_fenced_code_block_after_prose_is_repaired() {
    let md = "방향은 이거예요:```ts tag: chat-${messageId} ```그리고 data.roomId는 계속 유지.";
    let rendered: Vec<String> = render_markdown_with_width(md, Some(120))
        .iter()
        .map(line_to_string)
        .collect();

    assert_eq!(rendered.first().map(String::as_str), Some("방향은 이거예요:"));
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("┌─ ts")),
        "glued fence should become a TypeScript code block: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("tag: chat-${messageId}")),
        "same-line fence code should be moved into the code block body: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("그리고 data.roomId는 계속 유지.")),
        "text glued after the closing fence should render as prose: {rendered:?}"
    );
    assert!(
        rendered.iter().all(|line| !line.contains("이거예요:```")),
        "opening fence must not remain glued to prose: {rendered:?}"
    );
}

#[test]
fn test_multiple_glued_fenced_code_blocks_are_repaired() {
    let md = "방향은 이거예요:```ts tag: chat-${messageId} ```또는 방 정보도 남기고 싶으면:```ts tag: chat-room-${roomId}-message-${messageId} ```그리고 data는 유지.";
    let rendered: Vec<String> = render_markdown_with_width(md, Some(140))
        .iter()
        .map(line_to_string)
        .collect();

    let code_lines = rendered
        .iter()
        .filter(|line| line.contains("tag: chat-"))
        .count();
    assert_eq!(
        code_lines, 2,
        "both compact glued code snippets should render as code lines: {rendered:?}"
    );
    assert!(
        rendered.iter().any(|line| line.contains("또는 방 정보도")),
        "middle prose should not be swallowed by the first code block: {rendered:?}"
    );
    assert!(
        rendered.iter().any(|line| line.contains("그리고 data는 유지.")),
        "trailing prose should not be swallowed by the second code block: {rendered:?}"
    );
}

#[test]
fn test_extract_copy_targets_from_rendered_lines_for_code_block() {
    let lines = render_markdown("before\n\n```rust\nfn main() {}\nprintln!(\"hi\");\n```\n\nafter");
    let targets = extract_copy_targets_from_rendered_lines(&lines);

    assert_eq!(targets.len(), 1);
    let target = &targets[0];
    assert_eq!(
        target.kind,
        CopyTargetKind::CodeBlock {
            language: Some("rust".to_string())
        }
    );
    assert_eq!(target.content, "fn main() {}\nprintln!(\"hi\");");
    assert_eq!(target.start_raw_line, target.badge_raw_line);
    assert!(target.end_raw_line > target.start_raw_line);
}

#[test]
fn test_progress_bar() {
    let bar = progress_bar(0.5, 10);
    assert_eq!(bar.chars().count(), 10);
}

#[test]
fn test_table_render_basic() {
    let md = "| A | B |\n| - | - |\n| 1 | 2 |";
    let lines = render_markdown(md);
    let rendered: Vec<String> = lines.iter().map(line_to_string).collect();

    assert!(
        rendered
            .iter()
            .any(|l| l.contains('│') && l.contains('A') && l.contains('B'))
    );
    assert!(rendered.iter().any(|l| l.contains('─') && l.contains('┼')));
}

#[test]
fn test_wrapped_pipe_table_cell_continuation_keeps_following_rows() {
    let md = "확인하고 싶습니다. 어느 방향인가요?\n\n| 옵션 | 작업 |\n|---|---|\n| A (Recommended) | 일정 알림 토스트를 상단 중앙에 표시 (위치만 변경, 설정 기능 없이) |\n| B | 토스트 위치 선택 설정 기능 신설 (상단중앙/우상단/우하단 등)\n- 그 값 따름 |\n| C | Figma에 토스트 위치 명시가 있는지 다시 확인 (다른 노드/화면) |\n| D | 직접 알려주기 (위치/설정 범위) |\n\n어떻게 할까요?";
    let rendered: Vec<String> = render_markdown_with_width(md, Some(140))
        .iter()
        .map(line_to_string)
        .collect();

    assert!(
        rendered
            .iter()
            .any(|line| line.contains("A (Recommended)") && line.contains("일정 알림")),
        "A row should render in the table: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("B") && line.contains("그 값 따름")),
        "B continuation should stay inside the B table row: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("C") && line.contains("Figma")),
        "C row should remain part of the table: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("D") && line.contains("직접 알려주기")),
        "D row should remain part of the table: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .all(|line| !line.trim_start().starts_with("• 그 값 따름")
                && !line.trim_start().starts_with("| C |")
                && !line.trim_start().starts_with("| D |")),
        "wrapped table continuation must not leak as a bullet/raw pipe rows: {rendered:?}"
    );
}

#[test]
fn test_paragraph_to_pipe_table_preserves_visible_boundary() {
    let md = "응, 이해했어. 정책은 이렇게 정리하면 맞지?\n| 방 종류 | 예시 | 이름 변경 방식 |\n| --- | --- | --- |\n| 전체/시스템성 부서방 | 전체 채팅 | 개인화 alias |\n핵심 구현 방향";
    let rendered: Vec<String> = render_markdown_with_width(md, Some(180))
        .iter()
        .map(line_to_string)
        .collect();

    assert_eq!(
        rendered.first().map(String::as_str),
        Some("응, 이해했어. 정책은 이렇게 정리하면 맞지?")
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("방 종류") && line.contains("예시")),
        "table header should render separately from preceding prose: {rendered:?}"
    );
    assert!(
        rendered.iter().any(|line| line.contains("핵심 구현 방향")),
        "text after table should render separately: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .all(|line| !line.contains("맞지?|") && !line.contains("alias핵심")),
        "table rows must not be glued to surrounding prose: {rendered:?}"
    );
}

#[test]
fn test_glued_prose_pipe_table_header_before_separator_is_repaired() {
    let md = concat!(
        "캘린더 관련 알림은 휴가 신청 말고 현재 이렇게 있어. 상황 | 누구에게 | 알림/액션 |\n",
        "|---|---|---|\n",
        "| 회의 일정 생성 | 참석자로 지정된 직원, 생성자 제외 | 즉시 회의 일정 안내 알림 |\n",
        "| 회의 참석자 추가 | 추가된 직원, 수정자 제외 | 즉시 회의 참석자 추가 알림 |\n",
        "| 휴가 승인 | 휴가 신청자 | 즉시 휴가 승인 알림 |"
    );
    let rendered: Vec<String> = render_markdown_with_width(md, Some(180))
        .iter()
        .map(line_to_string)
        .collect();

    assert_eq!(
        rendered.first().map(String::as_str),
        Some("캘린더 관련 알림은 휴가 신청 말고 현재 이렇게 있어.")
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("상황") && line.contains("누구에게") && line.contains('│')),
        "glued table header should render as a table header: {rendered:?}"
    );
    assert!(
        rendered.iter().any(|line| line.contains("휴가 승인") && line.contains("휴가 신청자")),
        "table body rows should remain in the table: {rendered:?}"
    );
    assert!(
        rendered.iter().all(|line| {
            !line.contains("있어. 상황 |")
                && !line.trim_start().starts_with("|---|")
                && !line.trim_start().starts_with("| 회의")
        }),
        "table source must not leak raw pipe rows or stay glued to prose: {rendered:?}"
    );

    let lazy_rendered: Vec<String> = render_markdown_lazy(md, Some(180), 0..usize::MAX)
        .iter()
        .map(line_to_string)
        .collect();
    assert!(
        lazy_rendered
            .iter()
            .any(|line| line.contains("상황") && line.contains("누구에게") && line.contains('│')),
        "lazy renderer should apply the same glued table header repair: {lazy_rendered:?}"
    );
}

#[test]
fn test_colon_directly_glued_to_pipe_table_header_is_repaired() {
    let md = concat!(
        "현재 구현 기준:| 대상 | 액션카드 | 알림 |\n",
        "|---|---|---|\n",
        "| 회의 참석자료 선택된 전원 | 회의록 작성하기 액션 추가됨 | 액션 알림 대상 |\n",
        "| 참석자가 아닌 사람 | 없음 | 알림 없음 |"
    );
    let rendered: Vec<String> = render_markdown_with_width(md, Some(160))
        .iter()
        .map(line_to_string)
        .collect();

    assert_eq!(
        rendered.first().map(String::as_str),
        Some("현재 구현 기준:")
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("대상") && line.contains("액션카드") && line.contains('│')),
        "colon-glued header should render as a table header: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("회의 참석자료 선택된 전원") && line.contains("회의록 작성하기")),
        "body rows should render as table rows: {rendered:?}"
    );
    assert!(
        rendered.iter().all(|line| {
            !line.contains("기준:|")
                && !line.trim_start().starts_with("|---|")
                && !line.trim_start().starts_with("| 회의")
        }),
        "raw pipe table source should not leak: {rendered:?}"
    );
}

#[test]
fn test_bold_colon_directly_glued_to_pipe_table_header_is_repaired() {
    let md = concat!(
        "**최종 시각 검증 (electron-test MCP, dashboard 창):**| 항목 | Figma 6594:35407 | 실제 앱 | 일치 |\n",
        "|---|---|---|---|\n",
        "| 제목 | 신청 현황 | 신청 현황 | ✅ |\n",
        "| 컬럼 6개 | 신청일/종류/휴가기간/대체인원/사유/처리상태 | 신청일/종류/휴가기간/대체 인원/사유/처리 상태 | ✅ |"
    );

    let rendered: Vec<String> = render_markdown_with_width(md, Some(120))
        .iter()
        .map(line_to_string)
        .collect();

    assert_eq!(
        rendered.first().map(String::as_str),
        Some("최종 시각 검증 (electron-test MCP, dashboard 창):")
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("항목") && line.contains("Figma 6594:35407") && line.contains('│')),
        "bold-colon-glued header should render as a table header: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("컬럼 6개") && line.contains("신청일/종류")),
        "body rows should render as table rows: {rendered:?}"
    );
    assert!(
        rendered.iter().all(|line| {
            !line.contains("창):| 항목")
                && !line.contains("**|")
                && !line.trim_start().starts_with("|---|")
        }),
        "raw pipe table source should not leak after bold text: {rendered:?}"
    );

    let lazy_rendered: Vec<String> = render_markdown_lazy(md, Some(120), 0..usize::MAX)
        .iter()
        .map(line_to_string)
        .collect();
    assert!(
        lazy_rendered
            .iter()
            .any(|line| line.contains("항목") && line.contains("Figma 6594:35407") && line.contains('│')),
        "lazy renderer should apply the same bold-glued table header repair: {lazy_rendered:?}"
    );
    assert!(
        lazy_rendered.iter().all(|line| {
            !line.contains("창):| 항목")
                && !line.contains("**|")
                && !line.trim_start().starts_with("|---|")
        }),
        "lazy renderer should not leak raw pipe table source after bold text: {lazy_rendered:?}"
    );
}

#[test]
fn test_bold_first_cell_after_colon_boundary_keeps_opening_marker_with_table() {
    let md = concat!(
        "현재 구현 기준: **대상** | 액션카드 | 알림 |\n",
        "|---|---|---|\n",
        "| 회의 참석자료 선택된 전원 | 회의록 작성하기 액션 추가됨 | 액션 알림 대상 |"
    );

    let rendered: Vec<String> = render_markdown_with_width(md, Some(160))
        .iter()
        .map(line_to_string)
        .collect();

    assert_eq!(
        rendered.first().map(String::as_str),
        Some("현재 구현 기준:")
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("대상") && line.contains("액션카드") && line.contains('│')),
        "bold first cell should stay attached to the repaired table: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .all(|line| !line.contains("기준: **") && !line.contains("**대상")),
        "opening bold marker must not be left in the prose prefix: {rendered:?}"
    );
}

#[test]
fn test_heading_glued_to_pipe_table_and_following_heading_are_repaired() {
    let md = concat!(
        "## 현재 완성도 요약| 영역 | 완성도 | 상태 |\n",
        "|---|---|---|\n",
        "| 기본 철학 / record-first memory | 높음 | .lazy-harness가 canonical memory라는 방향은 확립됨 |\n",
        "| Product UX / dashboard | 낮음~중간 | 강력하지만 아직 사용 흐름이 무겁고 command가 많음 |## 지금 잘 된 부분\n",
        "- cli-tool-boundary.md 추가\n"
    );

    let rendered: Vec<String> = render_markdown_with_width(md, Some(160))
        .iter()
        .map(line_to_string)
        .collect();
    let joined = rendered.join("\n");

    let summary_heading = rendered
        .iter()
        .find(|line| line.contains("현재 완성도 요약"))
        .unwrap_or_else(|| panic!("missing repaired summary heading in {rendered:?}"));
    assert!(
        !summary_heading.contains("| 영역") && !summary_heading.contains("##"),
        "heading must not stay glued to the table header: {rendered:?}"
    );
    assert!(
        joined.contains("영역") && joined.contains("완성도") && joined.contains('│'),
        "glued table header should render as a table: {rendered:?}"
    );
    assert!(
        joined.contains("Product UX / dashboard") && joined.contains("command가 많음"),
        "last table row should remain in the table: {rendered:?}"
    );

    let next_heading = rendered
        .iter()
        .find(|line| line.contains("지금 잘 된 부분"))
        .unwrap_or_else(|| panic!("missing repaired following heading in {rendered:?}"));
    assert!(
        !next_heading.contains("| Product UX") && !next_heading.contains("##"),
        "following heading must not stay glued to the final table row: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("• cli-tool-boundary.md 추가")),
        "bullet following repaired heading should render as a list item: {rendered:?}"
    );
    assert!(
        rendered.iter().all(|line| {
            !line.contains("요약| 영역")
                && !line.contains("|---|---|---|")
                && !line.contains("많음 |##")
        }),
        "raw malformed table/heading source should not leak: {rendered:?}"
    );
}

#[test]
fn test_pipe_table_cell_node_id_is_not_split_as_ordered_list_marker() {
    let md = concat!(
        "Gate 1(metadata) + 현재 코드를 확보했습니다. 차이가 큽니다. metadata로 파악한 새 디자인:\n\n",
        "| 항목 | 현재 코드 | 새 Figma (6694:34186) |\n",
        "|---|---|---|\n",
        "| 헤더 타이틀 | \"휴가 현황\" + airplane | **\"휴가 신청 목록\"** + flight_takeoff 아이콘 |\n",
    );
    let rendered: Vec<String> = render_markdown_with_width(md, Some(96))
        .iter()
        .map(line_to_string)
        .collect();
    let joined = rendered.join("\n");

    assert!(
        joined.contains("새 Figma") && joined.contains('│'),
        "node id table should render as a table: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .all(|line| !line.trim_start().starts_with("34186") && !line.contains("|---|")),
        "node id inside a pipe row must not be split into a stray rendered line: {rendered:?}"
    );
}

#[test]
fn test_ordered_list_code_fences_then_table_do_not_render_as_raw_markdown() {
    let md = concat!(
        "찾아보니 구분이 필요합니다.\n\n",
        "1. **OpenAI 공식 API의 reasoning**\n",
        "   - `adaptive`라는 파라미터는 없습니다.\n",
        "   - 설정값은 보통:\n",
        "     ```text\n",
        "     none, minimal, low, medium, high, xhigh\n",
        "     ```\n",
        "   - 즉 OpenAI는:\n",
        "     ```json\n",
        "     {\n",
        "       \"reasoning\": { \"effort\": \"high\" }\n",
        "     }\n",
        "     ```\n",
        "     이런 식이지, Claude처럼:\n",
        "     ```json\n",
        "     {\n",
        "       \"thinking\": { \"type\": \"adaptive\" }\n",
        "     }\n",
        "     ```\n\n",
        "2. **OpenAI 공식 API의 auto model routing**\n",
        "   - 공식 OpenAI API에서 `model: \"auto\"` 같은 자동 모델 라우터는 못 찾았습니다.\n\n",
        "정리하면:\n\n",
        "| 구분 | OpenAI 공식 API | Microsoft Foundry | OpenRouter |\n",
        "|---|---:|---:|---:|\n",
        "| reasoning effort | 있음 | 있음 | 있음 |\n",
        "| adaptive thinking 파라미터 | 없음 | 없음/모델별 | 없음/모델별 |\n",
        "| 모델 자동 라우팅 | 공식 OpenAI API에는 확인 안 됨 | `model-router` 있음 | `openrouter/auto` 있음 |\n"
    );

    for width in [180, 96, 72, 48] {
        let rendered: Vec<String> = render_markdown_with_width(md, Some(width))
            .iter()
            .map(line_to_string)
            .collect();
        assert_rendered_response_shape(&rendered, width);
    }

    let lazy_rendered: Vec<String> = render_markdown_lazy(md, Some(96), 0..usize::MAX)
        .iter()
        .map(line_to_string)
        .collect();
    assert_rendered_response_shape(&lazy_rendered, 96);
}

fn assert_rendered_response_shape(rendered: &[String], width: usize) {
    let joined = rendered.join("\n");

    assert!(
        joined.contains("┌─ json") && joined.contains("reasoning"),
        "json fence should render as a code block at width {width}: {rendered:?}"
    );
    assert!(
        joined.contains("구분") && joined.contains('│') && joined.contains("OpenAI 공식 API"),
        "pipe table should render as a table, not raw markdown at width {width}: {rendered:?}"
    );
    assert!(
        rendered.iter().all(|line| !line.contains("```")),
        "fence markers must not leak as raw markdown at width {width}: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .all(|line| !line.contains("정리하면:|") && !line.contains("|---|")),
        "table source must not glue to prose or leak raw separator at width {width}: {rendered:?}"
    );
}

#[test]
fn test_nonstandard_pipe_table_rows_preserve_source_newlines() {
    let md = "정리하면 맞지?\n| 방 종류 | 예시 | 이름 변경 방식 |\n|—|—|—|\n| 일반 채팅 | 1:1 방 | 개인화 alias |\n핵심 구현 방향";
    let rendered: Vec<String> = render_markdown_with_width(md, Some(180))
        .iter()
        .map(line_to_string)
        .collect();

    assert_eq!(rendered.first().map(String::as_str), Some("정리하면 맞지?"));
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("| 방 종류 |")),
        "nonstandard table header row should keep its own line: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("|—|—|—|")),
        "nonstandard separator row should keep its own line: {rendered:?}"
    );
    assert!(
        rendered.iter().all(|line| {
            !line.contains("맞지?| 방") && !line.contains("|—|—|—||") && !line.contains("alias핵심")
        }),
        "pipe rows must preserve source newlines even when not parsed as a table: {rendered:?}"
    );
}

#[test]
fn test_single_cell_separator_row_is_expanded_to_header_columns() {
    let md = concat!(
        "• 병원별 저장값(HospitalHolidayWorkSetting)을 날짜로 merge. 두 종류 공휴일 (isCustom 으로 구분)\n",
        "| 법정공휴일 (isCustom=false) | 사용자 추가 (isCustom=true) |\n",
        "|---|\n",
        "| 의미 | 공휴일인데 일하려는 날 | 공휴일에 못 쉬어서 따로 쉬는 대체휴무 |\n",
        "| 삭제 | 불가 (버튼 없음) | 가능 (soft delete) |\n",
        "| 색 | 빨강 #FF2642 | 검정 #111111 |"
    );

    let rendered: Vec<String> = render_markdown_with_width(md, Some(160))
        .iter()
        .map(line_to_string)
        .collect();
    let joined = rendered.join("\n");

    assert!(
        joined.contains("법정공휴일") && joined.contains("사용자 추가") && joined.contains('│'),
        "single-cell separator should be normalized and rendered as a table: {rendered:?}"
    );
    assert!(
        joined.contains("의미") && joined.contains("대체휴무") && joined.contains('│'),
        "table body rows should render after separator normalization: {rendered:?}"
    );
    assert!(
        rendered.iter().all(|line| {
            !line.trim_start().starts_with("|---|") && !line.contains("| 법정공휴일")
        }),
        "raw malformed pipe table source should not leak: {rendered:?}"
    );

    let lazy_rendered: Vec<String> = render_markdown_lazy(md, Some(160), 0..usize::MAX)
        .iter()
        .map(line_to_string)
        .collect();
    let lazy_joined = lazy_rendered.join("\n");
    assert!(
        lazy_joined.contains("법정공휴일") && lazy_joined.contains("사용자 추가") && lazy_joined.contains('│'),
        "lazy renderer should normalize single-cell separators too: {lazy_rendered:?}"
    );
    assert!(
        lazy_rendered.iter().all(|line| {
            !line.trim_start().starts_with("|---|") && !line.contains("| 법정공휴일")
        }),
        "lazy renderer should not leak raw malformed pipe table source: {lazy_rendered:?}"
    );
}

#[test]
fn test_glued_pipe_table_header_with_single_cell_separator_is_repaired() {
    let md = concat!(
        "핵심은 이렇게 정리됩니다:| 법정공휴일 | 사용자 추가 |\n",
        "|---|\n",
        "| 의미 | 공휴일인데 일하려는 날 | 공휴일에 못 쉬어서 따로 쉬는 대체휴무 |"
    );

    let rendered: Vec<String> = render_markdown_with_width(md, Some(160))
        .iter()
        .map(line_to_string)
        .collect();
    let joined = rendered.join("\n");

    assert_eq!(
        rendered.first().map(String::as_str),
        Some("핵심은 이렇게 정리됩니다:")
    );
    assert!(
        joined.contains("법정공휴일") && joined.contains("사용자 추가") && joined.contains('│'),
        "glued header should render as a table even with a single-cell separator: {rendered:?}"
    );
    assert!(
        rendered.iter().all(|line| {
            !line.contains("정리됩니다:|") && !line.trim_start().starts_with("|---|")
        }),
        "raw glued malformed table source should not leak: {rendered:?}"
    );
}

#[test]
fn test_table_width_truncation() {
    let md = "| Column | Value |\n| - | - |\n| very_long_cell_value | 1234567890 |";
    let lines = render_markdown_with_width(md, Some(20));
    let rendered: Vec<String> = lines.iter().map(line_to_string).collect();

    assert!(rendered.iter().any(|l| l.contains('…')));
    let max_len = rendered
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0);
    assert!(max_len <= 20);
}

#[test]
fn test_table_width_truncation_with_three_columns_stays_within_limit() {
    let md =
        "| # | Principle | Story Ready |\n| - | - | - |\n| 1 | Customer Obsession | unchecked |";
    let lines = render_markdown_with_width(md, Some(24));
    let rendered: Vec<String> = lines.iter().map(line_to_string).collect();

    assert!(
        rendered.iter().any(|line| line.contains("─┼─")),
        "expected table separator line: {:?}",
        rendered
    );

    let max_width = rendered.iter().map(|line| line.width()).max().unwrap_or(0);
    assert!(
        max_width <= 24,
        "expected all rendered table lines to fit width 24, got {} in {:?}",
        max_width,
        rendered
    );
}

#[test]
fn test_table_cjk_alignment() {
    let md = "| Issue | You wrote |\n| - | - |\n| 政策 pronunciation | zhēn cí |";
    let lines = render_markdown(md);
    let rendered: Vec<String> = lines.iter().map(line_to_string).collect();

    let non_empty: Vec<&String> = rendered.iter().filter(|l| !l.is_empty()).collect();
    assert!(
        non_empty.len() >= 3,
        "Expected at least 3 non-empty lines, got {}: {:?}",
        non_empty.len(),
        non_empty
    );

    let header = non_empty[0];
    let separator = non_empty[1];
    let data_row = non_empty[2];

    let header_width = UnicodeWidthStr::width(header.as_str());
    let sep_width = UnicodeWidthStr::width(separator.as_str());
    let data_width = UnicodeWidthStr::width(data_row.as_str());

    assert_eq!(
        header_width, sep_width,
        "Header and separator should have same display width: header='{}' ({}) sep='{}' ({})",
        header, header_width, separator, sep_width
    );
    assert_eq!(
        header_width, data_width,
        "Header and data row should have same display width: header='{}' ({}) data='{}' ({})",
        header, header_width, data_row, data_width
    );
}

#[test]
fn test_mermaid_block_detection() {
    // Mermaid rendering is temporarily disabled by default, so Mermaid fences
    // should safely fall back to normal code blocks unless explicitly opted in.
    let md = "```mermaid\nflowchart LR\n    A --> B\n```";
    let lines = render_markdown(md);
    let text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
        .collect();

    assert!(text.contains("mermaid"), "Expected code block header: {text}");
    assert!(text.contains("flowchart LR"), "Expected raw Mermaid source: {text}");
}

#[test]
fn test_mixed_code_and_mermaid() {
    // Mixed content should render both correctly
    let md = "```rust\nfn main() {}\n```\n\n```mermaid\nflowchart TD\n    A\n```\n\n```python\nprint('hi')\n```";
    let lines = render_markdown(md);

    // Should have output for all blocks
    assert!(
        lines.len() >= 3,
        "Expected multiple lines for mixed content"
    );
}

#[test]
fn test_inline_math_render() {
    let lines = render_markdown("Area is $a^2$.");
    let rendered = lines_to_string(&lines);
    assert!(rendered.contains("$a^2$"));
}

#[test]
fn test_display_math_render() {
    let lines = render_markdown("$$\nE = mc^2\n$$");
    let rendered = lines_to_string(&lines);
    assert!(rendered.contains("┌─ math"));
    assert!(rendered.contains("E = mc^2"));
    assert!(rendered.contains("└─"));
}

#[test]
fn test_link_strike_and_image_render() {
    let md = "This is ~~old~~ and [docs](https://example.com).\n\n![chart](https://img.example/chart.png)";
    let lines = render_markdown(md);
    let rendered = lines_to_string(&lines);
    assert!(rendered.contains("old"));
    assert!(rendered.contains("docs (https://example.com)"));
    assert!(rendered.contains("[image: chart] (https://img.example/chart.png)"));
}

#[test]
fn test_ordered_and_task_list_render() {
    let md = "1. first\n2. second\n\n- [x] done\n- [ ] todo";
    let lines = render_markdown(md);
    let rendered = lines_to_string(&lines);
    assert!(rendered.contains("1. first"));
    assert!(rendered.contains("2. second"));
    assert!(rendered.contains("[x] done"));
    assert!(rendered.contains("[ ] todo"));
}

#[test]
fn test_blockquote_footnote_and_definition_list_render() {
    let md = "> quote line\n\nRef[^a]\n\n[^a]: footnote body\n\nTerm\n  : definition text";
    let lines = render_markdown(md);
    let rendered = lines_to_string(&lines);
    assert!(rendered.contains("│ quote line"));
    assert!(rendered.contains("[^a]"));
    assert!(rendered.contains("[^a]: footnote body"));
    assert!(rendered.contains("Term"));
    assert!(rendered.contains("definition text"));
}

#[test]
fn test_glued_blockquote_marker_after_sentence_is_repaired() {
    let md = "즉 목표를 이렇게 바꾸면 좋겠습니다:> “lazy-harness는 느린 감시자가 아니라, 빠른 분류기 + 강제 실행 경계다.”\n이 방향으로 계획을 수정하면 됩니다.";
    let rendered: Vec<String> = render_markdown_with_width(md, Some(140))
        .iter()
        .map(line_to_string)
        .collect();

    assert_eq!(
        rendered.first().map(String::as_str),
        Some("즉 목표를 이렇게 바꾸면 좋겠습니다:")
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("│ “lazy-harness는")),
        "glued blockquote should render as its own quoted line: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .all(|line| !line.contains("좋겠습니다:>") && !line.contains(":> “lazy")),
        "blockquote marker must not remain glued to prose: {rendered:?}"
    );
}

#[test]
fn test_plain_paragraph_alignment_remains_unset() {
    let lines = render_markdown("plain paragraph");
    let line = lines
        .iter()
        .find(|line| line_to_string(line).contains("plain paragraph"))
        .expect("paragraph line");
    assert_eq!(line.alignment, None);
}

#[test]
fn test_structured_markdown_lines_force_left_alignment() {
    let md = concat!(
        "- [x] done\n",
        "1. numbered\n\n",
        "> quoted\n\n",
        "[^a]: footnote body\n\n",
        "Term\n  : definition text\n\n",
        "| A | B |\n| - | - |\n| 1 | 2 |\n\n",
        "$$\nE = mc^2\n$$\n\n",
        "---\n\n",
        "<div>html</div>"
    );

    let saved = center_code_blocks();
    set_center_code_blocks(true);
    let lines = render_markdown_with_width(md, Some(40));
    set_center_code_blocks(saved);

    let expected = [
        "• [x] done",
        "1. numbered",
        "│ quoted",
        "[^a]: footnote body",
        "• Term",
        "  -> definition text",
        "A │ B",
        "─┼─",
        "1 │ 2",
        "┌─ math",
        "│ E = mc^2",
        "└─",
        "────",
        "<div>html</div>",
    ];

    for snippet in expected {
        let line = lines
            .iter()
            .find(|line| line_to_string(line).contains(snippet))
            .unwrap_or_else(|| panic!("missing line containing '{snippet}' in {lines:?}"));
        assert_eq!(
            line.alignment,
            Some(Alignment::Left),
            "expected left alignment for line containing '{snippet}'"
        );
    }
}

#[test]
fn test_llm_plan_markdown_preserves_heading_list_boundaries() {
    let md = concat!(
        "## 2. 재현 시나리오를 4개로 고정\n",
        "같은 조건으로 반복 측정해야 합니다.\n\n",
        "1. **Opus 응답 streaming 중**\n",
        "   - 긴 답변 받을 때\n",
        "   - 스크롤 위/아래 반복\n",
        "   - input 타이핑\n\n",
        "### 3. 로그 분류 기준\n",
        "이제 로그에 필드가 있으니 slow frame 하나를 이렇게 판정합니다.\n\n",
        "- `body_misses > 0`\n",
        "  - chat body cache miss\n",
        "  - 큰 transcript 재준비 문제\n\n",
        "- `draw_messages_area_ms`가 큼\n",
        "  - visible viewport render 자체가 무거움\n"
    );

    let rendered: Vec<String> = render_markdown_with_width(md, Some(96))
        .iter()
        .map(line_to_string)
        .collect();

    let heading_two = rendered
        .iter()
        .find(|line| line.contains("2. 재현 시나리오"))
        .unwrap_or_else(|| panic!("missing h2 heading in {rendered:?}"));
    assert!(
        !heading_two.contains("같은 조건"),
        "h2 heading and following paragraph should not be joined: {rendered:?}"
    );
    assert!(
        rendered.iter().any(|line| line.trim() == "같은 조건으로 반복 측정해야 합니다."),
        "missing paragraph after heading: {rendered:?}"
    );

    let heading_three = rendered
        .iter()
        .find(|line| line.contains("3. 로그 분류 기준"))
        .unwrap_or_else(|| panic!("missing h3 heading in {rendered:?}"));
    assert!(
        !heading_three.contains("이제 로그"),
        "h3 heading and following paragraph should not be joined: {rendered:?}"
    );
    assert!(
        rendered.iter().all(|line| !line.contains("###")),
        "heading markers should not leak for valid headings: {rendered:?}"
    );
    assert!(
        rendered.iter().any(|line| line.contains("1. Opus 응답 streaming 중")),
        "ordered list item should render separately: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.trim_start().starts_with("• chat body cache miss")),
        "nested bullet should render as its own line: {rendered:?}"
    );
}

#[test]
fn test_llm_plan_markdown_repairs_heading_markers_glued_after_sentence() {
    let md = concat!(
        "하이라이트 지연은 따로 봐야 함### 3. 로그 분류 기준\n",
        "이제 로그에 필드가 있으니 slow frame 하나를 이렇게 판정합니다.\n",
        "- `body_misses > 0`\n",
        "chat body cache miss\n"
    );

    let rendered: Vec<String> = render_markdown_with_width(md, Some(96))
        .iter()
        .map(line_to_string)
        .collect();

    assert!(
        rendered
            .iter()
            .any(|line| line.trim() == "하이라이트 지연은 따로 봐야 함"),
        "prefix sentence should be kept on its own line: {rendered:?}"
    );
    let heading = rendered
        .iter()
        .find(|line| line.contains("3. 로그 분류 기준"))
        .unwrap_or_else(|| panic!("missing repaired heading in {rendered:?}"));
    assert!(
        !heading.contains("###") && !heading.contains("하이라이트"),
        "glued heading marker should be repaired into a real heading: {rendered:?}"
    );
}

#[test]
fn test_wrapped_left_aligned_list_items_stay_left_aligned() {
    let lines = render_markdown("- this is a long list item that should wrap");
    let wrapped = wrap_lines(lines, 12);

    let non_empty: Vec<&Line<'_>> = wrapped
        .iter()
        .filter(|line| !line.spans.is_empty())
        .collect();
    assert!(
        non_empty.len() >= 2,
        "expected wrapped list item: {wrapped:?}"
    );
    assert!(
        non_empty
            .iter()
            .all(|line| line.alignment == Some(Alignment::Left)),
        "expected wrapped list lines to preserve left alignment: {wrapped:?}"
    );
}

#[test]
fn test_wrapped_code_block_repeats_gutter_on_continuations() {
    let lines = render_markdown("```text\nalpha beta gamma delta\n```");
    let wrapped = wrap_lines(lines, 10);
    let rendered: Vec<String> = wrapped.iter().map(line_to_string).collect();

    assert_eq!(
        rendered,
        vec![
            "┌─ text ",
            "│ alpha ",
            "│ beta ",
            "│ gamma ",
            "│ delta",
            "└─",
        ]
    );
}

#[test]
fn test_wrapped_syntax_highlighted_code_block_keeps_all_body_lines_in_frame() {
    let lines = render_markdown("```rust\nlet alpha_beta_gamma = delta_epsilon_zeta();\n```");
    let wrapped = wrap_lines(lines, 18);
    let rendered: Vec<String> = wrapped.iter().map(line_to_string).collect();

    assert!(
        rendered
            .first()
            .is_some_and(|line| line.starts_with("┌─ rust ")),
        "expected code block header: {rendered:?}"
    );
    assert_eq!(rendered.last().map(String::as_str), Some("└─"));

    let body = &rendered[1..rendered.len() - 1];
    assert!(body.len() >= 2, "expected wrapped code body: {rendered:?}");
    assert!(
        body.iter().all(|line| line.starts_with("│ ")),
        "every wrapped code line should remain inside the code block frame: {rendered:?}"
    );

    let flattened = body
        .iter()
        .map(|line| line.trim_start_matches("│ "))
        .collect::<String>();
    assert!(
        flattened.contains("let alpha_beta_gamma = delta_epsilon_zeta();"),
        "wrapped code body should preserve code text order: {rendered:?}"
    );
}

#[test]
fn test_wrapped_text_code_block_with_long_token_keeps_gutter_on_continuations() {
    let lines = render_markdown(
        "```text\nui_viewport::render_native_scrollbar|viewport::render_native_scrollbar|render_native_scrollbar(\n```",
    );
    let wrapped = wrap_lines(lines, 24);
    let rendered: Vec<String> = wrapped.iter().map(line_to_string).collect();

    assert_eq!(rendered.first().map(String::as_str), Some("┌─ text "));
    assert_eq!(rendered.last().map(String::as_str), Some("└─"));

    let body = &rendered[1..rendered.len() - 1];
    assert!(body.len() >= 2, "expected wrapped code body: {rendered:?}");
    assert!(
        body.iter().all(|line| line.starts_with("│ ")),
        "every wrapped continuation should preserve the framed gutter: {rendered:?}"
    );
    let body_text = body
        .iter()
        .map(|line| line.trim_start_matches("│ "))
        .collect::<String>();
    assert!(
        body_text.contains("render_native_scrollbar"),
        "wrapped code body should preserve the long identifier: {rendered:?}"
    );
}

#[test]
fn test_centered_mode_keeps_list_markers_flush_left() {
    let md = concat!(
        "1. Create a goal\n",
        "   - title\n",
        "   - description / \"why this matters\"\n",
        "   - success criteria\n",
        "2. Break it down\n",
        "   - milestones\n",
        "   - steps\n"
    );

    let saved = center_code_blocks();
    set_center_code_blocks(true);
    let lines = render_markdown_with_width(md, Some(80));
    set_center_code_blocks(saved);

    let numbered_1 = lines
        .iter()
        .find(|line| line_to_string(line).contains("1. Create a goal"))
        .expect("numbered list item");
    let numbered_2 = lines
        .iter()
        .find(|line| line_to_string(line).contains("2. Break it down"))
        .expect("second numbered list item");
    let bullet = lines
        .iter()
        .find(|line| line_to_string(line).contains("description /"))
        .expect("nested bullet item");

    let numbered_1_text = line_to_string(numbered_1);
    let numbered_2_text = line_to_string(numbered_2);
    let bullet_text = line_to_string(bullet);

    let numbered_pad = leading_spaces(&numbered_1_text);
    let numbered_2_pad = leading_spaces(&numbered_2_text);
    let bullet_pad = leading_spaces(&bullet_text);

    assert!(
        numbered_pad > 0,
        "numbered list should be centered as a block: {lines:?}"
    );
    assert!(
        numbered_pad == numbered_2_pad,
        "numbered items should share the same block padding: {lines:?}"
    );
    assert!(
        bullet_pad > numbered_pad,
        "nested bullet should keep additional internal indent within the centered block: {lines:?}"
    );
    assert!(
        numbered_1_text[numbered_pad..].starts_with("1. Create a goal"),
        "number marker should stay left-aligned within centered block: {lines:?}"
    );
    assert!(
        bullet_text[bullet_pad..].starts_with("• description /"),
        "bullet marker should stay left-aligned within centered block: {lines:?}"
    );
}

#[test]
fn test_centered_mode_centers_other_structured_blocks_as_blocks() {
    let md = concat!(
        "> quoted line\n\n",
        "[^a]: footnote body\n\n",
        "Term\n  : definition text\n\n",
        "| A | B |\n| - | - |\n| 1 | 2 |\n"
    );

    let saved = center_code_blocks();
    set_center_code_blocks(true);
    let lines = render_markdown_with_width(md, Some(50));
    set_center_code_blocks(saved);

    for snippet in ["│ quoted line", "[^a]: footnote body", "• Term", "A │ B"] {
        let line = lines
            .iter()
            .find(|line| line_to_string(line).contains(snippet))
            .unwrap_or_else(|| panic!("missing '{snippet}' in {lines:?}"));
        let text = line_to_string(line);
        assert!(
            leading_spaces(&text) > 0,
            "structured block line should be centered as a block: {text:?} / {lines:?}"
        );
    }
}

#[test]
fn test_centered_mode_still_centers_framed_code_blocks() {
    let saved = center_code_blocks();
    set_center_code_blocks(true);
    let lines = render_markdown_with_width("```rust\nfn main() {}\n```", Some(40));
    set_center_code_blocks(saved);

    let header = lines
        .iter()
        .find(|line| line_to_string(line).contains("┌─ rust "))
        .expect("code block header");
    assert!(
        line_to_string(header).starts_with(' '),
        "framed code block should keep centered padding: {lines:?}"
    );
}

#[test]
fn test_rule_and_inline_html_render() {
    let md = "before\n\n---\n\ninline <span>html</span> tag";
    let lines = render_markdown(md);
    let rendered = lines_to_string(&lines);
    assert!(rendered.contains("────────────────"));
    assert!(rendered.contains("<span>"));
    assert!(rendered.contains("</span>"));
}

#[test]
fn test_centered_mode_centers_rules_as_blocks() {
    let saved = center_code_blocks();
    set_center_code_blocks(true);
    let lines = render_markdown_with_width("before\n\n---\n\nafter", Some(50));
    set_center_code_blocks(saved);

    let rule_line = lines
        .iter()
        .find(|line| line_to_string(line).contains("────"))
        .expect("rule line");
    let text = line_to_string(rule_line);
    assert!(
        leading_spaces(&text) > 0,
        "rule should be centered: {text:?}"
    );
    assert!(
        UnicodeWidthStr::width(text.trim()) <= RULE_LEN,
        "rule should not span full width: {text:?}"
    );
}

#[test]
fn test_centered_mode_keeps_lists_left_aligned() {
    let saved = center_code_blocks();
    set_center_code_blocks(true);
    let lines = render_markdown_with_width("- one\n- two", Some(50));
    set_center_code_blocks(saved);

    let rendered: Vec<String> = lines
        .iter()
        .map(line_to_string)
        .filter(|line| !line.is_empty())
        .collect();

    assert_eq!(
        rendered.len(),
        2,
        "expected rendered list items: {rendered:?}"
    );
    let first_pad = leading_spaces(&rendered[0]);
    let second_pad = leading_spaces(&rendered[1]);
    assert_eq!(
        first_pad, second_pad,
        "list items should share the same block pad: {rendered:?}"
    );
    assert!(
        first_pad > 0,
        "list block should be centered in centered mode: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .all(|line| line[first_pad..].starts_with("• "))
    );
}
