use crossterm::event::{KeyCode, KeyEvent};
use std::sync::mpsc;

use crate::app::App;
use crate::event::AppEvent;

pub(super) fn handle_command_palette(
    app: &mut App,
    key: KeyEvent,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    let palette = match app.palette.as_mut() {
        Some(p) => p,
        None => return,
    };

    match key.code {
        KeyCode::Esc => {
            log::debug!("palette: closed via Esc");
            app.palette = None;
        }
        KeyCode::Down => {
            let count = palette.filtered_commands().len();
            if count > 0 {
                palette.selected = (palette.selected + 1).min(count - 1);
            }
        }
        KeyCode::Up => {
            palette.selected = palette.selected.saturating_sub(1);
        }
        KeyCode::Enter => {
            let filtered = palette.filtered_commands();
            // Clamp selected in case the filter shrank since last navigation
            let clamped = palette.selected.min(filtered.len().saturating_sub(1));
            if let Some(cmd) = filtered.get(clamped) {
                let key_char = cmd.key;
                log::debug!(
                    "palette: executing '{}' ({}) via Enter",
                    key_char,
                    cmd.label
                );
                app.palette = None;
                execute_command(app, key_char, events_tx);
            }
        }
        KeyCode::Backspace => {
            if palette.query.is_empty() {
                log::debug!("palette: closed via Backspace on empty query");
                app.palette = None;
            } else {
                palette.pop_query();
            }
        }
        KeyCode::Char(c) => {
            palette.push_query(c);
        }
        _ => {}
    }
}

/// Execute a palette command by dispatching to the host list handler.
fn execute_command(app: &mut App, key_char: char, events_tx: &mpsc::Sender<AppEvent>) {
    use crossterm::event::KeyModifiers;
    let key = KeyEvent::new(KeyCode::Char(key_char), KeyModifiers::NONE);
    super::host_list::handle_host_list(app, key, events_tx);
}
