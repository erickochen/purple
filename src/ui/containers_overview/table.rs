use super::*;

/// Column-width of `GAP`. Hand-paired with the literal so callers do
/// not pay a `UnicodeWidthStr::width()` call per row per frame.
pub(crate) const GAP_W: usize = 4;

pub(crate) const HIGHLIGHT_W: usize = 1;

pub(crate) const MARKER_W: usize = 1;

pub(crate) const STATUS_DOT_W: usize = 2;

pub(crate) const HOST_MIN: usize = 8;

pub(crate) const NAME_MIN: usize = 8;

pub(crate) const IMAGE_MIN: usize = 12;

/// Width of the UPTIME column. Holds compact labels produced by
/// `parse_uptime_from_status` (`5w`, `12d`, `<1m`, `3mo`) plus a
/// safety margin.
pub(crate) const UPTIME_W: usize = 8;

pub(crate) struct Columns {
    pub(crate) host: usize,
    pub(crate) name: usize,
    pub(crate) image: usize,
    /// Render the UPTIME column? `false` when the available width cannot
    /// fit it even with IMAGE shrunk to its minimum.
    pub(crate) show_uptime: bool,
    /// Render the HOST column? `false` in `AlphaHost` mode where the
    /// host alias already lives on the divider line above each
    /// group; suppressing the column reclaims ~16-30 cols for IMAGE
    /// and avoids the visual repetition the user flagged on review.
    pub(crate) show_host: bool,
}

/// `rows` only needs to be walked for `width()` of three string
/// fields per row; we accept anything that yields `&ContainerRow` so
/// callers can pass a `Vec<&ContainerRow>` (zero-copy from the items
/// list) instead of cloning every container per render tick.
pub(crate) fn compute_columns<'a, I>(rows: I, content_w: usize, show_host: bool) -> Columns
where
    I: IntoIterator<Item = &'a ContainerRow> + Clone,
{
    let host_content = rows
        .clone()
        .into_iter()
        .map(|r| r.alias.width())
        .max()
        .unwrap_or(0);
    let host = if show_host {
        host_content.max(HOST_MIN)
    } else {
        0
    };

    let name_content = rows
        .clone()
        .into_iter()
        .map(|r| r.name.width())
        .max()
        .unwrap_or(0);
    let name = name_content.max(NAME_MIN);

    let image_content = rows.into_iter().map(|r| r.image.width()).max().unwrap_or(0);
    let image_max = image_content.max(IMAGE_MIN);

    let host_segment = if show_host { host + GAP_W } else { 0 };
    let always_on_with_image =
        |image: usize| HIGHLIGHT_W + MARKER_W + STATUS_DOT_W + host_segment + name + GAP_W + image;

    // STATUS and HEALTH columns no longer exist. The per-row glyph
    // encodes both signals via colour and shape. PORTS was demoted to
    // detail-panel-only because it was empty for most rows.
    let with_uptime_min = always_on_with_image(IMAGE_MIN) + GAP_W + UPTIME_W;
    let show_uptime = content_w >= with_uptime_min;

    // IMAGE is the flex column. When the row needs more width than
    // available, shrink IMAGE toward IMAGE_MIN. When there is surplus
    // and UPTIME is on, expand IMAGE to absorb it so UPTIME anchors
    // to the right edge (mirrors host_list's flex_gap pattern; without
    // this, collapsing the detail panel leaves an empty column to the
    // right of IMAGE while UPTIME drifts left of its natural anchor).
    let total_max =
        always_on_with_image(image_max) + if show_uptime { GAP_W + UPTIME_W } else { 0 };
    let image = if total_max > content_w {
        let excess = total_max - content_w;
        image_max.saturating_sub(excess).max(IMAGE_MIN)
    } else if show_uptime {
        let consumed = always_on_with_image(0) + GAP_W + UPTIME_W;
        content_w.saturating_sub(consumed).max(image_max)
    } else {
        image_max
    };

    Columns {
        host,
        name,
        image,
        show_uptime,
        show_host,
    }
}

