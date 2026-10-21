//! State behind the in-TUI password prompt and the host key trust dialog.
//!
//! A background ssh (key push, file browser listing) can never reach the
//! terminal, so a host that wants a password or is not in `known_hosts`
//! yet asks through the TUI instead. Both dialogs carry a `PendingRetry`
//! that names the operation to run again once the user has answered.

use std::collections::HashMap;

use super::App;

/// Longest password the prompt accepts. Far above any real password and
/// keeps a held-down key from growing the buffer without bound.
pub const PASSWORD_MAX_CHARS: usize = 512;

/// The operation to run again after the user answered a dialog. One
/// variant per flow; each re-queues through the channel that flow owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingRetry {
    /// Push the key at `key_path` to this one host again. The key is named
    /// by its path, not its position: a successful push rebuilds the key
    /// list, and an index into the old list could then point at another key.
    KeyPush { key_path: String, alias: String },
    /// List `path` on the open file browser again. An empty path means the
    /// remote home lookup itself failed and runs first.
    FileBrowserListing { alias: String, path: String },
    /// Start this host's tunnel again.
    Tunnel { alias: String },
}

impl PendingRetry {
    pub fn alias(&self) -> &str {
        match self {
            PendingRetry::KeyPush { alias, .. }
            | PendingRetry::FileBrowserListing { alias, .. }
            | PendingRetry::Tunnel { alias } => alias,
        }
    }

    /// Short name for the log.
    pub fn kind_label(&self) -> &'static str {
        match self {
            PendingRetry::KeyPush { .. } => "key_push",
            PendingRetry::FileBrowserListing { .. } => "file_browser",
            PendingRetry::Tunnel { .. } => "tunnel",
        }
    }

    /// The flow a security key PIN typed for this retry belongs to.
    pub fn pin_flow(&self) -> crate::security_key::PinFlow {
        match self {
            PendingRetry::KeyPush { .. } => crate::security_key::PinFlow::KeyPush,
            PendingRetry::FileBrowserListing { .. } => crate::security_key::PinFlow::FileBrowser,
            PendingRetry::Tunnel { .. } => crate::security_key::PinFlow::Tunnel,
        }
    }
}

/// Which field of the password prompt has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PasswordPromptField {
    #[default]
    Password,
    Remember,
}

impl PasswordPromptField {
    pub fn next(self) -> Self {
        match self {
            PasswordPromptField::Password => PasswordPromptField::Remember,
            PasswordPromptField::Remember => PasswordPromptField::Password,
        }
    }
}

/// Payload of `Screen::PasswordPrompt`. Lives beside the screen instead of
/// inside the variant so the secret is never cloned on every render.
#[derive(Clone, PartialEq, Eq)]
pub struct PasswordPromptState {
    pub alias: String,
    /// True when the alias already resolves to `keychain`, so a "remember"
    /// submit stores the password without writing the config again.
    pub source_is_keychain: bool,
    pub input: String,
    pub remember: bool,
    pub focus: PasswordPromptField,
    pub retry: PendingRetry,
    /// Set when the prompt asks for a security key PIN instead: the key file
    /// it belongs to. A PIN is never stored in the keychain.
    pub security_key: Option<String>,
    /// Alias of the per-host tunnel list the prompt opened over, which it
    /// returns to.
    pub return_to_tunnel_list: Option<String>,
}

// Hand-written so a stray `{:?}` can never print the typed password. Shows
// whether one has been entered, never what it is.
impl std::fmt::Debug for PasswordPromptState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PasswordPromptState")
            .field("alias", &self.alias)
            .field("source_is_keychain", &self.source_is_keychain)
            .field("input_len", &self.input.chars().count())
            .field("remember", &self.remember)
            .field("focus", &self.focus)
            .field("retry", &self.retry)
            .field("security_key", &self.security_key)
            .field("return_to_tunnel_list", &self.return_to_tunnel_list)
            .finish()
    }
}

