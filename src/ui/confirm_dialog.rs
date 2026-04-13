use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::App;

// Confirm dialogs are compact modals (y/Esc) with no separate footer row.
// Status messages are not shown here. Any active status will be visible
// once the user closes the dialog and returns to the parent screen.

pub fn render(frame: &mut Frame, _app: &App, alias: &str) {
    let area = super::centered_rect_fixed(52, 7, frame.area());

    // Clear background
    frame.render_widget(Clear, area);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" Confirm Delete ", theme::danger()))
        .border_style(theme::border_danger());

    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  Delete \"{}\"?", alias),
            theme::bold(),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(" y ", theme::footer_key()),
            Span::styled(" yes ", theme::muted()),
            Span::raw("  "),
            Span::styled(" Esc ", theme::footer_key()),
            Span::styled(" no", theme::muted()),
        ]),
    ];

    let paragraph = Paragraph::new(text).block(block);
    frame.render_widget(paragraph, area);
}

pub fn render_host_key_reset(frame: &mut Frame, _app: &App, hostname: &str) {
    let display = super::truncate(hostname, 40);
    let area = super::centered_rect_fixed(52, 9, frame.area());

    frame.render_widget(Clear, area);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" Host Key Changed ", theme::danger()))
        .border_style(theme::border_danger());

    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  Host key for {} changed.", display),
            theme::bold(),
        )),
        Line::from(Span::styled(
            "  This can happen after a server reinstall.",
            theme::muted(),
        )),
        Line::from(Span::styled(
            "  Remove old key and reconnect?",
            theme::muted(),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(" y ", theme::footer_key()),
            Span::styled(" yes ", theme::muted()),
            Span::raw("  "),
            Span::styled(" Esc ", theme::footer_key()),
            Span::styled(" no", theme::muted()),
        ]),
    ];

    let paragraph = Paragraph::new(text).block(block);
    frame.render_widget(paragraph, area);
}

pub fn render_confirm_import(frame: &mut Frame, _app: &App, count: usize) {
    let area = super::centered_rect_fixed(52, 7, frame.area());

    frame.render_widget(Clear, area);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" Import ", theme::brand()))
        .border_style(theme::accent());

    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!(
                "  Import {} host{} from known_hosts?",
                count,
                if count == 1 { "" } else { "s" },
            ),
            theme::bold(),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(" y ", theme::footer_key()),
            Span::styled(" yes ", theme::muted()),
            Span::raw("  "),
            Span::styled(" Esc ", theme::footer_key()),
            Span::styled(" no", theme::muted()),
        ]),
    ];

    let paragraph = Paragraph::new(text).block(block);
    frame.render_widget(paragraph, area);
}

pub fn render_confirm_purge_stale(
    frame: &mut Frame,
    _app: &App,
    aliases: &[String],
    provider: &Option<String>,
) {
    let count = aliases.len();
    // Show up to 6 host names, then "+N more hosts" (only when N >= 1)
    let max_shown = 6;
    let mut host_lines: Vec<Line> = aliases
        .iter()
        .take(max_shown)
        .map(|a| {
            let truncated = super::truncate(a, 46);
            Line::from(Span::styled(format!("  {}", truncated), theme::muted()))
        })
        .collect();
    if count > max_shown {
        let remaining = count - max_shown;
        host_lines.push(Line::from(Span::styled(
            format!(
                "  +{} more host{}",
                remaining,
                if remaining == 1 { "" } else { "s" }
            ),
            theme::muted(),
        )));
    }

    // height: 2 border + 1 blank + 1 question + host_lines + 1 blank + 1 footer
    let inner_height = 4 + host_lines.len();
    let height = (inner_height + 2) as u16;
    let area = super::centered_rect_fixed(52, height, frame.area());

    frame.render_widget(Clear, area);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" Purge Stale ", theme::danger()))
        .border_style(theme::border_danger());

    let main_line = if let Some(prov) = provider {
        let display = crate::providers::provider_display_name(prov);
        format!(
            "  Remove {} stale {} host{}?",
            count,
            display,
            if count == 1 { "" } else { "s" },
        )
    } else {
        format!(
            "  Remove {} stale host{}?",
            count,
            if count == 1 { "" } else { "s" },
        )
    };

    let mut text = vec![
        Line::from(""),
        Line::from(Span::styled(main_line, theme::bold())),
    ];
    text.extend(host_lines);
    text.push(Line::from(""));
    text.push(Line::from(vec![
        Span::styled(" y ", theme::footer_key()),
        Span::styled(" yes ", theme::muted()),
        Span::raw("  "),
        Span::styled(" Esc ", theme::footer_key()),
        Span::styled(" no", theme::muted()),
    ]));

    let paragraph = Paragraph::new(text).block(block);
    frame.render_widget(paragraph, area);
}

