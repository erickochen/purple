//! Form baselines and dirty-state detection. Implements `impl App` continuation
//! with capture/compare logic for every form kind (host, tunnel, snippet,
//! provider) plus the mtime helpers that detect external config changes.

use crate::app::App;
use crate::app::Screen;
use crate::app::reload_state::{
    config_changed, get_mtime, snapshot_include_dir_mtimes, snapshot_include_mtimes,
};
use crate::app::{HostForm, SnippetForm, TunnelForm};
use crate::snippet::Snippet;
use crate::ssh_config::model::PatternEntry;
use crate::tunnel::TunnelRule;

/// Baseline snapshot of host form content for dirty-check on Esc.
#[derive(Clone)]
pub struct FormBaseline {
    pub alias: String,
    pub hostname: String,
    pub user: String,
    pub port: String,
    pub identity_file: String,
    pub proxy_jump: String,
    pub askpass: String,
    pub vault_ssh: String,
    pub vault_addr: String,
    pub tags: String,
}

/// Baseline snapshot of tunnel form content for dirty-check on Esc.
#[derive(Clone)]
pub struct TunnelFormBaseline {
    pub tunnel_type: crate::tunnel::TunnelType,
    pub bind_port: String,
    pub remote_host: String,
    pub remote_port: String,
    pub bind_address: String,
}

/// Baseline snapshot of snippet form content for dirty-check on Esc.
#[derive(Clone)]
pub struct SnippetFormBaseline {
    pub name: String,
    pub command: String,
    pub description: String,
    pub interactive: bool,
    pub default_hosts: Vec<String>,
}

/// Baseline snapshot of provider form content for dirty-check on Esc.
#[derive(Clone)]
pub struct ProviderFormBaseline {
    pub url: String,
    pub token: String,
    pub profile: String,
    pub project: String,
    pub compartment: String,
    pub regions: String,
    pub filter: String,
    pub alias_prefix: String,
    pub user: String,
    pub identity_file: String,
    pub verify_tls: bool,
    pub auto_sync: bool,
    pub vault_role: String,
    pub vault_addr: String,
    pub ssm: crate::providers::aws_ssm::SsmMode,
}

impl App {
    /// Clear form mtime state (call on form cancel or successful submit).
    pub fn clear_form_mtime(&mut self) {
        self.conflict.clear_form_mtimes();
    }

    /// Capture config and Include file mtimes when opening a host form.
    pub fn capture_form_mtime(&mut self) {
        self.conflict.form_mtime = get_mtime(&self.reload.config_path);
        self.conflict.form_include_mtimes = snapshot_include_mtimes(&self.hosts_state.ssh_config);
        self.conflict.form_include_dir_mtimes =
            snapshot_include_dir_mtimes(&self.env, &self.hosts_state.ssh_config);
    }

    /// Capture ~/.purple/providers mtime when opening a provider form.
    pub fn capture_provider_form_mtime(&mut self) {
        let path = self
            .env
            .paths()
            .map(crate::runtime::env::Paths::providers_config);
        self.conflict.provider_form_mtime = path.as_ref().and_then(|p| get_mtime(p));
    }

    /// Capture a baseline snapshot of the host form for dirty-check on Esc.
    pub fn capture_form_baseline(&mut self) {
        self.forms.host_baseline = Some(FormBaseline {
            alias: self.forms.host.alias.clone(),
            hostname: self.forms.host.hostname.clone(),
            user: self.forms.host.user.clone(),
            port: self.forms.host.port.clone(),
            identity_file: self.forms.host.identity_file.clone(),
            proxy_jump: self.forms.host.proxy_jump.clone(),
            askpass: self.forms.host.askpass.clone(),
            vault_ssh: self.forms.host.vault_ssh.clone(),
            vault_addr: self.forms.host.vault_addr.clone(),
            tags: self.forms.host.tags.clone(),
        });
    }

    /// Check if the host form has been modified since baseline was captured.
    pub fn host_form_is_dirty(&self) -> bool {
        self.forms.host_form_is_dirty()
    }

    /// Tear down host form state and return to the host list. Flush runs
    /// last because `flush_pending_vault_write` no-ops while a form is open.
    pub fn close_host_form(&mut self) {
        self.close_host_form_inner(None);
    }

    /// Close the host form and select the just-saved host. Use after a
    /// successful submit.
    pub fn close_host_form_after_save(&mut self, target_alias: &str) {
        self.close_host_form_inner(Some(target_alias));
    }

