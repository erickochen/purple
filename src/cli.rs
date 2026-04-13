//! CLI subcommand handlers. Each function handles one clap subcommand
//! (provider, tunnel, password, snippet, add, import, sync, logs, theme,
//! vault sign) and runs outside the TUI in a non-interactive terminal context.

use anyhow::{Context, Result};
use std::path::Path;

use crate::providers;
use crate::snippet;
use crate::ssh_config::model::{HostEntry, SshConfigFile};
use crate::vault_ssh;

use super::{
    PasswordCommands, ProviderCommands, SnippetCommands, ThemeCommands, TunnelCommands, askpass,
    import, logging, preferences, quick_add, should_write_certificate_file, ui,
};

pub(super) fn handle_quick_add(
    mut config: SshConfigFile,
    target: &str,
    alias: Option<&str>,
    key: Option<&str>,
) -> Result<()> {
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
        eprintln!("Alias can't be empty. Use --alias to specify one.");
        std::process::exit(1);
    }
    if alias_str.contains(char::is_whitespace) {
        eprintln!("Alias can't contain whitespace. Use --alias to pick a simpler name.");
        std::process::exit(1);
    }
    if crate::ssh_config::model::is_host_pattern(&alias_str) {
        eprintln!("Alias can't contain pattern characters. Use --alias to pick a different name.");
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
            eprintln!("{} contains control characters.", name);
            std::process::exit(1);
        }
    }

    // Reject whitespace in hostname and user (matches TUI validation)
    if parsed.hostname.contains(char::is_whitespace) {
        eprintln!("Hostname can't contain whitespace.");
        std::process::exit(1);
    }
    if parsed.user.contains(char::is_whitespace) {
        eprintln!("User can't contain whitespace.");
        std::process::exit(1);
    }

    if config.has_host(&alias_str) {
        eprintln!(
            "'{}' already exists. Use --alias to pick a different name.",
            alias_str
        );
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
    config.write()?;
    println!("Welcome aboard, {}!", alias_str);
    Ok(())
}

