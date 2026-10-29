//! CLI subcommand handlers. Each function handles one clap subcommand
//! (provider, tunnel, password, snippet, add, import, sync, logs, theme,
//! vault sign) and runs outside the TUI in a non-interactive terminal context.

use anyhow::{Context, Result};
use std::path::Path;

use crate::providers;
use crate::providers::ProviderKind;
use crate::snippet;
use crate::ssh_config::model::{HostEntry, SshConfigFile};
use crate::vault_ssh;

use super::cli_args::{
    PasswordCommands, ProviderCommands, SnippetCommands, ThemeCommands, TunnelCommands,
};
use super::{askpass, import, logging, preferences, quick_add, should_write_certificate_file, ui};

pub fn handle_quick_add(
    mut config: SshConfigFile,
    target: &str,
    alias: Option<&str>,
    key: Option<&str>,
) -> Result<()> {
    log::info!(
        "[purple] cli add: target={} alias={:?} key={:?}",
        target,
        alias,
        key
    );
    let parsed = quick_add::parse_target(target).map_err(|e| anyhow::anyhow!(e))?;

    let alias_str = alias.map(|a| a.to_string()).unwrap_or_else(|| {
        parsed
            .hostname
            .split('.')
            .next()
            .unwrap_or(&parsed.hostname)
            .to_string()
    });

    if alias_str.trim().is_empty() {
        eprintln!("{}", crate::i18n::text(crate::messages::cli::ALIAS_EMPTY));
        std::process::exit(1);
    }
    if alias_str.contains(char::is_whitespace) {
        eprintln!(
            "{}",
            crate::i18n::text(crate::messages::cli::ALIAS_WHITESPACE)
        );
        std::process::exit(1);
    }
    if crate::ssh_config::model::is_host_pattern(&alias_str) {
        eprintln!(
            "{}",
            crate::i18n::text(crate::messages::cli::ALIAS_PATTERN_CHARS)
        );
        std::process::exit(1);
    }

    // Reject control characters in alias, hostname, user and key
    let key_val = key.unwrap_or("").to_string();
    for (value, name) in [
        (&alias_str, "Alias"),
        (&parsed.hostname, "Hostname"),
        (&parsed.user, "User"),
        (&key_val, "Identity file"),
    ] {
        if value.chars().any(|c| c.is_control()) {
            eprintln!("{}", crate::messages::cli::control_chars(name));
            std::process::exit(1);
        }
    }

    // Reject whitespace in hostname and user (matches TUI validation)
    if parsed.hostname.contains(char::is_whitespace) {
        eprintln!(
            "{}",
            crate::i18n::text(crate::messages::cli::HOSTNAME_WHITESPACE)
        );
        std::process::exit(1);
    }
    if parsed.user.contains(char::is_whitespace) {
        eprintln!(
            "{}",
            crate::i18n::text(crate::messages::cli::USER_WHITESPACE)
        );
        std::process::exit(1);
    }

    if config.has_host(&alias_str) {
        eprintln!("{}", crate::messages::cli::alias_already_exists(&alias_str));
        std::process::exit(1);
    }

    let entry = HostEntry {
        alias: alias_str.clone(),
        hostname: parsed.hostname,
        user: parsed.user,
        port: parsed.port,
        identity_file: key_val,
        ..Default::default()
    };

    config.add_host(&entry);
    log::debug!("[config] cli add: writing ssh config (alias={})", alias_str);
    config.write()?;
    log::info!("[purple] cli add: host added alias={}", alias_str);
    println!("{}", crate::messages::cli::welcome(&alias_str));
    Ok(())
}

