//! Containers-tab overview screen.
//!
//! Lists every cached container across every host purple has inspected,
//! grouped by host divider in `AlphaHost` mode and flat in
//! `AlphaContainer` mode. Renders an animated detail panel on the right
//! that swaps content based on the selected row: container-level fields
//! (cache + inspect tiers) for a Container row, per-host summary
//! (running/exited counts, runtime, last sync, fold state) for a
//! HostHeader row. Group dividers are first-class selection targets so
//! the user can fold groups with Space and dispatch bulk K/S actions
//! against every running container on the host.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::design;
use super::host_list;
use super::theme;
use crate::app::{App, ContainersSortMode, ViewMode};

mod detail;
mod format;
mod host_detail;
mod model;
mod table;

pub(crate) use detail::*;
pub(crate) use format::*;
pub(crate) use host_detail::*;
pub(crate) use model::*;
pub(crate) use table::*;

const TOP_BAR_HEIGHT: u16 = 3;

const GAP: &str = "    ";

/// Detail panel width when the terminal is wide enough to render one.
/// Wider than the host-detail panel because container detail packs more
/// data per row (image, version, digest, command, mounts, log tail) and
/// the LOGS card needs room for a usable line of output.
const DETAIL_PANEL_WIDTH: u16 = 96;

/// Minimum animated detail-panel width before we stamp content into it.
/// Below this we still render an empty bordered placeholder so the list
/// area resizes smoothly without content briefly clipping the border.
/// Mirrors the host-list `DETAIL_RENDER_MIN` constant.
const DETAIL_RENDER_MIN: u16 = 8;

/// Below this total terminal width the detail panel is suppressed and
/// the list takes the full width. Threshold = panel width + the column
/// budget the list needs to render its columns at sensible minima with
/// some slack so HOST/IMAGE truncation does not bite at the threshold.
const DETAIL_MIN_TOTAL_WIDTH: u16 = 158;

