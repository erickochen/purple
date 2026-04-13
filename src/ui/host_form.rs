use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{App, FormField, Screen};

fn placeholder_for(field: FormField, is_pattern: bool) -> String {
    match field {
        FormField::AskPass => {
            if let Some(default) = crate::preferences::load_askpass_default() {
                format!("default: {}", default)
            } else {
                "Enter to pick a source".to_string()
            }
        }
        FormField::Alias if is_pattern => "10.0.0.* or *.example.com".to_string(),
        FormField::Alias => "prod, staging, db-01".to_string(),
        FormField::Hostname => "192.168.1.1 or example.com".to_string(),
        FormField::User => "root".to_string(),
        FormField::Port => "22".to_string(),
        FormField::IdentityFile => "Enter to pick a key".to_string(),
        FormField::ProxyJump => "Enter to pick a host".to_string(),
        // SSH secrets engine role (signs SSH certificates). Distinct from
        // Vault KV used in Password Source (vault:path/to/secret).
        FormField::VaultSsh => "e.g. ssh-client-signer/sign/my-role".to_string(),
        FormField::VaultAddr => {
            "e.g. http://127.0.0.1:8200 (inherits from provider or env when empty)".to_string()
        }
        FormField::Tags => "prod, staging, us-east".to_string(),
    }
}

/// Required fields (always visible).
const REQUIRED_FIELDS: &[(FormField, bool)] =
    &[(FormField::Alias, true), (FormField::Hostname, true)];

