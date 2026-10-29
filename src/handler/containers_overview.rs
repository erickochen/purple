//! Key handler for the global Containers tab (top_page = Containers).
//!
//! Routes navigation (j/k/g/G/PgUp/PgDn/Tab), search (`/`), sort (`s`),
//! detail-panel toggle (`v`), group fold (`Space` on a host divider),
//! refresh (`r`/`R`), add-host (`a`) and the destructive actions
//! (`Enter` shell, `K` restart, `S` stop, `e` exec, `l` logs,
//! `Ctrl-K` stack restart). Single-target actions (Enter shell, l, e)
//! gate on `selected_container_row`; host-scoped actions (K, S, r)
//! accept either a container row or a divider row, with the divider
//! variant queuing a bulk action against every running container on
//! the host.
//!
//! Enter queues an interactive shell session into the selected
//! container; the main loop drains `pending_container_exec` and spawns
//! `ssh -t <alias> <runtime> exec -it <id> sh -c 'bash || sh'`.

use std::sync::mpsc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::ctx::{Nav, Notify};
use crate::app::{
    App, ContainerState, ContainersOverviewState, HostState, JumpMode, Screen, StatusCenter,
    ViewMode,
};
use crate::containers::ContainerRuntime;
use crate::event::AppEvent;
use crate::preferences;

/// The slice of App every Containers-tab ACTION (exec, logs, restart/stop
/// confirms, host bulk restart/stop, stack restart) touches: the container
/// cache and pending exec/logs queues (`container_state`), the overview's
/// inspect cache (`containers_overview`, read to resolve a container's
/// compose project for the stack-restart member set), the host list
/// (read-only, to resolve each target's askpass and stale-host hint), the
/// status center, the screen and the demo flag. Queuing exec/logs or opening
/// a confirm screen runs entirely on the slice. the drain loop and the confirm
/// handlers spawn the SSH threads later, so no whole-App effect is deferred.
/// Navigation, search, sort and the `visible_items(app)` cursor resolution
/// legitimately stay on `&mut App` (they span ui + search + container_state +
/// hosts and use a whole-App render helper), exactly like the Tunnels tab.
struct ContainersActionCtx<'a> {
    container_state: &'a mut ContainerState,
    containers_overview: &'a mut ContainersOverviewState,
    hosts: &'a HostState,
    status: &'a mut StatusCenter,
    screen: &'a mut Screen,
    demo_mode: bool,
}

impl Nav for ContainersActionCtx<'_> {
    fn screen_mut(&mut self) -> &mut Screen {
        self.screen
    }
}

impl Notify for ContainersActionCtx<'_> {
    fn status_mut(&mut self) -> &mut StatusCenter {
        self.status
    }
}

