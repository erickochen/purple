//! Tunnels-tab overview screen — option A minimal layout.
//!
//! One bordered block at the top with one row per `(host, tunnel-rule)`
//! pair. No counter row, no column header, no separate type column.
//! Each row reads: `● alias  bind/forward                 KB/s  uptime`.
//! Below the list, when there is room, a second bordered block hosts the
//! live detail panel for the selected tunnel.

use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::design;
use super::host_list;
use super::theme;
use crate::app::{App, HostState, SearchState, TunnelSortMode, TunnelState};
use crate::history::ConnectionHistory;
use crate::tunnel::{TunnelRule, TunnelType, format_uptime};

/// Top-bar height (1 inner row + 2 border rows).
const TOP_BAR_HEIGHT: u16 = 3;

/// Inter-column gap used inside a row. Four spaces gives the eye
/// enough rhythm between the bold alias and the muted forward label
/// to read them as distinct columns; two spaces does not register.
const GAP: &str = "    ";
/// Width of the highlight symbol that ratatui draws to the left of every
/// row. With `design::HOST_HIGHLIGHT` this is 1 char.
const HIGHLIGHT_W: usize = 1;
/// Marker indent that holds the highlight cursor + breathing room.
const MARKER_W: usize = 1;
/// Width of the fixed STATUS column (one dot + one trailing space).
const STATUS_W: usize = 2;
/// Minimum width for the alias column so labels never collapse on narrow
/// terminals.
const ALIAS_MIN: usize = 8;
/// Minimum width for the forward column.
const FORWARD_MIN: usize = 12;
/// Reserved width for the SPEED column, right-aligned. Fits the
/// widest realistic readout ("999.9 KB/s") plus the active-but-idle
/// "sampling…" / "idle" variants.
const SPEED_W: usize = 10;
/// Reserved width for the UPTIME column, right-aligned. Fits the
/// widest realistic readout ("23h 59m").
const UPTIME_W: usize = 7;
/// Reserved width for the LAST column. Mirrors the host-list LAST
/// column slot. Fits "now", "12m", "5h", "3d" and the "<1m"
/// special-case shorthand.
const LAST_W: usize = 6;

/// One renderable row in the tunnels overview.
struct TunnelRow {
    alias: String,
    rule: TunnelRule,
    is_active: bool,
    started_at: Option<Instant>,
    /// Combined rx+tx bytes/sec on the most recent sample. Zero when the
    /// tunnel is stopped or no sample has arrived yet.
    current_bps: u64,
    /// True when the throughput sampler has produced at least one sample
    /// for this tunnel. UI distinguishes `sampling…` vs `idle` based on it.
    throughput_ready: bool,
    /// Pre-formatted LAST cell text from `ConnectionHistory::format_time_ago`
    /// for the tunnel's host. Empty when the host has never been connected.
    last_text: String,
}

/// Single source of truth for the tunnels-overview row list. Returns
/// `(alias, rule)` pairs in the exact order they will be rendered, after
/// applying the active search filter and `TunnelSortMode`. The handler
/// uses this to resolve cursor-relative actions (edit/delete/toggle) so
/// row N under the cursor always matches row N on screen.
pub(crate) fn visible_pairs(
    search: &SearchState,
    hosts: &HostState,
    tunnels: &TunnelState,
    history: &ConnectionHistory,
) -> Vec<(String, TunnelRule)> {
    let query = search
        .query()
        .map(|q| q.to_lowercase())
        .filter(|q| !q.is_empty());
    let mut pairs: Vec<(String, TunnelRule)> = Vec::new();
    for host in hosts.list() {
        let rules = hosts.ssh_config().find_tunnel_directives(&host.alias);
        for rule in rules {
            if let Some(ref q) = query {
                let alias_match = host.alias.to_lowercase().contains(q);
                let forward_match = forward_label(&rule).to_lowercase().contains(q);
                if !alias_match && !forward_match {
                    continue;
                }
            }
            pairs.push((host.alias.clone(), rule));
        }
    }
    sort_pairs(&mut pairs, tunnels, history);
    pairs
}

