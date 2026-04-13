mod command_palette;
mod confirm_dialog;
pub(crate) mod containers;
mod detail_panel;
mod file_browser;
mod help;
mod host_detail;
pub mod host_form;
mod host_list;
mod key_detail;
mod key_list;
mod provider_list;
mod snippet_form;
mod snippet_output;
mod snippet_param_form;
mod snippet_picker;
mod tag_picker;
pub mod theme;
mod theme_picker;
mod tunnel_form;
mod tunnel_list;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Screen};

const MIN_WIDTH: u16 = 50;
const MIN_HEIGHT: u16 = 14;

/// Top-level render dispatcher.
pub fn render(frame: &mut Frame, app: &mut App, anim: &mut crate::animation::AnimationState) {
    anim.tick_overlay_anim();
    let area = frame.area();

    // Terminal too small guard
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let msg = Paragraph::new(Line::from(vec![
            Span::styled("\u{26A0}", theme::error()),
            Span::raw(" Terminal too small. Need at least 50x14."),
        ]));
        frame.render_widget(msg, area);
        return;
    }

    // Render host list with animated detail panel width. When an overlay is active,
    // hide the status so it only appears in the overlay's own footer.
    // Note: host_list::render does not set app.status, so the unconditional restore
    // is safe. If that invariant ever changes, use get_or_insert semantics instead.
    let has_overlay = !matches!(app.screen, Screen::HostList) || app.palette.is_some();
    let status = if has_overlay { app.status.take() } else { None };
    let detail_progress = anim.detail_anim_progress();
    host_list::render(frame, app, anim.spinner_tick, detail_progress);
    if let Some(s) = status {
        app.status = Some(s);
    }
    match &app.screen {
        Screen::HostList => {
            render_overlay_close(frame, anim);
        }
        Screen::AddHost | Screen::EditHost { .. } => {
            render_overlay(frame, app, anim, host_form::render);
        }
        Screen::ConfirmDelete { alias } => {
            let alias = alias.clone();
            render_overlay(frame, app, anim, |frame, app| {
                confirm_dialog::render(frame, app, &alias)
            });
        }
        Screen::Help { .. } => {
            render_overlay(frame, app, anim, help::render);
        }
        Screen::KeyList => {
            render_overlay(frame, app, anim, key_list::render);
        }
        Screen::KeyDetail { index } => {
            let index = *index;
            render_overlay(frame, app, anim, |frame, app| {
                key_list::render(frame, app);
                key_detail::render(frame, app, index);
            });
        }
        Screen::HostDetail { index } => {
            let index = *index;
            render_overlay(frame, app, anim, |frame, app| {
                host_detail::render(frame, app, index)
            });
        }
        Screen::TagPicker => {
            render_overlay(frame, app, anim, tag_picker::render);
        }
        Screen::ThemePicker => {
            render_overlay_nodim(frame, app, anim, theme_picker::render);
        }
        Screen::Providers => {
            render_overlay(frame, app, anim, |frame, app| {
                provider_list::render_provider_list(frame, app)
            });
        }
        Screen::ProviderForm { provider } => {
            let provider = provider.clone();
            render_overlay(frame, app, anim, |frame, app| {
                provider_list::render_provider_form(frame, app, &provider)
            });
        }
        Screen::TunnelList { alias } => {
            let alias = alias.clone();
            render_overlay(frame, app, anim, |frame, app| {
                tunnel_list::render(frame, app, &alias)
            });
        }
        Screen::TunnelForm { alias, .. } => {
            let alias = alias.clone();
            render_overlay(frame, app, anim, |frame, app| {
                tunnel_list::render(frame, app, &alias);
                tunnel_form::render(frame, app);
            });
        }
        Screen::SnippetPicker { .. } => {
            render_overlay(frame, app, anim, snippet_picker::render);
        }
        Screen::SnippetForm { .. } => {
            render_overlay(frame, app, anim, |frame, app| {
                snippet_picker::render(frame, app);
                snippet_form::render(frame, app);
            });
        }
        Screen::ConfirmHostKeyReset { hostname, .. } => {
            let hostname = hostname.clone();
            render_overlay(frame, app, anim, |frame, app| {
                confirm_dialog::render_host_key_reset(frame, app, &hostname)
            });
        }
        Screen::FileBrowser { .. } => {
            render_overlay(frame, app, anim, file_browser::render);
        }
        Screen::SnippetOutput { .. } => {
            render_overlay(frame, app, anim, snippet_output::render);
        }
        Screen::SnippetParamForm { .. } => {
            render_overlay(frame, app, anim, |frame, app| {
                snippet_picker::render(frame, app);
                snippet_param_form::render(frame, app);
            });
        }
        Screen::ConfirmImport { count } => {
            let count = *count;
            render_overlay(frame, app, anim, |frame, app| {
                confirm_dialog::render_confirm_import(frame, app, count)
            });
        }
        Screen::Containers { .. } => {
            render_overlay(frame, app, anim, containers::render);
        }
        Screen::ConfirmVaultSign { signable } => {
            let aliases: Vec<String> = signable.iter().map(|(a, _, _, _, _)| a.clone()).collect();
            render_overlay(frame, app, anim, move |frame, app| {
                confirm_dialog::render_confirm_vault_sign(frame, app, &aliases)
            });
        }
        Screen::ConfirmPurgeStale { aliases, provider } => {
            let aliases = aliases.clone();
            let provider = provider.clone();
            render_overlay(frame, app, anim, |frame, app| {
                confirm_dialog::render_confirm_purge_stale(frame, app, &aliases, &provider)
            });
        }
        Screen::Welcome {
            has_backup,
            host_count,
            known_hosts_count,
        } => {
            let has_backup = *has_backup;
            let host_count = *host_count;
            let known_hosts_count = *known_hosts_count;
            render_overlay(frame, app, anim, |frame, app| {
                confirm_dialog::render_welcome(
                    frame,
                    app,
                    has_backup,
                    host_count,
                    known_hosts_count,
                )
            });
        }
    }

    // Command palette renders on top of any screen. Rendered directly (not via
    // render_overlay) to avoid polluting the overlay_close animation buffer,
    // which is reserved for Screen-driven overlays.
    if app.palette.is_some() {
        dim_background(frame);
        command_palette::render(frame, app);
    }

    // Toast overlay renders on top of everything
    render_toast(frame, app);
}

