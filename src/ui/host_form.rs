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
        FormField::Alias => "e.g. prod or db-01".to_string(),
        FormField::Hostname => "192.168.1.1 or example.com".to_string(),
        FormField::User => "root".to_string(),
        FormField::Port => "22".to_string(),
        FormField::IdentityFile => "Enter to pick a key".to_string(),
        FormField::ProxyJump => "Enter to pick a host".to_string(),
        // SSH secrets engine role (signs SSH certificates). Distinct from
        // Vault KV used in Password Source (vault:path/to/secret).
        FormField::VaultSsh => {
            "e.g. ssh-client-signer/sign/my-role (auth via vault login)".to_string()
        }
        FormField::VaultAddr => {
            "e.g. http://127.0.0.1:8200 (inherits from provider or env when empty)".to_string()
        }
        FormField::Tags => "e.g. prod, staging, us-east (comma-separated)".to_string(),
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
    let picker_open = app.ui.show_key_picker
        || app.ui.show_proxyjump_picker
        || app.ui.show_password_picker
        || app.ui.show_vault_role_picker;
    let has_vault_roles = !app.vault_role_candidates().is_empty();

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
            has_vault_roles,
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

    // Vault role picker popup overlay
    if app.ui.show_vault_role_picker {
        render_vault_role_picker_overlay(frame, app);
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
        super::render_picker_empty_overlay(frame, "ProxyJump", "No other hosts configured");
        return;
    }

    let width = super::picker_overlay_width(frame);
    // Row content width used by all items (Host, SectionLabel,
    // Separator). Matches the password picker's `inner_width` so both
    // overlays right-align their secondary column against the same
    // visual edge: overlay width − 2 borders − 2 highlight-gutter − 1
    // leading space − 1 trailing margin.
    let inner = (width as usize).saturating_sub(6);
    let alias_col = 20;
    let min_gap = 2;
    let host_max = inner.saturating_sub(alias_col + min_gap);

    let items: Vec<ListItem> = candidates
        .iter()
        .map(|candidate| match candidate {
            crate::app::ProxyJumpCandidate::SectionLabel(label) => ListItem::new(Line::from(
                Span::styled(format!("  {}", label.to_ascii_uppercase()), theme::muted()),
            )),
            crate::app::ProxyJumpCandidate::Separator => ListItem::new(Line::from(Span::styled(
                // Two leading spaces to match the SectionLabel indent,
                // then dashes that span the remainder of `inner` so
                // the separator has the same visual width as a Host
                // row.
                "  ".to_string() + &"─".repeat(inner.saturating_sub(2)),
                theme::muted(),
            ))),
            crate::app::ProxyJumpCandidate::Host {
                alias, hostname, ..
            } => {
                let alias_display = super::truncate(alias, alias_col);
                let host_display = super::truncate(hostname, host_max);
                // Right-align the hostname by padding the alias to
                // consume the remainder of `inner`. Use the hostname's
                // unicode display width (not `chars().count()`) so CJK
                // and wide glyphs in a hostname do not overflow the
                // right border. `alias_col` floors the padding so an
                // unusually long hostname on a narrow terminal never
                // collapses the alias column below its minimum width.
                let host_width = host_display.width();
                let alias_width = inner
                    .saturating_sub(host_width)
                    .saturating_sub(1)
                    .max(alias_col);
                let line = Line::from(vec![
                    Span::styled(
                        format!(" {:<width$}", alias_display, width = alias_width),
                        theme::bold(),
                    ),
                    Span::styled(host_display, theme::muted()),
                ]);
                ListItem::new(line)
            }
        })
        .collect();

    super::render_picker_overlay(
        frame,
        "ProxyJump",
        None,
        items,
        &mut app.ui.proxyjump_picker_state,
        16,
    );
}

fn render_vault_role_picker_overlay(frame: &mut Frame, app: &mut App) {
    let candidates = app.vault_role_candidates();

    let width = super::picker_overlay_width(frame);
    let max_role = (width as usize).saturating_sub(6);
    let items: Vec<ListItem> = candidates
        .iter()
        .map(|role| {
            ListItem::new(Line::from(Span::styled(
                format!("  {}", super::truncate(role, max_role)),
                theme::bold(),
            )))
        })
        .collect();

    super::render_picker_overlay(
        frame,
        "Vault SSH Role",
        None,
        items,
        &mut app.ui.vault_role_picker_state,
        12,
    );
}