/// Apply `TunnelSortMode` to `(alias, rule)` pairs. MostRecent ranks
/// active tunnels by `started_at` desc, then idle tunnels by host
/// last-connected desc. AlphaHostname sorts by alias ascending. Stable
/// sort preserves the per-host directive order within a tie.
fn sort_pairs(
    pairs: &mut [(String, TunnelRule)],
    tunnels: &TunnelState,
    history: &ConnectionHistory,
) {
    match tunnels.sort_mode() {
        TunnelSortMode::MostRecent => {
            pairs.sort_by(|a, b| {
                let a_started = tunnels.active_get(&a.0).map(|t| t.started_at);
                let b_started = tunnels.active_get(&b.0).map(|t| t.started_at);
                match (a_started, b_started) {
                    (Some(ax), Some(bx)) => bx.cmp(&ax),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => {
                        let ts_a = history.last_connected(&a.0);
                        let ts_b = history.last_connected(&b.0);
                        ts_b.cmp(&ts_a)
                            .then_with(|| a.0.to_ascii_lowercase().cmp(&b.0.to_ascii_lowercase()))
                    }
                }
            });
        }
        TunnelSortMode::AlphaHostname => {
            pairs.sort_by_key(|a| a.0.to_ascii_lowercase());
        }
    }
}

/// Decorate `visible_pairs` with active-state, `started_at`, and live
/// throughput readout for rendering.
fn build_rows(app: &App) -> Vec<TunnelRow> {
    visible_pairs(&app.search, &app.hosts_state, &app.tunnels, &app.history)
        .into_iter()
        .map(|(alias, rule)| {
            let runtime = app.tunnels.active_get(&alias);
            // In demo mode the runtime map is empty (no real ssh
            // children), so a host is "active" when it has a seeded
            // snapshot. That keeps the SPEED column live for snapshot
            // hosts and the open-dot for the rest.
            let demo_snapshot = if app.demo_mode {
                app.tunnels.demo_live_snapshots().get(&alias)
            } else {
                None
            };
            let is_active = runtime.is_some() || demo_snapshot.is_some();
            let started_at = runtime.map(|a| a.started_at).or_else(|| {
                demo_snapshot
                    .map(|s| Instant::now() - std::time::Duration::from_secs(s.uptime_secs))
            });

            let (current_bps, throughput_ready) = if let Some(s) = demo_snapshot {
                (
                    s.current_rx_bps.saturating_add(s.current_tx_bps),
                    s.throughput_ready,
                )
            } else {
                runtime
                    .map(|t| {
                        (
                            t.live.current_rx_bps.saturating_add(t.live.current_tx_bps),
                            t.live.last_throughput_at.is_some(),
                        )
                    })
                    .unwrap_or((0, false))
            };

            // Active tunnels resolve LAST to "now" — the host is being
            // talked to right now, regardless of when history last
            // recorded an SSH login. Stopped tunnels fall back to the
            // host's recorded last-connected timestamp.
            let last_text = if is_active {
                "now".to_string()
            } else {
                let ts = app.history.last_connected(&alias);
                crate::history::ConnectionHistory::format_time_ago(ts)
            };

            TunnelRow {
                is_active,
                started_at,
                alias,
                rule,
                current_bps,
                throughput_ready,
                last_text,
            }
        })
        .collect()
}

/// Render the forward / bind column. Encodes tunnel type by what the
/// arrow points at:
/// - Local:   `bind → remote`   (forwards to one specific destination)
/// - Remote:  `bind ← remote`   (the remote pushes back to a destination)
/// - Dynamic: `bind → any`      (SOCKS proxy, destination is "anywhere")
///
/// All three share the same `bind →` rhythm so the column reads as one
/// table. `any` reads honestly without conflating with shell-glob `*`;
/// the column header and the live detail panel both disambiguate the
/// SOCKS nature, so the trailing `SOCKS` label is redundant noise.
fn forward_label(rule: &TunnelRule) -> String {
    match rule.tunnel_type {
        TunnelType::Dynamic => format!("{} \u{2192} any", format_bind(rule)),
        TunnelType::Local => format!(
            "{} \u{2192} {}",
            format_bind(rule),
            format_remote(&rule.remote_host, rule.remote_port)
        ),
        TunnelType::Remote => format!(
            "{} \u{2190} {}",
            format_bind(rule),
            format_remote(&rule.remote_host, rule.remote_port)
        ),
    }
}

fn format_bind(rule: &TunnelRule) -> String {
    if rule.bind_address.is_empty() {
        rule.bind_port.to_string()
    } else if rule.bind_address.contains(':') {
        format!("[{}]:{}", rule.bind_address, rule.bind_port)
    } else {
        format!("{}:{}", rule.bind_address, rule.bind_port)
    }
}

