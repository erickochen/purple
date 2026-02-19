use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event as CrosstermEvent, KeyEvent, KeyEventKind};

/// Application events.
pub enum AppEvent {
    Key(KeyEvent),
    Tick,
}

/// Polls crossterm events in a background thread.
pub struct EventHandler {
    rx: mpsc::Receiver<AppEvent>,
    // Keep the thread handle alive
    _handle: thread::JoinHandle<()>,
}

impl EventHandler {
    pub fn new(tick_rate_ms: u64) -> Self {
        let (tx, rx) = mpsc::channel();
        let tick_rate = Duration::from_millis(tick_rate_ms);

        let handle = thread::spawn(move || {
            let mut last_tick = Instant::now();
            loop {
                let timeout = tick_rate
                    .checked_sub(last_tick.elapsed())
                    .unwrap_or(Duration::ZERO);

                if event::poll(timeout).unwrap_or(false) {
                    if let Ok(evt) = event::read() {
                        match evt {
                            CrosstermEvent::Key(key) if key.kind == KeyEventKind::Press => {
                                if tx.send(AppEvent::Key(key)).is_err() {
                                    return;
                                }
                            }
                            _ => {}
                        }
                    }
                }

                if last_tick.elapsed() >= tick_rate {
                    if tx.send(AppEvent::Tick).is_err() {
                        return;
                    }
                    last_tick = Instant::now();
                }
            }
        });

        Self {
            rx,
            _handle: handle,
        }
    }

    /// Get the next event (blocks until available).
    pub fn next(&self) -> Result<AppEvent> {
        Ok(self.rx.recv()?)
    }
}
