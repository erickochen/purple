use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use crossterm::event::{KeyCode, KeyEvent};
use log::{debug, warn};

use super::ctx::{Nav, Notify};
use crate::app::{
    App, FormState, HostState, Screen, SnippetState, StatusCenter, TunnelState, UiSelection,
};
use crate::clipboard;
use crate::event::AppEvent;
use crate::preferences;
use crate::runtime::env::Env;

/// A narrow, explicit borrow of the App state the snippet handlers touch. The
/// handlers operate on this slice instead of `&mut App`, so the compiler
/// rejects any reach into unrelated state (vault, containers, providers, ...).
/// Every snippet operation (form submit, picker selection, param substitution,
/// output capture) mutates only these fields, so there is nothing to defer:
/// the snippet store write, the form open/close, the picker selection and the
/// background execution spawn all stay slice-local. Output runs on a worker
/// thread that owns the inputs built here, so no effect outlives the borrow.
struct SnippetCtx<'a> {
    snippets: &'a mut SnippetState,
    ui: &'a mut UiSelection,
    forms: &'a mut FormState,
    hosts: &'a mut HostState,
    tunnels: &'a TunnelState,
    status: &'a mut StatusCenter,
    screen: &'a mut Screen,
    demo_mode: bool,
    env: &'a Env,
    config_path: &'a std::path::Path,
    bw_session: Option<&'a str>,
    /// Proven security key PINs a snippet run may hand out.
    security_key: crate::security_key::SecurityKeyRun,
}

impl Nav for SnippetCtx<'_> {
    fn screen_mut(&mut self) -> &mut Screen {
        self.screen
    }
}

impl Notify for SnippetCtx<'_> {
    fn status_mut(&mut self) -> &mut StatusCenter {
        self.status
    }
}

impl SnippetCtx<'_> {
    /// Move snippet picker selection down. Mirrors `App::select_next_snippet`.
    fn select_next_snippet(&mut self) {
        let len = self.snippets.store().snippets.len();
        crate::app::cycle_selection(self.ui.snippet_picker_state_mut(), len, true);
    }

    /// Move snippet picker selection up. Mirrors `App::select_prev_snippet`.
    fn select_prev_snippet(&mut self) {
        let len = self.snippets.store().snippets.len();
        crate::app::cycle_selection(self.ui.snippet_picker_state_mut(), len, false);
    }

    /// Open a blank snippet add form scoped to the given targets. Mirrors
    /// `App::open_snippet_add_form` on the slice (snippets form + baseline +
    /// screen only).
    fn open_snippet_add_form(&mut self, target_aliases: Vec<String>) {
        log::debug!(
            "[purple] open_snippet_add_form aliases={}",
            target_aliases.len()
        );
        *self.snippets.form_mut() = crate::app::SnippetForm::new();
        self.snippets.set_flow_targets(target_aliases);
        self.snippets.set_form_editing(None);
        self.set_screen(Screen::SnippetForm);
        self.capture_snippet_form_baseline();
    }

    /// Open an edit form for an existing snippet. Mirrors
    /// `App::open_snippet_edit_form` on the slice.
    fn open_snippet_edit_form(
        &mut self,
        snippet: &crate::snippet::Snippet,
        target_aliases: Vec<String>,
        editing: usize,
    ) {
        log::debug!(
            "[purple] open_snippet_edit_form name={} editing={}",
            snippet.name,
            editing
        );
        // Seed the form's default-hosts field from the snippet's saved targets so
        // it shows (and round-trips) the current defaults. Read before the form
        // is replaced to avoid an overlapping borrow.
        let saved_targets = self.snippets.store().targets_for(&snippet.name).to_vec();
        *self.snippets.form_mut() = crate::app::SnippetForm::from_snippet(snippet);
        self.snippets.form_mut().default_hosts = saved_targets;
        self.snippets.set_flow_targets(target_aliases);
        self.snippets.set_form_editing(Some(editing));
        self.set_screen(Screen::SnippetForm);
        self.capture_snippet_form_baseline();
    }

    /// Tear down snippet form state and return to the snippet picker. Mirrors
    /// `App::close_snippet_form` on the slice.
    fn close_snippet_form(&mut self, target_aliases: Vec<String>) {
        log::debug!(
            "[purple] close_snippet_form aliases={}",
            target_aliases.len()
        );
        self.snippets.set_form_baseline(None);
        self.snippets.set_flow_targets(target_aliases);
        self.snippets.set_form_editing(None);
        let screen = if self.snippets.form_return_to_tab() {
            Screen::HostList
        } else {
            Screen::SnippetPicker
        };
        self.set_screen(screen);
    }

    /// Capture a baseline of the snippet form for the dirty-check on Esc.
    /// Mirrors `App::capture_snippet_form_baseline`.
    fn capture_snippet_form_baseline(&mut self) {
        self.snippets
            .set_form_baseline(Some(crate::app::SnippetFormBaseline {
                name: self.snippets.form().name.clone(),
                command: self.snippets.form().command.clone(),
                description: self.snippets.form().description.clone(),
                interactive: self.snippets.form().interactive,
                default_hosts: self.snippets.form().default_hosts.clone(),
            }));
    }

    /// Open the host multi-select picker to choose the edit form's default
    /// hosts. Pre-selects the form's current default hosts that still exist.
    /// Guards the empty-host case so the picker never opens with no rows.
    fn open_default_hosts_picker(&mut self) {
        if self.hosts.list().is_empty() {
            self.notify_warning(crate::i18n::text(crate::messages::PICKER_NO_HOSTS));
            return;
        }
        self.snippets.reset_host_pick();
        self.snippets.host_pick_mut().purpose = crate::app::SnippetHostPickPurpose::EditDefault;
        let existing: std::collections::HashSet<String> =
            self.hosts.list().iter().map(|h| h.alias.clone()).collect();
        let seed: Vec<String> = self
            .snippets
            .form()
            .default_hosts
            .iter()
            .filter(|a| existing.contains(*a))
            .cloned()
            .collect();
        for alias in seed {
            self.snippets.host_pick_mut().selected.insert(alias);
        }
        // Carry the form's identity so the picker title reads correctly; the
        // EditDefault commit writes back to the form, it does not run.
        let snippet = crate::snippet::Snippet {
            name: self.snippets.form().name.clone(),
            command: self.snippets.form().command.clone(),
            description: self.snippets.form().description.clone(),
            interactive: self.snippets.form().interactive,
        };
        self.snippets.set_flow_snippet(Some(snippet));
        log::debug!(
            "[purple] open default-hosts picker: {} preselected",
            self.snippets.host_pick().selected.len()
        );
        self.set_screen(Screen::SnippetHostPicker);
    }

    /// Indices of snippets matching the active picker search query. Mirrors
    /// `App::filtered_snippet_indices` on the slice.
    fn filtered_snippet_indices(&self) -> Vec<usize> {
        crate::snippet::filtered_indices(
            self.snippets.store(),
            self.ui.snippet_search().map(String::as_str),
        )
    }
}

