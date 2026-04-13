use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::host_list::format_rtt;

// Box-drawing characters for section cards
const BOX_TL: &str = "\u{256D}"; // ╭
const BOX_TR: &str = "\u{256E}"; // ╮
const BOX_BL: &str = "\u{2570}"; // ╰
const BOX_BR: &str = "\u{256F}"; // ╯
const BOX_H: &str = "\u{2500}"; // ─
const BOX_V: &str = "\u{2502}"; // │

/// Push the opening line of a section card: ╭─ TITLE ───╮
fn section_open(lines: &mut Vec<Line<'static>>, title: &str, width: usize) {
    // prefix: "╭─ " border, then TITLE in bold, then " " — split styling
    let border_prefix = format!("{}\u{2500} ", BOX_TL);
    let title_suffix = " ";
    let prefix_width = border_prefix.width() + title.width() + title_suffix.width();
    let fill = width.saturating_sub(prefix_width).saturating_sub(1); // -1 for TR char
    lines.push(Line::from(vec![
        Span::styled(border_prefix, theme::border()),
        Span::styled(title.to_string(), theme::bold()),
        Span::styled(title_suffix, theme::border()),
        Span::styled(BOX_H.repeat(fill), theme::border()),
        Span::styled(BOX_TR, theme::border()),
    ]));
}

/// Push the opening line of a section card without a title: ╭───────╮
fn section_open_notitle(lines: &mut Vec<Line<'static>>, width: usize) {
    let fill = width.saturating_sub(2); // -1 for TL, -1 for TR
    lines.push(Line::from(vec![
        Span::styled(BOX_TL, theme::border()),
        Span::styled(BOX_H.repeat(fill), theme::border()),
        Span::styled(BOX_TR, theme::border()),
    ]));
}

/// Push a content row wrapped in box side characters: │ <spans...> │
/// Pads content to fill `width` columns (right-aligns the closing │).
fn section_line(lines: &mut Vec<Line<'static>>, spans: Vec<Span<'static>>, width: usize) {
    let mut full_spans: Vec<Span<'static>> =
        vec![Span::styled(format!("{} ", BOX_V), theme::border())];
    let content_width: usize = full_spans.iter().map(|s| s.content.width()).sum::<usize>()
        + spans.iter().map(|s| s.content.width()).sum::<usize>();
    full_spans.extend(spans);
    // Pad to align the right │ border
    let closing_offset = 1; // the │ character
    let padding = width
        .saturating_sub(content_width)
        .saturating_sub(closing_offset);
    if padding > 0 {
        full_spans.push(Span::raw(" ".repeat(padding)));
    }
    full_spans.push(Span::styled(BOX_V, theme::border()));
    lines.push(Line::from(full_spans));
}

/// Push the closing line of a section card: ╰───────╯
fn section_close(lines: &mut Vec<Line<'static>>, width: usize) {
    let fill = width.saturating_sub(2); // -1 for BL, -1 for BR
    lines.push(Line::from(vec![
        Span::styled(BOX_BL, theme::border()),
        Span::styled(BOX_H.repeat(fill), theme::border()),
        Span::styled(BOX_BR, theme::border()),
    ]));
}

/// Push a label+value field row inside a section card.
fn section_field(
    lines: &mut Vec<Line<'static>>,
    label: &str,
    value: &str,
    max_value_width: usize,
    box_width: usize,
) {
    let display = if max_value_width > 0 && value.width() > max_value_width {
        super::truncate(value, max_value_width)
    } else {
        value.to_string()
    };
    let spans = vec![
        Span::styled(
            format!("{:<width$}", label, width = LABEL_WIDTH),
            theme::muted(),
        ),
        Span::styled(display, theme::bold()),
    ];
    section_line(lines, spans, box_width);
}

use super::theme;
use crate::app::App;
use crate::history::ConnectionHistory;
use crate::ssh_config::model::ConfigElement;

const LABEL_WIDTH: usize = 14;

/// Testable detail panel data — what the detail panel will render.
/// Extracted from `App` state without requiring a `Frame`.
#[cfg(test)]
#[derive(Debug)]
#[allow(dead_code)]
pub struct DetailInfo {
    pub has_route: bool,
    pub is_proxy_loop: bool,
    pub route_hops: Vec<String>,
    pub pattern_matches: Vec<String>,
    pub pattern_proxy_jumps: Vec<(String, String)>, // (pattern, proxy_jump value)
    pub has_tags: bool,
    pub has_provider_meta: bool,
    pub has_tunnels: bool,
    pub has_containers: bool,
}

