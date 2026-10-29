use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use log::LevelFilter;
use simplelog::{
    ColorChoice, CombinedLogger, ConfigBuilder, SharedLogger, TermLogger, TerminalMode, WriteLogger,
};

// Fault domain convention: every log statement carries a prefix.
// - [external] = an external tool, binary, network or remote host failed.
// - [config]   = a user config, parse or user-data problem.
// - [purple]   = our own internal state, flow or invariant.

const MAX_LOG_SIZE: u64 = 5 * 1024 * 1024; // 5MB

/// Return the path to the log file: ~/.purple/purple.log
pub fn log_path(paths: Option<&crate::runtime::env::Paths>) -> Option<PathBuf> {
    paths.map(crate::runtime::env::Paths::log_file)
}

/// Rotate log file if it exceeds MAX_LOG_SIZE.
/// Renames purple.log -> purple.log.1 (overwrites previous backup).
fn rotate_if_needed(path: &Path) {
    if let Ok(meta) = fs::metadata(path)
        && meta.len() > MAX_LOG_SIZE
    {
        let backup = path.with_file_name("purple.log.1");
        let _ = fs::rename(path, backup);
    }
}

/// Determine log level. `env_override` takes precedence over `verbose` flag.
fn resolve_level(verbose: bool, env_override: Option<&str>) -> LevelFilter {
    if let Some(val) = env_override {
        match val.to_lowercase().as_str() {
            "trace" => return LevelFilter::Trace,
            "debug" => return LevelFilter::Debug,
            "info" => return LevelFilter::Info,
            "warn" => return LevelFilter::Warn,
            "error" => return LevelFilter::Error,
            "off" => return LevelFilter::Off,
            _ => {}
        }
    }
    if verbose {
        LevelFilter::Debug
    } else {
        LevelFilter::Warn
    }
}

/// Initialize logging. Call once at the start of main().
///
/// - `verbose`: whether --verbose was passed
/// - `cli_stderr`: if true, also log to stderr (for CLI subcommands, not TUI)
pub fn init(verbose: bool, cli_stderr: bool, env: &crate::runtime::env::Env) {
    let Some(path) = log_path(env.paths()) else {
        return;
    };

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    rotate_if_needed(&path);

    let level = resolve_level(verbose, env.var("PURPLE_LOG"));
    let config = ConfigBuilder::new()
        .set_time_format_rfc3339()
        .set_target_level(LevelFilter::Off)
        .set_thread_level(LevelFilter::Off)
        .build();

    let mut loggers: Vec<Box<dyn SharedLogger>> = Vec::with_capacity(2);

    if let Ok(file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        loggers.push(WriteLogger::new(level, config.clone(), file));
    }

    if cli_stderr {
        loggers.push(TermLogger::new(
            level,
            config,
            TerminalMode::Stderr,
            ColorChoice::Auto,
        ));
    }

    if !loggers.is_empty()
        && let Err(e) = CombinedLogger::init(loggers)
    {
        eprintln!("{}", crate::messages::logging::init_failed(&e));
    }
}

/// Format current UTC time as YYYY-MM-DD HH:MM:SS.
fn format_now_utc() -> String {
    let now = std::time::SystemTime::now();
    let secs = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let (year, month, day) = epoch_days_to_date(days);
    format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Convert days since Unix epoch to (year, month, day).
/// Algorithm from <http://howardhinnant.github.io/date_algorithms.html>.
fn epoch_days_to_date(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Startup banner info. Struct avoids argument-order bugs between &str params.
pub struct BannerInfo<'a> {
    pub version: &'a str,
    pub config_path: &'a str,
    pub providers: &'a [String],
    pub askpass_sources: &'a [String],
    pub vault_ssh_info: Option<&'a str>,
    pub ssh_version: &'a str,
    pub term: &'a str,
    pub colorterm: &'a str,
    pub level: &'a str,
    /// Theme currently in effect (loaded from preferences or `--theme`).
    pub theme: &'a str,
    /// Total number of host entries parsed from the SSH config.
    pub hosts: usize,
    /// Total number of patterns (wildcard / multi-alias `Host` lines).
    pub patterns: usize,
    /// Total number of snippets loaded from the snippet store.
    pub snippets: usize,
    /// Comma-separated list of env vars that affect proxy behaviour (HTTP_PROXY,
    /// HTTPS_PROXY, NO_PROXY) when any are set. `"none"` when none are present.
    pub proxy_env: &'a str,
}

/// Write startup banner directly to log file, bypassing level filters.
/// All timestamps are UTC (suffixed with Z).
///
/// Note: writes directly to the log file, bypassing simplelog's CombinedLogger.
/// This means the banner will not appear on stderr in CLI mode. This is intentional:
/// the banner is diagnostic context for the log file, not user-facing output.
pub fn write_banner(info: &BannerInfo<'_>, paths: Option<&crate::runtime::env::Paths>) {
    let Some(path) = log_path(paths) else { return };
    let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };

    let now = format_now_utc();
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let providers_joined = if info.providers.is_empty() {
        "none".to_string()
    } else {
        info.providers.join(",")
    };
    let askpass_joined = if info.askpass_sources.is_empty() {
        "none".to_string()
    } else {
        info.askpass_sources.join(",")
    };

    let mut banner = format!(
        "--- purple v{} started at {now} ---\n\
         \x20   os={os} arch={arch} config={}\n\
         \x20   ssh={}\n\
         \x20   term={} colorterm={}\n\
         \x20   theme={}\n\
         \x20   hosts={} patterns={} snippets={}\n\
         \x20   providers={providers_joined}\n\
         \x20   askpass={askpass_joined}\n\
         \x20   proxy_env={}\n",
        info.version,
        info.config_path,
        info.ssh_version,
        info.term,
        info.colorterm,
        info.theme,
        info.hosts,
        info.patterns,
        info.snippets,
        info.proxy_env,
    );
    if let Some(vault_info) = info.vault_ssh_info {
        banner.push_str(&format!("    vault_ssh={vault_info}\n"));
    }
    if let Some(p) = paths {
        banner.push_str(&format!(
            "    dirs: config={} data={} state={} cache={}\n",
            p.config_dir().display(),
            p.data_dir().display(),
            p.state_dir().display(),
            p.cache_dir().display()
        ));
    }
    banner.push_str(&format!("    log_level={}\n", info.level));

    // Note: banner lines use \x20 (space) prefix to prevent the Rust string
    // continuation from collapsing leading whitespace. This is a rustfmt-safe
    // idiom for multi-line format strings with indentation.

    // Non-fatal: banner write failure does not affect logging
    let _ = file.write_all(banner.as_bytes());
}

