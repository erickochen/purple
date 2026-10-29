//! Key handler for the jump overlay (the `:` command bar).
//!
//! Takes `&mut App`, not a per-domain slice: it forwards synthetic key events
//! into other handlers (host_list, tunnels_overview, containers) and lands the
//! cursor across domains, so it is a router. See `ctx.rs` for the slice pattern
//! the single-domain handlers use.

use crossterm::event::{KeyCode, KeyEvent};
use std::sync::mpsc;

use crate::app::{App, JumpActionTarget, JumpHit, JumpMode, Screen, TopPage};
use crate::event::AppEvent;

pub(super) fn handle_key(app: &mut App, key: KeyEvent, events_tx: &mpsc::Sender<AppEvent>) {
    if app.jump.is_none() {
        return;
    }

    match key.code {
        KeyCode::Esc => {
            log::debug!("[purple] jump: closed via Esc");
            app.close_jump();
        }
        KeyCode::Down => {
            if let Some(p) = app.jump.as_mut() {
                p.move_down();
            }
        }
        KeyCode::Up => {
            if let Some(p) = app.jump.as_mut() {
                p.move_up();
            }
        }
        KeyCode::Tab => {
            if let Some(p) = app.jump.as_mut() {
                p.reveal_cursor();
                p.jump_next_section();
            }
        }
        KeyCode::Enter => {
            // Force a recompute so direct state-poking tests, and any path
            // that mutated query without running through Char(c), see the
            // up-to-date hits before we read selection.
            app.recompute_jump_hits();
            let chosen = app
                .jump
                .as_ref()
                .and_then(|p| p.visible_hits().get(p.selected()).cloned());
            if let Some(hit) = chosen {
                log::debug!("[purple] jump: dispatching {:?} via Enter", hit.identity());
                app.record_jump_hit(&hit);
                let mode = app
                    .jump
                    .as_ref()
                    .map(|p| p.mode())
                    .unwrap_or(JumpMode::Hosts);
                app.close_jump();
                dispatch_hit(app, &hit, mode, events_tx);
            }
        }
        KeyCode::Backspace => {
            let close = app
                .jump
                .as_ref()
                .map(|p| p.query().is_empty())
                .unwrap_or(true);
            if close {
                log::debug!("[purple] jump: closed via Backspace on empty query");
                app.close_jump();
            } else if let Some(p) = app.jump.as_mut() {
                p.pop_query();
                if p.query().is_empty() {
                    p.reset_after_clear_query();
                }
                app.recompute_jump_hits();
            }
        }
        KeyCode::Char(c) => {
            if let Some(p) = app.jump.as_mut() {
                p.push_query(c);
                // Typing reveals the cursor — once a query exists the
                // selection IS meaningful again. Empty-query state
                // re-suppresses the cursor on next render.
                p.reveal_cursor();
            }
            app.recompute_jump_hits();
        }
        _ => {}
    }
}

