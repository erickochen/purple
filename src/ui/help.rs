use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use super::design;
use super::theme;
use crate::app::{App, Screen, TopPage};

#[cfg(not(test))]
fn build_date() -> &'static str {
    env!("PURPLE_BUILD_DATE")
}

#[cfg(test)]
fn build_date() -> &'static str {
    "1 Jan 2000"
}

pub fn render(frame: &mut Frame, app: &mut App) {
    let return_screen = match &app.screen {
        Screen::Help { return_screen } => return_screen.as_ref(),
        _ => return,
    };

    // The tunnels tab shares `Screen::HostList` with the actual host
    // list — the two are distinguished only by `app.top_page`. Detect
    // that here so help dispatches to tunnel content instead of
    // host-list content when the user opened help from the Tunnels tab.
    let is_tunnels_tab =
        matches!(return_screen, Screen::HostList) && matches!(app.top_page, TopPage::Tunnels);
    let is_containers_tab =
        matches!(return_screen, Screen::HostList) && matches!(app.top_page, TopPage::Containers);
    let is_snippets_tab =
        matches!(return_screen, Screen::HostList) && matches!(app.top_page, TopPage::Snippets);
    let is_keys_tab =
        matches!(return_screen, Screen::HostList) && matches!(app.top_page, TopPage::Keys);
    let is_host_list = !is_tunnels_tab
        && !is_containers_tab
        && !is_snippets_tab
        && !is_keys_tab
        && matches!(return_screen, Screen::HostList | Screen::Welcome { .. });
    // All top-level tabs share the same overlay chrome: logo, version,
    // wiki/issues info block, two-column body. Sub-screens use a
    // smaller compact layout.
    let is_main_view =
        is_host_list || is_tunnels_tab || is_containers_tab || is_snippets_tab || is_keys_tab;
    let title_text = if is_tunnels_tab {
        crate::i18n::text("Tunnels")
    } else if is_containers_tab {
        crate::i18n::text("Containers")
    } else if is_snippets_tab {
        crate::i18n::text("Snippets")
    } else if is_keys_tab {
        crate::i18n::text("Keys")
    } else {
        context_title(return_screen)
    };
    let use_two_cols = is_main_view && frame.area().width >= 96;

    let (col1, col2) = if is_host_list {
        host_list_columns()
    } else if is_tunnels_tab {
        tunnels_overview_columns()
    } else if is_containers_tab {
        containers_overview_columns()
    } else if is_snippets_tab {
        snippets_overview_columns()
    } else if is_keys_tab {
        keys_overview_columns()
    } else {
        let lines = match return_screen {
            Screen::FileBrowser { .. } => file_browser_lines(),
            Screen::SnippetPicker => snippet_picker_lines(),
            Screen::SnippetOutput => snippet_output_lines(),
            Screen::Containers { .. } => containers_lines(),
            Screen::TunnelList { .. } => tunnels_lines(),
            Screen::Providers => providers_lines(),
            Screen::KeyList => key_list_lines(),
            Screen::KeyDetail { .. } => key_detail_lines(),
            Screen::HostDetail { .. } => host_detail_lines(),
            Screen::TagPicker => tag_picker_lines(),
            Screen::BulkTagEditor => bulk_tag_editor_lines(),
            Screen::ThemePicker => vec![
                help_line("j/k ↑↓", crate::i18n::text("up / down")),
                help_line("Enter", crate::i18n::text("select theme")),
                help_line("?", crate::i18n::text("help")),
                help_line("Esc", crate::i18n::text("cancel")),
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

    let overlay_width = if is_main_view {
        88u16.min(frame.area().width.saturating_sub(4))
    } else {
        50u16.min(frame.area().width.saturating_sub(4))
    };

    // chrome = non-content rows the overlay consumes regardless of content.
    // Main view: 2 borders + top + logo(6) + gap(1) + 2 above info + 2 info + 1 bottom = 15.
    // Sub-screens: 2 borders + top + 1 bottom breathing = 4.
    // Footer renders BELOW the block via `design::form_footer`, so the
    // overlay reserves 1 row of vertical margin for it (saturating_sub(3)).
    let chrome = if is_main_view { 15 } else { 4 };
    let max_body = frame.area().height.saturating_sub(chrome);
    let height = (total_lines + chrome).min(frame.area().height.saturating_sub(3));
    let area = super::centered_rect_fixed(overlay_width, height, frame.area());

    frame.render_widget(Clear, area);

    let mut block = design::overlay_block(title_text);
    if is_main_view {
        let version = Line::from(vec![
            Span::styled(format!(" v{}", super::pkg_version()), theme::version()),
            Span::styled(format!(" (built {}) ", build_date()), theme::muted()),
        ]);
        block = block.title_bottom(version.right_aligned());
    }

    let inner = design::body_area(area);
    frame.render_widget(block, area);

    let rows = if is_main_view {
        Layout::vertical([
            Constraint::Length(1), // top breathing
            Constraint::Length(5), // logo
            Constraint::Length(1), // gap after logo
            Constraint::Min(0),    // content cols
            Constraint::Length(2), // breathing above info
            Constraint::Length(2), // wiki + issues info rows
            Constraint::Length(1), // bottom breathing
        ])
        .split(inner)
    } else {
        Layout::vertical([
            Constraint::Length(1), // top breathing
            Constraint::Min(0),    // content
            Constraint::Length(1), // bottom breathing
        ])
        .split(inner)
    };

    let max_scroll = total_lines.saturating_sub(max_body);
    if app.ui.help_scroll() > max_scroll {
        app.ui.set_help_scroll(max_scroll);
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

    // Row indices: main-view layout reserves extra rows for the logo.
    let content_row = if is_main_view { rows[3] } else { rows[1] };
    if is_main_view {
        let logo_lines: Vec<Line> = (0..design::LOGO.len())
            .map(|i| {
                design::logo_line(i, theme::accent_bold(), theme::logo_dot())
                    .alignment(ratatui::layout::Alignment::Center)
            })
            .collect();
        frame.render_widget(Paragraph::new(logo_lines), rows[1]);
    }
    use ratatui::widgets::Wrap;
    if use_two_cols {
        let cols = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(COL1_W),
            Constraint::Length(COL_GAP),
            Constraint::Length(COL2_W),
            Constraint::Fill(1),
        ])
        .split(content_row);
        // Two-column layout deliberately omits wrap so col1 and col2 rows
        // stay in vertical lock-step. Labels are pre-sized to fit each
        // column's budget (see help_line_short callers); CI check 23
        // enforces no overflow.
        let para1 = Paragraph::new(col1).scroll((app.ui.help_scroll(), 0));
        let para2 = Paragraph::new(col2).scroll((app.ui.help_scroll(), 0));
        frame.render_widget(para1, cols[1]);
        frame.render_widget(para2, cols[3]);
    } else if col2.is_empty() {
        // Single-column sub-screen help: enable wrap so a stray long
        // description folds to a continuation row instead of clipping.
        // Reserve the standard right margin via body-area math.
        let para = Paragraph::new(col1)
            .scroll((app.ui.help_scroll(), 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(para, content_row);
    } else {
        let mut all = col1;
        all.extend(col2);
        let para = Paragraph::new(all)
            .scroll((app.ui.help_scroll(), 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(para, content_row);
    }

    // Wiki + issues info block. The wiki line clearly tells the user this
    // overlay shows basic commands and the wiki has them all, while the
    // issues line stays the bug-report prompt. Both lines share the same
    // left margin as the content columns so everything is left-aligned on a
    // single imaginary vertical line. Main-view only (subscreen overlays
    // are too narrow for the full URLs).
    if is_main_view {
        let info_area = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(CONTENT_W),
            Constraint::Fill(1),
        ])
        .split(rows[5]);
        // Prefixes padded to the same column so both URLs line up exactly.
        // "All commands and docs:" = 22 chars + 2 spaces = 24.
        // "Got an idea or a bug?" = 21 chars + 3 spaces = 24.
        let info_lines = vec![
            Line::from(vec![
                Span::styled(
                    crate::i18n::text("All commands and docs:  "),
                    theme::muted(),
                ),
                Span::styled("github.com/erickochen/purple/wiki", theme::muted()),
            ]),
            Line::from(vec![
                Span::styled(
                    crate::i18n::text("Got an idea or a bug?   "),
                    theme::muted(),
                ),
                Span::styled("github.com/erickochen/purple/issues", theme::muted()),
            ]),
        ];
        frame.render_widget(Paragraph::new(info_lines), info_area[1]);
    }

    let can_scroll = total_lines > max_body;
    // Footer below the block
    let footer_area = design::render_overlay_footer(frame, area);
    use crate::messages::footer as fl;
    if can_scroll {
        let mut spans = design::Footer::new()
            .action(
                crate::i18n::text(fl::KEYS_SCROLL),
                crate::i18n::text(fl::LABEL_SCROLL),
            )
            .into_spans();
        let position = app.ui.help_scroll().saturating_add(1);
        let max = max_scroll.saturating_add(1);
        spans.push(Span::styled(
            format!(" [{}/{}]", position, max),
            theme::muted(),
        ));
        spans.push(Span::raw(design::FOOTER_GAP));
        spans.extend(
            design::Footer::new()
                .action("Esc", crate::i18n::text(fl::ESC_CLOSE))
                .into_spans(),
        );
        super::render_footer_with_status(frame, footer_area, spans, app);
    } else {
        design::Footer::new()
            .action("Esc", crate::i18n::text(fl::ESC_CLOSE))
            .render_with_status(frame, footer_area, app);
    }
}

fn context_title(screen: &Screen) -> &'static str {
    match screen {
        // The host list is purple's main screen, so its help pane title is
        // simply `Help` — no need to label "which" help, there is no other
        // help the user could mean from here.
        Screen::HostList | Screen::Welcome { .. } => crate::i18n::text("Help"),
        Screen::FileBrowser { .. } => crate::i18n::text("File Explorer"),
        Screen::SnippetPicker => crate::i18n::text("Snippets"),
        Screen::SnippetOutput => crate::i18n::text("Output"),
        Screen::Containers { .. } => crate::i18n::text("Containers"),
        Screen::TunnelList { .. } => crate::i18n::text("Tunnels"),
        Screen::Providers => crate::i18n::text("Providers"),
        Screen::KeyList => crate::i18n::text("SSH Keys"),
        Screen::KeyDetail { .. } => crate::i18n::text("Key Detail"),
        Screen::HostDetail { .. } => crate::i18n::text("All Directives"),
        Screen::TagPicker => crate::i18n::text("Tags"),
        Screen::BulkTagEditor => crate::i18n::text("Bulk tags"),
        Screen::ThemePicker => crate::i18n::text("Theme"),
        _ => crate::i18n::text("Help"),
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
    // (`Space`, `q/Esc`, `r`, `R`, `a`, `p/P`, single-letter shortcuts).
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
    // The jump bar (`:`) sits at the very bottom of col1, vertically
    // aligned with `q/Esc quit` in col2, so the last row of both columns is
    // a close/leave action. `r/R` mirrors `p/P` — single-target vs all.
    let col1 = vec![
        blank(),                                       // row 0
        section_header(crate::i18n::text("NAVIGATE")), // row 1
        blank(),                                       // row 2
        help_line("j/k ↑↓", crate::i18n::text("up / down")),
        help_line("PgDn/PgUp", crate::i18n::text("page down / up")),
        help_line("Enter", crate::i18n::text("connect")),
        help_line("/", crate::i18n::text("search (scoped)")),
        help_line("#", crate::i18n::text("tag picker")),
        help_line("Esc", crate::i18n::text("clear filter / selection")),
        blank(),                                   // row 9
        section_header(crate::i18n::text("VIEW")), // row 10 ↔ col2 CONNECT AND RUN
        blank(),
        help_line("v", crate::i18n::text("detail panel")),
        help_line("s", crate::i18n::text("cycle sort")),
        help_line("g", crate::i18n::text("group (off/provider/tag)")),
        blank(),                                        // row 15
        section_header(crate::i18n::text("CLIPBOARD")), // row 16 ↔ col2 TOOLS
        blank(),
        help_line("y", crate::i18n::text("copy ssh command")),
        help_line("n", crate::i18n::text("what's new")),
        blank(),
        blank(),
        blank(),
        help_line(":", crate::i18n::text("jump (search anything)")), // row 23 ↔ col2 q quit
    ];

    let col2 = vec![
        blank(),                                           // row 0
        section_header(crate::i18n::text("MANAGE HOSTS")), // row 1 ↔ col1 NAVIGATE
        blank(),
        help_line_short("a", crate::i18n::text("add host")),
        help_line_short("e", crate::i18n::text("edit")),
        help_line_short("d", crate::i18n::text("del")),
        help_line_short("u", crate::i18n::text("undo del")),
        help_line_short("t", crate::i18n::text("bulk tag")),
        blank(),
        blank(), // row 9  padding so headers align
        section_header(crate::i18n::text("CONNECT AND RUN")), // row 10 ↔ col1 VIEW
        blank(),
        help_line_short("Space", crate::i18n::text("multi-select")),
        help_line_short("r/R", crate::i18n::text("snippet / all")),
        help_line_short("p/P", crate::i18n::text("ping / all")),
        blank(),
        section_header(crate::i18n::text("TOOLS")), // row 16 ↔ col1 CLIPBOARD
        blank(),
        help_line_short("F", crate::i18n::text("file explorer")),
        help_line_short("T", crate::i18n::text("tunnels")),
        help_line_short("C", crate::i18n::text("containers")),
        help_line_short("K", crate::i18n::text("SSH keys")),
        help_line_short("S", crate::i18n::text("providers")),
        help_line_short("q", crate::i18n::text("quit")), // row 23 ↔ col1 `:`
    ];

    (col1, col2)
}

fn file_browser_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("Tab", crate::i18n::text("switch pane")));
    lines.push(help_line("j/k ↑↓", crate::i18n::text("up / down")));
    lines.push(help_line("Enter", crate::i18n::text("open dir / copy")));
    lines.push(help_line("Backspace", crate::i18n::text("go up")));
    lines.push(help_line("Space", crate::i18n::text("select / deselect")));
    lines.push(help_line("a", crate::i18n::text("select all / none")));
    lines.push(help_line(".", crate::i18n::text("toggle hidden")));
    lines.push(help_line("s", crate::i18n::text("cycle sort")));
    lines.push(help_line("R", crate::i18n::text("refresh")));
    lines.push(help_line("PgDn/PgUp", crate::i18n::text("page down / up")));
    lines.push(help_line("?", crate::i18n::text("help")));
    lines.push(help_line("q/Esc", crate::i18n::text("close")));
    lines
}

fn snippet_picker_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("Enter", crate::i18n::text("run (captured)")));
    lines.push(help_line("!", "run (raw terminal)"));
    lines.push(help_line("/", crate::i18n::text("search")));
    lines.push(help_line("a", crate::i18n::text("add snippet")));
    lines.push(help_line("e", crate::i18n::text("edit")));
    lines.push(help_line("d", crate::i18n::text("del")));
    lines.push(help_line("j/k ↑↓", crate::i18n::text("up / down")));
    lines.push(help_line("PgDn/PgUp", crate::i18n::text("page down / up")));
    lines.push(help_line("?", crate::i18n::text("help")));
    lines.push(help_line("q/Esc", crate::i18n::text("close")));
    lines
}

