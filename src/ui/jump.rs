use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph};

use super::design;
use super::theme;
use crate::app::{
    App, JumpHit, MatchSource as JumpMatchSource, match_source_for_host, parse_query_scope,
};

/// Maximum visible result rows. The overlay grows with content up to
/// this cap, then scrolls. Sized to comfortably fit the empty-state
/// action list on a typical 30-row terminal.
const MAX_VISIBLE_ROWS: u16 = 22;
/// One blank line between sections — pure breathing room.
const SECTION_GAP: u16 = 1;

pub fn render(frame: &mut Frame, app: &mut App) {
    let jump = match app.jump.as_ref() {
        Some(p) => p,
        None => return,
    };

    let visible_full = jump.visible_hits();
    let empty_query = jump.query().is_empty();

    // The data layer applies the empty-state cap, so the groups returned
    // here are already trimmed. The renderer just reflects them.
    let groups: Vec<(String, Vec<JumpHit>)> = if empty_query {
        jump.empty_state_groups()
            .into_iter()
            .map(|(l, h)| (l.to_string(), h))
            .collect()
    } else {
        jump.grouped_hits()
            .into_iter()
            .map(|(k, h)| (k.section_label().to_string(), h))
            .collect()
    };

    // Compare the visible action count to the total to decide whether to
    // render the relational header (`Actions  6 of 29`) or the bare count.
    let actions_total = jump.empty_state_actions_total();
    let actions_visible = groups
        .iter()
        .find(|(l, _)| l.eq_ignore_ascii_case("ACTIONS"))
        .map(|(_, h)| h.len())
        .unwrap_or(0);
    let actions_capped = empty_query && actions_visible < actions_total;

    let visible_count: u16 = groups.iter().map(|(_, h)| h.len() as u16).sum();
    let group_headers: u16 = groups.len() as u16;
    let gap_rows: u16 = if groups.is_empty() {
        0
    } else {
        (groups.len() as u16 - 1) * SECTION_GAP
    };
    // Header `Actions  6 of 29` already advertises that more exist;
    // the dedicated tease line below the list became redundant. Drop
    // it so the empty state recovers a row of vertical space.
    let total_rows = visible_count + group_headers + gap_rows;
    let truncated = total_rows > MAX_VISIBLE_ROWS;
    let list_rows = total_rows.clamp(1, MAX_VISIBLE_ROWS);
    let footer_hint_rows = if truncated { 1u16 } else { 0u16 };
    // border (2) + halo (1) + input box (3) + halo (1) + list +
    // (truncation hint) + bottom halo (1).
    let total_height = 2 + 1 + 3 + 1 + list_rows + footer_hint_rows + 1;

    // Wider overlay for breathing room. Min 72, prefer ~75% of terminal,
    // capped at 100 cols.
    let dynamic_width = 72u16.max(frame.area().width * 75 / 100);
    let overlay_width = dynamic_width
        .min(100)
        .min(frame.area().width.saturating_sub(4));
    let height = total_height.min(frame.area().height.saturating_sub(3));
    let area = super::centered_rect_fixed(overlay_width, height, frame.area());

    frame.render_widget(Clear, area);

    let block = design::overlay_block("Jump");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Vertical layout inside the rounded outer block:
    //   halo (1) | input box (3) | halo (1) | list (Min) |
    //   (truncation hint) | bottom halo (1)
    // The input is wrapped in its own rounded `accent`-coloured Block
    // so the field reads unmistakably as the focusable input.
    let mut constraints = vec![
        Constraint::Length(1), // halo above input
        Constraint::Length(3), // input box (with its own border)
        Constraint::Length(1), // halo below input
        Constraint::Min(1),    // result list
    ];
    if truncated {
        constraints.push(Constraint::Length(1)); // truncation hint row
    }
    constraints.push(Constraint::Length(1)); // bottom halo
    let rows = Layout::vertical(constraints).split(inner);

    // Input box: rounded brand-purple border around a single content
    // line. Cursor block in accent colour; placeholder dim. The
    // brand-coloured border IS the affordance — no extra `>` prompt
    // needed. `border_search` is the same focused-state purple used on
    // the host-list search border, so the focus signal is consistent
    // across purple's surfaces.
    //
    // Horizontal inset (2 cols each side) so the box does not span the
    // full overlay width — keeps the input visually distinct from the
    // result rows that sit edge-to-edge below it.
    let input_outer = rows[1];
    let inset = 2u16.min(input_outer.width / 4);
    let input_box_area = Rect::new(
        input_outer.x + inset,
        input_outer.y,
        input_outer.width.saturating_sub(inset * 2),
        input_outer.height,
    );
    let input_block = design::search_overlay_block_line(Line::from(""));
    let input_inner = input_block.inner(input_box_area);
    frame.render_widget(input_block, input_box_area);

    let input_line = if jump.query().is_empty() {
        Line::from(vec![
            Span::raw(" "),
            Span::styled("\u{2588}", theme::accent_bold()),
            Span::raw(" "),
            Span::styled(
                crate::i18n::text(crate::messages::PALETTE_PLACEHOLDER),
                theme::muted(),
            ),
        ])
    } else {
        Line::from(vec![
            Span::raw(" "),
            Span::styled(jump.query().to_string(), theme::brand()),
            Span::styled("\u{2588}", theme::accent_bold()),
        ])
    };
    frame.render_widget(Paragraph::new(input_line), input_inner);

    let list_row = rows[3];

    if visible_full.is_empty() {
        design::render_empty(
            frame,
            list_row,
            crate::i18n::text(crate::messages::PALETTE_NO_RESULTS),
        );
        render_footer(frame, area, app);
        return;
    }

    let inner_width = inner.width as usize;
    let mut items: Vec<ListItem> = Vec::with_capacity(total_rows as usize);
    let mut row_to_hit: Vec<Option<usize>> = Vec::with_capacity(items.capacity());
    let mut hit_cursor = 0usize;

    for (gi, (label, group)) in groups.iter().enumerate() {
        if gi > 0 {
            items.push(ListItem::new(Line::from("")));
            row_to_hit.push(None);
        }
        // Capped ACTIONS sections render as `Actions  6 of 29` so the
        // user sees the cap explicitly.
        let header = if actions_capped && label.eq_ignore_ascii_case("ACTIONS") {
            section_header_line_capped(label, group.len(), actions_total)
        } else {
            section_header_line(label, group.len())
        };
        items.push(ListItem::new(header));
        row_to_hit.push(None);
        for hit in group {
            let line = render_hit_row(hit, jump.query(), inner_width, false);
            items.push(ListItem::new(line));
            row_to_hit.push(Some(hit_cursor));
            hit_cursor += 1;
        }
    }

    // Selection visibility tracks `cursor_revealed`. On a fresh empty
    // open the eye stays on the input field. Once the user navigates
    // (Down/Up/Tab) or types, the selection cue appears.
    let selected_row: Option<usize> = if !jump.cursor_revealed() {
        None
    } else {
        Some(
            row_to_hit
                .iter()
                .position(|r| matches!(r, Some(i) if *i == jump.selected()))
                .unwrap_or(0),
        )
    };

    let list = List::new(items).highlight_style(theme::selected_row());
    let mut list_state = ListState::default();
    list_state.select(selected_row);
    frame.render_stateful_widget(list, list_row, &mut list_state);

    if truncated {
        let hidden = total_rows.saturating_sub(MAX_VISIBLE_ROWS);
        let hint = Line::from(vec![
            Span::raw("   "),
            Span::styled(
                crate::messages::jump_more_rows(hidden as usize),
                theme::muted().add_modifier(Modifier::DIM),
            ),
        ]);
        frame.render_widget(Paragraph::new(hint), rows[4]);
    }

    render_footer(frame, area, app);
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let footer_area = design::render_overlay_footer(frame, area);
    use crate::messages::footer as fl;
    design::Footer::new()
        .primary("Enter", crate::i18n::text(fl::ENTER_SELECT))
        .action("\u{2191}\u{2193}", crate::i18n::text(fl::ARROWS_SELECT))
        .action("Tab", crate::i18n::text(fl::TAB_NEXT))
        .action("Esc", crate::i18n::text(fl::ESC_CLOSE))
        .render_with_status(frame, footer_area, app);
}

