use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use crate::ssh_context::{OwnedSshContext, SshContext};

use ratatui::widgets::ListState;

/// Sort mode for file browser panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserSort {
    Name,
    Date,
    DateAsc,
}

/// A file or directory entry in the browser.
#[derive(Debug, Clone, PartialEq)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    /// Modification time as Unix timestamp (seconds since epoch).
    pub modified: Option<i64>,
}

/// Which pane is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserPane {
    Local,
    Remote,
}

/// Pending copy operation awaiting confirmation.
pub struct CopyRequest {
    pub sources: Vec<String>,
    pub source_pane: BrowserPane,
    pub has_dirs: bool,
}

/// State for the dual-pane file browser overlay.
pub struct FileBrowserState {
    pub alias: String,
    pub askpass: Option<String>,
    pub active_pane: BrowserPane,
    // Local
    pub local_path: PathBuf,
    pub local_entries: Vec<FileEntry>,
    pub local_list_state: ListState,
    pub local_selected: HashSet<String>,
    pub local_error: Option<String>,
    // Remote
    pub remote_path: String,
    pub remote_entries: Vec<FileEntry>,
    pub remote_list_state: ListState,
    pub remote_selected: HashSet<String>,
    pub remote_error: Option<String>,
    pub remote_loading: bool,
    // Options
    pub show_hidden: bool,
    pub sort: BrowserSort,
    // Copy confirmation
    pub confirm_copy: Option<CopyRequest>,
    // Transfer in progress
    pub transferring: Option<String>,
    // Transfer error (shown as dismissible dialog)
    pub transfer_error: Option<String>,
    // Whether the initial remote connection has been recorded in history
    pub connection_recorded: bool,
}

/// List local directory entries.
/// Sorts: directories first, then by name or date. Filters dotfiles based on show_hidden.
pub fn list_local(
    path: &Path,
    show_hidden: bool,
    sort: BrowserSort,
) -> anyhow::Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let metadata = entry.metadata()?;
        let is_dir = metadata.is_dir();
        let size = if is_dir { None } else { Some(metadata.len()) };
        let modified = metadata.modified().ok().and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs() as i64)
        });
        entries.push(FileEntry {
            name,
            is_dir,
            size,
            modified,
        });
    }
    sort_entries(&mut entries, sort);
    Ok(entries)
}

/// Sort file entries: directories first, then by the chosen mode.
pub fn sort_entries(entries: &mut [FileEntry], sort: BrowserSort) {
    match sort {
        BrowserSort::Name => {
            entries.sort_by(|a, b| {
                b.is_dir.cmp(&a.is_dir).then_with(|| {
                    a.name
                        .to_ascii_lowercase()
                        .cmp(&b.name.to_ascii_lowercase())
                })
            });
        }
        BrowserSort::Date => {
            entries.sort_by(|a, b| {
                b.is_dir.cmp(&a.is_dir).then_with(|| {
                    // Newest first: reverse order
                    b.modified.unwrap_or(0).cmp(&a.modified.unwrap_or(0))
                })
            });
        }
        BrowserSort::DateAsc => {
            entries.sort_by(|a, b| {
                b.is_dir.cmp(&a.is_dir).then_with(|| {
                    // Oldest first; unknown dates sort to the end
                    a.modified
                        .unwrap_or(i64::MAX)
                        .cmp(&b.modified.unwrap_or(i64::MAX))
                })
            });
        }
    }
}

