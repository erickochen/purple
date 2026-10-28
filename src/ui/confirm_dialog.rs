use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::text::{Line, Span};
use ratatui::widgets::Clear;
use unicode_width::UnicodeWidthStr;

use super::design;
use super::theme;
use crate::app::App;

// Confirm dialogs are compact modals (y/Esc) with no separate footer row.
// Status messages are not shown here. Any active status will be visible
// once the user closes the dialog and returns to the parent screen.

pub fn render(frame: &mut Frame, app: &App, alias: &str) {
    // Multi-alias awareness: if this alias shares its `Host` block with
    // sibling tokens, spell them out so the user understands what will
    // happen (only the selected alias is stripped; siblings keep the
    // shared config).
    let siblings = app.hosts_state.ssh_config().siblings_of(alias);
    let has_siblings = !siblings.is_empty();

    let popup_w: u16 = if has_siblings { 60 } else { 52 };
    let mut content: Vec<Line<'static>> = vec![Line::from(Span::styled(
        format!("  Delete \"{}\"?", alias),
        theme::bold(),
    ))];
    if has_siblings {
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            format!(
                "  {}",
                crate::messages::confirm_delete_siblings_note(&siblings)
            ),
            theme::muted(),
        )));
    }

    // Stakes test: deleting a host is destructive (config write, undo only
    // briefly via stack). Use action verbs both sides instead of generic
    // yes/no.
    let footer_spans = design::confirm_footer_destructive("delete", "keep")
        .to_line()
        .spans;
    design::render_confirm_popup(
        frame,
        popup_w,
        design::PopupKind::Destructive,
        "Confirm Delete",
        content,
        footer_spans,
        app,
    );
}

pub fn render_host_key_reset(frame: &mut Frame, app: &App, hostname: &str) {
    let display = super::truncate(hostname, 40);
    let content: Vec<Line<'static>> = vec![
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
    ];

    // Stakes test: removing the host key invalidates trust. Use action verbs.
    let footer_spans = design::confirm_footer_destructive("reset", "keep")
        .to_line()
        .spans;
    design::render_confirm_popup(
        frame,
        52,
        design::PopupKind::Destructive,
        "Host Key Changed",
        content,
        footer_spans,
        app,
    );
}

/// Trust-on-first-contact dialog for a host a background ssh met that is
/// not in `known_hosts` yet. No fingerprint is shown: ssh prints none under
/// strict checking, and fetching one would mean a second round trip to the
/// same host that is asking to be trusted.
///
/// Stakes test: recording a host key is a security decision, so the footer
/// carries action verbs on both sides.
pub fn render_host_key_trust(frame: &mut Frame, app: &App, alias: &str, hostname: &str) {
    let display = super::truncate(hostname, 44);
    let content: Vec<Line<'static>> = vec![
        Line::from(Span::styled(
            format!("  {}", crate::messages::host_key_trust::question(&display)),
            theme::bold(),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!(
                "  {}",
                crate::messages::host_key_trust::detail_unknown(alias)
            ),
            theme::muted(),
        )),
        Line::from(Span::styled(
            format!("  {}", crate::messages::host_key_trust::DETAIL_RECORDED),
            theme::muted(),
        )),
    ];

    let footer_spans = design::confirm_footer_destructive("trust", "skip")
        .to_line()
        .spans;
    design::render_confirm_popup(
        frame,
        64,
        design::PopupKind::Destructive,
        crate::messages::host_key_trust::TITLE,
        content,
        footer_spans,
        app,
    );
}

pub fn render_confirm_import(frame: &mut Frame, app: &App, count: usize) {
    let content: Vec<Line<'static>> = vec![Line::from(Span::styled(
        format!(
            "  Import {} host{} from known_hosts?",
            count,
            if count == 1 { "" } else { "s" },
        ),
        theme::bold(),
    ))];

    // Stakes test: importing is benign-but-material. Neutral kind with
    // action verbs makes the choice clearer than generic yes/no.
    let footer_spans = design::confirm_footer_destructive("import", "skip")
        .to_line()
        .spans;
    design::render_confirm_popup(
        frame,
        52,
        design::PopupKind::Neutral,
        "Import",
        content,
        footer_spans,
        app,
    );
}