fn section_header_line(label: &str, count: usize) -> Line<'static> {
    // Title-case the label so the header reads as a quiet sub-label
    // ("Recent  2", "Actions  29") instead of an ALL-CAPS shout. Style
    // is `muted + bold` so it has weight without competing with the
    // accent-coloured input row for attention.
    let pretty = pretty_section_label(label);
    Line::from(vec![
        Span::raw("   "),
        Span::styled(pretty, theme::muted().add_modifier(Modifier::BOLD)),
        Span::raw("   "),
        Span::styled(format!("{count}"), theme::muted()),
    ])
}

/// Capped variant: `Actions  6 of 29`. Used on the empty state when the
/// ACTIONS section is showing only the top-N entries.
fn section_header_line_capped(label: &str, shown: usize, total: usize) -> Line<'static> {
    let pretty = pretty_section_label(label);
    Line::from(vec![
        Span::raw("   "),
        Span::styled(pretty, theme::muted().add_modifier(Modifier::BOLD)),
        Span::raw("   "),
        Span::styled(format!("{shown} of {total}"), theme::muted()),
    ])
}

fn pretty_section_label(label: &str) -> String {
    if label.is_empty() {
        return String::new();
    }
    let lower = label.to_lowercase();
    let mut chars = lower.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Render one result row. `strip_category_prefix` drops the leading
/// `Category: ` from action labels because the subgroup header already
/// shows the category.
fn render_hit_row(
    hit: &JumpHit,
    query: &str,
    width: usize,
    strip_category_prefix: bool,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::raw("   "));

    let (primary, secondary, tail) = hit_columns(hit, strip_category_prefix);
    let (_, effective_query) = parse_query_scope(query);
    extend_with_match_highlight(&mut spans, &primary, effective_query);

    // Match-source hint when host matched via a non-visible field (User,
    // ProxyJump, VaultSsh, IdentityFile).
    if let JumpHit::Host(h) = hit
        && let Some(src) = match_source_for_host(h, effective_query)
    {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            match_source_hint(src, h),
            theme::muted().add_modifier(Modifier::DIM),
        ));
    }

    if !secondary.is_empty() {
        spans.push(Span::raw("   "));
        spans.push(Span::styled(secondary, theme::muted()));
    }

    if let Some(t) = tail {
        let used = compute_row_width(&spans) + t.chars().count() + 3;
        if used < width {
            spans.push(Span::raw(" ".repeat(width.saturating_sub(used))));
            spans.push(Span::styled(t, theme::muted()));
        } else {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(t, theme::muted()));
        }
    }

    if let JumpHit::Action(a) = hit {
        let key_str = a.key.to_string();
        let used = compute_row_width(&spans) + key_str.chars().count() + 3;
        if used < width {
            spans.push(Span::raw(" ".repeat(width.saturating_sub(used))));
            spans.push(Span::styled(key_str, theme::accent_bold()));
        }
    }

    Line::from(spans)
}

