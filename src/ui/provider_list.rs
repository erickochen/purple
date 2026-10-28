use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::design;
use super::theme;
use crate::app::{App, ProviderFormField};
use crate::history::ConnectionHistory;
use crate::providers::ProviderKind;

/// Render the provider management list as a centered overlay.
pub fn render_provider_list(frame: &mut Frame, app: &mut App) {
    let rows = app.providers.provider_list_rows();

    // Overlay: percentage-based width, height fits content. Reserve 1 row
    // below the block for the external footer.
    let item_count = rows.len();
    let height = (item_count as u16 + 3).min(frame.area().height.saturating_sub(5));
    let area = design::overlay_area(frame, design::OVERLAY_W, design::OVERLAY_H, height);
    frame.render_widget(Clear, area);

    let block = design::overlay_block("Providers");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Content width inside the overlay
    let content_width = inner.width as usize;

    // Reserve room for the tree-glyph column ONLY when at least one provider
    // in the list actually has 2+ configs. Otherwise every row would carry
    // 3 dead spaces of padding for an arrow that never renders, making the
    // overlay look wider than tunnel_list and key_list. Match design-system
    // convention: padding flows directly from LIST_HIGHLIGHT, no extra inset.
    let has_multi_config = rows.iter().any(|r| {
        matches!(
            r,
            crate::app::ProviderRow::Header {
                config_count,
                ..
            } if *config_count >= 2
        )
    });

    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| match row {
            crate::app::ProviderRow::Header { name, config_count } => {
                render_header_line(app, name, *config_count, content_width, has_multi_config)
            }
            crate::app::ProviderRow::Leaf { id } => render_leaf_line(app, id, content_width),
        })
        .collect();

    let list = List::new(items)
        .highlight_style(theme::selected_row())
        .highlight_symbol(design::LIST_HIGHLIGHT);

    frame.render_stateful_widget(list, inner, app.ui.provider_list_state_mut());

    // Footer below the block
    let footer_area = design::render_overlay_footer(frame, area);
    if app.providers.pending_delete().is_some() {
        let name = app.providers.pending_delete().unwrap_or("");
        let display = crate::providers::provider_display_name(name);
        // When removing one labeled config of a multi-config provider,
        // surface the label so the user can't mistake it for "remove all".
        let question = match app
            .providers
            .pending_delete_id()
            .and_then(|id| id.label.as_deref())
        {
            Some(label) => {
                crate::messages::confirm_provider_remove_labeled_question(display, label)
            }
            None => crate::messages::confirm_provider_remove_question(display),
        };
        let detail = crate::messages::confirm_provider_remove_detail(&app.config_path_display());
        // Destructive confirm: centred popup, action verbs both sides.
        design::render_destructive_popup(
            frame,
            crate::messages::CONFIRM_PROVIDER_REMOVE_TITLE,
            &question,
            &detail,
            "remove",
            "keep",
            app,
        );
        super::render_footer_with_status(frame, footer_area, Vec::new(), app);
    } else {
        // Count stale hosts for the currently selected row.
        let selected_row = app
            .ui
            .provider_list_state()
            .selected()
            .and_then(|idx| rows.get(idx));
        let selected_stale_count: usize = selected_row
            .map(|row| match row {
                crate::app::ProviderRow::Header { name, .. } => app
                    .hosts_state
                    .list()
                    .iter()
                    .filter(|h| h.stale.is_some() && h.provider.as_deref() == Some(name.as_str()))
                    .count(),
                crate::app::ProviderRow::Leaf { id } => app
                    .hosts_state
                    .list()
                    .iter()
                    .filter(|h| {
                        h.stale.is_some()
                            && h.provider.as_deref() == Some(id.provider.as_str())
                            && h.provider_label.as_deref() == id.label.as_deref()
                    })
                    .count(),
            })
            .unwrap_or(0);

        // Header on a multi-config provider: Enter expands; otherwise edits.
        // Single-word labels match the project convention (`add`, `del`,
        // `edit`) used in tunnel_list, host_list, key_list.
        use crate::messages::footer as fl;
        let edit_label = match selected_row {
            Some(crate::app::ProviderRow::Header { config_count, name }) if *config_count >= 2 => {
                if app.providers.expanded_providers().contains(name) {
                    fl::ENTER_COLLAPSE
                } else {
                    fl::ENTER_EXPAND
                }
            }
            _ => fl::ENTER_EDIT,
        };
        let mut f = design::Footer::new()
            .primary("Enter", edit_label)
            .action("a", fl::ACTION_ADD)
            .action("s", fl::ACTION_SYNC)
            .action("d", fl::ACTION_DEL);
        if selected_stale_count > 0 {
            f = f.action("X", &format!(" purge {} stale ", selected_stale_count));
        }
        f = f.action("Esc", fl::ESC_BACK);
        f.render_with_status(frame, footer_area, app);
    }
}

