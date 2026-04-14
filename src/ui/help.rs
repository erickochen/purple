use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use super::theme;
use crate::app::{App, Screen};

pub fn render(frame: &mut Frame, app: &mut App) {
    let return_screen = match &app.screen {
        Screen::Help { return_screen } => return_screen.as_ref(),
        _ => return,
    };

    let title_text = context_title(return_screen);
    let is_host_list = matches!(return_screen, Screen::HostList | Screen::Welcome { .. });
    let use_two_cols = is_host_list && frame.area().width >= 96;

    let (col1, col2) = if is_host_list {
        host_list_columns()
    } else {
        let lines = match return_screen {
            Screen::FileBrowser { .. } => file_browser_lines(),
            Screen::SnippetPicker { .. } => snippet_picker_lines(),
            Screen::SnippetOutput { .. } => snippet_output_lines(),
            Screen::Containers { .. } => containers_lines(),
            Screen::TunnelList { .. } => tunnels_lines(),
            Screen::Providers => providers_lines(),
            Screen::KeyList => key_list_lines(),
            Screen::KeyDetail { .. } => key_detail_lines(),
            Screen::HostDetail { .. } => host_detail_lines(),
            Screen::TagPicker => tag_picker_lines(),
            Screen::BulkTagEditor => bulk_tag_editor_lines(),
            Screen::ThemePicker => vec![
                help_line("j/k ↑↓", "up / down"),
                help_line("Enter", "select theme"),
                help_line("?", "help"),
                help_line("Esc", "cancel"),
            ],
            _ => vec![],
        };
        (lines, vec![])
    };

    let total_lines = if use_two_cols {
        col1.len().max(col2.len()) as u16
    } else if col2.is_empty() {
        col1.len() as u16
    } else {
        (col1.len() + col2.len()) as u16
    };

    let overlay_width = if is_host_list {
        88u16.min(frame.area().width.saturating_sub(4))
    } else {
        50u16.min(frame.area().width.saturating_sub(4))
    };

    // chrome = non-content rows the overlay consumes regardless of content.
    // Host list: 2 borders + top + 2 above info + 2 info + 2 above footer
    // + footer + 1 bottom = 11.
    // Others:   2 borders + top + spacer + footer = 5.
    let chrome = if is_host_list { 11 } else { 5 };
    let max_body = frame.area().height.saturating_sub(chrome);
    let height = (total_lines + chrome).min(frame.area().height.saturating_sub(2));
    let area = super::centered_rect_fixed(overlay_width, height, frame.area());

    frame.render_widget(Clear, area);

    let title = Span::styled(format!(" {} ", title_text), theme::brand());
    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title)
        .border_style(theme::accent());
    if is_host_list {
        let version = Line::from(vec![
            Span::styled(format!(" v{}", env!("CARGO_PKG_VERSION")), theme::version()),
            Span::styled(
                format!(" (built {}) ", env!("PURPLE_BUILD_DATE")),
                theme::muted(),
            ),
        ]);
        block = block.title_bottom(version.right_aligned());
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = if is_host_list {
        Layout::vertical([
            Constraint::Length(1), // top breathing (tighter, was 2)
            Constraint::Min(0),    // content cols
            Constraint::Length(2), // breathing above info
            Constraint::Length(2), // wiki + issues info rows
            Constraint::Length(2), // breathing above footer
            Constraint::Length(1), // footer
            Constraint::Length(1), // bottom breathing (tighter, was 2)
        ])
        .split(inner)
    } else {
        Layout::vertical([
            Constraint::Length(1), // top breathing
            Constraint::Min(0),    // content
            Constraint::Length(1), // spacer
            Constraint::Length(1), // footer
        ])
        .split(inner)
    };

    let max_scroll = total_lines.saturating_sub(max_body);
    if app.ui.help_scroll > max_scroll {
        app.ui.help_scroll = max_scroll;
    }

    // Fixed content widths: col1 fits its longest line
    // (`group (off/provider/tag)` plus key chrome = 36), col2 fits its
    // longest header (`CONNECT AND RUN` indented 9 = 24). Equal Fill(1)
    // margins on either side centre the whole block horizontally inside
    // the overlay so left and right whitespace are visually balanced.
    const COL1_W: u16 = 36;
    const COL_GAP: u16 = 4;
    const COL2_W: u16 = 24;
    const CONTENT_W: u16 = COL1_W + COL_GAP + COL2_W;

    if use_two_cols {
        let cols = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(COL1_W),
            Constraint::Length(COL_GAP),
            Constraint::Length(COL2_W),
            Constraint::Fill(1),
        ])
        .split(rows[1]);
        let para1 = Paragraph::new(col1).scroll((app.ui.help_scroll, 0));
        let para2 = Paragraph::new(col2).scroll((app.ui.help_scroll, 0));
        frame.render_widget(para1, cols[1]);
        frame.render_widget(para2, cols[3]);
    } else if col2.is_empty() {
        let para = Paragraph::new(col1).scroll((app.ui.help_scroll, 0));
        frame.render_widget(para, rows[1]);
    } else {
        let mut all = col1;
        all.extend(col2);
        let para = Paragraph::new(all).scroll((app.ui.help_scroll, 0));
        frame.render_widget(para, rows[1]);
    }

    // Wiki + issues info block. The wiki line clearly tells the user this
    // overlay shows basic commands and the wiki has them all, while the
    // issues line stays the bug-report prompt. Both lines share the same
    // left margin as the content columns so everything is left-aligned on a
    // single imaginary vertical line. Host-list only (subscreen overlays
    // are too narrow for the full URLs).
    if is_host_list {
        let info_area = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(CONTENT_W),
            Constraint::Fill(1),
        ])
        .split(rows[3]);
        // Prefixes padded to the same column so both URLs line up exactly.
        // "All commands and docs:" = 22 chars + 2 spaces = 24.
        // "Got an idea or a bug?" = 21 chars + 3 spaces = 24.
        let info_lines = vec![
            Line::from(vec![
                Span::styled("All commands and docs:  ", theme::muted()),
                Span::styled("github.com/erickochen/purple/wiki", theme::muted()),
            ]),
            Line::from(vec![
                Span::styled("Got an idea or a bug?   ", theme::muted()),
                Span::styled("github.com/erickochen/purple/issues", theme::muted()),
            ]),
        ];
        frame.render_widget(Paragraph::new(info_lines), info_area[1]);
    }

    let can_scroll = total_lines > max_body;
    let mut spans: Vec<Span<'_>> = Vec::new();
    if can_scroll {
        let [k, l] = super::footer_action("j/k", " scroll ");
        spans.extend([k, l]);
        let position = app.ui.help_scroll.saturating_add(1);
        let max = max_scroll.saturating_add(1);
        spans.push(Span::styled(
            format!(" [{}/{}]", position, max),
            theme::muted(),
        ));
        spans.push(Span::raw("  "));
    } else {
        spans.push(Span::raw(" "));
    }
    let [k, l] = super::footer_action("Esc", " close");
    spans.extend([k, l]);
    let footer_row = if is_host_list { rows[5] } else { rows[3] };
    super::render_footer_with_status(frame, footer_row, spans, app);
}

