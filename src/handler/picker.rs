use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::ctx::{Effectful, Effects, Notify};
use crate::app::{
    App, FormState, KeysState, ProviderState, ProxyJumpCandidate, StatusCenter, UiSelection,
};
use crate::runtime::env::Env;

/// The slice of App the host/provider-form pickers touch: the picker
/// selection state (`ui`), the host form and provider form (the fields a
/// selection writes into), the discovered key list (read-only, the key
/// picker's source), the status center and the resolved environment (for
/// the askpass-default preference path). Selecting a source can auto-submit
/// the host form, a whole-App config write, which is deferred as an effect
/// after the slice borrow ends. ProxyJump and Vault SSH role candidates read
/// hosts + providers + screen together, so they are resolved in the thin
/// wrapper while it still holds `&App` and passed in; the slice never reaches
/// into hosts, search or any other domain.
struct PickerCtx<'a> {
    ui: &'a mut UiSelection,
    forms: &'a mut FormState,
    providers: &'a mut ProviderState,
    keys: &'a KeysState,
    status: &'a mut StatusCenter,
    env: &'a Env,
    effects: Effects,
}

impl Notify for PickerCtx<'_> {
    fn status_mut(&mut self) -> &mut StatusCenter {
        self.status
    }
}

impl Effectful for PickerCtx<'_> {
    fn effects_mut(&mut self) -> &mut Effects {
        &mut self.effects
    }
}

impl PickerCtx<'_> {
    fn ctx_from_app<'a>(app: &'a mut App) -> PickerCtx<'a> {
        PickerCtx {
            ui: &mut app.ui,
            forms: &mut app.forms,
            providers: &mut app.providers,
            keys: &app.keys,
            status: &mut app.status_center,
            env: app.env.as_ref(),
            effects: Effects::default(),
        }
    }

    /// Close the password picker overlay. Mirrors `App::close_password_picker`
    /// on the slice (only touches `ui`).
    fn close_password_picker(&mut self) {
        log::debug!("[purple] close_password_picker");
        self.ui.password_picker_mut().open = false;
    }

    /// Close the key picker overlay. Mirrors `App::close_key_picker`.
    fn close_key_picker(&mut self) {
        log::debug!("[purple] close_key_picker");
        self.ui.key_picker_mut().open = false;
    }

    /// Close the ProxyJump picker overlay. Mirrors `App::close_proxyjump_picker`.
    fn close_proxyjump_picker(&mut self) {
        log::debug!("[purple] close_proxyjump_picker");
        self.ui.proxyjump_picker_mut().open = false;
    }

    /// Close the Vault SSH role picker overlay. Mirrors
    /// `App::close_vault_role_picker`.
    fn close_vault_role_picker(&mut self) {
        log::debug!("[purple] close_vault_role_picker");
        self.ui.vault_role_picker_mut().open = false;
    }

    /// Move password picker selection. Mirrors `App::select_next/prev_password_source`.
    fn step_password_source(&mut self, forward: bool) {
        crate::app::cycle_selection(
            &mut self.ui.password_picker_mut().list,
            crate::askpass::PASSWORD_SOURCES.len(),
            forward,
        );
    }

    /// Move key picker selection. Mirrors `App::select_next/prev_picker_key`.
    fn step_picker_key(&mut self, forward: bool) {
        crate::app::cycle_selection(
            &mut self.ui.key_picker_mut().list,
            self.keys.list().len(),
            forward,
        );
    }

    /// Move vault role picker selection. Mirrors
    /// `App::select_next/prev_vault_role`; the candidate count is resolved in
    /// the wrapper and passed in.
    fn step_vault_role(&mut self, len: usize, forward: bool) {
        crate::app::cycle_selection(&mut self.ui.vault_role_picker_mut().list, len, forward);
    }

    /// Move proxyjump picker selection, skipping separators. Mirrors
    /// `App::select_next/prev_proxyjump` (`step_proxyjump_selection`) on the
    /// slice; `candidates` is resolved in the wrapper and passed in.
    fn step_proxyjump(&mut self, candidates: &[ProxyJumpCandidate], forward: bool) {
        let len = candidates.len();
        if len == 0 {
            self.ui.proxyjump_picker_mut().list.select(None);
            return;
        }
        // When no prior selection exists, seed `next` so the first modular
        // step lands on index 0 (forward) or len-1 (backward). Without this
        // seed a fresh picker with selected() == None would skip index 0 on
        // a Down press.
        let seed: usize = match self.ui.proxyjump_picker().list.selected() {
            Some(idx) => idx,
            None if forward => len - 1,
            None => 0,
        };
        let mut next = seed;
        for _ in 0..len {
            next = if forward {
                (next + 1) % len
            } else {
                (next + len - 1) % len
            };
            if matches!(candidates.get(next), Some(ProxyJumpCandidate::Host { .. })) {
                self.ui.proxyjump_picker_mut().list.select(Some(next));
                return;
            }
        }
    }
}

