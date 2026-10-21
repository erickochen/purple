// Resolved process environment and filesystem paths, captured once at the
// edge (`launcher::run`) and threaded explicitly from there. Replaces ambient
// `std::env::var` / `dirs::home_dir()` reads scattered through the codebase so
// tests construct an `Env` directly instead of mutating process-global state.
//
// `Env` is immutable after construction, so an `Arc<Env>` crosses thread
// boundaries into worker closures without a lock.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The four categories purple's files fall into, following the XDG Base
/// Directory split. Each resolves to its own directory; by default all four
/// point at the legacy `~/.purple`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    /// Files the user edits or syncs as dotfiles: preferences, snippets,
    /// providers, themes.
    Config,
    /// Files that are important and not regenerable but not config: signed
    /// certificates and the original SSH config backup.
    Data,
    /// History, recents, activity ledgers, logs and retry markers.
    State,
    /// Regenerable caches: the container cache and the version check.
    Cache,
}

impl Category {
    pub const ALL: [Category; 4] = [
        Category::Config,
        Category::Data,
        Category::State,
        Category::Cache,
    ];

    /// The explicit purple override variable, e.g. `PURPLE_CONFIG_DIR`.
    pub fn purple_var(self) -> &'static str {
        match self {
            Category::Config => "PURPLE_CONFIG_DIR",
            Category::Data => "PURPLE_DATA_DIR",
            Category::State => "PURPLE_STATE_DIR",
            Category::Cache => "PURPLE_CACHE_DIR",
        }
    }

    /// The XDG base directory variable, e.g. `XDG_CONFIG_HOME`.
    pub fn xdg_var(self) -> &'static str {
        match self {
            Category::Config => "XDG_CONFIG_HOME",
            Category::Data => "XDG_DATA_HOME",
            Category::State => "XDG_STATE_HOME",
            Category::Cache => "XDG_CACHE_HOME",
        }
    }

    /// Lowercase name for log lines.
    pub fn name(self) -> &'static str {
        match self {
            Category::Config => "config",
            Category::Data => "data",
            Category::State => "state",
            Category::Cache => "cache",
        }
    }
}

/// Subdirectory under an XDG base directory.
const XDG_APP_DIR: &str = "purple";

/// Home-derived file locations under `~/.purple` and `~/.ssh`. One place that
/// knows the on-disk layout; every consumer asks here instead of joining the
/// home directory itself.
///
/// The purple files live in four category directories (see [`Category`]).
/// [`Paths::new`] puts all four at `~/.purple`; [`Paths::resolve`] honors the
/// `PURPLE_*_DIR` and `XDG_*_HOME` variables per category.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    home: PathBuf,
    config_dir: PathBuf,
    data_dir: PathBuf,
    state_dir: PathBuf,
    cache_dir: PathBuf,
}

impl Paths {
    /// The legacy layout: every category under `~/.purple`.
    pub fn new(home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        let legacy = home.join(".purple");
        Self {
            home,
            config_dir: legacy.clone(),
            data_dir: legacy.clone(),
            state_dir: legacy.clone(),
            cache_dir: legacy,
        }
    }

    /// Resolve each category directory from the environment. Precedence per
    /// category: `PURPLE_<CATEGORY>_DIR` when set and non-empty (a leading
    /// `~/` expands to the home directory), then `$XDG_<CATEGORY>_HOME/purple`
    /// when that variable is set, non-empty and absolute (a relative XDG path
    /// is invalid per the spec and ignored), else the legacy `~/.purple`.
    pub fn resolve(home: impl Into<PathBuf>, var: impl Fn(&str) -> Option<String>) -> Self {
        let mut paths = Self::new(home);
        for category in Category::ALL {
            let dir = resolve_category(&paths.home, category, &var);
            *paths.dir_mut(category) = dir;
        }
        paths
    }

