use std::io::Read;
use std::path::Path;
use std::sync::mpsc;

use anyhow::{Context, Result};
use log::{debug, info, warn};

use crate::event::AppEvent;

/// Current compiled-in version from Cargo.toml.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Max bytes kept from a release-notes headline before caching.
/// Bounds attacker-controlled input written to ~/.purple/last_version_check.
const HEADLINE_MAX_BYTES: usize = 200;

/// Extract a one-line headline from release notes for the TUI update badge.
/// Takes the first non-empty content line, strips leading `- ` bullet marker,
/// and truncates to `HEADLINE_MAX_BYTES` on a char boundary.
fn extract_headline(notes: &str) -> Option<String> {
    let line = notes
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty() && !l.starts_with('#'))?;
    let trimmed = line.strip_prefix("- ").unwrap_or(line);
    if trimmed.len() <= HEADLINE_MAX_BYTES {
        return Some(trimmed.to_string());
    }
    let mut cut = HEADLINE_MAX_BYTES;
    while cut > 0 && !trimmed.is_char_boundary(cut) {
        cut -= 1;
    }
    Some(trimmed[..cut].to_string())
}

/// Parse a semver string "X.Y.Z" into a tuple.
fn parse_version(v: &str) -> Option<(u32, u32, u32)> {
    let mut parts = v.splitn(3, '.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// Returns true if `latest` is strictly newer than `current`.
fn is_newer(current: &str, latest: &str) -> bool {
    match (parse_version(current), parse_version(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

/// Release info extracted from GitHub API response.
struct ReleaseInfo {
    version: String,
    /// Release notes body (markdown). May be empty.
    notes: String,
}

/// Extract version string and release notes from GitHub release JSON.
fn extract_release_info(json: &serde_json::Value) -> Result<ReleaseInfo> {
    let tag = json["tag_name"]
        .as_str()
        .context("Missing tag_name in release")?;

    let version = tag.strip_prefix('v').unwrap_or(tag);

    if parse_version(version).is_none() {
        anyhow::bail!("Invalid version format: {}", version);
    }

    let notes = json["body"].as_str().unwrap_or("").to_string();

    Ok(ReleaseInfo {
        version: version.to_string(),
        notes,
    })
}

/// Fetch the latest release info from GitHub.
fn check_latest_release(agent: &ureq::Agent) -> Result<ReleaseInfo> {
    let mut resp = agent
        .get("https://api.github.com/repos/erickochen/purple/releases/latest")
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", &format!("purple-ssh/{}", current_version()))
        .call()
        .context("Failed to fetch latest release. GitHub may be rate-limited.")?;

    let mut body = Vec::new();
    resp.body_mut()
        .as_reader()
        .take(1_048_576) // 1 MB limit for API response
        .read_to_end(&mut body)
        .context("Failed to read release JSON")?;

    let json: serde_json::Value =
        serde_json::from_slice(&body).context("Failed to parse release JSON")?;

    extract_release_info(&json)
}

/// TTL for version check cache (1 hour).
const VERSION_CHECK_TTL: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// Cached version info: version string and optional headline.
#[derive(Debug, PartialEq)]
struct CachedVersion {
    version: String,
    headline: Option<String>,
}

/// Parse cache file content and determine if a newer version is available.
/// Cache format: `timestamp\nversion\nheadline\n` (headline may be empty).
/// Returns `Some(Some(cached))` if cache is fresh and a newer version exists,
/// `Some(None)` if cache is fresh and we are up-to-date,
/// `None` if cache content is corrupt, expired or unparseable.
fn parse_version_cache(
    content: &str,
    now_secs: u64,
    current: &str,
) -> Option<Option<CachedVersion>> {
    let mut lines = content.lines();
    let timestamp: u64 = lines.next()?.parse().ok()?;
    let version = lines.next()?.to_string();
    let headline = lines
        .next()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());

    if version.is_empty() || parse_version(&version).is_none() {
        return None; // Corrupt version string
    }

    if now_secs.saturating_sub(timestamp) > VERSION_CHECK_TTL.as_secs() {
        return None; // Cache expired
    }

    if is_newer(current, &version) {
        Some(Some(CachedVersion { version, headline }))
    } else {
        Some(None) // Up-to-date, no API call needed
    }
}

/// Read cached version check result from ~/.purple/last_version_check.
/// Returns `Some(Some(cached))` if cache is fresh and a newer version exists,
/// `Some(None)` if cache is fresh and we are up-to-date,
/// `None` if cache is missing, corrupt or expired.
fn read_cached_version(
    paths: Option<&crate::runtime::env::Paths>,
) -> Option<Option<CachedVersion>> {
    let path = paths?.last_version_check();
    let content = std::fs::read_to_string(&path).ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    parse_version_cache(&content, now, current_version())
}

/// Write version check result to ~/.purple/last_version_check.
fn write_version_cache(
    version: &str,
    headline: Option<&str>,
    paths: Option<&crate::runtime::env::Paths>,
) {
    let Some(paths) = paths else {
        return;
    };
    let dir = paths.cache_dir();
    if let Err(e) = std::fs::create_dir_all(dir) {
        debug!("[config] Failed to create version cache directory: {e}");
        return;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let hl = headline.unwrap_or("");
    let content = format!("{}\n{}\n{}\n", now, version, hl);
    if let Err(e) = crate::fs_util::atomic_write(&paths.last_version_check(), content.as_bytes()) {
        debug!("[config] Failed to write version cache: {e}");
    }
}

/// Spawn a background thread to check for updates. Sends an event if a newer version exists.
/// Uses a local cache (~/.purple/last_version_check) with a 1h TTL to avoid unnecessary
/// GitHub API calls on frequent startup. Silently does nothing on any error.
pub fn spawn_version_check(
    tx: mpsc::Sender<AppEvent>,
    env: std::sync::Arc<crate::runtime::env::Env>,
) {
    let _ = std::thread::Builder::new()
        .name("version-check".to_string())
        .spawn(move || {
            debug!("[external] Version check started");
            // Check cache first — skip API call if fresh result exists
            match read_cached_version(env.paths()) {
                Some(Some(cached)) => {
                    debug!(
                        "[external] Version check: current={} latest={}",
                        current_version(),
                        cached.version
                    );
                    let _ = tx.send(AppEvent::UpdateAvailable {
                        version: cached.version,
                        headline: cached.headline,
                    });
                    return;
                }
                Some(None) => return, // Up-to-date, cache still fresh
                None => {}            // Cache missing or expired, fetch
            }

            // Short timeout: fire-and-forget background check,
            // don't tie up thread resources for 30s like the provider agent
            let agent = ureq::Agent::config_builder()
                .timeout_global(Some(std::time::Duration::from_secs(5)))
                .build()
                .new_agent();

            match check_latest_release(&agent) {
                Ok(info) => {
                    let current = current_version();
                    debug!(
                        "[external] Version check: current={current} latest={}",
                        info.version
                    );
                    let headline = extract_headline(&info.notes);
                    write_version_cache(&info.version, headline.as_deref(), env.paths());
                    if is_newer(current, &info.version) {
                        let _ = tx.send(AppEvent::UpdateAvailable {
                            version: info.version,
                            headline,
                        });
                    }
                }
                Err(err) => {
                    warn!("[external] Version check failed: {err}");
                }
            }
        });
}

/// Format text as bold, respecting NO_COLOR (resolved from the injected env).
fn bold(text: &str, no_color: bool) -> String {
    if no_color {
        text.to_string()
    } else {
        format!("\x1b[1m{}\x1b[0m", text)
    }
}

/// Format text as bold purple, respecting NO_COLOR (resolved from the env).
fn bold_purple(text: &str, no_color: bool) -> String {
    if no_color {
        text.to_string()
    } else {
        format!("\x1b[1;35m{}\x1b[0m", text)
    }
}

/// Install method detected from binary path.
enum InstallMethod {
    Homebrew,
    Cargo,
    CurlOrManual,
}

/// Check if exe_path is under a Homebrew Cellar directory.
/// Validates that the Cellar path ends with a "Cellar" component and
/// that the binary sits in the expected `.../Cellar/<formula>/.../` structure.
fn is_homebrew_path(exe_path: &Path, cellar: &Path) -> bool {
    // Cellar dir must end with "Cellar" component
    if cellar.file_name().and_then(|n| n.to_str()) != Some("Cellar") {
        return false;
    }
    // Path::starts_with is component-aware: /usr/local won't match /usr/local-bin
    if !exe_path.starts_with(cellar) {
        return false;
    }
    // Must have at least one component after Cellar (the formula name)
    exe_path
        .strip_prefix(cellar)
        .is_ok_and(|rest| rest.components().count() >= 1)
}

/// Check if exe_path's parent is exactly <cargo_home>/bin.
fn is_cargo_path(exe_path: &Path, cargo_home: &Path) -> bool {
    let cargo_bin = cargo_home.join("bin");
    exe_path.parent() == Some(cargo_bin.as_path())
}

/// Detect how purple was installed by checking the binary path against
/// known package manager directories. Uses Path::starts_with for
/// component-aware comparison (prevents /usr/local matching /usr/local-bin).
/// Env vars (HOMEBREW_CELLAR, HOMEBREW_PREFIX, CARGO_HOME) are treated
/// as hints and validated structurally before trusting. Falls back to
/// well-known default paths. Fails open to CurlOrManual when uncertain.
fn detect_install_method(exe_path: &Path, env: &crate::runtime::env::Env) -> InstallMethod {
    // Homebrew: check HOMEBREW_CELLAR env var first (most specific),
    // then derive Cellar from HOMEBREW_PREFIX, then fall back to
    // well-known default Cellar locations
    if let Some(cellar) = env.var("HOMEBREW_CELLAR")
        && is_homebrew_path(exe_path, Path::new(cellar))
    {
        return InstallMethod::Homebrew;
    }
    if let Some(prefix) = env.var("HOMEBREW_PREFIX") {
        let cellar = std::path::PathBuf::from(prefix).join("Cellar");
        if is_homebrew_path(exe_path, &cellar) {
            return InstallMethod::Homebrew;
        }
    }
    // Default Cellar locations (Apple Silicon + Intel + Linuxbrew)
    for cellar in [
        "/opt/homebrew/Cellar",
        "/usr/local/Cellar",
        "/home/linuxbrew/.linuxbrew/Cellar",
    ] {
        if is_homebrew_path(exe_path, Path::new(cellar)) {
            return InstallMethod::Homebrew;
        }
    }

    // Cargo: check CARGO_HOME env var first, then check if parent
    // is a "bin" dir inside a ".cargo" dir (component-aware fallback)
    if let Some(cargo_home) = env.var("CARGO_HOME")
        && is_cargo_path(exe_path, Path::new(cargo_home))
    {
        return InstallMethod::Cargo;
    }
    if let Some(parent) = exe_path.parent()
        && parent.file_name().and_then(|n| n.to_str()) == Some("bin")
        && let Some(grandparent) = parent.parent()
        && grandparent.file_name().and_then(|n| n.to_str()) == Some(".cargo")
    {
        return InstallMethod::Cargo;
    }

    InstallMethod::CurlOrManual
}

/// Detect the update command appropriate for how purple was installed.
pub fn update_hint(env: &crate::runtime::env::Env) -> &'static str {
    if !matches!(std::env::consts::OS, "macos" | "linux") {
        return "cargo install purple-ssh";
    }
    if let Ok(exe) = std::env::current_exe() {
        let path = std::fs::canonicalize(&exe).unwrap_or(exe);
        return match detect_install_method(&path, env) {
            InstallMethod::Homebrew => "brew upgrade erickochen/purple/purple",
            InstallMethod::Cargo => "cargo install purple-ssh",
            InstallMethod::CurlOrManual => "purple update",
        };
    }
    "purple update"
}

/// Self-update the purple binary to the latest release.
pub fn self_update(env: &crate::runtime::env::Env) -> Result<()> {
    // macOS and Linux only
    if !matches!(std::env::consts::OS, "macos" | "linux") {
        anyhow::bail!(
            "Self-update is available on macOS and Linux only.\n  \
             Update via: cargo install purple-ssh"
        );
    }

    let no_color = env.no_color();
    println!(
        "{}",
        crate::messages::update::header(&bold("purple.", no_color))
    );

    // Resolve current binary path
    let exe_path = std::env::current_exe().context("Failed to detect binary path")?;
    let exe_path = std::fs::canonicalize(&exe_path).unwrap_or(exe_path);
    println!("{}", crate::messages::update::binary_path(&exe_path));

    // Detect package manager installations
    match detect_install_method(&exe_path, env) {
        InstallMethod::Homebrew => {
            anyhow::bail!(
                "purple appears to be installed via Homebrew.\n  \
                 Update with: brew upgrade erickochen/purple/purple"
            );
        }
        InstallMethod::Cargo => {
            anyhow::bail!(
                "purple appears to be installed via cargo.\n  \
                 Update with: cargo install purple-ssh"
            );
        }
        InstallMethod::CurlOrManual => {}
    }

    // Fetch latest version (needs redirects for GitHub release asset downloads)
    print!(
        "{}",
        crate::i18n::text(crate::messages::update::STEP_CHECKING)
    );
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .build()
        .new_agent();
    let info = check_latest_release(&agent)?;
    let latest = info.version;
    let current = current_version();

    if !is_newer(current, &latest) {
        println!("{}", crate::messages::update::already_on(current));
        return Ok(());
    }

    println!("{}", crate::messages::update::available(&latest, current));
    info!("[purple] Update started: {current} -> {latest}");

    // Detect target
    let target = match (std::env::consts::ARCH, std::env::consts::OS) {
        ("aarch64", "macos") => "aarch64-apple-darwin",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        ("aarch64", "linux") => "aarch64-unknown-linux-gnu",
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
        (arch, os) => anyhow::bail!("Unsupported platform: {}-{}", arch, os),
    };

    // Check we can write to the binary location
    let parent = exe_path
        .parent()
        .context("Binary has no parent directory")?;

    // Warn when running via sudo — creates root-owned cache files
    if env.var("SUDO_USER").is_some() {
        eprintln!(
            "{}",
            crate::messages::update::sudo_warning_line(&bold("!", no_color))
        );
    }

    if !is_writable(parent) {
        anyhow::bail!(
            "No write permission to {}.\n  Check directory permissions or run with elevated privileges.",
            parent.display()
        );
    }

    // Clean up stale staged binaries from interrupted previous updates
    clean_stale_staged(parent);

    // Set up temp directory (create_dir fails if path exists, preventing symlink attacks)
    let tmp_dir = std::env::temp_dir().join(format!(
        "purple_update_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir(&tmp_dir).context("Failed to create temp directory")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_dir, std::fs::Permissions::from_mode(0o700))
            .context("Failed to set temp directory permissions")?;
    }

    // Ensure cleanup on any exit path
    let _cleanup = TempCleanup(&tmp_dir);

    let tarball_name = format!("purple-{}-{}.tar.gz", latest, target);
    let base_url = format!(
        "https://github.com/erickochen/purple/releases/download/v{}",
        latest
    );

    // Download tarball
    print!("{}", crate::messages::update::step_downloading(&latest));
    let tarball_path = tmp_dir.join(&tarball_name);
    download_file(
        &agent,
        &format!("{}/{}", base_url, tarball_name),
        &tarball_path,
    )?;

    // Download checksum
    let sha_path = tmp_dir.join(format!("{}.sha256", tarball_name));
    download_file(
        &agent,
        &format!("{}/{}.sha256", base_url, tarball_name),
        &sha_path,
    )?;
    println!("{}", crate::i18n::text(crate::messages::update::DONE));

    // Verify checksum
    print!(
        "{}",
        crate::i18n::text(crate::messages::update::STEP_VERIFYING_CHECKSUM)
    );
    verify_checksum(&tarball_path, &sha_path)?;
    println!(
        "{}",
        crate::i18n::text(crate::messages::update::CHECKSUM_OK)
    );

    // Extract
    print!(
        "{}",
        crate::i18n::text(crate::messages::update::STEP_INSTALLING)
    );
    let status = std::process::Command::new("tar")
        .arg("-xzf")
        .arg(&tarball_path)
        .arg("-C")
        .arg(&tmp_dir)
        .status()
        .context("Failed to run tar")?;
    if !status.success() {
        anyhow::bail!("tar extraction failed");
    }

    let new_binary = tmp_dir.join("purple");
    if !new_binary.exists() {
        anyhow::bail!("Binary not found in archive");
    }

    // Atomic replacement: stage new binary in the same directory via O_EXCL
    // (prevents symlink attacks), then rename over the target (atomic within
    // the same filesystem)
    let staged_path = parent.join(format!(".purple_new_{}", std::process::id()));
    {
        use std::io::Write;
        let source = std::fs::read(&new_binary).context("Failed to read new binary")?;
        let mut dest = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true) // O_EXCL: fails if path exists (prevents symlink following)
            .open(&staged_path)
            .context("Failed to create staged binary")?;
        dest.write_all(&source)
            .context("Failed to write staged binary")?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged_path, std::fs::Permissions::from_mode(0o755))
            .context("Failed to set permissions")?;
    }

    if let Err(e) = std::fs::rename(&staged_path, &exe_path) {
        // Clean up staged file on failure
        let _ = std::fs::remove_file(&staged_path);
        return Err(e).context("Failed to replace binary");
    }

    println!("{}", crate::i18n::text(crate::messages::update::DONE));
    info!("[purple] Update completed: {latest}");
    println!(
        "{}",
        crate::messages::update::installed_at(
            &bold_purple(&format!("purple v{}", latest), no_color),
            &exe_path,
        )
    );

    println!("{}", crate::messages::update::whats_new_hint_indented());
    println!();

    Ok(())
}

/// Download a file from a URL.
fn download_file(agent: &ureq::Agent, url: &str, dest: &Path) -> Result<()> {
    let mut resp = agent
        .get(url)
        .call()
        .with_context(|| format!("Failed to download {}", url))?;

    let mut bytes = Vec::new();
    resp.body_mut()
        .as_reader()
        .take(100 * 1024 * 1024) // 100 MB limit
        .read_to_end(&mut bytes)
        .context("Failed to read download")?;

    if bytes.is_empty() {
        anyhow::bail!("Empty response from {}", url);
    }

    crate::fs_util::atomic_write(dest, &bytes).context("Failed to write file")?;
    Ok(())
}

/// Verify SHA256 checksum of a file using the sha2 crate (no external tools).
fn verify_checksum(file: &Path, sha_file: &Path) -> Result<()> {
    let expected = std::fs::read_to_string(sha_file).context("Failed to read checksum file")?;
    let expected = expected
        .split_whitespace()
        .next()
        .context("Empty checksum file")?;

    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(file).context("Failed to read file for checksum")?;
    let actual = format!("{:x}", Sha256::digest(&bytes));

    if expected != actual {
        anyhow::bail!(
            "Checksum mismatch.\n    Expected: {}\n    Got:      {}",
            expected,
            actual
        );
    }

    Ok(())
}

/// Remove stale `.purple_new_*` files from previous interrupted updates.
fn clean_stale_staged(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && name.starts_with(".purple_new_")
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// Check if a directory is writable.
fn is_writable(path: &Path) -> bool {
    let probe = path.join(format!(".purple_write_test_{}", std::process::id()));
    if std::fs::File::create(&probe).is_ok() {
        let _ = std::fs::remove_file(&probe);
        true
    } else {
        false
    }
}

/// RAII guard that removes a temp directory on drop.
struct TempCleanup<'a>(&'a Path);

impl Drop for TempCleanup<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0);
    }
}

#[cfg(test)]
#[path = "update_tests.rs"]
mod tests;