/// Render a provider group header row in the tree-style list.
/// `config_count`: 0 = unconfigured, 1 = single config (no expand arrow),
/// 2+ = multi-config (shows ▾/▸ depending on expand state).
/// `has_multi_config_in_list`: if false, no row in the list needs the
/// tree-glyph column, so single/zero-config rows skip the 3-space padding
/// and start flush against LIST_HIGHLIGHT (matches tunnel_list/key_list).
fn render_header_line(
    app: &App,
    name: &str,
    config_count: usize,
    content_width: usize,
    has_multi_config_in_list: bool,
) -> ListItem<'static> {
    let display_name = crate::providers::provider_display_name(name);
    let configured = config_count > 0;

    // Expand indicator for multi-config providers, alignment whitespace
    // for siblings, or nothing at all when the list contains no
    // multi-config providers.
    let prefix = if config_count >= 2 {
        if app.providers.expanded_providers().contains(name) {
            format!(" {} ", design::TREE_EXPANDED)
        } else {
            format!(" {} ", design::TREE_COLLAPSED)
        }
    } else if has_multi_config_in_list {
        "   ".to_string()
    } else {
        String::new()
    };

    let name_col = format!("{}{:<14}", prefix, display_name);
    let mut spans = vec![Span::styled(name_col, theme::bold())];
    let mut used = 17;

    if !configured {
        return ListItem::new(Line::from(spans));
    }

    if config_count >= 2 {
        let suffix = format!(" ({} configs)", config_count);
        spans.push(Span::styled(suffix, theme::muted()));
        return ListItem::new(Line::from(spans));
    }

    // Single config: look up by the section's full id (matches the key
    // used in sync_history insertion) so the status icon and timestamp
    // surface for both bare configs and labeled-but-only-one configs.
    let single_section = app
        .providers
        .config()
        .sections_for_provider(name)
        .first()
        .cloned()
        .cloned();
    let key = single_section
        .as_ref()
        .map(|s| s.id.to_string())
        .unwrap_or_else(|| name.to_string());
    let has_error = app
        .providers
        .sync_history()
        .get(&key)
        .is_some_and(|r| r.is_error);
    if has_error {
        spans.push(Span::styled(design::ICON_WARNING, theme::error()));
    } else {
        spans.push(Span::styled(design::ICON_SUCCESS, theme::success()));
    }
    used += 1;

    if let Some(section) = single_section.as_ref()
        && !section.auto_sync
    {
        spans.push(Span::styled(" (manual)", theme::muted()));
        used += 9;
    }

    let stale_count = app
        .hosts_state
        .list()
        .iter()
        .filter(|h| h.stale.is_some() && h.provider.as_deref() == Some(name))
        .count();

    append_sync_detail(app, &key, stale_count, used, content_width, &mut spans);
    ListItem::new(Line::from(spans))
}

/// Render a leaf row (one labeled config) under an expanded provider header.
fn render_leaf_line(
    app: &App,
    id: &crate::providers::config::ProviderConfigId,
    content_width: usize,
) -> ListItem<'static> {
    let label = id.label.as_deref().unwrap_or("");
    // Indented under the header, with a tree-branch glyph.
    let name_col = format!("    {} {:<12}", design::TREE_BRANCH, label);
    let mut spans = vec![Span::styled(name_col, theme::bold())];
    let mut used = 17;

    let key = id.to_string();
    let has_error = app
        .providers
        .sync_history()
        .get(&key)
        .is_some_and(|r| r.is_error);
    if has_error {
        spans.push(Span::styled(design::ICON_WARNING, theme::error()));
    } else {
        spans.push(Span::styled(design::ICON_SUCCESS, theme::success()));
    }
    used += 1;

    if let Some(section) = app.providers.config().section_by_id(id)
        && !section.auto_sync
    {
        spans.push(Span::styled(" (manual)", theme::muted()));
        used += 9;
    }

    let stale_count = app
        .hosts_state
        .list()
        .iter()
        .filter(|h| {
            h.stale.is_some()
                && h.provider.as_deref() == Some(id.provider.as_str())
                && h.provider_label.as_deref() == id.label.as_deref()
        })
        .count();

    append_sync_detail(app, &key, stale_count, used, content_width, &mut spans);
    ListItem::new(Line::from(spans))
}

