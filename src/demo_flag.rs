use std::sync::atomic::{AtomicBool, Ordering};

/// Global flag checked by all disk-write functions.
static DEMO_MODE: AtomicBool = AtomicBool::new(false);

/// Returns true if demo mode is active (no disk writes).
pub fn is_demo() -> bool {
    DEMO_MODE.load(Ordering::Relaxed)
}

/// Enable demo mode. Called once at startup by `demo::build_demo_app()`.
pub fn enable() {
    DEMO_MODE.store(true, Ordering::Relaxed);
}

/// Disable demo mode. Used by tests to reset global state.
#[cfg(test)]
pub fn disable() {
    DEMO_MODE.store(false, Ordering::Relaxed);
}
