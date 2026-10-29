use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::ctx::{Effectful, Effects, Nav, Notify};
use crate::app::{App, FormState, ProviderState, Screen, StatusCenter};
use crate::event::AppEvent;
use crate::providers;
use crate::providers::ProviderKind;

mod region;

// Test-only: region_picker_rows is pub(crate) in region.rs but only re-exported
// here for handler::tests which validates the OVH endpoint picker row count.
// Production code calls handle_region_picker directly; it never needs the raw rows.
#[cfg(test)]
pub(super) use region::region_picker_rows;
pub(crate) use region::zone_data_for;

/// The slice of App the provider-form key handler touches while editing: the
/// provider form fields (`providers`), the form-discard flag (`forms`) and the
/// status center (`status`, for the AWS token-format warning). Submitting,
/// closing and opening a picker each reach far beyond a form slice (submit
/// writes the provider config, rewrites legacy SSH-config markers, spawns a
/// background sync and updates the footer summary; closing flushes a pending
/// vault write and clears the form mtime; the key picker rescans `~/.ssh`), so
/// those whole-App operations are deferred as effects and applied to the full
/// `App` after the slice borrow ends. The region picker only touches `ui`, so
/// it is deferred together with the key picker in the same arm to keep the
/// `App::open_*_picker` call shape identical to the pre-slice handler. The
/// slice never reaches into hosts, vault or any other domain directly.
struct ProviderFormCtx<'a> {
    providers: &'a mut ProviderState,
    forms: &'a mut FormState,
    status: &'a mut StatusCenter,
    effects: Effects,
}

impl Notify for ProviderFormCtx<'_> {
    fn status_mut(&mut self) -> &mut StatusCenter {
        self.status
    }
}

impl Effectful for ProviderFormCtx<'_> {
    fn effects_mut(&mut self) -> &mut Effects {
        &mut self.effects
    }
}

impl ProviderFormCtx<'_> {
    /// Show a non-blocking warning when leaving the Token field with an
    /// invalid AWS format. Slice-local: reads the form and writes a toast.
    fn warn_aws_token_format(&mut self, provider_name: &str) {
        if provider_name.parse::<ProviderKind>().ok() != Some(ProviderKind::Aws) {
            return;
        }
        if self.providers.form_mut().focused_field != crate::app::ProviderFormField::Token {
            return;
        }
        let token = self.providers.form_mut().token.trim();
        if token.is_empty() {
            return;
        }
        if !token.contains(':') {
            self.notify_warning(crate::i18n::text(crate::messages::TOKEN_FORMAT_AWS));
        }
    }
}

/// The slice of App the label-migration key handler touches while editing the
/// two label fields: the pending-migration scratch state (`providers`), the
/// status center (`status`, for label-validation errors), and the screen
/// (`screen`, so Esc can return to the providers list). The Enter action moves
/// on to the provider form via `App::open_provider_form`, which captures the
/// provider-config mtime and is therefore deferred as the terminal effect.
struct LabelMigrationCtx<'a> {
    providers: &'a mut ProviderState,
    status: &'a mut StatusCenter,
    screen: &'a mut Screen,
    effects: Effects,
}

impl Nav for LabelMigrationCtx<'_> {
    fn screen_mut(&mut self) -> &mut Screen {
        self.screen
    }
}

impl Notify for LabelMigrationCtx<'_> {
    fn status_mut(&mut self) -> &mut StatusCenter {
        self.status
    }
}

impl Effectful for LabelMigrationCtx<'_> {
    fn effects_mut(&mut self) -> &mut Effects {
        &mut self.effects
    }
}