fn match_source_hint(src: JumpMatchSource, h: &crate::app::HostHit) -> String {
    match src {
        JumpMatchSource::User => format!("via {}", h.user),
        JumpMatchSource::ProxyJump => format!("via {}", h.proxy_jump),
        JumpMatchSource::VaultSsh => format!("vault: {}", h.vault_ssh.as_deref().unwrap_or("")),
        JumpMatchSource::IdentityFile => format!("key {}", h.identity_file),
    }
}

fn compute_row_width(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.content.chars().count()).sum()
}

fn hit_columns(hit: &JumpHit, strip_category_prefix: bool) -> (String, String, Option<String>) {
    match hit {
        JumpHit::Action(a) => {
            let label = if strip_category_prefix {
                a.label
                    .split_once(':')
                    .map(|(_, rest)| rest.trim_start().to_string())
                    .unwrap_or_else(|| a.label.to_string())
            } else {
                a.label.to_string()
            };
            (label, String::new(), None)
        }
        JumpHit::Host(h) => {
            let primary = h.alias.clone();
            let tail = if h.hostname.is_empty() {
                None
            } else {
                Some(h.hostname.clone())
            };
            // Tags omitted from the secondary slot for visual minimalism;
            // they're still searchable. Hostname tail keeps the address
            // information density.
            (primary, String::new(), tail)
        }
        JumpHit::Tunnel(t) => {
            let primary = t.alias.clone();
            let secondary = format!("{} \u{2192} {}", t.bind_port, t.destination);
            let tail = Some(if t.active {
                "live".into()
            } else {
                "idle".into()
            });
            (primary, secondary, tail)
        }
        JumpHit::Container(c) => {
            let primary = format!("{} / {}", c.alias, c.container_name);
            (primary, String::new(), Some(c.state.clone()))
        }
        JumpHit::Snippet(s) => (
            s.name.clone(),
            String::new(),
            Some(s.command_preview.clone()),
        ),
    }
}