impl PasswordPromptState {
    pub fn new(alias: String, source_is_keychain: bool, retry: PendingRetry) -> Self {
        Self {
            alias,
            source_is_keychain,
            input: String::new(),
            remember: true,
            focus: PasswordPromptField::Password,
            retry,
            security_key: None,
            return_to_tunnel_list: None,
        }
    }

    /// The prompt for the PIN of the security key at `key_path`.
    pub fn security_key_pin(alias: String, key_path: String, retry: PendingRetry) -> Self {
        Self {
            remember: false,
            security_key: Some(key_path),
            ..Self::new(alias, false, retry)
        }
    }
}

/// Passwords typed with "remember" off, keyed by alias, kept until purple
/// exits. Every later background operation on the alias uses the entry
/// through the one-shot askpass channel. Nothing here reaches disk.
pub type SessionPasswords = HashMap<String, String>;

/// True when a password prompt may open for a host with this source: no
/// source at all, or the OS keychain. Any other source failing means the
/// source itself did not deliver, which a typed password cannot fix.
pub fn source_allows_prompt(source: Option<&str>) -> bool {
    matches!(source, None | Some("keychain"))
}

impl App {
    /// The password source a background ssh for `alias` resolves to: the
    /// host's own `# purple:askpass`, else the global default. Mirrors the
    /// lookup the askpass subprocess performs.
    pub(crate) fn askpass_source_for(&self, alias: &str) -> Option<String> {
        self.hosts_state
            .list
            .iter()
            .find(|h| h.alias == alias)
            .and_then(|h| h.askpass.clone())
            .or_else(|| crate::preferences::load_askpass_default(self.env.paths()))
    }

    /// True when ssh's refusal can be attributed to `alias` itself rather
    /// than to a jump host on the way there. Without a bastion there is only
    /// one host that could have refused, so the name ssh printed does not
    /// matter. With one, ssh names the hop that refused and only the
    /// target's own refusal is ours to answer: a dialog for a bastion would
    /// write that bastion's password onto the target. A `ProxyCommand`
    /// counts as a bastion as much as a `ProxyJump` does, since it reaches
    /// a machine of its own that purple cannot name.
    pub(crate) fn refusal_is_this_host(&self, alias: &str, named: Option<&str>) -> bool {
        let Some(host) = self.hosts_state.list.iter().find(|h| h.alias == alias) else {
            // Not in the list, so there is no route to read. Treat it as
            // the plain single-hop case.
            return true;
        };
        if host.proxy_jump.trim().is_empty() && !host.has_proxy_command {
            return true;
        }
        // Behind a jump host, so the name decides. A refusal naming nobody
        // could have come from either end.
        let Some(named) = named else {
            return false;
        };
        let named = crate::connection::bare_host(named);
        let target = if host.hostname.trim().is_empty() {
            alias
        } else {
            host.hostname.trim()
        };
        named.eq_ignore_ascii_case(target)
    }

    /// True while a dialog is waiting for an answer. A second one must not
    /// open over it: the retry the first was holding would be lost with no
    /// trace, and the user would answer a question they never saw asked.
    pub(crate) fn dialog_open(&self) -> bool {
        self.password_prompt.is_some()
            || matches!(
                self.screen,
                super::Screen::PasswordPrompt | super::Screen::ConfirmHostKeyTrust { .. }
            )
    }

    /// True when a background answer may take the screen. No other dialog
    /// may be waiting. The user has to be on a page a dialog returns to
    /// rather than inside a form, a picker or an overlay. Every typing mode
    /// that lives inside those pages has to be closed as well: the host
    /// list's search, its tag field and the jump bar. None of the three
    /// changes `self.screen`, so the screen alone does not rule them out,
    /// and the jump bar even takes every key ahead of the screen match.
    /// Opening over one of them would send the rest of what they type into
    /// the password field, and the Enter that ends a search would submit
    /// the fragment as a password. Anywhere else the question waits until
    /// they come back.
    pub(crate) fn can_open_dialog(&self) -> bool {
        !self.dialog_open()
            && self.search.query().is_none()
            && self.tags.input().is_none()
            && self.jump.is_none()
            && self.tunnels.pending_delete().is_none()
            && self.snippets.pending_delete().is_none()
            && !self.file_browser_is_busy()
            && matches!(
                self.screen,
                super::Screen::HostList | super::Screen::FileBrowser { .. }
            )
    }

