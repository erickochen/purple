//! Full-screen logs overlay for `Screen::ContainerLogs`. Renders a
//! single block with a top title (`logs · alias · name · fetched
//! Xs ago`), the captured log lines as a scrollable Paragraph, and
//! a footer with the navigation keys. No styling is applied to the
//! log lines themselves: container output is opaque and any
//! syntax-highlighting heuristic risks dragging false positives in.
//!
//! `/` opens a vim-style incremental search: characters extend the
//! query, matches are highlighted inline, n/N step through hits.
//! Enter commits, Esc clears.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use super::design;
use super::theme;
use crate::app::{App, ContainerLogsSearch, Screen};
use crate::handler::container_logs as logs_handler;
use crate::messages::footer as fl;

pub fn render(frame: &mut Frame, app: &mut App) {
    // Clone the small bits we need so the immutable borrow can drop
    // before we write `last_render_height` back. `body` and `search`
    // are cheap to clone for a single-shot overlay (200 lines max,
    // short query string), and the alternative — threading the
    // mutable borrow through every helper — would balloon the API.
    if !matches!(app.screen, Screen::ContainerLogs) {
        return;
    }
    let (alias, container_name, body, fetched_at, error, scroll, search) = {
        let Some(view) = app.container_state.logs_view() else {
            return;
        };
        (
            view.alias.clone(),
            view.container_name.clone(),
            view.body.clone(),
            view.fetched_at,
            view.error.clone(),
            view.scroll,
            view.search.clone(),
        )
    };

    let area = frame.area();
    frame.render_widget(Clear, area);

    // Reserve a row above the footer for the search bar when active.
    // The body shrinks by one row when the user opens `/`; reserving the
    // bar above the footer (rather than overlaying inside the block)
    // keeps the bar visually adjacent to the footer keycap row.
    let search_bar_h: u16 = if search.is_some() { 1 } else { 0 };
    let [body_area, search_area, footer_area] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(search_bar_h),
        Constraint::Length(1),
    ])
    .areas(area);

    let title_line = build_title(&alias, &container_name, fetched_at, error.as_deref(), app);

    // Switch the body border to the search-active purple while `/` is
    // open — same affordance as the host list border switch, so the
    // user has a strong "search is active" signal beyond the cursor
    // and the inline highlights.
    let block = if search.is_some() {
        design::search_overlay_block_line(title_line)
    } else {
        design::overlay_block_line(title_line)
    };
    let inner = block.inner(body_area);
    frame.render_widget(block, body_area);

    // Clamp scroll so it never leaves blank space below the body when
    // the body is longer than the viewport. With short bodies this is
    // a no-op (max_scroll = 0). The clamp guards against the
    // logs-arrival path running before the renderer has measured the
    // viewport (last_render_height = 0 yields scroll = body.len()).
    // Reuses the handler's `tail_scroll` so the two stay in lockstep.
    let max_scroll = logs_handler::tail_scroll(body.len(), inner.height);
    let effective_scroll = scroll.min(max_scroll);
    render_body(
        frame,
        inner,
        &body,
        error.as_deref(),
        effective_scroll,
        search.as_ref(),
    );

    if let Some(s) = search.as_ref() {
        render_search_bar(frame, search_area, s);
    }

    // Stash the rendered area height so the next logs-arrival or `G`
    // keypress can compute the tail-anchored scroll.
    if let Some(view) = app.container_state.logs_view_mut() {
        view.last_render_height = inner.height;
    }

    let footer_spans = build_footer_spans(search.as_ref());
    frame.render_widget(Paragraph::new(Line::from(footer_spans)), footer_area);
}

fn build_footer_spans(search: Option<&ContainerLogsSearch>) -> Vec<Span<'static>> {
    match search {
        Some(_) => design::Footer::new()
            .primary("Esc", crate::i18n::text(fl::ACTION_CLOSE))
            .action("Tab/Shift-Tab", crate::i18n::text(fl::ACTION_MATCH))
            .action("←/→", crate::i18n::text(fl::ACTION_MOVE))
            .action("Bksp", crate::i18n::text(fl::ACTION_DEL))
            .into_spans(),
        None => design::Footer::new()
            .primary("Esc", crate::i18n::text(fl::ACTION_BACK))
            .action("/", crate::i18n::text(fl::ACTION_SEARCH))
            .action("g", crate::i18n::text(fl::ACTION_TOP))
            .action("G", crate::i18n::text(fl::ACTION_BOTTOM))
            .action("j/k", crate::i18n::text(fl::ACTION_SCROLL))
            .action("PgUp/PgDn", crate::i18n::text(fl::ACTION_PAGE))
            .action("r", crate::i18n::text(fl::ACTION_REFRESH))
            .into_spans(),
    }
}