/// Render an overlay with dimmed background and scale-clip animation.
fn render_overlay(
    frame: &mut Frame,
    app: &mut App,
    anim: &mut crate::animation::AnimationState,
    f: impl FnOnce(&mut Frame, &mut App),
) {
    render_overlay_inner(frame, app, anim, true, f);
}

/// Render an overlay without dimming the background.
/// Used for the theme picker so the live preview stays visible.
fn render_overlay_nodim(
    frame: &mut Frame,
    app: &mut App,
    anim: &mut crate::animation::AnimationState,
    f: impl FnOnce(&mut Frame, &mut App),
) {
    render_overlay_inner(frame, app, anim, false, f);
}

/// Shared overlay render logic. Applies scale-clip animation for smooth open
/// transitions. Saves the buffer and dim flag together in `OverlayCloseState`
/// for the close animation. Status messages remain visible so overlay footers
/// can display them via `render_footer_with_status`.
fn render_overlay_inner(
    frame: &mut Frame,
    app: &mut App,
    anim: &mut crate::animation::AnimationState,
    dim: bool,
    f: impl FnOnce(&mut Frame, &mut App),
) {
    if dim {
        dim_background(frame);
    }

    // Save host list before overlay renders (needed for open animation).
    let progress = anim.overlay_anim_progress();
    let animating_open = progress.is_some();
    let pre_overlay = if animating_open {
        Some(frame.buffer_mut().clone())
    } else {
        None
    };

    f(frame, app);

    // Save overlay state for close animation once (first stable frame).
    // The dim flag is captured alongside the buffer so close knows whether to dim.
    if !animating_open && anim.overlay_close.is_none() {
        anim.overlay_close = Some(crate::animation::OverlayCloseState {
            buffer: frame.buffer_mut().clone(),
            dimmed: dim,
        });
    }

    // Apply opening animation: clip overlay to a growing scaled region.
    if let (Some(progress), Some(saved)) = (progress, pre_overlay) {
        if progress < 1.0 {
            apply_scale_clip(frame, &saved, progress);
        }
    }
}

