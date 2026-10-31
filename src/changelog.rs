use std::borrow::Cow;
use std::sync::OnceLock;

use semver::Version;

pub(crate) const EMBEDDED: &str = include_str!("../CHANGELOG.md");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Feature,
    Change,
    Fix,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub kind: EntryKind,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Section {
    pub version: Version,
    pub date: Option<String>,
    pub entries: Vec<Entry>,
}

static CACHE: OnceLock<Vec<Section>> = OnceLock::new();

pub fn cached() -> &'static Vec<Section> {
    CACHE.get_or_init(|| parse(EMBEDDED))
}

pub fn parse(input: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut current: Option<Section> = None;

    for line in input.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            if let Some(sec) = current.take()
                && !sec.entries.is_empty()
            {
                sections.push(sec);
            }
            if let Some((version, date)) = parse_header(rest) {
                current = Some(Section {
                    version,
                    date,
                    entries: Vec::new(),
                });
            } else {
                current = None;
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("- ")
            && let Some(sec) = current.as_mut()
        {
            let (kind, text) = classify(rest.trim());
            sec.entries.push(Entry { kind, text });
        }
    }

    if let Some(sec) = current.take()
        && !sec.entries.is_empty()
    {
        sections.push(sec);
    }

    sections
}

fn parse_header(rest: &str) -> Option<(Version, Option<String>)> {
    let trimmed = rest.trim();
    if let Some((vpart, dpart)) = trimmed.split_once(" - ") {
        let version = Version::parse(vpart.trim()).ok()?;
        let date = dpart.trim().to_string();
        Some((version, if date.is_empty() { None } else { Some(date) }))
    } else {
        Version::parse(trimmed).ok().map(|v| (v, None))
    }
}

pub(crate) fn classify(bullet: &str) -> (EntryKind, String) {
    let lower = bullet.to_ascii_lowercase();
    for (prefix, kind) in [
        ("feat:", EntryKind::Feature),
        ("fix:", EntryKind::Fix),
        ("change:", EntryKind::Change),
    ] {
        if lower.starts_with(prefix) {
            // prefix is all-ASCII so prefix.len() is a valid byte boundary in bullet.
            let text = bullet[prefix.len()..].trim().to_string();
            return (kind, text);
        }
    }
    (EntryKind::Change, bullet.to_string())
}

fn window_bounds(
    sections: &[Section],
    last_seen: Option<&Version>,
    current: &Version,
) -> Option<(usize, usize)> {
    let upper = sections.iter().position(|s| s.version <= *current)?;
    let lower = match last_seen {
        Some(seen) => sections
            .iter()
            .position(|s| s.version <= *seen)
            .unwrap_or(sections.len()),
        None => sections.len(),
    };
    Some((upper, lower))
}

pub fn versions_to_show<'a>(
    sections: &'a [Section],
    last_seen: Option<&Version>,
    current: &Version,
    cap: usize,
) -> &'a [Section] {
    if let Some(seen) = last_seen
        && seen >= current
    {
        return &[];
    }
    let Some((upper, lower)) = window_bounds(sections, last_seen, current) else {
        return &[];
    };
    let end = lower.min(upper.saturating_add(cap)).min(sections.len());
    if end <= upper {
        return &[];
    }
    &sections[upper..end]
}

#[cfg(test)]
pub mod test_override {
    use std::sync::Mutex;
    static OVERRIDE: Mutex<Option<String>> = Mutex::new(None);
    pub fn set(s: String) {
        *OVERRIDE.lock().unwrap_or_else(|e| e.into_inner()) = Some(s);
    }
    pub fn clear() {
        *OVERRIDE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    pub fn get() -> Option<String> {
        OVERRIDE.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// Process-wide changelog override for render tests. Holding the guard keeps
/// the fixture in place; dropping it (also on panic) clears the override.
/// The override is global, so callers must serialize on
/// `crate::demo_flag::GLOBAL_TEST_LOCK` before taking one.
#[cfg(test)]
pub struct TestOverride(());

#[cfg(test)]
impl Drop for TestOverride {
    fn drop(&mut self) {
        test_override::clear();
    }
}

#[cfg(test)]
#[must_use = "the override clears when this guard drops"]
pub fn set_test_override(s: String) -> TestOverride {
    test_override::set(s);
    TestOverride(())
}

pub fn current_for_render() -> Cow<'static, [Section]> {
    #[cfg(test)]
    if let Some(s) = test_override::get() {
        return Cow::Owned(parse(&s));
    }
    Cow::Borrowed(cached().as_slice())
}

#[cfg(test)]
#[path = "changelog_tests.rs"]
mod tests;
