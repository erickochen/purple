use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{App, ProviderFormField};
use crate::providers;

/// Render the provider management list screen.
pub fn render_provider_list(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    let title = Line::from(vec![
        Span::styled(" purple. ", theme::brand_badge()),
        Span::raw(" Providers "),
    ]);

    let items: Vec<ListItem> = providers::PROVIDER_NAMES
        .iter()
        .map(|&name| {
            let display_name = match name {
                "digitalocean" => "DigitalOcean",
                "vultr" => "Vultr",
                "linode" => "Linode",
                "hetzner" => "Hetzner",
                "upcloud" => "UpCloud",
                n => n,
            };
            let configured = app.provider_config.section(name).is_some();
            let status = if configured {
                "[configured]"
            } else {
                "[not configured]"
            };
            let status_style = if configured {
                theme::success()
            } else {
                theme::muted()
            };
            let mut spans = vec![
                Span::styled(format!("  {:<18}", display_name), theme::bold()),
                Span::styled(status, status_style),
            ];
            if configured {
                if let Some(section) = app.provider_config.section(name) {
                    spans.push(Span::styled(
                        format!("     {}-*", section.alias_prefix),
                        theme::muted(),
                    ));
                }
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let chunks = Layout::vertical([
        Constraint::Min(5),
        Constraint::Length(1),
    ])
    .split(area);

    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(theme::border()),
        )
        .highlight_style(theme::selected())
        .highlight_symbol("  ");

    frame.render_stateful_widget(list, chunks[0], &mut app.provider_list_state);

    // Footer
    if app.status.is_some() {
        super::render_status_bar(frame, chunks[1], app);
    } else {
        let footer = Line::from(vec![
            Span::styled(" Enter", theme::primary_action()),
            Span::styled(" configure  ", theme::muted()),
            Span::styled("s", theme::accent_bold()),
            Span::styled(" sync  ", theme::muted()),
            Span::styled("d", theme::accent_bold()),
            Span::styled(" remove  ", theme::muted()),
            Span::styled("Esc", theme::accent_bold()),
            Span::styled(" back", theme::muted()),
        ]);
        frame.render_widget(Paragraph::new(footer), chunks[1]);
    }
}

/// Render the provider configuration form.
pub fn render_provider_form(frame: &mut Frame, app: &mut App, provider_name: &str) {
    let area = frame.area();

    let display_name = match provider_name {
        "digitalocean" => "DigitalOcean",
        "vultr" => "Vultr",
        "linode" => "Linode",
        "hetzner" => "Hetzner",
        "upcloud" => "UpCloud",
        n => n,
    };
    let title = format!(" Configure {} ", display_name);

    let form_area = super::centered_rect(70, 80, area);
    frame.render_widget(Clear, form_area);

    let outer_block = Block::default()
        .title(Span::styled(title, theme::brand()))
        .borders(Borders::ALL)
        .border_style(theme::border());

    let inner = outer_block.inner(form_area);
    frame.render_widget(outer_block, form_area);

    let chunks = Layout::vertical([
        Constraint::Length(3), // Token
        Constraint::Length(3), // Alias Prefix
        Constraint::Length(3), // User
        Constraint::Length(3), // Identity File
        Constraint::Min(1),   // Spacer
        Constraint::Length(1), // Footer or status
    ])
    .split(inner);

    render_provider_field(frame, chunks[0], ProviderFormField::Token, &app.provider_form);
    render_provider_field(frame, chunks[1], ProviderFormField::AliasPrefix, &app.provider_form);
    render_provider_field(frame, chunks[2], ProviderFormField::User, &app.provider_form);
    render_provider_field(frame, chunks[3], ProviderFormField::IdentityFile, &app.provider_form);

    // Footer or status
    if app.status.is_some() {
        super::render_status_bar(frame, chunks[5], app);
    } else {
        let footer = Line::from(vec![
            Span::styled(" Enter", theme::primary_action()),
            Span::styled(" save  ", theme::muted()),
            Span::styled("Tab/S-Tab", theme::accent_bold()),
            Span::styled(" navigate  ", theme::muted()),
            Span::styled("K", theme::accent_bold()),
            Span::styled(" pick key  ", theme::muted()),
            Span::styled("Esc", theme::accent_bold()),
            Span::styled(" cancel", theme::muted()),
        ]);
        frame.render_widget(Paragraph::new(footer), chunks[5]);
    }

    // Key picker popup overlay
    if app.show_key_picker {
        super::host_form::render_key_picker_overlay(frame, app);
    }
}

fn placeholder_for(field: ProviderFormField) -> &'static str {
    match field {
        ProviderFormField::Token => "your-api-token",
        ProviderFormField::AliasPrefix => "do",
        ProviderFormField::User => "root",
        ProviderFormField::IdentityFile => "~/.ssh/id_ed25519",
    }
}

fn render_provider_field(
    frame: &mut Frame,
    area: Rect,
    field: ProviderFormField,
    form: &crate::app::ProviderFormFields,
) {
    let is_focused = form.focused_field == field;

    let value = match field {
        ProviderFormField::Token => &form.token,
        ProviderFormField::AliasPrefix => &form.alias_prefix,
        ProviderFormField::User => &form.user,
        ProviderFormField::IdentityFile => &form.identity_file,
    };

    let (border_style, label_style) = if is_focused {
        (theme::border_focused(), theme::accent_bold())
    } else {
        (theme::border(), theme::muted())
    };

    let is_required = matches!(field, ProviderFormField::Token);
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

    let display: Span = if value.is_empty() && !is_focused {
        Span::styled(placeholder_for(field), theme::muted())
    } else {
        Span::raw(display_value.as_str().to_string())
    };

    let paragraph = Paragraph::new(display).block(block);
    frame.render_widget(paragraph, area);

    if is_focused {
        let cursor_x = area
            .x
            .saturating_add(1)
            .saturating_add(value.width().min(u16::MAX as usize) as u16);
        let cursor_y = area.y + 1;
        if cursor_x < area.x + area.width - 1 {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}
