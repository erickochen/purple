//! Selection and navigation helpers: keys, tags, tunnels, snippets, and the
//! background tunnel polling that updates status when active tunnels exit.

use std::path::Path;

use ratatui::widgets::ListState;

use super::{
    BulkTagAction, BulkTagApplyResult, BulkTagEditorState, BulkTagRow, HostListItem,
    ProxyJumpCandidate, Screen,
};
use crate::app::App;
use crate::ssh_config::model::{HostEntry, PatternEntry};
use crate::ssh_keys;

impl App {
    /// Get the host index from the currently selected display list item.
    pub fn selected_host_index(&self) -> Option<usize> {
        if self.search.query.is_some() {
            // In search mode, list_state indexes into filtered_indices
            let sel = self.ui.list_state.selected()?;
            self.search.filtered_indices.get(sel).copied()
        } else {
            // In normal mode, list_state indexes into display_list
            let sel = self.ui.list_state.selected()?;
            match self.display_list.get(sel) {
                Some(HostListItem::Host { index }) => Some(*index),
                _ => None,
            }
        }
    }

    /// Get the currently selected host entry.
    pub fn selected_host(&self) -> Option<&HostEntry> {
        self.selected_host_index().and_then(|i| self.hosts.get(i))
    }

    /// Get the currently selected pattern entry (if a pattern is selected).
    pub fn selected_pattern(&self) -> Option<&PatternEntry> {
        if self.search.query.is_some() {
            let sel = self.ui.list_state.selected()?;
            let host_count = self.search.filtered_indices.len();
            if sel >= host_count {
                let pattern_idx = sel - host_count;
                return self
                    .search
                    .filtered_pattern_indices
                    .get(pattern_idx)
                    .and_then(|&i| self.patterns.get(i));
            }
            return None;
        }
        let sel = self.ui.list_state.selected()?;
        match self.display_list.get(sel) {
            Some(HostListItem::Pattern { index }) => self.patterns.get(*index),
            _ => None,
        }
    }

    /// Check if the currently selected item is a pattern.
    pub fn is_pattern_selected(&self) -> bool {
        if self.search.query.is_some() {
            let Some(sel) = self.ui.list_state.selected() else {
                return false;
            };
            let total =
                self.search.filtered_indices.len() + self.search.filtered_pattern_indices.len();
            return sel >= self.search.filtered_indices.len() && sel < total;
        }
        let Some(sel) = self.ui.list_state.selected() else {
            return false;
        };
        matches!(
            self.display_list.get(sel),
            Some(HostListItem::Pattern { .. })
        )
    }

    /// Move selection up, skipping group headers.
    pub fn select_prev(&mut self) {
        self.ui.detail_scroll = 0;
        if self.search.query.is_some() {
            let total =
                self.search.filtered_indices.len() + self.search.filtered_pattern_indices.len();
            super::cycle_selection(&mut self.ui.list_state, total, false);
        } else {
            self.select_prev_in_display_list();
        }
    }

    /// Move selection down, skipping group headers.
    pub fn select_next(&mut self) {
        self.ui.detail_scroll = 0;
        if self.search.query.is_some() {
            let total =
                self.search.filtered_indices.len() + self.search.filtered_pattern_indices.len();
            super::cycle_selection(&mut self.ui.list_state, total, true);
        } else {
            self.select_next_in_display_list();
        }
    }

    fn select_next_in_display_list(&mut self) {
        if self.display_list.is_empty() {
            return;
        }
        let len = self.display_list.len();
        let current = self.ui.list_state.selected().unwrap_or(0);
        // Find next selectable item after current (always skip headers)
        for offset in 1..=len {
            let idx = (current + offset) % len;
            if matches!(
                &self.display_list[idx],
                HostListItem::Host { .. } | HostListItem::Pattern { .. }
            ) {
                self.ui.list_state.select(Some(idx));
                return;
            }
        }
    }

