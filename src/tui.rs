use std::io::{self, Stdout, stdout};
use std::sync::Once;

use anyhow::Result;
use crossterm::{
    ExecutableCommand,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, prelude::CrosstermBackend};

use crate::app::App;
use crate::ui;

static PANIC_HOOK: Once = Once::new();

pub struct Tui {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl Tui {
    pub fn new() -> Result<Self> {
        let backend = CrosstermBackend::new(stdout());
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }

    /// Enter TUI mode: panic hook (installed once), raw mode, alternate screen.
    pub fn enter(&mut self) -> Result<()> {
        // Install panic hook BEFORE enabling raw mode to ensure cleanup on panic
        PANIC_HOOK.call_once(|| {
            let original_hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |panic_info| {
                Self::reset().expect("Failed to reset terminal");
                original_hook(panic_info);
            }));
        });

        enable_raw_mode()?;
        if let Err(e) = io::stdout().execute(EnterAlternateScreen) {
            disable_raw_mode()?;
            return Err(e.into());
        }

        self.terminal.hide_cursor()?;
        self.terminal.clear()?;
        Ok(())
    }

    /// Exit TUI mode: restore terminal.
    pub fn exit(&mut self) -> Result<()> {
        Self::reset()?;
        self.terminal.show_cursor()?;
        Ok(())
    }

    /// Reset terminal to normal mode.
    fn reset() -> Result<()> {
        disable_raw_mode()?;
        io::stdout().execute(LeaveAlternateScreen)?;
        Ok(())
    }

    /// Draw the UI.
    pub fn draw(&mut self, app: &mut App) -> Result<()> {
        self.terminal.draw(|frame| ui::render(frame, app))?;
        Ok(())
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = Self::reset();
        let _ = self.terminal.show_cursor();
    }
}
