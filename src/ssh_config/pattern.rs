//! OpenSSH host-pattern matching.
//!
//! Implements `Host` keyword wildcard semantics: `*`, `?`, `[charset]`,
//! `[!charset]`/`[^charset]`, `[a-z]` ranges and `!pattern` negation.
//! All functions here are pure and own no state.

/// Does this pattern contain any SSH wildcard metacharacters?
pub fn is_host_pattern(pattern: &str) -> bool {
    pattern.contains('*')
        || pattern.contains('?')
        || pattern.contains('[')
        || pattern.starts_with('!')
        || pattern.contains(' ')
        || pattern.contains('\t')
}

/// Match a text string against an SSH host pattern.
/// Supports `*` (any sequence), `?` (single char), `[charset]` (character class),
/// `[!charset]`/`[^charset]` (negated class), `[a-z]` (ranges) and `!pattern` (negation).
pub fn ssh_pattern_match(pattern: &str, text: &str) -> bool {
    if let Some(rest) = pattern.strip_prefix('!') {
        return !match_glob(rest, text);
    }
    match_glob(pattern, text)
}

/// Core glob matcher without negation prefix handling.
/// Empty text only matches empty pattern.
fn match_glob(pattern: &str, text: &str) -> bool {
    if text.is_empty() {
        return pattern.is_empty();
    }
    if pattern.is_empty() {
        return false;
    }
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    glob_match(&pat, &txt)
}

/// Iterative glob matching with star-backtracking.
fn glob_match(pat: &[char], txt: &[char]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star: Option<(usize, usize)> = None; // (pattern_pos, text_pos)

    while ti < txt.len() {
        if pi < pat.len() && pat[pi] == '?' {
            pi += 1;
            ti += 1;
        } else if pi < pat.len() && pat[pi] == '*' {
            star = Some((pi + 1, ti));
            pi += 1;
        } else if pi < pat.len() && pat[pi] == '[' {
            if let Some((matches, end)) = match_char_class(pat, pi, txt[ti]) {
                if matches {
                    pi = end;
                    ti += 1;
                } else if let Some((spi, sti)) = star {
                    let sti = sti + 1;
                    star = Some((spi, sti));
                    pi = spi;
                    ti = sti;
                } else {
                    return false;
                }
            } else if let Some((spi, sti)) = star {
                // Malformed class: backtrack
                let sti = sti + 1;
                star = Some((spi, sti));
                pi = spi;
                ti = sti;
            } else {
                return false;
            }
        } else if pi < pat.len() && pat[pi] == txt[ti] {
            pi += 1;
            ti += 1;
        } else if let Some((spi, sti)) = star {
            let sti = sti + 1;
            star = Some((spi, sti));
            pi = spi;
            ti = sti;
        } else {
            return false;
        }
    }

    while pi < pat.len() && pat[pi] == '*' {
        pi += 1;
    }
    pi == pat.len()
}

/// Parse and match a `[...]` character class starting at `pat[start]`.
/// Returns `Some((matched, end_index))` where `end_index` is past `]`.
/// Returns `None` if no closing `]` is found.
fn match_char_class(pat: &[char], start: usize, ch: char) -> Option<(bool, usize)> {
    let mut i = start + 1;
    if i >= pat.len() {
        return None;
    }

    let negate = pat[i] == '!' || pat[i] == '^';
    if negate {
        i += 1;
    }

    let mut matched = false;
    while i < pat.len() && pat[i] != ']' {
        if i + 2 < pat.len() && pat[i + 1] == '-' && pat[i + 2] != ']' {
            let lo = pat[i];
            let hi = pat[i + 2];
            if ch >= lo && ch <= hi {
                matched = true;
            }
            i += 3;
        } else {
            matched |= pat[i] == ch;
            i += 1;
        }
    }

    if i >= pat.len() {
        return None;
    }

    let result = if negate { !matched } else { matched };
    Some((result, i + 1))
}