pub(super) fn handle_provider_list_key(
    app: &mut App,
    key: KeyEvent,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    // Handle pending provider delete confirmation first.
    // `pending_delete_id` (Some) scopes the delete to one config; otherwise
    // `pending_delete` (legacy bare-name) deletes whatever single section
    // matches that provider name.
    if app.providers.pending_delete().is_some() && key.code != KeyCode::Char('?') {
        match super::route_confirm_key(key) {
            super::ConfirmAction::Yes => {
                let pending_id = app.providers.take_pending_delete_id();
                let Some(name) = app.providers.take_pending_delete() else {
                    return;
                };
                if let Some(id) = pending_id {
                    let Some(old_section) = app.providers.config().section_by_id(&id).cloned()
                    else {
                        return;
                    };
                    app.providers.config_mut().remove_section_by_id(&id);
                    if let Err(e) = app.providers.config().save() {
                        app.providers.config_mut().set_section(old_section);
                        app.notify_error(crate::messages::failed_to_save(&e));
                    } else {
                        app.providers.sync_history_mut().remove(&id.to_string());
                        crate::app::SyncRecord::save_all(
                            app.providers.sync_history(),
                            app.env.paths(),
                        );
                        // Drop the expand-state if this was the last config of
                        // its provider; otherwise a re-add would reopen expanded.
                        if app
                            .providers
                            .config()
                            .sections_for_provider(&id.provider)
                            .is_empty()
                        {
                            app.providers.expanded_providers_mut().remove(&id.provider);
                        }
                        log::debug!("[config] provider config removed: {id}");
                        let display_name = crate::providers::provider_display_name(&id.provider);
                        app.notify(crate::messages::provider_removed(display_name));
                    }
                } else if let Some(old_section) =
                    app.providers.config().section(name.as_str()).cloned()
                {
                    app.providers.config_mut().remove_section(name.as_str());
                    if let Err(e) = app.providers.config().save() {
                        app.providers.config_mut().set_section(old_section);
                        app.notify_error(crate::messages::failed_to_save(&e));
                    } else {
                        app.providers.sync_history_mut().remove(name.as_str());
                        crate::app::SyncRecord::save_all(
                            app.providers.sync_history(),
                            app.env.paths(),
                        );
                        app.providers.expanded_providers_mut().remove(&name);
                        log::debug!("[config] provider config removed: {name}");
                        let display_name = crate::providers::provider_display_name(name.as_str());
                        app.notify(crate::messages::provider_removed(display_name));
                    }
                }
            }
            super::ConfirmAction::No => {
                app.providers.cancel_delete();
            }
            super::ConfirmAction::Ignored => {}
        }
        return;
    }

    let rows = app.providers.provider_list_rows();
    let row_count = rows.len();
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            // Cancel all running syncs
            for cancel_flag in app.providers.syncing().values() {
                cancel_flag.store(true, Ordering::Relaxed);
            }
            app.set_screen(Screen::HostList);
        }
        KeyCode::Char('j') | KeyCode::Down => {
            crate::app::cycle_selection(app.ui.provider_list_state_mut(), row_count, true);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            crate::app::cycle_selection(app.ui.provider_list_state_mut(), row_count, false);
        }
        KeyCode::PageDown => {
            crate::app::page_down(app.ui.provider_list_state_mut(), row_count, 10);
        }
        KeyCode::PageUp => {
            crate::app::page_up(app.ui.provider_list_state_mut(), row_count, 10);
        }
        // SPACE GUARD MUST PRECEDE any generic Char(c) arm in this handler
        // so Space toggles expand/collapse on a multi-config header instead
        // of being consumed as a literal space.
        KeyCode::Char(' ') => {
            // Space toggles expand/collapse on a multi-config provider header.
            if let Some(idx) = app.ui.provider_list_state().selected()
                && let Some(crate::app::ProviderRow::Header { name, config_count }) = rows.get(idx)
                && *config_count >= 2
            {
                let n = name.clone();
                let now_expanded = app.providers.toggle_expanded(&n);
                log::debug!(
                    "[purple] provider tree: {} '{}'",
                    if now_expanded {
                        "expanded"
                    } else {
                        "collapsed"
                    },
                    n
                );
            }
        }
        KeyCode::Char('a') => {
            // Add another config for the selected provider. Triggers the
            // lazy-migration prompt when the provider already has one bare
            // config; for an already-labeled provider, opens the form
            // directly with an empty label.
            if let Some(idx) = app.ui.provider_list_state().selected() {
                let provider_name = rows.get(idx).map(|r| r.provider_name().to_string());
                if let Some(name) = provider_name {
                    open_add_config_flow(app, &name);
                }
            }
        }
        KeyCode::Enter => {
            if let Some(index) = app.ui.provider_list_state().selected() {
                let row = match rows.get(index) {
                    Some(r) => r.clone(),
                    None => return,
                };
                // Multi-config header: Enter toggles expand/collapse.
                if let crate::app::ProviderRow::Header { name, config_count } = &row
                    && *config_count >= 2
                {
                    let now_expanded = app.providers.toggle_expanded(name);
                    log::debug!(
                        "[purple] provider tree: {} '{}'",
                        if now_expanded {
                            "expanded"
                        } else {
                            "collapsed"
                        },
                        name
                    );
                    return;
                }
                // Single-config header or leaf: open the edit form.
                let target_id = match &row {
                    crate::app::ProviderRow::Header { name, config_count } => {
                        if *config_count == 1 {
                            app.providers
                                .config()
                                .sections_for_provider(name)
                                .first()
                                .map(|s| s.id.clone())
                                .unwrap_or_else(|| {
                                    crate::providers::config::ProviderConfigId::bare(name.clone())
                                })
                        } else {
                            crate::providers::config::ProviderConfigId::bare(name.clone())
                        }
                    }
                    crate::app::ProviderRow::Leaf { id } => id.clone(),
                };
                app.open_provider_form(target_id);
            }
        }
        KeyCode::Char('s') => {
            if app.demo_mode {
                app.notify_warning(crate::i18n::text(crate::messages::DEMO_SYNC_DISABLED));
                return;
            }
            let row = match app
                .ui
                .provider_list_state()
                .selected()
                .and_then(|i| rows.get(i))
            {
                Some(r) => r.clone(),
                None => return,
            };
            let sections_to_sync: Vec<providers::config::ProviderSection> = match &row {
                crate::app::ProviderRow::Header { name, .. } => app
                    .providers
                    .config()
                    .sections_for_provider(name)
                    .into_iter()
                    .cloned()
                    .collect(),
                crate::app::ProviderRow::Leaf { id } => app
                    .providers
                    .config()
                    .section_by_id(id)
                    .cloned()
                    .into_iter()
                    .collect(),
            };
            if sections_to_sync.is_empty() {
                let name = row.provider_name();
                let display_name = crate::providers::provider_display_name(name);
                app.notify_error(crate::messages::provider_configure_first(display_name));
                return;
            }
            for section in sections_to_sync {
                let key = section.id.to_string();
                if app.providers.syncing().contains_key(&key) {
                    continue;
                }
                app.providers.reset_batch_if_idle();
                let cancel = Arc::new(AtomicBool::new(false));
                app.providers.syncing_mut().insert(key, cancel.clone());
                app.providers.bump_batch_total();
                super::sync::spawn_provider_sync(
                    &section,
                    events_tx.clone(),
                    cancel,
                    std::sync::Arc::clone(&app.env),
                );
            }
            crate::set_sync_summary(app);
        }
        KeyCode::Char('d') => {
            let row = match app
                .ui
                .provider_list_state()
                .selected()
                .and_then(|i| rows.get(i))
            {
                Some(r) => r.clone(),
                None => return,
            };
            match &row {
                crate::app::ProviderRow::Leaf { id } => {
                    if app.providers.config().section_by_id(id).is_some() {
                        app.providers.request_delete(id.clone());
                    }
                }
                crate::app::ProviderRow::Header { name, config_count } => {
                    if *config_count == 0 {
                        let display_name = crate::providers::provider_display_name(name);
                        app.notify(crate::messages::provider_not_configured(display_name));
                    } else if *config_count >= 2 {
                        // Refuse to mass-delete from the header. Force the
                        // user to expand and pick a specific config.
                        app.notify(
                            crate::i18n::text(crate::messages::EXPAND_TO_REMOVE_CONFIG).to_string(),
                        );
                    } else {
                        // Single config: scope the confirm to that exact id.
                        if let Some(section) = app.providers.config().section(name) {
                            app.providers.request_delete(section.id.clone());
                        }
                    }
                }
            }
        }
        KeyCode::Char('?') => {
            app.push_help_overlay();
        }
        KeyCode::Char('X') => {
            // The list state indexes the FULL row list (headers + leaves),
            // not the bare-name list. Use the row to scope correctly:
            // - Header → all hosts of that provider (any label)
            // - Leaf   → only hosts of that labeled config
            let row = match app
                .ui
                .provider_list_state()
                .selected()
                .and_then(|i| rows.get(i))
            {
                Some(r) => r.clone(),
                None => return,
            };
            let stale = app.hosts_state.ssh_config().stale_hosts();
            let entries = app.hosts_state.ssh_config().host_entries();
            let (display, scope_provider, provider_stale): (String, Option<String>, Vec<_>) =
                match &row {
                    crate::app::ProviderRow::Header { name, .. } => {
                        let display = crate::providers::provider_display_name(name).to_string();
                        let scope = name.clone();
                        let filtered: Vec<_> = stale
                            .iter()
                            .filter(|(alias, _)| {
                                entries.iter().any(|e| {
                                    e.alias == *alias
                                        && e.provider.as_deref() == Some(name.as_str())
                                })
                            })
                            .collect();
                        (display, Some(scope), filtered)
                    }
                    crate::app::ProviderRow::Leaf { id } => {
                        let display = format!(
                            "{} ({})",
                            crate::providers::provider_display_name(&id.provider),
                            id.label.as_deref().unwrap_or("")
                        );
                        let prov = id.provider.clone();
                        let label = id.label.clone();
                        let filtered: Vec<_> = stale
                            .iter()
                            .filter(|(alias, _)| {
                                entries.iter().any(|e| {
                                    e.alias == *alias
                                        && e.provider.as_deref() == Some(prov.as_str())
                                        && e.provider_label == label
                                })
                            })
                            .collect();
                        (display, Some(prov), filtered)
                    }
                };
            if provider_stale.is_empty() {
                app.notify_warning(crate::messages::no_stale_hosts_for(&display));
            } else {
                let aliases: Vec<String> =
                    provider_stale.into_iter().map(|(a, _)| a.clone()).collect();
                app.providers.set_pending_purge(crate::app::PendingPurge {
                    aliases,
                    provider: scope_provider,
                });
                app.set_screen(Screen::ConfirmPurgeStale);
            }
        }
        _ => {}
    }
}

/// Step 1 of the lazy add-second-config flow: pick labels for the existing
/// (bare) config AND the new one. On Enter, validate both and transition
/// to step 2 (the standard provider form). On Esc, drop pending state.
pub fn handle_label_migration_key(
    app: &mut App,
    key: KeyEvent,
    _events_tx: &mpsc::Sender<AppEvent>,
) {
    // Resolve the provider name from the screen in the wrapper while it still
    // holds `&App`, then narrow to the label-migration slice.
    let provider = match &app.screen {
        Screen::ProviderLabelMigration { provider } => provider.clone(),
        _ => return,
    };
    let effects = {
        let mut ctx = LabelMigrationCtx {
            providers: &mut app.providers,
            status: &mut app.status_center,
            screen: &mut app.screen,
            effects: Effects::default(),
        };
        label_migration_key(&mut ctx, key, &provider);
        ctx.effects
    };
    effects.apply(app);
}

