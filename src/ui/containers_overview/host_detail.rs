use super::*;

/// Detail-panel content when the cursor is parked on a host-divider
/// row. Lists per-host scope (counts, runtime, last sync) plus a key
/// reminder so the user discovers the bulk-action affordances without
/// hunting through the help screen.
pub(crate) fn render_host_detail(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    alias: &str,
    total: usize,
    running: usize,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let lines = build_host_detail_lines(app, alias, total, running, area.width, area.height);
    frame.render_widget(Paragraph::new(lines), area);
}

/// Card stack for the host-overview detail panel: STATUS, FLEET,
/// optional ATTENTION, ACTIONS, HOST. The HOST card is last and stretches
/// to the panel bottom via `design::stretch_last_card`.
pub(crate) fn build_host_detail_lines(
    app: &App,
    alias: &str,
    total: usize,
    running: usize,
    width: u16,
    height: u16,
) -> Vec<Line<'static>> {
    let box_width = width as usize;
    let max_value_width = box_width
        .saturating_sub(4)
        .saturating_sub(design::SECTION_LABEL_W as usize);
    let mut lines: Vec<Line<'static>> = Vec::new();

    let entry = app.container_state.cache_entry(alias);
    let host = app.hosts_state.list().iter().find(|h| h.alias == alias);
    let collapsed = app.containers_overview.collapsed_hosts().contains(alias);
    let now = current_unix_secs();

    // STATUS card -----------------------------------------------------
    design::section_open(&mut lines, crate::i18n::text("STATUS"), box_width);
    push_ping_field(&mut lines, app, alias, max_value_width, box_width);
    if let Some(e) = entry {
        let age_secs = now.saturating_sub(e.timestamp);
        let age_text = crate::messages::relative_age(std::time::Duration::from_secs(age_secs));
        let style = if age_secs > 300 {
            theme::warning()
        } else {
            theme::muted()
        };
        design::section_field_styled(
            &mut lines,
            "Sync age",
            &age_text,
            style,
            max_value_width,
            box_width,
        );
        let runtime_label = match e.runtime {
            crate::containers::ContainerRuntime::Docker => "Docker",
            crate::containers::ContainerRuntime::Podman => "Podman",
        };
        let runtime_value = match e.engine_version.as_deref() {
            Some(v) if !v.is_empty() => format!("{} {}", runtime_label, v),
            _ => runtime_label.to_string(),
        };
        design::section_field(
            &mut lines,
            "Runtime",
            &runtime_value,
            max_value_width,
            box_width,
        );
    }
    if let Some(hist) = app.history.entry(alias) {
        let ago = crate::history::ConnectionHistory::format_time_ago(hist.last_connected);
        if !ago.is_empty() {
            design::section_field(
                &mut lines,
                "Last SSH",
                &format!("{} ago", ago),
                max_value_width,
                box_width,
            );
        }
    }
    design::section_close(&mut lines, box_width);

    // FLEET card ------------------------------------------------------
    design::section_open(&mut lines, "FLEET", box_width);
    let exited = total.saturating_sub(running);
    let counts = entry
        .map(|e| count_states(&e.containers))
        .unwrap_or_default();
    let dead = counts.dead;
    let paused = counts.paused;
    let restarting = counts.restarting;
    push_state_dots(&mut lines, running, exited, dead, paused, box_width);
    design::section_field(
        &mut lines,
        "Total",
        &format!("{}", total),
        max_value_width,
        box_width,
    );
    if let Some(e) = entry {
        let exit_nonzero = e
            .containers
            .iter()
            .filter(|c| container_has_nonzero_exit(app, c))
            .count();
        if exit_nonzero > 0 {
            design::section_field_styled(
                &mut lines,
                "Exit ne 0",
                &exit_nonzero.to_string(),
                theme::warning(),
                max_value_width,
                box_width,
            );
        }
    }
    let tunnel_active = app.tunnels.active_contains(alias);
    if tunnel_active {
        design::section_field_styled(
            &mut lines,
            crate::i18n::text("Tunnels"),
            "active",
            theme::online_dot(),
            max_value_width,
            box_width,
        );
    } else if let Some(h) = host
        && h.tunnel_count > 0
    {
        design::section_field(
            &mut lines,
            crate::i18n::text("Tunnels"),
            &h.tunnel_count.to_string(),
            max_value_width,
            box_width,
        );
    }
    if collapsed {
        design::section_field_styled(
            &mut lines,
            crate::i18n::text("Group"),
            "folded",
            theme::warning(),
            max_value_width,
            box_width,
        );
    }
    design::section_close(&mut lines, box_width);

    // ATTENTION card (conditional) ------------------------------------
    let stale_listing = entry
        .map(|e| now.saturating_sub(e.timestamp) > 300)
        .unwrap_or(false);
    let inspect_signals = entry
        .map(|e| collect_inspect_signals(app, &e.containers))
        .unwrap_or_default();
    let attention_needed = dead > 0
        || restarting > 0
        || stale_listing
        || !inspect_signals.restart_loops.is_empty()
        || inspect_signals.oom_count > 0
        || entry
            .map(|e| {
                e.containers
                    .iter()
                    .any(|c| container_has_nonzero_exit(app, c))
            })
            .unwrap_or(false);
    if attention_needed {
        design::section_open(&mut lines, "ATTENTION", box_width);
        if dead > 0 {
            design::section_field_styled(
                &mut lines,
                "Dead",
                &format!("{}  K to restart all running", dead),
                theme::error(),
                max_value_width,
                box_width,
            );
        }
        if restarting > 0 {
            design::section_field_styled(
                &mut lines,
                "Restarting",
                &restarting.to_string(),
                theme::warning(),
                max_value_width,
                box_width,
            );
        }
        if let Some(e) = entry {
            let bad_exit = e
                .containers
                .iter()
                .filter(|c| container_has_nonzero_exit(app, c))
                .count();
            if bad_exit > 0 {
                design::section_field_styled(
                    &mut lines,
                    "Exit ne 0",
                    &format!("{}  r to refresh", bad_exit),
                    theme::warning(),
                    max_value_width,
                    box_width,
                );
            }
        }
        for (name, count) in inspect_signals
            .restart_loops
            .iter()
            .take(ATTENTION_RESTART_LOOP_CAP)
        {
            let label = "Restart loop";
            let value = format!("{} ({})", name, count);
            design::section_field_styled(
                &mut lines,
                label,
                &value,
                theme::warning(),
                max_value_width,
                box_width,
            );
        }
        if inspect_signals.oom_count > 0 {
            design::section_field_styled(
                &mut lines,
                "OOM kills",
                &inspect_signals.oom_count.to_string(),
                theme::error(),
                max_value_width,
                box_width,
            );
        }
        if stale_listing {
            let ago = crate::messages::relative_age(std::time::Duration::from_secs(
                entry.map(|e| now.saturating_sub(e.timestamp)).unwrap_or(0),
            ));
            design::section_field_styled(
                &mut lines,
                "Stale",
                &format!("listing {}  r to refresh", ago),
                theme::warning(),
                max_value_width,
                box_width,
            );
        }
        design::section_close(&mut lines, box_width);
    }

    // ACTIONS card ----------------------------------------------------
    design::section_open(&mut lines, "ACTIONS", box_width);
    push_action_row(
        &mut lines,
        "K",
        "Restart running on host",
        running,
        running > 0,
        box_width,
    );
    push_action_row(
        &mut lines,
        "S",
        "Stop running on host",
        running,
        running > 0,
        box_width,
    );
    let refresh_qual = entry
        .map(|e| {
            let age = now.saturating_sub(e.timestamp);
            crate::messages::relative_age(std::time::Duration::from_secs(age))
        })
        .unwrap_or_else(|| "never synced".to_string());
    let r_qual = format!("last sync {}", refresh_qual);
    push_action_text_row(&mut lines, "r", "Refresh listing", &r_qual, true, box_width);
    let space_label = if collapsed {
        "Expand group"
    } else {
        "Collapse group"
    };
    push_action_text_row(&mut lines, "Space", space_label, "", true, box_width);
    design::section_close(&mut lines, box_width);

    // HOST card (last, stretches) -------------------------------------
    design::section_open(&mut lines, "HOST", box_width);
    if let Some(h) = host {
        let addr = if h.port != 22 {
            format!("{}:{}", h.hostname, h.port)
        } else {
            h.hostname.clone()
        };
        if !addr.is_empty() {
            design::section_field(&mut lines, "Address", &addr, max_value_width, box_width);
        }
        if !h.user.is_empty() {
            design::section_field(
                &mut lines,
                crate::i18n::text("User"),
                &h.user,
                max_value_width,
                box_width,
            );
        }
        if let Some(provider_name) = h.provider.as_deref() {
            let display = crate::providers::provider_display_name(provider_name);
            let region = h
                .provider_meta
                .iter()
                .find(|(k, _)| k == "region" || k == "zone" || k == "datacenter")
                .map(|(_, v)| v.clone());
            let value = match region {
                Some(r) if !r.is_empty() => format!("{} · {}", display, r),
                _ => display.to_string(),
            };
            design::section_field(
                &mut lines,
                crate::i18n::text("Provider"),
                &value,
                max_value_width,
                box_width,
            );
        }
        if !h.tags.is_empty() || !h.provider_tags.is_empty() {
            let combined: Vec<String> = h
                .provider_tags
                .iter()
                .chain(h.tags.iter())
                .cloned()
                .collect();
            let joined = combined.join(", ");
            design::section_field(
                &mut lines,
                crate::i18n::text("Tags"),
                &joined,
                max_value_width,
                box_width,
            );
        }
    } else {
        design::section_field(
            &mut lines,
            crate::i18n::text("Alias"),
            alias,
            max_value_width,
            box_width,
        );
    }
    design::section_close(&mut lines, box_width);

    design::stretch_last_card(&mut lines, height as usize, box_width);
    lines
}

