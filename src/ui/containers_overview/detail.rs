use super::*;

/// Render the detail panel for the selected container row. Cards are the
/// visual structure (no outer wrapper block). The top cards stack from
/// the top; the LOGS card stretches to fill the remaining height down
/// to the panel bottom so the panel always ends flush with the list.
pub(crate) fn render_detail(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    selected: Option<&ContainerRow>,
    spinner_tick: u64,
) {
    let Some(row) = selected else {
        design::render_empty(frame, area, "No container selected.");
        return;
    };

    let inspect = app
        .containers_overview
        .inspect_cache()
        .entries
        .get(&row.id)
        .map(|e| &e.result);
    let inspect_in_flight = app
        .containers_overview
        .inspect_cache()
        .in_flight
        .contains(&row.id);
    let logs = app
        .containers_overview
        .logs_cache()
        .entries
        .get(&row.id)
        .map(|e| &e.result);
    let logs_in_flight = app
        .containers_overview
        .logs_cache()
        .in_flight
        .contains(&row.id);

    let box_width = area.width as usize;
    let top_lines = build_detail_lines(row, inspect, inspect_in_flight, spinner_tick, box_width);
    let top_height = top_lines.len() as u16;

    // Reserve LOGS_FLOOR rows at the bottom for the LOGS card so it
    // ALWAYS lands flush with the panel bottom, even when the top
    // cards would otherwise consume the whole panel. Above that
    // floor, LOGS gets whatever extra space the top cards leave so
    // a tall terminal fills with more log history.
    const LOGS_FLOOR: u16 = 7; // open + 5 visible + close
    let body_height = area.height;
    let (top_h, logs_h) = if body_height < 3 {
        (body_height, 0)
    } else if body_height < LOGS_FLOOR {
        // Tiny panel: give LOGS the minimum 3 rows, top gets the rest.
        let logs = 3.min(body_height);
        (body_height.saturating_sub(logs), logs)
    } else {
        // Cap top to body - LOGS_FLOOR. When the natural top height
        // exceeds that cap, snap down to the most recent card-close
        // boundary so we never clip mid-card and break the visual
        // structure. LOGS then gets the remainder.
        let top_max = body_height.saturating_sub(LOGS_FLOOR);
        let actual_top = if top_height > top_max {
            snap_top_to_card_boundary(&top_lines, top_max)
        } else {
            top_height
        };
        let actual_logs = body_height.saturating_sub(actual_top);
        (actual_top, actual_logs)
    };

    if top_h > 0 {
        let top_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: top_h,
        };
        frame.render_widget(Paragraph::new(top_lines), top_area);
    }
    if logs_h >= 3 {
        let logs_area = Rect {
            x: area.x,
            y: area.y.saturating_add(top_h),
            width: area.width,
            height: logs_h,
        };
        let logs_lines = build_logs_card(logs, logs_in_flight, box_width, logs_h as usize);
        frame.render_widget(Paragraph::new(logs_lines), logs_area);
    }
}