fn snippet_output_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("G/g", "end / start"));
    lines.push(help_line("n/N", "next / prev host"));
    lines.push(help_line("c", "copy output"));
    lines.push(help_line("j/k ↑↓", "scroll"));
    lines.push(help_line("PgDn/PgUp", crate::i18n::text("page down / up")));
    lines.push(help_line("?", crate::i18n::text("help")));
    lines.push(help_line("Ctrl+C", "cancel run"));
    lines.push(help_line("q/Esc", "close / cancel"));
    lines
}

fn containers_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("j/k ↑↓", crate::i18n::text("up / down")));
    lines.push(help_line("s", "start"));
    lines.push(help_line("x", crate::i18n::text("stop")));
    lines.push(help_line("r", crate::i18n::text("restart")));
    lines.push(help_line("R", crate::i18n::text("refresh")));
    lines.push(help_line("PgDn/PgUp", crate::i18n::text("page down / up")));
    lines.push(help_line("?", crate::i18n::text("help")));
    lines.push(help_line("q/Esc", crate::i18n::text("close")));
    lines
}

fn tunnels_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("j/k ↑↓", crate::i18n::text("up / down")));
    lines.push(help_line("a", crate::i18n::text("add tunnel")));
    lines.push(help_line("e", crate::i18n::text("edit")));
    lines.push(help_line("d", crate::i18n::text("del")));
    lines.push(help_line("Enter", crate::i18n::text("start / stop")));
    lines.push(help_line("PgDn/PgUp", crate::i18n::text("page down / up")));
    lines.push(help_line("?", crate::i18n::text("help")));
    lines.push(help_line("q/Esc", crate::i18n::text("close")));
    lines
}

