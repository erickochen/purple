//! Language selection and catalog diagnostics.
pub fn error(error: &impl std::fmt::Display) -> String {
    format!("Language: {error}")
}
pub fn selected(language: &str) -> String {
    format!("Language: {language}")
}
pub fn invalid_override(path: &std::path::Path, error: &impl std::fmt::Display) -> String {
    format!(
        "Ignoring invalid language override {}: {error}. Using bundled translations; fix or move this override file to recover.",
        path.display()
    )
}