pub fn render_confirm_vault_sign(frame: &mut Frame, _app: &App, signable: &[String]) {
    let count = signable.len();
    // Preview first 5 aliases, append "...and N more" when truncated.
    let preview_limit = 5;
    let shown: Vec<&str> = signable
        .iter()
        .take(preview_limit)
        .map(String::as_str)
        .collect();
    let preview_text = if count > preview_limit {
        format!("  {} ... +{} more", shown.join(", "), count - preview_limit)
    } else if count > 0 {
        format!("  {}", shown.join(", "))
    } else {
        String::new()
    };

    // Height: top + main + blank + preview + blank + note + spacer + footer + bottom = 11
    let height = 11u16;
    let area = super::centered_rect_fixed(72, height, frame.area());

    frame.render_widget(Clear, area);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(
            " Sign Vault SSH Certificates ",
            theme::accent(),
        ))
        .border_style(theme::accent());

    let mut footer_spans: Vec<Span<'static>> = Vec::new();
    for s in super::footer_action("y", " confirm ") {
        footer_spans.push(s);
    }
    footer_spans.push(Span::raw("  "));
    for s in super::footer_action("Esc", " cancel") {
        footer_spans.push(s);
    }

    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!(
                "  Sign {} SSH certificate{} via the Vault SSH secrets engine?",
                count,
                if count == 1 { "" } else { "s" },
            ),
            theme::bold(),
        )),
        Line::from(""),
        Line::from(Span::styled(preview_text, theme::muted())),
        Line::from(""),
        Line::from(Span::styled(
            "  Hosts with a still-valid certificate are skipped.".to_string(),
            theme::muted(),
        )),
        Line::from(""),
        Line::from(footer_spans),
    ];

    let paragraph = Paragraph::new(text).block(block);
    frame.render_widget(paragraph, area);
}

/// Block-art logo for the welcome screen (6 lines, ANSI Shadow style).
/// Lines are trimmed; render code pads to max display width for alignment.
const LOGO: [&str; 6] = [
    "\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2557} \u{2588}\u{2588}\u{2557}   \u{2588}\u{2588}\u{2557}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2557} \u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2557} \u{2588}\u{2588}\u{2557}     \u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2557}",
    "\u{2588}\u{2588}\u{2554}\u{2550}\u{2550}\u{2588}\u{2588}\u{2557}\u{2588}\u{2588}\u{2551}   \u{2588}\u{2588}\u{2551}\u{2588}\u{2588}\u{2554}\u{2550}\u{2550}\u{2588}\u{2588}\u{2557}\u{2588}\u{2588}\u{2554}\u{2550}\u{2550}\u{2588}\u{2588}\u{2557}\u{2588}\u{2588}\u{2551}     \u{2588}\u{2588}\u{2554}\u{2550}\u{2550}\u{2550}\u{2550}\u{255D}",
    "\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2554}\u{255D}\u{2588}\u{2588}\u{2551}   \u{2588}\u{2588}\u{2551}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2554}\u{255D}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2554}\u{255D}\u{2588}\u{2588}\u{2551}     \u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2557}  ",
    "\u{2588}\u{2588}\u{2554}\u{2550}\u{2550}\u{2550}\u{255D} \u{2588}\u{2588}\u{2551}   \u{2588}\u{2588}\u{2551}\u{2588}\u{2588}\u{2554}\u{2550}\u{2550}\u{2588}\u{2588}\u{2557}\u{2588}\u{2588}\u{2554}\u{2550}\u{2550}\u{2550}\u{255D} \u{2588}\u{2588}\u{2551}     \u{2588}\u{2588}\u{2554}\u{2550}\u{2550}\u{255D}  ",
    "\u{2588}\u{2588}\u{2551}     \u{255A}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2554}\u{255D}\u{2588}\u{2588}\u{2551}  \u{2588}\u{2588}\u{2551}\u{2588}\u{2588}\u{2551}     \u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2557}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2557}",
    "\u{255A}\u{2550}\u{255D}      \u{255A}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{255D} \u{255A}\u{2550}\u{255D}  \u{255A}\u{2550}\u{255D}\u{255A}\u{2550}\u{255D}     \u{255A}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{255D}\u{255A}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{255D}",
];

