/// Update availability state.
///
/// `hint` defaults to `""` via `#[derive(Default)]`. In practice `App::new()`
/// always overwrites it with the detected install method, so the empty default
/// is only visible when constructing `UpdateState` in isolation (e.g. tests).
#[derive(Default)]
pub struct UpdateState {
    /// Available version string (None if up to date or unchecked).
    pub available: Option<String>,
    /// Update announcement headline.
    pub headline: Option<String>,
    /// Update hint string (install command suggestion).
    pub hint: &'static str,
}
