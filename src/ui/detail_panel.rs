use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::design::{
    self, BOX_BL, BOX_V, section_close, section_field, section_line, section_open,
    section_open_notitle,
};
use super::host_list::format_rtt;
use super::theme;
use crate::app::App;
use crate::history::ConnectionHistory;
use crate::ssh_config::model::ConfigElement;

const LABEL_WIDTH: usize = design::SECTION_LABEL_W as usize;

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
        has_containers: false, // requires app.container_state.cache, not testable here
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
            tag_width + 2 // ", " separator
        };
        if current_width > 0 && current_width + needed > max_width {
            rows.push(std::mem::take(&mut current_row));
            current_width = 0;
        }
        if current_width > 0 {
            current_width += 2; // ", "
        }
        current_row.push(tag);
        current_width += tag_width;
    }
    if !current_row.is_empty() {
        rows.push(current_row);
    }
    rows
}

/// Split a space-separated SSH `Host` pattern string into a list of
/// individual patterns that have each been truncated to fit in
/// `max_width`. Used by the PATTERN MATCH card to render long pattern
/// lists across multiple rows.
fn split_patterns_truncated(pattern: &str, max_width: usize) -> Vec<String> {
    pattern
        .split_whitespace()
        .map(|p| super::truncate(p, max_width))
        .collect()
}

/// Pack space-separated items into rows that fit within `max_width`
/// display columns. Each row joins its items with single-space
/// separators when rendered. Returns an empty Vec if the input is
/// empty or `max_width` is zero.
fn wrap_space_separated(items: &[String], max_width: usize) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    if max_width == 0 {
        return rows;
    }
    let mut current_row: Vec<String> = Vec::new();
    let mut current_width: usize = 0;
    for item in items {
        let item_width = UnicodeWidthStr::width(item.as_str());
        let needed = if current_width == 0 {
            item_width
        } else {
            item_width + 1 // single space separator
        };
        if current_width > 0 && current_width + needed > max_width {
            rows.push(std::mem::take(&mut current_row));
            current_width = 0;
        }
        if current_width > 0 {
            current_width += 1; // space
        }
        current_row.push(item.clone());
        current_width += item_width;
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
            // When the host list itself is empty, the TabEmpty card on
            // the left panel already explains the state. Showing
            // "Select a host to see details." in the right panel on top
            // of that re-introduces the double-message bug the design
            // system is meant to prevent — keep the detail panel quiet.
            if app.hosts_state.list().is_empty() {
                design::render_tab_empty_detail(frame, area);
            } else {
                design::render_empty(frame, area, "Select a host to see details.");
            }
            return;
        }
    };

    // box_width = area width; each section card spans the full width.
    // max_value_width = box_width - "│ " prefix (2) - " │" suffix (2) - LABEL_WIDTH
    let box_width = area.width as usize;
    let max_value_width = box_width.saturating_sub(4).saturating_sub(LABEL_WIDTH);

    let mut lines: Vec<Line<'static>> = Vec::new();

    render_header(&mut lines, app, host, box_width, spinner_tick);
    render_connection(&mut lines, app, host, box_width, max_value_width);
    render_activity(&mut lines, app, host, box_width, max_value_width);
    render_route(&mut lines, app, host, box_width);
    render_tags(&mut lines, host, box_width);
    render_provider_metadata(&mut lines, host, box_width, max_value_width);
    render_vault_cert(&mut lines, app, host, box_width, max_value_width);
    render_tunnels(&mut lines, app, host, box_width);
    render_snippets(&mut lines, app, box_width);
    render_containers(&mut lines, app, host, box_width, max_value_width);
    render_pattern_matches(&mut lines, app, host, box_width, max_value_width);
    render_source(&mut lines, host, box_width, max_value_width);

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

    let paragraph = Paragraph::new(lines).scroll((app.ui.detail_scroll(), 0));
    frame.render_widget(paragraph, area);
}