/// Compute detail panel information for a host without rendering.
#[cfg(test)]
pub fn compute_detail_info(
    host: &crate::ssh_config::model::HostEntry,
    hosts: &[crate::ssh_config::model::HostEntry],
    config: &crate::ssh_config::model::SshConfigFile,
) -> DetailInfo {
    let is_proxy_loop = !host.proxy_jump.is_empty()
        && crate::ssh_config::model::proxy_jump_contains_self(&host.proxy_jump, &host.alias);
    let chain = if is_proxy_loop {
        Vec::new()
    } else {
        resolve_proxy_chain(host, hosts)
    };
    let inherited = config.matching_patterns(&host.alias);
    DetailInfo {
        has_route: !is_proxy_loop && !host.proxy_jump.is_empty() && !chain.is_empty(),
        is_proxy_loop,
        route_hops: chain.iter().map(|(name, _, _)| name.clone()).collect(),
        pattern_matches: inherited.iter().map(|p| p.pattern.clone()).collect(),
        pattern_proxy_jumps: inherited
            .iter()
            .filter(|p| !p.proxy_jump.is_empty())
            .map(|p| (p.pattern.clone(), p.proxy_jump.clone()))
            .collect(),
        has_tags: !host.tags.is_empty()
            || !host.provider_tags.is_empty()
            || host.provider.is_some(),
        has_provider_meta: !host.provider_meta.is_empty(),
        has_tunnels: host.tunnel_count > 0,
        has_containers: false, // requires app.container_cache, not testable here
    }
}

/// Testable info for the pattern-selected detail view.
#[cfg(test)]
#[derive(Debug)]
pub struct PatternDetailInfo {
    pub matching_aliases: Vec<String>,
    pub has_directives: bool,
    pub has_tags: bool,
}

/// Compute pattern detail info without rendering.
/// Mirrors `render_pattern_detail` logic.
#[cfg(test)]
pub fn compute_pattern_detail_info(
    pattern: &crate::ssh_config::model::PatternEntry,
    hosts: &[crate::ssh_config::model::HostEntry],
) -> PatternDetailInfo {
    let matching_aliases: Vec<String> = hosts
        .iter()
        .filter(|h| crate::ssh_config::model::host_pattern_matches(&pattern.pattern, &h.alias))
        .map(|h| h.alias.clone())
        .collect();
    PatternDetailInfo {
        matching_aliases,
        has_directives: !pattern.directives.is_empty(),
        has_tags: !pattern.tags.is_empty(),
    }
}

/// Short label for a password source.
fn password_label(source: &str) -> &'static str {
    if source == "keychain" {
        "keychain"
    } else if source.starts_with("op://") {
        "1password"
    } else if source.starts_with("bw:") {
        "bitwarden"
    } else if source.starts_with("pass:") {
        "pass"
    } else if source.starts_with("vault:") {
        "vault-kv"
    } else {
        "custom"
    }
}

/// Wrap tags into rows that fit within `max_width` display columns.
/// Each row is a Vec of references into the input slice.
fn wrap_tags<'a>(tags: &'a [String], max_width: usize) -> Vec<Vec<&'a str>> {
    let mut rows: Vec<Vec<&'a str>> = Vec::new();
    let mut current_row: Vec<&'a str> = Vec::new();
    let mut current_width: usize = 0;
    for tag in tags {
        let tag_width = UnicodeWidthStr::width(tag.as_str());
        let needed = if current_width == 0 {
            tag_width
        } else {
            tag_width + 1 // space separator
        };
        if current_width > 0 && current_width + needed > max_width {
            rows.push(std::mem::take(&mut current_row));
            current_width = 0;
        }
        if current_width > 0 {
            current_width += 1; // space
        }
        current_row.push(tag);
        current_width += tag_width;
    }
    if !current_row.is_empty() {
        rows.push(current_row);
    }
    rows
}

