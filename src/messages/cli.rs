// ── Add host validation ─────────────────────────────────────────

pub const ALIAS_EMPTY: &str = "Alias can't be empty. Use --alias to specify one.";
pub const ALIAS_WHITESPACE: &str =
    "Alias can't contain whitespace. Use --alias to pick a simpler name.";
pub const ALIAS_PATTERN_CHARS: &str =
    "Alias can't contain pattern characters. Use --alias to pick a different name.";
pub const HOSTNAME_WHITESPACE: &str = "Hostname can't contain whitespace.";
pub const USER_WHITESPACE: &str = "User can't contain whitespace.";
pub const PASSWORD_EMPTY: &str = "Password can't be empty.";
pub const CANCELLED: &str = "Cancelled.";
// Snippet description control-char rejection: defined once in the snippet module
// and re-exported here so the CLI add path and the TUI form share one string.
pub use super::SNIPPET_DESCRIPTION_CONTROL_CHARS as DESCRIPTION_CONTROL_CHARS;

pub use super::contains_control_chars as control_chars;

// Session Manager copy: one string each for the form and for `provider add`,
// since the consequence is the same on both routes.
pub use super::AWS_SSM_WITHOUT_PROFILE;
pub use super::aws_ssm_profile_unsafe;

pub use super::welcome_aboard as welcome;

// ── Asset generation (internal) ─────────────────────────────────

/// Confirmation printed after `purple gen-assets` writes the SVG set.
pub fn gen_assets_done(count: usize, dir: &str) -> String {
    format!("Wrote {count} SVG asset(s) to {dir}.")
}

/// Confirmation printed after `purple gen-assets --hero-out` writes the
/// animated hero SVG.
pub fn gen_assets_hero_done(path: &str) -> String {
    format!("Wrote animated hero SVG to {path}.")
}

// ── Import ──────────────────────────────────────────────────────

pub const IMPORT_NO_FILE: &str =
    "Provide a file or use --known-hosts. Run 'purple import --help' for details.";

// ── Provider CLI ────────────────────────────────────────────────

pub const NO_PROVIDERS: &str = "No providers configured. Run 'purple provider add' to set one up.";

/// All supported provider slugs as a comma-separated string. Surfaced in
/// `unknown_provider` and `skipping_unknown_provider`. Kept as a single
/// const so adding a new provider only updates one place.
pub const PROVIDER_LIST: &str = "digitalocean, vultr, linode, hetzner, upcloud, proxmox, aws, \
     scaleway, gcp, azure, tailscale, oracle, ovh, leaseweb, i3d, transip, teleport, netbox";

/// Stderr line when the user passed `--provider X` for an unknown slug.
/// Different from `skipping_unknown_provider` so each call site can
/// evolve its wording without breaking the other.
pub fn unknown_provider(name: &str) -> String {
    format!("Never heard of '{}'. Try: {}.", name, PROVIDER_LIST)
}

/// Stderr line when iterating configured providers and one is unknown
/// (e.g. config file references a since-removed provider). The sync
/// continues with the remaining providers.
pub fn skipping_unknown_provider(name: &str) -> String {
    format!(
        "Skipping unknown provider '{}'. Try: {}.",
        name, PROVIDER_LIST
    )
}

/// Stderr line printed by `purple add` when the alias already exists.
/// Tells the user the exact flag to fix it instead of just complaining.
pub fn alias_already_exists(alias: &str) -> String {
    format!(
        "'{}' already exists. Use --alias to pick a different name.",
        alias
    )
}

/// Stderr lines printed after `purple import` when some lines failed
/// to parse or read. Use the singular/plural form via the count.
pub fn import_parse_failures(count: usize) -> String {
    let s = if count == 1 { "" } else { "s" };
    format!(
        "! {} line{} could not be parsed (invalid format).",
        count, s
    )
}

pub fn import_read_errors(count: usize) -> String {
    let s = if count == 1 { "" } else { "s" };
    format!("! {} line{} could not be read (encoding error).", count, s)
}

pub fn no_config_for(provider: &str) -> String {
    format!(
        "No configuration for {}. Run 'purple provider add {}' first.",
        provider, provider
    )
}

pub fn saved_config(provider: &str) -> String {
    format!("Saved {} configuration.", provider)
}

pub fn no_config_to_remove(provider: &str) -> String {
    format!("No configuration for '{}'. Nothing to remove.", provider)
}

pub fn removed_config(provider: &str) -> String {
    format!("Removed {} configuration.", provider)
}

pub fn removed_configs(provider: &str, count: usize) -> String {
    format!("Removed {} {} configurations.", count, provider)
}

pub fn invalid_label_flag(reason: &str) -> String {
    format!("Invalid --label: {}", reason)
}

