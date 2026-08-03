// Resolved process environment and filesystem paths, captured once at the
// edge (`launcher::run`) and threaded explicitly from there. Replaces ambient
// `std::env::var` / `dirs::home_dir()` reads scattered through the codebase so
// tests construct an `Env` directly instead of mutating process-global state.
//
// `Env` is immutable after construction, so an `Arc<Env>` crosses thread
// boundaries into worker closures without a lock.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Home-derived file locations under `~/.purple` and `~/.ssh`. One place that
/// knows the on-disk layout; every consumer asks here instead of joining the
/// home directory itself.
#[derive(Clone)]
pub struct Paths {
    home: PathBuf,
}

impl Paths {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self { home: home.into() }
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    /// `~/.purple`.
    pub fn purple_dir(&self) -> PathBuf {
        self.home.join(".purple")
    }

    /// `~/.purple/preferences`.
    pub fn preferences(&self) -> PathBuf {
        self.purple_dir().join("preferences")
    }

    /// `~/.purple/snippets`.
    pub fn snippets_dir(&self) -> PathBuf {
        self.purple_dir().join("snippets")
    }

    /// `~/.purple/container_cache.jsonl`.
    pub fn container_cache(&self) -> PathBuf {
        self.purple_dir().join("container_cache.jsonl")
    }

    /// `~/.purple/purple.log`.
    pub fn log_file(&self) -> PathBuf {
        self.purple_dir().join("purple.log")
    }

    /// `~/.purple/history.tsv`.
    pub fn history(&self) -> PathBuf {
        self.purple_dir().join("history.tsv")
    }

    /// `~/.purple/key_activity.json`.
    pub fn key_activity(&self) -> PathBuf {
        self.purple_dir().join("key_activity.json")
    }

    /// `~/.purple/snippet_runs.json`.
    pub fn snippet_runs(&self) -> PathBuf {
        self.purple_dir().join("snippet_runs.json")
    }

    /// `~/.purple/sync_history.tsv`.
    pub fn sync_history(&self) -> PathBuf {
        self.purple_dir().join("sync_history.tsv")
    }

    /// `~/.purple/recents.json`.
    pub fn recents(&self) -> PathBuf {
        self.purple_dir().join("recents.json")
    }

    /// `~/.purple/providers`, the provider config file.
    pub fn providers_config(&self) -> PathBuf {
        self.purple_dir().join("providers")
    }

    /// `~/.purple/themes`, the custom theme directory.
    pub fn themes_dir(&self) -> PathBuf {
        self.purple_dir().join("themes")
    }

    /// `~/.aws/credentials`, the shared AWS credentials file.
    pub fn aws_credentials_file(&self) -> PathBuf {
        self.home.join(".aws").join("credentials")
    }

    /// `~/.purple/last_version_check`.
    pub fn last_version_check(&self) -> PathBuf {
        self.purple_dir().join("last_version_check")
    }

    /// `~/.purple/certs`.
    pub fn certs_dir(&self) -> PathBuf {
        self.purple_dir().join("certs")
    }

    /// `~/.purple/certs/<alias>-cert.pub`.
    pub fn cert_for(&self, alias: &str) -> PathBuf {
        self.certs_dir().join(format!("{alias}-cert.pub"))
    }

    /// `~/.ssh`.
    pub fn ssh_dir(&self) -> PathBuf {
        self.home.join(".ssh")
    }

    /// Askpass retry marker `~/.purple/.askpass_<safe>`. The alias is
    /// sanitised (`/`, `\`, `.` become `_`) to prevent path traversal.
    pub fn askpass_marker(&self, alias: &str) -> PathBuf {
        let safe = alias.replace(['/', '\\', '.'], "_");
        self.purple_dir().join(format!(".askpass_{safe}"))
    }
}

/// The resolved environment for one process run: the home-derived paths plus a
/// snapshot of the process environment variables. Built once via
/// [`Env::from_process`] and passed down by reference (or `Arc`) rather than
/// re-read on demand.
#[derive(Clone)]
pub struct Env {
    paths: Option<Paths>,
    vars: HashMap<String, String>,
    // Test sandbox: owns the temp directory that `paths` points into so it
    // lives exactly as long as the Env (and any `Arc<Env>` clone). Absent from
    // production builds; `tempfile` is a dev-dependency.
    #[cfg(test)]
    _sandbox: Option<std::sync::Arc<tempfile::TempDir>>,
}

impl Env {
    fn new_inner(paths: Option<Paths>, vars: HashMap<String, String>) -> Self {
        Self {
            paths,
            vars,
            #[cfg(test)]
            _sandbox: None,
        }
    }

    /// Capture the real process environment: the home directory and a snapshot
    /// of all environment variables. The single point where production reads
    /// `std::env` and `dirs::home_dir`.
    pub fn from_process() -> Self {
        Self::new_inner(dirs::home_dir().map(Paths::new), std::env::vars().collect())
    }

