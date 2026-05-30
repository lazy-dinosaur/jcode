use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::prelude::*;
use serde::Serialize;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{LazyLock, Mutex};
use std::time::Instant;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style as SynStyle, ThemeSet};
use syntect::parsing::SyntaxSet;
use unicode_width::UnicodeWidthStr;

#[cfg(feature = "mermaid-renderer")]
use jcode_tui_mermaid as mermaid;

#[cfg(not(feature = "mermaid-renderer"))]
mod mermaid {
    use ratatui::prelude::*;

    #[allow(dead_code)]
    #[derive(Debug, Clone)]
    pub enum RenderResult {
        Image {
            hash: u64,
            path: std::path::PathBuf,
            width: u32,
            height: u32,
        },
        Error(String),
    }

    pub fn is_mermaid_lang(lang: &str) -> bool {
        lang.eq_ignore_ascii_case("mermaid") || lang.eq_ignore_ascii_case("mmd")
    }

    pub fn image_protocol_available() -> bool {
        false
    }

    pub fn render_mermaid_deferred_with_stream_scope(
        _content: &str,
        _terminal_width: Option<u16>,
        _stream_sequence: u64,
    ) -> Option<RenderResult> {
        Some(RenderResult::Error(
            "Mermaid rendering is disabled".to_string(),
        ))
    }

    pub fn render_mermaid_deferred_with_registration(
        _content: &str,
        _terminal_width: Option<u16>,
        _register_active: bool,
    ) -> Option<RenderResult> {
        Some(RenderResult::Error(
            "Mermaid rendering is disabled".to_string(),
        ))
    }

    pub fn render_mermaid_untracked(_content: &str, _terminal_width: Option<u16>) -> RenderResult {
        RenderResult::Error("Mermaid rendering is disabled".to_string())
    }

    pub fn render_mermaid_sized(_content: &str, _terminal_width: Option<u16>) -> RenderResult {
        RenderResult::Error("Mermaid rendering is disabled".to_string())
    }

    pub fn set_streaming_preview_diagram(
        _hash: u64,
        _width: u32,
        _height: u32,
        _label: Option<String>,
    ) {
    }

