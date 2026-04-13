use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Screen};

pub(super) fn handle_tag_picker_screen(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('#') => {
            app.screen = Screen::HostList;
        }
        KeyCode::Char('?') => {
            let old = std::mem::replace(&mut app.screen, Screen::HostList);
            app.screen = Screen::Help {
                return_screen: Box::new(old),
            };
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.select_next_tag();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select_prev_tag();
        }
        KeyCode::PageDown => {
            crate::app::page_down(&mut app.ui.tag_picker_state, app.tag_list.len(), 10);
        }
        KeyCode::PageUp => {
            crate::app::page_up(&mut app.ui.tag_picker_state, app.tag_list.len(), 10);
        }
        KeyCode::Enter => {
            if let Some(index) = app.ui.tag_picker_state.selected() {
                if let Some(tag) = app.tag_list.get(index) {
                    let tag: String = tag.clone();
                    app.screen = Screen::HostList;
                    app.start_search();
                    app.search.query = Some(format!("tag={}", tag));
                    app.apply_filter();
                }
            }
        }
        _ => {}
    }
}
