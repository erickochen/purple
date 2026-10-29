use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem};

use super::design;
use super::theme;
use crate::app::App;

pub fn render(frame: &mut Frame, app: &mut App) {
    if app.tags.list().is_empty() {
        super::render_picker_empty_overlay(
            frame,
            "Filter by Tag",
            "No tags yet. Press t on a host to add some.",
        );
        return;
    }

    // Count hosts per tag (including provider as virtual tag)
    let tag_counts: std::collections::HashMap<&str, usize> = {
        let mut counts = std::collections::HashMap::new();
        for host in app.hosts_state.list() {
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
                host.provider_label.as_deref(),
                app.providers.config(),
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
        for pattern in app.hosts_state.patterns() {
            for tag in &pattern.tags {
                *counts.entry(tag.as_str()).or_insert(0) += 1;
            }
        }
        counts
    };

    // Use the canonical picker geometry: width clamp[PICKER_MIN_W,
    // PICKER_MAX_W], height grows with item count up to PICKER_MAX_H.
    // Reserve 1 row below the block for the external footer.
    let width = super::picker_overlay_width(frame);
    let height = (app.tags.list().len() as u16 + 2)
        .min(design::PICKER_MAX_H)
        .min(frame.area().height.saturating_sub(3));
    if height < super::PICKER_MIN_HEIGHT {
        return;
    }
    let area = super::centered_rect_fixed(width, height, frame.area());
    frame.render_widget(Clear, area);

    let items: Vec<ListItem> = app
        .tags
        .list()
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

    let block = design::overlay_block("Filter by Tag");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let list = List::new(items)
        .highlight_style(theme::selected_row())
        .highlight_symbol(design::LIST_HIGHLIGHT);

    frame.render_stateful_widget(list, inner, app.ui.tag_picker_state_mut());

    let footer_area = design::render_overlay_footer(frame, area);
    use crate::messages::footer as fl;
    design::Footer::new()
        .primary("Enter", crate::i18n::text(fl::ENTER_SELECT))
        .action("Esc", crate::i18n::text(fl::ESC_CANCEL))
        .render_with_status(frame, footer_area, app);
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::design;

    #[test]
    fn footer_sits_directly_below_block() {
        let area = Rect::new(0, 0, 50, 15);
        let footer = design::form_footer(area, area.height);
        assert_eq!(footer.height, 1);
        assert_eq!(footer.y, area.y + area.height);
        assert_eq!(footer.x, area.x);
        assert_eq!(footer.width, area.width);
    }
}
