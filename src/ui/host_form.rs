use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{App, FormField, Screen};

fn placeholder_for(field: FormField) -> String {
    match field {
        FormField::AskPass => {
            if let Some(default) = crate::preferences::load_askpass_default() {
                format!("default: {}", default)
            } else {
                "Enter to pick a source".to_string()
            }
        }
        FormField::Alias => "my-server".to_string(),
        FormField::Hostname => "192.168.1.1 or example.com".to_string(),
        FormField::User => "root".to_string(),
        FormField::Port => "22".to_string(),
        FormField::IdentityFile => "Enter to pick a key".to_string(),
        FormField::ProxyJump => "bastion-host".to_string(),
        FormField::Tags => "prod, staging, us-east".to_string(),
    }
}

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    let title = match &app.screen {
        Screen::AddHost => " Add New Host ",
        Screen::EditHost { .. } => " Edit Host ",
        _ => " Host ",
    };

    // Center the form
    let form_area = super::centered_rect(70, 80, area);

    // Clear background
    frame.render_widget(Clear, form_area);

    let outer_block = Block::default()
        .title(Span::styled(title, theme::brand()))
        .borders(Borders::ALL)
        .border_style(theme::border());

    let inner = outer_block.inner(form_area);
    frame.render_widget(outer_block, form_area);

    // Layout: 8 fields grouped with spacing + footer
    let chunks = Layout::vertical([
        Constraint::Length(3), // 0: Alias
        Constraint::Length(3), // 1: Hostname
        Constraint::Length(3), // 2: User
        Constraint::Length(3), // 3: Port
        Constraint::Length(1), // 4: Spacer (Connection -> Security)
        Constraint::Length(3), // 5: IdentityFile
        Constraint::Length(3), // 6: ProxyJump
        Constraint::Length(3), // 7: AskPass
        Constraint::Length(1), // 8: Spacer (Security -> Meta)
        Constraint::Length(3), // 9: Tags
        Constraint::Min(1),   // 10: Spacer
        Constraint::Length(1), // 11: Footer or status
    ])
    .split(inner);

    // Connection
    render_field(frame, chunks[0], FormField::Alias, &app.form);
    render_field(frame, chunks[1], FormField::Hostname, &app.form);
    render_field(frame, chunks[2], FormField::User, &app.form);
    render_field(frame, chunks[3], FormField::Port, &app.form);
    // Security
    render_field(frame, chunks[5], FormField::IdentityFile, &app.form);
    render_field(frame, chunks[6], FormField::ProxyJump, &app.form);
    render_field(frame, chunks[7], FormField::AskPass, &app.form);
    // Meta
    render_field(frame, chunks[9], FormField::Tags, &app.form);

    // Footer with status right-aligned
    super::render_footer_with_status(frame, chunks[11], vec![
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
        render_key_picker_overlay(frame, app);
    }

    // Password source picker popup overlay
    if app.ui.show_password_picker {
        render_password_picker_overlay(frame, app);
    }
}

/// Render the key picker popup overlay. Public for reuse from provider form.
pub fn render_key_picker_overlay(frame: &mut Frame, app: &mut App) {
    if app.keys.is_empty() {
        // Small popup saying no keys found
        let area = super::centered_rect_fixed(50, 5, frame.area());
        frame.render_widget(Clear, area);
        let block = Block::default()
            .title(Span::styled(" Select Key ", theme::brand()))
            .borders(Borders::ALL)
            .border_style(theme::accent());
        let msg = Paragraph::new(Line::from(Span::styled(
            "  No keys found in ~/.ssh/",
            theme::muted(),
        )))
        .block(block);
        frame.render_widget(msg, area);
        return;
    }

    let height = (app.keys.len() as u16 + 4).min(16);
    let width = frame.area().width.clamp(58, 72);
    let area = super::centered_rect_fixed(width, height, frame.area());
    frame.render_widget(Clear, area);

    // Comment gets remaining space after name(16) + type(10) + borders(2) + highlight(2) + lead(1)
    let comment_max = (width as usize).saturating_sub(2 + 2 + 1 + 16 + 10);

    let items: Vec<ListItem> = app
        .keys
        .iter()
        .map(|key| {
            let type_display = key.type_display();
            let comment = if key.comment.is_empty() {
                String::new()
            } else {
                super::truncate(&key.comment, comment_max)
            };
            let line = Line::from(vec![
                Span::styled(format!(" {:<16}", key.name), theme::bold()),
                Span::styled(format!("{:<10}", type_display), theme::muted()),
                Span::styled(comment, theme::muted()),
            ]);
            ListItem::new(line)
        })
        .collect();

    let block = Block::default()
        .title(Span::styled(" Select Key ", theme::brand()))
        .borders(Borders::ALL)
        .border_style(theme::accent());

    let list = List::new(items)
        .block(block)
        .highlight_style(theme::selected())
        .highlight_symbol("  ");

    frame.render_stateful_widget(list, area, &mut app.ui.key_picker_state);
}