pub(super) fn handle_password_picker(app: &mut App, key: KeyEvent) {
    let effects = {
        let mut ctx = PickerCtx::ctx_from_app(app);
        password_picker_key(&mut ctx, key);
        ctx.effects
    };
    effects.apply(app);
}

fn password_picker_key(ctx: &mut PickerCtx, key: KeyEvent) {
    // Ctrl+D sets selected source as global default
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('d') {
        if let Some(index) = ctx.ui.password_picker().list.selected()
            && let Some(source) = crate::askpass::PASSWORD_SOURCES.get(index)
        {
            let is_none = source.label == "None";
            let value = if is_none { "" } else { source.value };
            match crate::preferences::save_askpass_default(ctx.env.paths(), value) {
                Ok(()) => {
                    if is_none {
                        ctx.notify(crate::i18n::text(crate::messages::GLOBAL_DEFAULT_CLEARED));
                    } else {
                        ctx.notify(crate::messages::global_default_set(source.label));
                    }
                }
                Err(e) => {
                    ctx.notify_error(crate::messages::save_default_failed(&e));
                }
            }
        }
        ctx.close_password_picker();
        return;
    }

    match key.code {
        KeyCode::Esc => {
            ctx.close_password_picker();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            ctx.step_password_source(true);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            ctx.step_password_source(false);
        }
        KeyCode::Enter => {
            let mut needs_more_input = false;
            if let Some(index) = ctx.ui.password_picker().list.selected()
                && let Some(source) = crate::askpass::PASSWORD_SOURCES.get(index)
            {
                let is_none = source.label == "None";
                let is_custom_cmd = source.label == "Custom command";
                let is_prefix = source.value.ends_with(':') || source.value.ends_with("//");
                if is_none {
                    ctx.forms
                        .host_mut()
                        .apply_password_source(String::new(), false);
                    ctx.notify(crate::i18n::text(crate::messages::PASSWORD_SOURCE_CLEARED));
                } else if is_custom_cmd {
                    ctx.forms
                        .host_mut()
                        .apply_password_source(String::new(), true);
                    ctx.notify(crate::i18n::text(
                        crate::messages::ASKPASS_CUSTOM_COMMAND_HINT,
                    ));
                    needs_more_input = true;
                } else if is_prefix {
                    ctx.forms
                        .host_mut()
                        .apply_password_source(source.value.to_string(), true);
                    ctx.notify(crate::messages::complete_path(source.label));
                    needs_more_input = true;
                } else {
                    ctx.forms
                        .host_mut()
                        .apply_password_source(source.value.to_string(), false);
                    ctx.notify(crate::messages::password_source_set(source.label));
                }
            }
            ctx.close_password_picker();
            if !needs_more_input {
                ctx.defer(try_auto_submit_after_picker);
            }
        }
        _ => {}
    }
}

/// Unified key picker handler for both host form and provider form.
pub(super) fn handle_key_picker_shared(app: &mut App, key: KeyEvent, for_provider: bool) {
    let effects = {
        let mut ctx = PickerCtx::ctx_from_app(app);
        key_picker_shared_key(&mut ctx, key, for_provider);
        ctx.effects
    };
    effects.apply(app);
}