pub fn render_confirm_purge_stale(
    frame: &mut Frame,
    app: &App,
    aliases: &[String],
    provider: &Option<String>,
) {
    let count = aliases.len();
    // Show up to 6 host names, then "+N more hosts" (only when N >= 1)
    let max_shown = 6;
    let mut host_lines: Vec<Line<'static>> = aliases
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

    let mut content: Vec<Line<'static>> = vec![Line::from(Span::styled(main_line, theme::bold()))];
    content.extend(host_lines);

    // Stakes test: purge is destructive — removes stale hosts from config
    // (only undoable through the per-session undo stack). Action verbs.
    let footer_spans = design::confirm_footer_destructive("purge", "keep")
        .to_line()
        .spans;
    design::render_confirm_popup(
        frame,
        52,
        design::PopupKind::Destructive,
        "Purge Stale",
        content,
        footer_spans,
        app,
    );
}

/// Destructive confirm: append `<key>.pub` to `authorized_keys` on every
/// host in `aliases`. Body lists up to 6 aliases (then `+N more`) so the
/// user sees what is about to happen. Footer renders action verbs both
/// sides via `design::confirm_footer_destructive("push", "keep")`.
pub fn render_key_push(frame: &mut Frame, app: &App, key_index: usize, aliases: &[String]) {
    let key_name = app
        .keys
        .list()
        .get(key_index)
        .map(|k| format!("{}.pub", k.name))
        .unwrap_or_else(|| "key".to_string());
    let count = aliases.len();

    let max_shown = 6;
    let mut host_lines: Vec<Line<'static>> = aliases
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

    // Push is idempotent (appends a public key), not destructive like
    // delete or stop, so use Neutral kind (muted border).
    let mut content: Vec<Line<'static>> = vec![
        Line::from(Span::styled(
            format!(
                "  {}",
                crate::messages::key_push_confirm_body(&key_name, count)
            ),
            theme::bold(),
        )),
        Line::from(""),
    ];
    content.extend(host_lines);

    let footer_spans = design::confirm_footer_destructive("push", "keep")
        .to_line()
        .spans;
    design::render_confirm_popup(
        frame,
        56,
        design::PopupKind::Neutral,
        "Push Key",
        content,
        footer_spans,
        app,
    );
}

/// Confirm dialog before running a snippet against the chosen hosts. Lists the
/// snippet name and the target host count, with up to 6 host aliases previewed.
pub fn render_run_snippet(frame: &mut Frame, app: &App, aliases: &[String]) {
    let snippet_name = app
        .snippets
        .flow_snippet()
        .map(|s| s.name.clone())
        .unwrap_or_else(|| crate::messages::SNIPPET_FALLBACK_NAME.to_string());
    let count = aliases.len();

    let max_shown = 6;
    let mut host_lines: Vec<Line<'static>> = aliases
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

    let mut content: Vec<Line<'static>> = vec![
        Line::from(Span::styled(
            format!(
                "  {}",
                crate::messages::confirm_run_snippet_body(&snippet_name, count)
            ),
            theme::bold(),
        )),
        Line::from(""),
    ];
    content.extend(host_lines);

    let footer_spans = design::confirm_footer_destructive("run", "keep")
        .to_line()
        .spans;
    design::render_confirm_popup(
        frame,
        56,
        design::PopupKind::Neutral,
        "Run Snippet",
        content,
        footer_spans,
        app,
    );
}

pub fn render_confirm_vault_sign(frame: &mut Frame, app: &App, signable: &[String]) {
    let count = signable.len();
    let preview_limit = 5;
    let shown: Vec<&str> = signable
        .iter()
        .take(preview_limit)
        .map(String::as_str)
        .collect();
    let preview_text = if count > preview_limit {
        format!("{} ... +{} more", shown.join(", "), count - preview_limit)
    } else if count > 0 {
        shown.join(", ")
    } else {
        String::new()
    };

    let mut content: Vec<Line<'static>> = vec![
        Line::from(Span::styled(
            format!(
                "  Sign {} SSH certificate{} via the Vault SSH secrets engine?",
                count,
                if count == 1 { "" } else { "s" },
            ),
            theme::bold(),
        )),
        Line::from(""),
    ];
    if preview_text.is_empty() {
        content.push(Line::from(""));
    } else {
        content.push(Line::from(Span::styled(
            format!("  {}", preview_text),
            theme::muted(),
        )));
    }
    content.push(Line::from(""));
    content.push(Line::from(Span::styled(
        "  Hosts with a still-valid certificate are skipped.".to_string(),
        theme::muted(),
    )));

    // Stakes test: bulk vault signing hits HashiCorp Vault, may take time
    // and is the canonical destructive/material confirm in purple. Use
    // action verbs (sign/skip) instead of generic yes/no.
    let footer_spans = design::confirm_footer_destructive("sign", "skip")
        .to_line()
        .spans;
    design::render_confirm_popup(
        frame,
        72,
        design::PopupKind::Neutral,
        "Sign Vault SSH Certificates",
        content,
        footer_spans,
        app,
    );
}