impl ContainersActionCtx<'_> {
    fn ctx_from_app(app: &mut App) -> ContainersActionCtx<'_> {
        ContainersActionCtx {
            container_state: &mut app.container_state,
            containers_overview: &mut app.containers_overview,
            hosts: &app.hosts_state,
            status: &mut app.status_center,
            screen: &mut app.screen,
            demo_mode: app.demo_mode,
        }
    }

    /// Build a `Vec<StackMember>` of every running container on `alias`.
    /// Used to seed the bulk-action confirm dialogs the user opens with K
    /// or S while the cursor is on a host-divider row.
    fn host_running_members(&self, alias: &str) -> Vec<crate::app::StackMember> {
        self.container_state
            .cache_entry(alias)
            .into_iter()
            .flat_map(|entry| entry.containers.iter())
            .filter(|c| c.state.eq_ignore_ascii_case("running"))
            .map(|c| crate::app::StackMember {
                container_id: c.id.clone(),
                container_name: c.names.trim_start_matches('/').to_string(),
                uptime: crate::containers::parse_uptime_from_status(&c.status),
            })
            .collect()
    }

    /// Validate a cursor-resolved row and resolve runtime + askpass +
    /// stale-host hint. The cursor itself is resolved on `&App` before the
    /// slice is built (it needs the whole-App `visible_items` render helper);
    /// this method runs the running-state check, the id validation, the cache
    /// runtime lookup, the askpass/stale-hint resolution and any toast on the
    /// slice. Returns `None` (with a toast already emitted) when any
    /// precondition fails. Demo guards live on the individual callers.
    fn validate_running_row(
        &mut self,
        row: crate::ui::containers_overview::ContainerRow,
    ) -> Option<(
        crate::ui::containers_overview::ContainerRow,
        crate::containers::ContainerRuntime,
        Option<String>,
    )> {
        if !row.state.eq_ignore_ascii_case("running") {
            self.notify_warning(crate::messages::container_not_running(&row.name));
            return None;
        }
        if let Err(e) = crate::containers::validate_container_id(&row.id) {
            self.notify_error(e);
            return None;
        }
        let entry = self.container_state.cache_entry(&row.alias)?;
        let runtime = entry.runtime;
        let (askpass, stale_hint) = self
            .hosts
            .list()
            .iter()
            .find(|h| h.alias == row.alias)
            .map(|h| {
                let hint = super::host_form::stale_hint_for(h);
                (h.askpass.clone(), hint)
            })
            .unwrap_or((None, None));
        if let Some(hint) = stale_hint {
            self.notify_warning(crate::messages::stale_host(&hint));
        }
        Some((row, runtime, askpass))
    }

    /// Compose-project label of `container_id` from the inspect cache, when a
    /// fresh inspect result exists. Used to seed the single-container
    /// restart/stop confirm dialogs.
    fn project_for(&self, container_id: &str) -> Option<String> {
        self.containers_overview
            .inspect_cache()
            .entries
            .get(container_id)
            .and_then(|e| e.result.as_ref().ok())
            .and_then(|i| i.compose_project.clone())
    }

    /// Queue an interactive container shell session for `row`. The main loop
    /// drains `pending_container_exec` on the next tick. Skips with a toast
    /// when the container is not running, the host has no cached runtime, or
    /// demo mode is active.
    fn queue_shell_into(&mut self, row: crate::ui::containers_overview::ContainerRow) {
        if self.demo_mode {
            self.notify_warning(crate::i18n::text(
                crate::messages::DEMO_CONTAINER_EXEC_DISABLED,
            ));
            return;
        }
        let Some((row, runtime, askpass)) = self.validate_running_row(row) else {
            return;
        };
        log::info!(
            "[purple] container exec queued: alias={} container={} id={}",
            row.alias,
            row.name,
            row.id
        );
        self.container_state
            .queue_exec(crate::app::ContainerExecRequest {
                alias: row.alias,
                askpass,
                runtime,
                container_id: row.id,
                container_name: row.name,
                command: None,
            });
    }

    /// Queue a logs fetch for `row` and open the logs overlay with an empty
    /// placeholder body so the user sees a loading state while the SSH call
    /// runs. The result event populates the body and clears the placeholder.
    fn queue_logs_fetch(&mut self, row: crate::ui::containers_overview::ContainerRow) {
        let Some((row, runtime, askpass)) = self.validate_running_row(row) else {
            return;
        };
        log::info!(
            "[purple] container_logs queued: alias={} container={} id={}",
            row.alias,
            row.name,
            row.id
        );
        self.container_state
            .queue_logs(crate::app::ContainerLogsRequest {
                alias: row.alias.clone(),
                askpass,
                runtime,
                container_id: row.id.clone(),
                container_name: row.name.clone(),
            });
        self.container_state.set_logs_view(crate::app::LogsView {
            alias: row.alias,
            container_id: row.id,
            container_name: row.name,
            body: Vec::new(),
            fetched_at: 0,
            error: None,
            scroll: 0,
            last_render_height: 0,
            search: None,
        });
        self.set_screen(Screen::ContainerLogs);
    }

    /// Demo-guard + validate `row`, then open the restart confirm dialog.
    fn open_restart_confirm(&mut self, row: crate::ui::containers_overview::ContainerRow) {
        if self.demo_mode {
            self.notify_warning(crate::i18n::text(
                crate::messages::DEMO_CONTAINER_ACTIONS_DISABLED,
            ));
            return;
        }
        let Some((row, _runtime, _askpass)) = self.validate_running_row(row) else {
            return;
        };
        let project = self.project_for(&row.id);
        log::debug!(
            "[purple] container_restart: confirm opened alias={} id={}",
            row.alias,
            row.id
        );
        self.set_screen(Screen::ConfirmContainerRestart {
            alias: row.alias,
            container_id: row.id,
            container_name: row.name,
            project,
            uptime: row.uptime,
        });
    }

    /// Demo-guard + validate `row`, then open the stop confirm dialog.
    fn open_stop_confirm(&mut self, row: crate::ui::containers_overview::ContainerRow) {
        if self.demo_mode {
            self.notify_warning(crate::i18n::text(
                crate::messages::DEMO_CONTAINER_ACTIONS_DISABLED,
            ));
            return;
        }
        let Some((row, _runtime, _askpass)) = self.validate_running_row(row) else {
            return;
        };
        let project = self.project_for(&row.id);
        log::debug!(
            "[purple] container_stop: confirm opened alias={} id={}",
            row.alias,
            row.id
        );
        self.set_screen(Screen::ConfirmContainerStop {
            alias: row.alias,
            container_id: row.id,
            container_name: row.name,
            project,
            uptime: row.uptime,
        });
    }

    /// `K` on a host-divider row: confirm-then-queue a Restart for every
    /// running container on the host. Aborts with a toast when the host has
    /// nothing to restart. Demo mode short-circuits.
    fn open_host_restart_all_confirm(&mut self, alias: &str) {
        if self.demo_mode {
            self.notify_warning(crate::i18n::text(
                crate::messages::DEMO_CONTAINER_ACTIONS_DISABLED,
            ));
            return;
        }
        let members = self.host_running_members(alias);
        if members.is_empty() {
            self.notify_warning(crate::messages::container_host_no_running(alias));
            return;
        }
        log::debug!(
            "[purple] container_restart_all: confirm opened alias={} members={}",
            alias,
            members.len()
        );
        self.containers_overview
            .set_pending_bulk_confirm(crate::app::BulkConfirmContext {
                kind: crate::app::BulkConfirmKind::HostRestartAll,
                alias: alias.to_string(),
                project: None,
                members,
            });
        self.set_screen(Screen::ConfirmHostRestartAll);
    }

    /// `S` on a host-divider row: confirm-then-queue a Stop for every running
    /// container on the host.
    fn open_host_stop_all_confirm(&mut self, alias: &str) {
        if self.demo_mode {
            self.notify_warning(crate::i18n::text(
                crate::messages::DEMO_CONTAINER_ACTIONS_DISABLED,
            ));
            return;
        }
        let members = self.host_running_members(alias);
        if members.is_empty() {
            self.notify_warning(crate::messages::container_host_no_running(alias));
            return;
        }
        log::debug!(
            "[purple] container_stop_all: confirm opened alias={} members={}",
            alias,
            members.len()
        );
        self.containers_overview
            .set_pending_bulk_confirm(crate::app::BulkConfirmContext {
                kind: crate::app::BulkConfirmKind::HostStopAll,
                alias: alias.to_string(),
                project: None,
                members,
            });
        self.set_screen(Screen::ConfirmHostStopAll);
    }

    /// `Ctrl-K` on a container row: confirm-then-queue a Restart for every
    /// running member of the selected container's compose stack. Degrades to a
    /// toast when the inspect entry (and thus the project label) is missing.
    /// The demo guard runs in the caller (Ctrl-K is dispatched even on a
    /// host-divider row, so the toast must fire before the row is resolved).
    fn open_stack_restart_confirm(&mut self, row: crate::ui::containers_overview::ContainerRow) {
        // Resolve the compose-project label from the inspect cache. The entry
        // exists when the user has the detail panel open on the selected row
        // (the auto-fetch ensures one is in flight). When it is missing the
        // user typed Ctrl-K too soon. degrade to a toast rather than guessing
        // a project name.
        let Some(project) = self.project_for(&row.id) else {
            self.notify_warning(crate::messages::container_stack_unknown(&row.name));
            return;
        };
        // Walk every container of this host and pick the running members that
        // share the same compose_project. Rows with no inspect entry (cache
        // TTL race) are excluded; the user can re-trigger after a refresh.
        let members: Vec<crate::app::StackMember> = self
            .container_state
            .cache_entry(&row.alias)
            .into_iter()
            .flat_map(|entry| entry.containers.iter())
            .filter(|c| c.state.eq_ignore_ascii_case("running"))
            .filter_map(|c| {
                let inspect = self
                    .containers_overview
                    .inspect_cache()
                    .entries
                    .get(&c.id)?
                    .result
                    .as_ref()
                    .ok()?;
                if inspect.compose_project.as_deref() != Some(project.as_str()) {
                    return None;
                }
                Some(crate::app::StackMember {
                    container_id: c.id.clone(),
                    container_name: c.names.trim_start_matches('/').to_string(),
                    uptime: crate::containers::parse_uptime_from_status(&c.status),
                })
            })
            .collect();
        if members.is_empty() {
            self.notify_warning(crate::messages::container_stack_no_running(&project));
            return;
        }
        log::debug!(
            "[purple] stack_restart: confirm opened alias={} project={} members={}",
            row.alias,
            project,
            members.len()
        );
        self.containers_overview
            .set_pending_bulk_confirm(crate::app::BulkConfirmContext {
                kind: crate::app::BulkConfirmKind::StackRestart,
                alias: row.alias,
                project: Some(project),
                members,
            });
        self.set_screen(Screen::ConfirmStackRestart);
    }

    /// Demo-guard + validate `row`, then open the exec command prompt.
    fn open_exec_prompt(&mut self, row: crate::ui::containers_overview::ContainerRow) {
        if self.demo_mode {
            self.notify_warning(crate::i18n::text(
                crate::messages::DEMO_CONTAINER_EXEC_DISABLED,
            ));
            return;
        }
        let Some((row, _runtime, _askpass)) = self.validate_running_row(row) else {
            return;
        };
        log::debug!(
            "[purple] container_exec_prompt: opened alias={} id={}",
            row.alias,
            row.id
        );
        self.set_screen(Screen::ContainerExecPrompt {
            alias: row.alias,
            container_id: row.id,
            container_name: row.name,
            query: String::new(),
        });
    }
}

