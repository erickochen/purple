use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, Paragraph};

use super::theme;
use crate::app::App;

pub fn render(frame: &mut Frame, app: &mut App) {
    if app.tag_list.is_empty() {
        let area = super::centered_rect_fixed(50, 5, frame.area());
        frame.render_widget(Clear, area);
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(Span::styled(" Filter by Tag ", theme::brand()))
            .border_style(theme::accent());
        let msg = Paragraph::new(Line::from(Span::styled(
            "  No tags yet. Press t on a host to add some.",
            theme::muted(),
        )))
        .block(block);
        frame.render_widget(msg, area);
        return;
    }

    // Count hosts per tag (including provider as virtual tag)
    let tag_counts: std::collections::HashMap<&str, usize> = {
        let mut counts = std::collections::HashMap::new();
        for host in &app.hosts {
            for tag in host.provider_tags.iter().chain(host.tags.iter()) {
                *counts.entry(tag.as_str()).or_insert(0) += 1;
            }
            if let Some(ref provider) = host.provider {
                *counts.entry(provider.as_str()).or_insert(0) += 1;
            }
            if host.stale.is_some()
                && !host
                    .tags
                    .iter()
                    .chain(host.provider_tags.iter())
                    .any(|t| t.eq_ignore_ascii_case("stale"))
            {
                *counts.entry("stale").or_insert(0) += 1;
            }
            if crate::vault_ssh::resolve_vault_role(
                host.vault_ssh.as_deref(),
                host.provider.as_deref(),
                &app.provider_config,
            )
            .is_some()
                && !host
                    .tags
                    .iter()
                    .chain(host.provider_tags.iter())
                    .any(|t| t.eq_ignore_ascii_case("vault-ssh"))
            {
                *counts.entry("vault-ssh").or_insert(0) += 1;
            }
            if host
                .askpass
                .as_deref()
                .map(|s| s.starts_with("vault:"))
                .unwrap_or(false)
                && !host
                    .tags
                    .iter()
                    .chain(host.provider_tags.iter())
                    .any(|t| t.eq_ignore_ascii_case("vault-kv"))
            {
                *counts.entry("vault-kv").or_insert(0) += 1;
            }
        }
        for pattern in &app.patterns {
            for tag in &pattern.tags {
                *counts.entry(tag.as_str()).or_insert(0) += 1;
            }
        }
        counts
    };

    let height = (app.tag_list.len() as u16 + 6).min(frame.area().height.saturating_sub(4));
    let area = super::centered_rect_fixed(50, height, frame.area());
    frame.render_widget(Clear, area);

    let items: Vec<ListItem> = app
        .tag_list
        .iter()
        .map(|tag| {
            let count = tag_counts.get(tag.as_str()).copied().unwrap_or(0);
            let line = Line::from(vec![
                Span::styled(format!(" {}", tag), theme::bold()),
                Span::styled(format!(" ({})", count), theme::muted()),
            ]);
            ListItem::new(line)
        })
        .collect();

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" Filter by Tag ", theme::brand()))
        .border_style(theme::accent());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);

    let list = List::new(items)
        .highlight_style(theme::selected_row())
        .highlight_symbol("  ");

    frame.render_stateful_widget(list, chunks[0], &mut app.ui.tag_picker_state);

    let spans = vec![
        Span::styled(" Enter ", theme::footer_key()),
        Span::styled(" select ", theme::muted()),
        Span::raw("  "),
        Span::styled(" Esc ", theme::footer_key()),
        Span::styled(" back", theme::muted()),
    ];
    super::render_footer_with_status(frame, chunks[2], spans, app);
}

#[cfg(test)]
mod tests {
    use ratatui::layout::{Constraint, Layout, Rect};

    #[test]
    fn layout_has_spacer_between_content_and_footer() {
        let area = Rect::new(0, 0, 50, 15);
        let chunks = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);
        assert_eq!(chunks[1].height, 1);
        assert_eq!(chunks[2].height, 1);
        assert!(chunks[2].y > chunks[0].y + chunks[0].height);
    }

    #[test]
    fn group_picker_layout_has_spacer_between_content_and_footer() {
        let area = Rect::new(0, 0, 50, 15);
        let chunks = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);
        assert_eq!(chunks[1].height, 1);
        assert_eq!(chunks[2].height, 1);
        assert!(chunks[2].y > chunks[0].y + chunks[0].height);
    }
}