/// Append "syncing..." or "N servers, M stale, T ago" to a row.
fn append_sync_detail(
    app: &App,
    key: &str,
    stale_count: usize,
    used: usize,
    content_width: usize,
    spans: &mut Vec<Span<'static>>,
) {
    if app.providers.syncing().contains_key(key) {
        let max = content_width.saturating_sub(used + 2);
        if max > 1 {
            spans.push(Span::styled(
                format!("  {}", super::truncate("syncing...", max)),
                theme::muted(),
            ));
        }
        return;
    }
    let Some(record) = app.providers.sync_history().get(key) else {
        return;
    };
    let ago = ConnectionHistory::format_time_ago(record.timestamp);
    let prefix = format!("  {}", record.message);
    let stale_text = if stale_count > 0 {
        format!(", {} stale", stale_count)
    } else {
        String::new()
    };
    let ago_text = if ago.is_empty() {
        String::new()
    } else {
        format!(", {} ago", ago)
    };
    let max = content_width.saturating_sub(used);
    let total_len = prefix.len() + stale_text.len() + ago_text.len();
    if max > 1 && total_len <= max {
        spans.push(Span::styled(prefix, theme::muted()));
        if stale_count > 0 {
            spans.push(Span::styled(stale_text, theme::warning()));
        }
        if !ago_text.is_empty() {
            spans.push(Span::styled(ago_text, theme::muted()));
        }
    } else if max > 1 {
        let combined = format!("{}{}{}", prefix, stale_text, ago_text);
        spans.push(Span::styled(
            super::truncate(&combined, max),
            theme::muted(),
        ));
    }
}