/// Borrow the disjoint App fields the snippet handlers need into one slice.
fn ctx_from_app(app: &mut App) -> SnippetCtx<'_> {
    let security_key = app.security_key_confirmed_run();
    SnippetCtx {
        snippets: &mut app.snippets,
        ui: &mut app.ui,
        forms: &mut app.forms,
        hosts: &mut app.hosts_state,
        tunnels: &app.tunnels,
        status: &mut app.status_center,
        screen: &mut app.screen,
        demo_mode: app.demo_mode,
        env: app.env.as_ref(),
        config_path: app.reload.config_path(),
        bw_session: app.bw_session.as_deref(),
        security_key,
    }
}

pub(super) fn open_snippet_picker(app: &mut App, aliases: Vec<String>) {
    let mut ctx = ctx_from_app(app);
    *ctx.snippets.store_mut() = crate::snippet::SnippetStore::load(ctx.env.paths());
    *ctx.ui.snippet_picker_state_mut() = ratatui::widgets::ListState::default();
    if !ctx.snippets.store().snippets.is_empty() {
        ctx.ui.snippet_picker_state_mut().select(Some(0));
    }
    ctx.snippets.set_flow_targets(aliases);
    ctx.set_screen(Screen::SnippetPicker);
}

/// Run the snippet chosen on the Snippets tab against the committed
/// `flow_targets`, reusing the standard param-or-output flow. Called from the
/// run-snippet confirm handler after the user confirms.
pub(super) fn run_flow_snippet(app: &mut App, events_tx: &mpsc::Sender<AppEvent>) {
    let Some(snippet) = app.snippets.take_flow_snippet() else {
        return;
    };
    let targets = app.snippets.flow_targets().to_vec();
    let terminal = app.snippets.flow_terminal();
    debug!(
        "[purple] snippet run confirmed: name={:?} hosts={} terminal={}",
        snippet.name,
        targets.len(),
        terminal
    );
    // A parametrised run opens the param form; mark it tab-origin so it renders
    // over the tab and Esc returns there rather than to the host-list picker.
    app.snippets.set_form_return_to_tab(true);
    let mut ctx = ctx_from_app(app);
    run_or_prompt_params(&mut ctx, snippet, targets, terminal, events_tx);
}

pub(super) fn handle_picker_key(app: &mut App, key: KeyEvent, events_tx: &mpsc::Sender<AppEvent>) {
    if !matches!(app.screen, Screen::SnippetPicker) {
        return;
    }
    // The picker is always picker-origin, so forms opened here return to it.
    app.snippets.set_form_return_to_tab(false);
    // Delete confirmation takes precedence over the main keymap, but `?` still
    // opens help and search mode still owns input (mirrors picker_key's order).
    if app.ui.snippet_search().is_none()
        && key.code != KeyCode::Char('?')
        && app.snippets.pending_delete().is_some()
    {
        confirm_pending_snippet_delete(app, key);
        return;
    }
    let mut ctx = ctx_from_app(app);
    picker_key(&mut ctx, key, events_tx);
}