    fn select_prev_in_display_list(&mut self) {
        if self.display_list.is_empty() {
            return;
        }
        let len = self.display_list.len();
        let current = self.ui.list_state.selected().unwrap_or(0);
        // Find prev selectable item before current (always skip headers)
        for offset in 1..=len {
            let idx = (current + len - offset) % len;
            if matches!(
                &self.display_list[idx],
                HostListItem::Host { .. } | HostListItem::Pattern { .. }
            ) {
                self.ui.list_state.select(Some(idx));
                return;
            }
        }
    }

    /// Page down in the host list, skipping group headers when ungrouped.
    pub fn page_down_host(&mut self) {
        self.ui.detail_scroll = 0;
        const PAGE_SIZE: usize = 10;
        if self.search.query.is_some() {
            super::page_down(
                &mut self.ui.list_state,
                self.search.filtered_indices.len(),
                PAGE_SIZE,
            );
        } else {
            let current = self.ui.list_state.selected().unwrap_or(0);
            let mut target = current;
            let mut items_skipped = 0;
            let len = self.display_list.len();
            for i in (current + 1)..len {
                if matches!(
                    self.display_list[i],
                    HostListItem::Host { .. } | HostListItem::Pattern { .. }
                ) {
                    target = i;
                    items_skipped += 1;
                    if items_skipped >= PAGE_SIZE {
                        break;
                    }
                }
            }
            if target != current {
                self.ui.list_state.select(Some(target));
                self.update_group_tab_follow();
            }
        }
    }

    /// Page up in the host list, skipping group headers.
    pub fn page_up_host(&mut self) {
        self.ui.detail_scroll = 0;
        const PAGE_SIZE: usize = 10;
        if self.search.query.is_some() {
            super::page_up(
                &mut self.ui.list_state,
                self.search.filtered_indices.len(),
                PAGE_SIZE,
            );
        } else {
            let current = self.ui.list_state.selected().unwrap_or(0);
            let mut target = current;
            let mut items_skipped = 0;
            for i in (0..current).rev() {
                if matches!(
                    self.display_list[i],
                    HostListItem::Host { .. } | HostListItem::Pattern { .. }
                ) {
                    target = i;
                    items_skipped += 1;
                    if items_skipped >= PAGE_SIZE {
                        break;
                    }
                }
            }
            if target != current {
                self.ui.list_state.select(Some(target));
                self.update_group_tab_follow();
            }
        }
    }
    pub fn scan_keys(&mut self) {
        if let Some(home) = dirs::home_dir() {
            let ssh_dir = home.join(".ssh");
            self.keys = ssh_keys::discover_keys(Path::new(&ssh_dir), &self.hosts);
            if !self.keys.is_empty() && self.ui.key_list_state.selected().is_none() {
                self.ui.key_list_state.select(Some(0));
            }
        }
    }

    /// Move key list selection up.
    pub fn select_prev_key(&mut self) {
        super::cycle_selection(&mut self.ui.key_list_state, self.keys.len(), false);
    }

    /// Move key list selection down.
    pub fn select_next_key(&mut self) {
        super::cycle_selection(&mut self.ui.key_list_state, self.keys.len(), true);
    }

    /// Move key picker selection up.
    pub fn select_prev_picker_key(&mut self) {
        super::cycle_selection(&mut self.ui.key_picker_state, self.keys.len(), false);
    }

    /// Move key picker selection down.
    pub fn select_next_picker_key(&mut self) {
        super::cycle_selection(&mut self.ui.key_picker_state, self.keys.len(), true);
    }

    /// Move password picker selection up.
    pub fn select_prev_password_source(&mut self) {
        super::cycle_selection(
            &mut self.ui.password_picker_state,
            crate::askpass::PASSWORD_SOURCES.len(),
            false,
        );
    }

    /// Move password picker selection down.
    pub fn select_next_password_source(&mut self) {
        super::cycle_selection(
            &mut self.ui.password_picker_state,
            crate::askpass::PASSWORD_SOURCES.len(),
            true,
        );
    }

