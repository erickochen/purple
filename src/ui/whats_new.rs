use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use super::{design, theme};
use crate::app::{App, Screen};
use crate::changelog::{self, Entry, EntryKind, Section};
use crate::messages::whats_new as msg;

/// Width of the bullet prefix rendered by `entry_line`:
/// `LIST_HIGHLIGHT` (2) + `KIND_*` label (8) + `COL_GAP_STR` (2) = 12 cols.
/// Wrapped continuation lines indent to this column so text aligns under
/// the first word of the bullet instead of snapping back to the left edge.
const ENTRY_INDENT: usize = 12;

pub fn render(frame: &mut Frame, app: &App) {
    let state = match &app.screen {
        Screen::WhatsNew(s) => s,
        _ => return,
    };

    let area = design::overlay_area(
        frame,
        design::OVERLAY_W,
        design::OVERLAY_H,
        frame.area().height.saturating_sub(1),
    );
    frame.render_widget(Clear, area);
    let block = design::overlay_block(msg::TITLE);
    let inner = design::body_area(area);

    let sections = changelog::current_for_render();
    let current = semver::Version::parse(super::pkg_version()).ok();
    let last = crate::preferences::load_last_seen_version(app.env().paths())
        .ok()
        .flatten()
        .and_then(|s| semver::Version::parse(&s).ok());

    // Always show the most recent N releases, even when the user is up to
    // date. Toast-trigger logic (onboarding.rs) still uses versions_to_show
    // for the "has something new?" signal, but the overlay itself is a
    // browsable history.
    const RECENT_CAP: usize = 10;
    let shown: &[Section] = sections
        .get(..sections.len().min(RECENT_CAP))
        .unwrap_or(&[]);

    let current_str = current.as_ref().map(|v| v.to_string()).unwrap_or_default();
    let last_str = last.as_ref().map(|v| v.to_string());

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));
    for i in 0..design::LOGO.len() {
        lines.push(
            design::logo_line(i, theme::accent_bold(), theme::logo_dot())
                .alignment(Alignment::Center),
        );
    }
    lines.push(Line::from(""));
    lines.push(
        Line::from(Span::styled(
            msg::subtitle(last_str.as_deref(), &current_str),
            theme::muted(),
        ))
        .alignment(Alignment::Center),
    );
    if let Some(new_version) = app.update.available() {
        lines.push(
            Line::from(Span::styled(
                msg::update_available(new_version),
                theme::accent_bold(),
            ))
            .alignment(Alignment::Center),
        );
    }
    lines.push(Line::from(""));
    lines.push(Line::from(""));

    if shown.is_empty() {
        lines.push(Line::from(Span::raw(msg::EMPTY)).alignment(Alignment::Center));
    } else {
        for (i, section) in shown.iter().enumerate() {
            if i > 0 {
                lines.push(Line::from(""));
                lines.push(design::section_divider());
                lines.push(Line::from(""));
            }
            lines.push(section_header_line(section));
            lines.push(Line::from(""));
            for entry in &section.entries {
                for line in entry_lines(entry, inner.width as usize) {
                    lines.push(line);
                }
            }
        }
    }
    // Bottom breathing room above the overlay border.
    lines.push(Line::from(""));
    lines.push(Line::from(""));
    lines.push(Line::from(""));

    let total_lines = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    let viewport = inner.height.max(1);
    let max_scroll = total_lines.saturating_sub(viewport);
    let effective = state.scroll.min(max_scroll);

    frame.render_widget(block, area);
    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((effective, 0));
    frame.render_widget(para, inner);

    let footer_area = design::render_overlay_footer(frame, area);
    use crate::messages::footer as fl;
    design::Footer::new()
        .action(msg::FOOTER_CLOSE_KEYS, crate::i18n::text(fl::ESC_CLOSE))
        .action(
            crate::i18n::text(fl::KEYS_SCROLL),
            crate::i18n::text(fl::LABEL_SCROLL),
        )
        .action(
            crate::i18n::text(fl::KEYS_TOP_BOTTOM),
            crate::i18n::text(fl::LABEL_TOP_BOTTOM),
        )
        .render_with_status(frame, footer_area, app);
}

fn section_header_line(section: &Section) -> Line<'static> {
    let mut spans = vec![
        Span::raw(design::LIST_HIGHLIGHT),
        Span::styled(section.version.to_string(), theme::section_header()),
    ];
    if let Some(date) = &section.date {
        spans.push(Span::raw(design::COL_GAP_STR));
        spans.push(Span::styled(date.clone(), theme::muted()));
    }
    Line::from(spans)
}