    fn dir_mut(&mut self, category: Category) -> &mut PathBuf {
        match category {
            Category::Config => &mut self.config_dir,
            Category::Data => &mut self.data_dir,
            Category::State => &mut self.state_dir,
            Category::Cache => &mut self.cache_dir,
        }
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    /// `~/.purple`, the single directory every category used before the
    /// split. Only the migration reads from here.
    pub fn legacy_dir(&self) -> PathBuf {
        self.home.join(".purple")
    }

    /// The directory a category resolved to.
    pub fn dir(&self, category: Category) -> &Path {
        match category {
            Category::Config => &self.config_dir,
            Category::Data => &self.data_dir,
            Category::State => &self.state_dir,
            Category::Cache => &self.cache_dir,
        }
    }

    /// Preferences, snippets, providers and themes.
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// Signed certificates and the original SSH config backup.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// History, recents, ledgers, logs and retry markers.
    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    /// Container cache and version check.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// True when at least one category lives outside `~/.purple`.
    pub fn is_split(&self) -> bool {
        let legacy = self.legacy_dir();
        Category::ALL.iter().any(|c| self.dir(*c) != legacy)
    }

    /// `<config>/preferences`.
    pub fn preferences(&self) -> PathBuf {
        self.config_dir.join("preferences")
    }

    /// `<config>/snippets`, the INI-style snippet store.
    pub fn snippets_file(&self) -> PathBuf {
        self.config_dir.join("snippets")
    }

    /// `<config>/providers`, the provider config file.
    pub fn providers_config(&self) -> PathBuf {
        self.config_dir.join("providers")
    }

    /// `<config>/themes`, the custom theme directory.
    pub fn themes_dir(&self) -> PathBuf {
        self.config_dir.join("themes")
    }

    /// `<data>/certs`.
    pub fn certs_dir(&self) -> PathBuf {
        self.data_dir.join("certs")
    }

    /// `<data>/certs/<alias>-cert.pub`.
    pub fn cert_for(&self, alias: &str) -> PathBuf {
        self.certs_dir().join(format!("{alias}-cert.pub"))
    }

    /// `<data>/config.original`, the untouched SSH config backup taken on
    /// the first launch.
    pub fn config_original(&self) -> PathBuf {
        self.data_dir.join("config.original")
    }

    /// `<state>/history.tsv`.
    pub fn history(&self) -> PathBuf {
        self.state_dir.join("history.tsv")
    }

    /// `<state>/recents.json`.
    pub fn recents(&self) -> PathBuf {
        self.state_dir.join("recents.json")
    }

    /// `<state>/key_activity.json`.
    pub fn key_activity(&self) -> PathBuf {
        self.state_dir.join("key_activity.json")
    }

    /// `<state>/snippet_runs.json`.
    pub fn snippet_runs(&self) -> PathBuf {
        self.state_dir.join("snippet_runs.json")
    }

    /// `<state>/sync_history.tsv`.
    pub fn sync_history(&self) -> PathBuf {
        self.state_dir.join("sync_history.tsv")
    }

    /// `<state>/purple.log`.
    pub fn log_file(&self) -> PathBuf {
        self.state_dir.join("purple.log")
    }

    /// `<state>/mcp-audit.log`.
    pub fn mcp_audit_log(&self) -> PathBuf {
        self.state_dir.join("mcp-audit.log")
    }

    /// Askpass retry marker `<state>/.askpass_<safe>`. The alias is
    /// sanitized (`/`, `\`, `.` become `_`) to prevent path traversal.
    pub fn askpass_marker(&self, alias: &str) -> PathBuf {
        let safe = alias.replace(['/', '\\', '.'], "_");
        self.state_dir.join(format!(".askpass_{safe}"))
    }

    /// Marker recording that a background run met a prompt it could not
    /// place. The dot after `withheld` keeps this apart from a retry marker,
    /// whose own sanitizer turns every dot into an underscore. The alias is
    /// percent-encoded rather than flattened, so `web.prod` and `web_prod`
    /// keep their own file and neither can walk out of the state directory.
    pub fn askpass_withheld_marker(&self, alias: &str) -> PathBuf {
        self.state_dir.join(format!(
            "{}{}",
            crate::askpass::WITHHELD_MARKER_PREFIX,
            encode_marker_alias(alias)
        ))
    }

    /// Marker recording that a background run met a security key PIN prompt
    /// it had no PIN for. Keyed on the flow that ran it, so a container
    /// refresh never answers for a tunnel. Holds the key path.
    pub fn askpass_pin_request_marker(&self, flow: &str, alias: &str) -> PathBuf {
        self.state_dir.join(format!(
            "{}request.{flow}.{}",
            crate::security_key::PIN_MARKER_PREFIX,
            encode_marker_alias(alias)
        ))
    }

    /// Marker listing which ssh process received which security key PIN for
    /// `alias`, one `<pid>\t<key path>` line each.
    pub fn askpass_pin_used_marker(&self, alias: &str) -> PathBuf {
        self.state_dir.join(format!(
            "{}used.{}",
            crate::security_key::PIN_MARKER_PREFIX,
            encode_marker_alias(alias)
        ))
    }

    /// Marker recording that a security key refused the PIN purple handed
    /// out. Keyed on a digest of the key path, which is free to be long.
    pub fn askpass_pin_refused_marker(&self, key_digest: &str) -> PathBuf {
        self.state_dir.join(format!(
            "{}refused.{key_digest}",
            crate::security_key::PIN_MARKER_PREFIX
        ))
    }

    /// Marker whose mtime is the last time askpass handed out the PIN of one
    /// security key, in any run. Keyed on the key path digest.
    pub fn askpass_pin_last_marker(&self, key_digest: &str) -> PathBuf {
        self.state_dir.join(format!(
            "{}last.{key_digest}",
            crate::security_key::PIN_MARKER_PREFIX
        ))
    }

    /// Lock file that queues the hand-outs of one security key's PIN.
    pub fn askpass_pin_lock_marker(&self, key_digest: &str) -> PathBuf {
        self.state_dir.join(format!(
            "{}lock.{key_digest}",
            crate::security_key::PIN_MARKER_PREFIX
        ))
    }

    /// `<cache>/container_cache.jsonl`.
    pub fn container_cache(&self) -> PathBuf {
        self.cache_dir.join("container_cache.jsonl")
    }

    /// `<cache>/last_version_check`.
    pub fn last_version_check(&self) -> PathBuf {
        self.cache_dir.join("last_version_check")
    }

    /// `~/.aws/credentials`, the shared AWS credentials file.
    pub fn aws_credentials_file(&self) -> PathBuf {
        self.home.join(".aws").join("credentials")
    }

    /// `~/.aws/config`, the shared AWS config file. Named profiles live here
    /// as `[profile <name>]`, and it is the only file that carries the
    /// assume-role keys.
    pub fn aws_config_file(&self) -> PathBuf {
        self.home.join(".aws").join("config")
    }

    /// `~/.ssh`.
    pub fn ssh_dir(&self) -> PathBuf {
        self.home.join(".ssh")
    }

    /// `path` for display, with the home directory abbreviated to `~`.
    pub fn abbreviate_home(&self, path: &Path) -> String {
        match path.strip_prefix(&self.home) {
            Ok(rest) if !rest.as_os_str().is_empty() => format!("~/{}", rest.display()),
            _ => path.display().to_string(),
        }
    }
}

/// Percent-encode an alias for a marker filename rather than flattening it,
/// so `web.prod` and `web_prod` keep their own file and neither can walk out
/// of the state directory.
fn encode_marker_alias(alias: &str) -> String {
    let mut safe = String::with_capacity(alias.len());
    for c in alias.chars() {
        match c {
            '%' => safe.push_str("%25"),
            '/' => safe.push_str("%2f"),
            '\\' => safe.push_str("%5c"),
            '.' => safe.push_str("%2e"),
            other => safe.push(other),
        }
    }
    safe
}

/// One category's directory per the precedence documented on
/// [`Paths::resolve`].
fn resolve_category(
    home: &Path,
    category: Category,
    var: &impl Fn(&str) -> Option<String>,
) -> PathBuf {
    if let Some(explicit) = var(category.purple_var()).filter(|v| !v.trim().is_empty()) {
        return expand_home(home, &explicit);
    }
    if let Some(base) = var(category.xdg_var()).filter(|v| !v.trim().is_empty()) {
        let base = PathBuf::from(base);
        if base.is_absolute() {
            return base.join(XDG_APP_DIR);
        }
    }
    home.join(".purple")
}

/// Expand a leading `~`, `$HOME` or `${HOME}` in an override value. A value
/// written in a file rather than typed in a shell arrives unexpanded.
fn expand_home(home: &Path, value: &str) -> PathBuf {
    if ["~", "$HOME", "${HOME}"].contains(&value) {
        return home.to_path_buf();
    }
    for prefix in ["~/", "$HOME/", "${HOME}/"] {
        if let Some(rest) = value.strip_prefix(prefix) {
            return home.join(rest);
        }
    }
    PathBuf::from(value)
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
    /// `std::env` and `dirs::home_dir`. The category directories resolve from
    /// the same snapshot.
    pub fn from_process() -> Self {
        let vars: HashMap<String, String> = std::env::vars().collect();
        let paths = dirs::home_dir().map(|home| Paths::resolve(home, |k| vars.get(k).cloned()));
        Self::new_inner(paths, vars)
    }

    /// A test environment rooted at `home` with the legacy layout and no
    /// environment variables. Add variables with [`Env::with_var`], swap the
    /// layout with [`Env::with_paths`].
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

    /// Builder: replace the paths, e.g. with a split layout from
    /// [`Paths::resolve`]. Chainable, for test construction.
    #[must_use]
    pub fn with_paths(mut self, paths: Paths) -> Self {
        self.paths = Some(paths);
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

    /// Where the shared AWS credentials file lives. `AWS_SHARED_CREDENTIALS_FILE`
    /// relocates it; otherwise it is `~/.aws/credentials`.
    pub fn aws_credentials_file(&self) -> Option<PathBuf> {
        self.relocatable_aws_file("AWS_SHARED_CREDENTIALS_FILE", Paths::aws_credentials_file)
    }

    /// Where the shared AWS config file lives. `AWS_CONFIG_FILE` relocates it;
    /// otherwise it is `~/.aws/config`.
    pub fn aws_config_file(&self) -> Option<PathBuf> {
        self.relocatable_aws_file("AWS_CONFIG_FILE", Paths::aws_config_file)
    }

    /// Shared resolution for the two relocatable AWS files.
    ///
    /// A leading `~` in the override expands against the known home directory,
    /// because the AWS CLI accepts that form and a shell does not expand it
    /// inside a quoted assignment. Exported blank means no file rather than
    /// the default one: the AWS CLI opens the empty path, finds nothing and
    /// carries on, so blanking the variable is how you keep a tool away from
    /// your keys.
    fn relocatable_aws_file(&self, var: &str, default: fn(&Paths) -> PathBuf) -> Option<PathBuf> {
        let Some(raw) = self.var(var) else {
            return self.paths().map(default);
        };
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        match self.paths() {
            Some(paths) => Some(expand_home(paths.home(), raw)),
            // No home to expand against, so the value can only be used as
            // written.
            None => Some(PathBuf::from(raw)),
        }
    }

    /// `PURPLE_TOKEN`, the self-invocation auth token. A variable that is
    /// exported but blank (`export PURPLE_TOKEN="$UNSET"`) reads as absent:
    /// it is ambient shell state, not a token the caller supplied.
    pub fn purple_token(&self) -> Option<&str> {
        self.var("PURPLE_TOKEN")
            .filter(|token| !token.trim().is_empty())
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

    fn lookup<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| (*v).to_string())
        }
    }