pub fn render(frame: &mut Frame, app: &App, area: Rect, spinner_tick: u64) {
    // Check if a pattern is selected — render pattern detail instead
    if let Some(pattern) = app.selected_pattern() {
        render_pattern_detail(frame, app, area, pattern);
        return;
    }

    let host = match app.selected_host() {
        Some(h) => h,
        None => {
            let empty = Paragraph::new(" Select a host to see details.").style(theme::muted());
            frame.render_widget(empty, area);
            return;
        }
    };

    // box_width = area width; each section card spans the full width.
    // max_value_width = box_width - "│ " prefix (2) - " │" suffix (2) - LABEL_WIDTH
    let box_width = area.width as usize;
    let max_value_width = box_width.saturating_sub(4).saturating_sub(LABEL_WIDTH);

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Header card: alias as title, then user@host:port + status line
    {
        section_open(&mut lines, &host.alias.clone(), box_width);

        let user_display = host.user.as_str();
        let port_display = host.port;
        let host_addr = host.hostname.as_str();
        let addr_str = if !user_display.is_empty() && !host_addr.is_empty() {
            format!("{}@{}:{}", user_display, host_addr, port_display)
        } else if !host_addr.is_empty() {
            format!("{}:{}", host_addr, port_display)
        } else {
            String::new()
        };
        if !addr_str.is_empty() {
            // Available width inside box: box_width - 2 (│ prefix+space) - 1 (closing │)
            let inner = box_width.saturating_sub(3);
            let truncated = super::truncate(&addr_str, inner);
            section_line(
                &mut lines,
                vec![Span::styled(truncated, theme::muted())],
                box_width,
            );
        }

        // Status line using dual-encoded glyphs (consistent with host list)
        let status_spans: Vec<Span<'static>> = match app.ping_status.get(&host.alias) {
            Some(status @ crate::app::PingStatus::Reachable { rtt_ms }) => {
                vec![Span::styled(
                    format!(
                        "{} online ({})",
                        crate::app::status_glyph(Some(status), spinner_tick),
                        format_rtt(*rtt_ms)
                    ),
                    theme::success(),
                )]
            }
            Some(status @ crate::app::PingStatus::Slow { rtt_ms }) => {
                vec![Span::styled(
                    format!(
                        "{} slow ({})",
                        crate::app::status_glyph(Some(status), spinner_tick),
                        format_rtt(*rtt_ms)
                    ),
                    theme::warning(),
                )]
            }
            Some(status @ crate::app::PingStatus::Unreachable) => {
                vec![Span::styled(
                    format!(
                        "{} offline",
                        crate::app::status_glyph(Some(status), spinner_tick)
                    ),
                    theme::error(),
                )]
            }
            Some(status @ crate::app::PingStatus::Checking) => {
                vec![Span::styled(
                    format!(
                        "{} checking",
                        crate::app::status_glyph(Some(status), spinner_tick)
                    ),
                    theme::muted(),
                )]
            }
            Some(crate::app::PingStatus::Skipped) | None => vec![],
        };
        if !status_spans.is_empty() {
            section_line(&mut lines, status_spans, box_width);
        }

        section_close(&mut lines, box_width);
    }

    // Connection section
    section_open(&mut lines, "CONNECTION", box_width);

    section_field(
        &mut lines,
        "Host",
        &host.hostname,
        max_value_width,
        box_width,
    );

    if !host.user.is_empty() {
        section_field(&mut lines, "User", &host.user, max_value_width, box_width);
    }

    if host.port != 22 {
        section_field(
            &mut lines,
            "Port",
            &host.port.to_string(),
            max_value_width,
            box_width,
        );
    }

    if !host.identity_file.is_empty() {
        let key_display = host
            .identity_file
            .rsplit('/')
            .next()
            .unwrap_or(&host.identity_file);
        section_field(&mut lines, "Key", key_display, max_value_width, box_width);
    }

    if let Some(ref askpass) = host.askpass {
        section_field(
            &mut lines,
            "Password",
            password_label(askpass),
            max_value_width,
            box_width,
        );
    }

    if let Some(status) = app.ping_status.get(&host.alias) {
        let ping_text = match status {
            crate::app::PingStatus::Reachable { rtt_ms }
            | crate::app::PingStatus::Slow { rtt_ms } => format_rtt(*rtt_ms),
            crate::app::PingStatus::Unreachable => "--".to_string(),
            crate::app::PingStatus::Skipped => "-- (proxied)".to_string(),
            crate::app::PingStatus::Checking => "...".to_string(),
        };
        section_field(&mut lines, "Ping", &ping_text, max_value_width, box_width);
    }

    if let Some(stale_ts) = host.stale {
        let ago = ConnectionHistory::format_time_ago(stale_ts);
        let stale_value = if ago.is_empty() {
            "yes".to_string()
        } else {
            format!("{} ago", ago)
        };
        let display = if max_value_width > 0 {
            super::truncate(&stale_value, max_value_width)
        } else {
            stale_value
        };
        section_line(
            &mut lines,
            vec![
                Span::styled(
                    format!("{:<width$}", "Stale", width = LABEL_WIDTH),
                    theme::muted(),
                ),
                Span::styled(display, theme::error()),
            ],
            box_width,
        );
    }

    section_close(&mut lines, box_width);

    // Activity section
    let history_entry = app.history.entries.get(&host.alias);

    if history_entry.is_some() {
        // The sparkline chart width is the inner box content width: box_width - 4
        // ("│ " prefix = 2, " │" suffix = 2)
        let chart_width = box_width.saturating_sub(4);
        section_open(&mut lines, "ACTIVITY", box_width);

        if let Some(entry) = history_entry {
            let ago = ConnectionHistory::format_time_ago(entry.last_connected);
            if !ago.is_empty() {
                section_field(
                    &mut lines,
                    "Last SSH",
                    &format!("{} ago", ago),
                    max_value_width,
                    box_width,
                );
            }
            section_field(
                &mut lines,
                "Connections",
                &entry.count.to_string(),
                max_value_width,
                box_width,
            );

            if !entry.timestamps.is_empty() && chart_width >= 10 {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                // Fewer than 3 connections: show a compact text list instead of sparkline
                let recent: Vec<u64> = entry
                    .timestamps
                    .iter()
                    .copied()
                    .filter(|&t| t <= now)
                    .collect();
                if recent.len() < SPARKLINE_MIN_CONNECTIONS {
                    let labels: Vec<String> = recent
                        .iter()
                        .rev()
                        .take(4)
                        .map(|&t| {
                            let ago = ConnectionHistory::format_time_ago(t);
                            if ago.is_empty() {
                                "now".to_string()
                            } else {
                                format!("{} ago", ago)
                            }
                        })
                        .collect();
                    if !labels.is_empty() {
                        let text = labels.join(", ");
                        let truncated = super::truncate(&text, chart_width);
                        section_line(
                            &mut lines,
                            vec![Span::styled(truncated, theme::muted())],
                            box_width,
                        );
                    }
                } else {
                    let chart_lines = activity_sparkline(&entry.timestamps, chart_width);
                    if !chart_lines.is_empty() {
                        // Empty separator row inside the box
                        section_line(&mut lines, vec![], box_width);
                        for chart_line in chart_lines {
                            section_line(
                                &mut lines,
                                chart_line.spans.into_iter().collect(),
                                box_width,
                            );
                        }
                    }
                }
            }
        }

        section_close(&mut lines, box_width);
    }

    // Route visualisation (only when ProxyJump resolves to known hosts)
    if !host.proxy_jump.is_empty() {
        let is_loop =
            crate::ssh_config::model::proxy_jump_contains_self(&host.proxy_jump, &host.alias);
        if is_loop {
            section_open(&mut lines, "ROUTE", box_width);
            let inner = box_width.saturating_sub(4);
            section_line(
                &mut lines,
                vec![Span::styled("ProxyJump loop", theme::error())],
                box_width,
            );
            let fix = format!("add !{} to pattern", host.alias);
            section_line(
                &mut lines,
                vec![Span::styled(super::truncate(&fix, inner), theme::muted())],
                box_width,
            );
            section_close(&mut lines, box_width);
        } else {
            let chain = resolve_proxy_chain(host, &app.hosts);
            if !chain.is_empty() {
                section_open(&mut lines, "ROUTE", box_width);
                // hop_width: content width minus "  ● " prefix (4 chars)
                let hop_width = box_width.saturating_sub(4 + 4); // box borders (4) + indent+bullet (4)
                section_line(
                    &mut lines,
                    vec![
                        Span::styled("  \u{25CB} ", theme::muted()),
                        Span::styled("you", theme::muted()),
                    ],
                    box_width,
                );
                for (name, hostname, in_config) in chain.iter().rev() {
                    section_line(
                        &mut lines,
                        vec![Span::styled("    \u{2502}", theme::muted())],
                        box_width,
                    );
                    let name_style = if *in_config {
                        theme::bold()
                    } else {
                        theme::error()
                    };
                    let name_trunc = super::truncate(name, hop_width);
                    let remaining = hop_width.saturating_sub(name_trunc.width());
                    let ip = if *in_config && name != hostname && remaining > 4 {
                        format!(
                            "  {}",
                            super::truncate(hostname, remaining.saturating_sub(2))
                        )
                    } else {
                        String::new()
                    };
                    section_line(
                        &mut lines,
                        vec![
                            Span::styled("  \u{25CF} ", theme::muted()),
                            Span::styled(name_trunc, name_style),
                            Span::styled(ip, theme::muted()),
                        ],
                        box_width,
                    );
                }
                section_line(
                    &mut lines,
                    vec![Span::styled("    \u{2502}", theme::muted())],
                    box_width,
                );
                let alias_trunc = super::truncate(&host.alias, hop_width);
                let remaining = hop_width.saturating_sub(alias_trunc.width());
                let target_ip = if remaining > 4 {
                    format!(
                        "  {}",
                        super::truncate(&host.hostname, remaining.saturating_sub(2))
                    )
                } else {
                    String::new()
                };
                section_line(
                    &mut lines,
                    vec![
                        Span::styled("  \u{25CF} ", theme::accent()),
                        Span::styled(alias_trunc, theme::bold()),
                        Span::styled(target_ip, theme::muted()),
                    ],
                    box_width,
                );
                section_close(&mut lines, box_width);
            }
        }
    }

    // Tags section
    if !host.tags.is_empty() || !host.provider_tags.is_empty() || host.provider.is_some() {
        section_open(&mut lines, "TAGS", box_width);

        let mut all_tags: Vec<String> = host
            .provider_tags
            .iter()
            .chain(host.tags.iter())
            .cloned()
            .collect();
        if let Some(ref provider) = host.provider {
            all_tags.push(provider.clone());
        }
        // Tag rows fit within box content width: box_width - 4 ("│ " + " │")
        let tag_content_width = box_width.saturating_sub(4);
        for row in wrap_tags(&all_tags, tag_content_width) {
            let mut spans: Vec<Span<'static>> = Vec::new();
            for (i, tag) in row.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw(" "));
                }
                spans.push(Span::styled(tag.to_string(), theme::accent()));
            }
            section_line(&mut lines, spans, box_width);
        }

        section_close(&mut lines, box_width);
    }

    // Provider metadata section
    if !host.provider_meta.is_empty() {
        let header = match host.provider.as_deref() {
            Some(name) => crate::providers::provider_display_name(name).to_uppercase(),
            None => "PROVIDER".to_string(),
        };
        section_open(&mut lines, &header, box_width);

        for (key, value) in &host.provider_meta {
            let label = meta_label(key);
            section_field(&mut lines, &label, value, max_value_width, box_width);
        }

        section_close(&mut lines, box_width);
    }

    // Vault certificate section
    {
        let effective_role = crate::vault_ssh::resolve_vault_role(
            host.vault_ssh.as_deref(),
            host.provider.as_deref(),
            &app.provider_config,
        );
        if let Some(ref role) = effective_role {
            section_open(&mut lines, "VAULT SSH", box_width);

            // Show the role name (last path segment). The full mount
            // path is a config detail visible in the edit form (e).
            let role_name = role.rsplit('/').next().unwrap_or(role);
            let role_inherited = host.vault_ssh.is_none();
            if role_inherited {
                let provider_name = host.provider.as_deref().unwrap_or("provider");
                let suffix = format!(" (from {})", provider_name);
                let role_budget = max_value_width.saturating_sub(suffix.len());
                let display_role = super::truncate(role_name, role_budget);
                section_line(
                    &mut lines,
                    vec![
                        Span::styled(
                            format!("{:<width$}", "Role", width = LABEL_WIDTH),
                            theme::muted(),
                        ),
                        Span::styled(display_role, theme::bold()),
                        Span::styled(suffix, theme::muted()),
                    ],
                    box_width,
                );
            } else {
                section_field(&mut lines, "Role", role_name, max_value_width, box_width);
            }

            // Vault address is visible in the edit form (e) or provider
            // form. Showing it here wastes space (the https:// prefix
            // dominates the narrow column) and adds no actionable info.
            // Check cert status from cache, fall back to file-existence check.
            // While a signing check is in flight for this host, show "Checking...".
            // `needs_action` flags states where the user can press V to fix
            // things (missing/expired/invalid). It is consumed below to render
            // a "(press V to sign)" affordance hint next to the status text.
            let mut needs_action = false;
            let (status_text, status_style) = if app.cert_check_in_flight.contains(&host.alias) {
                ("Checking...".to_string(), theme::muted())
            } else if let Some((checked_at, status, _mtime)) =
                app.cert_status_cache.get(&host.alias)
            {
                let elapsed = checked_at.elapsed().as_secs() as i64;
                match status {
                    crate::vault_ssh::CertStatus::Valid { remaining_secs, .. } => {
                        let adjusted = remaining_secs - elapsed;
                        if adjusted <= 0 {
                            needs_action = true;
                            ("Expired".to_string(), theme::error())
                        } else {
                            let text =
                                format!("Valid ({})", crate::vault_ssh::format_remaining(adjusted));
                            (text, theme::success())
                        }
                    }
                    crate::vault_ssh::CertStatus::Expired => {
                        needs_action = true;
                        ("Expired".to_string(), theme::error())
                    }
                    crate::vault_ssh::CertStatus::Missing => {
                        needs_action = true;
                        ("Not signed".to_string(), theme::muted())
                    }
                    crate::vault_ssh::CertStatus::Invalid(_) => {
                        needs_action = true;
                        ("Invalid".to_string(), theme::error())
                    }
                }
            } else {
                // No cached status -- check file existence as fallback.
                // Any resolve error collapses to "Not signed" since the cert
                // path is unreachable in practice (alias validated upstream).
                match crate::vault_ssh::resolve_cert_path(&host.alias, &host.certificate_file) {
                    Ok(cert_path) if cert_path.exists() => ("Signed".to_string(), theme::success()),
                    _ => {
                        needs_action = true;
                        ("Not signed".to_string(), theme::muted())
                    }
                }
            };

            // Affordance hint computed during the if/else chain above. When
            // set, the user can press V to remediate the cert state.
            let mut status_spans = vec![
                Span::styled(
                    format!("{:<width$}", "Status", width = LABEL_WIDTH),
                    theme::muted(),
                ),
                Span::styled(status_text, status_style),
            ];
            if needs_action {
                status_spans.push(Span::styled(" (press V to sign)", theme::muted()));
            }
            section_line(&mut lines, status_spans, box_width);

            section_close(&mut lines, box_width);
        }
    }

    // Tunnels section
    let tunnel_active = app.active_tunnels.contains_key(&host.alias);
    if host.tunnel_count > 0 {
        let tunnel_label = if tunnel_active {
            "TUNNELS (active)"
        } else {
            "TUNNELS"
        };
        section_open(&mut lines, tunnel_label, box_width);

        let rules = find_tunnel_rules(&app.config.elements, &host.alias);
        let style = if tunnel_active {
            theme::success()
        } else {
            theme::muted()
        };
        let rule_content_width = box_width.saturating_sub(4);
        for rule in &rules {
            let truncated = super::truncate(rule, rule_content_width);
            section_line(&mut lines, vec![Span::styled(truncated, style)], box_width);
        }

        section_close(&mut lines, box_width);
    }

    // Snippets hint
    let snippet_count = app.snippet_store.snippets.len();
    if snippet_count > 0 {
        section_open(&mut lines, "SNIPPETS", box_width);
        let msg = format!("{} available (r to run)", snippet_count);
        section_line(
            &mut lines,
            vec![Span::styled(msg, theme::muted())],
            box_width,
        );
        section_close(&mut lines, box_width);
    }

    // Containers section (only shown when cache data exists)
    if let Some(cache_entry) = app.container_cache.get(&host.alias) {
        section_open(&mut lines, "CONTAINERS", box_width);
        let running = cache_entry
            .containers
            .iter()
            .filter(|c| c.state == "running")
            .count();
        let total = cache_entry.containers.len();
        section_field(
            &mut lines,
            "Total",
            &format!("{} running / {} total", running, total),
            max_value_width,
            box_width,
        );
        section_field(
            &mut lines,
            "Runtime",
            cache_entry.runtime.as_str(),
            max_value_width,
            box_width,
        );
        section_field(
            &mut lines,
            "Last checked",
            &crate::containers::format_relative_time(cache_entry.timestamp),
            max_value_width,
            box_width,
        );
        for container in &cache_entry.containers {
            let (icon, icon_style) = match container.state.as_str() {
                "running" => ("\u{2713}", theme::success()),
                "exited" | "dead" => ("\u{2717}", theme::error()),
                _ => ("\u{25cf}", theme::bold()),
            };
            let name = crate::containers::truncate_str(
                &container.names,
                max_value_width.saturating_sub(2),
            );
            section_line(
                &mut lines,
                vec![
                    Span::styled(
                        format!("{:>width$}", "", width = LABEL_WIDTH),
                        theme::muted(),
                    ),
                    Span::styled(icon, icon_style),
                    Span::styled(" ", theme::muted()),
                    Span::styled(name, theme::bold()),
                ],
                box_width,
            );
        }
        section_close(&mut lines, box_width);
    }

    // Inherited directives section — alias-only matching (SSH-faithful).
    // OpenSSH Host keyword matches only the alias typed on the command line.
    let inherited = app.config.matching_patterns(&host.alias);
    for pattern_entry in &inherited {
        section_open(&mut lines, "PATTERN MATCH", box_width);
        section_line(
            &mut lines,
            vec![Span::styled(
                super::truncate(&pattern_entry.pattern, box_width.saturating_sub(4)),
                theme::bold(),
            )],
            box_width,
        );
        for (key, value) in &pattern_entry.directives {
            section_field(&mut lines, key, value, max_value_width, box_width);
        }
        section_close(&mut lines, box_width);
    }

    // Source section (for included hosts)
    if let Some(ref source) = host.source_file {
        section_open_notitle(&mut lines, box_width);
        section_field(
            &mut lines,
            "Source",
            &source.display().to_string(),
            max_value_width,
            box_width,
        );
        section_close(&mut lines, box_width);
    }

    // Stretch: give all remaining vertical space to the last section card.
    // Insert empty bordered lines before the last section_close line.
    let available = area.height as usize;
    if lines.len() < available {
        let extra = available - lines.len();
        // Find the last section_close line (╰...╯)
        if let Some(last_close) = lines.iter().rposition(|line| {
            line.spans
                .first()
                .map(|s| s.content.starts_with(BOX_BL))
                .unwrap_or(false)
        }) {
            for _ in 0..extra {
                lines.insert(last_close, section_empty_line(box_width));
            }
        }
    }

    let paragraph = Paragraph::new(lines).scroll((app.ui.detail_scroll, 0));
    frame.render_widget(paragraph, area);
}

