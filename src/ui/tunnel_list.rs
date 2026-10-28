use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem};

use super::design;
use super::theme;
use crate::app::App;

pub fn render(frame: &mut Frame, app: &mut App, alias: &str) {
    let is_active = app.tunnels.active_contains(alias);
    let is_readonly = app
        .hosts_state
        .list()
        .iter()
        .any(|h| h.alias == alias && h.source_file.is_some());

    // Overlay: percentage-based width, height fits content. Reserve 1 row
    // below the block for the external footer.
    let item_count = app.tunnels.list().len().max(1);
    let height = (item_count as u16 + 4).min(frame.area().height.saturating_sub(5));
    let area = design::overlay_area(frame, design::OVERLAY_W, design::OVERLAY_H, height);
    frame.render_widget(Clear, area);

    let mut block = design::overlay_block(&format!("Tunnels for {}", alias));
    if is_active {
        // `[running]` is a live-state indicator — same semantic tier
        // as the host-list online dot. Use `online_dot()` so the green
        // shade matches the rest of the app.
        block = block.title_top(Line::from(Span::styled("[running] ", theme::online_dot())));
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.tunnels.list().is_empty() {
        if is_readonly {
            design::render_empty(frame, inner, "Read-only (included file).");
        } else {
            design::render_empty_with_hint(frame, inner, "No tunnels.", "a", "add one");
        }
    } else {
        let items: Vec<ListItem> = app
            .tunnels
            .list()
            .iter()
            .map(|rule| {
                let type_label = format!(" {:<10}", rule.tunnel_type.label());
                let port_str = if rule.bind_address.is_empty() {
                    rule.bind_port.to_string()
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
            .highlight_symbol(design::LIST_HIGHLIGHT);

        frame.render_stateful_widget(list, inner, app.ui.tunnel_list_state_mut());
    }

    // Footer below the block — but only when there is no pending
    // confirm. Destructive confirms render as centred popups (the
    // design-system pattern shared with host delete, vault sign and
    // container actions) instead of inline footer prompts under the
    // parent overlay.
    let footer_area = design::render_overlay_footer(frame, area);
    if app.tunnels.pending_delete().is_some() {
        let detail = crate::messages::confirm_tunnel_delete_detail(&app.config_path_display());
        design::render_destructive_popup(
            frame,
            crate::messages::CONFIRM_TUNNEL_DELETE_TITLE,
            crate::messages::CONFIRM_TUNNEL_DELETE_QUESTION,
            &detail,
            "delete",
            "keep",
            app,
        );
    } else {
        use crate::messages::footer as fl;
        let mut f = design::Footer::new();
        if is_active {
            f = f.primary("Enter", fl::ENTER_STOP);
        } else if !app.tunnels.list().is_empty() {
            f = f.primary("Enter", fl::ENTER_START);
        }
        if !is_readonly {
            f = f.action("a", fl::ACTION_ADD);
            if !app.tunnels.list().is_empty() {
                f = f.action("e", fl::ACTION_EDIT).action("d", fl::ACTION_DEL);
            }
        }
        f = f.action("Esc", fl::ESC_BACK);
        f.render_with_status(frame, footer_area, app);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::design;

    // The per-host delete confirm names the config file it rewrites.
    #[test]
    fn delete_confirm_names_the_config_file() {
        let mut app = crate::ui::app_with_own_config("Host fwd\n  HostName 10.0.0.1\n");
        app.tunnels.request_delete(0);
        let text =
            crate::ui::render_to_text(120, 40, |frame| super::render(frame, &mut app, "fwd"));
        assert!(text.contains("~/.ssh/purple_config"), "{text}");
        assert!(!text.contains("~/.ssh/config"), "{text}");
    }

    #[test]
    fn footer_sits_directly_below_block() {
        let area = Rect::new(0, 0, 60, 20);
        let footer = design::form_footer(area, area.height);
        assert_eq!(footer.height, 1);
        assert_eq!(footer.y, area.y + area.height);
        assert_eq!(footer.x, area.x);
        assert_eq!(footer.width, area.width);
    }
}
