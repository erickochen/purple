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
    PingResult {
        alias: String,
        rtt_ms: Option<u32>,
        generation: u64,
    },
    SyncComplete {
        provider: String,
        hosts: Vec<crate::providers::ProviderHost>,
    },
    SyncPartial {
        provider: String,
        hosts: Vec<crate::providers::ProviderHost>,
        failures: usize,
        total: usize,
    },
    SyncError {
        provider: String,
        message: String,
    },
    SyncProgress {
        provider: String,
        message: String,
    },
    UpdateAvailable {
        version: String,
        headline: Option<String>,
    },
    FileBrowserListing {
        alias: String,
        path: String,
        entries: Result<Vec<crate::file_browser::FileEntry>, String>,
    },
    ScpComplete {
        alias: String,
        success: bool,
        message: String,
    },
    SnippetHostDone {
        run_id: u64,
        alias: String,
        stdout: String,
        stderr: String,
        exit_code: Option<i32>,
    },
    SnippetAllDone {
        run_id: u64,
    },
    SnippetProgress {
        run_id: u64,
        completed: usize,
        total: usize,
    },
    ContainerListing {
        alias: String,
        result: Result<
            (
                crate::containers::ContainerRuntime,
                Vec<crate::containers::ContainerInfo>,
            ),
            crate::containers::ContainerError,
        >,
    },
    ContainerActionComplete {
        alias: String,
        action: crate::containers::ContainerAction,
        result: Result<(), String>,
    },
    VaultSignResult {
        alias: String,
        /// Snapshot of the host's `CertificateFile` directive at signing time.
        /// Carried in the event so the main loop never has to re-look up the
        /// host (which would be O(n) and racy under concurrent renames). Empty
        /// when the host has no `CertificateFile` set; `should_write_certificate_file`
        /// uses this directly to decide whether to write a default directive.
        certificate_file: String,
        success: bool,
        message: String,
    },
    VaultSignProgress {
        alias: String,
        done: usize,
        total: usize,
    },
    VaultSignAllDone {
        signed: u32,
        failed: u32,
        skipped: u32,
        cancelled: bool,
        aborted_message: Option<String>,
        first_error: Option<String>,
    },
    CertCheckResult {
        alias: String,
        status: crate::vault_ssh::CertStatus,
    },
    CertCheckError {
        alias: String,
        message: String,
    },
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
                                CrosstermEvent::Key(key) if key.kind == KeyEventKind::Press => {
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

    /// Try to get the next event with a timeout.
    pub fn next_timeout(&self, timeout: Duration) -> Result<Option<AppEvent>> {
        match self.rx.recv_timeout(timeout) {
            Ok(event) => Ok(Some(event)),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(anyhow::anyhow!("event channel disconnected"))
            }
        }
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
        // Drain stale events, but keep background result events
        let mut preserved = Vec::new();
        while let Ok(event) = self.rx.try_recv() {
            match event {
                AppEvent::PingResult { .. }
                | AppEvent::SyncComplete { .. }
                | AppEvent::SyncPartial { .. }
                | AppEvent::SyncError { .. }
                | AppEvent::SyncProgress { .. }
                | AppEvent::UpdateAvailable { .. }
                | AppEvent::FileBrowserListing { .. }
                | AppEvent::ScpComplete { .. }
                | AppEvent::SnippetHostDone { .. }
                | AppEvent::SnippetAllDone { .. }
                | AppEvent::SnippetProgress { .. }
                | AppEvent::ContainerListing { .. }
                | AppEvent::ContainerActionComplete { .. }
                | AppEvent::VaultSignResult { .. }
                | AppEvent::VaultSignProgress { .. }
                | AppEvent::VaultSignAllDone { .. }
                | AppEvent::CertCheckResult { .. }
                | AppEvent::CertCheckError { .. } => preserved.push(event),
                _ => {}
            }
        }
        // Re-send preserved events
        for event in preserved {
            let _ = self.tx.send(event);
        }
        self.paused.store(false, Ordering::Release);
    }
}
