//! Keys tab. Three-card hero layout for SSH key inspection.
//!
//! Layout (top to bottom):
//!   ┌─ top nav bar (3 rows) ─┐
//!   ┌─ vault SSH strip (optional) ─┐
//!   ┌─ hero panel (3 cards side-by-side, equal height):
//!       Keys list | Randomart | Key info ─┐
//!   ┌─ linked-hosts grid (rest) ─┐
//!   ┌─ footer (1 row) ─┐
//!
//! The Keys list card uses the same `theme::selected_row()` +
//! `design::HOST_HIGHLIGHT` styling as the host list. The info card
//! holds a vertical strength gauge, kv-rows and a multi-line activity
//! chart that auto-scales its window via `super::activity_chart`.

mod bishop;
mod info_card;
mod linked_hosts;
mod vault_strip;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, Paragraph};

use super::design;
use super::host_list;
use super::theme;
use crate::app::App;
use crate::key_activity;
use crate::ssh_keys::SshKeyInfo;
use crate::vault_ssh;

/// Minimum width the hero needs to render the randomart and info cards
/// side-by-side. Below this it falls back to a single stacked card.
const HERO_MIN_WIDTH: u16 = 60;

/// Strength bar width inside the compact fallback layout.
const STRENGTH_BAR_WIDTH: usize = 8;

/// Top entry point. Renders the full Keys tab.
pub fn render(frame: &mut Frame, app: &mut App, spinner_tick: u64) {
    let area = frame.area();
    let top_bar_height: u16 = 3;
    let bishop_size = bishop::pick_bishop_size(area.height);

    let show_strip = vault_ssh::vault_ssh_in_use(app.hosts_state.list(), app.env().paths());
    let strip_rows = if show_strip {
        vault_strip::active_strip_rows(app)
    } else {
        Vec::new()
    };
    let strip_height: u16 = if show_strip {
        let body = strip_rows.len().max(1) as u16;
        2 + body
    } else {
        0
    };

    // Show the vertical key-list card only when there are 2+ keys or
    // search is active. With a single key, the card adds no value and
    // we give the freed width to the info card.
    let search_active = app.search.query().is_some();
    let show_key_list = app.keys.list().len() > 1 || search_active;

    // Order: top nav, optional Vault SSH strip, hero (with key list,
    // randomart and info side-by-side), linked hosts, footer.
    let mut constraints: Vec<Constraint> = vec![Constraint::Length(top_bar_height)];
    if show_strip {
        constraints.push(Constraint::Length(strip_height));
    }
    // Hero height is derived from the bishop variant picked above so
    // the rect we allocate and the variant `render_hero` draws into can
    // never drift apart.
    constraints.push(Constraint::Length(hero_h(bishop_size)));
    constraints.push(Constraint::Min(5));
    constraints.push(Constraint::Length(1));

    let chunks = Layout::vertical(constraints).split(area);

    render_top_bar(frame, app, chunks[0]);

    let mut next_idx = 1;
    if show_strip {
        vault_strip::render_vault_strip(frame, chunks[next_idx], &strip_rows);
        next_idx += 1;
    }
    let hero_area = chunks[next_idx];
    let hosts_area = chunks[next_idx + 1];
    let footer_area = chunks[next_idx + 2];

    if app.keys.list().is_empty() {
        render_empty_state(frame, hero_area, hosts_area);
    } else {
        // Split the hero into a fixed-width Keys list card on the left
        // (when there are 2+ keys or search is active) and the existing
        // randomart + info pair to its right.
        let (list_area, hero_remaining) = split_hero_for_key_list(hero_area, show_key_list);
        if show_key_list {
            render_key_list_card(frame, app, list_area);
        }
        let resolved = current_key_index(app);
        if let Some(idx) = resolved {
            if let Some(key) = app.keys.list().get(idx) {
                render_hero(
                    frame,
                    key,
                    app.keys.activity(),
                    hero_remaining,
                    bishop_size,
                    spinner_tick,
                );
                linked_hosts::render_linked_hosts(frame, app, key, hosts_area);
            } else {
                render_empty_state(frame, hero_remaining, hosts_area);
            }
        } else {
            render_empty_state(frame, hero_remaining, hosts_area);
        }
    }

    super::render_footer_with_help(frame, footer_area, footer_spans(show_strip), app);
}

/// Single source of truth for the hero panel height: bishop interior
/// rows plus the two card borders and the top/bottom randomart padding.
/// `render` uses this to allocate the rect; tests assert the result so
/// the formula can't drift without breaking both.
fn hero_h(bishop_size: (usize, usize)) -> u16 {
    (bishop_size.1 as u16) + 2 + 2 * bishop::RANDOMART_PAD_V
}