/// Renders the header card: alias title, user@host:port address, and ping status.
fn render_header(
    lines: &mut Vec<Line<'static>>,
    app: &App,
    host: &crate::ssh_config::model::HostEntry,
    box_width: usize,
    spinner_tick: u64,
) {
    section_open(lines, &host.alias.clone(), box_width);

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
        // Available width inside box: 2 cols left chrome (│ + space),
        // 2 cols right chrome (space + │) so truncate at `sub(4)` to
        // keep one column of breathing room before the right border.
        let inner = box_width.saturating_sub(4);
        let truncated = super::truncate(&addr_str, inner);
        section_line(
            lines,
            vec![Span::styled(truncated, theme::muted())],
            box_width,
        );
    }

    // Status line using dual-encoded glyphs (consistent with host list).
    // `online` pulses to match the host-list dot rhythm; other tiers use static styles.
    let mut status_spans: Vec<Span<'static>> = app
        .ping
        .status_of(&host.alias)
        .and_then(|status| {
            let glyph = crate::app::status_glyph(Some(status), spinner_tick);
            let payload = match status {
                crate::app::PingStatus::Reachable { rtt_ms } => Some((
                    format!("online ({})", format_rtt(*rtt_ms)),
                    theme::online_dot_pulsing(spinner_tick),
                )),
                crate::app::PingStatus::Slow { rtt_ms } => {
                    Some((format!("slow ({})", format_rtt(*rtt_ms)), theme::warning()))
                }
                crate::app::PingStatus::Unreachable => Some(("offline".into(), theme::error())),
                crate::app::PingStatus::Checking => Some(("checking".into(), theme::muted())),
                crate::app::PingStatus::Skipped => None,
            };
            payload.map(|(label, style)| vec![Span::styled(format!("{} {}", glyph, label), style)])
        })
        .unwrap_or_default();
    let stable = matches!(
        app.ping.status_of(&host.alias),
        Some(
            crate::app::PingStatus::Reachable { .. }
                | crate::app::PingStatus::Slow { .. }
                | crate::app::PingStatus::Unreachable
        )
    );
    if stable
        && !status_spans.is_empty()
        && let Some(t) = app.ping.last_checked_at(&host.alias)
    {
        status_spans.push(Span::styled(
            format!("  checked {}", crate::messages::relative_age(t.elapsed())),
            theme::muted(),
        ));
    }
    if !status_spans.is_empty() {
        section_line(lines, status_spans, box_width);
    }

    section_close(lines, box_width);
}

/// Renders the CONNECTION section: host, user, port, key, password, ping, stale.
fn render_connection(
    lines: &mut Vec<Line<'static>>,
    app: &App,
    host: &crate::ssh_config::model::HostEntry,
    box_width: usize,
    max_value_width: usize,
) {
    section_open(lines, crate::i18n::text("CONNECTION"), box_width);

    section_field(
        lines,
        crate::i18n::text("Host"),
        &host.hostname,
        max_value_width,
        box_width,
    );

    if !host.user.is_empty() {
        section_field(
            lines,
            crate::i18n::text("User"),
            &host.user,
            max_value_width,
            box_width,
        );
    }

    if host.port != 22 {
        section_field(
            lines,
            crate::i18n::text("Port"),
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
        section_field(lines, "Key", key_display, max_value_width, box_width);
    }

    if let Some(ref askpass) = host.askpass {
        section_field(
            lines,
            crate::i18n::text("Password"),
            password_label(askpass),
            max_value_width,
            box_width,
        );
    }

    if let Some(status) = app.ping.status_of(&host.alias) {
        let ping_text = match status {
            crate::app::PingStatus::Reachable { rtt_ms }
            | crate::app::PingStatus::Slow { rtt_ms } => format_rtt(*rtt_ms),
            crate::app::PingStatus::Unreachable => "--".to_string(),
            crate::app::PingStatus::Skipped => "-- (proxied)".to_string(),
            crate::app::PingStatus::Checking => "...".to_string(),
        };
        section_field(lines, "Ping", &ping_text, max_value_width, box_width);
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
            lines,
            vec![
                Span::styled(
                    format!("{:<width$}", "Stale", width = LABEL_WIDTH),
                    theme::muted(),
                ),
                Span::styled(display, theme::warning()),
            ],
            box_width,
        );
    }

    section_close(lines, box_width);
}

