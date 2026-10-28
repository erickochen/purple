//! Picker overlays (password source, key, proxy), tab empty-state
//! cards, destructive confirm popups and key-push run messages.
//! Grouped here because they share the picker UX surface and the
//! key-push flow lives inside the same overlay family.

// ── Picker (password source, key, proxy) ────────────────────────────

/// Shown when the user opens a picker that needs hosts (containers `a`,
/// keys `p` push, etc.) but no hosts exist in ~/.ssh/config yet.
/// Identical "Add a host first" closing across surfaces so the user
/// reads the same prerequisite regardless of which picker they tried.
pub const PICKER_NO_HOSTS: &str = "No hosts yet. Add a host first.";

pub const GLOBAL_DEFAULT_CLEARED: &str = "Global default cleared.";
pub const PASSWORD_SOURCE_CLEARED: &str = "Password source cleared.";
pub const ASKPASS_CUSTOM_COMMAND_HINT: &str =
    "Type your command. Use %a (alias) and %h (hostname) as placeholders.";

pub fn global_default_set(label: &str) -> String {
    format!("Global default set to {}.", label)
}

pub fn password_source_set(label: &str) -> String {
    format!("Password source set to {}.", label)
}

pub fn complete_path(label: &str) -> String {
    format!("Complete the {} path.", label)
}

pub fn key_selected(name: &str) -> String {
    format!("Locked and loaded with {}.", name)
}

// ── Keys tab ────────────────────────────────────────────────────────

/// Copy succeeded. Toast tells the user which key landed on the clipboard.
pub fn keys_copy_success(name: &str) -> String {
    format!("Copied {}.pub to clipboard.", name)
}

/// The .pub file could not be read from disk (deleted, permission denied).
pub fn keys_copy_read_failed(name: &str) -> String {
    format!("Could not read {}.pub from disk.", name)
}

// ── Tab empty-state cards (design::TabEmpty) ────────────────────────────
// One bundle per top-level tab. Each renders inside the existing outer
// block as a centred card via `design::render_tab_empty`. Headlines
// state the missing thing; explainers name the cause; hints surface the
// one or two keys that populate the tab.

pub const TAB_EMPTY_HOSTS_HEADLINE: &str = "It's quiet in here.";
pub fn tab_empty_hosts_explainer(config: &str) -> String {
    format!(
        "purple reads hosts from {config} and from the cloud providers you connect. Add one by hand or sync a provider and the list fills up."
    )
}
pub const TAB_EMPTY_HOSTS_HINT_ADD: &str = "add a host";
pub const TAB_EMPTY_HOSTS_HINT_SYNC: &str = "open providers to sync from the cloud";

pub const TAB_EMPTY_CONTAINERS_HEADLINE: &str = "No containers cached yet.";
pub const TAB_EMPTY_CONTAINERS_EXPLAINER: &str = "purple snapshots docker or podman output per host and caches it locally. Pick a host below and its containers show up here.";
pub const TAB_EMPTY_CONTAINERS_HINT_ADD: &str = "pick a host to scan";

pub const TAB_EMPTY_TUNNELS_HEADLINE: &str = "No tunnels yet.";
pub fn tab_empty_tunnels_explainer(config: &str) -> String {
    format!(
        "Tunnels are SSH port forwards stored per host in {config}. This tab aggregates Local, Remote and Dynamic forwards across every alias."
    )
}
pub const TAB_EMPTY_TUNNELS_HINT_ADD: &str = "add a tunnel";

pub const TAB_EMPTY_KEYS_HEADLINE: &str = "No SSH keys in ~/.ssh/ yet.";
pub const TAB_EMPTY_KEYS_EXPLAINER: &str = "purple reads every public-key file in ~/.ssh/ along with its activity history. Generate one and the new key shows up here on next refresh.";
pub const TAB_EMPTY_KEYS_HINT_KEYGEN: &str = "ssh-keygen -t ed25519 -C \"$(whoami)@$(hostname)\"";

// ── Destructive confirm popups (design::render_destructive_popup) ──────
// Every popup is rendered as a centred danger_block over the parent
// overlay, never as a footer prompt. Each surface owns a title, a
// question and an optional detail line; keep them centralised here so
// rewording requires one diff per surface, not per call site.

pub const CONFIRM_TUNNEL_DELETE_TITLE: &str = " Remove tunnel? ";
pub const CONFIRM_TUNNEL_DELETE_QUESTION: &str = "Remove the selected tunnel rule from this host?";
pub fn confirm_tunnel_delete_detail(config: &str) -> String {
    format!("Rewrites {config}. The rule is gone after save.")
}

pub const CONFIRM_SNIPPET_DELETE_TITLE: &str = " Remove snippet? ";
pub const CONFIRM_SNIPPET_DELETE_DETAIL: &str = "The snippet file is rewritten on disk.";
pub fn confirm_snippet_delete_question(name: &str) -> String {
    format!("Remove \"{}\" from the snippet store?", name)
}

