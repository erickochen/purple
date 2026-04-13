use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, Paragraph};

use super::theme;
use crate::app::App;

pub fn render(frame: &mut Frame, app: &mut App, alias: &str) {
    let is_active = app.active_tunnels.contains_key(alias);
    let is_readonly = app
        .hosts
        .iter()
        .any(|h| h.alias == alias && h.source_file.is_some());

    // Title
    let mut title_spans = vec![Span::styled(
        format!(" Tunnels for {} ", alias),
        theme::brand(),
    )];
    if is_active {
        title_spans.push(Span::styled("[running] ", theme::success()));
    }
    let title = Line::from(title_spans);

    // Overlay: percentage-based width, height fits content
    let item_count = app.tunnel_list.len().max(1);
    let height = (item_count as u16 + 6).min(frame.area().height.saturating_sub(4));
    let area = {
        let r = super::centered_rect(70, 80, frame.area());
        super::centered_rect_fixed(r.width, height, frame.area())
    };
    frame.render_widget(Clear, area);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title)
        .border_style(theme::accent());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);

    if app.tunnel_list.is_empty() {
        let msg = if is_readonly {
            "  Read-only (included file)."
        } else {
            "  No tunnels. Press 'a' to add one."
        };
        frame.render_widget(Paragraph::new(msg).style(theme::muted()), chunks[0]);
    } else {
        let items: Vec<ListItem> = app
            .tunnel_list
            .iter()
            .map(|rule| {
                let type_label = format!(" {:<10}", rule.tunnel_type.label());
                let port_str = if rule.bind_address.is_empty() {
                    format!("{}", rule.bind_port)
                } else if rule.bind_address.contains(':') {
                    format!("[{}]:{}", rule.bind_address, rule.bind_port)
                } else {
                    format!("{}:{}", rule.bind_address, rule.bind_port)
                };
                let dest = match rule.tunnel_type {
                    crate::tunnel::TunnelType::Dynamic => "(SOCKS proxy)".to_string(),
                    _ => {
                        if rule.remote_host.contains(':') {
                            format!("[{}]:{}", rule.remote_host, rule.remote_port)
                        } else {
                            format!("{}:{}", rule.remote_host, rule.remote_port)
                        }
                    }
                };
                let line = Line::from(vec![
                    Span::styled(type_label, theme::bold()),
                    Span::styled(format!("{:<14}", port_str), theme::bold()),
                    Span::raw("  "),
                    Span::styled(dest, theme::muted()),
                ]);
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items)
            .highlight_style(theme::selected_row())
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, chunks[0], &mut app.ui.tunnel_list_state);
    }

    // Footer
    if app.pending_tunnel_delete.is_some() {
        super::render_footer_with_status(
            frame,
            chunks[2],
            vec![
                Span::styled(" Remove tunnel? ", theme::bold()),
                Span::styled(" y ", theme::footer_key()),
                Span::styled(" yes ", theme::muted()),
                Span::raw("  "),
                Span::styled(" Esc ", theme::footer_key()),
                Span::styled(" no", theme::muted()),
            ],
            app,
        );
    } else {
        let mut spans: Vec<Span<'_>> = Vec::new();
        if is_active {
            let [k, l] = super::footer_primary("Enter", " stop ");
            spans.extend([k, l]);
        } else if !app.tunnel_list.is_empty() {
            let [k, l] = super::footer_primary("Enter", " start ");
            spans.extend([k, l]);
        }
        if !is_readonly {
            if !spans.is_empty() {
                spans.push(Span::raw("  "));
            }
            let [k, l] = super::footer_action("a", " add ");
            spans.extend([k, l]);
            if !app.tunnel_list.is_empty() {
                spans.push(Span::raw("  "));
                let [k, l] = super::footer_action("e", " edit ");
                spans.extend([k, l, Span::raw("  ")]);
                let [k, l] = super::footer_action("d", " del ");
                spans.extend([k, l]);
            }
        }
        if spans.is_empty() {
            let [k, l] = super::footer_action("Esc", " back");
            spans.extend([k, l]);
        } else {
            spans.push(Span::raw("  "));
            let [k, l] = super::footer_action("Esc", " back");
            spans.extend([k, l]);
        }
        super::render_footer_with_status(frame, chunks[2], spans, app);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::{Constraint, Layout, Rect};

    #[test]
    fn layout_has_spacer_between_content_and_footer() {
        let area = Rect::new(0, 0, 60, 20);
        let chunks = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);
        // chunks[0] = content, chunks[1] = spacer, chunks[2] = footer
        assert_eq!(chunks[1].height, 1, "spacer row should be 1 tall");
        assert_eq!(chunks[2].height, 1, "footer row should be 1 tall");
        assert!(
            chunks[2].y > chunks[0].y + chunks[0].height,
            "footer (y={}) should be below content end (y={})",
            chunks[2].y,
            chunks[0].y + chunks[0].height
        );
    }
}