/// Resolve a pending snippet delete from a confirm key. `y` removes the snippet
/// and saves, rolling back on failure; `n`/`Esc` cancels. Shared by the
/// host-list snippet picker and the Snippets tab. No-op when nothing is
/// pending. Adjusts both the picker cursor and the tab cursor so neither runs
/// off the end of the shortened list.
pub(super) fn confirm_pending_snippet_delete(app: &mut App, key: KeyEvent) {
    match super::route_confirm_key(key) {
        super::ConfirmAction::Yes => {
            let Some(sel) = app.snippets.take_pending_delete() else {
                return;
            };
            if sel < app.snippets.store().snippets.len() {
                let removed = app.snippets.store_mut().snippets.remove(sel);
                if let Err(e) = app.snippets.store_mut().save() {
                    warn!("[config] snippet delete save failed, rolling back: {e}");
                    app.snippets.store_mut().snippets.insert(sel, removed);
                    app.notify_error(crate::messages::failed_to_save(&e));
                } else {
                    let len = app.snippets.store().snippets.len();
                    // Picker cursor (store-indexed).
                    if len == 0 {
                        app.ui.snippet_picker_state_mut().select(None);
                    } else if sel >= len {
                        app.ui.snippet_picker_state_mut().select(Some(len - 1));
                    }
                    // Snippets-tab cursor (filtered-index). The renderer also
                    // re-clamps, but keep both cursors in range after a delete.
                    let filtered =
                        crate::snippet::filtered_indices(app.snippets.store(), app.search.query())
                            .len();
                    let tab_sel = app.snippets.list_state().selected();
                    if filtered == 0 {
                        app.snippets.list_state_mut().select(None);
                    } else if tab_sel.is_some_and(|i| i >= filtered) {
                        app.snippets.list_state_mut().select(Some(filtered - 1));
                    }
                    debug!("[purple] snippet removed: {}", removed.name);
                    // Drop the run ledger entry too, so recreating the same name
                    // starts with a clean TRACK RECORD instead of inheriting the
                    // deleted snippet's verdict and trend chart.
                    let paths = app.env().paths().cloned();
                    app.snippets.runs_mut().remove(&removed.name);
                    if let Err(e) = app.snippets.runs().flush(paths.as_ref()) {
                        warn!("[config] snippet_runs flush after delete failed: {e}");
                    }
                    app.notify(crate::messages::snippet_removed(&removed.name));
                }
            }
        }
        super::ConfirmAction::No => app.snippets.cancel_delete(),
        super::ConfirmAction::Ignored => {}
    }
}

/// Open a blank snippet add form from the Snippets tab. The form returns to the
/// tab on close (no target hosts; the tab is snippet-centric).
pub(super) fn open_add_form_for_tab(app: &mut App) {
    app.snippets.set_form_return_to_tab(true);
    let mut ctx = ctx_from_app(app);
    ctx.open_snippet_add_form(Vec::new());
}

/// Open an edit form for the snippet at `editing` from the Snippets tab.
pub(super) fn open_edit_form_for_tab(app: &mut App, editing: usize) {
    let Some(snippet) = app.snippets.store().snippets.get(editing).cloned() else {
        return;
    };
    app.snippets.set_form_return_to_tab(true);
    let mut ctx = ctx_from_app(app);
    ctx.open_snippet_edit_form(&snippet, Vec::new(), editing);
}

fn picker_key(ctx: &mut SnippetCtx, key: KeyEvent, events_tx: &mpsc::Sender<AppEvent>) {
    if !matches!(*ctx.screen, Screen::SnippetPicker) {
        return;
    }
    let target_aliases: Vec<String> = ctx.snippets.flow_targets().to_vec();

    // Allow ? to open help even during search
    if key.code == KeyCode::Char('?') {
        ctx.push_help_overlay();
        return;
    }

    // Search mode dispatch
    if ctx.ui.snippet_search().is_some() {
        handle_snippet_picker_search(ctx, key, &target_aliases, events_tx);
        return;
    }

    // Pending snippet-delete confirmation is resolved in `handle_picker_key`
    // (shared with the Snippets tab) before this dispatch runs.

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            ctx.ui.close_snippet_search();
            ctx.snippets.cancel_delete();
            ctx.set_screen(Screen::HostList);
        }
        KeyCode::Char('/') => {
            ctx.ui.open_snippet_search();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            ctx.select_next_snippet();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            ctx.select_prev_snippet();
        }
        KeyCode::PageDown => {
            let len = ctx.snippets.store().snippets.len();
            crate::app::page_down(ctx.ui.snippet_picker_state_mut(), len, 10);
        }
        KeyCode::PageUp => {
            let len = ctx.snippets.store().snippets.len();
            crate::app::page_up(ctx.ui.snippet_picker_state_mut(), len, 10);
        }
        KeyCode::Char('a') => {
            ctx.open_snippet_add_form(target_aliases.clone());
        }
        KeyCode::Char('e') => {
            if let Some(sel) = ctx.ui.snippet_picker_state().selected()
                && let Some(snippet) = ctx.snippets.store().snippets.get(sel).cloned()
            {
                ctx.open_snippet_edit_form(&snippet, target_aliases.clone(), sel);
            }
        }
        KeyCode::Char('d') => {
            if let Some(sel) = ctx.ui.snippet_picker_state().selected()
                && sel < ctx.snippets.store().snippets.len()
            {
                ctx.snippets.request_delete(sel);
            }
        }
        KeyCode::Enter => {
            if let Some(sel) = ctx.ui.snippet_picker_state().selected()
                && let Some(snippet) = ctx.snippets.store().snippets.get(sel).cloned()
            {
                run_or_prompt_params(ctx, snippet, target_aliases, false, events_tx);
            }
        }
        KeyCode::Char('!') => {
            if let Some(sel) = ctx.ui.snippet_picker_state().selected()
                && let Some(snippet) = ctx.snippets.store().snippets.get(sel).cloned()
            {
                run_or_prompt_params(ctx, snippet, target_aliases, true, events_tx);
            }
        }
        _ => {}
    }
}