fn context_title(screen: &Screen) -> &'static str {
    match screen {
        // The host list is purple's main screen, so its help pane title is
        // simply `Help` — no need to label "which" help, there is no other
        // help the user could mean from here.
        Screen::HostList | Screen::Welcome { .. } => "Help",
        Screen::FileBrowser { .. } => "File Explorer",
        Screen::SnippetPicker { .. } => "Snippets",
        Screen::SnippetOutput { .. } => "Output",
        Screen::Containers { .. } => "Containers",
        Screen::TunnelList { .. } => "Tunnels",
        Screen::Providers => "Providers",
        Screen::KeyList => "SSH Keys",
        Screen::KeyDetail { .. } => "Key Detail",
        Screen::HostDetail { .. } => "All Directives",
        Screen::TagPicker => "Tags",
        Screen::BulkTagEditor => "Bulk tags",
        Screen::ThemePicker => "Theme",
        _ => "Help",
    }
}

fn section_header(label: &str) -> Line<'static> {
    // Flush-left within the column. Matches the left edge of the info
    // block ("All commands and docs:" / "Got an idea or a bug?") and the
    // footer ("Esc close") so every group of content shares the same
    // visual left gutter.
    Line::from(Span::styled(label.to_string(), theme::section_header()))
}

fn help_line(key: &str, desc: &str) -> Line<'static> {
    help_line_w(key, desc, 9)
}

