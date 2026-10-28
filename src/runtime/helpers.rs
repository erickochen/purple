use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result};
use log::{debug, warn};

use crate::app::{self, App};
use crate::{askpass, cli, providers, ssh_config, vault_ssh};

/// Environment override for the SSH config path.
pub const SSH_CONFIG_ENV_VAR: &str = "PURPLE_SSH_CONFIG";
/// Preference key in `~/.purple/preferences`.
pub const SSH_CONFIG_PREF_KEY: &str = "ssh_config";
/// The default SSH config path as shown to the user.
pub const SSH_CONFIG_DEFAULT_DISPLAY: &str = "~/.ssh/config";

/// Where the SSH config path came from. Named in logs and in the
/// missing-file notice so a typo is found in the right place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    Flag,
    EnvVar,
    Preference,
    Default,
}

impl std::fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigSource::Flag => write!(f, "--config"),
            ConfigSource::EnvVar => write!(f, "{SSH_CONFIG_ENV_VAR}"),
            ConfigSource::Preference => write!(f, "preference {SSH_CONFIG_PREF_KEY}"),
            ConfigSource::Default => write!(f, "default"),
        }
    }
}

/// The SSH config path plus the source it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedConfigPath {
    pub path: PathBuf,
    pub source: ConfigSource,
}

/// Flag, then `PURPLE_SSH_CONFIG`, then the `ssh_config` preference, then
/// `~/.ssh/config`. Blank values read as unset.
pub fn resolve_ssh_config_path(
    env: &crate::runtime::env::Env,
    flag: Option<&str>,
) -> Result<ResolvedConfigPath> {
    let paths = env.paths();
    let flag = flag.map(str::trim).filter(|s| !s.is_empty());
    let env_value = env
        .var(SSH_CONFIG_ENV_VAR)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let chosen = match (flag, env_value) {
        (Some(v), _) => Some((v.to_string(), ConfigSource::Flag)),
        (None, Some(v)) => Some((v.to_string(), ConfigSource::EnvVar)),
        (None, None) => {
            crate::preferences::load_ssh_config_path(paths).map(|v| (v, ConfigSource::Preference))
        }
    };
    let Some((value, source)) = chosen else {
        // `ssh_dir()` keeps this identical to the default `-F` comparison.
        let path = paths
            .map(|p| p.ssh_dir().join("config"))
            .context("Could not determine home directory")?;
        return Ok(ResolvedConfigPath {
            path,
            source: ConfigSource::Default,
        });
    };
    let path = expand_user_path(paths, &value)?;
    debug!("[purple] ssh config from {source}: {}", path.display());
    Ok(ResolvedConfigPath { path, source })
}

/// Expand `~/`, `${HOME}/` and `$HOME/` prefixes against the user's home
/// directory. MCPB clients (e.g. Claude Desktop) do not always substitute
/// `${HOME}` before passing CLI args, so the binary must handle it.
pub fn expand_user_path(paths: Option<&crate::runtime::env::Paths>, path: &str) -> Result<PathBuf> {
    let home = || {
        paths
            .map(|p| p.home().to_path_buf())
            .context("Could not determine home directory")
    };
    let home_prefixes = ["~/", "${HOME}/", "$HOME/"];
    for prefix in home_prefixes {
        if let Some(rest) = path.strip_prefix(prefix) {
            return Ok(home()?.join(rest));
        }
    }
    if path == "~" || path == "${HOME}" || path == "$HOME" {
        return home();
    }
    Ok(PathBuf::from(path))
}

pub fn resolve_token(
    env: &crate::runtime::env::Env,
    explicit: Option<String>,
    from_stdin: bool,
) -> Result<String> {
    if let Some(t) = explicit {
        return Ok(t);
    }
    if from_stdin {
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf)?;
        return Ok(buf.trim().to_string());
    }
    if let Some(t) = env.purple_token() {
        return Ok(t.to_string());
    }
    anyhow::bail!("{}", crate::messages::cli::NO_TOKEN)
}

/// Replace the spinner frame prefix in a status text. Returns None if the
/// text does not start with a known spinner frame.
///
/// Animated statuses MUST start with a character from
/// [`crate::animation::SPINNER_FRAMES`] followed by a space, otherwise
/// `event_loop::handle_tick` cannot rotate the frame and the animation
/// silently stops.
pub fn replace_spinner_frame(text: &str, new_frame: &str) -> Option<String> {
    let starts_with_spinner = crate::animation::SPINNER_FRAMES
        .iter()
        .any(|f| text.starts_with(f));
    if !starts_with_spinner {
        return None;
    }
    text.split_once(' ')
        .map(|(_, rest)| format!("{} {}", new_frame, rest))
}

