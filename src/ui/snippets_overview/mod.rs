//! Snippets tab: list of snippets with an animated, toggleable detail panel.

mod detail;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, Paragraph};

use crate::app::{App, ViewMode};
use crate::ui::host_list::top_bar_spans;
use crate::ui::{design, theme};

const TOP_BAR_HEIGHT: u16 = 3;
const DETAIL_PANEL_WIDTH: u16 = 72;
const DETAIL_MIN_TOTAL_WIDTH: u16 = 120;

pub fn render(frame: &mut Frame, app: &mut App, detail_progress: Option<f32>) {
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

    let indices = crate::snippet::filtered_indices(app.snippets.store(), app.search.query());
    let count = indices.len();

    // Clamp the cursor into the filtered range.
    let sel = app.snippets.list_state().selected();
    let new_sel = match sel {
        Some(i) if i < count => Some(i),
        _ if count > 0 => Some(0),
        _ => None,
    };
    if new_sel != sel {
        app.snippets.list_state_mut().select(new_sel);
    }

    if search_active {
        let total = app.snippets.store().snippets.len();
        render_search_bar(frame, app, search_bar_area, count, total);
    }

    // Detail width animates between 0 and DETAIL_PANEL_WIDTH, gated by the `v`
    // toggle and a terminal-width threshold. Below the threshold the panel is
    // suppressed regardless of preference. Mirrors the Containers tab. Computed
    // before the empty branch so the list frame is always present, empty or not.
    let target_detail =
        app.snippets.view_mode() == ViewMode::Detailed && body_area.width >= DETAIL_MIN_TOTAL_WIDTH;
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

    // No snippets at all: draw the list frame and a centred TabEmpty card
    // inside it (plus a quiet detail placeholder when the panel is shown), the
    // same two-pane empty composition the Containers tab uses.
    if app.snippets.store().snippets.is_empty() {
        render_empty(frame, app, list_area, detail_area);
        render_footer(frame, footer_area, app);
        return;
    }

    render_list(frame, app, list_area, &indices);

    if let Some(detail_area) = detail_area {
        let selected_snippet = app
            .snippets
            .list_state()
            .selected()
            .and_then(|i| indices.get(i))
            .and_then(|&store_idx| app.snippets.store().snippets.get(store_idx))
            .cloned();
        let targets: Vec<String> = selected_snippet
            .as_ref()
            .map(|s| app.snippets.store().targets_for(&s.name).to_vec())
            .unwrap_or_default();
        detail::render(
            frame,
            detail_area,
            selected_snippet.as_ref(),
            &targets,
            app.snippets.runs(),
        );
    }

    render_footer(frame, footer_area, app);

    // Delete confirmation popup, shared with the host-list snippet picker.
    if app.snippets.pending_delete().is_some() {
        crate::ui::snippet_picker::render_delete_popup(frame, app);
    }
}

fn render_top_bar(frame: &mut Frame, app: &App, area: Rect) {
    let block = design::main_block_line(Line::default());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    // Offset by one column so the first tab does not butt against the rounded
    // border, matching the other tabs' nav bar inset.
    let content_area = Rect::new(
        inner.x.saturating_add(1),
        inner.y,
        inner.width.saturating_sub(1),
        1,
    );
    frame.render_widget(Paragraph::new(Line::from(top_bar_spans(app))), content_area);
}

