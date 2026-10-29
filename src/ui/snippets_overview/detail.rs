//! Detail panel for the Snippets tab. Stacked section cards: OVERVIEW, then a
//! TRACK RECORD card (when the snippet has a run history) carrying the
//! reliability verdict in its title border and an inset reliability-trend
//! Chart, then COMMAND, PARAMETERS and IMPACT. The Chart is a native ratatui
//! widget painted into an inset sub-Rect after the text Paragraph, so it has
//! real margins and never bleeds to the card border.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Chart, Dataset, GraphType, Paragraph};

use crate::snippet::{Category, Finding, Severity, Snippet};
use crate::snippet_runs::{RunRecord, RunStatus, RunTally, SnippetRunLog};
use crate::ui::{design, theme};

/// Reserved graph rows for the reliability trend chart, and the minimum inner
/// width below which the chart is suppressed (panel mid-animation or a tiny
/// terminal) rather than bled past the card border.
const CHART_ROWS: u16 = 3;
const MIN_CHART_WIDTH: u16 = 16;
/// Columns the chart `Rect` insets on each side, so the widget never reaches
/// the card border. A row reserves the chart only when the panel can spare
/// `MIN_CHART_WIDTH + 2 * CHART_INSET` columns.
const CHART_INSET: u16 = 3;
/// Top of the reliability y-axis. Drives both the axis bound and its label so
/// the 0..100 scale is defined once.
const CHART_Y_MAX: f64 = 100.0;

/// Render the detail panel for `snippet`. `None` renders an empty bordered
/// frame (nothing selected, e.g. an empty search result). `targets` are the
/// snippet's saved default hosts (shown in OVERVIEW).
pub fn render(
    frame: &mut Frame,
    area: Rect,
    snippet: Option<&Snippet>,
    targets: &[String],
    runs: &SnippetRunLog,
) {
    let Some(snippet) = snippet else {
        design::render_tab_empty_detail(frame, area);
        return;
    };
    let box_width = area.width as usize;
    let content_w = box_width.saturating_sub(4);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let chart_row = build_static_cards(&mut lines, snippet, targets, runs, box_width, content_w);
    design::stretch_last_card(&mut lines, area.height as usize, box_width);
    frame.render_widget(Paragraph::new(lines), area);

    // Second pass: paint the reliability-trend Chart over the rows the
    // TRACK RECORD card reserved for it. Rendered after the Paragraph so the
    // card frame and side borders are already drawn; the inset Rect keeps the
    // widget clear of them.
    if let Some(row) = chart_row {
        render_trend_chart(frame, area, row, runs.for_snippet(&snippet.name));
    }
}

