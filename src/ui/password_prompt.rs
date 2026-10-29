//! Password prompt overlay for `Screen::PasswordPrompt`.
//!
//! A background ssh cannot ask for a password on the terminal, so the host
//! that wants one asks here. The typed password is masked one star per
//! character; the toggle decides whether it goes to the OS keychain or
//! stays in memory until purple exits.

use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use super::design;
use super::theme;
use crate::app::{App, PasswordPromptField};
use crate::messages::askpass as msg;
use crate::messages::footer as fl;

/// Overlay width. Set by the footer, which is the widest row at 61 columns.
const PROMPT_W: u16 = 64;
/// borders (2) + blank + identity + blank + password + blank + remember.
const PROMPT_H: u16 = 8;
/// borders (2) + blank + identity + blank + key + PIN + blank + touch hint.
const PIN_PROMPT_H: u16 = 9;
/// Label column, padded so both field rows line up.
const LABEL_W: usize = 10;
/// Columns left for the key path: the inner width less the indent and the
/// label column.
const KEY_PATH_W: usize = PROMPT_W as usize - 2 - 2 - LABEL_W;

pub fn render(frame: &mut Frame, app: &mut App) {
    let Some(state) = app.password_prompt.as_ref() else {
        return;
    };
    if let Some(key_path) = state.security_key.as_deref() {
        let label =
            super::file_browser::truncate_left(&app.security_key_label(key_path), KEY_PATH_W);
        render_security_key_pin(frame, &state.alias, &label, state.input.chars().count());
        return;
    }

    let area = super::centered_rect_fixed(PROMPT_W, PROMPT_H, frame.area());
    frame.render_widget(Clear, area);
    let block = design::overlay_block(msg::PROMPT_TITLE);

    let identity = Line::from(vec![
        Span::raw("  "),
        Span::styled(state.alias.clone(), theme::bold()),
        Span::raw("  "),
        Span::styled(msg::PROMPT_NEEDS_PASSWORD, theme::muted()),
    ]);

    let password_focused = state.focus == PasswordPromptField::Password;
    let mut password_row = vec![
        label_span(msg::PROMPT_FIELD_PASSWORD, password_focused),
        Span::styled("*".repeat(state.input.chars().count()), theme::bold()),
    ];
    if password_focused {
        password_row.push(Span::styled("\u{2588}", theme::accent_bold()));
    }

    let remember_value = if state.remember {
        msg::PROMPT_REMEMBER_ON
    } else {
        msg::PROMPT_REMEMBER_OFF
    };
    let remember_row = vec![
        label_span(
            msg::PROMPT_FIELD_REMEMBER,
            state.focus == PasswordPromptField::Remember,
        ),
        Span::styled(if state.remember { "[x] " } else { "[ ] " }, theme::bold()),
        Span::styled(remember_value, theme::muted()),
    ];

    let text = vec![
        Line::from(""),
        identity,
        Line::from(""),
        Line::from(password_row),
        Line::from(""),
        Line::from(remember_row),
    ];
    design::render_body(frame, area, block, text);

    let footer_area = design::render_overlay_footer(frame, area);
    let footer = design::Footer::new()
        .primary("Enter", crate::i18n::text(fl::ENTER_CONTINUE))
        .action("Tab", crate::i18n::text(fl::TAB_NEXT))
        .action("Space", crate::i18n::text(fl::SPACE_TOGGLE))
        .action("Esc", crate::i18n::text(fl::ESC_CANCEL))
        .into_spans();
    frame.render_widget(Paragraph::new(Line::from(footer)), footer_area);
}

/// The PIN variant: the key the PIN belongs to, the masked PIN and a hint to
/// touch the key. A PIN never goes to the keychain, so there is no toggle.
fn render_security_key_pin(frame: &mut Frame, alias: &str, key_label: &str, pin_len: usize) {
    let area = super::centered_rect_fixed(PROMPT_W, PIN_PROMPT_H, frame.area());
    frame.render_widget(Clear, area);
    let block = design::overlay_block(msg::PIN_PROMPT_TITLE);

    let identity = Line::from(vec![
        Span::raw("  "),
        Span::styled(alias.to_string(), theme::bold()),
        Span::raw("  "),
        Span::styled(msg::PIN_PROMPT_NEEDS_PIN, theme::muted()),
    ]);
    let key_row = Line::from(vec![
        label_span(msg::PIN_FIELD_KEY, false),
        Span::styled(key_label.to_string(), theme::bold()),
    ]);
    let pin_row = Line::from(vec![
        label_span(msg::PIN_FIELD_PIN, true),
        Span::styled("*".repeat(pin_len), theme::bold()),
        Span::styled("\u{2588}", theme::accent_bold()),
    ]);
    let hint = Line::from(vec![
        Span::raw("  "),
        Span::styled(msg::PIN_TOUCH_HINT, theme::muted()),
    ]);

    let text = vec![
        Line::from(""),
        identity,
        Line::from(""),
        key_row,
        pin_row,
        Line::from(""),
        hint,
    ];
    design::render_body(frame, area, block, text);

    let footer_area = design::render_overlay_footer(frame, area);
    let footer = design::Footer::new()
        .primary("Enter", crate::i18n::text(fl::ENTER_CONTINUE))
        .action("Esc", crate::i18n::text(fl::ESC_CANCEL))
        .into_spans();
    frame.render_widget(Paragraph::new(Line::from(footer)), footer_area);
}

/// Left-hand field label, padded to `LABEL_W` so both rows align. The
/// focused row reads in the brand accent; the other stays muted.
fn label_span(label: &str, focused: bool) -> Span<'static> {
    let style = if focused {
        theme::accent_bold()
    } else {
        theme::muted()
    };
    Span::styled(format!("  {:<width$}", label, width = LABEL_W), style)
}