/// Carve a 28-col left strip off the hero for the key-list card and
/// return the remaining hero rect for randomart + info. When
/// `show_list` is false (single key, no search), the full hero stays
/// available for randomart + info.
fn split_hero_for_key_list(hero: Rect, show_list: bool) -> (Rect, Rect) {
    if !show_list {
        return (Rect::new(hero.x, hero.y, 0, hero.height), hero);
    }
    let list_w: u16 = 28.min(hero.width.saturating_sub(40));
    let gap: u16 = 1;
    let [list_area, _gap, rest] = Layout::horizontal([
        Constraint::Length(list_w),
        Constraint::Length(gap),
        Constraint::Min(20),
    ])
    .areas(hero);
    (list_area, rest)
}

/// Translate the UI-space selection into an `app.keys.list` index, honouring
/// an active search filter. Falls back to index 0 when nothing is
/// selected so the hero renders the first key by default.
fn current_key_index(app: &App) -> Option<usize> {
    let sel = app.keys.list_state().selected().unwrap_or(0);
    crate::ssh_keys::resolve_selection(app.keys.list(), app.search.query(), sel)
}

/// Render the shared top navigation bar via `host_list::top_bar_spans`.
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

/// Render the vertical Keys list card: one row per key, name-only. Uses
/// `List` with `theme::selected_row()` and `design::HOST_HIGHLIGHT` so
/// the selection styling matches the host list and tunnels list.
/// Search active flips the title to `search: <q> (N/M)` and filters the
/// rows; selection state is reused for both modes.
fn render_key_list_card(frame: &mut Frame, app: &mut App, area: Rect) {
    let total = app.keys.list().len();
    let filtered = crate::ssh_keys::filtered_key_indices(app.keys.list(), app.search.query());
    let search_active = app.search.query().is_some();

    let block = if search_active {
        // Mirror host_list / tunnels / containers: an active search
        // recolours the card border via `search_block_line` so the
        // focus indicator is consistent across tabs.
        let q = app.search.query().unwrap_or("");
        design::search_block_line(card_title(
            "SEARCH",
            Some(&format!("{} ({}/{})", q, filtered.len(), total)),
        ))
    } else {
        let sel = app.keys.list_state().selected().unwrap_or(0) + 1;
        design::main_block_line(card_title("KEYS", Some(&format!("{}/{}", sel, total))))
    };
    let block = crate::ui::host_list::with_update_badge(block, app, area.width);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let items: Vec<ListItem> = filtered
        .iter()
        .map(|&idx| ListItem::new(Line::from(Span::raw(app.keys.list()[idx].name.clone()))))
        .collect();

    let list = List::new(items)
        .highlight_style(theme::selected_row())
        .highlight_symbol(design::HOST_HIGHLIGHT);

    frame.render_stateful_widget(list, inner, app.keys.list_state_mut());
}