/// Dim all cells in the frame buffer so the host list behind an overlay appears muted.
/// On truecolor/ANSI-16 terminals the foreground is replaced with dark grey for a
/// stronger effect. Cells that already have a coloured background (badges, selected
/// row) only receive the DIM modifier so their text stays readable.
fn dim_background(frame: &mut Frame) {
    use ratatui::style::Color;

    let dim_only = Style::default().add_modifier(Modifier::DIM);
    let style = match theme::color_mode() {
        2 => Style::default()
            .fg(Color::Rgb(70, 70, 70))
            .add_modifier(Modifier::DIM),
        1 => Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM),
        _ => dim_only,
    };
    let area = frame.area();
    let buf = frame.buffer_mut();
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            let has_bg = buf[(x, y)].bg != Color::Reset;
            buf[(x, y)].set_style(if has_bg { dim_only } else { style });
        }
    }
}

/// Render the close animation: paint saved overlay buffer with shrinking scale clip.
/// Uses the dim flag captured alongside the buffer so it matches the open animation.
fn render_overlay_close(frame: &mut Frame, anim: &mut crate::animation::AnimationState) {
    let is_closing = anim.overlay_anim.as_ref().is_some_and(|a| !a.opening);
    if !is_closing {
        return;
    }

    let progress = match anim.overlay_anim_progress() {
        Some(p) => p,
        None => return,
    };

    if let Some(ref state) = anim.overlay_close {
        if progress > 0.0 {
            if state.dimmed {
                dim_background(frame);
            }
            let area = frame.area();
            let (left, right, top, bottom) = scale_clip_rect(area, progress);
            for y in top..bottom {
                for x in left..right {
                    if let Some(cell) = state.buffer.cell((x, y)) {
                        frame.buffer_mut()[(x, y)] = cell.clone();
                    }
                }
            }
        }
    }
}

/// Clip the frame buffer to a scaled region centered on screen (zoom effect).
/// Cells outside the clip are restored from `saved` (the pre-overlay host list).
fn apply_scale_clip(frame: &mut Frame, saved: &ratatui::buffer::Buffer, progress: f32) {
    let area = frame.area();
    let (left, right, top, bottom) = scale_clip_rect(area, progress);

    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            if y < top || y >= bottom || x < left || x >= right {
                if let Some(cell) = saved.cell((x, y)) {
                    frame.buffer_mut()[(x, y)] = cell.clone();
                }
            }
        }
    }
}

/// Calculate the visible rect for a scale/zoom animation centered on the area.
fn scale_clip_rect(area: Rect, progress: f32) -> (u16, u16, u16, u16) {
    let visible_w = (area.width as f32 * progress).ceil() as u16;
    let visible_h = (area.height as f32 * progress).ceil() as u16;
    let left = area.x + area.width.saturating_sub(visible_w) / 2;
    let right = (left + visible_w).min(area.x + area.width);
    let top = area.y + area.height.saturating_sub(visible_h) / 2;
    let bottom = (top + visible_h).min(area.y + area.height);
    (left, right, top, bottom)
}

/// Build a padded footer keycap span: ` key ` with reversed style.
pub fn footer_key_span(key: &str) -> Span<'static> {
    Span::styled(format!(" {} ", key), theme::footer_key())
}

/// Build a footer action span: padded keycap + muted label.
/// Use this for consistent footers across all screens.
pub fn footer_action(key: &str, label: &str) -> [Span<'static>; 2] {
    [
        footer_key_span(key),
        Span::styled(label.to_string(), theme::muted()),
    ]
}

/// Build a primary footer action span: padded keycap + muted label.
pub fn footer_primary(key: &str, label: &str) -> [Span<'static>; 2] {
    [
        footer_key_span(key),
        Span::styled(label.to_string(), theme::muted()),
    ]
}

