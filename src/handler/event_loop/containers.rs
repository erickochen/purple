//! Container listing, inspect, logs and action events. Maintains the
//! per-host container cache, the inspect and logs sub-caches keyed by
//! container ID and the `R`-batch refresh driver. Every handler is race
//! guarded: a result whose alias has since been removed from the SSH
//! config is dropped silently so the cache cannot be repopulated with
//! an orphan.

use std::sync::mpsc;

use crate::app::{App, Screen};
use crate::containers;
use crate::event::AppEvent;

/// Handle `AppEvent::ContainerListing`.
pub(crate) fn handle_container_listing(
    app: &mut App,
    alias: String,
    result: Result<containers::ContainerListing, containers::ContainerError>,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    // Race guard: a `docker ps` thread may return after the user
    // deleted the host (or `reload_hosts` pruned the cache via an
    // external config edit). Without this guard the cache would be
    // re-populated with an orphan and the next save would write it
    // back to disk. Drop the result silently in that case but still
    // tick the batch driver so an `R`-batch can complete cleanly.
    if !app.hosts_state.list().iter().any(|h| h.alias == alias) {
        log::debug!(
            "[purple] container_listing dropped: alias={} no longer in config",
            alias
        );
        crate::askpass::cleanup_marker(app.env.paths(), &alias);
        app.containers_overview.clear_auto_list_pending(&alias);
        drive_refresh_batch(app, &alias, events_tx);
        return;
    }
    // Always update cache, even if overlay is closed
    match &result {
        Ok(listing) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            app.container_state.insert_cache_entry(
                alias.clone(),
                containers::ContainerCacheEntry {
                    timestamp: now,
                    runtime: listing.runtime,
                    engine_version: listing.engine_version.clone(),
                    containers: listing.containers.clone(),
                },
            );
            containers::save_container_cache(app.env().paths(), app.container_state.cache());
            // Prefetch `docker inspect` for every container in this
            // listing so HEALTH and the inspect-sourced detail cards
            // populate without waiting for the user to scroll over
            // each row. Dedup via in_flight set + TTL.
            crate::handler::containers_overview::prefetch_inspect_for_listing(
                app,
                &alias,
                listing.runtime,
                &listing.containers,
                events_tx,
            );
        }
        Err(e) => {
            // Preserve runtime even on error
            if let Some(rt) = e.runtime
                && let Some(entry) = app.container_state.cache_entry_mut(&alias)
            {
                entry.runtime = rt;
            }
        }
    }
    // Update overlay state if open
    if let Some(ref mut state) = app.container_session
        && state.alias == alias
    {
        match result {
            Ok(listing) => {
                state.runtime = Some(listing.runtime);
                state.containers = listing.containers;
                state.loading = false;
                state.error = None;
                if let Some(sel) = state.list_state.selected() {
                    if sel >= state.containers.len() && !state.containers.is_empty() {
                        state.list_state.select(Some(0));
                    }
                } else if !state.containers.is_empty() {
                    state.list_state.select(Some(0));
                }
            }
            Err(e) => {
                if let Some(rt) = e.runtime {
                    state.runtime = Some(rt);
                }
                state.loading = false;
                state.error = Some(e.message);
            }
        }
    }
    crate::askpass::cleanup_marker(app.env.paths(), &alias);
    app.containers_overview.clear_auto_list_pending(&alias);

    // Drive the `R` batch refresh: a listing whose alias is in the
    // batch's in_flight set decrements the counter and pops the next
    // queued host. Non-batch listings (parallel `C`, `a`-add,
    // action-complete refresh) drop through unchanged.
    drive_refresh_batch(app, &alias, events_tx);
}