/// Two-column help content for the Tunnels tab. Mirrors the rhythm of
/// `host_list_columns` so headers align across columns: NAVIGATE on
/// row 1 ↔ MANAGE TUNNELS on row 1, ACTIONS on row 10 ↔ NAVIGATE TABS
/// on row 10. The bottom-most row is a closing action (`:` / `q/Esc`)
/// so each column ends on a leave-action verb.
fn tunnels_overview_columns() -> (Vec<Line<'static>>, Vec<Line<'static>>) {
    let col1 = vec![
        blank(),                                       // row 0
        section_header(crate::i18n::text("NAVIGATE")), // row 1 ↔ col2 MANAGE TUNNELS
        blank(),                                       // row 2
        help_line("j/k ↑↓", crate::i18n::text("up / down")),
        help_line("PgDn/PgUp", crate::i18n::text("page down / up")),
        help_line("g/G", "top / bottom"),
        help_line("/", crate::i18n::text("search")),
        help_line("Esc", "clear search"),
        blank(),                   // row 8
        blank(),                   // row 9 padding so headers align
        section_header("ACTIONS"), // row 10 ↔ col2 NAVIGATE TABS
        blank(),
        help_line("Enter", crate::i18n::text("start / stop")),
        help_line("s", crate::i18n::text("cycle sort")),
        blank(),
        help_line(":", crate::i18n::text("jump (search anything)")), // row 15 ↔ col2 q quit
    ];

    let col2 = vec![
        blank(),                          // row 0
        section_header("MANAGE TUNNELS"), // row 1 ↔ col1 NAVIGATE
        blank(),
        help_line_short("a", crate::i18n::text("add tunnel")),
        help_line_short("e", crate::i18n::text("edit")),
        help_line_short("d", crate::i18n::text("del")),
        blank(),
        blank(),
        blank(),
        blank(),                         // row 9 padding
        section_header("NAVIGATE TABS"), // row 10 ↔ col1 ACTIONS
        blank(),
        help_line_short("Tab", "switch tabs"),
        help_line_short("n", crate::i18n::text("what's new")),
        blank(),
        help_line_short("q", crate::i18n::text("quit")), // row 15 ↔ col1 `:`
    ];

    (col1, col2)
}

