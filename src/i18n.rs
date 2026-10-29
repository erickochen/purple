//! Runtime UI language selection. User host names, commands and SSH data are never translated.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};
static CHINESE: AtomicBool = AtomicBool::new(false);
static CATALOG: OnceLock<HashMap<String, String>> = OnceLock::new();
static LANGUAGE_FILE: OnceLock<PathBuf> = OnceLock::new();
pub fn is_chinese() -> bool {
    CHINESE.load(Ordering::Relaxed)
}
pub fn text(english: &'static str) -> &'static str {
    if !is_chinese() {
        return english;
    }
    let map = CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../locales/zh-CN.json"))
            .expect("valid bundled language pack")
    });
    map.get(english).map(String::as_str).unwrap_or(english)
}
pub fn initialize(language: Option<&str>, path: Option<PathBuf>) -> anyhow::Result<()> {
    if let Some(p) = path {
        let _ = LANGUAGE_FILE.set(p);
    }
    if let Some(path) = LANGUAGE_FILE
        .get()
        .and_then(|p| p.parent())
        .map(|p| p.join("locales/zh-CN.json"))
        && path.exists()
    {
        let map: HashMap<String, String> = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        for (key, value) in &map {
            anyhow::ensure!(
                key.matches("{}").count() == value.matches("{}").count(),
                "Invalid language placeholder: {key}"
            );
        }
        let _ = CATALOG.set(map);
    }
    let saved = LANGUAGE_FILE
        .get()
        .and_then(|p| std::fs::read_to_string(p).ok());
    CHINESE.store(
        language.or(saved.as_deref()).unwrap_or("en").trim() == "zh-CN",
        Ordering::Relaxed,
    );
    Ok(())
}
pub fn format_text(key: &'static str, args: &[String]) -> String {
    let template = text(key);
    let mut out = String::new();
    let mut parts = template.split("{}");
    out.push_str(parts.next().unwrap_or_default());
    for (part, arg) in parts.zip(args) {
        out.push_str(arg);
        out.push_str(part);
    }
    out
}
pub fn save(language: &str) -> std::io::Result<()> {
    let path = LANGUAGE_FILE
        .get()
        .ok_or_else(|| std::io::Error::other("Language settings path unavailable"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::fs_util::atomic_write(path, format!("{language}\n").as_bytes())?;
    CHINESE.store(language == "zh-CN", Ordering::Relaxed);
    Ok(())
}
pub fn toggle() -> std::io::Result<()> {
    save(if is_chinese() { "en" } else { "zh-CN" })
}
#[cfg(test)]
mod tests {
    #[test]
    fn catalog_is_valid_and_retains_placeholders() {
        let catalog: std::collections::HashMap<String, String> =
            serde_json::from_str(include_str!("../locales/zh-CN.json")).unwrap();
        for (en, zh) in &catalog {
            assert_eq!(en.matches('{').count(), zh.matches('{').count(), "{en}");
            assert!(!zh.is_empty());
        }
        assert_eq!(catalog["Password Source"], "密码来源");
        assert_eq!(catalog["Snippets"], "命令片段");
    }
}