    fn close_host_form_inner(&mut self, select: Option<&str>) {
        log::debug!("[purple] close_host_form select={:?}", select);
        self.clear_form_mtime();
        self.forms.host_baseline = None;
        self.set_screen(Screen::HostList);
        if let Some(alias) = select {
            self.select_host_by_alias(alias);
        }
        self.flush_pending_vault_write();
    }

    /// Tear down provider form state and return to the providers list. Same
    /// shape as `close_host_form`; provider forms have no per-save selection.
    pub fn close_provider_form(&mut self) {
        log::debug!("[purple] close_provider_form");
        self.clear_form_mtime();
        self.providers.form_baseline = None;
        self.set_screen(Screen::Providers);
        self.flush_pending_vault_write();
    }

    /// Tear down tunnel form state and return to the caller's screen. The
    /// return target varies (host detail overlay, tunnels overview, picker),
    /// so the caller passes it.
    pub fn close_tunnel_form(&mut self, return_to: Screen) {
        log::debug!(
            "[purple] close_tunnel_form return_to={:?}",
            std::mem::discriminant(&return_to)
        );
        self.clear_form_mtime();
        self.tunnels.form_baseline = None;
        self.set_screen(return_to);
    }

    /// Tear down snippet form state and return to the snippet picker for the
    /// given targets. Snippet forms intentionally skip clear_form_mtime; no
    /// mtime is captured on snippet form open.
    pub fn close_snippet_form(&mut self, target_aliases: Vec<String>) {
        log::debug!(
            "[purple] close_snippet_form aliases={}",
            target_aliases.len()
        );
        self.snippets.form_baseline = None;
        self.snippets.set_flow_targets(target_aliases);
        self.snippets.set_form_editing(None);
        self.set_screen(Screen::SnippetPicker);
    }

    /// Open a blank host add form. Mirror is `close_host_form`.
    pub fn open_host_add_form(&mut self) {
        log::debug!("[purple] open_host_add_form");
        self.forms.host = HostForm::new();
        self.set_screen(Screen::AddHost);
        self.capture_form_mtime();
        self.capture_form_baseline();
    }

    /// Open a blank pattern add form. Shares Screen::AddHost; the form
    /// constructor distinguishes pattern vs host entries internally.
    pub fn open_host_pattern_add_form(&mut self) {
        log::debug!("[purple] open_host_pattern_add_form");
        self.forms.host = HostForm::new_pattern();
        self.set_screen(Screen::AddHost);
        self.capture_form_mtime();
        self.capture_form_baseline();
    }

    /// Open the host edit form for `host`. Returns false (without changing
    /// screen) if the host lives in an Include file or its raw entry cannot
    /// be located. The caller computes `stale_hint` because it is derived
    /// from handler-local provider-display logic.
    pub fn open_host_edit_form(
        &mut self,
        host: crate::ssh_config::model::HostEntry,
        stale_hint: Option<String>,
    ) -> bool {
        if let Some(ref source) = host.source_file {
            self.notify_error(crate::messages::included_host_lives_in(
                &host.alias,
                &source.display(),
            ));
            return false;
        }
        // Load raw entry (no pattern inheritance) so inherited values do not
        // appear as editable own values.
        let raw = match self.hosts_state.ssh_config.raw_host_entry(&host.alias) {
            Some(entry) => entry,
            None => {
                self.notify_warning(crate::i18n::text(crate::messages::HOST_NOT_FOUND_IN_CONFIG));
                return false;
            }
        };
        let inherited = self.hosts_state.ssh_config.inherited_hints(&host.alias);
        log::debug!("[purple] open_host_edit_form alias={}", host.alias);
        self.forms.host = HostForm::from_entry(&raw, inherited);
        if let Some(hint) = stale_hint {
            self.notify_warning(crate::messages::stale_host(&hint));
        }
        self.set_screen(Screen::EditHost { alias: host.alias });
        self.capture_form_mtime();
        self.capture_form_baseline();
        true
    }

    /// Open an edit form for an existing pattern entry.
    pub fn open_host_pattern_edit_form(&mut self, pattern: &PatternEntry) {
        log::debug!(
            "[purple] open_host_pattern_edit_form pattern={}",
            pattern.pattern
        );
        self.forms.host = HostForm::from_pattern_entry(pattern);
        self.set_screen(Screen::EditHost {
            alias: pattern.pattern.clone(),
        });
        self.capture_form_mtime();
        self.capture_form_baseline();
    }

