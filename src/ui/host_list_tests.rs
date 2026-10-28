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
        0,  // host_min_w (no IPs)
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
        0,   // host_min_w (no IPs)
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
        0,  // host_min_w (no IPs)
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
fn test_columns_host_floor_respects_ip_min_width() {
    // Same scenario as the shrink test (host_w=30, content=40) but with an
    // IPv6 host present (host_min_w=39). The shrink pass would normally cut
    // host to 24, but host_floor = max(host_min_w, HOST_MIN) = 39, so the
    // shrink branch must hide the column entirely instead of yielding a
    // truncated IP that the user cannot copy.
    let cols = Columns::compute(
        8,  // alias_w
        30, // host_w (desired)
        39, // host_min_w — widest IP in the list (full IPv6)
        0,  // no tags
        0,  // no history
        40, // narrow content
        false,
    );
    assert_eq!(
        cols.host, 0,
        "host column must hide rather than truncate an IP below its full width"
    );
}

#[test]
fn test_columns_host_min_w_zero_falls_back_to_host_min() {
    // No IPs in the list → host_min_w=0. Column must still shrink down
    // to the legacy HOST_MIN=12 floor (not below), preserving the pre-
    // feature behaviour for DNS-only host lists.
    let cols = Columns::compute(
        8,  // alias_w
        30, // host_w
        0,  // host_min_w — no IPs
        0,  // tags
        0,  // history
        40, // narrow
        false,
    );
    assert!(
        cols.host >= HOST_MIN,
        "host column must respect HOST_MIN floor when no IPs present, got {}",
        cols.host
    );
}

#[test]
fn test_columns_host_keeps_ip_floor_at_intermediate_width() {
    // Plenty of room for the full host_w (padded ~33) but if we pin host_min_w
    // higher than the desired padded width, the column should grow to fit.
    let cols = Columns::compute(8, 15, 20, 0, 0, 200, false);
    assert!(
        cols.host >= 20,
        "host column must be >= host_min_w ({}), got {}",
        20,
        cols.host
    );
}

#[test]
fn test_footer_no_grouped_indicator() {
    // "grouped" indicator was removed (redundant with group bar)
    let spans = footer_spans(false, false, false);
    let text: String = spans.iter().map(|s| s.content.to_string()).collect();
    assert!(
        !text.contains("grouped"),
        "Footer should NOT contain 'grouped' indicator, got: {}",
        text
    );
}

#[test]
fn footer_shows_core_actions() {
    let spans = footer_spans(false, false, false);
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
    let spans = footer_spans(false, false, false);
    let text: String = spans.iter().map(|s| s.content.to_string()).collect();
    assert!(text.contains("detail"));
}

#[test]
fn footer_view_label_compact_when_detail() {
    let spans = footer_spans(true, false, false);
    let text: String = spans.iter().map(|s| s.content.to_string()).collect();
    assert!(text.contains("compact"));
}

#[test]
fn footer_down_only_indicator() {
    let spans = footer_spans(false, true, false);
    let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("DOWN ONLY"));
}