/// Run a snippet (captured output) or open param form if it has parameters.
fn run_or_prompt_params(
    ctx: &mut SnippetCtx,
    snippet: crate::snippet::Snippet,
    target_aliases: Vec<String>,
    terminal_mode: bool,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    if ctx.demo_mode {
        ctx.notify_warning(crate::i18n::text(crate::messages::DEMO_EXECUTION_DISABLED));
        return;
    }
    let params = crate::snippet::parse_params(&snippet.command);
    if !params.is_empty() {
        ctx.snippets
            .set_param_form(Some(crate::app::SnippetParamFormState::new(&params)));
        ctx.snippets.set_pending_terminal(terminal_mode);
        ctx.snippets.set_flow_targets(target_aliases);
        ctx.snippets.set_param_snippet(Some(snippet));
        ctx.set_screen(Screen::SnippetParamForm);
    } else if terminal_mode {
        ctx.snippets.set_pending(Some((snippet, target_aliases)));
        ctx.hosts.clear_multi_select();
        ctx.set_screen(Screen::HostList);
    } else {
        ctx.hosts.clear_multi_select();
        start_snippet_output(ctx, &snippet, &target_aliases, events_tx);
    }
}

/// Monotonically increasing run ID to distinguish snippet execution runs.
static SNIPPET_RUN_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Start in-TUI snippet execution.
fn start_snippet_output(
    ctx: &mut SnippetCtx,
    snippet: &crate::snippet::Snippet,
    target_aliases: &[String],
    events_tx: &mpsc::Sender<AppEvent>,
) {
    let cancel = Arc::new(AtomicBool::new(false));

    let askpass_map: Vec<(String, Option<String>)> = target_aliases
        .iter()
        .map(|alias| {
            let askpass = ctx
                .hosts
                .list()
                .iter()
                .find(|h| h.alias == *alias)
                .and_then(|h| h.askpass.clone())
                .or_else(|| preferences::load_askpass_default(ctx.env.paths()));
            (alias.clone(), askpass)
        })
        .collect();

    let tunnel_aliases: std::collections::HashSet<String> =
        ctx.tunnels.active().keys().cloned().collect();

    let run_id = SNIPPET_RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
    debug!(
        "[purple] snippet run started: run_id={} name={:?} hosts={} interactive={}",
        run_id,
        snippet.name,
        target_aliases.len(),
        snippet.interactive
    );

    ctx.snippets
        .set_output(Some(crate::app::SnippetOutputState {
            run_id,
            results: Vec::new(),
            scroll_offset: 0,
            completed: 0,
            total: target_aliases.len(),
            all_done: false,
            cancel: cancel.clone(),
            interactive: snippet.interactive,
        }));

    ctx.snippets.set_flow_targets(target_aliases.to_vec());
    ctx.snippets
        .set_output_snippet_name(Some(snippet.name.clone()));
    ctx.set_screen(Screen::SnippetOutput);

    crate::snippet::spawn_snippet_execution(
        run_id,
        askpass_map,
        ctx.config_path.to_path_buf(),
        std::sync::Arc::new(ctx.env.clone()),
        snippet.remote_command(),
        ctx.bw_session.map(str::to_string),
        tunnel_aliases,
        cancel,
        events_tx.clone(),
        target_aliases.len() > 1,
        ctx.security_key.clone(),
    );
}

/// Compute the line count for a snippet host result, matching the UI renderer.
fn snippet_result_lines(r: &crate::app::SnippetHostOutput, interactive: bool) -> usize {
    let hint = usize::from(r.shows_not_found_hint(interactive));
    let content = if r.stdout.is_empty() && r.stderr.is_empty() {
        1 // "[No output]" placeholder
    } else {
        let stdout_lines = if r.stdout.is_empty() {
            0
        } else {
            r.stdout.lines().count()
        };
        let stderr_lines = if r.stderr.is_empty() {
            0
        } else {
            r.stderr.lines().count()
        };
        stdout_lines + stderr_lines
    };
    // header + content + optional hint + blank line
    1 + content + hint + 1
}

pub(super) fn handle_output_key(app: &mut App, key: KeyEvent) {
    let mut ctx = ctx_from_app(app);
    output_key(&mut ctx, key);
}

fn output_key(ctx: &mut SnippetCtx, key: KeyEvent) {
    let total_lines = ctx
        .snippets
        .output()
        .map(|s| {
            s.results
                .iter()
                .map(|r| snippet_result_lines(r, s.interactive))
                .sum::<usize>()
        })
        .unwrap_or(0);

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            if let Some(state) = ctx.snippets.output()
                && !state.all_done
            {
                state.cancel.store(true, Ordering::Relaxed);
            }
            ctx.snippets.set_output(None);
            // Free the flow context slots so the next snippet flow opens
            // against a clean state, instead of inheriting the prior
            // targets and snippet name.
            ctx.snippets.clear_flow_targets();
            ctx.snippets.set_output_snippet_name(None);
            ctx.snippets.set_param_snippet(None);
            ctx.set_screen(Screen::HostList);
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if let Some(state) = ctx.snippets.output_mut() {
                state.scroll_offset = state.scroll_offset.saturating_add(1);
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Some(state) = ctx.snippets.output_mut() {
                state.scroll_offset = state.scroll_offset.saturating_sub(1);
            }
        }
        KeyCode::PageDown => {
            if let Some(state) = ctx.snippets.output_mut() {
                state.scroll_offset = state.scroll_offset.saturating_add(20);
            }
        }
        KeyCode::PageUp => {
            if let Some(state) = ctx.snippets.output_mut() {
                state.scroll_offset = state.scroll_offset.saturating_sub(20);
            }
        }
        KeyCode::Char('G') => {
            if let Some(state) = ctx.snippets.output_mut() {
                state.scroll_offset = total_lines.saturating_sub(1);
            }
        }
        KeyCode::Char('g') => {
            if let Some(state) = ctx.snippets.output_mut() {
                state.scroll_offset = 0;
            }
        }
        KeyCode::Char('n') => {
            // Jump to next host header
            if let Some(state) = ctx.snippets.output_mut() {
                let current = state.scroll_offset;
                let mut line = 0;
                for result in &state.results {
                    let section = snippet_result_lines(result, state.interactive);
                    if line > current {
                        state.scroll_offset = line;
                        return;
                    }
                    line += section;
                }
            }
        }
        KeyCode::Char('N') => {
            // Jump to previous host header
            if let Some(state) = ctx.snippets.output_mut() {
                let current = state.scroll_offset;
                let mut offsets = Vec::new();
                let mut line = 0;
                for result in &state.results {
                    offsets.push(line);
                    line += snippet_result_lines(result, state.interactive);
                }
                for &off in offsets.iter().rev() {
                    if off < current {
                        state.scroll_offset = off;
                        return;
                    }
                }
                state.scroll_offset = 0;
            }
        }
        KeyCode::Char('c') => {
            // Copy all output
            if let Some(state) = ctx.snippets.output() {
                let mut text = String::new();
                for result in &state.results {
                    text.push_str(&format!("-- {} --\n", result.alias));
                    if !result.stdout.is_empty() {
                        text.push_str(&result.stdout);
                        text.push('\n');
                    }
                    if !result.stderr.is_empty() {
                        text.push_str(&result.stderr);
                        text.push('\n');
                    }
                    text.push('\n');
                }
                match clipboard::copy_to_clipboard(&text) {
                    Ok(()) => ctx.notify(crate::i18n::text(crate::messages::OUTPUT_COPIED)),
                    Err(e) => ctx.notify_error(crate::messages::copy_failed(&e)),
                }
            }
        }
        KeyCode::Char('?') => {
            ctx.push_help_overlay();
        }
        _ => {}
    }
}