/// Alias of the container under the cursor, or `None` when the
/// cursor lands on a host-divider row. Pair with `selected_header_alias`
/// to resolve the alias for either kind of selectable row.
fn selected_container_alias(app: &App) -> Option<String> {
    selected_container_row(app).map(|row| row.alias)
}

/// Move cursor to the next visible item, header or container, wrapping
/// at the end. Headers are valid selection targets so the user can
/// drive bulk actions and group folding from a divider row.
fn select_next(app: &mut App) {
    let items = crate::ui::containers_overview::visible_items(app);
    let total = items.len();
    if total == 0 {
        app.ui.containers_overview_state_mut().select(None);
        return;
    }
    let cur = app.ui.containers_overview_state().selected().unwrap_or(0);
    let next = if cur + 1 >= total { 0 } else { cur + 1 };
    app.ui.containers_overview_state_mut().select(Some(next));
}

/// Move cursor to the previous visible item, header or container,
/// wrapping at the start.
fn select_prev(app: &mut App) {
    let items = crate::ui::containers_overview::visible_items(app);
    let total = items.len();
    if total == 0 {
        app.ui.containers_overview_state_mut().select(None);
        return;
    }
    let cur = app.ui.containers_overview_state().selected().unwrap_or(0);
    let prev = if cur == 0 { total - 1 } else { cur - 1 };
    app.ui.containers_overview_state_mut().select(Some(prev));
}

/// Index of the first/last visible item in the current list,
/// regardless of whether the row is a host divider or a container.
/// `g`/`G` use these to jump to the extremes.
fn first_visible_idx(app: &App) -> Option<usize> {
    let items = crate::ui::containers_overview::visible_items(app);
    if items.is_empty() { None } else { Some(0) }
}

fn last_visible_idx(app: &App) -> Option<usize> {
    let items = crate::ui::containers_overview::visible_items(app);
    if items.is_empty() {
        None
    } else {
        Some(items.len() - 1)
    }
}

/// Alias under the cursor when the cursor is parked on a host-divider
/// row. Returns `None` for container rows or when the list is empty.
fn selected_header_alias(app: &App) -> Option<String> {
    let sel = app.ui.containers_overview_state().selected()?;
    let items = crate::ui::containers_overview::visible_items(app);
    items.into_iter().nth(sel).and_then(|i| match i {
        crate::ui::containers_overview::ContainerListItem::HostHeader { alias, .. } => Some(alias),
        _ => None,
    })
}

/// `Space` keypress: if the cursor sits on a host-divider row, fold or
/// unfold that group. Persists the new state to preferences. Outside a
/// header row this is a no-op (fall-through to the literal-space arm
/// in search mode is handled separately by `handle_search_keys`).
fn toggle_collapse_for_selected_host(app: &mut App) {
    let Some(alias) = selected_header_alias(app) else {
        return;
    };
    let collapsed = app.containers_overview.toggle_host_collapsed(&alias);
    log::debug!("[purple] containers fold toggle: alias={alias} collapsed={collapsed}");
    if let Err(e) = preferences::save_containers_collapsed_hosts(
        app.env().paths(),
        app.containers_overview.collapsed_hosts(),
    ) {
        log::warn!("[config] Failed to persist containers collapsed hosts: {e}");
    }
}

/// The container under the cursor, or `None` when the cursor lands
/// on a host-divider row or nothing is selected. Action keys that
/// only make sense for a single container (Enter shell, K, S, l, e,
/// the inspect-detail trigger) gate their behaviour on this. The
/// host-aware actions (`r`, fold, bulk K/S) consult the helpers in
/// pairs (`selected_container_alias` + `selected_header_alias`).
fn selected_container_row(app: &App) -> Option<crate::ui::containers_overview::ContainerRow> {
    let sel = app.ui.containers_overview_state().selected()?;
    let items = crate::ui::containers_overview::visible_items(app);
    items.into_iter().nth(sel).and_then(|i| match i {
        crate::ui::containers_overview::ContainerListItem::Container(row) => Some(row),
        _ => None,
    })
}

/// Spawn one `docker ps` listing for `alias`. Used by both `r`
/// (single refresh) and the batch driver. Look-ups (`askpass`,
/// `cached_runtime`, `has_tunnel`) live in the caller so the batch
/// path can build queue items without re-borrowing `app` for every
/// pop.
fn spawn_refresh(
    config_path: std::path::PathBuf,
    env: std::sync::Arc<crate::runtime::env::Env>,
    bw_session: Option<String>,
    security_key: crate::security_key::SecurityKeyRun,
    item: crate::app::RefreshQueueItem,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    let ctx = crate::ssh_context::OwnedSshContext {
        alias: item.alias,
        config_path,
        askpass: item.askpass,
        session_password: item.session_password,
        bw_session,
        has_tunnel: item.has_tunnel,
        trust_new_host_key: false,
        security_key,
        env,
    };
    let tx = events_tx.clone();
    crate::containers::spawn_container_listing(ctx, item.cached_runtime, move |a, result| {
        let _ = tx.send(AppEvent::ContainerListing { alias: a, result });
    });
}