fn label_migration_key(ctx: &mut LabelMigrationCtx, key: KeyEvent, provider: &str) {
    match key.code {
        KeyCode::Esc => {
            ctx.providers.cancel_label_migration();
            ctx.set_screen(Screen::Providers);
        }
        KeyCode::Tab | KeyCode::Down => {
            if let Some(p) = ctx.providers.pending_label_migration_mut() {
                p.focused = match p.focused {
                    crate::app::LabelMigrationField::Existing => {
                        crate::app::LabelMigrationField::New
                    }
                    crate::app::LabelMigrationField::New => {
                        crate::app::LabelMigrationField::Existing
                    }
                };
                p.cursor_pos = p.focused_value().chars().count();
            }
        }
        KeyCode::BackTab | KeyCode::Up => {
            if let Some(p) = ctx.providers.pending_label_migration_mut() {
                p.focused = match p.focused {
                    crate::app::LabelMigrationField::Existing => {
                        crate::app::LabelMigrationField::New
                    }
                    crate::app::LabelMigrationField::New => {
                        crate::app::LabelMigrationField::Existing
                    }
                };
                p.cursor_pos = p.focused_value().chars().count();
            }
        }
        KeyCode::Left => {
            if let Some(p) = ctx.providers.pending_label_migration_mut()
                && p.cursor_pos > 0
            {
                p.cursor_pos -= 1;
            }
        }
        KeyCode::Right => {
            if let Some(p) = ctx.providers.pending_label_migration_mut() {
                let max = p.focused_value().chars().count();
                if p.cursor_pos < max {
                    p.cursor_pos += 1;
                }
            }
        }
        KeyCode::Home => {
            if let Some(p) = ctx.providers.pending_label_migration_mut() {
                p.cursor_pos = 0;
            }
        }
        KeyCode::End => {
            if let Some(p) = ctx.providers.pending_label_migration_mut() {
                p.cursor_pos = p.focused_value().chars().count();
            }
        }
        KeyCode::Backspace => {
            if let Some(p) = ctx.providers.pending_label_migration_mut()
                && p.cursor_pos > 0
            {
                let cursor_pos = p.cursor_pos;
                let target = p.focused_value_mut();
                let mut chars: Vec<char> = target.chars().collect();
                chars.remove(cursor_pos - 1);
                *target = chars.into_iter().collect();
                p.cursor_pos -= 1;
            }
        }
        KeyCode::Delete => {
            if let Some(p) = ctx.providers.pending_label_migration_mut() {
                let len = p.focused_value().chars().count();
                if p.cursor_pos < len {
                    let cursor_pos = p.cursor_pos;
                    let target = p.focused_value_mut();
                    let mut chars: Vec<char> = target.chars().collect();
                    chars.remove(cursor_pos);
                    *target = chars.into_iter().collect();
                }
            }
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            let allowed = c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
            if !allowed {
                return;
            }
            if let Some(p) = ctx.providers.pending_label_migration_mut() {
                let cursor_pos = p.cursor_pos;
                let target = p.focused_value_mut();
                if target.len() < 32 {
                    let mut chars: Vec<char> = target.chars().collect();
                    chars.insert(cursor_pos, c);
                    *target = chars.into_iter().collect();
                    p.cursor_pos += 1;
                }
            }
        }
        KeyCode::Enter => {
            let (existing, new) = match ctx.providers.pending_label_migration() {
                Some(p) => (p.existing_label.clone(), p.new_label.clone()),
                None => return,
            };
            if let Err(e) = crate::providers::config::validate_label(&existing) {
                ctx.notify_error(crate::messages::label_invalid(&e));
                return;
            }
            if let Err(e) = crate::providers::config::validate_label(&new) {
                ctx.notify_error(crate::messages::label_invalid(&e));
                return;
            }
            if existing == new {
                ctx.notify_error(crate::i18n::text(crate::messages::LABEL_MUST_DIFFER).to_string());
                return;
            }
            // Move on to step 2: the standard provider form, pre-keyed to
            // the new labeled id. submit_provider_form will pick up
            // pending_label_migration to also rewrite the existing section.
            // open_provider_form captures the provider-config mtime, so it
            // is deferred as the terminal effect.
            let provider = provider.to_string();
            ctx.defer(move |app| {
                app.open_provider_form(crate::providers::config::ProviderConfigId::labeled(
                    provider, new,
                ));
            });
        }
        _ => {}
    }
}

/// Begin the "add another config" flow for a provider.
/// - 0 existing configs: open the regular bare-config form (same as Enter).
/// - 1 existing bare config: prompt for a label for the existing one (Y step 1).
/// - 1+ existing labeled configs: open form for a new labeled config directly.
fn open_add_config_flow(app: &mut App, provider_name: &str) {
    let existing = app.providers.config().sections_for_provider(provider_name);
    match existing.len() {
        0 => {
            app.open_provider_form(crate::providers::config::ProviderConfigId::bare(
                provider_name,
            ));
        }
        1 if existing[0].id.label.is_none() => {
            // Lazy migration: existing config is bare. Prompt for both
            // labels (existing + new) on one screen so step 2 is the
            // standard provider form without an extra label field.
            log::debug!(
                "[config] provider lazy migration: started for '{}'",
                provider_name
            );
            app.providers
                .set_pending_label_migration(Some(crate::app::PendingLabelMigration {
                    provider: provider_name.to_string(),
                    existing_label: "default".to_string(),
                    new_label: String::new(),
                    // Focus the existing-config field so the unexpected rename
                    // gets attention instead of a blind tab-past "default".
                    focused: crate::app::LabelMigrationField::Existing,
                    cursor_pos: "default".chars().count(),
                }));
            app.set_screen(Screen::ProviderLabelMigration {
                provider: provider_name.to_string(),
            });
        }
        _ => {
            // One or more labeled configs already exist: open the form with
            // an empty label so the user can fill it in.
            app.open_provider_form(crate::providers::config::ProviderConfigId {
                provider: provider_name.to_string(),
                label: Some(String::new()),
            });
        }
    }
}

pub(super) fn handle_provider_form_key(
    app: &mut App,
    key: KeyEvent,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    // Picker dispatch stays in the wrapper: each picker handler takes the full
    // `&mut App` (the key picker rescans `~/.ssh`, the region picker reads the
    // screen), so we delegate before narrowing to the form slice.
    if app.ui.key_picker().open {
        super::picker::handle_key_picker_shared(app, key, true);
        return;
    }

    // Dispatch to region picker if open
    if app.ui.region_picker().open {
        region::handle_region_picker(app, key);
        return;
    }

    // Dispatch to the AWS profile picker if open
    if app.ui.profile_picker().open {
        handle_profile_picker(app, key);
        return;
    }

    let provider_name = match &app.screen {
        Screen::ProviderForm { id } => id.provider.clone(),
        _ => return,
    };

    let effects = {
        let mut ctx = ProviderFormCtx {
            providers: &mut app.providers,
            forms: &mut app.forms,
            status: &mut app.status_center,
            effects: Effects::default(),
        };
        provider_form_key(&mut ctx, key, &provider_name, events_tx);
        ctx.effects
    };
    effects.apply(app);
}

/// Keys for the AWS profile picker. Same shape as the sibling pickers: Esc
/// closes, j/k and the arrows move, Enter takes the row.
///
/// Takes the whole `App` because picking a profile also fills the Regions
/// field when that profile names a region and the field is still empty. That
/// is the one setting a profile carries which purple would otherwise ask for
/// twice.
fn handle_profile_picker(app: &mut App, key: KeyEvent) {
    // The rows the overlay was opened with, so walking and picking agree with
    // what is on screen even if `~/.aws` changes underneath.
    let candidates: Vec<(String, String)> = app
        .ui
        .aws_profile_rows()
        .iter()
        .map(|row| (row.name.clone(), row.region.clone()))
        .collect();
    match key.code {
        KeyCode::Esc => {
            log::debug!("[purple] close_profile_picker");
            app.ui.profile_picker_mut().close();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            crate::app::cycle_selection(
                &mut app.ui.profile_picker_mut().list,
                candidates.len(),
                true,
            );
        }
        KeyCode::Char('k') | KeyCode::Up => {
            crate::app::cycle_selection(
                &mut app.ui.profile_picker_mut().list,
                candidates.len(),
                false,
            );
        }
        KeyCode::Enter => {
            if let Some(index) = app.ui.profile_picker().list.selected()
                && let Some((name, region)) = candidates.get(index)
            {
                app.providers.form_mut().profile = name.clone();
                app.providers.form_mut().cursor_pos = name.chars().count();
                prefill_region_from_profile(app, name, region);
                log::debug!("[purple] provider form: profile set to '{}'", name);
            }
            app.ui.profile_picker_mut().close();
        }
        _ => {}
    }
}