/// Drive the `R` batch refresh on each `ContainerListing` arrival.
///
/// Only listings whose alias is in `batch.in_flight_aliases` mutate
/// the batch counters. Listings from concurrent non-batch triggers
/// (host-list `C`, action-complete refresh, `a`-add) drop through .
/// without that guard the counter would corrupt and the batch could
/// either complete prematurely or hang.
pub(crate) fn drive_refresh_batch(app: &mut App, alias: &str, events_tx: &mpsc::Sender<AppEvent>) {
    let Some(batch) = app.containers_overview.refresh_batch_mut() else {
        return;
    };
    if !batch.in_flight_aliases.remove(alias) {
        // Listing for an alias that is not part of this batch.
        log::debug!(
            "[purple] refresh_batch: alias={} not in batch in_flight, ignoring",
            alias
        );
        return;
    }
    batch.in_flight = batch.in_flight.saturating_sub(1);
    batch.completed += 1;

    let next = batch.queue.pop_front();
    let total = batch.total;
    let completed = batch.completed;
    let queue_remaining = batch.queue.len();

    let spawned = next.is_some();
    if let Some(item) = next {
        batch.in_flight += 1;
        batch.in_flight_aliases.insert(item.alias.clone());
        let config_path = app.reload.config_path().to_path_buf();
        let bw_session = app.bw_session.clone();
        let ctx = crate::ssh_context::OwnedSshContext {
            alias: item.alias,
            config_path,
            askpass: item.askpass,
            session_password: item.session_password,
            bw_session,
            has_tunnel: item.has_tunnel,
            trust_new_host_key: false,
            security_key: app.security_key_confirmed_run(),
            env: std::sync::Arc::clone(&app.env),
        };
        let tx = events_tx.clone();
        containers::spawn_container_listing(ctx, item.cached_runtime, move |a, r| {
            let _ = tx.send(AppEvent::ContainerListing {
                alias: a,
                result: r,
            });
        });
    }

    // Re-borrow after the mutable spawn block above so the final
    // counters are post-pop-and-respawn.
    let still_in_flight = app
        .containers_overview
        .refresh_batch()
        .map(|b| b.in_flight)
        .unwrap_or(0);

    log::debug!(
        "[purple] refresh_batch: alias={} done={}/{} in_flight={} queue={} spawned_next={}",
        alias,
        completed,
        total,
        still_in_flight,
        queue_remaining,
        spawned
    );

    // Update progress / completion notification.
    if queue_remaining == 0 && still_in_flight == 0 {
        app.containers_overview.clear_refresh();
        // Clear the sticky progress footer set by notify_progress so the
        // success toast is the only thing the user sees after the batch.
        app.clear_status();
        app.notify(crate::messages::container_refresh_complete(total));
    } else {
        app.notify_progress(crate::messages::container_refresh_progress(
            completed, total,
        ));
    }
}

