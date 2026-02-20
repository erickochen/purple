use std::sync::atomic::{AtomicBool, Ordering};

use ratatui::style::{Color, Modifier, Style};

static NO_COLOR_FLAG: AtomicBool = AtomicBool::new(false);

/// Initialize theme settings. Call once at startup.
pub fn init() {
    if std::env::var_os("NO_COLOR").is_some() {
        NO_COLOR_FLAG.store(true, Ordering::Release);
    }
}

/// Whether NO_COLOR is active (strip fg/bg colors, keep modifiers).
fn nc() -> bool {
    NO_COLOR_FLAG.load(Ordering::Acquire)
}

/// Apply fg color only when NO_COLOR is not set.
fn with_fg(style: Style, color: Color) -> Style {
    if nc() { style } else { style.fg(color) }
}

/// Brand accent: Magenta+Bold for dialog/popup titles.
pub fn brand() -> Style {
    with_fg(Style::default().add_modifier(Modifier::BOLD), Color::Magenta)
}

/// Brand badge: reversed chip for main screen titles.
/// Uses REVERSED without color so it always swaps the terminal's own fg/bg,
/// guaranteeing high contrast on every theme.
pub fn brand_badge() -> Style {
    Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
}

/// Primary accent: structural elements (borders, focus indicators).
pub fn accent() -> Style {
    with_fg(Style::default(), Color::Magenta)
}

/// Primary accent with bold: keybinding keys in footer/help.
pub fn accent_bold() -> Style {
    with_fg(
        Style::default().add_modifier(Modifier::BOLD),
        Color::Magenta,
    )
}

/// Search match highlight (secondary accent, Cyan for visual contrast).
pub fn highlight_bold() -> Style {
    with_fg(
        Style::default().add_modifier(Modifier::BOLD),
        Color::Cyan,
    )
}

/// Primary action key (connect/Enter) — stands out from secondary keys.
pub fn primary_action() -> Style {
    with_fg(
        Style::default().add_modifier(Modifier::BOLD),
        Color::Yellow,
    )
}

/// Muted/secondary text. Uses DIM instead of DarkGray for theme safety.
pub fn muted() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

/// Section headers (help overlay, host detail).
pub fn section_header() -> Style {
    with_fg(
        Style::default().add_modifier(Modifier::BOLD),
        Color::Blue,
    )
}

/// Selected item in a list — REVERSED is universally visible.
pub fn selected() -> Style {
    Style::default().add_modifier(Modifier::REVERSED)
}

/// Error message.
pub fn error() -> Style {
    with_fg(
        Style::default().add_modifier(Modifier::BOLD),
        Color::Red,
    )
}

/// Success message.
pub fn success() -> Style {
    with_fg(
        Style::default().add_modifier(Modifier::BOLD),
        Color::Green,
    )
}

/// Danger action key (delete "y").
pub fn danger() -> Style {
    with_fg(
        Style::default().add_modifier(Modifier::BOLD),
        Color::Red,
    )
}

/// Default border (unfocused).
pub fn border() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

/// Focused border.
pub fn border_focused() -> Style {
    with_fg(
        Style::default().add_modifier(Modifier::BOLD),
        Color::Magenta,
    )
}

/// Danger border (delete dialog).
pub fn border_danger() -> Style {
    with_fg(Style::default(), Color::Red)
}

/// Bold text (labels, emphasis).
pub fn bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}