    pub fn result_to_lines(result: RenderResult, _max_width: Option<usize>) -> Vec<Line<'static>> {
        match result {
            RenderResult::Image { .. } => Vec::new(),
            RenderResult::Error(message) => vec![Line::from(message)],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
pub enum DiagramDisplayMode {
    #[default]
    None,
    Margin,
    Pinned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
pub enum MarkdownSpacingMode {
    #[default]
    Compact,
    Document,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopyTargetKind {
    CodeBlock { language: Option<String> },
    Error,
    ToolOutput,
}

impl CopyTargetKind {
    pub fn label(&self) -> String {
        match self {
            Self::CodeBlock { language } => language
                .as_deref()
                .filter(|lang| !lang.is_empty())
                .unwrap_or("code")
                .to_string(),
            Self::Error => "error".to_string(),
            Self::ToolOutput => "output".to_string(),
        }
    }

    pub fn copied_notice(&self) -> String {
        match self {
            Self::CodeBlock { language } => {
                let label = language
                    .as_deref()
                    .filter(|lang| !lang.is_empty())
                    .unwrap_or("code block");
                format!("Copied {}", label)
            }
            Self::Error => "Copied error".to_string(),
            Self::ToolOutput => "Copied output".to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RawCopyTarget {
    pub kind: CopyTargetKind,
    pub content: String,
    pub start_raw_line: usize,
    pub end_raw_line: usize,
    pub badge_raw_line: usize,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MarkdownConfigSnapshot {
    pub diagram_mode: DiagramDisplayMode,
    pub markdown_spacing: MarkdownSpacingMode,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessMemorySnapshot {
    pub rss_bytes: Option<u64>,
    pub peak_rss_bytes: Option<u64>,
    pub virtual_bytes: Option<u64>,
}

static CONFIG_SNAPSHOT_HOOK: LazyLock<Mutex<fn() -> MarkdownConfigSnapshot>> =
    LazyLock::new(|| Mutex::new(default_config_snapshot));
static MEMORY_SNAPSHOT_HOOK: LazyLock<Mutex<fn() -> ProcessMemorySnapshot>> =
    LazyLock::new(|| Mutex::new(default_memory_snapshot));

fn default_config_snapshot() -> MarkdownConfigSnapshot {
    MarkdownConfigSnapshot::default()
}

fn default_memory_snapshot() -> ProcessMemorySnapshot {
    ProcessMemorySnapshot::default()
}

pub fn set_config_snapshot_hook(hook: fn() -> MarkdownConfigSnapshot) {
    if let Ok(mut current) = CONFIG_SNAPSHOT_HOOK.lock() {
        *current = hook;
    }
}

pub fn set_memory_snapshot_hook(hook: fn() -> ProcessMemorySnapshot) {
    if let Ok(mut current) = MEMORY_SNAPSHOT_HOOK.lock() {
        *current = hook;
    }
}

pub(crate) fn config_snapshot() -> MarkdownConfigSnapshot {
    CONFIG_SNAPSHOT_HOOK
        .lock()
        .map(|hook| hook())
        .unwrap_or_default()
}

pub(crate) fn process_memory_snapshot() -> ProcessMemorySnapshot {
    MEMORY_SNAPSHOT_HOOK
        .lock()
        .map(|hook| hook())
        .unwrap_or_default()
}

#[path = "markdown_context.rs"]
mod context;
#[path = "markdown_wrap.rs"]
mod wrap;

#[cfg(test)]
pub(crate) use context::with_markdown_spacing_mode_override;
pub use context::{
    center_code_blocks, get_diagram_mode_override, set_center_code_blocks,
    set_diagram_mode_override, with_deferred_mermaid_render_context,
};
use context::{
    deferred_mermaid_render_context_enabled, effective_diagram_mode,
    effective_markdown_spacing_mode, streaming_render_context_enabled,
    with_streaming_render_context,
};

#[path = "markdown_render_full.rs"]
mod render_full;
#[path = "markdown_render_lazy.rs"]
mod render_lazy;
#[path = "markdown_render_support.rs"]
mod render_support;

pub use render_full::render_markdown_with_width;
pub use render_lazy::render_markdown_lazy;
pub use render_support::extract_copy_targets_from_rendered_lines;
use render_support::{
    highlight_code_cached, line_plain_text, placeholder_code_block, ranges_overlap, render_table,
};
pub use render_support::{highlight_file_lines, highlight_line, render_table_with_width};

// Syntax highlighting resources (loaded once)
static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

// Syntax highlighting cache - keyed by (code content hash, language)
static HIGHLIGHT_CACHE: LazyLock<Mutex<HighlightCache>> =
    LazyLock::new(|| Mutex::new(HighlightCache::new()));

const HIGHLIGHT_CACHE_LIMIT: usize = 256;

#[derive(Debug, Clone, Default, Serialize)]
pub struct MarkdownDebugStats {
    pub total_renders: u64,
    pub last_render_ms: Option<f32>,
    pub last_text_len: Option<usize>,
    pub last_lines: Option<usize>,
    pub last_headings: usize,
    pub last_code_blocks: usize,
    pub last_mermaid_blocks: usize,
    pub last_tables: usize,
    pub last_list_items: usize,
    pub last_blockquotes: usize,
    pub highlight_cache_hits: u64,
    pub highlight_cache_misses: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MarkdownMemoryProfile {
    pub process_rss_bytes: Option<u64>,
    pub process_peak_rss_bytes: Option<u64>,
    pub process_virtual_bytes: Option<u64>,
    pub highlight_cache_entries: usize,
    pub highlight_cache_limit: usize,
    pub highlight_cache_lines: usize,
    pub highlight_cache_spans: usize,
    pub highlight_cache_text_bytes: usize,
    pub highlight_cache_estimate_bytes: usize,
}

#[derive(Debug, Clone, Default)]
struct MarkdownDebugState {
    stats: MarkdownDebugStats,
}

static MARKDOWN_DEBUG: LazyLock<Mutex<MarkdownDebugState>> =
    LazyLock::new(|| Mutex::new(MarkdownDebugState::default()));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkdownBlockKind {
    Heading,
    Paragraph,
    List,
    BlockQuote,
    DefinitionList,
    CodeBlock,
    DisplayMath,
    Rule,
    HtmlBlock,
    Table,
}

fn spacing_separates_after(kind: MarkdownBlockKind, mode: MarkdownSpacingMode) -> bool {
    match mode {
        MarkdownSpacingMode::Compact => !matches!(kind, MarkdownBlockKind::Heading),
        MarkdownSpacingMode::Document => true,
    }
}

fn line_is_blank(line: &Line<'_>) -> bool {
    line.spans.is_empty()
        || line
            .spans
            .iter()
            .all(|span| span.content.as_ref().is_empty())
}

fn rendered_task_marker_width(text: &str) -> Option<(usize, &str)> {
    if let Some(rest) = text.strip_prefix("[x] ") {
        return Some((UnicodeWidthStr::width("[x] "), rest));
    }
    if let Some(rest) = text.strip_prefix("[ ] ") {
        return Some((UnicodeWidthStr::width("[ ] "), rest));
    }
    None
}

fn rendered_list_marker_width(text: &str) -> Option<usize> {
    if let Some(rest) = text.strip_prefix("• ") {
        let mut width = UnicodeWidthStr::width("• ");
        if let Some((task_width, task_rest)) = rendered_task_marker_width(rest)
            && !task_rest.is_empty()
        {
            width += task_width;
        }
        return (!rest.is_empty()).then_some(width);
    }

    let digit_count = text.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digit_count == 0 {
        return None;
    }

    let suffix = text.get(digit_count..)?;
    let rest = suffix.strip_prefix(". ")?;
    let mut width = digit_count + UnicodeWidthStr::width(". ");
    if let Some((task_width, task_rest)) = rendered_task_marker_width(rest)
        && !task_rest.is_empty()
    {
        width += task_width;
    }
    (!rest.is_empty()).then_some(width)
}

fn repeated_gutter_prefix(line: &Line<'static>) -> Option<(Vec<Span<'static>>, usize)> {
    let plain = line_plain_text(line);
    let mut leading_width = 0usize;
    let mut prefix_bytes = 0usize;
    for ch in plain.chars() {
        if ch.is_whitespace() {
            leading_width += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            prefix_bytes += ch.len_utf8();
        } else {
            break;
        }
    }

    let mut rest = &plain[prefix_bytes..];
    let mut gutter_count = 0usize;
    while let Some(next) = rest.strip_prefix("│ ") {
        gutter_count += 1;
        rest = next;
    }
    let gutter_width = gutter_count * UnicodeWidthStr::width("│ ");
    let base_prefix_width = leading_width + gutter_width;

    if let Some(marker_width) = rendered_list_marker_width(rest) {
        let total_width = base_prefix_width + marker_width;
        if total_width > 0 {
            let mut spans = leading_spans_for_display_width(line, base_prefix_width);
            spans.push(Span::raw(" ".repeat(marker_width)));
            return Some((spans, total_width));
        }
    }

    if gutter_count > 0 {
        return Some((
            leading_spans_for_display_width(line, base_prefix_width),
            base_prefix_width,
        ));
    }

    if leading_width > 0 && line.alignment == Some(Alignment::Left) {
        return Some((
            leading_spans_for_display_width(line, leading_width),
            leading_width,
        ));
    }

    None
}

fn leading_spans_for_display_width(
    line: &Line<'static>,
    target_width: usize,
) -> Vec<Span<'static>> {
    if target_width == 0 {
        return Vec::new();
    }

    let mut spans = Vec::new();
    let mut collected_width = 0usize;

    for span in &line.spans {
        if collected_width >= target_width {
            break;
        }

        let mut text = String::new();
        let mut span_width = 0usize;
        for ch in span.content.chars() {
            let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if collected_width + span_width + ch_width > target_width {
                break;
            }
            text.push(ch);
            span_width += ch_width;
        }

        if !text.is_empty() {
            spans.push(Span::styled(text, span.style));
            collected_width += span_width;
        }
    }

    spans
}

fn push_blank_separator(lines: &mut Vec<Line<'static>>) {
    if lines.last().map(line_is_blank).unwrap_or(false) {
        return;
    }
    lines.push(Line::default());
}

fn push_block_separator(
    lines: &mut Vec<Line<'static>>,
    kind: MarkdownBlockKind,
    mode: MarkdownSpacingMode,
) {
    if spacing_separates_after(kind, mode) {
        push_blank_separator(lines);
    }
}

fn normalize_block_separators(lines: &mut Vec<Line<'static>>) {
    let mut normalized = Vec::with_capacity(lines.len());
    let mut previous_blank = true;

    for line in lines.drain(..) {
        let is_blank = line_is_blank(&line);
        if is_blank {
            if previous_blank {
                continue;
            }
            normalized.push(Line::default());
        } else {
            normalized.push(line);
        }
        previous_blank = is_blank;
    }

    while normalized.last().map(line_is_blank).unwrap_or(false) {
        normalized.pop();
    }

    *lines = normalized;
}

struct HighlightCache {
    entries: HashMap<u64, Vec<Line<'static>>>,
}

impl HighlightCache {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    fn get(&self, hash: u64) -> Option<Vec<Line<'static>>> {
        self.entries.get(&hash).cloned()
    }

    fn insert(&mut self, hash: u64, lines: Vec<Line<'static>>) {
        // Evict if cache is too large
        if self.entries.len() >= HIGHLIGHT_CACHE_LIMIT {
            self.entries.clear();
        }
        self.entries.insert(hash, lines);
    }
}

fn hash_code(code: &str, lang: Option<&str>) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    let mut hasher = DefaultHasher::new();
    code.hash(&mut hasher);
    lang.hash(&mut hasher);
    hasher.finish()
}

#[path = "markdown_incremental.rs"]
mod incremental;

pub use incremental::IncrementalMarkdownRenderer;

fn rendered_rule_width(max_width: Option<usize>) -> usize {
    match max_width {
        Some(width) if center_code_blocks() => width.min(RULE_LEN),
        Some(width) => width,
        None => RULE_LEN,
    }
}

// Colors matching ui.rs palette
use jcode_tui_workspace::color_support::rgb;
fn code_bg() -> Color {
    rgb(45, 45, 45)
}
fn code_fg() -> Color {
    rgb(180, 180, 180)
}
fn math_fg() -> Color {
    rgb(130, 210, 235)
}
fn link_fg() -> Color {
    rgb(120, 180, 240)
}
fn html_fg() -> Color {
    rgb(140, 140, 150)
}
fn text_color() -> Color {
    rgb(200, 200, 195)
}
fn bold_color() -> Color {
    rgb(240, 240, 235)
}
fn heading_h1_color() -> Color {
    rgb(255, 215, 100)
}
fn heading_h2_color() -> Color {
    rgb(240, 190, 90)
}
fn heading_h3_color() -> Color {
    rgb(220, 170, 80)
}
fn heading_color() -> Color {
    rgb(200, 155, 75)
}
fn md_dim_color() -> Color {
    rgb(100, 100, 100)
}
const RULE_LEN: usize = 24;

#[derive(Debug, Clone)]
struct ListRenderState {
    ordered: bool,
    next_index: u64,
    item_line_starts: Vec<usize>,
    max_marker_digits: usize,
}

#[derive(Debug, Default)]
struct CenteredStructuredBlockState {
    depth: usize,
    start_line: Option<usize>,
    ranges: Vec<std::ops::Range<usize>>,
}

fn diagram_side_only() -> bool {
    matches!(effective_diagram_mode(), DiagramDisplayMode::Pinned)
}

fn mermaid_should_register_active() -> bool {
    !matches!(effective_diagram_mode(), DiagramDisplayMode::None)
}

fn mermaid_rendering_enabled() -> bool {
    // M28 (lazydino fork): when this build was compiled with
    // `mermaid-renderer` enabled, mermaid is rendered by default. The
    // `JCODE_ENABLE_MERMAID` env var still acts as an explicit override —
    // set it to `0` (or any value other than `1`) to force-disable, or `1`
    // to force-enable (matters when the cargo feature is off).
    //
    // Upstream behaviour: opt-in only via `JCODE_ENABLE_MERMAID=1` while
    // the renderer was considered unstable. lazydino's binary always ships
    // the renderer feature enabled (see top-level `Cargo.toml`), so the
    // runtime gate becomes "on unless explicitly turned off".
    match std::env::var("JCODE_ENABLE_MERMAID") {
        Ok(value) if value == "1" => true,
        Ok(_) => false,
        Err(_) => cfg!(feature = "mermaid-renderer"),
    }
}

fn mermaid_sidebar_placeholder(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::default().fg(md_dim_color()),
    ))
    .left_aligned()
}

fn apply_inline_decorations(mut style: Style, strike: bool, in_link: bool) -> Style {
    if strike {
        style = style.crossed_out();
    }
    if in_link {
        style = style.fg(link_fg()).underlined();
    }
    style
}

fn ensure_blockquote_prefix(current_spans: &mut Vec<Span<'static>>, blockquote_depth: usize) {
    if blockquote_depth == 0 || !current_spans.is_empty() {
        return;
    }
    let prefix = "│ ".repeat(blockquote_depth);
    current_spans.push(Span::styled(prefix, Style::default().fg(md_dim_color())));
}

fn with_blockquote_prefix(line: Line<'static>, blockquote_depth: usize) -> Line<'static> {
    if blockquote_depth == 0 {
        return line;
    }
    let mut spans = vec![Span::styled(
        "│ ".repeat(blockquote_depth),
        Style::default().fg(md_dim_color()),
    )];
    let alignment = line.alignment;
    spans.extend(line.spans);
    let line = Line::from(spans);
    match alignment {
        Some(align) => line.alignment(align),
        None => line.left_aligned(),
    }
}

fn flush_current_line_with_alignment(
    lines: &mut Vec<Line<'static>>,
    current_spans: &mut Vec<Span<'static>>,
    alignment: Option<Alignment>,
) {
    if !current_spans.is_empty() {
        let line = Line::from(std::mem::take(current_spans));
        lines.push(match alignment {
            Some(align) => line.alignment(align),
            None => line,
        });
    }
}

fn enter_centered_structured_block(state: &mut CenteredStructuredBlockState, current_line: usize) {
    if state.depth == 0 {
        state.start_line = Some(current_line);
    }
    state.depth = state.depth.saturating_add(1);
}

fn exit_centered_structured_block(state: &mut CenteredStructuredBlockState, current_line: usize) {
    if state.depth == 0 {
        return;
    }
    state.depth = state.depth.saturating_sub(1);
    if state.depth == 0
        && let Some(start) = state.start_line.take()
        && current_line > start
    {
        state.ranges.push(start..current_line);
    }
}

fn record_centered_independent_block(
    state: &mut CenteredStructuredBlockState,
    start_line: usize,
    end_line: usize,
) {
    if state.depth == 0 && end_line > start_line {
        state.ranges.push(start_line..end_line);
    }
}

fn finalize_centered_structured_blocks(
    state: &mut CenteredStructuredBlockState,
    current_line: usize,
) {
    if state.depth > 0 {
        state.depth = 0;
        if let Some(start) = state.start_line.take()
            && current_line > start
        {
            state.ranges.push(start..current_line);
        }
    }
}

fn center_structured_block_ranges(
    lines: &mut [Line<'static>],
    width: usize,
    ranges: &[std::ops::Range<usize>],
) {
    if width == 0 {
        return;
    }

    for range in ranges {
        if range.start >= range.end || range.end > lines.len() {
            continue;
        }

        let run = &mut lines[range.start..range.end];
        let max_line_width = run
            .iter()
            .filter(|line| !line_is_blank(line))
            .map(Line::width)
            .max()
            .unwrap_or(0);
        let pad = width.saturating_sub(max_line_width) / 2;
        if pad > 0 {
            let pad_str = " ".repeat(pad);
            for line in run {
                if line_is_blank(line) {
                    continue;
                }
                line.spans.insert(0, Span::raw(pad_str.clone()));
                line.alignment = Some(Alignment::Left);
            }
        }
    }
}

fn leading_raw_padding_width(line: &Line<'_>) -> usize {
    line.spans
        .iter()
        .take_while(|span| {
            span.style == Style::default()
                && !span.content.is_empty()
                && span.content.chars().all(|ch| ch == ' ')
        })
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

fn strip_leading_raw_padding(line: &mut Line<'static>, trim_width: usize) {
    if trim_width == 0 {
        return;
    }

    let mut remaining = trim_width;
    while remaining > 0 && !line.spans.is_empty() {
        let span = &line.spans[0];
        let is_raw_padding = span.style == Style::default()
            && !span.content.is_empty()
            && span.content.chars().all(|ch| ch == ' ');
        if !is_raw_padding {
            break;
        }

        let span_width = UnicodeWidthStr::width(span.content.as_ref());
        if span_width <= remaining {
            line.spans.remove(0);
            remaining -= span_width;
            continue;
        }

        let keep = span_width.saturating_sub(remaining);
        line.spans[0].content = " ".repeat(keep).into();
        remaining = 0;
    }
}

fn blockquote_gutter_width(text: &str) -> (usize, &str) {
    let mut rest = text;
    let mut width = 0usize;
    while let Some(next) = rest.strip_prefix("│ ") {
        width += UnicodeWidthStr::width("│ ");
        rest = next;
    }
    (width, rest)
}

fn ordered_marker_components(text: &str) -> Option<(usize, usize)> {
    let indent_width = text.chars().take_while(|ch| *ch == ' ').count();
    let suffix = text.get(indent_width..)?;
    let digit_count = suffix.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digit_count == 0 {
        return None;
    }
    let rest = suffix.get(digit_count..)?;
    rest.strip_prefix(". ")?;
    Some((indent_width, digit_count))
}

fn ordered_marker_info(line: &Line<'_>) -> Option<(usize, usize, usize)> {
    let plain = line_plain_text(line);
    let leading_width = plain
        .chars()
        .take_while(|ch: &char| ch.is_whitespace())
        .count();
    let rest = plain.get(leading_width..)?;
    let (gutter_width, rest) = blockquote_gutter_width(rest);
    let (indent_width, digit_count) = ordered_marker_components(rest)?;
    Some((leading_width + gutter_width, indent_width, digit_count))
}

fn pad_ordered_marker_line(
    line: &mut Line<'static>,
    marker_prefix_width: usize,
    indent_width: usize,
    extra_pad: usize,
) {
    if extra_pad == 0 {
        return;
    }

    let mut consumed_width = 0usize;
    for span in &mut line.spans {
        let span_width = UnicodeWidthStr::width(span.content.as_ref());
        if consumed_width + span_width <= marker_prefix_width {
            consumed_width += span_width;
            continue;
        }

        let content = span.content.as_ref();
        let indent_prefix = " ".repeat(indent_width);
        if let Some(rest) = content.strip_prefix(&indent_prefix) {
            let digit_count = rest.chars().take_while(|ch| ch.is_ascii_digit()).count();
            if digit_count > 0 {
                let mut updated = indent_prefix;
                updated.push_str(&" ".repeat(extra_pad));
                updated.push_str(rest);
                span.content = updated.into();
            }
        }
        break;
    }
}

fn align_ordered_list_markers(
    lines: &mut [Line<'static>],
    item_starts: &[usize],
    max_digits: usize,
) {
    if max_digits <= 1 {
        return;
    }

    for &line_idx in item_starts {
        let Some(line) = lines.get_mut(line_idx) else {
            continue;
        };
        let Some((marker_prefix_width, indent_width, digit_count)) = ordered_marker_info(line)
        else {
            continue;
        };
        let extra_pad = max_digits.saturating_sub(digit_count);
        pad_ordered_marker_line(line, marker_prefix_width, indent_width, extra_pad);
    }
}

pub fn recenter_structured_blocks_for_display(lines: &mut [Line<'static>], width: usize) {
    if width == 0 {
        return;
    }

    let mut idx = 0usize;
    while idx < lines.len() {
        let is_structured =
            !line_is_blank(&lines[idx]) && lines[idx].alignment == Some(Alignment::Left);
        if !is_structured {
            idx += 1;
            continue;
        }

        let start = idx;
        while idx < lines.len()
            && !line_is_blank(&lines[idx])
            && lines[idx].alignment == Some(Alignment::Left)
        {
            idx += 1;
        }

        let run = &mut lines[start..idx];
        let common_pad = run.iter().map(leading_raw_padding_width).min().unwrap_or(0);
        if common_pad > 0 {
            for line in run.iter_mut() {
                strip_leading_raw_padding(line, common_pad);
            }
        }

        let max_line_width = run.iter().map(Line::width).max().unwrap_or(0);
        let pad = width.saturating_sub(max_line_width) / 2;
        if pad > 0 {
            let pad_str = " ".repeat(pad);
            for line in run.iter_mut() {
                line.spans.insert(0, Span::raw(pad_str.clone()));
                line.alignment = Some(Alignment::Left);
            }
        }
    }
}

fn structured_markdown_alignment(
    blockquote_depth: usize,
    list_stack: &[ListRenderState],
    in_definition_list: bool,
    in_footnote_definition: bool,
) -> Option<Alignment> {
    if blockquote_depth > 0
        || !list_stack.is_empty()
        || in_definition_list
        || in_footnote_definition
    {
        Some(Alignment::Left)
    } else {
        None
    }
}

fn parse_opening_fence(line: &str) -> Option<(char, usize)> {
    let indent = line.chars().take_while(|c| *c == ' ').count();
    if indent > 3 {
        return None;
    }
    let trimmed = &line[indent..];
    let first = trimmed.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }

    let fence_len = trimmed.chars().take_while(|c| *c == first).count();
    if fence_len < 3 {
        return None;
    }

    Some((first, fence_len))
}

fn is_closing_fence(line: &str, fence_char: char, min_len: usize) -> bool {
    let indent = line.chars().take_while(|c| *c == ' ').count();
    if indent > 3 {
        return false;
    }
    let trimmed = &line[indent..];

    let fence_len = trimmed.chars().take_while(|c| *c == fence_char).count();
    if fence_len < min_len {
        return false;
    }

    trimmed[fence_len..].trim().is_empty()
}

fn count_unescaped_double_dollar(line: &str) -> usize {
    let bytes = line.as_bytes();
    let mut count = 0usize;
    let mut ix = 0usize;

    while ix + 1 < bytes.len() {
        if bytes[ix] == b'\\' {
            ix += 2;
            continue;
        }
        if bytes[ix] == b'$' && bytes[ix + 1] == b'$' {
            count += 1;
            ix += 2;
            continue;
        }
        ix += 1;
    }

    count
}

fn math_inline_span(math: &str) -> Span<'static> {
    Span::styled(format!("${}$", math), Style::default().fg(math_fg()))
}

fn math_display_lines(math: &str) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let dim = Style::default().fg(md_dim_color());
    out.push(Line::from(Span::styled("┌─ math ", dim)).left_aligned());
    for line in math.lines() {
        out.push(
            Line::from(vec![
                Span::styled("│ ", dim),
                Span::styled(line.to_string(), Style::default().fg(math_fg())),
            ])
            .left_aligned(),
        );
    }
    if math.is_empty() {
        out.push(
            Line::from(vec![
                Span::styled("│ ", dim),
                Span::styled("", Style::default().fg(math_fg())),
            ])
            .left_aligned(),
        );
    }
    out.push(Line::from(Span::styled("└─", dim)).left_aligned());
    out
}
fn table_color() -> Color {
    rgb(150, 150, 150)
}

/// Render markdown text to styled ratatui Lines
pub fn render_markdown(text: &str) -> Vec<Line<'static>> {
    render_markdown_with_width(text, None)
}

/// Escape dollar signs that look like currency amounts so the math parser
/// doesn't swallow them.  Currency: `$` followed by a digit (e.g. `$35`,
/// `$5.99`).  We turn those into `\$` which pulldown-cmark passes through
/// as literal text rather than starting an inline-math span.
///
/// We skip dollars inside code spans/fences and already-escaped `\$`.
fn escape_currency_dollars(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut in_code_fence = false;
    let mut inline_code_len: usize = 0;
    let mut at_line_start = true;
    let mut leading_spaces = 0;

    let count_backticks = |chars: &[char], start: usize| {
        let mut j = start;
        while j < chars.len() && chars[j] == '`' {
            j += 1;
        }
        j - start
    };

    let is_escaped = |chars: &[char], pos: usize| {
        let mut backslashes = 0usize;
        let mut j = pos;
        while j > 0 {
            if chars[j - 1] != '\\' {
                break;
            }
            backslashes += 1;
            j -= 1;
        }
        backslashes % 2 == 1
    };

    while i < len {
        let c = chars[i];

        if c == '\n' {
            at_line_start = true;
            leading_spaces = 0;
            out.push('\n');
            i += 1;
            continue;
        }

        if at_line_start && (c == ' ' || c == '\t') {
            leading_spaces += 1;
            out.push(c);
            i += 1;
            continue;
        }

        let maybe_fence = inline_code_len == 0 && c == '`' && count_backticks(&chars, i) >= 3;
        if maybe_fence && at_line_start && leading_spaces <= 3 {
            let run = count_backticks(&chars, i);
            for _ in 0..run {
                out.push('`');
            }
            i += run;
            in_code_fence = !in_code_fence;
            at_line_start = false;
            leading_spaces = 0;
            continue;
        }

        if c == '`' {
            let run = count_backticks(&chars, i);
            if inline_code_len > 0 {
                if run == inline_code_len {
                    inline_code_len = 0;
                }
                for _ in 0..run {
                    out.push('`');
                }
                i += run;
                at_line_start = false;
                leading_spaces = 0;
                continue;
            }

            inline_code_len = run;
            for _ in 0..run {
                out.push('`');
            }
            i += run;
            at_line_start = false;
            leading_spaces = 0;
            continue;
        }

        if at_line_start {
            at_line_start = false;
        }

        if c == ' ' || c == '\t' {
            out.push(c);
            i += 1;
            continue;
        }

        if in_code_fence || inline_code_len > 0 {
            out.push(c);
            i += 1;
            continue;
        }

        if c == '$' && i + 1 < len && chars[i + 1] == '$' {
            out.push_str("$$");
            i += 2;
            continue;
        }

        if c == '$' && i + 1 < len && chars[i + 1].is_ascii_digit() {
            if is_escaped(&chars, i) {
                out.push('$');
            } else {
                out.push_str("\\$");
            }
            i += 1;
            continue;
        }

        out.push(c);
        i += 1;
    }
    out
}

fn looks_like_line_oriented_transcript_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return false;
    }

    if trimmed.starts_with("tool:")
        || trimmed.starts_with("tools:")
        || trimmed.starts_with("broadcast from ")
    {
        return true;
    }

    matches!(trimmed.chars().next(), Some('✓' | '✗' | '┌' | '│' | '└'))
}

fn repair_glued_markdown_headings(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let lines: Vec<&str> = text.split('\n').collect();
    let mut in_code_fence = false;
    let mut fence_char = '\0';
    let mut fence_len = 0usize;

    for (idx, line) in lines.iter().enumerate() {
        if in_code_fence {
            out.push_str(line);
        } else {
            out.push_str(&repair_glued_heading_markers_in_line(line));
        }

        if idx + 1 < lines.len() {
            out.push('\n');
        }

        if in_code_fence {
            if is_closing_fence(line, fence_char, fence_len) {
                in_code_fence = false;
                fence_char = '\0';
                fence_len = 0;
            }
        } else if let Some((marker, min_len)) = parse_opening_fence(line) {
            in_code_fence = true;
            fence_char = marker;
            fence_len = min_len;
        }
    }

    out
}

fn repair_glued_code_fences(text: &str) -> String {
    let mut out = Vec::new();
    let mut in_code_fence = false;
    let mut fence_char = '\0';
    let mut fence_len = 0usize;

    for line in text.split('\n') {
        let mut pending = std::collections::VecDeque::from([line.to_string()]);
        while let Some(segment) = pending.pop_front() {
            if !in_code_fence {
                if let Some((before, after)) = split_before_glued_fence_marker(&segment) {
                    out.push(before);
                    pending.push_front(after);
                    continue;
                }

                if let Some((opener, code_after)) = split_compact_fence_opening(&segment) {
                    update_code_fence_state_after_line(
                        &opener,
                        &mut in_code_fence,
                        &mut fence_char,
                        &mut fence_len,
                    );
                    out.push(opener);
                    pending.push_front(code_after);
                    continue;
                }

                update_code_fence_state_after_line(
                    &segment,
                    &mut in_code_fence,
                    &mut fence_char,
                    &mut fence_len,
                );
                out.push(segment);
                continue;
            }

            if let Some((before, after)) =
                split_code_line_before_closing_marker(&segment, fence_char)
            {
                if !before.is_empty() {
                    out.push(before);
                }
                pending.push_front(after);
                continue;
            }

            if let Some((closing, trailing)) =
                split_closing_fence_with_trailing(&segment, fence_char, fence_len)
            {
                out.push(closing.clone());
                update_code_fence_state_after_line(
                    &closing,
                    &mut in_code_fence,
                    &mut fence_char,
                    &mut fence_len,
                );
                if !trailing.is_empty() {
                    pending.push_front(trailing);
                }
                continue;
            }

            update_code_fence_state_after_line(
                &segment,
                &mut in_code_fence,
                &mut fence_char,
                &mut fence_len,
            );
            out.push(segment);
        }
    }

    out.join("\n")
}

fn repair_glued_list_markers(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_code_fence = false;
    let mut fence_char = '\0';
    let mut fence_len = 0usize;

    for (idx, line) in text.split('\n').enumerate() {
        if idx > 0 {
            out.push('\n');
        }

        if in_code_fence {
            out.push_str(line);
            update_code_fence_state_after_line(
                line,
                &mut in_code_fence,
                &mut fence_char,
                &mut fence_len,
            );
            continue;
        }

        let repaired = repair_glued_list_markers_in_line(line);
        update_code_fence_state_after_line(
            &repaired,
            &mut in_code_fence,
            &mut fence_char,
            &mut fence_len,
        );
        out.push_str(&repaired);
    }

    out
}

fn repair_glued_list_markers_in_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut cursor = 0usize;
    let mut scan = 0usize;
    let mut line_start = true;

    while scan < line.len() {
        if !line.is_char_boundary(scan) {
            scan += 1;
            continue;
        }

        if let Some(marker) = glued_list_marker_at(line, scan, line_start) {
            out.push_str(line[cursor..scan].trim_end());
            out.push('\n');
            if let Some(replacement) = marker.replacement {
                out.push_str(replacement);
                cursor = scan + marker.len;
                scan = cursor;
            } else {
                cursor = scan;
                scan += marker.len;
            }
            line_start = true;
            continue;
        }

        if let Some(ch) = line[scan..].chars().next() {
            if ch == '\n' {
                line_start = true;
            } else if !ch.is_whitespace() {
                line_start = false;
            }
            scan += ch.len_utf8();
        } else {
            break;
        }
    }

    out.push_str(&line[cursor..]);
    out
}

struct GluedListMarker {
    len: usize,
    replacement: Option<&'static str>,
}

fn glued_list_marker_at(line: &str, idx: usize, line_start: bool) -> Option<GluedListMarker> {
    if idx == 0 || line_start || inside_inline_backticks(line, idx) {
        return None;
    }

    let before = previous_non_whitespace_char(line, idx)?;
    if !is_list_glue_boundary_char(before) {
        return None;
    }

    ordered_list_marker_len_at(line, idx).or_else(|| bullet_list_marker_at(line, idx))
}

fn previous_non_whitespace_char(line: &str, idx: usize) -> Option<char> {
    line[..idx].chars().rev().find(|ch| !ch.is_whitespace())
}

fn ordered_list_marker_len_at(line: &str, idx: usize) -> Option<GluedListMarker> {
    let rest = &line[idx..];
    let mut digit_bytes = 0usize;
    let mut digit_count = 0usize;
    for ch in rest.chars() {
        if ch.is_ascii_digit() {
            digit_bytes += ch.len_utf8();
            digit_count += 1;
        } else {
            break;
        }
    }
    if digit_count == 0 || digit_count > 9 {
        return None;
    }

    let marker = rest[digit_bytes..].chars().next()?;
    if !matches!(marker, '.' | ')') {
        return None;
    }
    let after_marker_idx = digit_bytes + marker.len_utf8();
    let after_marker = rest[after_marker_idx..].chars().next()?;
    if !after_marker.is_whitespace() {
        return None;
    }

    Some(GluedListMarker {
        len: after_marker_idx + after_marker.len_utf8(),
        replacement: None,
    })
}

fn bullet_list_marker_at(line: &str, idx: usize) -> Option<GluedListMarker> {
    let rest = &line[idx..];
    let marker = rest.chars().next()?;
    if !matches!(marker, '-' | '*' | '+' | '•') {
        return None;
    }
    let after_marker_idx = marker.len_utf8();
    let after_marker = rest[after_marker_idx..].chars().next()?;
    if !after_marker.is_whitespace() {
        return None;
    }
    Some(GluedListMarker {
        len: after_marker_idx + after_marker.len_utf8(),
        replacement: (marker == '•').then_some("- "),
    })
}

fn is_list_glue_boundary_char(ch: char) -> bool {
    matches!(
        ch,
        ':' | ';' | '.' | '!' | '?' | ')' | ']' | '}' | '。' | '！' | '？'
    )
}

fn update_code_fence_state_after_line(
    line: &str,
    in_code_fence: &mut bool,
    fence_char: &mut char,
    fence_len: &mut usize,
) {
    if *in_code_fence {
        if is_closing_fence(line, *fence_char, *fence_len) {
            *in_code_fence = false;
            *fence_char = '\0';
            *fence_len = 0;
        }
    } else if let Some((marker, min_len)) = parse_opening_fence(line) {
        *in_code_fence = true;
        *fence_char = marker;
        *fence_len = min_len;
    }
}

fn split_before_glued_fence_marker(line: &str) -> Option<(String, String)> {
    find_fence_run(line).and_then(|(idx, _marker, _len)| {
        if idx == 0 || line[..idx].trim().is_empty() || inside_inline_backticks(line, idx) {
            return None;
        }

        let before = line[..idx].trim_end().to_string();
        let after = line[idx..].trim_start().to_string();
        if before.is_empty() || after.is_empty() {
            None
        } else {
            Some((before, after))
        }
    })
}

fn split_code_line_before_closing_marker(line: &str, fence_char: char) -> Option<(String, String)> {
    let marker = fence_char.to_string().repeat(3);
    let idx = line.find(&marker)?;
    if idx == 0 || line[..idx].trim().is_empty() || inside_inline_backticks(line, idx) {
        return None;
    }
    let prev = line[..idx].chars().next_back()?;
    if !prev.is_whitespace() {
        return None;
    }
    Some((line[..idx].trim_end().to_string(), line[idx..].to_string()))
}

fn split_closing_fence_with_trailing(
    line: &str,
    fence_char: char,
    min_len: usize,
) -> Option<(String, String)> {
    let indent_len = line.chars().take_while(|c| *c == ' ').count();
    if indent_len > 3 {
        return None;
    }
    let trimmed = &line[indent_len..];
    let fence_len = trimmed.chars().take_while(|c| *c == fence_char).count();
    if fence_len < min_len {
        return None;
    }

    let fence_byte_len = trimmed
        .char_indices()
        .nth(fence_len)
        .map(|(idx, _)| idx)
        .unwrap_or(trimmed.len());
    let trailing = trimmed[fence_byte_len..].trim_start();
    if trailing.is_empty() {
        return None;
    }

    Some((
        line[..indent_len + fence_byte_len].to_string(),
        trailing.to_string(),
    ))
}

fn split_compact_fence_opening(line: &str) -> Option<(String, String)> {
    let indent_len = line.chars().take_while(|c| *c == ' ').count();
    if indent_len > 3 {
        return None;
    }
    let trimmed = &line[indent_len..];
    let (marker, fence_len) = parse_opening_fence(line)?;
    let fence_byte_len = trimmed
        .char_indices()
        .nth(fence_len)
        .map(|(idx, _)| idx)
        .unwrap_or(trimmed.len());
    let info = trimmed[fence_byte_len..].trim_start();
    let (language, rest) = split_fence_info_language_and_inline_code(info)?;
    if !should_treat_fence_info_rest_as_code(rest) {
        return None;
    }

    let opener = format!(
        "{}{}{}",
        &line[..indent_len],
        marker.to_string().repeat(fence_len),
        language
    );
    Some((opener, rest.to_string()))
}

fn split_fence_info_language_and_inline_code(info: &str) -> Option<(&str, &str)> {
    let mut language_end = None;
    for (idx, ch) in info.char_indices() {
        if ch.is_whitespace() {
            language_end = Some(idx);
            break;
        }
    }
    let language_end = language_end?;
    let language = &info[..language_end];
    let rest = info[language_end..].trim_start();
    if language.is_empty() || rest.is_empty() || !looks_like_markdown_code_language(language) {
        return None;
    }
    Some((language, rest))
}

fn looks_like_markdown_code_language(language: &str) -> bool {
    language
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '+' | '#' | '.'))
}

fn should_treat_fence_info_rest_as_code(rest: &str) -> bool {
    let trimmed = rest.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return false;
    }
    trimmed.chars().any(|ch| {
        matches!(
            ch,
            ':' | '=' | '(' | ')' | '{' | '}' | '[' | ']' | '<' | '>' | ';' | '$' | '\'' | '"'
        )
    })
}

