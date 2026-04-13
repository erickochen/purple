use super::build_update_label;
use crate::app::GroupBy;

#[test]
fn label_fits_fully() {
    let label = build_update_label("2.7.0", Some("New feature"), "purple update", 80);
    assert_eq!(label, " v2.7.0: New feature (run purple update) ");
}

#[test]
fn label_no_headline() {
    let label = build_update_label("2.7.0", None, "purple update", 80);
    assert_eq!(label, " v2.7.0 available, run purple update ");
}

#[test]
fn label_truncates_at_various_widths() {
    use unicode_width::UnicodeWidthStr;

    let hl = "Provider metadata uses provider-specific terminology (instance, vm_size, zone, location, image, specs)";
    let hint = "purple update";
    let full = " v2.7.0: Provider metadata uses provider-specific terminology (instance, vm_size, zone, location, image, specs) (run purple update) ";

    // Full label is 132 display columns; budget = width - 4
    assert_eq!(full.width(), 132);

    // 136+ cols: fits fully (budget >= 132)
    assert_eq!(build_update_label("2.7.0", Some(hl), hint, 136), full);

    // 80 cols: budget 76, headline truncated with ellipsis
    let label_80 = build_update_label("2.7.0", Some(hl), hint, 80);
    assert!(
        label_80.contains('\u{2026}'),
        "Should contain ellipsis: {}",
        label_80
    );
    assert!(label_80.contains("(run purple update)"));
    assert!(
        label_80.width() <= 76,
        "Should fit in budget: width={}",
        label_80.width()
    );

    // 60 cols: budget 56, headline truncated further
    let label_60 = build_update_label("2.7.0", Some(hl), hint, 60);
    assert!(label_60.contains('\u{2026}'));
    assert!(label_60.contains("(run purple update)"));
    assert!(
        label_60.width() <= 56,
        "Should fit in budget: width={}",
        label_60.width()
    );

    // Verify progressive truncation
    assert!(label_60.width() < label_80.width());

    // 30 cols: not enough room for headline, falls back to version-only
    assert_eq!(
        build_update_label("2.7.0", Some(hl), hint, 30),
        " v2.7.0 available, run purple update "
    );
}

#[test]
fn label_falls_back_when_very_narrow() {
    let label = build_update_label("2.7.0", Some("Headline"), "purple update", 30);
    assert_eq!(label, " v2.7.0 available, run purple update ");
}

#[test]
fn label_brew_hint() {
    let label = build_update_label(
        "2.7.0",
        Some("Fix"),
        "brew upgrade erickochen/purple/purple",
        80,
    );
    assert_eq!(
        label,
        " v2.7.0: Fix (run brew upgrade erickochen/purple/purple) "
    );
}

#[test]
fn label_zero_width() {
    let label = build_update_label("2.7.0", Some("Headline"), "purple update", 0);
    assert_eq!(label, " v2.7.0 available, run purple update ");
}

// =========================================================================
// Columns tests
// =========================================================================

use super::{Columns, HOST_MIN, MARKER_WIDTH, footer_spans, pattern_footer_spans};

#[test]
fn test_padded_zero() {
    assert_eq!(Columns::padded(0), 0);
}

#[test]
fn test_padded_nonzero() {
    // padded(10) = 10 + 10/10 + 1 = 12
    assert_eq!(Columns::padded(10), 12);
}

#[test]
fn test_columns_collapse_priority_last_then_tags_then_address() {
    // Set up widths that are too wide for content area.
    // LAST should be hidden first, then TAGS, then ADDRESS.
    // left = MARKER(2) + 1 + status(2) + alias(padded 12) + gap(2) + host(padded 23) = 42
    // right = tags(padded 12) + gap(2) + history(padded 7) = 21
    // total = 42 + 2 + 21 = 65. At 60, history hides but tags still fit (42+2+12=56).
    let cols = Columns::compute(
        10, // alias_w
        20, // host_w
        10, // tags_w
        6,  // history_w
        60, // narrow enough to hide LAST but keep TAGS
        false,
    );
    assert_eq!(
        cols.history, 0,
        "LAST should be hidden first when too narrow"
    );
    assert!(
        cols.tags > 0,
        "Tags should still be present after LAST is hidden"
    );
}

