use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, Paragraph, StatefulWidget};

use super::theme;
use crate::app::App;
use crate::file_browser::{BrowserPane, BrowserSort, FileBrowserState};

pub fn render(frame: &mut Frame, app: &mut App) {
    let fb = match app.file_browser.as_mut() {
        Some(fb) => fb,
        None => return,
    };

    let area = frame.area();

    // Terminal too narrow guard
    if area.width < 70 {
        let overlay = super::centered_rect(60, 20, area);
        frame.render_widget(Clear, overlay);
        let msg = Paragraph::new("Terminal too narrow for file browser. Need 70+ columns.")
            .style(theme::error());
        frame.render_widget(msg, overlay);
        return;
    }

    let overlay = super::centered_rect(90, 85, area);
    frame.render_widget(Clear, overlay);

    let title = format!(" Files: {} ", fb.alias);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(title, theme::brand()))
        .border_style(theme::accent());

    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    // Layout: path headers + divider + file lists + spacer + footer
    let rows = Layout::vertical([
        Constraint::Length(1), // path headers
        Constraint::Length(1), // divider
        Constraint::Min(0),    // file lists
        Constraint::Length(1), // spacer
        Constraint::Length(1), // footer
    ])
    .split(inner);

    // Split into two panes
    let pane_cols =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[2]);

    let path_cols =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[0]);

    let div_cols =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[1]);

    // Path headers
    let local_path_str = fb.local_path.to_string_lossy().to_string();
    let local_path_display = truncate_left(
        &local_path_str,
        (path_cols[0].width as usize).saturating_sub(1),
    );
    let remote_path_display = if fb.remote_path.is_empty() {
        "...".to_string()
    } else {
        truncate_left(
            &fb.remote_path,
            (path_cols[1].width as usize).saturating_sub(1),
        )
    };

    let local_path_style = if fb.active_pane == BrowserPane::Local {
        theme::accent_bold()
    } else {
        theme::muted()
    };
    let remote_path_style = if fb.active_pane == BrowserPane::Remote {
        theme::accent_bold()
    } else {
        theme::muted()
    };

    frame.render_widget(
        Paragraph::new(Span::styled(
            format!(" {}", local_path_display),
            local_path_style,
        )),
        path_cols[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            format!(" {}", remote_path_display),
            remote_path_style,
        )),
        path_cols[1],
    );

    // Dividers
    let local_div_style = if fb.active_pane == BrowserPane::Local {
        theme::accent()
    } else {
        theme::border()
    };
    let remote_div_style = if fb.active_pane == BrowserPane::Remote {
        theme::accent()
    } else {
        theme::border()
    };
    let local_div = "─".repeat(div_cols[0].width as usize);
    let remote_div = "─".repeat(div_cols[1].width as usize);
    frame.render_widget(
        Paragraph::new(Span::styled(local_div, local_div_style)),
        div_cols[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(remote_div, remote_div_style)),
        div_cols[1],
    );

    // Local pane
    render_local_pane(frame, fb, pane_cols[0]);

    // Remote pane
    render_remote_pane(frame, fb, pane_cols[1]);

    // Confirm dialog overlay
    if let Some(ref req) = fb.confirm_copy {
        render_confirm_dialog(frame, fb, req, area);
    }

    // Transfer progress overlay
    if let Some(ref label) = fb.transferring {
        render_transfer_dialog(frame, label, area);
    }

    // Transfer error overlay
    if let Some(ref err) = fb.transfer_error {
        render_error_dialog(frame, err, area);
    }

    // Footer
    let mut footer_spans = Vec::new();
    let selected_count = match fb.active_pane {
        BrowserPane::Local => fb.local_selected.len(),
        BrowserPane::Remote => fb.remote_selected.len(),
    };

    footer_spans.push(Span::styled(" Enter ", theme::footer_key()));
    footer_spans.push(Span::styled(" copy ", theme::muted()));
    footer_spans.push(Span::raw("  "));
    footer_spans.push(Span::styled(" Tab ", theme::footer_key()));
    footer_spans.push(Span::styled(" switch ", theme::muted()));
    footer_spans.push(Span::raw("  "));
    footer_spans.push(Span::styled(" ^Space ", theme::footer_key()));
    footer_spans.push(Span::styled(" select ", theme::muted()));
    footer_spans.push(Span::raw("  "));
    footer_spans.push(Span::styled(" ^A ", theme::footer_key()));
    footer_spans.push(Span::styled(" all ", theme::muted()));
    footer_spans.push(Span::raw("  "));
    footer_spans.push(Span::styled(" s ", theme::footer_key()));
    let sort_label = match fb.sort {
        BrowserSort::Name => " sort:name ",
        BrowserSort::Date => " sort:date\u{2193} ",
        BrowserSort::DateAsc => " sort:date\u{2191} ",
    };
    footer_spans.push(Span::styled(sort_label, theme::muted()));
    footer_spans.push(Span::raw("  "));
    footer_spans.push(Span::styled(" . ", theme::footer_key()));
    footer_spans.push(Span::styled(" hidden ", theme::muted()));
    footer_spans.push(Span::raw("  "));
    footer_spans.push(Span::styled(" R ", theme::footer_key()));
    footer_spans.push(Span::styled(" refresh ", theme::muted()));
    footer_spans.push(Span::raw("  "));
    footer_spans.push(Span::styled(" Esc ", theme::footer_key()));
    footer_spans.push(Span::styled(" close", theme::muted()));

    if selected_count > 0 {
        footer_spans.push(Span::raw("  "));
        footer_spans.push(Span::styled(
            format!("{} selected", selected_count),
            theme::accent_bold(),
        ));
    }

    super::render_footer_with_status(frame, rows[4], footer_spans, app);
}

