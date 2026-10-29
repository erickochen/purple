//! Right card of the hero: vertical strength gauge against the left
//! edge, kv-rows for security/identity/usage in the middle, activity
//! sparkline at the bottom that auto-scales its time window.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Padding, Paragraph};

use crate::key_activity;
use crate::ssh_keys::SshKeyInfo;
use crate::ui::{self, design, theme};

/// Per-side padding inside the info card.
const INFO_PAD_H: u16 = 1;
const INFO_PAD_V: u16 = 1;

/// Vertical strength gauge interior width. Block character lives in the
/// middle column, side columns hold the rounded frame.
const GAUGE_INNER_W: u16 = 1;
const GAUGE_OUTER_W: u16 = GAUGE_INNER_W + 2;

pub(super) fn render_info_card(
    frame: &mut Frame,
    key: &SshKeyInfo,
    activity: &key_activity::KeyActivityLog,
    area: Rect,
) {
    let block = design::main_block_line(Line::default())
        .padding(Padding::new(INFO_PAD_H, INFO_PAD_H, INFO_PAD_V, INFO_PAD_V));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 24 || inner.height < 4 {
        return;
    }

    let gauge_w = GAUGE_OUTER_W;
    let gap: u16 = 2;
    let [gauge_area, _gap, content_area] = Layout::horizontal([
        Constraint::Length(gauge_w),
        Constraint::Length(gap),
        Constraint::Min(20),
    ])
    .areas(inner);

    render_strength_gauge_vertical(frame, key.strength_score, gauge_area);
    render_info_content(frame, key, activity, content_area);
}

/// Body of the info card: kv-rows + a multi-line activity chart at the
/// bottom that auto-scales its time window to the oldest recorded use.
/// The separator between identity and security drops first on tight
/// hero heights; the chart drops next if there really is no room.
fn render_info_content(
    frame: &mut Frame,
    key: &SshKeyInfo,
    activity: &key_activity::KeyActivityLog,
    area: Rect,
) {
    const LABEL_W: usize = 12;

    let now = key_activity::now_for_render();
    let value_budget = (area.width as usize).saturating_sub(LABEL_W);

    // ── Security block ────────────────────────────────────────────
    let strength_text = format!("{} / 100", key.strength_score);
    let (passphrase_text, passphrase_style) = if key.encrypted {
        ("encrypted", theme::healthy())
    } else {
        ("none", theme::error())
    };
    let (agent_text, agent_style) = if key.agent_loaded {
        ("loaded", theme::healthy())
    } else {
        ("not loaded", theme::muted())
    };
    let (created_text, created_style) = match key.mtime_ts {
        Some(ts) => {
            let text = key_activity::format_created(now, ts);
            let age_days = now.saturating_sub(ts) / 86_400;
            let style = if age_days >= 4 * 365 {
                theme::error()
            } else if age_days >= 2 * 365 {
                theme::warning()
            } else {
                theme::bold()
            };
            (text, style)
        }
        None => ("unknown".to_string(), theme::muted()),
    };

    let security: [Line; 4] = [
        kv(
            LABEL_W,
            "Strength",
            &strength_text,
            strength_color(key.strength_score),
        ),
        kv(LABEL_W, "Passphrase", passphrase_text, passphrase_style),
        kv(LABEL_W, "Agent", agent_text, agent_style),
        kv(LABEL_W, "Modified", &created_text, created_style),
    ];

    // ── Identity block ─────────────────────────────────────────────
    let mut type_val = if key.bits.is_empty() {
        key.key_type.to_uppercase()
    } else {
        format!("{} {}", key.key_type.to_uppercase(), key.bits)
    };
    if key.is_certificate {
        type_val.push_str(" cert");
    }

    let comment_text = if key.comment.is_empty() {
        "(none)".to_string()
    } else {
        ui::truncate(&key.comment, value_budget)
    };
    let comment_style = if key.comment.is_empty() {
        theme::muted()
    } else {
        theme::bold()
    };

    let identity: [Line; 4] = [
        kv(LABEL_W, crate::i18n::text("Type"), &type_val, theme::bold()),
        kv(
            LABEL_W,
            crate::i18n::text("Fingerprint"),
            &ui::truncate(&key.fingerprint, value_budget),
            theme::muted(),
        ),
        kv(
            LABEL_W,
            crate::i18n::text("Path"),
            &ui::truncate(&key.display_path, value_budget),
            theme::bold(),
        ),
        kv(LABEL_W, "Comment", &comment_text, comment_style),
    ];

    // ── Usage block ────────────────────────────────────────────────
    let linked_text = format!("{}", key.linked_hosts.len());
    let used_text = match activity.last_use_for_aliases(&key.linked_hosts) {
        Some(ts) => key_activity::humanize_last_use(now, ts),
        None => "never".to_string(),
    };

    let usage: [Line; 2] = [
        kv(LABEL_W, "Linked", &linked_text, theme::bold()),
        kv(LABEL_W, "Used", &used_text, theme::bold()),
    ];

    // ── Activity chart ─────────────────────────────────────────────
    let chart_width = area.width as usize;
    let timestamps = activity.timestamps_for_aliases(&key.linked_hosts);
    let chart = if chart_width >= 20 {
        ui::activity_chart::render_with_baseline(&timestamps, chart_width, now)
    } else {
        Vec::new()
    };
    let chart_rows = chart.len();

    let kv_rows = security.len() + identity.len() + usage.len();
    let h = area.height as usize;
    let want_chart = chart_rows > 0 && h >= kv_rows + chart_rows;
    let want_separators = h >= kv_rows + 2 + chart_rows;

    let mut lines: Vec<Line> = Vec::with_capacity(h);
    lines.extend(security);
    if want_separators {
        lines.push(Line::from(""));
    }
    lines.extend(identity);
    if want_separators {
        lines.push(Line::from(""));
    }
    lines.extend(usage);

    if want_chart {
        let trailing = h.saturating_sub(lines.len() + chart_rows);
        for _ in 0..trailing {
            lines.push(Line::from(""));
        }
        lines.extend(chart);
    }

    frame.render_widget(Paragraph::new(lines), area);
}

