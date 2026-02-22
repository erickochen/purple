use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{App, FormField, Screen};

fn placeholder_for(field: FormField) -> &'static str {
    match field {
        FormField::Alias => "my-server",
        FormField::Hostname => "192.168.1.1 or example.com",
        FormField::User => "root",
        FormField::Port => "22",
        FormField::IdentityFile => "~/.ssh/id_ed25519",
        FormField::ProxyJump => "bastion-host",
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

    // Layout: 6 fields + spacer + footer/status (merged)
    let chunks = Layout::vertical([
        Constraint::Length(3), // Alias
        Constraint::Length(3), // Hostname
        Constraint::Length(3), // User
        Constraint::Length(3), // Port
        Constraint::Length(3), // IdentityFile
        Constraint::Length(3), // ProxyJump
        Constraint::Min(1),   // Spacer
        Constraint::Length(1), // Footer or status
    ])
    .split(inner);

    // Render each field
    render_field(frame, chunks[0], FormField::Alias, &app.form);
    render_field(frame, chunks[1], FormField::Hostname, &app.form);
    render_field(frame, chunks[2], FormField::User, &app.form);
    render_field(frame, chunks[3], FormField::Port, &app.form);
    render_field(frame, chunks[4], FormField::IdentityFile, &app.form);
    render_field(frame, chunks[5], FormField::ProxyJump, &app.form);

    // Footer or status (merged)
    if app.status.is_some() {
        super::render_status_bar(frame, chunks[7], app);
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
        frame.render_widget(Paragraph::new(footer), chunks[7]);
    }

    // Key picker popup overlay
    if app.show_key_picker {
        render_key_picker(frame, app);
    }
}

fn render_key_picker(frame: &mut Frame, app: &mut App) {
    if app.keys.is_empty() {
        // Small popup saying no keys found
        let area = super::centered_rect_fixed(44, 5, frame.area());
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
    let area = super::centered_rect_fixed(68, height, frame.area());
    frame.render_widget(Clear, area);

    let items: Vec<ListItem> = app
        .keys
        .iter()
        .map(|key| {
            let type_display = key.type_display();
            let comment = if key.comment.is_empty() {
                String::new()
            } else {
                truncate_comment(&key.comment, 22)
            };
            let line = Line::from(vec![
                Span::styled(format!(" {:<18}", key.name), theme::bold()),
                Span::styled(format!("{:<12}", type_display), theme::muted()),
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

    frame.render_stateful_widget(list, area, &mut app.key_picker_state);
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

    // Show placeholder when field is empty and not focused
    let display: Span = if value.is_empty() && !is_focused {
        Span::styled(placeholder_for(field), theme::muted())
    } else {
        Span::raw(value.as_str())
    };

    let paragraph = Paragraph::new(display).block(block);
    frame.render_widget(paragraph, area);

    // Place cursor at end of focused field (use display width for multibyte chars)
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

/// Truncate a comment string to `max_len` characters.
fn truncate_comment(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}
