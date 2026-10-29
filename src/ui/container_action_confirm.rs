//! Confirm dialogs for the per-container destructive actions
//! (`Screen::ConfirmContainerRestart`, `Screen::ConfirmContainerStop`).
//! Both dialogs share a layout and only differ in the title block,
//! the body wording and the footer verb. One module, two thin
//! entry points.

use ratatui::Frame;
use ratatui::text::{Line, Span};

use super::design;
use super::theme;
use crate::app::{App, Screen};

/// Render the restart confirm dialog. Pulls `name`, `alias`,
/// `project`, and `uptime` from `Screen::ConfirmContainerRestart`.
/// No-op when the screen variant differs (defensive. main loop
/// guards against this but the renderer should be predictable).
pub fn render_restart(frame: &mut Frame, app: &mut App) {
    let Screen::ConfirmContainerRestart {
        alias,
        container_name,
        project,
        uptime,
        ..
    } = &app.screen
    else {
        return;
    };
    render_dialog(
        frame,
        app,
        crate::i18n::text(" Restart container? "),
        container_name,
        alias,
        project.as_deref(),
        uptime.as_deref(),
        crate::i18n::text(crate::messages::CONTAINER_RESTART_BODY),
        ("restart", "keep"),
    );
}

/// Render the stack-restart confirm dialog. Lists every running
/// member that will be cycled. Sized larger than the per-container
/// confirms so the member list fits without scrolling.
pub fn render_stack(frame: &mut Frame, app: &mut App) {
    if !matches!(app.screen, Screen::ConfirmStackRestart) {
        return;
    }
    let app_ref: &App = app;
    let Some(payload) = app_ref.containers_overview.pending_bulk_confirm() else {
        return;
    };
    let identity = Line::from(vec![
        Span::raw("  "),
        Span::styled(payload.project.clone().unwrap_or_default(), theme::bold()),
        Span::raw("  "),
        Span::styled(format!("on  {}", payload.alias), theme::muted()),
        Span::raw("  "),
        Span::styled(
            format!("· {} running", payload.members.len()),
            theme::muted(),
        ),
    ]);
    render_bulk_dialog(
        frame,
        app_ref,
        crate::i18n::text(" Restart stack? "),
        identity,
        &payload.members,
        crate::i18n::text(crate::messages::CONTAINER_STACK_RESTART_BODY),
        ("restart", "keep"),
    );
}

/// Render the bulk-restart-host confirm dialog. Lists every running
/// container on the host that will be cycled. Same layout as
/// `render_stack` so users on the divider see a familiar sheet.
pub fn render_host_restart_all(frame: &mut Frame, app: &mut App) {
    if !matches!(app.screen, Screen::ConfirmHostRestartAll) {
        return;
    }
    let app_ref: &App = app;
    let Some(payload) = app_ref.containers_overview.pending_bulk_confirm() else {
        return;
    };
    let identity = Line::from(vec![
        Span::raw("  "),
        Span::styled(payload.alias.clone(), theme::bold()),
        Span::raw("  "),
        Span::styled(
            format!("· {} running", payload.members.len()),
            theme::muted(),
        ),
    ]);
    render_bulk_dialog(
        frame,
        app_ref,
        crate::i18n::text(" Restart all containers on host? "),
        identity,
        &payload.members,
        crate::i18n::text(crate::messages::CONTAINER_HOST_RESTART_ALL_BODY),
        ("restart", "keep"),
    );
}

/// Render the bulk-stop-host confirm dialog.
pub fn render_host_stop_all(frame: &mut Frame, app: &mut App) {
    if !matches!(app.screen, Screen::ConfirmHostStopAll) {
        return;
    }
    let app_ref: &App = app;
    let Some(payload) = app_ref.containers_overview.pending_bulk_confirm() else {
        return;
    };
    let identity = Line::from(vec![
        Span::raw("  "),
        Span::styled(payload.alias.clone(), theme::bold()),
        Span::raw("  "),
        Span::styled(
            format!("· {} running", payload.members.len()),
            theme::muted(),
        ),
    ]);
    render_bulk_dialog(
        frame,
        app_ref,
        crate::i18n::text(" Stop all containers on host? "),
        identity,
        &payload.members,
        crate::i18n::text(crate::messages::CONTAINER_HOST_STOP_ALL_BODY),
        ("stop", "keep"),
    );
}