/// Empty bordered line for padding: │                              │
fn section_empty_line(width: usize) -> Line<'static> {
    let fill = width.saturating_sub(2);
    Line::from(vec![
        Span::styled(BOX_V, theme::border()),
        Span::raw(" ".repeat(fill)),
        Span::styled(BOX_V, theme::border()),
    ])
}

fn render_pattern_detail(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    pattern: &crate::ssh_config::model::PatternEntry,
) {
    let box_width = area.width as usize;
    let max_value_width = box_width.saturating_sub(4).saturating_sub(LABEL_WIDTH);

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Header card: PATTERN MATCH with pattern on first line
    section_open(&mut lines, "PATTERN MATCH", box_width);
    section_line(
        &mut lines,
        vec![Span::styled(pattern.pattern.clone(), theme::bold())],
        box_width,
    );
    section_close(&mut lines, box_width);

    // Directives section
    if !pattern.directives.is_empty() {
        section_open(&mut lines, "DIRECTIVES", box_width);
        for (key, value) in &pattern.directives {
            section_field(&mut lines, key, value, max_value_width, box_width);
        }
        section_close(&mut lines, box_width);
    }

    // Tags section
    if !pattern.tags.is_empty() {
        section_open(&mut lines, "TAGS", box_width);
        let tag_strings: Vec<String> = pattern.tags.to_vec();
        let inner_width = box_width.saturating_sub(4);
        let tag_rows = wrap_tags(&tag_strings, inner_width);
        for row in &tag_rows {
            section_line(
                &mut lines,
                vec![Span::styled(row.join(" "), theme::accent())],
                box_width,
            );
        }
        section_close(&mut lines, box_width);
    }

    // Matches section — alias-only matching (SSH-faithful).
    let matching_aliases: Vec<String> = app
        .hosts
        .iter()
        .filter(|h| crate::ssh_config::model::host_pattern_matches(&pattern.pattern, &h.alias))
        .map(|h| h.alias.clone())
        .collect();

    if !matching_aliases.is_empty() {
        section_open(
            &mut lines,
            &format!("MATCHES ({})", matching_aliases.len()),
            box_width,
        );
        let inner_width = box_width.saturating_sub(4);
        for alias in &matching_aliases {
            section_line(
                &mut lines,
                vec![Span::styled(
                    super::truncate(alias, inner_width),
                    theme::bold(),
                )],
                box_width,
            );
        }
        section_close(&mut lines, box_width);
    }

    // Source file
    if let Some(ref source) = pattern.source_file {
        section_open(&mut lines, "SOURCE", box_width);
        section_field(
            &mut lines,
            "File",
            &source.display().to_string(),
            max_value_width,
            box_width,
        );
        section_close(&mut lines, box_width);
    }

    // Stretch: give all remaining vertical space to the last section card.
    let available = area.height as usize;
    if lines.len() < available {
        let extra = available - lines.len();
        if let Some(last_close) = lines.iter().rposition(|line| {
            line.spans
                .first()
                .map(|s| s.content.starts_with(BOX_BL))
                .unwrap_or(false)
        }) {
            for _ in 0..extra {
                lines.insert(last_close, section_empty_line(box_width));
            }
        }
    }

    let paragraph = Paragraph::new(lines).scroll((app.ui.detail_scroll, 0));
    frame.render_widget(paragraph, area);
}