/// Thin re-export. The real implementation lives in `crate::messages` so
/// every user-facing string funnels through one module.
pub fn format_vault_sign_summary(
    signed: u32,
    failed: u32,
    skipped: u32,
    first_error: Option<&str>,
) -> String {
    crate::messages::vault_sign_summary(signed, failed, skipped, first_error)
}

pub fn format_sync_diff(added: usize, updated: usize, stale: usize) -> String {
    let diff_parts: Vec<String> = [(added, "+"), (updated, "~"), (stale, "-")]
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, prefix)| format!("{}{}", prefix, n))
        .collect();
    if diff_parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", diff_parts.join(" "))
    }
}

/// Footer status that surfaces in-flight providers as the batch progresses.
/// While a sync is running the line is `⠋ Syncing AWS, Hetzner · 1/3 (+12 ~3 -1)`,
/// where the leading char is a braille spinner frame rotated by
/// `event_loop::handle_tick` and the names are the providers that have not yet
/// reported back. Once every provider in the batch has resolved the line
/// becomes `Synced 5/5 · AWS, DO, Vultr, Hetzner, Linode (+12 ~3 -1)` and the
/// batch state resets. Persists `sync_history.tsv` on completion.
pub fn set_sync_summary(app: &mut App) {
    let still_syncing = !app.providers.syncing().is_empty();
    let done = app.providers.sync_done().len();
    let total = app
        .providers
        .batch_total()
        .max(done + app.providers.syncing().len());
    let added = app.providers.batch_added();
    let updated = app.providers.batch_updated();
    let stale = app.providers.batch_stale();
    if still_syncing {
        let mut active: Vec<String> = app
            .providers
            .syncing()
            .keys()
            .map(|name| crate::providers::provider_display_name(name).to_string())
            .collect();
        active.sort();
        let active_names = active.join(", ");
        let spinner = crate::animation::SPINNER_FRAMES[0];
        let text = crate::messages::synced_progress(
            spinner,
            &active_names,
            done,
            total,
            added,
            updated,
            stale,
        );
        if app.providers.sync_had_errors() {
            app.notify_background_error(text);
        } else {
            app.notify_background(text);
        }
    } else {
        let names = app.providers.sync_done().join(", ");
        let text = crate::messages::synced_done(done, total, &names, added, updated, stale);
        if app.providers.sync_had_errors() {
            app.notify_background_error(text);
        } else {
            app.notify_background(text);
        }
        app::SyncRecord::save_all(app.providers.sync_history(), app.env().paths());
        app.providers.finish_batch();
    }
}