pub fn add_requires_label(provider: &str) -> String {
    format!(
        "Provider '{}' already has labeled configs. Pass --label to add another.",
        provider
    )
}

pub fn add_label_collides_with_bare(provider: &str) -> String {
    format!(
        "Provider '{}' has a bare config. Remove it first or use the TUI add flow which prompts for labels.",
        provider
    )
}

// ── Tunnel CLI ──────────────────────────────────────────────────

pub fn no_tunnels_for(alias: &str) -> String {
    format!("No tunnels configured for {}.", alias)
}

pub fn tunnels_for(alias: &str) -> String {
    format!("Tunnels for {}:", alias)
}

pub const NO_TUNNELS: &str = "No tunnels configured.";

pub fn starting_tunnel(alias: &str) -> String {
    format!("Starting tunnel for {}... (Ctrl+C to stop)", alias)
}

pub fn host_not_found(alias: &str) -> String {
    format!("No host '{}' found.", alias)
}

pub fn added_forward(forward: &str, alias: &str) -> String {
    format!("Added {} to {}.", forward, alias)
}

pub fn forward_exists(forward: &str, alias: &str) -> String {
    format!("Forward {} already exists on {}.", forward, alias)
}

pub fn forward_not_found(forward: &str, alias: &str) -> String {
    format!("No matching forward {} found on {}.", forward, alias)
}

pub fn removed_forward(forward: &str, alias: &str) -> String {
    format!("Removed {} from {}.", forward, alias)
}

pub fn no_forwards(alias: &str) -> String {
    format!("No forwarding directives configured for '{}'.", alias)
}

pub fn save_config_failed(e: &impl std::fmt::Display) -> String {
    format!("Failed to save config: {}", e)
}

pub fn included_host_read_only(alias: &str) -> String {
    format!(
        "Host '{}' is from an included file and cannot be modified.",
        alias
    )
}

pub fn operation_failed(e: &impl std::fmt::Display) -> String {
    format!("Failed: {}", e)
}

// ── Snippet CLI ─────────────────────────────────────────────────

pub const NO_SNIPPETS: &str = "No snippets configured. Use 'purple snippet add' to create one.";

pub use super::snippet_added;
pub use super::snippet_removed;
pub use super::snippet_updated;

/// CLI wording of `SNIPPET_NOT_FOUND_HINT`: the CLI changes a snippet through
/// `snippet add` instead of the edit form.
pub const SNIPPET_NOT_FOUND_HINT: &str = "Tip: bash or zsh host? Run purple snippet add again \
     with --interactive (and --description, if it has one) to load ~/.bashrc or ~/.zshrc.";

pub fn snippet_not_found(name: &str) -> String {
    format!("No snippet '{}' found.", name)
}

pub fn no_hosts_with_tag(tag: &str) -> String {
    format!("No hosts found with tag '{}'.", tag)
}

pub const SPECIFY_TARGET: &str = "Specify a host alias, --tag or --all.";

// ── Run/exec output ─────────────────────────────────────────────

pub fn beaming_up(alias: &str) -> String {
    format!("Beaming you up to {}...\n", alias)
}

pub fn running_snippet_on(name: &str, alias: &str) -> String {
    format!("Running '{}' on {}...\n", name, alias)
}

pub fn host_separator(alias: &str) -> String {
    format!("── {} ──", alias)
}

pub fn exited_with_code(code: i32) -> String {
    format!("Exited with code {}.", code)
}

pub const DONE: &str = "Done.";

pub fn done_multi(name: &str, count: usize) -> String {
    format!("Done. Ran '{}' on {} hosts.", name, count)
}

pub const PRESS_ENTER: &str = "Press Enter to continue...";

pub fn host_failed(alias: &str, e: &impl std::fmt::Display) -> String {
    format!("[{}] Failed: {}", alias, e)
}

pub fn skipping_host(alias: &str, e: &impl std::fmt::Display) -> String {
    format!("Skipping {}: {}", alias, e)
}

// ── Password CLI ────────────────────────────────────────────────

pub fn password_removed(alias: &str) -> String {
    crate::tformat!("Password removed for {}.", alias)
}

// ── Log CLI ─────────────────────────────────────────────────────

pub fn log_deleted(path: &impl std::fmt::Display) -> String {
    format!("Log file deleted: {}", path)
}

pub fn no_log_file(path: &impl std::fmt::Display) -> String {
    format!("No log file found at {}", path)
}

// ── Theme CLI ───────────────────────────────────────────────────

pub const BUILTIN_THEMES: &str = "Built-in themes:";
pub const CUSTOM_THEMES: &str = "\nCustom themes:";

