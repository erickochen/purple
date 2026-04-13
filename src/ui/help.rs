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
        host_list_columns(app)
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
            Screen::ThemePicker => vec![
                help_line("j/k", "up / down"),
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

    let chrome = if is_host_list { 5 } else { 4 }; // host list: border(2) + footer(1) + 2 spacers; others: border(2) + footer(1) + 1 spacer
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
        let author = Line::from(Span::styled(
            " Bugs or ideas? github.com/erickochen/purple/issues ",
            theme::muted(),
        ));
        let version = Line::from(vec![
            Span::styled(format!(" v{}", env!("CARGO_PKG_VERSION")), theme::version()),
            Span::styled(
                format!(" (built {}) ", env!("PURPLE_BUILD_DATE")),
                theme::muted(),
            ),
        ]);
        block = block
            .title_bottom(author)
            .title_bottom(version.right_aligned());
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = if is_host_list {
        Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1), // spacer above footer
            Constraint::Length(1), // footer
            Constraint::Length(1), // spacer below footer (before bottom border with github url)
        ])
        .split(inner)
    } else {
        Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1), // spacer above footer
            Constraint::Length(1), // footer
        ])
        .split(inner)
    };

    let max_scroll = total_lines.saturating_sub(max_body);
    if app.ui.help_scroll > max_scroll {
        app.ui.help_scroll = max_scroll;
    }

    if use_two_cols {
        let cols = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(rows[0]);
        let para1 = Paragraph::new(col1).scroll((app.ui.help_scroll, 0));
        let para2 = Paragraph::new(col2).scroll((app.ui.help_scroll, 0));
        frame.render_widget(para1, cols[0]);
        frame.render_widget(para2, cols[1]);
    } else if col2.is_empty() {
        let para = Paragraph::new(col1).scroll((app.ui.help_scroll, 0));
        frame.render_widget(para, rows[0]);
    } else {
        let mut all = col1;
        all.extend(col2);
        let para = Paragraph::new(all).scroll((app.ui.help_scroll, 0));
        frame.render_widget(para, rows[0]);
    }

    let can_scroll = total_lines > max_body;
    let mut spans: Vec<Span<'_>> = Vec::new();
    if can_scroll {
        let [k, l] = super::footer_action("j/k", " scroll ");
        spans.extend([k, l]);
        spans.push(Span::raw("  "));
    } else {
        spans.push(Span::raw(" "));
    }
    let [k, l] = super::footer_action("Esc", " close");
    spans.extend([k, l]);
    super::render_footer_with_status(frame, rows[2], spans, app);
}

fn context_title(screen: &Screen) -> &'static str {
    match screen {
        Screen::HostList | Screen::Welcome { .. } => "Host List",
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
        Screen::ThemePicker => "Theme",
        _ => "Help",
    }
}

fn section_header(label: &str) -> Vec<Line<'static>> {
    let rule: String = "\u{2500}".repeat(label.len());
    vec![
        Line::from(Span::styled(
            format!("  {}", label),
            theme::section_header(),
        )),
        Line::from(Span::styled(format!("  {}", rule), theme::muted())),
    ]
}

fn help_line(key: &str, desc: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {:>11}  ", key), theme::accent_bold()),
        Span::styled(desc.to_string(), theme::muted()),
    ])
}

fn blank() -> Line<'static> {
    Line::from("")
}