/// Copy the chosen profile's `region` into the still-empty Regions field.
///
/// Only when the field is empty, so a region set by hand or by the region
/// picker is never overwritten, and only when purple knows the code, so a
/// region it cannot sync never lands in the form. The value comes off the row
/// the picker was opened with, so picking reads no file.
fn prefill_region_from_profile(app: &mut App, profile: &str, region: &str) {
    if !app.providers.form_mut().regions.trim().is_empty() {
        return;
    }
    if region.is_empty() {
        return;
    }
    if !crate::providers::aws::AWS_REGIONS
        .iter()
        .any(|(code, _)| *code == region)
    {
        return;
    }
    log::debug!(
        "[purple] provider form: regions prefilled with '{}' from profile '{}'",
        region,
        profile
    );
    app.providers.form_mut().regions = region.to_string();
}

fn provider_form_key(
    ctx: &mut ProviderFormCtx,
    key: KeyEvent,
    provider_name: &str,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    // Progressive disclosure: hide `VaultAddr` when no role is set so Tab
    // navigation skips the hidden field. `visible_fields` is a filtered
    // snapshot of `fields_for(provider)` taken once per key press.
    let visible = ctx.providers.form_mut().visible_fields(provider_name);
    let fields: &[crate::app::ProviderFormField] = &visible;
    // Field-kind predicates live on `ProviderFormField` so the rule is
    // enforced in one place. Note: `is_picker` here matches the full set
    // (aws/scaleway/gcp/oracle/ovh) -- the previous local closure only
    // matched aws/scaleway/gcp which was a bug; oracle/ovh need the picker
    // too because their `Regions` Space-handler at the bottom of this match
    // expects to open the picker. `is_picker` on the type is the source of
    // truth.
    let is_toggle = |f: crate::app::ProviderFormField| f.is_toggle();
    let is_picker = |f: crate::app::ProviderFormField| f.is_picker(provider_name);

    // Handle discard confirmation dialog via the shared confirm router.
    if ctx.forms.is_discard_pending() {
        match super::route_confirm_key(key) {
            super::ConfirmAction::Yes => {
                ctx.forms.dismiss_discard_confirm();
                ctx.defer(App::close_provider_form);
            }
            super::ConfirmAction::No => {
                ctx.forms.dismiss_discard_confirm();
            }
            super::ConfirmAction::Ignored => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc => {
            if ctx.providers.form_is_dirty() {
                ctx.forms.request_discard_confirm();
            } else {
                ctx.defer(App::close_provider_form);
            }
        }
        KeyCode::Tab | KeyCode::Down => {
            ctx.warn_aws_token_format(provider_name);
            if !ctx.providers.form_mut().expanded {
                // Use visible_fields so the dynamic Label (issue #51) joins the
                // navigation cycle alongside provider-static fields. Required
                // fields are always first in the per-provider arrays, and the
                // prepended Label is required when present, so the same prefix
                // slice rule holds.
                let all = &visible;
                let req_count = all
                    .iter()
                    .filter(|f| {
                        crate::app::ProviderFormField::is_required_field(**f, provider_name)
                    })
                    .count();
                let required = &all[..req_count];
                if required.is_empty() {
                    // Fallback: no required fields, use full field list
                    ctx.providers.form_mut().focused_field =
                        ctx.providers.form_mut().focused_field.next(fields);
                } else {
                    let pos = required
                        .iter()
                        .position(|f| *f == ctx.providers.form_mut().focused_field);
                    if let Some(idx) = pos {
                        if idx + 1 < required.len() {
                            ctx.providers.form_mut().focused_field = required[idx + 1];
                        } else if req_count < all.len() {
                            // Last required field: expand and focus first optional
                            ctx.providers.form_mut().expanded = true;
                            ctx.providers.form_mut().focused_field = all[req_count];
                        } else {
                            // No optional fields, wrap
                            ctx.providers.form_mut().focused_field = required[0];
                        }
                    } else {
                        ctx.providers.form_mut().focused_field =
                            ctx.providers.form_mut().focused_field.next(fields);
                    }
                }
            } else {
                ctx.providers.form_mut().focused_field =
                    ctx.providers.form_mut().focused_field.next(fields);
            }
            ctx.providers.form_mut().sync_cursor_to_end();
        }
        KeyCode::BackTab | KeyCode::Up => {
            ctx.warn_aws_token_format(provider_name);
            if !ctx.providers.form_mut().expanded {
                let all = &visible;
                let req_count = all
                    .iter()
                    .filter(|f| {
                        crate::app::ProviderFormField::is_required_field(**f, provider_name)
                    })
                    .count();
                let required = &all[..req_count];
                if required.is_empty() {
                    // Fallback: no required fields, use full field list
                    ctx.providers.form_mut().focused_field =
                        ctx.providers.form_mut().focused_field.prev(fields);
                } else {
                    let pos = required
                        .iter()
                        .position(|f| *f == ctx.providers.form_mut().focused_field);
                    if let Some(idx) = pos {
                        let prev_idx = if idx > 0 { idx - 1 } else { required.len() - 1 };
                        ctx.providers.form_mut().focused_field = required[prev_idx];
                    } else {
                        // Focus is on a non-required field while collapsed; go to last required
                        ctx.providers.form_mut().focused_field = required[required.len() - 1];
                    }
                }
            } else {
                ctx.providers.form_mut().focused_field =
                    ctx.providers.form_mut().focused_field.prev(fields);
            }
            ctx.providers.form_mut().sync_cursor_to_end();
        }
        KeyCode::Left if ctx.providers.form_mut().cursor_pos > 0 => {
            ctx.providers.form_mut().cursor_pos -= 1;
        }
        KeyCode::Right => {
            let len = ctx.providers.form_mut().focused_value().chars().count();
            if ctx.providers.form_mut().cursor_pos < len {
                ctx.providers.form_mut().cursor_pos += 1;
            }
        }
        KeyCode::Home => {
            ctx.providers.form_mut().cursor_pos = 0;
        }
        KeyCode::End => {
            ctx.providers.form_mut().sync_cursor_to_end();
        }
        KeyCode::Enter => {
            // INVARIANT: Enter ALWAYS submits the form, regardless of focused
            // field. Pickers/toggles are reached via Space (see arms below).
            // Submit writes the provider config, rewrites legacy SSH-config
            // markers, spawns a background sync and updates the footer summary,
            // so it runs on the full `App` as the deferred terminal action.
            let tx = events_tx.clone();
            ctx.defer(move |app| submit_provider_form(app, &tx));
        }
        // SPACE GUARDS MUST PRECEDE the generic Char(c) arm.
        // Order: toggle first, picker second (no overlap, but explicit
        // ordering protects against future ProviderFormField additions).
        KeyCode::Char(' ')
            if ctx.providers.form_mut().focused_field
                == crate::app::ProviderFormField::VerifyTls =>
        {
            ctx.providers.form_mut().verify_tls = !ctx.providers.form_mut().verify_tls;
        }
        KeyCode::Char(' ')
            if ctx.providers.form_mut().focused_field
                == crate::app::ProviderFormField::AutoSync =>
        {
            ctx.providers.form_mut().auto_sync = !ctx.providers.form_mut().auto_sync;
        }
        KeyCode::Char(' ')
            if ctx.providers.form_mut().focused_field == crate::app::ProviderFormField::Ssm =>
        {
            let next = ctx.providers.form_mut().ssm.next();
            ctx.providers.form_mut().ssm = next;
        }
        // Empty-field gate: same rationale as host_form — once the user
        // has typed anything, Space inserts a literal space so custom
        // identity paths (e.g. `~/My Keys/id_rsa`) and free-form region
        // lists work. On an empty picker field, Space opens the picker.
        // Both picker opens reach beyond the slice (the key picker rescans
        // `~/.ssh`), so the open is deferred to the full `App`.
        KeyCode::Char(' ')
            if is_picker(ctx.providers.form_mut().focused_field)
                && ctx.providers.form_mut().focused_value().is_empty() =>
        {
            let f = ctx.providers.form_mut().focused_field;
            ctx.defer(move |app| {
                if f == crate::app::ProviderFormField::IdentityFile {
                    app.open_key_picker();
                } else if f == crate::app::ProviderFormField::Regions {
                    app.open_region_picker();
                } else if f == crate::app::ProviderFormField::Profile && !app.open_profile_picker()
                {
                    // An empty picker reads as a bug, so say why it did not
                    // open and name the field that works instead.
                    app.notify_warning(crate::i18n::text(crate::messages::PICKER_NO_AWS_PROFILES));
                }
            });
        }
        KeyCode::Char(c) => {
            // Toggle fields (VerifyTls/AutoSync) have no text value to mutate;
            // every other field, including picker fields, accepts free-text
            // typing so users can supply custom paths or region values not
            // surfaced by the picker. Matches the host form's Char arm.
            let f = ctx.providers.form_mut().focused_field;
            if is_toggle(f) {
                // Nothing to do.
            } else if f == crate::app::ProviderFormField::Label {
                // Label charset and length must mirror validate_label so a
                // value typed into this field always survives save-time
                // validation. Reject silently like the migration screen
                // (handler/provider.rs:502) does for the same constraints.
                let allowed = c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
                if allowed && ctx.providers.form_mut().label.len() < 32 {
                    ctx.providers.form_mut().insert_char(c);
                }
            } else {
                ctx.providers.form_mut().insert_char(c);
            }
        }
        KeyCode::Backspace => {
            let f = ctx.providers.form_mut().focused_field;
            if !is_toggle(f) {
                ctx.providers.form_mut().delete_char_before_cursor();
            }
        }
        _ => {}
    }
}

fn submit_provider_form(app: &mut App, events_tx: &mpsc::Sender<AppEvent>) {
    if app.demo_mode {
        app.notify_warning(crate::i18n::text(
            crate::messages::DEMO_PROVIDER_CHANGES_DISABLED,
        ));
        app.set_screen(Screen::Providers);
        return;
    }
    let mut form_id = match &app.screen {
        Screen::ProviderForm { id } => id.clone(),
        _ => return,
    };
    let provider_name = form_id.provider.clone();
    let kind = provider_name.parse::<ProviderKind>().ok();

    // Label-entry mode (issue #51): seal the typed label into form_id before
    // any further validation or persistence. Without this the section would
    // serialize with the empty label still embedded in the screen id and the
    // save would fail downstream in validate_label. Reject early with a
    // pointed message so the user is not surprised by a "label is empty"
    // toast at the end of a long form.
    if app.providers.form_mut().label_entry {
        let typed = app.providers.form_mut().label.trim().to_string();
        if let Err(e) = providers::config::validate_label(&typed) {
            app.notify_error(crate::messages::label_invalid(&e));
            app.providers.form_mut().focused_field = crate::app::ProviderFormField::Label;
            return;
        }
        // Reject collisions explicitly so the user understands why the save
        // would have been rejected by the dedup pass in validate().
        let candidate =
            providers::config::ProviderConfigId::labeled(provider_name.clone(), typed.clone());
        if app.providers.config().section_by_id(&candidate).is_some() {
            app.notify_error(crate::messages::label_already_in_use(&typed));
            app.providers.form_mut().focused_field = crate::app::ProviderFormField::Label;
            return;
        }
        // The form opened with `alias_prefix = <short_label>` because we did
        // not know the label yet. Now that the user has typed one, suffix it
        // so the new section does not collide with a pre-existing bare-style
        // prefix on a sibling labeled config (validate() rejects duplicates).
        // Mirrors the migration helper at lines below that suffixes the
        // existing bare section's prefix when it equals the short label.
        let short = providers::get_provider(provider_name.as_str())
            .map(|p| p.short_label().to_string())
            .unwrap_or_else(|| provider_name.clone());
        if app.providers.form_mut().alias_prefix.trim() == short {
            app.providers.form_mut().alias_prefix = format!("{}-{}", short, typed);
        }
        form_id = candidate;
    }

    // Check for external provider config changes since form was opened
    if app.provider_config_changed_since_form_open() {
        app.notify_error(crate::i18n::text(
            crate::messages::PROVIDER_CONFIG_CHANGED_EXTERNALLY,
        ));
        return;
    }

    // Reject control characters in all fields (prevents INI injection)
    let control_char_field = {
        let pf = app.providers.form();
        [
            (&pf.url, "URL"),
            (&pf.token, "Token"),
            (&pf.alias_prefix, "Alias Prefix"),
            (&pf.user, "User"),
            (&pf.identity_file, "Identity File"),
            (&pf.profile, "Profile"),
            (&pf.project, "Project ID"),
            (&pf.regions, "Regions"),
            (&pf.filter, "Filter"),
        ]
        .into_iter()
        .find(|(value, _)| value.chars().any(|c| c.is_control()))
        .map(|(_, name)| name)
    };
    if let Some(name) = control_char_field {
        app.notify_warning(crate::messages::contains_control_chars(name));
        return;
    }

    // Providers with a self-hosted endpoint require a URL
    if kind.is_some_and(ProviderKind::requires_url) {
        let url = app.providers.form_mut().url.trim();
        if url.is_empty() {
            let msg = if kind == Some(ProviderKind::NetBox) {
                crate::i18n::text(crate::messages::URL_REQUIRED_NETBOX)
            } else {
                crate::i18n::text(crate::messages::URL_REQUIRED_PROXMOX)
            };
            app.notify_warning(msg);
            return;
        }
        if !url.to_ascii_lowercase().starts_with("https://") {
            app.notify_error(crate::i18n::text(
                crate::messages::PROVIDER_URL_REQUIRES_HTTPS,
            ));
            return;
        }
    }

    // Token may be left unset for providers that resolve credentials
    // elsewhere: an AWS profile or environment, the Tailscale or Teleport
    // local CLI.
    if app.providers.form_mut().token.trim().is_empty()
        && !kind.is_some_and(ProviderKind::token_is_optional)
    {
        let hint = if kind == Some(ProviderKind::Gcp) {
            crate::i18n::text(crate::messages::PROVIDER_TOKEN_REQUIRED_GCP).to_string()
        } else if kind == Some(ProviderKind::Oracle) {
            crate::i18n::text(crate::messages::PROVIDER_TOKEN_REQUIRED_ORACLE).to_string()
        } else {
            let display_name = crate::providers::provider_display_name(provider_name.as_str());
            crate::messages::provider_token_required(display_name)
        };
        app.notify_error(hint);
        return;
    }

    // GCP requires a project ID
    if kind == Some(ProviderKind::Gcp) && app.providers.form_mut().project.trim().is_empty() {
        app.notify_warning(crate::i18n::text(crate::messages::PROJECT_REQUIRED_GCP));
        return;
    }

    // Oracle requires a compartment OCID
    if kind == Some(ProviderKind::Oracle) && app.providers.form_mut().compartment.trim().is_empty()
    {
        app.notify_warning(crate::i18n::text(crate::messages::COMPARTMENT_REQUIRED_OCI));
        return;
    }

    // AWS/Scaleway require at least one region/zone
    if kind == Some(ProviderKind::Aws) && app.providers.form_mut().regions.trim().is_empty() {
        app.notify_warning(crate::i18n::text(crate::messages::REGIONS_REQUIRED_AWS));
        return;
    }
    if kind == Some(ProviderKind::Scaleway) && app.providers.form_mut().regions.trim().is_empty() {
        app.notify_warning(crate::i18n::text(crate::messages::ZONES_REQUIRED_SCALEWAY));
        return;
    }
    if kind == Some(ProviderKind::Azure) {
        let subs = app.providers.form().regions.trim().to_string();
        if subs.is_empty() {
            app.notify_warning(crate::i18n::text(
                crate::messages::SUBSCRIPTIONS_REQUIRED_AZURE,
            ));
            return;
        }
        for sub in subs.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
            if !crate::providers::azure::is_valid_subscription_id(sub) {
                app.notify_error(crate::messages::azure_subscription_id_invalid(sub));
                return;
            }
        }
    }

    let token = app.providers.form_mut().token.trim().to_string();
    let alias_prefix = app.providers.form_mut().alias_prefix.trim().to_string();
    if crate::ssh_config::model::is_host_pattern(&alias_prefix) {
        app.notify_warning(crate::i18n::text(crate::messages::ALIAS_PREFIX_INVALID));
        return;
    }

    let user = {
        let u = app.providers.form_mut().user.trim();
        if u.is_empty() {
            "root".to_string()
        } else {
            u.to_string()
        }
    };
    if user.contains(char::is_whitespace) {
        app.notify_warning(crate::i18n::text(crate::messages::USER_NO_WHITESPACE));
        return;
    }

    // The profile goes into a shell line purple writes, so a name it cannot
    // quote is refused here rather than at the next sync.
    if kind == Some(ProviderKind::Aws) && app.providers.form().ssm.is_enabled() {
        let profile = app.providers.form().profile.trim().to_string();
        if !profile.is_empty() && !crate::providers::aws_ssm::is_safe_profile_name(&profile) {
            app.notify_error(crate::messages::aws_ssm_profile_unsafe(&profile));
            return;
        }
    }

    let vault_role_trimmed = app.providers.form_mut().vault_role.trim();
    if !vault_role_trimmed.is_empty() && !crate::vault_ssh::is_valid_role(vault_role_trimmed) {
        app.notify_warning(crate::i18n::text(crate::messages::VAULT_ROLE_FORMAT));
        return;
    }

    let section = providers::config::ProviderSection {
        id: form_id.clone(),
        token: token.clone(),
        alias_prefix,
        user,
        identity_file: app.providers.form_mut().identity_file.trim().to_string(),
        url: app.providers.form_mut().url.trim().to_string(),
        verify_tls: app.providers.form_mut().verify_tls,
        auto_sync: app.providers.form_mut().auto_sync,
        profile: app.providers.form_mut().profile.trim().to_string(),
        regions: app.providers.form_mut().regions.trim().to_string(),
        project: app.providers.form_mut().project.trim().to_string(),
        compartment: app.providers.form_mut().compartment.trim().to_string(),
        filter: app.providers.form_mut().filter.trim().to_string(),
        vault_role: app.providers.form_mut().vault_role.trim().to_string(),
        vault_addr: app.providers.form_mut().vault_addr.trim().to_string(),
        ssm: app.providers.form_mut().ssm,
    };

    // Captured before the section moves into the config.
    let saved_record = section.log_record();

    // Snapshot for rollback. When migrating from bare to labeled, we have
    // an extra rename step on the existing section as well. Capture it.
    let old_section_by_id = app.providers.config().section_by_id(&form_id).cloned();
    let bare_id = providers::config::ProviderConfigId::bare(provider_name.clone());
    let old_bare_section = app.providers.config().section_by_id(&bare_id).cloned();

    let pending_migration = app.providers.pending_label_migration().cloned();

    // Step 1: if a lazy migration is pending, rewrite the existing bare
    // section to its new labeled id BEFORE inserting the new section.
    if let Some(migration) = &pending_migration
        && migration.provider == provider_name
        && let Some(mut existing) = old_bare_section.clone()
    {
        let new_id = providers::config::ProviderConfigId::labeled(
            migration.provider.clone(),
            migration.existing_label.clone(),
        );
        existing.id = new_id.clone();
        // Default a sensible alias_prefix for the relabeled config
        // when the user hadn't set a custom one. Bare configs use
        // the provider short label as alias_prefix; once labeled,
        // we suffix the label so the two configs don't collide.
        let short = providers::get_provider(provider_name.as_str())
            .map(|p| p.short_label().to_string())
            .unwrap_or_else(|| provider_name.clone());
        if existing.alias_prefix == short {
            existing.alias_prefix = format!("{}-{}", short, migration.existing_label);
        }
        app.providers.config_mut().remove_section_by_id(&bare_id);
        app.providers.config_mut().set_section(existing);
    }

    app.providers.config_mut().set_section(section);
    if let Err(e) = app.providers.config().save() {
        log::warn!(
            "[config] Save failed for [{}]: {}; rolling back in-memory state",
            form_id,
            e
        );
        // Rollback: restore the previous section state for the form id
        // AND any migration-time relabel of the existing bare section.
        match old_section_by_id {
            Some(old) => app.providers.config_mut().set_section(old),
            None => app.providers.config_mut().remove_section_by_id(&form_id),
        }
        if let Some(old_bare) = old_bare_section {
            // Drop any new labeled section the migration may have inserted,
            // then restore the bare one.
            if let Some(migration) = &pending_migration {
                let migrated_id = providers::config::ProviderConfigId::labeled(
                    migration.provider.clone(),
                    migration.existing_label.clone(),
                );
                app.providers
                    .config_mut()
                    .remove_section_by_id(&migrated_id);
            }
            app.providers.config_mut().set_section(old_bare);
        }
        // Drop pending migration state on failure too, so a retry doesn't
        // pick up half-applied input.
        app.providers.cancel_label_migration();
        app.notify_error(crate::messages::failed_to_save(&e));
        return;
    }
    log::debug!("[purple] provider saved: [{}] {}", form_id, saved_record);
    // Migration succeeded. Before clearing the pending state, also rewrite
    // any legacy 2-segment markers in ~/.ssh/config for this provider to
    // the new `existing_label`. Without this, the formerly-bare config's
    // hosts would be invisible to the labeled-default sync (different id),
    // get stale-marked or duplicated, and surface as data loss.
    if let Some(migration) = &pending_migration {
        let rewritten = app
            .hosts_state
            .ssh_config_mut()
            .rewrite_legacy_markers_to_label(&migration.provider, &migration.existing_label);
        if rewritten > 0 {
            log::debug!(
                "[config] provider lazy migration: rewrote {} legacy marker(s) for '{}' to label '{}'",
                rewritten,
                migration.provider,
                migration.existing_label
            );
            if let Err(e) = app.hosts_state.ssh_config().write() {
                app.notify_error(crate::messages::failed_to_save(&e));
            }
        }
        log::debug!(
            "[config] provider lazy migration: completed for '{}'",
            provider_name
        );
    }
    app.providers.cancel_label_migration();

    let display_name = crate::providers::provider_display_name(provider_name.as_str());

    // Look up by the EXACT id we just saved, not the bare provider name.
    // Otherwise a labeled save like `do:personal` would auto-sync `do:work`
    // (the first-found section for that provider).
    let sync_section = app.providers.config().section_by_id(&form_id).cloned();
    let sync_key = sync_section
        .as_ref()
        .map(|s| s.id.to_string())
        .unwrap_or_else(|| form_id.to_string());
    if !app.providers.syncing().contains_key(&sync_key) {
        if let Some(sync_section) = sync_section {
            app.providers.reset_batch_if_idle();
            let cancel = Arc::new(AtomicBool::new(false));
            app.providers.syncing_mut().insert(sync_key, cancel.clone());
            app.providers.bump_batch_total();
            app.notify(crate::messages::provider_saved_syncing(display_name));
            super::sync::spawn_provider_sync(
                &sync_section,
                events_tx.clone(),
                cancel,
                std::sync::Arc::clone(&app.env),
            );
            crate::set_sync_summary(app);
        }
    } else {
        app.notify(crate::messages::provider_saved(display_name));
    }
    // Said after the save toast, which a warning outranks, so the one fact
    // that decides whether a routed host still connects is the one left on
    // screen.
    if kind == Some(ProviderKind::Aws)
        && app.providers.form().ssm.is_enabled()
        && app.providers.form().profile.trim().is_empty()
    {
        app.notify_warning(crate::i18n::text(crate::messages::AWS_SSM_WITHOUT_PROFILE));
    }
    app.close_provider_form();
}