    /// Get hosts available as ProxyJump targets (excludes the host being
    /// edited), ranked so likely jump hosts appear on top. Ranking combines
    /// three signals: usage count as ProxyJump on other hosts, alias or
    /// hostname matching a jump-host keyword (`jump`, `bastion`, `gateway`,
    /// `proxy`, `gw`), and sharing the last two domain labels with the
    /// hostname of the host being edited. Items with a non-zero score are
    /// grouped in a "suggested" section above a visual `Separator`. The
    /// remaining items are listed alphabetically below. If no item scores,
    /// the full list is alphabetical with no separator.
    pub fn proxyjump_candidates(&self) -> Vec<ProxyJumpCandidate> {
        let editing_alias = match &self.screen {
            Screen::EditHost { alias, .. } => Some(alias.as_str()),
            _ => None,
        };
        let editing_hostname = match &self.screen {
            Screen::EditHost { alias, .. } => self
                .hosts
                .iter()
                .find(|h| h.alias == *alias)
                .map(|h| h.hostname.as_str()),
            _ => None,
        };
        let editing_suffix = editing_hostname.and_then(domain_suffix);

        let usage_counts = proxyjump_usage_counts(&self.hosts, editing_alias);
        let mut scored = score_proxyjump_candidates(
            &self.hosts,
            editing_alias,
            editing_suffix.as_deref(),
            &usage_counts,
        );

        // Top-3 suggestions: score > 0, sorted by score desc then alias asc.
        scored.sort_by(|(sa, a), (sb, b)| sb.cmp(sa).then_with(|| a.alias.cmp(&b.alias)));
        let suggested: Vec<&HostEntry> = scored
            .iter()
            .filter(|(s, _)| *s > 0)
            .take(3)
            .map(|(_, h)| *h)
            .collect();
        let suggested_aliases: std::collections::HashSet<&str> =
            suggested.iter().map(|h| h.alias.as_str()).collect();

        // Rest: everything not suggested, alphabetical by alias.
        scored.sort_by(|(_, a), (_, b)| a.alias.cmp(&b.alias));
        let rest: Vec<&HostEntry> = scored
            .into_iter()
            .map(|(_, h)| h)
            .filter(|h| !suggested_aliases.contains(h.alias.as_str()))
            .collect();

        build_proxyjump_items(&suggested, &rest)
    }

    /// Find the first selectable (non-separator) index in the ProxyJump
    /// picker, or None if the list has no hosts.
    pub fn proxyjump_first_host_index(&self) -> Option<usize> {
        self.proxyjump_candidates()
            .iter()
            .position(|c| matches!(c, ProxyJumpCandidate::Host { .. }))
    }

    /// Move proxyjump picker selection up, skipping separators.
    pub fn select_prev_proxyjump(&mut self) {
        step_proxyjump_selection(self, false);
    }

    /// Move proxyjump picker selection down, skipping separators.
    pub fn select_next_proxyjump(&mut self) {
        step_proxyjump_selection(self, true);
    }

    /// Collect unique Vault SSH roles from all hosts and providers, sorted.
    pub fn vault_role_candidates(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut roles = Vec::new();
        for host in &self.hosts {
            if let Some(ref role) = host.vault_ssh {
                if seen.insert(role.clone()) {
                    roles.push(role.clone());
                }
            }
        }
        // Also collect from provider configs.
        for section in &self.provider_config.sections {
            let role = section.vault_role.trim();
            if !role.is_empty() && seen.insert(role.to_string()) {
                roles.push(role.to_string());
            }
        }
        roles.sort();
        roles
    }

    /// Move vault role picker selection up.
    pub fn select_prev_vault_role(&mut self) {
        let len = self.vault_role_candidates().len();
        super::cycle_selection(&mut self.ui.vault_role_picker_state, len, false);
    }

    /// Move vault role picker selection down.
    pub fn select_next_vault_role(&mut self) {
        let len = self.vault_role_candidates().len();
        super::cycle_selection(&mut self.ui.vault_role_picker_state, len, true);
    }