    /// A test environment rooted at `home` with no environment variables. Add
    /// variables with [`Env::with_var`].
    pub fn for_test(home: impl Into<PathBuf>) -> Self {
        Self::new_inner(Some(Paths::new(home)), HashMap::new())
    }

    /// An environment with neither a home directory nor variables. Models the
    /// rare case where `dirs::home_dir()` returns `None`.
    pub fn empty() -> Self {
        Self::new_inner(None, HashMap::new())
    }

    /// A self-cleaning sandbox rooted at a fresh temp directory, owned by the
    /// Env. Each call is isolated, so parallel tests never share on-disk state
    /// and need no lock. The default for test `App` fixtures.
    #[cfg(test)]
    pub fn sandboxed() -> Self {
        let dir = tempfile::tempdir().expect("create test sandbox tempdir");
        let mut env = Self::for_test(dir.path());
        env._sandbox = Some(std::sync::Arc::new(dir));
        env
    }

    /// Builder: set a variable. Chainable, for test construction.
    #[must_use]
    pub fn with_var(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.vars.insert(key.into(), value.into());
        self
    }

    /// Home-derived paths, or `None` when the home directory is unknown.
    pub fn paths(&self) -> Option<&Paths> {
        self.paths.as_ref()
    }

    /// Raw lookup of an arbitrary variable. Used by SSH config `${VAR}`
    /// expansion, which references user-chosen names.
    pub fn var(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(String::as_str)
    }

    /// `VAULT_ADDR` fallback for Vault SSH address resolution.
    pub fn vault_addr(&self) -> Option<&str> {
        self.var("VAULT_ADDR")
    }

    /// `(AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY)` when both are set.
    pub fn aws_credentials(&self) -> Option<(&str, &str)> {
        match (
            self.var("AWS_ACCESS_KEY_ID"),
            self.var("AWS_SECRET_ACCESS_KEY"),
        ) {
            (Some(id), Some(secret)) => Some((id, secret)),
            _ => None,
        }
    }

    /// `AWS_SESSION_TOKEN`, set alongside temporary STS credentials.
    pub fn aws_session_token(&self) -> Option<&str> {
        self.var("AWS_SESSION_TOKEN")
    }

    /// `PURPLE_TOKEN`, the self-invocation auth token.
    pub fn purple_token(&self) -> Option<&str> {
        self.var("PURPLE_TOKEN")
    }

    /// True when `NO_COLOR` is present (any value), per the no-color convention.
    pub fn no_color(&self) -> bool {
        self.vars.contains_key("NO_COLOR")
    }

    /// `COLORTERM`.
    pub fn colorterm(&self) -> Option<&str> {
        self.var("COLORTERM")
    }

    /// `TERM_PROGRAM`.
    pub fn term_program(&self) -> Option<&str> {
        self.var("TERM_PROGRAM")
    }

    /// `TERM`.
    pub fn term(&self) -> Option<&str> {
        self.var("TERM")
    }

    /// True when running inside tmux (`TMUX` is set).
    pub fn in_tmux(&self) -> bool {
        self.vars.contains_key("TMUX")
    }

    /// Proxy-related variable names that are set to a non-empty value, in a
    /// stable order. Drives the startup banner's proxy summary.
    pub fn active_proxy_vars(&self) -> Vec<&'static str> {
        ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY"]
            .into_iter()
            .filter(|k| self.var(k).is_some_and(|v| !v.is_empty()))
            .collect()
    }

    /// Build a `Command` for `program` whose environment is exactly this Env's
    /// snapshot. In production the snapshot is the full process environment
    /// captured at startup, so the subprocess sees the same env it would have
    /// inherited. Tests construct an `Env` with only the vars they care about
    /// (e.g. a stub-binary `PATH`), so subprocess resolution and env-dependent
    /// behaviour are controlled without mutating the process-global env (no
    /// `unsafe set_var`, no lock).
    pub fn command(&self, program: &str) -> std::process::Command {
        let mut cmd = std::process::Command::new(program);
        cmd.env_clear();
        cmd.envs(&self.vars);
        cmd
    }
}