/// `r` keypress: refresh the cache for the host under the cursor.
/// Works whether the cursor is on a container row (use its host) or a
/// host-divider row (use that alias directly).
fn refresh_selected_host(app: &mut App, events_tx: &mpsc::Sender<AppEvent>) {
    let Some(alias) = selected_container_alias(app).or_else(|| selected_header_alias(app)) else {
        return;
    };
    if app.demo_mode {
        // Synthetic refresh: post a ContainerListing event for this
        // host built from the cached entry. The standard handler
        // bumps the cache timestamp to now, clearing the staleness
        // banner. Short delay so the in-flight toast is visible.
        let Some(entry) = app.container_state.cache_entry(&alias).cloned() else {
            return;
        };
        app.notify(crate::messages::container_refreshing(&alias));
        app.containers_overview
            .mark_auto_list_pending(alias.clone());
        let tx = events_tx.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(600));
            let listing = crate::containers::ContainerListing {
                runtime: entry.runtime,
                engine_version: entry.engine_version.clone(),
                containers: entry.containers.clone(),
            };
            let _ = tx.send(AppEvent::ContainerListing {
                alias,
                result: Ok(listing),
            });
        });
        return;
    }
    let cached_runtime = app.container_state.cache_entry(&alias).map(|e| e.runtime);
    let askpass = app
        .hosts_state
        .list()
        .iter()
        .find(|h| h.alias == alias)
        .and_then(|h| h.askpass.clone());
    let has_tunnel = app.tunnels.active_contains(&alias);
    let session_password = app.session_password_for(&alias);
    log::debug!("[purple] container refresh: alias={}", alias);
    app.notify(crate::messages::container_refreshing(&alias));
    // Mark the alias as in-flight so the post-key auto-refresh in
    // `ensure_list_for_selected_host` does not spawn a second
    // `docker ps` for the same host on the very same keystroke.
    app.containers_overview
        .mark_auto_list_pending(alias.clone());
    spawn_refresh(
        app.reload.config_path().to_path_buf(),
        std::sync::Arc::clone(&app.env),
        app.bw_session.clone(),
        app.security_key_confirmed_run(),
        crate::app::RefreshQueueItem {
            alias,
            askpass,
            session_password,
            cached_runtime,
            has_tunnel,
        },
        events_tx,
    );
}

/// Drain `pending_container_fetch_aliases` and queue a `docker ps`
/// for each survivor through the shared `RefreshBatch` driver.
/// Filters: alias still in config, non-empty hostname, no cache
/// entry yet, deduped within the drain. Demo mode + empty drain
/// are no-ops. The drain happens unconditionally so a second tick
/// does not retry the same items.
pub(crate) fn auto_fetch_new_hosts(app: &mut App, events_tx: &mpsc::Sender<AppEvent>) {
    let drained: Vec<String> = app.container_state.drain_pending_fetches();
    if app.demo_mode || drained.is_empty() {
        return;
    }
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut new_items: std::collections::VecDeque<crate::app::RefreshQueueItem> =
        std::collections::VecDeque::new();
    for alias in drained {
        if !seen.insert(alias.clone()) {
            continue;
        }
        if app.container_state.cache_contains(&alias) {
            continue;
        }
        let Some(host) = app.hosts_state.list().iter().find(|h| h.alias == alias) else {
            continue;
        };
        if host.hostname.is_empty() {
            continue;
        }
        let askpass = host.askpass.clone();
        let has_tunnel = app.tunnels.active_contains(&alias);
        let session_password = app.session_password_for(&alias);
        new_items.push_back(crate::app::RefreshQueueItem {
            alias,
            askpass,
            session_password,
            cached_runtime: None, // first fetch. runtime is detected on the SSH side
            has_tunnel,
        });
    }
    if new_items.is_empty() {
        return;
    }
    log::debug!(
        "[purple] container auto-fetch: queued {} new host(s)",
        new_items.len()
    );

    if let Some(batch) = app.containers_overview.refresh_batch_mut() {
        // Concurrent batch in flight (manual `R` already running):
        // append the new hosts to the existing queue. The driver
        // picks them up as in-flight slots free. `total` becomes
        // "what is done + what is in flight + what is queued" so
        // the progress toast stays consistent.
        batch.queue.extend(new_items);
        batch.total = batch.completed + batch.in_flight + batch.queue.len();
        return;
    }

    let total = new_items.len();
    let initial: Vec<crate::app::RefreshQueueItem> = (0..crate::app::REFRESH_MAX_PARALLEL
        .min(total))
        .filter_map(|_| new_items.pop_front())
        .collect();
    let in_flight = initial.len();
    let in_flight_aliases: std::collections::HashSet<String> =
        initial.iter().map(|i| i.alias.clone()).collect();

    app.containers_overview
        .start_refresh(crate::app::RefreshBatch {
            queue: new_items,
            in_flight,
            total,
            completed: 0,
            in_flight_aliases,
        });

    let config_path = app.reload.config_path().to_path_buf();
    let bw_session = app.bw_session.clone();
    for item in initial {
        spawn_refresh(
            config_path.clone(),
            std::sync::Arc::clone(&app.env),
            bw_session.clone(),
            app.security_key_confirmed_run(),
            item,
            events_tx,
        );
    }
}