fn find_fence_run(line: &str) -> Option<(usize, char, usize)> {
    let bytes = line.as_bytes();
    let mut idx = 0usize;
    while idx < bytes.len() {
        let marker = match bytes[idx] {
            b'`' => '`',
            b'~' => '~',
            _ => {
                idx += 1;
                continue;
            }
        };

        let mut len = 0usize;
        while idx + len < bytes.len() && bytes[idx + len] == bytes[idx] {
            len += 1;
        }
        if len >= 3 {
            return Some((idx, marker, len));
        }
        idx += len.max(1);
    }
    None
}

fn repair_line_oriented_markdown_boundaries(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let lines: Vec<&str> = text.split('\n').collect();
    let mut in_code_fence = false;
    let mut fence_char = '\0';
    let mut fence_len = 0usize;

    for (idx, line) in lines.iter().enumerate() {
        let prev_line = idx.checked_sub(1).map(|prev| lines[prev]);
        if !in_code_fence
            && line_starts_interrupting_markdown_block(line)
            && prev_line.is_some_and(|prev| {
                !prev.trim().is_empty() && !line_starts_interrupting_markdown_block(prev)
            })
            && !out.ends_with("\n\n")
        {
            out.push('\n');
        }

        out.push_str(line);
        if idx + 1 < lines.len() {
            out.push('\n');
        }

        if in_code_fence {
            if is_closing_fence(line, fence_char, fence_len) {
                in_code_fence = false;
                fence_char = '\0';
                fence_len = 0;
            }
        } else if let Some((marker, min_len)) = parse_opening_fence(line) {
            in_code_fence = true;
            fence_char = marker;
            fence_len = min_len;
        }
    }

    out
}