pub fn render(frame: &mut Frame, app: &mut App, spinner_tick: u64, detail_progress: Option<f32>) {
    let area = frame.area();

    let search_active = app.search.query().is_some();
    let search_bar_h = if search_active { 1 } else { 0 };
    let [top_bar_area, body_area, search_bar_area, footer_area] = Layout::vertical([
        Constraint::Length(TOP_BAR_HEIGHT),
        Constraint::Min(0),
        Constraint::Length(search_bar_h),
        Constraint::Length(1),
    ])
    .areas(area);
    render_top_bar(frame, app, top_bar_area);

    let items = visible_items(app);
    let item_count = items.len();
    let row_count = items.iter().filter(|i| i.as_container().is_some()).count();
    let total_cached = if search_active {
        total_cached_count(app)
    } else {
        0
    };
    if search_active {
        render_search_bar(frame, app, search_bar_area, row_count, total_cached);
    }

    let sel = app.ui.containers_overview_state().selected();
    let new_sel = match sel {
        // Headers are now valid selection targets (the user can park on a
        // group divider to drive bulk actions or fold the group). Empty
        // lists or out-of-range indices still get snapped to the first
        // visible item, header or container, whichever comes first.
        Some(i) if i < item_count => Some(i),
        _ => first_visible_index(&items),
    };
    if new_sel != sel {
        app.ui.containers_overview_state_mut().select(new_sel);
    }

    // Detail panel is gated by both the user's `v` toggle and the
    // terminal width threshold. Below the threshold the panel is
    // suppressed regardless of preference. Width animates between
    // 0 and DETAIL_PANEL_WIDTH using `detail_progress`, matching the
    // host-list cubic ease-out.
    let target_detail = app.containers_overview.view_mode() == ViewMode::Detailed
        && body_area.width >= DETAIL_MIN_TOTAL_WIDTH;
    let detail_width = if body_area.width >= DETAIL_MIN_TOTAL_WIDTH {
        if let Some(progress) = detail_progress {
            (progress * DETAIL_PANEL_WIDTH as f32).round() as u16
        } else if target_detail {
            DETAIL_PANEL_WIDTH
        } else {
            0
        }
    } else {
        0
    };
    let (list_area, detail_area) = if detail_width > 0 {
        let [left, right] =
            Layout::horizontal([Constraint::Min(0), Constraint::Length(detail_width)])
                .areas(body_area);
        (left, Some(right))
    } else {
        (body_area, None)
    };

    let update_title = app.update.available().map(|ver| {
        let label = host_list::build_update_label(
            ver,
            app.update.headline(),
            app.update.hint(),
            list_area.width,
        );
        Line::from(Span::styled(label, theme::update_badge()))
    });

    let url_label = Line::from(Span::styled(" getpurple.sh ", theme::muted()));
    let mut block = if search_active {
        let title = Line::from(vec![Span::styled(
            format!(" search: {}/{} ", row_count, total_cached),
            theme::bold(),
        )]);
        design::search_block_line(title).title_bottom(url_label.right_aligned())
    } else {
        // Top-border header is the bare visible-row count. Per-host
        // running/exited live on group-divider rows; the active top
        // tab already says "containers" so labelling the count here
        // would be redundant.
        let title = build_stats_title(row_count);
        design::main_block_line(title).title_bottom(url_label.right_aligned())
    };
    if let Some(update) = update_title.as_ref() {
        block = block.title_top(update.clone().right_aligned());
    }
    // Sync freshness no longer surfaces in the list-block title; it
    // moves to the LIFECYCLE card on the detail panel where it is
    // tied to a specific container's last refresh.

    if items.is_empty() {
        // Empty state: render ONE centred TabEmpty card inside the list
        // block (no second message in the detail area). The detail panel,
        // when present, becomes a quiet bordered placeholder so the
        // two-pane composition still reads as the Containers tab, just
        // empty, instead of as a half-rendered screen.
        frame.render_widget(block, list_area);
        let hints = [(
            "a",
            crate::i18n::text(crate::messages::TAB_EMPTY_CONTAINERS_HINT_ADD),
        )];
        let empty = design::TabEmpty {
            card_title: crate::i18n::text("Containers"),
            headline: crate::i18n::text(crate::messages::TAB_EMPTY_CONTAINERS_HEADLINE),
            explainer: crate::i18n::text(crate::messages::TAB_EMPTY_CONTAINERS_EXPLAINER),
            hints: &hints,
        };
        design::render_tab_empty(frame, list_area, &empty);
        if let Some(detail) = detail_area {
            design::render_tab_empty_detail(frame, detail);
        }
        render_footer(frame, footer_area, app);
        return;
    }

    let block_inner = block.inner(list_area);
    frame.render_widget(block, list_area);

    let inner = Rect {
        x: block_inner.x.saturating_add(1),
        y: block_inner.y,
        width: block_inner.width.saturating_sub(2),
        height: block_inner.height,
    };

    let [col_header_area, underline_area, list_inner_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(inner);

    let content_w = (inner.width as usize).saturating_sub(1);
    let containers_only: Vec<&ContainerRow> =
        items.iter().filter_map(|i| i.as_container()).collect();
    // HOST column lives on the divider in AlphaHost mode; flat
    // AlphaContainer mode shows it per-row so users keep cross-host
    // context when sorted by name.
    let show_host = matches!(
        app.containers_overview.sort_mode(),
        ContainersSortMode::AlphaContainer
    );
    let cols = compute_columns(containers_only.iter().copied(), content_w, show_host);

    // In `AlphaHost` mode the host alias lives on the divider line,
    // so suppress the HOST column from the column header to match.
    render_header(
        frame,
        col_header_area,
        &cols,
        app.containers_overview.sort_mode(),
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            "\u{2500}".repeat(underline_area.width as usize),
            theme::muted(),
        )),
        underline_area,
    );

    let list_items: Vec<ListItem> = items
        .iter()
        .map(|item| match item {
            ContainerListItem::Container(row) => {
                // Pull the cached health value out of the inspect cache
                // for this row. `None` when no inspect has landed yet
                // (HEALTH cell will show `-`); the auto-fetch trigger
                // populates it within a frame or two of selection.
                let inspect = app
                    .containers_overview
                    .inspect_cache()
                    .entries
                    .get(&row.id)
                    .and_then(|e| e.result.as_ref().ok());
                let health = inspect.and_then(|i| i.health.as_deref());
                // Pass the inspected ExitCode through so the state glyph
                // can warn on non-zero exits even when `Status` is empty
                // (podman emits no status string).
                let inspect_exit_code = inspect.map(|i| i.exit_code);
                render_row(row, &cols, health, inspect_exit_code, spinner_tick)
            }
            ContainerListItem::HostHeader { alias, .. } => render_host_header_row(alias, content_w),
        })
        .collect();
    let list = List::new(list_items)
        .highlight_style(theme::selected_row())
        .highlight_symbol(design::HOST_HIGHLIGHT);

    let selected_item = app
        .ui
        .containers_overview_state()
        .selected()
        .and_then(|i| items.get(i).cloned());

    frame.render_stateful_widget(
        list,
        list_inner_area,
        app.ui.containers_overview_state_mut(),
    );

    if let Some(detail) = detail_area {
        if detail.width >= DETAIL_RENDER_MIN {
            match &selected_item {
                Some(ContainerListItem::Container(row)) => {
                    render_detail(frame, app, detail, Some(row), spinner_tick);
                }
                Some(ContainerListItem::HostHeader {
                    alias,
                    total,
                    running,
                    ..
                }) => {
                    render_host_detail(frame, app, detail, alias, *total, *running);
                }
                None => {
                    render_detail(frame, app, detail, None, spinner_tick);
                }
            }
        } else {
            let block = design::main_block_line(Line::default());
            frame.render_widget(block, detail);
        }
    }

    render_footer(frame, footer_area, app);
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

fn render_search_bar(frame: &mut Frame, app: &App, area: Rect, visible_count: usize, total: usize) {
    let query = app.search.query().unwrap_or("");
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

fn render_footer(frame: &mut Frame, area: Rect, app: &mut App) {
    use crate::messages::footer as fl;
    // Mirror the host_list footer shape (Enter · / · # · v · :) with the
    // tag affordance swapped for `l logs`. K/S/e (restart, stop, exec),
    // s (sort) and r/R (refresh) stay as keybindings and are documented
    // in the help screen, but live outside the always-visible footer to
    // keep it readable on narrow terminals.
    let view_label = if app.containers_overview.view_mode() == ViewMode::Detailed {
        " compact "
    } else {
        crate::i18n::text(fl::ACTION_DETAIL)
    };
    let spans = design::Footer::new()
        .primary("Enter", crate::i18n::text(fl::ENTER_SHELL))
        .action("/", crate::i18n::text(fl::ACTION_SEARCH))
        .action("l", crate::i18n::text(fl::ACTION_LOGS))
        .action("v", view_label)
        .action(":", crate::i18n::text(fl::ACTION_JUMP))
        .into_spans();
    super::render_footer_with_help(frame, area, spans, app);
}

#[cfg(test)]
mod tests;