fn containers_overview_columns() -> (Vec<Line<'static>>, Vec<Line<'static>>) {
    let col1 = vec![
        blank(),
        section_header(crate::i18n::text("NAVIGATE")),
        blank(),
        help_line("j/k ↑↓", crate::i18n::text("up / down")),
        help_line("PgDn/PgUp", crate::i18n::text("page down / up")),
        help_line("g/G", "top / bottom"),
        help_line("/", crate::i18n::text("search")),
        help_line("Esc", "clear search"),
        help_line("v", "toggle detail panel"),
        help_line("Space", "fold / unfold host group"),
        blank(),
        blank(),
        section_header("ACTIONS"),
        blank(),
        help_line("Enter", "shell into container"),
        help_line("l", "logs (last 200 lines)"),
        help_line("K", "kick (restart)"),
        help_line("Ctrl-K", "kick whole stack"),
        help_line("S", crate::i18n::text("stop")),
        help_line("e", "exec one-off command"),
        help_line("s", crate::i18n::text("cycle sort")),
        blank(),
        help_line(":", crate::i18n::text("jump (search anything)")),
    ];

    let col2 = vec![
        blank(),
        section_header("ON HOST DIVIDER"),
        blank(),
        help_line_short("K", "restart all"),
        help_line_short("S", "stop all"),
        help_line_short("r", "refresh listing"),
        help_line_short("Space", "fold group"),
        blank(),
        blank(),
        section_header("REFRESH"),
        blank(),
        help_line_short("r", "refresh host"),
        help_line_short("R", "refresh all"),
        help_line_short("a", crate::i18n::text("add host")),
        blank(),
        blank(),
        section_header("NAVIGATE TABS"),
        blank(),
        help_line_short("Tab", "switch tabs"),
        help_line_short("n", crate::i18n::text("what's new")),
        blank(),
        help_line_short("q", crate::i18n::text("quit")),
    ];

    (col1, col2)
}