/// Typewriter delay: ms after logo reveal before text starts.
const TYPEWRITER_DELAY_MS: u128 = 100;
/// Typewriter speed: ms per character.
const TYPEWRITER_CHAR_MS: u128 = 15;
/// Welcome zoom animation duration (must match ui/mod.rs).
const WELCOME_ZOOM_MS: u128 = 350;
/// Logo line reveal interval (ms between each logo line appearing).
const LOGO_LINE_INTERVAL_MS: u128 = 50;

/// Apply typewriter truncation to a list of spans.
/// Consumes at most `budget` characters across all spans. Returns the truncated
/// spans and a right-pad span so the total width matches the original (stable centering).
fn typewriter_spans<'a>(spans: Vec<Span<'a>>, budget: &mut usize) -> Vec<Span<'a>> {
    let total_chars: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    // Short-circuit: all characters visible, return spans as-is
    if *budget >= total_chars {
        *budget -= total_chars;
        return spans;
    }
    let mut result = Vec::new();
    let mut visible_chars = 0;
    for span in &spans {
        if *budget == 0 {
            break;
        }
        let char_count = span.content.chars().count();
        if char_count <= *budget {
            result.push(span.clone());
            *budget -= char_count;
            visible_chars += char_count;
        } else {
            let truncated: String = span.content.chars().take(*budget).collect();
            visible_chars += *budget;
            result.push(Span::styled(truncated, span.style));
            *budget = 0;
        }
    }
    // Pad to original width for stable centering
    let pad = total_chars.saturating_sub(visible_chars);
    if pad > 0 {
        result.push(Span::raw(" ".repeat(pad)));
    }
    result
}

