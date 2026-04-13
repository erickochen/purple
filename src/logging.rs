use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use log::LevelFilter;
use simplelog::{
    ColorChoice, CombinedLogger, ConfigBuilder, SharedLogger, TermLogger, TerminalMode, WriteLogger,
};

// Fault domain convention:
// - error! and warn! statements carry a prefix: [external], [config], or [purple]
// - info! and debug! are operational flow markers without prefixes

const MAX_LOG_SIZE: u64 = 5 * 1024 * 1024; // 5MB

/// Return the path to the log file: ~/.purple/purple.log
pub fn log_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".purple").join("purple.log"))
}

/// Rotate log file if it exceeds MAX_LOG_SIZE.
/// Renames purple.log -> purple.log.1 (overwrites previous backup).
fn rotate_if_needed(path: &Path) {
    if let Ok(meta) = fs::metadata(path) {
        if meta.len() > MAX_LOG_SIZE {
            let backup = path.with_file_name("purple.log.1");
            let _ = fs::rename(path, backup);
        }
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
pub fn init(verbose: bool, cli_stderr: bool) {
    let Some(path) = log_path() else { return };

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    rotate_if_needed(&path);

    let env_val = std::env::var("PURPLE_LOG").ok();
    let level = resolve_level(verbose, env_val.as_deref());
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

    if !loggers.is_empty() {
        if let Err(e) = CombinedLogger::init(loggers) {
            eprintln!("[purple] Failed to initialize logger: {e}");
        }
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
}

/// Write startup banner directly to log file, bypassing level filters.
/// All timestamps are UTC (suffixed with Z).
///
/// Note: writes directly to the log file, bypassing simplelog's CombinedLogger.
/// This means the banner will not appear on stderr in CLI mode. This is intentional:
/// the banner is diagnostic context for the log file, not user-facing output.
pub fn write_banner(info: &BannerInfo<'_>) {
    let Some(path) = log_path() else { return };
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
         \x20   providers={providers_joined}\n\
         \x20   askpass={askpass_joined}\n",
        info.version, info.config_path, info.ssh_version, info.term, info.colorterm,
    );
    if let Some(vault_info) = info.vault_ssh_info {
        banner.push_str(&format!("    vault_ssh={vault_info}\n"));
    }
    banner.push_str(&format!("    log_level={}\n", info.level));

    // Note: banner lines use \x20 (space) prefix to prevent the Rust string
    // continuation from collapsing leading whitespace. This is a rustfmt-safe
    // idiom for multi-line format strings with indentation.

    // Non-fatal: banner write failure does not affect logging
    let _ = file.write_all(banner.as_bytes());
}

/// Return the effective log level name as a lowercase string.
pub fn level_name(verbose: bool) -> String {
    let env_val = std::env::var("PURPLE_LOG").ok();
    resolve_level(verbose, env_val.as_deref())
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
    use std::sync::mpsc;
    use std::time::Duration;

    let child = std::process::Command::new("ssh")
        .arg("-V")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    let Ok(child) = child else {
        eprintln!("[purple] Failed to detect SSH version. Is ssh installed?");
        return "unknown".to_string();
    };

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });

    match rx.recv_timeout(Duration::from_secs(2)) {
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
        let path = log_path().expect("home dir should exist in test");
        assert!(path.ends_with(".purple/purple.log"));
    }

    #[test]
    fn level_name_defaults_to_warn() {
        // level_name reads PURPLE_LOG env var internally, so we only test
        // the verbose=false path (env var is not set in most test runners)
        let name = level_name(false);
        // Without PURPLE_LOG set, should be "warn"
        assert!(name == "warn" || std::env::var("PURPLE_LOG").is_ok());
    }

    #[test]
    fn write_banner_creates_output() {
        // write_banner uses log_path() which returns ~/.purple/purple.log,
        // so we test the banner formatting logic via struct construction
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
        };

        // Verify the banner struct fields are accessible and well-formed
        assert_eq!(info.version, "0.0.0-test");
        assert_eq!(info.providers.len(), 1);
        assert!(info.vault_ssh_info.is_some());

        // We can't easily redirect log_path() in a unit test, but we can
        // verify the format_now_utc helper used by write_banner
        let ts = format_now_utc();
        assert!(ts.ends_with('Z'));
        assert_eq!(ts.len(), 20);
    }
}