/// FLEET state-dot row: one mixed-style line summarising counts.
pub(crate) fn push_state_dots(
    lines: &mut Vec<Line<'static>>,
    running: usize,
    exited: usize,
    dead: usize,
    paused: usize,
    box_width: usize,
) {
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled(
        format!(
            "{:<width$}",
            "State",
            width = design::SECTION_LABEL_W as usize
        ),
        theme::muted(),
    ));
    spans.push(Span::styled(
        format!("{} ", design::ICON_ONLINE),
        theme::online_dot(),
    ));
    spans.push(Span::styled(
        format!("{} running  ", running),
        theme::bold(),
    ));
    spans.push(Span::styled(
        format!("{} ", design::ICON_STOPPED),
        theme::muted(),
    ));
    spans.push(Span::styled(format!("{} exited  ", exited), theme::bold()));
    if dead > 0 {
        spans.push(Span::styled(
            format!("{} ", design::ICON_ERROR),
            theme::error(),
        ));
        spans.push(Span::styled(format!("{} dead", dead), theme::error()));
    }
    if paused > 0 {
        if dead > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            format!("{} ", design::ICON_PAUSED),
            theme::warning(),
        ));
        spans.push(Span::styled(format!("{} paused", paused), theme::warning()));
    }
    design::section_line(lines, spans, box_width);
}