fn build_title(
    alias: &str,
    container_name: &str,
    fetched_at: u64,
    error: Option<&str>,
    app: &App,
) -> Line<'static> {
    let now = if app.demo_mode {
        crate::demo_flag::now_secs()
    } else {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    };
    let mut spans = vec![
        Span::styled(" logs ", theme::bold()),
        Span::styled(format!("· {} ", container_name), theme::bold()),
        Span::styled(format!("· on {} ", alias), theme::muted()),
    ];
    if let Some(e) = error {
        spans.push(Span::styled(
            format!("· {} ", crate::messages::container_logs_failed(e)),
            theme::error(),
        ));
    } else if fetched_at == 0 {
        spans.push(Span::styled(
            format!(
                "· {} ",
                crate::i18n::text(crate::messages::CONTAINER_LOGS_LOADING)
            ),
            theme::muted(),
        ));
    } else {
        let age = now.saturating_sub(fetched_at);
        spans.push(Span::styled(
            format!("· {} ", crate::messages::container_logs_fetched(age)),
            theme::muted(),
        ));
    }
    Line::from(spans)
}

fn render_search_bar(frame: &mut Frame, area: Rect, search: &ContainerLogsSearch) {
    use unicode_width::UnicodeWidthStr;

    let suffix = if search.query.is_empty() {
        String::new()
    } else if search.matches.is_empty() {
        format!(
            " ({})",
            crate::i18n::text(crate::messages::CONTAINER_LOGS_SEARCH_NO_MATCHES)
        )
    } else {
        format!(
            " ({})",
            crate::messages::container_logs_search_position(
                search.current + 1,
                search.matches.len()
            )
        )
    };
    let spans = vec![
        Span::styled(" / ", theme::brand_badge()),
        Span::raw(" "),
        Span::raw(search.query.clone()),
        Span::styled(suffix, theme::muted()),
    ];
    frame.render_widget(Paragraph::new(Line::from(spans)), area);

    // Native terminal cursor: visible whenever search is open. The
    // viewer is modeless — there is no "navigation mode" where the
    // cursor would be misleading, so the blinking cursor doubles as
    // the "search is active" indicator.
    //
    // Layout of the search line: " / " (3 cols brand_badge) + " "
    // separator + query. Cursor sits after `cursor_pos` chars of the
    // query. Use unicode width so wide chars (CJK, emoji) line up
    // correctly. Mirrors host_form's cursor positioning.
    const PREFIX_W: u16 = 4; // " / " + " "
    let prefix: String = search.query.chars().take(search.cursor_pos).collect();
    let cursor_x = area
        .x
        .saturating_add(PREFIX_W)
        .saturating_add(prefix.width() as u16);
    if area.width > 0 && cursor_x < area.x.saturating_add(area.width) {
        frame.set_cursor_position((cursor_x, area.y));
    }
}

fn render_body(
    frame: &mut Frame,
    area: Rect,
    body: &[String],
    error: Option<&str>,
    scroll: u16,
    search: Option<&ContainerLogsSearch>,
) {
    if let Some(e) = error {
        // Error path: centred message in error style. Body is
        // empty because the SSH call failed before any output landed.
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("  fetch failed: {}", e),
                theme::error(),
            )),
        ];
        frame.render_widget(Paragraph::new(lines), area);
        return;
    }
    if body.is_empty() {
        // Loading path: spinner-less placeholder. The SSH call is
        // bounded and short; a static label avoids fighting the
        // crossterm tick cadence.
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                format!(
                    "  {}",
                    crate::i18n::text(crate::messages::CONTAINER_LOGS_LOADING)
                ),
                theme::muted(),
            )),
        ];
        frame.render_widget(Paragraph::new(lines), area);
        return;
    }
    let current_line = search.and_then(|s| s.matches.get(s.current).copied());
    let lines: Vec<Line<'_>> = body
        .iter()
        .enumerate()
        .map(|(idx, line)| highlight_line(idx, line, search, current_line))
        .collect();
    let paragraph = Paragraph::new(lines).scroll((scroll, 0));
    frame.render_widget(paragraph, area);
}

/// Build a row of spans for one log line, splitting at search-match
/// byte boundaries so matches render in the highlight style. Matches
/// on the row the cursor is on render in the stronger "selected_row"
/// style; everything else uses `highlight_bold`.
fn highlight_line<'a>(
    idx: usize,
    line: &'a str,
    search: Option<&ContainerLogsSearch>,
    current_line: Option<usize>,
) -> Line<'a> {
    let Some(s) = search.filter(|s| !s.query.is_empty()) else {
        return Line::from(line);
    };
    let positions = logs_handler::match_indices_smart(line, &s.query);
    if positions.is_empty() {
        return Line::from(line);
    }
    let q_len = s.query.len();
    let style = if current_line == Some(idx) {
        theme::selected_row()
    } else {
        theme::highlight_bold()
    };
    let mut spans: Vec<Span<'a>> = Vec::with_capacity(positions.len() * 2 + 1);
    let mut cursor = 0;
    for start in positions {
        if start > cursor {
            spans.push(Span::raw(&line[cursor..start]));
        }
        let end = (start + q_len).min(line.len());
        spans.push(Span::styled(&line[start..end], style));
        cursor = end;
    }
    if cursor < line.len() {
        spans.push(Span::raw(&line[cursor..]));
    }
    Line::from(spans)
}