pub fn theme_set(name: &str) -> String {
    format!("Theme set to: {}", name)
}

// ── Sync output ─────────────────────────────────────────────────

pub fn syncing(name: &str, summary: &str) -> String {
    format!("\x1b[2K\rSyncing {}... {}", name, summary)
}

/// One-shot "Syncing X... " prefix without the cursor-clear/CR escapes.
/// Used before progress callbacks start emitting overwrite-style updates,
/// so the user sees something happening even if the provider is slow to
/// produce the first progress event.
pub fn syncing_start(name: &str) -> String {
    format!("Syncing {}... ", name)
}

/// Rendered before the dot-progress (`\u{2713}` or error) on each
/// per-host vault sign in the CLI bulk path. The trailing space is
/// intentional — the success/fail glyph follows on the same line.
pub fn vault_signing_host(alias: &str) -> String {
    format!("Signing {}... ", alias)
}

/// Stderr line emitted by `purple vault sign --all` when a host block
/// disappeared between the moment we enumerated it and the moment we
/// tried to write its CertificateFile (rename, delete, race with another
/// process). The cert is on disk; only the wiring is missing.
pub fn vault_sign_host_block_gone(alias: &str) -> String {
    format!(
        "  warning: {} no longer in ssh config; CertificateFile not written (cert saved on disk)",
        alias
    )
}

/// Single-word "failed." status the CLI sync output appends when a
/// provider fetch hit a hard error. Mirrors the trailing-status pattern
/// used by `syncing_start` so the line reads `Syncing X... failed.`.
pub const SYNC_FAILED: &str = "failed.";

pub fn servers_found_with_failures(count: usize, failures: usize, total: usize) -> String {
    format!(
        "{} servers found ({} of {} failed to fetch).",
        count, failures, total
    )
}

pub fn servers_found(count: usize) -> String {
    format!("{} servers found.", count)
}

pub fn sync_result(prefix: &str, added: usize, updated: usize, unchanged: usize) -> String {
    format!(
        "{}Added {}, updated {}, unchanged {}.",
        prefix, added, updated, unchanged
    )
}

pub fn sync_removed(count: usize) -> String {
    format!("  Removed {}.", count)
}

pub fn sync_stale(count: usize) -> String {
    format!("  Marked {} stale.", count)
}

pub fn sync_skip_remove(display_name: &str) -> String {
    format!(
        "! {}: skipping --remove due to partial failures.",
        display_name
    )
}

pub fn sync_error(display_name: &str, e: &impl std::fmt::Display) -> String {
    format!("! {}: {}", display_name, e)
}

pub const SYNC_SKIP_WRITE: &str =
    "! Skipping config write due to sync failures. Fix the errors and re-run.";

// ── Provider validation (CLI) ───────────────────────────────────

pub const PROXMOX_URL_REQUIRED: &str =
    "Proxmox requires --url (e.g. --url https://pve.example.com:8006).";
pub const NETBOX_URL_REQUIRED: &str =
    "NetBox requires --url (e.g. --url https://netbox.example.com).";
pub const AWS_REGIONS_REQUIRED: &str =
    "AWS requires --regions (e.g. --regions us-east-1,eu-west-1).";
pub const AZURE_REGIONS_REQUIRED: &str =
    "Azure requires --regions with one or more subscription IDs.";
pub const GCP_PROJECT_REQUIRED: &str = "GCP requires --project (e.g. --project my-gcp-project-id).";
pub use super::ALIAS_PREFIX_INVALID;

/// An unknown `--ssm` value. Refused rather than read as off, because a typo
/// would otherwise leave every host on its IP address without saying so.
pub fn ssm_mode_invalid(value: &str) -> String {
    format!(
        "Unknown --ssm value '{}'. Use off, auto or always.",
        value.trim()
    )
}

/// One line after a bare config is updated, naming the route that keeps a
/// second account alongside it instead of replacing this one.
///
/// Not `--label`: a labeled add is refused while a bare config exists, because
/// the two spellings cannot be mixed. Labeling the existing config renames it
/// and rewrites its host markers, which the TUI asks about and a one-shot CLI
/// add cannot.
pub fn provider_replaced_hint(provider: &str) -> String {
    format!(
        "Updated the existing [{}] config in place. To keep two side by side, add the second one from the TUI with 'a' in the provider list, which names this config first.",
        provider
    )
}

pub const WARN_URL_NOT_USED: &str =
    "Warning: --url is only used by self-hosted providers (Proxmox, NetBox). Ignoring.";
pub const WARN_PROFILE_NOT_USED: &str =
    "Warning: --profile is only used by the AWS provider. Ignoring.";