fn render_local_pane(frame: &mut Frame, fb: &mut FileBrowserState, area: Rect) {
    if let Some(ref err) = fb.local_error {
        let lines = vec![
            Line::from(Span::styled(err.as_str(), theme::error())),
            Line::from(""),
            Line::from(Span::styled("R to retry", theme::muted())),
        ];
        frame.render_widget(Paragraph::new(lines), area);
        return;
    }

    let pane_width = area.width as usize;
    let show_date = matches!(fb.sort, BrowserSort::Date | BrowserSort::DateAsc);
    let items = build_file_list_items(&fb.local_entries, &fb.local_selected, pane_width, show_date);

    let list = List::new(items).highlight_style(if fb.active_pane == BrowserPane::Local {
        theme::selected_row()
    } else {
        Style::default()
    });

    StatefulWidget::render(list, area, frame.buffer_mut(), &mut fb.local_list_state);
}

fn render_remote_pane(frame: &mut Frame, fb: &mut FileBrowserState, area: Rect) {
    if fb.remote_loading {
        let path = if fb.remote_path.is_empty() {
            "~".to_string()
        } else {
            fb.remote_path.clone()
        };
        let msg = format!(" Loading {} ...", path);
        frame.render_widget(Paragraph::new(Span::styled(msg, theme::muted())), area);
        return;
    }

    if let Some(ref err) = fb.remote_error {
        let lines = vec![
            Line::from(Span::styled(format!(" {}", err), theme::error())),
            Line::from(""),
            Line::from(Span::styled(" R to retry", theme::muted())),
        ];
        frame.render_widget(Paragraph::new(lines), area);
        return;
    }

    let pane_width = area.width as usize;
    let show_date = matches!(fb.sort, BrowserSort::Date | BrowserSort::DateAsc);
    let items = build_file_list_items(
        &fb.remote_entries,
        &fb.remote_selected,
        pane_width,
        show_date,
    );

    let list = List::new(items).highlight_style(if fb.active_pane == BrowserPane::Remote {
        theme::selected_row()
    } else {
        Style::default()
    });

    StatefulWidget::render(list, area, frame.buffer_mut(), &mut fb.remote_list_state);
}