    #[test]
    fn paths_derive_under_purple_and_ssh() {
        let p = Paths::new("/home/u");
        assert_eq!(p.legacy_dir(), PathBuf::from("/home/u/.purple"));
        assert_eq!(
            p.preferences(),
            PathBuf::from("/home/u/.purple/preferences")
        );
        assert_eq!(p.snippets_file(), PathBuf::from("/home/u/.purple/snippets"));
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
        assert_eq!(
            p.config_original(),
            PathBuf::from("/home/u/.purple/config.original")
        );
        assert_eq!(
            p.mcp_audit_log(),
            PathBuf::from("/home/u/.purple/mcp-audit.log")
        );
        assert_eq!(p.ssh_dir(), PathBuf::from("/home/u/.ssh"));
        assert!(!p.is_split());
    }

    #[test]
    fn legacy_layout_puts_every_category_under_dot_purple() {
        let p = Paths::new("/home/u");
        for c in Category::ALL {
            assert_eq!(p.dir(c), Path::new("/home/u/.purple"), "{c:?}");
        }
    }

    #[test]
    fn resolve_without_variables_is_the_legacy_layout() {
        let resolved = Paths::resolve("/home/u", |_| None);
        assert_eq!(resolved, Paths::new("/home/u"));
    }

    #[test]
    fn resolve_honors_each_xdg_variable_independently() {
        let vars = [("XDG_CONFIG_HOME", "/home/u/.config")];
        let p = Paths::resolve("/home/u", lookup(&vars));
        assert_eq!(p.config_dir(), Path::new("/home/u/.config/purple"));
        assert_eq!(
            p.preferences(),
            PathBuf::from("/home/u/.config/purple/preferences")
        );
        assert_eq!(
            p.themes_dir(),
            PathBuf::from("/home/u/.config/purple/themes")
        );
        // The other categories stay put when their variable is unset.
        assert_eq!(p.data_dir(), Path::new("/home/u/.purple"));
        assert_eq!(p.state_dir(), Path::new("/home/u/.purple"));
        assert_eq!(p.cache_dir(), Path::new("/home/u/.purple"));
        assert!(p.is_split());
    }

