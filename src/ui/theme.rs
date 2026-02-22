use std::sync::atomic::{AtomicU8, Ordering};

use ratatui::style::{Color, Modifier, Style};

/// Color mode: 0 = NO_COLOR, 1 = ANSI 16, 2 = truecolor.
static COLOR_MODE: AtomicU8 = AtomicU8::new(1);

/// Initialize theme settings. Call once at startup.
pub fn init() {
    if std::env::var_os("NO_COLOR").is_some() {
        COLOR_MODE.store(0, Ordering::Release);
    } else if std::env::var("COLORTERM")
        .map(|v| v == "truecolor" || v == "24bit")
        .unwrap_or(false)
    {
        COLOR_MODE.store(2, Ordering::Release);
    }
}

/// Brand badge: purple background with white text. The single splash of color.
/// Truecolor: #9333EA purple bg. ANSI 16: Magenta bg. NO_COLOR: REVERSED.
/// Removes DIM so border_style doesn't leak through ratatui's Style::patch().
pub fn brand_badge() -> Style {
    match COLOR_MODE.load(Ordering::Acquire) {
        0 => Style::default()
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            .remove_modifier(Modifier::DIM),
        2 => Style::default()
            .fg(Color::White)
            .bg(Color::Rgb(147, 51, 234))
            .add_modifier(Modifier::BOLD)
            .remove_modifier(Modifier::DIM),
        _ => Style::default()
            .fg(Color::White)
            .bg(Color::Magenta)
            .add_modifier(Modifier::BOLD)
            .remove_modifier(Modifier::DIM),
    }
}

/// Brand accent for dialog/popup titles.
pub fn brand() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// Structural elements (overlay borders, tags).
pub fn accent() -> Style {
    Style::default()
}

/// Keybinding keys in footer/help.
pub fn accent_bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// Search match highlight.
pub fn highlight_bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
}

/// Primary action key (connect/Enter).
pub fn primary_action() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// Muted/secondary text.
pub fn muted() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

/// Section headers (help overlay, host detail).
pub fn section_header() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// Selected item in a list.
pub fn selected() -> Style {
    Style::default().add_modifier(Modifier::REVERSED)
}

/// Error message.
pub fn error() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// Success message.
pub fn success() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// Danger action key (delete confirmation).
pub fn danger() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// Default border (unfocused).
pub fn border() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

/// Focused border.
pub fn border_focused() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// Danger border (delete dialog).
pub fn border_danger() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// Bold text (labels, emphasis).
pub fn bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}
