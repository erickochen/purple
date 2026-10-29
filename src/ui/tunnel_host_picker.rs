//! Host picker overlay reached from the Tunnels overview when adding a new
//! tunnel. Layout, input affordance and footer mirror the jump bar
//! so users get the same "type to filter" experience everywhere in purple.
//!
//! Row rendering, search input, separator, overlay geometry and the
//! selection clamp are shared with `container_host_picker` via
//! `super::picker_helpers`. Only the title, empty-state text and the
//! app-state field names live here.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::widgets::{Clear, List, ListItem};

use super::design;
use super::picker_helpers;
use super::theme;
use crate::app::App;
use crate::handler::tunnel_host_picker::{editable_aliases, filtered_hosts};

pub fn render(frame: &mut Frame, app: &mut App) {
    let total_editable = editable_aliases(app).len();
    let visible = filtered_hosts(app);
    let query = app.ui.tunnel_host_picker_query().clone();

    let area = picker_helpers::host_picker_overlay_area(frame, visible.len());
    frame.render_widget(Clear, area);

    let title = if query.is_empty() {
        format!("Add Tunnel \u{203A} Select Host ({})", total_editable)
    } else {
        format!(
            "Add Tunnel \u{203A} Select Host ({} of {})",
            visible.len(),
            total_editable
        )
    };
    let block = design::overlay_block(&title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Length(1), // input line
        Constraint::Length(1), // separator
        Constraint::Min(1),    // host list
    ])
    .split(inner);

    picker_helpers::render_search_input(frame, rows[0], &query, true);
    picker_helpers::render_picker_separator(frame, rows[1]);

    if visible.is_empty() {
        let msg = if total_editable == 0 {
            crate::i18n::text(crate::messages::TUNNEL_NO_EDITABLE_HOSTS)
        } else {
            crate::i18n::text(crate::messages::TUNNEL_HOST_PICKER_NO_MATCH)
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
            app.ui.tunnel_host_picker_state_mut(),
            visible.len(),
        );

        let list = List::new(items).highlight_style(theme::selected_row());
        frame.render_stateful_widget(list, rows[2], app.ui.tunnel_host_picker_state_mut());
    }

    let footer_area = design::render_overlay_footer(frame, area);
    use crate::messages::footer as fl;
    design::Footer::new()
        .primary("Enter", crate::i18n::text(fl::ENTER_SELECT))
        .action("\u{2191}\u{2193}", crate::i18n::text(fl::ARROWS_SELECT))
        .action("Esc", crate::i18n::text(fl::ESC_CANCEL))
        .render_with_status(frame, footer_area, app);
}