fn repair_line_oriented_list_item_continuations(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let lines: Vec<&str> = text.split('\n').collect();
    let mut in_code_fence = false;
    let mut fence_char = '\0';
    let mut fence_len = 0usize;

    for (idx, line) in lines.iter().enumerate() {
        out.push_str(line);
        if idx + 1 < lines.len() {
            let next = lines[idx + 1];
            if should_preserve_list_item_continuation_break(line, next, in_code_fence)
                && !line.ends_with("  ")
            {
                out.push_str("  ");
            }
            out.push('\n');
        }

        if in_code_fence {
            if is_closing_fence(line, fence_char, fence_len) {
                in_code_fence = false;
                fence_char = '\0';
                fence_len = 0;
            }
        } else if let Some((marker, min_len)) = parse_opening_fence(line) {
            in_code_fence = true;
            fence_char = marker;
            fence_len = min_len;
        }
    }

    out
}

fn should_preserve_list_item_continuation_break(
    line: &str,
    next: &str,
    in_code_fence: bool,
) -> bool {
    if in_code_fence {
        return false;
    }
    let trimmed = line.trim_start();
    let next_trimmed = next.trim_start();
    if trimmed.is_empty() || next_trimmed.is_empty() {
        return false;
    }
    if !starts_with_ordered_or_bullet_list_marker(trimmed) {
        return false;
    }
    if line_starts_interrupting_markdown_block(next)
        || starts_with_ordered_or_bullet_list_marker(next_trimmed)
        || next.starts_with(' ')
        || next.starts_with('\t')
    {
        return false;
    }

    true
}

