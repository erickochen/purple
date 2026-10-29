use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use super::design;
use super::theme;
use crate::app::App;

pub fn render(frame: &mut Frame, app: &App, index: usize) {
    let Some(key) = app.keys.list().get(index) else {
        return;
    };

    // Calculate height based on content, capped to prevent overflow
    let linked_count = key.linked_hosts.len();
    let max_visible_hosts = 10;
    let visible_hosts = linked_count.min(max_visible_hosts);
    let overflow_line = if linked_count > max_visible_hosts {
        1
    } else {
        0
    };
    // 2 (border) + 1 (blank) + 4 (metadata) + 1 (blank) + 2 (header+sep) + hosts + overflow + 1 (blank).
    // Footer renders below the block via `design::form_footer`, so no row reserved here.
    let height = (11 + visible_hosts.max(1) + overflow_line) as u16;
    let width = frame.area().width.clamp(58, 80);
    let area = super::centered_rect_fixed(width, height, frame.area());

    frame.render_widget(Clear, area);

    let block = design::overlay_block(&key.name);

    let type_display = key.type_display();
    let mut lines = vec![
        Line::from(""),
        design::kv_line(
            crate::i18n::text("Type"),
            &type_display,
            design::KV_LABEL_WIDE,
        ),
        design::kv_line(
            crate::i18n::text("Fingerprint"),
            &key.fingerprint,
            design::KV_LABEL_WIDE,
        ),
        design::kv_line(
            "Comment",
            if key.comment.is_empty() {
                "(none)"
            } else {
                &key.comment
            },
            design::KV_LABEL_WIDE,
        ),
        design::kv_line(
            crate::i18n::text("Path"),
            &key.display_path,
            design::KV_LABEL_WIDE,
        ),
        Line::from(""),
    ];
    lines.extend(design::content_section("Linked Hosts"));

    if key.linked_hosts.is_empty() {
        lines.push(design::empty_line("(none)"));
    } else {
        for alias in key.linked_hosts.iter().take(max_visible_hosts) {
            let hostname = app
                .hosts_state
                .list()
                .iter()
                .find(|h| h.alias == *alias)
                .map(|h| h.hostname.as_str())
                .unwrap_or("");
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<14}", alias), theme::bold()),
                Span::styled(" -> ", theme::muted()),
                Span::styled(hostname.to_string(), theme::muted()),
            ]));
        }
        if linked_count > max_visible_hosts {
            lines.push(Line::from(Span::styled(
                format!("  (and {} more...)", linked_count - max_visible_hosts),
                theme::muted(),
            )));
        }
    }

    lines.push(Line::from(""));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);

    // Footer below the block
    let footer_area = design::render_overlay_footer(frame, area);
    design::Footer::new()
        .action("Esc", crate::i18n::text(crate::messages::footer::ESC_CLOSE))
        .render_with_status(frame, footer_area, app);
}