/// Handle `AppEvent::ContainerInspectComplete`. Stores the result (Ok or
/// Err) in the per-overview cache keyed by container ID and clears the
/// in-flight marker. The result itself is held even on error so the
/// detail panel can render the message instead of spinning forever.
pub(crate) fn handle_container_inspect_complete(
    app: &mut App,
    alias: String,
    container_id: String,
    result: Result<containers::ContainerInspect, String>,
) {
    // Race guard mirrors handle_container_listing: drop the result
    // when the host (and therefore its containers) is gone.
    if !app.hosts_state.list().iter().any(|h| h.alias == alias) {
        log::debug!(
            "[purple] container_inspect_complete dropped: alias={} no longer in config",
            alias
        );
        app.containers_overview
            .inspect_cache_mut()
            .in_flight
            .remove(&container_id);
        return;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    match &result {
        Ok(_) => log::debug!(
            "[purple] container_inspect_complete: alias={} id={} ok=true",
            alias,
            container_id,
        ),
        Err(msg) => log::warn!(
            "[external] container inspect failed: alias={} id={}: {}",
            alias,
            container_id,
            msg,
        ),
    }
    app.containers_overview.inspect_cache_mut().entries.insert(
        container_id.clone(),
        crate::app::InspectCacheEntry {
            timestamp: now,
            result,
        },
    );
    app.containers_overview
        .inspect_cache_mut()
        .in_flight
        .remove(&container_id);
}

/// Handle `AppEvent::ContainerLogsTailComplete`. Stores the result in
/// the per-overview logs cache keyed by container ID and clears the
/// in-flight marker. The result is held even on error so the LOGS card
/// can render the message in place of spinning.
pub(crate) fn handle_container_logs_tail_complete(
    app: &mut App,
    alias: String,
    container_id: String,
    result: Result<Vec<String>, String>,
) {
    if !app.hosts_state.list().iter().any(|h| h.alias == alias) {
        log::debug!(
            "[purple] container_logs_tail_complete dropped: alias={} no longer in config",
            alias
        );
        app.containers_overview
            .logs_cache_mut()
            .in_flight
            .remove(&container_id);
        return;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    match &result {
        Ok(_) => log::debug!(
            "[purple] container_logs_tail_complete: alias={} id={} ok=true",
            alias,
            container_id,
        ),
        Err(msg) => log::warn!(
            "[external] container logs tail failed: alias={} id={}: {}",
            alias,
            container_id,
            msg,
        ),
    }
    app.containers_overview.logs_cache_mut().entries.insert(
        container_id.clone(),
        crate::app::LogsCacheEntry {
            timestamp: now,
            result,
        },
    );
    app.containers_overview
        .logs_cache_mut()
        .in_flight
        .remove(&container_id);
}

/// Handle `AppEvent::ContainerLogsComplete`. When the user is still
/// on the matching `Screen::ContainerLogs` overlay, populate the body
/// (or error). When the screen has changed (Esc, Tab away), drop the
/// payload silently. The user opted out before the SSH call returned.
pub(crate) fn handle_container_logs_complete(
    app: &mut App,
    alias: String,
    container_id: String,
    container_name: String,
    result: Result<Vec<String>, String>,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if matches!(app.screen, Screen::ContainerLogs)
        && let Some(view) = app.container_state.logs_view_mut()
        && view.alias == alias
        && view.container_id == container_id
    {
        match result {
            Ok(lines) => {
                log::debug!(
                    "[purple] container_logs_complete: {} lines for alias={} id={}",
                    lines.len(),
                    alias,
                    container_id
                );
                view.body = lines;
                view.error = None;
                view.fetched_at = now;
                // Tail-anchor: align the bottom of the body with
                // the bottom of the viewport so the latest line
                // sits at the last visible row, with N preceding
                // lines filling the gap. The renderer wrote
                // `last_render_height` while painting the loading
                // placeholder one frame earlier.
                view.scroll = crate::handler::container_logs::tail_scroll(
                    view.body.len(),
                    view.last_render_height,
                );
                // Recompute search matches against the refreshed
                // body so an active `/foo` survives the `r` cycle.
                // Re-centre the viewport on the current match so
                // the user lands back on a visible hit.
                if let Some(mut s) = view.search.take() {
                    crate::handler::container_logs::refresh_search(&view.body, &mut s);
                    log::debug!(
                        "[purple] container_logs: search refreshed query={:?} matches={}",
                        s.query,
                        s.matches.len()
                    );
                    let body_len = view.body.len();
                    let last_h = view.last_render_height;
                    crate::handler::container_logs::recenter_on_match(
                        body_len,
                        last_h,
                        &s,
                        &mut view.scroll,
                    );
                    view.search = Some(s);
                }
            }
            Err(e) => {
                log::warn!(
                    "[external] container_logs_complete: alias={} id={} error={}",
                    alias,
                    container_id,
                    e
                );
                view.body.clear();
                view.error = Some(e);
                view.fetched_at = now;
                view.scroll = 0;
            }
        }
        return;
    }
    log::debug!(
        "[purple] container_logs_complete dropped (overlay closed): alias={} id={} name={}",
        alias,
        container_id,
        container_name
    );
}

/// Handle `AppEvent::ContainerActionComplete`.
pub(crate) fn handle_container_action_complete(
    app: &mut App,
    alias: String,
    action: containers::ContainerAction,
    result: Result<(), String>,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    log::debug!(
        "[purple] container action result: {} alias={alias} ok={}",
        action.as_str(),
        result.is_ok()
    );
    // Check if overlay matches and extract refresh info before notify
    let should_refresh = if let Some(ref mut state) = app.container_session {
        if state.alias == alias {
            state.action_in_progress = None;
            match result {
                Ok(()) => {
                    state.loading = true;
                    Some((state.alias.clone(), state.askpass.clone(), state.runtime))
                }
                Err(e) => {
                    state.error = Some(e);
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };
    if let Some((refresh_alias, askpass, cached_runtime)) = should_refresh {
        app.notify(crate::messages::container_action_complete(action.as_str()));
        // Mark in-flight so the scroll-driven auto-refresh does not
        // double-spawn for the same alias while this post-action
        // listing is still pending.
        app.containers_overview
            .mark_auto_list_pending(refresh_alias.clone());
        let ctx = app.ssh_context_for(refresh_alias, askpass);
        let tx = events_tx.clone();
        containers::spawn_container_listing(ctx, cached_runtime, move |a, r| {
            let _ = tx.send(AppEvent::ContainerListing {
                alias: a,
                result: r,
            });
        });
    }
    crate::askpass::cleanup_marker(app.env.paths(), &alias);
}