/// `R` keypress: queue every host that already has a cache entry for
/// a windowed-concurrency refresh. The batch is driven by the
/// completion handler in `event_loop.rs`, which pops the next queued
/// item every time a listing returns.
fn refresh_all_hosts(app: &mut App, events_tx: &mpsc::Sender<AppEvent>) {
    if app.containers_overview.refresh_batch().is_some() {
        app.notify_warning(crate::i18n::text(
            crate::messages::REFRESH_BATCH_ALREADY_RUNNING,
        ));
        return;
    }
    if app.demo_mode {
        // Synthetic batch refresh for the demo: stage the same
        // RefreshBatch the real flow uses so the progress toast and
        // divider spinners render identically, then spawn a worker
        // that posts a ContainerListing event per host with a short
        // stagger. The standard handler bumps each entry's timestamp
        // to now, clearing every staleness banner in turn.
        let snapshots: Vec<(String, crate::containers::ContainerCacheEntry)> = app
            .container_state
            .cache()
            .iter()
            .map(|(a, e)| (a.clone(), e.clone()))
            .collect();
        let total = snapshots.len();
        if total == 0 {
            app.notify_warning(crate::i18n::text(
                crate::messages::REFRESH_NOTHING_TO_REFRESH,
            ));
            return;
        }
        let in_flight_aliases: std::collections::HashSet<String> =
            snapshots.iter().map(|(a, _)| a.clone()).collect();
        app.containers_overview
            .start_refresh(crate::app::RefreshBatch {
                queue: std::collections::VecDeque::new(),
                in_flight: total,
                total,
                completed: 0,
                in_flight_aliases,
            });
        app.notify_progress(crate::messages::container_refresh_progress(0, total));
        let tx = events_tx.clone();
        std::thread::spawn(move || {
            // 800ms initial pause so the spinner is unmistakable
            // before any host flips to fresh, then 200ms between
            // each completion so the progress toast ticks visibly.
            std::thread::sleep(std::time::Duration::from_millis(800));
            for (alias, entry) in snapshots.into_iter() {
                let listing = crate::containers::ContainerListing {
                    runtime: entry.runtime,
                    engine_version: entry.engine_version.clone(),
                    containers: entry.containers.clone(),
                };
                let _ = tx.send(AppEvent::ContainerListing {
                    alias,
                    result: Ok(listing),
                });
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        });
        return;
    }
    let mut queue: std::collections::VecDeque<crate::app::RefreshQueueItem> =
        std::collections::VecDeque::new();
    for (alias, entry) in app.container_state.cache() {
        let askpass = app
            .hosts_state
            .list()
            .iter()
            .find(|h| h.alias == *alias)
            .and_then(|h| h.askpass.clone());
        let has_tunnel = app.tunnels.active_contains(alias);
        let session_password = app.session_passwords.get(alias).cloned();
        queue.push_back(crate::app::RefreshQueueItem {
            alias: alias.clone(),
            askpass,
            session_password,
            cached_runtime: Some(entry.runtime),
            has_tunnel,
        });
    }
    let total = queue.len();
    if total == 0 {
        app.notify_warning(crate::i18n::text(
            crate::messages::REFRESH_NOTHING_TO_REFRESH,
        ));
        return;
    }
    log::info!("[purple] container refresh-all: queued {} hosts", total);

    // Pop up to MAX_PARALLEL items and spawn them immediately. Build
    // the in_flight_aliases set in lock-step so the batch driver can
    // ignore listings that arrive from non-batch triggers.
    let initial_batch: Vec<crate::app::RefreshQueueItem> = (0..crate::app::REFRESH_MAX_PARALLEL
        .min(total))
        .filter_map(|_| queue.pop_front())
        .collect();
    let in_flight = initial_batch.len();
    let in_flight_aliases: std::collections::HashSet<String> =
        initial_batch.iter().map(|i| i.alias.clone()).collect();

    app.containers_overview
        .start_refresh(crate::app::RefreshBatch {
            queue,
            in_flight,
            total,
            completed: 0,
            in_flight_aliases,
        });
    app.notify_progress(crate::messages::container_refresh_progress(0, total));

    let config_path = app.reload.config_path().to_path_buf();
    let bw_session = app.bw_session.clone();
    for item in initial_batch {
        spawn_refresh(
            config_path.clone(),
            std::sync::Arc::clone(&app.env),
            bw_session.clone(),
            app.security_key_confirmed_run(),
            item,
            events_tx,
        );
    }
}

/// Queue an interactive container shell session for the row under the
/// cursor. Same pattern as the host-list Enter → `pending_connect` flow.
/// The cursor row is resolved on `&App` (it needs the whole-App
/// `visible_items` render helper); queuing then runs on the action slice.
fn exec_into_selected_container(app: &mut App) {
    let Some(row) = selected_container_row(app) else {
        return;
    };
    ContainersActionCtx::ctx_from_app(app).queue_shell_into(row);
}

fn queue_logs_fetch_for_selected(app: &mut App) {
    let Some(row) = selected_container_row(app) else {
        return;
    };
    ContainersActionCtx::ctx_from_app(app).queue_logs_fetch(row);
}

fn open_restart_confirm(app: &mut App) {
    let Some(row) = selected_container_row(app) else {
        return;
    };
    ContainersActionCtx::ctx_from_app(app).open_restart_confirm(row);
}

fn open_stop_confirm(app: &mut App) {
    let Some(row) = selected_container_row(app) else {
        return;
    };
    ContainersActionCtx::ctx_from_app(app).open_stop_confirm(row);
}

/// `K` on a host-divider row: confirm-then-queue a Restart for every
/// running container on the host. Aborts with a toast when the host
/// has nothing to restart so the user is not staring at an empty
/// confirm dialog. Demo mode short-circuits with the standard
/// "actions disabled" toast.
fn open_host_restart_all_confirm(app: &mut App, alias: &str) {
    ContainersActionCtx::ctx_from_app(app).open_host_restart_all_confirm(alias);
}

/// `S` on a host-divider row: confirm-then-queue a Stop for every
/// running container on the host.
fn open_host_stop_all_confirm(app: &mut App, alias: &str) {
    ContainersActionCtx::ctx_from_app(app).open_host_stop_all_confirm(alias);
}

fn open_stack_restart_confirm(app: &mut App) {
    // Demo guard precedes the cursor resolution to match the original
    // order: Ctrl-K is dispatched unconditionally (not gated on a
    // container row), so the demo toast must fire even when the cursor
    // sits on a host-divider row.
    if app.demo_mode {
        app.notify_warning(crate::i18n::text(
            crate::messages::DEMO_CONTAINER_ACTIONS_DISABLED,
        ));
        return;
    }
    let Some(row) = selected_container_row(app) else {
        return;
    };
    ContainersActionCtx::ctx_from_app(app).open_stack_restart_confirm(row);
}

fn open_exec_prompt(app: &mut App) {
    let Some(row) = selected_container_row(app) else {
        return;
    };
    ContainersActionCtx::ctx_from_app(app).open_exec_prompt(row);
}

/// Handle a key event while the user is on the Containers tab. The
/// inspect-fetch trigger lives in `handler.rs::handle_main_screen`
/// because it needs the sender after this returns; refresh and add
/// (`r`/`R`/`a`) need the sender directly to spawn listings.
pub(super) fn handle_key(app: &mut App, key: KeyEvent, events_tx: &mpsc::Sender<AppEvent>) {
    if app.search.query().is_some() {
        handle_search_keys(app, key);
        return;
    }

    match key.code {
        // Ctrl-k restarts the whole compose stack of the selected
        // container. Crossterm delivers Ctrl-k as `Char('k')` with
        // the CONTROL modifier on every modern terminal; this arm
        // MUST precede the plain `Char('k')` (=select_prev) below
        // because match arms are walked top-down and the bare arm
        // would otherwise swallow the keystroke.
        KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            open_stack_restart_confirm(app);
        }
        // Legacy terminal fallback: `^K` arrives as the C0 control
        // codepoint with no modifier set.
        KeyCode::Char('\u{000B}') => {
            open_stack_restart_confirm(app);
        }
        KeyCode::Char('j') | KeyCode::Down => select_next(app),
        KeyCode::Char('k') | KeyCode::Up => select_prev(app),
        KeyCode::PageDown => {
            for _ in 0..10 {
                select_next(app);
            }
        }
        KeyCode::PageUp => {
            for _ in 0..10 {
                select_prev(app);
            }
        }
        KeyCode::Char('g') => {
            if let Some(idx) = first_visible_idx(app) {
                app.ui.containers_overview_state_mut().select(Some(idx));
            }
        }
        KeyCode::Char('G') => {
            if let Some(idx) = last_visible_idx(app) {
                app.ui.containers_overview_state_mut().select(Some(idx));
            }
        }
        KeyCode::Char('/') => {
            app.search.set_query(Some(String::new()));
            // Snap to the first non-header item; pre-search list may
            // start with a host divider in AlphaHost mode.
            let idx = first_visible_idx(app);
            app.ui.containers_overview_state_mut().select(idx);
        }
        KeyCode::Char('s') => {
            // Capture the alias under the cursor BEFORE flipping the
            // sort: visible_items() reads app.containers_overview.sort_mode,
            // so resolving the cursor after the mutation would pick a
            // different row. Track whether the cursor sat on a header
            // so we can land back on the divider rather than its first
            // child container after the flip. AlphaContainer mode has
            // no headers so a `prefer_header` request gracefully falls
            // back to the alias's first container.
            let was_header = selected_header_alias(app).is_some();
            let pinned = selected_container_alias(app).or_else(|| selected_header_alias(app));
            let new_mode = app.containers_overview.sort_mode().next();
            let paths = app.env().paths().cloned();
            let save_result = app
                .containers_overview
                .set_sort_mode(paths.as_ref(), new_mode);
            match pinned {
                Some(alias) => reposition_cursor_on(app, &alias, was_header),
                None => {
                    let idx = first_visible_idx(app);
                    app.ui.containers_overview_state_mut().select(idx);
                }
            }
            match save_result {
                Ok(()) => app.notify(crate::messages::sorted_by(new_mode.label())),
                Err(e) => {
                    app.notify_error(crate::messages::sorted_by_save_failed(new_mode.label(), &e))
                }
            }
        }
        KeyCode::Char(':') => {
            log::debug!("[purple] jump: opened from containers overview");
            app.open_jump(JumpMode::Containers);
        }
        KeyCode::Tab => {
            app.cycle_top_page_next();
            app.search.set_query(None);
        }
        KeyCode::BackTab => {
            app.cycle_top_page_prev();
            app.search.set_query(None);
        }
        // Enter on a container row queues an exec. On a host-header
        // row Enter falls through silently; Space is the single binding
        // for fold/unfold, consistent with purple's Space-toggles
        // convention.
        KeyCode::Enter if selected_header_alias(app).is_none() => {
            exec_into_selected_container(app);
        }
        KeyCode::Char('l') => {
            if selected_header_alias(app).is_some() {
                app.notify(crate::messages::container_action_needs_single("Logs"));
            } else {
                queue_logs_fetch_for_selected(app);
            }
        }
        KeyCode::Char('K') => {
            if let Some(alias) = selected_header_alias(app) {
                open_host_restart_all_confirm(app, &alias);
            } else {
                open_restart_confirm(app);
            }
        }
        KeyCode::Char('S') => {
            if let Some(alias) = selected_header_alias(app) {
                open_host_stop_all_confirm(app, &alias);
            } else {
                open_stop_confirm(app);
            }
        }
        KeyCode::Char('e') => {
            if selected_header_alias(app).is_some() {
                app.notify(crate::messages::container_action_needs_single("Exec"));
            } else {
                open_exec_prompt(app);
            }
        }
        KeyCode::Char('r') => {
            refresh_selected_host(app, events_tx);
        }
        KeyCode::Char('R') => {
            refresh_all_hosts(app, events_tx);
        }
        KeyCode::Char('a') => {
            if app.demo_mode {
                app.notify_warning(crate::i18n::text(
                    crate::messages::DEMO_CONTAINER_REFRESH_DISABLED,
                ));
                return;
            }
            // Guard: opening a host picker with zero hosts surfaces an
            // empty list, which reads as a bug. Mirror the tunnels-tab
            // guard pattern: notify the user and short-circuit so the
            // picker only opens when it has something to pick from.
            if app.hosts_state.list().is_empty() {
                app.notify_warning(crate::i18n::text(crate::messages::PICKER_NO_HOSTS));
                return;
            }
            app.ui.container_host_picker_state_mut().select(Some(0));
            app.ui.container_host_picker_query_mut().clear();
            app.set_screen(Screen::ContainerHostPicker);
        }
        KeyCode::Char('n') => {
            super::whats_new::dismiss_whats_new_toast(app);
            app.set_screen(Screen::WhatsNew(crate::app::WhatsNewState::default()));
        }
        KeyCode::Char('v') => {
            let new_mode = if app.containers_overview.view_mode() == ViewMode::Compact {
                ViewMode::Detailed
            } else {
                ViewMode::Compact
            };
            let paths = app.env().paths().cloned();
            let _ = app
                .containers_overview
                .set_view_mode(paths.as_ref(), new_mode);
            app.ui.set_detail_toggle_pending(true);
            app.ui.set_detail_scroll(0);
        }
        // SPACE GUARD MUST PRECEDE the generic Char(c) arm below. Without
        // it, fuzz-style char input would shadow the host-row collapse
        // toggle on Space.
        KeyCode::Char(' ') => {
            toggle_collapse_for_selected_host(app);
        }
        KeyCode::Char('?') => {
            // return_screen = HostList because TopPage::Containers shares
            // the HostList screen variant; the help dispatcher reads
            // app.top_page to pick the tab-specific column content.
            app.set_screen(Screen::Help {
                return_screen: Box::new(Screen::HostList),
            });
        }
        KeyCode::Char('q') => {
            app.running = false;
        }
        KeyCode::Esc
            if !app.ui.esc_quit_hint_shown()
                && !app.status_center.toast().is_some_and(|t| t.sticky) =>
        {
            log::debug!("[purple] esc on idle containers overview, showing quit hint toast");
            app.notify(crate::i18n::text(crate::messages::ESC_QUIT_HINT));
            app.ui.set_esc_quit_hint_shown(true);
        }
        _ => {}
    }
}