fn key_picker_shared_key(ctx: &mut PickerCtx, key: KeyEvent, for_provider: bool) {
    match key.code {
        KeyCode::Esc => {
            ctx.close_key_picker();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            ctx.step_picker_key(true);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            ctx.step_picker_key(false);
        }
        KeyCode::Enter => {
            if let Some(index) = ctx.ui.key_picker().list.selected()
                && let Some(key_info) = ctx.keys.list().get(index)
            {
                if for_provider {
                    ctx.providers.form_mut().identity_file = key_info.display_path.clone();
                    ctx.providers.form_mut().sync_cursor_to_end();
                } else {
                    ctx.forms.host_mut().identity_file = key_info.display_path.clone();
                    ctx.forms.host_mut().sync_cursor_to_end();
                }
                ctx.notify(crate::messages::key_selected(&key_info.name));
            }
            ctx.close_key_picker();
            if !for_provider {
                ctx.defer(try_auto_submit_after_picker);
            }
        }
        _ => {}
    }
}

/// ProxyJump picker handler for the host form.
pub(super) fn handle_proxyjump_picker(app: &mut App, key: KeyEvent) {
    // ProxyJump candidates read hosts + screen together, so resolve them in
    // the wrapper while it still holds `&App`, then pass into the slice.
    let candidates = app.proxyjump_candidates();
    let effects = {
        let mut ctx = PickerCtx::ctx_from_app(app);
        proxyjump_picker_key(&mut ctx, key, &candidates);
        ctx.effects
    };
    effects.apply(app);
}

fn proxyjump_picker_key(ctx: &mut PickerCtx, key: KeyEvent, candidates: &[ProxyJumpCandidate]) {
    match key.code {
        KeyCode::Esc => {
            ctx.close_proxyjump_picker();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            ctx.step_proxyjump(candidates, true);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            ctx.step_proxyjump(candidates, false);
        }
        KeyCode::Enter => {
            if let Some(index) = ctx.ui.proxyjump_picker().list.selected()
                && let Some(crate::app::ProxyJumpCandidate::Host { alias, .. }) =
                    candidates.get(index)
            {
                ctx.forms.host_mut().proxy_jump = alias.clone();
                ctx.forms.host_mut().sync_cursor_to_end();
                ctx.notify(crate::messages::proxy_jump_set(alias));
                ctx.close_proxyjump_picker();
                ctx.defer(try_auto_submit_after_picker);
            }
            // Separator selected: no-op, stay in picker.
        }
        _ => {}
    }
}

pub(super) fn handle_vault_role_picker(app: &mut App, key: KeyEvent) {
    // Vault SSH role candidates read hosts + providers together, so resolve
    // them in the wrapper while it still holds `&App`, then pass into the
    // slice. This path defers nothing today, but applying effects keeps it
    // consistent with the sibling pickers and future-proof.
    let candidates = app.vault_role_candidates();
    let effects = {
        let mut ctx = PickerCtx::ctx_from_app(app);
        vault_role_picker_key(&mut ctx, key, &candidates);
        ctx.effects
    };
    effects.apply(app);
}

fn vault_role_picker_key(ctx: &mut PickerCtx, key: KeyEvent, candidates: &[String]) {
    match key.code {
        KeyCode::Esc => {
            ctx.close_vault_role_picker();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            ctx.step_vault_role(candidates.len(), true);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            ctx.step_vault_role(candidates.len(), false);
        }
        KeyCode::Enter => {
            if let Some(index) = ctx.ui.vault_role_picker().list.selected()
                && let Some(role) = candidates.get(index)
            {
                ctx.forms.host_mut().vault_ssh = role.clone();
                ctx.forms.host_mut().sync_cursor_to_end();
                ctx.notify(crate::messages::vault_role_set(role));
            }
            ctx.close_vault_role_picker();
        }
        _ => {}
    }
}

/// Auto-submit the host form after a picker selection if all required fields are filled.
pub(super) fn try_auto_submit_after_picker(app: &mut App) {
    if !app.forms.host_mut().alias.is_empty() && !app.forms.host_mut().hostname.is_empty() {
        super::host_form::submit_form(app);
    }
}
