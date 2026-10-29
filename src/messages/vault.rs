//! Vault SSH (cert signing) and Vault SSH library error messages. Bulk
//! signing summaries and the cancelled-summary helper also live here
//! because they are surfaced by the same UX flow.

pub const VAULT_SIGNING_CANCELLED: &str = "Vault SSH signing cancelled.";

/// Sticky error shown when bulk signing hits 3 consecutive failures and
/// gives up. `failed` is the running failure count; `last_error` carries
/// the scrubbed Vault stderr so the user can act (run `vault login`,
/// fix the address, etc.).
pub fn vault_signing_aborted(failed: u32, last_error: Option<&str>) -> String {
    format!(
        "Vault SSH signing aborted after {} consecutive failures. Press V to retry. Last error: {}",
        failed,
        last_error.unwrap_or("unknown")
    )
}

/// Status line shown after a bulk Vault SSH sign run completes. Combines
/// signed/failed/skipped counters into one line, with the first error
/// inlined when there's room. Single-host sign runs show only the error
/// (no stats prefix) because the counter would just be noise.
pub fn vault_sign_summary(
    signed: u32,
    failed: u32,
    skipped: u32,
    first_error: Option<&str>,
) -> String {
    let total = signed + failed + skipped;
    let cert_word = if total == 1 {
        "certificate"
    } else {
        "certificates"
    };
    if failed > 0 {
        if let Some(err) = first_error {
            if total == 1 {
                return err.to_string();
            }
            format!(
                "Signed {} of {} {}. {} failed: {}",
                signed, total, cert_word, failed, err
            )
        } else {
            format!(
                "Signed {} of {} {}. {} failed",
                signed, total, cert_word, failed
            )
        }
    } else if skipped > 0 && signed == 0 {
        format!(
            "All {} {} already valid. Nothing to sign.",
            total, cert_word
        )
    } else if skipped > 0 {
        format!(
            "Signed {} of {} {}. {} already valid.",
            signed, total, cert_word, skipped
        )
    } else {
        format!("Signed {} of {} {}.", signed, total, cert_word)
    }
}
pub const VAULT_NO_ROLE_CONFIGURED: &str = "No Vault SSH role configured. Set one in the host form \
     (Vault SSH role field) or on a provider for shared defaults.";
pub const VAULT_NO_HOSTS_WITH_ROLE: &str = "No hosts with a Vault SSH role configured.";
pub const VAULT_ALL_CERTS_VALID: &str = "All Vault SSH certificates are still valid.";
pub const VAULT_NO_ADDRESS: &str = "No Vault address set. Edit the host (e) or provider \
     and fill in the Vault SSH Address field.";

pub fn vault_error(msg: &str) -> String {
    format!("Vault SSH: {}", msg)
}

pub fn vault_signed(alias: &str) -> String {
    format!("Signed Vault SSH cert for {}", alias)
}

pub fn vault_sign_failed(alias: &str, message: &str) -> String {
    format!("Vault SSH: failed to sign {}: {}", alias, message)
}

pub fn vault_signing_progress(spinner: &str, done: usize, total: usize, alias: &str) -> String {
    format!(
        "{} Signing {}/{}: {} (V to cancel)",
        spinner, done, total, alias
    )
}

pub fn vault_cert_saved_host_gone(alias: &str) -> String {
    format!(
        "Vault SSH cert saved for {} but host no longer in config \
         (renamed or deleted). CertificateFile NOT written.",
        alias
    )
}

pub fn vault_spawn_failed(e: &impl std::fmt::Display) -> String {
    format!("Vault SSH: failed to spawn signing thread: {}", e)
}

pub fn vault_cert_check_failed(alias: &str, message: &str) -> String {
    format!("Cert check failed for {}: {}", alias, message)
}

pub fn vault_role_set(role: &str) -> String {
    format!("Vault SSH role set to {}.", role)
}

/// Toast shown after a successful pre-connect signing for a single host.
/// Distinct from `vault_signed` (used by bulk sign and form-submit) so the
/// connect path can mention that the cert was signed *as part of* connecting.
pub fn vault_signed_pre_connect(alias: &str) -> String {
    format!("Signed Vault SSH cert for {}.", alias)
}

/// Toast shown after a successful pre-connect signing covered multiple
/// chained hosts (target + ProxyJump hops). The `count` includes only hosts
/// that actually got a fresh cert; hosts whose cert was already valid are
/// excluded.
pub fn vault_signed_pre_connect_chain(target: &str, count: usize) -> String {
    if count <= 1 {
        format!("Signed Vault SSH cert for {}.", target)
    } else {
        format!("Signed Vault SSH certs for {} ({} hosts).", target, count)
    }
}