fn starts_with_ordered_or_bullet_list_marker(line: &str) -> bool {
    looks_like_ordered_list_item_for_boundary_repair(line)
        || matches!(line.as_bytes(), [b'-' | b'*' | b'+', b' ' | b'\t', ..])
        || line.starts_with("• ")
}

fn line_starts_interrupting_markdown_block(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return false;
    }
    trimmed.starts_with("- ")
        || trimmed.starts_with("* ")
        || trimmed.starts_with("+ ")
        || trimmed.starts_with("- [")
        || trimmed.starts_with("* [")
        || trimmed.starts_with("+ [")
        || trimmed.starts_with("> ")
        || trimmed.starts_with("```")
        || trimmed.starts_with("~~~")
        || is_heading_line_for_boundary_repair(trimmed)
        || looks_like_ordered_list_item_for_boundary_repair(trimmed)
}

fn is_heading_line_for_boundary_repair(line: &str) -> bool {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&hashes) && line.chars().nth(hashes) == Some(' ')
}

fn looks_like_ordered_list_item_for_boundary_repair(line: &str) -> bool {
    let digit_count = line.chars().take_while(|c| c.is_ascii_digit()).count();
    digit_count > 0
        && matches!(line.chars().nth(digit_count), Some('.' | ')'))
        && matches!(line.chars().nth(digit_count + 1), Some(' ' | '\t'))
}

