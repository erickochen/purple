// Process bootstrap and the few CLI/TUI entry points that were small enough
// to live next to fn main while big enough to clutter it. Lives in the lib
// so integration tests and future entry points (e.g. an embedded TUI) can
// reach the same surface.

use std::path::Path;

use anyhow::Result;
use clap::CommandFactory;
use clap_complete::generate;

use crate::app::App;
use crate::cli_args::{Cli, Commands, VaultCommands};
use crate::runtime::helpers::{
    ConfigSource, ResolvedConfigPath, apply_saved_sort, ensure_bw_session,
    ensure_keychain_password, ensure_proton_login, ensure_vault_ssh_chain_if_needed,
    expand_user_path, resolve_ssh_config_path,
};
use crate::ssh_config::model::SshConfigFile;
use crate::tui_loop::run_tui;
use crate::{
    askpass, cli, connection, demo, history, key_activity, logging, mcp, messages, preferences,
    providers, snippet, ui, update,
};

/// Bootstrap the process after `Cli::parse()`. Owns theme init, logging
/// init, the subcommand dispatch table and the TUI launch path. Returns
/// when the chosen mode exits (subcommand return, direct-connect exit,
/// TUI quit).
pub fn run(cli: Cli, env: std::sync::Arc<crate::runtime::env::Env>) -> Result<()> {
    crate::i18n::initialize(
        cli.lang.as_deref(),
        env.paths().map(|p| p.config_dir().join("language")),
    )?;
    if let Some(language) = &cli.set_language {
        crate::i18n::save(language)?;
        println!("{}", crate::messages::language::selected(language));
        return Ok(());
    }
    let migration = seed_layout_and_theme(&env);

    // Determine if this is a CLI subcommand (log to stderr too) or TUI (file only)
    let is_cli_subcommand = cli.command.is_some() || cli.list || cli.connect.is_some();
    logging::init(cli.verbose, is_cli_subcommand, &env);
    if let Some(report) = migration {
        log_migration(&report);
    }

    if let Some(ref name) = cli.theme {
        if let Some(theme) = ui::theme::ThemeDef::find_builtin(name).or_else(|| {
            ui::theme::ThemeDef::load_custom(env.paths())
                .into_iter()
                .find(|t| t.name.eq_ignore_ascii_case(name))
        }) {
            ui::theme::set_theme(theme);
        } else {
            anyhow::bail!("Unknown theme: {}", name);
        }
    }

    // Shell completions (no config file needed)
    if let Some(shell) = cli.completions {
        let mut cmd = Cli::command();
        generate(shell, &mut cmd, "purple", &mut std::io::stdout());
        return Ok(());
    }

    if cli.demo {
        let mut app = demo::build_demo_app();
        demo::seed_whats_new_toast(&mut app);
        demo::seed_tunnel_live_snapshots(&mut app);
        return run_tui(app);
    }

    // Asset generation (internal, used by the release imagery pipeline). Uses
    // demo data, so no SSH config is needed.
    if let Some(Commands::GenAssets {
        out_dir,
        font_dir,
        hero_out,
    }) = &cli.command
    {
        let font = if font_dir.is_empty() {
            None
        } else {
            Some(Path::new(font_dir))
        };
        let written = crate::asset_gen::generate(Path::new(out_dir), font)?;
        println!("{}", messages::cli::gen_assets_done(written.len(), out_dir));
        if let Some(hero) = hero_out {
            crate::asset_gen::generate_hero(Path::new(hero), font)?;
            println!("{}", messages::cli::gen_assets_hero_done(hero));
        }
        return Ok(());
    }

    // Provider and Update subcommands don't need SSH config
    if let Some(Commands::Provider { command }) = cli.command {
        return cli::handle_provider_command(&env, command);
    }
    if let Some(Commands::Update) = cli.command {
        return update::self_update(&env);
    }
    if let Some(Commands::Password { command }) = cli.command {
        return cli::handle_password_command(&env, command);
    }
    if let Some(Commands::Mcp {
        read_only,
        no_audit,
        audit_log,
    }) = cli.command
    {
        let resolved = resolve_ssh_config_path(&env, cli.config.as_deref())?;
        missing_config_notice(&resolved);
        let audit_log_path = if no_audit {
            None
        } else if let Some(path) = audit_log {
            Some(expand_user_path(env.paths(), &path)?)
        } else {
            mcp::default_audit_log_path(env.paths())
        };
        let options = mcp::McpOptions {
            read_only,
            audit_log_path,
        };
        return mcp::run(&resolved.path, options, std::sync::Arc::clone(&env));
    }
    if let Some(Commands::Logs { tail, clear }) = cli.command {
        return cli::handle_logs_command(tail, clear, &env);
    }
    if let Some(Commands::Theme { command }) = cli.command {
        return cli::handle_theme_command(&env, command);
    }
    if let Some(Commands::WhatsNew { since }) = &cli.command {
        let output = cli::run_whats_new(since.as_deref())?;
        print!("{}", output);
        return Ok(());
    }

    let resolved = resolve_ssh_config_path(&env, cli.config.as_deref())?;
    let config_missing = missing_config_notice(&resolved);
    let config_path = resolved.path;
    let mut config = SshConfigFile::parse_with_env(&config_path, &env)?;
    let notices = StartupNotices {
        repaired_groups: config.repair_absorbed_group_comments(),
        orphaned_headers: config.remove_all_orphaned_group_headers(),
        config_missing,
    };

    let ssh_version = write_startup_banner(&config, &config_path, cli.verbose, &env);

    // Handle subcommands that need SSH config
    match cli.command {
        Some(Commands::Add { target, alias, key }) => {
            return cli::handle_quick_add(config, &target, alias.as_deref(), key.as_deref());
        }
        Some(Commands::Import {
            file,
            known_hosts,
            group,
        }) => {
            return cli::handle_import(
                &env,
                config,
                file.as_deref(),
                known_hosts,
                group.as_deref(),
            );
        }
        Some(Commands::Sync {
            provider,
            dry_run,
            remove,
        }) => {
            return cli::handle_sync(&env, config, provider.as_deref(), dry_run, remove);
        }
        Some(Commands::Tunnel { command }) => {
            return cli::handle_tunnel_command(config, command);
        }
        Some(Commands::Snippet { command }) => {
            return cli::handle_snippet_command(&env, config, command, &config_path);
        }
        Some(Commands::Vault {
            command:
                VaultCommands::Sign {
                    alias,
                    all,
                    vault_addr: cli_vault_addr,
                },
        }) => {
            return cli::handle_vault_sign_command(&env, config, alias, all, cli_vault_addr);
        }
        Some(Commands::Provider { .. })
        | Some(Commands::Update)
        | Some(Commands::Password { .. })
        | Some(Commands::Mcp { .. })
        | Some(Commands::Theme { .. })
        | Some(Commands::Logs { .. })
        | Some(Commands::WhatsNew { .. })
        | Some(Commands::GenAssets { .. }) => unreachable!(),
        None => {}
    }

    // Direct connect mode (--connect)
    if let Some(alias) = cli.connect {
        run_direct_connect(alias, &mut config, &config_path, &env)?;
    }

    // List mode
    if cli.list {
        print_host_list(&config);
        return Ok(());
    }

    // Positional argument: exact match → connect, otherwise → TUI with filter
    if let Some(ref alias) = cli.alias {
        return run_positional_alias(alias, config, &config_path, &notices, env, &ssh_version);
    }

    // Interactive TUI mode
    let mut app = App::with_env(config, std::sync::Arc::clone(&env));
    app.seed_ssh_version(&ssh_version);
    app.post_init();
    apply_saved_sort(&mut app);
    notify_startup(&mut app, &notices);
    run_tui(app)
}