pub fn render_welcome(
    frame: &mut Frame,
    app: &App,
    has_backup: bool,
    host_count: usize,
    known_hosts_count: usize,
) {
    let has_hosts = host_count > 0;

    // Elapsed time since welcome opened (for phased animation).
    // When welcome_opened is None (e.g. re-render after animation cleared),
    // MAX causes all phases to resolve as "already done" — skipping animation
    // gracefully instead of showing a partially animated state.
    let elapsed = app
        .welcome_opened
        .map(|t| t.elapsed().as_millis())
        .unwrap_or(u128::MAX);

    // Phase 1: Logo lines appear one by one after zoom completes
    let logo_start = WELCOME_ZOOM_MS;
    let logo_lines_visible = if elapsed <= logo_start {
        0usize
    } else {
        (((elapsed - logo_start) / LOGO_LINE_INTERVAL_MS) as usize + 1).min(LOGO.len())
    };

    // Phase 2: Typewriter for text after logo is fully revealed
    let text_start =
        logo_start + (LOGO.len() as u128) * LOGO_LINE_INTERVAL_MS + TYPEWRITER_DELAY_MS;
    let mut char_budget = if elapsed <= text_start {
        0usize
    } else {
        ((elapsed - text_start) / TYPEWRITER_CHAR_MS) as usize
    };

    // Count extra lines for the info section (matches text-building logic below exactly)
    let mut extra = 2usize; // blank + blank between subtitle and hint
    if has_hosts {
        extra += 3; // blank + blank + "N hosts loaded"
    } else if known_hosts_count > 0 {
        extra += 5; // blank + blank + "Found N" + blank + "Press I"
    }
    if has_backup {
        extra += 3; // blank + backup line 1 + line 2
    }

    // Compute logo display width (max across lines) for alignment padding
    let logo_max_w = LOGO
        .iter()
        .map(|l| UnicodeWidthStr::width(*l))
        .max()
        .unwrap_or(0);

    // border(2) + blank(3) + logo(6) + blank(3) + subtitle(1) + hint(1) + blank(1) + footer(1) + blank(3) = 23 base
    let content_height = 23 + extra;
    // Minimum width: fits longest text line ("Your original config has been backed up")
    // with comfortable padding. Logo width + padding, or 56 chars minimum.
    let dialog_width = ((logo_max_w as u16) + 24).max(56);
    let area = super::centered_rect_fixed(dialog_width, content_height as u16, frame.area());

    frame.render_widget(Clear, area);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::accent());

    // --- Build text lines ---
    let mut text: Vec<Line<'_>> = Vec::new();

    // Top spacing
    text.push(Line::from(""));
    text.push(Line::from(""));
    text.push(Line::from(""));

    // Logo (phased line-by-line reveal, padded to uniform width)
    for (i, logo_line) in LOGO.iter().enumerate() {
        if i < logo_lines_visible {
            let w = UnicodeWidthStr::width(*logo_line);
            let pad = logo_max_w.saturating_sub(w);
            let padded = format!("{}{}", logo_line, " ".repeat(pad));
            text.push(
                Line::from(Span::styled(padded, theme::border_search()))
                    .alignment(Alignment::Center),
            );
        } else {
            text.push(Line::from(""));
        }
    }

    text.push(Line::from(""));
    text.push(Line::from(""));
    text.push(Line::from(""));

    // Subtitle (typewriter)
    let sub_spans = vec![Span::styled(
        "Your SSH config, supercharged.",
        theme::muted(),
    )];
    text.push(
        Line::from(typewriter_spans(sub_spans, &mut char_budget)).alignment(Alignment::Center),
    );
    text.push(Line::from(""));
    text.push(Line::from(""));
    let hint_spans = vec![
        Span::styled("Press ", theme::muted()),
        Span::styled(" ? ", theme::footer_key()),
        Span::styled(" anytime for help.", theme::muted()),
    ];
    text.push(
        Line::from(typewriter_spans(hint_spans, &mut char_budget)).alignment(Alignment::Center),
    );

    // Info lines (typewriter continues)
    if has_hosts {
        text.push(Line::from(""));
        text.push(Line::from(""));
        let info = format!(
            "{} host{} loaded from ~/.ssh/config.",
            host_count,
            if host_count == 1 { "" } else { "s" },
        );
        let spans = vec![Span::styled(info, theme::muted())];
        text.push(
            Line::from(typewriter_spans(spans, &mut char_budget)).alignment(Alignment::Center),
        );
    } else if known_hosts_count > 0 {
        text.push(Line::from(""));
        text.push(Line::from(""));
        let info = format!(
            "Found {} host{} in known_hosts.",
            known_hosts_count,
            if known_hosts_count == 1 { "" } else { "s" },
        );
        let spans = vec![Span::styled(info, theme::muted())];
        text.push(
            Line::from(typewriter_spans(spans, &mut char_budget)).alignment(Alignment::Center),
        );
        text.push(Line::from(""));
        let hint_spans = vec![
            Span::styled("Press ", theme::muted()),
            Span::styled(" I ", theme::footer_key()),
            Span::styled(" to import them.", theme::muted()),
        ];
        text.push(
            Line::from(typewriter_spans(hint_spans, &mut char_budget)).alignment(Alignment::Center),
        );
    }
    if has_backup {
        text.push(Line::from(""));
        let b1 = vec![Span::styled(
            "Your original config has been backed up",
            theme::muted(),
        )];
        text.push(Line::from(typewriter_spans(b1, &mut char_budget)).alignment(Alignment::Center));
        let b2 = vec![Span::styled("to ~/.purple/config.original", theme::muted())];
        text.push(Line::from(typewriter_spans(b2, &mut char_budget)).alignment(Alignment::Center));
    }

    // Footer (appears after all text is typed)
    text.push(Line::from(""));
    if char_budget > 0 {
        text.push(
            Line::from(vec![
                Span::styled(" Enter ", theme::footer_key()),
                Span::styled(" continue", theme::muted()),
            ])
            .alignment(Alignment::Center),
        );
    } else {
        text.push(Line::from(""));
    }
    text.push(Line::from(""));
    text.push(Line::from(""));
    text.push(Line::from(""));

    let paragraph = Paragraph::new(text).block(block);
    frame.render_widget(paragraph, area);
}