#[test]
fn test_columns_compute_flex_gap() {
    let cols = Columns::compute(
        10,  // alias_w
        15,  // host_w
        8,   // tags_w
        5,   // history_w
        200, // wide content
        false,
    );
    assert!(
        cols.flex_gap > 0,
        "flex_gap should be positive with wide content"
    );
    // Total consumed should not exceed content width
    let gap = if 200 >= 120 { 3 } else { 2 };
    let left = MARKER_WIDTH + 1 + 2 + cols.alias + gap + cols.host; // +2 for status indicator
    let mut right = 0;
    if cols.tags > 0 {
        right += cols.tags;
    }
    if cols.history > 0 {
        right += cols.history;
    }
    // Count gaps between right-cluster columns
    let right_cols = [cols.tags > 0, cols.history > 0]
        .iter()
        .filter(|&&b| b)
        .count();
    let right_gaps = if right_cols > 1 {
        (right_cols - 1) * gap
    } else {
        0
    };
    // flex_gap fills the remaining space
    assert_eq!(
        cols.flex_gap,
        200usize.saturating_sub(left + right + right_gaps)
    );
}

#[test]
fn test_columns_compute_host_shrinks() {
    // Narrow content: host shrinks but stays >= HOST_MIN.
    // left = MARKER(2) + 1 + status(2) + alias(padded 9) + gap(2) + host(padded 34) = 50
    // No right columns, so nothing to hide. Host won't be hidden since
    // left without host (14) + gap(2) + rw(0) = 14 < 40, but total with host = 50 > 40.
    // The shrink path reduces host by (50-40)=10, from 34 to 24 (>= HOST_MIN).
    let cols = Columns::compute(
        8,  // alias_w
        30, // host_w — should shrink
        0,  // no tags
        0,  // no history
        40, // narrow enough to shrink host, but not hide it
        false,
    );
    assert!(
        cols.host >= HOST_MIN,
        "Host should stay >= HOST_MIN ({}), got {}",
        HOST_MIN,
        cols.host
    );
    assert!(
        cols.host < 34,
        "Host should have shrunk from padded value (34), got {}",
        cols.host
    );
}

#[test]
fn test_footer_no_grouped_indicator() {
    // "grouped" indicator was removed (redundant with group bar)
    let spans = footer_spans(false, false);
    let text: String = spans.iter().map(|s| s.content.to_string()).collect();
    assert!(
        !text.contains("grouped"),
        "Footer should NOT contain 'grouped' indicator, got: {}",
        text
    );
}

#[test]
fn footer_shows_core_actions() {
    let spans = footer_spans(false, false);
    let text: String = spans.iter().map(|s| s.content.to_string()).collect();
    assert!(text.contains("Enter"));
    assert!(text.contains("connect"));
    assert!(text.contains("/"));
    assert!(text.contains("search"));
    assert!(text.contains("#"));
    assert!(text.contains("tag"));
    assert!(text.contains("v"));
}

#[test]
fn footer_view_label_detail_when_compact() {
    let spans = footer_spans(false, false);
    let text: String = spans.iter().map(|s| s.content.to_string()).collect();
    assert!(text.contains("detail"));
}

#[test]
fn footer_view_label_compact_when_detail() {
    let spans = footer_spans(true, false);
    let text: String = spans.iter().map(|s| s.content.to_string()).collect();
    assert!(text.contains("compact"));
}

#[test]
fn footer_down_only_indicator() {
    let spans = footer_spans(false, true);
    let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("DOWN ONLY"));
}

#[test]
fn brand_label_purple_when_ungrouped_hosts_when_grouped() {
    use super::brand_label_for_group;
    assert_eq!(brand_label_for_group(&GroupBy::None), " purple ");
    assert_eq!(brand_label_for_group(&GroupBy::Provider), " HOSTS ");
    assert_eq!(
        brand_label_for_group(&GroupBy::Tag("env".to_string())),
        " HOSTS "
    );
}

#[test]
fn pattern_footer_shows_core_actions() {
    let spans = pattern_footer_spans(false);
    let text: String = spans.iter().map(|s| s.content.to_string()).collect();
    assert!(text.contains("/"));
    assert!(text.contains("search"));
    assert!(text.contains("#"));
    assert!(text.contains("v"));
}

#[test]
fn pattern_footer_detail_label_when_compact() {
    let spans = pattern_footer_spans(false);
    let text: String = spans.iter().map(|s| s.content.to_string()).collect();
    assert!(text.contains("detail"));
}

#[test]
fn layout_has_group_bar_and_footer() {
    use ratatui::layout::{Constraint, Layout, Rect};
    let area = Rect::new(0, 0, 120, 40);
    // Matches render() layout when grouping is active and not searching
    let chunks = Layout::vertical([
        Constraint::Length(3), // Group bar
        Constraint::Min(5),    // Host list
        Constraint::Length(1), // Footer
    ])
    .split(area);
    assert_eq!(chunks[0].height, 3, "group bar should be 3 rows");
    assert_eq!(chunks[2].height, 1, "footer should be 1 row");
    assert!(chunks[2].y > chunks[1].y + chunks[1].height - 1);
}