/// Toasts shown once the TUI is up: config repairs and a config path that
/// does not exist yet.
struct StartupNotices {
    repaired_groups: usize,
    orphaned_headers: usize,
    config_missing: Option<String>,
}

fn notify_startup(app: &mut App, notices: &StartupNotices) {
    if notices.repaired_groups > 0 || notices.orphaned_headers > 0 {
        app.notify(messages::config_repaired(
            notices.repaired_groups,
            notices.orphaned_headers,
        ));
    }
    if let Some(notice) = &notices.config_missing {
        app.notify_warning(notice.clone());
    }
}

/// Log a notice when a config path the user chose does not exist yet and
/// return it for the TUI. The default path stays silent: a fresh install
/// gets the welcome screen instead.
fn missing_config_notice(resolved: &ResolvedConfigPath) -> Option<String> {
    if resolved.source == ConfigSource::Default || resolved.path.exists() {
        return None;
    }
    let notice = messages::ssh_config_not_found(&resolved.path.display(), &resolved.source);
    log::warn!("[config] {notice}");
    Some(notice)
}

/// Seed split category directories from `~/.purple`, then load the theme.
/// The order matters: the theme comes from the config directory, so the
/// copy has to land first. Logging is not up yet, so the report is returned
/// for the caller to log.
fn seed_layout_and_theme(
    env: &crate::runtime::env::Env,
) -> Option<crate::runtime::layout::MigrationReport> {
    let migration = env
        .paths()
        .map(crate::runtime::layout::migrate_legacy_layout);
    ui::theme::init(env);
    migration
}