/// Resolve the ProxyJump chain for a host. Returns the list of hops from
/// the user's machine to the target: [(alias_or_name, hostname, in_config)].
/// Follows ProxyJump directives through the config (max 10 hops to prevent loops).
fn resolve_proxy_chain(
    host: &crate::ssh_config::model::HostEntry,
    hosts: &[crate::ssh_config::model::HostEntry],
) -> Vec<(String, String, bool)> {
    let mut chain = Vec::new();
    let mut current_jump = host.proxy_jump.clone();
    let mut seen = std::collections::HashSet::new();
    seen.insert(host.alias.clone()); // Prevent loops back to the target host
    for _ in 0..10 {
        if current_jump.is_empty() || current_jump.eq_ignore_ascii_case("none") {
            break;
        }
        // ProxyJump can be comma-separated for multi-hop: host1,host2
        // SSH processes them left to right (first hop first)
        let hops: Vec<&str> = current_jump.split(',').map(|s| s.trim()).collect();
        for hop_name in &hops {
            if hop_name.is_empty() {
                continue;
            }
            let name = hop_name.to_string();
            if !seen.insert(name.clone()) {
                // Loop detected
                return chain;
            }
            if let Some(jump_host) = hosts.iter().find(|h| h.alias == name) {
                chain.push((name, jump_host.hostname.clone(), true));
            } else {
                // Host not in config (external or typo)
                chain.push((name.clone(), name, false));
            }
        }
        // Follow the chain: check the last hop's ProxyJump
        let last_hop = hops.last().unwrap_or(&"");
        if let Some(next) = hosts.iter().find(|h| h.alias == *last_hop) {
            current_jump = next.proxy_jump.clone();
        } else {
            break;
        }
    }
    chain
}