/// Re-anchor the cursor on `alias` after a layout-changing event
/// (sort flip, fold toggle). When `prefer_header` is true the cursor
/// lands on the host-divider row for `alias`; otherwise it lands on
/// the first container belonging to that host. Falls back to the
/// first visible row in either order when the alias has disappeared.
fn reposition_cursor_on(app: &mut App, alias: &str, prefer_header: bool) {
    let items = crate::ui::containers_overview::visible_items(app);
    if items.is_empty() {
        app.ui.containers_overview_state_mut().select(None);
        return;
    }
    let header_pos = items.iter().position(|i| match i {
        crate::ui::containers_overview::ContainerListItem::HostHeader { alias: a, .. } => {
            a == alias
        }
        _ => false,
    });
    let container_pos = items.iter().position(|i| match i {
        crate::ui::containers_overview::ContainerListItem::Container(row) => row.alias == alias,
        _ => false,
    });
    let new_idx = if prefer_header {
        header_pos.or(container_pos)
    } else {
        container_pos.or(header_pos)
    }
    .unwrap_or(0);
    app.ui.containers_overview_state_mut().select(Some(new_idx));
}

/// If the cursor points at a container whose `docker inspect` data is
/// missing or stale and no fetch is in flight, spawn one. Demo mode is
/// skipped: the demo seeds the cache directly with deterministic data so
/// the panel renders without SSH.
///
/// Called from `handler.rs` after every key event that lands the user
/// on the Containers tab. covers fresh entry via Tab as well as
/// in-tab navigation. Bursts on rapid scroll: the in-flight set dedups
/// per container ID, the 30s cache TTL prevents repeats, but a first
/// pass through 50 rows still fans out 50 SSH threads. Acceptable
/// because container IDs are unique, threads are cheap, and OpenSSH's
/// ControlMaster amortises connection setup. Revisit if real-world
/// usage shows lag.
pub(super) fn ensure_inspect_for_selected(app: &mut App, events_tx: &mpsc::Sender<AppEvent>) {
    if app.demo_mode || !app.running {
        // !running guard: once `q` flips it, the app is shutting down
        // and a new fetch would deliver into a dead receiver.
        return;
    }
    let Some((alias, container_id, runtime)) = selected_inspect_target(app) else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if app
        .containers_overview
        .inspect_cache()
        .fresh(&container_id, now)
        .is_some()
    {
        return;
    }
    if app
        .containers_overview
        .inspect_cache()
        .in_flight
        .contains(&container_id)
    {
        return;
    }
    app.containers_overview
        .inspect_cache_mut()
        .in_flight
        .insert(container_id.clone());

    let askpass = app
        .hosts_state
        .list()
        .iter()
        .find(|h| h.alias == alias)
        .and_then(|h| h.askpass.clone());
    let ctx = app.ssh_context_for(alias, askpass);
    let tx = events_tx.clone();
    crate::containers::spawn_container_inspect_listing(
        ctx,
        runtime,
        container_id,
        move |alias, container_id, result| {
            let _ = tx.send(AppEvent::ContainerInspectComplete {
                alias,
                container_id,
                result: Box::new(result),
            });
        },
    );
}