fn render_search_bar(frame: &mut Frame, app: &App, area: Rect, visible: usize, total: usize) {
    let query = app.search.query().unwrap_or("");
    let match_info = if query.is_empty() {
        String::new()
    } else {
        format!(" ({} of {})", visible, total)
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

fn render_empty(frame: &mut Frame, app: &App, list_area: Rect, detail_area: Option<Rect>) {
    let block = design::main_block_line(Line::from(Span::styled(" Snippets (0) ", theme::bold())));
    let block = crate::ui::host_list::with_update_badge(block, app, list_area.width);
    frame.render_widget(block, list_area);
    let hints = [(
        "a",
        crate::i18n::text(crate::messages::TAB_EMPTY_SNIPPETS_HINT_ADD),
    )];
    let empty = design::TabEmpty {
        card_title: crate::i18n::text("Snippets"),
        headline: crate::i18n::text(crate::messages::TAB_EMPTY_SNIPPETS_HEADLINE),
        explainer: crate::i18n::text(crate::messages::TAB_EMPTY_SNIPPETS_EXPLAINER),
        hints: &hints,
    };
    design::render_tab_empty(frame, list_area, &empty);
    if let Some(detail) = detail_area {
        design::render_tab_empty_detail(frame, detail);
    }
}

fn render_list(frame: &mut Frame, app: &mut App, area: Rect, indices: &[usize]) {
    let title = Line::from(Span::styled(
        format!(" Snippets ({}) ", indices.len()),
        theme::bold(),
    ));
    let mut block = if app.search.query().is_some() {
        design::search_block_line(title)
    } else {
        design::main_block_line(title)
    };
    block = crate::ui::host_list::with_update_badge(block, app, area.width);
    let block_inner = block.inner(area);
    frame.render_widget(block, area);

    // Inset the content one column on each side so the underline rule and the
    // columns do not kiss the rounded border, matching the Containers and
    // Tunnels overviews.
    let inner = Rect {
        x: block_inner.x.saturating_add(1),
        y: block_inner.y,
        width: block_inner.width.saturating_sub(2),
        height: block_inner.height,
    };

    let [header_area, underline_area, list_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(inner);

    // Columns: NAME (24) PARAMS (8) DESCRIPTION (rest). The leading space on
    // the header aligns it with the 1-col highlight symbol the list reserves.
    let header = format!(" {:<24}{:<8}{}", "NAME", "PARAMS", "DESCRIPTION");
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(header, theme::bold()))),
        header_area,
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            "\u{2500}".repeat(underline_area.width as usize),
            theme::muted(),
        )),
        underline_area,
    );

    let items: Vec<ListItem> = indices
        .iter()
        .filter_map(|&i| app.snippets.store().snippets.get(i))
        .map(|s| {
            let pc = crate::snippet::count_params(&s.command);
            let params = if pc == 0 {
                design::ICON_PENDING.to_string()
            } else {
                pc.to_string()
            };
            let name = super::truncate(&s.name, 23);
            let desc = super::truncate(&s.description, 40);
            // NAME is the primary key (bold), like the picker overlay and the
            // sibling overviews; params and description are secondary (muted).
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<24}", name), theme::bold()),
                Span::styled(format!("{:<8}", params), theme::muted()),
                Span::styled(desc, theme::muted()),
            ]))
        })
        .collect();
    let list = List::new(items)
        .highlight_style(theme::selected_row())
        .highlight_symbol(design::HOST_HIGHLIGHT);
    frame.render_stateful_widget(list, list_area, app.snippets.list_state_mut());
}

fn render_footer(frame: &mut Frame, area: Rect, app: &mut App) {
    use crate::messages::footer as fl;
    let view_label = if app.snippets.view_mode() == ViewMode::Detailed {
        " compact "
    } else {
        crate::i18n::text(fl::ACTION_DETAIL)
    };
    let spans = design::Footer::new()
        .primary("Enter", crate::i18n::text(fl::ENTER_RUN))
        .action("/", crate::i18n::text(fl::ACTION_SEARCH))
        .action("a", crate::i18n::text(fl::ACTION_ADD))
        .action("e", crate::i18n::text(fl::ACTION_EDIT))
        .action("d", crate::i18n::text(fl::ACTION_DEL))
        .action("v", view_label)
        .action(":", crate::i18n::text(fl::ACTION_JUMP))
        .into_spans();
    super::render_footer_with_help(frame, area, spans, app);
}