/// Minimum number of connections before showing a sparkline chart.
/// Below this threshold, a compact text list is shown instead.
const SPARKLINE_MIN_CONNECTIONS: usize = 3;

/// Map metadata keys to human-readable labels.
fn meta_label(key: &str) -> String {
    match key {
        "region" => "Region".to_string(),
        "zone" => "Zone".to_string(),
        "datacenter" => "Datacenter".to_string(), // legacy, pre-2.6.0
        "location" => "Location".to_string(),
        "instance" => "Instance".to_string(),
        "size" => "Size".to_string(),
        "machine" => "Machine".to_string(),
        "vm_size" => "VM Size".to_string(),
        "plan" => "Plan".to_string(),
        "specs" => "Specs".to_string(),
        "type" => "Type".to_string(),
        "shape" => "Shape".to_string(),
        "os" => "OS".to_string(),
        "image" => "Image".to_string(),
        "status" => "State".to_string(),
        "node" => "Node".to_string(),
        other => {
            // Capitalize first letter
            let mut chars = other.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        }
    }
}

// Block sparkline using lower block elements (▁▂▃▄▅▆▇█).
// 2 rows tall = 16 height levels. Auto-scales from 5 days to 1 year.
// History retains 365 days of timestamps; chart range adapts to data age.
const BLOCKS: [char; 9] = [
    ' ', '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}',
    '\u{2588}',
];
/// Predefined time ranges for auto-scaling the sparkline.
/// The smallest range that contains the oldest timestamp is used.
/// Predefined time ranges for auto-scaling the sparkline.
/// (days, left_label, midpoint_label)
const CHART_RANGES: &[(u64, &str, &str)] = &[
    (5, "5d", "~2d"),
    (10, "10d", "~5d"),
    (14, "2w", "~1w"),
    (21, "3w", "~10d"),
    (30, "30d", "~2w"),
    (60, "2mo", "~1mo"),
    (84, "12w", "~6w"),
    (180, "6mo", "~3mo"),
    (365, "1y", "~6mo"),
];

