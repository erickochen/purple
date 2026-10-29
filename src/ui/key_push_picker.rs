//! Multi-select host picker for the Keys-tab `p` push action.
//!
//! Renders an overlay listing every host from `~/.ssh/config` with a
//! three-state checkbox column. Hosts whose `vault_ssh` role is configured
//! render as `[-]` and are skipped during selection commands so the user
//! does not accidentally append a static key onto a host that already uses
//! signed certs.

use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem};
use unicode_width::UnicodeWidthStr;

use super::design;
use super::theme;
use crate::app::App;
use crate::handler::key_push_picker::{is_vault_host, pickable_hosts};
use crate::ssh_config::model::HostEntry;

/// Maximum visible rows before the picker starts scrolling.
const MAX_VISIBLE_ROWS: u16 = 16;

/// Render the picker overlay. `key_index` is the index into `app.keys.list`
/// of the key being pushed; it drives the overlay title only.
pub fn render(frame: &mut Frame, app: &mut App, key_index: usize) {
    let hosts: Vec<&HostEntry> = pickable_hosts(app).collect();
    let selected_count = app.keys.push().selected.len();
    let total = hosts.len();
    let paths = app.env().paths();
    let eligible_total = hosts.iter().filter(|h| !is_vault_host(h, paths)).count();

    let key_label = app
        .keys
        .list()
        .get(key_index)
        .map(|k| format!("{}.pub", k.name))
        .unwrap_or_else(|| "key".to_string());

    let title = if selected_count == 0 {
        crate::messages::key_push_picker_title_eligible(&key_label, eligible_total, total)
    } else {
        crate::messages::key_push_picker_title_selected(
            &key_label,
            selected_count,
            total,
            eligible_total,
        )
    };

    let list_rows = (hosts.len() as u16).clamp(1, MAX_VISIBLE_ROWS);
    let total_height = 2 + list_rows; // borders + list

    // Honour the design-system picker bounds: at least PICKER_MIN_W
    // wide, at most PICKER_MAX_W, and never wider than the terminal
    // minus a 4-char inset. Height capped by PICKER_MAX_H so very tall
    // host lists scroll instead of pushing the footer off screen.
    let term_w = frame.area().width;
    let term_h = frame.area().height;
    let target_w = design::PICKER_MIN_W.max(term_w * 60 / 100);
    let overlay_width = target_w
        .min(design::PICKER_MAX_W)
        .min(term_w.saturating_sub(4));
    let height = total_height
        .min(design::PICKER_MAX_H)
        .min(term_h.saturating_sub(3));
    let area = super::centered_rect_fixed(overlay_width, height, frame.area());

    frame.render_widget(Clear, area);
    let block = design::overlay_block(&title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if hosts.is_empty() {
        let empty = crate::messages::key_push_no_hosts(&app.config_path_display());
        design::render_empty(frame, inner, &empty);
    } else {
        let content_w = inner.width as usize;
        let items: Vec<ListItem> = hosts
            .iter()
            .map(|h| build_row(h, &app.keys.push().selected, content_w, paths))
            .collect();

        let sel = app.keys.push().list_state.selected();
        let new_sel = match sel {
            Some(i) if i < hosts.len() => Some(i),
            _ => Some(0),
        };
        if new_sel != sel {
            app.keys.push_mut().list_state.select(new_sel);
        }

        let list = List::new(items).highlight_style(theme::selected_row());
        frame.render_stateful_widget(list, inner, &mut app.keys.push_mut().list_state);
    }

    let footer_area = design::render_overlay_footer(frame, area);
    use crate::messages::footer as fl;
    design::Footer::new()
        .primary("Enter", crate::i18n::text(fl::ENTER_CONFIRM))
        .action("Space", crate::i18n::text(fl::SPACE_TOGGLE))
        .action("a", crate::i18n::text(fl::ACTION_ALL))
        .action("Esc", crate::i18n::text(fl::ESC_CANCEL))
        .render_with_status(frame, footer_area, app);
}

/// One picker row's spans: `[x|-|space] alias    hostname    (vault?)`.
/// Extracted so unit tests can inspect the text without round-tripping
/// through ratatui's `ListItem` (whose `content` field is private).
fn build_row_spans(
    host: &HostEntry,
    selected: &std::collections::HashSet<String>,
    content_w: usize,
    paths: Option<&crate::runtime::env::Paths>,
) -> Vec<Span<'static>> {
    let is_vault = is_vault_host(host, paths);
    let is_selected = selected.contains(&host.alias);
    let checkbox = if is_vault {
        "[-]"
    } else if is_selected {
        "[x]"
    } else {
        "[ ]"
    };
    // `[x]` indicates a user-action state (selected for the next push),
    // not a live-state signal, so use `accent_bold` (brand purple) rather
    // than `online_dot`/`success` which encode "live right now".
    let checkbox_style = if is_vault {
        theme::muted()
    } else if is_selected {
        theme::accent_bold()
    } else {
        theme::muted()
    };
    let alias_style = if is_vault {
        theme::muted()
    } else {
        theme::bold()
    };
    let hostname_style = theme::muted();

    let alias_w = host.alias.width();
    let leading = 2;
    let checkbox_w = 3;
    let gap = 2;
    let vault_tag = if is_vault {
        crate::i18n::text(crate::messages::KEY_PUSH_VAULT_TAG)
    } else {
        ""
    };
    let vault_w = vault_tag.width();
    let used = leading + checkbox_w + gap + alias_w + gap + vault_w;
    let hostname_budget = content_w.saturating_sub(used);
    // Column-aware truncation so East Asian wide chars (IDN-decoded CJK)
    // do not overflow the picker row. `super::truncate` uses
    // `UnicodeWidthStr` to count display columns rather than code points.
    let hostname_truncated = super::truncate(&host.hostname, hostname_budget);

    vec![
        Span::raw(" "),
        Span::styled(checkbox.to_string(), checkbox_style),
        Span::raw(" "),
        Span::styled(host.alias.clone(), alias_style),
        Span::raw(design::COL_GAP_STR),
        Span::styled(hostname_truncated, hostname_style),
        Span::styled(vault_tag.to_string(), theme::muted()),
    ]
}