fn format_remote(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{}]:{}", host, port)
    } else {
        format!("{}:{}", host, port)
    }
}

/// SPEED cell text + style. For active flowing tunnels: bold accent
/// throughput readout. For active idle tunnels: muted "idle" /
/// "sampling…". For stopped tunnels: muted em-dash.
fn render_speed_cell(row: &TunnelRow) -> (String, ratatui::style::Style) {
    if !row.is_active {
        return ("\u{2014}".to_string(), theme::muted());
    }
    if row.current_bps > 0 {
        return (
            super::tunnels_format::format_bps(row.current_bps),
            theme::bold(),
        );
    }
    let label = if row.throughput_ready {
        "idle"
    } else {
        "sampling\u{2026}"
    };
    (label.to_string(), theme::muted())
}

/// UPTIME cell text. Empty when stopped (the SPEED em-dash already
/// marks the row as inactive).
fn render_uptime_cell(row: &TunnelRow, now: Instant) -> String {
    if !row.is_active {
        return String::new();
    }
    match row.started_at {
        Some(start) => format_uptime(now.saturating_duration_since(start)),
        None => String::new(),
    }
}

/// Compose the right-side status text for a row. Three variants:
/// - active + flowing  → `"18.2 KB/s   1m 29s"` (bold KB/s)
/// - active + idle     → `"idle   47m"`
/// - stopped           → `"—"`
#[allow(dead_code)]
fn render_right_text(row: &TunnelRow, now: Instant) -> Vec<Span<'static>> {
    if !row.is_active {
        return vec![Span::styled("\u{2014}", theme::muted())];
    }
    let uptime = match row.started_at {
        Some(start) => format_uptime(now.saturating_duration_since(start)),
        None => String::new(),
    };
    if row.current_bps > 0 {
        let bps = super::tunnels_format::format_bps(row.current_bps);
        vec![
            Span::styled(bps, theme::bold()),
            Span::raw("   "),
            Span::styled(uptime, theme::muted()),
        ]
    } else {
        let lhs = if row.throughput_ready {
            "idle"
        } else {
            "sampling\u{2026}"
        };
        vec![
            Span::styled(lhs.to_string(), theme::muted()),
            Span::raw("   "),
            Span::styled(uptime, theme::muted()),
        ]
    }
}

/// Compute alias and forward column widths from row content, capped to
/// the available width.
struct Columns {
    alias: usize,
    forward: usize,
    flex_gap: usize,
    last: usize,
}

fn compute_columns(rows: &[TunnelRow], content_w: usize) -> Columns {
    let alias_content = rows.iter().map(|r| r.alias.width()).max().unwrap_or(0);
    let alias = alias_content.max(ALIAS_MIN);

    let forward_content = rows
        .iter()
        .map(|r| forward_label(&r.rule).width())
        .max()
        .unwrap_or(0);
    let forward = forward_content.max(FORWARD_MIN);

    let gap = GAP.width();
    let min_flex = gap;
    // LAST sizes to its content (capped at LAST_W) so the column does
    // not waste space when no host has history yet.
    let last_content = rows
        .iter()
        .map(|r| r.last_text.width())
        .max()
        .unwrap_or(0)
        .max(crate::i18n::text("LAST").width()); // header label always fits
    let last = last_content.min(LAST_W);

    // Layout:
    //   HIGHLIGHT + MARKER + STATUS + alias + gap + forward + flex
    //   + SPEED + gap + UPTIME + gap + LAST
    let fixed = HIGHLIGHT_W
        + MARKER_W
        + STATUS_W
        + alias
        + gap
        + forward
        + min_flex
        + SPEED_W
        + gap
        + UPTIME_W
        + gap
        + last;

    let (forward_final, flex_gap) = if fixed > content_w {
        let excess = fixed - content_w;
        let shrunk = forward.saturating_sub(excess).max(FORWARD_MIN);
        (shrunk, min_flex)
    } else {
        let extra = content_w - fixed;
        (forward, min_flex + extra)
    };

    Columns {
        alias,
        forward: forward_final,
        flex_gap,
        last,
    }
}

fn pad(s: &str, w: usize) -> String {
    let cur = s.width();
    if cur >= w {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(w - cur))
    }
}