/// Renders the ACTIVITY section with connection history and a sparkline
/// auto-scaled to the oldest recorded timestamp within the chart window.
fn render_activity(
    lines: &mut Vec<Line<'static>>,
    app: &App,
    host: &crate::ssh_config::model::HostEntry,
    box_width: usize,
    max_value_width: usize,
) {
    let Some(entry) = app.history.entry(&host.alias) else {
        return;
    };

    // The sparkline chart width is the inner box content width: box_width - 4
    // ("│ " prefix = 2, " │" suffix = 2)
    let chart_width = box_width.saturating_sub(4);
    section_open(lines, "ACTIVITY", box_width);

    let ago = ConnectionHistory::format_time_ago(entry.last_connected);
    if !ago.is_empty() {
        section_field(
            lines,
            "Last SSH",
            &format!("{} ago", ago),
            max_value_width,
            box_width,
        );
    }
    section_field(
        lines,
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
        let chart_lines = super::activity_chart::render(&entry.timestamps, chart_width, now);
        if !chart_lines.is_empty() {
            section_line(lines, vec![], box_width);
            for chart_line in chart_lines {
                section_line(lines, chart_line.spans.into_iter().collect(), box_width);
            }
        }
    }

    section_close(lines, box_width);
}

/// Renders the ROUTE section showing the ProxyJump chain (or loop error).
fn render_route(
    lines: &mut Vec<Line<'static>>,
    app: &App,
    host: &crate::ssh_config::model::HostEntry,
    box_width: usize,
) {
    // Route visualisation (only when ProxyJump resolves to known hosts)
    if !host.proxy_jump.is_empty() {
        let is_loop =
            crate::ssh_config::model::proxy_jump_contains_self(&host.proxy_jump, &host.alias);
        if is_loop {
            section_open(lines, "ROUTE", box_width);
            let inner = box_width.saturating_sub(4);
            section_line(
                lines,
                vec![Span::styled("ProxyJump loop", theme::error())],
                box_width,
            );
            let fix = format!("add !{} to pattern", host.alias);
            section_line(
                lines,
                vec![Span::styled(super::truncate(&fix, inner), theme::muted())],
                box_width,
            );
            section_close(lines, box_width);
        } else {
            let chain = resolve_proxy_chain(host, app.hosts_state.list());
            if !chain.is_empty() {
                section_open(lines, "ROUTE", box_width);
                // hop_width: content width minus "  ● " prefix (4 chars)
                let hop_width = box_width.saturating_sub(4 + 4); // box borders (4) + indent+bullet (4)
                section_line(
                    lines,
                    vec![
                        Span::styled(format!("  {} ", design::ICON_STOPPED), theme::muted()),
                        Span::styled("you", theme::muted()),
                    ],
                    box_width,
                );
                for (name, hostname, in_config) in chain.iter().rev() {
                    section_line(
                        lines,
                        vec![Span::styled(
                            format!("  {}", design::ROUTE_BRANCH),
                            theme::muted(),
                        )],
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
                        lines,
                        vec![
                            Span::styled(format!("  {} ", design::ICON_ONLINE), theme::muted()),
                            Span::styled(name_trunc, name_style),
                            Span::styled(ip, theme::muted()),
                        ],
                        box_width,
                    );
                }
                section_line(
                    lines,
                    vec![Span::styled(
                        format!("  {}", design::ROUTE_BRANCH),
                        theme::muted(),
                    )],
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
                // Target host uses the fisheye glyph so the destination
                // stands apart from intermediate hops even on terminals
                // that don't honour the accent colour.
                section_line(
                    lines,
                    vec![
                        Span::styled(format!("  {} ", design::ICON_TARGET), theme::accent_bold()),
                        Span::styled(alias_trunc, theme::bold()),
                        Span::styled(target_ip, theme::muted()),
                    ],
                    box_width,
                );
                section_close(lines, box_width);
            }
        }
    }
}

/// Renders the TAGS section with provider tags, user tags and provider label.
fn render_tags(
    lines: &mut Vec<Line<'static>>,
    host: &crate::ssh_config::model::HostEntry,
    box_width: usize,
) {
    if !host.tags.is_empty() || !host.provider_tags.is_empty() || host.provider.is_some() {
        section_open(lines, crate::i18n::text("TAGS"), box_width);

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
                    spans.push(Span::styled(", ".to_string(), theme::muted()));
                }
                spans.push(Span::styled(tag.to_string(), theme::tag_user()));
            }
            section_line(lines, spans, box_width);
        }

        section_close(lines, box_width);
    }
}