    /// True while the file browser has a question or a transfer of its own
    /// on screen. All three render inside `Screen::FileBrowser`, so a
    /// dialog opening over them would take the answer meant for them.
    fn file_browser_is_busy(&self) -> bool {
        self.file_browser_session.as_ref().is_some_and(|fb| {
            fb.confirm_copy.is_some() || fb.transferring.is_some() || fb.transfer_error.is_some()
        })
    }

    /// Drop a prompt whose screen moved on without it. `dialog_open` reads
    /// the state rather than the screen, so a stranded one would refuse
    /// every later question for the rest of the session. Returns true when
    /// one was cleared.
    pub(crate) fn drop_stranded_password_prompt(&mut self) -> bool {
        if self.password_prompt.is_none() || matches!(self.screen, super::Screen::PasswordPrompt) {
            return false;
        }
        let alias = self
            .password_prompt
            .take()
            .map(|s| s.alias)
            .unwrap_or_default();
        log::warn!("[purple] password prompt: dropped stranded state for alias={alias}");
        self.notify_warning(crate::messages::askpass::prompt_cancelled(&alias));
        true
    }

    /// True when purple already handed `alias` a password and the server
    /// still refused: either a source resolves for it or a password typed
    /// this session is on file. The prompt then says so, instead of
    /// reopening as though nothing had been tried.
    pub(crate) fn password_was_supplied_for(&self, alias: &str) -> bool {
        self.askpass_source_for(alias).is_some() || self.session_passwords.contains_key(alias)
    }

    /// Open the password prompt for `alias` with `retry` queued behind it.
    /// A password the server just rejected is dropped first, so the retry
    /// after this prompt carries the new one. `supplied_before` says whether
    /// an earlier answer was already refused, which the toast reports.
    pub(crate) fn open_password_prompt(
        &mut self,
        alias: &str,
        retry: PendingRetry,
        supplied_before: bool,
    ) {
        self.session_passwords.remove(alias);
        let source_is_keychain = self.askpass_source_for(alias).as_deref() == Some("keychain");
        log::debug!(
            "[purple] password prompt: open alias={} keychain_source={} supplied_before={} retry={}",
            alias,
            source_is_keychain,
            supplied_before,
            retry.kind_label()
        );
        self.password_prompt = Some(PasswordPromptState::new(
            alias.to_string(),
            source_is_keychain,
            retry,
        ));
        self.set_screen(super::Screen::PasswordPrompt);
        if supplied_before {
            self.notify_warning(crate::messages::askpass::password_rejected(alias));
        }
    }

    /// The password typed for `alias` this session, if the user chose not to
    /// store it. Handed to every later background ssh for that host through
    /// the one-shot askpass channel, so a host is asked once and not once
    /// per directory.
    pub(crate) fn session_password_for(&self, alias: &str) -> Option<String> {
        self.session_passwords.get(alias).cloned()
    }

    /// Build the SSH context a background operation on `alias` runs with:
    /// the caller's resolved askpass source, any password typed this
    /// session, the Bitwarden token and whether a tunnel is already open.
    /// `trust_new_host_key` is false; only a retry after the trust dialog
    /// sets it.
    pub(crate) fn ssh_context_for(
        &self,
        alias: impl Into<String>,
        askpass: Option<String>,
    ) -> crate::ssh_context::OwnedSshContext {
        let alias = alias.into();
        crate::ssh_context::OwnedSshContext {
            session_password: self.session_password_for(&alias),
            has_tunnel: self.tunnels.active_contains(&alias),
            alias,
            config_path: self.reload.config_path().to_path_buf(),
            askpass,
            bw_session: self.bw_session.clone(),
            trust_new_host_key: false,
            security_key: self.security_key_confirmed_run(),
            env: std::sync::Arc::clone(&self.env),
        }
    }