/// Render the column header row above the tunnel list. Mirrors the
/// hosts list NAME / FORWARD / LAST band so the two screens scan
/// identically. A `▾` arrow next to the active sort column tells the
/// reader which order rows are in.
fn render_header(frame: &mut Frame, area: Rect, cols: &Columns, sort_mode: TunnelSortMode) {
    let style = theme::bold();
    let gap = " ".repeat(GAP.width());
    let flex = " ".repeat(cols.flex_gap);

    let alpha_sort = matches!(sort_mode, TunnelSortMode::AlphaHostname);
    let recent_sort = matches!(sort_mode, TunnelSortMode::MostRecent);

    let alias_label = if alpha_sort {
        "NAME \u{25BE}"
    } else {
        crate::i18n::text("NAME")
    };

    // Include HIGHLIGHT_W so column titles line up with row content. The List
    // widget reserves a column for highlight_symbol on each row but the header
    // is rendered as a Paragraph and must compensate manually.
    let leading_pad = " ".repeat(HIGHLIGHT_W + MARKER_W + STATUS_W);
    let mut spans = vec![
        Span::styled(leading_pad, style),
        Span::styled(
            format!("{:<width$}", alias_label, width = cols.alias),
            style,
        ),
    ];
    spans.push(Span::raw(gap.clone()));
    spans.push(Span::styled(
        format!("{:<width$}", "FORWARD", width = cols.forward),
        style,
    ));
    spans.push(Span::raw(flex));
    spans.push(Span::styled(
        format!("{:>width$}", "SPEED", width = SPEED_W),
        style,
    ));
    spans.push(Span::raw(gap.clone()));
    spans.push(Span::styled(
        format!("{:>width$}", "UPTIME", width = UPTIME_W),
        style,
    ));
    spans.push(Span::raw(gap));
    // Render LAST as separate span. The optional sort indicator is appended
    // outside cols.last to avoid overflowing the column budget. Same pattern
    // as host_list. Trailing slack on the row absorbs the extra char.
    spans.push(Span::styled(
        format!("{:>width$}", "LAST", width = cols.last),
        style,
    ));
    if recent_sort {
        spans.push(Span::styled(design::SORT_DESC, style));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Render a single tunnel row.
fn render_row<'a>(
    row: &'a TunnelRow,
    cols: &Columns,
    now: Instant,
    spinner_tick: u64,
) -> ListItem<'a> {
    // Active tunnels get the same `online_dot_pulsing` rhythm the host
    // list uses for reachable hosts — `success()` is reserved for
    // positive action outcomes, not live-state indicators.
    let (status_glyph, status_style) = if row.is_active {
        (design::ICON_ONLINE, theme::online_dot_pulsing(spinner_tick))
    } else {
        (design::ICON_STOPPED, theme::muted())
    };

    let (speed_text, speed_style) = render_speed_cell(row);
    let uptime_text = render_uptime_cell(row, now);

    let last_display = if row.last_text.is_empty() {
        "\u{2014}".to_string()
    } else {
        row.last_text.clone()
    };
    let last_style = if row.is_active {
        theme::online_dot_pulsing(spinner_tick)
    } else {
        theme::muted()
    };

    let spans: Vec<Span<'static>> = vec![
        // MARKER_W spaces sit between ratatui's highlight symbol (rendered
        // automatically to the left of the row) and the status dot.
        Span::raw(" ".repeat(MARKER_W)),
        Span::styled(format!("{} ", status_glyph), status_style),
        Span::styled(pad(&row.alias, cols.alias), theme::bold()),
        Span::raw(GAP),
        Span::styled(pad(&forward_label(&row.rule), cols.forward), theme::muted()),
        Span::raw(" ".repeat(cols.flex_gap)),
        Span::styled(
            format!("{:>width$}", speed_text, width = SPEED_W),
            speed_style,
        ),
        Span::raw(GAP),
        Span::styled(
            format!("{:>width$}", uptime_text, width = UPTIME_W),
            theme::muted(),
        ),
        Span::raw(GAP),
        Span::styled(
            format!("{:>width$}", last_display, width = cols.last),
            last_style,
        ),
    ];
    ListItem::new(Line::from(spans))
}