/// Route an Enter on a jump hit to the right destination. For actions
/// this is the existing key-dispatch shim; for data sources it switches
/// tab/screen and selects the matched item. Default action runs after the
/// jump (connect host, toggle tunnel, run snippet, open container detail).
fn dispatch_hit(app: &mut App, hit: &JumpHit, _mode: JumpMode, events_tx: &mpsc::Sender<AppEvent>) {
    match hit {
        JumpHit::Action(action) => execute_action(app, action, events_tx),
        JumpHit::Host(h) => {
            app.top_page = TopPage::Hosts;
            app.set_screen(Screen::HostList);
            app.select_host_by_alias(&h.alias);
            // Default action on a host: trigger the connect flow exactly like
            // pressing Enter on the host list.
            super::host_list::handle_main_key(
                app,
                KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE),
                events_tx,
            );
        }
        JumpHit::Tunnel(t) => {
            app.top_page = TopPage::Tunnels;
            app.set_screen(Screen::HostList);
            app.select_host_by_alias(&t.alias);
            // The tunnels overview reads its highlight from
            // `tunnels_overview_state`, NOT the host list. Compute the
            // matching row in the overview's visible_pairs and set the
            // ListState there so the cursor lands on the actual tunnel
            // the user picked.
            let pairs = crate::ui::tunnels_overview::visible_pairs(
                &app.search,
                &app.hosts_state,
                &app.tunnels,
                &app.history,
            );
            let landed_on_match = pairs
                .iter()
                .position(|(alias, rule)| alias == &t.alias && rule.bind_port == t.bind_port)
                .map(|idx| {
                    app.ui.tunnels_overview_state_mut().select(Some(idx));
                    true
                })
                .unwrap_or(false);
            // Mirror the host-hit auto-Enter: trigger the tunnel's primary
            // action (start / stop) so picking a tunnel from the palette
            // is a one-shot toggle, just like picking a host is a one-shot
            // connect. Only fire when the cursor actually landed on the
            // matching tunnel; otherwise we would toggle whichever row
            // happens to be selected and confuse the user.
            if landed_on_match {
                super::tunnels_overview::handle_key(
                    app,
                    KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE),
                );
            }
        }
        JumpHit::Container(c) => {
            // Land on the global containers tab with the cursor on the
            // picked container row. Falls back to the host-divider row
            // when the container's group is currently folded; falls
            // through to the first visible row when the cache no
            // longer carries the container.
            app.top_page = TopPage::Containers;
            app.set_screen(Screen::HostList);
            let landed_on_container = crate::ui::containers_overview::position_of_container(
                app,
                &c.alias,
                &c.container_id,
            );
            let target_idx = landed_on_container
                .or_else(|| {
                    crate::ui::containers_overview::visible_items(app)
                        .iter()
                        .position(|item| match item {
                            crate::ui::containers_overview::ContainerListItem::HostHeader {
                                alias,
                                ..
                            } => alias == &c.alias,
                            _ => false,
                        })
                })
                .or_else(|| {
                    crate::ui::containers_overview::first_visible_index(
                        &crate::ui::containers_overview::visible_items(app),
                    )
                });
            app.ui.containers_overview_state_mut().select(target_idx);
            // Mirror the host-hit auto-Enter: when the cursor lands on the
            // actual container row (not a fallback host-divider), trigger
            // the primary action (exec prompt) so picking a container
            // from the palette is a one-shot shell-in. Skip the synthetic
            // Enter when we fell back to a host header because Enter on a
            // header is a no-op and would silently consume the gesture.
            if landed_on_container.is_some() {
                super::containers_overview::handle_key(
                    app,
                    KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE),
                    events_tx,
                );
            }
        }
        JumpHit::Snippet(_s) => {
            // The snippet picker requires at least one target host. If the
            // user opened the jump bar with no host selected, surface a
            // warning instead of opening an unusable picker.
            let target: Vec<String> = app
                .selected_host()
                .map(|h| h.alias.clone())
                .into_iter()
                .collect();
            if target.is_empty() {
                app.notify_warning(crate::i18n::text(
                    crate::messages::PALETTE_SNIPPET_NEEDS_HOST,
                ));
                return;
            }
            app.snippets.set_flow_targets(target);
            app.set_screen(Screen::SnippetPicker);
        }
    }
}

/// Execute a jump action by routing it to the handler indicated by the
/// action's `target`. Switches `top_page` first so cross-tab actions
/// (e.g. picking `Tunnels: Add tunnel` from the Hosts tab) land the user
/// on the right page before the synthetic keypress fires.
fn execute_action(
    app: &mut App,
    action: &crate::app::JumpAction,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    let key = KeyEvent::new(KeyCode::Char(action.key), action.modifiers);
    match action.target {
        JumpActionTarget::Hosts => {
            app.top_page = TopPage::Hosts;
            super::host_list::handle_main_key(app, key, events_tx);
        }
        JumpActionTarget::Tunnels => {
            app.top_page = TopPage::Tunnels;
            super::tunnels_overview::handle_key(app, key);
        }
        JumpActionTarget::Containers => {
            app.top_page = TopPage::Containers;
            app.set_screen(Screen::HostList);
            super::containers_overview::handle_key(app, key, events_tx);
        }
        JumpActionTarget::Keys => {
            app.top_page = TopPage::Keys;
            app.set_screen(Screen::HostList);
            super::keys_overview::handle_key(app, key);
        }
    }
}