fn build_static_cards(
    lines: &mut Vec<Line<'static>>,
    snippet: &Snippet,
    targets: &[String],
    runs: &SnippetRunLog,
    box_width: usize,
    content_w: usize,
) -> Option<usize> {
    let impact = crate::snippet::analyze_command(&snippet.command);
    let params = crate::snippet::parse_params(&snippet.command);

    // OVERVIEW
    design::section_open(lines, "OVERVIEW", box_width);
    let desc = if snippet.description.is_empty() {
        "(no description)".to_string()
    } else {
        snippet.description.clone()
    };
    for row in wrap_lines(&desc, content_w) {
        design::section_line(lines, vec![Span::styled(row, theme::muted())], box_width);
    }
    lines.push(design::section_empty_line(box_width));
    design::section_field(
        lines,
        crate::i18n::text("Parameters"),
        &params.len().to_string(),
        0,
        box_width,
    );
    if !targets.is_empty() {
        let value = crate::messages::snippet::snippet_default_hosts_summary(targets);
        design::section_field(
            lines,
            crate::i18n::text("Default hosts"),
            &value,
            content_w,
            box_width,
        );
    }
    if snippet.interactive {
        design::section_field(
            lines,
            crate::i18n::text(crate::messages::snippet::SNIPPET_INTERACTIVE_LABEL),
            crate::i18n::text(crate::messages::snippet::SNIPPET_INTERACTIVE_ON),
            content_w,
            box_width,
        );
    }
    design::section_close(lines, box_width);

    // TRACK RECORD (reliability verdict + inset trend chart), only with runs.
    let records = runs.for_snippet(&snippet.name);
    let chart_row = if records.is_empty() {
        None
    } else {
        build_track_record(lines, &snippet.name, runs, records, box_width)
    };

    // COMMAND
    design::section_open(lines, crate::i18n::text("COMMAND"), box_width);
    for row in wrap_lines(&snippet.command, content_w) {
        design::section_line(lines, highlight_params(&row), box_width);
    }
    design::section_close(lines, box_width);

    // PARAMETERS
    design::section_open(lines, "PARAMETERS", box_width);
    if params.is_empty() {
        design::section_line(lines, vec![Span::styled("none", theme::muted())], box_width);
    } else {
        for p in &params {
            let (value, style) = match &p.default {
                Some(d) => (format!("= {d}"), theme::muted()),
                None => ("required".to_string(), theme::warning()),
            };
            design::section_field_styled(lines, &p.name, &value, style, 0, box_width);
        }
    }
    design::section_close(lines, box_width);

    // IMPACT: a worst-severity verdict in the title border, then up to four
    // severity-ordered callouts, then the fleet-scope line (recoloured at
    // Elevated+ because the fleet is the blast-radius multiplier).
    let verdict = impact.verdict();
    design::section_open_with_status(lines, "IMPACT", impact_verdict_status(verdict), box_width);
    for f in impact.callouts().into_iter().take(4) {
        design::section_line(
            lines,
            vec![Span::styled(
                format!("{} {}", severity_glyph(f.severity), callout_text(f)),
                severity_style(f.severity),
            )],
            box_width,
        );
    }
    let fleet_style = if verdict >= Severity::Elevated {
        theme::warning()
    } else {
        theme::muted()
    };
    design::section_line(
        lines,
        vec![Span::styled(
            format!(
                "{} {}",
                design::ICON_PENDING,
                crate::i18n::text(crate::messages::snippet::IMPACT_FLEET_SCOPE)
            ),
            fleet_style,
        )],
        box_width,
    );
    design::section_close(lines, box_width);

    chart_row
}

/// Wrap `text` to `width` display columns (unicode-aware), splitting on logical
/// newlines first. Delegates the column-aware word wrap to
/// [`design::wrap_indented`] so wide chars never overflow the card.
fn wrap_lines(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    for logical in text.split('\n') {
        if logical.is_empty() {
            out.push(String::new());
        } else {
            out.extend(design::wrap_indented(logical, "", width));
        }
    }
    out
}

/// Split a single command row into spans, accenting `{{param}}` tokens. A token
/// split across a wrap boundary renders plain (rare on a 60+ column panel).
fn highlight_params(row: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut rest = row;
    while let Some(start) = rest.find("{{") {
        if let Some(end_rel) = rest[start + 2..].find("}}") {
            let end = start + 2 + end_rel + 2;
            if start > 0 {
                spans.push(Span::raw(rest[..start].to_string()));
            }
            spans.push(Span::styled(
                rest[start..end].to_string(),
                theme::accent_bold(),
            ));
            rest = &rest[end..];
            continue;
        }
        break;
    }
    if !rest.is_empty() {
        spans.push(Span::raw(rest.to_string()));
    }
    if spans.is_empty() {
        spans.push(Span::raw(String::new()));
    }
    spans
}

/// Semantic colour for a run outcome. Single source of truth for the
/// outcome-strip glyph and the last-run marker.
fn status_style(status: RunStatus) -> Style {
    match status {
        RunStatus::Ok => theme::healthy(),
        RunStatus::Partial => theme::warning(),
        RunStatus::Failed => theme::error(),
    }
}

fn status_glyph(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Ok => design::ICON_SUCCESS,
        RunStatus::Partial => design::ICON_WARNING,
        RunStatus::Failed => design::ICON_ERROR,
    }
}

/// Reliability colour by success-rate threshold: green at or above 90%, amber
/// at or above 60%, red below.
fn rate_style(rate: f64) -> Style {
    if rate >= 0.9 {
        theme::healthy()
    } else if rate >= 0.6 {
        theme::warning()
    } else {
        theme::error()
    }
}

