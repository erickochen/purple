use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::app::ProviderFormBaseline;
use crate::app::forms::ProviderFormFields;
use crate::providers::config::{ProviderConfig, ProviderConfigId};

/// Record of the last sync result for a provider.
#[derive(Debug, Clone)]
pub struct SyncRecord {
    pub timestamp: u64,
    pub message: String,
    pub is_error: bool,
}

impl SyncRecord {
    /// Load sync history from ~/.purple/sync_history.tsv.
    /// Format: provider\ttimestamp\tis_error\tmessage
    pub fn load_all(paths: Option<&crate::runtime::env::Paths>) -> HashMap<String, SyncRecord> {
        let mut map = HashMap::new();
        let Some(path) = paths.map(crate::runtime::env::Paths::sync_history) else {
            return map;
        };
        let Ok(content) = std::fs::read_to_string(&path) else {
            return map;
        };
        for line in content.lines() {
            let parts: Vec<&str> = line.splitn(4, '\t').collect();
            if parts.len() < 4 {
                continue;
            }
            let Some(ts) = parts[1].parse::<u64>().ok() else {
                continue;
            };
            let is_error = parts[2] == "1";
            map.insert(
                parts[0].to_string(),
                SyncRecord {
                    timestamp: ts,
                    message: parts[3].to_string(),
                    is_error,
                },
            );
        }
        map
    }

    /// Save sync history to ~/.purple/sync_history.tsv.
    pub fn save_all(
        history: &HashMap<String, SyncRecord>,
        paths: Option<&crate::runtime::env::Paths>,
    ) {
        if crate::demo_flag::is_demo() {
            return;
        }
        let Some(path) = paths.map(crate::runtime::env::Paths::sync_history) else {
            return;
        };
        let mut lines = Vec::new();
        for (provider, record) in history {
            lines.push(format!(
                "{}\t{}\t{}\t{}",
                provider,
                record.timestamp,
                if record.is_error { "1" } else { "0" },
                record.message
            ));
        }
        if let Err(e) = crate::fs_util::atomic_write(&path, lines.join("\n").as_bytes()) {
            log::warn!(
                "[config] failed to save sync_history.tsv at {}: {e}",
                path.display()
            );
        }
    }

    /// Parse sync history from TSV content string (for demo/test use).
    pub fn load_from_content(content: &str) -> HashMap<String, SyncRecord> {
        let mut map = HashMap::new();
        for line in content.lines() {
            let parts: Vec<&str> = line.splitn(4, '\t').collect();
            if parts.len() < 4 {
                continue;
            }
            let Some(ts) = parts[1].parse::<u64>().ok() else {
                continue;
            };
            let is_error = parts[2] == "1";
            map.insert(
                parts[0].to_string(),
                SyncRecord {
                    timestamp: ts,
                    message: parts[3].to_string(),
                    is_error,
                },
            );
        }
        map
    }
}

/// Provider-owned state grouped off the `App` god-struct. Holds the
/// provider config, the edit form, the in-flight sync tracking
/// (cancel flags, completed names, error aggregate), the pending
/// delete alias, the on-disk sync history and the dirty-check baseline.
/// Pure state container.
pub struct ProviderState {
    pub(in crate::app) config: ProviderConfig,
    pub(in crate::app) form: ProviderFormFields,
    pub(in crate::app) syncing: HashMap<String, Arc<AtomicBool>>,
    /// Names of providers that completed during this sync batch.
    pub(in crate::app) sync_done: Vec<String>,
    /// Whether any provider in the current batch had errors.
    pub(in crate::app) sync_had_errors: bool,
    /// Aggregate diff counts across the current sync batch. Reset when the
    /// batch finishes (no providers left in `syncing`). Used by the footer
    /// background status to render `(+3 ~1 -2)` next to the provider list.
    pub(in crate::app) batch_added: usize,
    pub(in crate::app) batch_updated: usize,
    pub(in crate::app) batch_stale: usize,
    /// Total provider count for the current batch (done + still syncing).
    /// Captured when sync starts so the `n/total` counter does not jump
    /// when providers complete and leave `syncing`.
    pub(in crate::app) batch_total: usize,
    pub(in crate::app) pending_delete: Option<String>,
    /// When deleting a single labeled config, this carries the full id.
    /// `pending_delete` is used for whole-provider delete (header confirm).
    pub(in crate::app) pending_delete_id: Option<ProviderConfigId>,
    pub(in crate::app) sync_history: HashMap<String, SyncRecord>,
    pub(in crate::app) form_baseline: Option<ProviderFormBaseline>,
    /// Provider names that are expanded in the tree-style provider list.
    /// Only matters when a provider has 2+ labeled configs.
    pub(in crate::app) expanded_providers: HashSet<String>,
    /// In-progress lazy migration: when adding a 2nd config of a provider
    /// that currently has a single bare config, we first prompt for a label
    /// for the existing config. The chosen label lives here until the new
    /// config form is saved (then both writes happen atomically). When the
    /// user cancels the new config form, this is dropped and nothing is
    /// written.
    pub(in crate::app) pending_label_migration: Option<PendingLabelMigration>,
    /// Payload of an open `Screen::ConfirmPurgeStale` dialog. The Screen
    /// variant is data-less; the alias list and provider scope live here
    /// so the dialog can be opened with hundreds of stale aliases
    /// without paying for an enum clone on every transition.
    pub(in crate::app) pending_purge: Option<PendingPurge>,
}