/// Reset snippet picker selection to first match after search query changes.
fn reset_snippet_search_selection(ctx: &mut SnippetCtx) {
    let filtered = ctx.filtered_snippet_indices();
    if filtered.is_empty() {
        ctx.ui.snippet_picker_state_mut().select(None);
    } else {
        ctx.ui.snippet_picker_state_mut().select(Some(0));
    }
}

fn handle_snippet_picker_search(
    ctx: &mut SnippetCtx,
    key: KeyEvent,
    target_aliases: &[String],
    events_tx: &mpsc::Sender<AppEvent>,
) {
    match key.code {
        KeyCode::Esc => {
            ctx.ui.close_snippet_search();
            // Restore selection to full list
            if !ctx.snippets.store().snippets.is_empty()
                && ctx.ui.snippet_picker_state().selected().is_none()
            {
                ctx.ui.snippet_picker_state_mut().select(Some(0));
            }
        }
        KeyCode::Enter => {
            let filtered = ctx.filtered_snippet_indices();
            if let Some(sel) = ctx.ui.snippet_picker_state().selected()
                && sel < filtered.len()
            {
                let real_idx = filtered[sel];
                if let Some(snippet) = ctx.snippets.store().snippets.get(real_idx).cloned() {
                    ctx.ui.close_snippet_search();
                    run_or_prompt_params(ctx, snippet, target_aliases.to_vec(), false, events_tx);
                }
            }
        }
        KeyCode::Char(c) => {
            if let Some(query) = ctx.ui.snippet_search_mut() {
                query.push(c);
            }
            reset_snippet_search_selection(ctx);
        }
        KeyCode::Backspace => {
            if let Some(query) = ctx.ui.snippet_search_mut() {
                query.pop();
                if query.is_empty() {
                    ctx.ui.close_snippet_search();
                    if !ctx.snippets.store().snippets.is_empty() {
                        ctx.ui.snippet_picker_state_mut().select(Some(0));
                    }
                    return;
                }
            }
            reset_snippet_search_selection(ctx);
        }
        KeyCode::Down => {
            let count = ctx.filtered_snippet_indices().len();
            if let Some(sel) = ctx.ui.snippet_picker_state().selected()
                && sel + 1 < count
            {
                ctx.ui.snippet_picker_state_mut().select(Some(sel + 1));
            }
        }
        KeyCode::Up => {
            if let Some(sel) = ctx.ui.snippet_picker_state().selected()
                && sel > 0
            {
                ctx.ui.snippet_picker_state_mut().select(Some(sel - 1));
            }
        }
        _ => {}
    }
}

pub(super) fn handle_param_form_key(
    app: &mut App,
    key: KeyEvent,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    let mut ctx = ctx_from_app(app);
    param_form_key(&mut ctx, key, events_tx);
}

