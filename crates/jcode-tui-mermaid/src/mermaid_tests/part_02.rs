#[test]
fn precise_viewport_accepts_high_auto_zoom_without_panicking() {
    let area = ratatui::prelude::Rect::new(0, 0, 40, 20);
    let mut buf = ratatui::buffer::Buffer::empty(area);

    // No picker/cache is installed in this unit test, so rendering returns 0.
    // The important regression coverage is that the high-zoom precise API is
    // accepted and follows the normal graceful early-return path.
    assert_eq!(
        super::render_image_widget_viewport_precise(0xdef, area, &mut buf, 12, 0, 1000, false),
        0
    );
}

#[test]
fn viewport_crop_resize_scales_complete_zoomed_crops_to_fill_destination() {
    // A high-zoom fit-fill viewport crops a small source rectangle, then must
    // scale that crop back up to the destination cell area. Rendering it with
    // Fit caused the pane to report fit-fill while visually staying tiny.
    assert!(super::viewport_render::viewport_crop_should_scale_to_area(
        280, 180, 280, 180
    ));

    // When the requested viewport is larger than the source on an axis, the
    // crop is the whole remaining source image. That case should keep aspect
    // ratio instead of stretching a non-cropped image.
    assert!(!super::viewport_render::viewport_crop_should_scale_to_area(
        280, 120, 280, 180
    ));
    assert!(!super::viewport_render::viewport_crop_should_scale_to_area(
        200, 180, 280, 180
    ));
}

#[test]
fn preferred_aspect_ratio_context_is_scoped_and_bucketed() {
    assert_eq!(super::current_preferred_aspect_ratio_bucket(), None);

    let outer = super::with_preferred_aspect_ratio(Some(0.75), || {
        assert_eq!(super::current_preferred_aspect_ratio_bucket(), Some(750));
        super::with_preferred_aspect_ratio(Some(1.25), || {
            assert_eq!(super::current_preferred_aspect_ratio_bucket(), Some(1250));
        });
        super::current_preferred_aspect_ratio_bucket()
    });

    assert_eq!(outer, Some(750));
    assert_eq!(super::current_preferred_aspect_ratio_bucket(), None);
}

#[test]
fn preferred_aspect_ratio_adjusts_render_height_without_changing_width_bucket() {
    let (default_width, default_height) = super::calculate_render_size(6, 5, Some(80));
    let (profile_width, profile_height) = super::with_preferred_aspect_ratio(Some(0.5), || {
        super::calculate_render_size(6, 5, Some(80))
    });

    assert_eq!(profile_width, default_width);
    assert!(
        profile_height > default_height,
        "portrait side-pane aspect should request a taller render: default={default_height}, profiled={profile_height}"
    );
    assert!((profile_width / profile_height - 0.5).abs() < 0.01);
}

#[test]
fn deferred_render_supersedes_prefix_stream_updates_only() {
    let partial = "flowchart TD\nA[Start] --> B[In progress]";
    let extended = "flowchart TD\nA[Start] --> B[In progress]\nB --> C[Done]";

    assert!(super::cache_render::is_likely_stream_update(
        partial, extended
    ));
    assert!(super::cache_render::is_likely_stream_update(
        extended, partial
    ));

    assert!(!super::cache_render::is_likely_stream_update(
        "flowchart TD\nA[Start] --> B[One]",
        "flowchart TD\nA[Start] --> C[Different]",
    ));
    assert!(!super::cache_render::is_likely_stream_update(
        "flowchart TD\nA",
        "flowchart TD\nA[short]",
    ));
}

#[cfg(all(feature = "mmdr-size-api", mmdr_size_api_available))]
#[test]
fn mmdr_size_api_reports_explicit_png_canvas() {
    super::reset_debug_stats();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let content = format!("flowchart TD\nA[Start {unique}] --> B[End]");

    let result = super::render_mermaid_untracked(&content, Some(100));
    let (width, height) = match result {
        super::RenderResult::Image { width, height, .. } => (width, height),
        super::RenderResult::Error(error) => panic!("render failed: {error}"),
    };
    let stats = super::debug_stats();

    assert_eq!(stats.last_measured_width, stats.last_target_width);
    assert_eq!(stats.last_measured_height, stats.last_target_height);
    assert_eq!(Some(width), stats.last_measured_width);
    assert_eq!(Some(height), stats.last_measured_height);
    assert!(stats.last_viewbox_width.unwrap_or_default() > 0);
    assert!(stats.last_viewbox_height.unwrap_or_default() > 0);
}

// ---------------------------------------------------------------------------
// Inline mermaid crop-bug regression tests (M28 follow-up).
//
// These tests guard the placeholder-vs-render geometry that drove the
// chat-message bottom/right cropping bug. The math here mirrors what
// `estimate_image_height` and `render_image_widget_fit` rely on, without
// requiring a real terminal image protocol.
// ---------------------------------------------------------------------------

