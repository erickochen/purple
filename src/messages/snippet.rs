//! Snippet store messages: validation, CRUD toasts, output overlay,
//! snippet runner errors.

// ── Form validation (SnippetForm + snippet store) ───────────────────

pub const SNIPPET_NAME_EMPTY: &str = "Snippet name cannot be empty.";
pub const SNIPPET_NAME_WHITESPACE: &str =
    "Snippet name cannot have leading or trailing whitespace.";
pub const SNIPPET_NAME_INVALID_CHARS: &str = "Snippet name cannot contain #, [ or ].";
pub const SNIPPET_NAME_CONTROL_CHARS: &str = "Snippet name cannot contain control characters.";
pub const SNIPPET_COMMAND_EMPTY: &str = "Command cannot be empty.";
pub const SNIPPET_COMMAND_CONTROL_CHARS: &str = "Command cannot contain control characters.";
pub const SNIPPET_DESCRIPTION_CONTROL_CHARS: &str = "Description contains control characters.";
pub const SNIPPET_PARAM_NAME_EMPTY: &str = "Parameter name cannot be empty.";

pub fn snippet_param_name_invalid(name: &str) -> String {
    crate::tformat!("Parameter name '{}' contains invalid characters.", name)
}

/// Fallback display name when a snippet flow has no chosen snippet. Defensive:
/// shared by the host picker title and the run confirm so the literal lives in
/// one place.
pub const SNIPPET_FALLBACK_NAME: &str = "snippet";

// ── Snippets tab empty state ────────────────────────────────────────

pub const TAB_EMPTY_SNIPPETS_HEADLINE: &str = "No snippets yet";
pub const TAB_EMPTY_SNIPPETS_EXPLAINER: &str =
    "Snippets are reusable commands you can run across your hosts.";
pub const TAB_EMPTY_SNIPPETS_HINT_ADD: &str = "add a snippet";

// ── Snippets ────────────────────────────────────────────────────────

pub fn snippet_removed(name: &str) -> String {
    crate::tformat!("Removed snippet '{}'.", name)
}

pub fn snippet_added(name: &str) -> String {
    crate::tformat!("Added snippet '{}'.", name)
}

pub fn snippet_updated(name: &str) -> String {
    crate::tformat!("Updated snippet '{}'.", name)
}

pub fn snippet_exists(name: &str) -> String {
    crate::tformat!("'{}' already exists.", name)
}

/// Confirm body shown before running a snippet against the chosen hosts.
pub fn confirm_run_snippet_body(snippet_name: &str, host_count: usize) -> String {
    if host_count == 1 {
        crate::tformat!("Run {} on 1 host?", snippet_name)
    } else {
        crate::tformat!("Run {} on {} hosts?", snippet_name, host_count)
    }
}

// ── IMPACT card ─────────────────────────────────────────────────────
//
// Verdict headlines (shown in the card's title border, coloured by severity)
// and per-finding callout text. The model in `snippet_impact` emits structured
// findings; the prose lives here and the colour/glyph in the UI.

pub const IMPACT_VERDICT_READONLY: &str = "read-only, safe to fan out";
pub const IMPACT_VERDICT_WRITES: &str = "writes state on each host";
pub const IMPACT_VERDICT_ELEVATED: &str = "elevated: read carefully";
pub const IMPACT_VERDICT_CRITICAL: &str = "irreversible or takes hosts offline";

/// Final card line, recoloured to a warning at Elevated+ (the fleet multiplier).
pub const IMPACT_FLEET_SCOPE: &str = "runs on every host you select";

pub const IMPACT_PRIVILEGE: &str = "runs as root (sudo)";
pub const IMPACT_REMOTE_EXEC: &str = "fetches and runs remote code";
pub const IMPACT_AVAILABILITY: &str = "reboots or powers off the host";
pub const IMPACT_SECRETS: &str = "may expose secrets in captured output";

pub fn impact_destructive(subject: &str) -> String {
    format!("{subject}: deletes or overwrites data")
}

pub fn impact_irreversible(subject: &str) -> String {
    format!("{subject}: irreversible data loss")
}

pub fn impact_service(subject: &str) -> String {
    format!("{subject}: changes running state")
}

pub fn impact_package(subject: &str) -> String {
    format!("{subject}: changes installed packages")
}

pub fn impact_redirect(subject: &str) -> String {
    format!("overwrites {subject}")
}

pub fn impact_unknown(subject: &str) -> String {
    format!("{subject}: effect not verified")
}

/// Default-hosts field value: the chosen aliases, abbreviated past three with a
/// `+N` overflow. Empty string when no default hosts are set (the field then
/// shows its placeholder).
pub fn snippet_default_hosts_summary(hosts: &[String]) -> String {
    if hosts.is_empty() {
        return String::new();
    }
    let shown = hosts.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
    if hosts.len() > 3 {
        format!("{shown}  +{}", hosts.len() - 3)
    } else {
        shown
    }
}

// ── Detail panel: TRACK RECORD ──────────────────────────────────────

/// Headline reliability shown in the TRACK RECORD card's title border.
pub fn reliability_headline(pct: u16) -> String {
    format!("{pct}% reliable")
}

/// Host-execution tally in the TRACK RECORD summary line.
pub fn hosts_ok_ratio(ok: usize, total: usize) -> String {
    format!("{ok} of {total} host runs ok")
}

/// Prefix of the recency phrase, rendered as `last <glyph> <ago>`.
pub const RUN_LAST_PREFIX: &str = "last";

/// Non-ok run categories in the TRACK RECORD summary. The glyph and colour
/// are applied at the call site; these carry only the wording.
pub fn runs_partial(n: usize) -> String {
    format!("{n} partial")
}

pub fn runs_failed(n: usize) -> String {
    format!("{n} failed")
}

/// Shown under a host that exited with "command not found" on a snippet that
/// runs in a plain shell.
pub const SNIPPET_NOT_FOUND_HINT: &str =
    "Tip: bash or zsh host? On the Snippets tab, press e and turn on Interactive shell.";

/// Detail panel label and value for a snippet with Interactive shell on. The
/// label fits the panel's label column.
pub const SNIPPET_INTERACTIVE_LABEL: &str = "Interactive";
pub const SNIPPET_INTERACTIVE_ON: &str = "yes";

pub const OUTPUT_COPIED: &str = "Output copied.";

pub fn copy_failed(e: &impl std::fmt::Display) -> String {
    crate::tformat!("Copy failed: {}", e)
}

// ── Snippet runner errors ───────────────────────────────────────────

pub fn snippet_ssh_launch_failed(e: &impl std::fmt::Display) -> String {
    crate::tformat!("Failed to launch ssh: {}", e)
}
