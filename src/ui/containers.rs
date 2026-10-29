use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, Paragraph};

use super::design;
use super::theme;
use crate::app::App;
use crate::containers::truncate_str;

pub fn render(frame: &mut Frame, app: &mut App) {
    let state = match app.container_session.as_mut() {
        Some(s) => s,
        None => return,
    };

    let alias = state.alias.clone();

    // Overlay sizing: percentage-based width, height fits content
    let item_count = state.containers.len().max(1);
    let has_header = true; // Always show column headers for visual consistency
    let header_row = if has_header { 1u16 } else { 0 };
    let action_row = if state.action_in_progress.is_some() {
        1u16
    } else {
        0
    };
    // Reserve 1 row below the block for the external footer.
    let height = (item_count as u16 + 4 + header_row + action_row)
        .min(frame.area().height.saturating_sub(5));
    let area = design::overlay_area(frame, design::OVERLAY_W, design::OVERLAY_H, height);
    frame.render_widget(Clear, area);

    let mut block = design::overlay_block(&format!("Containers for {}", alias));
    if let Some(ref rt) = state.runtime {
        block = block.title_top(Line::from(Span::styled(
            format!(" [{}] ", rt.as_str()),
            theme::muted(),
        )));
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Layout inside the block: optional header + list + optional action.
    // Footer renders BELOW the block via design::form_footer.
    let mut constraints = Vec::new();
    if has_header {
        constraints.push(Constraint::Length(1)); // column header
    }
    constraints.push(Constraint::Min(0)); // list
    if state.action_in_progress.is_some() {
        constraints.push(Constraint::Length(1)); // action in progress
    }
    let chunks = Layout::vertical(constraints).split(inner);

    // Resolve chunk indices
    let header_ci = if has_header { Some(0) } else { None };
    let list_ci = has_header as usize;
    let action_ci = if state.action_in_progress.is_some() {
        Some(list_ci + 1)
    } else {
        None
    };

    let list_area = chunks[list_ci];

    // Column layout following host_list pattern:
    // Left cluster: NAME + gap + IMAGE (IMAGE is flex like HOST in host_list)
    // Flex gap (absorbs surplus, pushes right cluster to the right)
    // Right cluster: STATE + gap + STATUS
    let usable = list_area.width.saturating_sub(2) as usize; // 1 highlight + 1 right margin
    let gap: usize = design::COL_GAP as usize;

    // Measure and pad each column
    let name_w = design::padded_usize(
        state
            .containers
            .iter()
            .map(|c| c.names.len())
            .max()
            .unwrap_or(4)
            .max(4),
    );
    let image_w = design::padded_usize(
        state
            .containers
            .iter()
            .map(|c| c.image.len())
            .max()
            .unwrap_or(5)
            .max(5),
    );
    let state_w = design::padded_usize(
        state
            .containers
            .iter()
            .map(|c| c.state.len())
            .max()
            .unwrap_or(5)
            .max(5),
    );
    let status_w = design::padded_usize(
        state
            .containers
            .iter()
            .map(|c| c.status.len())
            .max()
            .unwrap_or(6)
            .max(6),
    );

    // Left cluster: NAME + gap + IMAGE
    let left = name_w + gap + image_w;
    // Right cluster: STATE + gap + STATUS
    let right = state_w + gap + status_w;
    // Flex gap between left and right (like host_list flex_gap)
    let flex_gap = usable.saturating_sub(left + gap + right).max(gap);

    // Column header
    let gap_str = design::COL_GAP_STR;
    let flex_str = " ".repeat(flex_gap);
    if let Some(hi) = header_ci {
        let style = theme::bold();
        let hdr = Line::from(vec![
            Span::styled(
                format!("{}{:<name_w$}", design::COLUMN_HEADER_PREFIX, "NAME"),
                style,
            ),
            Span::raw(gap_str),
            Span::styled(format!("{:<image_w$}", "IMAGE"), style),
            Span::raw(flex_str.clone()),
            Span::styled(format!("{:<state_w$}", "STATE"), style),
            Span::raw(gap_str),
            Span::styled(format!("{:<status_w$}", "STATUS"), style),
        ]);
        frame.render_widget(Paragraph::new(hdr), chunks[hi]);
    }

    // Content
    if state.loading && state.containers.is_empty() {
        design::render_loading(frame, list_area, "Loading containers...");
    } else if let Some(ref err) = state.error {
        design::render_error(frame, list_area, err);
    } else if state.containers.is_empty() {
        design::render_empty(
            frame,
            list_area,
            "No containers found. Is Docker or Podman installed?",
        );
    } else {
        let items: Vec<ListItem> = state
            .containers
            .iter()
            .map(|c| {
                let name_str = truncate_str(&c.names, name_w);
                let image_str = truncate_str(&c.image, image_w);
                // Shared {state -> style} mapping so this overlay agrees
                // with the per-host detail panel and the containers
                // overview about what dead, paused and exited look like.
                let (_, state_style) =
                    design::container_state_style(&c.state, None, &c.status, None, 0);
                let line = Line::from(vec![
                    Span::styled(format!(" {:<name_w$}", name_str), theme::bold()),
                    Span::raw(gap_str),
                    Span::styled(format!("{:<image_w$}", image_str), theme::muted()),
                    Span::raw(flex_str.clone()),
                    Span::styled(format!("{:<state_w$}", c.state), state_style),
                    Span::raw(gap_str),
                    Span::styled(
                        format!("{:<status_w$}", truncate_str(&c.status, status_w)),
                        theme::muted(),
                    ),
                ]);
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items)
            .highlight_style(theme::selected_row())
            .highlight_symbol(design::LIST_HIGHLIGHT);

        frame.render_stateful_widget(list, list_area, &mut state.list_state);
    }

    // Action in progress
    if let Some(ci) = action_ci
        && let Some(ref msg) = state.action_in_progress
    {
        design::render_loading(frame, chunks[ci], msg);
    }

    // Footer below the block
    let footer_area = design::render_overlay_footer(frame, area);
    use crate::messages::footer as fl;
    design::Footer::new()
        .action("s", crate::i18n::text(fl::ACTION_START))
        .action("x", crate::i18n::text(fl::ACTION_STOP))
        .action("r", crate::i18n::text(fl::ACTION_RESTART))
        .action("R", crate::i18n::text(fl::ACTION_REFRESH))
        .action("Esc", crate::i18n::text(fl::ESC_BACK))
        .render_with_status(frame, footer_area, app);

    // Confirmation dialog for stop/restart
    if let Some(ref confirm_state) = app.container_session
        && let Some((ref action, ref name, _)) = confirm_state.confirm_action
    {
        let verb = action.as_str();
        let display_name = truncate_str(name, 30);
        let content: Vec<Line<'static>> = vec![Line::from(Span::styled(
            format!("  {} \"{}\"?", verb, display_name),
            theme::bold(),
        ))];
        // Stakes test: stop/restart take effect on the remote
        // immediately, so use destructive action verbs (stop/restart
        // vs keep) instead of generic yes/no.
        let footer_spans = design::confirm_footer_destructive(verb, "keep")
            .to_line()
            .spans;
        design::render_confirm_popup(
            frame,
            52,
            design::PopupKind::Destructive,
            &format!("Confirm {}", verb),
            content,
            footer_spans,
            app,
        );
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;

    use super::design;
    use crate::app::{App, ContainerSession};
    use crate::ssh_config::model::SshConfigFile;

    fn make_app() -> App {
        let config = SshConfigFile {
            elements: SshConfigFile::parse_content(""),
            path: tempfile::tempdir()
                .expect("tempdir")
                .keep()
                .join("test_containers_config"),
            crlf: false,
            bom: false,
        };
        App::new(config)
    }

    #[test]
    fn render_noops_when_container_state_is_none() {
        let mut app = make_app();
        assert!(app.container_session.is_none());
        render_app(&mut app);
    }

    fn state_with(
        loading: bool,
        error: Option<String>,
        action_in_progress: Option<String>,
    ) -> ContainerSession {
        ContainerSession {
            alias: "test-host".to_string(),
            askpass: None,
            runtime: None,
            containers: Vec::new(),
            list_state: ratatui::widgets::ListState::default(),
            loading,
            error,
            action_in_progress,
            confirm_action: None,
        }
    }

    fn render_app(app: &mut App) {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| super::render(frame, app)).unwrap();
    }

    #[test]
    fn render_survives_empty_container_state() {
        let mut app = make_app();
        app.container_session = Some(state_with(false, None, None));
        render_app(&mut app);
    }

    #[test]
    fn render_survives_loading_state() {
        let mut app = make_app();
        app.container_session = Some(state_with(true, None, None));
        render_app(&mut app);
    }

    #[test]
    fn render_survives_error_state() {
        let mut app = make_app();
        app.container_session = Some(state_with(
            false,
            Some("docker not running".to_string()),
            None,
        ));
        render_app(&mut app);
    }

    #[test]
    fn render_survives_action_in_progress_state() {
        let mut app = make_app();
        app.container_session = Some(state_with(false, None, Some("stopping nginx".to_string())));
        render_app(&mut app);
    }

    #[test]
    fn footer_sits_directly_below_block() {
        let area = Rect::new(0, 0, 60, 20);
        let footer = design::form_footer(area, area.height);
        assert_eq!(footer.height, 1);
        assert_eq!(footer.y, area.y + area.height);
    }
}
