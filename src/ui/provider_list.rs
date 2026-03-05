use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{App, ProviderFormField};
use crate::history::ConnectionHistory;

/// Render the provider management list as a centered overlay.
pub fn render_provider_list(frame: &mut Frame, app: &mut App) {
    let sorted_names = app.sorted_provider_names();

    // Overlay: percentage-based width, height fits content
    let item_count = sorted_names.len();
    let height = (item_count as u16 + 4).min(frame.area().height.saturating_sub(4));
    let pct_width: u16 = 70;
    let area = {
        let r = super::centered_rect(pct_width, 80, frame.area());
        super::centered_rect_fixed(r.width, height, frame.area())
    };
    frame.render_widget(Clear, area);

    let title = Span::styled(" Providers ", theme::brand());

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(theme::accent());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Content width inside the overlay
    let content_width = inner.width as usize;

    let items: Vec<ListItem> = sorted_names
        .iter()
        .map(|name| {
            let display_name = crate::providers::provider_display_name(name.as_str());
            let configured = app.provider_config.section(name.as_str()).is_some();

            let name_col = format!(" {:<16}", display_name);
            let mut spans = vec![Span::styled(name_col, theme::bold())];
            let mut used = 17;

            if configured {
                let has_error = app.sync_history.get(name.as_str()).is_some_and(|r| r.is_error);
                if has_error {
                    spans.push(Span::styled("\u{26A0}", theme::error()));
                } else {
                    spans.push(Span::styled("\u{2713}", theme::success()));
                }
                used += 1;

                if let Some(section) = app.provider_config.section(name.as_str()) {
                    if !section.auto_sync {
                        spans.push(Span::styled(" (manual)", theme::muted()));
                        used += 9;
                    }
                }

                // Sync detail on same line
                let sync_detail = if app.syncing_providers.contains_key(name.as_str()) {
                    Some("syncing...".to_string())
                } else if let Some(record) = app.sync_history.get(name.as_str()) {
                    let ago = ConnectionHistory::format_time_ago(record.timestamp);
                    if ago.is_empty() {
                        Some(record.message.clone())
                    } else {
                        Some(format!("{}, {}", record.message, ago))
                    }
                } else {
                    None
                };
                if let Some(detail) = sync_detail {
                    let max = content_width.saturating_sub(used + 2);
                    if max > 1 {
                        spans.push(Span::styled(
                            format!("  {}", super::truncate(&detail, max)),
                            theme::muted(),
                        ));
                    }
                }
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let chunks = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);

    let list = List::new(items)
        .highlight_style(theme::selected())
        .highlight_symbol("  ");

    frame.render_stateful_widget(list, chunks[0], &mut app.ui.provider_list_state);

    // Footer with status
    if app.pending_provider_delete.is_some() {
        let name = app.pending_provider_delete.as_deref().unwrap_or("");
        let display = crate::providers::provider_display_name(name);
        super::render_footer_with_status(frame, chunks[1], vec![
            Span::styled(format!(" Remove {}? ", display), theme::bold()),
            Span::styled("y", theme::accent_bold()),
            Span::styled(" yes  ", theme::muted()),
            Span::styled("Esc", theme::accent_bold()),
            Span::styled(" no", theme::muted()),
        ], app);
    } else {
        super::render_footer_with_status(frame, chunks[1], vec![
            Span::styled(" s", theme::accent_bold()),
            Span::styled(" sync  ", theme::muted()),
            Span::styled("Enter", theme::primary_action()),
            Span::styled(" configure  ", theme::muted()),
            Span::styled("d", theme::accent_bold()),
            Span::styled(" remove  ", theme::muted()),
            Span::styled("Esc", theme::accent_bold()),
            Span::styled(" back", theme::muted()),
        ], app);
    }
}

/// Render the provider configuration form.
pub fn render_provider_form(frame: &mut Frame, app: &mut App, provider_name: &str) {
    let area = frame.area();

    let display_name = crate::providers::provider_display_name(provider_name);
    let title = format!(" Configure {} ", display_name);

    let form_area = super::centered_rect(70, 80, area);
    frame.render_widget(Clear, form_area);

    let outer_block = Block::default()
        .title(Span::styled(title, theme::brand()))
        .borders(Borders::ALL)
        .border_style(theme::border());

    let inner = outer_block.inner(form_area);
    frame.render_widget(outer_block, form_area);

    let fields = ProviderFormField::fields_for(provider_name);
    let mut constraints: Vec<Constraint> = fields.iter().map(|_| {
        Constraint::Length(3)
    }).collect();
    constraints.push(Constraint::Min(1));   // Spacer
    constraints.push(Constraint::Length(1)); // Footer

    let chunks = Layout::vertical(constraints).split(inner);

    for (i, field) in fields.iter().enumerate() {
        if *field == ProviderFormField::VerifyTls {
            render_provider_toggle_field(frame, chunks[i], &app.provider_form);
        } else if *field == ProviderFormField::AutoSync {
            render_provider_auto_sync_field(frame, chunks[i], &app.provider_form);
        } else {
            render_provider_field(frame, chunks[i], *field, &app.provider_form, provider_name);
        }
    }

    // Footer with status
    let footer_idx = fields.len() + 1;
    super::render_footer_with_status(frame, chunks[footer_idx], vec![
        Span::styled(" Enter", theme::primary_action()),
        Span::styled(" save ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("Tab", theme::accent_bold()),
        Span::styled(" next  ", theme::muted()),
        Span::styled("Esc", theme::accent_bold()),
        Span::styled(" cancel", theme::muted()),
    ], app);

    // Key picker popup overlay
    if app.ui.show_key_picker {
        super::host_form::render_key_picker_overlay(frame, app);
    }
}

fn placeholder_for(field: ProviderFormField, provider_name: &str) -> &'static str {
    match field {
        ProviderFormField::Url => "https://pve.example.com:8006",
        ProviderFormField::Token => match provider_name {
            "proxmox" => "user@pam!token=secret",
            "upcloud" => "your-api-token",
            _ => "your-api-token",
        },
        ProviderFormField::AliasPrefix => match provider_name {
            "digitalocean" => "do",
            "vultr" => "vultr",
            "linode" => "linode",
            "hetzner" => "hetzner",
            "upcloud" => "uc",
            "proxmox" => "pve",
            _ => "prefix",
        },
        ProviderFormField::User => "root",
        ProviderFormField::IdentityFile => "Enter to pick a key",
        ProviderFormField::VerifyTls => "",
        ProviderFormField::AutoSync => "",
    }
}

fn render_provider_field(
    frame: &mut Frame,
    area: Rect,
    field: ProviderFormField,
    form: &crate::app::ProviderFormFields,
    provider_name: &str,
) {
    let is_focused = form.focused_field == field;

    let value = match field {
        ProviderFormField::Url => &form.url,
        ProviderFormField::Token => &form.token,
        ProviderFormField::AliasPrefix => &form.alias_prefix,
        ProviderFormField::User => &form.user,
        ProviderFormField::IdentityFile => &form.identity_file,
        ProviderFormField::VerifyTls => unreachable!("VerifyTls uses render_provider_toggle_field"),
        ProviderFormField::AutoSync => unreachable!("AutoSync uses render_provider_auto_sync_field"),
    };

    let (border_style, label_style) = if is_focused {
        (theme::border_focused(), theme::accent_bold())
    } else {
        (theme::border(), theme::muted())
    };

    let is_required = matches!(field, ProviderFormField::Token | ProviderFormField::Url);
    let label = if is_required {
        format!(" {}* ", field.label())
    } else {
        format!(" {} ", field.label())
    };

    let block = Block::default()
        .title(Span::styled(label, label_style))
        .borders(Borders::ALL)
        .border_style(border_style);

    // Mask token except last 4 chars
    let display_value: String = if field == ProviderFormField::Token && !value.is_empty() && !is_focused {
        let char_count = value.chars().count();
        if char_count > 4 {
            let last4: String = value.chars().skip(char_count - 4).collect();
            format!("{}{}", "*".repeat(char_count - 4), last4)
        } else {
            value.clone()
        }
    } else {
        value.clone()
    };

    let is_picker = field == ProviderFormField::IdentityFile;

    let content = if value.is_empty() && !is_focused {
        Line::from(Span::styled(placeholder_for(field, provider_name), theme::muted()))
    } else if is_picker && is_focused {
        let inner_width = area.width.saturating_sub(2) as usize;
        let arrow_pos = inner_width.saturating_sub(1);
        let val_width = UnicodeWidthStr::width(display_value.as_str());
        let gap = arrow_pos.saturating_sub(val_width);
        Line::from(vec![
            Span::raw(display_value),
            Span::raw(" ".repeat(gap)),
            Span::styled("\u{25B8}", theme::muted()),
        ])
    } else {
        Line::from(Span::raw(display_value))
    };

    let paragraph = Paragraph::new(content).block(block);
    frame.render_widget(paragraph, area);

    if is_focused {
        let prefix: String = value.chars().take(form.cursor_pos).collect();
        let cursor_x = area
            .x
            .saturating_add(1)
            .saturating_add(UnicodeWidthStr::width(prefix.as_str()).min(u16::MAX as usize) as u16);
        let cursor_y = area.y + 1;
        if area.width > 1 && cursor_x < area.x.saturating_add(area.width).saturating_sub(1) {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

fn render_provider_toggle_field(
    frame: &mut Frame,
    area: Rect,
    form: &crate::app::ProviderFormFields,
) {
    let is_focused = form.focused_field == ProviderFormField::VerifyTls;

    let (border_style, label_style) = if is_focused {
        (theme::border_focused(), theme::accent_bold())
    } else {
        (theme::border(), theme::muted())
    };

    let block = Block::default()
        .title(Span::styled(" Verify TLS ", label_style))
        .borders(Borders::ALL)
        .border_style(border_style);

    let value_text = if form.verify_tls {
        "yes"
    } else {
        "no (accept self-signed)"
    };

    let content = if is_focused {
        let inner_width = area.width.saturating_sub(2) as usize;
        let val_width = UnicodeWidthStr::width(value_text);
        let gap = inner_width.saturating_sub(val_width + 3);
        Line::from(vec![
            Span::raw(value_text),
            Span::raw(" ".repeat(gap)),
            Span::styled("\u{25C2} \u{25B8}", theme::muted()),
        ])
    } else {
        Line::from(Span::raw(value_text))
    };

    let paragraph = Paragraph::new(content).block(block);
    frame.render_widget(paragraph, area);
}

fn render_provider_auto_sync_field(
    frame: &mut Frame,
    area: Rect,
    form: &crate::app::ProviderFormFields,
) {
    let is_focused = form.focused_field == ProviderFormField::AutoSync;

    let (border_style, label_style) = if is_focused {
        (theme::border_focused(), theme::accent_bold())
    } else {
        (theme::border(), theme::muted())
    };

    let block = Block::default()
        .title(Span::styled(" Auto Sync ", label_style))
        .borders(Borders::ALL)
        .border_style(border_style);

    let value_text = if form.auto_sync {
        "yes"
    } else {
        "no (sync manually)"
    };

    let content = if is_focused {
        let inner_width = area.width.saturating_sub(2) as usize;
        let val_width = UnicodeWidthStr::width(value_text);
        let gap = inner_width.saturating_sub(val_width + 3);
        Line::from(vec![
            Span::raw(value_text),
            Span::raw(" ".repeat(gap)),
            Span::styled("\u{25C2} \u{25B8}", theme::muted()),
        ])
    } else {
        Line::from(Span::raw(value_text))
    };

    let paragraph = Paragraph::new(content).block(block);
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::super::truncate;

    #[test]
    fn truncate_fits() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_exact_fit() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_ascii() {
        assert_eq!(truncate("hello world", 8), "hello w…");
    }

    #[test]
    fn truncate_no_room() {
        assert_eq!(truncate("hello", 1), "");
        assert_eq!(truncate("hello", 0), "");
    }

    #[test]
    fn truncate_wide_cjk() {
        // CJK chars are 2 columns wide each. "你好世界" = 8 columns.
        // With max 5: target = 4 columns, fits "你好" (4 cols) + "…"
        assert_eq!(truncate("你好世界", 5), "你好…");
    }

    #[test]
    fn truncate_wide_cjk_odd_boundary() {
        // max 4: target = 3 columns, "你" = 2 cols fits, "好" = 2 cols won't
        assert_eq!(truncate("你好世界", 4), "你…");
    }

    #[test]
    fn truncate_mixed_ascii_cjk() {
        // "ab你好" = 2 + 4 = 6 columns. max 5: target = 4, "ab你" fits (4 cols)
        assert_eq!(truncate("ab你好", 5), "ab你…");
    }

    #[test]
    fn truncate_multibyte_emoji() {
        // "🚀🔥" = 2+2 = 4 columns (each emoji is 2 cols wide).
        // max 3: target = 2, "🚀" fits (2 cols)
        assert_eq!(truncate("🚀🔥", 3), "🚀…");
    }
}