/// Render footer with shortcuts on the left and "? more" or Info/Progress status on the right.
/// Keyboard hints are always visible. Toast-class messages are NOT shown here.
pub fn render_footer_with_help(
    frame: &mut Frame,
    area: Rect,
    footer_spans: Vec<Span<'_>>,
    app: &App,
) {
    // Only show footer-class status (Info or Progress), not toast-class
    let footer_status = app.status.as_ref().filter(|s| !s.is_toast());
    if let Some(status) = footer_status {
        render_footer_status_right(frame, area, footer_spans, status);
        return;
    }
    let right_spans = vec![
        Span::raw("  "),
        Span::styled(" ? ", theme::footer_key()),
        Span::styled(" more", theme::muted()),
    ];
    let right_width: u16 = right_spans.iter().map(|s| s.width()).sum::<usize>() as u16;
    let [left, right] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(right_width)]).areas(area);
    frame.render_widget(Paragraph::new(Line::from(footer_spans)), left);
    frame.render_widget(Paragraph::new(Line::from(right_spans)), right);
}

/// Render footer with shortcuts always visible and optional status right-aligned.
/// Used by overlay screens. Shows any active footer status (Info, Progress, or
/// sticky messages set via set_sticky_status).
pub fn render_footer_with_status(
    frame: &mut Frame,
    area: Rect,
    footer_spans: Vec<Span<'_>>,
    app: &App,
) {
    if let Some(ref status) = app.status {
        render_footer_status_right(frame, area, footer_spans, status);
    } else {
        frame.render_widget(Paragraph::new(Line::from(footer_spans)), area);
    }
}

/// Render footer with shortcuts left and a status message right-aligned.
/// Used for Info and Progress messages only (non-toast).
fn render_footer_status_right(
    frame: &mut Frame,
    area: Rect,
    mut footer_spans: Vec<Span<'_>>,
    status: &crate::app::StatusMessage,
) {
    let shortcuts_width: usize = footer_spans.iter().map(|s| s.width()).sum();
    let total_width = area.width as usize;

    let (icon, icon_style, text) = if status.sticky {
        // Sticky non-error = in-progress action. The spinner character
        // is embedded in the status text by the caller, so no extra
        // glyph prefix is needed here.
        ("", Style::default(), format!(" {} ", status.text))
    } else if status.is_error() {
        ("\u{26A0}", theme::error(), format!(" {} ", status.text))
    } else {
        ("", theme::muted(), format!(" {} ", status.text))
    };

    let available = total_width.saturating_sub(shortcuts_width + icon.width() + 2);
    let display_text = if text.width() > available && available > 3 {
        format!(" {} ", truncate(&status.text, available - 1))
    } else {
        text
    };
    let status_width = icon.width() + display_text.width();
    let gap = total_width.saturating_sub(shortcuts_width + status_width);
    if gap > 0 {
        footer_spans.push(Span::raw(" ".repeat(gap)));
        if !icon.is_empty() {
            footer_spans.push(Span::styled(icon, icon_style));
        }
        footer_spans.push(Span::styled(display_text, icon_style));
    }
    frame.render_widget(Paragraph::new(Line::from(footer_spans)), area);
}

/// Render a toast notification overlay in the bottom-right corner.
/// Toast is a small bordered box (max 40 cols wide, 3 rows tall).
fn render_toast(frame: &mut Frame, app: &App) {
    let toast = match app.toast.as_ref() {
        Some(t) => t,
        None => return,
    };

    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        return;
    }

    let (icon, border_style) = match toast.class {
        crate::app::MessageClass::Alert => ("\u{26A0} ", theme::toast_border_error()),
        crate::app::MessageClass::Confirmation
        | crate::app::MessageClass::Info
        | crate::app::MessageClass::Progress => ("\u{2713} ", theme::toast_border_success()),
    };

    let content = format!("{}{}", icon, toast.text);
    let content_width = content.width();
    // +4 for border (2) + padding (2). Cap at 60% of terminal width.
    let max_width = (area.width as usize * 60 / 100).max(30);
    let box_width =
        (content_width.saturating_add(4).min(max_width) as u16).min(area.width.saturating_sub(4));
    let box_height = 3u16;
    let x = area.width.saturating_sub(box_width + 2);
    // Position above the footer row (which is the last row)
    let y = area.height.saturating_sub(box_height + 2);

    let rect = Rect::new(x, y, box_width, box_height);

    // Clear the area behind the toast so it doesn't blend with content
    frame.render_widget(ratatui::widgets::Clear, rect);

    let block = ratatui::widgets::Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(border_style);

    // Truncate content to fit inside box (box_width - 2 for borders - 2 for padding)
    let inner_width = box_width.saturating_sub(4) as usize;
    let display = if content_width > inner_width {
        format!(" {} ", truncate(&content, inner_width))
    } else {
        format!(" {} ", content)
    };

    let paragraph = Paragraph::new(display).block(block);
    frame.render_widget(paragraph, rect);
}