/// Build a card title in the host-detail sub-card style: bold
/// UPPERCASE label, optional muted `─ N` count separator. Returns a
/// pre-padded `Line` ready for `main_block_line(...)`. Shared with
/// `vault_strip` and `linked_hosts` so all four cards on the Keys tab
/// pick up the same title rhythm.
pub(super) fn card_title(label: &str, count: Option<&str>) -> Line<'static> {
    let mut spans = vec![
        Span::raw(" "),
        Span::styled(label.to_uppercase(), theme::bold()),
    ];
    if let Some(c) = count {
        spans.push(Span::styled(" \u{2500} ", theme::muted()));
        spans.push(Span::styled(c.to_string(), theme::muted()));
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

/// Render the hero: two side-by-side cards (randomart left, info right)
/// when there is room, falling back to a single stacked card on narrow
/// terminals. `bishop_size` is computed once by the caller from the
/// terminal height so the rect allocated for the hero and the variant
/// drawn into it cannot diverge.
fn render_hero(
    frame: &mut Frame,
    key: &SshKeyInfo,
    activity: &key_activity::KeyActivityLog,
    area: Rect,
    bishop_size: (usize, usize),
    spinner_tick: u64,
) {
    if area.width < 24 || area.height < 6 {
        return;
    }

    if area.width < HERO_MIN_WIDTH {
        render_hero_stacked(frame, key, activity, area);
        return;
    }

    // Width guard: when the info card cannot keep at least 60 cols for
    // the kv-list, shrink the bishop variant rather than steal from the
    // info side. Height was already accounted for by `pick_bishop_size`.
    let mut bishop_size = bishop_size;
    let info_min_w: u16 = 60;
    let gap: u16 = 1;
    let mut left_w = (bishop_size.0 as u16) + 2 + 2 * bishop::RANDOMART_PAD_H;
    while area.width < left_w + gap + info_min_w {
        bishop_size = match bishop_size {
            bishop::BISHOP_LARGE => bishop::BISHOP_CANONICAL,
            _ => break,
        };
        left_w = (bishop_size.0 as u16) + 2 + 2 * bishop::RANDOMART_PAD_H;
    }

    let [left_area, _gap, right_area] = Layout::horizontal([
        Constraint::Length(left_w),
        Constraint::Length(gap),
        Constraint::Min(20),
    ])
    .areas(area);

    bishop::render_randomart_card(frame, key, left_area, bishop_size, spinner_tick);
    info_card::render_info_card(frame, key, activity, right_area);
}

/// Stacked fallback for narrow terminals. One rounded card titled by the
/// key name, type/strength/last-touch/reach lines on top, randomart
/// below.
fn render_hero_stacked(
    frame: &mut Frame,
    key: &SshKeyInfo,
    activity: &key_activity::KeyActivityLog,
    area: Rect,
) {
    use ratatui::widgets::Padding;
    let title = Line::from(vec![
        Span::raw(" "),
        Span::styled(key.name.clone(), theme::bold()),
        Span::raw(" "),
    ]);
    let block = design::main_block_line(title).padding(Padding::new(1, 1, 0, 0));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(key.type_display(), theme::bold())));
    let bar = info_card::build_strength_bar(key.strength_score, STRENGTH_BAR_WIDTH);
    let bar_style = info_card::strength_color(key.strength_score);
    lines.push(Line::from(vec![
        Span::styled("Strength    ", theme::muted()),
        Span::styled(bar, bar_style),
        Span::raw("  "),
        Span::styled(format!("{}", key.strength_score), theme::bold()),
    ]));
    let now = key_activity::now_for_render();
    let last_use = activity
        .last_use_for_aliases(&key.linked_hosts)
        .map(|ts| key_activity::humanize_last_use(now, ts))
        .unwrap_or_else(|| "never".to_string());
    lines.push(Line::from(vec![
        Span::styled("Last used   ", theme::muted()),
        Span::styled(last_use, theme::bold()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Linked hosts", theme::muted()),
        Span::raw("  "),
        Span::styled(format!("{} hosts", key.linked_hosts.len()), theme::bold()),
    ]));
    lines.push(Line::from(""));
    for raw in key.bishop_lines() {
        lines.push(Line::from(Span::styled(raw.to_string(), theme::muted())));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_empty_state(frame: &mut Frame, hero_area: Rect, hosts_area: Rect) {
    let combined = Rect::new(
        hero_area.x,
        hero_area.y,
        hero_area.width,
        hero_area.height + hosts_area.height,
    );
    let block = design::main_block_line(Line::default());
    frame.render_widget(block, combined);
    let hints = [(
        "$",
        crate::i18n::text(crate::messages::TAB_EMPTY_KEYS_HINT_KEYGEN),
    )];
    let empty = design::TabEmpty {
        card_title: crate::i18n::text("Keys"),
        headline: crate::i18n::text(crate::messages::TAB_EMPTY_KEYS_HEADLINE),
        explainer: crate::i18n::text(crate::messages::TAB_EMPTY_KEYS_EXPLAINER),
        hints: &hints,
    };
    design::render_tab_empty(frame, combined, &empty);
}

fn footer_spans(vault_active: bool) -> Vec<Span<'static>> {
    use crate::messages::footer as fl;
    let mut footer = design::Footer::new()
        .primary("Enter", crate::i18n::text(fl::ENTER_COPY))
        .action("p", crate::i18n::text(fl::ACTION_PUSH));
    if vault_active {
        footer = footer.action("V", crate::i18n::text(fl::ACTION_VAULT_SIGN));
    }
    footer
        .action(":", crate::i18n::text(fl::ACTION_JUMP))
        .into_spans()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_hero_returns_full_hero_when_list_hidden() {
        let hero = Rect::new(0, 0, 100, 17);
        let (list, rest) = split_hero_for_key_list(hero, false);
        assert_eq!(list.width, 0);
        assert_eq!(rest, hero);
    }

    #[test]
    fn split_hero_carves_28_col_strip_when_list_shown() {
        let hero = Rect::new(0, 0, 100, 17);
        let (list, rest) = split_hero_for_key_list(hero, true);
        assert_eq!(list.width, 28);
        assert_eq!(rest.width, 100 - 28 - 1);
        assert_eq!(list.height, rest.height);
    }

    #[test]
    fn split_hero_shrinks_list_when_hero_is_narrow() {
        // 50 cols total: list cannot take the full 28 without starving
        // the randomart/info pair (40-col floor).
        let hero = Rect::new(0, 0, 50, 17);
        let (list, _rest) = split_hero_for_key_list(hero, true);
        assert!(list.width <= 28);
        assert!(list.width <= hero.width.saturating_sub(40));
    }

    #[test]
    fn hero_h_canonical_bishop_totals_13() {
        // The canonical 17x9 bishop plus 2 borders plus 2*PAD_V (=2)
        // must total 13 rows. `render()` calls this helper directly, so
        // any drift in the formula or in BISHOP_CANONICAL/RANDOMART_PAD_V
        // shows up here without ever divergng from the production path.
        assert_eq!(hero_h(bishop::BISHOP_CANONICAL), 13);
    }

    #[test]
    fn hero_h_large_bishop_totals_17() {
        assert_eq!(hero_h(bishop::BISHOP_LARGE), 17);
    }
}