    /// Collect all unique tags from hosts, sorted alphabetically.
    pub fn collect_unique_tags(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut tags = Vec::new();
        let mut has_stale = false;
        let mut has_vault_ssh = false;
        let mut has_vault_kv = false;
        for host in &self.hosts {
            for tag in host.provider_tags.iter().chain(host.tags.iter()) {
                if seen.insert(tag.clone()) {
                    tags.push(tag.clone());
                }
            }
            if let Some(ref provider) = host.provider {
                if seen.insert(provider.clone()) {
                    tags.push(provider.clone());
                }
            }
            if host.stale.is_some() {
                has_stale = true;
            }
            if crate::vault_ssh::resolve_vault_role(
                host.vault_ssh.as_deref(),
                host.provider.as_deref(),
                &self.provider_config,
            )
            .is_some()
            {
                has_vault_ssh = true;
            }
            if host
                .askpass
                .as_deref()
                .map(|s| s.starts_with("vault:"))
                .unwrap_or(false)
            {
                has_vault_kv = true;
            }
        }
        for pattern in &self.patterns {
            for tag in &pattern.tags {
                if seen.insert(tag.clone()) {
                    tags.push(tag.clone());
                }
            }
        }
        if has_stale && seen.insert("stale".to_string()) {
            tags.push("stale".to_string());
        }
        if !has_vault_ssh {
            for section in &self.provider_config.sections {
                if !section.vault_role.is_empty() {
                    has_vault_ssh = true;
                    break;
                }
            }
        }
        if has_vault_ssh && seen.insert("vault-ssh".to_string()) {
            tags.push("vault-ssh".to_string());
        }
        if has_vault_kv && seen.insert("vault-kv".to_string()) {
            tags.push("vault-kv".to_string());
        }
        tags.sort_by_cached_key(|a| a.to_lowercase());
        tags
    }

    /// Open the bulk tag editor for every host currently in `multi_select`.
    /// Returns false (and leaves the screen untouched) when the selection
    /// is empty or contains only pattern entries — callers can then fall
    /// back to single-host tag editing or show a status message.
    ///
    /// Hosts that live in an Include file are still listed in `aliases` but
    /// get surfaced via `skipped_included`. `bulk_tag_apply` honours that
    /// split so included hosts are never mutated in place.
    pub fn open_bulk_tag_editor(&mut self) -> bool {
        let mut aliases: Vec<String> = Vec::new();
        let mut skipped: Vec<String> = Vec::new();
        let mut alias_set: std::collections::HashSet<String> = std::collections::HashSet::new();
        for &idx in &self.multi_select {
            if let Some(host) = self.hosts.get(idx) {
                if !alias_set.insert(host.alias.clone()) {
                    continue;
                }
                if host.source_file.is_some() {
                    skipped.push(host.alias.clone());
                }
                aliases.push(host.alias.clone());
            }
        }
        if aliases.is_empty() {
            return false;
        }
        aliases.sort();
        skipped.sort();

        // Collect candidate tags: union of all user tags across the whole
        // config. This lets users apply an existing tag that none of the
        // selected hosts have yet (the common "tag a new batch with prod"
        // case).
        let mut candidate_tags: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        for host in &self.hosts {
            for tag in &host.tags {
                candidate_tags.insert(tag.clone());
            }
        }
        for pattern in &self.patterns {
            for tag in &pattern.tags {
                candidate_tags.insert(tag.clone());
            }
        }

        let selected_set: std::collections::HashSet<&str> =
            aliases.iter().map(|s| s.as_str()).collect();
        let rows: Vec<BulkTagRow> = candidate_tags
            .into_iter()
            .map(|tag| {
                let initial_count = self
                    .hosts
                    .iter()
                    .filter(|h| selected_set.contains(h.alias.as_str()))
                    .filter(|h| h.tags.iter().any(|t| t == &tag))
                    .count();
                BulkTagRow {
                    tag,
                    initial_count,
                    action: BulkTagAction::Leave,
                }
            })
            .collect();

        self.bulk_tag_editor = BulkTagEditorState {
            rows,
            aliases,
            skipped_included: skipped,
            new_tag_input: None,
            new_tag_cursor: 0,
        };
        self.ui.bulk_tag_editor_state = ListState::default();
        if !self.bulk_tag_editor.rows.is_empty() {
            self.ui.bulk_tag_editor_state.select(Some(0));
        }
        self.screen = Screen::BulkTagEditor;
        true
    }

