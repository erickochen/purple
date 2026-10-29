//! Host-related user-facing strings: stale-host hint, host list (sort,
//! group, filter, navigation), host form, form validation, host CRUD
//! (add/edit), bulk tag editor, confirm delete, clone, host detail
//! tags, SSH config repair, host key reset and confirm/host key.

// ── Stale host ──────────────────────────────────────────────────────

/// Compose a "Stale host." warning with an optional hint clause.
/// Trims the hint, drops a trailing period to avoid doubling, and uses
/// a space separator so the result reads as one sentence. With an empty
/// hint the bare "Stale host." remains.
pub fn stale_host(hint: &str) -> String {
    let trimmed = hint.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        crate::i18n::text("Stale host.").to_string()
    } else {
        crate::tformat!("Stale host. {}.", trimmed)
    }
}

// ── Host list ───────────────────────────────────────────────────────

pub fn copied_ssh_command(alias: &str) -> String {
    crate::tformat!("Copied SSH command for {}.", alias)
}

pub fn copied_config_block(alias: &str) -> String {
    crate::tformat!("Copied config block for {}.", alias)
}

pub fn showing_unreachable(count: usize) -> String {
    crate::tformat!(
        "Showing {} unreachable host{}.",
        count,
        if count == 1 { "" } else { "s" }
    )
}

pub fn sorted_by(label: &str) -> String {
    crate::tformat!("Sorted by {}.", label)
}

pub fn sorted_by_save_failed(label: &str, e: &impl std::fmt::Display) -> String {
    crate::tformat!("Sorted by {}. (save failed: {})", label, e)
}

pub fn grouped_by(label: &str) -> String {
    crate::tformat!("Grouped by {}.", label)
}

pub fn grouped_by_save_failed(label: &str, e: &impl std::fmt::Display) -> String {
    crate::tformat!("Grouped by {}. (save failed: {})", label, e)
}

pub const UNGROUPED: &str = "Ungrouped.";

pub fn ungrouped_save_failed(e: &impl std::fmt::Display) -> String {
    crate::tformat!("Ungrouped. (save failed: {})", e)
}

pub const GROUPED_BY_TAG: &str = "Grouped by tag.";

pub fn grouped_by_tag_save_failed(e: &impl std::fmt::Display) -> String {
    crate::tformat!("Grouped by tag. (save failed: {})", e)
}

pub fn host_restored(alias: &str) -> String {
    format!("{} is back from the dead.", alias)
}

pub fn restored_tags(count: usize) -> String {
    format!(
        "Restored tags on {} host{}.",
        count,
        if count == 1 { "" } else { "s" }
    )
}

pub const NOTHING_TO_UNDO: &str = "Nothing to undo.";
pub const NO_IMPORTABLE_HOSTS: &str = "No importable hosts in known_hosts.";
pub const NO_STALE_HOSTS: &str = "No stale hosts.";
pub const NO_HOST_SELECTED: &str = "No host selected.";
pub const NO_HOSTS_TO_RUN: &str = "No hosts to run on.";
pub const NO_HOSTS_TO_TAG: &str = "No hosts to tag.";
pub const PING_FIRST: &str = "Ping first (p/P), then filter with !.";
pub const PINGING_ALL: &str = "Pinging all the things...";
pub const ESC_QUIT_HINT: &str = "Nothing to cancel. Press q to quit.";

pub fn included_file_edit(name: &str) -> String {
    format!("{} is in an included file. Edit it there.", name)
}

pub fn included_file_delete(name: &str) -> String {
    format!("{} is in an included file. Delete it there.", name)
}

pub fn included_file_clone(name: &str) -> String {
    format!("{} is in an included file. Clone it there.", name)
}

pub fn included_host_lives_in(alias: &str, path: &impl std::fmt::Display) -> String {
    format!("{} lives in {}. Edit it there.", alias, path)
}

pub fn included_host_clone_there(alias: &str, path: &impl std::fmt::Display) -> String {
    format!("{} lives in {}. Clone it there.", alias, path)
}

pub fn included_host_tag_there(alias: &str, path: &impl std::fmt::Display) -> String {
    format!("{} is included from {}. Tag it there.", alias, path)
}

pub const HOST_NOT_FOUND_IN_CONFIG: &str = "Host not found in config.";