fn render_password_picker_overlay(frame: &mut Frame, app: &mut App) {
    let sources = crate::askpass::PASSWORD_SOURCES;
    let width = super::picker_overlay_width(frame);
    // Inner usable width = overlay width − 2 borders − highlight gutter (2)
    // − left label pad (1) − one trailing space before the hint.
    let inner_width = (width as usize).saturating_sub(6);
    let items: Vec<ListItem> = sources
        .iter()
        .map(|src| {
            let hint_width = src.hint.len();
            let label_width = inner_width.saturating_sub(hint_width).saturating_sub(1);
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

    super::render_picker_overlay(
        frame,
        "Password Source",
        Some("Ctrl+D: global default"),
        items,
        &mut app.ui.password_picker_state,
        16,
    );
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
    has_vault_roles: bool,
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
    ) || (field == FormField::VaultSsh && has_vault_roles);

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
            let ph = if field == FormField::VaultSsh {
                "Enter to pick a role or type one".to_string()
            } else {
                placeholder_for(field, form.is_pattern)
            };
            (ph, theme::muted())
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
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    fn make_app() -> App {
        let config = crate::ssh_config::model::SshConfigFile {
            elements: crate::ssh_config::model::SshConfigFile::parse_content(""),
            path: PathBuf::from("/tmp/test_config"),
            crlf: false,
            bom: false,
        };
        App::new(config)
    }

    fn buffer_dump(buf: &ratatui::buffer::Buffer) -> String {
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    /// Pin the wiring between `render_password_picker_overlay` and the
    /// shared `render_picker_overlay` helper: the Ctrl+D affordance
    /// must reach the rendered buffer via the title hint at this
    /// specific callsite. A future edit that passes `None` or alters
    /// the literal would fail this test.
    #[test]
    fn render_password_picker_overlay_shows_ctrl_d_hint_in_title() {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = make_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(0));
        terminal
            .draw(|frame| {
                render_password_picker_overlay(frame, &mut app);
                let dump = buffer_dump(frame.buffer_mut());
                assert!(
                    dump.contains("Password Source · Ctrl+D: global default"),
                    "password picker must surface Ctrl+D hint in title, got:\n{dump}"
                );
            })
            .unwrap();
    }

    /// Negative assertion pinning the intent of the refactor: after
    /// moving the Ctrl+D affordance to the title, no row of the
    /// rendered overlay should contain a "Ctrl+D" footer. Prevents a
    /// regression where a future edit accidentally re-introduces a
    /// divergent per-picker footer.
    #[test]
    fn render_password_picker_overlay_has_no_footer_row_with_ctrl_d() {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = make_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(0));
        terminal
            .draw(|frame| {
                render_password_picker_overlay(frame, &mut app);
                let buf = frame.buffer_mut();
                // Only the title row (top border) may contain "Ctrl+D".
                // Every other row must be free of that string so a
                // footer reintroduction is caught immediately.
                let mut title_row: Option<u16> = None;
                for y in 0..buf.area.height {
                    let mut row = String::new();
                    for x in 0..buf.area.width {
                        row.push_str(buf[(x, y)].symbol());
                    }
                    if row.contains("Password Source") {
                        title_row = Some(y);
                        break;
                    }
                }
                let title_row = title_row.expect("title row must exist");
                for y in 0..buf.area.height {
                    if y == title_row {
                        continue;
                    }
                    let mut row = String::new();
                    for x in 0..buf.area.width {
                        row.push_str(buf[(x, y)].symbol());
                    }
                    assert!(
                        !row.contains("Ctrl+D"),
                        "row {y} must not contain 'Ctrl+D' (footer regression): {row:?}"
                    );
                }
            })
            .unwrap();
    }

    /// Build a ProxyJump picker app with the given SSH config content,
    /// open the edit screen for `editing_alias`, and select the first
    /// available host in the picker. Returns an `App` ready for
    /// `render_proxyjump_picker_overlay`.
    fn proxyjump_picker_fixture(config_text: &str, editing_alias: &str) -> App {
        let cfg = crate::ssh_config::model::SshConfigFile {
            elements: crate::ssh_config::model::SshConfigFile::parse_content(config_text),
            path: PathBuf::from("/tmp/test"),
            crlf: false,
            bom: false,
        };
        let mut app = App::new(cfg);
        app.screen = Screen::EditHost {
            alias: editing_alias.to_string(),
        };
        app.ui.show_proxyjump_picker = true;
        app.ui
            .proxyjump_picker_state
            .select(app.proxyjump_first_host_index());
        app
    }

    /// Locate the first row and column range containing `needle` in a
    /// terminal buffer by scanning cell-by-cell. Returns (row, end_col)
    /// where `end_col` is the inclusive column of the last cell of
    /// the match. Avoids the byte-vs-column mismatch that comes from
    /// `str::find` on a row with multi-byte border glyphs.
    fn find_needle_in_buffer(
        buf: &ratatui::buffer::Buffer,
        needle: &str,
    ) -> Option<(u16, u16, u16)> {
        let chars: Vec<String> = needle.chars().map(|c| c.to_string()).collect();
        let len = chars.len() as u16;
        if len == 0 || buf.area.width < len {
            return None;
        }
        for y in 0..buf.area.height {
            for start_x in 0..=buf.area.width - len {
                let matches = (0..len).all(|i| buf[(start_x + i, y)].symbol() == chars[i as usize]);
                if matches {
                    return Some((y, start_x, start_x + len - 1));
                }
            }
        }
        None
    }

    /// Return the rightmost border glyph column on the given row.
    fn right_border_col(buf: &ratatui::buffer::Buffer, y: u16) -> Option<u16> {
        for x in (0..buf.area.width).rev() {
            let s = buf[(x, y)].symbol();
            if s == "│" || s == "╮" || s == "╯" {
                return Some(x);
            }
        }
        None
    }

    /// The ProxyJump picker's hostname column should end at the right
    /// edge of the inner content area, mirroring the password picker
    /// layout. We verify this by locating the hostname substring on
    /// its rendered row and checking that no non-space glyph follows
    /// before the right border.
    #[test]
    fn render_proxyjump_picker_host_column_is_right_aligned() {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = proxyjump_picker_fixture(
            concat!(
                "Host editing\n  HostName 9.9.9.9\n",
                "Host plain\n  HostName 1.1.1.1\n",
            ),
            "editing",
        );
        terminal
            .draw(|frame| {
                render_proxyjump_picker_overlay(frame, &mut app);
                let buf = frame.buffer_mut();
                let (y, _start, end_col) = find_needle_in_buffer(buf, "1.1.1.1")
                    .expect("candidate host row must render '1.1.1.1'");
                let border = right_border_col(buf, y).expect("right border on host row");
                let gap = border.saturating_sub(end_col);
                assert!(
                    end_col < border && gap <= 3,
                    "hostname must end flush with right border (end_col={end_col}, border_x={border}, gap={gap})"
                );
            })
            .unwrap();
    }

    /// A hostname long enough to hit the truncation limit must still
    /// stay strictly inside the right border — no overflow past the
    /// inner area regardless of how much the alias + hostname together
    /// would otherwise claim.
    #[test]
    fn render_proxyjump_picker_long_hostname_does_not_overflow() {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        // 55-char hostname forces truncation (host_max at width=64 is
        // inner(58) - alias_col(20) - min_gap(2) = 36).
        let mut app = proxyjump_picker_fixture(
            concat!(
                "Host editing\n  HostName 9.9.9.9\n",
                "Host plain\n  HostName very-long-hostname-that-should-be-truncated.example.com\n",
            ),
            "editing",
        );
        terminal
            .draw(|frame| {
                render_proxyjump_picker_overlay(frame, &mut app);
                let buf = frame.buffer_mut();
                // Find the row that renders the truncated hostname
                // prefix; the full string will not fit so we anchor on
                // a prefix that is guaranteed to survive truncation.
                let (y, _start, end_col) = find_needle_in_buffer(buf, "very-long-hostname")
                    .expect("truncated hostname prefix must render");
                let border = right_border_col(buf, y).expect("right border on host row");
                assert!(
                    end_col < border,
                    "truncated hostname must not overflow right border (end_col={end_col}, border_x={border})"
                );
            })
            .unwrap();
    }

    /// On the minimum-width overlay (50 cols), the right-align math
    /// must still place the hostname inside the right border without
    /// collapsing the alias column below its floor.
    #[test]
    fn render_proxyjump_picker_right_aligns_on_narrow_terminal() {
        // Terminal width equals the overlay minimum width clamp so the
        // picker uses PICKER_MIN_WIDTH (50) exactly.
        let backend = TestBackend::new(50, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = proxyjump_picker_fixture(
            concat!(
                "Host editing\n  HostName 9.9.9.9\n",
                "Host plain\n  HostName 1.1.1.1\n",
            ),
            "editing",
        );
        terminal
            .draw(|frame| {
                render_proxyjump_picker_overlay(frame, &mut app);
                let buf = frame.buffer_mut();
                let (y, _start, end_col) = find_needle_in_buffer(buf, "1.1.1.1")
                    .expect("hostname must render on narrow terminal");
                let border = right_border_col(buf, y).expect("right border present");
                assert!(
                    end_col < border && border - end_col <= 3,
                    "right-align must hold on narrow terminal (end_col={end_col}, border_x={border})"
                );
            })
            .unwrap();
    }

    /// When a host is promoted into the suggested section and rendered
    /// below a `SectionLabel`, the right-align layout must still apply
    /// so the two sections of the picker share a visual right edge.
    #[test]
    fn render_proxyjump_picker_right_aligns_suggested_host_below_label() {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        // `bastion` scores via the keyword heuristic so it is promoted
        // into the suggested section below a `SectionLabel`.
        let mut app = proxyjump_picker_fixture(
            concat!(
                "Host editing\n  HostName 9.9.9.9\n",
                "Host bastion\n  HostName 1.2.3.4\n",
                "Host plain\n  HostName 5.6.7.8\n",
            ),
            "editing",
        );
        terminal
            .draw(|frame| {
                render_proxyjump_picker_overlay(frame, &mut app);
                let buf = frame.buffer_mut();
                // Locate the SectionLabel row so we can anchor the
                // search for the suggested host strictly below it.
                let (label_y, _, _) = find_needle_in_buffer(buf, "SUGGESTIONS")
                    .expect("SectionLabel must render above the suggested host");
                let (y, _start, end_col) = find_needle_in_buffer(buf, "1.2.3.4")
                    .expect("suggested host must render");
                assert!(
                    y > label_y,
                    "suggested host must render below the SectionLabel (label_y={label_y}, host_y={y})"
                );
                let border = right_border_col(buf, y).expect("right border on host row");
                assert!(
                    end_col < border && border - end_col <= 3,
                    "suggested host must right-align (end_col={end_col}, border_x={border})"
                );
            })
            .unwrap();
    }

    /// Both Host rows in the same picker must share the same right
    /// edge. Prevents a regression where one row miscomputes the
    /// right-align math while another keeps it correct.
    #[test]
    fn render_proxyjump_picker_multiple_hosts_share_right_edge() {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = proxyjump_picker_fixture(
            concat!(
                "Host editing\n  HostName 9.9.9.9\n",
                "Host host-a\n  HostName 1.1.1.1\n",
                "Host host-b\n  HostName 2.2.2.2\n",
            ),
            "editing",
        );
        terminal
            .draw(|frame| {
                render_proxyjump_picker_overlay(frame, &mut app);
                let buf = frame.buffer_mut();
                let (y1, _, end1) =
                    find_needle_in_buffer(buf, "1.1.1.1").expect("host-a row must render");
                let (y2, _, end2) =
                    find_needle_in_buffer(buf, "2.2.2.2").expect("host-b row must render");
                assert_ne!(y1, y2, "two distinct rows expected");
                assert_eq!(
                    end1, end2,
                    "both hostnames must end at the same column (end1={end1}, end2={end2})"
                );
            })
            .unwrap();
    }

    /// The `min_gap = 2` contract must leave at least two spaces
    /// between the end of the alias column and the start of the
    /// hostname column so the two visually distinct columns never run
    /// into each other.
    #[test]
    fn render_proxyjump_picker_preserves_minimum_gap_between_columns() {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = proxyjump_picker_fixture(
            concat!(
                "Host editing\n  HostName 9.9.9.9\n",
                "Host a\n  HostName 1.1.1.1\n",
            ),
            "editing",
        );
        terminal
            .draw(|frame| {
                render_proxyjump_picker_overlay(frame, &mut app);
                let buf = frame.buffer_mut();
                let (y, host_start, _) = find_needle_in_buffer(buf, "1.1.1.1")
                    .expect("hostname must render for gap check");
                // Walk left from the hostname start until we hit a
                // non-space cell — that is the alias column's last
                // glyph. Count the intervening spaces.
                let mut gap = 0_u16;
                let mut x = host_start;
                while x > 0 {
                    x -= 1;
                    if buf[(x, y)].symbol() == " " {
                        gap += 1;
                    } else {
                        break;
                    }
                }
                assert!(
                    gap >= 2,
                    "at least two spaces must separate alias and hostname columns (gap={gap})"
                );
            })
            .unwrap();
    }
}
