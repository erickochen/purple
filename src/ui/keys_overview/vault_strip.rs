//! Vault SSH strip: one row per active cached cert, with a TTL bar
//! colour-coded on the green/amber/red ramp. Rendered above the hero
//! when at least one host has Vault SSH configured.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::ui::{design, theme};
use crate::vault_ssh::{self, ActiveCert};

use super::card_title;

pub(super) fn active_strip_rows(app: &App) -> Vec<ActiveCert> {
    vault_ssh::active_certs_for_strip(
        app.hosts_state.list(),
        app.vault.cert_cache(),
        app.env().paths(),
    )
}

pub(super) fn render_vault_strip(frame: &mut Frame, area: Rect, rows: &[ActiveCert]) {
    let block = design::main_block_line(card_title(
        "VAULT SSH",
        Some(&format!("{} active", rows.len())),
    ));
    let inner = design::body_area(area);
    frame.render_widget(block, area);

    if rows.is_empty() {
        let line = Line::from(Span::styled(
            crate::i18n::text(crate::messages::VAULT_STRIP_EMPTY),
            theme::muted(),
        ));
        frame.render_widget(Paragraph::new(line), inner);
        return;
    }

    let alias_w = rows
        .iter()
        .map(|r| r.alias.width())
        .max()
        .unwrap_or(6)
        .max(6);
    let role_w = rows
        .iter()
        .map(|r| r.role.width())
        .max()
        .unwrap_or(8)
        .max(8);

    let gauge_w = inner
        .width
        .saturating_sub((2 + alias_w + 2 + role_w + 2 + 6 + 2) as u16)
        .max(8) as usize;

    let lines: Vec<Line> = rows
        .iter()
        .map(|cert| build_strip_line(cert, alias_w, role_w, gauge_w))
        .collect();

    frame.render_widget(Paragraph::new(lines), inner);
}

fn build_strip_line(
    cert: &ActiveCert,
    alias_w: usize,
    role_w: usize,
    gauge_w: usize,
) -> Line<'static> {
    let ratio = vault_ssh::cert_fill_ratio(cert.remaining_secs, cert.total_secs);
    let filled = (ratio * gauge_w as f32).round() as usize;
    let empty = gauge_w.saturating_sub(filled);
    // Use the lower-half block (▄) for both filled and empty segments
    // so each row only paints the bottom half of its terminal cell.
    // The upper half stays transparent and creates a thin breathing
    // line between adjacent rows; no extra spacer rows required.
    let filled_chars = "\u{2584}".repeat(filled);
    let empty_chars = "\u{2584}".repeat(empty);
    let bar_style = ttl_bar_style(cert.remaining_secs);
    let remaining = format_strip_remaining(cert.remaining_secs);

    Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{:<alias_w$}", cert.alias), theme::bold()),
        Span::raw("  "),
        Span::styled(format!("{:<role_w$}", cert.role), theme::muted()),
        Span::raw("  "),
        Span::styled(filled_chars, bar_style),
        Span::styled(empty_chars, theme::muted()),
        Span::raw("  "),
        Span::styled(remaining, theme::muted()),
    ])
}

fn ttl_bar_style(remaining_secs: i64) -> Style {
    if remaining_secs >= 300 {
        theme::healthy()
    } else if remaining_secs >= 120 {
        theme::warning()
    } else {
        theme::error()
    }
}

fn format_strip_remaining(remaining_secs: i64) -> String {
    if remaining_secs <= 0 {
        return "expired".to_string();
    }
    if remaining_secs < 3600 {
        let m = remaining_secs / 60;
        let s = remaining_secs % 60;
        format!("{:>2}:{:02}", m, s)
    } else {
        let h = remaining_secs / 3600;
        let m = (remaining_secs % 3600) / 60;
        format!("{}h {}m", h, m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cert(secs: i64, total: i64) -> ActiveCert {
        ActiveCert {
            alias: "prod-eu1".into(),
            role: "ops/prod".into(),
            remaining_secs: secs,
            total_secs: total,
        }
    }

    #[test]
    fn ttl_bar_style_green_above_5min() {
        assert_eq!(ttl_bar_style(301), theme::online_dot());
        assert_eq!(ttl_bar_style(3600), theme::online_dot());
    }

    #[test]
    fn ttl_bar_style_amber_between_2_and_5_min() {
        assert_eq!(ttl_bar_style(299), theme::warning());
        assert_eq!(ttl_bar_style(120), theme::warning());
    }

    #[test]
    fn ttl_bar_style_red_under_2min() {
        assert_eq!(ttl_bar_style(119), theme::error());
        assert_eq!(ttl_bar_style(1), theme::error());
    }

    #[test]
    fn format_strip_remaining_under_one_hour() {
        assert_eq!(format_strip_remaining(125), " 2:05");
        assert_eq!(format_strip_remaining(60), " 1:00");
        assert_eq!(format_strip_remaining(59), " 0:59");
        assert_eq!(format_strip_remaining(900), "15:00");
    }

    #[test]
    fn format_strip_remaining_above_one_hour() {
        assert_eq!(format_strip_remaining(3661), "1h 1m");
    }

    #[test]
    fn format_strip_remaining_expired() {
        assert_eq!(format_strip_remaining(0), "expired");
        assert_eq!(format_strip_remaining(-5), "expired");
    }

    #[test]
    fn build_strip_line_returns_span_count_consistent_with_layout() {
        // 9 spans: leading pad, alias, pad, role, pad, filled bar,
        // empty bar (lower-half blocks dimmed), pad, remaining label.
        let c = cert(900, 1800);
        let line = build_strip_line(&c, 8, 8, 20);
        assert_eq!(line.spans.len(), 9);
    }

    #[test]
    fn ttl_bar_style_exactly_300_is_green() {
        assert_eq!(ttl_bar_style(300), theme::online_dot());
    }

    #[test]
    fn format_strip_remaining_exactly_one_hour_uses_hours_branch() {
        assert_eq!(format_strip_remaining(3600), "1h 0m");
    }
}