#[test]
fn footer_selection_active_replaces_view_hints_with_bulk_actions() {
    let spans = footer_spans(false, false, true);
    let text: String = spans.iter().map(|s| s.content.to_string()).collect();
    assert!(text.contains("bulk tag"), "missing bulk tag hint: {text}");
    assert!(text.contains(" run "), "missing run hint: {text}");
    assert!(text.contains("clear"), "missing clear hint: {text}");
    assert!(text.contains("help"), "missing help hint: {text}");
    // Confirm we drop the noisy `:` and `v` keys to keep the line short.
    assert!(
        !text.contains(" cmds "),
        "selection footer should drop cmds"
    );
    assert!(
        !text.contains(" detail "),
        "selection footer should drop view label"
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
fn layout_has_top_bar_and_footer() {
    use ratatui::layout::{Constraint, Layout, Rect};
    let area = Rect::new(0, 0, 120, 40);
    // Matches render() layout: top bar always visible, no search/tag bar.
    let chunks = Layout::vertical([
        Constraint::Length(3), // Top navigation bar
        Constraint::Min(5),    // Host list
        Constraint::Length(1), // Footer
    ])
    .split(area);
    assert_eq!(chunks[0].height, 3, "top bar should be 3 rows");
    assert_eq!(chunks[2].height, 1, "footer should be 1 row");
    assert!(chunks[2].y > chunks[1].y + chunks[1].height - 1);
}

#[test]
fn layout_with_search_has_top_bar() {
    use ratatui::layout::{Constraint, Layout, Rect};
    let area = Rect::new(0, 0, 120, 40);
    // Matches render() layout when searching: top bar still visible.
    let chunks = Layout::vertical([
        Constraint::Length(3), // Top navigation bar
        Constraint::Min(5),    // Host list
        Constraint::Length(1), // Search bar
        Constraint::Length(1), // Footer
    ])
    .split(area);
    assert_eq!(chunks[0].height, 3, "top bar should be 3 rows");
    assert_eq!(chunks[2].height, 1, "search bar should be 1 row");
    assert_eq!(chunks[3].height, 1, "footer should be 1 row");
}

// =========================================================================
// Column hide priority tests
// =========================================================================

#[test]
fn columns_hide_full_priority_chain() {
    // Wide enough for everything
    let cols_wide = Columns::compute(10, 15, 0, 8, 5, 200, false);
    assert!(cols_wide.history > 0, "history visible at 200");
    assert!(cols_wide.tags > 0, "tags visible at 200");
    assert!(cols_wide.host > 0, "host visible at 200");

    // Progressively narrower: LAST (history) hides first
    let cols_no_history = Columns::compute(10, 15, 0, 8, 5, 50, false);
    assert_eq!(cols_no_history.history, 0, "history should hide first");

    // Narrower still: TAGS hides next
    let cols_no_tags = Columns::compute(10, 15, 0, 8, 5, 40, false);
    assert_eq!(cols_no_tags.history, 0, "history still hidden");
    assert_eq!(cols_no_tags.tags, 0, "tags should hide second");

    // Extremely narrow: ADDRESS hides last
    let cols_no_host = Columns::compute(10, 15, 0, 8, 5, 20, false);
    assert_eq!(cols_no_host.history, 0);
    assert_eq!(cols_no_host.tags, 0);
    assert_eq!(cols_no_host.host, 0, "host should hide last");
}

#[test]
fn columns_detail_mode_no_host() {
    let cols = Columns::compute(10, 15, 0, 8, 5, 200, true);
    assert_eq!(cols.host, 0, "host should be 0 in detail_mode");
    assert!(cols.detail_mode, "detail_mode flag should be set");
    assert!(cols.tags > 0, "tags visible in detail_mode");
    assert!(cols.history > 0, "history visible in detail_mode");
}

#[test]
fn columns_alias_grows_beyond_32_on_wide_terminal() {
    // Reporter (issue #67): long aliases were clamped to 32 cols even on
    // wide terminals. With ample room, the alias column must scale to its
    // full padded content width and never truncate.
    let cols = Columns::compute(60, 9, 0, 0, 0, 200, false);
    assert_eq!(
        cols.alias,
        Columns::padded(60),
        "alias column must grow to full padded width on a wide terminal, got {}",
        cols.alias
    );
    assert!(
        cols.alias > 32,
        "alias must be able to exceed the legacy 32-cap, got {}",
        cols.alias
    );
}

#[test]
fn columns_alias_full_when_other_columns_drop() {
    // 60-char alias plus address, tags and history on a 90-col terminal.
    // The fit-pass should drop history/tags (and shrink/hide host) so the
    // alias keeps its full content width.
    let cols = Columns::compute(60, 9, 0, 4, 6, 90, false);
    assert_eq!(
        cols.alias,
        Columns::padded(60),
        "alias must keep full width when right cluster can collapse to make room"
    );
}

#[test]
fn columns_alias_shrinks_when_terminal_cannot_fit_full_width() {
    // 60-char alias on a 50-col terminal, no other columns. After everything
    // else has collapsed the alias itself must shrink so the row still fits.
    // Without an alias shrink step this would overflow the row.
    let cols = Columns::compute(60, 0, 0, 0, 0, 50, false);
    // Available for alias: content - marker(2) - highlight(1) - status(2) = 45
    assert!(
        cols.alias <= 45,
        "alias must shrink to fit available width, got {}",
        cols.alias
    );
}

#[test]
fn columns_alias_shrinks_in_detail_mode_on_very_narrow_terminal() {
    // detail_mode forces host=0, so the fit-pass for alias must operate
    // without a host segment. With a 40-char alias on a 15-col terminal
    // (no tags or history), the alias must shrink below its padded width.
    let cols = Columns::compute(40, 0, 0, 0, 0, 15, true);
    assert_eq!(cols.host, 0, "detail_mode forces host to 0");
    assert!(
        cols.alias < Columns::padded(40),
        "alias must shrink in detail_mode when terminal is too narrow, got {}",
        cols.alias
    );
}

#[test]
fn columns_short_alias_keeps_floor_of_8() {
    // 3-char alias: padded(3)=4, but the column floor is 8 so aliases stay
    // readable even when the SSH config contains very short labels.
    let cols = Columns::compute(3, 0, 0, 0, 0, 200, false);
    assert_eq!(
        cols.alias, 8,
        "alias column must keep an 8-col floor for short aliases, got {}",
        cols.alias
    );
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

// composite_host_width_if_ip tests

#[test]
fn composite_host_width_if_ip_bare_ipv4() {
    let host = crate::ssh_config::model::HostEntry {
        hostname: "192.168.0.100".to_string(),
        port: 22,
        ..Default::default()
    };
    assert_eq!(super::composite_host_width_if_ip(&host), 13);
}

#[test]
fn composite_host_width_if_ip_bare_ipv6() {
    let host = crate::ssh_config::model::HostEntry {
        hostname: "2001:db8::1".to_string(),
        port: 22,
        ..Default::default()
    };
    assert_eq!(super::composite_host_width_if_ip(&host), 11);
}

#[test]
fn composite_host_width_if_ip_bracketed_ipv6() {
    // OpenSSH requires brackets around an IPv6 literal in HostName when
    // a non-default port is present. IpAddr::parse rejects brackets, so
    // the helper must strip them before parsing — otherwise the host
    // column will truncate a full IPv6 literal that the user cannot copy.
    let host = crate::ssh_config::model::HostEntry {
        hostname: "[2001:db8::1]".to_string(),
        port: 22,
        ..Default::default()
    };
    assert_eq!(
        super::composite_host_width_if_ip(&host),
        "[2001:db8::1]".len(),
        "bracketed IPv6 must be recognised as an IP"
    );
}

#[test]
fn composite_host_width_if_ip_bracketed_ipv6_loopback() {
    let host = crate::ssh_config::model::HostEntry {
        hostname: "[::1]".to_string(),
        port: 22,
        ..Default::default()
    };
    assert_eq!(
        super::composite_host_width_if_ip(&host),
        5,
        "[::1] must be recognised as an IP"
    );
}

#[test]
fn composite_host_width_if_ip_dns_returns_zero() {
    // DNS hostnames must remain shrinkable — helper returns 0 so they do
    // not inflate the column floor.
    let host = crate::ssh_config::model::HostEntry {
        hostname: "web-01.prod.example.com".to_string(),
        port: 22,
        ..Default::default()
    };
    assert_eq!(super::composite_host_width_if_ip(&host), 0);
}

#[test]
fn composite_host_width_if_ip_garbage_brackets_return_zero() {
    // Only well-formed bracketed IPv6 counts. Brackets around garbage
    // must not accidentally pass for an IP.
    let host = crate::ssh_config::model::HostEntry {
        hostname: "[not-an-ip]".to_string(),
        port: 22,
        ..Default::default()
    };
    assert_eq!(super::composite_host_width_if_ip(&host), 0);
}

// =========================================================================
// Columns detail_mode collapse priority tests
// =========================================================================

#[test]
fn columns_detail_mode_collapse_priority() {
    // detail_mode=true, progressively narrower
    // LAST hides first, then TAGS (ADDRESS already 0)
    let cols_wide = Columns::compute(10, 15, 0, 8, 5, 100, true);
    assert_eq!(cols_wide.host, 0, "detail_mode: no host");
    assert!(cols_wide.tags > 0, "tags visible at 100");
    assert!(cols_wide.history > 0, "history visible at 100");

    // Narrow: LAST hides first
    let cols_narrow = Columns::compute(10, 15, 0, 8, 5, 25, true);
    assert_eq!(cols_narrow.host, 0);
    assert_eq!(
        cols_narrow.history, 0,
        "history should hide first in detail_mode"
    );

    // Very narrow: TAGS hides next
    let cols_very_narrow = Columns::compute(10, 15, 0, 8, 5, 18, true);
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

// =========================================================================
// build_host_item detail-mode indicator tests
// =========================================================================

use super::{HostItemContext, build_host_item};
use crate::ssh_config::model::HostEntry;

fn detail_columns(alias: usize) -> Columns {
    Columns {
        alias,
        host: 0,
        tags: 0,
        history: 0,
        gap: 2,
        flex_gap: 0,
        detail_mode: true,
    }
}

fn full_columns(alias: usize, host: usize) -> Columns {
    Columns {
        alias,
        host,
        tags: 0,
        history: 0,
        gap: 2,
        flex_gap: 0,
        detail_mode: false,
    }
}

/// Render a ListItem to a plain string via a ratatui List + Buffer.
fn render_item_to_string(item: ratatui::widgets::ListItem<'_>, width: u16) -> String {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::widgets::{List, Widget};
    let area = Rect::new(0, 0, width, 1);
    let mut buf = Buffer::empty(area);
    let list = List::new(vec![item]);
    list.render(area, &mut buf);
    let mut s = String::new();
    for x in 0..width {
        let cell = &buf[(x, 0)];
        s.push_str(cell.symbol());
    }
    s
}

fn make_ctx<'a>(cols: &'a Columns, tunnel_active: bool, detail_mode: bool) -> HostItemContext<'a> {
    let ping = Box::leak(Box::new(std::collections::HashMap::new()));
    let history = Box::leak(Box::new(crate::history::ConnectionHistory::default()));
    HostItemContext {
        ping_status: ping,
        history,
        tunnel_active,
        query: None,
        cols,
        multi_selected: false,
        group_by: &GroupBy::None,
        detail_mode,
        spinner_tick: 0,
    }
}

#[test]
fn detail_mode_no_indicators() {
    let cols = detail_columns(20);
    let host = HostEntry {
        alias: "web-server".into(),
        ..Default::default()
    };
    let ctx = make_ctx(&cols, false, true);
    let item = build_host_item(&host, &ctx);
    let rendered = render_item_to_string(item, 30);
    assert!(rendered.contains("web-server"));
    assert!(!rendered.contains('\u{2197}'), "no jump indicator expected");
    assert!(
        !rendered.contains('\u{21C4}'),
        "no tunnel indicator expected"
    );
}

#[test]
fn detail_mode_jump_indicator_visible() {
    let cols = detail_columns(20);
    let host = HostEntry {
        alias: "bastion".into(),
        proxy_jump: "gateway".into(),
        ..Default::default()
    };
    let ctx = make_ctx(&cols, false, true);
    let item = build_host_item(&host, &ctx);
    let rendered = render_item_to_string(item, 30);
    assert!(rendered.contains("bastion"));
    assert!(rendered.contains('\u{2197}'), "jump indicator missing");
    assert!(
        !rendered.contains('\u{21C4}'),
        "tunnel indicator should not appear"
    );
}

#[test]
fn detail_mode_tunnel_indicator_visible() {
    let cols = detail_columns(20);
    let host = HostEntry {
        alias: "db-primary".into(),
        tunnel_count: 1,
        ..Default::default()
    };
    let ctx = make_ctx(&cols, false, true);
    let item = build_host_item(&host, &ctx);
    let rendered = render_item_to_string(item, 30);
    assert!(rendered.contains("db-primary"));
    assert!(rendered.contains('\u{21C4}'), "tunnel indicator missing");
    assert!(
        !rendered.contains('\u{2197}'),
        "jump indicator should not appear"
    );
}

#[test]
fn detail_mode_both_indicators_truncate_alias() {
    let cols = detail_columns(20);
    let host = HostEntry {
        alias: "very-long-hostname-here".into(),
        proxy_jump: "gateway".into(),
        tunnel_count: 2,
        ..Default::default()
    };
    let ctx = make_ctx(&cols, false, true);
    let item = build_host_item(&host, &ctx);
    let rendered = render_item_to_string(item, 30);
    assert!(rendered.contains('\u{2197}'), "jump indicator missing");
    assert!(rendered.contains('\u{21C4}'), "tunnel indicator missing");
    // Full alias (22 chars) should be truncated to fit indicators (4 cols) in 20-col budget
    assert!(
        !rendered.contains("very-long-hostname-here"),
        "alias should be truncated"
    );
}

#[test]
fn non_detail_mode_indicators_in_address_column() {
    let cols = full_columns(20, 30);
    let host = HostEntry {
        alias: "bastion".into(),
        hostname: "10.0.0.1".into(),
        proxy_jump: "gateway".into(),
        tunnel_count: 1,
        ..Default::default()
    };
    let ctx = make_ctx(&cols, true, false);
    let item = build_host_item(&host, &ctx);
    let rendered = render_item_to_string(item, 60);
    assert!(rendered.contains('\u{2197}'), "jump indicator missing");
    assert!(rendered.contains('\u{21C4}'), "tunnel indicator missing");
    assert!(
        rendered.contains("10.0.0.1"),
        "hostname should appear in address column"
    );
}

// =========================================================================
// Top navigation bar tests
// =========================================================================

mod top_bar {
    use super::super::top_bar_spans;
    use crate::app::{App, TopPage};
    use crate::ssh_config::model::SshConfigFile;
    use crate::tunnel::{TunnelRule, TunnelType};
    use crate::ui::theme;

    fn make_test_app(content: &str) -> App {
        let scratch = tempfile::tempdir().expect("tempdir").keep();
        let config = SshConfigFile {
            elements: SshConfigFile::parse_content(content),
            path: scratch.join("test_config"),
            crlf: false,
            bom: false,
        };
        App::new(config)
    }

    fn add_tunnel_rule(app: &mut App, bind_port: u16) {
        app.tunnels.list_mut().push(TunnelRule {
            tunnel_type: TunnelType::Local,
            bind_address: String::new(),
            bind_port,
            remote_host: "127.0.0.1".to_string(),
            remote_port: bind_port,
        });
    }

    #[test]
    fn brand_slot_is_first_and_uses_brand_badge() {
        let app = make_test_app("Host web1\n  HostName 1.1.1.1\n");
        let spans = top_bar_spans(&app);
        assert_eq!(
            spans[0].content.as_ref(),
            " purple ",
            "first slot must be the brand label"
        );
        assert_eq!(
            spans[0].style,
            theme::brand_badge(),
            "brand slot must use brand_badge style"
        );
    }

    #[test]
    fn labels_have_no_count_or_padding() {
        let mut app =
            make_test_app("Host web1\n  HostName 1.1.1.1\nHost web2\n  HostName 2.2.2.2\n");
        add_tunnel_rule(&mut app, 8080);
        add_tunnel_rule(&mut app, 8081);

        let spans = top_bar_spans(&app);
        let host_span = spans
            .iter()
            .find(|s| s.content.contains("hosts"))
            .expect("hosts span");
        let tunnels_span = spans
            .iter()
            .find(|s| s.content.contains("tunnels"))
            .expect("tunnels span");

        // Page labels are bare — no surrounding spaces, no parenthesised count.
        assert_eq!(host_span.content.as_ref(), "hosts");
        assert_eq!(tunnels_span.content.as_ref(), "tunnels");
    }

    #[test]
    fn brand_and_first_tab_are_separated_by_a_visible_gap() {
        let app = make_test_app("Host web1\n  HostName 1.1.1.1\n");
        let spans = top_bar_spans(&app);

        let brand_pos = spans
            .iter()
            .position(|s| s.content.contains("purple"))
            .expect("brand span");
        let host_pos = spans
            .iter()
            .position(|s| s.content.as_ref() == "hosts")
            .expect("hosts span");

        assert!(host_pos > brand_pos, "hosts must come after brand");
        let between: usize = spans[brand_pos + 1..host_pos]
            .iter()
            .map(|s| s.content.len())
            .sum();
        assert!(
            between >= 4,
            "need a visible gap between brand badge and first tab; got {between}"
        );
    }

    #[test]
    fn active_page_uses_nav_active_inactive_is_muted() {
        // Same lock order as the mutators below, so the style cannot change
        // between the render and the assertion.
        let _global_test_lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _guard = THEME_CAPS_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let app = make_test_app("Host web1\n  HostName 1.1.1.1\n");
        assert_eq!(app.top_page, TopPage::Hosts);
        let spans = top_bar_spans(&app);

        let host_span = spans
            .iter()
            .find(|s| s.content.contains("hosts"))
            .expect("hosts span");
        let tunnels_span = spans
            .iter()
            .find(|s| s.content.contains("tunnels"))
            .expect("tunnels span");

        assert_eq!(host_span.style, theme::nav_active());
        assert_eq!(tunnels_span.style, theme::muted());
    }

    #[test]
    fn switching_to_tunnels_swaps_active_style() {
        let _global_test_lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _guard = THEME_CAPS_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let mut app = make_test_app("Host web1\n  HostName 1.1.1.1\n");
        app.top_page = TopPage::Tunnels;
        let spans = top_bar_spans(&app);

        let host_span = spans
            .iter()
            .find(|s| s.content.contains("hosts"))
            .expect("hosts span");
        let tunnels_span = spans
            .iter()
            .find(|s| s.content.contains("tunnels"))
            .expect("tunnels span");

        assert_eq!(host_span.style, theme::muted());
        assert_eq!(tunnels_span.style, theme::nav_active());
    }

    #[test]
    fn nav_active_is_underlined_not_a_solid_badge() {
        use ratatui::style::Modifier;
        let _guard = THEME_CAPS_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        theme::set_colored_underline_support(true);
        let style = theme::nav_active();
        assert!(
            style.add_modifier.contains(Modifier::UNDERLINED),
            "active tab must underline"
        );
        assert_ne!(
            style,
            theme::brand_badge(),
            "active tab must not use the brand badge style"
        );
    }

    /// Module-wide lock for tests that flip `theme::COLORED_UNDERLINE` so they
    /// do not race with each other under parallel `cargo test` execution.
    static THEME_CAPS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn nav_active_uses_colored_underline_when_supported() {
        let _global_test_lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _guard = THEME_CAPS_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        // Pin to ANSI 16 so the accent.ansi16 branch is exercised (default in
        // most CI / dev shells) and the assertion is deterministic.
        theme::init_with_mode(1);
        theme::set_colored_underline_support(true);
        let style = theme::nav_active();
        assert!(
            style.underline_color.is_some(),
            "modern terminals: nav_active must set a styled underline color"
        );
        assert!(
            style.fg.is_none(),
            "modern terminals: foreground stays default so the label reads white"
        );
        // Restore the supported default for any unrelated tests that follow.
        theme::set_colored_underline_support(true);
    }

    #[test]
    fn nav_active_falls_back_to_plain_underline_when_unsupported() {
        let _global_test_lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _guard = THEME_CAPS_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        theme::init_with_mode(1);
        theme::set_colored_underline_support(false);
        let style = theme::nav_active();
        assert!(
            style.underline_color.is_none(),
            "legacy terminals: must not emit SGR 58 (no styled underline color)"
        );
        assert!(
            style.fg.is_none(),
            "legacy terminals: leave fg default so the label stays white instead of brand-coloured"
        );
        use ratatui::style::Modifier;
        assert!(
            style.add_modifier.contains(Modifier::UNDERLINED),
            "legacy terminals: keep SGR 4 so the active tab still has an underline"
        );
        // Restore for unrelated tests.
        theme::set_colored_underline_support(true);
    }

    #[test]
    fn nav_active_no_color_mode_skips_accent_in_both_paths() {
        let _global_test_lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _guard = THEME_CAPS_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        theme::init_with_mode(0);
        for supported in [true, false] {
            theme::set_colored_underline_support(supported);
            let style = theme::nav_active();
            assert!(
                style.underline_color.is_none(),
                "NO_COLOR: never emit a styled underline color (cap={supported})"
            );
            assert!(
                style.fg.is_none(),
                "NO_COLOR: never set a fg accent (cap={supported})"
            );
        }
        // Restore mode + cap for unrelated tests.
        theme::init_with_mode(1);
        theme::set_colored_underline_support(true);
    }

    #[test]
    fn no_provider_or_tag_labels_appear_in_top_bar() {
        let content = "\
Host aws-web
  HostName 1.1.1.1
  # purple:provider aws:i-1
  # purple:tags production

Host do-db
  HostName 2.2.2.2
  # purple:provider digitalocean:abc
  # purple:tags staging
";
        let app = make_test_app(content);
        let text: String = top_bar_spans(&app)
            .into_iter()
            .map(|s| s.content.into_owned())
            .collect();

        for legacy in [
            "AWS",
            "PROXMOX",
            "DIGITALOCEAN",
            "production",
            "staging",
            "All (",
        ] {
            assert!(
                !text.contains(legacy),
                "top bar must not contain legacy category {legacy:?}, got: {text}"
            );
        }
    }
}

// The empty Hosts tab names the config file purple reads.
#[test]
fn empty_hosts_tab_names_the_config_file() {
    let mut app = crate::ui::app_with_own_config("");
    let mut anim = crate::animation::AnimationState::default();
    let text = crate::ui::render_to_text(120, 40, |frame| {
        crate::ui::render(frame, &mut app, &mut anim)
    });
    assert!(
        text.contains("~/.ssh/purple_config"),
        "empty state must name the config file: {text}"
    );
    assert!(
        !text.contains("~/.ssh/config"),
        "empty state must not name the default: {text}"
    );
}