/// LOGS card sized to `card_height`. Renders the trailing log lines
/// that fit in `inner_capacity`, padded with empty rows so the close
/// border lands at the panel bottom. Loading / error / empty render a
/// single status row in the same chrome.
pub(crate) fn build_logs_card(
    logs: Option<&Result<Vec<String>, String>>,
    in_flight: bool,
    box_width: usize,
    card_height: usize,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    if card_height < 3 {
        return lines;
    }
    design::section_open(&mut lines, "LOGS", box_width);
    let inner_capacity = card_height.saturating_sub(2);

    // Render the trailing `inner_capacity` lines so a tall terminal
    // fills with history while a short panel still shows the most
    // recent rows. Padding rows below the content keep the bottom
    // border at card_height - 1.
    let mut content_rows: Vec<String> = Vec::new();
    match logs {
        Some(Ok(entries)) => {
            if entries.is_empty() {
                content_rows.push("(no output)".to_string());
            } else {
                let take = inner_capacity.min(entries.len());
                let start = entries.len().saturating_sub(take);
                for line in &entries[start..] {
                    content_rows.push(line.clone());
                }
            }
        }
        Some(Err(e)) => {
            content_rows.push(format!("error: {}", e));
        }
        None if in_flight => {
            content_rows.push("loading…".to_string());
        }
        None => {
            content_rows.push("(no logs cached)".to_string());
        }
    }

    let max_value_width = box_width.saturating_sub(4);
    for raw in content_rows.iter().take(inner_capacity) {
        let trimmed = raw.replace('\t', "    ");
        let value = if trimmed.chars().count() > max_value_width {
            crate::ui::truncate(&trimmed, max_value_width)
        } else {
            trimmed
        };
        let style = if matches!(logs, Some(Err(_))) {
            theme::error()
        } else {
            theme::muted()
        };
        design::section_line(&mut lines, vec![Span::styled(value, style)], box_width);
    }

    // Pad with empty rows so the closing border lands at card bottom.
    let used_rows = content_rows.len().min(inner_capacity);
    let padding_rows = inner_capacity.saturating_sub(used_rows);
    for _ in 0..padding_rows {
        design::section_line(&mut lines, vec![Span::raw(" ")], box_width);
    }
    design::section_close(&mut lines, box_width);
    lines
}

