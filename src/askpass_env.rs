//! SSH_ASKPASS environment wiring shared by `connection`, `tunnel`, `file_browser`
//! and `snippet`. Lives in the library crate so every ssh/scp call site can route
//! through a single configuration point and a single regression test covers them all.

use std::path::Path;
use std::process::Command;

/// Configure an `ssh` or `scp` [`Command`] so the child process invokes purple
/// as its SSH_ASKPASS program. Sets:
///
/// - `SSH_ASKPASS` to the current purple binary (falling back to argv\[0\]).
/// - `SSH_ASKPASS_REQUIRE=force` so OpenSSH invokes askpass regardless of whether
///   a TTY is attached or `DISPLAY`/`WAYLAND_DISPLAY` is set. OpenSSH's `prefer`
///   mode gates askpass on a non-empty `DISPLAY` or `WAYLAND_DISPLAY` (see
///   `readpass.c` in openssh-portable); inside a headless ssh session on Linux
///   both are empty, so `prefer` would silently no-op and ssh would fall back to
///   the TTY prompt, bypassing purple's vault lookup entirely.
/// - `PURPLE_ASKPASS_MODE`, `PURPLE_HOST_ALIAS`, `PURPLE_CONFIG_PATH` so the
///   askpass subprocess (re-entering purple) can look up the right host config.
///
/// Only the env vars are set; stdio, args and working directory are left to the
/// caller. `BW_SESSION` is also the caller's concern since not every call site
/// forwards it explicitly.
pub(crate) fn configure_ssh_command(cmd: &mut Command, alias: &str, config_path: &Path) {
    let exe = std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
        .or_else(|| std::env::args().next())
        .unwrap_or_else(|| "purple".to_string());
    cmd.env("SSH_ASKPASS", &exe)
        .env("SSH_ASKPASS_REQUIRE", "force")
        .env("PURPLE_ASKPASS_MODE", "1")
        .env("PURPLE_HOST_ALIAS", alias)
        .env("PURPLE_CONFIG_PATH", config_path.as_os_str());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::path::PathBuf;

    /// Snapshot the env vars configured on a Command into a HashMap for inspection.
    /// Skips entries whose value is `None` (those are env removals, not additions).
    fn snapshot_envs(cmd: &Command) -> HashMap<OsString, OsString> {
        cmd.get_envs()
            .filter_map(|(k, v)| v.map(|val| (k.to_os_string(), val.to_os_string())))
            .collect()
    }

    #[test]
    fn sets_ssh_askpass_require_to_force() {
        // Regression test for GitHub issue #19: purple previously used `prefer`,
        // which silently no-ops when DISPLAY and WAYLAND_DISPLAY are empty. `force`
        // bypasses that gate. This test locks the value so a future change back to
        // `prefer` (or any other value) fails CI.
        let mut cmd = Command::new("ssh");
        configure_ssh_command(&mut cmd, "myhost", &PathBuf::from("/tmp/cfg"));
        let envs = snapshot_envs(&cmd);
        assert_eq!(
            envs.get(&OsString::from("SSH_ASKPASS_REQUIRE")),
            Some(&OsString::from("force")),
            "SSH_ASKPASS_REQUIRE must be 'force' to work in headless ssh sessions"
        );
    }

    #[test]
    fn sets_ssh_askpass_to_current_exe() {
        let mut cmd = Command::new("ssh");
        configure_ssh_command(&mut cmd, "myhost", &PathBuf::from("/tmp/cfg"));
        let envs = snapshot_envs(&cmd);
        let askpass = envs
            .get(&OsString::from("SSH_ASKPASS"))
            .expect("SSH_ASKPASS must be set");
        assert!(
            !askpass.is_empty(),
            "SSH_ASKPASS must point at a non-empty path"
        );
    }

    #[test]
    fn sets_purple_context_vars() {
        let mut cmd = Command::new("ssh");
        configure_ssh_command(&mut cmd, "myhost", &PathBuf::from("/tmp/my/ssh_config"));
        let envs = snapshot_envs(&cmd);
        assert_eq!(
            envs.get(&OsString::from("PURPLE_ASKPASS_MODE")),
            Some(&OsString::from("1"))
        );
        assert_eq!(
            envs.get(&OsString::from("PURPLE_HOST_ALIAS")),
            Some(&OsString::from("myhost"))
        );
        assert_eq!(
            envs.get(&OsString::from("PURPLE_CONFIG_PATH")),
            Some(&OsString::from("/tmp/my/ssh_config"))
        );
    }

    #[test]
    fn passes_alias_with_spaces_and_slashes_unmodified() {
        let mut cmd = Command::new("ssh");
        configure_ssh_command(&mut cmd, "my host/with slash", &PathBuf::from("/tmp/cfg"));
        let envs = snapshot_envs(&cmd);
        assert_eq!(
            envs.get(&OsString::from("PURPLE_HOST_ALIAS")),
            Some(&OsString::from("my host/with slash"))
        );
    }

    #[test]
    fn does_not_set_bw_session() {
        // BW_SESSION forwarding is the caller's responsibility (connection.rs
        // forwards it explicitly; tunnel.rs relies on inheritance). The helper
        // must not touch it.
        let mut cmd = Command::new("ssh");
        configure_ssh_command(&mut cmd, "myhost", &PathBuf::from("/tmp/cfg"));
        let envs = snapshot_envs(&cmd);
        assert!(!envs.contains_key(&OsString::from("BW_SESSION")));
    }
}
