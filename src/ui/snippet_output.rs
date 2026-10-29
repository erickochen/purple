use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::design;
use super::theme;
use crate::app::{App, Screen};

pub fn render(frame: &mut Frame, app: &mut App) {
    if !matches!(app.screen, Screen::SnippetOutput) {
        return;
    }
    let snippet_name = app.snippets.output_snippet_name().unwrap_or("").to_string();
    let host_count = app.snippets.flow_targets().len();

    let state = match app.snippets.output() {
        Some(s) => s,
        None => return,
    };

    // Reserve 1 row below the block for the external footer.
    let area = design::overlay_area(frame, 90, 85, frame.area().height.saturating_sub(1));
    frame.render_widget(Clear, area);

    // Title with progress
    let host_word = if host_count == 1 { "host" } else { "hosts" };
    let title = if state.all_done {
        format!("Ran '{}' on {} {}", snippet_name, host_count, host_word)
    } else {
        format!(
            "Running '{}' ({}/{} {})",
            snippet_name, state.completed, state.total, host_word
        )
    };

    let block = design::overlay_block(&title);

    let inner = design::body_area(area);
    frame.render_widget(block, area);

    let content = inner;
    let width = content.width as usize;

    // Build all lines from results
    let mut lines: Vec<Line<'_>> = Vec::new();

    if state.results.is_empty() {
        let msg = if state.all_done {
            "No results."
        } else {
            "Running..."
        };
        lines.push(design::empty_line(msg));
    }

    for result in &state.results {
        // Host header with exit code
        let status_text = match result.exit_code {
            Some(0) => format!(" {}", design::ICON_SUCCESS),
            Some(code) => format!(" exit {}", code),
            None => " error".to_string(),
        };
        let status_style = match result.exit_code {
            Some(0) => theme::success(),
            _ => theme::error(),
        };

        let prefix = format!("  \u{2500}\u{2500} {} ", result.alias);
        let used = prefix.width() + status_text.width() + 1;
        let fill = width.saturating_sub(used);

        lines.push(Line::from(vec![
            Span::styled(prefix, theme::bold()),
            Span::styled(status_text, status_style),
            Span::styled(format!(" {}", "\u{2500}".repeat(fill)), theme::border()),
        ]));

        if result.stdout.is_empty() && result.stderr.is_empty() {
            lines.push(Line::from(Span::styled(
                crate::i18n::text("  [No output]"),
                theme::muted(),
            )));
        } else {
            for line in result.stdout.lines() {
                lines.push(Line::from(Span::raw(format!("  {}", line))));
            }
            for line in result.stderr.lines() {
                lines.push(Line::from(Span::styled(
                    format!("  {}", line),
                    theme::error(),
                )));
            }
        }
        if result.shows_not_found_hint(state.interactive) {
            lines.push(Line::from(Span::styled(
                format!(
                    "  {}",
                    crate::i18n::text(crate::messages::SNIPPET_NOT_FOUND_HINT)
                ),
                theme::muted(),
            )));
        }
        lines.push(Line::from(""));
    }

    // Offset-based rendering: slice to visible window (no u16 limit)
    let visible_height = content.height as usize;
    let total = lines.len();
    let max_offset = total.saturating_sub(visible_height);
    let offset = state.scroll_offset.min(max_offset);
    let visible: Vec<Line<'_>> = lines
        .into_iter()
        .skip(offset)
        .take(visible_height)
        .collect();

    frame.render_widget(Paragraph::new(visible), content);

    // Footer below the block. Reads left-to-right as "what you can do
    // next" with the exit key last, matching the canonical purple
    // ordering: primary action → secondary → exit.
    let footer_area = design::render_overlay_footer(frame, area);
    use crate::messages::footer as fl;
    let mut f = design::Footer::new();
    if state.all_done {
        f = f
            .action("c", crate::i18n::text(fl::SNIPPET_OUTPUT_COPY))
            .action(
                crate::i18n::text(fl::KEYS_SCROLL),
                crate::i18n::text(fl::LABEL_SCROLL),
            )
            .action(
                crate::i18n::text(fl::KEYS_NEXT_PREV_HOST),
                crate::i18n::text(fl::LABEL_NEXT_PREV_HOST),
            )
            .action(
                crate::i18n::text(fl::KEYS_TOP_BOTTOM),
                crate::i18n::text(fl::LABEL_TOP_BOTTOM),
            )
            .action("Esc", crate::i18n::text(fl::ESC_CLOSE));
    } else {
        f = f
            .action(
                crate::i18n::text(fl::KEYS_SCROLL),
                crate::i18n::text(fl::LABEL_SCROLL),
            )
            .action(
                crate::i18n::text(fl::KEYS_NEXT_PREV_HOST),
                crate::i18n::text(fl::LABEL_NEXT_PREV_HOST),
            )
            .action(
                crate::i18n::text(fl::KEYS_TOP_BOTTOM),
                crate::i18n::text(fl::LABEL_TOP_BOTTOM),
            )
            .action("Ctrl+C", crate::i18n::text(fl::CTRL_C_CANCEL));
    }
    f.render_with_status(frame, footer_area, app);
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;

    use super::design;
    use crate::app::{App, Screen, SnippetHostOutput, SnippetOutputState};
    use crate::ssh_config::model::SshConfigFile;

    const W: u16 = 120;
    const H: u16 = 30;

    /// Render the output overlay for one host that exited with `exit_code`.
    fn render_text(exit_code: i32, interactive: bool) -> String {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::ui::theme::init_with_mode(1);
        crate::ui::theme::set_theme(crate::ui::theme::ThemeDef::purple());
        let scratch = tempfile::tempdir().expect("tempdir");
        let config = SshConfigFile {
            elements: SshConfigFile::parse_content(""),
            path: scratch.path().join("config"),
            crlf: false,
            bom: false,
        };
        let mut app = App::new(config);
        app.snippets.set_output(Some(SnippetOutputState {
            run_id: 1,
            results: vec![SnippetHostOutput {
                alias: "vps".to_string(),
                stdout: String::new(),
                stderr: "bash: line 1: pm2: command not found".to_string(),
                exit_code: Some(exit_code),
            }],
            scroll_offset: 0,
            completed: 1,
            total: 1,
            all_done: true,
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            interactive,
        }));
        app.snippets
            .set_output_snippet_name(Some("pm2-status".to_string()));
        app.snippets.set_flow_targets(vec!["vps".to_string()]);
        app.screen = Screen::SnippetOutput;
        let mut terminal = Terminal::new(TestBackend::new(W, H)).expect("terminal");
        terminal.draw(|f| super::render(f, &mut app)).expect("draw");
        let buf = terminal.backend().buffer().clone();
        (0..H)
            .map(|y| {
                (0..W)
                    .map(|x| buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(""))
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn command_not_found_shows_the_interactive_shell_tip() {
        let text = render_text(crate::snippet::EXIT_COMMAND_NOT_FOUND, false);
        assert!(text.contains(crate::i18n::text(crate::messages::SNIPPET_NOT_FOUND_HINT)));
    }

    #[test]
    fn tip_stays_away_from_other_exit_codes_and_interactive_snippets() {
        assert!(!render_text(1, false).contains("Tip:"));
        assert!(!render_text(crate::snippet::EXIT_COMMAND_NOT_FOUND, true).contains("Tip:"));
    }

    #[test]
    fn footer_sits_directly_below_block() {
        let area = Rect::new(0, 0, 80, 30);
        let footer = design::form_footer(area, area.height);
        assert_eq!(footer.height, 1);
        assert_eq!(footer.y, area.y + area.height);
        assert_eq!(footer.x, area.x);
        assert_eq!(footer.width, area.width);
    }
}