/// Parse `ls -lhAL` output into FileEntry list.
/// With -L, symlinks are dereferenced so their target type is shown directly.
/// Recognizes directories via 'd' permission prefix. Skips the "total" line.
/// Broken symlinks are omitted by ls -L (they cannot be transferred anyway).
pub fn parse_ls_output(output: &str, show_hidden: bool, sort: BrowserSort) -> Vec<FileEntry> {
    let mut entries = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("total ") {
            continue;
        }
        // ls -l format: permissions links owner group size month day time name
        // Split on whitespace runs, taking 9 fields (last gets the rest including spaces)
        let mut parts: Vec<&str> = Vec::with_capacity(9);
        let mut rest = line;
        for _ in 0..8 {
            rest = rest.trim_start();
            if rest.is_empty() {
                break;
            }
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            parts.push(&rest[..end]);
            rest = &rest[end..];
        }
        rest = rest.trim_start();
        if !rest.is_empty() {
            parts.push(rest);
        }
        if parts.len() < 9 {
            continue;
        }
        let permissions = parts[0];
        let is_dir = permissions.starts_with('d');
        let name = parts[8];
        // Skip empty names
        if name.is_empty() {
            continue;
        }
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        // Parse human-readable size (e.g. "1.1K", "4.0M", "512")
        let size = if is_dir {
            None
        } else {
            Some(parse_human_size(parts[4]))
        };
        // Parse date from month/day/time-or-year (parts[5..=7])
        let modified = parse_ls_date(parts[5], parts[6], parts[7]);
        entries.push(FileEntry {
            name: name.to_string(),
            is_dir,
            size,
            modified,
        });
    }
    sort_entries(&mut entries, sort);
    entries
}

/// Parse a human-readable size string like "1.1K", "4.0M", "512" into bytes.
fn parse_human_size(s: &str) -> u64 {
    let s = s.trim();
    if s.is_empty() {
        return 0;
    }
    let last = s.as_bytes()[s.len() - 1];
    let multiplier = match last {
        b'K' => 1024,
        b'M' => 1024 * 1024,
        b'G' => 1024 * 1024 * 1024,
        b'T' => 1024u64 * 1024 * 1024 * 1024,
        _ => 1,
    };
    let num_str = if multiplier > 1 { &s[..s.len() - 1] } else { s };
    let num: f64 = num_str.parse().unwrap_or(0.0);
    (num * multiplier as f64) as u64
}

/// Parse the date fields from `ls -l` with `LC_ALL=C`.
/// Recent files: "Jan 1 12:34" (month day HH:MM).
/// Old files: "Jan 1 2024" (month day year).
/// Returns approximate Unix timestamp or None if unparseable.
fn parse_ls_date(month_str: &str, day_str: &str, time_or_year: &str) -> Option<i64> {
    let month = match month_str {
        "Jan" => 0,
        "Feb" => 1,
        "Mar" => 2,
        "Apr" => 3,
        "May" => 4,
        "Jun" => 5,
        "Jul" => 6,
        "Aug" => 7,
        "Sep" => 8,
        "Oct" => 9,
        "Nov" => 10,
        "Dec" => 11,
        _ => return None,
    };
    let day: i64 = day_str.parse().ok()?;
    if !(1..=31).contains(&day) {
        return None;
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let now_year = epoch_to_year(now);

    if time_or_year.contains(':') {
        // Recent format: "HH:MM"
        let mut parts = time_or_year.splitn(2, ':');
        let hour: i64 = parts.next()?.parse().ok()?;
        let min: i64 = parts.next()?.parse().ok()?;
        // Determine year: if month/day is in the future, it's last year
        let mut year = now_year;
        let approx = approximate_epoch(year, month, day, hour, min);
        if approx > now + 86400 {
            year -= 1;
        }
        Some(approximate_epoch(year, month, day, hour, min))
    } else {
        // Old format: "2024" (year)
        let year: i64 = time_or_year.parse().ok()?;
        if !(1970..=2100).contains(&year) {
            return None;
        }
        Some(approximate_epoch(year, month, day, 0, 0))
    }
}

/// Rough Unix timestamp from date components (no leap second precision needed).
fn approximate_epoch(year: i64, month: i64, day: i64, hour: i64, min: i64) -> i64 {
    // Days from 1970-01-01 to start of year
    let y = year - 1970;
    let mut days = y * 365 + (y + 1) / 4; // approximate leap years
    // Days to start of month (non-leap approximation, close enough for sorting)
    let month_days = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    days += month_days[month as usize];
    // Add leap day if applicable
    if month > 1 && year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
        days += 1;
    }
    days += day - 1;
    days * 86400 + hour * 3600 + min * 60
}