/// Render step 1 of the lazy add-second-config flow as a proper form.
/// Composes the same primitives as `render_provider_form`: per-field
/// dividers, accent_bold focus style, real cursor positioning, and the
/// standard `form_save_footer`. No ad-hoc spans or fake `[___]` boxes.
pub fn render_label_migration(frame: &mut Frame, app: &mut App, provider_name: &str) {
    let display_name = crate::providers::provider_display_name(provider_name);
    let title = format!("Add a 2nd {} config", display_name);

    // Block layout: 1 top pad + 2 field rows (divider+content each = 4 rows)
    // + 1 trailing pad = 6 inner rows. Plus borders = 8. The title implies
    // the why ("you have one and want a second"); the action verb in the
    // field labels carries the instruction.
    let block_height: u16 = 8;
    let area = design::overlay_area(frame, design::OVERLAY_W, design::OVERLAY_H, block_height);
    frame.render_widget(Clear, area);

    let block = design::overlay_block(&title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (existing, new, focused, cursor_pos) = match app.providers.pending_label_migration() {
        Some(p) => (
            p.existing_label.as_str(),
            p.new_label.as_str(),
            p.focused,
            p.cursor_pos,
        ),
        None => ("", "", crate::app::LabelMigrationField::Existing, 0),
    };

    let fields = [
        (
            crate::app::LabelMigrationField::Existing,
            crate::messages::LABEL_MIGRATION_FIELD_CURRENT,
            existing,
        ),
        (
            crate::app::LabelMigrationField::New,
            crate::messages::LABEL_MIGRATION_FIELD_NEW,
            new,
        ),
    ];

    let mut y_offset: u16 = 1; // skip top pad
    let mut focused_cursor: Option<(u16, u16)> = None;
    for (field, label, value) in fields {
        let divider_y = inner.y + y_offset;
        let content_y = divider_y + 1;
        y_offset += 2;

        let is_focused = focused == field;
        let label_style = if is_focused {
            theme::accent_bold()
        } else {
            theme::muted()
        };
        super::render_divider(
            frame,
            area,
            divider_y,
            label,
            label_style,
            theme::border_dim(),
        );

        let content_area = Rect::new(inner.x + 1, content_y, inner.width.saturating_sub(1), 1);
        let content = if value.is_empty() {
            Line::from(Span::raw(""))
        } else {
            Line::from(Span::styled(value.to_string(), theme::bold()))
        };
        frame.render_widget(Paragraph::new(content), content_area);
        if is_focused {
            let prefix: String = value.chars().take(cursor_pos).collect();
            let cursor_x = content_area
                .x
                .saturating_add(prefix.width().min(u16::MAX as usize) as u16);
            if content_area.width > 0
                && cursor_x < content_area.x.saturating_add(content_area.width)
            {
                focused_cursor = Some((cursor_x, content_area.y));
            }
        }
    }

    // Standard form save footer: Enter save / Tab next / Esc cancel.
    let footer_area = design::render_overlay_footer(frame, area);
    design::form_save_footer(design::FormFooterMode::Expanded(design::FieldKind::Text))
        .render_with_status(frame, footer_area, app);

    if let Some((x, y)) = focused_cursor {
        frame.set_cursor_position((x, y));
    }
}

/// Render the provider configuration form.
pub fn render_provider_form(frame: &mut Frame, app: &mut App, provider_name: &str) {
    let kind = provider_name.parse::<ProviderKind>().ok();
    let display_name = crate::providers::provider_display_name(provider_name);
    let title = format!("Providers > {}", display_name);

    let expanded = app.providers.form().expanded;
    // Progressive disclosure: when `vault_role` is empty, `VaultAddr` is
    // filtered out by `visible_fields(provider)` and therefore never
    // rendered or navigable. Re-enabling the role brings the field back
    // with whatever value the user had previously typed.
    let filtered_all: Vec<ProviderFormField> = app.providers.form().visible_fields(provider_name);
    let all_fields: &[ProviderFormField] = &filtered_all;
    let required_count = all_fields
        .iter()
        .filter(|f| ProviderFormField::is_required_field(**f, provider_name))
        .count();
    // VaultRole and VaultAddr are both optional fields and are gated behind
    // the expanded state, identical to every other non-required field.
    // Per-host VaultSsh (in host_form.rs) follows the same rule.
    let base_fields: &[ProviderFormField] = if expanded {
        all_fields
    } else {
        // Required fields are always first in fields_for() ordering
        &all_fields[..required_count]
    };
    let visible_fields: &[ProviderFormField] = base_fields;
    // Block: top(1) + fields * 2 (divider + content) + bottom(1)
    let block_height = 2 + visible_fields.len() as u16 * 2;
    let total_height = block_height + 1; // + footer

    let form_area = design::overlay_area(frame, design::OVERLAY_W, design::OVERLAY_H, total_height);
    frame.render_widget(Clear, form_area);

    let block_area = Rect::new(form_area.x, form_area.y, form_area.width, block_height);

    let block = design::overlay_block(&title);

    let inner = block.inner(block_area);
    frame.render_widget(block, block_area);

    let mut y_offset: u16 = 0;
    for &field in visible_fields.iter() {
        let divider_y = inner.y + y_offset;
        let content_y = divider_y + 1;
        y_offset += 2;

        let is_focused = app.providers.form().focused_field == field;
        let label_style = if is_focused {
            theme::accent_bold()
        } else {
            theme::muted()
        };
        let is_mandatory = ProviderFormField::is_mandatory_field(field, provider_name);
        let field_label = if field == ProviderFormField::Regions
            && matches!(kind, Some(ProviderKind::Scaleway) | Some(ProviderKind::Gcp))
        {
            "Zones"
        } else if field == ProviderFormField::Regions && kind == Some(ProviderKind::Azure) {
            "Subscriptions"
        } else if field == ProviderFormField::Regions && kind == Some(ProviderKind::Ovh) {
            "Endpoint"
        } else {
            field.label()
        };
        let label = if is_mandatory {
            format!(" {}* ", field_label)
        } else {
            format!(" {} ", field_label)
        };
        super::render_divider(
            frame,
            block_area,
            divider_y,
            &label,
            label_style,
            theme::border_dim(),
        );

        let content_area = Rect::new(inner.x + 1, content_y, inner.width.saturating_sub(1), 1);
        render_field_content(
            frame,
            content_area,
            field,
            app.providers.form(),
            provider_name,
        );
    }

    // Footer below the block. Discard prompt takes precedence; otherwise the
    // save footer reflects the focused field's kind so users discover
    // Space-toggle and Space-pick. Which field is which comes from
    // `ProviderFormField::kind`, so the footer and the arrow never disagree.
    let footer_area = design::render_overlay_footer(frame, block_area);
    if app.forms.is_discard_pending() {
        design::render_discard_prompt(frame, footer_area, app);
    } else {
        let mode = if !expanded && visible_fields.len() < all_fields.len() {
            design::FormFooterMode::Collapsed
        } else {
            design::FormFooterMode::Expanded(app.providers.form().focused_field.kind(provider_name))
        };
        design::form_save_footer(mode).render_with_status(frame, footer_area, app);
    }

    // Key picker popup overlay
    if app.ui.key_picker().open {
        super::host_form::render_key_picker_overlay(frame, app);
    }

    // Region picker popup overlay
    if app.ui.region_picker().open {
        render_region_picker_overlay(frame, app);
    }

    // AWS profile picker popup overlay
    if app.ui.profile_picker().open {
        render_profile_picker_overlay(frame, app);
    }
}

/// The profiles found in `~/.aws`, so the name is picked rather than typed.
/// A profile that assumes a role is labeled, because that is the one fact that
/// decides whether purple will reach a second account with it. A profile whose
/// shape purple refuses says so in the warning color, so the row is not a
/// choice that only fails on the next sync.
fn render_profile_picker_overlay(frame: &mut Frame, app: &mut App) {
    let width = super::picker_overlay_width(frame);
    let max_name = (width as usize).saturating_sub(6);
    let items: Vec<ListItem> = app
        .ui
        .aws_profile_rows()
        .iter()
        .map(|row| {
            let mut spans = vec![Span::styled(
                format!("  {}", super::truncate(&row.name, max_name)),
                theme::bold(),
            )];
            if let Some(note) = &row.note {
                let style = if note.usable {
                    theme::muted()
                } else {
                    theme::warning()
                };
                spans.push(Span::styled(format!("  {}", note.text), style));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    super::render_picker_overlay(
        frame,
        "AWS Profile",
        None,
        items,
        &mut app.ui.profile_picker_mut().list,
    );
}

fn placeholder_for(field: ProviderFormField, provider_name: &str) -> &'static str {
    use crate::messages::hints;
    let kind = provider_name.parse::<ProviderKind>().ok();
    match field {
        ProviderFormField::Label => hints::PROVIDER_LABEL,
        ProviderFormField::Url => match kind {
            Some(ProviderKind::NetBox) => hints::PROVIDER_URL_NETBOX,
            _ => hints::PROVIDER_URL,
        },
        ProviderFormField::Token => match kind {
            None => hints::PROVIDER_TOKEN_DEFAULT,
            Some(ProviderKind::Proxmox) => hints::PROVIDER_TOKEN_PROXMOX,
            Some(ProviderKind::NetBox) => hints::PROVIDER_TOKEN_NETBOX,
            Some(ProviderKind::Aws) => hints::PROVIDER_TOKEN_AWS,
            Some(ProviderKind::Gcp) => hints::PROVIDER_TOKEN_GCP,
            Some(ProviderKind::Azure) => hints::PROVIDER_TOKEN_AZURE,
            Some(ProviderKind::Tailscale) => hints::PROVIDER_TOKEN_TAILSCALE,
            Some(ProviderKind::Teleport) => hints::PROVIDER_TOKEN_TELEPORT,
            Some(ProviderKind::Oracle) => hints::PROVIDER_TOKEN_ORACLE,
            Some(ProviderKind::Ovh) => hints::PROVIDER_TOKEN_OVH,
            Some(
                ProviderKind::DigitalOcean
                | ProviderKind::Hetzner
                | ProviderKind::I3d
                | ProviderKind::Leaseweb
                | ProviderKind::Linode
                | ProviderKind::Scaleway
                | ProviderKind::Transip
                | ProviderKind::UpCloud
                | ProviderKind::Vultr,
            ) => hints::PROVIDER_TOKEN_DEFAULT,
        },
        ProviderFormField::Profile => hints::PROVIDER_PROFILE,
        ProviderFormField::Project => match kind {
            None => hints::PROVIDER_PROJECT_DEFAULT,
            Some(ProviderKind::Ovh) => hints::PROVIDER_PROJECT_OVH,
            Some(
                ProviderKind::Aws
                | ProviderKind::Azure
                | ProviderKind::DigitalOcean
                | ProviderKind::Gcp
                | ProviderKind::Hetzner
                | ProviderKind::I3d
                | ProviderKind::Leaseweb
                | ProviderKind::Linode
                | ProviderKind::NetBox
                | ProviderKind::Oracle
                | ProviderKind::Proxmox
                | ProviderKind::Scaleway
                | ProviderKind::Tailscale
                | ProviderKind::Teleport
                | ProviderKind::Transip
                | ProviderKind::UpCloud
                | ProviderKind::Vultr,
            ) => hints::PROVIDER_PROJECT_DEFAULT,
        },
        ProviderFormField::Compartment => hints::PROVIDER_COMPARTMENT,
        ProviderFormField::Regions => match kind {
            None => hints::PROVIDER_REGIONS_DEFAULT,
            Some(ProviderKind::Gcp) => hints::PROVIDER_REGIONS_GCP,
            Some(ProviderKind::Scaleway) => hints::PROVIDER_REGIONS_SCALEWAY,
            Some(ProviderKind::Azure) => hints::PROVIDER_REGIONS_AZURE,
            Some(ProviderKind::Ovh) => hints::PROVIDER_REGIONS_OVH,
            Some(
                ProviderKind::Aws
                | ProviderKind::DigitalOcean
                | ProviderKind::Hetzner
                | ProviderKind::I3d
                | ProviderKind::Leaseweb
                | ProviderKind::Linode
                | ProviderKind::NetBox
                | ProviderKind::Oracle
                | ProviderKind::Proxmox
                | ProviderKind::Tailscale
                | ProviderKind::Teleport
                | ProviderKind::Transip
                | ProviderKind::UpCloud
                | ProviderKind::Vultr,
            ) => hints::PROVIDER_REGIONS_DEFAULT,
        },
        // Alias prefix lookup table lives on ProviderKind so additions stay typed.
        ProviderFormField::AliasPrefix => kind
            .map(ProviderKind::alias_prefix)
            .unwrap_or(hints::PROVIDER_ALIAS_PREFIX_DEFAULT),
        ProviderFormField::User => match kind {
            None => hints::DEFAULT_SSH_USER,
            Some(ProviderKind::Aws) => hints::PROVIDER_USER_AWS,
            Some(ProviderKind::Gcp) => hints::PROVIDER_USER_GCP,
            Some(ProviderKind::Azure) => hints::PROVIDER_USER_AZURE,
            Some(ProviderKind::Oracle) => hints::PROVIDER_USER_ORACLE,
            Some(ProviderKind::Ovh) => hints::PROVIDER_USER_OVH,
            Some(ProviderKind::Teleport) => hints::PROVIDER_USER_TELEPORT,
            Some(
                ProviderKind::DigitalOcean
                | ProviderKind::Hetzner
                | ProviderKind::I3d
                | ProviderKind::Leaseweb
                | ProviderKind::Linode
                | ProviderKind::NetBox
                | ProviderKind::Proxmox
                | ProviderKind::Scaleway
                | ProviderKind::Tailscale
                | ProviderKind::Transip
                | ProviderKind::UpCloud
                | ProviderKind::Vultr,
            ) => hints::DEFAULT_SSH_USER,
        },
        ProviderFormField::Filter => hints::PROVIDER_FILTER_NETBOX,
        ProviderFormField::IdentityFile => hints::IDENTITY_FILE_PICK,
        ProviderFormField::VaultRole => hints::PROVIDER_VAULT_ROLE,
        ProviderFormField::VaultAddr => hints::PROVIDER_VAULT_ADDR,
        // Toggles render their current value, never a placeholder.
        ProviderFormField::Ssm | ProviderFormField::VerifyTls | ProviderFormField::AutoSync => "",
    }
}

fn render_field_content(
    frame: &mut Frame,
    area: Rect,
    field: ProviderFormField,
    form: &crate::app::ProviderFormFields,
    provider_name: &str,
) {
    let is_focused = form.focused_field == field;

    // Toggle fields
    if field == ProviderFormField::VerifyTls {
        let value_text = if form.verify_tls {
            "yes"
        } else {
            "no (accept self-signed)"
        };
        render_toggle_content(frame, area, value_text, is_focused);
        return;
    }
    if field == ProviderFormField::AutoSync {
        let value_text = if form.auto_sync {
            "yes"
        } else {
            "no (sync manually)"
        };
        render_toggle_content(frame, area, value_text, is_focused);
        return;
    }
    if field == ProviderFormField::Ssm {
        render_toggle_content(
            frame,
            area,
            crate::messages::hints::ssm_mode_value(form.ssm),
            is_focused,
        );
        return;
    }

    let value = match field {
        ProviderFormField::Label => &form.label,
        ProviderFormField::Url => &form.url,
        ProviderFormField::Token => &form.token,
        ProviderFormField::Profile => &form.profile,
        ProviderFormField::Project => &form.project,
        ProviderFormField::Compartment => &form.compartment,
        ProviderFormField::Regions => &form.regions,
        ProviderFormField::Filter => &form.filter,
        ProviderFormField::AliasPrefix => &form.alias_prefix,
        ProviderFormField::User => &form.user,
        ProviderFormField::IdentityFile => &form.identity_file,
        ProviderFormField::VaultRole => &form.vault_role,
        ProviderFormField::VaultAddr => &form.vault_addr,
        ProviderFormField::Ssm | ProviderFormField::VerifyTls | ProviderFormField::AutoSync => {
            debug_assert!(
                false,
                "toggle fields must be handled by the early-return branches above"
            );
            return;
        }
    };

    // Mask token except last 4 chars when not focused
    let display_value: String =
        if field == ProviderFormField::Token && !value.is_empty() && !is_focused {
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

    // The same rule the footer and the key handler read, so a field that
    // advertises "Space pick" also draws the arrow.
    let is_picker = field.is_picker(provider_name);

    let content = if value.is_empty() && is_focused && !is_picker {
        Line::from(Span::styled(
            placeholder_for(field, provider_name),
            theme::muted(),
        ))
    } else if is_picker && is_focused {
        let inner_width = area.width as usize;
        let arrow_pos = inner_width.saturating_sub(1);
        let (display, display_style) = if value.is_empty() {
            (
                placeholder_for(field, provider_name).to_string(),
                theme::muted(),
            )
        } else {
            (display_value.clone(), theme::bold())
        };
        let val_width = display.width();
        let gap = arrow_pos.saturating_sub(val_width);
        Line::from(vec![
            Span::styled(display, display_style),
            Span::raw(" ".repeat(gap)),
            Span::styled(design::PICKER_ARROW, theme::muted()),
        ])
    } else if display_value.is_empty() {
        Line::from(Span::raw(""))
    } else {
        Line::from(Span::styled(display_value, theme::bold()))
    };

    frame.render_widget(Paragraph::new(content), area);

    if is_focused {
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

fn render_toggle_content(frame: &mut Frame, area: Rect, value_text: &str, is_focused: bool) {
    let content = if is_focused {
        let inner_width = area.width as usize;
        let val_width = value_text.width();
        let gap = inner_width.saturating_sub(val_width + 3);
        Line::from(vec![
            Span::styled(value_text, theme::bold()),
            Span::raw(" ".repeat(gap)),
            Span::styled(design::TOGGLE_HINT, theme::muted()),
        ])
    } else {
        Line::from(Span::styled(value_text, theme::bold()))
    };
    frame.render_widget(Paragraph::new(content), area);
}

/// Build display rows for the grouped region/zone picker.
/// Returns a list of (label, Option<region_code>) pairs.
/// Group headers have None as region_code, regions have Some(code).
fn build_region_rows(provider: &str) -> Vec<(String, Option<&'static str>)> {
    let (zones, groups) = crate::handler::zone_data_for(provider);
    let mut rows = Vec::new();
    for &(label, start, end) in groups {
        rows.push((format!(" {}", label), None));
        for &(code, name) in &zones[start..end] {
            rows.push((format!("{} {}", code, name), Some(code)));
        }
    }
    rows
}

fn render_region_picker_overlay(frame: &mut Frame, app: &mut App) {
    let provider_name = match &app.screen {
        crate::app::Screen::ProviderForm { id } => id.provider.as_str(),
        _ => "aws",
    };
    let picker_kind = provider_name.parse::<ProviderKind>().ok();
    let rows = build_region_rows(provider_name);
    let selected: std::collections::HashSet<&str> = app
        .providers
        .form()
        .regions
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let visible_rows = 18u16;
    let block_height = visible_rows + 2; // top + bottom border
    let total_height = block_height + 1; // + footer
    let picker_area = design::overlay_area(frame, 60, 80, total_height);
    frame.render_widget(Clear, picker_area);

    let count = selected.len();
    let zone_label = if matches!(
        picker_kind,
        Some(ProviderKind::Scaleway) | Some(ProviderKind::Gcp)
    ) {
        "Zones"
    } else if picker_kind == Some(ProviderKind::Ovh) {
        "Endpoint"
    } else {
        "Regions"
    };
    let title = format!("Select {} ({} selected)", zone_label, count);
    let block_area = Rect::new(
        picker_area.x,
        picker_area.y,
        picker_area.width,
        block_height,
    );
    let block = design::overlay_block(&title);
    let inner = block.inner(block_area);
    frame.render_widget(block, block_area);

    // Scroll so cursor is always visible
    let cursor = app.ui.region_picker().cursor;
    let scroll_offset = if cursor >= visible_rows as usize {
        cursor - visible_rows as usize + 1
    } else {
        0
    };

    for (i, y) in (0..visible_rows as usize).zip(inner.y..) {
        let idx = scroll_offset + i;
        if idx >= rows.len() {
            break;
        }
        let (label, region_code) = &rows[idx];
        let is_cursor = idx == cursor;

        if let Some(code) = region_code {
            // Region row
            let is_selected = selected.contains(code);
            let check: String = if is_selected {
                format!(" {} ", design::ICON_SUCCESS)
            } else {
                "   ".to_string()
            };
            let display = format!("{}{}", check, label);
            let style = if is_cursor {
                theme::selected_row()
            } else if is_selected {
                theme::bold()
            } else {
                theme::muted()
            };
            let row_area = Rect::new(inner.x, y, inner.width, 1);
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    super::truncate(&display, inner.width as usize),
                    style,
                ))),
                row_area,
            );
        } else {
            // Group header
            let style = if is_cursor {
                theme::selected_row()
            } else {
                theme::accent_bold()
            };
            let row_area = Rect::new(inner.x, y, inner.width, 1);
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    super::truncate(label, inner.width as usize),
                    style,
                ))),
                row_area,
            );
        }
    }

    let footer_area = design::render_overlay_footer(frame, block_area);
    use crate::messages::footer as fl;
    design::Footer::new()
        .primary("Enter", fl::ENTER_APPLY)
        .action("Space", fl::SPACE_TOGGLE)
        .action("Esc", fl::ESC_CANCEL)
        .render_with_status(frame, footer_area, app);
}