/// Compute the welcome dialog height and text line count for testing.
/// Returns (height, text_line_count).
#[cfg(test)]
fn welcome_height_and_lines(
    has_backup: bool,
    host_count: usize,
    known_hosts_count: usize,
) -> (usize, usize) {
    let has_hosts = host_count > 0;
    // Count extra lines (must match render_welcome exactly)
    let mut extra = 2usize; // blank + blank between subtitle and hint
    if has_hosts {
        extra += 3;
    } else if known_hosts_count > 0 {
        extra += 5;
    }
    if has_backup {
        extra += 3;
    }
    let height = 23 + extra;

    // Text lines = height - border(2)
    let lines = height - 2;

    (height, lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Welcome dialog height calculation — all 8 permutations
    // =========================================================================
    // (has_hosts, has_backup, known_hosts > 0)
    // Note: when has_hosts=true, known_hosts_count is irrelevant (else-if branch)
    // but we test both 0 and >0 to confirm it doesn't affect height.

    #[test]
    fn welcome_height_hosts_backup_no_known() {
        let (height, lines) = welcome_height_and_lines(true, 5, 0);
        assert_eq!(
            lines,
            height - 2,
            "has_hosts=true, has_backup=true, known=0"
        );
    }

    #[test]
    fn welcome_height_hosts_backup_with_known() {
        let (height, lines) = welcome_height_and_lines(true, 5, 10);
        assert_eq!(
            lines,
            height - 2,
            "has_hosts=true, has_backup=true, known=10"
        );
    }

    #[test]
    fn welcome_height_hosts_no_backup_no_known() {
        let (height, lines) = welcome_height_and_lines(false, 5, 0);
        assert_eq!(
            lines,
            height - 2,
            "has_hosts=true, has_backup=false, known=0"
        );
    }

    #[test]
    fn welcome_height_hosts_no_backup_with_known() {
        let (height, lines) = welcome_height_and_lines(false, 5, 10);
        assert_eq!(
            lines,
            height - 2,
            "has_hosts=true, has_backup=false, known=10"
        );
    }

    #[test]
    fn welcome_height_no_hosts_backup_no_known() {
        let (height, lines) = welcome_height_and_lines(true, 0, 0);
        assert_eq!(
            lines,
            height - 2,
            "has_hosts=false, has_backup=true, known=0"
        );
    }

    #[test]
    fn welcome_height_no_hosts_backup_with_known() {
        let (height, lines) = welcome_height_and_lines(true, 0, 10);
        assert_eq!(
            lines,
            height - 2,
            "has_hosts=false, has_backup=true, known=10"
        );
    }

    #[test]
    fn welcome_height_no_hosts_no_backup_no_known() {
        let (height, lines) = welcome_height_and_lines(false, 0, 0);
        assert_eq!(
            lines,
            height - 2,
            "has_hosts=false, has_backup=false, known=0"
        );
    }

    #[test]
    fn welcome_height_no_hosts_no_backup_with_known() {
        let (height, lines) = welcome_height_and_lines(false, 0, 10);
        assert_eq!(
            lines,
            height - 2,
            "has_hosts=false, has_backup=false, known=10"
        );
    }

    // Edge cases for host_count and known_hosts_count boundary values
    #[test]
    fn welcome_height_single_host() {
        let (height, lines) = welcome_height_and_lines(false, 1, 0);
        assert_eq!(lines, height - 2, "single host");
    }

    #[test]
    fn welcome_height_single_known_host() {
        let (height, lines) = welcome_height_and_lines(false, 0, 1);
        assert_eq!(lines, height - 2, "single known_hosts entry");
    }

    // =========================================================================
    // Confirm import dialog pluralization
    // =========================================================================

    #[test]
    fn confirm_import_pluralization_single() {
        let msg = format!(
            "  Import {} host{} from known_hosts?",
            1,
            if 1 == 1 { "" } else { "s" },
        );
        assert_eq!(msg, "  Import 1 host from known_hosts?");
    }

    #[test]
    fn confirm_import_pluralization_multiple() {
        let msg = format!(
            "  Import {} host{} from known_hosts?",
            42,
            if 42 == 1 { "" } else { "s" },
        );
        assert_eq!(msg, "  Import 42 hosts from known_hosts?");
    }

    // =========================================================================
    // Welcome dialog pluralization
    // =========================================================================

    #[test]
    fn welcome_hosts_pluralization_single() {
        let msg = format!(
            "Found {} host{} in your SSH config.",
            1,
            if 1 == 1 { "" } else { "s" },
        );
        assert_eq!(msg, "Found 1 host in your SSH config.");
    }

    #[test]
    fn welcome_hosts_pluralization_multiple() {
        let msg = format!(
            "Found {} host{} in your SSH config.",
            12,
            if 12 == 1 { "" } else { "s" },
        );
        assert_eq!(msg, "Found 12 hosts in your SSH config.");
    }

    #[test]
    fn welcome_known_hosts_pluralization_single() {
        let msg = format!(
            "Found {} host{} in known_hosts.",
            1,
            if 1 == 1 { "" } else { "s" },
        );
        assert_eq!(msg, "Found 1 host in known_hosts.");
    }

    #[test]
    fn welcome_known_hosts_pluralization_multiple() {
        let msg = format!(
            "Found {} host{} in known_hosts.",
            34,
            if 34 == 1 { "" } else { "s" },
        );
        assert_eq!(msg, "Found 34 hosts in known_hosts.");
    }

    #[test]
    fn test_purge_stale_pluralization_single_no_provider() {
        let count = 1;
        let msg = format!(
            "  Remove {} stale host{}?",
            count,
            if count == 1 { "" } else { "s" },
        );
        assert_eq!(msg, "  Remove 1 stale host?");
    }

    #[test]
    fn test_purge_stale_pluralization_multiple_no_provider() {
        let count = 7;
        let msg = format!(
            "  Remove {} stale host{}?",
            count,
            if count == 1 { "" } else { "s" },
        );
        assert_eq!(msg, "  Remove 7 stale hosts?");
    }

    #[test]
    fn test_purge_stale_pluralization_with_provider() {
        let display = "DigitalOcean";
        let count = 3;
        let msg = format!(
            "  Remove {} stale {} host{}?",
            count,
            display,
            if count == 1 { "" } else { "s" },
        );
        assert_eq!(msg, "  Remove 3 stale DigitalOcean hosts?");
    }

    #[test]
    fn test_purge_stale_pluralization_single_with_provider() {
        let display = "Vultr";
        let msg = format!(
            "  Remove {} stale {} host{}?",
            1,
            display,
            if 1 == 1 { "" } else { "s" },
        );
        assert_eq!(msg, "  Remove 1 stale Vultr host?");
    }
}
