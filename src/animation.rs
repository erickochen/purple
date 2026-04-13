use std::time::Instant;

use ratatui::buffer::Buffer;

use crate::app::{App, PingStatus, Screen};

/// Braille spinner sequence for ping Checking status.
pub const SPINNER_FRAMES: &[&str] = &[
    "\u{280B}", // ⠋
    "\u{2819}", // ⠙
    "\u{2839}", // ⠹
    "\u{2838}", // ⠸
    "\u{283C}", // ⠼
    "\u{2834}", // ⠴
    "\u{2826}", // ⠦
    "\u{2827}", // ⠧
    "\u{2807}", // ⠇
    "\u{280F}", // ⠏
];

/// Detail panel animation duration in milliseconds.
const DETAIL_ANIM_DURATION_MS: u128 = 200;

/// Overlay animation duration in milliseconds.
const OVERLAY_ANIM_DURATION_MS: u128 = 250;

/// Welcome overlay animation duration in milliseconds.
const WELCOME_ANIM_DURATION_MS: u128 = 350;

/// Active detail panel width animation.
pub(crate) struct DetailAnim {
    start: Instant,
    opening: bool,
    start_progress: f32,
}

/// Active overlay open/close animation.
pub(crate) struct OverlayAnim {
    pub(crate) start: Instant,
    pub(crate) opening: bool,
    pub(crate) duration_ms: u128,
}

/// Captured overlay state for close animation. Bundles the buffer snapshot with the
/// dim flag so they are always in sync (the close animation knows whether to dim).
pub(crate) struct OverlayCloseState {
    pub(crate) buffer: Buffer,
    pub(crate) dimmed: bool,
}

/// Animation state kept separate from App (render-layer concern).
pub struct AnimationState {
    pub spinner_tick: u64,
    pub(crate) prev_was_overlay: bool,
    pub(crate) detail_anim: Option<DetailAnim>,
    pub(crate) overlay_anim: Option<OverlayAnim>,
    /// Saved overlay state for close animation (captured once when overlay is stable).
    pub(crate) overlay_close: Option<OverlayCloseState>,
}

impl AnimationState {
    pub fn new() -> Self {
        Self {
            spinner_tick: 0,
            prev_was_overlay: false,
            detail_anim: None,
            overlay_anim: None,
            overlay_close: None,
        }
    }

    /// Whether any animation is running.
    pub fn is_animating(&self, app: &App) -> bool {
        let welcome_animating = app
            .welcome_opened
            .is_some_and(|t| t.elapsed().as_millis() < 3000);
        self.detail_anim.is_some() || self.overlay_anim.is_some() || welcome_animating
    }

    /// Whether any host has PingStatus::Checking (spinner needs ticking).
    pub fn has_checking_hosts(&self, app: &App) -> bool {
        app.ping
            .status
            .values()
            .any(|s| matches!(s, PingStatus::Checking))
    }

    /// Advance spinner tick. Called from main loop at ~80ms intervals.
    pub fn tick_spinner(&mut self) {
        self.spinner_tick = self.spinner_tick.wrapping_add(1);
    }