    /// `ssh_context_for` for a file browser listing, which may ask for a
    /// missing security key PIN and carries the trial PIN typed for it.
    pub(crate) fn file_browser_ssh_context(
        &self,
        alias: impl Into<String>,
        askpass: Option<String>,
    ) -> crate::ssh_context::OwnedSshContext {
        let mut ctx = self.ssh_context_for(alias, askpass);
        ctx.security_key =
            self.security_key_run_for(&ctx.alias, crate::security_key::PinFlow::FileBrowser);
        ctx
    }

    /// Open the trust dialog for `alias` with `retry` queued behind it. The
    /// dialog names the configured HostName, falling back to the alias.
    pub(crate) fn open_host_key_trust(&mut self, alias: &str, retry: PendingRetry) {
        let hostname = self
            .hosts_state
            .list
            .iter()
            .find(|h| h.alias == alias)
            .map(|h| h.hostname.clone())
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| alias.to_string());
        log::debug!(
            "[purple] host key trust: open alias={} hostname={}",
            alias,
            hostname
        );
        self.set_screen(super::Screen::ConfirmHostKeyTrust {
            alias: alias.to_string(),
            hostname,
            retry,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Screen;
    use crate::ssh_config::model::SshConfigFile;

    fn app_with(config: &str) -> App {
        let scratch = tempfile::tempdir().expect("tempdir").keep();
        let cfg = SshConfigFile {
            elements: SshConfigFile::parse_content(config),
            path: scratch.join("config"),
            crlf: false,
            bom: false,
        };
        App::new(cfg)
    }

    #[test]
    fn source_allows_prompt_only_for_none_and_keychain() {
        assert!(source_allows_prompt(None));
        assert!(source_allows_prompt(Some("keychain")));
        for other in [
            "bw:item",
            "op://v/i/f",
            "proton:v/i/f",
            "pass:x",
            "vault:s#p",
            "my-cmd %h",
        ] {
            assert!(!source_allows_prompt(Some(other)), "{other}");
        }
    }

    #[test]
    fn field_focus_cycles_between_the_two_fields() {
        assert_eq!(
            PasswordPromptField::Password.next(),
            PasswordPromptField::Remember
        );
        assert_eq!(
            PasswordPromptField::Remember.next(),
            PasswordPromptField::Password
        );
    }

    #[test]
    fn new_state_defaults_to_remember_on_and_password_focus() {
        let s = PasswordPromptState::new(
            "h".into(),
            false,
            PendingRetry::KeyPush {
                key_path: "~/.ssh/id_test".into(),
                alias: "h".into(),
            },
        );
        assert!(s.remember);
        assert_eq!(s.focus, PasswordPromptField::Password);
        assert!(s.input.is_empty());
    }

    #[test]
    fn pending_retry_alias_names_the_host() {
        let a = PendingRetry::KeyPush {
            key_path: "~/.ssh/id_test".into(),
            alias: "web".into(),
        };
        let b = PendingRetry::FileBrowserListing {
            alias: "db".into(),
            path: "/".into(),
        };
        assert_eq!(a.alias(), "web");
        assert_eq!(b.alias(), "db");
    }

    #[test]
    fn askpass_source_prefers_the_host_comment() {
        let app = app_with("Host h\n  HostName 1.1.1.1\n  # purple:askpass bw:item\n");
        assert_eq!(app.askpass_source_for("h").as_deref(), Some("bw:item"));
        assert_eq!(app.askpass_source_for("missing"), None);
    }

    #[test]
    fn askpass_source_falls_back_to_the_global_default() {
        let app = app_with("Host h\n  HostName 1.1.1.1\n");
        crate::preferences::save_askpass_default(app.env().paths(), "keychain").unwrap();
        assert_eq!(app.askpass_source_for("h").as_deref(), Some("keychain"));
    }

    #[test]
    fn open_password_prompt_sets_state_screen_and_drops_rejected_password() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n  # purple:askpass keychain\n");
        app.session_passwords
            .insert("h".to_string(), "wrong".to_string());
        let retry = PendingRetry::KeyPush {
            key_path: "~/.ssh/id_test".into(),
            alias: "h".into(),
        };
        app.open_password_prompt("h", retry.clone(), false);
        assert_eq!(app.screen, Screen::PasswordPrompt);
        let state = app.password_prompt.as_ref().expect("prompt state");
        assert_eq!(state.alias, "h");
        assert!(state.source_is_keychain);
        assert_eq!(state.retry, retry);
        assert!(
            !app.session_passwords.contains_key("h"),
            "a rejected session password must not survive into the retry"
        );
    }

    #[test]
    fn a_second_dialog_is_seen_as_already_open() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        assert!(!app.dialog_open());
        app.open_password_prompt(
            "h",
            PendingRetry::KeyPush {
                key_path: "~/.ssh/id_test".into(),
                alias: "h".into(),
            },
            false,
        );
        assert!(app.dialog_open());
        app.password_prompt = None;
        app.screen = Screen::HostList;
        app.open_host_key_trust(
            "h",
            PendingRetry::FileBrowserListing {
                alias: "h".into(),
                path: "/".into(),
            },
        );
        assert!(app.dialog_open());
    }