#[cfg(test)]
mod label_migration_tests {
    use super::*;
    use crate::app::LabelMigrationField;
    use crate::ssh_config::model::SshConfigFile;
    use crossterm::event::KeyModifiers;

    fn make_app() -> App {
        let scratch = tempfile::tempdir().expect("tempdir").keep();
        let config = SshConfigFile {
            elements: SshConfigFile::parse_content(""),
            path: scratch.join("test_config"),
            crlf: false,
            bom: false,
        };
        let mut app = App::new(config);
        app.screen = Screen::ProviderLabelMigration {
            provider: "aws".to_string(),
        };
        app.providers
            .set_pending_label_migration(Some(crate::app::PendingLabelMigration {
                provider: "aws".to_string(),
                existing_label: "default".to_string(),
                new_label: String::new(),
                focused: LabelMigrationField::New,
                cursor_pos: 0,
            }));
        app
    }

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn esc_returns_to_providers_and_clears_pending() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app();
        let (tx, _rx) = mpsc::channel();
        handle_label_migration_key(&mut app, k(KeyCode::Esc), &tx);
        assert!(matches!(app.screen, Screen::Providers));
        assert!(app.providers.pending_label_migration().is_none());
    }

    #[test]
    fn tab_toggles_focused_field() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app();
        let (tx, _rx) = mpsc::channel();
        handle_label_migration_key(&mut app, k(KeyCode::Tab), &tx);
        let p = app.providers.pending_label_migration().unwrap();
        assert!(matches!(p.focused, LabelMigrationField::Existing));
    }
}