fn help_line_short(key: &str, desc: &str) -> Line<'static> {
    // Narrower key column for columns that hold short keys only
    // (`^Space`, `q/Esc`, `r`, `R`, `^A`, `p/P`, single-letter shortcuts).
    // Reclaims horizontal room for the description text.
    help_line_w(key, desc, 6)
}

fn help_line_w(key: &str, desc: &str, width: usize) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!(" {:>width$}  ", key, width = width),
            theme::accent_bold(),
        ),
        Span::styled(desc.to_string(), theme::muted()),
    ])
}

fn blank() -> Line<'static> {
    Line::from("")
}

fn host_list_columns() -> (Vec<Line<'static>>, Vec<Line<'static>>) {
    // Essentials only. Headers in col2 are shifted down so CONNECT AND RUN
    // vertically aligns with VIEW (col1) and TOOLS aligns with CLIPBOARD.
    // The command palette (`:`) sits at the very bottom of col1, vertically
    // aligned with `q/Esc quit` in col2, so the last row of both columns is
    // a close/leave action. `r/R` mirrors `p/P` — single-target vs all.
    let col1 = vec![
        blank(),                    // row 0
        section_header("NAVIGATE"), // row 1
        blank(),                    // row 2
        help_line("j/k ↑↓", "up / down"),
        help_line("PgDn/PgUp", "page down / up"),
        help_line("Enter", "connect"),
        help_line("/", "search (scoped)"),
        help_line("#", "tag picker"),
        help_line("Esc", "clear filter / quit"),
        blank(),                // row 9
        section_header("VIEW"), // row 10 ↔ col2 CONNECT AND RUN
        blank(),
        help_line("v", "detail panel"),
        help_line("s", "cycle sort"),
        help_line("g", "group (off/provider/tag)"),
        blank(),                     // row 15
        section_header("CLIPBOARD"), // row 16 ↔ col2 TOOLS
        blank(),
        help_line("y", "copy ssh command"),
        blank(),
        blank(),
        blank(),
        blank(),
        help_line(":", "command palette"), // row 23 ↔ col2 q/Esc quit
    ];

    let col2 = vec![
        blank(),                        // row 0
        section_header("MANAGE HOSTS"), // row 1 ↔ col1 NAVIGATE
        blank(),
        help_line_short("a", "add host"),
        help_line_short("e", "edit"),
        help_line_short("d", "del"),
        help_line_short("u", "undo del"),
        help_line_short("t", "tag (bulk if sel.)"),
        blank(),
        blank(),                           // row 9  padding so headers align
        section_header("CONNECT AND RUN"), // row 10 ↔ col1 VIEW
        blank(),
        help_line_short("Space", "multi-select"),
        help_line_short("r/R", "snippet / all"),
        help_line_short("p/P", "ping / all"),
        blank(),
        section_header("TOOLS"), // row 16 ↔ col1 CLIPBOARD
        blank(),
        help_line_short("F", "file explorer"),
        help_line_short("T", "tunnels"),
        help_line_short("C", "containers"),
        help_line_short("K", "SSH keys"),
        help_line_short("S", "providers"),
        help_line_short("q/Esc", "quit"), // row 23 ↔ col1 `:`
    ];

    (col1, col2)
}

