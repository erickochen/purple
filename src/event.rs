use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event as CrosstermEvent, KeyEvent, KeyEventKind};

/// Application events.
pub enum AppEvent {
    Key(KeyEvent),
    Tick,
    PingResult { alias: String, reachable: bool },
    PollError,
}

/// Polls crossterm events in a background thread.
pub struct EventHandler {
    tx: mpsc::Sender<AppEvent>,
    rx: mpsc::Receiver<AppEvent>,
    paused: Arc<AtomicBool>,
    // Keep the thread handle alive
    _handle: thread::JoinHandle<()>,
}

impl EventHandler {
    pub fn new(tick_rate_ms: u64) -> Self {
        let (tx, rx) = mpsc::channel();
        let tick_rate = Duration::from_millis(tick_rate_ms);
        let event_tx = tx.clone();
        let paused = Arc::new(AtomicBool::new(false));
        let paused_flag = paused.clone();

        let handle = thread::spawn(move || {
            let mut last_tick = Instant::now();
            loop {
                // When paused, sleep instead of polling stdin
                if paused_flag.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(50));
                    continue;
                }

                // Cap poll timeout at 50ms so we notice pause flag quickly
                let remaining = tick_rate
                    .checked_sub(last_tick.elapsed())
                    .unwrap_or(Duration::ZERO);
                let timeout = remaining.min(Duration::from_millis(50));

                match event::poll(timeout) {
                    Ok(true) => {
                        if let Ok(evt) = event::read() {
                            match evt {
                                CrosstermEvent::Key(key)
                                    if key.kind == KeyEventKind::Press =>
                                {
                                    if event_tx.send(AppEvent::Key(key)).is_err() {
                                        return;
                                    }
                                }
                                CrosstermEvent::Resize(..) => {
                                    // Trigger immediate redraw on terminal resize
                                    if event_tx.send(AppEvent::Tick).is_err() {
                                        return;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Ok(false) => {}
                    Err(_) => {
                        // Poll error (e.g. stdin closed). Notify main loop and exit.
                        let _ = event_tx.send(AppEvent::PollError);
                        return;
                    }
                }

                if last_tick.elapsed() >= tick_rate {
                    if event_tx.send(AppEvent::Tick).is_err() {
                        return;
                    }
                    last_tick = Instant::now();
                }
            }
        });

        Self {
            tx,
            rx,
            paused,
            _handle: handle,
        }
    }

    /// Get the next event (blocks until available).
    pub fn next(&self) -> Result<AppEvent> {
        Ok(self.rx.recv()?)
    }

    /// Get a clone of the sender for sending events from other threads.
    pub fn sender(&self) -> mpsc::Sender<AppEvent> {
        self.tx.clone()
    }

    /// Pause event polling (call before spawning SSH).
    pub fn pause(&self) {
        self.paused.store(true, Ordering::Release);
    }

    /// Resume event polling (call after SSH exits).
    pub fn resume(&self) {
        // Drain stale events, but keep PingResult events
        let mut ping_results = Vec::new();
        while let Ok(event) = self.rx.try_recv() {
            if let AppEvent::PingResult { alias, reachable } = event {
                ping_results.push(AppEvent::PingResult { alias, reachable });
            }
        }
        // Re-send preserved PingResult events
        for event in ping_results {
            let _ = self.tx.send(event);
        }
        self.paused.store(false, Ordering::Release);
    }
}