fn repair_glued_heading_markers_in_line(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut splits = Vec::new();
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] != b'#' {
            i += 1;
            continue;
        }

        let mut hashes = 0usize;
        while i + hashes < bytes.len() && bytes[i + hashes] == b'#' {
            hashes += 1;
        }

        if (2..=6).contains(&hashes)
            && i + hashes < bytes.len()
            && bytes[i + hashes] == b' '
            && i > 0
            && !line[..i].trim().is_empty()
            && line[..i]
                .chars()
                .next_back()
                .is_some_and(|ch| !ch.is_whitespace() && ch != '`')
            && !inside_inline_backticks(line, i)
        {
            splits.push(i);
        }

        i += hashes.max(1);
    }

    if splits.is_empty() {
        return line.to_string();
    }

    let mut out = String::with_capacity(line.len() + splits.len());
    let mut start = 0usize;
    for split in splits {
        out.push_str(line[start..split].trim_end());
        out.push('\n');
        start = split;
    }
    out.push_str(&line[start..]);
    out
}

fn inside_inline_backticks(line: &str, byte_idx: usize) -> bool {
    let mut tick_runs = 0usize;
    let mut escaped = false;
    for (idx, ch) in line.char_indices() {
        if idx >= byte_idx {
            break;
        }
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
        } else if ch == '`' {
            tick_runs += 1;
        }
    }
    tick_runs % 2 == 1
}

