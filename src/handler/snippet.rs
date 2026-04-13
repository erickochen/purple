use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Screen};
use crate::clipboard;
use crate::event::AppEvent;
use crate::preferences;

pub(super) fn open_snippet_picker(app: &mut App, aliases: Vec<String>) {
    app.snippet_store = crate::snippet::SnippetStore::load();
    app.ui.snippet_picker_state = ratatui::widgets::ListState::default();
    if !app.snippet_store.snippets.is_empty() {
        app.ui.snippet_picker_state.select(Some(0));
    }
    app.screen = Screen::SnippetPicker {
        target_aliases: aliases,
    };
}

pub(super) fn handle_snippet_picker(
    app: &mut App,
    key: KeyEvent,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    let target_aliases = match &app.screen {
        Screen::SnippetPicker { target_aliases } => target_aliases.clone(),
        _ => return,
    };

    // Allow ? to open help even during search
    if key.code == KeyCode::Char('?') {
        let old = std::mem::replace(&mut app.screen, Screen::HostList);
        app.screen = Screen::Help {
            return_screen: Box::new(old),
        };
        return;
    }

    // Search mode dispatch
    if app.ui.snippet_search.is_some() {
        handle_snippet_picker_search(app, key, &target_aliases, events_tx);
        return;
    }

    // Handle pending snippet delete confirmation
    if app.pending_snippet_delete.is_some() && key.code != KeyCode::Char('?') {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let Some(sel) = app.pending_snippet_delete.take() else {
                    return;
                };
                if sel < app.snippet_store.snippets.len() {
                    let removed = app.snippet_store.snippets.remove(sel);
                    if let Err(e) = app.snippet_store.save() {
                        app.snippet_store.snippets.insert(sel, removed);
                        app.set_status(format!("Failed to save: {}", e), true);
                    } else {
                        if app.snippet_store.snippets.is_empty() {
                            app.ui.snippet_picker_state.select(None);
                        } else if sel >= app.snippet_store.snippets.len() {
                            app.ui
                                .snippet_picker_state
                                .select(Some(app.snippet_store.snippets.len() - 1));
                        }
                        app.set_status(format!("Removed snippet '{}'.", removed.name), false);
                    }
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.pending_snippet_delete = None;
            }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.ui.snippet_search = None;
            app.pending_snippet_delete = None;
            app.screen = Screen::HostList;
        }
        KeyCode::Char('/') => {
            app.ui.snippet_search = Some(String::new());
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.select_next_snippet();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select_prev_snippet();
        }
        KeyCode::PageDown => {
            crate::app::page_down(
                &mut app.ui.snippet_picker_state,
                app.snippet_store.snippets.len(),
                10,
            );
        }
        KeyCode::PageUp => {
            crate::app::page_up(
                &mut app.ui.snippet_picker_state,
                app.snippet_store.snippets.len(),
                10,
            );
        }
        KeyCode::Char('a') => {
            app.snippet_form = crate::app::SnippetForm::new();
            app.screen = Screen::SnippetForm {
                target_aliases: target_aliases.clone(),
                editing: None,
            };
            app.capture_snippet_form_baseline();
        }
        KeyCode::Char('e') => {
            if let Some(sel) = app.ui.snippet_picker_state.selected() {
                if let Some(snippet) = app.snippet_store.snippets.get(sel) {
                    app.snippet_form = crate::app::SnippetForm::from_snippet(snippet);
                    app.screen = Screen::SnippetForm {
                        target_aliases: target_aliases.clone(),
                        editing: Some(sel),
                    };
                    app.capture_snippet_form_baseline();
                }
            }
        }
        KeyCode::Char('d') => {
            if let Some(sel) = app.ui.snippet_picker_state.selected() {
                if sel < app.snippet_store.snippets.len() {
                    app.pending_snippet_delete = Some(sel);
                }
            }
        }
        KeyCode::Enter => {
            if let Some(sel) = app.ui.snippet_picker_state.selected() {
                if let Some(snippet) = app.snippet_store.snippets.get(sel).cloned() {
                    run_or_prompt_params(app, snippet, target_aliases, false, events_tx);
                }
            }
        }
        KeyCode::Char('!') => {
            if let Some(sel) = app.ui.snippet_picker_state.selected() {
                if let Some(snippet) = app.snippet_store.snippets.get(sel).cloned() {
                    run_or_prompt_params(app, snippet, target_aliases, true, events_tx);
                }
            }
        }
        _ => {}
    }
}