pub const CONFIRM_PROVIDER_REMOVE_TITLE: &str = " Remove provider? ";
pub fn confirm_provider_remove_detail(config: &str) -> String {
    format!("Synced hosts stay in {config}. The integration is gone after save.")
}
pub fn confirm_provider_remove_question(display: &str) -> String {
    format!("Remove the \"{}\" provider config?", display)
}
pub fn confirm_provider_remove_labeled_question(display: &str, label: &str) -> String {
    format!("Remove the \"{}\" config labelled \"{}\"?", display, label)
}

/// Empty-state message for the key-push picker when the config has no
/// host entries to target.
pub fn key_push_no_hosts(config: &str) -> String {
    format!("No hosts in {config}. Add a host first, then come back here.")
}

/// Header line for the Vault SSH strip when there is no Valid cached
/// cert. Tells the user how to populate the strip.
pub const VAULT_STRIP_EMPTY: &str =
    "  No active certs. Press V to sign all Vault SSH hosts at once.";

/// Inline tag appended to vault-ssh host rows in the push picker to
/// document why they cannot be selected.
pub const KEY_PUSH_VAULT_TAG: &str = "  (vault)";

/// Picker overlay title formats.
pub fn key_push_picker_title_eligible(key_label: &str, eligible: usize, total: usize) -> String {
    format!(
        "Push {} \u{203A} Select Hosts ({} eligible of {})",
        key_label, eligible, total
    )
}

pub fn key_push_picker_title_selected(
    key_label: &str,
    selected: usize,
    total: usize,
    eligible: usize,
) -> String {
    format!(
        "Push {} \u{203A} {} selected of {} ({} eligible)",
        key_label, selected, total, eligible
    )
}

/// Toast when the user presses `p` but no public key file is readable.
pub fn key_push_no_pubkey(name: &str) -> String {
    format!(
        "Cannot read {}.pub. The file is missing or unreadable.",
        name
    )
}

/// Toast when the user committed a host picker with zero hosts selected.
/// Shared by the key-push picker and the snippet host picker.
pub const PICKER_NONE_SELECTED: &str = "Select at least one host with Space.";

/// Shown in a host picker when the type-to-filter query matches no hosts.
pub const PICKER_NO_MATCHES: &str = "No matching hosts.";

/// Guard for the AWS profile picker: nothing to pick from, and the field has
/// an alternative, so name it.
pub const PICKER_NO_AWS_PROFILES: &str =
    "No profiles in ~/.aws/config or ~/.aws/credentials. Fill in Token instead.";

/// Title for the snippet host picker (snippet -> hosts run flow).
pub fn snippet_host_picker_title(snippet_name: &str, selected: usize, total: usize) -> String {
    format!(
        "Run {} \u{203A} Select Hosts ({} of {} selected)",
        snippet_name, selected, total
    )
}

/// Title for the host picker when opened from the edit form to choose a
/// snippet's default target hosts (no run is launched).
pub fn snippet_default_hosts_picker_title(
    snippet_name: &str,
    selected: usize,
    total: usize,
) -> String {
    format!(
        "Default hosts for {} \u{203A} Select ({} of {} selected)",
        snippet_name, selected, total
    )
}

/// Toast shown when the user tries to select a vault-ssh host. These
/// hosts are managed via signed certs (`V`), not static authorized_keys
/// appends.
pub const KEY_PUSH_VAULT_SKIP: &str =
    "Vault SSH host. Use V on the host list to sign a cert instead.";

/// Progress toast at the start of a push run.
pub fn key_push_in_progress(key_name: &str, host_count: usize) -> String {
    format!("Pushing {} to {} host(s)...", key_name, host_count)
}

/// Error toast when std::thread::spawn fails (essentially OOM / rlimit).
pub fn key_push_thread_spawn_failed() -> String {
    "Could not spawn push worker thread. Check resource limits.".to_string()
}

/// Warning toast when the user presses `p` while a push is still
/// running. Tells them how to recover.
pub const KEY_PUSH_ALREADY_IN_PROGRESS: &str =
    "A push is already running. Press Esc to cancel first.";

/// Error toast when the `.pub` file is not a regular file, is a symlink,
/// or could not be opened with `O_NOFOLLOW`. Stops the push before any
/// remote SSH call is made.
pub fn key_push_pubkey_not_regular(name: &str) -> String {
    format!("{}.pub is not a regular file. Symlinks are rejected.", name)
}

/// Error toast when the `.pub` file exceeds the 16 KiB cap. The most
/// common cause is a `.pub` symlink that resolved to a log file or a
/// truncated dump from an unrelated tool.
pub fn key_push_pubkey_too_large(name: &str, bytes: u64) -> String {
    format!(
        "{}.pub is {} bytes, larger than the 16 KiB push limit.",
        name, bytes
    )
}

/// Error toast when the `.pub` file does not parse as a single, valid
/// `authorized_keys` line. Catches multi-line content (which silently
/// installs multiple entries, including embedded `command=` clauses),
/// unsupported algorithms, and malformed base64 blobs.
pub fn key_push_invalid_pubkey(name: &str, detail: &str) -> String {
    format!("{}.pub failed validation: {}. Push aborted.", name, detail)
}