/// Resolve the (alias, container_id, runtime) tuple for the row under
/// the cursor. `None` when no row is selected, when the host has no
/// runtime cached yet (so we cannot pick docker vs podman), or when the
/// selected row's data has gone stale between frames.
fn selected_inspect_target(app: &App) -> Option<(String, String, ContainerRuntime)> {
    let row = selected_container_row(app)?;
    let entry = app.container_state.cache_entry(&row.alias)?;
    Some((row.alias, row.id, entry.runtime))
}

/// Prefetch `docker inspect` for every container in a fresh listing
/// so HEALTH and inspect-sourced detail cards populate without
/// per-row scroll. Dedup via the existing in-flight set + TTL.
pub(crate) fn prefetch_inspect_for_listing(
    app: &mut App,
    alias: &str,
    runtime: ContainerRuntime,
    containers: &[crate::containers::ContainerInfo],
    events_tx: &mpsc::Sender<AppEvent>,
) {
    if app.demo_mode || !app.running {
        return;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let askpass = app
        .hosts_state
        .list()
        .iter()
        .find(|h| h.alias == alias)
        .and_then(|h| h.askpass.clone());
    let has_tunnel = app.tunnels.active_contains(alias);
    let session_password = app.session_password_for(alias);
    let config_path = app.reload.config_path().to_path_buf();
    let bw_session = app.bw_session.clone();
    for c in containers {
        // Skip containers that already have a fresh inspect or a
        // pending fetch. A new SSH thread per container would
        // amortise OK against ControlMaster, but no point firing
        // when the existing trigger already covers it.
        if app
            .containers_overview
            .inspect_cache()
            .fresh(&c.id, now)
            .is_some()
        {
            continue;
        }
        if app
            .containers_overview
            .inspect_cache()
            .in_flight
            .contains(&c.id)
        {
            continue;
        }
        app.containers_overview
            .inspect_cache_mut()
            .in_flight
            .insert(c.id.clone());
        let ctx = crate::ssh_context::OwnedSshContext {
            alias: alias.to_string(),
            config_path: config_path.clone(),
            askpass: askpass.clone(),
            session_password: session_password.clone(),
            bw_session: bw_session.clone(),
            has_tunnel,
            trust_new_host_key: false,
            security_key: app.security_key_confirmed_run(),
            env: std::sync::Arc::clone(&app.env),
        };
        let tx = events_tx.clone();
        crate::containers::spawn_container_inspect_listing(
            ctx,
            runtime,
            c.id.clone(),
            move |alias, container_id, result| {
                let _ = tx.send(AppEvent::ContainerInspectComplete {
                    alias,
                    container_id,
                    result: Box::new(result),
                });
            },
        );
    }
}

/// Equivalent of `ensure_inspect_for_selected` for the LOGS card: spawn
/// `docker logs --tail N` for the row under the cursor when the cache
/// is stale and no fetch is in flight. Same per-keystroke trigger
/// shape so logs and inspect refresh in lockstep.
pub(super) fn ensure_logs_for_selected(app: &mut App, events_tx: &mpsc::Sender<AppEvent>) {
    if app.demo_mode || !app.running {
        return;
    }
    let Some((alias, container_id, runtime)) = selected_inspect_target(app) else {
        return;
    };
    // Resolve the container name once so the event carries it back for
    // the toast / log line, mirroring the existing inspect path.
    let container_name = app
        .container_state
        .cache_entry(&alias)
        .and_then(|e| e.containers.iter().find(|c| c.id == container_id))
        .map(|c| c.names.clone())
        .unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if app
        .containers_overview
        .logs_cache()
        .fresh(&container_id, now)
        .is_some()
    {
        return;
    }
    if app
        .containers_overview
        .logs_cache()
        .in_flight
        .contains(&container_id)
    {
        return;
    }
    app.containers_overview
        .logs_cache_mut()
        .in_flight
        .insert(container_id.clone());

    let askpass = app
        .hosts_state
        .list()
        .iter()
        .find(|h| h.alias == alias)
        .and_then(|h| h.askpass.clone());
    let ctx = app.ssh_context_for(alias, askpass);
    let tx = events_tx.clone();
    crate::containers::spawn_container_logs_fetch(
        ctx,
        runtime,
        container_id,
        container_name,
        crate::app::LOGS_TAIL,
        move |alias, container_id, _name, result| {
            let _ = tx.send(AppEvent::ContainerLogsTailComplete {
                alias,
                container_id,
                result: Box::new(result),
            });
        },
    );
}

/// Maximum number of containers to fire `docker inspect` for when the
/// cursor lands on a host-header row. Sized to cover the FLEET roll-up
/// and surface restart-loop / OOM signals in the host detail panel
/// without fanning out one SSH thread per container on hosts with
/// hundreds of services. Running containers are preferred; the rest is
/// best-effort.
const HOST_HEADER_INSPECT_FANOUT: usize = 10;

/// Pre-fetch up to `HOST_HEADER_INSPECT_FANOUT` `docker inspect` calls
/// when the cursor lands on a host-header row, so the ATTENTION card can
/// surface restart loops and OOM kills. Dedups via inspect_cache.fresh
/// + in_flight; no-op in demo mode or before the runtime is known.
pub(super) fn ensure_inspect_for_host_header(app: &mut App, events_tx: &mpsc::Sender<AppEvent>) {
    if app.demo_mode || !app.running {
        return;
    }
    let Some(alias) = selected_header_alias(app) else {
        return;
    };
    let Some(cache_entry) = app.container_state.cache_entry(&alias) else {
        return;
    };
    let runtime = cache_entry.runtime;
    // Running containers first so the inspect aggregate prioritises
    // signals tied to live workloads (restart loops on services that are
    // back up, recent OOMs on services that came back). Both buckets are
    // sorted by id so the spawn order is identical across repeated
    // renders, regardless of how `docker ps` interleaved its NDJSON.
    let mut ordered: Vec<String> = cache_entry
        .containers
        .iter()
        .filter(|c| c.state == "running")
        .map(|c| c.id.clone())
        .collect();
    ordered.sort();
    let mut rest: Vec<String> = cache_entry
        .containers
        .iter()
        .filter(|c| c.state != "running")
        .map(|c| c.id.clone())
        .collect();
    rest.sort();
    ordered.extend(rest);
    ordered.truncate(HOST_HEADER_INSPECT_FANOUT);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let askpass = app
        .hosts_state
        .list()
        .iter()
        .find(|h| h.alias == alias)
        .and_then(|h| h.askpass.clone());
    let has_tunnel = app.tunnels.active_contains(&alias);
    let session_password = app.session_password_for(&alias);
    let config_path = app.reload.config_path().to_path_buf();
    let bw_session = app.bw_session.clone();

    let mut fired: usize = 0;
    for container_id in ordered {
        if app
            .containers_overview
            .inspect_cache()
            .fresh(&container_id, now)
            .is_some()
        {
            continue;
        }
        if app
            .containers_overview
            .inspect_cache()
            .in_flight
            .contains(&container_id)
        {
            continue;
        }
        app.containers_overview
            .inspect_cache_mut()
            .in_flight
            .insert(container_id.clone());
        let ctx = crate::ssh_context::OwnedSshContext {
            alias: alias.clone(),
            config_path: config_path.clone(),
            askpass: askpass.clone(),
            session_password: session_password.clone(),
            bw_session: bw_session.clone(),
            has_tunnel,
            trust_new_host_key: false,
            security_key: app.security_key_confirmed_run(),
            env: std::sync::Arc::clone(&app.env),
        };
        let tx = events_tx.clone();
        crate::containers::spawn_container_inspect_listing(
            ctx,
            runtime,
            container_id.clone(),
            move |alias, container_id, result| {
                let _ = tx.send(AppEvent::ContainerInspectComplete {
                    alias,
                    container_id,
                    result: Box::new(result),
                });
            },
        );
        fired += 1;
    }
    if fired > 0 {
        log::debug!(
            "[purple] host_header inspect prefetch: alias={} fired={}",
            alias,
            fired
        );
    }
}

/// Refresh the selected row's host listing if cache is stale or
/// missing and no fetch is already in flight for that alias.
/// Skips if the alias is owned by a running `R` batch so the
/// batch driver retains exclusive completion semantics. No-op in
/// demo mode and during shutdown.
pub(super) fn ensure_list_for_selected_host(app: &mut App, events_tx: &mpsc::Sender<AppEvent>) {
    if app.demo_mode || !app.running {
        return;
    }
    // Header rows are selectable too; resolve the alias from either a
    // container row (its `alias` field) or a divider row (the alias is
    // the header itself) so the auto-refresh keeps the row under the
    // cursor fresh in both cases.
    let Some(alias) = selected_container_alias(app).or_else(|| selected_header_alias(app)) else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let cached_runtime = if let Some(entry) = app.container_state.cache_entry(&alias) {
        if now.saturating_sub(entry.timestamp) < crate::app::LIST_CACHE_TTL_SECS {
            return;
        }
        Some(entry.runtime)
    } else {
        // Should not happen. selected_container_row only returns rows
        // backed by a cache entry. but treat as "no cached runtime".
        None
    };
    if app.containers_overview.auto_list_pending(&alias) {
        return;
    }
    if let Some(batch) = app.containers_overview.refresh_batch()
        && batch.in_flight_aliases.contains(&alias)
    {
        return;
    }
    app.containers_overview
        .mark_auto_list_pending(alias.clone());

    let askpass = app
        .hosts_state
        .list()
        .iter()
        .find(|h| h.alias == alias)
        .and_then(|h| h.askpass.clone());
    let has_tunnel = app.tunnels.active_contains(&alias);
    let session_password = app.session_password_for(&alias);
    log::debug!("[purple] auto-list refresh: alias={}", alias);
    spawn_refresh(
        app.reload.config_path().to_path_buf(),
        std::sync::Arc::clone(&app.env),
        app.bw_session.clone(),
        app.security_key_confirmed_run(),
        crate::app::RefreshQueueItem {
            alias,
            askpass,
            session_password,
            cached_runtime,
            has_tunnel,
        },
        events_tx,
    );
}

fn handle_search_keys(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.search.set_query(None);
            let idx = first_visible_idx(app);
            app.ui.containers_overview_state_mut().select(idx);
        }
        KeyCode::Enter => {
            // Mirror the main handler: silent no-op on host headers,
            // exec on container rows. Clear search either way so the
            // user always returns to the full listing.
            app.search.set_query(None);
            if selected_header_alias(app).is_none() {
                exec_into_selected_container(app);
            }
        }
        KeyCode::Down | KeyCode::Tab => select_next(app),
        KeyCode::Up | KeyCode::BackTab => select_prev(app),
        KeyCode::PageDown => {
            for _ in 0..10 {
                select_next(app);
            }
        }
        KeyCode::PageUp => {
            for _ in 0..10 {
                select_prev(app);
            }
        }
        KeyCode::Backspace => {
            app.search.pop_query_char();
            let idx = first_visible_idx(app);
            app.ui.containers_overview_state_mut().select(idx);
        }
        KeyCode::Char(c) => {
            app.search.push_query_char(c);
            let idx = first_visible_idx(app);
            app.ui.containers_overview_state_mut().select(idx);
        }
        _ => {}
    }
}