    /// Open a blank tunnel add form scoped to `alias`. The alias is set on
    /// the screen variant so submit/cancel return to the right host context.
    pub fn open_tunnel_add_form(&mut self, alias: String) {
        log::debug!("[purple] open_tunnel_add_form alias={}", alias);
        self.tunnels.form = TunnelForm::new();
        self.set_screen(Screen::TunnelForm {
            alias,
            editing: None,
        });
        self.capture_form_mtime();
        self.capture_tunnel_form_baseline();
    }

    /// Open an edit form for an existing tunnel rule. `editing` is the index
    /// into `tunnels.list` that the save path mutates.
    pub fn open_tunnel_edit_form(&mut self, alias: String, rule: &TunnelRule, editing: usize) {
        log::debug!(
            "[purple] open_tunnel_edit_form alias={} editing={}",
            alias,
            editing
        );
        self.tunnels.form = TunnelForm::from_rule(rule);
        self.set_screen(Screen::TunnelForm {
            alias,
            editing: Some(editing),
        });
        self.capture_form_mtime();
        self.capture_tunnel_form_baseline();
    }

    /// Open a blank snippet add form scoped to the given target aliases.
    /// No mtime capture (snippet forms have no mtime tracking).
    pub fn open_snippet_add_form(&mut self, target_aliases: Vec<String>) {
        log::debug!(
            "[purple] open_snippet_add_form aliases={}",
            target_aliases.len()
        );
        self.snippets.form = SnippetForm::new();
        self.snippets.set_flow_targets(target_aliases);
        self.snippets.set_form_editing(None);
        self.set_screen(Screen::SnippetForm);
        self.capture_snippet_form_baseline();
    }

    /// Open an edit form for an existing snippet. `editing` is the index
    /// into the snippet store that the save path mutates.
    pub fn open_snippet_edit_form(
        &mut self,
        snippet: &Snippet,
        target_aliases: Vec<String>,
        editing: usize,
    ) {
        log::debug!(
            "[purple] open_snippet_edit_form name={} editing={}",
            snippet.name,
            editing
        );
        self.snippets.form = SnippetForm::from_snippet(snippet);
        self.snippets.set_flow_targets(target_aliases);
        self.snippets.set_form_editing(Some(editing));
        self.set_screen(Screen::SnippetForm);
        self.capture_snippet_form_baseline();
    }

    /// Open a provider form for `id`, populating defaults for new configs
    /// or existing data for edits. When `id.label` is `Some("")` the form
    /// opens in label-entry mode so the user types the label first.
    pub fn open_provider_form(&mut self, id: crate::providers::config::ProviderConfigId) {
        let provider_impl = crate::providers::get_provider(id.provider.as_str());
        let short_label = provider_impl
            .as_ref()
            .map(|p| p.short_label().to_string())
            .unwrap_or_else(|| id.provider.clone());
        let existing_section = self.providers.config.section_by_id(&id).cloned();
        let label_entry = existing_section.is_none() && id.label.as_deref() == Some("");
        let provider_first_field =
            crate::app::ProviderFormField::fields_for(id.provider.as_str())[0];
        let first_field = if label_entry {
            crate::app::ProviderFormField::Label
        } else {
            provider_first_field
        };
        log::debug!(
            "[purple] open_provider_form provider={} label_entry={}",
            id.provider,
            label_entry
        );

        self.providers.form = if let Some(section) = existing_section {
            let cursor_pos = match first_field {
                crate::app::ProviderFormField::Url => section.url.chars().count(),
                crate::app::ProviderFormField::Token => section.token.chars().count(),
                _ => 0,
            };
            crate::app::ProviderFormFields {
                label: String::new(),
                label_entry: false,
                url: section.url.clone(),
                token: section.token.clone(),
                profile: section.profile.clone(),
                project: section.project.clone(),
                compartment: section.compartment.clone(),
                regions: section.regions.clone(),
                filter: section.filter.clone(),
                alias_prefix: section.alias_prefix.clone(),
                user: section.user.clone(),
                identity_file: section.identity_file.clone(),
                verify_tls: section.verify_tls,
                auto_sync: section.auto_sync,
                vault_role: section.vault_role.clone(),
                ssm: section.ssm,
                vault_addr: section.vault_addr.clone(),
                focused_field: first_field,
                cursor_pos,
                expanded: true,
            }
        } else {
            // New config: derive a sensible default alias_prefix. For a labeled
            // config with a known label, suggest `<short>-<label>` (e.g. `do-work`);
            // when the label is still empty (label-entry mode), fall back to the
            // bare short prefix so the field has a stable value the user can edit.
            let default_prefix = match id.label.as_deref() {
                Some("") | None => short_label.clone(),
                Some(l) => format!("{}-{}", short_label, l),
            };
            crate::app::ProviderFormFields {
                label: String::new(),
                label_entry,
                url: String::new(),
                token: String::new(),
                profile: String::new(),
                project: String::new(),
                compartment: String::new(),
                regions: String::new(),
                filter: String::new(),
                alias_prefix: default_prefix,
                user: "root".to_string(),
                identity_file: String::new(),
                verify_tls: true,
                auto_sync: id
                    .kind()
                    .is_none_or(crate::providers::ProviderKind::default_auto_sync),
                vault_role: String::new(),
                ssm: crate::providers::aws_ssm::SsmMode::default(),
                vault_addr: String::new(),
                focused_field: first_field,
                cursor_pos: 0,
                expanded: false,
            }
        };
        self.set_screen(Screen::ProviderForm { id });
        self.capture_provider_form_mtime();
        self.capture_provider_form_baseline();
    }