/// Vertical strength gauge. Frame is rounded box drawing; body cells
/// fill from the bottom up, colour-coded on the same green/amber/red
/// ramp as the Vault SSH TTL bar.
fn render_strength_gauge_vertical(frame: &mut Frame, score: u8, area: Rect) {
    let height = area.height as usize;
    if height < 3 {
        return;
    }
    let inner_h = height - 2;
    let filled = ((score as usize) * inner_h + 50) / 100;
    let color = strength_color(score);

    let mut lines: Vec<Line> = Vec::with_capacity(height);
    lines.push(Line::from(Span::styled(
        "\u{256D}\u{2500}\u{256E}",
        theme::border(),
    )));
    for i in 0..inner_h {
        let from_bottom = inner_h - 1 - i;
        let is_filled = from_bottom < filled;
        let ch = if is_filled { '\u{2588}' } else { '\u{2591}' };
        let cell_style = if is_filled { color } else { theme::muted() };
        lines.push(Line::from(vec![
            Span::styled("\u{2502}", theme::border()),
            Span::styled(ch.to_string(), cell_style),
            Span::styled("\u{2502}", theme::border()),
        ]));
    }
    lines.push(Line::from(Span::styled(
        "\u{2570}\u{2500}\u{256F}",
        theme::border(),
    )));
    frame.render_widget(Paragraph::new(lines), area);
}

/// Inline kv builder for the info card: muted label padded to
/// `label_w`, value rendered in the requested style. Mirrors the look
/// of `design::kv_line` but allows coloured values.
fn kv(label_w: usize, label: &str, value: &str, value_style: Style) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{:<width$}", label, width = label_w),
            theme::muted(),
        ),
        Span::styled(value.to_string(), value_style),
    ])
}

pub(super) fn strength_color(score: u8) -> Style {
    if score >= 70 {
        theme::healthy()
    } else if score >= 40 {
        theme::warning()
    } else {
        theme::error()
    }
}

pub(super) fn build_strength_bar(score: u8, width: usize) -> String {
    let filled = ((score as usize) * width / 100).min(width);
    let empty = width.saturating_sub(filled);
    format!("{}{}", "\u{2588}".repeat(filled), "\u{2591}".repeat(empty))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_strength_bar_fills_proportional() {
        assert_eq!(
            build_strength_bar(0, 10),
            "\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}"
        );
        assert_eq!(
            build_strength_bar(100, 10),
            "\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}"
        );
        assert_eq!(
            build_strength_bar(50, 10),
            "\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}"
        );
    }

    #[test]
    fn strength_color_ramp() {
        assert_eq!(strength_color(95), theme::healthy());
        assert_eq!(strength_color(55), theme::warning());
        assert_eq!(strength_color(10), theme::error());
    }
}