pub fn handle_import(
    env: &crate::runtime::env::Env,
    mut config: SshConfigFile,
    file: Option<&str>,
    known_hosts: bool,
    group: Option<&str>,
) -> Result<()> {
    log::info!(
        "[purple] cli import: source={} group={:?}",
        if known_hosts {
            "known_hosts".to_string()
        } else {
            file.unwrap_or("(missing)").to_string()
        },
        group
    );
    let result = if known_hosts {
        import::import_from_known_hosts(env.paths(), &mut config, group)
    } else if let Some(path) = file {
        let resolved = super::expand_user_path(env.paths(), path)?;
        import::import_from_file(&mut config, &resolved, group)
    } else {
        eprintln!(
            "{}",
            crate::i18n::text(crate::messages::cli::IMPORT_NO_FILE)
        );
        std::process::exit(1);
    };

    match result {
        Ok((imported, skipped, parse_failures, read_errors)) => {
            if imported > 0 {
                log::debug!(
                    "[config] cli import: writing ssh config ({} new hosts)",
                    imported
                );
                config.write()?;
            }
            log::info!(
                "[purple] cli import: imported={} skipped={} parse_failures={} read_errors={}",
                imported,
                skipped,
                parse_failures,
                read_errors
            );
            println!("{}", crate::messages::imported_hosts(imported, skipped));
            if parse_failures > 0 {
                eprintln!(
                    "{}",
                    crate::messages::cli::import_parse_failures(parse_failures)
                );
            }
            if read_errors > 0 {
                eprintln!("{}", crate::messages::cli::import_read_errors(read_errors));
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

pub fn handle_sync(
    env: &crate::runtime::env::Env,
    mut config: SshConfigFile,
    provider_name: Option<&str>,
    dry_run: bool,
    remove: bool,
) -> Result<()> {
    log::info!(
        "[purple] cli sync: provider={:?} dry_run={} remove={}",
        provider_name,
        dry_run,
        remove
    );
    let provider_config = providers::config::ProviderConfig::load(env.paths());
    // The positional argument accepts either a bare provider name (sync ALL
    // configs of that provider) or a labeled identifier `provider:label`
    // (sync exactly that one config). No explicit flag form.
    let sections: Vec<&providers::config::ProviderSection> = if let Some(arg) = provider_name {
        let id: providers::config::ProviderConfigId = match arg.parse() {
            Ok(id) => id,
            Err(e) => {
                eprintln!("{}: {}", arg, e);
                std::process::exit(1);
            }
        };
        if providers::get_provider(&id.provider).is_none() {
            eprintln!("{}", crate::messages::cli::unknown_provider(&id.provider));
            std::process::exit(1);
        }
        let matched: Vec<&providers::config::ProviderSection> = match &id.label {
            Some(_) => provider_config.section_by_id(&id).into_iter().collect(),
            None => provider_config.sections_for_provider(&id.provider),
        };
        if matched.is_empty() {
            eprintln!("{}", crate::messages::cli::no_config_for(arg));
            std::process::exit(1);
        }
        matched
    } else {
        let configured = provider_config.configured_providers();
        if configured.is_empty() {
            eprintln!("{}", crate::i18n::text(crate::messages::cli::NO_PROVIDERS));
            std::process::exit(1);
        }
        configured.iter().collect()
    };

    let mut any_changes = false;
    let mut any_failures = false;
    let mut any_hard_failures = false;
    let mut all_renames: Vec<(String, String)> = Vec::new();

    for section in &sections {
        let provider = match providers::get_provider_with_config(section) {
            Some(p) => p,
            None => {
                log::warn!(
                    "[config] cli sync: skipping unknown provider '{}'",
                    section.provider()
                );
                eprintln!(
                    "{}",
                    crate::messages::cli::skipping_unknown_provider(section.provider())
                );
                any_failures = true;
                // Not a hard failure: unknown provider contributes no changes,
                // so other providers' successful results should still be written.
                continue;
            }
        };
        let display_name = providers::provider_display_name(section.provider());
        log::debug!(
            "[external] cli sync: starting provider={} label={:?}",
            section.provider(),
            section.id.label
        );
        let is_tty = std::io::IsTerminal::is_terminal(&std::io::stdout());
        print!("{}", crate::messages::cli::syncing_start(display_name));
        let _ = std::io::Write::flush(&mut std::io::stdout());

        let last_summary = std::cell::RefCell::new(String::new());
        let progress = |msg: &str| {
            *last_summary.borrow_mut() = msg.to_string();
            if is_tty {
                print!("{}", crate::messages::cli::syncing(display_name, msg));
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
        };
        let fetch_result = provider.fetch_hosts_with_progress(
            &section.token,
            &std::sync::atomic::AtomicBool::new(false),
            env,
            &progress,
        );
        let summary = last_summary.into_inner();
        // Complete the Syncing line: TTY overwrites with summary; non-TTY appends.
        if is_tty {
            if summary.is_empty() {
                print!("{}", crate::messages::cli::syncing(display_name, ""));
            } else {
                println!("{}", crate::messages::cli::syncing(display_name, &summary));
            }
            let _ = std::io::Write::flush(&mut std::io::stdout());
        } else if !summary.is_empty() {
            println!("{}", summary);
        }
        let (hosts, suppress_remove) = match fetch_result {
            Ok(hosts) => (hosts, false),
            Err(providers::ProviderError::PartialResult {
                hosts,
                failures,
                total,
            }) => {
                println!(
                    "{}",
                    crate::messages::cli::servers_found_with_failures(hosts.len(), failures, total)
                );
                if remove {
                    eprintln!("{}", crate::messages::cli::sync_skip_remove(display_name));
                }
                any_failures = true;
                (hosts, true)
            }
            Err(e) => {
                println!("{}", crate::i18n::text(crate::messages::cli::SYNC_FAILED));
                eprintln!("{}", crate::messages::cli::sync_error(display_name, &e));
                any_failures = true;
                any_hard_failures = true;
                continue;
            }
        };
        if !suppress_remove {
            println!("{}", crate::messages::cli::servers_found(hosts.len()));
        }
        let effective_remove = remove && !suppress_remove;
        let result = providers::sync::sync_provider(
            &mut config,
            &*provider,
            &hosts,
            section,
            effective_remove,
            suppress_remove, // suppress stale marking when partial failures occurred
            dry_run,
        );
        let prefix = if dry_run {
            crate::i18n::text(crate::messages::cli::SYNC_RESULT_PREFIX_DRY_RUN)
        } else {
            crate::i18n::text(crate::messages::cli::SYNC_RESULT_PREFIX_LIVE)
        };
        println!(
            "{}",
            crate::messages::cli::sync_result(
                prefix,
                result.added,
                result.updated,
                result.unchanged
            )
        );
        if result.removed > 0 {
            println!("{}", crate::messages::cli::sync_removed(result.removed));
        }
        if result.stale > 0 {
            println!("{}", crate::messages::cli::sync_stale(result.stale));
        }
        if result.added > 0 || result.updated > 0 || result.removed > 0 || result.stale > 0 {
            any_changes = true;
        }
        if !dry_run {
            all_renames.extend(result.renames);
        }
    }

    if any_changes && !dry_run {
        if any_hard_failures {
            log::warn!("[config] cli sync: skipping ssh config write due to hard failures");
            eprintln!(
                "{}",
                crate::i18n::text(crate::messages::cli::SYNC_SKIP_WRITE)
            );
        } else {
            log::debug!("[config] cli sync: writing ssh config");
            config.write()?;
            log::info!("[purple] cli sync: ssh config written");
            // Migrate per-host state keyed by alias for every host the
            // sync renamed. Tied to the successful config write: a
            // skipped or failed write must not move history/recents
            // to a new alias that did not land in `~/.ssh/config`.
            if !all_renames.is_empty() {
                log::info!(
                    "[purple] cli sync: migrating per-host state for {} rename(s)",
                    all_renames.len()
                );
                crate::app::migrate_renames_persistent_state(env.paths(), &all_renames);
            }
        }
    }

    if any_failures {
        log::warn!("[purple] cli sync: completed with failures (exit 1)");
        std::process::exit(1);
    }

    log::info!("[purple] cli sync: completed successfully");
    Ok(())
}

/// Fill every `provider add` field the caller left out from the config already
/// stored under the same id.
///
/// Matched on the exact id being written: a lookup by provider name alone
/// returns the first section, which would carry one account's token into
/// another account's config.
#[allow(clippy::too_many_arguments)]
fn apply_stored_provider_defaults(
    existing: &providers::config::ProviderSection,
    kind: Option<ProviderKind>,
    env: &crate::runtime::env::Env,
    token: &mut Option<String>,
    token_stdin: bool,
    prefix: &mut Option<String>,
    user: &mut Option<String>,
    key: &mut Option<String>,
    url: &mut Option<String>,
    no_verify_tls: &mut bool,
    verify_tls: bool,
    profile: &mut Option<String>,
    ssm: &mut Option<String>,
    regions: &mut Option<String>,
    project: &mut Option<String>,
    compartment: &mut Option<String>,
    filter: &mut Option<String>,
) {
    // URL fallback only applies to providers that use the url field
    if kind.is_some_and(ProviderKind::requires_url) && url.is_none() && !existing.url.is_empty() {
        *url = Some(existing.url.clone());
    }
    if token.is_none() && !token_stdin && env.purple_token().is_none() && !existing.token.is_empty()
    {
        *token = Some(existing.token.clone());
    }
    if prefix.is_none() {
        *prefix = Some(existing.alias_prefix.clone());
    }
    if user.is_none() {
        *user = Some(existing.user.clone());
    }
    if key.is_none() && !existing.identity_file.is_empty() {
        *key = Some(existing.identity_file.clone());
    }
    // Preserve verify_tls=false unless the user explicitly overrides it either way
    if !*no_verify_tls && !verify_tls && !existing.verify_tls {
        *no_verify_tls = true;
    }
    // AWS: fall back to stored profile/regions
    if kind == Some(ProviderKind::Aws) && profile.is_none() && !existing.profile.is_empty() {
        *profile = Some(existing.profile.clone());
    }
    // AWS: fall back to the stored Session Manager mode
    if kind == Some(ProviderKind::Aws) && ssm.is_none() && existing.ssm.is_enabled() {
        *ssm = Some(existing.ssm.to_string());
    }
    // Providers that accept --regions: fall back to stored regions
    if kind.is_some_and(ProviderKind::accepts_cli_regions)
        && regions.is_none()
        && !existing.regions.is_empty()
    {
        *regions = Some(existing.regions.clone());
    }
    // GCP: fall back to stored project
    if kind == Some(ProviderKind::Gcp) && project.is_none() && !existing.project.is_empty() {
        *project = Some(existing.project.clone());
    }
    // Oracle: fall back to stored compartment
    if kind == Some(ProviderKind::Oracle)
        && compartment.is_none()
        && !existing.compartment.is_empty()
    {
        *compartment = Some(existing.compartment.clone());
    }
    // NetBox: fall back to stored filter
    if kind == Some(ProviderKind::NetBox) && filter.is_none() && !existing.filter.is_empty() {
        *filter = Some(existing.filter.clone());
    }
}

/// The `--ssm` value as a mode. An unknown value is refused rather than
/// silently read as off: a typo would quietly leave every host on its IP
/// address.
fn resolve_ssm_mode(raw: Option<&str>) -> providers::aws_ssm::SsmMode {
    let Some(raw) = raw else {
        return providers::aws_ssm::SsmMode::default();
    };
    match providers::aws_ssm::SsmMode::ALL
        .iter()
        .find(|m| m.as_str().eq_ignore_ascii_case(raw.trim()))
    {
        Some(mode) => *mode,
        None => {
            eprintln!("{}", crate::messages::cli::ssm_mode_invalid(raw));
            std::process::exit(1);
        }
    }
}

fn print_localized_notice(message: &'static str) {
    eprintln!("{}", crate::i18n::text(message));
}

pub fn handle_provider_command(
    env: &crate::runtime::env::Env,
    command: ProviderCommands,
) -> Result<()> {
    log::info!("[purple] cli provider: dispatch");
    match command {
        ProviderCommands::Add {
            provider,
            token,
            token_stdin,
            mut prefix,
            mut user,
            mut key,
            url,
            mut profile,
            ssm,
            mut regions,
            mut project,
            mut compartment,
            mut filter,
            no_verify_tls,
            verify_tls,
            auto_sync,
            no_auto_sync,
            label,
        } => {
            let p = match providers::get_provider(&provider) {
                Some(p) => p,
                None => {
                    eprintln!("{}", crate::messages::cli::unknown_provider(&provider));
                    std::process::exit(1);
                }
            };
            // provider is validated above, so from_str always returns Some here.
            let kind = provider.parse::<ProviderKind>().ok();

            // --url, --no-verify-tls and --verify-tls only apply to providers
            // with a self-hosted endpoint; clear them for the rest
            let mut token = token;
            let mut url = url;
            let mut no_verify_tls = no_verify_tls;
            let mut verify_tls = verify_tls;
            if !kind.is_some_and(ProviderKind::requires_url) {
                if url.is_some() {
                    print_localized_notice(crate::messages::cli::WARN_URL_NOT_USED);
                    url = None;
                }
                if no_verify_tls {
                    print_localized_notice(crate::messages::cli::WARN_NO_VERIFY_TLS_NOT_USED);
                    no_verify_tls = false;
                }
                if verify_tls {
                    print_localized_notice(crate::messages::cli::WARN_VERIFY_TLS_NOT_USED);
                    verify_tls = false;
                }
            }
            // --profile and --ssm are AWS-only, --regions is AWS/Scaleway/GCP/Azure,
            // --project is GCP-only
            let mut ssm = ssm;
            if kind != Some(ProviderKind::Aws) && profile.is_some() {
                print_localized_notice(crate::messages::cli::WARN_PROFILE_NOT_USED);
                profile = None;
            }
            if kind != Some(ProviderKind::Aws) && ssm.is_some() {
                print_localized_notice(crate::messages::cli::WARN_SSM_NOT_USED);
                ssm = None;
            }
            if !kind.is_some_and(ProviderKind::accepts_cli_regions) && regions.is_some() {
                print_localized_notice(crate::messages::cli::WARN_REGIONS_NOT_USED);
                regions = None;
            }
            if kind != Some(ProviderKind::Gcp) && project.is_some() {
                print_localized_notice(crate::messages::cli::WARN_PROJECT_NOT_USED);
                project = None;
            }
            if kind != Some(ProviderKind::Oracle) && compartment.is_some() {
                print_localized_notice(crate::messages::cli::WARN_COMPARTMENT_NOT_USED);
                compartment = None;
            }
            if kind != Some(ProviderKind::NetBox) && filter.is_some() {
                print_localized_notice(crate::messages::cli::WARN_FILTER_NOT_USED);
                filter = None;
            }

            // When updating an existing section, fall back to stored values for
            // fields not supplied. Matched on the exact id being written: a
            // lookup by provider name alone returns the first section, which
            // would carry one account's token into another account's config.
            // The label is validated further down; an invalid one simply
            // matches nothing here.
            let target_id = match label.as_deref() {
                Some(l) => providers::config::ProviderConfigId::labeled(provider.clone(), l),
                None => providers::config::ProviderConfigId::bare(provider.clone()),
            };
            let existing_section = providers::config::ProviderConfig::load(env.paths())
                .section_by_id(&target_id)
                .cloned();

            if let Some(ref existing) = existing_section {
                apply_stored_provider_defaults(
                    existing,
                    kind,
                    env,
                    &mut token,
                    token_stdin,
                    &mut prefix,
                    &mut user,
                    &mut key,
                    &mut url,
                    &mut no_verify_tls,
                    verify_tls,
                    &mut profile,
                    &mut ssm,
                    &mut regions,
                    &mut project,
                    &mut compartment,
                    &mut filter,
                );
            }

            // Providers with a self-hosted endpoint require --url
            if kind.is_some_and(ProviderKind::requires_url) {
                if url.is_none() || url.as_deref().unwrap_or("").trim().is_empty() {
                    let msg = if kind == Some(ProviderKind::NetBox) {
                        crate::i18n::text(crate::messages::cli::NETBOX_URL_REQUIRED)
                    } else {
                        crate::i18n::text(crate::messages::cli::PROXMOX_URL_REQUIRED)
                    };
                    eprintln!("{}", msg);
                    std::process::exit(1);
                }
                let u = url.as_deref().unwrap();
                if !u.to_ascii_lowercase().starts_with("https://") {
                    print_localized_notice(crate::messages::cli::PROVIDER_URL_REQUIRES_HTTPS);
                    std::process::exit(1);
                }
            }

            // Token may be left unset for providers that resolve credentials
            // elsewhere: an AWS profile or environment, the Tailscale or
            // Teleport local CLI. `token_omitted` only decides whether to skip
            // resolve_token, which exits when it finds nothing at all. It never
            // decides whether an empty token is acceptable.
            let token_omitted = token.is_none() && !token_stdin && env.purple_token().is_none();
            let token_optional = kind.is_some_and(ProviderKind::token_is_optional);
            let token = if token_optional && token_omitted {
                String::new()
            } else {
                match super::resolve_token(env, token, token_stdin) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                }
            };

            // Asking for a token that resolves to nothing stays legal here, so
            // `--token ""` still clears a stored one. A script that piped a
            // blank secret looks identical from here, so it gets said out loud,
            // and losing a stored credential is named separately.
            if token.trim().is_empty() && token_optional && !token_omitted {
                let display_name = providers::provider_display_name(&provider);
                let replaces_stored = existing_section
                    .as_ref()
                    .is_some_and(|s| !s.token.trim().is_empty());
                if replaces_stored {
                    eprintln!(
                        "{}",
                        crate::messages::cli::warn_token_replaced_with_empty(display_name)
                    );
                } else {
                    eprintln!(
                        "{}",
                        crate::messages::cli::warn_token_resolved_empty(display_name)
                    );
                }
            }

            if token.trim().is_empty() && !token_optional {
                if kind == Some(ProviderKind::Gcp) {
                    print_localized_notice(crate::messages::cli::PROVIDER_TOKEN_REQUIRED_GCP);
                } else if kind == Some(ProviderKind::Oracle) {
                    print_localized_notice(crate::messages::cli::PROVIDER_TOKEN_REQUIRED_ORACLE);
                } else {
                    eprintln!(
                        "{}",
                        crate::messages::cli::provider_token_required(
                            providers::provider_display_name(&provider)
                        )
                    );
                }
                std::process::exit(1);
            }

            let ssm_mode = resolve_ssm_mode(ssm.as_deref());

            // A labeled config defaults to `<short>-<label>`, matching what the
            // TUI suggests and what the parser derives. Without it a second
            // account for the same provider collides on the bare prefix and
            // the save is refused.
            let alias_prefix = prefix.unwrap_or_else(|| match label.as_deref() {
                Some(l) => format!("{}-{}", p.short_label(), l),
                None => p.short_label().to_string(),
            });
            if crate::ssh_config::model::is_host_pattern(&alias_prefix) {
                print_localized_notice(crate::messages::cli::ALIAS_PREFIX_INVALID);
                std::process::exit(1);
            }

            let user = user.unwrap_or_else(|| "root".to_string());
            let identity_file = key.unwrap_or_default();

            // Reject control characters in all fields (prevents INI injection)
            let url_value = url.clone().unwrap_or_default();
            let profile_value = profile.clone().unwrap_or_default();
            let regions_value = regions.clone().unwrap_or_default();
            let project_value = project.clone().unwrap_or_default();
            let compartment_value = compartment.clone().unwrap_or_default();
            let filter_value = filter.clone().unwrap_or_default();
            for (value, name) in [
                (&url_value, "URL"),
                (&token, "Token"),
                (&alias_prefix, "Alias prefix"),
                (&user, "User"),
                (&identity_file, "Identity file"),
                (&profile_value, "Profile"),
                (&project_value, "Project"),
                (&regions_value, "Regions"),
                (&compartment_value, "Compartment"),
                (&filter_value, "Filter"),
            ] {
                if value.chars().any(|c| c.is_control()) {
                    eprintln!("{}", crate::messages::cli::control_chars(name));
                    std::process::exit(1);
                }
            }
            if user.contains(char::is_whitespace) {
                print_localized_notice(crate::messages::cli::USER_WHITESPACE);
                std::process::exit(1);
            }

            // Resolve auto_sync: explicit flags > existing config > provider default
            let resolved_auto_sync = if auto_sync {
                true
            } else if no_auto_sync {
                false
            } else if let Some(ref existing) = existing_section {
                existing.auto_sync
            } else {
                kind.is_none_or(ProviderKind::default_auto_sync)
            };

            let resolved_profile = profile.unwrap_or_default();
            // Captured before the section takes the profile: Session Manager
            // can only hand the aws CLI a --profile, never the inline key
            // pair, so this is worth saying once at save time.
            let ssm_without_profile = ssm_mode.is_enabled() && resolved_profile.trim().is_empty();
            // The profile goes into a shell line purple writes, so a name it
            // cannot quote is refused here rather than at the next sync.
            if ssm_mode.is_enabled()
                && !resolved_profile.trim().is_empty()
                && !providers::aws_ssm::is_safe_profile_name(resolved_profile.trim())
            {
                eprintln!(
                    "{}",
                    crate::messages::cli::aws_ssm_profile_unsafe(resolved_profile.trim())
                );
                std::process::exit(1);
            }
            let resolved_regions = regions.unwrap_or_default();
            let resolved_project = project.unwrap_or_default();
            let resolved_compartment = compartment.unwrap_or_default();

            // AWS/Scaleway/Azure requires at least one region/zone/subscription
            if kind == Some(ProviderKind::Aws) && resolved_regions.trim().is_empty() {
                print_localized_notice(crate::messages::cli::AWS_REGIONS_REQUIRED);
                std::process::exit(1);
            }
            if kind == Some(ProviderKind::Scaleway) && resolved_regions.trim().is_empty() {
                print_localized_notice(crate::messages::cli::SCALEWAY_REGIONS_REQUIRED);
                std::process::exit(1);
            }
            if kind == Some(ProviderKind::Azure) {
                if resolved_regions.trim().is_empty() {
                    print_localized_notice(crate::messages::cli::AZURE_REGIONS_REQUIRED);
                    std::process::exit(1);
                }
                for sub in resolved_regions
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                {
                    if !providers::azure::is_valid_subscription_id(sub) {
                        eprintln!(
                            "{}",
                            crate::messages::cli::azure_subscription_id_invalid(sub)
                        );
                        std::process::exit(1);
                    }
                }
            }
            // GCP requires --project
            if kind == Some(ProviderKind::Gcp) && resolved_project.trim().is_empty() {
                print_localized_notice(crate::messages::cli::GCP_PROJECT_REQUIRED);
                std::process::exit(1);
            }
            // Oracle requires --compartment
            if kind == Some(ProviderKind::Oracle) && resolved_compartment.trim().is_empty() {
                print_localized_notice(crate::messages::cli::ORACLE_COMPARTMENT_REQUIRED);
                std::process::exit(1);
            }

            let mut config = providers::config::ProviderConfig::load(env.paths());

            // Resolve the target ProviderConfigId given --label and the
            // provider's existing config layout. Rules:
            //   --label X:    add/update [provider:X]; refuse mix with bare
            //   no --label:   single bare config OR the only labeled config
            //                 (if 2+ labeled exist, error: ambiguous)
            let id: providers::config::ProviderConfigId = match label.as_deref() {
                Some(l) => {
                    if let Err(e) = providers::config::validate_label(l) {
                        eprintln!("{}", crate::messages::cli::invalid_label_flag(&e));
                        std::process::exit(1);
                    }
                    providers::config::ProviderConfigId::labeled(provider.clone(), l)
                }
                None => providers::config::ProviderConfigId::bare(provider.clone()),
            };

            // Refuse to mix bare and labeled configs for the same provider:
            // mirrors the parser invariant.
            let existing = config.sections_for_provider(&provider);
            let has_bare = existing.iter().any(|s| s.id.label.is_none());
            let has_labeled = existing.iter().any(|s| s.id.label.is_some());
            if id.label.is_none() && has_labeled {
                eprintln!("{}", crate::messages::cli::add_requires_label(&provider));
                std::process::exit(1);
            }
            if id.label.is_some() && has_bare {
                eprintln!(
                    "{}",
                    crate::messages::cli::add_label_collides_with_bare(&provider)
                );
                std::process::exit(1);
            }

            let section = providers::config::ProviderSection {
                id: id.clone(),
                token,
                alias_prefix,
                user,
                identity_file,
                url: url.unwrap_or_default(),
                verify_tls: !no_verify_tls,
                auto_sync: resolved_auto_sync,
                profile: resolved_profile,
                regions: resolved_regions,
                project: resolved_project,
                compartment: resolved_compartment,
                filter: filter.unwrap_or_default(),
                vault_role: String::new(),
                vault_addr: String::new(),
                ssm: ssm_mode,
            };

            // Captured before the section moves into the config.
            let saved_record = section.log_record();
            config.set_section(section);
            config.save().map_err(|e| {
                log::warn!("[config] Save failed for [{}]: {}", id, e);
                anyhow::anyhow!("Failed to save: {}", e)
            })?;
            log::debug!("[purple] provider saved: [{}] {}", id, saved_record);
            println!("{}", crate::messages::cli::saved_config(&id.to_string()));
            // Replacing a bare config in place is the documented behavior but
            // it reads as data loss, so say once how to end up with two
            // configs instead.
            if existing_section.is_some() && id.label.is_none() {
                println!(
                    "{}",
                    crate::messages::cli::provider_replaced_hint(&provider)
                );
            }
            if ssm_without_profile {
                print_localized_notice(crate::messages::cli::AWS_SSM_WITHOUT_PROFILE);
            }
            Ok(())
        }
        ProviderCommands::List => {
            let config = providers::config::ProviderConfig::load(env.paths());
            let sections = config.configured_providers();
            if sections.is_empty() {
                println!("{}", crate::i18n::text(crate::messages::cli::NO_PROVIDERS));
            } else {
                for s in sections {
                    let display_name = providers::provider_display_name(s.provider());
                    let label_suffix = match &s.id.label {
                        Some(l) => format!(" ({})", l),
                        None => String::new(),
                    };
                    println!(
                        "  {:<24} {}-*{:>8}",
                        format!("{}{}", display_name, label_suffix),
                        s.alias_prefix,
                        s.user
                    );
                }
            }
            Ok(())
        }
        ProviderCommands::Remove { provider } => {
            // Accept either `digitalocean` (remove all configs of that
            // provider) or `digitalocean:work` (remove only that one).
            let id: providers::config::ProviderConfigId = match provider.parse() {
                Ok(id) => id,
                Err(e) => {
                    eprintln!("{}: {}", provider, e);
                    std::process::exit(1);
                }
            };
            let mut config = providers::config::ProviderConfig::load(env.paths());
            let removed = match &id.label {
                Some(_) => {
                    if config.section_by_id(&id).is_none() {
                        eprintln!("{}", crate::messages::cli::no_config_to_remove(&provider));
                        std::process::exit(1);
                    }
                    config.remove_section_by_id(&id);
                    1
                }
                None => {
                    let count = config.sections_for_provider(&id.provider).len();
                    if count == 0 {
                        eprintln!("{}", crate::messages::cli::no_config_to_remove(&provider));
                        std::process::exit(1);
                    }
                    config.remove_section(&id.provider);
                    count
                }
            };
            config.save().map_err(|e| {
                log::warn!("[config] Save failed removing [{}]: {}", id, e);
                anyhow::anyhow!("Failed to save: {}", e)
            })?;
            log::debug!(
                "[purple] provider removed: [{}], {} section(s)",
                id,
                removed
            );
            if removed == 1 {
                println!("{}", crate::messages::cli::removed_config(&provider));
            } else {
                println!(
                    "{}",
                    crate::messages::cli::removed_configs(&provider, removed)
                );
            }
            Ok(())
        }
    }
}

pub fn handle_tunnel_command(mut config: SshConfigFile, command: TunnelCommands) -> Result<()> {
    log::info!("[purple] cli tunnel: dispatch");
    match command {
        TunnelCommands::List { alias } => {
            if let Some(alias) = alias {
                // Show tunnels for a specific host
                if !config.has_host(&alias) {
                    eprintln!("{}", crate::messages::cli::host_not_found(&alias));
                    std::process::exit(1);
                }
                let rules = config.find_tunnel_directives(&alias);
                if rules.is_empty() {
                    println!("{}", crate::messages::cli::no_tunnels_for(&alias));
                } else {
                    println!("{}", crate::messages::cli::tunnels_for(&alias));
                    for rule in &rules {
                        println!("  {}", rule.display());
                    }
                }
            } else {
                // Show all hosts with tunnels
                let entries = config.host_entries();
                let with_tunnels: Vec<_> = entries.iter().filter(|e| e.tunnel_count > 0).collect();
                if with_tunnels.is_empty() {
                    println!("{}", crate::i18n::text(crate::messages::cli::NO_TUNNELS));
                } else {
                    for (i, host) in with_tunnels.iter().enumerate() {
                        if i > 0 {
                            println!();
                        }
                        println!("{}:", host.alias);
                        for rule in config.find_tunnel_directives(&host.alias) {
                            println!("  {}", rule.display());
                        }
                    }
                }
            }
            Ok(())
        }
        TunnelCommands::Add { alias, forward } => {
            if !config.has_host(&alias) {
                eprintln!("{}", crate::messages::cli::host_not_found(&alias));
                std::process::exit(1);
            }
            if config.is_included_host(&alias) {
                eprintln!("{}", crate::messages::cli::included_host_read_only(&alias));
                std::process::exit(1);
            }
            let rule = crate::tunnel::TunnelRule::from_cli_spec(&forward).unwrap_or_else(|e| {
                eprintln!("{}", e);
                std::process::exit(1);
            });
            let key = rule.tunnel_type.directive_key();
            let value = rule.to_directive_value();
            // Check for duplicate forward
            if config.has_forward(&alias, key, &value) {
                eprintln!("{}", crate::messages::cli::forward_exists(&forward, &alias));
                std::process::exit(1);
            }
            config.add_forward(&alias, key, &value);
            log::debug!(
                "[config] cli tunnel add: writing ssh config (alias={})",
                alias
            );
            if let Err(e) = config.write() {
                log::warn!("[purple] cli tunnel add: write failed: {}", e);
                eprintln!("{}", crate::messages::cli::save_config_failed(&e));
                std::process::exit(1);
            }
            log::info!(
                "[purple] cli tunnel add: forward={} alias={}",
                forward,
                alias
            );
            println!("{}", crate::messages::cli::added_forward(&forward, &alias));
            Ok(())
        }
        TunnelCommands::Remove { alias, forward } => {
            if !config.has_host(&alias) {
                eprintln!("{}", crate::messages::cli::host_not_found(&alias));
                std::process::exit(1);
            }
            if config.is_included_host(&alias) {
                eprintln!("{}", crate::messages::cli::included_host_read_only(&alias));
                std::process::exit(1);
            }
            let rule = crate::tunnel::TunnelRule::from_cli_spec(&forward).unwrap_or_else(|e| {
                eprintln!("{}", e);
                std::process::exit(1);
            });
            let key = rule.tunnel_type.directive_key();
            let value = rule.to_directive_value();
            let removed = config.remove_forward(&alias, key, &value);
            if !removed {
                eprintln!(
                    "{}",
                    crate::messages::cli::forward_not_found(&forward, &alias)
                );
                std::process::exit(1);
            }
            log::debug!(
                "[config] cli tunnel remove: writing ssh config (alias={})",
                alias
            );
            if let Err(e) = config.write() {
                log::warn!("[purple] cli tunnel remove: write failed: {}", e);
                eprintln!("{}", crate::messages::cli::save_config_failed(&e));
                std::process::exit(1);
            }
            log::info!(
                "[purple] cli tunnel remove: forward={} alias={}",
                forward,
                alias
            );
            println!(
                "{}",
                crate::messages::cli::removed_forward(&forward, &alias)
            );
            Ok(())
        }
        TunnelCommands::Start { alias } => {
            log::info!("[purple] cli tunnel start: alias={}", alias);
            if !config.has_host(&alias) {
                eprintln!("{}", crate::messages::cli::host_not_found(&alias));
                std::process::exit(1);
            }
            let tunnels = config.find_tunnel_directives(&alias);
            if tunnels.is_empty() {
                log::warn!("[purple] cli tunnel start: no forwards for alias={}", alias);
                eprintln!("{}", crate::messages::cli::no_forwards(&alias));
                std::process::exit(1);
            }
            println!("{}", crate::messages::cli::starting_tunnel(&alias));
            // Run ssh -N in foreground with inherited stdio
            let status = std::process::Command::new("ssh")
                .arg("-F")
                .arg(&config.path)
                .arg("-N")
                .arg("--")
                .arg(&alias)
                .status()
                .map_err(|e| anyhow::anyhow!("Failed to start ssh: {}", e))?;
            let code = status.code().unwrap_or(1);
            std::process::exit(code);
        }
    }
}

/// Read a line of input with echo disabled. Returns None if the user presses Esc.
pub fn prompt_hidden_input(prompt: &str) -> Result<Option<String>> {
    eprint!("{}", prompt);
    crossterm::terminal::enable_raw_mode()?;
    let mut input = String::new();
    loop {
        if let crossterm::event::Event::Key(key) = crossterm::event::read()? {
            match key.code {
                crossterm::event::KeyCode::Enter => break,
                crossterm::event::KeyCode::Char(c) => {
                    input.push(c);
                    eprint!("*");
                }
                crossterm::event::KeyCode::Backspace if input.pop().is_some() => {
                    eprint!("\x08 \x08");
                }
                crossterm::event::KeyCode::Esc => {
                    crossterm::terminal::disable_raw_mode()?;
                    eprintln!();
                    return Ok(None);
                }
                _ => {}
            }
        }
    }
    crossterm::terminal::disable_raw_mode()?;
    eprintln!();
    Ok(Some(input))
}

/// Resolve the current on-disk mtime of a host's Vault SSH certificate.
///
/// Used by the `CertCheckResult` handler so every cache entry carries a
/// mtime alongside its status, enabling mtime-based lazy invalidation when
/// an external actor (CLI, another purple instance) rewrites the cert.
pub fn handle_password_command(
    env: &crate::runtime::env::Env,
    command: PasswordCommands,
) -> Result<()> {
    log::info!("[purple] cli password: dispatch");
    match command {
        PasswordCommands::Set { alias } => {
            let password =
                match prompt_hidden_input(&crate::messages::askpass::password_prompt(&alias))? {
                    Some(p) if !p.is_empty() => p,
                    Some(_) => {
                        eprintln!(
                            "{}",
                            crate::i18n::text(crate::messages::cli::PASSWORD_EMPTY)
                        );
                        std::process::exit(1);
                    }
                    None => {
                        eprintln!("{}", crate::i18n::text(crate::messages::cli::CANCELLED));
                        std::process::exit(1);
                    }
                };

            askpass::store_in_keychain(env, &alias, &password)?;
            println!(
                "Password stored for {}. Set 'keychain' as password source to use it.",
                alias
            );
            Ok(())
        }
        PasswordCommands::Remove { alias } => {
            askpass::remove_from_keychain(env, &alias)?;
            println!("{}", crate::messages::cli::password_removed(&alias));
            Ok(())
        }
    }
}

/// Print the Interactive shell tip on stderr when a plain-shell run ended with
/// "command not found".
pub(crate) fn print_not_found_hint(snip: &snippet::Snippet, exit_code: Option<i32>) {
    if snippet::not_found_hint_applies(snip.interactive, exit_code) {
        eprintln!(
            "{}",
            crate::i18n::text(crate::messages::cli::SNIPPET_NOT_FOUND_HINT)
        );
    }
}

pub fn handle_snippet_command(
    env: &crate::runtime::env::Env,
    config: SshConfigFile,
    command: SnippetCommands,
    config_path: &Path,
) -> Result<()> {
    log::info!("[purple] cli snippet: dispatch");
    match command {
        SnippetCommands::List => {
            let store = snippet::SnippetStore::load(env.paths());
            if store.snippets.is_empty() {
                println!("{}", crate::i18n::text(crate::messages::cli::NO_SNIPPETS));
            } else {
                for s in &store.snippets {
                    if s.description.is_empty() {
                        println!("  {}  {}", s.name, s.command);
                    } else {
                        println!("  {}  {}  ({})", s.name, s.command, s.description);
                    }
                }
            }
            Ok(())
        }
        SnippetCommands::Add {
            name,
            command,
            description,
            interactive,
        } => {
            if let Err(e) = snippet::validate_name(&name) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
            if let Err(e) = snippet::validate_command(&command) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
            if let Some(ref desc) = description
                && desc.contains(|c: char| c.is_control())
            {
                eprintln!(
                    "{}",
                    crate::i18n::text(crate::messages::cli::DESCRIPTION_CONTROL_CHARS)
                );
                std::process::exit(1);
            }
            let mut store = snippet::SnippetStore::load(env.paths());
            let is_update = store.get(&name).is_some();
            store.set(snippet::Snippet {
                name: name.clone(),
                command,
                description: description.unwrap_or_default(),
                interactive,
            });
            store.save()?;
            log::debug!("[purple] cli snippet saved: {name} (interactive={interactive})");
            if is_update {
                println!("{}", crate::messages::cli::snippet_updated(&name));
            } else {
                println!("{}", crate::messages::cli::snippet_added(&name));
            }
            Ok(())
        }
        SnippetCommands::Remove { name } => {
            let mut store = snippet::SnippetStore::load(env.paths());
            if store.get(&name).is_none() {
                eprintln!("{}", crate::messages::cli::snippet_not_found(&name));
                std::process::exit(1);
            }
            store.remove(&name);
            store.save()?;
            println!("{}", crate::messages::cli::snippet_removed(&name));
            Ok(())
        }
        SnippetCommands::Run {
            name,
            alias,
            tag,
            all,
            parallel,
        } => {
            let store = snippet::SnippetStore::load(env.paths());
            let snip = match store.get(&name) {
                Some(s) => s.clone(),
                None => {
                    eprintln!("{}", crate::messages::cli::snippet_not_found(&name));
                    std::process::exit(1);
                }
            };

            let entries = config.host_entries();

            // Determine target hosts
            let targets: Vec<&HostEntry> = if let Some(ref alias) = alias {
                match entries.iter().find(|h| h.alias == *alias) {
                    Some(h) => vec![h],
                    None => {
                        eprintln!("{}", crate::messages::cli::host_not_found(alias));
                        std::process::exit(1);
                    }
                }
            } else if let Some(ref tag_filter) = tag {
                let matched: Vec<_> = entries
                    .iter()
                    .filter(|h| h.tags.iter().any(|t| t.eq_ignore_ascii_case(tag_filter)))
                    .collect();
                if matched.is_empty() {
                    eprintln!("{}", crate::messages::cli::no_hosts_with_tag(tag_filter));
                    std::process::exit(1);
                }
                matched
            } else if all {
                entries.iter().collect()
            } else {
                eprintln!(
                    "{}",
                    crate::i18n::text(crate::messages::cli::SPECIFY_TARGET)
                );
                std::process::exit(1);
            };

            if targets.len() == 1 {
                // Single host: run directly
                let host = targets[0];
                let askpass = host
                    .askpass
                    .clone()
                    .or_else(|| preferences::load_askpass_default(env.paths()));
                super::ensure_proton_login(env, askpass.as_deref());
                let bw_session = super::ensure_bw_session(env, None, askpass.as_deref());
                super::ensure_keychain_password(env, &host.alias, askpass.as_deref());
                let result = snippet::run_snippet(
                    &host.alias,
                    config_path,
                    env,
                    &snip.remote_command(),
                    askpass.as_deref(),
                    bw_session.as_deref(),
                    false,
                    false,
                );
                // Drop the retry marker this run may have armed, before any
                // exit path. Left behind, it reads as a rejected password on
                // the next run within the minute and sends ssh to the tty
                // instead of the stored one.
                crate::askpass::cleanup_marker(env.paths(), &host.alias);
                match result {
                    Ok(r) => {
                        print_not_found_hint(&snip, r.status.code());
                        if !r.status.success() {
                            std::process::exit(r.status.code().unwrap_or(1));
                        }
                    }
                    Err(e) => {
                        eprintln!("{}", crate::messages::cli::operation_failed(&e));
                        std::process::exit(1);
                    }
                }
            } else if parallel {
                // Multi-host parallel
                use std::sync::mpsc;
                use std::thread;
                let (tx, rx) = mpsc::channel();
                let max_concurrent: usize = 20;
                let (slot_tx, slot_rx) = mpsc::channel();
                for _ in 0..max_concurrent {
                    let _ = slot_tx.send(());
                }
                let config_path = config_path.to_path_buf();
                // Resolve BW session if any target uses Bitwarden
                let any_bw = targets.iter().any(|h| {
                    let askpass = h
                        .askpass
                        .clone()
                        .or_else(|| preferences::load_askpass_default(env.paths()));
                    askpass.as_deref().unwrap_or("").starts_with("bw:")
                });
                let bw_session = if any_bw {
                    let bw_askpass = targets
                        .iter()
                        .find_map(|h| h.askpass.as_ref().filter(|a| a.starts_with("bw:")))
                        .cloned()
                        .or_else(|| preferences::load_askpass_default(env.paths()));
                    super::ensure_bw_session(env, None, bw_askpass.as_deref())
                } else {
                    None
                };
                // Resolve Proton Pass login if any target uses it. Proton Pass
                // persists its session on disk; we do not propagate a token, we
                // only ensure the user is logged in once before the batch starts.
                let target_askpass: Vec<Option<String>> =
                    targets.iter().map(|h| h.askpass.clone()).collect();
                if let Some(askpass) = select_proton_askpass(
                    &target_askpass,
                    preferences::load_askpass_default(env.paths()),
                ) {
                    super::ensure_proton_login(env, Some(&askpass));
                }
                let targets_info: Vec<_> = targets
                    .iter()
                    .map(|h| {
                        let askpass = h
                            .askpass
                            .clone()
                            .or_else(|| preferences::load_askpass_default(env.paths()));
                        super::ensure_keychain_password(env, &h.alias, askpass.as_deref());
                        (h.alias.clone(), askpass)
                    })
                    .collect();
                let command = snip.remote_command();
                let env = std::sync::Arc::new(env.clone());
                thread::spawn(move || {
                    for (alias, askpass) in targets_info {
                        let _ = slot_rx.recv();
                        let slot_tx = slot_tx.clone();
                        let tx = tx.clone();
                        let config_path = config_path.clone();
                        let env = std::sync::Arc::clone(&env);
                        let command = command.clone();
                        let bw_session = bw_session.clone();
                        thread::spawn(move || {
                            let result = snippet::run_snippet(
                                &alias,
                                &config_path,
                                &env,
                                &command,
                                askpass.as_deref(),
                                bw_session.as_deref(),
                                true,
                                false,
                            );
                            let _ = tx.send((alias, result));
                            let _ = slot_tx.send(());
                        });
                    }
                });

                let host_count = targets.len();
                for _ in 0..host_count {
                    if let Ok((alias, result)) = rx.recv() {
                        match result {
                            Ok(r) => {
                                for line in r.stdout.lines() {
                                    println!("[{}] {}", alias, line);
                                }
                                for line in r.stderr.lines() {
                                    eprintln!("[{}] {}", alias, line);
                                }
                                if snippet::not_found_hint_applies(
                                    snip.interactive,
                                    r.status.code(),
                                ) {
                                    eprintln!(
                                        "[{}] {}",
                                        alias,
                                        crate::i18n::text(
                                            crate::messages::cli::SNIPPET_NOT_FOUND_HINT
                                        )
                                    );
                                }
                            }
                            Err(e) => {
                                eprintln!("{}", crate::messages::cli::host_failed(&alias, &e))
                            }
                        }
                    }
                }
            } else {
                // Multi-host sequential
                let mut bw_session: Option<String> = None;
                for host in &targets {
                    let askpass = host
                        .askpass
                        .clone()
                        .or_else(|| preferences::load_askpass_default(env.paths()));
                    super::ensure_proton_login(env, askpass.as_deref());
                    if let Some(token) =
                        super::ensure_bw_session(env, bw_session.as_deref(), askpass.as_deref())
                    {
                        bw_session = Some(token);
                    }
                    super::ensure_keychain_password(env, &host.alias, askpass.as_deref());
                    println!("{}", crate::messages::cli::host_separator(&host.alias));
                    let result = snippet::run_snippet(
                        &host.alias,
                        config_path,
                        env,
                        &snip.remote_command(),
                        askpass.as_deref(),
                        bw_session.as_deref(),
                        false,
                        false,
                    );
                    crate::askpass::cleanup_marker(env.paths(), &host.alias);
                    match result {
                        Ok(r) => {
                            print_not_found_hint(&snip, r.status.code());
                            if !r.status.success() {
                                eprintln!(
                                    "{}",
                                    crate::messages::cli::exited_with_code(
                                        r.status.code().unwrap_or(1)
                                    )
                                );
                            }
                        }
                        Err(e) => {
                            eprintln!("{}", crate::messages::cli::host_failed(&host.alias, &e))
                        }
                    }
                    println!();
                }
            }
            Ok(())
        }
    }
}

pub fn handle_logs_command(tail: bool, clear: bool, env: &crate::runtime::env::Env) -> Result<()> {
    let path = logging::log_path(env.paths()).context("Could not determine log path")?;
    if clear {
        if path.exists() {
            std::fs::remove_file(&path)?;
            println!("{}", crate::messages::cli::log_deleted(&path.display()));
        } else {
            println!("{}", crate::messages::cli::no_log_file(&path.display()));
        }
    } else if tail {
        let status = std::process::Command::new("tail")
            .args(["-f", &path.to_string_lossy()])
            .status()
            .context("Failed to run tail")?;
        std::process::exit(status.code().unwrap_or(1));
    } else {
        println!("{}", path.display());
    }
    Ok(())
}

pub fn handle_theme_command(env: &crate::runtime::env::Env, command: ThemeCommands) -> Result<()> {
    log::info!("[purple] cli theme: dispatch");
    match command {
        ThemeCommands::List => {
            let current =
                preferences::load_theme(env.paths()).unwrap_or_else(|| "Purple".to_string());
            println!(
                "{}",
                crate::i18n::text(crate::messages::cli::BUILTIN_THEMES)
            );
            for theme in ui::theme::ThemeDef::builtins() {
                let marker = if theme.name.eq_ignore_ascii_case(&current) {
                    "*"
                } else {
                    " "
                };
                println!("  {} {}", marker, theme.name);
            }
            let custom = ui::theme::ThemeDef::load_custom(env.paths());
            if !custom.is_empty() {
                println!("{}", crate::i18n::text(crate::messages::cli::CUSTOM_THEMES));
                for theme in &custom {
                    let marker = if theme.name.eq_ignore_ascii_case(&current) {
                        "*"
                    } else {
                        " "
                    };
                    println!("  {} {}", marker, theme.name);
                }
            }
        }
        ThemeCommands::Set { name } => {
            let found = ui::theme::ThemeDef::find_builtin(&name).or_else(|| {
                ui::theme::ThemeDef::load_custom(env.paths())
                    .into_iter()
                    .find(|t| t.name.eq_ignore_ascii_case(&name))
            });
            match found {
                Some(theme) => {
                    preferences::save_theme(env.paths(), &theme.name)?;
                    println!("{}", crate::messages::cli::theme_set(&theme.name));
                }
                None => {
                    anyhow::bail!("Unknown theme: {}", name);
                }
            }
        }
    }
    Ok(())
}

pub fn handle_vault_sign_command(
    env: &crate::runtime::env::Env,
    mut config: SshConfigFile,
    alias: Option<String>,
    all: bool,
    cli_vault_addr: Option<String>,
) -> Result<()> {
    log::info!(
        "[purple] cli vault sign: alias={:?} all={} vault_addr={:?}",
        alias,
        all,
        cli_vault_addr
    );
    if let Some(ref addr) = cli_vault_addr
        && !vault_ssh::is_valid_vault_addr(addr)
    {
        anyhow::bail!(
            "Invalid --vault-addr value. Must be non-empty, no whitespace or control chars."
        );
    }
    let provider_config = providers::config::ProviderConfig::load(env.paths());
    let entries = config.host_entries();

    if all {
        let mut signed = 0u32;
        let mut failed = 0u32;
        let mut skipped = 0u32;

        for entry in &entries {
            let role = match vault_ssh::resolve_vault_role(
                entry.vault_ssh.as_deref(),
                entry.provider.as_deref(),
                entry.provider_label.as_deref(),
                &provider_config,
            ) {
                Some(r) => r,
                None => {
                    skipped += 1;
                    continue;
                }
            };

            let pubkey = match vault_ssh::resolve_pubkey_path(env.paths(), &entry.identity_file) {
                Ok(p) => p,
                Err(e) => {
                    println!("{}", crate::messages::cli::skipping_host(&entry.alias, &e));
                    failed += 1;
                    continue;
                }
            };
            let cert_path =
                vault_ssh::resolve_cert_path(env.paths(), &entry.alias, &entry.certificate_file)?;
            let status = vault_ssh::check_cert_validity(env, &cert_path);

            if !vault_ssh::needs_renewal(&status) {
                skipped += 1;
                continue;
            }

            // Flag beats per-host beats provider default.
            let resolved_addr = cli_vault_addr.clone().or_else(|| {
                vault_ssh::resolve_vault_addr(
                    entry.vault_addr.as_deref(),
                    entry.provider.as_deref(),
                    entry.provider_label.as_deref(),
                    &provider_config,
                )
            });
            print!("{}", crate::messages::cli::vault_signing_host(&entry.alias));
            match vault_ssh::sign_certificate(
                env,
                &role,
                &pubkey,
                &entry.alias,
                resolved_addr.as_deref(),
            ) {
                Ok(result) => {
                    println!("\u{2713}");
                    // Honor the same invariant as the TUI paths: never
                    // overwrite a user-set CertificateFile.
                    if should_write_certificate_file(&entry.certificate_file) {
                        let updated = config.set_host_certificate_file(
                            &entry.alias,
                            &result.cert_path.to_string_lossy(),
                        );
                        if !updated {
                            eprintln!(
                                "{}",
                                crate::messages::cli::vault_sign_host_block_gone(&entry.alias)
                            );
                        }
                    }
                    signed += 1;
                }
                Err(e) => {
                    println!("{}", crate::messages::cli::vault_sign_failed(&e));
                    failed += 1;
                }
            }
        }
        if signed > 0
            && let Err(e) = config.write()
        {
            eprintln!("{}", crate::messages::cli::vault_config_update_warning(&e));
        }
        println!(
            "\nSigned: {}, failed: {}, skipped (valid): {}",
            signed, failed, skipped
        );
        if failed > 0 {
            std::process::exit(1);
        }
    } else if let Some(alias) = alias {
        let entry = entries
            .iter()
            .find(|h| h.alias == alias)
            .with_context(|| format!("Host '{}' not found", alias))?;

        let role = vault_ssh::resolve_vault_role(
            entry.vault_ssh.as_deref(),
            entry.provider.as_deref(),
            entry.provider_label.as_deref(),
            &provider_config,
        )
        .with_context(|| crate::messages::cli::vault_no_role(&alias))?;

        let pubkey = vault_ssh::resolve_pubkey_path(env.paths(), &entry.identity_file)?;
        let resolved_addr = cli_vault_addr.clone().or_else(|| {
            vault_ssh::resolve_vault_addr(
                entry.vault_addr.as_deref(),
                entry.provider.as_deref(),
                entry.provider_label.as_deref(),
                &provider_config,
            )
        });
        let result =
            vault_ssh::sign_certificate(env, &role, &pubkey, &alias, resolved_addr.as_deref())?;
        // Honor the same invariant as the TUI paths: never overwrite a
        // user-set CertificateFile. Only write the directive (and the
        // SSH config) when the host has none yet.
        if should_write_certificate_file(&entry.certificate_file) {
            let updated =
                config.set_host_certificate_file(&alias, &result.cert_path.to_string_lossy());
            if !updated {
                // Host disappeared between the `entries` snapshot and
                // the config mutation. In the single-host CLI path
                // both reads happen back-to-back in the same process,
                // so this is effectively unreachable, but surface it
                // loudly if the invariant ever breaks instead of
                // silently writing a cert nobody references.
                anyhow::bail!(
                    "Host '{}' disappeared from ssh config before CertificateFile could be written. Cert saved to {}.",
                    alias,
                    result.cert_path.display()
                );
            }
            config
                .write()
                .with_context(|| "Failed to update SSH config with CertificateFile")?;
        }
        println!(
            "{}",
            crate::messages::cli::vault_cert_signed(&result.cert_path.display())
        );
    } else {
        anyhow::bail!("Provide a host alias or use --all");
    }
    Ok(())
}

/// Pick the askpass value that drives a single Proton Pass pre-flight call for
/// a batch of hosts. Returns `Some(value)` if any host uses a `proton:` source
/// (per-host override OR the global default), preferring the first `proton:`
/// value by position in the slice before falling back to the default. Returns
/// `None` when no host in the batch uses Proton Pass.
pub fn select_proton_askpass(
    target_askpass: &[Option<String>],
    default: Option<String>,
) -> Option<String> {
    let any_proton = target_askpass.iter().any(|a| {
        let resolved = a.clone().or_else(|| default.clone());
        resolved.as_deref().unwrap_or("").starts_with("proton:")
    });
    if !any_proton {
        return None;
    }
    target_askpass
        .iter()
        .find_map(|a| a.as_ref().filter(|s| s.starts_with("proton:")))
        .cloned()
        .or(default)
}

pub fn run_whats_new(since: Option<&str>) -> Result<String> {
    use crate::changelog::{self, EntryKind};
    use semver::Version;

    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .with_context(|| "failed to parse current version")?;
    let last = match since {
        Some(s) => Some(Version::parse(s).with_context(|| format!("invalid --since version {s}"))?),
        None => None,
    };

    let sections = changelog::cached();
    let shown = changelog::versions_to_show(sections, last.as_ref(), &current, sections.len());

    let mut out = String::new();
    out.push_str(crate::i18n::text(crate::messages::cli::whats_new::HEADER));
    out.push_str("\n\n");
    for section in shown {
        out.push_str(&format!("## {}", section.version));
        if let Some(date) = &section.date {
            out.push_str(&format!(" - {}", date));
        }
        out.push('\n');
        for entry in &section.entries {
            let prefix = match entry.kind {
                EntryKind::Feature => "+ ",
                EntryKind::Change => "~ ",
                EntryKind::Fix => "! ",
            };
            out.push_str(prefix);
            out.push_str(&entry.text);
            out.push('\n');
        }
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod whats_new_tests {
    use super::*;

    #[test]
    fn whats_new_cli_outputs_header() {
        let output = run_whats_new(None).unwrap();
        assert!(output.contains("purple release notes"));
    }

    #[test]
    fn whats_new_cli_filters_by_since() {
        let output = run_whats_new(Some("999.0.0")).unwrap();
        assert!(!output.contains("## "));
    }

    #[test]
    fn whats_new_cli_returns_error_on_bad_version() {
        let result = run_whats_new(Some("not-a-version"));
        assert!(result.is_err());
    }
}

#[cfg(test)]
mod select_proton_askpass_tests {
    use super::*;

    #[test]
    fn returns_none_when_no_target_uses_proton_and_no_default() {
        let targets = vec![
            Some("bw:foo".to_string()),
            Some("keychain".to_string()),
            None,
        ];
        assert_eq!(select_proton_askpass(&targets, None), None);
    }

    #[test]
    fn returns_none_when_no_target_uses_proton_and_default_is_not_proton() {
        let targets = vec![Some("bw:foo".to_string()), None];
        let default = Some("keychain".to_string());
        assert_eq!(select_proton_askpass(&targets, default), None);
    }

    #[test]
    fn returns_proton_value_when_one_target_uses_proton() {
        let targets = vec![
            Some("bw:other".to_string()),
            Some("proton:Vault/Item/p".to_string()),
            None,
        ];
        assert_eq!(
            select_proton_askpass(&targets, None),
            Some("proton:Vault/Item/p".to_string())
        );
    }

    #[test]
    fn prefers_first_per_host_proton_value_over_default() {
        let targets = vec![
            Some("proton:First/Item/p".to_string()),
            Some("proton:Second/Item/p".to_string()),
        ];
        let default = Some("proton:Default/Item/p".to_string());
        assert_eq!(
            select_proton_askpass(&targets, default),
            Some("proton:First/Item/p".to_string())
        );
    }

    #[test]
    fn falls_back_to_default_when_no_per_host_proton_value_but_default_is_proton() {
        let targets = vec![None, Some("bw:foo".to_string()), None];
        let default = Some("proton:Default/Item/p".to_string());
        assert_eq!(
            select_proton_askpass(&targets, default),
            Some("proton:Default/Item/p".to_string())
        );
    }

    #[test]
    fn handles_all_proton_targets() {
        let targets = vec![
            Some("proton:A/x/p".to_string()),
            Some("proton:B/y/p".to_string()),
        ];
        assert_eq!(
            select_proton_askpass(&targets, None),
            Some("proton:A/x/p".to_string())
        );
    }

    #[test]
    fn handles_empty_target_list_with_proton_default() {
        let default = Some("proton:Default/Item/p".to_string());
        assert_eq!(select_proton_askpass(&[], default), None);
    }

    #[test]
    fn handles_empty_target_list_with_no_default() {
        assert_eq!(select_proton_askpass(&[], None), None);
    }

    #[test]
    fn empty_string_askpass_does_not_match_proton() {
        let targets = vec![Some(String::new()), Some("bw:foo".to_string())];
        assert_eq!(select_proton_askpass(&targets, None), None);
    }
}