    /// Current overlay animation progress (0.0 = hidden, 1.0 = fully visible).
    pub fn overlay_anim_progress(&self) -> Option<f32> {
        let anim = self.overlay_anim.as_ref()?;
        let elapsed = anim.start.elapsed().as_millis();
        if elapsed >= anim.duration_ms {
            return None;
        }
        let t = elapsed as f32 / anim.duration_ms as f32;
        let eased = 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t);
        Some(if anim.opening { eased } else { 1.0 - eased })
    }

    /// Tick overlay animation: clean up when complete.
    pub fn tick_overlay_anim(&mut self) {
        if self.overlay_anim.is_some() && self.overlay_anim_progress().is_none() {
            let was_closing = self.overlay_anim.as_ref().is_some_and(|a| !a.opening);
            self.overlay_anim = None;
            if was_closing {
                self.overlay_close = None;
            }
        }
    }

    /// Current detail panel animation progress (0.0 = closed, 1.0 = open).
    pub fn detail_anim_progress(&mut self) -> Option<f32> {
        let anim = self.detail_anim.as_ref()?;
        let elapsed = anim.start.elapsed().as_millis();
        if elapsed >= DETAIL_ANIM_DURATION_MS {
            self.detail_anim = None;
            return None;
        }
        let t = elapsed as f32 / DETAIL_ANIM_DURATION_MS as f32;
        let eased = 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t);
        let progress = if anim.opening {
            anim.start_progress + (1.0 - anim.start_progress) * eased
        } else {
            anim.start_progress * (1.0 - eased)
        };
        Some(progress)
    }

    /// Detect overlay open/close transitions and start animations.
    pub fn detect_transitions(&mut self, app: &mut App) {
        let is_overlay = !matches!(app.screen, Screen::HostList);

        if is_overlay && !self.prev_was_overlay {
            let is_welcome = matches!(app.screen, Screen::Welcome { .. });
            if is_welcome {
                app.welcome_opened = Some(Instant::now());
            }
            self.overlay_anim = Some(OverlayAnim {
                start: Instant::now(),
                opening: true,
                duration_ms: if is_welcome {
                    WELCOME_ANIM_DURATION_MS
                } else {
                    OVERLAY_ANIM_DURATION_MS
                },
            });
        } else if !is_overlay && self.prev_was_overlay {
            if self.overlay_close.is_some() {
                self.overlay_anim = Some(OverlayAnim {
                    start: Instant::now(),
                    opening: false,
                    duration_ms: OVERLAY_ANIM_DURATION_MS,
                });
            }
            app.welcome_opened = None;
        }

        // Detail panel toggle
        if app.detail_toggle_pending {
            app.detail_toggle_pending = false;
            let opening = app.view_mode == crate::app::ViewMode::Detailed;
            let start_progress =
                self.detail_anim_progress()
                    .unwrap_or(if opening { 0.0 } else { 1.0 });
            self.detail_anim = Some(DetailAnim {
                start: Instant::now(),
                opening,
                start_progress,
            });
        }

        self.prev_was_overlay = is_overlay;
    }
}

