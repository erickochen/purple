use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Screen};

pub(super) fn handle_theme_picker(app: &mut App, key: KeyEvent) {
    let builtins = &app.ui.theme_picker_builtins;
    let custom = &app.ui.theme_picker_custom;
    let has_custom = !custom.is_empty();
    let divider_idx = if has_custom {
        Some(builtins.len())
    } else {
        None
    };
    let total = builtins.len() + if has_custom { 1 + custom.len() } else { 0 };

    if total == 0 {
        app.screen = Screen::HostList;
        return;
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            // Restore the theme that was active when the picker opened
            if let Some(original) = app.ui.theme_picker_original.take() {
                crate::ui::theme::set_theme(original);
            }
            app.ui.theme_picker_builtins = Vec::new();
            app.ui.theme_picker_custom = Vec::new();
            app.ui.theme_picker_saved_name = String::new();
            app.screen = Screen::HostList;
        }
        KeyCode::Char('?') => {
            let old = std::mem::replace(&mut app.screen, Screen::HostList);
            app.screen = Screen::Help {
                return_screen: Box::new(old),
            };
        }
        KeyCode::Char('j') | KeyCode::Down => {
            let current = app.ui.theme_picker_state.selected().unwrap_or(0);
            let mut next = current + 1;
            if next >= total {
                next = 0;
            }
            if divider_idx == Some(next) {
                next += 1;
                if next >= total {
                    next = 0;
                }
            }
            app.ui.theme_picker_state.select(Some(next));
            preview_theme_at_index(next, builtins, custom, divider_idx);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let current = app.ui.theme_picker_state.selected().unwrap_or(0);
            let mut next = if current == 0 { total - 1 } else { current - 1 };
            if divider_idx == Some(next) {
                next = if next == 0 { total - 1 } else { next - 1 };
            }
            app.ui.theme_picker_state.select(Some(next));
            preview_theme_at_index(next, builtins, custom, divider_idx);
        }
        KeyCode::Enter => {
            if let Some(theme) = theme_at_index(
                app.ui.theme_picker_state.selected().unwrap_or(0),
                builtins,
                custom,
                divider_idx,
            ) {
                if !crate::demo_flag::is_demo() {
                    let _ = crate::preferences::save_theme(&theme.name);
                }
                crate::ui::theme::set_theme(theme);
            }
            app.ui.theme_picker_builtins = Vec::new();
            app.ui.theme_picker_custom = Vec::new();
            app.ui.theme_picker_saved_name = String::new();
            app.ui.theme_picker_original = None;
            app.screen = Screen::HostList;
        }
        _ => {}
    }
}

fn preview_theme_at_index(
    idx: usize,
    builtins: &[crate::ui::theme::ThemeDef],
    custom: &[crate::ui::theme::ThemeDef],
    divider_idx: Option<usize>,
) {
    if let Some(theme) = theme_at_index(idx, builtins, custom, divider_idx) {
        crate::ui::theme::set_theme(theme);
    }
}

pub(super) fn theme_at_index(
    idx: usize,
    builtins: &[crate::ui::theme::ThemeDef],
    custom: &[crate::ui::theme::ThemeDef],
    divider_idx: Option<usize>,
) -> Option<crate::ui::theme::ThemeDef> {
    if idx < builtins.len() {
        return Some(builtins[idx].clone());
    }
    if let Some(div) = divider_idx {
        if idx == div {
            return None; // divider row
        }
        let custom_idx = idx - div - 1;
        return custom.get(custom_idx).cloned();
    }
    None
}