#[test]
fn estimate_image_height_caps_height_to_fit_max_width() {
    // Picker is rarely initialized in unit tests, so the fallback path is the
    // primary contract we want to lock down: it must never recommend more rows
    // than `max_width` permits while preserving aspect ratio.
    let max_width = 96u16;

    // Wide diagram (4:1) - height should be small.
    let h_wide = super::estimate_image_height(800, 200, max_width);
    assert!(
        h_wide > 0 && h_wide <= 30,
        "wide diagram height should be modest, got {h_wide}"
    );

    // Tall diagram (1:3) - fallback caps at 30 rows, so verify the cap holds.
    let h_tall = super::estimate_image_height(200, 600, max_width);
    assert!(
        h_tall > 0 && h_tall <= 30,
        "tall diagram height should respect the conservative cap, got {h_tall}"
    );

    // Square diagram.
    let h_square = super::estimate_image_height(400, 400, max_width);
    assert!(
        h_square > 0 && h_square <= 30,
        "square diagram height should be reasonable, got {h_square}"
    );
}

/// Compute the rendered cell area for a PNG of `(img_w_px, img_h_px)` shown
/// inside `(area_w_cells, area_h_cells)` using `Resize::Fit`, given a font
/// cell of `(font_w_px, font_h_px)`. Mirrors ratatui-image's Fit behaviour.
fn fit_rendered_cells(
    img_w_px: u32,
    img_h_px: u32,
    area_w_cells: u16,
    area_h_cells: u16,
    font_w_px: u16,
    font_h_px: u16,
) -> (u16, u16) {
    let area_w_px = area_w_cells as f32 * font_w_px as f32;
    let area_h_px = area_h_cells as f32 * font_h_px as f32;
    let scale = (area_w_px / img_w_px as f32).min(area_h_px / img_h_px as f32);
    let rendered_w_px = img_w_px as f32 * scale;
    let rendered_h_px = img_h_px as f32 * scale;
    let w_cells = (rendered_w_px / font_w_px as f32).ceil() as u16;
    let h_cells = (rendered_h_px / font_h_px as f32).ceil() as u16;
    (w_cells.min(area_w_cells), h_cells.min(area_h_cells))
}

#[test]
fn fit_mode_never_overflows_placeholder_height() {
    // Three aspect-ratio classes that previously triggered bottom-crop under
    // Resize::Crop. With Resize::Fit, the rendered cell coverage must stay
    // within the placeholder area for every variant.
    let cases: &[(u32, u32, &str)] = &[
        (800, 200, "wide"),
        (200, 800, "tall"),
        (400, 400, "square"),
        (1200, 900, "4:3"),
        (1920, 1080, "16:9"),
    ];

    let font = (8u16, 16u16);
    let area_w = 118u16; // content_area.width - BORDER_WIDTH
    let area_h = 24u16; // a typical placeholder height

    for (img_w, img_h, label) in cases {
        let (w_cells, h_cells) = fit_rendered_cells(*img_w, *img_h, area_w, area_h, font.0, font.1);
        assert!(
            w_cells <= area_w,
            "{label}: fit width {w_cells} must not exceed area width {area_w}"
        );
        assert!(
            h_cells <= area_h,
            "{label}: fit height {h_cells} must not exceed area height {area_h} (this was the bottom-crop bug)"
        );
    }
}

#[test]
fn placeholder_marker_and_height_roundtrip_through_prepared_region_scan() {
    // The prepared-frame scan in ui_prepare.rs locates inline mermaid regions
    // by detecting the marker line then counting empty lines that follow. This
    // test simulates that scan to confirm the placeholder lines produced by
    // `result_to_lines` (when an image protocol is available) correctly encode
    // their full height into the region detection path.
    use ratatui::text::Line;

    let hash = 0xfeedfacedeadbeefu64;
    let height = 17u16;

    let lines: Vec<Line<'static>> = super::content_render::image_widget_placeholder(hash, height);
    assert_eq!(
        lines.len(),
        height as usize,
        "placeholder must reserve exactly `height` rows for the image region"
    );

    // Marker line.
    let parsed = super::parse_image_placeholder(&lines[0]).expect("first line should encode hash");
    assert_eq!(parsed, hash);

    // Following lines must be empty so the region scan sees them as
    // continuation rows.
    for (i, line) in lines.iter().enumerate().skip(1) {
        let is_empty = line.spans.is_empty()
            || (line.spans.len() == 1 && line.spans[0].content.is_empty());
        assert!(is_empty, "line {i} must be empty (got {:?})", line.spans);
    }
}

#[test]
fn fit_renderer_returns_zero_for_uncached_hash() {
    // Without a cache entry, the inline-fit path must report zero rows so
    // ui_viewport can render its fallback string instead of leaving stale
    // pixels on screen.
    let area = ratatui::prelude::Rect::new(0, 0, 40, 10);
    let mut buf = ratatui::buffer::Buffer::empty(area);

    let rows = super::render_image_widget_fit(0xdeadbeef_cafef00d, area, &mut buf, true, true);
    assert_eq!(rows, 0, "uncached fit render must return 0 to trigger fallback");
}
