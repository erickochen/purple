//! Tick, ping result and update-available events. Spinner animation,
//! ping TTL expiry, config-change detection, tunnel exit polling and
//! update notification all belong to the live-state lifecycle.

use std::time::Instant;

use crate::app::{self, App};

/// Handle `AppEvent::Tick` and `None` (timeout): spinner animation, ping TTL
/// expiry, config change detection and tunnel exit polling.
pub(crate) fn handle_tick(
    app: &mut App,
    anim: &mut crate::animation::AnimationState,
    vault_signing: bool,
    last_config_check: &mut Instant,
) {
    app.tick_status();
    app.tick_toast();
    let provider_syncing = !app.providers.syncing().is_empty();
    // Tick the spinner whenever something needs animation. Reachable hosts
    // drive the breathing online-dot pulse via `online_dot_pulsing(tick)`,
    // so they share the same monotonically-incrementing tick counter as
    // the spinner. saves a parallel tick driver. Active tunnels also
    // tick the spinner so the live chart wave has a continuous phase.
    let tunnels_animating =
        matches!(app.top_page, crate::app::TopPage::Tunnels) && !app.tunnels.active().is_empty();
    if anim.has_checking_hosts(app)
        || vault_signing
        || provider_syncing
        || anim.has_reachable_hosts(app)
        || tunnels_animating
    {
        anim.tick_spinner();
    }
    // Update the spinner character in the signing status text
    // so the spinner animates between VaultSignProgress events.
    if vault_signing
        && let Some(status) = app.status_center.status_mut()
        && status.sticky
        && !status.is_error()
    {
        let frame = crate::animation::SPINNER_FRAMES
            [anim.spinner_tick as usize % crate::animation::SPINNER_FRAMES.len()];
        if let Some(updated) = crate::replace_spinner_frame(&status.text, frame) {
            status.text = updated;
        }
    }
    // Animate the provider-sync footer: rotate the leading spinner frame on
    // each tick while a sync is in flight. The status is non-sticky (Info),
    // so we match by spinner-prefix instead of the sticky flag like
    // vault_signing does.
    if provider_syncing && let Some(status) = app.status_center.status_mut() {
        let frame = crate::animation::SPINNER_FRAMES
            [anim.spinner_tick as usize % crate::animation::SPINNER_FRAMES.len()];
        if let Some(updated) = crate::replace_spinner_frame(&status.text, frame) {
            status.text = updated;
            // Refresh created_at so the Info-class footer message does not
            // expire by length-proportional timeout in the gap between
            // sync_complete events. The message stays alive as long as at
            // least one provider is still syncing.
            status.created_at = std::time::Instant::now();
        }
    }
    // Throttle config file stat() to every 4 seconds
    if last_config_check.elapsed() >= std::time::Duration::from_secs(4) {
        app.check_config_changed();
        app.check_keys_changed();
        *last_config_check = Instant::now();
    }
    // A question the last push left behind waits while the user is in a
    // form or another dialog. This is where it gets its turn once they are
    // back on a page that can carry one.
    super::key_push::drain_next_key_push_prompt(app);

    // Poll active tunnels for exit
    let exited = app.poll_tunnels();
    for exit in exited {
        handle_tunnel_exit(app, exit);
    }
    // A tunnel that came up on a trial PIN proves it right away; it does not
    // wait for an exit that may never come.
    if let Some((alias, outcome)) = app.settle_authenticated_tunnel_trial() {
        report_refused_but_connected(app, &alias, &outcome);
    }
}

/// Settle what an exited tunnel says about PINs, then report the exit.
fn handle_tunnel_exit(app: &mut App, exit: app::TunnelExit) {
    // A tunnel that got in and then failed still proved its PIN.
    let end = if !exit.is_error || exit.authenticated {
        app::RunEnd::Connected
    } else if exit.auth_refused {
        app::RunEnd::AuthRefused
    } else {
        app::RunEnd::OtherFailure
    };
    let outcome =
        app.settle_security_key_run(&exit.alias, crate::security_key::PinFlow::Tunnel, end);
    if end != app::RunEnd::Connected && ask_tunnel_pin(app, &exit.alias, outcome.clone()) {
        return;
    }
    report_refused_but_connected(app, &exit.alias, &outcome);
    if exit.is_error {
        app.notify_background_error(exit.message);
    } else {
        app.notify_background(exit.message);
    }
}