/// Build the TRACK RECORD card: the reliability verdict (glyph + percent, rate
/// coloured) sits in the title border, then `CHART_ROWS` rows reserved for the
/// inset trend Chart (only when there are >= 2 runs to trend), then a recency +
/// tally summary line. Returns the line index of the first reserved chart row
/// so the caller can paint the Chart over it, or `None` when no chart is drawn.
fn build_track_record(
    lines: &mut Vec<Line<'static>>,
    name: &str,
    runs: &SnippetRunLog,
    records: &[RunRecord],
    box_width: usize,
) -> Option<usize> {
    let rate = runs.success_rate(name).unwrap_or(0.0).clamp(0.0, 1.0);
    let pct = (rate * 100.0).round() as u16;
    let headline = vec![Span::styled(
        format!(
            "{} {}",
            rate_glyph(rate),
            crate::messages::snippet::reliability_headline(pct)
        ),
        rate_style(rate).add_modifier(Modifier::BOLD),
    )];
    design::section_open_with_status(lines, "TRACK RECORD", headline, box_width);

    // Reserve blank bordered rows for the trend chart only when there is a
    // trend to draw (>= 2 runs) AND the panel is wide enough to actually paint
    // it. The width gate mirrors `chart_rect` so a narrow / mid-animation panel
    // shows the summary alone instead of an empty gap. The chart is painted
    // over these rows in the second render pass.
    let chart_fits = box_width >= (MIN_CHART_WIDTH + 2 * CHART_INSET) as usize;
    let chart_row = if records.len() >= 2 && chart_fits {
        // Top margin: braille fills the top of its cell, so without this row the
        // graph sits flush against the title border while the other cards' text
        // rows do not. The blank row gives the chart the same visual breathing
        // room as the text cards.
        lines.push(design::section_empty_line(box_width));
        let start = lines.len();
        for _ in 0..CHART_ROWS {
            lines.push(design::section_empty_line(box_width));
        }
        Some(start)
    } else {
        None
    };

    design::section_line(lines, summary_spans(records), box_width);
    design::section_close(lines, box_width);
    chart_row
}

/// Paint the reliability-trend Chart into its inset sub-Rect. The line plots
/// each run's host-success percent against a fixed 0-100 y-axis, so the eye
/// reads whether reliability is stable or drifting, not just the aggregate
/// percent. Skips rendering when the panel is too narrow or short (the inset
/// `Rect` would otherwise bleed past the card or a clipped region).
fn render_trend_chart(frame: &mut Frame, area: Rect, chart_row: usize, records: &[RunRecord]) {
    let Some(rect) = chart_rect(area, chart_row, CHART_ROWS) else {
        return;
    };
    let points = chart_points(records);
    // Backstops the `records.len() - 1` x-axis bound below against an empty or
    // single-point slice. Unreachable via build_track_record's >= 2 gate, but
    // keeps this stand-alone fn safe against a 0-width x-axis / usize underflow.
    if points.len() < 2 {
        return;
    }
    let total: usize = records.iter().map(|r| r.hosts).sum();
    let ok: usize = records.iter().map(|r| r.ok).sum();
    // Same convention as the title-border headline (success_rate -> 0.0 when
    // there are no host attempts) so the line colour never disagrees with the
    // verdict glyph on the degenerate all-zero-host history.
    let rate = if total == 0 {
        0.0
    } else {
        ok as f64 / total as f64
    };
    let last_x = (records.len() - 1) as f64;
    let dataset = Dataset::default()
        .data(&points)
        .graph_type(GraphType::Line)
        .marker(symbols::Marker::Braille)
        .style(rate_style(rate));
    let x_axis = Axis::default()
        .bounds([0.0, last_x])
        .style(theme::border_dim());
    let y_axis = Axis::default()
        .bounds([0.0, CHART_Y_MAX])
        .labels(["0".to_string(), (CHART_Y_MAX as u16).to_string()])
        .style(theme::border_dim());
    let chart = Chart::new(vec![dataset])
        .x_axis(x_axis)
        .y_axis(y_axis)
        .style(theme::muted())
        .legend_position(None);
    frame.render_widget(chart, rect);
}