fn host_list_columns(_app: &App) -> (Vec<Line<'static>>, Vec<Line<'static>>) {
    // Both columns are aligned: section headers at matching heights.
    // Each section pair has matching blank/header lines to stay in sync.
    let mut col1 = vec![blank()];
    let mut col2 = vec![blank()];

    // Row 1: NAVIGATE / MANAGE HOSTS
    col1.extend(section_header("NAVIGATE"));
    col2.extend(section_header("MANAGE HOSTS"));

    col1.push(help_line("j/k", "up / down"));
    col2.push(help_line("a", "add host"));

    col1.push(help_line("PgDn/PgUp", "page down / up"));
    col2.push(help_line("A", "add pattern"));

    col1.push(help_line("Enter", "connect"));
    col2.push(help_line("e", "edit"));

    col1.push(help_line("Tab", "next group"));
    col2.push(help_line("d", "del"));

    col1.push(help_line("Shift+Tab", "prev group"));
    col2.push(help_line("c", "clone"));

    col1.push(help_line("/", "search (scoped)"));
    col2.push(help_line("u", "undo del"));

    col1.push(help_line("#", "filter by tag"));
    col2.push(help_line("t", "tag (inline)"));

    col1.push(help_line("Esc", "clear filter / quit"));
    col2.push(help_line("i", "all directives"));

    // Row 2: VIEW / CONNECT AND RUN
    col1.push(blank());
    col2.push(blank());

    col1.extend(section_header("VIEW"));
    col2.extend(section_header("CONNECT AND RUN"));

    col1.push(help_line("v", "detail panel"));
    col2.push(help_line("^Space", "multi-select"));

    col1.push(help_line("s", "cycle sort"));
    col2.push(help_line("^A", "select all / none"));

    col1.push(help_line("g", "group (off/provider/tag)"));
    col2.push(help_line("r", "run snippet"));

    col1.push(help_line("[ / ]", "scroll detail"));
    col2.push(help_line("R", "run on all visible"));

    col1.push(help_line("tag:name", "fuzzy tag filter"));
    col2.push(help_line("p/P", "ping / all"));

    col1.push(help_line("tag=name", "exact tag filter"));
    col2.push(help_line("!", "down-only filter"));

    // Row 3: CLIPBOARD / TOOLS
    col1.push(blank());
    col2.push(blank());

    col1.extend(section_header("CLIPBOARD"));
    col2.extend(section_header("TOOLS"));

    col1.push(help_line("y", "copy ssh command"));
    col2.push(help_line("F", "file explorer"));

    col1.push(help_line("x", "copy config block"));
    col2.push(help_line("T", "tunnels"));

    col1.push(help_line("X", "purge stale"));
    col2.push(help_line("C", "containers"));

    col1.push(blank());
    col2.push(help_line("K", "SSH keys"));

    // Row 4: FORMS / continued TOOLS + quit
    col1.extend(section_header("FORMS"));
    col2.push(help_line("S", "providers"));

    // Always show V so the Vault SSH feature is discoverable. When no role is
    // configured yet, the keybinding still appears with the same label; the
    // status message produced by the handler explains where to configure it.
    col1.push(blank());
    col2.push(help_line("V", "vault sign"));

    col1.push(help_line("Tab", "next field"));
    col2.push(help_line("I", "import known_hosts"));

    col1.push(help_line("Shift+Tab", "prev field"));
    col2.push(help_line("m", "theme"));

    col1.push(blank());
    col2.push(help_line(":", "search commands"));

    col1.push(help_line("Enter", "save / picker"));
    col2.push(help_line("q/Esc", "quit"));

    col1.push(help_line("Space", "toggle / cycle"));
    col2.push(blank());

    col1.push(help_line("^D", "set default"));
    col2.push(blank());

    col1.push(help_line("Esc", "cancel"));
    col2.push(blank());

    (col1, col2)
}

fn file_browser_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("Tab", "switch pane"));
    lines.push(help_line("j/k", "up / down"));
    lines.push(help_line("Enter", "open dir / copy"));
    lines.push(help_line("Backspace", "go up"));
    lines.push(help_line("^Space", "select / deselect"));
    lines.push(help_line("^A", "select all / none"));
    lines.push(help_line(".", "toggle hidden"));
    lines.push(help_line("s", "cycle sort"));
    lines.push(help_line("R", "refresh"));
    lines.push(help_line("PgDn/PgUp", "page down / up"));
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
    lines.push(help_line("j/k", "up / down"));
    lines.push(help_line("PgDn/PgUp", "page down / up"));
    lines.push(help_line("q/Esc", "close"));
    lines
}

fn snippet_output_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("G/g", "end / start"));
    lines.push(help_line("n/N", "next / prev host"));
    lines.push(help_line("c", "copy output"));
    lines.push(help_line("j/k", "scroll"));
    lines.push(help_line("PgDn/PgUp", "page down / up"));
    lines.push(help_line("q/Esc", "close / cancel"));
    lines
}

fn containers_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("j/k", "up / down"));
    lines.push(help_line("s", "start"));
    lines.push(help_line("x", "stop"));
    lines.push(help_line("r", "restart"));
    lines.push(help_line("R", "refresh"));
    lines.push(help_line("PgDn/PgUp", "page down / up"));
    lines.push(help_line("q/Esc", "close"));
    lines
}

fn tunnels_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("j/k", "up / down"));
    lines.push(help_line("a", "add tunnel"));
    lines.push(help_line("e", "edit"));
    lines.push(help_line("d", "del"));
    lines.push(help_line("Enter", "start / stop"));
    lines.push(help_line("PgDn/PgUp", "page down / up"));
    lines.push(help_line("q/Esc", "close"));
    lines
}

