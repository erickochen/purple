use std::collections::HashMap;

use crate::app::PingStatus;

/// Ping/health-check state for all hosts.
pub struct PingState {
    pub status: HashMap<String, PingStatus>,
    pub has_pinged: bool,
    pub generation: u64,
    pub slow_threshold_ms: u16,
    pub auto_ping: bool,
    pub filter_down_only: bool,
    pub checked_at: Option<std::time::Instant>,
}

impl Default for PingState {
    fn default() -> Self {
        Self {
            status: HashMap::new(),
            has_pinged: false,
            generation: 0,
            slow_threshold_ms: 500,
            auto_ping: false,
            filter_down_only: false,
            checked_at: None,
        }
    }
}