/// All fields in order: required first, then optional. `VaultAddr` lives
/// immediately after `VaultSsh` and is progressively disclosed at render
/// time by filtering against `HostForm::visible_fields()` — the constant
/// keeps the full schema so dirty-check, baselines and non-render callers
/// see a consistent ordering.
const ALL_FIELDS: &[(FormField, bool)] = &[
    (FormField::Alias, true),
    (FormField::Hostname, true),
    (FormField::User, false),
    (FormField::Port, false),
    (FormField::IdentityFile, false),
    (FormField::VaultSsh, false),
    (FormField::VaultAddr, false),
    (FormField::ProxyJump, false),
    (FormField::AskPass, false),
    (FormField::Tags, false),
];

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    // Determine visible fields based on progressive disclosure state.
    // The Vault SSH Role override field follows the same expand/collapse rule
    // as every other optional field: hidden in collapsed state, shown in
    // expanded state. The Vault SSH Address field has an additional gate:
    // it is only rendered when a Vault SSH Role is set on this form (the
    // address is meaningless without a role, and hiding it keeps the form
    // compact for the 99% of hosts that do not use Vault SSH).
    let expanded = app.form.expanded;
    let role_set = !app.form.vault_ssh.trim().is_empty();
    let base: &[(FormField, bool)] = if expanded {
        ALL_FIELDS
    } else {
        REQUIRED_FIELDS
    };
    let filtered: Vec<(FormField, bool)> = base
        .iter()
        .copied()
        .filter(|(f, _)| *f != FormField::VaultAddr || role_set)
        .collect();
    let visible_fields: &[(FormField, bool)] = &filtered;
    // Block: top(1) + fields * 2 (divider + content) + bottom(1)
    let block_height = 2 + visible_fields.len() as u16 * 2;
    let total_height = block_height + 1; // + footer

    let base = super::centered_rect(70, 80, area);

    let title = if app.form.is_pattern {
        match &app.screen {
            Screen::AddHost => " Add Pattern ".to_string(),
            Screen::EditHost { alias } => {
                let max_alias = (base.width as usize).saturating_sub(14);
                let truncated = super::truncate(alias, max_alias);
                format!(" Edit: {} ", truncated)
            }
            _ => " Pattern ".to_string(),
        }
    } else {
        match &app.screen {
            Screen::AddHost => " Add New Host ".to_string(),
            Screen::EditHost { alias } => {
                let max_alias = (base.width as usize).saturating_sub(12);
                let truncated = super::truncate(alias, max_alias);
                format!(" Edit: {} ", truncated)
            }
            _ => " Host ".to_string(),
        }
    };
    let form_area = super::centered_rect_fixed(base.width, total_height, area);
    frame.render_widget(Clear, form_area);

    let block_area = Rect::new(form_area.x, form_area.y, form_area.width, block_height);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(title, theme::brand()))
        .border_style(theme::accent());

    let inner = block.inner(block_area);
    frame.render_widget(block, block_area);

    // Suppress cursor when a picker overlay is visible above this form
    let picker_open =
        app.ui.show_key_picker || app.ui.show_proxyjump_picker || app.ui.show_password_picker;

    // Compute provider vault role hint for the VaultSsh field placeholder
    let vault_provider_hint: Option<(String, String)> =
        if let Screen::EditHost { alias } = &app.screen {
            app.hosts
                .iter()
                .find(|h| h.alias == *alias)
                .and_then(|h| h.provider.as_ref())
                .and_then(|prov| {
                    app.provider_config.section(prov).and_then(|s| {
                        if s.vault_role.is_empty() {
                            None
                        } else {
                            Some((s.vault_role.clone(), prov.clone()))
                        }
                    })
                })
        } else {
            None
        };

    // Symmetric hint for the VaultAddr field: show the provider default
    // address (if any) when the host-level field is empty, so the user
    // knows a provider default is already in play without having to save
    // and re-open the detail panel to find out.
    let vault_addr_provider_hint: Option<(String, String)> =
        if let Screen::EditHost { alias } = &app.screen {
            app.hosts
                .iter()
                .find(|h| h.alias == *alias)
                .and_then(|h| h.provider.as_ref())
                .and_then(|prov| {
                    app.provider_config.section(prov).and_then(|s| {
                        if s.vault_addr.is_empty() {
                            None
                        } else {
                            Some((s.vault_addr.clone(), prov.clone()))
                        }
                    })
                })
        } else {
            None
        };

    let mut y_offset: u16 = 0;
    for &(field, field_required) in visible_fields.iter() {
        let divider_y = inner.y + y_offset;
        let content_y = divider_y + 1;
        y_offset += 2;

        let is_focused = app.form.focused_field == field;
        let label_style = if is_focused {
            theme::accent_bold()
        } else {
            theme::muted()
        };
        let field_label = if app.form.is_pattern && field == FormField::Alias {
            "Pattern"
        } else {
            field.label()
        };
        let is_required = if app.form.is_pattern && field == FormField::Hostname {
            false
        } else {
            field_required
        };
        let label = if is_required {
            format!(" {}* ", field_label)
        } else {
            format!(" {} ", field_label)
        };
        render_divider(
            frame,
            block_area,
            divider_y,
            &label,
            label_style,
            theme::accent(),
        );

        let content_area = Rect::new(inner.x + 1, content_y, inner.width.saturating_sub(1), 1);
        render_field_content(
            frame,
            content_area,
            field,
            &app.form,
            picker_open,
            vault_provider_hint.as_ref(),
            vault_addr_provider_hint.as_ref(),
        );
    }

    // Footer below the block
    let footer_area = Rect::new(form_area.x, form_area.y + block_height, form_area.width, 1);
    let mut footer_spans = if app.pending_discard_confirm {
        vec![
            Span::styled(" Discard changes? ", theme::error()),
            Span::styled(" y ", theme::footer_key()),
            Span::styled(" yes ", theme::muted()),
            Span::raw("  "),
            Span::styled(" Esc ", theme::footer_key()),
            Span::styled(" no", theme::muted()),
        ]
    } else if !expanded {
        // Collapsed: show hint about more options
        vec![
            Span::styled(" Enter ", theme::footer_key()),
            Span::styled(" save ", theme::muted()),
            Span::raw("  "),
            Span::styled(" \u{2193} ", theme::footer_key()),
            Span::styled(" more options ", theme::muted()),
            Span::raw("  "),
            Span::styled(" Esc ", theme::footer_key()),
            Span::styled(" cancel", theme::muted()),
        ]
    } else {
        vec![
            Span::styled(" Enter ", theme::footer_key()),
            Span::styled(" save ", theme::muted()),
            Span::raw("  "),
            Span::styled(" Tab ", theme::footer_key()),
            Span::styled(" next ", theme::muted()),
            Span::raw("  "),
            Span::styled(" Esc ", theme::footer_key()),
            Span::styled(" cancel", theme::muted()),
        ]
    };
    if let Some(ref hint) = app.form.form_hint {
        let hint_width: usize = hint.width() + 4; // " ⚠ {hint} "
        let shortcuts_width: usize = footer_spans.iter().map(|s| s.width()).sum();
        let total = footer_area.width as usize;
        let gap = total.saturating_sub(shortcuts_width + hint_width);
        if gap > 0 {
            footer_spans.push(Span::raw(" ".repeat(gap)));
            footer_spans.push(Span::styled(format!("\u{26A0} {} ", hint), theme::error()));
        }
    }
    // Only use render_footer_with_status when no form_hint (to avoid double status)
    if app.form.form_hint.is_some() {
        frame.render_widget(Paragraph::new(Line::from(footer_spans)), footer_area);
    } else {
        super::render_footer_with_status(frame, footer_area, footer_spans, app);
    }

    // Key picker popup overlay
    if app.ui.show_key_picker {
        render_key_picker_overlay(frame, app);
    }

    // ProxyJump picker popup overlay
    if app.ui.show_proxyjump_picker {
        render_proxyjump_picker_overlay(frame, app);
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
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(Span::styled(" Select Key ", theme::brand()))
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
    let area = {
        let r = super::centered_rect(70, 80, frame.area());
        super::centered_rect_fixed(r.width, height, frame.area())
    };
    frame.render_widget(Clear, area);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" Select Key ", theme::brand()))
        .border_style(theme::accent());

    let inner_width = block.inner(area).width;

    // Column layout following containers.rs pattern
    let usable = inner_width.saturating_sub(2) as usize; // 1 highlight + 1 right margin
    let gap: usize = 2;
    let padded = |w: usize| -> usize { if w == 0 { 0 } else { w + w / 10 + 1 } };

    let name_w = padded(
        app.keys
            .iter()
            .map(|k| k.name.len())
            .max()
            .unwrap_or(4)
            .max(4),
    );
    let type_w = padded(
        app.keys
            .iter()
            .map(|k| k.type_display().len())
            .max()
            .unwrap_or(4)
            .max(4),
    );
    let left = name_w + gap + type_w;
    let comment_w = usable.saturating_sub(left + gap);
    let gap_str = " ".repeat(gap);

    let items: Vec<ListItem> = app
        .keys
        .iter()
        .map(|key| {
            let type_display = key.type_display();
            let comment = if key.comment.is_empty() {
                String::new()
            } else {
                super::truncate(&key.comment, comment_w.saturating_sub(1))
            };
            let mut spans = vec![
                Span::styled(format!(" {:<name_w$}", key.name), theme::bold()),
                Span::raw(gap_str.clone()),
                Span::styled(format!("{:<type_w$}", type_display), theme::muted()),
            ];
            if comment_w > 0 {
                spans.push(Span::raw(gap_str.clone()));
                spans.push(Span::styled(comment, theme::muted()));
            }
            let line = Line::from(spans);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(theme::selected_row())
        .highlight_symbol("  ");

    frame.render_stateful_widget(list, area, &mut app.ui.key_picker_state);
}

fn render_proxyjump_picker_overlay(frame: &mut Frame, app: &mut App) {
    let candidates = app.proxyjump_candidates();

    if candidates.is_empty() {
        let area = super::centered_rect_fixed(50, 5, frame.area());
        frame.render_widget(Clear, area);
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(Span::styled(" ProxyJump ", theme::brand()))
            .border_style(theme::accent());
        let msg = Paragraph::new(Line::from(Span::styled(
            "  No other hosts configured",
            theme::muted(),
        )))
        .block(block);
        frame.render_widget(msg, area);
        return;
    }

    let height = (candidates.len() as u16 + 2).min(16);
    let width = frame.area().width.clamp(50, 64);
    let area = super::centered_rect_fixed(width, height, frame.area());
    frame.render_widget(Clear, area);

    let alias_col = 20;
    let gap = 2;
    let host_max = (width as usize).saturating_sub(2 + 2 + 1 + alias_col + gap);

    let items: Vec<ListItem> = candidates
        .iter()
        .map(|(alias, hostname)| {
            let host_display = super::truncate(hostname, host_max);
            let line = Line::from(vec![
                Span::styled(
                    format!(
                        " {:<width$}",
                        super::truncate(alias, alias_col),
                        width = alias_col
                    ),
                    theme::bold(),
                ),
                Span::raw(" ".repeat(gap)),
                Span::styled(host_display, theme::muted()),
            ]);
            ListItem::new(line)
        })
        .collect();

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" ProxyJump ", theme::brand()))
        .border_style(theme::accent());

    let list = List::new(items)
        .block(block)
        .highlight_style(theme::selected_row())
        .highlight_symbol("  ");

    frame.render_stateful_widget(list, area, &mut app.ui.proxyjump_picker_state);
}