    /// Move bulk tag editor selection down.
    pub fn bulk_tag_editor_next(&mut self) {
        super::cycle_selection(
            &mut self.ui.bulk_tag_editor_state,
            self.bulk_tag_editor.rows.len(),
            true,
        );
    }

    /// Move bulk tag editor selection up.
    pub fn bulk_tag_editor_prev(&mut self) {
        super::cycle_selection(
            &mut self.ui.bulk_tag_editor_state,
            self.bulk_tag_editor.rows.len(),
            false,
        );
    }

    /// Cycle the action on the currently selected row:
    /// `Leave` → `AddToAll` → `RemoveFromAll` → `Leave`.
    pub fn bulk_tag_editor_cycle_current(&mut self) {
        let Some(idx) = self.ui.bulk_tag_editor_state.selected() else {
            return;
        };
        if let Some(row) = self.bulk_tag_editor.rows.get_mut(idx) {
            row.action = row.action.cycle();
        }
    }

    /// Append a freshly typed tag to the row list. The new row is marked
    /// `AddToAll` so the user's intent ("add this new tag to all selected
    /// hosts") is preserved without a second keystroke. No-op for empty
    /// input or duplicate tag names.
    pub fn bulk_tag_editor_commit_new_tag(&mut self) {
        let Some(input) = self.bulk_tag_editor.new_tag_input.take() else {
            return;
        };
        self.bulk_tag_editor.new_tag_cursor = 0;
        let tag = input.trim().to_string();
        if tag.is_empty() {
            return;
        }
        // Reuse an existing row when the tag already exists — simply flip
        // its action to AddToAll rather than create a duplicate.
        if let Some(existing) = self.bulk_tag_editor.rows.iter().position(|r| r.tag == tag) {
            self.bulk_tag_editor.rows[existing].action = BulkTagAction::AddToAll;
            self.ui.bulk_tag_editor_state.select(Some(existing));
            return;
        }
        let row = BulkTagRow {
            tag,
            initial_count: 0,
            action: BulkTagAction::AddToAll,
        };
        let insert_at = self.bulk_tag_editor.rows.len();
        self.bulk_tag_editor.rows.push(row);
        self.ui.bulk_tag_editor_state.select(Some(insert_at));
    }