/// Return the effective log level name as a lowercase string.
pub fn level_name(verbose: bool, env: &crate::runtime::env::Env) -> String {
    resolve_level(verbose, env.var("PURPLE_LOG"))
        .as_str()
        .to_lowercase()
}

/// Detect SSH version by running `ssh -V` (output goes to stderr).
/// Uses a 2-second timeout via mpsc channel to avoid hanging startup
/// if the ssh binary is broken or on a slow filesystem.
///
/// On timeout, the spawned thread and child process continue running until
/// `ssh -V` exits naturally. This is acceptable because `ssh -V` is
/// near-instant and this only runs once at startup.
pub fn detect_ssh_version() -> String {
    query_ssh_version().unwrap_or_else(|| {
        eprintln!(
            "{}",
            crate::i18n::text(crate::messages::logging::SSH_VERSION_FAILED)
        );
        "unknown".to_string()
    })
}

/// `ssh -V` output, or "unknown" when ssh gave no usable answer in time.
/// `None` when ssh could not be started. Prints nothing, so the TUI can call
/// it while it owns the terminal.
pub(crate) fn query_ssh_version() -> Option<String> {
    use std::sync::mpsc;
    use std::time::Duration;

    let child = std::process::Command::new("ssh")
        .arg("-V")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .ok()?;

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });

    Some(match rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(output)) => {
            let out = if output.stderr.is_empty() {
                output.stdout
            } else {
                output.stderr
            };
            String::from_utf8(out)
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| "unknown".to_string())
        }
        _ => "unknown".to_string(),
    })
}

/// Thread-local log capture for tests. The global logger writes records into a
/// per-thread sink that is only active inside `capture`, so parallel test
/// threads never see each other's records and no global lock is needed.
#[cfg(test)]
pub(crate) mod capture {
    use std::cell::RefCell;
    use std::sync::Once;

    use log::{Level, Log, Metadata, Record};

    thread_local! {
        static SINK: RefCell<Option<Vec<(Level, String)>>> = const { RefCell::new(None) };
    }

    struct CaptureLogger;

    impl Log for CaptureLogger {
        fn enabled(&self, _: &Metadata) -> bool {
            true
        }
        fn log(&self, record: &Record) {
            SINK.with(|s| {
                if let Some(buf) = s.borrow_mut().as_mut() {
                    buf.push((record.level(), record.args().to_string()));
                }
            });
        }
        fn flush(&self) {}
    }

    static LOGGER: CaptureLogger = CaptureLogger;
    static INSTALL: Once = Once::new();

    /// Run `f` and return every log record it emitted on this thread.
    pub(crate) fn capture<F: FnOnce()>(f: F) -> Vec<(Level, String)> {
        INSTALL.call_once(|| {
            // No production test installs a logger, so this wins. If one ever
            // does, our per-thread sink simply stays empty (set_logger errors).
            if log::set_logger(&LOGGER).is_ok() {
                log::set_max_level(log::LevelFilter::Trace);
            }
        });
        SINK.with(|s| *s.borrow_mut() = Some(Vec::new()));
        f();
        SINK.with(|s| s.borrow_mut().take().unwrap_or_default())
    }