fn render_password_picker_overlay(frame: &mut Frame, app: &mut App) {
    let sources = crate::askpass::PASSWORD_SOURCES;
    let height = sources.len() as u16 + 5; // items + borders + spacer + footer
    let area = super::centered_rect_fixed(54, height, frame.area());
    frame.render_widget(Clear, area);

    let items: Vec<ListItem> = sources
        .iter()
        .map(|src| {
            let hint_width = src.hint.len();
            let label_width = 48_usize
                .saturating_sub(4)
                .saturating_sub(hint_width)
                .saturating_sub(1);
            let line = Line::from(vec![
                Span::styled(
                    format!(" {:<width$}", src.label, width = label_width),
                    theme::bold(),
                ),
                Span::styled(src.hint, theme::muted()),
            ]);
            ListItem::new(line)
        })
        .collect();

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" Password Source ", theme::brand()))
        .border_style(theme::accent());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split into list area + spacer + footer
    let chunks = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Min(0),
        ratatui::layout::Constraint::Length(1),
        ratatui::layout::Constraint::Length(1),
    ])
    .split(inner);

    let list = List::new(items)
        .highlight_style(theme::selected_row())
        .highlight_symbol("  ");

    frame.render_stateful_widget(list, chunks[0], &mut app.ui.password_picker_state);

    let spans = vec![
        Span::styled(" Enter ", theme::footer_key()),
        Span::styled(" select ", theme::muted()),
        Span::raw("  "),
        Span::styled(" Ctrl+D ", theme::footer_key()),
        Span::styled(" global default ", theme::muted()),
        Span::raw("  "),
        Span::styled(" Esc ", theme::footer_key()),
        Span::styled(" cancel", theme::muted()),
    ];
    super::render_footer_with_status(frame, chunks[2], spans, app);
}