pub(crate) fn push_ping_field(
    lines: &mut Vec<Line<'static>>,
    app: &App,
    alias: &str,
    _max_value_width: usize,
    box_width: usize,
) {
    let label_span = Span::styled(
        format!(
            "{:<width$}",
            "Ping",
            width = design::SECTION_LABEL_W as usize
        ),
        theme::muted(),
    );
    let value_spans: Vec<Span<'static>> = match app.ping.status_of(alias) {
        Some(crate::app::PingStatus::Reachable { rtt_ms }) => vec![
            Span::styled(format!("{} ", design::ICON_ONLINE), theme::online_dot()),
            Span::styled(host_list::format_rtt(*rtt_ms), theme::online_dot()),
        ],
        Some(crate::app::PingStatus::Slow { rtt_ms }) => vec![
            Span::styled(format!("{} ", design::ICON_STOPPED), theme::warning()),
            Span::styled(
                format!("slow {}", host_list::format_rtt(*rtt_ms)),
                theme::warning(),
            ),
        ],
        Some(crate::app::PingStatus::Unreachable) => vec![
            Span::styled(format!("{} ", design::ICON_ERROR), theme::error()),
            Span::styled("unreachable", theme::error()),
        ],
        Some(crate::app::PingStatus::Checking) => {
            vec![Span::styled("checking", theme::muted())]
        }
        Some(crate::app::PingStatus::Skipped) | None => {
            vec![Span::styled("--", theme::muted())]
        }
    };
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(1 + value_spans.len());
    spans.push(label_span);
    spans.extend(value_spans);
    design::section_line(lines, spans, box_width);
}

/// ACTIONS row with a count qualifier (e.g. "12 containers"). Disabled
/// rows render the key dimmed and the qualifier replaced with the muted
/// reason (`nothing running`).
pub(crate) fn push_action_row(
    lines: &mut Vec<Line<'static>>,
    key: &str,
    verb: &str,
    count: usize,
    enabled: bool,
    box_width: usize,
) {
    let qualifier = if !enabled {
        "nothing running".to_string()
    } else if count == 1 {
        "1 container".to_string()
    } else {
        format!("{} containers", count)
    };
    push_action_text_row(lines, key, verb, &qualifier, enabled, box_width);
}