/// Regression tests for issue #51. The TUI add-another-config flow used to
/// open the provider form with `label: Some("")` and no input field for the
/// label itself, which left the section unsavable. The fixes:
/// 1. `open_provider_form` flips `label_entry` on for the `_ =>` add branch
///    so `visible_fields()` prepends a `Label` input.
/// 2. `submit_provider_form` seals the typed label into `form_id` before
///    persisting (and rejects empty / duplicate labels with explicit toasts).
/// 3. Char input on `Label` restricts to the `[a-z0-9-]` charset to mirror
///    `validate_label` so the user cannot type something save would reject.
#[cfg(test)]
mod labeled_add_tests {
    use super::*;
    use crate::providers::config::{ProviderConfigId, ProviderSection};
    use crate::ssh_config::model::SshConfigFile;
    use crossterm::event::KeyModifiers;

    fn make_app() -> App {
        let scratch = tempfile::tempdir().expect("tempdir").keep();
        let config = SshConfigFile {
            elements: SshConfigFile::parse_content(""),
            path: scratch.join("test_config"),
            crlf: false,
            bom: false,
        };
        let mut app = App::new(config);
        let providers_path = scratch.join("providers");
        *app.providers.config_mut() = crate::providers::config::ProviderConfig {
            sections: Vec::new(),
            path_override: Some(providers_path),
        };
        app
    }

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn proxmox_section(label: Option<&str>) -> ProviderSection {
        ProviderSection {
            filter: String::new(),
            id: match label {
                Some(l) => ProviderConfigId::labeled("proxmox", l),
                None => ProviderConfigId::bare("proxmox"),
            },
            token: "user@pam!t=secret".to_string(),
            alias_prefix: "pve".to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            url: "https://pve.example.com:8006".to_string(),
            verify_tls: false,
            auto_sync: false,
            profile: String::new(),
            regions: String::new(),
            project: String::new(),
            compartment: String::new(),
            vault_role: String::new(),
            vault_addr: String::new(),
            ssm: crate::providers::aws_ssm::SsmMode::default(),
        }
    }