// ── Host form ───────────────────────────────────────────────────────

pub const SMART_PARSED: &str = "Smart-parsed that for you. Check the fields.";
pub const LOOKS_LIKE_ADDRESS: &str = "Looks like an address. Suggested as Host.";

// ── Form validation (HostForm) ──────────────────────────────────────
//
// Surfaced via `notify_error(msg)` after `HostForm::validate()`. All
// strings live here so the central message audit (`check-messages.sh`)
// covers them and so the wording stays consistent with the rest of the
// TUI copy.

pub const HOST_ALIAS_EMPTY: &str = "Alias can't be empty. Every host needs a name!";
pub const HOST_PATTERN_EMPTY: &str = "Pattern can't be empty.";
pub const HOST_PATTERN_NEEDS_WILDCARD: &str =
    "Pattern needs a wildcard (*, ?, [) or multiple hosts.";
pub const HOST_ALIAS_WHITESPACE: &str = "Alias can't contain whitespace. Keep it simple.";
pub const HOST_ALIAS_HASH: &str =
    "Alias can't contain '#'. That's a comment character in SSH config.";
pub const HOST_ALIAS_PATTERN_CHARS: &str =
    "Alias can't contain pattern characters. That creates a match pattern, not a host.";
pub const HOST_HOSTNAME_EMPTY: &str = "Hostname can't be empty. Where should we connect to?";
pub const HOST_HOSTNAME_WHITESPACE: &str = "Hostname can't contain whitespace.";
pub const HOST_PORT_INVALID: &str = "That's not a port number. Ports are 1-65535, not poetry.";
pub const HOST_PORT_ZERO: &str = "Port 0? Bold choice, but no. Try 1-65535.";
pub const HOST_VAULT_ROLE_INVALID: &str = "Vault SSH role: only letters, digits, /, _ and - \
     are allowed (e.g. ssh-client-signer/sign/my-role).";
pub const HOST_VAULT_ADDR_INVALID: &str = "Vault SSH address: must be a non-empty URL \
     without spaces or control characters (e.g. http://127.0.0.1:8200).";

/// Long-form "{} contains control characters." used by `HostForm::validate`
/// where the toast doubles as guidance ("that's not going to work").
pub fn field_control_chars(name: &str) -> String {
    crate::tformat!(
        "{} contains control characters. That's not going to work.",
        name
    )
}

// ── Host CRUD (add / edit) ──────────────────────────────────────────

pub fn pattern_already_exists(alias: &str) -> String {
    crate::tformat!("Pattern '{}' already exists.", alias)
}

pub fn host_alias_already_exists(alias: &str) -> String {
    crate::tformat!("'{}' already exists. Aliases must be unique.", alias)
}

pub const PATTERN_NO_LONGER_EXISTS: &str = "Pattern no longer exists.";
pub const HOST_NO_LONGER_EXISTS: &str = "Host no longer exists.";

pub fn cert_path_resolve_failed(e: &impl std::fmt::Display) -> String {
    format!("Failed to resolve cert path: {}", e)
}

/// Toast shown after a host is added through the TUI form. The CLI
/// `purple add` flow shares this string via `messages::cli::welcome`.
pub fn welcome_aboard(alias: &str) -> String {
    crate::tformat!("Welcome aboard, {}!", alias)
}

// ── Bulk tag editor ─────────────────────────────────────────────────

pub const BULK_TAG_NO_HOSTS_SELECTED: &str = "No hosts selected.";

/// Status string shown after a successful bulk tag apply. Returns an
/// empty string when nothing was changed and nothing was skipped, so the
/// caller can detect a no-op and skip setting a status.
pub fn bulk_tag_apply_status(
    changed_hosts: usize,
    added: usize,
    removed: usize,
    skipped_included: usize,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if changed_hosts > 0 {
        let host_word = if changed_hosts == 1 { "" } else { "s" };
        let mut head = format!("Updated {} host{}", changed_hosts, host_word);
        let mut delta = Vec::new();
        if added > 0 {
            delta.push(format!("+{}", added));
        }
        if removed > 0 {
            delta.push(format!("-{}", removed));
        }
        if !delta.is_empty() {
            head = format!("{} ({})", head, delta.join(" "));
        }
        parts.push(head);
    }
    if skipped_included > 0 {
        let file_word = if skipped_included == 1 { "" } else { "s" };
        parts.push(format!(
            "skipped {} in include file{}",
            skipped_included, file_word
        ));
    }
    parts.join(". ")
}