    /// True when any captured record matches `level` and contains `needle`.
    pub(crate) fn has(records: &[(Level, String)], level: Level, needle: &str) -> bool {
        records
            .iter()
            .any(|(lvl, msg)| *lvl == level && msg.contains(needle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn rotate_if_needed_renames_large_file() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("purple.log");
        let backup = dir.path().join("purple.log.1");

        let mut f = fs::File::create(&log).unwrap();
        let data = vec![0u8; (MAX_LOG_SIZE + 1) as usize];
        f.write_all(&data).unwrap();
        drop(f);

        rotate_if_needed(&log);

        assert!(!log.exists());
        assert!(backup.exists());
        assert!(fs::metadata(&backup).unwrap().len() > MAX_LOG_SIZE);
    }

    #[test]
    fn rotate_if_needed_leaves_small_file() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("purple.log");

        fs::write(&log, "small content").unwrap();

        rotate_if_needed(&log);

        assert!(log.exists());
        assert!(!dir.path().join("purple.log.1").exists());
    }

    #[test]
    fn rotate_if_needed_handles_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("purple.log");

        // Should not panic
        rotate_if_needed(&log);
    }

    #[test]
    fn resolve_level_defaults_to_warn() {
        assert_eq!(resolve_level(false, None), LevelFilter::Warn);
    }

    #[test]
    fn resolve_level_verbose_returns_debug() {
        assert_eq!(resolve_level(true, None), LevelFilter::Debug);
    }

    #[test]
    fn resolve_level_env_overrides_verbose() {
        assert_eq!(resolve_level(false, Some("trace")), LevelFilter::Trace);
        assert_eq!(resolve_level(true, Some("error")), LevelFilter::Error);
    }

    #[test]
    fn resolve_level_ignores_unknown_env_value() {
        assert_eq!(resolve_level(false, Some("bogus")), LevelFilter::Warn);
        assert_eq!(resolve_level(true, Some("bogus")), LevelFilter::Debug);
    }

    #[test]
    fn epoch_days_to_date_unix_epoch() {
        // Day 0 = 1970-01-01
        assert_eq!(epoch_days_to_date(0), (1970, 1, 1));
    }

    #[test]
    fn epoch_days_to_date_known_date() {
        // 2026-04-10 = day 20553
        assert_eq!(epoch_days_to_date(20553), (2026, 4, 10));
    }

    #[test]
    fn epoch_days_to_date_leap_year() {
        // 2000-02-29 = day 11016
        assert_eq!(epoch_days_to_date(11016), (2000, 2, 29));
    }

    #[test]
    fn format_now_utc_returns_valid_timestamp() {
        let ts = format_now_utc();
        // Should be in YYYY-MM-DD HH:MM:SSZ format
        assert_eq!(ts.len(), 20);
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], " ");
        assert_eq!(&ts[13..14], ":");
        assert_eq!(&ts[16..17], ":");
        assert!(ts.ends_with('Z'));
    }

    #[test]
    fn log_path_ends_with_purple_log() {
        let paths = crate::runtime::env::Paths::new("/home/u");
        let path = log_path(Some(&paths)).expect("paths present");
        assert!(path.ends_with(".purple/purple.log"));
    }

    #[test]
    fn level_name_defaults_to_warn() {
        // With no PURPLE_LOG in the injected env, the default is "warn".
        let env = crate::runtime::env::Env::empty();
        let name = level_name(false, &env);
        assert_eq!(name, "warn");
    }

    #[test]
    fn write_banner_creates_output() {
        let info = BannerInfo {
            version: "0.0.0-test",
            config_path: "/tmp/config",
            providers: &["testprov".to_string()],
            askpass_sources: &["keychain:".to_string()],
            vault_ssh_info: Some("enabled (addr=https://vault:8200)"),
            ssh_version: "OpenSSH_9.0",
            term: "xterm-256color",
            colorterm: "truecolor",
            level: "warn",
            theme: "Purple",
            hosts: 42,
            patterns: 3,
            snippets: 7,
            proxy_env: "none",
        };

        // The banner lands in the state directory's log file and names the
        // resolved category directories, so a support bundle shows where
        // purple read and wrote.
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state").to_string_lossy().to_string();
        let paths = crate::runtime::env::Paths::resolve(dir.path(), move |k| {
            (k == "XDG_STATE_HOME").then(|| state.clone())
        });
        std::fs::create_dir_all(paths.state_dir()).unwrap();
        write_banner(&info, Some(&paths));

        let body = std::fs::read_to_string(paths.log_file()).unwrap();
        assert!(body.contains("--- purple v0.0.0-test started at "));
        assert!(body.contains("hosts=42 patterns=3 snippets=7"));
        assert!(body.contains("providers=testprov"));
        assert!(body.contains("vault_ssh=enabled (addr=https://vault:8200)"));
        assert!(
            body.contains(&format!(
                "dirs: config={} data={} state={} cache={}",
                paths.config_dir().display(),
                paths.data_dir().display(),
                paths.state_dir().display(),
                paths.cache_dir().display()
            )),
            "{body}"
        );
        assert!(body.contains("log_level=warn"));

        let ts = format_now_utc();
        assert!(ts.ends_with('Z'));
        assert_eq!(ts.len(), 20);
    }
}
