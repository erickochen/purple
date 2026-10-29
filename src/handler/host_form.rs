use crossterm::event::{KeyCode, KeyEvent};

use super::ctx::{Effectful, Effects, Notify};
use crate::app::{App, FormField, FormState, Screen, StatusCenter};
use crate::quick_add;
use crate::ssh_config::model::HostEntry;

/// The slice of App the host-form key handler touches while editing: the host
/// form fields (`forms`) and the status center (`status`, for the smart-paste
/// toasts). Submitting, closing and opening a picker each reach far beyond a
/// form slice (the save path writes the config, reloads hosts, migrates
/// alias-keyed caches and refreshes the cert cache; closing flushes a pending
/// vault write; the key/proxyjump/vault-role pickers read keys, hosts and
/// providers), so those whole-App operations are deferred as effects and
/// applied to the full `App` after the slice borrow ends. The slice never
/// reaches into hosts, vault, providers or any other domain directly.
struct HostFormCtx<'a> {
    forms: &'a mut FormState,
    status: &'a mut StatusCenter,
    effects: Effects,
}

impl Notify for HostFormCtx<'_> {
    fn status_mut(&mut self) -> &mut StatusCenter {
        self.status
    }
}

impl Effectful for HostFormCtx<'_> {
    fn effects_mut(&mut self) -> &mut Effects {
        &mut self.effects
    }
}

pub(super) fn handle_key(app: &mut App, key: KeyEvent) {
    // Picker dispatch stays in the wrapper: each picker handler takes the full
    // `&mut App` (its own migrated slice reads keys/hosts/providers), so we
    // delegate before narrowing to the form slice.
    if app.ui.password_picker().open {
        super::picker::handle_password_picker(app, key);
        return;
    }
    if app.ui.key_picker().open {
        super::picker::handle_key_picker_shared(app, key, false);
        return;
    }
    if app.ui.proxyjump_picker().open {
        super::picker::handle_proxyjump_picker(app, key);
        return;
    }
    if app.ui.vault_role_picker().open {
        super::picker::handle_vault_role_picker(app, key);
        return;
    }

    let effects = {
        let mut ctx = HostFormCtx {
            forms: &mut app.forms,
            status: &mut app.status_center,
            effects: Effects::default(),
        };
        host_form_key(&mut ctx, key);
        ctx.effects
    };
    effects.apply(app);
}