fn activity_sparkline(timestamps: &[u64], chart_width: usize) -> Vec<Line<'static>> {
    if chart_width == 0 {
        return Vec::new();
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Auto-scale: pick the smallest range that contains the oldest timestamp
    let oldest = timestamps
        .iter()
        .copied()
        .filter(|&t| t <= now)
        .min()
        .unwrap_or(now);
    let data_age_days = now.saturating_sub(oldest) / 86400 + 1;
    let chart_days = CHART_RANGES
        .iter()
        .find(|(days, _, _)| *days >= data_age_days)
        .map(|(days, _, _)| *days)
        .unwrap_or(CHART_RANGES.last().unwrap().0);

    let range_secs = chart_days * 86400;
    let bucket_secs = range_secs as f64 / chart_width as f64;
    let cutoff = now.saturating_sub(range_secs);

    let mut buckets = vec![0u64; chart_width];
    for &ts in timestamps {
        if ts < cutoff || ts > now {
            continue;
        }
        let age = now.saturating_sub(ts);
        let idx =
            chart_width - 1 - ((age as f64 / bucket_secs).floor() as usize).min(chart_width - 1);
        buckets[idx] += 1;
    }

    if buckets.iter().all(|&v| v == 0) {
        return Vec::new();
    }

    let max_val = buckets.iter().copied().max().unwrap_or(1).max(1);
    let total_levels = 16usize; // 2 rows x 8 levels

    let heights: Vec<usize> = buckets
        .iter()
        .map(|&v| {
            if v == 0 {
                0
            } else {
                ((v as f64 / max_val as f64) * total_levels as f64).ceil() as usize
            }
        })
        .collect();

    let mut chart_lines = Vec::new();

    // Top row (only rendered if any bar exceeds half height)
    if heights.iter().any(|&h| h > 8) {
        let mut top = String::with_capacity(chart_width * 3);
        for &h in &heights {
            if h > 8 {
                top.push(BLOCKS[(h - 8).min(8)]);
            } else {
                top.push(' ');
            }
        }
        chart_lines.push(Line::from(Span::styled(top, theme::bold())));
    }

    // Bottom row with dotted baseline for empty buckets
    let mut bottom_spans: Vec<Span<'static>> = Vec::new();
    let mut run_empty = String::new();
    let mut run_filled = String::new();

    for &h in &heights {
        if h == 0 {
            if !run_filled.is_empty() {
                bottom_spans.push(Span::styled(std::mem::take(&mut run_filled), theme::bold()));
            }
            run_empty.push('\u{00B7}'); // · (middle dot)
        } else {
            if !run_empty.is_empty() {
                bottom_spans.push(Span::styled(std::mem::take(&mut run_empty), theme::muted()));
            }
            if h >= 8 {
                run_filled.push(BLOCKS[8]);
            } else {
                run_filled.push(BLOCKS[h]);
            }
        }
    }
    // Flush remaining runs
    if !run_filled.is_empty() {
        bottom_spans.push(Span::styled(run_filled, theme::bold()));
    }
    if !run_empty.is_empty() {
        bottom_spans.push(Span::styled(run_empty, theme::muted()));
    }
    chart_lines.push(Line::from(bottom_spans));

    // Axis labels: left ... midpoint ... now
    let range_entry = CHART_RANGES.iter().find(|(days, _, _)| *days == chart_days);
    let left_label = range_entry
        .map(|(_, label, _)| label.to_string())
        .unwrap_or_else(|| format!("{}d", chart_days));
    let mid_label = range_entry
        .map(|(_, _, mid)| mid.to_string())
        .unwrap_or_default();
    let right_label = "now";

    let labels_width = left_label.len() + mid_label.len() + right_label.len();
    if !mid_label.is_empty() && chart_width > labels_width + 4 {
        // Three-point axis: left ... mid ... now
        let total_gap = chart_width.saturating_sub(labels_width);
        let gap_left = total_gap / 2;
        let gap_right = total_gap - gap_left;
        chart_lines.push(Line::from(vec![
            Span::styled(left_label, theme::muted()),
            Span::raw(" ".repeat(gap_left)),
            Span::styled(mid_label, theme::muted()),
            Span::raw(" ".repeat(gap_right)),
            Span::styled(right_label.to_string(), theme::muted()),
        ]));
    } else {
        // Two-point axis (narrow panel): left ... now
        let gap = chart_width.saturating_sub(left_label.len() + right_label.len());
        chart_lines.push(Line::from(vec![
            Span::styled(left_label, theme::muted()),
            Span::raw(" ".repeat(gap)),
            Span::styled(right_label.to_string(), theme::muted()),
        ]));
    }

    chart_lines
}

fn find_tunnel_rules(elements: &[ConfigElement], alias: &str) -> Vec<String> {
    for element in elements {
        match element {
            ConfigElement::HostBlock(block) if block.host_pattern == alias => {
                return block
                    .directives
                    .iter()
                    .filter(|d| !d.is_non_directive)
                    .filter_map(|d| {
                        let prefix = match d.key.to_lowercase().as_str() {
                            "localforward" => "L",
                            "remoteforward" => "R",
                            "dynamicforward" => "D",
                            _ => return None,
                        };
                        let formatted = match d.value.split_once(char::is_whitespace) {
                            Some((src, dst)) => {
                                format!("{} {} \u{2192} {}", prefix, src, dst.trim_start())
                            }
                            None => format!("{} {}", prefix, d.value),
                        };
                        Some(formatted)
                    })
                    .collect();
            }
            ConfigElement::Include(include) => {
                for file in &include.resolved_files {
                    let result = find_tunnel_rules(&file.elements, alias);
                    if !result.is_empty() {
                        return result;
                    }
                }
            }
            _ => {}
        }
    }
    Vec::new()
}

#[cfg(test)]
#[path = "detail_panel_tests.rs"]
mod tests;
