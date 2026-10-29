use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem};

use super::design;
use super::theme;
use crate::app::App;
use crate::ui::theme::ThemeDef;

pub fn render(frame: &mut Frame, app: &mut App) {
    let builtins = &app.ui.theme_picker().builtins;
    let custom = &app.ui.theme_picker().custom;
    let current_name = &app.ui.theme_picker().saved_name;

    let has_custom = !custom.is_empty();
    let total = builtins.len() + if has_custom { 1 + custom.len() } else { 0 };
    // Use the canonical picker geometry: width clamp[PICKER_MIN_W,
    // PICKER_MAX_W], height grows with item count up to PICKER_MAX_H.
    // Reserve 1 row below the block for the external footer.
    let width = super::picker_overlay_width(frame);
    let height = (total as u16 + 2)
        .min(design::PICKER_MAX_H)
        .min(frame.area().height.saturating_sub(3));
    if height < super::PICKER_MIN_HEIGHT {
        return;
    }
    let area = super::centered_rect_fixed(width, height, frame.area());
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

    let block = design::overlay_block("Theme");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let list = List::new(items)
        .highlight_style(theme::selected_row())
        .highlight_symbol(design::LIST_HIGHLIGHT);

    frame.render_stateful_widget(list, inner, &mut app.ui.theme_picker_mut().list);

    let footer_area = design::render_overlay_footer(frame, area);
    use crate::messages::footer as fl;
    design::Footer::new()
        .primary("Enter", crate::i18n::text(fl::ENTER_SELECT))
        .action("Esc", crate::i18n::text(fl::ESC_BACK))
        .render_with_status(frame, footer_area, app);
}

fn theme_item<'a>(t: &ThemeDef, current_name: &str) -> ListItem<'a> {
    let marker: String = if t.name.eq_ignore_ascii_case(current_name) {
        format!("{} ", design::ICON_SUCCESS)
    } else {
        "  ".to_string()
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