/// Recency + tally line: `last ✓ 2h ago · 28 of 29 host runs ok · 1 partial`.
/// The last-run glyph carries its outcome colour; the rest stays muted.
fn summary_spans(records: &[RunRecord]) -> Vec<Span<'static>> {
    use crate::messages::snippet as msg;
    let total: usize = records.iter().map(|r| r.hosts).sum();
    let ok: usize = records.iter().map(|r| r.ok).sum();
    let tally = RunTally::of(records);

    let mut spans: Vec<Span<'static>> = Vec::new();
    if let Some(last) = records.last() {
        let st = last.status();
        let ago =
            crate::key_activity::humanize_last_use(crate::key_activity::now_for_render(), last.ts);
        spans.push(Span::styled(
            format!("{} ", msg::RUN_LAST_PREFIX),
            theme::muted(),
        ));
        spans.push(Span::styled(
            format!("{} ", status_glyph(st)),
            status_style(st),
        ));
        spans.push(Span::styled(format!("{ago} \u{00B7} "), theme::muted()));
    }
    spans.push(Span::styled(msg::hosts_ok_ratio(ok, total), theme::muted()));
    if tally.partial > 0 {
        spans.push(Span::styled(" \u{00B7} ", theme::muted()));
        spans.push(Span::styled(
            msg::runs_partial(tally.partial),
            theme::muted(),
        ));
    }
    if tally.failed > 0 {
        spans.push(Span::styled(" \u{00B7} ", theme::muted()));
        spans.push(Span::styled(msg::runs_failed(tally.failed), theme::muted()));
    }
    spans
}

/// Per-run host-success ratios as chart points: x = run index (oldest first),
/// y = percent of hosts that exited 0 in that run (0.0..=100.0). A zero-host
/// run is 100.0 (vacuously ok, matching [`RunRecord::status`]).
fn chart_points(records: &[RunRecord]) -> Vec<(f64, f64)> {
    records
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let pct = if r.hosts == 0 {
                100.0
            } else {
                r.ok as f64 / r.hosts as f64 * 100.0
            };
            (i as f64, pct)
        })
        .collect()
}

/// Threshold glyph paired with the reliability headline so the verdict reads
/// under `NO_COLOR`. Same thresholds as [`rate_style`].
fn rate_glyph(rate: f64) -> &'static str {
    if rate >= 0.9 {
        design::ICON_SUCCESS
    } else if rate >= 0.6 {
        design::ICON_WARNING
    } else {
        design::ICON_ERROR
    }
}

/// Glyph for an impact severity (also reads under `NO_COLOR`).
fn severity_glyph(s: Severity) -> &'static str {
    match s {
        Severity::ReadOnly => design::ICON_SUCCESS,
        Severity::WritesState => design::ICON_PENDING,
        Severity::Elevated => design::ICON_WARNING,
        Severity::Critical => design::ICON_ERROR,
    }
}

/// Colour for an impact severity: healthy/muted/warning/error.
fn severity_style(s: Severity) -> Style {
    match s {
        Severity::ReadOnly => theme::healthy(),
        Severity::WritesState => theme::muted(),
        Severity::Elevated => theme::warning(),
        Severity::Critical => theme::error(),
    }
}

/// The IMPACT card's title-border verdict (glyph + headline, bold, coloured by
/// the worst severity).
fn impact_verdict_status(v: Severity) -> Vec<Span<'static>> {
    use crate::messages::snippet as msg;
    let label = match v {
        Severity::ReadOnly => msg::IMPACT_VERDICT_READONLY,
        Severity::WritesState => msg::IMPACT_VERDICT_WRITES,
        Severity::Elevated => msg::IMPACT_VERDICT_ELEVATED,
        Severity::Critical => msg::IMPACT_VERDICT_CRITICAL,
    };
    vec![Span::styled(
        format!("{} {}", severity_glyph(v), label),
        severity_style(v).add_modifier(Modifier::BOLD),
    )]
}

/// Build a callout's text from its category, parameterised by the subject.
fn callout_text(f: &Finding) -> String {
    use crate::messages::snippet as msg;
    match f.category {
        Category::Destructive => msg::impact_destructive(&f.subject),
        Category::Irreversible => msg::impact_irreversible(&f.subject),
        Category::Privilege => msg::IMPACT_PRIVILEGE.to_string(),
        Category::RemoteExec => msg::IMPACT_REMOTE_EXEC.to_string(),
        Category::Service => msg::impact_service(&f.subject),
        Category::Availability => msg::IMPACT_AVAILABILITY.to_string(),
        Category::Package => msg::impact_package(&f.subject),
        Category::Redirect => msg::impact_redirect(&f.subject),
        Category::Secrets => msg::IMPACT_SECRETS.to_string(),
        Category::Unknown => msg::impact_unknown(&f.subject),
    }
}