/// Pad-or-truncate to exactly `w` columns. `compute_columns` already
/// sizes alias/name to the longest content so they always pad rather
/// than truncate, but a future caller (or a config reload mid-render)
/// could feed in a longer string; falling through to `crate::ui::truncate`
/// keeps the row from overflowing into the next column instead of
/// silently corrupting the layout.
pub(crate) fn pad_or_truncate(s: &str, w: usize) -> String {
    let cur = s.width();
    match cur.cmp(&w) {
        std::cmp::Ordering::Equal => s.to_string(),
        std::cmp::Ordering::Less => format!("{}{}", s, " ".repeat(w - cur)),
        std::cmp::Ordering::Greater => {
            let truncated = crate::ui::truncate(s, w);
            // `truncate` may produce slightly fewer cols than `w` when
            // it appends `…`; pad the remainder so columns line up.
            let tw = truncated.width();
            if tw < w {
                format!("{}{}", truncated, " ".repeat(w - tw))
            } else {
                truncated
            }
        }
    }
}

/// Index of the first visible item, header or container. Used when
/// snapping the cursor on first paint or after a sort flip. With
/// header rows now selectable the cursor is allowed to land on a
/// divider, so this is the right entry point for "first row".
pub(crate) fn first_visible_index(items: &[ContainerListItem]) -> Option<usize> {
    if items.is_empty() { None } else { Some(0) }
}

/// Resolve a (alias, container_id) pair to its index in the currently
/// rendered visible_items list. Returns `None` when the container is
/// not in the cache, its host group is folded, or it has been filtered
/// out by an active search query. Used by the jump dispatcher to land
/// the cursor on the picked container without leaving the tab.
pub(crate) fn position_of_container(app: &App, alias: &str, container_id: &str) -> Option<usize> {
    visible_items(app).iter().position(|item| match item {
        ContainerListItem::Container(row) => row.alias == alias && row.id == container_id,
        _ => false,
    })
}

/// Build the block title. Per-state breakdowns (running/exited) live on
/// the per-host group dividers (`(3 of 4 running)`); repeating them in
/// the top header was redundant noise. The header mirrors the host_list
/// pattern: bare visible count. The active tab in the top bar already
/// disambiguates scope (containers vs hosts vs tunnels), so labelling
/// the count here would be redundant.
pub(crate) fn build_stats_title(container_count: usize) -> Line<'static> {
    Line::from(vec![Span::styled(
        format!(" {} ", container_count),
        theme::bold(),
    )])
}

/// Render a `── alias (N running) ──────────` divider for one host
/// group. Mirrors `HostListItem::GroupHeader` styling: bold prefix,
/// muted dashes filling the row. Folded groups switch the suffix to
/// `(N hidden)` so the user sees that the children are suppressed,
/// not lost.
pub(crate) fn render_host_header_row<'a>(alias: &'a str, content_w: usize) -> ListItem<'a> {
    // Counts (running / total / hidden) used to live in the header but
    // duplicated information already visible: the per-row dots show
    // running vs not, and the host detail panel summarises the fleet.
    // Header now carries the alias only.
    let prefix = format!("── {} ", alias);
    // Subtract 1 for ratatui's highlight_symbol gutter (rendered to
    // the left of every list item). Same accounting as the host-list
    // group-header row.
    let available = content_w.saturating_sub(1);
    let fill_width = available.saturating_sub(prefix.width());
    ListItem::new(Line::from(vec![
        Span::styled(prefix, theme::bold()),
        Span::styled("─".repeat(fill_width), theme::muted()),
    ]))
}