/// Highlight matched chars. For verbatim substring matches we highlight
/// the contiguous run only (cleaner visual). For genuine fuzzy matches
/// the per-char fallback highlights each matched letter.
fn extend_with_match_highlight(spans: &mut Vec<Span<'static>>, text: &str, query: &str) {
    if text.is_empty() {
        return;
    }
    if query.is_empty() {
        spans.push(Span::styled(text.to_string(), theme::brand()));
        return;
    }
    let highlight = theme::accent_bold().add_modifier(Modifier::UNDERLINED);
    let base = theme::brand();

    // Fast path: verbatim substring (case-insensitive) -> highlight just
    // that run. Reads cleaner than scattered chars.
    let lower_text = text.to_lowercase();
    let lower_query = query.to_lowercase();
    if let Some(byte_pos) = lower_text.find(&lower_query) {
        let char_start = lower_text[..byte_pos].chars().count();
        let char_end = char_start + lower_query.chars().count();
        let mut before = String::new();
        let mut middle = String::new();
        let mut after = String::new();
        for (i, c) in text.chars().enumerate() {
            if i < char_start {
                before.push(c);
            } else if i < char_end {
                middle.push(c);
            } else {
                after.push(c);
            }
        }
        if !before.is_empty() {
            spans.push(Span::styled(before, base));
        }
        if !middle.is_empty() {
            spans.push(Span::styled(middle, highlight));
        }
        if !after.is_empty() {
            spans.push(Span::styled(after, base));
        }
        return;
    }

    // Fuzzy path: walk query chars left-to-right, highlight each match.
    let lower_q: Vec<char> = query.chars().flat_map(|c| c.to_lowercase()).collect();
    let mut q_idx = 0usize;
    let mut buf = String::new();
    let mut current_highlight = false;
    for c in text.chars() {
        let is_match = q_idx < lower_q.len() && c.to_lowercase().any(|cc| cc == lower_q[q_idx]);
        if is_match {
            q_idx += 1;
        }
        if is_match != current_highlight {
            if !buf.is_empty() {
                let style = if current_highlight { highlight } else { base };
                spans.push(Span::styled(buf.clone(), style));
                buf.clear();
            }
            current_highlight = is_match;
        }
        buf.push(c);
    }
    if !buf.is_empty() {
        let style = if current_highlight { highlight } else { base };
        spans.push(Span::styled(buf, style));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::JumpState;

    fn test_app() -> App {
        let config = crate::ssh_config::model::SshConfigFile {
            elements: Vec::new(),
            path: tempfile::tempdir()
                .expect("tempdir")
                .keep()
                .join("purple_jump_test"),
            crlf: false,
            bom: false,
        };
        let mut app = App::new(config);
        app.jump = Some(JumpState::default());
        app.recompute_jump_hits();
        app
    }

    #[test]
    fn jump_renders_without_panic() {
        let mut app = test_app();
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &mut app)).unwrap();
    }

    #[test]
    fn jump_renders_action_section_when_no_filter() {
        let mut app = test_app();
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = buf.content.iter().map(|c| c.symbol().to_string()).collect();
        assert!(
            text.contains("Actions"),
            "should show Actions section header"
        );
    }

    #[test]
    fn jump_includes_whats_new() {
        let actions = crate::app::JumpAction::all();
        assert!(
            actions
                .iter()
                .any(|a| a.key == 'n' && a.label.contains("What's new")),
            "jump bar must include what's new action"
        );
    }

    #[test]
    fn jump_renders_filtered_hits() {
        let mut app = test_app();
        if let Some(p) = app.jump.as_mut() {
            for c in "browse".chars() {
                p.push_query(c);
            }
        }
        app.recompute_jump_hits();
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = buf.content.iter().map(|c| c.symbol().to_string()).collect();
        assert!(
            text.contains("Browse remote files"),
            "Files action should match alias 'browse'"
        );
    }

    #[test]
    fn jump_renders_empty_state() {
        let mut app = test_app();
        if let Some(p) = app.jump.as_mut() {
            for c in "zzzqqq".chars() {
                p.push_query(c);
            }
        }
        app.recompute_jump_hits();
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = buf.content.iter().map(|c| c.symbol().to_string()).collect();
        assert!(
            text.contains("no matches") || text.contains(crate::i18n::text("No matches")),
            "should render no-matches placeholder"
        );
    }

    #[test]
    fn highlight_substring_run_only() {
        // Verbatim substring path: only the matched contiguous run is
        // highlighted, not stray chars elsewhere in the text.
        let mut spans: Vec<Span<'static>> = Vec::new();
        extend_with_match_highlight(&mut spans, "vault-node-01-ams3", "ams");
        let highlighted: String = spans
            .iter()
            .filter(|s| s.style.add_modifier.contains(Modifier::UNDERLINED))
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(
            highlighted, "ams",
            "substring run should highlight 'ams' only, not stray a"
        );
    }

    #[test]
    fn highlight_fuzzy_walks_per_char_when_no_substring() {
        // No verbatim substring -> per-char fuzzy highlight.
        let mut spans: Vec<Span<'static>> = Vec::new();
        extend_with_match_highlight(&mut spans, "Open files", "of");
        let combined: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(combined, "Open files");
    }

    #[test]
    fn highlight_empty_text_yields_no_spans() {
        let mut spans: Vec<Span<'static>> = Vec::new();
        extend_with_match_highlight(&mut spans, "", "anything");
        assert!(spans.is_empty(), "empty haystack should add nothing");
    }

    #[test]
    fn highlight_empty_query_emits_single_base_span() {
        let mut spans: Vec<Span<'static>> = Vec::new();
        extend_with_match_highlight(&mut spans, "Open files", "");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.as_ref(), "Open files");
    }

    #[test]
    fn pretty_section_label_cases() {
        assert_eq!(pretty_section_label("RECENT"), "Recent");
        assert_eq!(pretty_section_label("ACTIONS"), "Actions");
        assert_eq!(pretty_section_label("HOSTS"), "Hosts");
        assert_eq!(pretty_section_label(""), "");
        assert_eq!(pretty_section_label("a"), "A");
    }

    #[test]
    fn empty_state_caps_actions_and_shows_relational_count() {
        // With ~29 actions in the unified set, the empty state caps to
        // crate::app::jump::JUMP_EMPTY_STATE_ACTIONS_CAP and the header reads `Actions  N of 29`.
        let total = crate::app::JumpAction::all().len();
        assert!(
            total > crate::app::jump::JUMP_EMPTY_STATE_ACTIONS_CAP,
            "this test assumes the unified set has more actions than the cap"
        );
        let mut app = test_app();
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = buf.content.iter().map(|c| c.symbol().to_string()).collect();
        // Header advertises the relational `N of TOTAL` count.
        assert!(
            text.contains(&format!("of {total}")),
            "Actions header must show `... of {total}`, got: {text:?}"
        );
        // Old tease line must NOT appear.
        assert!(
            !text.contains("start typing to filter"),
            "tease line should be gone — header carries the cap signal now"
        );
    }

    #[test]
    fn empty_state_has_no_default_selection() {
        // Fresh empty open: `cursor_revealed` is false, the renderer
        // suppresses the selection cue, eye stays on the input field.
        // The state-flag invariant is the source of truth — handler
        // tests cover the Down-keystroke transition.
        let app = test_app();
        let _ = app; // app is kept alive but state suffices for the assert.
        let mut app = test_app();
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        assert!(!app.jump.as_ref().unwrap().cursor_revealed());
    }

    #[test]
    fn jump_footer_advertises_select_next_close() {
        // Renamed from `..._open_close`: Enter-label normalised to `select`
        // across pickers per design-system-reference.md (single canonical
        // verb for "open the focused item's detail").
        let mut app = test_app();
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = buf.content.iter().map(|c| c.symbol().to_string()).collect();
        for label in ["select", "next", "close"] {
            assert!(
                text.contains(label),
                "footer must advertise '{label}', got: {text:?}"
            );
        }
    }

    #[test]
    fn jump_placeholder_reads_find_anything() {
        let mut app = test_app();
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = buf.content.iter().map(|c| c.symbol().to_string()).collect();
        assert!(
            text.contains("Find anything"),
            "empty-state placeholder must read 'Find anything'"
        );
    }

    #[test]
    fn empty_state_action_top_n_round_robins_categories() {
        // Hosts mode (default): the top-N must NOT be six host actions
        // in a row; we want a taste of every category. With ~6
        // categories and the cap at 6, each visible action should come
        // from a distinct category.
        let mut app = test_app();
        app.recompute_jump_hits();
        let groups = app.jump.as_ref().unwrap().empty_state_groups();
        let actions = groups
            .iter()
            .find(|(l, _)| *l == "ACTIONS")
            .map(|(_, h)| h.clone())
            .unwrap_or_default();
        assert!(actions.len() >= crate::app::jump::JUMP_EMPTY_STATE_ACTIONS_CAP);
        let mut categories = std::collections::HashSet::new();
        for hit in actions
            .iter()
            .take(crate::app::jump::JUMP_EMPTY_STATE_ACTIONS_CAP)
        {
            if let crate::app::JumpHit::Action(a) = hit {
                let cat = a.label.split_once(':').map(|(c, _)| c.trim().to_string());
                categories.insert(cat);
            }
        }
        let cap = crate::app::jump::JUMP_EMPTY_STATE_ACTIONS_CAP;
        assert!(
            categories.len() >= 4,
            "top-{cap} should sample at least 4 distinct categories, got {categories:?}"
        );
    }

    #[test]
    fn empty_state_biases_containers_actions_when_opened_from_containers_tab() {
        // Tab-aware empty state: the first three slots must surface
        // `Containers:` actions when the bar is opened with
        // `JumpMode::Containers` so the user sees tab-relevant actions
        // before the cross-tab hub menu.
        let mut app = test_app();
        app.jump = Some(JumpState::for_mode(crate::app::JumpMode::Containers));
        app.recompute_jump_hits();
        let actions = app
            .jump
            .as_ref()
            .unwrap()
            .empty_state_groups()
            .into_iter()
            .find(|(l, _)| *l == "ACTIONS")
            .map(|(_, h)| h)
            .unwrap_or_default();
        let leading_categories: Vec<String> = actions
            .iter()
            .take(3)
            .filter_map(|h| match h {
                crate::app::JumpHit::Action(a) => {
                    Some(a.label.split_once(':').map(|(c, _)| c.trim().to_string())?)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            leading_categories,
            vec![
                crate::i18n::text("Containers").to_string(),
                crate::i18n::text("Containers").to_string(),
                crate::i18n::text("Containers").to_string()
            ],
            "first three slots must be Containers actions on the containers tab"
        );
    }

    #[test]
    fn empty_state_biases_tunnel_actions_when_opened_from_tunnels_tab() {
        let mut app = test_app();
        app.jump = Some(JumpState::for_mode(crate::app::JumpMode::Tunnels));
        app.recompute_jump_hits();
        let actions = app
            .jump
            .as_ref()
            .unwrap()
            .empty_state_groups()
            .into_iter()
            .find(|(l, _)| *l == "ACTIONS")
            .map(|(_, h)| h)
            .unwrap_or_default();
        let leading_categories: Vec<String> = actions
            .iter()
            .take(3)
            .filter_map(|h| match h {
                crate::app::JumpHit::Action(a) => {
                    Some(a.label.split_once(':').map(|(c, _)| c.trim().to_string())?)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            leading_categories,
            vec![
                crate::i18n::text("Tunnels").to_string(),
                crate::i18n::text("Tunnels").to_string(),
                crate::i18n::text("Tunnels").to_string()
            ],
            "first three slots must be Tunnels actions on the tunnels tab"
        );
    }

    #[test]
    fn empty_state_hosts_mode_keeps_hub_distribution() {
        // Hosts (default) mode is the discovery hub. The first three
        // slots must each come from a different category, NOT three
        // Hosts actions in a row.
        let mut app = test_app();
        app.jump = Some(JumpState::for_mode(crate::app::JumpMode::Hosts));
        app.recompute_jump_hits();
        let actions = app
            .jump
            .as_ref()
            .unwrap()
            .empty_state_groups()
            .into_iter()
            .find(|(l, _)| *l == "ACTIONS")
            .map(|(_, h)| h)
            .unwrap_or_default();
        let leading_categories: Vec<String> = actions
            .iter()
            .take(3)
            .filter_map(|h| match h {
                crate::app::JumpHit::Action(a) => {
                    Some(a.label.split_once(':').map(|(c, _)| c.trim().to_string())?)
                }
                _ => None,
            })
            .collect();
        let unique: std::collections::HashSet<&String> = leading_categories.iter().collect();
        assert_eq!(
            unique.len(),
            3,
            "hosts mode must keep hub distribution: 3 distinct categories in first 3 slots, got {leading_categories:?}"
        );
    }

    #[test]
    fn visible_hits_and_empty_state_groups_agree_on_actions() {
        // Regression: the navigation cursor (`visible_hits`) must walk
        // the same action list the renderer (`empty_state_groups`) shows.
        // The shared `empty_state_actions` helper guarantees this; this
        // test pins the invariant against future refactors.
        for mode in [
            crate::app::JumpMode::Hosts,
            crate::app::JumpMode::Tunnels,
            crate::app::JumpMode::Containers,
        ] {
            let mut app = test_app();
            app.jump = Some(JumpState::for_mode(mode));
            app.recompute_jump_hits();
            let visible = app.jump.as_ref().unwrap().visible_hits();
            let group_actions = app
                .jump
                .as_ref()
                .unwrap()
                .empty_state_groups()
                .into_iter()
                .find(|(l, _)| *l == "ACTIONS")
                .map(|(_, h)| h)
                .unwrap_or_default();
            // visible_hits prepends recents; with no seeded recents the
            // tail should equal the rendered ACTIONS list.
            let visible_actions: Vec<_> = visible
                .iter()
                .filter(|h| matches!(h, crate::app::JumpHit::Action(_)))
                .cloned()
                .collect();
            assert_eq!(
                visible_actions, group_actions,
                "visible_hits actions must equal empty_state_groups ACTIONS for mode {mode:?}"
            );
        }
    }
}