    /// Apply all pending actions from the bulk tag editor. Leaves the
    /// config untouched (and returns an error) if the write fails so the
    /// user can retry without losing state. On success, hosts are reloaded
    /// (which clears `multi_select`).
    pub fn bulk_tag_apply(&mut self) -> Result<BulkTagApplyResult, String> {
        if self.bulk_tag_editor.aliases.is_empty() {
            return Err("No hosts selected.".to_string());
        }
        let aliases = self.bulk_tag_editor.aliases.clone();
        let rows = self.bulk_tag_editor.rows.clone();
        let skipped_set: std::collections::HashSet<&str> = self
            .bulk_tag_editor
            .skipped_included
            .iter()
            .map(|s| s.as_str())
            .collect();

        // Short-circuit when the user opened the editor but never changed
        // any row. Avoids a no-op config write and a confusing toast.
        let has_pending = rows.iter().any(|r| r.action != BulkTagAction::Leave);
        if !has_pending {
            return Ok(BulkTagApplyResult {
                skipped_included: skipped_set.len(),
                ..Default::default()
            });
        }

        let mut changed_hosts: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut added = 0usize;
        let mut removed = 0usize;
        let mut skipped_included = 0usize;
        // Captured only when a host actually changes so `u` can undo the
        // whole bulk op in one keystroke. Collected before the write so a
        // write failure leaves the snapshot untouched (we roll back config
        // anyway below).
        let mut undo_snapshot: Vec<(String, Vec<String>)> = Vec::new();

        for alias in &aliases {
            if skipped_set.contains(alias.as_str()) {
                skipped_included += 1;
                continue;
            }
            let Some(host) = self.hosts.iter().find(|h| &h.alias == alias) else {
                continue;
            };
            let original_tags = host.tags.clone();
            let mut new_tags = original_tags.clone();
            let mut host_changed = false;
            for row in &rows {
                match row.action {
                    BulkTagAction::Leave => {}
                    BulkTagAction::AddToAll => {
                        if !new_tags.iter().any(|t| t == &row.tag) {
                            new_tags.push(row.tag.clone());
                            added += 1;
                            host_changed = true;
                        }
                    }
                    BulkTagAction::RemoveFromAll => {
                        let before = new_tags.len();
                        new_tags.retain(|t| t != &row.tag);
                        if new_tags.len() != before {
                            removed += 1;
                            host_changed = true;
                        }
                    }
                }
            }
            if host_changed {
                self.config.set_host_tags(alias, &new_tags);
                changed_hosts.insert(alias.clone());
                undo_snapshot.push((alias.clone(), original_tags));
            }
        }

        if changed_hosts.is_empty() {
            return Ok(BulkTagApplyResult {
                skipped_included,
                ..Default::default()
            });
        }

        // Clone only when we actually need to write. Deferred from the top
        // of the function so no-op applies (all hosts already have the tag)
        // skip the allocation entirely.
        let config_backup = self.config.clone();
        if let Err(e) = self.config.write() {
            log::error!("[purple] bulk tag apply write failed: {e}");
            self.config = config_backup;
            return Err(format!("Failed to save: {}", e));
        }

        log::debug!(
            "bulk tag apply: {} hosts, +{} -{}, skipped {}",
            changed_hosts.len(),
            added,
            removed,
            skipped_included
        );
        // Store the undo snapshot so `u` can restore previous tags. Cleared
        // by a successful undo or by the next config mutation.
        if !undo_snapshot.is_empty() {
            self.bulk_tag_undo = Some(undo_snapshot);
        }
        self.update_last_modified();
        self.reload_hosts();

        Ok(BulkTagApplyResult {
            changed_hosts: changed_hosts.len(),
            added,
            removed,
            skipped_included,
        })
    }

    /// Open the tag picker overlay.
    pub fn open_tag_picker(&mut self) {
        self.tags.list = self.collect_unique_tags();
        self.ui.tag_picker_state = ListState::default();
        if !self.tags.list.is_empty() {
            self.ui.tag_picker_state.select(Some(0));
        }
        self.screen = Screen::TagPicker;
    }

    /// Move tag picker selection up.
    pub fn select_prev_tag(&mut self) {
        super::cycle_selection(&mut self.ui.tag_picker_state, self.tags.list.len(), false);
    }

    /// Move tag picker selection down.
    pub fn select_next_tag(&mut self) {
        super::cycle_selection(&mut self.ui.tag_picker_state, self.tags.list.len(), true);
    }

    /// Load tunnel directives for a host alias.
    /// Uses find_tunnel_directives for Include-aware, multi-pattern host lookup.
    pub fn refresh_tunnel_list(&mut self, alias: &str) {
        self.tunnel_list = self.config.find_tunnel_directives(alias);
    }

    /// Move tunnel list selection up.
    pub fn select_prev_tunnel(&mut self) {
        super::cycle_selection(
            &mut self.ui.tunnel_list_state,
            self.tunnel_list.len(),
            false,
        );
    }

    /// Move tunnel list selection down.
    pub fn select_next_tunnel(&mut self) {
        super::cycle_selection(&mut self.ui.tunnel_list_state, self.tunnel_list.len(), true);
    }