pub(crate) fn render_header(
    frame: &mut Frame,
    area: Rect,
    cols: &Columns,
    sort_mode: ContainersSortMode,
) {
    let style = theme::bold();
    let gap = " ".repeat(GAP_W);

    let host_arrow = matches!(sort_mode, ContainersSortMode::AlphaHost);
    let name_arrow = matches!(sort_mode, ContainersSortMode::AlphaContainer);

    let host_label = if host_arrow { "HOST \u{25BE}" } else { "HOST" };
    let name_label = if name_arrow {
        "NAME \u{25BE}"
    } else {
        crate::i18n::text("NAME")
    };

    let leading_pad = " ".repeat(HIGHLIGHT_W + MARKER_W + STATUS_DOT_W);
    let mut spans = vec![Span::styled(leading_pad, style)];
    if cols.show_host {
        spans.push(Span::styled(
            format!("{:<width$}", host_label, width = cols.host),
            style,
        ));
        spans.push(Span::raw(gap.clone()));
    }
    spans.push(Span::styled(
        format!("{:<width$}", name_label, width = cols.name),
        style,
    ));
    spans.push(Span::raw(gap.clone()));
    spans.push(Span::styled(
        format!("{:<width$}", "IMAGE", width = cols.image),
        style,
    ));
    if cols.show_uptime {
        spans.push(Span::raw(gap));
        spans.push(Span::styled(
            format!("{:>width$}", "UPTIME", width = UPTIME_W),
            style,
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

pub(crate) fn render_row<'a>(
    row: &'a ContainerRow,
    cols: &Columns,
    health: Option<&str>,
    inspect_exit_code: Option<i32>,
    spinner_tick: u64,
) -> ListItem<'a> {
    let (state_glyph, state_style) = state_glyph(
        &row.state,
        health,
        &row.status,
        inspect_exit_code,
        spinner_tick,
    );
    let image = crate::ui::truncate(&row.image, cols.image);

    let mut spans: Vec<Span<'static>> = vec![
        Span::raw(" ".repeat(MARKER_W)),
        Span::styled(format!("{} ", state_glyph), state_style),
    ];
    if cols.show_host {
        spans.push(Span::styled(
            pad_or_truncate(&row.alias, cols.host),
            theme::bold(),
        ));
        spans.push(Span::raw(GAP));
    }
    spans.push(Span::styled(
        pad_or_truncate(&row.name, cols.name),
        theme::bold(),
    ));
    spans.push(Span::raw(GAP));
    spans.push(Span::styled(
        pad_or_truncate(&image, cols.image),
        theme::muted(),
    ));
    if cols.show_uptime {
        spans.push(Span::raw(GAP));
        match &row.uptime {
            Some(uptime) => spans.push(Span::styled(
                format!("{:>width$}", uptime, width = UPTIME_W),
                theme::muted(),
            )),
            None => spans.push(Span::styled(
                format!("{:>width$}", "-", width = UPTIME_W),
                theme::muted(),
            )),
        }
    }
    ListItem::new(Line::from(spans))
}

/// State glyph + style for a container row. Encodes both lifecycle state
/// and health into the leading dot so the row needs no STATUS or HEALTH
/// column. Running + healthy/no-healthcheck pulses green; running +
/// unhealthy goes red, running + starting goes yellow. Exited with a
/// non-zero code uses the error glyph in warning tier; `dead` uses the
/// error glyph in error tier; paused/restarting use the half-circle in
/// warning tier; everything else falls back to the muted hollow circle.
///
/// `inspect_exit_code` is the optional ExitCode from a prior
/// `container inspect` cache. Podman emits an empty `Status` so the
/// docker-format `"Exited (N)"` parse fails; this fallback lets the
/// glyph turn warning as soon as inspect data lands rather than waiting
/// for the user to open the detail panel. Podman 3.x uses `"stopped"`
/// where podman 5.x and docker use `"exited"`; both are treated the
/// same here.
pub(crate) fn state_glyph(
    state: &str,
    health: Option<&str>,
    status: &str,
    inspect_exit_code: Option<i32>,
    spinner_tick: u64,
) -> (&'static str, ratatui::style::Style) {
    design::container_state_style(state, health, status, inspect_exit_code, spinner_tick)
}