/// Strip a single pair of surrounding double quotes from a Host pattern
/// token. OpenSSH accepts `Host "alpha"` as equivalent to `Host alpha`; without
/// this strip purple's stored pattern would contain literal quote characters
/// and never match the user-typed alias.
fn unquote_pattern_token(token: &str) -> &str {
    if token.len() >= 2 && token.starts_with('"') && token.ends_with('"') {
        &token[1..token.len() - 1]
    } else {
        token
    }
}

/// Check whether a `Host` pattern matches a given alias.
/// OpenSSH `Host` keyword matches only against the target alias typed on the
/// command line, never against the resolved HostName.
pub fn host_pattern_matches(host_pattern: &str, alias: &str) -> bool {
    let patterns: Vec<&str> = host_pattern.split_whitespace().collect();
    if patterns.is_empty() {
        return false;
    }

    let mut any_positive_match = false;
    for pat in &patterns {
        let pat = unquote_pattern_token(pat);
        if let Some(neg) = pat.strip_prefix('!') {
            if match_glob(neg, alias) {
                return false;
            }
        } else if ssh_pattern_match(pat, alias) {
            any_positive_match = true;
        }
    }

    any_positive_match
}

/// Match `text` against a comma-separated pattern list, the form `Match`
/// criteria use. A matching `!` entry excludes, otherwise one hit is enough.
/// Only `*` and `?` are wildcards here, as in OpenSSH. `fold_case` compares
/// ASCII case-insensitively, which ssh does for host names.
pub fn pattern_list_matches(list: &str, text: &str, fold_case: bool) -> bool {
    let text = if fold_case {
        text.to_ascii_lowercase()
    } else {
        text.to_string()
    };
    let mut any_positive = false;
    for pat in list.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let pat = if fold_case {
            pat.to_ascii_lowercase()
        } else {
            pat.to_string()
        };
        if let Some(neg) = pat.strip_prefix('!') {
            if wildcard_match(neg, &text) {
                return false;
            }
        } else if wildcard_match(&pat, &text) {
            any_positive = true;
        }
    }
    any_positive
}

/// `*` and `?` matching with star backtracking. No character classes.
fn wildcard_match(pattern: &str, text: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < txt.len() {
        if pi < pat.len() && pat[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if pi < pat.len() && (pat[pi] == '?' || pat[pi] == txt[ti]) {
            pi += 1;
            ti += 1;
        } else if let Some((spi, sti)) = star {
            pi = spi + 1;
            ti = sti + 1;
            star = Some((spi, sti + 1));
        } else {
            return false;
        }
    }
    pat[pi..].iter().all(|&c| c == '*')
}

/// Returns true if any hop in a (possibly comma-separated) ProxyJump value
/// matches the given alias. Strips optional `user@` prefix and `:port`
/// suffix from each hop before comparing. Handles IPv6 bracket notation
/// `[addr]:port`. Used to detect self-referencing loops.
pub fn proxy_jump_contains_self(proxy_jump: &str, alias: &str) -> bool {
    proxy_jump.split(',').any(|hop| {
        let h = hop.trim();
        // Strip optional user@ prefix (take everything after the first @).
        let h = h.split_once('@').map_or(h, |(_, host)| host);
        // Strip optional :port suffix. Handle [IPv6]:port bracket notation.
        let h = if let Some(bracketed) = h.strip_prefix('[') {
            bracketed.split_once(']').map_or(h, |(host, _)| host)
        } else {
            h.rsplit_once(':').map_or(h, |(host, _)| host)
        };
        h == alias
    })
}

/// Fill the fields that are still empty from one source, first match wins.
/// A self-referencing ProxyJump is assigned on purpose so the UI can warn.
pub(super) fn apply_first_match_values(
    proxy_jump: &mut String,
    user: &mut String,
    identity_file: &mut String,
    src_proxy_jump: &str,
    src_user: &str,
    src_identity_file: &str,
) {
    if proxy_jump.is_empty() && !src_proxy_jump.is_empty() {
        src_proxy_jump.clone_into(proxy_jump);
    }
    if user.is_empty() && !src_user.is_empty() {
        src_user.clone_into(user);
    }
    if identity_file.is_empty() && !src_identity_file.is_empty() {
        src_identity_file.clone_into(identity_file);
    }
}