fn build_row(
    host: &HostEntry,
    selected: &std::collections::HashSet<String>,
    content_w: usize,
    paths: Option<&crate::runtime::env::Paths>,
) -> ListItem<'static> {
    ListItem::new(Line::from(build_row_spans(
        host, selected, content_w, paths,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(alias: &str, hostname: &str, vault: Option<&str>) -> HostEntry {
        HostEntry {
            alias: alias.to_string(),
            hostname: hostname.to_string(),
            vault_ssh: vault.map(|s| s.to_string()),
            ..Default::default()
        }
    }

    fn flatten(spans: &[Span<'_>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    // With no hosts the picker names the config file it reads.
    #[test]
    fn empty_picker_names_the_config_file() {
        let mut app = crate::ui::app_with_own_config("");
        let text = crate::ui::render_to_text(120, 40, |frame| render(frame, &mut app, 0));
        assert!(text.contains("~/.ssh/purple_config"), "{text}");
        assert!(!text.contains("~/.ssh/config"), "{text}");
    }

    #[test]
    fn build_row_marks_vault_with_dash() {
        let host = h("prod-vault", "10.0.0.1", Some("ops/prod"));
        let selected = std::collections::HashSet::new();
        let text = flatten(&build_row_spans(&host, &selected, 80, None));
        assert!(text.contains("[-]"));
        assert!(text.contains("(vault)"));
    }

    #[test]
    fn build_row_marks_selected_with_x() {
        let host = h("prod", "10.0.0.2", None);
        let mut selected = std::collections::HashSet::new();
        selected.insert("prod".to_string());
        let text = flatten(&build_row_spans(&host, &selected, 80, None));
        assert!(text.contains("[x]"));
    }

    #[test]
    fn build_row_unselected_shows_empty_box() {
        let host = h("staging", "1.2.3.4", None);
        let selected = std::collections::HashSet::new();
        let text = flatten(&build_row_spans(&host, &selected, 80, None));
        assert!(text.contains("[ ]"));
        assert!(!text.contains("[x]"));
    }
}