/// First-launch initialization: create the data directory and back up the
/// original SSH config there. Returns `Some(has_backup)` on a first launch.
/// Returns `None` once any purple file exists.
pub fn first_launch_init(paths: &crate::runtime::env::Paths, config_path: &Path) -> Option<bool> {
    let markers = [
        paths.config_original(),
        paths.preferences(),
        paths.history(),
        paths.container_cache(),
        paths.last_version_check(),
        paths.providers_config(),
        paths.snippets_file(),
        paths.themes_dir(),
    ];
    if markers.iter().any(|m| m.exists()) {
        return None;
    }
    // The directory may already exist (logging opened its file moments
    // ago), so set the mode explicitly instead of only on creation.
    let data_dir = paths.data_dir();
    if let Err(e) = std::fs::create_dir_all(data_dir) {
        warn!(
            "[config] Failed to create data directory {}: {e}",
            data_dir.display()
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(data_dir, std::fs::Permissions::from_mode(0o700)) {
            warn!(
                "[config] Failed to set data directory permissions on {}: {e}",
                data_dir.display()
            );
        }
    }
    let original_backup = paths.config_original();
    if config_path.exists() {
        if let Err(e) = std::fs::copy(config_path, &original_backup) {
            warn!(
                "[config] Failed to backup SSH config to {}: {e}",
                original_backup.display()
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(e) =
                std::fs::set_permissions(&original_backup, std::fs::Permissions::from_mode(0o600))
            {
                warn!("[config] Failed to set backup permissions: {e}");
            }
        }
    }
    Some(original_backup.exists())
}

/// Check and renew Vault SSH certificate if the host has a vault role configured.
/// Writes the cert file to ~/.purple/certs/ AND sets CertificateFile on the host
/// block when it is empty, so `ssh` actually uses the freshly signed cert.
pub fn ensure_vault_ssh_if_needed(
    env: &crate::runtime::env::Env,
    alias: &str,
    host: &ssh_config::model::HostEntry,
    provider_config: &providers::config::ProviderConfig,
    config: &mut ssh_config::model::SshConfigFile,
) -> Option<(String, bool)> {
    let role = vault_ssh::resolve_vault_role(
        host.vault_ssh.as_deref(),
        host.provider.as_deref(),
        host.provider_label.as_deref(),
        provider_config,
    )?;

    let pubkey = match vault_ssh::resolve_pubkey_path(env.paths(), &host.identity_file) {
        Ok(p) => p,
        Err(e) => {
            return Some((crate::messages::vault_cert_pubkey_resolve_failed(&e), true));
        }
    };

    let check_path =
        vault_ssh::resolve_cert_path(env.paths(), alias, &host.certificate_file).ok()?;
    let status = vault_ssh::check_cert_validity(env, &check_path);
    if !vault_ssh::needs_renewal(&status) {
        return None;
    }

    let vault_addr = vault_ssh::resolve_vault_addr(
        host.vault_addr.as_deref(),
        host.provider.as_deref(),
        host.provider_label.as_deref(),
        provider_config,
    );
    match vault_ssh::ensure_cert(
        env,
        &role,
        &pubkey,
        alias,
        &host.certificate_file,
        vault_addr.as_deref(),
    ) {
        Ok(cert_path) => {
            if should_write_certificate_file(&host.certificate_file) {
                let cert_str = cert_path.to_string_lossy().to_string();
                let updated = config.set_host_certificate_file(alias, &cert_str);
                if !updated {
                    eprintln!(
                        "{}",
                        crate::messages::vault_cert_host_block_missing(alias, &cert_path)
                    );
                } else if let Err(e) = config.write() {
                    eprintln!(
                        "{}",
                        crate::messages::vault_cert_config_write_failed(alias, &e)
                    );
                }
            }
            Some((crate::messages::vault_signed_pre_connect(alias), false))
        }
        Err(e) => {
            let msg = e.to_string();
            eprintln!(
                "{}",
                crate::messages::vault_sign_failed_pre_connect(alias, &msg)
            );
            Some((
                crate::messages::vault_sign_failed_pre_connect(alias, &msg),
                true,
            ))
        }
    }
}

/// Resolve the effective ProxyJump chain for `target_alias` and run
/// `ensure_vault_ssh_if_needed` for every host in it.
pub fn ensure_vault_ssh_chain_if_needed(
    env: &crate::runtime::env::Env,
    target_alias: &str,
    config_path: &Path,
    provider_config: &providers::config::ProviderConfig,
    config: &mut ssh_config::model::SshConfigFile,
) -> Option<(String, bool)> {
    let chain = vault_ssh::resolve_proxy_chain(config_path, target_alias);
    let mut signed_count: usize = 0;
    let mut last_error: Option<String> = None;

    for hop_alias in &chain {
        let host_entry = config
            .host_entries()
            .into_iter()
            .find(|h| h.alias == *hop_alias);
        let Some(host) = host_entry else {
            continue;
        };
        if let Some((msg, is_error)) =
            ensure_vault_ssh_if_needed(env, hop_alias, &host, provider_config, config)
        {
            if is_error {
                last_error = Some(msg);
            } else {
                signed_count += 1;
            }
        }
    }

    if let Some(err) = last_error {
        return Some((err, true));
    }
    if signed_count == 0 {
        return None;
    }
    Some((
        crate::messages::vault_signed_pre_connect_chain(target_alias, signed_count),
        false,
    ))
}

/// Per-alias locks serializing concurrent Vault SSH renewals. Container
/// listing, inspect, logs and action fetches run on separate worker threads
/// and can target the same host at once. Without this, two threads would
/// sign the same cert in parallel and both rewrite the sacred ~/.ssh/config.
static RENEWAL_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();

fn renewal_lock(alias: &str) -> Arc<Mutex<()>> {
    RENEWAL_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .entry(alias.to_string())
        .or_default()
        .clone()
}

/// Worker-thread-safe Vault SSH renewal for a single host alias.
///
/// The connect path renews via `ensure_vault_ssh_chain_if_needed` with the
/// main-thread App config in hand. The SSH chokepoints (container listing,
/// exec, logs, actions and the file browser) run on worker threads with no
/// App access, so this loads the provider and SSH config from disk itself.
/// Serialized per alias so concurrent fetches never sign the same cert twice.
/// Returns the same `(message, is_error)` summary as the chain helper;
/// chokepoints log it and continue, the SSH call surfaces any real failure.
pub fn ensure_vault_cert_for_alias(
    env: &crate::runtime::env::Env,
    alias: &str,
    config_path: &Path,
) -> Option<(String, bool)> {
    let lock = renewal_lock(alias);
    let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());

    let mut config = match ssh_config::model::SshConfigFile::parse_with_env(config_path, env) {
        Ok(c) => c,
        Err(e) => {
            warn!("[config] Vault SSH renewal skipped for '{alias}': {e}");
            return None;
        }
    };
    let provider_config = providers::config::ProviderConfig::load(env.paths());
    let result =
        ensure_vault_ssh_chain_if_needed(env, alias, config_path, &provider_config, &mut config);
    match &result {
        Some((msg, true)) => warn!("[external] Vault SSH renewal for '{alias}': {msg}"),
        Some((msg, false)) => debug!("[external] Vault SSH renewal for '{alias}': {msg}"),
        None => {}
    }
    result
}

/// Decide whether to write a `CertificateFile` directive after a successful
/// Vault SSH signing. Only write when the host has no existing
/// `CertificateFile`. A user-set custom path must never be silently
/// overwritten with purple's default cert path. Whitespace-only values count
/// as empty.
pub fn should_write_certificate_file(existing: &str) -> bool {
    existing.trim().is_empty()
}

/// Pre-flight check for Bitwarden vault. If the askpass source uses `bw:` and
/// no session token is cached, prompts the user to unlock the vault.
pub fn ensure_bw_session(
    env: &crate::runtime::env::Env,
    existing: Option<&str>,
    askpass: Option<&str>,
) -> Option<String> {
    let askpass = askpass?;
    if !askpass.starts_with("bw:") || existing.is_some() {
        return None;
    }
    let status = askpass::bw_vault_status(env);
    match status {
        askpass::BwStatus::Unlocked => None,
        askpass::BwStatus::NotInstalled => {
            eprintln!("{}", crate::messages::askpass::BW_NOT_FOUND);
            None
        }
        askpass::BwStatus::NotAuthenticated => {
            eprintln!("{}", crate::messages::askpass::BW_NOT_LOGGED_IN);
            None
        }
        askpass::BwStatus::Locked => {
            for attempt in 0..2 {
                let password = match cli::prompt_hidden_input("Bitwarden master password: ") {
                    Ok(Some(p)) if !p.is_empty() => p,
                    Ok(Some(_)) => {
                        eprintln!("{}", crate::messages::askpass::EMPTY_PASSWORD);
                        return None;
                    }
                    Ok(None) => return None,
                    Err(e) => {
                        eprintln!("{}", crate::messages::askpass::read_failed(&e));
                        return None;
                    }
                };
                match askpass::bw_unlock(env, &password) {
                    Ok(token) => return Some(token),
                    Err(e) => {
                        if attempt == 0 {
                            eprintln!("{}", crate::messages::askpass::unlock_failed_retry(&e));
                        } else {
                            eprintln!("{}", crate::messages::askpass::unlock_failed_prompt(&e));
                        }
                    }
                }
            }
            None
        }
    }
}

/// Pre-flight Proton Pass login. If the askpass source is `proton:` and the
/// CLI is installed but the user is not authenticated, prompt for a Personal
/// Access Token on stdin and run `pass-cli login`.
pub fn ensure_proton_login(env: &crate::runtime::env::Env, askpass: Option<&str>) {
    ensure_proton_login_with(
        env,
        askpass,
        || askpass::proton_status(env),
        || cli::prompt_hidden_input(crate::messages::askpass::PROTON_LOGIN_PROMPT),
    );
}

/// Test seam for `ensure_proton_login`. Inject the status check and the PAT
/// prompt so the routing logic can be exercised without a real `pass-cli` or a
/// real stdin tty.
pub fn ensure_proton_login_with<S, P>(
    env: &crate::runtime::env::Env,
    askpass: Option<&str>,
    status_fn: S,
    mut prompt_pat: P,
) where
    S: FnOnce() -> askpass::ProtonStatus,
    P: FnMut() -> Result<Option<String>>,
{
    let Some(askpass) = askpass else {
        return;
    };
    if !askpass.starts_with("proton:") {
        return;
    }
    match status_fn() {
        askpass::ProtonStatus::Authenticated => {
            debug!("[external] Proton Pass pre-flight: already authenticated");
        }
        askpass::ProtonStatus::NotInstalled => {
            debug!("[config] Proton Pass pre-flight: pass-cli not installed");
            eprintln!("{}", crate::messages::askpass::PROTON_NOT_FOUND);
        }
        askpass::ProtonStatus::NotAuthenticated => {
            debug!("[external] Proton Pass pre-flight: not authenticated, prompting for PAT");
            for attempt in 0..2 {
                let pat = match prompt_pat() {
                    Ok(Some(p)) if !p.is_empty() => p,
                    Ok(Some(_)) => {
                        debug!("[external] Proton Pass pre-flight: empty PAT, aborting");
                        eprintln!("{}", crate::messages::askpass::EMPTY_PASSWORD);
                        return;
                    }
                    Ok(None) => {
                        debug!("[external] Proton Pass pre-flight: PAT prompt dismissed (Esc/EOF)");
                        return;
                    }
                    Err(e) => {
                        warn!("[external] Proton Pass PAT prompt read failed: {e}");
                        eprintln!("{}", crate::messages::askpass::read_failed(&e));
                        return;
                    }
                };
                match askpass::proton_login(env, &pat) {
                    Ok(()) => {
                        debug!(
                            "[external] Proton Pass pre-flight: login succeeded on attempt {attempt}"
                        );
                        eprintln!("{}", crate::messages::askpass::PROTON_LOGIN_SUCCESS);
                        return;
                    }
                    Err(e) => {
                        debug!(
                            "[external] Proton Pass pre-flight: login attempt {attempt} failed: {e}"
                        );
                        if attempt == 0 {
                            eprintln!(
                                "{}",
                                crate::messages::askpass::proton_login_failed_retry(&e)
                            );
                        } else {
                            warn!("[external] Proton Pass login failed after retries: {e}");
                            eprintln!(
                                "{}",
                                crate::messages::askpass::proton_login_failed_prompt(&e)
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Apply saved sort/group/view preferences to a fresh App. Reads
/// `~/.purple/preferences`, restores sort/group/view modes, clears stale
/// group keys, and re-runs `apply_sort` plus `select_first_host` so the
/// first row visible after startup matches the saved sort order.
pub fn apply_saved_sort(app: &mut App) {
    let paths = app.env().paths().cloned();
    let p = paths.as_ref();
    let saved = crate::preferences::load_sort_mode(p);
    let group = crate::preferences::load_group_by(p);
    app.hosts_state.set_sort_mode(saved);
    app.hosts_state.set_group_by_raw(group);
    app.hosts_state
        .set_view_mode(crate::preferences::load_view_mode(p));
    app.containers_overview.hydrate_from_prefs(p);
    if app.clear_stale_group_tag()
        && let Err(e) = crate::preferences::save_group_by(p, app.hosts_state.group_by())
    {
        app.notify_error(crate::messages::group_pref_reset_failed(&e));
    }
    if saved != app::SortMode::Original || !matches!(app.hosts_state.group_by(), app::GroupBy::None)
    {
        app.apply_sort();
        app.select_first_host();
    }
}

/// Pre-flight check for keychain password. If the askpass source is `keychain` and
/// no password is stored yet, prompts the user to enter one and stores it.
pub fn ensure_keychain_password(
    env: &crate::runtime::env::Env,
    alias: &str,
    askpass: Option<&str>,
) {
    if askpass != Some("keychain") {
        return;
    }
    if askpass::keychain_has_password(env, alias) {
        return;
    }
    let password = match cli::prompt_hidden_input(
        &crate::messages::askpass::keychain_password_prompt(alias),
    ) {
        Ok(Some(p)) if !p.is_empty() => p,
        Ok(Some(_)) => {
            eprintln!("{}", crate::messages::askpass::EMPTY_PASSWORD);
            return;
        }
        Ok(None) => return,
        Err(_) => return,
    };
    match askpass::store_in_keychain(env, alias, &password) {
        Ok(()) => eprintln!("{}", crate::messages::askpass::PASSWORD_IN_KEYCHAIN),
        Err(e) => eprintln!("{}", crate::messages::askpass::keychain_store_failed(&e)),
    }
}

#[cfg(test)]
#[path = "../main_tests.rs"]
mod tests;