/// Inset sub-Rect for the trend chart, or `None` when the panel is too narrow
/// or too short to render it without bleeding past the card borders. The chart
/// owns a Rect we inset 3 columns each side, so it is never full-bleed; the
/// height/width guards keep it from painting over a clipped panel.
fn chart_rect(area: Rect, chart_row: usize, height: u16) -> Option<Rect> {
    let row = u16::try_from(chart_row).ok()?;
    let width = area.width.checked_sub(CHART_INSET * 2)?;
    if width < MIN_CHART_WIDTH || row.checked_add(height)? > area.height {
        return None;
    }
    Some(Rect::new(area.x + CHART_INSET, area.y + row, width, height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snippet::Snippet;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    fn rec(hosts: usize, ok: usize) -> RunRecord {
        RunRecord {
            ts: 0,
            hosts,
            ok,
            failed: hosts - ok,
        }
    }

    fn text_of(spans: &[Span<'static>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn snippet(name: &str) -> Snippet {
        Snippet {
            name: name.to_string(),
            command: "echo {{x:hi}}".to_string(),
            description: "demo snippet".to_string(),
            interactive: false,
        }
    }

    fn runs_with(name: &str, samples: &[(usize, usize)]) -> SnippetRunLog {
        let mut log = SnippetRunLog::default();
        for &(hosts, ok) in samples {
            log.record(name, rec(hosts, ok));
        }
        log
    }

    fn is_braille(symbol: &str) -> bool {
        symbol
            .chars()
            .next()
            .is_some_and(|c| ('\u{2800}'..='\u{28FF}').contains(&c))
    }

    /// Render the detail panel into a TestBackend buffer. Holds the shared test
    /// lock and pins the theme so it serialises against the visual suite.
    fn render_buffer(width: u16, height: u16, snip: &Snippet, runs: &SnippetRunLog) -> Buffer {
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::ui::theme::init_with_mode(1);
        crate::ui::theme::set_theme(crate::ui::theme::ThemeDef::purple());
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|f| render(f, Rect::new(0, 0, width, height), Some(snip), &[], runs))
            .expect("draw");
        terminal.backend().buffer().clone()
    }

    fn row_text(buf: &Buffer, y: u16, width: u16) -> String {
        (0..width)
            .map(|x| buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(""))
            .collect()
    }

    fn buffer_contains(buf: &Buffer, width: u16, height: u16, needle: &str) -> bool {
        (0..height).any(|y| row_text(buf, y, width).contains(needle))
    }

    #[test]
    fn overview_shows_interactive_shell_only_when_on() {
        let (w, h) = (60, 30);
        let label = crate::i18n::text(crate::messages::snippet::SNIPPET_INTERACTIVE_LABEL);
        let plain = snippet("plain");
        let buf = render_buffer(w, h, &plain, &SnippetRunLog::default());
        assert!(!buffer_contains(&buf, w, h, label));

        let interactive = Snippet {
            interactive: true,
            ..snippet("pm2")
        };
        let buf = render_buffer(w, h, &interactive, &SnippetRunLog::default());
        // The value sits apart from its label, not glued onto it.
        let row = (0..h)
            .map(|y| row_text(&buf, y, w))
            .find(|r| r.contains(label))
            .expect("interactive row");
        let after = row.split(label).nth(1).unwrap_or_default();
        assert!(after.starts_with("  "), "{row}");
        assert!(after.trim_start().starts_with("yes"), "{row}");
    }

    #[test]
    fn chart_points_map_per_run_host_success_percent() {
        // The demo deploy history: one partial (4 hosts, 3 ok = 75%) at index 3.
        let records = [
            rec(3, 3),
            rec(3, 3),
            rec(4, 4),
            rec(4, 3),
            rec(4, 4),
            rec(5, 5),
            rec(3, 3),
            rec(3, 3),
        ];
        let pts = chart_points(&records);
        let ys: Vec<f64> = pts.iter().map(|p| p.1).collect();
        assert_eq!(
            ys,
            vec![100.0, 100.0, 100.0, 75.0, 100.0, 100.0, 100.0, 100.0]
        );
        // x is the 0-based run index, oldest first.
        assert_eq!(pts.first().map(|p| p.0), Some(0.0));
        assert_eq!(pts.last().map(|p| p.0), Some(7.0));
    }

    #[test]
    fn chart_points_failed_run_is_zero() {
        // A run where no host succeeded plots at the 0 baseline.
        let pts = chart_points(&[rec(1, 1), rec(1, 0), rec(1, 1)]);
        assert_eq!(pts[1].1, 0.0);
    }

    #[test]
    fn chart_points_zero_host_run_is_full() {
        // Degenerate zero-host run is vacuously ok (matches RunRecord::status).
        let pts = chart_points(&[rec(0, 0)]);
        assert_eq!(pts[0].1, 100.0);
    }

    #[test]
    fn rate_glyph_follows_rate_style_thresholds() {
        assert_eq!(rate_glyph(0.97), design::ICON_SUCCESS);
        assert_eq!(rate_glyph(0.90), design::ICON_SUCCESS);
        assert_eq!(rate_glyph(0.89), design::ICON_WARNING);
        assert_eq!(rate_glyph(0.60), design::ICON_WARNING);
        assert_eq!(rate_glyph(0.59), design::ICON_ERROR);
        assert_eq!(rate_glyph(0.0), design::ICON_ERROR);
    }

    #[test]
    fn rate_style_follows_thresholds() {
        // The colour half of the verdict must track the same thresholds as the
        // glyph half, so a flaky snippet never reads as healthy green.
        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        assert_eq!(rate_style(0.97), theme::healthy());
        assert_eq!(rate_style(0.90), theme::healthy());
        assert_eq!(rate_style(0.89), theme::warning());
        assert_eq!(rate_style(0.60), theme::warning());
        assert_eq!(rate_style(0.59), theme::error());
        assert_eq!(rate_style(0.0), theme::error());
    }

    #[test]
    fn chart_rect_insets_within_a_full_panel() {
        let area = Rect::new(0, 0, 72, 40);
        let r = chart_rect(area, 8, CHART_ROWS).expect("fits");
        assert_eq!(r.x, 3);
        assert_eq!(r.width, 66);
        assert_eq!(r.y, 8);
        assert_eq!(r.height, CHART_ROWS);
        // Stays strictly inside the card side borders (col 0 and col 71).
        assert!(r.x >= 1);
        assert!(r.x + r.width <= 71);
    }

    #[test]
    fn chart_rect_suppressed_when_too_narrow() {
        // Mid-animation / tiny terminal: a narrow panel yields no chart rather
        // than a bar bleeding past the border.
        assert!(chart_rect(Rect::new(0, 0, 10, 40), 8, CHART_ROWS).is_none());
    }

    #[test]
    fn chart_rect_suppressed_when_below_panel_bottom() {
        // Reserved rows that fall outside a short panel are skipped, so the
        // chart never paints over a clipped region.
        assert!(chart_rect(Rect::new(0, 0, 72, 10), 9, CHART_ROWS).is_none());
    }

    #[test]
    fn chart_rect_width_boundary_is_exact() {
        // inner = width - 2*CHART_INSET; 22 -> 16 == MIN_CHART_WIDTH draws,
        // 21 -> 15 suppresses. Pins the `<` guard against an off-by-one.
        let r = chart_rect(Rect::new(0, 0, 22, 40), 8, CHART_ROWS).expect("22 fits");
        assert_eq!(r.width, MIN_CHART_WIDTH);
        assert!(chart_rect(Rect::new(0, 0, 21, 40), 8, CHART_ROWS).is_none());
    }

    #[test]
    fn chart_rect_reduced_width_stays_inset() {
        // A partial (mid-animation) panel width still insets cleanly inside the
        // card borders rather than bleeding.
        let r = chart_rect(Rect::new(0, 0, 40, 24), 8, CHART_ROWS).expect("40 fits");
        assert_eq!(r.x, CHART_INSET);
        assert_eq!(r.width, 40 - 2 * CHART_INSET);
        // Right edge stays before the card's right border column (39).
        assert!(r.x + r.width < 40);
    }

    #[test]
    fn chart_rect_bottom_boundary_is_exact() {
        // Exact fit (row + height == area.height) draws; one row taller skips.
        assert!(chart_rect(Rect::new(0, 0, 72, 11), 8, CHART_ROWS).is_some());
        assert!(chart_rect(Rect::new(0, 0, 72, 10), 8, CHART_ROWS).is_none());
    }

    #[test]
    fn summary_reports_host_tally_and_nonzero_categories() {
        // 28 of 29 host runs ok, one partial run.
        let records = [rec(3, 3), rec(4, 4), rec(5, 5), rec(4, 3), rec(13, 13)];
        let text = text_of(&summary_spans(&records));
        assert!(text.contains("28 of 29 host runs ok"));
        assert!(text.contains("1 partial"));
        assert!(!text.contains("failed"));
        assert!(text.contains(crate::i18n::text(crate::messages::snippet::RUN_LAST_PREFIX)));
        // The trailing run is Ok, so the last-run marker is the success glyph.
        assert!(text.contains(design::ICON_SUCCESS));
    }

    #[test]
    fn status_glyph_covers_every_outcome() {
        assert_eq!(status_glyph(RunStatus::Ok), design::ICON_SUCCESS);
        assert_eq!(status_glyph(RunStatus::Partial), design::ICON_WARNING);
        assert_eq!(status_glyph(RunStatus::Failed), design::ICON_ERROR);
    }

    #[test]
    fn track_record_single_run_reserves_no_chart_rows() {
        // One run is a point, not a trend: no chart rows reserved, returns None,
        // and the summary line still renders. (open + summary + close = 3.)
        let runs = runs_with("s", &[(3, 3)]);
        let records = runs.for_snippet("s");
        let mut lines = Vec::new();
        let row = build_track_record(&mut lines, "s", &runs, records, 72);
        assert!(row.is_none());
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn track_record_reserves_chart_rows_for_a_trend_on_a_wide_panel() {
        // >= 2 runs on a full-width panel: a one-row top margin then the chart
        // rows (open + margin + CHART_ROWS + summary + close). The chart starts
        // at index 2 (after the title border and the margin row).
        let runs = runs_with("s", &[(3, 3), (3, 2)]);
        let records = runs.for_snippet("s");
        let mut lines = Vec::new();
        let row = build_track_record(&mut lines, "s", &runs, records, 72);
        assert_eq!(row, Some(2));
        assert_eq!(lines.len(), 4 + CHART_ROWS as usize);
    }

    #[test]
    fn track_record_skips_chart_rows_on_a_narrow_panel() {
        // Mirrors chart_rect's width gate: a panel too narrow to paint the chart
        // reserves nothing, so a mid-animation slide shows no empty gap.
        let runs = runs_with("s", &[(3, 3), (3, 2)]);
        let records = runs.for_snippet("s");
        let mut lines = Vec::new();
        let row = build_track_record(&mut lines, "s", &runs, records, 20);
        assert!(row.is_none());
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn chart_renders_inset_at_an_intermediate_panel_width() {
        // The detail panel width animates; the chart must stay inside the card
        // and never paint braille onto a border column.
        let runs = runs_with("deploy", &[(4, 4), (4, 3), (4, 4)]);
        let width = 40u16;
        let height = 24u16;
        let buf = render_buffer(width, height, &snippet("deploy"), &runs);
        let mut interior_braille = 0;
        for y in 0..height {
            for x in 0..width {
                let sym = buf.cell((x, y)).map(|c| c.symbol()).unwrap_or("");
                if is_braille(sym) {
                    assert!(x > 0 && x < width - 1, "braille bled onto border col {x}");
                    interior_braille += 1;
                }
            }
        }
        assert!(
            interior_braille > 0,
            "chart should paint braille at width 40"
        );
    }

    #[test]
    fn error_tier_headline_renders_the_error_glyph() {
        // A sub-60% snippet must render the red error verdict glyph in the
        // TRACK RECORD title, not the green/amber one.
        let runs = runs_with("flaky", &[(3, 1), (3, 1)]); // 2 of 6 ok = 33%
        let width = 72u16;
        let height = 24u16;
        let buf = render_buffer(width, height, &snippet("flaky"), &runs);
        let title = (0..height)
            .map(|y| row_text(&buf, y, width))
            .find(|row| row.contains("TRACK RECORD"))
            .expect("TRACK RECORD card renders");
        assert!(
            title.contains(design::ICON_ERROR),
            "error tier title: {title}"
        );
        assert!(!title.contains(design::ICON_SUCCESS));
    }
}