/// Create a centered rect of given percentage within the parent rect.
pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}

/// Truncate a string to fit within `max_cols` display columns (unicode-width-aware).
pub(crate) fn truncate(s: &str, max_cols: usize) -> String {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
    if s.width() <= max_cols {
        return s.to_string();
    }
    if max_cols <= 1 {
        return String::new();
    }
    let target = max_cols - 1;
    let mut col = 0;
    let mut byte_end = 0;
    for ch in s.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if col + w > target {
            break;
        }
        col += w;
        byte_end += ch.len_utf8();
    }
    format!("{}…", &s[..byte_end])
}

/// Render a horizontal divider: ├─ Label ───────┤
/// The `├` and `┤` connectors use the border style so they blend with the outer
/// border. The horizontal `─` fill is rendered DIM to keep dividers visually
/// subordinate to the border.
pub(crate) fn render_divider(
    frame: &mut Frame,
    block_area: Rect,
    y: u16,
    label: &str,
    label_style: Style,
    border_style: Style,
) {
    let dim = theme::muted();
    let width = block_area.width as usize;
    let label_w = label.width();
    let fill = width.saturating_sub(3 + label_w);
    let line = Line::from(vec![
        Span::styled("├", border_style),
        Span::styled("─", dim),
        Span::styled(label.to_string(), label_style),
        Span::styled("─".repeat(fill), dim),
        Span::styled("┤", border_style),
    ]);
    frame.render_widget(
        Paragraph::new(line),
        Rect::new(block_area.x, y, block_area.width, 1),
    );
}