    #[test]
    fn open_add_flow_with_existing_labeled_enters_label_mode() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app();
        app.providers
            .config_mut()
            .set_section(proxmox_section(Some("server1")));
        open_add_config_flow(&mut app, "proxmox");
        assert!(matches!(app.screen, Screen::ProviderForm { ref id }
                if id.provider == "proxmox" && id.label.as_deref() == Some("")));
        assert!(app.providers.form_mut().label_entry);
        assert_eq!(
            app.providers.form_mut().focused_field,
            crate::app::ProviderFormField::Label
        );
        assert_eq!(app.providers.form_mut().label, "");
    }

    #[test]
    fn open_add_flow_with_bare_only_goes_to_migration_not_label_mode() {
        // The migration screen handles label collection for the bare-to-labeled
        // transition. Label-entry on the form must stay off so the two flows
        // don't double-prompt for a name.
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app();
        app.providers
            .config_mut()
            .set_section(proxmox_section(None));
        open_add_config_flow(&mut app, "proxmox");
        assert!(matches!(
            app.screen,
            Screen::ProviderLabelMigration { ref provider } if provider == "proxmox"
        ));
        // No ProviderFormFields::label_entry assertion here; the form is not
        // open at this point (migration screen is up).
    }

    #[test]
    fn open_add_flow_with_zero_existing_opens_bare_form() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app();
        open_add_config_flow(&mut app, "proxmox");
        assert!(matches!(app.screen, Screen::ProviderForm { ref id }
                if id.provider == "proxmox" && id.label.is_none()));
        assert!(!app.providers.form_mut().label_entry);
    }

    #[test]
    fn open_form_for_existing_labeled_does_not_enable_label_entry() {
        // Editing an already-named labeled config must not surface the label
        // input. Rename is out of scope here; the user changes other fields.
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app();
        app.providers
            .config_mut()
            .set_section(proxmox_section(Some("server1")));
        app.open_provider_form(ProviderConfigId::labeled("proxmox", "server1"));
        assert!(!app.providers.form_mut().label_entry);
        assert_ne!(
            app.providers.form_mut().focused_field,
            crate::app::ProviderFormField::Label
        );
    }

    #[test]
    fn visible_fields_prepends_label_when_label_entry_is_on() {
        let mut form = crate::app::ProviderFormFields::new();
        form.label_entry = true;
        let visible = form.visible_fields("proxmox");
        assert_eq!(visible.first(), Some(&crate::app::ProviderFormField::Label));
    }

    #[test]
    fn visible_fields_omits_label_when_label_entry_is_off() {
        let mut form = crate::app::ProviderFormFields::new();
        form.label_entry = false;
        let visible = form.visible_fields("proxmox");
        assert!(!visible.contains(&crate::app::ProviderFormField::Label));
    }

    #[test]
    fn char_input_on_label_field_rejects_illegal_chars() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app();
        app.providers
            .config_mut()
            .set_section(proxmox_section(Some("server1")));
        open_add_config_flow(&mut app, "proxmox");
        let (tx, _rx) = mpsc::channel();
        // Allowed
        handle_provider_form_key(&mut app, k(KeyCode::Char('w')), &tx);
        handle_provider_form_key(&mut app, k(KeyCode::Char('o')), &tx);
        handle_provider_form_key(&mut app, k(KeyCode::Char('r')), &tx);
        handle_provider_form_key(&mut app, k(KeyCode::Char('k')), &tx);
        // Rejected: uppercase, space, special
        handle_provider_form_key(&mut app, k(KeyCode::Char('A')), &tx);
        handle_provider_form_key(&mut app, k(KeyCode::Char(' ')), &tx);
        handle_provider_form_key(&mut app, k(KeyCode::Char('@')), &tx);
        assert_eq!(app.providers.form_mut().label, "work");
    }

    #[test]
    fn char_input_on_label_field_caps_at_32_chars() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app();
        app.providers
            .config_mut()
            .set_section(proxmox_section(Some("server1")));
        open_add_config_flow(&mut app, "proxmox");
        let (tx, _rx) = mpsc::channel();
        for _ in 0..40 {
            handle_provider_form_key(&mut app, k(KeyCode::Char('a')), &tx);
        }
        assert_eq!(app.providers.form_mut().label.len(), 32);
    }

    #[test]
    fn submit_with_empty_label_keeps_form_open_and_focuses_label() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app();
        app.providers
            .config_mut()
            .set_section(proxmox_section(Some("server1")));
        open_add_config_flow(&mut app, "proxmox");
        let (tx, _rx) = mpsc::channel();
        // Enter without typing anything.
        handle_provider_form_key(&mut app, k(KeyCode::Enter), &tx);
        assert!(matches!(app.screen, Screen::ProviderForm { .. }));
        assert_eq!(
            app.providers.form_mut().focused_field,
            crate::app::ProviderFormField::Label
        );
        // Original section still alone in the config.
        assert_eq!(
            app.providers
                .config()
                .sections_for_provider("proxmox")
                .len(),
            1
        );
    }

    #[test]
    fn submit_third_labeled_config_persists_with_typed_label() {
        // Issue #51 happy path: provider already has one labeled config. The
        // user presses `a`, fills in the new label, then the required Proxmox
        // fields, and presses Enter. The new section must land in the config
        // under the typed label (not under an empty-string label that
        // validate() would reject) and the form must close.
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app();
        app.providers
            .config_mut()
            .set_section(proxmox_section(Some("server1")));
        open_add_config_flow(&mut app, "proxmox");
        let (tx, _rx) = mpsc::channel();
        for c in "server2".chars() {
            handle_provider_form_key(&mut app, k(KeyCode::Char(c)), &tx);
        }
        // Drive focus through the form rather than relying on Tab navigation
        // semantics so the test verifies persistence, not collapsed-mode key
        // routing (covered separately).
        app.providers.form_mut().expanded = true;
        app.providers.form_mut().focused_field = crate::app::ProviderFormField::Url;
        app.providers.form_mut().sync_cursor_to_end();
        for c in "https://pve2.example.com:8006".chars() {
            handle_provider_form_key(&mut app, k(KeyCode::Char(c)), &tx);
        }
        app.providers.form_mut().focused_field = crate::app::ProviderFormField::Token;
        app.providers.form_mut().sync_cursor_to_end();
        for c in "user@pam!t=secret".chars() {
            handle_provider_form_key(&mut app, k(KeyCode::Char(c)), &tx);
        }
        handle_provider_form_key(&mut app, k(KeyCode::Enter), &tx);
        // Both labeled sections must coexist after submit.
        let names: Vec<String> = app
            .providers
            .config()
            .sections_for_provider("proxmox")
            .iter()
            .map(|s| s.id.to_string())
            .collect();
        assert!(
            names.contains(&"proxmox:server1".to_string()),
            "got {names:?} (label={:?} url={:?} token_len={})",
            app.providers.form().label,
            app.providers.form().url,
            app.providers.form().token.len()
        );
        assert!(
            names.contains(&"proxmox:server2".to_string()),
            "got {names:?} (label={:?} url={:?} token_len={})",
            app.providers.form().label,
            app.providers.form().url,
            app.providers.form().token.len()
        );
    }

    #[test]
    fn delete_then_readd_same_label_persists_through_label_entry() {
        // Issue #51 workaround scenario: the user has two labeled configs,
        // deletes one, then tries to add it back. The previously-deleted
        // label must no longer count as a collision, the label-entry flow
        // must accept it, and the new section must land in the config under
        // the same id with full provider data.
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app();
        app.providers.config_mut().set_section(ProviderSection {
            alias_prefix: "pve-server1".to_string(),
            ..proxmox_section(Some("server1"))
        });
        app.providers.config_mut().set_section(ProviderSection {
            alias_prefix: "pve-server2".to_string(),
            ..proxmox_section(Some("server2"))
        });
        assert_eq!(
            app.providers
                .config()
                .sections_for_provider("proxmox")
                .len(),
            2
        );

        // Delete server2 directly via the config API. The handler flow does
        // the same after the confirm step; routing it through the keystroke
        // sequence would not exercise additional code relevant here.
        app.providers
            .config_mut()
            .remove_section_by_id(&ProviderConfigId::labeled("proxmox", "server2"));
        assert_eq!(
            app.providers
                .config()
                .sections_for_provider("proxmox")
                .len(),
            1
        );

        // Press `a` and re-add `server2`. With one labeled config remaining,
        // the `_ =>` branch fires and label-entry mode opens.
        open_add_config_flow(&mut app, "proxmox");
        assert!(app.providers.form_mut().label_entry);
        let (tx, _rx) = mpsc::channel();
        for c in "server2".chars() {
            handle_provider_form_key(&mut app, k(KeyCode::Char(c)), &tx);
        }
        app.providers.form_mut().expanded = true;
        app.providers.form_mut().focused_field = crate::app::ProviderFormField::Url;
        app.providers.form_mut().sync_cursor_to_end();
        for c in "https://pve-readd.example.com:8006".chars() {
            handle_provider_form_key(&mut app, k(KeyCode::Char(c)), &tx);
        }
        app.providers.form_mut().focused_field = crate::app::ProviderFormField::Token;
        app.providers.form_mut().sync_cursor_to_end();
        for c in "user@pam!t=secret".chars() {
            handle_provider_form_key(&mut app, k(KeyCode::Char(c)), &tx);
        }
        handle_provider_form_key(&mut app, k(KeyCode::Enter), &tx);

        let names: Vec<String> = app
            .providers
            .config()
            .sections_for_provider("proxmox")
            .iter()
            .map(|s| s.id.to_string())
            .collect();
        assert!(
            names.contains(&"proxmox:server1".to_string()),
            "got {names:?}"
        );
        assert!(
            names.contains(&"proxmox:server2".to_string()),
            "got {names:?}"
        );
        let readded = app
            .providers
            .config()
            .section_by_id(&ProviderConfigId::labeled("proxmox", "server2"))
            .expect("readded section must be present");
        assert_eq!(readded.url, "https://pve-readd.example.com:8006");
    }

    #[test]
    fn submit_with_duplicate_label_keeps_form_open() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app();
        app.providers
            .config_mut()
            .set_section(proxmox_section(Some("server1")));
        open_add_config_flow(&mut app, "proxmox");
        let (tx, _rx) = mpsc::channel();
        // Type the SAME label as the existing config.
        for c in "server1".chars() {
            handle_provider_form_key(&mut app, k(KeyCode::Char(c)), &tx);
        }
        // Tab to Token and type something so URL/Token validation doesn't
        // shadow the duplicate-label check.
        handle_provider_form_key(&mut app, k(KeyCode::Tab), &tx);
        for c in "fake-url-fake".chars() {
            handle_provider_form_key(&mut app, k(KeyCode::Char(c)), &tx);
        }
        handle_provider_form_key(&mut app, k(KeyCode::Enter), &tx);
        assert!(matches!(app.screen, Screen::ProviderForm { .. }));
        assert_eq!(
            app.providers.form_mut().focused_field,
            crate::app::ProviderFormField::Label
        );
        // No duplicate inserted.
        assert_eq!(
            app.providers
                .config()
                .sections_for_provider("proxmox")
                .len(),
            1
        );
    }
}