// ── Confirm delete ──────────────────────────────────────────────────

pub fn goodbye_host(alias: &str) -> String {
    crate::tformat!("Goodbye, {}. We barely knew ye. (u to undo)", alias)
}

pub fn host_not_found(alias: &str) -> String {
    crate::tformat!("Host '{}' not found.", alias)
}

/// Toast after stripping an alias token from a shared `Host` line. Undo is
/// not offered because re-inserting a whole block would not reverse a token
/// strip (sibling aliases and their directives stay in place).
pub fn siblings_stripped(alias: &str, sibling_count: usize) -> String {
    if sibling_count == 1 {
        format!(
            "Stripped {}. 1 sibling alias kept its shared config.",
            alias
        )
    } else {
        format!(
            "Stripped {}. {} sibling aliases kept their shared config.",
            alias, sibling_count
        )
    }
}

/// One-line note rendered inside the confirm-delete dialog when the target
/// alias shares its `Host` block with siblings. Explains that the other
/// tokens survive.
pub fn confirm_delete_siblings_note(siblings: &[String]) -> String {
    let shown: Vec<&str> = siblings.iter().take(3).map(String::as_str).collect();
    let tail = if siblings.len() > shown.len() {
        format!(" +{} more", siblings.len() - shown.len())
    } else {
        String::new()
    };
    format!("Siblings kept: {}{}", shown.join(", "), tail)
}

pub fn cert_cleanup_warning(path: &impl std::fmt::Display, e: &impl std::fmt::Display) -> String {
    format!("Warning: failed to clean up Vault SSH cert {}: {}", path, e)
}

// ── Clone ───────────────────────────────────────────────────────────

pub const CLONED_VAULT_CLEARED: &str = "Cloned. Vault SSH role cleared on copy.";

// ── SSH config repair ───────────────────────────────────────────────

pub fn config_repaired(groups: usize, orphaned: usize) -> String {
    format!(
        "Repaired SSH config ({} absorbed, {} orphaned group headers).",
        groups, orphaned
    )
}

pub fn no_exact_match(alias: &str) -> String {
    format!("No exact match for '{}'. Here's what we found.", alias)
}

pub fn group_pref_reset_failed(e: &impl std::fmt::Display) -> String {
    format!("Group preference reset. (save failed: {})", e)
}

// ── Host key reset ──────────────────────────────────────────────────

pub fn host_key_remove_failed(stderr: &str) -> String {
    crate::tformat!("Failed to remove host key: {}", stderr)
}

pub fn ssh_keygen_failed(e: &impl std::fmt::Display) -> String {
    crate::tformat!("Failed to run ssh-keygen: {}", e)
}

// ── Confirm / host key ──────────────────────────────────────────────

pub fn removed_host_key(hostname: &str) -> String {
    crate::tformat!("Removed host key for {}. Reconnecting...", hostname)
}

// ── Host detail (tags) ──────────────────────────────────────────────

pub fn tagged_host(alias: &str, count: usize) -> String {
    format!(
        "Tagged {} with {} label{}.",
        alias,
        count,
        if count == 1 { "" } else { "s" }
    )
}

#[cfg(test)]
mod stale_host_tests {
    use super::stale_host;

    #[test]
    fn empty_hint_returns_bare_sentence() {
        assert_eq!(stale_host(""), "Stale host.");
    }

    #[test]
    fn empty_after_trim_returns_bare_sentence() {
        assert_eq!(stale_host("   "), "Stale host.");
    }

    #[test]
    fn provider_hint_is_appended_with_space_and_period() {
        assert_eq!(
            stale_host("Gone from DigitalOcean"),
            "Stale host. Gone from DigitalOcean."
        );
    }

    #[test]
    fn trailing_period_in_hint_is_not_doubled() {
        assert_eq!(
            stale_host("Gone from DigitalOcean."),
            "Stale host. Gone from DigitalOcean."
        );
    }

    #[test]
    fn leading_space_in_hint_is_trimmed() {
        assert_eq!(stale_host(" Gone from AWS"), "Stale host. Gone from AWS.");
    }
}