    /// Move snippet picker selection up.
    pub fn select_prev_snippet(&mut self) {
        super::cycle_selection(
            &mut self.ui.snippet_picker_state,
            self.snippet_store.snippets.len(),
            false,
        );
    }

    /// Move snippet picker selection down.
    pub fn select_next_snippet(&mut self) {
        super::cycle_selection(
            &mut self.ui.snippet_picker_state,
            self.snippet_store.snippets.len(),
            true,
        );
    }

    /// Poll active tunnels for exit status. Returns messages for any that exited.
    /// Move selection to the next non-header item.
    pub fn select_next_skipping_headers(&mut self) {
        let current = self.ui.list_state.selected().unwrap_or(0);
        for i in (current + 1)..self.display_list.len() {
            if !matches!(self.display_list[i], HostListItem::GroupHeader(_)) {
                self.ui.list_state.select(Some(i));
                self.update_group_tab_follow();
                return;
            }
        }
    }

    /// Move selection to the previous non-header item.
    pub fn select_prev_skipping_headers(&mut self) {
        let current = self.ui.list_state.selected().unwrap_or(0);
        for i in (0..current).rev() {
            if !matches!(self.display_list[i], HostListItem::GroupHeader(_)) {
                self.ui.list_state.select(Some(i));
                self.update_group_tab_follow();
                return;
            }
        }
    }
}

const JUMP_KEYWORDS: &[&str] = &["jump", "bastion", "gateway", "proxy", "gw"];

/// Count how often each alias appears as a ProxyJump hop across all hosts,
/// excluding the host currently being edited.
fn proxyjump_usage_counts(
    hosts: &[HostEntry],
    editing_alias: Option<&str>,
) -> std::collections::HashMap<String, u32> {
    let mut counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for h in hosts {
        if h.proxy_jump.is_empty() || editing_alias == Some(h.alias.as_str()) {
            continue;
        }
        for hop in parse_proxy_jump_hops(&h.proxy_jump) {
            *counts.entry(hop).or_insert(0) += 1;
        }
    }
    counts
}

/// Score each host as a ProxyJump candidate. Excludes the host being edited.
/// Score = usage * 10 + keyword_hit * 5 + shared_domain_suffix * 3.
fn score_proxyjump_candidates<'a>(
    hosts: &'a [HostEntry],
    editing_alias: Option<&str>,
    editing_suffix: Option<&str>,
    usage_counts: &std::collections::HashMap<String, u32>,
) -> Vec<(u32, &'a HostEntry)> {
    hosts
        .iter()
        .filter(|h| editing_alias.is_none_or(|a| h.alias != a))
        .map(|h| {
            let usage = usage_counts.get(&h.alias).copied().unwrap_or(0);
            let kw = has_jump_keyword(&h.alias, &h.hostname);
            let same = editing_suffix
                .and_then(|suf| domain_suffix(&h.hostname).map(|s| s == suf))
                .unwrap_or(false);
            let score = usage * 10 + u32::from(kw) * 5 + u32::from(same) * 3;
            (score, h)
        })
        .collect()
}

/// Assemble the final picker list from pre-sorted `suggested` and `rest`
/// slices. Inserts a `Suggestions` section label and a `Separator` only when
/// both sides are non-empty.
fn build_proxyjump_items(suggested: &[&HostEntry], rest: &[&HostEntry]) -> Vec<ProxyJumpCandidate> {
    let mut items = Vec::with_capacity(suggested.len() + rest.len() + 2);
    if !suggested.is_empty() {
        items.push(ProxyJumpCandidate::SectionLabel("Suggestions"));
    }
    for h in suggested {
        items.push(ProxyJumpCandidate::Host {
            alias: h.alias.clone(),
            hostname: h.hostname.clone(),
            suggested: true,
        });
    }
    if !suggested.is_empty() && !rest.is_empty() {
        items.push(ProxyJumpCandidate::Separator);
    }
    for h in rest {
        items.push(ProxyJumpCandidate::Host {
            alias: h.alias.clone(),
            hostname: h.hostname.clone(),
            suggested: false,
        });
    }
    items
}