/// Toast shown when pre-connect signing failed for a host. Includes the
/// scrubbed Vault error so the user can act (run `vault login`, fix the
/// address, etc.). Distinct from `vault_sign_failed` so the wording can
/// reflect the connect context without breaking bulk-sign callers.
pub fn vault_sign_failed_pre_connect(alias: &str, message: &str) -> String {
    format!("Vault SSH signing failed for {}: {}", alias, message)
}

/// Toast shown when resolving the public key path for a Vault sign call
/// failed (missing pubkey, non-UTF8 path, etc.). Surfaced at the connect
/// step before any Vault round-trip happens.
pub fn vault_cert_pubkey_resolve_failed(e: &impl std::fmt::Display) -> String {
    format!("Vault SSH cert failed: {}", e)
}

/// Stderr warning emitted when a cert was signed but the matching host
/// block is no longer present (renamed or deleted between the connect
/// keypress and the signing call). The cert is still written to disk;
/// the user just has no `CertificateFile` directive pointing at it.
pub fn vault_cert_host_block_missing(alias: &str, cert_path: &std::path::Path) -> String {
    format!(
        "Warning: signed cert for {} but host block is no longer in ssh config; \
         CertificateFile not written (cert saved to {})",
        alias,
        cert_path.display()
    )
}

/// Stderr warning emitted when the cert was signed but writing the
/// updated SSH config back to disk failed.
pub fn vault_cert_config_write_failed(alias: &str, e: &impl std::fmt::Display) -> String {
    format!(
        "Warning: signed cert for {} but failed to update SSH config CertificateFile: {}",
        alias, e
    )
}

// ── Vault SSH library errors ────────────────────────────────────────
//
// Reach the user via the anyhow chain that `ensure_vault_ssh_chain_if_needed`
// turns into a toast. `vault_create_dir_failed` and `vault_write_cert_failed`
// are with_context strings, so they appear after a colon in the error chain.

pub fn vault_create_dir_failed(path: &impl std::fmt::Display) -> String {
    format!("Failed to create {}", path)
}

pub fn vault_write_cert_failed(path: &impl std::fmt::Display) -> String {
    format!("Failed to write certificate to {}", path)
}

pub fn vault_ssh_keygen_run_failed(e: &impl std::fmt::Display) -> String {
    crate::tformat!("Failed to run ssh-keygen: {}", e)
}

// ── Vault SSH bulk signing summaries (event_loop.rs) ────────────────

pub fn vault_config_reapply_failed(signed: usize, e: &impl std::fmt::Display) -> String {
    format!(
        "External edits detected; signed {} certs but failed to re-apply CertificateFile: {}",
        signed, e
    )
}

pub fn vault_external_edits_merged(summary: &str, reapplied: usize) -> String {
    format!(
        "{} External ssh config edits detected, merged {} CertificateFile directives.",
        summary, reapplied
    )
}

pub fn vault_external_edits_no_write(summary: &str) -> String {
    format!(
        "{} External ssh config edits detected; certs on disk, no CertificateFile written.",
        summary
    )
}

pub fn vault_reparse_failed(signed: usize, e: &impl std::fmt::Display) -> String {
    format!(
        "Signed {} certs but cannot re-parse ssh config after external edit: {}. \
         Certs are on disk in purple's certs directory.",
        signed, e
    )
}

pub fn vault_config_update_failed(signed: usize, e: &impl std::fmt::Display) -> String {
    format!(
        "Signed {} certs but failed to update SSH config: {}",
        signed, e
    )
}

pub fn vault_config_write_after_sign(e: &impl std::fmt::Display) -> String {
    format!("Failed to update config after vault signing: {}", e)
}

pub fn vault_config_skipped_external_change() -> &'static str {
    "Config changed on disk since signing started. Cert files are saved; re-run vault sign to wire them up."
}

// ── Vault signing cancelled summary ─────────────────────────────────

pub fn vault_signing_cancelled_summary(
    signed: u32,
    failed: u32,
    first_error: Option<&str>,
) -> String {
    let mut msg = format!(
        "Vault SSH signing cancelled ({} signed, {} failed)",
        signed, failed
    );
    if let Some(err) = first_error {
        msg.push_str(": ");
        msg.push_str(err);
    }
    msg
}