/// Shared layout for every bulk-action confirm dialog (stack-restart,
/// host-restart-all, host-stop-all). Caller supplies the title, the
/// identity line that introduces the scope (project-on-host vs host
/// alone), the member list to cycle, the body line and the footer
/// verbs. Body lines wrap so longer wording does not get clipped at
/// the dialog edge; the height budget reserves two extra rows for the
/// body so a one- or two-line wrap stays inside the box.
fn render_bulk_dialog(
    frame: &mut Frame,
    app: &App,
    title: &str,
    identity: Line<'static>,
    members: &[crate::app::StackMember],
    body: &str,
    verbs: (&str, &str),
) {
    let mut content: Vec<Line<'static>> = vec![identity, Line::from("")];
    for m in members {
        let uptime = m.uptime.clone().unwrap_or_else(|| "-".to_string());
        content.push(Line::from(vec![
            Span::raw("   "),
            Span::raw(design::ICON_ONLINE),
            Span::raw(" "),
            Span::styled(m.container_name.clone(), theme::bold()),
            Span::raw("   "),
            Span::styled(uptime, theme::muted()),
        ]));
    }
    content.push(Line::from(""));
    content.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(body.to_string(), theme::muted()),
    ]));

    let footer_spans = design::confirm_footer_destructive(verbs.0, verbs.1)
        .to_line()
        .spans;
    design::render_confirm_popup(
        frame,
        64,
        design::PopupKind::Destructive,
        title,
        content,
        footer_spans,
        app,
    );
}

/// Render the stop confirm dialog. Same shape as `render_restart`.
pub fn render_stop(frame: &mut Frame, app: &mut App) {
    let Screen::ConfirmContainerStop {
        alias,
        container_name,
        project,
        uptime,
        ..
    } = &app.screen
    else {
        return;
    };
    render_dialog(
        frame,
        app,
        crate::i18n::text(" Stop container? "),
        container_name,
        alias,
        project.as_deref(),
        uptime.as_deref(),
        crate::i18n::text(crate::messages::CONTAINER_STOP_BODY),
        ("stop", "keep"),
    );
}

#[allow(clippy::too_many_arguments)]
fn render_dialog(
    frame: &mut Frame,
    app: &App,
    title: &str,
    name: &str,
    alias: &str,
    project: Option<&str>,
    uptime: Option<&str>,
    body: &str,
    verbs: (&str, &str),
) {
    let identity_line = Line::from(vec![
        Span::raw("  "),
        Span::styled(name.to_string(), theme::bold()),
        Span::raw("  "),
        Span::styled(format!("on  {}", alias), theme::muted()),
    ]);

    let mut meta_parts: Vec<String> = Vec::new();
    if let Some(p) = project {
        meta_parts.push(p.to_string());
    }
    if let Some(u) = uptime {
        meta_parts.push(format!("Up {}", u));
    }
    let meta_line = if meta_parts.is_empty() {
        Line::from("")
    } else {
        Line::from(vec![
            Span::raw("  "),
            Span::styled(meta_parts.join("  ·  "), theme::muted()),
        ])
    };

    let body_line = Line::from(vec![
        Span::raw("  "),
        Span::styled(body.to_string(), theme::muted()),
    ]);

    let content: Vec<Line<'static>> = vec![identity_line, meta_line, Line::from(""), body_line];

    // Stakes test: destructive action, action verbs both sides.
    let footer_spans = design::confirm_footer_destructive(verbs.0, verbs.1)
        .to_line()
        .spans;
    design::render_confirm_popup(
        frame,
        60,
        design::PopupKind::Destructive,
        title,
        content,
        footer_spans,
        app,
    );
}