#[cfg(test)]
mod tests {
    use super::super::truncate;
    use super::render_field_content;
    use crate::app::{ProviderFormField, ProviderFormFields};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;

    // Every ProviderFormField variant must render without hitting the
    // debug_assert fallback. Adding a new variant to the enum without
    // handling it in render_field_content will cause the match below to
    // fail to compile, flagging the gap before it reaches production.
    #[test]
    fn render_field_content_handles_every_variant() {
        let form = ProviderFormFields::new();
        let area = Rect::new(0, 0, 40, 1);
        let backend = TestBackend::new(40, 3);
        let mut terminal = Terminal::new(backend).unwrap();

        let all: &[ProviderFormField] = &[
            ProviderFormField::Label,
            ProviderFormField::Url,
            ProviderFormField::Token,
            ProviderFormField::Profile,
            ProviderFormField::Project,
            ProviderFormField::Compartment,
            ProviderFormField::Regions,
            ProviderFormField::Filter,
            ProviderFormField::Ssm,
            ProviderFormField::AliasPrefix,
            ProviderFormField::User,
            ProviderFormField::IdentityFile,
            ProviderFormField::VerifyTls,
            ProviderFormField::VaultRole,
            ProviderFormField::VaultAddr,
            ProviderFormField::AutoSync,
        ];

        // Exhaustiveness guard: the compiler forces this match to cover
        // every variant. Add new variants to `all` above when adding them
        // to ProviderFormField.
        for variant in all {
            match variant {
                ProviderFormField::Label
                | ProviderFormField::Url
                | ProviderFormField::Token
                | ProviderFormField::Profile
                | ProviderFormField::Project
                | ProviderFormField::Compartment
                | ProviderFormField::Regions
                | ProviderFormField::Filter
                | ProviderFormField::AliasPrefix
                | ProviderFormField::User
                | ProviderFormField::IdentityFile
                | ProviderFormField::VerifyTls
                | ProviderFormField::VaultRole
                | ProviderFormField::VaultAddr
                | ProviderFormField::AutoSync
                | ProviderFormField::Ssm => {}
            }
        }

        for variant in all {
            terminal
                .draw(|frame| render_field_content(frame, area, *variant, &form, "aws"))
                .unwrap();
        }
    }

    // The provider remove confirm names the config file the hosts stay in.
    #[test]
    fn remove_confirm_names_the_config_file() {
        let mut app = crate::ui::app_with_own_config("");
        app.providers
            .request_delete(crate::providers::config::ProviderConfigId::bare("hetzner"));
        let text = crate::ui::render_to_text(120, 40, |frame| {
            super::render_provider_list(frame, &mut app)
        });
        assert!(text.contains("~/.ssh/purple_config"), "{text}");
        assert!(!text.contains("~/.ssh/config"), "{text}");
    }

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

    #[test]
    fn provider_list_layout_has_spacer() {
        use ratatui::layout::{Constraint, Layout, Rect};
        let area = Rect::new(0, 0, 60, 20);
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