/// Run a snippet (captured output) or open param form if it has parameters.
fn run_or_prompt_params(
    app: &mut App,
    snippet: crate::snippet::Snippet,
    target_aliases: Vec<String>,
    terminal_mode: bool,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    if app.demo_mode {
        app.set_status("Demo mode. Execution disabled.".to_string(), false);
        return;
    }
    let params = crate::snippet::parse_params(&snippet.command);
    if !params.is_empty() {
        app.snippet_param_form = Some(crate::app::SnippetParamFormState::new(&params));
        app.pending_snippet_terminal = terminal_mode;
        app.screen = Screen::SnippetParamForm {
            snippet,
            target_aliases,
        };
    } else if terminal_mode {
        app.pending_snippet = Some((snippet, target_aliases));
        app.multi_select.clear();
        app.screen = Screen::HostList;
    } else {
        app.multi_select.clear();
        start_snippet_output(app, &snippet, &target_aliases, events_tx);
    }
}

/// Monotonically increasing run ID to distinguish snippet execution runs.
static SNIPPET_RUN_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Start in-TUI snippet execution.
fn start_snippet_output(
    app: &mut App,
    snippet: &crate::snippet::Snippet,
    target_aliases: &[String],
    events_tx: &mpsc::Sender<AppEvent>,
) {
    let cancel = Arc::new(AtomicBool::new(false));

    let askpass_map: Vec<(String, Option<String>)> = target_aliases
        .iter()
        .map(|alias| {
            let askpass = app
                .hosts
                .iter()
                .find(|h| h.alias == *alias)
                .and_then(|h| h.askpass.clone())
                .or_else(preferences::load_askpass_default);
            (alias.clone(), askpass)
        })
        .collect();

    let tunnel_aliases: std::collections::HashSet<String> =
        app.active_tunnels.keys().cloned().collect();

    let run_id = SNIPPET_RUN_COUNTER.fetch_add(1, Ordering::Relaxed);

    app.snippet_output = Some(crate::app::SnippetOutputState {
        run_id,
        results: Vec::new(),
        scroll_offset: 0,
        completed: 0,
        total: target_aliases.len(),
        all_done: false,
        cancel: cancel.clone(),
    });

    app.screen = Screen::SnippetOutput {
        snippet_name: snippet.name.clone(),
        target_aliases: target_aliases.to_vec(),
    };

    let stx = super::snippet_event_bridge(events_tx);
    crate::snippet::spawn_snippet_execution(
        run_id,
        askpass_map,
        app.reload.config_path.clone(),
        snippet.command.clone(),
        app.bw_session.clone(),
        tunnel_aliases,
        cancel,
        stx,
        target_aliases.len() > 1,
    );
}

/// Compute the line count for a snippet host result, matching the UI renderer.
fn snippet_result_lines(r: &crate::app::SnippetHostOutput) -> usize {
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
    // header + content + blank line
    1 + content + 1
}