fn build_file_list_items<'a>(
    entries: &[crate::file_browser::FileEntry],
    selected: &std::collections::HashSet<String>,
    pane_width: usize,
    show_date: bool,
) -> Vec<ListItem<'a>> {
    let mut items = Vec::with_capacity(entries.len() + 1);

    // ".." entry
    items.push(ListItem::new(Line::from(Span::raw("   .."))));

    if entries.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "   (empty directory)",
            theme::muted(),
        ))));
        return items;
    }

    let size_col_width = 9; // " 1.1 KB  "
    let date_col_width = if show_date { 10 } else { 0 }; // " 3d ago  " / " Jan 15  "
    let prefix_width = 3; // " * " or "   "
    let name_col = pane_width.saturating_sub(size_col_width + date_col_width + prefix_width);

    for entry in entries {
        let is_selected = selected.contains(&entry.name);
        let prefix = if is_selected { " * " } else { "   " };
        let name_display = if entry.is_dir {
            let dir_name = format!("\u{25b8} {}/", entry.name);
            super::truncate(&dir_name, name_col)
        } else {
            super::truncate(&entry.name, name_col)
        };

        // Pad name to fixed column width so sizes align
        let name_width = unicode_width::UnicodeWidthStr::width(name_display.as_str());
        let padding = name_col.saturating_sub(name_width);
        let name_padded = format!("{}{}", name_display, " ".repeat(padding));

        let size_str = match entry.size {
            Some(bytes) => {
                let s = crate::file_browser::format_size(bytes);
                format!("{:>8} ", s)
            }
            None => " ".repeat(size_col_width),
        };

        let name_style = if entry.is_dir {
            theme::bold()
        } else if is_selected {
            theme::accent_bold()
        } else {
            Style::default()
        };

        let mut spans = vec![
            Span::styled(
                prefix.to_string(),
                if is_selected {
                    theme::accent_bold()
                } else {
                    Style::default()
                },
            ),
            Span::styled(name_padded, name_style),
            Span::styled(size_str, theme::muted()),
        ];

        if show_date {
            let date_str = match entry.modified {
                Some(ts) => {
                    let s = crate::file_browser::format_relative_time(ts);
                    format!("{:>9} ", s)
                }
                None => " ".repeat(date_col_width),
            };
            spans.push(Span::styled(date_str, theme::muted()));
        }

        items.push(ListItem::new(Line::from(spans)));
    }
    items
}

fn render_confirm_dialog(
    frame: &mut Frame,
    fb: &FileBrowserState,
    req: &crate::file_browser::CopyRequest,
    area: Rect,
) {
    let direction = match req.source_pane {
        BrowserPane::Local => "remote",
        BrowserPane::Remote => "local",
    };

    let dest_path = match req.source_pane {
        BrowserPane::Local => fb.remote_path.as_str(),
        BrowserPane::Remote => fb.local_path.to_str().unwrap_or("?"),
    };

    let header = if req.sources.len() == 1 {
        format!("  Copy to {}:", direction)
    } else {
        format!("  Copy {} files to {}:", req.sources.len(), direction)
    };

    let mut content_lines: Vec<String> = Vec::new();
    content_lines.push(header.clone());
    if req.sources.len() <= 5 {
        for name in &req.sources {
            content_lines.push(format!("    {} -> {}/", name, dest_path));
        }
    } else {
        for name in req.sources.iter().take(4) {
            content_lines.push(format!("    {} -> {}/", name, dest_path));
        }
        content_lines.push(format!("    ... and {} more", req.sources.len() - 4));
    }

    // Calculate width from content (+ 4 for border + padding)
    let max_content: usize = content_lines
        .iter()
        .map(|l| unicode_width::UnicodeWidthStr::width(l.as_str()))
        .max()
        .unwrap_or(30);
    let width = ((max_content + 4) as u16)
        .max(30)
        .min(area.width.saturating_sub(4));

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(header, theme::bold())));
    if req.sources.len() <= 5 {
        for name in &req.sources {
            lines.push(Line::from(format!("    {} -> {}/", name, dest_path)));
        }
    } else {
        for name in req.sources.iter().take(4) {
            lines.push(Line::from(format!("    {} -> {}/", name, dest_path)));
        }
        lines.push(Line::from(format!(
            "    ... and {} more",
            req.sources.len() - 4
        )));
    }
    lines.push(Line::from(""));

    let height = (lines.len() + 3) as u16;
    let dialog_area = super::centered_rect_fixed(width, height, area);

    frame.render_widget(Clear, dialog_area);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" Confirm ", theme::brand()))
        .border_style(theme::accent());
    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    let rows = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);

    frame.render_widget(Paragraph::new(lines), rows[0]);

    let footer = vec![
        Span::styled(" y ", theme::footer_key()),
        Span::styled(" confirm ", theme::muted()),
        Span::raw("  "),
        Span::styled(" Esc ", theme::footer_key()),
        Span::styled(" cancel", theme::muted()),
    ];
    frame.render_widget(Paragraph::new(Line::from(footer)), rows[2]);
}