/// Payload of the stale-host purge confirm dialog.
#[derive(Debug, Clone)]
pub struct PendingPurge {
    /// Aliases the user will be told they are about to remove.
    pub aliases: Vec<String>,
    /// Provider scope filter; `None` purges across providers.
    pub provider: Option<String>,
}

/// State carried between step 1 (label both configs) and step 2
/// (fill in the new labeled config form) of the lazy-migration add flow.
#[derive(Debug, Clone)]
pub struct PendingLabelMigration {
    pub provider: String,
    /// User-chosen label for the EXISTING (currently bare) config.
    pub existing_label: String,
    /// User-chosen label for the NEW config being added.
    pub new_label: String,
    /// Which field has focus in the label-migration screen.
    pub focused: LabelMigrationField,
    /// Cursor position (char index) within the focused field's value.
    pub cursor_pos: usize,
}

impl PendingLabelMigration {
    /// Get the focused field's value.
    pub fn focused_value(&self) -> &str {
        match self.focused {
            LabelMigrationField::Existing => &self.existing_label,
            LabelMigrationField::New => &self.new_label,
        }
    }

    /// Get the focused field's value mutably.
    pub fn focused_value_mut(&mut self) -> &mut String {
        match self.focused {
            LabelMigrationField::Existing => &mut self.existing_label,
            LabelMigrationField::New => &mut self.new_label,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelMigrationField {
    Existing,
    New,
}

/// One row in the tree-style provider list.
#[derive(Debug, Clone)]
pub enum ProviderRow {
    /// Provider group header. `config_count` 0 = unconfigured, 1 = single
    /// config (bare or labeled), 2+ = group that can be expanded.
    Header { name: String, config_count: usize },
    /// One labeled config under an expanded header.
    Leaf { id: ProviderConfigId },
}

impl ProviderRow {
    pub fn provider_name(&self) -> &str {
        match self {
            ProviderRow::Header { name, .. } => name,
            ProviderRow::Leaf { id } => &id.provider,
        }
    }
}

impl ProviderState {
    /// Reset batch counters when a completely new sync run begins.
    ///
    /// Call before inserting into `syncing` on every spawn path. When both
    /// `syncing` and `sync_done` are empty a fresh batch is starting, so
    /// stale `batch_total` / `batch_added` / `batch_updated` / `batch_stale`
    /// values from a previous (non-completed) run are cleared. Without this
    /// guard a rare edge case could leak state from an interrupted batch
    /// into a smaller follow-up batch and show "Syncing 1/5" while only
    /// one provider is actually in flight.
    pub fn reset_batch_if_idle(&mut self) {
        if self.syncing.is_empty() && self.sync_done.is_empty() {
            self.batch_total = 0;
            self.batch_added = 0;
            self.batch_updated = 0;
            self.batch_stale = 0;
            self.sync_had_errors = false;
        }
    }

    /// Clear all batch tracking once a sync run has fully completed (no
    /// providers left in `syncing`). Drops the completed-name list, the
    /// error flag and every batch counter so the next run starts clean.
    pub fn finish_batch(&mut self) {
        self.sync_done.clear();
        self.sync_had_errors = false;
        self.batch_added = 0;
        self.batch_updated = 0;
        self.batch_stale = 0;
        self.batch_total = 0;
    }

    /// Open a delete confirmation for a provider config. `pending_delete`
    /// carries the bare provider name for the renderer; `pending_delete_id`
    /// carries the full id (including optional label) used by the confirm
    /// handler to scope the removal to a single config when the provider
    /// has multiple labeled configs.
    pub fn request_delete(&mut self, id: ProviderConfigId) {
        self.pending_delete = Some(id.provider.clone());
        self.pending_delete_id = Some(id);
    }

    /// Dismiss a pending provider delete confirmation. Idempotent.
    pub fn cancel_delete(&mut self) {
        self.pending_delete = None;
        self.pending_delete_id = None;
    }

    /// Toggle the expanded state of a provider group in the tree-style
    /// provider list. Returns `true` when the provider is now expanded
    /// (was added) and `false` when it is now collapsed (was removed)
    /// so the caller can log the transition without re-reading state.
    pub fn toggle_expanded(&mut self, name: &str) -> bool {
        let added = !self.expanded_providers.contains(name);
        if added {
            self.expanded_providers.insert(name.to_string());
        } else {
            self.expanded_providers.remove(name);
        }
        added
    }

    /// Dismiss an in-progress lazy label-migration. Idempotent.
    pub fn cancel_label_migration(&mut self) {
        self.pending_label_migration = None;
    }

    pub fn config(&self) -> &ProviderConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut ProviderConfig {
        &mut self.config
    }

    pub fn form(&self) -> &ProviderFormFields {
        &self.form
    }

    pub fn form_mut(&mut self) -> &mut ProviderFormFields {
        &mut self.form
    }

    pub fn syncing(&self) -> &HashMap<String, Arc<AtomicBool>> {
        &self.syncing
    }

    pub fn syncing_mut(&mut self) -> &mut HashMap<String, Arc<AtomicBool>> {
        &mut self.syncing
    }

    pub fn sync_done(&self) -> &[String] {
        &self.sync_done
    }

    pub fn push_sync_done(&mut self, name: String) {
        self.sync_done.push(name);
    }

    pub fn clear_sync_done(&mut self) {
        self.sync_done.clear();
    }

    pub fn sync_had_errors(&self) -> bool {
        self.sync_had_errors
    }

    pub fn set_sync_had_errors(&mut self, value: bool) {
        self.sync_had_errors = value;
    }

    pub fn batch_added(&self) -> usize {
        self.batch_added
    }

    pub fn batch_updated(&self) -> usize {
        self.batch_updated
    }

    pub fn batch_stale(&self) -> usize {
        self.batch_stale
    }

    /// Fold one provider's diff counts into the current batch aggregate
    /// rendered by the footer summary.
    pub fn add_batch_diff(&mut self, added: usize, updated: usize, stale: usize) {
        self.batch_added += added;
        self.batch_updated += updated;
        self.batch_stale += stale;
    }

    pub fn batch_total(&self) -> usize {
        self.batch_total
    }

    pub fn set_batch_total(&mut self, value: usize) {
        self.batch_total = value;
    }

    /// Raise `batch_total` to at least the number of providers known to be
    /// part of the current batch (done plus still syncing). Never lowers it
    /// so the `n/total` counter stays stable as providers complete.
    pub fn bump_batch_total(&mut self) {
        self.batch_total = self
            .batch_total
            .max(self.sync_done.len() + self.syncing.len());
    }

    pub fn pending_delete(&self) -> Option<&str> {
        self.pending_delete.as_deref()
    }

    pub fn take_pending_delete(&mut self) -> Option<String> {
        self.pending_delete.take()
    }

    pub fn pending_delete_id(&self) -> Option<&ProviderConfigId> {
        self.pending_delete_id.as_ref()
    }

    pub fn take_pending_delete_id(&mut self) -> Option<ProviderConfigId> {
        self.pending_delete_id.take()
    }

    pub fn sync_history(&self) -> &HashMap<String, SyncRecord> {
        &self.sync_history
    }

    pub fn sync_history_mut(&mut self) -> &mut HashMap<String, SyncRecord> {
        &mut self.sync_history
    }

    /// Record a provider's sync outcome in the on-disk-backed history map,
    /// overwriting any previous record for the same key.
    pub fn record_sync(&mut self, key: String, record: SyncRecord) {
        self.sync_history.insert(key, record);
    }

    pub fn form_baseline(&self) -> Option<&ProviderFormBaseline> {
        self.form_baseline.as_ref()
    }

    pub fn set_form_baseline(&mut self, baseline: Option<ProviderFormBaseline>) {
        self.form_baseline = baseline;
    }

    /// True if the provider form differs from its captured baseline.
    pub fn form_is_dirty(&self) -> bool {
        match &self.form_baseline {
            Some(b) => {
                self.form.url != b.url
                    || self.form.token != b.token
                    || self.form.profile != b.profile
                    || self.form.project != b.project
                    || self.form.compartment != b.compartment
                    || self.form.regions != b.regions
                    || self.form.filter != b.filter
                    || self.form.interface != b.interface
                    || self.form.ssh_port != b.ssh_port
                    || self.form.alias_prefix != b.alias_prefix
                    || self.form.user != b.user
                    || self.form.identity_file != b.identity_file
                    || self.form.verify_tls != b.verify_tls
                    || self.form.auto_sync != b.auto_sync
                    || self.form.vault_role != b.vault_role
                    || self.form.vault_addr != b.vault_addr
                    || self.form.ssm != b.ssm
            }
            None => false,
        }
    }

    pub fn expanded_providers(&self) -> &HashSet<String> {
        &self.expanded_providers
    }

    pub fn expanded_providers_mut(&mut self) -> &mut HashSet<String> {
        &mut self.expanded_providers
    }

    pub fn pending_label_migration(&self) -> Option<&PendingLabelMigration> {
        self.pending_label_migration.as_ref()
    }

    pub fn pending_label_migration_mut(&mut self) -> Option<&mut PendingLabelMigration> {
        self.pending_label_migration.as_mut()
    }

    pub fn set_pending_label_migration(&mut self, migration: Option<PendingLabelMigration>) {
        self.pending_label_migration = migration;
    }

    pub fn pending_purge(&self) -> Option<&PendingPurge> {
        self.pending_purge.as_ref()
    }

    pub fn set_pending_purge(&mut self, pending: PendingPurge) {
        self.pending_purge = Some(pending);
    }

    pub fn take_pending_purge(&mut self) -> Option<PendingPurge> {
        self.pending_purge.take()
    }
}

impl Default for ProviderState {
    /// Truly empty default. No disk I/O. Call sites that need persisted
    /// state (App::new) construct with struct-update syntax:
    /// `ProviderState { config: ProviderConfig::load(paths), sync_history: SyncRecord::load_all(paths), ..Default::default() }`.
    fn default() -> Self {
        Self {
            config: ProviderConfig::default(),
            form: ProviderFormFields::new(),
            syncing: HashMap::new(),
            sync_done: Vec::new(),
            sync_had_errors: false,
            batch_added: 0,
            batch_updated: 0,
            batch_stale: 0,
            batch_total: 0,
            pending_delete: None,
            pending_delete_id: None,
            sync_history: HashMap::new(),
            form_baseline: None,
            expanded_providers: HashSet::new(),
            pending_label_migration: None,
            pending_purge: None,
        }
    }
}

impl ProviderState {
    /// Construct with persisted state loaded from disk.
    pub fn load(paths: Option<&crate::runtime::env::Paths>) -> Self {
        Self {
            config: crate::providers::config::ProviderConfig::load(paths),
            sync_history: SyncRecord::load_all(paths),
            ..Self::default()
        }
    }

    /// One row in the provider list, in display order.
    /// Each provider is a `Header`. When the provider has 2+ labeled configs
    /// AND is in `expanded_providers`, its `Leaf` rows follow immediately.
    /// When the provider has 0 or 1 config, no leaves are emitted.
    pub fn provider_list_rows(&self) -> Vec<ProviderRow> {
        let mut rows = Vec::new();
        for name in self.sorted_names() {
            let configs = self.config.sections_for_provider(&name);
            rows.push(ProviderRow::Header {
                name: name.clone(),
                config_count: configs.len(),
            });
            if configs.len() >= 2 && self.expanded_providers.contains(&name) {
                let mut sorted = configs.clone();
                sorted.sort_by(|a, b| {
                    a.id.label
                        .as_deref()
                        .unwrap_or("")
                        .cmp(b.id.label.as_deref().unwrap_or(""))
                });
                for s in sorted {
                    rows.push(ProviderRow::Leaf { id: s.id.clone() });
                }
            }
        }
        rows
    }

    /// Provider names sorted by last sync (most recent first), then configured,
    /// then unconfigured. Includes any unknown provider names found in the
    /// config file (e.g. typos or future providers).
    pub fn sorted_names(&self) -> Vec<String> {
        use crate::providers;
        let mut names: Vec<String> = providers::PROVIDER_NAMES
            .iter()
            .map(|s| s.to_string())
            .collect();
        // Append configured providers not in the known list so they are visible and removable
        for section in &self.config.sections {
            let name = section.provider().to_string();
            if !names.contains(&name) {
                names.push(name);
            }
        }
        // For multi-config providers the sync_history keys are the full id
        // ("digitalocean:work"), not the bare name. Take the MAX timestamp
        // across any history entry whose key matches this provider so the
        // recency sort works for both single and multi-config layouts.
        let max_ts = |provider: &str| -> u64 {
            self.sync_history
                .iter()
                .filter(|(k, _)| {
                    k.as_str() == provider || k.split_once(':').is_some_and(|(p, _)| p == provider)
                })
                .map(|(_, r)| r.timestamp)
                .max()
                .unwrap_or(0)
        };
        names.sort_by(|a, b| {
            let conf_a = self.config.section(a.as_str()).is_some();
            let conf_b = self.config.section(b.as_str()).is_some();
            let ts_a = max_ts(a.as_str());
            let ts_b = max_ts(b.as_str());
            // Configured first (by most recent sync), then unconfigured alphabetically
            conf_b.cmp(&conf_a).then(ts_b.cmp(&ts_a)).then(a.cmp(b))
        });
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_empty() {
        // Must not touch disk. Constructed with ProviderConfig::default()
        // and an empty sync_history. App::new() layers the real on-disk
        // state on top via struct-update syntax.
        let s = ProviderState::default();
        assert!(s.config.sections.is_empty());
        assert!(s.config.path_override.is_none());
        assert!(s.syncing.is_empty());
        assert!(s.sync_done.is_empty());
        assert!(!s.sync_had_errors);
        assert!(s.pending_delete.is_none());
        assert!(s.sync_history.is_empty());
        assert!(s.form_baseline.is_none());
    }

    #[test]
    fn sorted_names_returns_configured_providers_before_unconfigured() {
        use crate::providers::config::ProviderSection;

        let mut state = ProviderState::default();
        state.config.sections.push(ProviderSection {
            id: crate::providers::config::ProviderConfigId::bare("vultr"),
            token: "tok".to_string(),
            alias_prefix: "vultr".to_string(),
            ..ProviderSection::default()
        });
        state.config.sections.push(ProviderSection {
            id: crate::providers::config::ProviderConfigId::bare("digitalocean"),
            token: "tok".to_string(),
            alias_prefix: "do".to_string(),
            ..ProviderSection::default()
        });
        state.sync_history.insert(
            "digitalocean".to_string(),
            SyncRecord {
                timestamp: 2_000,
                message: "ok".to_string(),
                is_error: false,
            },
        );
        state.sync_history.insert(
            "vultr".to_string(),
            SyncRecord {
                timestamp: 1_000,
                message: "ok".to_string(),
                is_error: false,
            },
        );

        let names = state.sorted_names();
        // Configured providers (most recent sync first) precede unconfigured.
        assert_eq!(&names[0], "digitalocean");
        assert_eq!(&names[1], "vultr");
        // Every known provider name must be present.
        for &known in crate::providers::PROVIDER_NAMES {
            assert!(names.iter().any(|n| n == known), "missing {}", known);
        }
        // Unconfigured tail is sorted alphabetically.
        let unconfigured: Vec<&String> = names.iter().skip(2).collect();
        let mut sorted = unconfigured.clone();
        sorted.sort();
        assert_eq!(unconfigured, sorted);
    }

    #[test]
    fn sorted_names_includes_unknown_providers_from_config() {
        use crate::providers::config::ProviderSection;

        let mut state = ProviderState::default();
        state.config.sections.push(ProviderSection {
            id: crate::providers::config::ProviderConfigId::bare("someday_provider"),
            token: "tok".to_string(),
            alias_prefix: "x".to_string(),
            ..ProviderSection::default()
        });

        let names = state.sorted_names();
        assert!(names.iter().any(|n| n == "someday_provider"));
    }

    #[test]
    fn request_delete_sets_both_pending_fields() {
        let mut s = ProviderState::default();
        let id = crate::providers::config::ProviderConfigId::bare("digitalocean");
        s.request_delete(id.clone());
        assert_eq!(s.pending_delete.as_deref(), Some("digitalocean"));
        assert_eq!(s.pending_delete_id.as_ref(), Some(&id));
    }

    #[test]
    fn request_delete_with_labeled_id_keeps_provider_name_in_pending_delete() {
        let mut s = ProviderState::default();
        let id = crate::providers::config::ProviderConfigId::labeled("digitalocean", "work");
        s.request_delete(id.clone());
        // pending_delete carries only the provider name; the full id with
        // label is in pending_delete_id.
        assert_eq!(s.pending_delete.as_deref(), Some("digitalocean"));
        assert_eq!(s.pending_delete_id.as_ref(), Some(&id));
    }

    #[test]
    fn request_delete_overwrites_existing_pending() {
        let mut s = ProviderState::default();
        s.request_delete(crate::providers::config::ProviderConfigId::bare("vultr"));
        let new_id = crate::providers::config::ProviderConfigId::bare("hetzner");
        s.request_delete(new_id.clone());
        assert_eq!(s.pending_delete.as_deref(), Some("hetzner"));
        assert_eq!(s.pending_delete_id.as_ref(), Some(&new_id));
    }

    #[test]
    fn cancel_delete_clears_both_pending_fields() {
        let mut s = ProviderState::default();
        s.request_delete(crate::providers::config::ProviderConfigId::bare("vultr"));
        s.cancel_delete();
        assert!(s.pending_delete.is_none());
        assert!(s.pending_delete_id.is_none());
    }

    #[test]
    fn cancel_delete_is_idempotent() {
        let mut s = ProviderState::default();
        s.cancel_delete();
        s.cancel_delete();
        assert!(s.pending_delete.is_none());
        assert!(s.pending_delete_id.is_none());
    }

    #[test]
    fn toggle_expanded_adds_when_absent_and_returns_true() {
        let mut s = ProviderState::default();
        assert!(!s.expanded_providers.contains("digitalocean"));
        let added = s.toggle_expanded("digitalocean");
        assert!(added);
        assert!(s.expanded_providers.contains("digitalocean"));
    }

    #[test]
    fn toggle_expanded_removes_when_present_and_returns_false() {
        let mut s = ProviderState::default();
        s.expanded_providers.insert("digitalocean".to_string());
        let added = s.toggle_expanded("digitalocean");
        assert!(!added);
        assert!(!s.expanded_providers.contains("digitalocean"));
    }

    #[test]
    fn cancel_label_migration_clears_pending() {
        let mut s = ProviderState {
            pending_label_migration: Some(PendingLabelMigration {
                provider: "digitalocean".to_string(),
                existing_label: "old".to_string(),
                new_label: "new".to_string(),
                focused: LabelMigrationField::Existing,
                cursor_pos: 0,
            }),
            ..Default::default()
        };
        s.cancel_label_migration();
        assert!(s.pending_label_migration.is_none());
    }

    #[test]
    fn cancel_label_migration_is_idempotent_when_already_none() {
        let mut s = ProviderState::default();
        s.cancel_label_migration();
        s.cancel_label_migration();
        assert!(s.pending_label_migration.is_none());
    }

    #[test]
    fn add_batch_diff_accumulates_each_counter() {
        let mut s = ProviderState::default();
        s.add_batch_diff(3, 1, 2);
        s.add_batch_diff(1, 0, 4);
        assert_eq!(s.batch_added(), 4);
        assert_eq!(s.batch_updated(), 1);
        assert_eq!(s.batch_stale(), 6);
    }

    #[test]
    fn bump_batch_total_raises_to_done_plus_syncing() {
        let mut s = ProviderState::default();
        s.push_sync_done("aws".to_string());
        s.syncing_mut()
            .insert("vultr".to_string(), Arc::new(AtomicBool::new(false)));
        s.bump_batch_total();
        assert_eq!(s.batch_total(), 2);
    }

    #[test]
    fn bump_batch_total_never_lowers_existing_peak() {
        let mut s = ProviderState::default();
        s.set_batch_total(5);
        s.push_sync_done("aws".to_string());
        s.bump_batch_total();
        assert_eq!(s.batch_total(), 5);
    }

    #[test]
    fn finish_batch_clears_all_batch_state() {
        let mut s = ProviderState::default();
        s.push_sync_done("aws".to_string());
        s.set_sync_had_errors(true);
        s.add_batch_diff(2, 3, 4);
        s.set_batch_total(7);
        s.finish_batch();
        assert!(s.sync_done().is_empty());
        assert!(!s.sync_had_errors());
        assert_eq!(s.batch_added(), 0);
        assert_eq!(s.batch_updated(), 0);
        assert_eq!(s.batch_stale(), 0);
        assert_eq!(s.batch_total(), 0);
    }

    fn state_matching_baseline() -> ProviderState {
        let b = ProviderFormBaseline {
            url: "https://api".into(),
            token: "tok".into(),
            profile: "default".into(),
            project: "proj".into(),
            compartment: "comp".into(),
            regions: "eu-west".into(),
            filter: "status=active".into(),
            interface: "ens192".into(),
            ssh_port: "2222".into(),
            alias_prefix: "ap".into(),
            user: "ec2-user".into(),
            identity_file: "~/.ssh/id".into(),
            verify_tls: true,
            auto_sync: false,
            vault_role: "role".into(),
            vault_addr: "https://vault".into(),
            ssm: crate::providers::aws_ssm::SsmMode::default(),
        };
        let mut s = ProviderState::default();
        s.form.url = b.url.clone();
        s.form.token = b.token.clone();
        s.form.profile = b.profile.clone();
        s.form.project = b.project.clone();
        s.form.compartment = b.compartment.clone();
        s.form.regions = b.regions.clone();
        s.form.filter = b.filter.clone();
        s.form.interface = b.interface.clone();
        s.form.ssh_port = b.ssh_port.clone();
        s.form.alias_prefix = b.alias_prefix.clone();
        s.form.user = b.user.clone();
        s.form.identity_file = b.identity_file.clone();
        s.form.verify_tls = b.verify_tls;
        s.form.auto_sync = b.auto_sync;
        s.form.vault_role = b.vault_role.clone();
        s.form.vault_addr = b.vault_addr.clone();
        s.set_form_baseline(Some(b));
        s
    }

    #[test]
    fn form_is_dirty_is_false_without_a_baseline() {
        let mut s = ProviderState::default();
        s.form.url = "edited".into();
        assert!(!s.form_is_dirty());
    }

    #[test]
    fn form_is_dirty_is_false_when_form_equals_baseline() {
        assert!(!state_matching_baseline().form_is_dirty());
    }

    fn assert_field_change_is_dirty(field: &str, mutate: impl FnOnce(&mut ProviderFormFields)) {
        let mut s = state_matching_baseline();
        mutate(&mut s.form);
        assert!(s.form_is_dirty(), "a change in {field} must read dirty");
    }

    #[test]
    fn form_is_dirty_detects_a_change_in_each_field() {
        assert_field_change_is_dirty("url", |f| f.url.push('x'));
        assert_field_change_is_dirty("token", |f| f.token.push('x'));
        assert_field_change_is_dirty("profile", |f| f.profile.push('x'));
        assert_field_change_is_dirty("project", |f| f.project.push('x'));
        assert_field_change_is_dirty("compartment", |f| f.compartment.push('x'));
        assert_field_change_is_dirty("regions", |f| f.regions.push('x'));
        assert_field_change_is_dirty("interface", |f| f.interface.push('x'));
        assert_field_change_is_dirty("ssh_port", |f| f.ssh_port.push('x'));
        assert_field_change_is_dirty("alias_prefix", |f| f.alias_prefix.push('x'));
        assert_field_change_is_dirty("user", |f| f.user.push('x'));
        assert_field_change_is_dirty("identity_file", |f| f.identity_file.push('x'));
        assert_field_change_is_dirty("verify_tls", |f| f.verify_tls = !f.verify_tls);
        assert_field_change_is_dirty("auto_sync", |f| f.auto_sync = !f.auto_sync);
        assert_field_change_is_dirty("vault_role", |f| f.vault_role.push('x'));
        assert_field_change_is_dirty("vault_addr", |f| f.vault_addr.push('x'));
        assert_field_change_is_dirty("filter", |f| f.filter.push('x'));
        assert_field_change_is_dirty("ssm", |f| f.ssm = f.ssm.next());
    }
}