fn key_list_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("j/k", "up / down"));
    lines.push(help_line("Enter", "view detail"));
    lines.push(help_line("PgDn/PgUp", "page down / up"));
    lines.push(help_line("q/Esc", "close"));
    lines
}

fn key_detail_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("q/Esc", "close"));
    lines
}

fn host_detail_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("e", "edit host"));
    lines.push(help_line("r", "run snippet"));
    lines.push(help_line("T", "tunnels"));
    lines.push(help_line("q/Esc/i", "close"));
    lines
}

fn tag_picker_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("j/k", "up / down"));
    lines.push(help_line("Enter", "filter by tag"));
    lines.push(help_line("PgDn/PgUp", "page down / up"));
    lines.push(help_line("q/Esc/#", "close"));
    lines
}

fn providers_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("j/k", "up / down"));
    lines.push(help_line("Enter", "configure"));
    lines.push(help_line("s", "sync"));
    lines.push(help_line("d", "del config"));
    lines.push(help_line("X", "purge stale"));
    lines.push(help_line("PgDn/PgUp", "page down / up"));
    lines.push(help_line("q/Esc", "close"));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Screen;
    use ratatui::layout::Rect;
    use ratatui::style::Modifier;

    fn empty_app() -> App {
        let config = crate::ssh_config::model::SshConfigFile {
            elements: Vec::new(),
            path: std::path::PathBuf::from("/tmp/purple_help_test"),
            crlf: false,
            bom: false,
        };
        App::new(config)
    }

    #[test]
    fn host_list_produces_two_column_groups() {
        let (col1, col2) = host_list_columns(&empty_app());
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
        let lines = section_header("TEST");
        assert_eq!(lines.len(), 2, "header + rule");
        let header_span = &lines[0].spans[0];
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
        assert_eq!(context_title(&Screen::HostList), "Host List");
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
    fn host_list_layout_has_spacers_around_footer() {
        // Host list: content + spacer + footer + spacer (before bottom border with github url)
        let area = Rect::new(0, 0, 80, 30);
        let rows = ratatui::layout::Layout::vertical([
            ratatui::layout::Constraint::Min(0),
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(1),
        ])
        .split(area);
        assert_eq!(rows[1].height, 1, "spacer above footer should be 1 tall");
        assert_eq!(rows[2].height, 1, "footer row should be 1 tall");
        assert_eq!(rows[3].height, 1, "spacer below footer should be 1 tall");
    }

    #[test]
    fn compact_layout_has_spacer_and_footer() {
        // Sub-screens: content + spacer + footer
        let area = Rect::new(0, 0, 80, 30);
        let rows = ratatui::layout::Layout::vertical([
            ratatui::layout::Constraint::Min(0),
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(1),
        ])
        .split(area);
        assert_eq!(rows[1].height, 1, "spacer above footer should be 1 tall");
        assert_eq!(rows[2].height, 1, "footer row should be 1 tall");
    }

    // --- Content completeness tests ---

    #[test]
    fn host_list_col2_contains_all_tool_shortcuts() {
        let (col1, col2) = host_list_columns(&empty_app());
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
            "import known_hosts",
            "purge stale",
            "copy ssh command",
            "copy config block",
        ] {
            assert!(all_text.contains(desc), "help columns missing '{}'", desc);
        }
    }

    #[test]
    fn host_list_col1_contains_navigate_view_forms() {
        let (col1, _) = host_list_columns(&empty_app());
        let text: String = col1.iter().map(|l| l.to_string()).collect();
        for desc in &[
            "up / down",
            "page down / up",
            "connect",
            "search",
            "filter by tag",
            "detail panel",
            "cycle sort",
        ] {
            assert!(text.contains(desc), "col1 missing '{}'", desc);
        }
    }

    // --- Section header rule width ---

    #[test]
    fn section_header_rule_width_matches_label() {
        let lines = section_header("NAVIGATE");
        let rule_text = lines[1].spans[0].content.trim_start();
        let rule_char_count = rule_text.chars().count();
        assert_eq!(rule_char_count, "NAVIGATE".len());
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
    fn host_list_contains_palette_shortcut() {
        let (col1, col2) = host_list_columns(&empty_app());
        let all_text: String = col1
            .iter()
            .chain(col2.iter())
            .map(|l| l.to_string())
            .collect();
        assert!(
            all_text.contains("commands"),
            "help should mention command palette"
        );
    }
}