    #[test]
    fn resolve_maps_every_file_to_its_category() {
        let vars = [
            ("XDG_CONFIG_HOME", "/x/cfg"),
            ("XDG_DATA_HOME", "/x/data"),
            ("XDG_STATE_HOME", "/x/state"),
            ("XDG_CACHE_HOME", "/x/cache"),
        ];
        let p = Paths::resolve("/home/u", lookup(&vars));
        assert_eq!(p.preferences(), PathBuf::from("/x/cfg/purple/preferences"));
        assert_eq!(p.snippets_file(), PathBuf::from("/x/cfg/purple/snippets"));
        assert_eq!(
            p.providers_config(),
            PathBuf::from("/x/cfg/purple/providers")
        );
        assert_eq!(p.themes_dir(), PathBuf::from("/x/cfg/purple/themes"));
        assert_eq!(p.certs_dir(), PathBuf::from("/x/data/purple/certs"));
        assert_eq!(
            p.cert_for("web"),
            PathBuf::from("/x/data/purple/certs/web-cert.pub")
        );
        assert_eq!(
            p.config_original(),
            PathBuf::from("/x/data/purple/config.original")
        );
        assert_eq!(p.history(), PathBuf::from("/x/state/purple/history.tsv"));
        assert_eq!(p.recents(), PathBuf::from("/x/state/purple/recents.json"));
        assert_eq!(
            p.key_activity(),
            PathBuf::from("/x/state/purple/key_activity.json")
        );
        assert_eq!(
            p.snippet_runs(),
            PathBuf::from("/x/state/purple/snippet_runs.json")
        );
        assert_eq!(
            p.sync_history(),
            PathBuf::from("/x/state/purple/sync_history.tsv")
        );
        assert_eq!(p.log_file(), PathBuf::from("/x/state/purple/purple.log"));
        assert_eq!(
            p.mcp_audit_log(),
            PathBuf::from("/x/state/purple/mcp-audit.log")
        );
        assert_eq!(
            p.askpass_marker("a"),
            PathBuf::from("/x/state/purple/.askpass_a")
        );
        assert_eq!(
            p.container_cache(),
            PathBuf::from("/x/cache/purple/container_cache.jsonl")
        );
        assert_eq!(
            p.last_version_check(),
            PathBuf::from("/x/cache/purple/last_version_check")
        );
        // Home-relative paths outside purple's own tree do not move.
        assert_eq!(p.ssh_dir(), PathBuf::from("/home/u/.ssh"));
        assert_eq!(
            p.aws_credentials_file(),
            PathBuf::from("/home/u/.aws/credentials")
        );
        assert_eq!(p.legacy_dir(), PathBuf::from("/home/u/.purple"));
    }

