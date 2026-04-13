use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use log::{debug, error};

/// Advisory file lock using a `.lock` file.
/// The lock is released when the `FileLock` is dropped.
pub struct FileLock {
    lock_path: PathBuf,
    #[cfg(unix)]
    _file: fs::File,
}

impl FileLock {
    /// Acquire an advisory lock for the given path.
    /// Creates a `.purple_lock` file alongside the target and holds an `flock` on it.
    /// Blocks until the lock is acquired (or returns an error on failure).
    pub fn acquire(path: &Path) -> io::Result<Self> {
        let mut lock_name = path.file_name().unwrap_or_default().to_os_string();
        lock_name.push(".purple_lock");
        let lock_path = path.with_file_name(lock_name);

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(&lock_path)?;

            // SAFETY: flock() is safe to call on any valid file descriptor.
            // The fd comes from a File we just opened and own. LOCK_EX
            // requests an exclusive advisory lock, blocking until acquired.
            let ret =
                unsafe { libc::flock(std::os::unix::io::AsRawFd::as_raw_fd(&file), libc::LOCK_EX) };
            if ret != 0 {
                return Err(io::Error::last_os_error());
            }

            Ok(FileLock {
                lock_path,
                _file: file,
            })
        }

        #[cfg(not(unix))]
        {
            // On non-Unix, use a simple lock file (best-effort)
            let file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
                .or_else(|_| {
                    // If it already exists, wait briefly and retry
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    fs::remove_file(&lock_path).ok();
                    fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&lock_path)
                })?;
            Ok(FileLock {
                lock_path,
                _file: file,
            })
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // On Unix, flock is released when the file descriptor is closed (automatic).
        // Clean up the lock file.
        let _ = fs::remove_file(&self.lock_path);
    }
}

/// Atomic write: write content to a PID-suffixed temp file with chmod 600, then rename.
/// Uses O_EXCL (create_new) to prevent symlink attacks on the temp file path.
/// Cleans up the temp file on failure.
pub fn atomic_write(path: &Path, content: &[u8]) -> io::Result<()> {
    debug!("Atomic write: {}", path.display());
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(format!(".purple_tmp.{}", std::process::id()));
    let tmp_path = path.with_file_name(tmp_name);

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        // Try O_EXCL first. If a stale tmp file exists from a crashed run, remove
        // it and retry once. This avoids a TOCTOU gap from removing before creating.
        let open = || {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp_path)
        };
        let mut file = match open() {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(&tmp_path);
                open().map_err(|e| {
                    io::Error::new(
                        e.kind(),
                        format!("Failed to create temp file {}: {}", tmp_path.display(), e),
                    )
                })?
            }
            Err(e) => {
                return Err(io::Error::new(
                    e.kind(),
                    format!("Failed to create temp file {}: {}", tmp_path.display(), e),
                ));
            }
        };
        if let Err(e) = file.write_all(content) {
            drop(file);
            let _ = fs::remove_file(&tmp_path);
            return Err(e);
        }
        if let Err(e) = file.sync_all() {
            drop(file);
            let _ = fs::remove_file(&tmp_path);
            return Err(e);
        }
    }

    #[cfg(not(unix))]
    {
        if let Err(e) = fs::write(&tmp_path, content) {
            let _ = fs::remove_file(&tmp_path);
            return Err(e);
        }
        // sync_all via reopen since fs::write doesn't return a File handle
        match fs::File::open(&tmp_path) {
            Ok(f) => {
                if let Err(e) = f.sync_all() {
                    let _ = fs::remove_file(&tmp_path);
                    return Err(e);
                }
            }
            Err(e) => {
                let _ = fs::remove_file(&tmp_path);
                return Err(e);
            }
        }
    }

    let result = fs::rename(&tmp_path, path);
    if let Err(ref err) = result {
        let _ = fs::remove_file(&tmp_path);
        error!("[purple] Atomic write failed: {}: {err}", path.display());
    }
    result
}