/// Renders the provider metadata section (e.g. AWS, DigitalOcean fields).
fn render_provider_metadata(
    lines: &mut Vec<Line<'static>>,
    host: &crate::ssh_config::model::HostEntry,
    box_width: usize,
    max_value_width: usize,
) {
    if !host.provider_meta.is_empty() {
        let header = match host.provider.as_deref() {
            Some(name) => crate::providers::provider_display_name(name).to_uppercase(),
            None => "PROVIDER".to_string(),
        };
        section_open(lines, &header, box_width);

        for (key, value) in &host.provider_meta {
            let label = meta_label(key);
            section_field(lines, &label, value, max_value_width, box_width);
        }

        section_close(lines, box_width);
    }
}

/// Renders the VAULT SSH section showing role and certificate status.
fn render_vault_cert(
    lines: &mut Vec<Line<'static>>,
    app: &App,
    host: &crate::ssh_config::model::HostEntry,
    box_width: usize,
    max_value_width: usize,
) {
    let effective_role = crate::vault_ssh::resolve_vault_role(
        host.vault_ssh.as_deref(),
        host.provider.as_deref(),
        host.provider_label.as_deref(),
        app.providers.config(),
    );
    if let Some(ref role) = effective_role {
        section_open(lines, "VAULT SSH", box_width);

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
                lines,
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
            section_field(lines, "Role", role_name, max_value_width, box_width);
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
        let (status_text, status_style) = if app.vault.is_cert_check_in_flight(&host.alias) {
            ("Checking...".to_string(), theme::muted())
        } else if let Some((checked_at, status, _mtime)) = app.vault.cert_entry(&host.alias) {
            let elapsed = checked_at.elapsed().as_secs() as i64;
            match status {
                crate::vault_ssh::CertStatus::Valid { remaining_secs, .. } => {
                    let adjusted = remaining_secs - elapsed;
                    if adjusted <= 0 {
                        needs_action = true;
                        (crate::i18n::text("Expired").to_string(), theme::error())
                    } else {
                        let text =
                            format!("Valid ({})", crate::vault_ssh::format_remaining(adjusted));
                        (text, theme::success())
                    }
                }
                crate::vault_ssh::CertStatus::Expired => {
                    needs_action = true;
                    (crate::i18n::text("Expired").to_string(), theme::error())
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
            match crate::vault_ssh::resolve_cert_path(
                app.env().paths(),
                &host.alias,
                &host.certificate_file,
            ) {
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
        section_line(lines, status_spans, box_width);

        section_close(lines, box_width);
    }
}

/// Renders the TUNNELS section listing port-forward rules.
fn render_tunnels(
    lines: &mut Vec<Line<'static>>,
    app: &App,
    host: &crate::ssh_config::model::HostEntry,
    box_width: usize,
) {
    let tunnel_active = app.tunnels.active_contains(&host.alias);
    if host.tunnel_count > 0 {
        let tunnel_label = if tunnel_active {
            "TUNNELS (active)"
        } else {
            "TUNNELS"
        };
        section_open(lines, tunnel_label, box_width);

        let rules = find_tunnel_rules(&app.hosts_state.ssh_config().elements, &host.alias);
        let style = if tunnel_active {
            theme::success()
        } else {
            theme::muted()
        };
        let rule_content_width = box_width.saturating_sub(4);
        for rule in &rules {
            let truncated = super::truncate(rule, rule_content_width);
            section_line(lines, vec![Span::styled(truncated, style)], box_width);
        }

        section_close(lines, box_width);
    }
}

/// Renders the SNIPPETS hint when snippets are available.
fn render_snippets(lines: &mut Vec<Line<'static>>, app: &App, box_width: usize) {
    let snippet_count = app.snippets.store().snippets.len();
    if snippet_count > 0 {
        section_open(lines, "SNIPPETS", box_width);
        let msg = format!("{} available (r to run)", snippet_count);
        section_line(lines, vec![Span::styled(msg, theme::muted())], box_width);
        section_close(lines, box_width);
    }
}

/// Renders the CONTAINERS section when container cache data exists.
fn render_containers(
    lines: &mut Vec<Line<'static>>,
    app: &App,
    host: &crate::ssh_config::model::HostEntry,
    box_width: usize,
    max_value_width: usize,
) {
    if let Some(cache_entry) = app.container_state.cache_entry(&host.alias) {
        section_open(lines, "CONTAINERS", box_width);
        let running = cache_entry
            .containers
            .iter()
            .filter(|c| c.state == "running")
            .count();
        let total = cache_entry.containers.len();
        section_field(
            lines,
            "Total",
            &format!("{} running / {} total", running, total),
            max_value_width,
            box_width,
        );
        section_field(
            lines,
            "Runtime",
            cache_entry.runtime.as_str(),
            max_value_width,
            box_width,
        );
        section_field(
            lines,
            "Last checked",
            &crate::containers::format_relative_time(cache_entry.timestamp),
            max_value_width,
            box_width,
        );
        for container in &cache_entry.containers {
            // Single source of truth for the {state -> (icon, style)}
            // mapping. Keeps this surface in lockstep with the containers
            // overview and per-host overlay so a paused or dead container
            // looks the same regardless of which screen the user is on.
            let (icon, icon_style) =
                design::container_state_style(&container.state, None, "", None, 0);
            let name = crate::containers::truncate_str(
                &container.names,
                max_value_width.saturating_sub(2),
            );
            section_line(
                lines,
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
        section_close(lines, box_width);
    }
}

/// Renders PATTERN MATCH cards for inherited SSH config directives.
fn render_pattern_matches(
    lines: &mut Vec<Line<'static>>,
    app: &App,
    host: &crate::ssh_config::model::HostEntry,
    box_width: usize,
    max_value_width: usize,
) {
    // Inherited directives section — alias-only matching (SSH-faithful).
    // OpenSSH Host keyword matches only the alias typed on the command line.
    let inherited = app.hosts_state.ssh_config().matching_patterns(&host.alias);
    for pattern_entry in &inherited {
        section_open(lines, "PATTERN MATCH", box_width);
        let inner = box_width.saturating_sub(4);
        let parts = split_patterns_truncated(&pattern_entry.pattern, inner);
        let pattern_rows = wrap_space_separated(&parts, inner);
        for row in &pattern_rows {
            let mut spans: Vec<Span<'static>> = Vec::new();
            for (i, p) in row.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw(" "));
                }
                spans.push(Span::styled(p.clone(), theme::bold()));
            }
            section_line(lines, spans, box_width);
        }
        for (key, value) in &pattern_entry.directives {
            section_field(lines, key, value, max_value_width, box_width);
        }
        section_close(lines, box_width);
    }
}

/// Renders the source file card for hosts loaded via SSH config Include.
fn render_source(
    lines: &mut Vec<Line<'static>>,
    host: &crate::ssh_config::model::HostEntry,
    box_width: usize,
    max_value_width: usize,
) {
    if let Some(ref source) = host.source_file {
        section_open_notitle(lines, box_width);
        section_field(
            lines,
            crate::i18n::text("Source"),
            &source.display().to_string(),
            max_value_width,
            box_width,
        );
        section_close(lines, box_width);
    }
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

    render_pat_header(&mut lines, &pattern.pattern, box_width);
    render_pat_directives(&mut lines, &pattern.directives, box_width);
    render_pat_tags(&mut lines, &pattern.tags, box_width);
    render_pat_matches(&mut lines, &pattern.pattern, app, box_width);
    render_pat_source(&mut lines, &pattern.source_file, max_value_width, box_width);

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

    let paragraph = Paragraph::new(lines).scroll((app.ui.detail_scroll(), 0));
    frame.render_widget(paragraph, area);
}

/// Renders the PATTERN MATCH header card with wrapped pattern tokens.
fn render_pat_header(lines: &mut Vec<Line<'static>>, pattern: &str, box_width: usize) {
    section_open(lines, "PATTERN MATCH", box_width);
    let inner = box_width.saturating_sub(4);
    let parts = split_patterns_truncated(pattern, inner);
    let pattern_rows = wrap_space_separated(&parts, inner);
    for row in &pattern_rows {
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (i, p) in row.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(p.clone(), theme::bold()));
        }
        section_line(lines, spans, box_width);
    }
    section_close(lines, box_width);
}

/// Renders one card per directive category present in the block, in canonical
/// order. Skipped entirely when there are no directives; empty categories
/// produce no card. Unrecognized keywords land in the OTHER card so nothing
/// the user wrote is hidden.
fn render_pat_directives(
    lines: &mut Vec<Line<'static>>,
    directives: &[(String, String)],
    box_width: usize,
) {
    use crate::ssh_config::directive_category::DirectiveCategory;
    if directives.is_empty() {
        return;
    }
    for category in DirectiveCategory::DISPLAY_ORDER {
        let mut opened = false;
        for (key, value) in directives {
            if DirectiveCategory::for_key(key) != category {
                continue;
            }
            if !opened {
                section_open(lines, category.title(), box_width);
                opened = true;
            }
            push_directive_row(lines, key, value, box_width);
        }
        if opened {
            section_close(lines, box_width);
        }
    }
}

/// Push a directive row inside a category card: muted keyword, bold value.
/// The keyword column is at least `LABEL_WIDTH` wide so short keys align, and
/// always keeps one space before the value so long keywords never glue to it.
fn push_directive_row(lines: &mut Vec<Line<'static>>, key: &str, value: &str, box_width: usize) {
    let label_w = (key.width() + 1).max(LABEL_WIDTH);
    let value_budget = box_width.saturating_sub(4).saturating_sub(label_w);
    let display = if value_budget > 0 && value.width() > value_budget {
        super::truncate(value, value_budget)
    } else {
        value.to_string()
    };
    let spans = vec![
        Span::styled(format!("{key:<label_w$}"), theme::muted()),
        Span::styled(display, theme::bold()),
    ];
    section_line(lines, spans, box_width);
}

/// Renders the TAGS card; skipped when there are no tags.
fn render_pat_tags(lines: &mut Vec<Line<'static>>, tags: &[String], box_width: usize) {
    if tags.is_empty() {
        return;
    }
    section_open(lines, crate::i18n::text("TAGS"), box_width);
    let inner_width = box_width.saturating_sub(4);
    let tag_rows = wrap_tags(tags, inner_width);
    for row in &tag_rows {
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (i, tag) in row.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(", ".to_string(), theme::muted()));
            }
            spans.push(Span::styled(tag.to_string(), theme::tag_user()));
        }
        section_line(lines, spans, box_width);
    }
    section_close(lines, box_width);
}