/// Get the placeholder text for a field (public for tests).
#[cfg(test)]
pub fn placeholder_text(field: FormField) -> String {
    placeholder_for(field, false)
}

#[cfg(test)]
pub fn placeholder_text_pattern(field: FormField) -> String {
    placeholder_for(field, true)
}

/// Delegate to shared render_divider in mod.rs.
fn render_divider(
    frame: &mut Frame,
    block_area: Rect,
    y: u16,
    label: &str,
    label_style: Style,
    border_style: Style,
) {
    super::render_divider(frame, block_area, y, label, label_style, border_style);
}

/// Render a single field's content (value or placeholder) and set cursor.
#[allow(clippy::too_many_arguments)]
fn render_field_content(
    frame: &mut Frame,
    area: Rect,
    field: FormField,
    form: &crate::app::HostForm,
    picker_open: bool,
    vault_provider_hint: Option<&(String, String)>,
    vault_addr_provider_hint: Option<&(String, String)>,
) {
    let is_focused = form.focused_field == field;

    let value = match field {
        FormField::Alias => &form.alias,
        FormField::Hostname => &form.hostname,
        FormField::User => &form.user,
        FormField::Port => &form.port,
        FormField::IdentityFile => &form.identity_file,
        FormField::ProxyJump => &form.proxy_jump,
        FormField::AskPass => &form.askpass,
        FormField::VaultSsh => &form.vault_ssh,
        FormField::VaultAddr => &form.vault_addr,
        FormField::Tags => &form.tags,
    };

    let is_picker = matches!(
        field,
        FormField::IdentityFile | FormField::ProxyJump | FormField::AskPass
    );

    // Inherited hint for this field (value + source pattern).
    let inherited_hint = match field {
        FormField::ProxyJump => form.inherited.proxy_jump.as_ref(),
        FormField::User => form.inherited.user.as_ref(),
        FormField::IdentityFile => form.inherited.identity_file.as_ref(),
        _ => None,
    };

    // Inherited hints are shown regardless of focus (unlike input placeholders) because
    // they are informational: they show the effective SSH config, not an input prompt.
    let content = if let (true, Some((inh_val, inh_src))) = (value.is_empty(), inherited_hint) {
        let inner_width = area.width as usize;
        // Detect self-referencing ProxyJump loop.
        let is_loop = field == FormField::ProxyJump
            && crate::ssh_config::model::proxy_jump_contains_self(inh_val, &form.alias);
        if is_loop {
            let msg = format!("loops via {}", inh_src);
            let display = super::truncate(&msg, inner_width);
            Line::from(vec![Span::styled(display, theme::error())])
        } else {
            let source_suffix = format!("  \u{2190} {}", inh_src);
            let val_budget = inner_width.saturating_sub(source_suffix.width());
            let display = super::truncate(inh_val, val_budget);
            if is_picker && is_focused {
                let arrow_pos = inner_width.saturating_sub(1);
                let used = display.width() + source_suffix.width();
                let gap = arrow_pos.saturating_sub(used);
                Line::from(vec![
                    Span::styled(display, theme::muted()),
                    Span::styled(source_suffix, theme::muted()),
                    Span::raw(" ".repeat(gap)),
                    Span::styled("\u{25B8}", theme::muted()),
                ])
            } else {
                Line::from(vec![
                    Span::styled(display, theme::muted()),
                    Span::styled(source_suffix, theme::muted()),
                ])
            }
        }
    } else if let (true, FormField::VaultSsh, Some((role, prov))) =
        (value.is_empty(), field, vault_provider_hint)
    {
        let hint = format!("inherits {} from {}", role, prov);
        Line::from(Span::styled(hint, theme::muted()))
    } else if let (true, FormField::VaultAddr, Some((addr, prov))) =
        (value.is_empty(), field, vault_addr_provider_hint)
    {
        let hint = format!("inherits {} from {}", addr, prov);
        Line::from(Span::styled(hint, theme::muted()))
    } else if value.is_empty() && is_focused && !is_picker {
        let ph = placeholder_for(field, form.is_pattern);
        Line::from(Span::styled(ph, theme::muted()))
    } else if is_picker && is_focused {
        let inner_width = area.width as usize;
        let arrow_pos = inner_width.saturating_sub(1);
        let (display, display_style) = if value.is_empty() {
            (placeholder_for(field, form.is_pattern), theme::muted())
        } else {
            (value.to_string(), theme::bold())
        };
        let val_width = display.width();
        let gap = arrow_pos.saturating_sub(val_width);
        Line::from(vec![
            Span::styled(display, display_style),
            Span::raw(" ".repeat(gap)),
            Span::styled("\u{25B8}", theme::muted()),
        ])
    } else if value.is_empty() {
        Line::from(Span::raw(""))
    } else {
        Line::from(Span::styled(value.to_string(), theme::bold()))
    };

    frame.render_widget(Paragraph::new(content), area);

    if is_focused && !picker_open {
        let prefix: String = value.chars().take(form.cursor_pos).collect();
        let cursor_x = area
            .x
            .saturating_add(prefix.width().min(u16::MAX as usize) as u16);
        let cursor_y = area.y;
        if area.width > 0 && cursor_x < area.x.saturating_add(area.width) {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::{Constraint, Layout, Rect};

    #[test]
    fn password_picker_layout_has_spacer() {
        let area = Rect::new(0, 0, 54, 15);
        let chunks = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);
        assert_eq!(chunks[1].height, 1, "spacer row should be 1 tall");
        assert_eq!(chunks[2].height, 1, "footer row should be 1 tall");
        assert!(
            chunks[2].y > chunks[0].y + chunks[0].height,
            "footer should be below content end"
        );
    }
}