pub(super) fn handle_snippet_output(app: &mut App, key: KeyEvent) {
    let total_lines = app
        .snippet_output
        .as_ref()
        .map(|s| s.results.iter().map(snippet_result_lines).sum::<usize>())
        .unwrap_or(0);

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            if let Some(ref state) = app.snippet_output {
                if !state.all_done {
                    state.cancel.store(true, Ordering::Relaxed);
                }
            }
            app.snippet_output = None;
            app.screen = Screen::HostList;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if let Some(ref mut state) = app.snippet_output {
                state.scroll_offset = state.scroll_offset.saturating_add(1);
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Some(ref mut state) = app.snippet_output {
                state.scroll_offset = state.scroll_offset.saturating_sub(1);
            }
        }
        KeyCode::PageDown => {
            if let Some(ref mut state) = app.snippet_output {
                state.scroll_offset = state.scroll_offset.saturating_add(20);
            }
        }
        KeyCode::PageUp => {
            if let Some(ref mut state) = app.snippet_output {
                state.scroll_offset = state.scroll_offset.saturating_sub(20);
            }
        }
        KeyCode::Char('G') => {
            if let Some(ref mut state) = app.snippet_output {
                state.scroll_offset = total_lines.saturating_sub(1);
            }
        }
        KeyCode::Char('g') => {
            if let Some(ref mut state) = app.snippet_output {
                state.scroll_offset = 0;
            }
        }
        KeyCode::Char('n') => {
            // Jump to next host header
            if let Some(ref mut state) = app.snippet_output {
                let current = state.scroll_offset;
                let mut line = 0;
                for result in &state.results {
                    let section = snippet_result_lines(result);
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
            if let Some(ref mut state) = app.snippet_output {
                let current = state.scroll_offset;
                let mut offsets = Vec::new();
                let mut line = 0;
                for result in &state.results {
                    offsets.push(line);
                    line += snippet_result_lines(result);
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
            if let Some(ref state) = app.snippet_output {
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
                    Ok(()) => app.set_status("Output copied.", false),
                    Err(e) => app.set_status(format!("Copy failed: {}", e), true),
                }
            }
        }
        KeyCode::Char('?') => {
            let old = std::mem::replace(&mut app.screen, Screen::HostList);
            app.screen = Screen::Help {
                return_screen: Box::new(old),
            };
        }
        _ => {}
    }
}

/// Reset snippet picker selection to first match after search query changes.
fn reset_snippet_search_selection(app: &mut App) {
    let filtered = app.filtered_snippet_indices();
    if filtered.is_empty() {
        app.ui.snippet_picker_state.select(None);
    } else {
        app.ui.snippet_picker_state.select(Some(0));
    }
}

pub(super) fn handle_snippet_picker_search(
    app: &mut App,
    key: KeyEvent,
    target_aliases: &[String],
    events_tx: &mpsc::Sender<AppEvent>,
) {
    match key.code {
        KeyCode::Esc => {
            app.ui.snippet_search = None;
            // Restore selection to full list
            if !app.snippet_store.snippets.is_empty()
                && app.ui.snippet_picker_state.selected().is_none()
            {
                app.ui.snippet_picker_state.select(Some(0));
            }
        }
        KeyCode::Enter => {
            let filtered = app.filtered_snippet_indices();
            if let Some(sel) = app.ui.snippet_picker_state.selected() {
                if sel < filtered.len() {
                    let real_idx = filtered[sel];
                    if let Some(snippet) = app.snippet_store.snippets.get(real_idx).cloned() {
                        app.ui.snippet_search = None;
                        run_or_prompt_params(
                            app,
                            snippet,
                            target_aliases.to_vec(),
                            false,
                            events_tx,
                        );
                    }
                }
            }
        }
        KeyCode::Char(c) => {
            if let Some(ref mut query) = app.ui.snippet_search {
                query.push(c);
            }
            reset_snippet_search_selection(app);
        }
        KeyCode::Backspace => {
            if let Some(ref mut query) = app.ui.snippet_search {
                query.pop();
                if query.is_empty() {
                    app.ui.snippet_search = None;
                    if !app.snippet_store.snippets.is_empty() {
                        app.ui.snippet_picker_state.select(Some(0));
                    }
                    return;
                }
            }
            reset_snippet_search_selection(app);
        }
        KeyCode::Down => {
            let count = app.filtered_snippet_indices().len();
            if let Some(sel) = app.ui.snippet_picker_state.selected() {
                if sel + 1 < count {
                    app.ui.snippet_picker_state.select(Some(sel + 1));
                }
            }
        }
        KeyCode::Up => {
            if let Some(sel) = app.ui.snippet_picker_state.selected() {
                if sel > 0 {
                    app.ui.snippet_picker_state.select(Some(sel - 1));
                }
            }
        }
        _ => {}
    }
}

pub(super) fn handle_snippet_param_form(
    app: &mut App,
    key: KeyEvent,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    let (snippet, target_aliases) = match &app.screen {
        Screen::SnippetParamForm {
            snippet,
            target_aliases,
        } => (snippet.clone(), target_aliases.clone()),
        _ => return,
    };

    let form = match app.snippet_param_form.as_mut() {
        Some(f) => f,
        None => return,
    };

    // Handle discard confirmation dialog
    if app.pending_discard_confirm {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.pending_discard_confirm = false;
                app.snippet_param_form = None;
                app.pending_snippet_terminal = false;
                app.screen = Screen::SnippetPicker { target_aliases };
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.pending_discard_confirm = false;
            }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc => {
            if form.is_dirty() {
                app.pending_discard_confirm = true;
            } else {
                app.snippet_param_form = None;
                app.pending_snippet_terminal = false;
                app.screen = Screen::SnippetPicker { target_aliases };
            }
        }
        KeyCode::Tab | KeyCode::Down => {
            if form.focused_index + 1 < form.params.len() {
                form.focused_index += 1;
                form.cursor_pos = form.values[form.focused_index].chars().count();
                let vis = form.visible_count.max(1);
                if form.focused_index >= form.scroll_offset + vis {
                    form.scroll_offset = form.focused_index.saturating_sub(vis - 1);
                }
            }
        }
        KeyCode::BackTab | KeyCode::Up => {
            if form.focused_index > 0 {
                form.focused_index -= 1;
                form.cursor_pos = form.values[form.focused_index].chars().count();
                if form.focused_index < form.scroll_offset {
                    form.scroll_offset = form.focused_index;
                }
            }
        }
        KeyCode::Left => {
            if form.cursor_pos > 0 {
                form.cursor_pos -= 1;
            }
        }
        KeyCode::Right => {
            let len = form.values[form.focused_index].chars().count();
            if form.cursor_pos < len {
                form.cursor_pos += 1;
            }
        }
        KeyCode::Enter => {
            let values_map = form.values_map();
            let mut resolved = snippet.clone();
            resolved.command = crate::snippet::substitute_params(&snippet.command, &values_map);

            let terminal_mode = app.pending_snippet_terminal;
            app.snippet_param_form = None;
            app.pending_snippet_terminal = false;

            if terminal_mode {
                app.pending_snippet = Some((resolved, target_aliases));
                app.multi_select.clear();
                app.screen = Screen::HostList;
            } else {
                app.multi_select.clear();
                start_snippet_output(app, &resolved, &target_aliases, events_tx);
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

pub(super) fn handle_snippet_form(app: &mut App, key: KeyEvent) {
    let (target_aliases, editing) = match &app.screen {
        Screen::SnippetForm {
            target_aliases,
            editing,
        } => (target_aliases.clone(), *editing),
        _ => return,
    };

    // Handle discard confirmation dialog
    if app.pending_discard_confirm {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.pending_discard_confirm = false;
                app.snippet_form_baseline = None;
                app.screen = Screen::SnippetPicker {
                    target_aliases: target_aliases.clone(),
                };
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.pending_discard_confirm = false;
            }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc => {
            if app.snippet_form_is_dirty() {
                app.pending_discard_confirm = true;
            } else {
                app.snippet_form_baseline = None;
                app.screen = Screen::SnippetPicker {
                    target_aliases: target_aliases.clone(),
                };
            }
        }
        KeyCode::Tab | KeyCode::Down => {
            app.snippet_form.focused_field = app.snippet_form.focused_field.next();
            app.snippet_form.sync_cursor_to_end();
        }
        KeyCode::BackTab | KeyCode::Up => {
            app.snippet_form.focused_field = app.snippet_form.focused_field.prev();
            app.snippet_form.sync_cursor_to_end();
        }
        KeyCode::Left => {
            if app.snippet_form.cursor_pos > 0 {
                app.snippet_form.cursor_pos -= 1;
            }
        }
        KeyCode::Right => {
            let len = app.snippet_form.focused_value().chars().count();
            if app.snippet_form.cursor_pos < len {
                app.snippet_form.cursor_pos += 1;
            }
        }
        KeyCode::Home => {
            app.snippet_form.cursor_pos = 0;
        }
        KeyCode::End => {
            app.snippet_form.sync_cursor_to_end();
        }
        KeyCode::Enter => {
            submit_snippet_form(app, &target_aliases, editing);
        }
        KeyCode::Char(c) => {
            app.snippet_form.insert_char(c);
        }
        KeyCode::Backspace => {
            app.snippet_form.delete_char_before_cursor();
        }
        _ => {}
    }
}

fn submit_snippet_form(app: &mut App, target_aliases: &[String], editing: Option<usize>) {
    if let Err(msg) = app.snippet_form.validate() {
        app.set_status(msg, true);
        return;
    }

    let new_name = app.snippet_form.name.trim().to_string();
    let new_command = app.snippet_form.command.trim().to_string();
    let new_description = app.snippet_form.description.trim().to_string();

    // Check for duplicate name (skip the snippet being edited)
    let old_name =
        editing.and_then(|idx| app.snippet_store.snippets.get(idx).map(|s| s.name.clone()));
    let name_taken = app
        .snippet_store
        .snippets
        .iter()
        .any(|s| s.name == new_name && Some(&s.name) != old_name.as_ref());
    if name_taken {
        app.set_status(format!("'{}' already exists.", new_name), true);
        return;
    }

    let snippet = crate::snippet::Snippet {
        name: new_name,
        command: new_command,
        description: new_description,
    };

    // Save a snapshot for rollback
    let snapshot = app.snippet_store.snippets.clone();

    // If editing and name changed, remove the old one
    if let Some(old_name) = &old_name {
        if *old_name != snippet.name {
            app.snippet_store.remove(old_name);
        }
    }

    let is_new = editing.is_none();
    app.snippet_store.set(snippet);

    if let Err(e) = app.snippet_store.save() {
        app.snippet_store.snippets = snapshot;
        app.set_status(format!("Failed to save: {}", e), true);
        return;
    }

    // Re-select in picker
    let name = app.snippet_form.name.trim().to_string();
    let new_idx = app
        .snippet_store
        .snippets
        .iter()
        .position(|s| s.name == name);
    app.ui.snippet_picker_state.select(new_idx);

    app.snippet_form_baseline = None;
    if is_new {
        app.set_status(format!("Added snippet '{}'.", name), false);
    } else {
        app.set_status(format!("Updated snippet '{}'.", name), false);
    }
    app.screen = Screen::SnippetPicker {
        target_aliases: target_aliases.to_vec(),
    };
}