fn entry_lines(entry: &Entry, available_width: usize) -> Vec<Line<'static>> {
    let (label, style) = match entry.kind {
        EntryKind::Feature => (msg::KIND_FEAT, theme::accent_bold()),
        EntryKind::Change => (msg::KIND_CHANGE, Style::default()),
        EntryKind::Fix => (msg::KIND_FIX, Style::default()),
    };
    let text = strip_inline_markdown(&entry.text);
    let text_width = available_width.saturating_sub(ENTRY_INDENT).max(1);
    let wrapped = wrap_text(&text, text_width);

    let indent: String = " ".repeat(ENTRY_INDENT);
    let mut out: Vec<Line<'static>> = Vec::with_capacity(wrapped.len().max(1));
    let mut iter = wrapped.into_iter();
    let first = iter.next().unwrap_or_default();
    out.push(Line::from(vec![
        Span::raw(design::LIST_HIGHLIGHT),
        Span::styled(label, style),
        Span::raw(design::COL_GAP_STR),
        Span::raw(first),
    ]));
    for cont in iter {
        out.push(Line::from(vec![Span::raw(indent.clone()), Span::raw(cont)]));
    }
    out
}

/// Greedy word-wrap: splits on whitespace and fits as many words per line as
/// possible within `width` display columns. Uses unicode display widths so
/// wide characters (CJK, emoji) do not overflow. Single words longer than
/// `width` are emitted on their own line to preserve readability rather than
/// being broken mid-word.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() || width == 0 {
        return vec![text.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    for word in text.split_whitespace() {
        let word_width = UnicodeWidthStr::width(word);
        if current.is_empty() {
            current.push_str(word);
            current_width = word_width;
            continue;
        }
        // +1 accounts for the single joining space.
        if current_width + 1 + word_width <= width {
            current.push(' ');
            current.push_str(word);
            current_width += 1 + word_width;
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
            current_width = word_width;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Strip the small subset of inline markdown that appears in CHANGELOG bullets
/// so overlay entries read as prose instead of raw syntax. The CLI `whats-new`
/// subcommand prints the raw text on purpose so it can be piped to a renderer.
///
/// Handles `[text](url)` links (rendered as `text`), `**bold**` and `__bold__`
/// markers. Bare `[...]` without a paired `(...)` is left untouched so
/// technical content like `array[0]` or `[ERROR]` is preserved.
fn strip_inline_markdown(input: &str) -> String {
    let mut out = input.replace("**", "").replace("__", "");
    let mut pos = 0;
    while let Some(rel) = out[pos..].find('[') {
        let start = pos + rel;
        let Some(mid) = out[start..].find("](") else {
            pos = start + 1;
            continue;
        };
        let Some(end) = out[start + mid..].find(')') else {
            pos = start + 1;
            continue;
        };
        let text = out[start + 1..start + mid].to_string();
        out.replace_range(start..start + mid + end + 1, &text);
        pos = start + text.len();
    }
    out
}

#[cfg(test)]
mod strip_inline_markdown_tests {
    use super::strip_inline_markdown;

    #[test]
    fn strips_link_keeps_text() {
        assert_eq!(
            strip_inline_markdown("Closes [#32](https://github.com/x/y/issues/32)"),
            "Closes #32"
        );
    }

    #[test]
    fn strips_multiple_links() {
        assert_eq!(
            strip_inline_markdown("See [a](http://a) and [b](http://b)"),
            "See a and b"
        );
    }

    #[test]
    fn strips_bold_markers() {
        assert_eq!(
            strip_inline_markdown("This is **important** and __critical__"),
            "This is important and critical"
        );
    }

    #[test]
    fn leaves_bare_brackets_alone() {
        assert_eq!(strip_inline_markdown("array[0] = 1"), "array[0] = 1");
    }

    #[test]
    fn handles_unclosed_link() {
        assert_eq!(
            strip_inline_markdown("Broken [link](no-close"),
            "Broken [link](no-close"
        );
    }

    #[test]
    fn handles_plain_text() {
        assert_eq!(
            strip_inline_markdown("no markdown here"),
            "no markdown here"
        );
    }
}

#[cfg(test)]
mod wrap_tests {
    use super::wrap_text;

    #[test]
    fn returns_single_line_when_fits() {
        assert_eq!(wrap_text("short", 20), vec!["short".to_string()]);
    }

    #[test]
    fn wraps_on_word_boundary() {
        let out = wrap_text("one two three four five", 10);
        assert_eq!(out, vec!["one two", "three four", "five"]);
    }

    #[test]
    fn long_word_gets_own_line() {
        // A single word longer than width is emitted whole. The overlay's
        // outer Paragraph wrap then soft-wraps it as a last resort.
        let out = wrap_text("tiny supercalifragilistic end", 10);
        assert_eq!(out, vec!["tiny", "supercalifragilistic", "end"]);
    }

    #[test]
    fn handles_empty_input() {
        assert_eq!(wrap_text("", 10), vec!["".to_string()]);
    }

    #[test]
    fn handles_zero_width() {
        assert_eq!(wrap_text("anything", 0), vec!["anything".to_string()]);
    }
}