    #[test]
    fn a_reopened_prompt_says_the_last_password_was_refused() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        app.session_passwords
            .insert("h".to_string(), "wrong".to_string());
        assert!(app.password_was_supplied_for("h"));
        let supplied = app.password_was_supplied_for("h");
        app.open_password_prompt(
            "h",
            PendingRetry::KeyPush {
                key_path: "~/.ssh/id_test".into(),
                alias: "h".into(),
            },
            supplied,
        );
        let toast = app.status_center.toast().expect("toast");
        assert!(toast.text.contains('h'), "got: {}", toast.text);
        // The refused password is gone, so the retry carries the new one.
        assert!(!app.session_passwords.contains_key("h"));
    }

    #[test]
    fn a_keychain_source_counts_as_a_password_already_supplied() {
        let app = app_with("Host h\n  HostName 1.1.1.1\n  # purple:askpass keychain\n");
        assert!(app.password_was_supplied_for("h"));
        let bare = app_with("Host h\n  HostName 1.1.1.1\n");
        assert!(!bare.password_was_supplied_for("h"));
    }

    #[test]
    fn ssh_context_for_carries_the_session_password_and_defaults_to_strict_checking() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        app.session_passwords
            .insert("h".to_string(), "hunter2".to_string());
        let ctx = app.ssh_context_for("h", Some("keychain".to_string()));
        assert_eq!(ctx.alias, "h");
        assert_eq!(ctx.askpass.as_deref(), Some("keychain"));
        assert_eq!(ctx.session_password.as_deref(), Some("hunter2"));
        assert!(!ctx.trust_new_host_key);
        assert!(!ctx.has_tunnel);
    }

    #[test]
    fn ssh_context_for_leaves_the_password_empty_for_another_host() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        app.session_passwords
            .insert("other".to_string(), "hunter2".to_string());
        assert!(app.ssh_context_for("h", None).session_password.is_none());
    }

    #[test]
    fn open_host_key_trust_names_the_hostname_and_falls_back_to_alias() {
        let mut app = app_with("Host h\n  HostName db.example.com\n");
        let retry = PendingRetry::FileBrowserListing {
            alias: "h".into(),
            path: String::new(),
        };
        app.open_host_key_trust("h", retry.clone());
        assert_eq!(
            app.screen,
            Screen::ConfirmHostKeyTrust {
                alias: "h".into(),
                hostname: "db.example.com".into(),
                retry: retry.clone(),
            }
        );
        app.open_host_key_trust("ghost", retry);
        match &app.screen {
            Screen::ConfirmHostKeyTrust { hostname, .. } => assert_eq!(hostname, "ghost"),
            other => panic!("expected trust dialog, got {other:?}"),
        }
    }

    #[test]
    fn a_proxy_command_counts_as_a_bastion_in_front_of_the_host() {
        // `ProxyCommand ssh -W %h:%p bastion` reaches a second machine just
        // as `ProxyJump bastion` does. A refusal naming nobody could have
        // come from either end, so it is not this host's to answer.
        let app =
            app_with("Host h\n  HostName db.example.com\n  ProxyCommand ssh -W %h:%p bastion\n");
        assert!(!app.refusal_is_this_host("h", None));
        assert!(!app.refusal_is_this_host("h", Some("bastion.example.com")));
        assert!(app.refusal_is_this_host("h", Some("db.example.com")));
    }

    #[test]
    fn a_proxy_command_set_to_none_leaves_the_host_direct() {
        // `ProxyCommand none` is how a wildcard block is opted out of, so it
        // puts no machine in front of this host.
        let app = app_with("Host h\n  HostName db.example.com\n  ProxyCommand none\n");
        assert!(app.refusal_is_this_host("h", None));
    }

    #[test]
    fn a_dialog_waits_while_the_host_list_search_is_open() {
        // The search input lives inside Screen::HostList, so the screen
        // alone does not say the user is free to be asked a question.
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        app.screen = Screen::HostList;
        assert!(app.can_open_dialog());
        app.search.set_query(Some("web".to_string()));
        assert!(
            !app.can_open_dialog(),
            "a half typed search must keep the screen"
        );
        app.search.set_query(None);
        assert!(app.can_open_dialog());
    }

    #[test]
    fn a_dialog_waits_while_the_jump_bar_is_open() {
        // The jump bar takes every key ahead of the screen match, so a
        // dialog opening under it would collect what the user types at it.
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        app.screen = Screen::HostList;
        app.open_jump(crate::app::JumpMode::Hosts);
        assert!(!app.can_open_dialog(), "the jump bar keeps the screen");
    }

    #[test]
    fn a_dialog_waits_while_another_confirm_is_on_screen() {
        // A tunnel or snippet delete confirm renders inside the host list
        // and answers with y or n. Those keys must not reach a password
        // field that appeared under them.
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        app.screen = Screen::HostList;
        app.tunnels.pending_delete = Some(0);
        assert!(!app.can_open_dialog(), "a tunnel confirm keeps the screen");
        app.tunnels.take_pending_delete();
        assert!(app.can_open_dialog());

        app.snippets.pending_delete = Some(0);
        assert!(!app.can_open_dialog(), "a snippet confirm keeps the screen");
        app.snippets.pending_delete = None;
        assert!(app.can_open_dialog());
    }

    #[test]
    fn a_dialog_waits_while_the_tag_input_is_open() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        app.screen = Screen::HostList;
        app.tags.open_tag_input("web".to_string());
        assert!(
            !app.can_open_dialog(),
            "a half typed tag must keep the screen"
        );
    }

    #[test]
    fn a_refusal_is_parsed_out_of_stderr_before_it_is_attributed() {
        // The seam: what ssh printed, through the parser, into the decision
        // whether this host may be asked for a password.
        let app = app_with("Host h\n  HostName target.example.com\n  ProxyJump bastion\n");
        let bastion = "ops@bastion.example.com: Permission denied (publickey,password).\n";
        let named = crate::connection::denied_host(bastion);
        assert_eq!(named, Some("bastion.example.com"));
        assert!(
            !app.refusal_is_this_host("h", named),
            "the jump host's refusal is not the target's to answer"
        );

        let target = "ops@target.example.com: Permission denied (publickey,password).\n";
        let named = crate::connection::denied_host(target);
        assert!(app.refusal_is_this_host("h", named));
    }

    #[test]
    fn a_bracketed_host_from_the_trust_line_matches_the_target() {
        let app = app_with("Host h\n  HostName 10.0.0.1\n  ProxyJump bastion\n");
        let stderr = "No ED25519 host key is known for [10.0.0.1]:2222 and you have requested strict checking.\n";
        let named = crate::connection::unknown_host_key_host(stderr);
        assert_eq!(named, Some("[10.0.0.1]:2222"));
        assert!(app.refusal_is_this_host("h", named));
    }
}