fn param_form_key(ctx: &mut SnippetCtx, key: KeyEvent, events_tx: &mpsc::Sender<AppEvent>) {
    if !matches!(*ctx.screen, Screen::SnippetParamForm) {
        return;
    }
    let snippet = match ctx.snippets.param_snippet() {
        Some(s) => s.clone(),
        None => return,
    };
    let target_aliases: Vec<String> = ctx.snippets.flow_targets().to_vec();

    let form = match ctx.snippets.param_form_mut() {
        Some(f) => f,
        None => return,
    };

    // Handle discard confirmation dialog via the shared confirm router.
    if ctx.forms.is_discard_pending() {
        match super::route_confirm_key(key) {
            super::ConfirmAction::Yes => {
                ctx.forms.dismiss_discard_confirm();
                ctx.snippets.close_param_form();
                ctx.snippets.set_param_snippet(None);
                ctx.snippets.set_flow_snippet(None);
                let screen = if ctx.snippets.form_return_to_tab() {
                    Screen::HostList
                } else {
                    Screen::SnippetPicker
                };
                ctx.set_screen(screen);
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
            if form.is_dirty() {
                ctx.forms.request_discard_confirm();
            } else {
                ctx.snippets.close_param_form();
                ctx.snippets.set_param_snippet(None);
                ctx.snippets.set_flow_snippet(None);
                let screen = if ctx.snippets.form_return_to_tab() {
                    Screen::HostList
                } else {
                    Screen::SnippetPicker
                };
                ctx.set_screen(screen);
            }
        }
        KeyCode::Tab | KeyCode::Down if form.focused_index + 1 < form.params.len() => {
            form.focused_index += 1;
            form.cursor_pos = form.values[form.focused_index].chars().count();
            let vis = form.visible_count.max(1);
            if form.focused_index >= form.scroll_offset + vis {
                form.scroll_offset = form.focused_index.saturating_sub(vis - 1);
            }
        }
        KeyCode::BackTab | KeyCode::Up if form.focused_index > 0 => {
            form.focused_index -= 1;
            form.cursor_pos = form.values[form.focused_index].chars().count();
            if form.focused_index < form.scroll_offset {
                form.scroll_offset = form.focused_index;
            }
        }
        KeyCode::Left if form.cursor_pos > 0 => {
            form.cursor_pos -= 1;
        }
        KeyCode::Right => {
            let len = form
                .values
                .get(form.focused_index)
                .map_or(0, |v| v.chars().count());
            if form.cursor_pos < len {
                form.cursor_pos += 1;
            }
        }
        KeyCode::Enter => {
            let values_map = form.values_map();
            let mut resolved = snippet.clone();
            resolved.command = crate::snippet::substitute_params(&snippet.command, &values_map);

            let terminal_mode = ctx.snippets.pending_terminal();
            ctx.snippets.close_param_form();

            if terminal_mode {
                ctx.snippets.set_pending(Some((resolved, target_aliases)));
                ctx.hosts.clear_multi_select();
                ctx.set_screen(Screen::HostList);
            } else {
                ctx.hosts.clear_multi_select();
                start_snippet_output(ctx, &resolved, &target_aliases, events_tx);
            }
        }
        KeyCode::Char(c) => {
            if c.is_control() {
                return;
            }
            form.insert_char(c);
        }
        KeyCode::Backspace => {
            form.delete_char_before_cursor();
        }
        _ => {}
    }
}

pub(super) fn handle_form_key(app: &mut App, key: KeyEvent) {
    let mut ctx = ctx_from_app(app);
    form_key(&mut ctx, key);
}

fn form_key(ctx: &mut SnippetCtx, key: KeyEvent) {
    if !matches!(*ctx.screen, Screen::SnippetForm) {
        return;
    }
    let target_aliases: Vec<String> = ctx.snippets.flow_targets().to_vec();
    let editing: Option<usize> = ctx.snippets.form_editing();

    // Handle discard confirmation dialog via the shared confirm router.
    if ctx.forms.is_discard_pending() {
        match super::route_confirm_key(key) {
            super::ConfirmAction::Yes => {
                ctx.forms.dismiss_discard_confirm();
                ctx.close_snippet_form(target_aliases.clone());
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
            if ctx.snippets.form_is_dirty() {
                ctx.forms.request_discard_confirm();
            } else {
                ctx.close_snippet_form(target_aliases.clone());
            }
        }
        KeyCode::Tab | KeyCode::Down => {
            ctx.snippets.form_mut().focus_next();
        }
        KeyCode::BackTab | KeyCode::Up => {
            ctx.snippets.form_mut().focus_prev();
        }
        KeyCode::Left if ctx.snippets.form_mut().cursor_pos > 0 => {
            ctx.snippets.form_mut().cursor_pos -= 1;
        }
        KeyCode::Right => {
            let len = ctx.snippets.form_mut().focused_value().chars().count();
            if ctx.snippets.form_mut().cursor_pos < len {
                ctx.snippets.form_mut().cursor_pos += 1;
            }
        }
        KeyCode::Home => {
            ctx.snippets.form_mut().cursor_pos = 0;
        }
        KeyCode::End => {
            ctx.snippets.form_mut().sync_cursor_to_end();
        }
        KeyCode::Enter => {
            submit_snippet_form(ctx, &target_aliases, editing);
        }
        // SPACE GUARD MUST PRECEDE the generic Char(c) arm: Space on the
        // toggle field flips it and Space on a picker field (Default hosts)
        // opens the host picker instead of inserting a literal space.
        KeyCode::Char(' ') if ctx.snippets.form().focused_field.is_toggle() => {
            let form = ctx.snippets.form_mut();
            form.interactive = !form.interactive;
        }
        KeyCode::Char(' ') if ctx.snippets.form().focused_field.is_picker() => {
            ctx.open_default_hosts_picker();
        }
        KeyCode::Char(c) => {
            ctx.snippets.form_mut().insert_char(c);
        }
        KeyCode::Backspace => {
            ctx.snippets.form_mut().delete_char_before_cursor();
        }
        _ => {}
    }
}

fn submit_snippet_form(ctx: &mut SnippetCtx, target_aliases: &[String], editing: Option<usize>) {
    if let Err(msg) = ctx.snippets.form_mut().validate() {
        ctx.notify_error(msg);
        return;
    }

    let new_name = ctx.snippets.form_mut().name.trim().to_string();
    let new_command = ctx.snippets.form_mut().command.trim().to_string();
    let new_description = ctx.snippets.form_mut().description.trim().to_string();
    let new_interactive = ctx.snippets.form().interactive;

    // Check for duplicate name (skip the snippet being edited)
    let old_name = editing.and_then(|idx| {
        ctx.snippets
            .store()
            .snippets
            .get(idx)
            .map(|s| s.name.clone())
    });
    let name_taken = ctx
        .snippets
        .store()
        .snippets
        .iter()
        .any(|s| s.name == new_name && Some(&s.name) != old_name.as_ref());
    if name_taken {
        ctx.notify_warning(crate::messages::snippet_exists(&new_name));
        return;
    }

    let snippet = crate::snippet::Snippet {
        name: new_name,
        command: new_command,
        description: new_description,
        interactive: new_interactive,
    };

    // The form owns the default hosts (seeded from the saved targets on open, so
    // they carry across a rename unless the user changed them). Persist them
    // under the new name below.
    let new_default_hosts = ctx.snippets.form().default_hosts.clone();

    // Snapshot both snippets AND targets for rollback: a rename mutates targets
    // (remove drops the old name's entry), so a failed save must restore both.
    let snapshot_snippets = ctx.snippets.store().snippets.clone();
    let snapshot_targets = ctx.snippets.store().targets.clone();

    // On a rename, drop the old name's entry; run history is migrated after a
    // successful save.
    let rename_from = old_name.as_ref().filter(|o| **o != snippet.name).cloned();
    if let Some(old) = &rename_from {
        ctx.snippets.store_mut().remove(old);
    }

    let new_name = snippet.name.clone();
    let is_new = editing.is_none();
    ctx.snippets.store_mut().set(snippet);
    ctx.snippets
        .store_mut()
        .set_targets(&new_name, new_default_hosts);

    if let Err(e) = ctx.snippets.store_mut().save() {
        warn!("[config] snippet store save failed, rolling back: {e}");
        ctx.snippets.store_mut().snippets = snapshot_snippets;
        ctx.snippets.store_mut().targets = snapshot_targets;
        ctx.notify_error(crate::messages::failed_to_save(&e));
        return;
    }
    // Save succeeded: migrate the run ledger so the renamed snippet keeps its
    // TRACK RECORD verdict and trend chart, then persist the move.
    if let Some(old) = &rename_from {
        ctx.snippets.runs_mut().rename(old, &new_name);
        if let Err(e) = ctx.snippets.runs().flush(ctx.env.paths()) {
            warn!("[config] snippet_runs flush after rename failed: {e}");
        }
    }

    // Re-select in picker
    let name = ctx.snippets.form_mut().name.trim().to_string();
    let new_idx = ctx
        .snippets
        .store()
        .snippets
        .iter()
        .position(|s| s.name == name);
    ctx.ui.snippet_picker_state_mut().select(new_idx);

    debug!(
        "[purple] snippet saved: {name} (new={is_new}, interactive={new_interactive}, default_hosts={})",
        ctx.snippets.store().targets_for(&name).len()
    );
    if is_new {
        ctx.notify(crate::messages::snippet_added(&name));
    } else {
        ctx.notify(crate::messages::snippet_updated(&name));
    }
    ctx.close_snippet_form(target_aliases.to_vec());
}

#[cfg(test)]
mod param_form_tests {
    use super::*;
    use crate::app::SnippetParamFormState;
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
        let snippet = crate::snippet::Snippet {
            name: "test".to_string(),
            command: "echo hi".to_string(),
            description: String::new(),
            interactive: false,
        };
        app.snippets.set_param_snippet(Some(snippet));
        app.snippets.set_flow_targets(vec!["h1".to_string()]);
        app.screen = Screen::SnippetParamForm;
        app.snippets
            .set_param_form(Some(SnippetParamFormState::new(&[])));
        app
    }

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn esc_on_clean_form_returns_to_snippet_picker() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app();
        let (tx, _rx) = mpsc::channel();
        handle_param_form_key(&mut app, k(KeyCode::Esc), &tx);
        assert!(matches!(app.screen, Screen::SnippetPicker));
        assert!(app.snippets.param_form().is_none());
    }

    #[test]
    fn esc_on_clean_form_returns_to_host_list_when_tab_origin() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app();
        // Opened from the Snippets tab: Esc returns there (HostList), not the
        // host-list snippet picker.
        app.snippets.set_form_return_to_tab(true);
        let (tx, _rx) = mpsc::channel();
        handle_param_form_key(&mut app, k(KeyCode::Esc), &tx);
        assert!(matches!(app.screen, Screen::HostList));
        assert!(app.snippets.param_form().is_none());
    }

    #[test]
    fn typing_a_char_inserts_into_focused_param() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app();
        let params = vec![crate::snippet::SnippetParam {
            name: "name".to_string(),
            default: None,
        }];
        app.snippets
            .set_param_form(Some(SnippetParamFormState::new(&params)));
        if let Some(mut s) = app.snippets.param_snippet().cloned() {
            s.command = "echo {{name}}".to_string();
            app.snippets.set_param_snippet(Some(s));
        }
        let (tx, _rx) = mpsc::channel();
        handle_param_form_key(&mut app, k(KeyCode::Char('h')), &tx);
        handle_param_form_key(&mut app, k(KeyCode::Char('i')), &tx);
        let state = app.snippets.param_form().expect("form state");
        assert_eq!(state.values[0], "hi");
    }

    fn make_dirty_app() -> App {
        let mut app = make_app();
        let params = vec![crate::snippet::SnippetParam {
            name: "name".to_string(),
            default: None,
        }];
        app.snippets
            .set_param_form(Some(SnippetParamFormState::new(&params)));
        if let Some(mut s) = app.snippets.param_snippet().cloned() {
            s.command = "echo {{name}}".to_string();
            app.snippets.set_param_snippet(Some(s));
        }
        app.snippets
            .param_form_mut()
            .expect("form state")
            .insert_char('x');
        // Pre-set pending_terminal so the y-branch reset is observable;
        // without this the assertion in discard_confirm_y_... would pass
        // vacuously against the default false value.
        app.snippets.set_pending_terminal(true);
        app
    }

    #[test]
    fn dirty_esc_arms_discard_confirmation() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_dirty_app();
        let (tx, _rx) = mpsc::channel();
        handle_param_form_key(&mut app, k(KeyCode::Esc), &tx);
        assert!(app.forms.is_discard_pending());
        assert!(matches!(app.screen, Screen::SnippetParamForm));
    }

    #[test]
    fn discard_confirm_y_closes_form_and_returns_to_picker() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_dirty_app();
        let (tx, _rx) = mpsc::channel();
        handle_param_form_key(&mut app, k(KeyCode::Esc), &tx);
        handle_param_form_key(&mut app, k(KeyCode::Char('y')), &tx);
        assert!(!app.forms.is_discard_pending());
        assert!(app.snippets.param_form().is_none());
        assert!(!app.snippets.pending_terminal());
        assert!(matches!(app.screen, Screen::SnippetPicker));
    }

    #[test]
    fn discard_confirm_n_clears_pending_and_keeps_form() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_dirty_app();
        let (tx, _rx) = mpsc::channel();
        handle_param_form_key(&mut app, k(KeyCode::Esc), &tx);
        handle_param_form_key(&mut app, k(KeyCode::Char('n')), &tx);
        assert!(!app.forms.is_discard_pending());
        assert!(app.snippets.param_form().is_some());
        assert!(matches!(app.screen, Screen::SnippetParamForm));
    }

    // Pins the route_confirm_key Ignored contract: a stray key must NOT
    // silently cancel the discard prompt (no false positive on Yes, no
    // false negative on No that would dismiss the discard confirm).
    #[test]
    fn discard_confirm_unrelated_key_keeps_pending() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_dirty_app();
        let (tx, _rx) = mpsc::channel();
        handle_param_form_key(&mut app, k(KeyCode::Esc), &tx);
        handle_param_form_key(&mut app, k(KeyCode::Char('x')), &tx);
        assert!(app.forms.is_discard_pending());
        assert!(app.snippets.param_form().is_some());
    }

    // Defensive: production never opens a param form with zero params (the run
    // path guards on parse_params non-empty), but Right/Char must not panic if
    // it ever does.
    #[test]
    fn empty_param_form_right_and_char_do_not_panic() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app(); // param_form is SnippetParamFormState::new(&[])
        let (tx, _rx) = mpsc::channel();
        handle_param_form_key(&mut app, k(KeyCode::Right), &tx);
        handle_param_form_key(&mut app, k(KeyCode::Char('x')), &tx);
        assert!(matches!(app.screen, Screen::SnippetParamForm));
        assert!(app.snippets.param_form().expect("form").values.is_empty());
    }
}