pub(super) fn handle_import(
    mut config: SshConfigFile,
    file: Option<&str>,
    known_hosts: bool,
    group: Option<&str>,
) -> Result<()> {
    let result = if known_hosts {
        import::import_from_known_hosts(&mut config, group)
    } else if let Some(path) = file {
        let resolved = super::resolve_config_path(path)?;
        import::import_from_file(&mut config, &resolved, group)
    } else {
        eprintln!("Provide a file or use --known-hosts. Run 'purple import --help' for details.");
        std::process::exit(1);
    };

    match result {
        Ok((imported, skipped, parse_failures, read_errors)) => {
            if imported > 0 {
                config.write()?;
            }
            println!(
                "Imported {} host{}, skipped {} duplicate{}.",
                imported,
                if imported == 1 { "" } else { "s" },
                skipped,
                if skipped == 1 { "" } else { "s" },
            );
            if parse_failures > 0 {
                eprintln!(
                    "! {} line{} could not be parsed (invalid format).",
                    parse_failures,
                    if parse_failures == 1 { "" } else { "s" },
                );
            }
            if read_errors > 0 {
                eprintln!(
                    "! {} line{} could not be read (encoding error).",
                    read_errors,
                    if read_errors == 1 { "" } else { "s" },
                );
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

pub(super) fn handle_sync(
    mut config: SshConfigFile,
    provider_name: Option<&str>,
    dry_run: bool,
    remove: bool,
) -> Result<()> {
    let provider_config = providers::config::ProviderConfig::load();
    let sections: Vec<&providers::config::ProviderSection> = if let Some(name) = provider_name {
        if providers::get_provider(name).is_none() {
            eprintln!(
                "Never heard of '{}'. Try: digitalocean, vultr, linode, hetzner, upcloud, proxmox, aws, scaleway, gcp, azure, tailscale, oracle, ovh, leaseweb, i3d, transip.",
                name
            );
            std::process::exit(1);
        }
        match provider_config.section(name) {
            Some(s) => vec![s],
            None => {
                eprintln!(
                    "No configuration for {}. Run 'purple provider add {}' first.",
                    name, name
                );
                std::process::exit(1);
            }
        }
    } else {
        let configured = provider_config.configured_providers();
        if configured.is_empty() {
            eprintln!("No providers configured. Run 'purple provider add' to set one up.");
            std::process::exit(1);
        }
        configured.iter().collect()
    };

    let mut any_changes = false;
    let mut any_failures = false;
    let mut any_hard_failures = false;

    for section in &sections {
        let provider = match providers::get_provider_with_config(&section.provider, section) {
            Some(p) => p,
            None => {
                eprintln!(
                    "Skipping unknown provider '{}'. Try: digitalocean, vultr, linode, hetzner, upcloud, proxmox, aws, scaleway, gcp, azure, tailscale, oracle, ovh, leaseweb, i3d, transip.",
                    section.provider
                );
                any_failures = true;
                // Not a hard failure: unknown provider contributes no changes,
                // so other providers' successful results should still be written.
                continue;
            }
        };
        let display_name = providers::provider_display_name(section.provider.as_str());
        let is_tty = std::io::IsTerminal::is_terminal(&std::io::stdout());
        print!("Syncing {}... ", display_name);
        let _ = std::io::Write::flush(&mut std::io::stdout());

        let last_summary = std::cell::RefCell::new(String::new());
        let progress = |msg: &str| {
            *last_summary.borrow_mut() = msg.to_string();
            if is_tty {
                print!("\x1b[2K\rSyncing {}... {}", display_name, msg);
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
        };
        let fetch_result = provider.fetch_hosts_with_progress(
            &section.token,
            &std::sync::atomic::AtomicBool::new(false),
            &progress,
        );
        let summary = last_summary.into_inner();
        // Complete the Syncing line: TTY overwrites with summary; non-TTY appends.
        if is_tty {
            if summary.is_empty() {
                print!("\x1b[2K\rSyncing {}... ", display_name);
            } else {
                println!("\x1b[2K\rSyncing {}... {}", display_name, summary);
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
                    "{} servers found ({} of {} failed to fetch).",
                    hosts.len(),
                    failures,
                    total
                );
                if remove {
                    eprintln!(
                        "! {}: skipping --remove due to partial failures.",
                        display_name
                    );
                }
                any_failures = true;
                (hosts, true)
            }
            Err(e) => {
                println!("failed.");
                eprintln!("! {}: {}", display_name, e);
                any_failures = true;
                any_hard_failures = true;
                continue;
            }
        };
        if !suppress_remove {
            println!("{} servers found.", hosts.len());
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
        let prefix = if dry_run { "  Would have: " } else { "  " };
        println!(
            "{}Added {}, updated {}, unchanged {}.",
            prefix, result.added, result.updated, result.unchanged
        );
        if result.removed > 0 {
            println!("  Removed {}.", result.removed);
        }
        if result.stale > 0 {
            println!("  Marked {} stale.", result.stale);
        }
        if result.added > 0 || result.updated > 0 || result.removed > 0 || result.stale > 0 {
            any_changes = true;
        }
    }

    if any_changes && !dry_run {
        if any_hard_failures {
            eprintln!("! Skipping config write due to sync failures. Fix the errors and re-run.");
        } else {
            config.write()?;
        }
    }

    if any_failures {
        std::process::exit(1);
    }

    Ok(())
}

pub(super) fn handle_provider_command(command: ProviderCommands) -> Result<()> {
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
            mut regions,
            mut project,
            mut compartment,
            no_verify_tls,
            verify_tls,
            auto_sync,
            no_auto_sync,
        } => {
            let p = match providers::get_provider(&provider) {
                Some(p) => p,
                None => {
                    eprintln!(
                        "Never heard of '{}'. Try: digitalocean, vultr, linode, hetzner, upcloud, proxmox, aws, scaleway, gcp, azure, tailscale, oracle, ovh, leaseweb, i3d, transip.",
                        provider
                    );
                    std::process::exit(1);
                }
            };

            // --url, --no-verify-tls and --verify-tls are Proxmox-only; clear them for other providers
            let mut token = token;
            let mut url = url;
            let mut no_verify_tls = no_verify_tls;
            let mut verify_tls = verify_tls;
            if provider != "proxmox" {
                if url.is_some() {
                    eprintln!("Warning: --url is only used by the Proxmox provider. Ignoring.");
                    url = None;
                }
                if no_verify_tls {
                    eprintln!(
                        "Warning: --no-verify-tls is only used by the Proxmox provider. Ignoring."
                    );
                    no_verify_tls = false;
                }
                if verify_tls {
                    eprintln!(
                        "Warning: --verify-tls is only used by the Proxmox provider. Ignoring."
                    );
                    verify_tls = false;
                }
            }
            // --profile is AWS-only, --regions is AWS/Scaleway/GCP/Azure, --project is GCP-only
            if provider != "aws" && profile.is_some() {
                eprintln!("Warning: --profile is only used by the AWS provider. Ignoring.");
                profile = None;
            }
            if !matches!(
                provider.as_str(),
                "aws" | "scaleway" | "gcp" | "azure" | "oracle"
            ) && regions.is_some()
            {
                eprintln!(
                    "Warning: --regions is only used by the AWS, Scaleway, GCP, Azure and Oracle providers. Ignoring."
                );
                regions = None;
            }
            if provider != "gcp" && project.is_some() {
                eprintln!("Warning: --project is only used by the GCP provider. Ignoring.");
                project = None;
            }
            if provider != "oracle" && compartment.is_some() {
                eprintln!("Warning: --compartment is only used by the Oracle provider. Ignoring.");
                compartment = None;
            }

            // When updating an existing section, fall back to stored values for fields not supplied
            let existing_section = providers::config::ProviderConfig::load()
                .section(&provider)
                .cloned();

            if let Some(ref existing) = existing_section {
                // URL fallback only applies to Proxmox (only provider that uses the url field)
                if provider == "proxmox" && url.is_none() && !existing.url.is_empty() {
                    url = Some(existing.url.clone());
                }
                if token.is_none()
                    && !token_stdin
                    && std::env::var("PURPLE_TOKEN").is_err()
                    && !existing.token.is_empty()
                {
                    token = Some(existing.token.clone());
                }
                if prefix.is_none() {
                    prefix = Some(existing.alias_prefix.clone());
                }
                if user.is_none() {
                    user = Some(existing.user.clone());
                }
                if key.is_none() && !existing.identity_file.is_empty() {
                    key = Some(existing.identity_file.clone());
                }
                // Preserve verify_tls=false unless the user explicitly overrides it either way
                if !no_verify_tls && !verify_tls && !existing.verify_tls {
                    no_verify_tls = true;
                }
                // AWS: fall back to stored profile/regions
                if provider == "aws" && profile.is_none() && !existing.profile.is_empty() {
                    profile = Some(existing.profile.clone());
                }
                // AWS/Scaleway/GCP/Azure: fall back to stored regions
                if matches!(
                    provider.as_str(),
                    "aws" | "scaleway" | "gcp" | "azure" | "oracle"
                ) && regions.is_none()
                    && !existing.regions.is_empty()
                {
                    regions = Some(existing.regions.clone());
                }
                // GCP: fall back to stored project
                if provider == "gcp" && project.is_none() && !existing.project.is_empty() {
                    project = Some(existing.project.clone());
                }
                // Oracle: fall back to stored compartment
                if provider == "oracle" && compartment.is_none() && !existing.compartment.is_empty()
                {
                    compartment = Some(existing.compartment.clone());
                }
            }

            // Proxmox requires --url
            if provider == "proxmox" {
                if url.is_none() || url.as_deref().unwrap_or("").trim().is_empty() {
                    eprintln!("Proxmox requires --url (e.g. --url https://pve.example.com:8006).");
                    std::process::exit(1);
                }
                let u = url.as_deref().unwrap();
                if !u.to_ascii_lowercase().starts_with("https://") {
                    eprintln!(
                        "URL must start with https://. For self-signed certificates use --no-verify-tls."
                    );
                    std::process::exit(1);
                }
            }

            // AWS allows empty token when --profile is set
            let aws_has_profile =
                provider == "aws" && profile.as_deref().is_some_and(|p| !p.trim().is_empty());
            let token = if aws_has_profile
                && token.is_none()
                && !token_stdin
                && std::env::var("PURPLE_TOKEN").is_err()
            {
                String::new()
            } else {
                match super::resolve_token(token, token_stdin) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                }
            };

            if token.trim().is_empty() && !aws_has_profile && provider != "tailscale" {
                if provider == "gcp" {
                    eprintln!(
                        "Token can't be empty. Provide a service account JSON key file path or access token."
                    );
                } else if provider == "oracle" {
                    eprintln!(
                        "Token can't be empty. Provide the path to your OCI config file (e.g. ~/.oci/config)."
                    );
                } else {
                    eprintln!(
                        "Token can't be empty. Grab one from your {} dashboard.",
                        providers::provider_display_name(&provider)
                    );
                }
                std::process::exit(1);
            }

            let alias_prefix = prefix.unwrap_or_else(|| p.short_label().to_string());
            if crate::ssh_config::model::is_host_pattern(&alias_prefix) {
                eprintln!("Alias prefix can't contain spaces or pattern characters (*, ?, [, !).");
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
            ] {
                if value.chars().any(|c| c.is_control()) {
                    eprintln!("{} contains control characters.", name);
                    std::process::exit(1);
                }
            }
            if user.contains(char::is_whitespace) {
                eprintln!("User can't contain whitespace.");
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
                !matches!(provider.as_str(), "proxmox")
            };

            let resolved_profile = profile.unwrap_or_default();
            let resolved_regions = regions.unwrap_or_default();
            let resolved_project = project.unwrap_or_default();
            let resolved_compartment = compartment.unwrap_or_default();

            // AWS/Scaleway/Azure requires at least one region/zone/subscription
            if provider == "aws" && resolved_regions.trim().is_empty() {
                eprintln!("AWS requires --regions (e.g. --regions us-east-1,eu-west-1).");
                std::process::exit(1);
            }
            if provider == "scaleway" && resolved_regions.trim().is_empty() {
                eprintln!(
                    "Scaleway requires --regions with one or more zones (e.g. --regions fr-par-1,nl-ams-1)."
                );
                std::process::exit(1);
            }
            if provider == "azure" {
                if resolved_regions.trim().is_empty() {
                    eprintln!("Azure requires --regions with one or more subscription IDs.");
                    std::process::exit(1);
                }
                for sub in resolved_regions
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                {
                    if !providers::azure::is_valid_subscription_id(sub) {
                        eprintln!(
                            "Invalid subscription ID '{}'. Expected UUID format (e.g. 12345678-1234-1234-1234-123456789012).",
                            sub
                        );
                        std::process::exit(1);
                    }
                }
            }
            // GCP requires --project
            if provider == "gcp" && resolved_project.trim().is_empty() {
                eprintln!("GCP requires --project (e.g. --project my-gcp-project-id).");
                std::process::exit(1);
            }
            // Oracle requires --compartment
            if provider == "oracle" && resolved_compartment.trim().is_empty() {
                eprintln!(
                    "Oracle requires --compartment (e.g. --compartment ocid1.compartment.oc1..aaa...)."
                );
                std::process::exit(1);
            }

            let section = providers::config::ProviderSection {
                provider: provider.clone(),
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
                vault_role: String::new(),
                vault_addr: String::new(),
            };

            let mut config = providers::config::ProviderConfig::load();
            config.set_section(section);
            config
                .save()
                .map_err(|e| anyhow::anyhow!("Failed to save: {}", e))?;
            println!("Saved {} configuration.", provider);
            Ok(())
        }
        ProviderCommands::List => {
            let config = providers::config::ProviderConfig::load();
            let sections = config.configured_providers();
            if sections.is_empty() {
                println!("No providers configured. Run 'purple provider add' to set one up.");
            } else {
                for s in sections {
                    let display_name = providers::provider_display_name(s.provider.as_str());
                    println!("  {:<16} {}-*{:>8}", display_name, s.alias_prefix, s.user);
                }
            }
            Ok(())
        }
        ProviderCommands::Remove { provider } => {
            let mut config = providers::config::ProviderConfig::load();
            if config.section(&provider).is_none() {
                eprintln!("No configuration for '{}'. Nothing to remove.", provider);
                std::process::exit(1);
            }
            config.remove_section(&provider);
            config
                .save()
                .map_err(|e| anyhow::anyhow!("Failed to save: {}", e))?;
            println!("Removed {} configuration.", provider);
            Ok(())
        }
    }
}

pub(super) fn handle_tunnel_command(
    mut config: SshConfigFile,
    command: TunnelCommands,
) -> Result<()> {
    match command {
        TunnelCommands::List { alias } => {
            if let Some(alias) = alias {
                // Show tunnels for a specific host
                if !config.has_host(&alias) {
                    eprintln!("No host '{}' found.", alias);
                    std::process::exit(1);
                }
                let rules = config.find_tunnel_directives(&alias);
                if rules.is_empty() {
                    println!("No tunnels configured for {}.", alias);
                } else {
                    println!("Tunnels for {}:", alias);
                    for rule in &rules {
                        println!("  {}", rule.display());
                    }
                }
            } else {
                // Show all hosts with tunnels
                let entries = config.host_entries();
                let with_tunnels: Vec<_> = entries.iter().filter(|e| e.tunnel_count > 0).collect();
                if with_tunnels.is_empty() {
                    println!("No tunnels configured.");
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
                eprintln!("No host '{}' found.", alias);
                std::process::exit(1);
            }
            if config.is_included_host(&alias) {
                eprintln!(
                    "Host '{}' is from an included file and cannot be modified.",
                    alias
                );
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
                eprintln!("Forward {} already exists on {}.", forward, alias);
                std::process::exit(1);
            }
            config.add_forward(&alias, key, &value);
            if let Err(e) = config.write() {
                eprintln!("Failed to save config: {}", e);
                std::process::exit(1);
            }
            println!("Added {} to {}.", forward, alias);
            Ok(())
        }
        TunnelCommands::Remove { alias, forward } => {
            if !config.has_host(&alias) {
                eprintln!("No host '{}' found.", alias);
                std::process::exit(1);
            }
            if config.is_included_host(&alias) {
                eprintln!(
                    "Host '{}' is from an included file and cannot be modified.",
                    alias
                );
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
                eprintln!("No matching forward {} found on {}.", forward, alias);
                std::process::exit(1);
            }
            if let Err(e) = config.write() {
                eprintln!("Failed to save config: {}", e);
                std::process::exit(1);
            }
            println!("Removed {} from {}.", forward, alias);
            Ok(())
        }
        TunnelCommands::Start { alias } => {
            if !config.has_host(&alias) {
                eprintln!("No host '{}' found.", alias);
                std::process::exit(1);
            }
            let tunnels = config.find_tunnel_directives(&alias);
            if tunnels.is_empty() {
                eprintln!("No forwarding directives configured for '{}'.", alias);
                std::process::exit(1);
            }
            println!("Starting tunnel for {}... (Ctrl+C to stop)", alias);
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
pub(super) fn prompt_hidden_input(prompt: &str) -> Result<Option<String>> {
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
                crossterm::event::KeyCode::Backspace => {
                    if input.pop().is_some() {
                        eprint!("\x08 \x08");
                    }
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
pub(super) fn handle_password_command(command: PasswordCommands) -> Result<()> {
    match command {
        PasswordCommands::Set { alias } => {
            let password = match prompt_hidden_input(&format!("Password for {}: ", alias))? {
                Some(p) if !p.is_empty() => p,
                Some(_) => {
                    eprintln!("Password can't be empty.");
                    std::process::exit(1);
                }
                None => {
                    eprintln!("Cancelled.");
                    std::process::exit(1);
                }
            };

            askpass::store_in_keychain(&alias, &password)?;
            println!(
                "Password stored for {}. Set 'keychain' as password source to use it.",
                alias
            );
            Ok(())
        }
        PasswordCommands::Remove { alias } => {
            askpass::remove_from_keychain(&alias)?;
            println!("Password removed for {}.", alias);
            Ok(())
        }
    }
}

pub(super) fn handle_snippet_command(
    config: SshConfigFile,
    command: SnippetCommands,
    config_path: &Path,
) -> Result<()> {
    match command {
        SnippetCommands::List => {
            let store = snippet::SnippetStore::load();
            if store.snippets.is_empty() {
                println!("No snippets configured. Use 'purple snippet add' to create one.");
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
        } => {
            if let Err(e) = snippet::validate_name(&name) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
            if let Err(e) = snippet::validate_command(&command) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
            if let Some(ref desc) = description {
                if desc.contains(|c: char| c.is_control()) {
                    eprintln!("Description contains control characters.");
                    std::process::exit(1);
                }
            }
            let mut store = snippet::SnippetStore::load();
            let is_update = store.get(&name).is_some();
            store.set(snippet::Snippet {
                name: name.clone(),
                command,
                description: description.unwrap_or_default(),
            });
            store.save()?;
            if is_update {
                println!("Updated snippet '{}'.", name);
            } else {
                println!("Added snippet '{}'.", name);
            }
            Ok(())
        }
        SnippetCommands::Remove { name } => {
            let mut store = snippet::SnippetStore::load();
            if store.get(&name).is_none() {
                eprintln!("No snippet '{}' found.", name);
                std::process::exit(1);
            }
            store.remove(&name);
            store.save()?;
            println!("Removed snippet '{}'.", name);
            Ok(())
        }
        SnippetCommands::Run {
            name,
            alias,
            tag,
            all,
            parallel,
        } => {
            let store = snippet::SnippetStore::load();
            let snip = match store.get(&name) {
                Some(s) => s.clone(),
                None => {
                    eprintln!("No snippet '{}' found.", name);
                    std::process::exit(1);
                }
            };

            let entries = config.host_entries();

            // Determine target hosts
            let targets: Vec<&HostEntry> = if let Some(ref alias) = alias {
                match entries.iter().find(|h| h.alias == *alias) {
                    Some(h) => vec![h],
                    None => {
                        eprintln!("No host '{}' found.", alias);
                        std::process::exit(1);
                    }
                }
            } else if let Some(ref tag_filter) = tag {
                let matched: Vec<_> = entries
                    .iter()
                    .filter(|h| h.tags.iter().any(|t| t.eq_ignore_ascii_case(tag_filter)))
                    .collect();
                if matched.is_empty() {
                    eprintln!("No hosts found with tag '{}'.", tag_filter);
                    std::process::exit(1);
                }
                matched
            } else if all {
                entries.iter().collect()
            } else {
                eprintln!("Specify a host alias, --tag or --all.");
                std::process::exit(1);
            };

            if targets.len() == 1 {
                // Single host: run directly
                let host = targets[0];
                let askpass = host
                    .askpass
                    .clone()
                    .or_else(preferences::load_askpass_default);
                let bw_session = super::ensure_bw_session(None, askpass.as_deref());
                super::ensure_keychain_password(&host.alias, askpass.as_deref());
                match snippet::run_snippet(
                    &host.alias,
                    config_path,
                    &snip.command,
                    askpass.as_deref(),
                    bw_session.as_deref(),
                    false,
                    false,
                ) {
                    Ok(r) => {
                        if !r.status.success() {
                            std::process::exit(r.status.code().unwrap_or(1));
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed: {}", e);
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
                    let askpass = h.askpass.clone().or_else(preferences::load_askpass_default);
                    askpass.as_deref().unwrap_or("").starts_with("bw:")
                });
                let bw_session = if any_bw {
                    let bw_askpass = targets
                        .iter()
                        .find_map(|h| h.askpass.as_ref().filter(|a| a.starts_with("bw:")))
                        .cloned()
                        .or_else(preferences::load_askpass_default);
                    super::ensure_bw_session(None, bw_askpass.as_deref())
                } else {
                    None
                };
                let targets_info: Vec<_> = targets
                    .iter()
                    .map(|h| {
                        let askpass = h.askpass.clone().or_else(preferences::load_askpass_default);
                        super::ensure_keychain_password(&h.alias, askpass.as_deref());
                        (h.alias.clone(), askpass)
                    })
                    .collect();
                let command = snip.command.clone();
                thread::spawn(move || {
                    for (alias, askpass) in targets_info {
                        let _ = slot_rx.recv();
                        let slot_tx = slot_tx.clone();
                        let tx = tx.clone();
                        let config_path = config_path.clone();
                        let command = command.clone();
                        let bw_session = bw_session.clone();
                        thread::spawn(move || {
                            let result = snippet::run_snippet(
                                &alias,
                                &config_path,
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
                            }
                            Err(e) => eprintln!("[{}] Failed: {}", alias, e),
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
                        .or_else(preferences::load_askpass_default);
                    if let Some(token) =
                        super::ensure_bw_session(bw_session.as_deref(), askpass.as_deref())
                    {
                        bw_session = Some(token);
                    }
                    super::ensure_keychain_password(&host.alias, askpass.as_deref());
                    println!("── {} ──", host.alias);
                    match snippet::run_snippet(
                        &host.alias,
                        config_path,
                        &snip.command,
                        askpass.as_deref(),
                        bw_session.as_deref(),
                        false,
                        false,
                    ) {
                        Ok(r) => {
                            if !r.status.success() {
                                eprintln!("Exited with code {}.", r.status.code().unwrap_or(1));
                            }
                        }
                        Err(e) => eprintln!("[{}] Failed: {}", host.alias, e),
                    }
                    println!();
                }
            }
            Ok(())
        }
    }
}

pub(super) fn handle_logs_command(tail: bool, clear: bool) -> Result<()> {
    let path = logging::log_path().context("Could not determine log path")?;
    if clear {
        if path.exists() {
            std::fs::remove_file(&path)?;
            println!("Log file deleted: {}", path.display());
        } else {
            println!("No log file found at {}", path.display());
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

pub(super) fn handle_theme_command(command: ThemeCommands) -> Result<()> {
    match command {
        ThemeCommands::List => {
            let current = preferences::load_theme().unwrap_or_else(|| "Purple".to_string());
            println!("Built-in themes:");
            for theme in ui::theme::ThemeDef::builtins() {
                let marker = if theme.name.eq_ignore_ascii_case(&current) {
                    "*"
                } else {
                    " "
                };
                println!("  {} {}", marker, theme.name);
            }
            let custom = ui::theme::ThemeDef::load_custom();
            if !custom.is_empty() {
                println!("\nCustom themes:");
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
                ui::theme::ThemeDef::load_custom()
                    .into_iter()
                    .find(|t| t.name.eq_ignore_ascii_case(&name))
            });
            match found {
                Some(theme) => {
                    preferences::save_theme(&theme.name)?;
                    println!("Theme set to: {}", theme.name);
                }
                None => {
                    anyhow::bail!("Unknown theme: {}", name);
                }
            }
        }
    }
    Ok(())
}

pub(super) fn handle_vault_sign_command(
    mut config: SshConfigFile,
    alias: Option<String>,
    all: bool,
    cli_vault_addr: Option<String>,
) -> Result<()> {
    if let Some(ref addr) = cli_vault_addr {
        if !vault_ssh::is_valid_vault_addr(addr) {
            anyhow::bail!(
                "Invalid --vault-addr value. Must be non-empty, no whitespace or control chars."
            );
        }
    }
    let provider_config = providers::config::ProviderConfig::load();
    let entries = config.host_entries();

    if all {
        let mut signed = 0u32;
        let mut failed = 0u32;
        let mut skipped = 0u32;

        for entry in &entries {
            let role = match vault_ssh::resolve_vault_role(
                entry.vault_ssh.as_deref(),
                entry.provider.as_deref(),
                &provider_config,
            ) {
                Some(r) => r,
                None => {
                    skipped += 1;
                    continue;
                }
            };

            let pubkey = match vault_ssh::resolve_pubkey_path(&entry.identity_file) {
                Ok(p) => p,
                Err(e) => {
                    println!("Skipping {}: {}", entry.alias, e);
                    failed += 1;
                    continue;
                }
            };
            let cert_path = vault_ssh::resolve_cert_path(&entry.alias, &entry.certificate_file)?;
            let status = vault_ssh::check_cert_validity(&cert_path);

            if !vault_ssh::needs_renewal(&status) {
                skipped += 1;
                continue;
            }

            // Flag beats per-host beats provider default.
            let resolved_addr = cli_vault_addr.clone().or_else(|| {
                vault_ssh::resolve_vault_addr(
                    entry.vault_addr.as_deref(),
                    entry.provider.as_deref(),
                    &provider_config,
                )
            });
            print!("Signing {}... ", entry.alias);
            match vault_ssh::sign_certificate(
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
                                "  warning: {} no longer in ssh config; CertificateFile not written (cert saved on disk)",
                                entry.alias
                            );
                        }
                    }
                    signed += 1;
                }
                Err(e) => {
                    println!("failed: {}", e);
                    failed += 1;
                }
            }
        }
        if signed > 0 {
            if let Err(e) = config.write() {
                eprintln!("Warning: Failed to update SSH config: {}", e);
            }
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
            &provider_config,
        )
        .with_context(|| {
            format!(
                "No Vault SSH role configured for '{}'. Set it in the host form (Vault SSH Role field) or in the provider config (vault_role).",
                alias
            )
        })?;

        let pubkey = vault_ssh::resolve_pubkey_path(&entry.identity_file)?;
        let resolved_addr = cli_vault_addr.clone().or_else(|| {
            vault_ssh::resolve_vault_addr(
                entry.vault_addr.as_deref(),
                entry.provider.as_deref(),
                &provider_config,
            )
        });
        let result = vault_ssh::sign_certificate(&role, &pubkey, &alias, resolved_addr.as_deref())?;
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
                // so this is effectively unreachable — but surface it
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
        println!("Certificate signed: {}", result.cert_path.display());
    } else {
        anyhow::bail!("Provide a host alias or use --all");
    }
    Ok(())
}