/// Error toast when the picker commits with zero eligible aliases. The
/// picker should always block this earlier, but the worker guard exists
/// as a defence-in-depth so the progress toast never sticks.
pub const KEY_PUSH_NO_HOSTS_SELECTED: &str =
    "Picker committed with no eligible hosts. Push aborted.";

/// Error toast when the user tries to push a certificate file. Pushing
/// a cert into authorized_keys bypasses its TTL and undermines the
/// signed-cert workflow.
pub const KEY_PUSH_CERT_NOT_PUSHABLE: &str =
    "Certificates cannot be pushed as static keys. Sign with V instead.";

/// Toast after the user pressed Esc to cancel an in-flight push run.
/// Names the per-host progress at the moment of cancel so the user
/// knows what may or may not have already been authorized.
pub fn key_push_cancelled(done: usize, total: usize) -> String {
    format!(
        "Push cancelled after {} of {} host(s). Re-run to finish the rest.",
        done, total,
    )
}

/// Body line shown inside the confirm dialog.
pub fn key_push_confirm_body(key_name: &str, host_count: usize) -> String {
    if host_count == 1 {
        format!("Push {} to 1 host?", key_name)
    } else {
        format!("Push {} to {} hosts?", key_name, host_count)
    }
}

/// Toast after a fully successful push run.
pub fn key_push_success(appended: usize, already: usize) -> String {
    if appended == 0 && already > 0 {
        format!("Key already present on {} host(s). Nothing to do.", already)
    } else if already == 0 {
        format!("Pushed to {} host(s).", appended)
    } else {
        format!(
            "Pushed to {} host(s). Already present on {}.",
            appended, already
        )
    }
}

/// Toast after a partial-failure push run. The detailed per-host errors
/// land in the sticky-error overlay rendered separately.
pub fn key_push_partial_failure(succeeded: usize, failed: usize) -> String {
    format!("Pushed to {} host(s). {} failed.", succeeded, failed)
}

/// Sticky-error overlay body when every host failed.
pub fn key_push_all_failed(count: usize) -> String {
    format!(
        "Push failed for all {} host(s). Check the host log for details.",
        count
    )
}

/// Sticky-error body when a single-host run failed. The reason travels
/// with it: one host has one reason, and it says more than a count.
pub fn key_push_single_failure(alias: &str, reason: &str) -> String {
    format!("Push to {} failed. {}", alias, reason)
}

/// Toast when a finished run left hosts waiting on a dialog. Leads with
/// what already landed. The dialog for the first of the rest opens behind
/// it, unless the user is in a form, so the count stands on its own.
pub fn key_push_pending_answers(done: usize, pending: usize) -> String {
    if done == 0 {
        return format!("{} host(s) need an answer.", pending);
    }
    format!(
        "Pushed to {} host(s). {} more need an answer.",
        done, pending
    )
}

pub fn proxy_jump_set(alias: &str) -> String {
    format!("Jumping through {}.", alias)
}

pub fn save_default_failed(e: &impl std::fmt::Display) -> String {
    format!("Failed to save default: {}", e)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT: &str = "~/.ssh/config";
    const OWN: &str = "~/.ssh/purple_config";

    #[test]
    fn tab_empty_explainers_name_the_config() {
        assert_eq!(
            tab_empty_hosts_explainer(DEFAULT),
            "purple reads hosts from ~/.ssh/config and from the cloud providers you connect. Add one by hand or sync a provider and the list fills up."
        );
        assert!(tab_empty_hosts_explainer(OWN).contains("from ~/.ssh/purple_config and"));
        assert_eq!(
            tab_empty_tunnels_explainer(DEFAULT),
            "Tunnels are SSH port forwards stored per host in ~/.ssh/config. This tab aggregates Local, Remote and Dynamic forwards across every alias."
        );
        assert!(tab_empty_tunnels_explainer(OWN).contains("per host in ~/.ssh/purple_config."));
    }

    #[test]
    fn confirm_details_name_the_config() {
        assert_eq!(
            confirm_tunnel_delete_detail(DEFAULT),
            "Rewrites ~/.ssh/config. The rule is gone after save."
        );
        assert_eq!(
            confirm_tunnel_delete_detail(OWN),
            "Rewrites ~/.ssh/purple_config. The rule is gone after save."
        );
        assert_eq!(
            confirm_provider_remove_detail(DEFAULT),
            "Synced hosts stay in ~/.ssh/config. The integration is gone after save."
        );
        assert_eq!(
            confirm_provider_remove_detail(OWN),
            "Synced hosts stay in ~/.ssh/purple_config. The integration is gone after save."
        );
    }

    #[test]
    fn key_push_no_hosts_names_the_config() {
        assert_eq!(
            key_push_no_hosts(DEFAULT),
            "No hosts in ~/.ssh/config. Add a host first, then come back here."
        );
        assert_eq!(
            key_push_no_hosts(OWN),
            "No hosts in ~/.ssh/purple_config. Add a host first, then come back here."
        );
    }
}