/// Render the tunnels overview screen.
pub fn render(frame: &mut Frame, app: &mut App, anim: &mut crate::animation::AnimationState) {
    let spinner_tick = anim.spinner_tick;
    let area = frame.area();

    let search_active = app.search.query().is_some();
    let search_bar_h = if search_active { 1 } else { 0 };
    // Search bar sits between the body and the footer, mirroring the
    // host-list layout. TUI convention (vim/less/fzf/lazygit/htop) puts
    // `/` input at the bottom so the list keeps maximal vertical room
    // and pressing `/` doesn't push the list rows down a line.
    let [top_bar_area, body_area, search_bar_area, footer_area] = Layout::vertical([
        Constraint::Length(TOP_BAR_HEIGHT),
        Constraint::Min(0),
        Constraint::Length(search_bar_h),
        Constraint::Length(1),
    ])
    .areas(area);
    render_top_bar(frame, app, top_bar_area);

    let rows = build_rows(app);
    let row_count = rows.len();
    if search_active {
        render_search_bar(frame, app, search_bar_area, row_count);
    }

    // Clamp the cursor before the panel-height calculation needs it.
    let sel = app.ui.tunnels_overview_state().selected();
    let new_sel = match sel {
        Some(i) if i < row_count => Some(i),
        _ if row_count > 0 => Some(0),
        _ => None,
    };
    if new_sel != sel {
        app.ui.tunnels_overview_state_mut().select(new_sel);
    }

    let target_panel_height = panel_stretch_height(body_area, &rows);
    let panel_visible_target = is_selected_tunnel_active(app, &rows);
    anim.note_tunnel_panel_target(panel_visible_target);
    let panel_progress = anim.tunnel_panel_anim_progress();
    let (panel_visible, panel_height) = match panel_progress {
        Some(p) => {
            let h = ((target_panel_height as f32 * p).round() as u16).max(1);
            (true, h)
        }
        None => {
            if panel_visible_target {
                (true, target_panel_height)
            } else {
                (false, 0)
            }
        }
    };
    let (list_block_area, dashboard_area) = partition_body(body_area, panel_visible, panel_height);

    // Mirror the host-list update badge on the tunnels-tab list card so
    // the "new version available" affordance is tab-independent — users
    // see it in the same visual slot regardless of which tab is active.
    let update_title = app.update.available().map(|ver| {
        let label = host_list::build_update_label(
            ver,
            app.update.headline(),
            app.update.hint(),
            list_block_area.width,
        );
        Line::from(Span::styled(label, theme::update_badge()))
    });

    // While search is active swap the muted main border for the
    // accent-coloured search border and surface a `search: N/total`
    // header — same visual cue the host-list uses, so the colour
    // change is tab-independent.
    let url_label = Line::from(Span::styled(" getpurple.sh ", theme::muted()));
    let mut block = if search_active {
        let total: usize = app
            .hosts_state
            .list()
            .iter()
            .map(|h| {
                app.hosts_state
                    .ssh_config()
                    .find_tunnel_directives(&h.alias)
                    .len()
            })
            .sum();
        let title = Line::from(vec![Span::styled(
            format!(" search: {}/{} ", row_count, total),
            theme::bold(),
        )]);
        design::search_block_line(title).title_bottom(url_label.right_aligned())
    } else {
        design::main_block_line(Line::default()).title_bottom(url_label.right_aligned())
    };
    if let Some(update) = update_title.as_ref() {
        block = block.title_top(update.clone().right_aligned());
    }
    let block_inner = block.inner(list_block_area);
    frame.render_widget(block, list_block_area);

    if rows.is_empty() {
        let hints = [("a", crate::messages::TAB_EMPTY_TUNNELS_HINT_ADD)];
        let explainer = crate::messages::tab_empty_tunnels_explainer(&app.config_path_display());
        let empty = design::TabEmpty {
            card_title: crate::i18n::text("Tunnels"),
            headline: crate::i18n::text(crate::messages::TAB_EMPTY_TUNNELS_HEADLINE),
            explainer: &explainer,
            hints: &hints,
        };
        design::render_tab_empty(frame, list_block_area, &empty);
        render_footer(frame, footer_area, app, &rows);
        return;
    }

    // Inset the content one column on each side so the LAST cell does
    // not kiss the rounded border. The list, header and underline all
    // render inside this padded rect.
    let inner = Rect {
        x: block_inner.x.saturating_add(1),
        y: block_inner.y,
        width: block_inner.width.saturating_sub(2),
        height: block_inner.height,
    };

    // Inner layout mirrors the hosts list: header band, underline
    // rule, then the list body. No top blank so the header sits flush
    // under the rounded border, matching the hosts list rhythm.
    let [header_area, underline_area, list_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(inner);

    // Subtract 1 left margin (highlight column) so columns do not butt
    // against the rounded border on either side. The right margin is
    // already absorbed by the inset above.
    let content_w = (inner.width as usize).saturating_sub(1);
    let cols = compute_columns(&rows, content_w);

    render_header(frame, header_area, &cols, app.tunnels.sort_mode());
    frame.render_widget(
        Paragraph::new(Span::styled(
            "\u{2500}".repeat(underline_area.width as usize),
            theme::muted(),
        )),
        underline_area,
    );

    let now = Instant::now();
    let items: Vec<ListItem> = rows
        .iter()
        .map(|r| render_row(r, &cols, now, spinner_tick))
        .collect();
    let list = List::new(items)
        .highlight_style(theme::selected_row())
        .highlight_symbol(design::HOST_HIGHLIGHT);
    frame.render_stateful_widget(list, list_area, app.ui.tunnels_overview_state_mut());

    if let Some(dash) = dashboard_area {
        if panel_visible_target {
            super::tunnels_detail::render(frame, app, dash, spinner_tick);
        } else {
            // Closing animation: render an empty bordered block while
            // the panel slides out. The tunnel is already stopped so
            // there is no content to show.
            let block = design::main_block_line(Line::default());
            frame.render_widget(block, dash);
        }
    }

    render_footer(frame, footer_area, app, &rows);
}

/// Minimum number of rows the tunnel list block needs to remain
/// readable: 2 borders plus chrome leaves zero list rows at this floor,
/// but the panel never claims rows below it so the borders stay intact
/// even on tiny terminals.
const LIST_MIN_HEIGHT: u16 = 4;

/// Inner chrome the list block always carries: 1 header row, 1
/// underline row, and 2 border rows. Total list height is
/// `rows + LIST_CHROME_ROWS`. There is no inner top blank (the header
/// sits flush under the rounded border, matching the hosts list) and
/// no inner bottom blank (the rounded border is enough breathing room).
const LIST_CHROME_ROWS: u16 = 4;

/// Visual gap rendered between the tunnels list block and the live
/// detail block. Set to 0 so the two cards sit flush against each
/// other — same rhythm as the tab bar against the list block. Both
/// blocks carry their own rounded border, which doubles as the
/// visual separator without an extra blank row.
const PANEL_GAP: u16 = 0;

/// Compute the panel height in stretch mode. The list keeps exactly
/// what its rows need (`rows + LIST_CHROME_ROWS`) and the panel
/// absorbs everything else, minus the visual gap between blocks.
/// Below `LIST_MIN_HEIGHT` rows the formula floors so a sparse list
/// does not yield a microscopic panel either.
fn panel_stretch_height(body_area: Rect, rows: &[TunnelRow]) -> u16 {
    let needed_list = (rows.len() as u16)
        .saturating_add(LIST_CHROME_ROWS)
        .max(LIST_MIN_HEIGHT);
    body_area
        .height
        .saturating_sub(needed_list)
        .saturating_sub(PANEL_GAP)
}

/// Partition the body area into the tunnel list block and the live
/// detail block, with a one-row visual gap between them. The detail
/// block is skipped when no tunnel is active or when the body is too
/// short. `panel_height` is the stretch target from
/// `panel_stretch_height`; `partition_body` only enforces the
/// minimum-list and minimum-panel floors.
fn partition_body(area: Rect, panel_visible: bool, panel_height: u16) -> (Rect, Option<Rect>) {
    use super::tunnels_detail::DASHBOARD_MIN_BODY_HEIGHT;
    if !panel_visible || area.height < DASHBOARD_MIN_BODY_HEIGHT {
        return (area, None);
    }
    let height = panel_height.min(area.height.saturating_sub(LIST_MIN_HEIGHT + PANEL_GAP));
    if height < 3 {
        return (area, None);
    }
    let [list_area, _gap, dashboard] = Layout::vertical([
        Constraint::Min(LIST_MIN_HEIGHT),
        Constraint::Length(PANEL_GAP),
        Constraint::Length(height),
    ])
    .areas(area);
    (list_area, Some(dashboard))
}

/// True when the selected tunnel row is active (running). Demo-mode
/// snapshots count as active so the detail panel still renders for
/// the marketing demo.
fn is_selected_tunnel_active(app: &App, rows: &[TunnelRow]) -> bool {
    let sel = app.ui.tunnels_overview_state().selected();
    let alias = match sel.and_then(|i| rows.get(i)).map(|r| r.alias.clone()) {
        Some(a) => a,
        None => return false,
    };
    if app.demo_mode {
        return app.tunnels.demo_live_snapshots().contains_key(&alias);
    }
    app.tunnels
        .active_get(&alias)
        .map(|t| t.live.last_exit.is_none())
        .unwrap_or(false)
}

fn render_top_bar(frame: &mut Frame, app: &App, area: Rect) {
    let block = design::main_block_line(Line::default());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let content_area = Rect::new(
        inner.x.saturating_add(1),
        inner.y,
        inner.width.saturating_sub(1),
        1,
    );
    let line = Line::from(host_list::top_bar_spans(app));
    frame.render_widget(Paragraph::new(line), content_area);
}

fn render_search_bar(frame: &mut Frame, app: &App, area: Rect, visible_count: usize) {
    let query = app.search.query().unwrap_or("");
    let total: usize = app
        .hosts_state
        .list()
        .iter()
        .map(|h| {
            app.hosts_state
                .ssh_config()
                .find_tunnel_directives(&h.alias)
                .len()
        })
        .sum();
    let match_info = if query.is_empty() {
        String::new()
    } else {
        format!(" ({} of {})", visible_count, total)
    };
    let line = Line::from(vec![
        Span::styled(" / ", theme::brand_badge()),
        Span::raw(" "),
        Span::raw(query.to_string()),
        Span::styled("_", theme::accent()),
        Span::styled(match_info, theme::muted()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_footer(frame: &mut Frame, area: Rect, app: &mut App, rows: &[TunnelRow]) {
    if app.tunnels.pending_delete().is_some() {
        // Destructive: render the centred popup so the affordance
        // matches host delete and the other danger confirms instead of
        // a footer prompt under the overview.
        let detail = crate::messages::confirm_tunnel_delete_detail(&app.config_path_display());
        design::render_destructive_popup(
            frame,
            crate::i18n::text(crate::messages::CONFIRM_TUNNEL_DELETE_TITLE),
            crate::i18n::text(crate::messages::CONFIRM_TUNNEL_DELETE_QUESTION),
            &detail,
            "delete",
            "keep",
            app,
        );
        // Still render an empty footer so the row below the block does
        // not bleed the parent screen's footer through the cleared row.
        super::render_footer_with_status(frame, area, Vec::new(), app);
        return;
    }

    let row_active = app
        .ui
        .tunnels_overview_state()
        .selected()
        .and_then(|i| rows.get(i))
        .map(|r| r.is_active)
        .unwrap_or(false);
    let primary_label = if row_active { " stop " } else { " start " };

    // The detail-view toggle (`v compact`/`v detail`) is a host-list
    // affordance — there is no compact tunnels variant. Drop it from
    // this footer so the keys advertised here are the ones that
    // actually do something on the tunnels overview.
    use crate::messages::footer as fl;
    let spans = design::Footer::new()
        .primary("Enter", primary_label)
        .action("/", crate::i18n::text(fl::ACTION_SEARCH))
        .action("s", crate::i18n::text(fl::ACTION_SORT))
        .action(":", crate::i18n::text(fl::ACTION_JUMP))
        .into_spans();
    super::render_footer_with_help(frame, area, spans, app);
}

#[cfg(test)]
mod tests {
    use super::super::tunnels_detail::DASHBOARD_MIN_BODY_HEIGHT;
    use super::*;

    fn area(width: u16, height: u16) -> Rect {
        Rect::new(0, 0, width, height)
    }

    fn make_row(alias: &str) -> TunnelRow {
        TunnelRow {
            alias: alias.to_string(),
            rule: TunnelRule {
                tunnel_type: TunnelType::Dynamic,
                bind_address: String::new(),
                bind_port: 8080,
                remote_host: String::new(),
                remote_port: 0,
            },
            last_text: "now".to_string(),
            is_active: true,
            started_at: None,
            current_bps: 0,
            throughput_ready: false,
        }
    }

    /// On terminals shorter than `DASHBOARD_MIN_BODY_HEIGHT`, the
    /// dashboard is suppressed and the list keeps the full body area.
    // The tunnel delete confirm names the config file it rewrites.
    #[test]
    fn delete_confirm_names_the_config_file() {
        let mut app = crate::ui::app_with_own_config(
            "Host fwd\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n",
        );
        app.top_page = crate::app::TopPage::Tunnels;
        app.ui.tunnels_overview_state_mut().select(Some(0));
        app.tunnels.request_delete(0);
        let mut anim = crate::animation::AnimationState::default();
        let text = crate::ui::render_to_text(120, 40, |frame| {
            crate::ui::render(frame, &mut app, &mut anim)
        });
        assert!(
            text.contains("~/.ssh/purple_config"),
            "delete confirm must name the config file: {text}"
        );
        assert!(
            !text.contains("~/.ssh/config"),
            "delete confirm must not name the default: {text}"
        );
    }

    #[test]
    fn partition_skips_dashboard_when_body_short() {
        let body = area(120, DASHBOARD_MIN_BODY_HEIGHT - 1);
        let rows = vec![make_row("a")];
        let target = panel_stretch_height(body, &rows);
        let (list, dash) = partition_body(body, true, target);
        assert_eq!(list.height, body.height);
        assert!(dash.is_none());
    }

    /// `panel_visible = false` keeps the list on the full body height,
    /// no matter what the stretch helper would otherwise hand out.
    #[test]
    fn partition_skips_dashboard_when_panel_invisible() {
        let (list, dash) = partition_body(area(120, 30), false, 25);
        assert_eq!(list.height, 30);
        assert!(dash.is_none());
    }

    /// On tall terminals the panel absorbs everything beyond the list
    /// minimum so the list stays as compact as its row count allows.
    /// The visual gap between blocks subtracts one extra row from the
    /// panel total.
    #[test]
    fn stretch_panel_fills_remaining_body() {
        let body = area(120, 60);
        let rows = vec![make_row("a"), make_row("b"), make_row("c")];
        let target = panel_stretch_height(body, &rows);
        let expected_panel = 60 - 3 - LIST_CHROME_ROWS - PANEL_GAP;
        assert_eq!(target, expected_panel);
        let (list, dash) = partition_body(body, true, target);
        let dash = dash.expect("dashboard rendered when tall");
        assert_eq!(list.height + dash.height + PANEL_GAP, 60);
        assert!(list.height >= LIST_MIN_HEIGHT);
    }

    /// At the minimum body threshold the panel takes a clamped height
    /// and the list keeps the floor minimum. The gap row is included
    /// in the body partition.
    #[test]
    fn partition_renders_dashboard_at_threshold() {
        // Body needs room for: list chrome + gap + panel min + dashboard threshold.
        let body = area(
            120,
            (DASHBOARD_MIN_BODY_HEIGHT + LIST_CHROME_ROWS + PANEL_GAP).max(13),
        );
        let rows = vec![make_row("a")];
        let target = panel_stretch_height(body, &rows);
        let (list, dash) = partition_body(body, true, target);
        let dash = dash.expect("dashboard rendered at threshold");
        assert_eq!(list.height + dash.height + PANEL_GAP, body.height);
        assert!(list.height >= LIST_MIN_HEIGHT);
    }

    /// Forward labels encode the tunnel type by punctuation so the
    /// renderer can drop the explicit type column.
    #[test]
    fn forward_label_dynamic_uses_any_remote() {
        let r = TunnelRule {
            tunnel_type: TunnelType::Dynamic,
            bind_address: String::new(),
            bind_port: 8080,
            remote_host: String::new(),
            remote_port: 0,
        };
        assert_eq!(forward_label(&r), "8080 \u{2192} any");
    }

    #[test]
    fn forward_label_local_uses_right_arrow() {
        let r = TunnelRule {
            tunnel_type: TunnelType::Local,
            bind_address: String::new(),
            bind_port: 5432,
            remote_host: "10.40.0.20".to_string(),
            remote_port: 5432,
        };
        assert!(forward_label(&r).contains('\u{2192}'));
    }

    #[test]
    fn forward_label_remote_uses_left_arrow() {
        let r = TunnelRule {
            tunnel_type: TunnelType::Remote,
            bind_address: String::new(),
            bind_port: 9000,
            remote_host: "10.40.0.20".to_string(),
            remote_port: 9000,
        };
        assert!(forward_label(&r).contains('\u{2190}'));
    }
}