fn host_form_key(ctx: &mut HostFormCtx, key: KeyEvent) {
    // Handle discard confirmation dialog via the shared confirm router.
    if ctx.forms.is_discard_pending() {
        match super::route_confirm_key(key) {
            super::ConfirmAction::Yes => {
                ctx.forms.dismiss_discard_confirm();
                ctx.defer(App::close_host_form);
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
            if ctx.forms.host_form_is_dirty() {
                ctx.forms.request_discard_confirm();
            } else {
                ctx.defer(App::close_host_form);
            }
        }
        KeyCode::Tab | KeyCode::Down => {
            // Smart paste detection: when leaving Alias field, check for user@host:port
            if ctx.forms.host_mut().focused_field == FormField::Alias {
                smart_paste(ctx.forms, ctx.status);
            }
            if !ctx.forms.host_mut().expanded {
                // Collapsed mode: Tab/Down from last required field expands
                match ctx.forms.host_mut().focused_field {
                    FormField::Alias => {
                        ctx.forms.host_mut().focused_field = FormField::Hostname;
                    }
                    FormField::Hostname => {
                        ctx.forms.host_mut().expanded = true;
                        ctx.forms.host_mut().focused_field = FormField::User;
                    }
                    // Defensive: if focus is on an optional field while collapsed, reset
                    _ => {
                        ctx.forms.host_mut().focused_field = FormField::Alias;
                    }
                }
            } else {
                // Progressive disclosure: advance through the visible field
                // subset so Tab skips over the hidden `VaultAddr` field when
                // no role is set.
                ctx.forms.host_mut().focus_next_visible();
            }
            ctx.forms.host_mut().sync_cursor_to_end();
            ctx.forms.host_mut().update_hint();
        }
        KeyCode::BackTab | KeyCode::Up => {
            if !ctx.forms.host_mut().expanded {
                // Collapsed: cycle within required fields only
                ctx.forms.host_mut().focused_field = match ctx.forms.host_mut().focused_field {
                    FormField::Alias => FormField::Hostname,
                    // Any other field (including Hostname): go to Alias
                    _ => FormField::Alias,
                };
            } else {
                ctx.forms.host_mut().focus_prev_visible();
            }
            ctx.forms.host_mut().sync_cursor_to_end();
            ctx.forms.host_mut().update_hint();
        }
        KeyCode::Left if ctx.forms.host_mut().cursor_pos > 0 => {
            ctx.forms.host_mut().cursor_pos -= 1;
        }
        KeyCode::Right => {
            let len = ctx.forms.host_mut().focused_value().chars().count();
            if ctx.forms.host_mut().cursor_pos < len {
                ctx.forms.host_mut().cursor_pos += 1;
            }
        }
        KeyCode::Home => {
            ctx.forms.host_mut().cursor_pos = 0;
        }
        KeyCode::End => {
            ctx.forms.host_mut().sync_cursor_to_end();
        }
        KeyCode::Enter => {
            // INVARIANT: Enter ALWAYS submits the form, regardless of focused
            // field. Pickers are reached via Space (see Char(' ') arm below).
            // Smart-paste detection runs before submit on the Alias field so
            // pasted user@host:port targets get split into the right fields.
            if ctx.forms.host_mut().focused_field == FormField::Alias {
                smart_paste(ctx.forms, ctx.status);
            }
            ctx.defer(submit_form);
        }
        // SPACE GUARD MUST PRECEDE the generic Char(c) arm.
        // Rust matches arms top-to-bottom; reordering this arm below the
        // generic insert-char would let Space fall through as a literal
        // character and break picker activation.
        //
        // The "empty-field" gate preserves free-text editing: once the
        // user has typed anything, Space inserts a literal space (so paths
        // like `/home/me/My Keys/id_rsa` and custom askpass commands like
        // `my-script %h` work). On an empty picker field, Space opens the
        // picker — that is the affordance that makes pickers discoverable.
        //
        // Edge case: `VaultSsh` is `is_picker() == true` even when no role
        // candidates are configured (the role list is provider-derived).
        // In that case `open_picker_for_focused_field` short-circuits and
        // inserts a literal space — Space on empty VaultSsh with no
        // candidates degrades cleanly to "type the role yourself".
        KeyCode::Char(' ')
            if ctx.forms.host_mut().focused_field.is_picker()
                && ctx.forms.host_mut().focused_value().is_empty() =>
        {
            ctx.defer(open_picker_for_focused_field);
        }
        KeyCode::Char(c) => {
            ctx.forms.host_mut().insert_char(c);
            ctx.forms.host_mut().update_hint();
        }
        KeyCode::Backspace => {
            ctx.forms.host_mut().delete_char_before_cursor();
            ctx.forms.host_mut().update_hint();
        }
        _ => {}
    }
}

/// If the alias field contains something like user@host:port, auto-parse and fill fields.
/// Also detects bare domains and IP addresses (e.g. "db.example.com", "192.168.1.1")
/// and moves them to the hostname field with a short alias derived from the first segment.
/// `notify` matches `App::notify` exactly, so the toast wording and class are
/// unchanged from the pre-slice handler.
fn smart_paste(forms: &mut FormState, status: &mut StatusCenter) {
    let alias_value = forms.host().alias.clone();
    if quick_add::looks_like_target(&alias_value) {
        if let Ok(parsed) = quick_add::parse_target(&alias_value) {
            let clean_alias = parsed
                .hostname
                .split('.')
                .next()
                .unwrap_or(&parsed.hostname)
                .to_string();
            forms.host_mut().apply_smart_paste(parsed, clean_alias);
            status.notify(crate::i18n::text(crate::messages::SMART_PARSED));
            log::debug!(
                "[purple] host_form: smart-paste parsed alias={} host={} user={} port={}",
                forms.host().alias,
                forms.host().hostname,
                forms.host().user,
                forms.host().port
            );
        }
        return;
    }

    // Detect bare domain or IP address in the alias field.
    // Must contain a dot, no interior whitespace, and only valid hostname
    // characters (alphanumeric, dot, hyphen, underscore). Colons are excluded
    // to avoid false positives on IPv6 notations like ::ffff:192.0.2.1.
    let trimmed = alias_value.trim();
    if trimmed.len() >= 4
        && trimmed.contains('.')
        && !trimmed.starts_with('.')
        && !trimmed.ends_with('.')
        && !trimmed.contains(char::is_whitespace)
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
        && forms.host_mut().hostname.is_empty()
    {
        // Copy the value to the Host field as a suggestion. The Name field
        // stays unchanged so the user keeps full control over the alias.
        forms.host_mut().hostname = trimmed.to_string();
        status.notify(crate::i18n::text(crate::messages::LOOKS_LIKE_ADDRESS));
        log::debug!("[purple] host_form: auto-suggest hostname={trimmed}");
    }
}

/// Open the picker overlay appropriate for the currently focused field.
///
/// Space activates picker fields. `VaultSsh` is special: when the host has
/// no role candidates (no provider configured a role) Space still inserts a
/// literal space so the user can type the role manually. Other picker
/// fields always open their picker.
fn open_picker_for_focused_field(app: &mut App) {
    match app.forms.host_mut().focused_field {
        FormField::IdentityFile => {
            app.open_key_picker();
        }
        FormField::ProxyJump => {
            app.open_proxyjump_picker();
        }
        FormField::VaultSsh => {
            let candidates = app.vault_role_candidates();
            if candidates.is_empty() {
                // No candidates → fall through to literal-space insert so
                // the user can type the role manually. Picker opens only
                // when there is something to pick.
                app.forms.host_mut().insert_char(' ');
                app.forms.host_mut().update_hint();
            } else {
                app.open_vault_role_picker();
            }
        }
        FormField::AskPass => {
            app.open_password_picker();
        }
        // Defensive: only reached if `FormField::is_picker()` grows a new
        // variant without a matching arm here. Insert a literal space so
        // typing keeps working while the gap is fixed; debug builds panic
        // to surface the drift.
        other => {
            debug_assert!(
                false,
                "open_picker_for_focused_field has no arm for picker field {:?}",
                other
            );
            app.forms.host_mut().insert_char(' ');
            app.forms.host_mut().update_hint();
        }
    }
}

pub(super) fn submit_form(app: &mut App) {
    log::debug!(
        "[purple] host form submit: screen={:?} alias='{}' is_pattern={}",
        std::mem::discriminant(&app.screen),
        app.forms.host().alias,
        app.forms.host().is_pattern
    );
    // Check for external config changes since form was opened
    if app.config_changed_since_form_open() {
        log::warn!("[purple] host form submit: external config change, aborting");
        app.notify_warning(crate::i18n::text(
            crate::messages::CONFIG_CHANGED_EXTERNALLY,
        ));
        return;
    }

    // Validate
    if let Err(msg) = app.forms.host_mut().validate() {
        log::warn!("[purple] host form validate failed: {}", msg);
        app.notify_error(msg);
        return;
    }

    // Track old askpass to detect keychain removal
    let old_askpass = match &app.screen {
        Screen::EditHost { alias } => app
            .hosts_state
            .list()
            .iter()
            .find(|h| h.alias == *alias)
            .and_then(|h| h.askpass.clone()),
        _ => None,
    };

    let result = match &app.screen {
        Screen::AddHost => app.add_host_from_form(),
        Screen::EditHost { alias } => {
            let old = alias.clone();
            app.edit_host_from_form(&old)
        }
        _ => return,
    };
    match result {
        Ok(msg) => {
            // Clear undo buffer after successful write
            app.hosts_state.clear_undo();
            // Handle keychain changes on edit
            let mut final_msg = msg;
            if old_askpass.as_deref() == Some("keychain") {
                if app.forms.host_mut().askpass != "keychain" {
                    // Source changed away from keychain. remove old entry
                    if let Screen::EditHost { ref alias } = app.screen {
                        let _ = crate::askpass::remove_from_keychain(&app.env, alias);
                    }
                    final_msg = format!("{}. Keychain entry removed.", final_msg);
                } else if let Screen::EditHost { ref alias } = app.screen {
                    // Alias renamed. migrate keychain entry
                    if *alias != app.forms.host_mut().alias
                        && let Ok(pw) = crate::askpass::retrieve_keychain_password(&app.env, alias)
                        && crate::askpass::store_in_keychain(
                            &app.env,
                            &app.forms.host_mut().alias,
                            &pw,
                        )
                        .is_ok()
                    {
                        let _ = crate::askpass::remove_from_keychain(&app.env, alias);
                    }
                }
            }
            // Drain any side-channel cleanup warning produced during the
            // mutation. When set, it overrides the success message because
            // the user needs to see that something on disk failed.
            if let Some(warning) = app.vault.take_cleanup_warning() {
                app.notify_error(warning);
            } else {
                app.notify(final_msg);
            }
        }
        Err(msg) => {
            app.notify_error(msg);
            return;
        }
    }

    let target_alias = app.forms.host_mut().alias.trim().to_string();
    // Editing a stale host means the user asserts it is still wanted.
    // `edit_host_from_form` already invoked `rename_aliases` which moves
    // every alias-keyed cache and persistent state in one step, so
    // submit_form no longer needs a separate migration call here.
    if let Screen::EditHost { ref alias } = app.screen {
        let _ = app.hosts_state.ssh_config_mut().clear_host_stale(alias);
        if *alias != target_alias {
            let _ = app
                .hosts_state
                .ssh_config_mut()
                .clear_host_stale(&target_alias);
        }
    }
    app.close_host_form_after_save(&target_alias);
    // Form save may have introduced a new host (Add) or renamed an
    // existing one (Edit). Queue exactly the saved alias for the
    // initial container-cache fetch. Drained next tick by the main
    // loop. Only this alias is fetched: prevents an unrelated
    // cache-missing host from triggering an unwanted SSH connection
    // when the user edits an existing host.
    app.container_state.queue_fetch(target_alias);
}

/// Compute the stale-hint that App::open_host_edit_form expects.
pub(super) fn stale_hint_for(host: &HostEntry) -> Option<String> {
    host.stale
        .is_some()
        .then(|| super::host_list::stale_provider_hint(host))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stale_host(provider: Option<&str>) -> HostEntry {
        HostEntry {
            alias: "h".to_string(),
            stale: Some(1_711_900_000),
            provider: provider.map(str::to_string),
            ..HostEntry::default()
        }
    }

    #[test]
    fn returns_none_when_host_is_not_stale() {
        let host = HostEntry {
            alias: "fresh".to_string(),
            ..HostEntry::default()
        };
        assert_eq!(stale_hint_for(&host), None);
    }

    #[test]
    fn returns_provider_label_when_stale_and_provider_known() {
        let host = stale_host(Some("digitalocean"));
        let hint = stale_hint_for(&host).expect("stale host yields Some");
        assert!(
            hint.contains("DigitalOcean"),
            "expected display name in hint, got {hint:?}"
        );
    }

    // Stale host without a provider yields Some("") (not None). Eleven handler
    // callsites treat the Option as "warn iff Some", so an empty hint still
    // surfaces the stale warning; the toast simply omits the provider clause.
    #[test]
    fn returns_some_empty_when_stale_but_provider_unset() {
        let host = stale_host(None);
        assert_eq!(stale_hint_for(&host), Some(String::new()));
    }
}