/// Two-column help content for the Keys tab. Layout mirrors the rhythm
/// of the Tunnels and Containers help pages: NAVIGATE / ACTIONS on the
/// left, AT-A-GLANCE column legend and NAVIGATE TABS on the right.
fn keys_overview_columns() -> (Vec<Line<'static>>, Vec<Line<'static>>) {
    // Layout aligns headers with the col2 rhythm so the two columns read
    // as paired sections (NAVIGATE ↔ AT A GLANCE on row 1, ACTIONS ↔
    // VAULT STRIP on row 10, NAVIGATE TABS appears on both columns on
    // row 15).
    // Mirrors host_list rhythm: NAVIGATE + ACTIONS in col1 (24-char
    // descriptions fit `push (ssh-copy-id)` and `sign Vault SSH cert`);
    // STRENGTH legend + VAULT STRIP legend + TABS in col2 (15-char
    // descriptions via help_line_short).
    let col1 = vec![
        blank(),                                       // row 0
        section_header(crate::i18n::text("NAVIGATE")), // row 1
        blank(),
        help_line("j/k ↑↓", crate::i18n::text("up / down")),
        help_line("PgDn/PgUp", crate::i18n::text("page down / up")),
        help_line("g/G", "first / last"),
        help_line("/", crate::i18n::text("search (scoped)")),
        help_line(":", crate::i18n::text("jump (search anything)")),
        help_line("Esc", "clear filter"),
        blank(),
        section_header("ACTIONS"),
        blank(),
        help_line("Enter/c", "copy public key"),
        help_line("p", "push (ssh-copy-id)"),
        help_line("V", "sign Vault SSH cert"),
    ];

    let col2 = vec![
        blank(),                    // row 0
        section_header("STRENGTH"), // row 1
        blank(),
        help_line_short("strong", "ED25519, sk-*"),
        help_line_short("ok", "RSA 3072+"),
        help_line_short("weak", "RSA 2048"),
        help_line_short("poor", "DSA, RSA <2048"),
        blank(),
        section_header("VAULT STRIP"),
        blank(),
        help_line_short("green", "> 5 min left"),
        help_line_short("amber", "2 – 5 min left"),
        help_line_short("red", "< 2 min, soon"),
        blank(),
        section_header("TABS"),
        blank(),
        help_line_short("Tab", "switch tabs"),
        help_line_short("n", crate::i18n::text("what's new")),
        help_line_short("q", crate::i18n::text("quit")),
    ];

    (col1, col2)
}