fn render_transfer_dialog(frame: &mut Frame, label: &str, area: Rect) {
    // Fixed width so the dialog doesn't jump around as progress updates
    let width = 60u16.min(area.width.saturating_sub(4));
    let inner_width = width.saturating_sub(4) as usize; // 2 border + 2 padding
    let display = super::truncate(label, inner_width);
    let height = 5u16;
    let dialog_area = super::centered_rect_fixed(width, height, area);

    frame.render_widget(Clear, dialog_area);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" Transfer ", theme::brand()))
        .border_style(theme::accent());
    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(format!("  {}", display), theme::accent_bold())),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_error_dialog(frame: &mut Frame, message: &str, area: Rect) {
    // Build content lines, wrapping long messages
    let mut content_lines: Vec<String> = Vec::new();
    for line in message.lines() {
        content_lines.push(format!("  {}", line));
    }
    if content_lines.is_empty() {
        content_lines.push("  Copy failed.".to_string());
    }

    let max_content: usize = content_lines
        .iter()
        .map(|l| unicode_width::UnicodeWidthStr::width(l.as_str()))
        .max()
        .unwrap_or(20);
    let width = ((max_content + 4) as u16)
        .max(30)
        .min(area.width.saturating_sub(4));

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));
    for cl in &content_lines {
        lines.push(Line::from(Span::styled(cl.clone(), theme::error())));
    }
    lines.push(Line::from(""));

    let height = ((lines.len() + 3) as u16).min(area.height.saturating_sub(4));
    let dialog_area = super::centered_rect_fixed(width, height, area);

    frame.render_widget(Clear, dialog_area);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" Error ", theme::brand()))
        .border_style(theme::border_danger());
    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    let rows = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);

    frame.render_widget(Paragraph::new(lines), rows[0]);

    let footer = vec![
        Span::styled(" Esc ", theme::footer_key()),
        Span::styled(" dismiss", theme::muted()),
    ];
    frame.render_widget(Paragraph::new(Line::from(footer)), rows[2]);
}

/// Truncate a string from the LEFT, prefixing with `\u{2026}` if truncated.
/// Used for path display where the end is more important than the beginning.
pub(crate) fn truncate_left(s: &str, max_cols: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    if s.width() <= max_cols {
        return s.to_string();
    }
    if max_cols <= 1 {
        return "\u{2026}".to_string();
    }
    let target = max_cols - 1; // 1 for the ellipsis
    // Walk backwards through chars to find how many fit
    let chars: Vec<char> = s.chars().collect();
    let mut col = 0;
    let mut start_idx = chars.len();
    for i in (0..chars.len()).rev() {
        let w = unicode_width::UnicodeWidthChar::width(chars[i]).unwrap_or(0);
        if col + w > target {
            break;
        }
        col += w;
        start_idx = i;
    }
    let suffix: String = chars[start_idx..].iter().collect();
    format!("\u{2026}{}", suffix)
}

#[cfg(test)]
mod tests {
    use ratatui::layout::{Constraint, Layout, Rect};

    #[test]
    fn main_layout_has_spacer_between_filelist_and_footer() {
        let area = Rect::new(0, 0, 80, 30);
        let rows = Layout::vertical([
            Constraint::Length(1), // path headers
            Constraint::Length(1), // divider
            Constraint::Min(0),    // file lists
            Constraint::Length(1), // spacer
            Constraint::Length(1), // footer
        ])
        .split(area);
        assert_eq!(rows[3].height, 1);
        assert_eq!(rows[4].height, 1);
        assert!(
            rows[4].y > rows[2].y + rows[2].height,
            "footer should be below file list end"
        );
    }

    #[test]
    fn confirm_dialog_layout_has_spacer() {
        let area = Rect::new(0, 0, 50, 10);
        let rows = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);
        assert!(rows[2].y > rows[0].y + rows[0].height);
    }

    #[test]
    fn error_dialog_layout_has_spacer() {
        let area = Rect::new(0, 0, 40, 10);
        let rows = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);
        assert!(rows[2].y > rows[0].y + rows[0].height);
    }
}