/// Parse a ProxyJump directive value into its list of alias hops, stripping
/// optional `user@` prefix and `:port` suffix (including IPv6 brackets).
/// Malformed hops (empty, missing closing bracket on an IPv6 literal) are
/// dropped rather than passed through as garbage that could never match a
/// real alias.
pub(crate) fn parse_proxy_jump_hops(proxy_jump: &str) -> Vec<String> {
    proxy_jump
        .split(',')
        .filter_map(|hop| {
            let h = hop.trim();
            if h.is_empty() {
                return None;
            }
            let h = h.split_once('@').map_or(h, |(_, host)| host);
            let h = if let Some(bracketed) = h.strip_prefix('[') {
                let (inner, _) = bracketed.split_once(']')?;
                inner
            } else {
                h.rsplit_once(':').map_or(h, |(host, _)| host)
            };
            if h.is_empty() {
                None
            } else {
                Some(h.to_string())
            }
        })
        .collect()
}

/// True when the alias or hostname mentions a common jump-host keyword
/// (`jump`, `bastion`, `gateway`, `proxy`, `gw`) as a substring.
pub(crate) fn has_jump_keyword(alias: &str, hostname: &str) -> bool {
    let a = alias.to_ascii_lowercase();
    let h = hostname.to_ascii_lowercase();
    JUMP_KEYWORDS
        .iter()
        .any(|kw| a.contains(kw) || h.contains(kw))
}

/// Extract the last two dot-separated labels of a hostname for domain
/// matching. Returns None for single-label hostnames, IPv4 literals, and
/// bracketed IPv6 literals where domain matching would be meaningless.
/// Also rejects any string that parses as a valid `IpAddr` (which catches
/// 4-octet IPv4 shapes without relying on a naive all-digits-per-label
/// check that would miss mixed strings like `192.168.1.foo`).
pub(crate) fn domain_suffix(hostname: &str) -> Option<String> {
    let h = hostname.trim();
    if h.is_empty() || h.starts_with('[') {
        return None;
    }
    if h.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }
    let labels: Vec<&str> = h.split('.').collect();
    if labels.len() < 2 {
        return None;
    }
    // Trailing empty labels (e.g. `example.com.` FQDN) would silently
    // produce a bogus `.com` suffix; normalise by trimming them off.
    let mut end = labels.len();
    while end > 0 && labels[end - 1].is_empty() {
        end -= 1;
    }
    if end < 2 {
        return None;
    }
    let tail = &labels[end - 2..end];
    Some(tail.join(".").to_ascii_lowercase())
}

/// Step the ProxyJump picker selection one position in the requested
/// direction, wrapping around and skipping any `Separator` entries. When
/// nothing is currently selected, the first step lands on the first
/// selectable host (forward) or the last selectable host (backward)
/// instead of advancing past index 0.
fn step_proxyjump_selection(app: &mut App, forward: bool) {
    let candidates = app.proxyjump_candidates();
    let len = candidates.len();
    if len == 0 {
        app.ui.proxyjump_picker_state.select(None);
        return;
    }
    // When no prior selection exists, seed `next` so the first modular
    // step lands on index 0 (forward) or len-1 (backward). Without this
    // seed a fresh picker with selected() == None would skip index 0 on
    // a Down press.
    let seed: usize = match app.ui.proxyjump_picker_state.selected() {
        Some(idx) => idx,
        None if forward => len - 1,
        None => 0,
    };
    let mut next = seed;
    for _ in 0..len {
        next = if forward {
            (next + 1) % len
        } else {
            (next + len - 1) % len
        };
        if matches!(candidates.get(next), Some(ProxyJumpCandidate::Host { .. })) {
            app.ui.proxyjump_picker_state.select(Some(next));
            return;
        }
    }
}