/// Compose the detail panel as a stack of section cards. Conditional cards
/// only materialise when they have content, mirroring host detail's
/// VAULT SSH / SNIPPETS pattern.
// Linear card composition: a flat sequence of conditional pushes onto one
// buffer. Splitting per card would fragment a single render with no gain.
#[allow(clippy::too_many_lines)]
pub(crate) fn build_detail_lines(
    row: &ContainerRow,
    inspect: Option<&Result<crate::containers::ContainerInspect, String>>,
    in_flight: bool,
    spinner_tick: u64,
    box_width: usize,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let max_value_width = box_width
        .saturating_sub(4)
        .saturating_sub(design::SECTION_LABEL_W as usize);
    let running = is_running(&row.state);

    // Header card: container name in title border, "on alias" + state line.
    design::section_open(&mut lines, &row.name, box_width);
    design::section_line(
        &mut lines,
        vec![Span::styled(format!("on {}", row.alias), theme::muted())],
        box_width,
    );

    let (glyph, glyph_style) = if running {
        (design::ICON_ONLINE, theme::online_dot_pulsing(spinner_tick))
    } else {
        (design::ICON_STOPPED, theme::muted())
    };
    let state_text = if running {
        row.status.clone()
    } else if row.status.is_empty() {
        row.state.to_lowercase()
    } else {
        row.status.clone()
    };
    let state_style = if running {
        theme::online_dot_pulsing(spinner_tick)
    } else {
        theme::muted()
    };
    design::section_line(
        &mut lines,
        vec![
            Span::styled(format!("{} ", glyph), glyph_style),
            Span::styled(state_text, state_style),
        ],
        box_width,
    );
    design::section_close(&mut lines, box_width);

    let inspect_ok = inspect.and_then(|r| r.as_ref().ok());

    // ATTENTION card: only when something demands it. Bubbles up above the
    // happy-path lifecycle card so an exited / OOM row catches the eye
    // first.
    if let Some(insp) = inspect_ok {
        let attention = insp.oom_killed || (insp.exit_code != 0 && !running);
        if attention {
            design::section_open(&mut lines, "ATTENTION", box_width);
            if insp.exit_code != 0 {
                let meaning = crate::containers::exit_code_meaning(insp.exit_code);
                let value = match meaning {
                    Some(m) => format!("{}  {}", insp.exit_code, m),
                    None => insp.exit_code.to_string(),
                };
                // exit != 0 is "be aware" (warning), OOM is "act now" (error).
                design::section_field_styled(
                    &mut lines,
                    "Exit",
                    &value,
                    theme::warning(),
                    max_value_width,
                    box_width,
                );
            }
            if insp.oom_killed {
                design::section_field_styled(
                    &mut lines,
                    "OOM",
                    "killed",
                    theme::error(),
                    max_value_width,
                    box_width,
                );
            }
            design::section_close(&mut lines, box_width);
        }
    }

    // LIFECYCLE card: restart, timestamps, pid, stop signal.
    if let Some(insp) = inspect_ok {
        let has_lifecycle = insp.restart_policy.is_some()
            || insp.restart_count > 0
            || !insp.created_at.is_empty()
            || !insp.started_at.is_empty()
            || !insp.finished_at.is_empty()
            || insp.stop_signal.is_some()
            || insp.pid.is_some();
        if has_lifecycle {
            design::section_open(&mut lines, "LIFECYCLE", box_width);
            if let Some(p) = insp.restart_policy.as_deref() {
                design::section_field(
                    &mut lines,
                    crate::i18n::text("Restart"),
                    p,
                    max_value_width,
                    box_width,
                );
            }
            // Restart count: only render when the field is meaningful (a
            // policy is set or we already saw restarts).
            let show_count = insp.restart_policy.is_some() || insp.restart_count > 0;
            if show_count {
                let style = if insp.restart_count > 0 {
                    theme::warning()
                } else {
                    theme::muted()
                };
                design::section_field_styled(
                    &mut lines,
                    "Restarts",
                    &insp.restart_count.to_string(),
                    style,
                    max_value_width,
                    box_width,
                );
            }
            if let Some(c) = format_iso_timestamp(&insp.created_at) {
                design::section_field(
                    &mut lines,
                    crate::i18n::text("Created"),
                    &c,
                    max_value_width,
                    box_width,
                );
            }
            if let Some(s) = format_iso_timestamp(&insp.started_at) {
                design::section_field(&mut lines, "Started", &s, max_value_width, box_width);
            }
            if let Some(s) = format_iso_timestamp(&insp.finished_at) {
                design::section_field(
                    &mut lines,
                    crate::i18n::text("Stopped"),
                    &s,
                    max_value_width,
                    box_width,
                );
            }
            // Stop signal: SIGTERM is the implicit docker default; only
            // surface when the image overrides it.
            if let Some(sig) = insp.stop_signal.as_deref()
                && sig != "SIGTERM"
            {
                let stop_text = match insp.stop_timeout {
                    Some(t) => format!("{} · {}s timeout", sig, t),
                    None => sig.to_string(),
                };
                design::section_field(
                    &mut lines,
                    "Stop sig",
                    &stop_text,
                    max_value_width,
                    box_width,
                );
            }
            if let Some(p) = insp.pid {
                design::section_field(
                    &mut lines,
                    "Pid",
                    &p.to_string(),
                    max_value_width,
                    box_width,
                );
            }
            // Synced: when the host's container listing was last refreshed.
            // Replaces the old list-header `synced Xm` badge so the staleness
            // signal is tied to a specific container instead of the whole tab.
            if row.cache_timestamp > 0 {
                let now = current_unix_secs();
                let age_secs = now.saturating_sub(row.cache_timestamp);
                let age_text =
                    crate::messages::relative_age(std::time::Duration::from_secs(age_secs));
                let style = if age_secs > 300 {
                    theme::warning()
                } else {
                    theme::muted()
                };
                design::section_field_styled(
                    &mut lines,
                    "Synced",
                    &age_text,
                    style,
                    max_value_width,
                    box_width,
                );
            }
            design::section_close(&mut lines, box_width);
        }
    } else if let Some(Err(e)) = inspect {
        design::section_open(&mut lines, "DETAILS", box_width);
        design::section_field_styled(
            &mut lines,
            "error",
            e,
            theme::error(),
            max_value_width,
            box_width,
        );
        design::section_close(&mut lines, box_width);
    } else if inspect.is_none() && in_flight {
        design::section_open(&mut lines, "DETAILS", box_width);
        design::section_field(
            &mut lines,
            "loading",
            "fetching inspect…",
            max_value_width,
            box_width,
        );
        design::section_close(&mut lines, box_width);
    }

    // APP card: image identity + run command. Always renders because Image
    // and ID come from the cached docker ps row, even when inspect is
    // absent.
    {
        design::section_open(&mut lines, "APP", box_width);
        design::section_field(&mut lines, "Image", &row.image, max_value_width, box_width);
        if let Some(insp) = inspect_ok {
            if let Some(v) = insp.image_version.as_deref() {
                let version_text = match insp.image_revision.as_deref() {
                    Some(r) => format!("{} · #{}", v, r),
                    None => v.to_string(),
                };
                design::section_field(
                    &mut lines,
                    "Version",
                    &version_text,
                    max_value_width,
                    box_width,
                );
            }
            if let Some(s) = insp.image_source.as_deref() {
                design::section_field(
                    &mut lines,
                    crate::i18n::text("Source"),
                    s,
                    max_value_width,
                    box_width,
                );
            }
            if let Some(d) = insp.image_digest.as_deref() {
                design::section_field(
                    &mut lines,
                    "Digest",
                    &short_digest(d),
                    max_value_width,
                    box_width,
                );
            }
        }
        design::section_field(
            &mut lines,
            "ID",
            &short_id(&row.id),
            max_value_width,
            box_width,
        );
        if let Some(insp) = inspect_ok {
            // WorkDir: drop the implicit "/" so default-rooted images
            // stay quiet. Cmd / Entrypoint moved out to its own CMD
            // card below so long commands can wrap without a label
            // column eating their width.
            if let Some(w) = insp.working_dir.as_deref()
                && w != "/"
                && !w.is_empty()
            {
                design::section_field(&mut lines, "WorkDir", w, max_value_width, box_width);
            }
        }
        design::section_close(&mut lines, box_width);
    }

    // CMD card: full-width, no label column. Cmd takes precedence
    // (it is what actually runs); Entrypoint surfaces only when Cmd
    // is absent. Wraps onto multiple lines so long pipelines stay
    // legible instead of getting truncated by a row-height budget.
    if let Some(insp) = inspect_ok {
        let cmd_text = insp
            .command
            .as_deref()
            .filter(|c| !c.is_empty())
            .map(|c| c.join(" "))
            .or_else(|| {
                insp.entrypoint
                    .as_deref()
                    .filter(|e| !e.is_empty())
                    .map(|e| e.join(" "))
            });
        if let Some(text) = cmd_text {
            design::section_open(&mut lines, "CMD", box_width);
            // Subtract one extra column so wrapped content keeps a
            // space before the right `│`. MOUNTS and LOGS already
            // breathe this way; CMD now matches.
            let wrap_width = box_width.saturating_sub(4);
            for chunk in wrap_to_lines(&text, wrap_width, 3) {
                design::section_line(
                    &mut lines,
                    vec![Span::styled(chunk, theme::muted())],
                    box_width,
                );
            }
            design::section_close(&mut lines, box_width);
        }
    }

    // HEALTH card: present when the image defines a healthcheck or the
    // runtime reports a status. Otherwise stay quiet so containers without
    // probes don't add a blank card.
    if let Some(insp) = inspect_ok {
        let render_health = insp.health.is_some()
            || insp.health_test.is_some()
            || insp.health_failing_streak.is_some();
        if render_health {
            design::section_open(&mut lines, "HEALTH", box_width);
            let (status_text, status_style) = match insp.health.as_deref() {
                Some("healthy") => ("healthy".to_string(), theme::online_dot()),
                Some("unhealthy") => ("unhealthy".to_string(), theme::error()),
                Some("starting") => ("starting".to_string(), theme::warning()),
                Some(other) => (other.to_string(), theme::muted()),
                None => ("not reporting".to_string(), theme::muted()),
            };
            design::section_field_styled(
                &mut lines,
                "Status",
                &status_text,
                status_style,
                max_value_width,
                box_width,
            );
            if let Some(test) = insp.health_test.as_deref() {
                let cmd_text = format_health_test(test);
                if !cmd_text.is_empty() {
                    design::section_field(
                        &mut lines,
                        "Check",
                        &cmd_text,
                        max_value_width,
                        box_width,
                    );
                }
            }
            if let Some(streak) = insp.health_failing_streak {
                let interval_suffix = insp
                    .health_interval_ns
                    .map(|n| format!(" · {} interval", format_duration_ns(n)))
                    .unwrap_or_default();
                let streak_text = format!("{} failing{}", streak, interval_suffix);
                let style = if streak > 0 {
                    theme::warning()
                } else {
                    theme::muted()
                };
                design::section_field_styled(
                    &mut lines,
                    "Streak",
                    &streak_text,
                    style,
                    max_value_width,
                    box_width,
                );
            }
            design::section_close(&mut lines, box_width);
        }
    }

    // SECURITY card: only when something deviates from default. Sits
    // high in the priority order so audit-relevant findings stay
    // visible even on tight panels where lower cards get clipped.
    if let Some(insp) = inspect_ok {
        let user_is_root = matches!(insp.user.as_deref(), Some("root") | Some("0") | Some("0:0"));
        let apparmor_deviates = matches!(
            insp.apparmor_profile.as_deref(),
            Some(p) if p != "docker-default" && p != "default" && !p.is_empty()
        );
        let seccomp_deviates = matches!(
            insp.seccomp_profile.as_deref(),
            Some(p) if p != "default" && !p.is_empty()
        );
        let render_security = insp.privileged
            || insp.readonly_rootfs
            || !insp.cap_add.is_empty()
            || !insp.cap_drop.is_empty()
            || apparmor_deviates
            || seccomp_deviates
            || user_is_root;
        if render_security {
            design::section_open(&mut lines, "SECURITY", box_width);
            if let Some(u) = insp.user.as_deref() {
                let user_style = if user_is_root {
                    theme::warning()
                } else {
                    theme::muted()
                };
                design::section_field_styled(
                    &mut lines,
                    crate::i18n::text("User"),
                    u,
                    user_style,
                    max_value_width,
                    box_width,
                );
            }
            if insp.privileged {
                design::section_field_styled(
                    &mut lines,
                    "Privileged",
                    "yes",
                    theme::error(),
                    max_value_width,
                    box_width,
                );
            }
            if insp.readonly_rootfs {
                design::section_field(&mut lines, "RO rootfs", "yes", max_value_width, box_width);
            }
            if !insp.cap_add.is_empty() {
                design::section_field(
                    &mut lines,
                    "Caps +",
                    &insp.cap_add.join(", "),
                    max_value_width,
                    box_width,
                );
            }
            if !insp.cap_drop.is_empty() {
                design::section_field(
                    &mut lines,
                    "Caps -",
                    &insp.cap_drop.join(", "),
                    max_value_width,
                    box_width,
                );
            }
            if apparmor_deviates && let Some(p) = insp.apparmor_profile.as_deref() {
                design::section_field(&mut lines, "AppArmor", p, max_value_width, box_width);
            }
            if seccomp_deviates && let Some(p) = insp.seccomp_profile.as_deref() {
                design::section_field(&mut lines, "Seccomp", p, max_value_width, box_width);
            }
            design::section_close(&mut lines, box_width);
        }
    }

    // RESOURCES card: only when any limit is set or the log driver
    // deviates from json-file/journald (which our `l logs` action assumes).
    if let Some(insp) = inspect_ok {
        let non_standard_log = insp
            .log_driver
            .as_deref()
            .map(|d| d != "json-file" && d != "journald")
            .unwrap_or(false);
        let has_resources = insp.memory_limit.is_some()
            || insp.cpu_limit_nanos.is_some()
            || insp.pids_limit.is_some()
            || non_standard_log;
        if has_resources {
            design::section_open(&mut lines, "RESOURCES", box_width);
            if let Some(m) = insp.memory_limit {
                design::section_field(
                    &mut lines,
                    "Memory",
                    &format_memory_bytes(m),
                    max_value_width,
                    box_width,
                );
            }
            if let Some(c) = insp.cpu_limit_nanos {
                design::section_field(
                    &mut lines,
                    "CPU",
                    &format_cpu_nanos(c),
                    max_value_width,
                    box_width,
                );
            }
            if let Some(p) = insp.pids_limit {
                design::section_field(
                    &mut lines,
                    "PIDs",
                    &p.to_string(),
                    max_value_width,
                    box_width,
                );
            }
            if non_standard_log && let Some(d) = insp.log_driver.as_deref() {
                design::section_field(
                    &mut lines,
                    crate::i18n::text("Logs"),
                    d,
                    max_value_width,
                    box_width,
                );
            }
            design::section_close(&mut lines, box_width);
        }
    }

    // NETWORK card: ROUTE-style ladder. ○ host → ● network → ◉ container,
    // with port mappings as branches. Mirrors the host-detail ROUTE
    // pattern so the network stack reads as a path, not a field list.
    {
        let inspect_has_net = inspect_ok
            .map(|i| {
                i.network_mode.is_some()
                    || i.hostname
                        .as_deref()
                        .map(|s| !s.is_empty())
                        .unwrap_or(false)
                    || !i.networks.is_empty()
            })
            .unwrap_or(false);
        let has_ports = !row.ports.trim().is_empty();
        if inspect_has_net || has_ports {
            design::section_open(&mut lines, "NETWORK", box_width);
            let mode = inspect_ok
                .and_then(|i| i.network_mode.as_deref())
                .unwrap_or("");
            let hostname = inspect_ok.and_then(|i| i.hostname.as_deref()).unwrap_or("");
            let networks: &[crate::containers::NetworkInfo] =
                inspect_ok.map(|i| i.networks.as_slice()).unwrap_or(&[]);

            // Node 1: ○ host (always)
            design::section_line(
                &mut lines,
                vec![
                    Span::styled(format!("  {} ", design::ICON_STOPPED), theme::muted()),
                    Span::styled("host", theme::muted()),
                ],
                box_width,
            );

            // Node 2: ● network (only when not host/none mode and a
            // network is attached; matches docker semantics where host
            // mode shares the host namespace and none has no network).
            let show_net_node = mode != "host" && mode != "none" && !networks.is_empty();
            if show_net_node {
                design::section_line(
                    &mut lines,
                    vec![Span::styled("  \u{250A}", theme::muted())],
                    box_width,
                );
                for net in networks {
                    let net_label = if mode.is_empty() || mode == net.name {
                        net.name.clone()
                    } else {
                        format!("{} \u{00B7} {}", mode, net.name)
                    };
                    design::section_line(
                        &mut lines,
                        vec![
                            Span::styled(format!("  {} ", design::ICON_ONLINE), theme::muted()),
                            Span::styled(net_label, theme::bold()),
                        ],
                        box_width,
                    );
                    if !net.ip_address.is_empty() {
                        design::section_line(
                            &mut lines,
                            vec![
                                Span::styled("  \u{250A}  ", theme::muted()),
                                Span::styled(net.ip_address.clone(), theme::muted()),
                            ],
                            box_width,
                        );
                    }
                }
                design::section_line(
                    &mut lines,
                    vec![Span::styled("  \u{250A}", theme::muted())],
                    box_width,
                );
            } else if mode == "host" {
                design::section_line(
                    &mut lines,
                    vec![
                        Span::styled("  \u{250A}  ", theme::muted()),
                        Span::styled("host network".to_string(), theme::muted()),
                    ],
                    box_width,
                );
            } else if mode == "none" {
                design::section_line(
                    &mut lines,
                    vec![
                        Span::styled("  \u{250A}  ", theme::muted()),
                        Span::styled("network: none".to_string(), theme::warning()),
                    ],
                    box_width,
                );
            }

            // Node 3: ◉ container
            let container_id_short = short_id(&row.id);
            let container_label = if !hostname.is_empty() && hostname != container_id_short {
                format!("{}  ({})", row.name, hostname)
            } else if !container_id_short.is_empty() {
                format!("{}  ({})", row.name, container_id_short)
            } else {
                row.name.clone()
            };
            design::section_line(
                &mut lines,
                vec![
                    Span::styled(format!("  {} ", design::ICON_TARGET), theme::accent_bold()),
                    Span::styled(container_label, theme::bold()),
                ],
                box_width,
            );

            // Port branches: filter to public bindings, dedupe IPv4/IPv6.
            if has_ports {
                let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                for raw in row.ports.split(',').map(str::trim) {
                    if raw.is_empty() {
                        continue;
                    }
                    let is_public = raw.starts_with("0.0.0.0:") || raw.starts_with("[::]:");
                    let after_host = raw
                        .strip_prefix("0.0.0.0:")
                        .or_else(|| raw.strip_prefix("[::]:"))
                        .unwrap_or(raw);
                    let port_part = after_host.split("->").next().unwrap_or(after_host);
                    let port_part = port_part.split('/').next().unwrap_or(port_part);
                    let proto = after_host
                        .split('/')
                        .nth(1)
                        .map(|p| format!("/{}", p))
                        .unwrap_or_default();
                    let key = format!(":{}{}", port_part, proto);
                    if !seen.insert(key.clone()) {
                        continue;
                    }
                    let suffix = if is_public { "  pub" } else { "" };
                    design::section_line(
                        &mut lines,
                        vec![
                            Span::styled("      \u{2192} ", theme::muted()),
                            Span::styled(key, theme::muted()),
                            Span::styled(suffix.to_string(), theme::muted()),
                        ],
                        box_width,
                    );
                }
            }
            design::section_close(&mut lines, box_width);
        }
    }

    // MOUNTS card: aligned 2-column table. host-path → container-path
    // with a trailing rw/ro mode tag. Source and dest pad to the
    // longest content per column instead of a 50/50 split, so short
    // paths do not leave a wide gap before the arrow. The mode flag
    // stays right-flush via a flex spacer between dest and mode.
    // Falls back to a 50/50 truncated split when content cannot fit.
    if let Some(insp) = inspect_ok
        && !insp.mounts.is_empty()
    {
        design::section_open(&mut lines, "MOUNTS", box_width);
        // section_line strips 3 cols for the left/right borders.
        // Subtract one more so the mode tag does not press against
        // the right `│`; other section helpers get this gap for free
        // via their right-padding, but the mounts row fills `inner`
        // exactly with its own spacer.
        let inner = box_width.saturating_sub(4);
        const ARROW: &str = " \u{2192} ";
        const ARROW_W: usize = 3;
        const MODE_W: usize = 2;
        const SEP_MIN: usize = 2;
        let source_max = insp
            .mounts
            .iter()
            .map(|m| m.source.width())
            .max()
            .unwrap_or(0);
        let dest_max = insp
            .mounts
            .iter()
            .map(|m| m.destination.width())
            .max()
            .unwrap_or(0);
        let needed = source_max + ARROW_W + dest_max + SEP_MIN + MODE_W;
        let (source_w, dest_w) = if needed <= inner {
            (source_max, dest_max)
        } else {
            let total_path = inner.saturating_sub(ARROW_W + SEP_MIN + MODE_W);
            let s = total_path / 2;
            (s, total_path.saturating_sub(s))
        };
        for m in &insp.mounts {
            let source = pad_or_truncate_path(&m.source, source_w);
            let dest = pad_or_truncate_path(&m.destination, dest_w);
            let mode = if m.read_only { "ro" } else { "rw" };
            let used = source_w + ARROW_W + dest_w + MODE_W;
            let spacer_w = inner.saturating_sub(used).max(SEP_MIN);
            design::section_line(
                &mut lines,
                vec![
                    Span::styled(source, theme::muted()),
                    Span::styled(ARROW, theme::muted()),
                    Span::styled(dest, theme::bold()),
                    Span::raw(" ".repeat(spacer_w)),
                    Span::styled(mode.to_string(), theme::muted()),
                ],
                box_width,
            );
        }
        design::section_close(&mut lines, box_width);
    }

    // COMPOSE card: only for compose-managed containers. Mirrors the
    // PROXMOX VE / VAULT SSH cards on the host detail.
    if let Some(insp) = inspect_ok
        && (insp.compose_project.is_some() || insp.compose_service.is_some())
    {
        design::section_open(&mut lines, "COMPOSE", box_width);
        if let Some(p) = insp.compose_project.as_deref() {
            design::section_field(&mut lines, "Project", p, max_value_width, box_width);
        }
        if let Some(s) = insp.compose_service.as_deref() {
            design::section_field(&mut lines, "Service", s, max_value_width, box_width);
        }
        design::section_close(&mut lines, box_width);
    }

    lines
}