#[test]
fn layout_no_group_bar_when_ungrouped() {
    use ratatui::layout::{Constraint, Layout, Rect};
    let area = Rect::new(0, 0, 120, 40);
    // Matches render() layout when GroupBy::None (group_bar_height = 0)
    let chunks = Layout::vertical([
        Constraint::Length(0), // Group bar hidden
        Constraint::Min(5),    // Host list
        Constraint::Length(1), // Footer
    ])
    .split(area);
    assert_eq!(chunks[0].height, 0, "group bar should be hidden");
    assert_eq!(
        chunks[1].height, 39,
        "host list should get all remaining rows"
    );
}

#[test]
fn layout_with_search_has_group_bar() {
    use ratatui::layout::{Constraint, Layout, Rect};
    let area = Rect::new(0, 0, 120, 40);
    // Matches render() layout when grouping is active and searching
    let chunks = Layout::vertical([
        Constraint::Length(3), // Group bar
        Constraint::Min(5),    // Host list
        Constraint::Length(1), // Search bar
        Constraint::Length(1), // Footer
    ])
    .split(area);
    assert_eq!(chunks[0].height, 3, "group bar should be 3 rows");
    assert_eq!(chunks[2].height, 1, "search bar should be 1 row");
    assert_eq!(chunks[3].height, 1, "footer should be 1 row");
}

// =========================================================================
// Column hide priority tests
// =========================================================================

#[test]
fn columns_hide_full_priority_chain() {
    // Wide enough for everything
    let cols_wide = Columns::compute(10, 15, 8, 5, 200, false);
    assert!(cols_wide.history > 0, "history visible at 200");
    assert!(cols_wide.tags > 0, "tags visible at 200");
    assert!(cols_wide.host > 0, "host visible at 200");

    // Progressively narrower: LAST (history) hides first
    let cols_no_history = Columns::compute(10, 15, 8, 5, 50, false);
    assert_eq!(cols_no_history.history, 0, "history should hide first");

    // Narrower still: TAGS hides next
    let cols_no_tags = Columns::compute(10, 15, 8, 5, 40, false);
    assert_eq!(cols_no_tags.history, 0, "history still hidden");
    assert_eq!(cols_no_tags.tags, 0, "tags should hide second");

    // Extremely narrow: ADDRESS hides last
    let cols_no_host = Columns::compute(10, 15, 8, 5, 20, false);
    assert_eq!(cols_no_host.history, 0);
    assert_eq!(cols_no_host.tags, 0);
    assert_eq!(cols_no_host.host, 0, "host should hide last");
}

#[test]
fn columns_detail_mode_no_host() {
    let cols = Columns::compute(10, 15, 8, 5, 200, true);
    assert_eq!(cols.host, 0, "host should be 0 in detail_mode");
    assert!(cols.detail_mode, "detail_mode flag should be set");
    assert!(cols.tags > 0, "tags visible in detail_mode");
    assert!(cols.history > 0, "history visible in detail_mode");
}

#[test]
fn format_rtt_millis() {
    assert_eq!(super::format_rtt(42), "42ms");
}

#[test]
fn format_rtt_zero() {
    assert_eq!(super::format_rtt(0), "0ms");
}

#[test]
fn format_rtt_boundary_999() {
    assert_eq!(super::format_rtt(999), "999ms");
}

#[test]
fn format_rtt_boundary_1000() {
    assert_eq!(super::format_rtt(1000), "1.0s");
}

#[test]
fn format_rtt_seconds() {
    assert_eq!(super::format_rtt(1500), "1.5s");
}

#[test]
fn format_rtt_capped() {
    assert_eq!(super::format_rtt(12000), "10s+");
}

#[test]
fn format_rtt_boundary_9949() {
    assert_eq!(super::format_rtt(9949), "9.9s");
}

#[test]
fn format_rtt_boundary_9950() {
    assert_eq!(super::format_rtt(9950), "10s+");
}

#[test]
fn format_rtt_boundary_10000() {
    assert_eq!(super::format_rtt(10000), "10s+");
}

#[test]
fn format_rtt_u32_max() {
    assert_eq!(super::format_rtt(u32::MAX), "10s+");
}

// =========================================================================
// composite_host_label tests
// =========================================================================

#[test]
fn composite_host_label_hostname_only() {
    let host = crate::ssh_config::model::HostEntry {
        hostname: "example.com".to_string(),
        port: 22,
        ..Default::default()
    };
    assert_eq!(super::composite_host_label(&host), "example.com");
}

