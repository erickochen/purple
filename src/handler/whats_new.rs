use crossterm::event::{KeyCode, KeyEvent};
use log::debug;

use super::ctx::Nav;
use crate::app::{App, Screen, StatusCenter};
use crate::runtime::env::Env;

/// The slice of App the What's New overlay touches: the screen (scroll state
/// lives inside `Screen::WhatsNew`, plus the return-to-host-list transition),
/// the status center (to drop the "what's new" invite toast on close) and the
/// resolved environment (for the preferences path used to persist the last
/// seen version). It never reaches into hosts, tunnels or any other domain.
struct WhatsNewCtx<'a> {
    screen: &'a mut Screen,
    status: &'a mut StatusCenter,
    env: &'a Env,
}

impl Nav for WhatsNewCtx<'_> {
    fn screen_mut(&mut self) -> &mut Screen {
        self.screen
    }
}

pub(super) fn handle_key(app: &mut App, key: KeyEvent) {
    let mut ctx = WhatsNewCtx {
        screen: &mut app.screen,
        status: &mut app.status_center,
        env: app.env.as_ref(),
    };
    whats_new_key(&mut ctx, key);
}

fn whats_new_key(ctx: &mut WhatsNewCtx, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('n') => close_and_mark_seen(ctx),
        KeyCode::Char('j') | KeyCode::Down => {
            if let Screen::WhatsNew(ref mut state) = *ctx.screen {
                state.scroll = state.scroll.saturating_add(1);
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Screen::WhatsNew(ref mut state) = *ctx.screen {
                state.scroll = state.scroll.saturating_sub(1);
            }
        }
        KeyCode::PageDown => {
            if let Screen::WhatsNew(ref mut state) = *ctx.screen {
                state.scroll = state.scroll.saturating_add(10);
            }
        }
        KeyCode::PageUp => {
            if let Screen::WhatsNew(ref mut state) = *ctx.screen {
                state.scroll = state.scroll.saturating_sub(10);
            }
        }
        KeyCode::Char('g') | KeyCode::Home => {
            if let Screen::WhatsNew(ref mut state) = *ctx.screen {
                state.scroll = 0;
            }
        }
        KeyCode::Char('G') | KeyCode::End => {
            if let Screen::WhatsNew(ref mut state) = *ctx.screen {
                state.scroll = u16::MAX;
            }
        }
        _ => {}
    }
}

fn close_and_mark_seen(ctx: &mut WhatsNewCtx) {
    let version = env!("CARGO_PKG_VERSION");
    if let Err(e) = crate::preferences::save_last_seen_version(ctx.env.paths(), version) {
        log::warn!("[purple] failed to persist last_seen_version: {}", e);
    }
    let fragment = crate::i18n::text(crate::messages::whats_new_toast::INVITE_FRAGMENT);
    ctx.status.drop_toasts_where(|t| t.text.contains(fragment));
    debug!("[purple] whats-new closed, marked seen={}", version);
    ctx.set_screen(Screen::HostList);
}

pub(super) fn dismiss_whats_new_toast(app: &mut App) {
    let fragment = crate::i18n::text(crate::messages::whats_new_toast::INVITE_FRAGMENT);
    app.status_center
        .drop_toasts_where(|t| t.text.contains(fragment));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh_config::model::SshConfigFile;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn make_app_on_whats_new(initial_scroll: u16) -> App {
        let scratch = tempfile::tempdir().expect("tempdir").keep();
        let config = SshConfigFile {
            elements: SshConfigFile::parse_content(""),
            path: scratch.join("test_config"),
            crlf: false,
            bom: false,
        };
        let mut app = App::new(config);
        app.screen = Screen::WhatsNew(crate::app::WhatsNewState {
            scroll: initial_scroll,
        });
        app
    }

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn current_scroll(app: &App) -> u16 {
        match &app.screen {
            Screen::WhatsNew(state) => state.scroll,
            other => panic!("expected WhatsNew, got {other:?}"),
        }
    }

    #[test]
    fn esc_closes_overlay_and_returns_to_host_list() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app_on_whats_new(0);
        handle_key(&mut app, k(KeyCode::Esc));
        assert!(matches!(app.screen, Screen::HostList));
    }

    #[test]
    fn n_key_also_closes_overlay() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app_on_whats_new(0);
        handle_key(&mut app, k(KeyCode::Char('n')));
        assert!(matches!(app.screen, Screen::HostList));
    }

    #[test]
    fn j_and_down_scroll_one_line() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app_on_whats_new(5);
        handle_key(&mut app, k(KeyCode::Char('j')));
        assert_eq!(current_scroll(&app), 6);
        handle_key(&mut app, k(KeyCode::Down));
        assert_eq!(current_scroll(&app), 7);
    }

    #[test]
    fn k_and_up_scroll_back_and_saturate_at_zero() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app_on_whats_new(1);
        handle_key(&mut app, k(KeyCode::Char('k')));
        assert_eq!(current_scroll(&app), 0);
        handle_key(&mut app, k(KeyCode::Up));
        assert_eq!(current_scroll(&app), 0, "scroll saturates at zero");
    }

    #[test]
    fn page_down_jumps_ten_lines() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app_on_whats_new(0);
        handle_key(&mut app, k(KeyCode::PageDown));
        assert_eq!(current_scroll(&app), 10);
    }

    #[test]
    fn home_and_g_reset_to_top() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app_on_whats_new(42);
        handle_key(&mut app, k(KeyCode::Home));
        assert_eq!(current_scroll(&app), 0);
        let mut app = make_app_on_whats_new(42);
        handle_key(&mut app, k(KeyCode::Char('g')));
        assert_eq!(current_scroll(&app), 0);
    }

    #[test]
    fn end_and_capital_g_jump_to_bottom() {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = make_app_on_whats_new(0);
        handle_key(&mut app, k(KeyCode::End));
        assert_eq!(current_scroll(&app), u16::MAX);
    }
}
