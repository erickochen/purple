use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use super::design;
use super::theme;
use crate::app::App;
use crate::ssh_config::model::ConfigElement;

pub fn render(frame: &mut Frame, app: &App, index: usize) {
    let Some(host) = app.hosts_state.list().get(index) else {
        return;
    };

    let directives = find_host_directives(&app.hosts_state.ssh_config().elements, &host.alias);

    let directive_count = directives.len();
    let max_visible = 15;
    let visible = directive_count.min(max_visible);
    // 2 (border) + 1 (blank) + 1 (header) + 1 (separator) + directives + 1 (overflow) + source.
    // Footer renders below the block via `design::form_footer`, so no row reserved here.
    let askpass_lines = if host.askpass.is_some() { 2 } else { 0 };
    let source_lines = if host.source_file.is_some() { 2 } else { 0 };
    let overflow_line = if directive_count > max_visible { 1 } else { 0 };
    let height = (6 + visible.max(1) + overflow_line + askpass_lines + source_lines) as u16;
    let width = frame.area().width.clamp(58, 80);
    let area = super::centered_rect_fixed(width, height, frame.area());

    frame.render_widget(Clear, area);

    let block = design::overlay_block(&host.alias);

    let mut lines = vec![Line::from("")];
    lines.extend(design::content_section("Directives"));

    if directives.is_empty() {
        lines.push(design::empty_line("(none)"));
    } else {
        for (key, value) in directives.iter().take(max_visible) {
            lines.push(design::kv_line(key, value, design::KV_LABEL_WIDE));
        }
        if directive_count > max_visible {
            lines.push(Line::from(Span::styled(
                format!("  (and {} more...)", directive_count - max_visible),
                theme::muted(),
            )));
        }
    }

    if let Some(ref askpass) = host.askpass {
        lines.push(Line::from(""));
        lines.push(design::kv_line(
            crate::i18n::text("Password"),
            &askpass.to_string(),
            design::KV_LABEL_WIDE,
        ));
    }

    if let Some(ref source) = host.source_file {
        lines.push(Line::from(""));
        lines.push(design::kv_line(
            crate::i18n::text("Source"),
            &source.display().to_string(),
            design::KV_LABEL_WIDE,
        ));
    }

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);

    // Footer below the block
    let is_included = host.source_file.is_some();
    use crate::messages::footer as fl;
    let mut footer_builder = design::Footer::new();
    if !is_included {
        footer_builder = footer_builder.action("e", crate::i18n::text(fl::ACTION_EDIT));
    }
    let footer_spans = footer_builder
        .action("T", crate::i18n::text(fl::ACTION_TUNNELS))
        .action("r", crate::i18n::text(fl::ACTION_SNIPPET))
        .action("Esc", crate::i18n::text(fl::ESC_BACK))
        .into_spans();
    let footer_area = design::render_overlay_footer(frame, area);
    super::render_footer_with_status(frame, footer_area, footer_spans, app);
}

/// Find all real directives for a host by searching config elements.
fn find_host_directives(elements: &[ConfigElement], alias: &str) -> Vec<(String, String)> {
    for element in elements {
        match element {
            ConfigElement::HostBlock(block) if block.host_pattern == alias => {
                return block
                    .directives
                    .iter()
                    .filter(|d| !d.is_non_directive)
                    .map(|d| (d.key.clone(), d.value.clone()))
                    .collect();
            }
            ConfigElement::Include(include) => {
                for file in &include.resolved_files {
                    let result = find_host_directives(&file.elements, alias);
                    if !result.is_empty() {
                        return result;
                    }
                }
            }
            _ => {}
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::design;

    #[test]
    fn footer_sits_directly_below_block() {
        let area = Rect::new(0, 0, 60, 12);
        let footer = design::form_footer(area, area.height);
        assert_eq!(footer.height, 1);
        assert_eq!(footer.y, area.y + area.height);
    }
}