/// Renders the MATCHES card listing aliases that match the pattern; skipped when empty.
fn render_pat_matches(lines: &mut Vec<Line<'static>>, pattern: &str, app: &App, box_width: usize) {
    let matching_aliases: Vec<String> = app
        .hosts_state
        .list()
        .iter()
        .filter(|h| crate::ssh_config::model::host_pattern_matches(pattern, &h.alias))
        .map(|h| h.alias.clone())
        .collect();

    if matching_aliases.is_empty() {
        return;
    }
    section_open(
        lines,
        &format!("MATCHES ({})", matching_aliases.len()),
        box_width,
    );
    let inner_width = box_width.saturating_sub(4);
    for alias in &matching_aliases {
        section_line(
            lines,
            vec![Span::styled(
                super::truncate(alias, inner_width),
                theme::bold(),
            )],
            box_width,
        );
    }
    section_close(lines, box_width);
}

/// Renders the SOURCE card showing the config file path; skipped when absent.
fn render_pat_source(
    lines: &mut Vec<Line<'static>>,
    source_file: &Option<std::path::PathBuf>,
    max_value_width: usize,
    box_width: usize,
) {
    let Some(source) = source_file else {
        return;
    };
    section_open(lines, "SOURCE", box_width);
    section_field(
        lines,
        crate::i18n::text("File"),
        &source.display().to_string(),
        max_value_width,
        box_width,
    );
    section_close(lines, box_width);
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

/// Map metadata keys to human-readable labels.
fn meta_label(key: &str) -> String {
    match key {
        "region" => "Region".to_string(),
        "zone" => "Zone".to_string(),
        "datacenter" => "Datacenter".to_string(), // legacy, pre-2.6.0
        "location" => "Location".to_string(),
        "instance" => "Instance".to_string(),
        "size" => crate::i18n::text("Size").to_string(),
        "machine" => "Machine".to_string(),
        "vm_size" => "VM Size".to_string(),
        "plan" => "Plan".to_string(),
        "specs" => "Specs".to_string(),
        "type" => crate::i18n::text("Type").to_string(),
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

/// Thin wrapper kept for the in-module tests that exercise the
/// auto-scaling sparkline. Production code now uses
/// `super::activity_chart::render` directly.
#[cfg(test)]
fn activity_sparkline(timestamps: &[u64], chart_width: usize) -> Vec<Line<'static>> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    super::activity_chart::render(timestamps, chart_width, now)
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