pub const WARN_SSM_NOT_USED: &str = "Warning: --ssm is only used by the AWS provider. Ignoring.";
pub const WARN_PROJECT_NOT_USED: &str =
    "Warning: --project is only used by the GCP provider. Ignoring.";
pub const WARN_COMPARTMENT_NOT_USED: &str =
    "Warning: --compartment is only used by the Oracle provider. Ignoring.";
pub const WARN_NO_VERIFY_TLS_NOT_USED: &str =
    "Warning: --no-verify-tls is only used by self-hosted providers (Proxmox, NetBox). Ignoring.";
pub const WARN_VERIFY_TLS_NOT_USED: &str =
    "Warning: --verify-tls is only used by self-hosted providers (Proxmox, NetBox). Ignoring.";
pub const WARN_FILTER_NOT_USED: &str =
    "Warning: --filter is only used by the NetBox provider. Ignoring.";
pub const WARN_REGIONS_NOT_USED: &str = "Warning: --regions is only used by the AWS, Scaleway, GCP, Azure and Oracle providers. \
     Ignoring.";

/// A token was asked for but resolved to nothing. Valid for providers that
/// read credentials elsewhere, so the save goes through. Saying so out loud
/// separates a deliberate clear from a script that piped a blank secret.
pub fn warn_token_resolved_empty(display_name: &str) -> String {
    format!(
        "Warning: empty token. {} will look for credentials elsewhere when it syncs.",
        display_name
    )
}

/// Same case, except a stored token gets overwritten by the empty one. The
/// loss is the part worth naming: nothing else reports it.
pub fn warn_token_replaced_with_empty(display_name: &str) -> String {
    format!(
        "Warning: empty token replaces the one stored for {}. It will look for credentials elsewhere when it syncs.",
        display_name
    )
}

/// Per-host status prefixes for `purple sync` output. Indented two
/// spaces so the result line aligns under the `Syncing X...` header.
/// `--dry-run` mode prepends "Would have:" so the user knows nothing
/// was written.
pub const SYNC_RESULT_PREFIX_LIVE: &str = "  ";
pub const SYNC_RESULT_PREFIX_DRY_RUN: &str = "  Would have: ";

/// Stderr line printed by `purple provider add` when the user-supplied
/// URL does not start with `https://`. CLI-flavoured: tells the user to
/// use the `--no-verify-tls` flag, distinct from the TUI variant which
/// references a Verify TLS toggle.
pub const PROVIDER_URL_REQUIRES_HTTPS: &str =
    "URL must start with https://. For self-signed certificates use --no-verify-tls.";

/// Reuse the TUI form's `Token can't be empty...` lines verbatim — the
/// remediation steps (paste a JSON file path, paste an OCI config path,
/// grab a token from the dashboard) are identical between CLI and TUI.
pub use super::PROVIDER_TOKEN_REQUIRED_GCP;
pub use super::PROVIDER_TOKEN_REQUIRED_ORACLE;
pub use super::azure_subscription_id_invalid;
pub use super::provider_token_required;

/// Stderr line printed when `purple provider add scaleway` is missing
/// `--regions`. Mirrors the Azure/GCP/Oracle pattern of including the
/// concrete flag form in the message.
pub const SCALEWAY_REGIONS_REQUIRED: &str = "Scaleway requires --regions with one or more zones \
     (e.g. --regions fr-par-1,nl-ams-1).";

pub const ORACLE_COMPARTMENT_REQUIRED: &str =
    "Oracle requires --compartment (e.g. --compartment ocid1.compartment.oc1..aaa...).";

// ── Vault CLI ───────────────────────────────────────────────────

pub fn vault_no_role(alias: &str) -> String {
    format!(
        "No Vault SSH role configured for '{}'. Set it in the host form \
         (Vault SSH Role field) or in the provider config (vault_role).",
        alias
    )
}

pub fn vault_cert_signed(path: &impl std::fmt::Display) -> String {
    format!("Certificate signed: {}", path)
}

pub fn vault_sign_failed(e: &impl std::fmt::Display) -> String {
    format!("failed: {}", e)
}

pub fn vault_config_update_warning(e: &impl std::fmt::Display) -> String {
    format!("Warning: Failed to update SSH config: {}", e)
}

// ── List hosts ──────────────────────────────────────────────────

pub const NO_HOSTS: &str = "No hosts configured. Run 'purple' to add some!";

// ── Token ───────────────────────────────────────────────────────

pub const NO_TOKEN: &str =
    "No token provided. Use --token, --token-stdin, or set PURPLE_TOKEN env var.";

// ── What's new (CLI) ────────────────────────────────────────────

pub mod whats_new {
    pub const HEADER: &str = "purple release notes";
}