#[test]
fn composite_host_label_non_default_port() {
    let host = crate::ssh_config::model::HostEntry {
        hostname: "example.com".to_string(),
        port: 2222,
        ..Default::default()
    };
    assert_eq!(super::composite_host_label(&host), "example.com:2222");
}

#[test]
fn composite_host_label_no_user_prefix() {
    // User field is set but composite_host_label should NOT include user@
    let host = crate::ssh_config::model::HostEntry {
        hostname: "example.com".to_string(),
        user: "admin".to_string(),
        port: 22,
        ..Default::default()
    };
    let label = super::composite_host_label(&host);
    assert!(
        !label.contains('@'),
        "composite label should not include user@"
    );
    assert_eq!(label, "example.com");
}

// composite_host_width tests (allocation-free path)

#[test]
fn composite_host_width_default_port() {
    let host = crate::ssh_config::model::HostEntry {
        hostname: "example.com".to_string(),
        port: 22,
        ..Default::default()
    };
    assert_eq!(super::composite_host_width(&host), "example.com".len());
}

#[test]
fn composite_host_width_non_default_port() {
    let host = crate::ssh_config::model::HostEntry {
        hostname: "example.com".to_string(),
        port: 2222,
        ..Default::default()
    };
    // "example.com" (11) + ":" (1) + "2222" (4) = 16
    assert_eq!(super::composite_host_width(&host), 16);
}

#[test]
fn composite_host_width_port_zero() {
    let host = crate::ssh_config::model::HostEntry {
        hostname: "host".to_string(),
        port: 0,
        ..Default::default()
    };
    // "host" (4) + ":" (1) + "0" (1) = 6
    assert_eq!(super::composite_host_width(&host), 6);
}

#[test]
fn composite_host_width_port_max() {
    let host = crate::ssh_config::model::HostEntry {
        hostname: "h".to_string(),
        port: 65535,
        ..Default::default()
    };
    // "h" (1) + ":" (1) + "65535" (5) = 7
    assert_eq!(super::composite_host_width(&host), 7);
}

// =========================================================================
// Columns detail_mode collapse priority tests
// =========================================================================

#[test]
fn columns_detail_mode_collapse_priority() {
    // detail_mode=true, progressively narrower
    // LAST hides first, then TAGS (ADDRESS already 0)
    let cols_wide = Columns::compute(10, 15, 8, 5, 100, true);
    assert_eq!(cols_wide.host, 0, "detail_mode: no host");
    assert!(cols_wide.tags > 0, "tags visible at 100");
    assert!(cols_wide.history > 0, "history visible at 100");

    // Narrow: LAST hides first
    let cols_narrow = Columns::compute(10, 15, 8, 5, 25, true);
    assert_eq!(cols_narrow.host, 0);
    assert_eq!(
        cols_narrow.history, 0,
        "history should hide first in detail_mode"
    );

    // Very narrow: TAGS hides next
    let cols_very_narrow = Columns::compute(10, 15, 8, 5, 18, true);
    assert_eq!(cols_very_narrow.host, 0);
    assert_eq!(cols_very_narrow.history, 0);
    assert_eq!(cols_very_narrow.tags, 0, "tags should hide after history");
}

// --- tag_bar_spans tests ---

#[test]
fn tag_bar_empty_input_shows_cursor_then_placeholder() {
    let spans = super::tag_bar_spans("", &[]);
    let texts: Vec<&str> = spans.iter().map(|s| s.content.as_ref()).collect();
    // " tags: " then cursor "_" then placeholder hint
    assert_eq!(texts[0], " tags: ");
    assert_eq!(texts[1], "_");
    assert!(texts[2].contains("e.g."));
}

#[test]
fn tag_bar_with_input_shows_input_then_cursor() {
    let spans = super::tag_bar_spans("prod, staging", &[]);
    let texts: Vec<&str> = spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(texts[0], " tags: ");
    assert_eq!(texts[1], "prod, staging");
    assert_eq!(texts[2], "_");
    assert_eq!(texts.len(), 3);
}

#[test]
fn tag_bar_with_provider_tags_shows_prefix() {
    let ptags = vec!["cloud".to_string(), "eu".to_string()];
    let spans = super::tag_bar_spans("web", &ptags);
    let texts: Vec<&str> = spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(texts[0], " tags: ");
    assert_eq!(texts[1], "[cloud, eu] ");
    assert_eq!(texts[2], "web");
    assert_eq!(texts[3], "_");
}
