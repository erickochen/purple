use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem};

use super::theme;
use crate::app::App;
use crate::ui::theme::ThemeDef;

pub fn render(frame: &mut Frame, app: &mut App) {
    let builtins = &app.ui.theme_picker_builtins;
    let custom = &app.ui.theme_picker_custom;
    let current_name = &app.ui.theme_picker_saved_name;

    let has_custom = !custom.is_empty();
    let total = builtins.len() + if has_custom { 1 + custom.len() } else { 0 };
    let height = (total as u16 + 4).min(frame.area().height.saturating_sub(4));
    let area = super::centered_rect_fixed(50, height, frame.area());
    frame.render_widget(Clear, area);

    let mut items: Vec<ListItem> = Vec::new();

    for t in builtins {
        items.push(theme_item(t, current_name));
    }

    if has_custom {
        items.push(ListItem::new(Line::from(Span::styled(
            " \u{2500}\u{2500} custom \u{2500}\u{2500}",
            theme::muted(),
        ))));
        for t in custom {
            items.push(theme_item(t, current_name));
        }
    }

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" Theme ", theme::brand()))
        .border_style(theme::accent());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);

    let list = List::new(items)
        .highlight_style(theme::selected_row())
        .highlight_symbol("  ");

    frame.render_stateful_widget(list, chunks[0], &mut app.ui.theme_picker_state);

    let spans = vec![
        Span::styled(" Enter ", theme::footer_key()),
        Span::styled(" select ", theme::muted()),
        Span::raw("  "),
        Span::styled(" Esc ", theme::footer_key()),
        Span::styled(" cancel", theme::muted()),
    ];
    super::render_footer_with_status(frame, chunks[2], spans, app);
}

fn theme_item<'a>(t: &ThemeDef, current_name: &str) -> ListItem<'a> {
    let marker = if t.name.eq_ignore_ascii_case(current_name) {
        "\u{2713} "
    } else {
        "  "
    };

    let mode = theme::color_mode();
    let swatches = vec![
        Span::styled("\u{2588}", t.accent.to_style(mode)),
        Span::raw(" "),
        Span::styled("\u{2588}", t.success.to_style(mode)),
        Span::raw(" "),
        Span::styled("\u{2588}", t.warning.to_style(mode)),
        Span::raw(" "),
        Span::styled("\u{2588}", t.error.to_style(mode)),
    ];

    let mut spans = vec![
        Span::styled(marker.to_string(), theme::bold()),
        Span::styled(format!("{:<24}", t.name), theme::bold()),
    ];
    spans.extend(swatches);

    ListItem::new(Line::from(spans))
}