fn preserve_line_oriented_softbreaks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let lines: Vec<&str> = text.split('\n').collect();
    let mut in_code_fence = false;
    let mut fence_char = '\0';
    let mut fence_len = 0usize;

    for (idx, line) in lines.iter().enumerate() {
        let prev_line = idx.checked_sub(1).map(|prev| lines[prev]);
        let prev_log_like = prev_line.is_some_and(looks_like_line_oriented_transcript_line);
        let next_log_like =
            idx + 1 < lines.len() && looks_like_line_oriented_transcript_line(lines[idx + 1]);
        let line_log_like = looks_like_line_oriented_transcript_line(line);
        let entering_log_block = !in_code_fence
            && line_log_like
            && !prev_log_like
            && prev_line.is_some_and(|prev| !prev.trim().is_empty());
        let leaving_log_block = !in_code_fence
            && line_log_like
            && !next_log_like
            && idx + 1 < lines.len()
            && !lines[idx + 1].trim().is_empty();
        let preserve_softbreak = !in_code_fence && line_log_like && next_log_like;

        if entering_log_block && !out.ends_with("\n\n") {
            out.push('\n');
        }

        out.push_str(line);
        if idx + 1 < lines.len() {
            if preserve_softbreak && !line.ends_with("  ") {
                out.push_str("  ");
            }
            out.push('\n');
            if leaving_log_block {
                out.push('\n');
            }
        }

        if in_code_fence {
            if is_closing_fence(line, fence_char, fence_len) {
                in_code_fence = false;
                fence_char = '\0';
                fence_len = 0;
            }
        } else if let Some((marker, min_len)) = parse_opening_fence(line) {
            in_code_fence = true;
            fence_char = marker;
            fence_len = min_len;
        }
    }

    out
}