fn render_password_picker_overlay(frame: &mut Frame, app: &mut App) {
    let sources = crate::askpass::PASSWORD_SOURCES;
    let height = sources.len() as u16 + 4; // items + borders + footer
    let area = super::centered_rect_fixed(54, height, frame.area());
    frame.render_widget(Clear, area);

    let items: Vec<ListItem> = sources
        .iter()
        .map(|src| {
            let hint_width = src.hint.len();
            let label_width = 48_usize.saturating_sub(4).saturating_sub(hint_width).saturating_sub(1);
            let line = Line::from(vec![
                Span::styled(format!(" {:<width$}", src.label, width = label_width), theme::bold()),
                Span::styled(src.hint, theme::muted()),
            ]);
            ListItem::new(line)
        })
        .collect();

    let block = Block::default()
        .title(Span::styled(" Password Source ", theme::brand()))
        .borders(Borders::ALL)
        .border_style(theme::accent());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split into list area and footer
    let chunks = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);

    let list = List::new(items)
        .highlight_style(theme::selected())
        .highlight_symbol("  ");

    frame.render_stateful_widget(list, chunks[0], &mut app.ui.password_picker_state);

    let footer = Line::from(vec![
        Span::styled(" Enter", theme::accent_bold()),
        Span::styled(" select  ", theme::muted()),
        Span::styled("Ctrl+D", theme::accent_bold()),
        Span::styled(" global default  ", theme::muted()),
        Span::styled("Esc", theme::accent_bold()),
        Span::styled(" cancel", theme::muted()),
    ]);
    frame.render_widget(Paragraph::new(footer), chunks[1]);
}

/// Get the placeholder text for a field (public for tests).
#[cfg(test)]
pub fn placeholder_text(field: FormField) -> String {
    placeholder_for(field)
}

fn render_field(frame: &mut Frame, area: Rect, field: FormField, form: &crate::app::HostForm) {
    let is_focused = form.focused_field == field;

    let value = match field {
        FormField::Alias => &form.alias,
        FormField::Hostname => &form.hostname,
        FormField::User => &form.user,
        FormField::Port => &form.port,
        FormField::IdentityFile => &form.identity_file,
        FormField::ProxyJump => &form.proxy_jump,
        FormField::AskPass => &form.askpass,
        FormField::Tags => &form.tags,
    };

    let (border_style, label_style) = if is_focused {
        (theme::border_focused(), theme::accent_bold())
    } else {
        (theme::border(), theme::muted())
    };

    // Required fields get an asterisk
    let is_required = matches!(field, FormField::Alias | FormField::Hostname);
    let label = if is_required {
        format!(" {}* ", field.label())
    } else {
        format!(" {} ", field.label())
    };

    let block = Block::default()
        .title(Span::styled(label, label_style))
        .borders(Borders::ALL)
        .border_style(border_style);

    let is_picker = matches!(field, FormField::IdentityFile | FormField::AskPass);

    // Show placeholder when field is empty and not focused
    let content = if value.is_empty() && !is_focused {
        let ph = placeholder_for(field);
        Line::from(Span::styled(ph, theme::muted()))
    } else if is_picker && is_focused {
        let inner_width = area.width.saturating_sub(2) as usize;
        let arrow_pos = inner_width.saturating_sub(1);
        let val_width = value.width();
        let gap = arrow_pos.saturating_sub(val_width);
        Line::from(vec![
            Span::raw(value.as_str()),
            Span::raw(" ".repeat(gap)),
            Span::styled("\u{25B8}", theme::muted()),
        ])
    } else {
        Line::from(Span::raw(value.as_str()))
    };

    let paragraph = Paragraph::new(content).block(block);
    frame.render_widget(paragraph, area);

    if is_focused {
        let prefix: String = value.chars().take(form.cursor_pos).collect();
        let cursor_x = area
            .x
            .saturating_add(1)
            .saturating_add(prefix.width().min(u16::MAX as usize) as u16);
        let cursor_y = area.y + 1;
        if area.width > 1 && cursor_x < area.x.saturating_add(area.width).saturating_sub(1) {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