#[cfg(test)]
mod output_tests {
    use super::*;
    use crate::app::{SnippetHostOutput, SnippetOutputState};
    use crate::ssh_config::model::SshConfigFile;
    use crossterm::event::KeyModifiers;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    fn make_app_on_output(line_count: usize) -> App {
        let scratch = tempfile::tempdir().expect("tempdir").keep();
        let config = SshConfigFile {
            elements: SshConfigFile::parse_content(""),
            path: scratch.join("test_config"),
            crlf: false,
            bom: false,
        };
        let mut app = App::new(config);
        let results = (0..line_count)
            .map(|i| SnippetHostOutput {
                alias: format!("h{i}"),
                stdout: "ok".to_string(),
                stderr: String::new(),
                exit_code: Some(0),
            })
            .collect();
        app.snippets.set_output(Some(SnippetOutputState {
            run_id: 1,
            results,
            scroll_offset: 0,
            completed: line_count,
            total: line_count,
            all_done: true,
            cancel: Arc::new(AtomicBool::new(false)),
            interactive: false,
        }));
        app.snippets
            .set_output_snippet_name(Some("echo".to_string()));
        app.snippets.set_flow_targets(vec!["h0".to_string()]);
        app.screen = Screen::SnippetOutput;
        app
    }

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn j_scrolls_output_down_one_line() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app_on_output(5);
        handle_output_key(&mut app, k(KeyCode::Char('j')));
        let state = app.snippets.output().expect("output state");
        assert_eq!(state.scroll_offset, 1);
    }

    #[test]
    fn result_lines_count_the_not_found_tip() {
        let r = SnippetHostOutput {
            alias: "h".to_string(),
            stdout: String::new(),
            stderr: "pm2: command not found".to_string(),
            exit_code: Some(crate::snippet::EXIT_COMMAND_NOT_FOUND),
        };
        // header + stderr + tip + blank line
        assert_eq!(snippet_result_lines(&r, false), 4);
        // No tip once the snippet already runs interactively.
        assert_eq!(snippet_result_lines(&r, true), 3);
    }

    #[test]
    fn esc_closes_overlay_and_clears_output_state() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app_on_output(3);
        handle_output_key(&mut app, k(KeyCode::Esc));
        assert!(matches!(app.screen, Screen::HostList));
        assert!(app.snippets.output().is_none());
    }
}