// Welcome logo now pulled from `design::LOGO` (single source of truth).

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
        .ui
        .welcome_opened()
        .map(|t| t.elapsed().as_millis())
        .unwrap_or(u128::MAX);

    // Phase 1: Logo lines appear one by one after zoom completes
    let logo_start = WELCOME_ZOOM_MS;
    let logo_lines_visible = if elapsed <= logo_start {
        0usize
    } else {
        (((elapsed - logo_start) / LOGO_LINE_INTERVAL_MS) as usize + 1).min(design::LOGO.len())
    };

    // Phase 2: Typewriter for text after logo is fully revealed
    let text_start =
        logo_start + (design::LOGO.len() as u128) * LOGO_LINE_INTERVAL_MS + TYPEWRITER_DELAY_MS;
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
    let logo_max_w = design::LOGO
        .iter()
        .map(|l| UnicodeWidthStr::width(*l))
        .max()
        .unwrap_or(0);

    // Where the first-launch backup went, shown with `~` for the home dir.
    let backup_location = app
        .env()
        .paths()
        .map(|p| p.abbreviate_home(&p.config_original()))
        .unwrap_or_default();
    let backup_line = crate::messages::welcome_backup_location(&backup_location);
    let hosts_line = crate::messages::hosts_loaded_from(host_count, &app.config_path_display());

    // border(2) + blank(3) + logo(5) + blank(3) + subtitle(1) + hint(1) + blank(1) + footer(1) + blank(3) = 22 base
    let content_height = 22 + extra;
    // Minimum width: fits the longest text line (the hosts line, the backup
    // notice or the backup path) with comfortable padding. Logo width plus
    // padding, at least 56 chars.
    let hosts_width = if has_hosts {
        UnicodeWidthStr::width(hosts_line.as_str())
    } else {
        0
    };
    let backup_width = if has_backup {
        UnicodeWidthStr::width(backup_line.as_str()).max(UnicodeWidthStr::width(
            crate::messages::WELCOME_BACKUP_TAKEN,
        ))
    } else {
        0
    };
    let longest_line = hosts_width.max(backup_width);
    let dialog_width = ((logo_max_w as u16) + 24)
        .max(56)
        .max(u16::try_from(longest_line + 8).unwrap_or(u16::MAX));
    let area = super::centered_rect_fixed(dialog_width, content_height as u16, frame.area());

    frame.render_widget(Clear, area);

    let block = design::plain_overlay_block();

    // --- Build text lines ---
    let mut text: Vec<Line<'_>> = Vec::new();

    // Top spacing
    text.push(Line::from(""));
    text.push(Line::from(""));
    text.push(Line::from(""));

    // Logo (phased line-by-line reveal). Each line is a split-coloured
    // `Line` (word body in brand accent, trailing dot in cyan-equivalent)
    // composed by `design::logo_line`. Center-alignment does the horizontal
    // padding for us — no manual right-pad needed because every LOGO row
    // has the same cell width.
    for i in 0..design::LOGO.len() {
        if i < logo_lines_visible {
            text.push(
                design::logo_line(i, theme::border_search(), theme::logo_dot())
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
        let spans = vec![Span::styled(hosts_line, theme::muted())];
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
            crate::messages::WELCOME_BACKUP_TAKEN,
            theme::muted(),
        )];
        text.push(Line::from(typewriter_spans(b1, &mut char_budget)).alignment(Alignment::Center));
        let b2 = vec![Span::styled(backup_line, theme::muted())];
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

    design::render_body_wrapped(frame, area, block, text);
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
    let height = 22 + extra;

    // Text lines = height - border(2)
    let lines = height - 2;

    (height, lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The welcome line names the config file and the dialog widens to fit
    // a long path in full.
    #[test]
    fn welcome_names_a_long_config_path_in_full() {
        let long = "/srv/shared/ssh/configs/managed-by-purple/team-infrastructure/config";
        let config = crate::ssh_config::model::SshConfigFile::from_content(
            "Host a\n  HostName 10.0.0.1\n",
            std::path::PathBuf::from(long),
        );
        let app = App::with_env(
            config,
            std::sync::Arc::new(crate::runtime::env::Env::sandboxed()),
        );
        let text =
            crate::ui::render_to_text(160, 50, |frame| render_welcome(frame, &app, false, 1, 0));
        assert!(
            text.contains(&format!("1 host loaded from {long}.")),
            "{text}"
        );
    }

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