/// Ask for the PIN a failed tunnel wanted and start it again with the answer.
/// Returns true when the PIN outcome replaced the plain exit message.
fn ask_tunnel_pin(app: &mut App, alias: &str, outcome: crate::app::PinOutcome) -> bool {
    let (key_path, refused) = match outcome {
        crate::app::PinOutcome::Needed { key_path } => (key_path, None),
        crate::app::PinOutcome::Rejected {
            key_path, reason, ..
        } => (key_path, Some(reason)),
        crate::app::PinOutcome::NotInvolved => return false,
    };
    if !app.can_open_tunnel_dialog() {
        log::debug!("[purple] tunnel: PIN needed but no dialog may open alias={alias}");
        let label = app.security_key_label(&key_path);
        let message = match refused {
            Some(reason) => crate::messages::askpass::pin_refused(&label, reason),
            None => crate::messages::askpass::pin_needed_tunnel(alias, &label),
        };
        app.notify_background_error(message);
        return true;
    }
    let retry = crate::app::PendingRetry::Tunnel {
        alias: alias.to_string(),
    };
    app.open_security_key_pin_prompt(alias, &key_path, retry, refused, None);
    true
}

/// Toast when the key refused the PIN although the host let ssh in another
/// way. The PIN is already forgotten.
fn report_refused_but_connected(app: &mut App, alias: &str, outcome: &crate::app::PinOutcome) {
    if let crate::app::PinOutcome::Rejected {
        key_path,
        connected: true,
        ..
    } = outcome
    {
        let label = app.security_key_label(key_path);
        app.notify_warning(crate::messages::askpass::pin_refused_but_connected(
            alias, &label,
        ));
    }
}

/// Handle `AppEvent::PingResult`.
pub(crate) fn handle_ping_result(
    app: &mut App,
    alias: String,
    rtt_ms: Option<u32>,
    generation: u64,
) {
    if generation == app.ping.generation() {
        let status = app::classify_ping(rtt_ms, app.ping.slow_threshold_ms());
        let now = Instant::now();
        log::debug!(
            "[external] ping-result: {} → {:?} (rtt={:?}ms, gen={})",
            alias,
            status,
            rtt_ms,
            generation
        );
        app.ping.insert_status(alias.clone(), status.clone());
        app.ping.record_check(alias.clone(), now);
        // Propagate bastion status to all ProxyJump dependents.
        app::propagate_ping_to_dependents(
            app.hosts_state.list(),
            app.ping.status_map_mut(),
            &alias,
            &status,
        );
        let mut propagated = 0usize;
        for h in app.hosts_state.list() {
            if h.proxy_jump == alias {
                app.ping.record_check(h.alias.clone(), now);
                propagated += 1;
            }
        }
        if propagated > 0 {
            log::debug!(
                "[purple] ping-result: propagated bastion {} status+timestamp to {} dependent(s)",
                alias,
                propagated
            );
        }
        // Update live filter/sort as results arrive
        if app.ping.filter_down_only() {
            app.apply_filter();
        }
        if app.hosts_state.sort_mode() == app::SortMode::Status {
            app.apply_sort();
        }
        // Update "last checked" timestamp when all pings are done
        if !app.ping.status_is_empty()
            && app
                .ping
                .status_map()
                .values()
                .all(|s| !matches!(s, app::PingStatus::Checking))
        {
            app.ping.set_checked_at(Some(Instant::now()));
        }
    }
}

/// Handle `AppEvent::UpdateAvailable`.
pub(crate) fn handle_update_available(app: &mut App, version: String, headline: Option<String>) {
    app.update.announce(version, headline);
}

#[cfg(test)]
mod pin_tests {
    use super::*;
    use crate::app::{PendingRetry, PinOutcome, Screen};
    use crate::ssh_config::model::SshConfigFile;

