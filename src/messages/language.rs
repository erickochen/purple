pub fn error(error: &impl std::fmt::Display) -> String {
    format!("Language: {error}")
}
pub fn selected(language: &str) -> String {
    format!("Language: {language}")
}