    #[test]
    fn resolve_prefers_purple_override_over_xdg() {
        let vars = [
            ("PURPLE_CONFIG_DIR", "/srv/purple-cfg"),
            ("XDG_CONFIG_HOME", "/home/u/.config"),
        ];
        let p = Paths::resolve("/home/u", lookup(&vars));
        // The override is used verbatim: no `purple` subdirectory appended.
        assert_eq!(p.config_dir(), Path::new("/srv/purple-cfg"));
    }

    #[test]
    fn resolve_expands_home_in_purple_overrides() {
        for value in ["~/purple-cfg", "$HOME/purple-cfg", "${HOME}/purple-cfg"] {
            let vars = [("PURPLE_CONFIG_DIR", value)];
            let p = Paths::resolve("/home/u", lookup(&vars));
            assert_eq!(p.config_dir(), Path::new("/home/u/purple-cfg"), "{value}");
        }
    }

    #[test]
    fn resolve_treats_empty_variables_as_unset() {
        let vars = [
            ("PURPLE_STATE_DIR", ""),
            ("XDG_STATE_HOME", "  "),
            ("XDG_CACHE_HOME", ""),
        ];
        let p = Paths::resolve("/home/u", lookup(&vars));
        assert_eq!(p.state_dir(), Path::new("/home/u/.purple"));
        assert_eq!(p.cache_dir(), Path::new("/home/u/.purple"));
    }