pub(crate) fn push_action_text_row(
    lines: &mut Vec<Line<'static>>,
    key: &str,
    verb: &str,
    qualifier: &str,
    enabled: bool,
    box_width: usize,
) {
    let key_style = if enabled {
        theme::accent_bold()
    } else {
        theme::muted()
    };
    let verb_style = if enabled {
        theme::bold()
    } else {
        theme::muted()
    };
    // Key column width matches `design::SECTION_LABEL_W` so action
    // verbs line up vertically with the values in sibling cards
    // (STATUS/FLEET/HOST). The longest binding ("Space" = 5 cols)
    // still fits comfortably; the extra width is breathing room that
    // keeps the inner-card grid visually consistent.
    let key_field = format!("{:<width$}", key, width = design::SECTION_LABEL_W as usize);
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled(key_field, key_style));
    spans.push(Span::styled(verb.to_string(), verb_style));
    if !qualifier.is_empty() {
        spans.push(Span::styled(format!("  {}", qualifier), theme::muted()));
    }
    design::section_line(lines, spans, box_width);
}

#[derive(Default, Debug, PartialEq)]
pub(crate) struct StateCounts {
    pub(crate) running: usize,
    pub(crate) exited: usize,
    pub(crate) dead: usize,
    pub(crate) paused: usize,
    pub(crate) restarting: usize,
    pub(crate) created: usize,
}

pub(crate) fn count_states(containers: &[crate::containers::ContainerInfo]) -> StateCounts {
    let mut c = StateCounts::default();
    for ci in containers {
        match ci.state.as_str() {
            "running" => c.running += 1,
            "exited" => c.exited += 1,
            "dead" => c.dead += 1,
            "paused" => c.paused += 1,
            "restarting" => c.restarting += 1,
            "created" => c.created += 1,
            _ => {}
        }
    }
    c
}

/// Extract the integer in `Exited (N)` from a docker `Status` string. Returns
/// `None` when the prefix is absent or the captured slice is not a valid
/// integer. Used by the FLEET / ATTENTION cards to flag non-zero exits
/// without firing a per-container inspect.
pub(crate) fn parse_exit_code_from_status(status: &str) -> Option<i32> {
    design::parse_container_exit_code(status)
}

/// True when the container exited with a non-zero code. First tries the
/// docker `Status` string (`Exited (N)`); on podman where Status is empty,
/// falls back to the inspect cache for the same container ID. Returns
/// `false` when no exit signal is yet available (no Status pattern AND
/// no cached inspect) so the user sees a clean state rather than a false
/// warning until data lands.
pub(crate) fn container_has_nonzero_exit(app: &App, c: &crate::containers::ContainerInfo) -> bool {
    if let Some(code) = parse_exit_code_from_status(&c.status) {
        return code != 0;
    }
    if c.state != "exited" && c.state != "stopped" {
        return false;
    }
    app.containers_overview
        .inspect_cache()
        .entries
        .get(&c.id)
        .and_then(|e| e.result.as_ref().ok())
        .map(|i| i.exit_code != 0)
        .unwrap_or(false)
}

#[derive(Default, Debug)]
pub(crate) struct InspectSignals {
    pub(crate) restart_loops: Vec<(String, u32)>,
    pub(crate) oom_count: usize,
}

/// Restart-count threshold above which a container is flagged as a
/// restart loop in the host detail ATTENTION card. Five gives docker's
/// `on-failure:5` policy room to do its job before purple raises
/// the flag. Only persistent loops past that policy show up here.
pub(crate) const RESTART_LOOP_THRESHOLD: u32 = 5;

/// Maximum restart-loop rows to render inside one ATTENTION card. More
/// than this and the eye starts to skim past; the rest are dropped
/// silently. Aligned with the truncation on the host detail render path.
pub(crate) const ATTENTION_RESTART_LOOP_CAP: usize = 3;

/// Best-effort aggregate over the inspect cache for containers on this
/// host. Iterates the cache by container ID; missing entries are simply
/// absent from the result.
pub(crate) fn collect_inspect_signals(
    app: &App,
    containers: &[crate::containers::ContainerInfo],
) -> InspectSignals {
    let mut out = InspectSignals::default();
    for c in containers {
        let Some(entry) = app.containers_overview.inspect_cache().entries.get(&c.id) else {
            continue;
        };
        let Ok(insp) = entry.result.as_ref() else {
            continue;
        };
        if insp.oom_killed {
            out.oom_count += 1;
        }
        if insp.restart_count > RESTART_LOOP_THRESHOLD {
            out.restart_loops
                .push((clean_name(&c.names), insp.restart_count));
        }
    }
    out
}