fn file_browser_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("Tab", "switch pane"));
    lines.push(help_line("j/k ↑↓", "up / down"));
    lines.push(help_line("Enter", "open dir / copy"));
    lines.push(help_line("Backspace", "go up"));
    lines.push(help_line("^Space", "select / deselect"));
    lines.push(help_line("^A", "select all / none"));
    lines.push(help_line(".", "toggle hidden"));
    lines.push(help_line("s", "cycle sort"));
    lines.push(help_line("R", "refresh"));
    lines.push(help_line("PgDn/PgUp", "page down / up"));
    lines.push(help_line("?", "help"));
    lines.push(help_line("q/Esc", "close"));
    lines
}

fn snippet_picker_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("Enter", "run (captured)"));
    lines.push(help_line("!", "run (raw terminal)"));
    lines.push(help_line("/", "search"));
    lines.push(help_line("a", "add snippet"));
    lines.push(help_line("e", "edit"));
    lines.push(help_line("d", "del"));
    lines.push(help_line("j/k ↑↓", "up / down"));
    lines.push(help_line("PgDn/PgUp", "page down / up"));
    lines.push(help_line("?", "help"));
    lines.push(help_line("q/Esc", "close"));
    lines
}

fn snippet_output_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("G/g", "end / start"));
    lines.push(help_line("n/N", "next / prev host"));
    lines.push(help_line("c", "copy output"));
    lines.push(help_line("j/k ↑↓", "scroll"));
    lines.push(help_line("PgDn/PgUp", "page down / up"));
    lines.push(help_line("?", "help"));
    lines.push(help_line("q/Esc", "close / cancel"));
    lines
}

fn containers_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("j/k ↑↓", "up / down"));
    lines.push(help_line("s", "start"));
    lines.push(help_line("x", "stop"));
    lines.push(help_line("r", "restart"));
    lines.push(help_line("R", "refresh"));
    lines.push(help_line("PgDn/PgUp", "page down / up"));
    lines.push(help_line("?", "help"));
    lines.push(help_line("q/Esc", "close"));
    lines
}

fn tunnels_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("j/k ↑↓", "up / down"));
    lines.push(help_line("a", "add tunnel"));
    lines.push(help_line("e", "edit"));
    lines.push(help_line("d", "del"));
    lines.push(help_line("Enter", "start / stop"));
    lines.push(help_line("PgDn/PgUp", "page down / up"));
    lines.push(help_line("?", "help"));
    lines.push(help_line("q/Esc", "close"));
    lines
}

fn key_list_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("j/k ↑↓", "up / down"));
    lines.push(help_line("Enter", "view detail"));
    lines.push(help_line("PgDn/PgUp", "page down / up"));
    lines.push(help_line("?", "help"));
    lines.push(help_line("q/Esc", "close"));
    lines
}

fn key_detail_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("?", "help"));
    lines.push(help_line("q/Esc", "close"));
    lines
}

fn host_detail_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("e", "edit host"));
    lines.push(help_line("r", "run snippet"));
    lines.push(help_line("T", "tunnels"));
    lines.push(help_line("?", "help"));
    lines.push(help_line("q/Esc/i", "close"));
    lines
}

fn tag_picker_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("j/k ↑↓", "up / down"));
    lines.push(help_line("Enter", "filter by tag"));
    lines.push(help_line("PgDn/PgUp", "page down / up"));
    lines.push(help_line("?", "help"));
    lines.push(help_line("q/Esc/#", "close"));
    lines
}

fn bulk_tag_editor_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("j/k ↑↓", "up / down"));
    lines.push(help_line("Space", "cycle [~] [x] [ ]"));
    lines.push(help_line("+", "new tag"));
    lines.push(help_line("Enter", "apply"));
    lines.push(help_line("?", "help"));
    lines.push(help_line("q/Esc", "cancel"));
    lines
}