/// Convert epoch seconds to a year (correctly handles year boundaries).
fn epoch_to_year(ts: i64) -> i64 {
    let mut y = 1970 + ts / 31_557_600;
    if approximate_epoch(y, 0, 1, 0, 0) > ts {
        y -= 1;
    } else if approximate_epoch(y + 1, 0, 1, 0, 0) <= ts {
        y += 1;
    }
    y
}

fn is_leap_year(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// Format a Unix timestamp as a relative or short date string.
/// Returns strings like "2m ago", "3h ago", "5d ago", "Jan 15", "Mar 2024".
pub fn format_relative_time(ts: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let diff = now - ts;
    if diff < 0 {
        // Future timestamp (clock skew), just show date
        return format_short_date(ts);
    }
    if diff < 60 {
        return "just now".to_string();
    }
    if diff < 3600 {
        return format!("{}m ago", diff / 60);
    }
    if diff < 86400 {
        return format!("{}h ago", diff / 3600);
    }
    if diff < 86400 * 30 {
        return format!("{}d ago", diff / 86400);
    }
    format_short_date(ts)
}

/// Format a timestamp as "Mon DD" (same year) or "Mon YYYY" (different year).
fn format_short_date(ts: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let now_year = epoch_to_year(now);
    let ts_year = epoch_to_year(ts);

    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    // Approximate month and day from day-of-year
    let year_start = approximate_epoch(ts_year, 0, 1, 0, 0);
    let day_of_year = ((ts - year_start) / 86400).max(0) as usize;
    let feb = if is_leap_year(ts_year) { 29 } else { 28 };
    let month_lengths = [31, feb, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0;
    let mut remaining = day_of_year;
    for (i, &len) in month_lengths.iter().enumerate() {
        if remaining < len {
            m = i;
            break;
        }
        remaining -= len;
        m = i + 1;
    }
    let m = m.min(11);
    let d = remaining + 1;

    if ts_year == now_year {
        format!("{} {:>2}", months[m], d)
    } else {
        format!("{} {}", months[m], ts_year)
    }
}

/// Shell-escape a path with single quotes.
fn shell_escape(path: &str) -> String {
    crate::snippet::shell_escape(path)
}

/// Get the remote home directory via `pwd`.
pub fn get_remote_home(
    alias: &str,
    config_path: &Path,
    askpass: Option<&str>,
    bw_session: Option<&str>,
    has_active_tunnel: bool,
) -> anyhow::Result<String> {
    let result = crate::snippet::run_snippet(
        alias,
        config_path,
        "pwd",
        askpass,
        bw_session,
        true,
        has_active_tunnel,
    )?;
    if result.status.success() {
        Ok(result.stdout.trim().to_string())
    } else {
        let msg = filter_ssh_warnings(result.stderr.trim());
        if msg.is_empty() {
            anyhow::bail!("Failed to connect.")
        } else {
            anyhow::bail!("{}", msg)
        }
    }
}

/// Fetch remote directory listing synchronously (used by spawn_remote_listing).
pub fn fetch_remote_listing(
    ctx: &SshContext<'_>,
    remote_path: &str,
    show_hidden: bool,
    sort: BrowserSort,
) -> Result<Vec<FileEntry>, String> {
    let command = format!("LC_ALL=C ls -lhAL {}", shell_escape(remote_path));
    let result = crate::snippet::run_snippet(
        ctx.alias,
        ctx.config_path,
        &command,
        ctx.askpass,
        ctx.bw_session,
        true,
        ctx.has_tunnel,
    );
    match result {
        Ok(r) if r.status.success() => Ok(parse_ls_output(&r.stdout, show_hidden, sort)),
        Ok(r) => {
            let msg = filter_ssh_warnings(r.stderr.trim());
            if msg.is_empty() {
                Err(format!(
                    "ls exited with code {}.",
                    r.status.code().unwrap_or(1)
                ))
            } else {
                Err(msg)
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Spawn background thread for remote directory listing.
/// Sends result back via the provided sender function.
pub fn spawn_remote_listing<F>(
    ctx: OwnedSshContext,
    remote_path: String,
    show_hidden: bool,
    sort: BrowserSort,
    send: F,
) where
    F: FnOnce(String, String, Result<Vec<FileEntry>, String>) + Send + 'static,
{
    std::thread::spawn(move || {
        let borrowed = SshContext {
            alias: &ctx.alias,
            config_path: &ctx.config_path,
            askpass: ctx.askpass.as_deref(),
            bw_session: ctx.bw_session.as_deref(),
            has_tunnel: ctx.has_tunnel,
        };
        let listing = fetch_remote_listing(&borrowed, &remote_path, show_hidden, sort);
        send(ctx.alias, remote_path, listing);
    });
}

/// Result of an scp transfer.
pub struct ScpResult {
    pub status: ExitStatus,
    pub stderr_output: String,
}

/// Run scp in the background with captured stderr for error reporting.
/// Stderr is piped and captured so errors can be extracted. Progress percentage
/// is not available because scp only outputs progress to a TTY, not to a pipe.
/// Stdin is null (askpass handles authentication). Stdout is null (scp has no
/// meaningful stdout output).
pub fn run_scp(
    alias: &str,
    config_path: &Path,
    askpass: Option<&str>,
    bw_session: Option<&str>,
    has_active_tunnel: bool,
    scp_args: &[String],
) -> anyhow::Result<ScpResult> {
    let mut cmd = Command::new("scp");
    cmd.arg("-F").arg(config_path);

    if has_active_tunnel {
        cmd.arg("-o").arg("ClearAllForwardings=yes");
    }

    for arg in scp_args {
        cmd.arg(arg);
    }

    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    if askpass.is_some() {
        crate::askpass_env::configure_ssh_command(&mut cmd, alias, config_path);
    }

    if let Some(token) = bw_session {
        cmd.env("BW_SESSION", token);
    }

    let output = cmd
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run scp: {}", e))?;

    let stderr_output = String::from_utf8_lossy(&output.stderr).to_string();

    Ok(ScpResult {
        status: output.status,
        stderr_output,
    })
}

/// Filter SSH warning noise from stderr, keeping only actionable error lines.
/// Strips lines like "** WARNING: connection is not using a post-quantum key exchange".
pub fn filter_ssh_warnings(stderr: &str) -> String {
    stderr
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty()
                && !trimmed.starts_with("** ")
                && !trimmed.starts_with("Warning:")
                && !trimmed.contains("see https://")
                && !trimmed.contains("See https://")
                && !trimmed.starts_with("The server may need")
                && !trimmed.starts_with("This session may be")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build scp arguments for a file transfer.
/// Returns the args to pass after `scp -F <config>`.
///
/// Remote paths are NOT shell-escaped because scp is invoked via Command::arg()
/// which bypasses the shell entirely. The colon in `alias:path` is the only
/// special character scp interprets. Paths with spaces, globbing chars etc. are
/// passed through literally by the OS exec layer.
pub fn build_scp_args(
    alias: &str,
    source_pane: BrowserPane,
    local_path: &Path,
    remote_path: &str,
    filenames: &[String],
    has_dirs: bool,
) -> Vec<String> {
    let mut args = Vec::new();
    if has_dirs {
        args.push("-r".to_string());
    }
    args.push("--".to_string());

    match source_pane {
        // Upload: local files -> remote
        BrowserPane::Local => {
            for name in filenames {
                args.push(local_path.join(name).to_string_lossy().to_string());
            }
            let dest = format!("{}:{}", alias, remote_path);
            args.push(dest);
        }
        // Download: remote files -> local
        BrowserPane::Remote => {
            let base = remote_path.trim_end_matches('/');
            for name in filenames {
                let rpath = format!("{}/{}", base, name);
                args.push(format!("{}:{}", alias, rpath));
            }
            args.push(local_path.to_string_lossy().to_string());
        }
    }
    args
}

/// Format a file size in human-readable form.
pub fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
#[path = "file_browser_tests.rs"]
mod tests;
