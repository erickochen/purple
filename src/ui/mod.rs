pub(crate) mod activity_chart;
mod bulk_tag_editor;
pub(crate) mod confirm_dialog;
pub(crate) mod container_action_confirm;
pub(crate) mod container_exec_prompt;
pub(crate) mod container_host_picker;
pub(crate) mod container_logs;
pub(crate) mod containers;
pub(crate) mod containers_overview;
pub mod design;
mod detail_panel;
mod file_browser;
mod help;
mod host_detail;
pub mod host_form;
mod host_list;
mod jump;
mod key_detail;
mod key_list;
pub(crate) mod key_push_picker;
pub(crate) mod keys_overview;
pub(crate) mod password_prompt;
mod picker_helpers;
mod provider_list;
mod snippet_form;
mod snippet_host_picker;
mod snippet_output;
mod snippet_param_form;
mod snippet_picker;
mod snippets_overview;
pub mod svg_export;
mod tag_picker;
pub mod theme;
mod theme_picker;
mod tunnel_form;
mod tunnel_host_picker;
mod tunnel_list;
mod tunnels_detail;
mod tunnels_format;
pub(crate) mod tunnels_overview;
mod whats_new;
#[cfg(test)]
mod whats_new_tests;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Screen, TopPage};

const MIN_WIDTH: u16 = 50;
const MIN_HEIGHT: u16 = 14;

/// App version string for display. Pinned in test builds, like `build_date`,
/// so a `Cargo.toml` version bump does not invalidate the help and What's New
/// goldens. Non-display version logic keeps reading `CARGO_PKG_VERSION`.
#[cfg(not(test))]
pub(crate) fn pkg_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
pub(crate) fn pkg_version() -> &'static str {
    "0.0.0-test"
}

/// Test app whose SSH config is `~/.ssh/purple_config` in a sandbox home,
/// so rendered text names a file other than the default.
#[cfg(test)]
pub(crate) fn app_with_own_config(content: &str) -> App {
    let env = std::sync::Arc::new(crate::runtime::env::Env::sandboxed());
    let home = env.paths().expect("sandbox paths").home().to_path_buf();
    let config = crate::ssh_config::model::SshConfigFile::from_content(
        content,
        home.join(".ssh").join("purple_config"),
    );
    App::with_env(config, env)
}

/// Draw into a test buffer and return every cell as one string.
#[cfg(test)]
pub(crate) fn render_to_text(width: u16, height: u16, draw: impl FnOnce(&mut Frame)) -> String {
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
    terminal.draw(draw).expect("render");
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect()
}