    #[test]
    fn resolve_ignores_relative_xdg_paths() {
        // The spec calls a relative XDG path invalid; it must be ignored.
        let vars = [("XDG_DATA_HOME", "relative/share")];
        let p = Paths::resolve("/home/u", lookup(&vars));
        assert_eq!(p.data_dir(), Path::new("/home/u/.purple"));
    }

    #[test]
    fn resolve_empty_purple_override_falls_through_to_xdg() {
        let vars = [
            ("PURPLE_DATA_DIR", ""),
            ("XDG_DATA_HOME", "/home/u/.local/share"),
        ];
        let p = Paths::resolve("/home/u", lookup(&vars));
        assert_eq!(p.data_dir(), Path::new("/home/u/.local/share/purple"));
    }

    #[test]
    fn category_variable_names() {
        assert_eq!(Category::Config.purple_var(), "PURPLE_CONFIG_DIR");
        assert_eq!(Category::Data.xdg_var(), "XDG_DATA_HOME");
        assert_eq!(Category::State.purple_var(), "PURPLE_STATE_DIR");
        assert_eq!(Category::Cache.xdg_var(), "XDG_CACHE_HOME");
        assert_eq!(Category::State.name(), "state");
    }

    #[test]
    fn abbreviate_home_swaps_the_home_prefix_for_a_tilde() {
        let p = Paths::new("/home/u");
        assert_eq!(
            p.abbreviate_home(&p.config_original()),
            "~/.purple/config.original"
        );
        assert_eq!(p.abbreviate_home(Path::new("/srv/x/y")), "/srv/x/y");
        assert_eq!(p.abbreviate_home(Path::new("/home/u")), "/home/u");
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
    fn askpass_withheld_marker_keeps_each_alias_to_itself() {
        // Provider-synced hosts carry dots, so flattening them would let one
        // host's marker speak for another.
        let p = Paths::new("/home/u");
        assert_ne!(
            p.askpass_withheld_marker("web.prod"),
            p.askpass_withheld_marker("web_prod")
        );
        assert_eq!(
            p.askpass_withheld_marker("web.prod"),
            PathBuf::from("/home/u/.purple/.askpass_withheld.web%2eprod")
        );
    }

    #[test]
    fn askpass_withheld_marker_cannot_walk_out_of_the_state_dir() {
        let p = Paths::new("/home/u");
        assert_eq!(
            p.askpass_withheld_marker("../../etc/passwd"),
            PathBuf::from("/home/u/.purple/.askpass_withheld.%2e%2e%2f%2e%2e%2fetc%2fpasswd")
        );
    }

    #[test]
    fn askpass_withheld_marker_never_collides_with_a_retry_marker() {
        // A host literally named `withheld.x` must not land on the file the
        // withheld marker for `x` uses.
        let p = Paths::new("/home/u");
        assert_ne!(
            p.askpass_marker("withheld.x"),
            p.askpass_withheld_marker("x")
        );
    }

    #[test]
    fn askpass_pin_markers_cannot_walk_out_of_the_state_dir() {
        let p = Paths::new("/home/u");
        assert_eq!(
            p.askpass_pin_request_marker("tunnel", "../x"),
            PathBuf::from("/home/u/.purple/.askpass_pin.request.tunnel.%2e%2e%2fx")
        );
        assert_eq!(
            p.askpass_pin_used_marker("web.prod"),
            PathBuf::from("/home/u/.purple/.askpass_pin.used.web%2eprod")
        );
        assert_eq!(
            p.askpass_pin_refused_marker("abc123"),
            PathBuf::from("/home/u/.purple/.askpass_pin.refused.abc123")
        );
        assert_eq!(
            p.askpass_pin_last_marker("abc123"),
            PathBuf::from("/home/u/.purple/.askpass_pin.last.abc123")
        );
        assert_eq!(
            p.askpass_pin_lock_marker("abc123"),
            PathBuf::from("/home/u/.purple/.askpass_pin.lock.abc123")
        );
    }

    #[test]
    fn askpass_pin_markers_never_collide_with_a_retry_marker() {
        // A retry marker turns every dot into an underscore, so a host named
        // `pin.used.x` cannot land on the PIN marker of `x`.
        let p = Paths::new("/home/u");
        assert_ne!(
            p.askpass_marker("pin.used.x"),
            p.askpass_pin_used_marker("x")
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
    fn with_paths_replaces_the_layout() {
        let split = Paths::resolve("/tmp/x", lookup(&[("XDG_CACHE_HOME", "/tmp/c")]));
        let env = Env::for_test("/tmp/x").with_paths(split.clone());
        assert_eq!(env.paths(), Some(&split));
        assert_eq!(env.paths().unwrap().cache_dir(), Path::new("/tmp/c/purple"));
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
    fn purple_token_treats_a_blank_variable_as_absent() {
        // `export PURPLE_TOKEN="$UNSET"` leaves the variable present and
        // empty. Reading that as a supplied token makes callers skip their
        // stored-token fallback and save the blank over a real credential.
        assert_eq!(Env::for_test("/tmp/x").purple_token(), None);
        for blank in ["", "   "] {
            assert_eq!(
                Env::for_test("/tmp/x")
                    .with_var("PURPLE_TOKEN", blank)
                    .purple_token(),
                None,
                "a blank PURPLE_TOKEN must read as absent"
            );
        }
        assert_eq!(
            Env::for_test("/tmp/x")
                .with_var("PURPLE_TOKEN", "tok")
                .purple_token(),
            Some("tok")
        );
    }

    #[test]
    fn aws_session_token_reads_env_var() {
        let env = Env::for_test("/tmp/x");
        assert_eq!(env.aws_session_token(), None);
        let with_token = env.with_var("AWS_SESSION_TOKEN", "TOKEN");
        assert_eq!(with_token.aws_session_token(), Some("TOKEN"));
    }

    #[test]
    fn aws_session_token_is_independent_of_key_pair() {
        // Temporary credentials always arrive as a triple, but the accessor
        // must not depend on the key pair being present.
        let env = Env::for_test("/tmp/x").with_var("AWS_SESSION_TOKEN", "TOKEN");
        assert_eq!(env.aws_credentials(), None);
        assert_eq!(env.aws_session_token(), Some("TOKEN"));
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
    #[test]
    fn the_aws_files_default_to_the_dot_aws_directory() {
        let env = Env::for_test("/home/u");
        assert_eq!(
            env.aws_credentials_file(),
            Some(PathBuf::from("/home/u/.aws/credentials"))
        );
        assert_eq!(
            env.aws_config_file(),
            Some(PathBuf::from("/home/u/.aws/config"))
        );
    }

    #[test]
    fn an_absolute_override_relocates_the_aws_file() {
        let env = Env::for_test("/home/u")
            .with_var("AWS_SHARED_CREDENTIALS_FILE", "/etc/aws/creds")
            .with_var("AWS_CONFIG_FILE", "/etc/aws/conf");
        assert_eq!(
            env.aws_credentials_file(),
            Some(PathBuf::from("/etc/aws/creds"))
        );
        assert_eq!(env.aws_config_file(), Some(PathBuf::from("/etc/aws/conf")));
    }

    #[test]
    fn a_home_relative_override_expands_against_the_home_directory() {
        // The AWS CLI accepts this form, and a value read from a file rather
        // than typed in a shell arrives unexpanded.
        let env = Env::for_test("/home/u")
            .with_var("AWS_SHARED_CREDENTIALS_FILE", "~/work/creds")
            .with_var("AWS_CONFIG_FILE", "${HOME}/work/conf");
        assert_eq!(
            env.aws_credentials_file(),
            Some(PathBuf::from("/home/u/work/creds"))
        );
        assert_eq!(
            env.aws_config_file(),
            Some(PathBuf::from("/home/u/work/conf"))
        );
    }

    #[test]
    fn a_bare_tilde_override_is_the_home_directory() {
        let env = Env::for_test("/home/u").with_var("AWS_CONFIG_FILE", "~");
        assert_eq!(env.aws_config_file(), Some(PathBuf::from("/home/u")));
    }

    #[test]
    fn an_exported_but_blank_override_means_no_file() {
        // Blanking the variable is how a user keeps a tool away from their
        // keys. Falling back to the default would read them anyway.
        let env = Env::for_test("/home/u")
            .with_var("AWS_SHARED_CREDENTIALS_FILE", "")
            .with_var("AWS_CONFIG_FILE", "   ");
        assert_eq!(env.aws_credentials_file(), None);
        assert_eq!(env.aws_config_file(), None);
    }

    #[test]
    fn without_a_home_directory_only_an_override_names_an_aws_file() {
        assert_eq!(Env::empty().aws_credentials_file(), None);
        assert_eq!(
            Env::empty()
                .with_var("AWS_CONFIG_FILE", "/etc/aws/conf")
                .aws_config_file(),
            Some(PathBuf::from("/etc/aws/conf"))
        );
    }
}