fn snippets_overview_columns() -> (Vec<Line<'static>>, Vec<Line<'static>>) {
    let col1 = vec![
        blank(),
        section_header(crate::i18n::text("NAVIGATE")),
        blank(),
        help_line("j/k ↑↓", crate::i18n::text("up / down")),
        help_line("PgDn/PgUp", crate::i18n::text("page down / up")),
        help_line("g/G", "first / last"),
        help_line("/", crate::i18n::text("search (scoped)")),
        help_line("Esc", "clear filter"),
        blank(),
        section_header("ACTIONS"),
        blank(),
        help_line("Enter/r", "run on hosts"),
        help_line("!", "run in terminal"),
        help_line("a", crate::i18n::text("add snippet")),
        help_line("e", crate::i18n::text("edit snippet")),
        help_line("d", crate::i18n::text("delete snippet")),
        help_line("v", "toggle detail"),
    ];

    let col2 = vec![
        blank(),
        section_header("RUN FLOW"),
        blank(),
        help_line_short("1.", "pick a snippet"),
        help_line_short("2.", "choose hosts"),
        help_line_short("3.", "confirm + run"),
        blank(),
        section_header("TABS"),
        blank(),
        help_line_short("Tab", "switch tabs"),
        help_line_short("n", crate::i18n::text("what's new")),
        help_line_short("q", crate::i18n::text("quit")),
    ];

    (col1, col2)
}

fn key_list_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("j/k ↑↓", crate::i18n::text("up / down")));
    lines.push(help_line("Enter", "view detail"));
    lines.push(help_line("PgDn/PgUp", crate::i18n::text("page down / up")));
    lines.push(help_line("?", crate::i18n::text("help")));
    lines.push(help_line("q/Esc", crate::i18n::text("close")));
    lines
}

fn key_detail_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("?", crate::i18n::text("help")));
    lines.push(help_line("q/Esc", crate::i18n::text("close")));
    lines
}

fn host_detail_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("e", "edit host"));
    lines.push(help_line("r", crate::i18n::text("run snippet")));
    lines.push(help_line("T", crate::i18n::text("tunnels")));
    lines.push(help_line("?", crate::i18n::text("help")));
    lines.push(help_line("q/Esc/i", crate::i18n::text("close")));
    lines
}

fn tag_picker_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("j/k ↑↓", crate::i18n::text("up / down")));
    lines.push(help_line("Enter", "filter by tag"));
    lines.push(help_line("PgDn/PgUp", crate::i18n::text("page down / up")));
    lines.push(help_line("?", crate::i18n::text("help")));
    lines.push(help_line("q/Esc/#", crate::i18n::text("close")));
    lines
}

fn bulk_tag_editor_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("j/k ↑↓", crate::i18n::text("up / down")));
    lines.push(help_line("Space", "cycle [~] [x] [ ]"));
    lines.push(help_line("+", "new tag"));
    lines.push(help_line("Enter", crate::i18n::text("apply")));
    lines.push(help_line("?", crate::i18n::text("help")));
    lines.push(help_line("q/Esc", crate::i18n::text("cancel")));
    lines
}