// Manual Debug so a stray `{:?}` never dumps secrets (PURPLE_TOKEN, AWS keys,
// VAULT_ADDR). Shows the home directory and the set of variable names only.
impl std::fmt::Debug for Env {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut names: Vec<&str> = self.vars.keys().map(String::as_str).collect();
        names.sort_unstable();
        f.debug_struct("Env")
            .field("home", &self.paths.as_ref().map(Paths::home))
            .field("var_names", &names)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_derive_under_purple_and_ssh() {
        let p = Paths::new("/home/u");
        assert_eq!(p.purple_dir(), PathBuf::from("/home/u/.purple"));
        assert_eq!(
            p.preferences(),
            PathBuf::from("/home/u/.purple/preferences")
        );
        assert_eq!(p.snippets_dir(), PathBuf::from("/home/u/.purple/snippets"));
        assert_eq!(
            p.container_cache(),
            PathBuf::from("/home/u/.purple/container_cache.jsonl")
        );
        assert_eq!(p.log_file(), PathBuf::from("/home/u/.purple/purple.log"));
        assert_eq!(p.history(), PathBuf::from("/home/u/.purple/history.tsv"));
        assert_eq!(
            p.last_version_check(),
            PathBuf::from("/home/u/.purple/last_version_check")
        );
        assert_eq!(p.certs_dir(), PathBuf::from("/home/u/.purple/certs"));
        assert_eq!(p.ssh_dir(), PathBuf::from("/home/u/.ssh"));
    }

    #[test]
    fn cert_for_uses_alias_filename() {
        let p = Paths::new("/home/u");
        assert_eq!(
            p.cert_for("web-1"),
            PathBuf::from("/home/u/.purple/certs/web-1-cert.pub")
        );
    }

    #[test]
    fn askpass_marker_sanitises_traversal_chars() {
        let p = Paths::new("/home/u");
        assert_eq!(
            p.askpass_marker("a/b\\c.d"),
            PathBuf::from("/home/u/.purple/.askpass_a_b_c_d")
        );
    }

    #[test]
    fn for_test_has_paths_and_no_vars() {
        let env = Env::for_test("/tmp/x");
        assert_eq!(env.paths().unwrap().home(), Path::new("/tmp/x"));
        assert_eq!(env.var("ANYTHING"), None);
        assert!(!env.no_color());
    }

    #[test]
    fn empty_has_no_paths() {
        let env = Env::empty();
        assert!(env.paths().is_none());
    }

    #[test]
    fn sandboxed_gives_isolated_existing_dirs() {
        let a = Env::sandboxed();
        let b = Env::sandboxed();
        let pa = a.paths().unwrap().home().to_path_buf();
        let pb = b.paths().unwrap().home().to_path_buf();
        assert_ne!(pa, pb, "each sandbox is a distinct directory");
        assert!(pa.exists(), "sandbox home exists for the Env's lifetime");
        // Writing through the derived paths works (atomic_write creates parents).
        let prefs = a.paths().unwrap().preferences();
        crate::fs_util::atomic_write(&prefs, b"theme=Purple\n").unwrap();
        assert_eq!(std::fs::read_to_string(&prefs).unwrap(), "theme=Purple\n");
    }

    #[test]
    fn with_var_sets_typed_accessors() {
        let env = Env::for_test("/tmp/x")
            .with_var("VAULT_ADDR", "https://vault.example:8200")
            .with_var("COLORTERM", "truecolor")
            .with_var("NO_COLOR", "1")
            .with_var("TMUX", "/tmp/tmux-1000/default,1,0");
        assert_eq!(env.vault_addr(), Some("https://vault.example:8200"));
        assert_eq!(env.colorterm(), Some("truecolor"));
        assert!(env.no_color());
        assert!(env.in_tmux());
    }

    #[test]
    fn aws_credentials_require_both_keys() {
        let only_id = Env::for_test("/tmp/x").with_var("AWS_ACCESS_KEY_ID", "AKIA");
        assert_eq!(only_id.aws_credentials(), None);
        let both = only_id.with_var("AWS_SECRET_ACCESS_KEY", "secret");
        assert_eq!(both.aws_credentials(), Some(("AKIA", "secret")));
    }

    #[test]
    fn active_proxy_vars_filters_empty_and_orders() {
        let env = Env::for_test("/tmp/x")
            .with_var("HTTPS_PROXY", "http://proxy:3128")
            .with_var("HTTP_PROXY", "")
            .with_var("NO_PROXY", "localhost");
        assert_eq!(env.active_proxy_vars(), vec!["HTTPS_PROXY", "NO_PROXY"]);
    }

    #[test]
    fn debug_redacts_secret_values() {
        let env = Env::for_test("/tmp/x")
            .with_var("PURPLE_TOKEN", "super-secret")
            .with_var("VAULT_ADDR", "https://vault.example:8200");
        let rendered = format!("{env:?}");
        assert!(!rendered.contains("super-secret"));
        assert!(!rendered.contains("vault.example"));
        assert!(rendered.contains("PURPLE_TOKEN"));
        assert!(rendered.contains("VAULT_ADDR"));
    }

    #[test]
    fn from_process_captures_home_and_vars() {
        // Smoke test against the real process: home is usually set, and the
        // snapshot is internally consistent with the typed accessors.
        let env = Env::from_process();
        // No assertion on specific vars (CI environments differ); just verify
        // the snapshot mechanism works end to end.
        let _ = env.paths();
        let _ = env.var("PATH");
    }
}