/// Create a centered rect with fixed dimensions.
pub fn centered_rect_fixed(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;

    use super::*;

    fn make_app() -> App {
        use std::path::PathBuf;
        let config = crate::ssh_config::model::SshConfigFile {
            elements: crate::ssh_config::model::SshConfigFile::parse_content(""),
            path: PathBuf::from("/tmp/test_config"),
            crlf: false,
            bom: false,
        };
        App::new(config)
    }

    #[test]
    fn dim_background_applies_dim_modifier() {
        let backend = TestBackend::new(10, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                // Write some text so cells are non-empty.
                let area = frame.area();
                frame.render_widget(ratatui::widgets::Paragraph::new("hello"), area);
                dim_background(frame);
                let buf = frame.buffer_mut();
                for x in 0..5 {
                    assert!(
                        buf[(x, 0)].modifier.contains(Modifier::DIM),
                        "cell ({x}, 0) should have DIM modifier"
                    );
                }
            })
            .unwrap();
    }

    #[test]
    fn dim_background_preserves_bg_color_cells() {
        let backend = TestBackend::new(10, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let buf = frame.buffer_mut();
                // Set a cell with a background color.
                buf[(0, 0)].set_bg(Color::Blue);
                buf[(0, 0)].set_fg(Color::White);
                dim_background(frame);
                let buf = frame.buffer_mut();
                // Cells with bg color should only get DIM, not fg recolor.
                assert!(buf[(0, 0)].modifier.contains(Modifier::DIM));
                assert_eq!(buf[(0, 0)].fg, Color::White);
            })
            .unwrap();
    }

    #[test]
    fn render_overlay_inner_captures_dimmed_true() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = make_app();
        let mut anim = crate::animation::AnimationState::new();
        terminal
            .draw(|frame| {
                render_overlay_inner(frame, &mut app, &mut anim, true, |_frame, _app| {});
            })
            .unwrap();
        let close = anim.overlay_close.as_ref().unwrap();
        assert!(close.dimmed);
    }

    #[test]
    fn render_overlay_inner_captures_dimmed_false() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = make_app();
        let mut anim = crate::animation::AnimationState::new();
        terminal
            .draw(|frame| {
                render_overlay_inner(frame, &mut app, &mut anim, false, |_frame, _app| {});
            })
            .unwrap();
        let close = anim.overlay_close.as_ref().unwrap();
        assert!(!close.dimmed);
    }

    #[test]
    fn render_overlay_inner_preserves_status_during_render() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = make_app();
        app.set_info_status("test");
        let mut anim = crate::animation::AnimationState::new();
        terminal
            .draw(|frame| {
                render_overlay_inner(frame, &mut app, &mut anim, true, |_frame, app| {
                    assert!(
                        app.status.is_some(),
                        "status should be visible during overlay render"
                    );
                });
            })
            .unwrap();
        assert!(
            app.status.is_some(),
            "status should still be present after overlay render"
        );
    }

    #[test]
    fn overlay_footer_renders_status_text_in_buffer() {
        let backend = TestBackend::new(80, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = make_app();
        app.set_info_status("sync failed");
        let mut anim = crate::animation::AnimationState::new();
        terminal
            .draw(|frame| {
                render_overlay_inner(frame, &mut app, &mut anim, false, |frame, app| {
                    let area = frame.area();
                    // Render a footer row using the last line of the frame.
                    let footer = ratatui::layout::Rect::new(0, area.height - 1, area.width, 1);
                    render_footer_with_status(frame, footer, vec![], app);
                });
            })
            .unwrap();
        // Read the last row from the buffer and check the status text is present.
        let buf = terminal.backend().buffer();
        let mut line = String::new();
        for x in 0..80 {
            line.push_str(buf[(x, 2)].symbol());
        }
        assert!(
            line.contains("sync failed"),
            "status text should appear in overlay footer buffer, got: {line:?}"
        );
    }

    #[test]
    fn host_list_footer_has_no_status_when_overlay_active() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = make_app();
        app.set_info_status("sync failed");
        // Simulate an overlay being active.
        app.screen = crate::app::Screen::Help {
            return_screen: Box::new(crate::app::Screen::HostList),
        };
        let has_overlay = !matches!(app.screen, crate::app::Screen::HostList);
        assert!(has_overlay, "should detect overlay");
        // Mimic render(): take status during host list render, then restore.
        let status = app.status.take();
        terminal
            .draw(|frame| {
                let area = frame.area();
                let footer = ratatui::layout::Rect::new(0, area.height - 1, area.width, 1);
                render_footer_with_status(frame, footer, vec![], &app);
            })
            .unwrap();
        // Host list footer should NOT contain the status text.
        let buf = terminal.backend().buffer();
        let mut line = String::new();
        for x in 0..80 {
            line.push_str(buf[(x, 23)].symbol());
        }
        assert!(
            !line.contains("sync failed"),
            "host list footer should not show status when overlay active, got: {line:?}"
        );
        // Restore and verify status is preserved for overlay.
        if let Some(s) = status {
            app.status = Some(s);
        }
        assert!(
            app.status.is_some(),
            "status should be restored for overlay footer"
        );
    }

    #[test]
    fn render_overlay_inner_saves_close_state() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = make_app();
        let mut anim = crate::animation::AnimationState::new();
        assert!(anim.overlay_close.is_none());
        terminal
            .draw(|frame| {
                render_overlay_inner(frame, &mut app, &mut anim, true, |_frame, _app| {});
            })
            .unwrap();
        assert!(anim.overlay_close.is_some());
    }

    #[test]
    fn scale_clip_rect_full_progress_covers_area() {
        let area = Rect::new(0, 0, 80, 24);
        let (left, right, top, bottom) = scale_clip_rect(area, 1.0);
        assert_eq!(left, 0);
        assert_eq!(right, 80);
        assert_eq!(top, 0);
        assert_eq!(bottom, 24);
    }

    #[test]
    fn scale_clip_rect_zero_progress_is_empty() {
        let area = Rect::new(0, 0, 80, 24);
        let (left, right, top, bottom) = scale_clip_rect(area, 0.0);
        assert_eq!(right - left, 0);
        assert_eq!(bottom - top, 0);
    }

    #[test]
    fn scale_clip_rect_half_progress_centered() {
        let area = Rect::new(0, 0, 80, 24);
        let (left, right, top, bottom) = scale_clip_rect(area, 0.5);
        let w = right - left;
        let h = bottom - top;
        assert_eq!(w, 40);
        assert_eq!(h, 12);
        // Centered
        assert_eq!(left, 20);
        assert_eq!(top, 6);
    }

    // --- render_overlay_close tests ---

    /// Helper: set up a closing animation at ~50% progress with a saved buffer and dim flag.
    fn setup_close_anim(anim: &mut crate::animation::AnimationState, dimmed: bool) {
        use std::time::{Duration, Instant};
        let duration = Duration::from_secs(1);
        anim.overlay_close = Some(crate::animation::OverlayCloseState {
            buffer: ratatui::buffer::Buffer::empty(Rect::new(0, 0, 20, 5)),
            dimmed,
        });
        // Start halfway through the close animation so the clip is small enough
        // that corner cells remain outside it (and thus show the dim effect).
        anim.overlay_anim = Some(crate::animation::OverlayAnim {
            start: Instant::now() - duration / 2,
            opening: false,
            duration_ms: duration.as_millis(),
        });
    }

    #[test]
    fn render_overlay_close_dims_when_close_state_dimmed() {
        let backend = TestBackend::new(20, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut anim = crate::animation::AnimationState::new();
        setup_close_anim(&mut anim, true);
        terminal
            .draw(|frame| {
                // Write visible text so we can detect dimming.
                let area = frame.area();
                frame.render_widget(ratatui::widgets::Paragraph::new("ABCDE"), area);
                render_overlay_close(frame, &mut anim);
                // Cells outside the shrinking clip should be dimmed.
                let buf = frame.buffer_mut();
                // Corner cell (0,4) is outside any reasonable clip at the start of close.
                assert!(
                    buf[(0, 4)].modifier.contains(Modifier::DIM),
                    "background should be dimmed during close of a dimmed overlay"
                );
            })
            .unwrap();
    }

    #[test]
    fn render_overlay_close_no_dim_when_close_state_not_dimmed() {
        let backend = TestBackend::new(20, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut anim = crate::animation::AnimationState::new();
        setup_close_anim(&mut anim, false);
        terminal
            .draw(|frame| {
                let area = frame.area();
                frame.render_widget(ratatui::widgets::Paragraph::new("ABCDE"), area);
                render_overlay_close(frame, &mut anim);
                let buf = frame.buffer_mut();
                assert!(
                    !buf[(0, 4)].modifier.contains(Modifier::DIM),
                    "background should NOT be dimmed during close of a non-dimmed overlay"
                );
            })
            .unwrap();
    }

    #[test]
    fn render_overlay_close_skips_when_not_closing() {
        let backend = TestBackend::new(20, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut anim = crate::animation::AnimationState::new();
        // No close animation set up.
        terminal
            .draw(|frame| {
                let area = frame.area();
                frame.render_widget(ratatui::widgets::Paragraph::new("ABCDE"), area);
                render_overlay_close(frame, &mut anim);
                let buf = frame.buffer_mut();
                // Nothing should change.
                assert!(
                    !buf[(0, 0)].modifier.contains(Modifier::DIM),
                    "no dimming when there is no close animation"
                );
            })
            .unwrap();
    }

    // --- apply_scale_clip tests ---

    #[test]
    fn apply_scale_clip_restores_cells_outside_clip() {
        let backend = TestBackend::new(10, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                // Render overlay content (fills entire buffer).
                frame.render_widget(ratatui::widgets::Paragraph::new("OVERLAY OK"), area);

                // Create a "saved" background buffer with different content.
                let mut saved = ratatui::buffer::Buffer::empty(area);
                for x in 0..area.width {
                    for y in 0..area.height {
                        saved[(x, y)].set_symbol("B");
                    }
                }

                // Apply clip at 50% progress: center 5x2 region keeps overlay,
                // outer cells restored from saved.
                apply_scale_clip(frame, &saved, 0.5);

                let buf = frame.buffer_mut();
                // (0,0) is outside the clip and should be restored to "B".
                assert_eq!(buf[(0, 0)].symbol(), "B");
                // Center cell should still have overlay content.
                let cx = area.width / 2;
                let cy = area.height / 2;
                assert_ne!(buf[(cx, cy)].symbol(), "B");
            })
            .unwrap();
    }

    #[test]
    fn render_toast_shows_confirmation_in_buffer() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = make_app();
        app.set_status("Copied web01", false); // Goes to toast (Confirmation)
        terminal
            .draw(|frame| {
                render_toast(frame, &app);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut found = false;
        for y in 0..24 {
            let mut line = String::new();
            for x in 0..80 {
                line.push_str(buf[(x, y)].symbol());
            }
            if line.contains("Copied web01") {
                found = true;
                break;
            }
        }
        assert!(found, "toast text should appear in buffer");
    }

    #[test]
    fn render_toast_not_shown_when_no_toast() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = make_app();
        assert!(app.toast.is_none());
        terminal
            .draw(|frame| {
                render_toast(frame, &app);
            })
            .unwrap();
        // Should not panic, just no-op
    }

    #[test]
    fn render_toast_shows_alert_with_warning_icon() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = make_app();
        app.set_status("Connection failed", true); // Goes to toast (Alert)
        terminal
            .draw(|frame| {
                render_toast(frame, &app);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut found_text = false;
        let mut found_icon = false;
        for y in 0..24 {
            let mut line = String::new();
            for x in 0..80 {
                line.push_str(buf[(x, y)].symbol());
            }
            if line.contains("Connection failed") {
                found_text = true;
            }
            if line.contains("\u{26A0}") {
                found_icon = true;
            }
        }
        assert!(found_text, "alert text should appear in buffer");
        assert!(found_icon, "alert should show warning icon");
    }

    #[test]
    fn footer_shows_hints_when_toast_active() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = make_app();
        app.set_status("Copied", false); // Goes to toast, NOT footer
        assert!(app.toast.is_some());
        assert!(app.status.is_none()); // Footer should be clear
        let footer_spans = vec![
            Span::styled(" ? ", theme::footer_key()),
            Span::styled(" more", theme::muted()),
        ];
        terminal
            .draw(|frame| {
                let area = Rect::new(0, 23, 80, 1);
                render_footer_with_help(frame, area, footer_spans, &app);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut line = String::new();
        for x in 0..80 {
            line.push_str(buf[(x, 23)].symbol());
        }
        assert!(
            line.contains("more"),
            "footer should show hints when only toast is active"
        );
    }

    #[test]
    fn footer_shows_info_status_instead_of_help_hint() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = make_app();
        app.set_info_status("Syncing AWS...");
        assert!(app.status.is_some());
        assert!(app.toast.is_none());
        let footer_spans = vec![
            Span::styled(" ? ", theme::footer_key()),
            Span::styled(" more", theme::muted()),
        ];
        terminal
            .draw(|frame| {
                let area = Rect::new(0, 23, 80, 1);
                render_footer_with_help(frame, area, footer_spans, &app);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut line = String::new();
        for x in 0..80 {
            line.push_str(buf[(x, 23)].symbol());
        }
        assert!(
            line.contains("Syncing AWS"),
            "footer should show info status, got: {line:?}"
        );
    }

    #[test]
    fn apply_scale_clip_full_progress_keeps_all_overlay() {
        let backend = TestBackend::new(10, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                frame.render_widget(ratatui::widgets::Paragraph::new("OVERLAY OK"), area);
                let mut saved = ratatui::buffer::Buffer::empty(area);
                for x in 0..area.width {
                    for y in 0..area.height {
                        saved[(x, y)].set_symbol("B");
                    }
                }
                // Full progress: nothing should be restored from saved.
                apply_scale_clip(frame, &saved, 1.0);
                let buf = frame.buffer_mut();
                assert_eq!(buf[(0, 0)].symbol(), "O"); // First char of "OVERLAY OK"
            })
            .unwrap();
    }
}