fn providers_lines() -> Vec<Line<'static>> {
    let mut lines = vec![blank()];
    lines.push(help_line("j/k ↑↓", crate::i18n::text("up / down")));
    lines.push(help_line("Enter", "configure"));
    lines.push(help_line("s", crate::i18n::text("sync")));
    lines.push(help_line("d", "del config"));
    lines.push(help_line("X", "purge stale"));
    lines.push(help_line("PgDn/PgUp", crate::i18n::text("page down / up")));
    lines.push(help_line("?", crate::i18n::text("help")));
    lines.push(help_line("q/Esc", crate::i18n::text("close")));
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
            text.contains(crate::i18n::text("run (captured)")),
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
        let line = help_line("j/k", crate::i18n::text("up / down"));
        let key_text = line.spans[0].to_string();
        assert!(key_text.starts_with(' '), "key should have leading spaces");
        assert!(
            key_text.trim_start().starts_with("j/k"),
            "key content should be j/k"
        );
    }

    #[test]
    fn help_line_description_is_dim() {
        let line = help_line("j/k", crate::i18n::text("up / down"));
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
            crate::i18n::text("File Explorer")
        );
        assert_eq!(context_title(&Screen::SnippetPicker), "Snippets");
        assert_eq!(context_title(&Screen::SnippetOutput), "Output");
        assert_eq!(
            context_title(&Screen::Containers {
                alias: "test".into()
            }),
            crate::i18n::text("Containers")
        );
        assert_eq!(
            context_title(&Screen::TunnelList {
                alias: "test".into()
            }),
            crate::i18n::text("Tunnels")
        );
    }

    #[test]
    fn host_list_layout_breathing_content_info_bottom() {
        // Host list: top(1) + content + breathing(2) + 2 info rows + bottom(1).
        // Footer renders BELOW the block via design::form_footer (no row in
        // the inner layout).
        let area = Rect::new(0, 0, 80, 40);
        let rows = ratatui::layout::Layout::vertical([
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Min(0),
            ratatui::layout::Constraint::Length(2),
            ratatui::layout::Constraint::Length(2),
            ratatui::layout::Constraint::Length(1),
        ])
        .split(area);
        assert_eq!(rows[0].height, 1, "top breathing");
        assert_eq!(rows[3].height, 2, "info rows");
        assert_eq!(rows[4].height, 1, "bottom breathing");
    }

    #[test]
    fn compact_layout_breathing_content_bottom() {
        // Sub-screens: top(1) + content + bottom breathing(1).
        // Footer renders BELOW the block via design::form_footer.
        let area = Rect::new(0, 0, 80, 30);
        let rows = ratatui::layout::Layout::vertical([
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Min(0),
            ratatui::layout::Constraint::Length(1),
        ])
        .split(area);
        assert_eq!(rows[0].height, 1, "top breathing");
        assert_eq!(rows[2].height, 1, "bottom breathing");
    }

    #[test]
    fn footer_sits_directly_below_block() {
        let area = Rect::new(0, 0, 80, 30);
        let footer = design::form_footer(area, area.height);
        assert_eq!(footer.height, 1);
        assert_eq!(footer.y, area.y + area.height);
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
            crate::i18n::text("file explorer"),
            crate::i18n::text("tunnels"),
            crate::i18n::text("containers"),
            crate::i18n::text("SSH keys"),
            crate::i18n::text("providers"),
            crate::i18n::text("copy ssh command"),
        ] {
            assert!(all_text.contains(desc), "help columns missing '{}'", desc);
        }
    }

    #[test]
    fn host_list_col1_contains_navigate_and_view() {
        let (col1, _) = host_list_columns();
        let text: String = col1.iter().map(|l| l.to_string()).collect();
        for desc in &[
            crate::i18n::text("up / down"),
            crate::i18n::text("page down / up"),
            crate::i18n::text("connect"),
            crate::i18n::text("search"),
            crate::i18n::text("tag picker"),
            crate::i18n::text("detail panel"),
            crate::i18n::text("cycle sort"),
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
            (crate::i18n::text("containers"), containers_lines()),
            (crate::i18n::text("tunnels"), tunnels_lines()),
            ("key_list", key_list_lines()),
            ("key_detail", key_detail_lines()),
            ("host_detail", host_detail_lines()),
            ("tag_picker", tag_picker_lines()),
            (crate::i18n::text("providers"), providers_lines()),
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
            crate::i18n::text("NAVIGATE"),
            crate::i18n::text("VIEW"),
            crate::i18n::text("CLIPBOARD"),
            crate::i18n::text("MANAGE HOSTS"),
            crate::i18n::text("CONNECT AND RUN"),
            crate::i18n::text("TOOLS"),
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
            app.ui.help_scroll(),
            0,
            "host-list help should fit without scrolling at 100x50"
        );
        for header in [
            crate::i18n::text("NAVIGATE"),
            crate::i18n::text("VIEW"),
            crate::i18n::text("MANAGE HOSTS"),
            crate::i18n::text("TOOLS"),
        ] {
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
        app.ui.set_help_scroll(999);
        render_to_text(&mut app, 100, 50);
        assert_eq!(
            app.ui.help_scroll(),
            0,
            "stale scroll must clamp to 0 when content fits"
        );
    }

    #[test]
    fn tunnels_overview_columns_cover_tab_specific_shortcuts() {
        // Tunnels tab supports more than the per-host TunnelList — at
        // minimum search, sort, jump bar and tab switching.
        let (col1, col2) = tunnels_overview_columns();
        let text: String = col1
            .iter()
            .chain(col2.iter())
            .map(|l| l.to_string())
            .collect();
        for desc in &[
            crate::i18n::text("start / stop"),
            crate::i18n::text("add tunnel"),
            crate::i18n::text("edit"),
            crate::i18n::text("search"),
            crate::i18n::text("cycle sort"),
            crate::i18n::text("jump (search anything)"),
            "switch tabs",
        ] {
            assert!(
                text.contains(desc),
                "tunnels-overview help missing '{desc}'"
            );
        }
    }

    #[test]
    fn tunnels_overview_columns_share_section_header_rhythm_with_host_list() {
        // Headers must land on the same row indices in both columns so
        // the eye reads them as paired (NAVIGATE ↔ MANAGE TUNNELS,
        // ACTIONS ↔ NAVIGATE TABS).
        let (col1, col2) = tunnels_overview_columns();
        let h1_a = col1[1].to_string();
        let h1_b = col2[1].to_string();
        assert!(h1_a.contains("NAVIGATE"), "col1 row 1 must be NAVIGATE");
        assert!(
            h1_b.contains("MANAGE TUNNELS"),
            "col2 row 1 must be MANAGE TUNNELS"
        );
        let h2_a = col1[10].to_string();
        let h2_b = col2[10].to_string();
        assert!(h2_a.contains("ACTIONS"), "col1 row 10 must be ACTIONS");
        assert!(
            h2_b.contains("NAVIGATE TABS"),
            "col2 row 10 must be NAVIGATE TABS"
        );
    }

    #[test]
    fn help_on_tunnels_tab_renders_logo_and_wiki_block_with_tab_columns() {
        // Pressing ? from the Tunnels tab keeps `app.screen = Help` with
        // `return_screen = HostList`; the dispatcher uses `app.top_page`
        // to render tunnel-specific columns. Chrome (logo + wiki) stays
        // identical to the host-list overlay so the two tabs feel like
        // siblings.
        let mut app = help_test_app(Screen::HostList);
        app.top_page = TopPage::Tunnels;
        let text = render_to_text(&mut app, 100, 40);
        assert!(
            text.contains("MANAGE TUNNELS"),
            "tunnels-tab help must show tunnel section header"
        );
        assert!(
            text.contains(crate::i18n::text("start / stop")),
            "tunnels-tab help should show Enter shortcut"
        );
        assert!(
            !text.contains(crate::i18n::text("MANAGE HOSTS")),
            "tunnels-tab help must not show host-list section headers"
        );
        assert!(
            text.contains("github.com/erickochen/purple/wiki"),
            "tunnels-tab help must render wiki info block"
        );
        assert!(
            text.contains("github.com/erickochen/purple/issues"),
            "tunnels-tab help must render issues info block"
        );
    }

    #[test]
    fn containers_overview_columns_cover_tab_specific_shortcuts() {
        let (col1, col2) = containers_overview_columns();
        let text: String = col1
            .iter()
            .chain(col2.iter())
            .map(|l| l.to_string())
            .collect();
        for desc in &[
            "shell into container",
            crate::i18n::text("search"),
            crate::i18n::text("cycle sort"),
            crate::i18n::text("jump (search anything)"),
            "switch tabs",
            "refresh host",
            "refresh all",
            crate::i18n::text("add host"),
        ] {
            assert!(
                text.contains(desc),
                "containers-overview help missing '{desc}'"
            );
        }
    }

    #[test]
    fn snippets_overview_columns_cover_tab_specific_shortcuts() {
        let (col1, col2) = snippets_overview_columns();
        let text: String = col1
            .iter()
            .chain(col2.iter())
            .map(|l| l.to_string())
            .collect();
        for desc in &[
            "run on hosts",
            "run in terminal",
            crate::i18n::text("add snippet"),
            crate::i18n::text("edit snippet"),
            crate::i18n::text("delete snippet"),
            "toggle detail",
            "switch tabs",
        ] {
            assert!(
                text.contains(desc),
                "snippets-overview help missing '{desc}'"
            );
        }
    }

    #[test]
    fn help_on_snippets_tab_renders_tab_columns() {
        let mut app = help_test_app(Screen::HostList);
        app.top_page = TopPage::Snippets;
        let text = render_to_text(&mut app, 100, 40);
        assert!(
            text.contains("RUN FLOW"),
            "snippets-tab help must show RUN FLOW section header"
        );
        assert!(
            text.contains("run on hosts"),
            "snippets-tab help must show Enter/r shortcut"
        );
        assert!(
            text.contains(crate::i18n::text("add snippet")),
            "snippets-tab help must show 'a' shortcut"
        );
        assert!(
            !text.contains("MANAGE TUNNELS"),
            "snippets-tab help must not show tunnels section headers"
        );
    }

    #[test]
    fn help_on_containers_tab_renders_tab_columns() {
        let mut app = help_test_app(Screen::HostList);
        app.top_page = TopPage::Containers;
        let text = render_to_text(&mut app, 100, 40);
        assert!(
            text.contains("REFRESH"),
            "containers-tab help must show REFRESH section header"
        );
        assert!(
            text.contains("refresh host"),
            "containers-tab help must show 'r' shortcut"
        );
        assert!(
            text.contains("refresh all"),
            "containers-tab help must show 'R' shortcut"
        );
        assert!(
            text.contains(crate::i18n::text("add host")),
            "containers-tab help must show 'a' shortcut"
        );
        assert!(
            text.contains("shell into container"),
            "containers-tab help should show Enter shortcut"
        );
        assert!(
            !text.contains("MANAGE TUNNELS"),
            "containers-tab help must not show tunnels section headers"
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