    fn app_with(config: &str) -> App {
        let scratch = tempfile::tempdir().expect("tempdir").keep();
        let cfg = SshConfigFile {
            elements: SshConfigFile::parse_content(config),
            path: scratch.join("config"),
            crlf: false,
            bom: false,
        };
        App::new(cfg)
    }

    #[test]
    fn a_tunnel_that_wanted_a_pin_asks_for_it_and_queues_a_restart() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        let asked = ask_tunnel_pin(
            &mut app,
            "h",
            PinOutcome::Needed {
                key_path: "/k".into(),
            },
        );
        assert!(asked);
        assert_eq!(app.screen, Screen::PasswordPrompt);
        let state = app.password_prompt.as_ref().expect("prompt");
        assert_eq!(state.retry, PendingRetry::Tunnel { alias: "h".into() });
        assert_eq!(state.security_key.as_deref(), Some("/k"));
    }

    #[test]
    fn a_tunnel_pin_waits_as_a_toast_while_the_user_is_busy() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        app.search.set_query(Some("x".to_string()));
        let asked = ask_tunnel_pin(
            &mut app,
            "h",
            PinOutcome::Rejected {
                key_path: "/k".into(),
                reason: crate::security_key::RefusalReason::WrongPin,
                connected: false,
            },
        );
        assert!(asked);
        assert!(app.password_prompt.is_none());
        let toast = app.status_center.toast().expect("toast");
        assert_eq!(
            toast.text,
            crate::messages::askpass::pin_refused(
                "/k",
                crate::security_key::RefusalReason::WrongPin
            ),
            "the reason is not lost"
        );
    }

    #[test]
    fn a_tunnel_that_wants_a_pin_while_the_user_is_busy_says_how_to_give_it() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        app.search.set_query(Some("x".to_string()));
        assert!(ask_tunnel_pin(
            &mut app,
            "h",
            PinOutcome::Needed {
                key_path: "/k".into(),
            },
        ));
        let toast = app.status_center.toast().expect("toast");
        assert_eq!(
            toast.text,
            crate::messages::askpass::pin_needed_tunnel("h", "/k")
        );
    }

    fn exit(alias: &str, authenticated: bool, auth_refused: bool) -> app::TunnelExit {
        app::TunnelExit {
            alias: alias.into(),
            message: format!("Tunnel for {alias}: forward failed"),
            is_error: true,
            auth_refused,
            authenticated,
        }
    }

    #[test]
    fn a_tunnel_that_got_in_and_then_failed_still_proves_its_pin() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        app.security_key_pins.start_trial(crate::app::PinTrial {
            alias: "h".into(),
            key_path: "/k".into(),
            pin: "4242".into(),
            flow: crate::security_key::PinFlow::Tunnel,
        });
        crate::security_key::record_hand_out(app.env().paths(), "h", 1, "/k");
        handle_tunnel_exit(&mut app, exit("h", true, false));
        assert_eq!(app.security_key_pins.confirmed_pin("/k"), Some("4242"));
        assert!(app.password_prompt.is_none());
        let toast = app.status_center.toast().expect("toast");
        assert_eq!(toast.text, "Tunnel for h: forward failed");
    }

    #[test]
    fn a_tunnel_refused_at_login_asks_for_the_pin_it_wanted() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        crate::security_key::arm_pin_request(
            app.env().paths(),
            crate::security_key::PinFlow::Tunnel,
            "h",
            "/k",
        );
        handle_tunnel_exit(&mut app, exit("h", false, true));
        assert_eq!(app.screen, Screen::PasswordPrompt);
        assert_eq!(
            app.password_prompt
                .as_ref()
                .and_then(|s| s.security_key.as_deref()),
            Some("/k")
        );
    }

    #[test]
    fn a_tunnel_failure_without_a_pin_keeps_its_own_message() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        assert!(!ask_tunnel_pin(&mut app, "h", PinOutcome::NotInvolved));
        assert!(app.password_prompt.is_none());
    }
}
