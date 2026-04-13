use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, Paragraph};

use super::theme;
use crate::app::App;
use crate::containers::truncate_str;

pub fn render(frame: &mut Frame, app: &mut App) {
    let state = match app.container_state.as_mut() {
        Some(s) => s,
        None => return,
    };

    let alias = state.alias.clone();

    // Title
    let mut title_spans = vec![Span::styled(
        format!(" Containers for {} ", alias),
        theme::brand(),
    )];
    if let Some(ref rt) = state.runtime {
        title_spans.push(Span::styled(format!(" [{}] ", rt.as_str()), theme::muted()));
    }
    let title = Line::from(title_spans);

    // Overlay sizing: percentage-based width, height fits content
    let item_count = state.containers.len().max(1);
    let has_header = true; // Always show column headers for visual consistency
    let header_row = if has_header { 1u16 } else { 0 };
    let action_row = if state.action_in_progress.is_some() {
        1u16
    } else {
        0
    };
    let height = (item_count as u16 + 6 + header_row + action_row)
        .min(frame.area().height.saturating_sub(4));
    let area = {
        let r = super::centered_rect(70, 80, frame.area());
        super::centered_rect_fixed(r.width, height, frame.area())
    };
    frame.render_widget(Clear, area);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title)
        .border_style(theme::accent());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Layout: optional header + list + optional action + spacer + footer
    let mut constraints = Vec::new();
    if has_header {
        constraints.push(Constraint::Length(1)); // column header
    }
    constraints.push(Constraint::Min(0)); // list
    if state.action_in_progress.is_some() {
        constraints.push(Constraint::Length(1)); // action in progress
    }
    constraints.push(Constraint::Length(1)); // spacer
    constraints.push(Constraint::Length(1)); // footer
    let chunks = Layout::vertical(constraints).split(inner);

    // Resolve chunk indices
    let header_ci = if has_header { Some(0) } else { None };
    let list_ci = has_header as usize;
    let mut next = list_ci + 1;
    let action_ci = if state.action_in_progress.is_some() {
        let ci = next;
        next += 1;
        Some(ci)
    } else {
        None
    };
    let _spacer_ci = next;
    let footer_ci = next + 1;

    let list_area = chunks[list_ci];

    // Column layout following host_list pattern:
    // Left cluster: NAME + gap + IMAGE (IMAGE is flex like HOST in host_list)
    // Flex gap (absorbs surplus, pushes right cluster to the right)
    // Right cluster: STATE + gap + STATUS
    let usable = list_area.width.saturating_sub(2) as usize; // 1 highlight + 1 right margin
    let gap: usize = 2;

    // ~110% of content width (same formula as host_list::Columns::padded)
    let padded = |w: usize| -> usize { if w == 0 { 0 } else { w + w / 10 + 1 } };

    // Measure and pad each column
    let name_w = padded(
        state
            .containers
            .iter()
            .map(|c| c.names.len())
            .max()
            .unwrap_or(4)
            .max(4),
    );
    let image_w = padded(
        state
            .containers
            .iter()
            .map(|c| c.image.len())
            .max()
            .unwrap_or(5)
            .max(5),
    );
    let state_w = padded(
        state
            .containers
            .iter()
            .map(|c| c.state.len())
            .max()
            .unwrap_or(5)
            .max(5),
    );
    let status_w = padded(
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
    let gap_str = " ".repeat(gap);
    let flex_str = " ".repeat(flex_gap);
    if let Some(hi) = header_ci {
        let style = theme::bold();
        let hdr = Line::from(vec![
            Span::styled(format!("   {:<name_w$}", "NAME"), style),
            Span::raw(&gap_str),
            Span::styled(format!("{:<image_w$}", "IMAGE"), style),
            Span::raw(&flex_str),
            Span::styled(format!("{:<state_w$}", "STATE"), style),
            Span::raw(&gap_str),
            Span::styled(format!("{:<status_w$}", "STATUS"), style),
        ]);
        frame.render_widget(Paragraph::new(hdr), chunks[hi]);
    }

    // Content
    if state.loading && state.containers.is_empty() {
        frame.render_widget(
            Paragraph::new("  Loading containers...").style(theme::muted()),
            list_area,
        );
    } else if let Some(ref err) = state.error {
        let err_msg = format!("  {}", err);
        frame.render_widget(Paragraph::new(err_msg).style(theme::error()), list_area);
    } else if state.containers.is_empty() {
        frame.render_widget(
            Paragraph::new("  No containers found. Is Docker or Podman installed?")
                .style(theme::muted()),
            list_area,
        );
    } else {
        let items: Vec<ListItem> = state
            .containers
            .iter()
            .map(|c| {
                let name_str = truncate_str(&c.names, name_w);
                let image_str = truncate_str(&c.image, image_w);
                let state_style = match c.state.as_str() {
                    "running" => theme::success(),
                    "exited" | "dead" => theme::muted(),
                    _ => theme::bold(),
                };
                let line = Line::from(vec![
                    Span::styled(format!(" {:<name_w$}", name_str), theme::bold()),
                    Span::raw(&gap_str),
                    Span::styled(format!("{:<image_w$}", image_str), theme::muted()),
                    Span::raw(&flex_str),
                    Span::styled(format!("{:<state_w$}", c.state), state_style),
                    Span::raw(&gap_str),
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
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, list_area, &mut state.list_state);
    }

    // Action in progress
    if let Some(ci) = action_ci {
        if let Some(ref msg) = state.action_in_progress {
            let action_line = format!("  {}", msg);
            frame.render_widget(
                Paragraph::new(action_line).style(theme::muted()),
                chunks[ci],
            );
        }
    }

    // Footer
    let mut spans: Vec<Span<'_>> = Vec::new();
    let [k, l] = super::footer_action("s", " start ");
    spans.extend([k, l, Span::raw("  ")]);
    let [k, l] = super::footer_action("x", " stop ");
    spans.extend([k, l, Span::raw("  ")]);
    let [k, l] = super::footer_action("r", " restart ");
    spans.extend([k, l, Span::raw("  ")]);
    let [k, l] = super::footer_action("R", " refresh ");
    spans.extend([k, l, Span::raw("  ")]);
    let [k, l] = super::footer_action("Esc", " back");
    spans.extend([k, l]);
    super::render_footer_with_status(frame, chunks[footer_ci], spans, app);

    // Confirmation dialog for stop/restart
    if let Some(ref confirm_state) = app.container_state {
        if let Some((ref action, ref name, _)) = confirm_state.confirm_action {
            let verb = action.as_str();
            let display_name = truncate_str(name, 30);
            let dialog_area = super::centered_rect_fixed(52, 7, frame.area());
            frame.render_widget(ratatui::widgets::Clear, dialog_area);
            let block = ratatui::widgets::Block::bordered()
                .border_type(ratatui::widgets::BorderType::Rounded)
                .title(Span::styled(format!(" Confirm {} ", verb), theme::danger()))
                .border_style(theme::border_danger());
            let text = vec![
                Line::from(""),
                Line::from(Span::styled(
                    format!("  {} \"{}\"?", verb, display_name),
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
            let paragraph = ratatui::widgets::Paragraph::new(text).block(block);
            frame.render_widget(paragraph, dialog_area);
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::{Constraint, Layout, Rect};

    use crate::SshConfigFile;
    use crate::app::{App, ContainerState};
    use std::path::PathBuf;

    fn make_app() -> App {
        let config = SshConfigFile {
            elements: SshConfigFile::parse_content(""),
            path: PathBuf::from("/tmp/test_containers_config"),
            crlf: false,
            bom: false,
        };
        App::new(config)
    }

    #[test]
    fn render_noops_when_container_state_is_none() {
        let mut app = make_app();
        assert!(app.container_state.is_none());
        render_app(&mut app);
    }

    fn state_with(
        loading: bool,
        error: Option<String>,
        action_in_progress: Option<String>,
    ) -> ContainerState {
        ContainerState {
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
        app.container_state = Some(state_with(false, None, None));
        render_app(&mut app);
    }

    #[test]
    fn render_survives_loading_state() {
        let mut app = make_app();
        app.container_state = Some(state_with(true, None, None));
        render_app(&mut app);
    }

    #[test]
    fn render_survives_error_state() {
        let mut app = make_app();
        app.container_state = Some(state_with(
            false,
            Some("docker not running".to_string()),
            None,
        ));
        render_app(&mut app);
    }

    #[test]
    fn render_survives_action_in_progress_state() {
        let mut app = make_app();
        app.container_state = Some(state_with(false, None, Some("stopping nginx".to_string())));
        render_app(&mut app);
    }

    #[test]
    fn layout_has_spacer_between_content_and_footer() {
        let area = Rect::new(0, 0, 60, 20);
        let chunks = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);
        assert_eq!(chunks[1].height, 1, "spacer row should be 1 tall");
        assert_eq!(chunks[2].height, 1, "footer row should be 1 tall");
        assert!(
            chunks[2].y > chunks[0].y + chunks[0].height,
            "footer (y={}) should be below content end (y={})",
            chunks[2].y,
            chunks[0].y + chunks[0].height
        );
    }

    #[test]
    fn layout_with_header_has_spacer() {
        let area = Rect::new(0, 0, 60, 20);
        let chunks = Layout::vertical([
            Constraint::Length(1), // header
            Constraint::Min(0),    // list
            Constraint::Length(1), // spacer
            Constraint::Length(1), // footer
        ])
        .split(area);
        let list_ci = 1;
        let footer_ci = 3;
        assert!(chunks[footer_ci].y > chunks[list_ci].y + chunks[list_ci].height);
    }

    #[test]
    fn layout_with_action_row() {
        let area = Rect::new(0, 0, 60, 20);
        let chunks = Layout::vertical([
            Constraint::Length(1), // header
            Constraint::Min(0),    // list
            Constraint::Length(1), // action
            Constraint::Length(1), // spacer
            Constraint::Length(1), // footer
        ])
        .split(area);
        assert_eq!(chunks[4].height, 1, "footer should be 1 tall");
        assert_eq!(chunks[3].height, 1, "spacer should be 1 tall");
        assert_eq!(chunks[2].height, 1, "action row should be 1 tall");
    }
}