fn providers_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("j/k ↑↓", "up / down"));
    lines.push(help_line("Enter", "configure"));
    lines.push(help_line("s", "sync"));
    lines.push(help_line("d", "del config"));
    lines.push(help_line("X", "purge stale"));
    lines.push(help_line("PgDn/PgUp", "page down / up"));
    lines.push(help_line("?", "help"));
    lines.push(help_line("q/Esc", "close"));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Screen;
    use ratatui::layout::Rect;
    use ratatui::style::Modifier;

    #[test]
    fn host_list_produces_two_column_groups() {
        let (col1, col2) = host_list_columns();
        assert!(!col1.is_empty(), "column 1 should have content");
        assert!(!col2.is_empty(), "column 2 should have content");
    }

    #[test]
    fn file_browser_produces_content() {
        let lines = file_browser_lines();
        assert!(!lines.is_empty());
        let text: String = lines.iter().map(|l| l.to_string()).collect();
        assert!(text.contains("switch pane"), "should have Tab shortcut");
    }

    #[test]
    fn snippet_picker_produces_content() {
        let lines = snippet_picker_lines();
        assert!(!lines.is_empty());
        let text: String = lines.iter().map(|l| l.to_string()).collect();
        assert!(
            text.contains("run (captured)"),
            "should have Enter shortcut"
        );
    }

    #[test]
    fn snippet_output_produces_content() {
        let lines = snippet_output_lines();
        assert!(!lines.is_empty());
        let text: String = lines.iter().map(|l| l.to_string()).collect();
        assert!(text.contains("copy output"), "should have copy shortcut");
    }

    #[test]
    fn containers_produces_content() {
        let lines = containers_lines();
        assert!(!lines.is_empty());
        let text: String = lines.iter().map(|l| l.to_string()).collect();
        assert!(text.contains("start"), "should have start shortcut");
    }

    #[test]
    fn tunnels_produces_content() {
        let lines = tunnels_lines();
        assert!(!lines.is_empty());
        let text: String = lines.iter().map(|l| l.to_string()).collect();
        assert!(text.contains("add tunnel"), "should have add shortcut");
    }

    #[test]
    fn section_header_is_bold() {
        let line = section_header("TEST");
        let header_span = &line.spans[0];
        assert!(
            header_span.style.add_modifier.contains(Modifier::BOLD),
            "header should be bold"
        );
    }

    #[test]
    fn help_line_has_right_aligned_key() {
        let line = help_line("j/k", "up / down");
        let key_text = line.spans[0].to_string();
        assert!(key_text.starts_with(' '), "key should have leading spaces");
        assert!(
            key_text.trim_start().starts_with("j/k"),
            "key content should be j/k"
        );
    }

    #[test]
    fn help_line_description_is_dim() {
        let line = help_line("j/k", "up / down");
        let desc_span = &line.spans[1];
        assert!(
            desc_span.style.add_modifier.contains(Modifier::DIM),
            "description should be dim"
        );
    }

    #[test]
    fn overlay_title_matches_context() {
        assert_eq!(context_title(&Screen::HostList), "Help");
        assert_eq!(
            context_title(&Screen::FileBrowser {
                alias: "test".into()
            }),
            "File Explorer"
        );
        assert_eq!(
            context_title(&Screen::SnippetPicker {
                target_aliases: vec![]
            }),
            "Snippets"
        );
        assert_eq!(
            context_title(&Screen::SnippetOutput {
                snippet_name: "x".into(),
                target_aliases: vec![],
            }),
            "Output"
        );
        assert_eq!(
            context_title(&Screen::Containers {
                alias: "test".into()
            }),
            "Containers"
        );
        assert_eq!(
            context_title(&Screen::TunnelList {
                alias: "test".into()
            }),
            "Tunnels"
        );
    }

    #[test]
    fn host_list_layout_breathing_content_info_footer() {
        // Host list: top(1) + content + breathing(2) + 2 info rows
        // + breathing(2) + footer + bottom(1).
        let area = Rect::new(0, 0, 80, 40);
        let rows = ratatui::layout::Layout::vertical([
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Min(0),
            ratatui::layout::Constraint::Length(2),
            ratatui::layout::Constraint::Length(2),
            ratatui::layout::Constraint::Length(2),
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(1),
        ])
        .split(area);
        assert_eq!(rows[0].height, 1, "top breathing");
        assert_eq!(rows[3].height, 2, "info rows");
        assert_eq!(rows[5].height, 1, "footer");
        assert_eq!(rows[6].height, 1, "bottom breathing");
    }

    #[test]
    fn compact_layout_breathing_content_footer() {
        // Sub-screens: top(1) + content + spacer(1) + footer.
        let area = Rect::new(0, 0, 80, 30);
        let rows = ratatui::layout::Layout::vertical([
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Min(0),
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(1),
        ])
        .split(area);
        assert_eq!(rows[0].height, 1, "top breathing");
        assert_eq!(rows[3].height, 1, "footer");
    }

    // --- Content completeness tests ---

    #[test]
    fn host_list_col2_contains_all_tool_shortcuts() {
        let (col1, col2) = host_list_columns();
        let all_text: String = col1
            .iter()
            .chain(col2.iter())
            .map(|l| l.to_string())
            .collect();
        for desc in &[
            "file explorer",
            "tunnels",
            "containers",
            "SSH keys",
            "providers",
            "copy ssh command",
        ] {
            assert!(all_text.contains(desc), "help columns missing '{}'", desc);
        }
    }

    #[test]
    fn host_list_col1_contains_navigate_and_view() {
        let (col1, _) = host_list_columns();
        let text: String = col1.iter().map(|l| l.to_string()).collect();
        for desc in &[
            "up / down",
            "page down / up",
            "connect",
            "search",
            "tag picker",
            "detail panel",
            "cycle sort",
        ] {
            assert!(text.contains(desc), "col1 missing '{}'", desc);
        }
    }

    // --- Context title fallback ---

    #[test]
    fn context_title_unknown_screen_returns_help() {
        assert_eq!(context_title(&Screen::AddHost), "Help");
    }

    #[test]
    fn context_title_providers_returns_providers() {
        assert_eq!(context_title(&Screen::Providers), "Providers");
    }

    #[test]
    fn context_title_key_list_returns_ssh_keys() {
        assert_eq!(context_title(&Screen::KeyList), "SSH Keys");
    }

    #[test]
    fn context_title_tag_picker_returns_tags() {
        assert_eq!(context_title(&Screen::TagPicker), "Tags");
    }

    #[test]
    fn providers_produces_content() {
        let lines = providers_lines();
        assert!(!lines.is_empty());
        let text: String = lines.iter().map(|l| l.to_string()).collect();
        assert!(text.contains("sync"), "should have sync shortcut");
    }

    #[test]
    fn key_list_produces_content() {
        let lines = key_list_lines();
        assert!(!lines.is_empty());
        let text: String = lines.iter().map(|l| l.to_string()).collect();
        assert!(text.contains("view detail"), "should have Enter shortcut");
    }

    #[test]
    fn tag_picker_produces_content() {
        let lines = tag_picker_lines();
        assert!(!lines.is_empty());
        let text: String = lines.iter().map(|l| l.to_string()).collect();
        assert!(text.contains("filter by tag"), "should have Enter shortcut");
    }

    #[test]
    fn all_subscreens_include_help_shortcut() {
        let cases: Vec<(&str, Vec<Line<'_>>)> = vec![
            ("file_browser", file_browser_lines()),
            ("snippet_picker", snippet_picker_lines()),
            ("snippet_output", snippet_output_lines()),
            ("containers", containers_lines()),
            ("tunnels", tunnels_lines()),
            ("key_list", key_list_lines()),
            ("key_detail", key_detail_lines()),
            ("host_detail", host_detail_lines()),
            ("tag_picker", tag_picker_lines()),
            ("providers", providers_lines()),
        ];
        for (name, lines) in cases {
            let has_help_key = lines.iter().any(|l| {
                l.spans
                    .first()
                    .map(|s| s.content.trim() == "?")
                    .unwrap_or(false)
            });
            assert!(has_help_key, "{name} missing help_line with key '?'");
        }
    }

    #[test]
    fn host_list_contains_arrow_keys() {
        let (col1, _) = host_list_columns();
        let text: String = col1.iter().map(|l| l.to_string()).collect();
        assert!(
            text.contains("\u{2191}\u{2193}"),
            "host list should show arrow key hints"
        );
    }

    fn help_test_app(return_screen: Screen) -> App {
        let config = crate::ssh_config::model::SshConfigFile {
            elements: Vec::new(),
            path: std::path::PathBuf::new(),
            crlf: false,
            bom: false,
        };
        let mut app = App::new(config);
        app.screen = Screen::Help {
            return_screen: Box::new(return_screen),
        };
        app
    }

    fn render_to_text(app: &mut App, width: u16, height: u16) -> String {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol().to_string())
            .collect()
    }

    #[test]
    fn host_list_help_renders_wiki_link() {
        // Feature 6: the redesigned host-list help overlay shows a wiki link
        // so users can discover the full command reference.
        let mut app = help_test_app(Screen::HostList);
        let text = render_to_text(&mut app, 100, 40);
        assert!(
            text.contains("github.com/erickochen/purple/wiki"),
            "host-list help should render wiki link in the info block"
        );
        assert!(
            text.contains("github.com/erickochen/purple/issues"),
            "host-list help should render issues link in the info block"
        );
    }

    #[test]
    fn host_list_help_renders_two_column_section_headers() {
        // Feature 6: centered two-column layout on wide terminals shows
        // both column 1 (NAVIGATE/VIEW/CLIPBOARD) and column 2
        // (MANAGE HOSTS/CONNECT AND RUN/TOOLS) headers.
        let mut app = help_test_app(Screen::HostList);
        let text = render_to_text(&mut app, 100, 40);
        for header in [
            "NAVIGATE",
            "VIEW",
            "CLIPBOARD",
            "MANAGE HOSTS",
            "CONNECT AND RUN",
            "TOOLS",
        ] {
            assert!(
                text.contains(header),
                "host-list help should render section header '{header}'"
            );
        }
    }

    #[test]
    fn host_list_help_fits_chrome_in_tall_terminal() {
        // Feature 6: chrome constant raised from 5 to 11 on the host list
        // overlay to account for the extra breathing rows and info block.
        // Render at a generous 100x50 and assert (1) all content is visible
        // without scrolling and (2) the scroll position stays at zero. If
        // the chrome constant drifts too low, max_body grows and
        // help_scroll can advance past zero. If it drifts too high,
        // content is clipped and section headers will not render.
        let mut app = help_test_app(Screen::HostList);
        let text = render_to_text(&mut app, 100, 50);
        assert_eq!(
            app.ui.help_scroll, 0,
            "host-list help should fit without scrolling at 100x50"
        );
        for header in ["NAVIGATE", "VIEW", "MANAGE HOSTS", "TOOLS"] {
            assert!(
                text.contains(header),
                "host-list help should render '{header}' fully at 100x50"
            );
        }
    }

    #[test]
    fn host_list_help_clamps_stale_scroll_when_content_fits() {
        // Exercise the scroll-clamp branch directly: when content fits the
        // viewport (max_scroll == 0), a previously elevated help_scroll
        // must be clamped back to 0 so the overlay does not render blank.
        let mut app = help_test_app(Screen::HostList);
        app.ui.help_scroll = 999;
        render_to_text(&mut app, 100, 50);
        assert_eq!(
            app.ui.help_scroll, 0,
            "stale scroll must clamp to 0 when content fits"
        );
    }

    #[test]
    fn subscreen_help_renders_without_wiki_block() {
        // Sub-screen overlays are narrower and intentionally omit the wiki
        // and issues lines because the full URLs don't fit.
        let mut app = help_test_app(Screen::Providers);
        let text = render_to_text(&mut app, 80, 30);
        assert!(
            !text.contains("github.com/erickochen/purple/wiki"),
            "sub-screen help should not render wiki link"
        );
    }
}