/// Top-level render dispatcher.
pub fn render(frame: &mut Frame, app: &mut App, anim: &mut crate::animation::AnimationState) {
    anim.tick_overlay_anim();
    let area = frame.area();

    // Terminal too small guard
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let msg = Paragraph::new(Line::from(vec![
            Span::styled(design::ICON_WARNING, theme::warning()),
            Span::raw(" Terminal too small. Need at least 50x14."),
        ]));
        frame.render_widget(msg, area);
        return;
    }

    // Render host list with animated detail panel width. When an overlay is active,
    // hide the status so it only appears in the overlay's own footer.
    // Note: host_list::render does not set app.status_center.status, so the unconditional restore
    // is safe. If that invariant ever changes, use get_or_insert semantics instead.
    let has_overlay = !matches!(app.screen, Screen::HostList) || app.jump.is_some();
    let status = if has_overlay {
        app.status_center.take_status()
    } else {
        None
    };
    let detail_progress = anim.detail_anim_progress();
    match app.top_page {
        TopPage::Hosts => host_list::render(frame, app, anim.spinner_tick, detail_progress),
        TopPage::Tunnels => tunnels_overview::render(frame, app, anim),
        TopPage::Containers => {
            containers_overview::render(frame, app, anim.spinner_tick, detail_progress)
        }
        TopPage::Snippets => snippets_overview::render(frame, app, detail_progress),
        TopPage::Keys => keys_overview::render(frame, app, anim.spinner_tick),
    }
    if let Some(s) = status {
        app.status_center.restore_status(Some(s));
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
        Screen::KeyPushPicker { key_index } => {
            let key_index = *key_index;
            render_overlay(frame, app, anim, move |frame, app| {
                key_push_picker::render(frame, app, key_index)
            });
        }
        Screen::ConfirmKeyPush { key_index } => {
            let key_index = *key_index;
            render_overlay(frame, app, anim, move |frame, app| {
                let aliases = app.keys.push().committed.clone();
                confirm_dialog::render_key_push(frame, app, key_index, &aliases)
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
        Screen::BulkTagEditor => {
            render_overlay(frame, app, anim, bulk_tag_editor::render);
        }
        Screen::ThemePicker => {
            render_overlay_nodim(frame, app, anim, theme_picker::render);
        }
        Screen::Providers => {
            render_overlay(frame, app, anim, |frame, app| {
                provider_list::render_provider_list(frame, app)
            });
        }
        Screen::ProviderForm { id } => {
            let provider = id.provider.clone();
            render_overlay(frame, app, anim, |frame, app| {
                provider_list::render_provider_form(frame, app, &provider)
            });
        }
        Screen::ProviderLabelMigration { provider } => {
            let provider = provider.clone();
            render_overlay(frame, app, anim, |frame, app| {
                provider_list::render_label_migration(frame, app, &provider)
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
                // When the form is reached from the Tunnels overview the
                // background is already the overview itself, so do not paint
                // a per-host TunnelList behind it.
                if !matches!(app.top_page, TopPage::Tunnels) {
                    tunnel_list::render(frame, app, &alias);
                }
                tunnel_form::render(frame, app);
            });
        }
        Screen::TunnelHostPicker => {
            render_overlay(frame, app, anim, tunnel_host_picker::render);
        }
        Screen::ContainerHostPicker => {
            render_overlay(frame, app, anim, container_host_picker::render);
        }
        Screen::ContainerLogs => {
            render_overlay(frame, app, anim, container_logs::render);
        }
        Screen::ConfirmContainerRestart { .. } => {
            render_overlay(frame, app, anim, container_action_confirm::render_restart);
        }
        Screen::ConfirmContainerStop { .. } => {
            render_overlay(frame, app, anim, container_action_confirm::render_stop);
        }
        Screen::ContainerExecPrompt { .. } => {
            render_overlay(frame, app, anim, container_exec_prompt::render);
        }
        Screen::ConfirmStackRestart => {
            render_overlay(frame, app, anim, container_action_confirm::render_stack);
        }
        Screen::ConfirmHostRestartAll => {
            render_overlay(
                frame,
                app,
                anim,
                container_action_confirm::render_host_restart_all,
            );
        }
        Screen::ConfirmHostStopAll => {
            render_overlay(
                frame,
                app,
                anim,
                container_action_confirm::render_host_stop_all,
            );
        }
        Screen::SnippetPicker => {
            render_overlay(frame, app, anim, snippet_picker::render);
        }
        Screen::SnippetForm => {
            render_overlay(frame, app, anim, |frame, app| {
                if app.snippets.form_return_to_tab() {
                    snippets_overview::render(frame, app, None);
                } else {
                    snippet_picker::render(frame, app);
                }
                snippet_form::render(frame, app);
            });
        }
        Screen::ConfirmHostKeyReset { hostname, .. } => {
            let hostname = hostname.clone();
            render_overlay(frame, app, anim, |frame, app| {
                confirm_dialog::render_host_key_reset(frame, app, &hostname)
            });
        }
        Screen::ConfirmHostKeyTrust {
            alias, hostname, ..
        } => {
            let alias = alias.clone();
            let hostname = hostname.clone();
            render_overlay(frame, app, anim, |frame, app| {
                confirm_dialog::render_host_key_trust(frame, app, &alias, &hostname)
            });
        }
        Screen::PasswordPrompt => {
            render_overlay(frame, app, anim, password_prompt::render);
        }
        Screen::FileBrowser { .. } => {
            render_overlay(frame, app, anim, file_browser::render);
        }
        Screen::SnippetOutput => {
            render_overlay(frame, app, anim, snippet_output::render);
        }
        Screen::SnippetParamForm => {
            render_overlay(frame, app, anim, |frame, app| {
                if app.snippets.form_return_to_tab() {
                    snippets_overview::render(frame, app, None);
                } else {
                    snippet_picker::render(frame, app);
                }
                snippet_param_form::render(frame, app);
            });
        }
        Screen::SnippetHostPicker => {
            render_overlay(frame, app, anim, snippet_host_picker::render);
        }
        Screen::ConfirmRunSnippet => {
            render_overlay(frame, app, anim, |frame, app| {
                let aliases = app.snippets.flow_targets().to_vec();
                confirm_dialog::render_run_snippet(frame, app, &aliases);
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
        Screen::ConfirmVaultSign => {
            let aliases: Vec<String> = app
                .vault
                .pending_sign()
                .map(|s| s.iter().map(|t| t.alias.clone()).collect())
                .unwrap_or_default();
            render_overlay(frame, app, anim, move |frame, app| {
                confirm_dialog::render_confirm_vault_sign(frame, app, &aliases)
            });
        }
        Screen::ConfirmPurgeStale => {
            let Some(payload) = app.providers.pending_purge() else {
                return;
            };
            let aliases = payload.aliases.clone();
            let provider = payload.provider.clone();
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
        Screen::WhatsNew(_) => {
            render_overlay(frame, app, anim, |frame, app| whats_new::render(frame, app));
        }
    }

    // Jump renders on top of any screen. Rendered directly (not via
    // render_overlay) to avoid polluting the overlay_close animation buffer,
    // which is reserved for Screen-driven overlays.
    if app.jump.is_some() {
        dim_background(frame);
        jump::render(frame, app);
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
    if let (Some(progress), Some(saved)) = (progress, pre_overlay)
        && progress < 1.0
    {
        apply_scale_clip(frame, &saved, progress);
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
            .fg(Color::Rgb(
                design::DIM_FG_RGB.0,
                design::DIM_FG_RGB.1,
                design::DIM_FG_RGB.2,
            ))
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

    if let Some(ref state) = anim.overlay_close
        && progress > 0.0
    {
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

/// Clip the frame buffer to a scaled region centered on screen (zoom effect).
/// Cells outside the clip are restored from `saved` (the pre-overlay host list).
fn apply_scale_clip(frame: &mut Frame, saved: &ratatui::buffer::Buffer, progress: f32) {
    let area = frame.area();
    let (left, right, top, bottom) = scale_clip_rect(area, progress);

    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            if (y < top || y >= bottom || x < left || x >= right)
                && let Some(cell) = saved.cell((x, y))
            {
                frame.buffer_mut()[(x, y)] = cell.clone();
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
#[deprecated(note = "use design::Footer builder instead")]
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
    let footer_status = app.status_center.status().filter(|s| !s.is_toast());
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
/// sticky messages set via notify_progress).
pub fn render_footer_with_status(
    frame: &mut Frame,
    area: Rect,
    footer_spans: Vec<Span<'_>>,
    app: &App,
) {
    if let Some(status) = app.status_center.status() {
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
    } else if matches!(status.class, crate::app::MessageClass::Error) {
        (
            design::ICON_ERROR,
            theme::error(),
            format!(" {} ", status.text),
        )
    } else if matches!(status.class, crate::app::MessageClass::Warning) {
        (
            design::ICON_WARNING,
            theme::warning(),
            format!(" {} ", status.text),
        )
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
/// Toast is a small bordered box (max 60% of terminal width, 3 rows tall)
/// with a thin "drain bar" along the bottom border that visualises the
/// remaining lifetime of the toast (full = just shown, empty = about to
/// expire). Sticky toasts (Errors, Progress) skip the drain bar.
fn render_toast(frame: &mut Frame, app: &App) {
    let toast = match app.status_center.toast() {
        Some(t) => t,
        None => return,
    };

    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        return;
    }

    let (icon, border_style) = match toast.class {
        crate::app::MessageClass::Error => (
            format!("{} ", design::ICON_ERROR),
            theme::toast_border_error(),
        ),
        crate::app::MessageClass::Warning => (
            format!("{} ", design::ICON_WARNING),
            theme::toast_border_warning(),
        ),
        crate::app::MessageClass::Success
        | crate::app::MessageClass::Info
        | crate::app::MessageClass::Progress => (
            format!("{} ", design::ICON_SUCCESS),
            theme::toast_border_success(),
        ),
    };

    let content = format!("{}{}", icon, toast.text);
    let content_width = content.width();
    // +4 for border (2) + padding (2). Cap at 60% of terminal width.
    let max_width = (area.width as usize * 60 / 100).max(30);
    let box_width =
        (content_width.saturating_add(4).min(max_width) as u16).min(area.width.saturating_sub(4));
    let box_height = 3u16;
    let x = area.width.saturating_sub(box_width + design::TOAST_INSET_X);
    // Position above the footer row (which is the last row)
    let y = area
        .height
        .saturating_sub(box_height + design::TOAST_INSET_Y);

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

    // Drain bar: thin horizontal bar across the bottom border that shrinks
    // smoothly from full to empty as the toast nears expiry. The bar uses
    // wall-clock time (Instant) so it animates at render frame-rate
    // (currently 50ms / 20fps). Skips sticky toasts (Errors, Progress)
    // where there is no expiry.
    if !toast.sticky && !matches!(toast.class, crate::app::MessageClass::Progress) {
        let total_ms = toast.timeout_ms();
        if total_ms != u64::MAX && total_ms > 0 {
            let elapsed_ms = toast.created_at.elapsed().as_millis() as u64;
            // remaining_ratio: 1.0 = just shown, 0.0 = about to expire.
            let remaining_ratio = if elapsed_ms >= total_ms {
                0.0
            } else {
                1.0 - (elapsed_ms as f64 / total_ms as f64)
            };
            let inner_w = box_width.saturating_sub(2);
            let bar_cols = (remaining_ratio * f64::from(inner_w)) as u16;
            if bar_cols > 0 {
                let bar_y = rect.y + rect.height.saturating_sub(1);
                let bar_x = rect.x + 1;
                let bar_rect = Rect::new(bar_x, bar_y, bar_cols.min(inner_w), 1);
                let bar = Paragraph::new(Line::from(Span::styled(
                    "\u{2501}".repeat(bar_rect.width as usize),
                    border_style,
                )));
                frame.render_widget(bar, bar_rect);
            }
        }
    }
}

/// Create a centered rect of given percentage within the parent rect.
pub(crate) fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
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
pub(crate) fn centered_rect_fixed(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}

/// Uniform width clamp for picker overlays (ProxyJump, Vault role,
/// Password source). Keeps all simple list pickers visually aligned at
/// the same minimum and maximum width regardless of terminal size.
/// Re-exported under `ui::` for the nearby `#[cfg(test)]` module; the
/// canonical values live in `design.rs`.
#[cfg(test)]
pub(crate) const PICKER_MIN_WIDTH: u16 = crate::ui::design::PICKER_MIN_W;
#[cfg(test)]
pub(crate) const PICKER_MAX_WIDTH: u16 = crate::ui::design::PICKER_MAX_W;

/// Width a picker overlay should use on this frame. Delegates to
/// `design::picker_width` so the picker-width formula lives in one place.
pub fn picker_overlay_width(frame: &Frame) -> u16 {
    design::picker_width(frame)
}

/// Minimum overlay height required to render rounded borders plus at
/// least one row of content. Below this the overlay is skipped so
/// ratatui does not collapse the borders into an unreadable glyph
/// soup on extremely short terminals.
pub const PICKER_MIN_HEIGHT: u16 = 3;

/// Compose a picker block title, gracefully dropping a hint that would
/// not fit inside the usable title width (overlay width minus the two
/// border columns). Protects against silent clipping of picker-specific
/// keybindings when the overlay is constrained by a narrow terminal.
fn picker_title_text(title: &str, title_hint: Option<&str>, width: u16) -> String {
    let inner = (width as usize).saturating_sub(2);
    match title_hint {
        Some(hint) => {
            let full = format!(" {} · {} ", title, hint);
            if full.chars().count() <= inner {
                full
            } else {
                format!(" {} ", title)
            }
        }
        None => format!(" {} ", title),
    }
}

/// Render a list-style picker overlay with the canonical purple look:
/// fixed width range (`design::PICKER_MIN_W..=PICKER_MAX_W`), height grows
/// with item count up to `design::PICKER_MAX_H`, rounded border, muted
/// accent, highlight on the selected row and a two-space highlight gutter.
///
/// The `title_hint`, if present and space permits, is appended to the block
/// title separated by a middle dot so picker-specific keybindings (e.g.
/// Ctrl+D for Password Source) can be surfaced without adding a divergent
/// footer. If the full hinted title would overflow, the hint is dropped
/// rather than silently clipped.
///
/// All pickers share this single helper so they look identical regardless
/// of which form field opened them. The previous `_wide` variant has been
/// removed; pickers that need more horizontal room (Key Picker's 3-column
/// layout) truncate their secondary metadata instead of widening.
pub fn render_picker_overlay<'a>(
    frame: &mut Frame,
    title: &str,
    title_hint: Option<&str>,
    items: Vec<ratatui::widgets::ListItem<'a>>,
    list_state: &mut ratatui::widgets::ListState,
) {
    use ratatui::widgets::{Block, BorderType, Clear, List};

    let width = picker_overlay_width(frame);
    let content_rows = items.len() as u16;
    let height = (content_rows + 2).min(design::PICKER_MAX_H);
    if height < PICKER_MIN_HEIGHT {
        return;
    }
    let area = centered_rect_fixed(width, height, frame.area());
    if area.height < PICKER_MIN_HEIGHT {
        return;
    }
    frame.render_widget(Clear, area);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(
            picker_title_text(title, title_hint, width),
            theme::brand(),
        ))
        .border_style(theme::border_dim());

    let list = List::new(items)
        .block(block)
        .highlight_style(theme::selected_row())
        .highlight_symbol(design::LIST_HIGHLIGHT);

    frame.render_stateful_widget(list, area, list_state);
}

/// Render an empty-state picker overlay with a muted message in place of
/// a list. Used when a picker is opened with no candidates (e.g. no
/// other hosts to use as ProxyJump).
pub fn render_picker_empty_overlay(frame: &mut Frame, title: &str, message: &str) {
    use ratatui::widgets::{Block, BorderType, Clear};

    let width = picker_overlay_width(frame);
    let area = centered_rect_fixed(width, 5, frame.area());
    if area.height < PICKER_MIN_HEIGHT {
        return;
    }
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(
            picker_title_text(title, None, width),
            theme::brand(),
        ))
        .border_style(theme::border_dim());
    let msg = Paragraph::new(Line::from(Span::styled(
        format!("  {}", message),
        theme::muted(),
    )))
    .block(block);
    frame.render_widget(msg, area);
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;

    use super::*;

    fn make_app() -> App {
        let config = crate::ssh_config::model::SshConfigFile {
            elements: crate::ssh_config::model::SshConfigFile::parse_content(""),
            path: tempfile::tempdir()
                .expect("tempdir")
                .keep()
                .join("test_config"),
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
        app.notify_info("test");
        let mut anim = crate::animation::AnimationState::new();
        terminal
            .draw(|frame| {
                render_overlay_inner(frame, &mut app, &mut anim, true, |_frame, app| {
                    assert!(
                        app.status_center.status().is_some(),
                        "status should be visible during overlay render"
                    );
                });
            })
            .unwrap();
        assert!(
            app.status_center.status().is_some(),
            "status should still be present after overlay render"
        );
    }

    #[test]
    fn overlay_footer_renders_status_text_in_buffer() {
        let backend = TestBackend::new(80, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = make_app();
        app.notify_info("sync failed");
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
        app.notify_info("sync failed");
        // Simulate an overlay being active.
        app.screen = crate::app::Screen::Help {
            return_screen: Box::new(crate::app::Screen::HostList),
        };
        let has_overlay = !matches!(app.screen, crate::app::Screen::HostList);
        assert!(has_overlay, "should detect overlay");
        // Mimic render(): take status during host list render, then restore.
        let status = app.status_center.take_status();
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
            app.status_center.restore_status(Some(s));
        }
        assert!(
            app.status_center.status().is_some(),
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
        app.notify("Copied web01"); // Goes to toast (Confirmation)
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
        assert!(app.status_center.toast().is_none());
        terminal
            .draw(|frame| {
                render_toast(frame, &app);
            })
            .unwrap();
        // Should not panic, just no-op
    }

    #[test]
    fn render_toast_shows_error_with_error_icon() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = make_app();
        app.notify_error("Connection failed"); // Routes to Error toast
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
            // Errors use the heavy multiplication X glyph (ICON_ERROR),
            // distinct from the warning sign used by Warning-class toasts.
            if line.contains(design::ICON_ERROR) {
                found_icon = true;
            }
        }
        assert!(found_text, "error text should appear in buffer");
        assert!(found_icon, "error should show error icon");
    }

    #[test]
    fn render_toast_shows_warning_with_alert_icon() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = make_app();
        app.notify_warning("Stale host configuration");
        terminal.draw(|frame| render_toast(frame, &app)).unwrap();
        let buf = terminal.backend().buffer();
        let mut found_text = false;
        let mut found_icon = false;
        for y in 0..24 {
            let mut line = String::new();
            for x in 0..80 {
                line.push_str(buf[(x, y)].symbol());
            }
            if line.contains("Stale host configuration") {
                found_text = true;
            }
            // Warnings keep the warning sign glyph; errors use a different glyph.
            if line.contains(design::ICON_WARNING) {
                found_icon = true;
            }
        }
        assert!(found_text, "warning text should appear in buffer");
        assert!(
            found_icon,
            "warning should show warning sign (ICON_WARNING)"
        );
    }

    #[test]
    fn render_toast_drain_bar_shrinks_over_time() {
        // Non-sticky toast (Success) → drain bar should appear at the
        // bottom border row and shrink smoothly as wall-clock time
        // advances toward the timeout. At created_at = now the bar fills
        // the inner width; at elapsed >= timeout_ms the bar is gone.
        use std::time::{Duration, Instant};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = make_app();
        app.notify("Saved profile changes successfully");
        let timeout_ms = app.status_center.toast().unwrap().timeout_ms();

        // Helper: count `\u{2501}` cells in the rendered buffer.
        let count_drain_bar = |app: &App, terminal: &mut Terminal<TestBackend>| -> usize {
            terminal.draw(|frame| render_toast(frame, app)).unwrap();
            let buf = terminal.backend().buffer();
            let mut count = 0;
            for y in 0..24 {
                for x in 0..80 {
                    if buf[(x, y)].symbol() == "\u{2501}" {
                        count += 1;
                    }
                }
            }
            count
        };

        // Just created → full bar.
        let bar_full = count_drain_bar(&app, &mut terminal);
        assert!(
            bar_full > 0,
            "non-sticky Success toast must render a drain bar when just created"
        );

        // Simulate halfway elapsed by backdating created_at.
        if let Some(toast) = app.status_center.toast_mut() {
            toast.created_at = Instant::now() - Duration::from_millis(timeout_ms / 2);
        }
        let bar_half = count_drain_bar(&app, &mut terminal);
        assert!(
            bar_half < bar_full,
            "drain bar must shrink as time passes ({} >= {})",
            bar_half,
            bar_full
        );

        // Simulate past expiry.
        if let Some(toast) = app.status_center.toast_mut() {
            toast.created_at = Instant::now() - Duration::from_millis(timeout_ms + 1000);
        }
        let bar_empty = count_drain_bar(&app, &mut terminal);
        assert_eq!(
            bar_empty, 0,
            "drain bar must be empty once elapsed time exceeds timeout"
        );
    }

    #[test]
    fn render_toast_drain_bar_absent_for_sticky_error() {
        // Sticky toasts (Errors, Progress) carry no expiry, so no drain bar.
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = make_app();
        app.notify_error("Permission denied");
        terminal.draw(|frame| render_toast(frame, &app)).unwrap();
        let buf = terminal.backend().buffer();
        let mut count = 0;
        for y in 0..24 {
            for x in 0..80 {
                if buf[(x, y)].symbol() == "\u{2501}" {
                    count += 1;
                }
            }
        }
        assert_eq!(
            count, 0,
            "sticky error toast must NOT render a drain bar (nothing to drain)"
        );
    }

    #[test]
    fn footer_shows_hints_when_toast_active() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = make_app();
        app.notify("Copied"); // Goes to toast, NOT footer
        assert!(app.status_center.toast().is_some());
        assert!(app.status_center.status().is_none()); // Footer should be clear
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
        app.notify_info("Syncing AWS...");
        assert!(app.status_center.status().is_some());
        assert!(app.status_center.toast().is_none());
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

    /// Picker overlay width should clamp narrow terminals to
    /// `PICKER_MIN_WIDTH` so the layout never collapses below the
    /// minimum that the item renderers assume.
    #[test]
    fn picker_overlay_width_clamps_narrow_terminal() {
        let backend = TestBackend::new(30, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                assert_eq!(picker_overlay_width(frame), PICKER_MIN_WIDTH);
            })
            .unwrap();
    }

    /// Picker overlay width should cap wide terminals at
    /// `PICKER_MAX_WIDTH` so the overlay stays centered and compact
    /// instead of stretching across the full width of a large terminal.
    #[test]
    fn picker_overlay_width_caps_wide_terminal() {
        let backend = TestBackend::new(200, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                assert_eq!(picker_overlay_width(frame), PICKER_MAX_WIDTH);
            })
            .unwrap();
    }

    /// Terminals between `PICKER_MIN_WIDTH` and `PICKER_MAX_WIDTH`
    /// should use the terminal's actual width so the overlay fills
    /// available space without exceeding the cap.
    #[test]
    fn picker_overlay_width_passes_through_midrange() {
        // PICKER_MIN_W (60) < 66 < PICKER_MAX_W (72), so passes through unclamped.
        let backend = TestBackend::new(66, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                assert_eq!(picker_overlay_width(frame), 66);
            })
            .unwrap();
    }

    /// Concatenate every row of a terminal buffer into a single string
    /// so tests can grep for rendered content without pinning the exact
    /// centering offset of an overlay.
    fn buffer_dump(buf: &ratatui::buffer::Buffer) -> String {
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    /// `render_picker_overlay` must surface picker-specific keybindings
    /// via the block title rather than a divergent footer. A title hint
    /// should appear as `" Title · hint "` in the rendered buffer so
    /// all pickers share the same outer shape.
    #[test]
    fn render_picker_overlay_writes_title_hint_to_border() {
        use ratatui::widgets::{ListItem, ListState};
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let mut state = ListState::default();
                let items = vec![ListItem::new("one"), ListItem::new("two")];
                render_picker_overlay(
                    frame,
                    "Password Source",
                    Some("Ctrl+D: global default"),
                    items,
                    &mut state,
                );
                let dump = buffer_dump(frame.buffer_mut());
                assert!(
                    dump.contains("Password Source · Ctrl+D: global default"),
                    "rendered buffer must contain the hinted title, got:\n{dump}"
                );
            })
            .unwrap();
    }

    /// A picker without a hint should render the title as-is, with no
    /// middle-dot separator. Prevents a regression where a bare `None`
    /// accidentally introduces stray punctuation into the title.
    #[test]
    fn render_picker_overlay_plain_title_has_no_dot_separator() {
        use ratatui::widgets::{ListItem, ListState};
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let mut state = ListState::default();
                let items = vec![ListItem::new("one")];
                render_picker_overlay(frame, "ProxyJump", None, items, &mut state);
                let dump = buffer_dump(frame.buffer_mut());
                assert!(dump.contains("ProxyJump"));
                assert!(
                    !dump.contains('·'),
                    "plain title must not emit a middle-dot separator, got:\n{dump}"
                );
            })
            .unwrap();
    }

    /// `render_picker_overlay` must cap the rendered height at
    /// `design::PICKER_MAX_H` even when the item count would demand more.
    /// The overlay is pinned at exactly that height so a long list
    /// scrolls inside the overlay instead of growing off-screen.
    #[test]
    fn render_picker_overlay_caps_height_at_design_max() {
        use ratatui::widgets::{ListItem, ListState};
        let backend = TestBackend::new(80, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let mut state = ListState::default();
                let items: Vec<ListItem> = (0..40)
                    .map(|i| ListItem::new(format!("item {}", i)))
                    .collect();
                render_picker_overlay(frame, "Many", None, items, &mut state);
                let dump = buffer_dump(frame.buffer_mut());
                // Count rows that contain any overlay glyph (border or
                // title or list content) to assert the overlay itself
                // is exactly `PICKER_MAX_H` rows tall.
                let rows_with_overlay = dump
                    .lines()
                    .filter(|line| line.contains('╭') || line.contains('╰') || line.contains('│'))
                    .count();
                assert_eq!(
                    rows_with_overlay,
                    design::PICKER_MAX_H as usize,
                    "overlay must be capped at design::PICKER_MAX_H, got:\n{dump}"
                );
            })
            .unwrap();
    }

    /// When the hinted title would overflow the overlay's inner width,
    /// `render_picker_overlay` must drop the hint instead of silently
    /// clipping it — the affordance is either fully visible or
    /// gracefully suppressed.
    #[test]
    fn render_picker_overlay_drops_hint_when_it_would_overflow() {
        use ratatui::widgets::{ListItem, ListState};
        // Narrow terminal → clamped to PICKER_MIN_WIDTH (50).
        let backend = TestBackend::new(40, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let mut state = ListState::default();
                let items = vec![ListItem::new("only")];
                // A hint that together with the title clearly exceeds
                // the 48-col inner title bar at width 50.
                render_picker_overlay(
                    frame,
                    "Password Source",
                    Some("this is an excessively long keybinding description that will not fit"),
                    items,
                    &mut state,
                );
                let dump = buffer_dump(frame.buffer_mut());
                assert!(
                    dump.contains("Password Source"),
                    "title must still render, got:\n{dump}"
                );
                assert!(
                    !dump.contains('·'),
                    "overflow hint must be dropped, not clipped, got:\n{dump}"
                );
            })
            .unwrap();
    }

    /// Unit test for the pure title composer: ensures graceful hint
    /// drop without rendering side effects. Pins the behaviour that
    /// `render_picker_overlay` depends on.
    #[test]
    fn picker_title_text_drops_overflow_hint() {
        let plain = picker_title_text("Title", None, 50);
        assert_eq!(plain, " Title ");
        let fits = picker_title_text("Title", Some("short"), 50);
        assert_eq!(fits, " Title · short ");
        let overflows = picker_title_text("Title", Some(&"x".repeat(200)), 50);
        assert_eq!(
            overflows, " Title ",
            "overlong hint must be dropped entirely"
        );
    }

    /// On a terminal too short to host the rounded borders and a row
    /// of content, `render_picker_overlay` must bail out rather than
    /// emit a degenerate box that ratatui would render as unreadable
    /// glyphs.
    #[test]
    fn render_picker_overlay_skips_terminal_shorter_than_minimum() {
        use ratatui::widgets::{ListItem, ListState};
        let backend = TestBackend::new(80, 2);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let mut state = ListState::default();
                let items = vec![ListItem::new("entry")];
                render_picker_overlay(frame, "Tiny", None, items, &mut state);
                let dump = buffer_dump(frame.buffer_mut());
                assert!(
                    !dump.contains("Tiny"),
                    "overlay must not render on a 2-row terminal, got:\n{dump}"
                );
            })
            .unwrap();
    }

    /// Empty-state overlay should reuse the uniform picker width and
    /// surface both the title and the body message so it is visually
    /// consistent with the populated variant that replaces it the
    /// moment a candidate becomes available.
    #[test]
    fn render_picker_empty_overlay_renders_title_and_message() {
        let backend = TestBackend::new(200, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_picker_empty_overlay(frame, "ProxyJump", "No other hosts configured");
                let dump = buffer_dump(frame.buffer_mut());
                assert!(dump.contains("ProxyJump"), "title missing, got:\n{dump}");
                assert!(
                    dump.contains("No other hosts configured"),
                    "empty-state message missing, got:\n{dump}"
                );
            })
            .unwrap();
    }
}
