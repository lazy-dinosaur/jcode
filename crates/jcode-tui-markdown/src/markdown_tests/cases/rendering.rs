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