impl Default for AnimationState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::*;

    fn make_app() -> App {
        use std::path::PathBuf;
        let config = crate::ssh_config::model::SshConfigFile {
            elements: crate::ssh_config::model::SshConfigFile::parse_content(""),
            path: PathBuf::from("/tmp/test_config"),
            crlf: false,
            bom: false,
        };
        App::new(config)
    }

    // --- Spinner tests ---

    #[test]
    fn spinner_frames_are_10() {
        assert_eq!(SPINNER_FRAMES.len(), 10);
    }

    #[test]
    fn spinner_frames_cycle_via_index() {
        assert_eq!(SPINNER_FRAMES[0], "\u{280B}");
        assert_eq!(SPINNER_FRAMES[1], "\u{2819}");
        assert_eq!(SPINNER_FRAMES[10 % SPINNER_FRAMES.len()], "\u{280B}");
    }

    #[test]
    fn spinner_frames_at_u64_max() {
        let idx = (u64::MAX as usize) % SPINNER_FRAMES.len();
        assert_eq!(SPINNER_FRAMES[idx], "\u{2834}");
    }

    #[test]
    fn spinner_tick_wraps() {
        let mut anim = AnimationState::new();
        anim.spinner_tick = u64::MAX;
        anim.tick_spinner();
        assert_eq!(anim.spinner_tick, 0);
    }

    #[test]
    fn spinner_tick_increments_by_one() {
        let mut anim = AnimationState::new();
        assert_eq!(anim.spinner_tick, 0);
        anim.tick_spinner();
        assert_eq!(anim.spinner_tick, 1);
    }

    // --- is_animating tests ---

    #[test]
    fn new_state_not_animating() {
        let app = make_app();
        let anim = AnimationState::new();
        assert!(!anim.is_animating(&app));
    }

    #[test]
    fn is_animating_with_overlay_anim() {
        let mut app = make_app();
        let mut anim = AnimationState::new();
        app.screen = Screen::Help {
            return_screen: Box::new(Screen::HostList),
        };
        anim.detect_transitions(&mut app);
        assert!(anim.is_animating(&app));
    }

    #[test]
    fn is_animating_with_detail_anim() {
        let mut app = make_app();
        let mut anim = AnimationState::new();
        app.detail_toggle_pending = true;
        app.view_mode = crate::app::ViewMode::Detailed;
        anim.detect_transitions(&mut app);
        assert!(anim.is_animating(&app));
    }

    // --- has_checking_hosts tests ---

    #[test]
    fn has_checking_hosts_empty() {
        let app = make_app();
        let anim = AnimationState::new();
        assert!(!anim.has_checking_hosts(&app));
    }

    #[test]
    fn has_checking_hosts_only_reachable() {
        let mut app = make_app();
        app.ping
            .status
            .insert("host1".to_string(), PingStatus::Reachable { rtt_ms: 10 });
        app.ping
            .status
            .insert("host2".to_string(), PingStatus::Unreachable);
        let anim = AnimationState::new();
        assert!(!anim.has_checking_hosts(&app));
    }

    #[test]
    fn has_checking_hosts_with_checking() {
        let mut app = make_app();
        app.ping
            .status
            .insert("host2".to_string(), PingStatus::Checking);
        let anim = AnimationState::new();
        assert!(anim.has_checking_hosts(&app));
    }

    // --- overlay animation tests ---

    #[test]
    fn detect_transitions_opens_overlay() {
        let mut app = make_app();
        let mut anim = AnimationState::new();
        app.screen = Screen::Help {
            return_screen: Box::new(Screen::HostList),
        };
        anim.detect_transitions(&mut app);
        assert!(anim.prev_was_overlay);
        assert!(anim.overlay_anim.is_some());
        assert!(anim.overlay_anim.as_ref().unwrap().opening);
    }

    #[test]
    fn detect_transitions_closes_overlay() {
        let mut app = make_app();
        let mut anim = AnimationState::new();
        app.screen = Screen::Help {
            return_screen: Box::new(Screen::HostList),
        };
        anim.detect_transitions(&mut app);
        // Simulate saved buffer
        anim.overlay_close = Some(OverlayCloseState {
            buffer: Buffer::empty(Rect::new(0, 0, 80, 24)),
            dimmed: true,
        });

        app.screen = Screen::HostList;
        anim.detect_transitions(&mut app);
        assert!(!anim.prev_was_overlay);
        assert!(anim.overlay_anim.is_some());
        assert!(!anim.overlay_anim.as_ref().unwrap().opening);
    }

    #[test]
    fn overlay_close_without_buffer_skips_anim() {
        let mut app = make_app();
        let mut anim = AnimationState::new();
        app.screen = Screen::Help {
            return_screen: Box::new(Screen::HostList),
        };
        anim.detect_transitions(&mut app);
        // No overlay_buffer saved

        app.screen = Screen::HostList;
        anim.detect_transitions(&mut app);
        // No close animation without a saved buffer
        assert!(anim.overlay_anim.is_none() || anim.overlay_anim.as_ref().unwrap().opening);
    }

    #[test]
    fn overlay_anim_progress_returns_value() {
        let mut app = make_app();
        let mut anim = AnimationState::new();
        app.screen = Screen::Help {
            return_screen: Box::new(Screen::HostList),
        };
        anim.detect_transitions(&mut app);
        let progress = anim.overlay_anim_progress();
        assert!(progress.is_some());
        assert!((0.0..=1.0).contains(&progress.unwrap()));
    }

    #[test]
    fn tick_overlay_anim_clears_on_completion() {
        let mut app = make_app();
        let mut anim = AnimationState::new();
        app.screen = Screen::Help {
            return_screen: Box::new(Screen::HostList),
        };
        anim.detect_transitions(&mut app);
        // Fast-forward
        anim.overlay_anim.as_mut().unwrap().start =
            Instant::now() - std::time::Duration::from_millis(500);
        anim.tick_overlay_anim();
        assert!(anim.overlay_anim.is_none());
    }

    #[test]
    fn tick_overlay_close_clears_buffer() {
        let mut app = make_app();
        let mut anim = AnimationState::new();
        app.screen = Screen::Help {
            return_screen: Box::new(Screen::HostList),
        };
        anim.detect_transitions(&mut app);
        anim.overlay_close = Some(OverlayCloseState {
            buffer: Buffer::empty(Rect::new(0, 0, 80, 24)),
            dimmed: true,
        });

        // Close
        app.screen = Screen::HostList;
        anim.detect_transitions(&mut app);
        // Fast-forward close
        anim.overlay_anim.as_mut().unwrap().start =
            Instant::now() - std::time::Duration::from_millis(500);
        anim.tick_overlay_anim();
        assert!(anim.overlay_anim.is_none());
        assert!(anim.overlay_close.is_none());
    }

    #[test]
    fn detect_transitions_stable_hostlist_no_anim() {
        let mut app = make_app();
        let mut anim = AnimationState::new();
        anim.detect_transitions(&mut app);
        anim.detect_transitions(&mut app);
        assert!(!anim.prev_was_overlay);
        assert!(anim.overlay_anim.is_none());
    }

    #[test]
    fn detect_transitions_welcome_sets_welcome_opened() {
        let mut app = make_app();
        let mut anim = AnimationState::new();
        app.screen = Screen::Welcome {
            has_backup: false,
            host_count: 0,
            known_hosts_count: 0,
        };
        anim.detect_transitions(&mut app);
        assert!(app.welcome_opened.is_some());
        assert_eq!(
            anim.overlay_anim.as_ref().unwrap().duration_ms,
            WELCOME_ANIM_DURATION_MS
        );
    }

    #[test]
    fn detect_transitions_welcome_close_clears_welcome_opened() {
        let mut app = make_app();
        let mut anim = AnimationState::new();
        app.screen = Screen::Welcome {
            has_backup: false,
            host_count: 0,
            known_hosts_count: 0,
        };
        anim.detect_transitions(&mut app);
        app.screen = Screen::HostList;
        anim.detect_transitions(&mut app);
        assert!(app.welcome_opened.is_none());
    }

    #[test]
    fn close_non_welcome_overlay_clears_welcome_opened() {
        let mut app = make_app();
        let mut anim = AnimationState::new();
        app.welcome_opened = Some(Instant::now());
        app.screen = Screen::Help {
            return_screen: Box::new(Screen::HostList),
        };
        anim.detect_transitions(&mut app);
        app.screen = Screen::HostList;
        anim.detect_transitions(&mut app);
        assert!(app.welcome_opened.is_none());
    }

    // --- detail animation tests ---

    #[test]
    fn detail_toggle_open_starts_anim() {
        let mut app = make_app();
        let mut anim = AnimationState::new();
        app.detail_toggle_pending = true;
        app.view_mode = crate::app::ViewMode::Detailed;
        anim.detect_transitions(&mut app);
        assert!(!app.detail_toggle_pending);
        assert!(anim.detail_anim.is_some());
    }

    #[test]
    fn detail_toggle_close_starts_anim() {
        let mut app = make_app();
        let mut anim = AnimationState::new();
        app.detail_toggle_pending = true;
        app.view_mode = crate::app::ViewMode::Compact;
        anim.detect_transitions(&mut app);
        assert!(anim.detail_anim.is_some());
    }

    #[test]
    fn detail_anim_progress_returns_value() {
        let mut app = make_app();
        let mut anim = AnimationState::new();
        app.detail_toggle_pending = true;
        app.view_mode = crate::app::ViewMode::Detailed;
        anim.detect_transitions(&mut app);
        let p = anim.detail_anim_progress();
        assert!(p.is_some());
        assert!((0.0..=1.0).contains(&p.unwrap()));
    }

    #[test]
    fn detail_anim_progress_none_when_no_anim() {
        let mut anim = AnimationState::new();
        assert!(anim.detail_anim_progress().is_none());
    }

    #[test]
    fn detail_anim_completes_and_clears() {
        let mut app = make_app();
        let mut anim = AnimationState::new();
        app.detail_toggle_pending = true;
        app.view_mode = crate::app::ViewMode::Detailed;
        anim.detect_transitions(&mut app);
        anim.detail_anim.as_mut().unwrap().start =
            Instant::now() - std::time::Duration::from_millis(300);
        assert!(anim.detail_anim_progress().is_none());
        assert!(anim.detail_anim.is_none());
    }

    #[test]
    fn detail_anim_reversal_mid_flight() {
        let mut app = make_app();
        let mut anim = AnimationState::new();
        app.detail_toggle_pending = true;
        app.view_mode = crate::app::ViewMode::Detailed;
        anim.detect_transitions(&mut app);
        let _ = anim.detail_anim_progress();

        app.detail_toggle_pending = true;
        app.view_mode = crate::app::ViewMode::Compact;
        anim.detect_transitions(&mut app);
        assert!(anim.detail_anim.is_some());
        assert!(!anim.detail_anim.as_ref().unwrap().opening);
    }

    #[test]
    fn detail_anim_independent_of_overlay() {
        let mut app = make_app();
        let mut anim = AnimationState::new();
        app.detail_toggle_pending = true;
        app.view_mode = crate::app::ViewMode::Detailed;
        app.screen = Screen::Help {
            return_screen: Box::new(Screen::HostList),
        };
        anim.detect_transitions(&mut app);
        assert!(anim.detail_anim.is_some());
        assert!(anim.overlay_anim.is_some());
    }

    #[test]
    fn overlay_close_state_initially_none() {
        let anim = AnimationState::new();
        assert!(anim.overlay_close.is_none());
    }
}
