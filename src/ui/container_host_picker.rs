//! Host picker overlay reached from the containers overview when
//! adding a host to the container cache. Layout, input and footer
//! mirror the tunnel host picker so the two pickers feel identical.
//!
//! Row rendering, search input, separator, overlay geometry and the
//! selection clamp are shared with `tunnel_host_picker` via
//! `super::picker_helpers`. Only the title, empty-state text and the
//! app-state field names live here.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::widgets::{Clear, List, ListItem};

use super::design;
use super::picker_helpers;
use super::theme;
use crate::app::App;
use crate::handler::container_host_picker::{filtered_hosts, uncached_aliases};

pub fn render(frame: &mut Frame, app: &mut App) {
    let total_uncached = uncached_aliases(app).len();
    let visible = filtered_hosts(app);
    let query = app.ui.container_host_picker_query().clone();

    let area = picker_helpers::host_picker_overlay_area(frame, visible.len());
    frame.render_widget(Clear, area);

    let title = if query.is_empty() {
        format!("Add Container Host \u{203A} Select ({})", total_uncached)
    } else {
        format!(
            "Add Container Host \u{203A} Select ({} of {})",
            visible.len(),
            total_uncached
        )
    };
    let block = design::overlay_block(&title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .split(inner);

    picker_helpers::render_search_input(frame, rows[0], &query, true);
    picker_helpers::render_picker_separator(frame, rows[1]);

    if visible.is_empty() {
        let msg = if total_uncached == 0 {
            crate::i18n::text(crate::messages::CONTAINER_HOST_PICKER_NOTHING_TO_ADD)
        } else {
            crate::i18n::text(crate::messages::CONTAINER_HOST_PICKER_NO_MATCH)
        };
        design::render_empty(frame, rows[2], msg);
    } else {
        let content_w = rows[2].width as usize;
        let items: Vec<ListItem> = visible
            .iter()
            .map(|(alias, hostname)| {
                picker_helpers::build_alias_hostname_row(alias, hostname, content_w)
            })
            .collect();

        picker_helpers::clamp_picker_selection(
            app.ui.container_host_picker_state_mut(),
            visible.len(),
        );

        let list = List::new(items).highlight_style(theme::selected_row());
        frame.render_stateful_widget(list, rows[2], app.ui.container_host_picker_state_mut());
    }

    let footer_area = design::render_overlay_footer(frame, area);
    use crate::messages::footer as fl;
    design::Footer::new()
        .primary("Enter", crate::i18n::text(fl::ENTER_SELECT))
        .action("\u{2191}\u{2193}", crate::i18n::text(fl::ARROWS_SELECT))
        .action("Esc", crate::i18n::text(fl::ESC_CANCEL))
        .render_with_status(frame, footer_area, app);
}