    /// Capture a baseline snapshot of the tunnel form for dirty-check on Esc.
    pub fn capture_tunnel_form_baseline(&mut self) {
        self.tunnels.form_baseline = Some(TunnelFormBaseline {
            tunnel_type: self.tunnels.form.tunnel_type,
            bind_port: self.tunnels.form.bind_port.clone(),
            remote_host: self.tunnels.form.remote_host.clone(),
            remote_port: self.tunnels.form.remote_port.clone(),
            bind_address: self.tunnels.form.bind_address.clone(),
        });
    }

    /// Check if the tunnel form has been modified since baseline was captured.
    pub fn tunnel_form_is_dirty(&self) -> bool {
        self.tunnels.form_is_dirty()
    }

    /// Capture a baseline snapshot of the snippet form for dirty-check on Esc.
    pub fn capture_snippet_form_baseline(&mut self) {
        self.snippets.form_baseline = Some(SnippetFormBaseline {
            name: self.snippets.form.name.clone(),
            command: self.snippets.form.command.clone(),
            description: self.snippets.form.description.clone(),
            interactive: self.snippets.form.interactive,
            default_hosts: self.snippets.form.default_hosts.clone(),
        });
    }

    /// Check if the snippet form has been modified since baseline was captured.
    pub fn snippet_form_is_dirty(&self) -> bool {
        self.snippets.form_is_dirty()
    }

    /// Capture a baseline snapshot of the provider form for dirty-check on Esc.
    pub fn capture_provider_form_baseline(&mut self) {
        self.providers.form_baseline = Some(ProviderFormBaseline {
            url: self.providers.form.url.clone(),
            token: self.providers.form.token.clone(),
            profile: self.providers.form.profile.clone(),
            project: self.providers.form.project.clone(),
            compartment: self.providers.form.compartment.clone(),
            regions: self.providers.form.regions.clone(),
            filter: self.providers.form.filter.clone(),
            alias_prefix: self.providers.form.alias_prefix.clone(),
            user: self.providers.form.user.clone(),
            identity_file: self.providers.form.identity_file.clone(),
            verify_tls: self.providers.form.verify_tls,
            auto_sync: self.providers.form.auto_sync,
            vault_role: self.providers.form.vault_role.clone(),
            vault_addr: self.providers.form.vault_addr.clone(),
            ssm: self.providers.form.ssm,
        });
    }

    /// Check if the provider form has been modified since baseline was captured.
    pub fn provider_form_is_dirty(&self) -> bool {
        self.providers.form_is_dirty()
    }

    /// Check if config or any Include file/directory has changed since the form was opened.
    pub fn config_changed_since_form_open(&self) -> bool {
        config_changed(&self.conflict, &self.reload.config_path)
    }

    /// Check if ~/.purple/providers has changed since the provider form was opened.
    pub fn provider_config_changed_since_form_open(&self) -> bool {
        let path = self
            .env
            .paths()
            .map(crate::runtime::env::Paths::providers_config);
        let current_mtime = path.as_ref().and_then(|p| get_mtime(p));
        self.conflict.provider_form_mtime != current_mtime
    }
}