/// Record what the startup layout migration copied. Every copy is a state
/// change on disk; failures leave the legacy file as the only copy.
fn log_migration(report: &crate::runtime::layout::MigrationReport) {
    for (src, dst) in &report.copied {
        log::debug!(
            "[purple] Copied {} to {} (legacy copy left in place)",
            src.display(),
            dst.display()
        );
    }
    for (src, err) in &report.failed {
        log::warn!(
            "[config] Could not copy {} to its new directory: {err}",
            src.display()
        );
    }
    if !report.copied.is_empty() {
        log::info!(
            "[purple] Seeded {} entries from ~/.purple into the split directories",
            report.copied.len()
        );
    }
}

/// Collect environment + config metadata and write a startup banner to the
/// log file. Runs once at process start so support bundles always show
/// the SSH config path, active providers, askpass sources and Vault
/// posture under which purple ran.
fn write_startup_banner(
    config: &SshConfigFile,
    config_path: &Path,
    verbose: bool,
    env: &crate::runtime::env::Env,
) -> String {
    let level_str = logging::level_name(verbose, env);
    let provider_config = providers::config::ProviderConfig::load(env.paths());

    let provider_names: Vec<String> = provider_config
        .sections
        .iter()
        .map(|s| s.provider().to_string())
        .collect();

    let askpass_sources: Vec<String> = config
        .host_entries()
        .iter()
        .filter_map(|h| h.askpass.as_ref())
        .map(|s| s.to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    let vault_ssh_info = {
        let has_host_level = config.host_entries().iter().any(|h| h.vault_ssh.is_some());
        let has_provider_level = provider_config
            .sections
            .iter()
            .any(|s| !s.vault_role.is_empty());
        if has_host_level || has_provider_level {
            let addr = config
                .host_entries()
                .iter()
                .find_map(|h| h.vault_addr.clone())
                .or_else(|| {
                    provider_config
                        .sections
                        .iter()
                        .find(|s| !s.vault_addr.is_empty())
                        .map(|s| s.vault_addr.clone())
                })
                .or_else(|| env.vault_addr().map(str::to_string))
                .unwrap_or_else(|| "not set".to_string());
            Some(format!("enabled (addr={addr})"))
        } else {
            None
        }
    };

    let ssh_version = logging::detect_ssh_version();
    let term = env.term().unwrap_or("unset").to_string();
    let colorterm = env.colorterm().unwrap_or("unset").to_string();
    let theme = preferences::load_theme(env.paths()).unwrap_or_else(|| "Purple".to_string());
    let hosts = config.host_entries().len();
    let patterns = config.pattern_entries().len();
    let snippets = snippet::SnippetStore::load(env.paths()).snippets.len();
    let proxy_env = collect_proxy_env(env);

    logging::write_banner(
        &logging::BannerInfo {
            version: env!("CARGO_PKG_VERSION"),
            config_path: &config_path.display().to_string(),
            providers: &provider_names,
            askpass_sources: &askpass_sources,
            vault_ssh_info: vault_ssh_info.as_deref(),
            ssh_version: &ssh_version,
            term: &term,
            colorterm: &colorterm,
            level: &level_str,
            theme: &theme,
            hosts,
            patterns,
            snippets,
            proxy_env: &proxy_env,
        },
        env.paths(),
    );
    ssh_version
}

/// Build a compact string describing the proxy-related env vars in effect.
/// Returns `"none"` when none of HTTP_PROXY/HTTPS_PROXY/ALL_PROXY/NO_PROXY
/// are set. Only var names are recorded; values may contain credentials.
fn collect_proxy_env(env: &crate::runtime::env::Env) -> String {
    let set = env.active_proxy_vars();
    if set.is_empty() {
        "none".to_string()
    } else {
        set.join(",")
    }
}

/// Direct-connect mode (`purple --connect <alias>`): resolve askpass and
/// Vault SSH, run `ssh` inline and exit with its status code. Never
/// returns on success. Always calls `std::process::exit`.
fn run_direct_connect(
    alias: String,
    config: &mut SshConfigFile,
    config_path: &Path,
    env: &crate::runtime::env::Env,
) -> Result<()> {
    let provider_config = providers::config::ProviderConfig::load(env.paths());
    let host_entry = config.host_entries().into_iter().find(|h| h.alias == alias);
    if host_entry.is_some()
        && let Some((msg, _is_error)) =
            ensure_vault_ssh_chain_if_needed(env, &alias, config_path, &provider_config, config)
    {
        eprintln!("{}", msg);
    }
    let askpass = host_entry
        .as_ref()
        .and_then(|h| h.askpass.clone())
        .or_else(|| preferences::load_askpass_default(env.paths()));
    ensure_proton_login(env, askpass.as_deref());
    let bw_session = ensure_bw_session(env, None, askpass.as_deref());
    ensure_keychain_password(env, &alias, askpass.as_deref());
    let launcher = crate::ssh_launcher::SshLauncher::resolve(env);
    let result = connection::connect(
        &alias,
        config_path,
        askpass.as_deref(),
        bw_session.as_deref(),
        false,
        &launcher,
    )?;
    let code = result.status.code().unwrap_or(1);
    if code != 255 {
        history::ConnectionHistory::load(env.paths()).record(&alias);
        key_activity::KeyActivityLog::record_oneshot(&alias, key_activity::now_secs(), env.paths());
    }
    askpass::cleanup_marker(env.paths(), &alias);
    std::process::exit(code);
}

/// Positional-alias mode (`purple <alias>`): if the alias is an exact
/// match, connect directly. Otherwise open the TUI with the alias
/// pre-filled as a search filter.
fn run_positional_alias(
    alias: &str,
    mut config: SshConfigFile,
    config_path: &Path,
    notices: &StartupNotices,
    env: std::sync::Arc<crate::runtime::env::Env>,
    ssh_version: &str,
) -> Result<()> {
    let host_opt = config
        .host_entries()
        .iter()
        .find(|h| h.alias == *alias)
        .cloned();
    if let Some(host) = host_opt {
        let provider_config = providers::config::ProviderConfig::load(env.paths());
        if let Some((msg, _is_error)) = ensure_vault_ssh_chain_if_needed(
            &env,
            &host.alias,
            config_path,
            &provider_config,
            &mut config,
        ) {
            eprintln!("{}", msg);
        }
        let alias = host.alias.clone();
        let askpass = host
            .askpass
            .clone()
            .or_else(|| preferences::load_askpass_default(env.paths()));
        ensure_proton_login(&env, askpass.as_deref());
        let bw_session = ensure_bw_session(&env, None, askpass.as_deref());
        ensure_keychain_password(&env, &alias, askpass.as_deref());
        print!("{}", messages::cli::beaming_up(&alias));
        let launcher = crate::ssh_launcher::SshLauncher::resolve(&env);
        let result = connection::connect(
            &alias,
            config_path,
            askpass.as_deref(),
            bw_session.as_deref(),
            false,
            &launcher,
        )?;
        let code = result.status.code().unwrap_or(1);
        if code != 255 {
            history::ConnectionHistory::load(env.paths()).record(&alias);
            key_activity::KeyActivityLog::record_oneshot(
                &alias,
                key_activity::now_secs(),
                env.paths(),
            );
        }
        askpass::cleanup_marker(env.paths(), &alias);
        std::process::exit(code);
    }

    // No exact match. Open TUI with search pre-filled.
    let mut app = App::with_env(config, env);
    app.seed_ssh_version(ssh_version);
    app.post_init();
    apply_saved_sort(&mut app);
    notify_startup(&mut app, notices);
    start_alias_search(&mut app, alias, notices);
    run_tui(app)
}

/// Pre-fill the search with `alias` and say so when nothing matches. A
/// missing config explains the empty result better than a no-match toast.
fn start_alias_search(app: &mut App, alias: &str, notices: &StartupNotices) {
    app.start_search_with(alias);
    if app.search.filtered_indices().is_empty() && notices.config_missing.is_none() {
        app.notify(messages::no_exact_match(alias));
    }
}

/// Plain-text host listing for `purple --list`. Prints `alias user@host:port`
/// rows or the NO_HOSTS marker when the config has no Host blocks.
fn print_host_list(config: &SshConfigFile) {
    let entries = config.host_entries();
    if entries.is_empty() {
        println!("{}", crate::i18n::text(messages::cli::NO_HOSTS));
        return;
    }
    for host in &entries {
        let user = if host.user.is_empty() {
            String::new()
        } else {
            format!("{}@", host.user)
        };
        let port = if host.port == 22 {
            String::new()
        } else {
            format!(":{}", host.port)
        };
        println!("{:<20} {}{}{}", host.alias, user, host.hostname, port);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Builds each scenario as an injected Env. No process-global mutation, so
    // no lock and no serialization against other tests.
    #[test]
    fn collect_proxy_env_reports_set_vars_and_none() {
        use crate::runtime::env::Env;

        assert_eq!(collect_proxy_env(&Env::for_test("/tmp/x")), "none");

        let one = Env::for_test("/tmp/x").with_var("HTTPS_PROXY", "http://proxy.example:3128");
        assert_eq!(collect_proxy_env(&one), "HTTPS_PROXY");

        let two = one.clone().with_var("NO_PROXY", "localhost,127.0.0.1");
        assert_eq!(collect_proxy_env(&two), "HTTPS_PROXY,NO_PROXY");

        // Empty value counts as unset.
        let empty_https = two.with_var("HTTPS_PROXY", "");
        assert_eq!(collect_proxy_env(&empty_https), "NO_PROXY");
    }

    // The theme lives in the config directory, so on the first start with a
    // split layout the seed from ~/.purple must land before the theme loads.
    #[test]
    fn seed_layout_and_theme_loads_the_migrated_theme_on_the_first_run() {
        use crate::runtime::env::{Env, Paths};

        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("home");
        let legacy = home.join(".purple");
        std::fs::create_dir_all(&legacy).expect("legacy dir");
        crate::fs_util::atomic_write(&legacy.join("preferences"), b"theme=Nord\n")
            .expect("seed preferences");
        let cfg = dir.path().join("cfg").to_string_lossy().to_string();
        let paths = Paths::resolve(&home, move |k| {
            (k == "XDG_CONFIG_HOME").then(|| cfg.clone())
        });
        let env = Env::for_test(&home).with_paths(paths.clone());

        let report = seed_layout_and_theme(&env).expect("paths present");

        assert_eq!(report.copied.len(), 1, "{report:?}");
        assert!(paths.preferences().exists());
        assert_eq!(ui::theme::current_theme().name, "Nord");

        ui::theme::set_theme(ui::theme::ThemeDef::purple());
        ui::theme::init_with_mode(1);
    }

    fn startup_app() -> App {
        App::new(SshConfigFile::from_content(
            "",
            std::path::PathBuf::from("/nonexistent/config"),
        ))
    }

    #[test]
    fn notify_startup_queues_the_missing_config_warning() {
        let mut app = startup_app();
        let notice = messages::ssh_config_not_found(&"/nope", &ConfigSource::Preference);
        notify_startup(
            &mut app,
            &StartupNotices {
                repaired_groups: 0,
                orphaned_headers: 0,
                config_missing: Some(notice.clone()),
            },
        );
        let toast = app.status_center.toast().expect("toast queued");
        assert_eq!(toast.text, notice);
        assert_eq!(toast.class, crate::app::MessageClass::Warning);
    }

    #[test]
    fn alias_search_skips_the_no_match_toast_when_the_config_is_missing() {
        let notice = messages::ssh_config_not_found(&"/nope", &ConfigSource::Preference);
        let missing = StartupNotices {
            repaired_groups: 0,
            orphaned_headers: 0,
            config_missing: Some(notice.clone()),
        };
        let mut app = startup_app();
        notify_startup(&mut app, &missing);
        start_alias_search(&mut app, "web", &missing);
        let toast = app.status_center.toast().expect("toast queued");
        assert_eq!(toast.text, notice);

        let present = StartupNotices {
            config_missing: None,
            ..missing
        };
        let mut app = startup_app();
        start_alias_search(&mut app, "web", &present);
        let toast = app.status_center.toast().expect("toast queued");
        assert_eq!(toast.text, messages::no_exact_match("web"));
    }

    #[test]
    fn notify_startup_is_silent_without_notices() {
        let mut app = startup_app();
        notify_startup(
            &mut app,
            &StartupNotices {
                repaired_groups: 0,
                orphaned_headers: 0,
                config_missing: None,
            },
        );
        assert!(app.status_center.toast().is_none());
    }

    #[test]
    fn missing_config_notice_only_for_a_chosen_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("nope");
        let present = dir.path().join("config");
        std::fs::write(&present, "").expect("write config");
        let at = |path: &std::path::Path, source| ResolvedConfigPath {
            path: path.to_path_buf(),
            source,
        };

        let notice = missing_config_notice(&at(&missing, ConfigSource::Preference))
            .expect("chosen missing file gets a notice");
        assert!(notice.contains("SSH config not found at"), "{notice}");
        assert!(notice.contains("(from preference ssh_config)"), "{notice}");

        assert!(missing_config_notice(&at(&missing, ConfigSource::Default)).is_none());
        assert!(missing_config_notice(&at(&present, ConfigSource::Flag)).is_none());
    }

    // The release imagery pipeline drives `purple gen-assets`. Exercise the
    // dispatch arm itself: empty --font-dir maps to None and --hero-out drives
    // the optional animated-hero branch (clap parsing is covered separately).
    #[test]
    fn gen_assets_dispatch_writes_scenes_and_hero() {
        use clap::Parser;

        let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().expect("tempdir");
        let out_dir = tmp.path().join("svg");
        let hero = tmp.path().join("hero.svg");
        let cli = Cli::try_parse_from([
            "purple",
            "gen-assets",
            out_dir.to_str().expect("utf8"),
            "--font-dir",
            "",
            "--hero-out",
            hero.to_str().expect("utf8"),
        ])
        .expect("parse gen-assets");
        let env = std::sync::Arc::new(crate::runtime::env::Env::for_test(tmp.path()));

        run(cli, env).expect("gen-assets dispatch");

        let svgs: Vec<_> = std::fs::read_dir(&out_dir)
            .expect("read svg dir")
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "svg"))
            .collect();
        assert!(!svgs.is_empty(), "scene SVGs written");
        assert!(hero.exists(), "--hero-out wrote the animated hero");
        // Empty --font-dir maps to None, so no @font-face is embedded.
        let body = std::fs::read_to_string(svgs[0].path()).expect("read scene svg");
        assert!(
            !body.contains("@font-face"),
            "empty font-dir embeds no font: {}",
            svgs[0].path().display()
        );

        crate::demo_flag::disable();
    }
}
