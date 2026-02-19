use ratatui::style::{Color, Modifier, Style};

/// Brand accent: used ONLY for titles. Magenta+Bold triggers bright magenta.
pub fn brand() -> Style {
    Style::default()
        .fg(Color::Magenta)
        .add_modifier(Modifier::BOLD)
}

/// Primary accent: structural elements (borders, focus indicators).
pub fn accent() -> Style {
    Style::default().fg(Color::Cyan)
}

/// Primary accent with bold: keybinding keys in footer/help.
pub fn accent_bold() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

/// Primary action key (connect/Enter) — stands out from secondary keys.
pub fn primary_action() -> Style {
    Style::default()
        .fg(Color::Green)
        .add_modifier(Modifier::BOLD)
}

/// Muted/secondary text. Uses DIM instead of DarkGray for theme safety.
pub fn muted() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

/// Section headers (help overlay).
pub fn section_header() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

/// Selected item in a list — REVERSED is universally visible.
pub fn selected() -> Style {
    Style::default().add_modifier(Modifier::REVERSED)
}

/// Error message.
pub fn error() -> Style {
    Style::default()
        .fg(Color::Red)
        .add_modifier(Modifier::BOLD)
}

/// Success message.
pub fn success() -> Style {
    Style::default()
        .fg(Color::Green)
        .add_modifier(Modifier::BOLD)
}

/// Danger action key (delete "y").
pub fn danger() -> Style {
    Style::default()
        .fg(Color::Red)
        .add_modifier(Modifier::BOLD)
}

/// Default border (unfocused).
pub fn border() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

/// Focused border.
pub fn border_focused() -> Style {
    Style::default().fg(Color::Cyan)
}

/// Danger border (delete dialog).
pub fn border_danger() -> Style {
    Style::default().fg(Color::Red)
}

/// Bold text (labels, emphasis).
pub fn bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}