pub fn debug_stats() -> MarkdownDebugStats {
    if let Ok(state) = MARKDOWN_DEBUG.lock() {
        return state.stats.clone();
    }
    MarkdownDebugStats::default()
}

pub fn debug_memory_profile() -> MarkdownMemoryProfile {
    let process = crate::process_memory_snapshot();
    let mut profile = MarkdownMemoryProfile {
        process_rss_bytes: process.rss_bytes,
        process_peak_rss_bytes: process.peak_rss_bytes,
        process_virtual_bytes: process.virtual_bytes,
        highlight_cache_limit: HIGHLIGHT_CACHE_LIMIT,
        ..MarkdownMemoryProfile::default()
    };

    if let Ok(cache) = HIGHLIGHT_CACHE.lock() {
        profile.highlight_cache_entries = cache.entries.len();
        for lines in cache.entries.values() {
            profile.highlight_cache_lines += lines.len();
            profile.highlight_cache_estimate_bytes += estimate_lines_bytes(lines);
            for line in lines {
                profile.highlight_cache_spans += line.spans.len();
                profile.highlight_cache_text_bytes += line
                    .spans
                    .iter()
                    .map(|span| span.content.len())
                    .sum::<usize>();
            }
        }
    }

    profile
}

pub fn reset_debug_stats() {
    if let Ok(mut state) = MARKDOWN_DEBUG.lock() {
        state.stats = MarkdownDebugStats::default();
    }
}

fn estimate_lines_bytes(lines: &[Line<'static>]) -> usize {
    lines
        .iter()
        .map(|line| {
            std::mem::size_of::<Line<'static>>()
                + line.spans.len() * std::mem::size_of::<Span<'static>>()
                + line
                    .spans
                    .iter()
                    .map(|span| span.content.len())
                    .sum::<usize>()
        })
        .sum()
}

pub fn debug_stats_json() -> Option<serde_json::Value> {
    serde_json::to_value(debug_stats()).ok()
}

/// Render markdown with optional width constraint for tables
pub fn wrap_line(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    wrap::wrap_line(line, width, repeated_gutter_prefix)
}

pub fn wrap_lines(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    wrap::wrap_lines(lines, width, repeated_gutter_prefix)
}

pub fn progress_bar(progress: f32, width: usize) -> String {
    wrap::progress_bar(progress, width)
}

pub fn progress_line(label: &str, progress: f32, width: usize) -> Line<'static> {
    wrap::progress_line(label, progress, width)
}

#[cfg(test)]
#[path = "markdown_tests/mod.rs"]
mod tests;
