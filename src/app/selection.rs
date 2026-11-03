//! Selection and navigation helpers: keys, tags, tunnels, snippets, and the
//! background tunnel polling that updates status when active tunnels exit.

use std::path::Path;

use ratatui::widgets::ListState;

use super::{
    BulkTagAction, BulkTagApplyResult, BulkTagEditorState, BulkTagRow, HostListItem,
    ProxyJumpCandidate, Screen,
};
use crate::app::App;
use crate::ssh_config::model::{HostEntry, MatchBlockEntry, PatternEntry};
use crate::ssh_keys;

/// Identifies a Match block across a reload: its file, its criteria text and
/// which of the blocks sharing that text it is. Positions shift whenever a
/// section above it is added or removed, so they are not part of the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchBlockKey {
    pub(crate) source_file: Option<std::path::PathBuf>,
    pub(crate) criteria_text: String,
    pub(crate) nth: usize,
}

impl App {
    /// Transition to a new screen. Logs the transition at debug level for
    /// support-bundle traceability. Callers should prefer this over direct
    /// `app.screen = ...` assignment.
    pub fn set_screen(&mut self, screen: Screen) {
        if self.screen != screen {
            log::debug!(
                "[purple] screen: {} → {}",
                self.screen.variant_name(),
                screen.variant_name()
            );
        }
        self.screen = screen;
    }

    /// Open the Help overlay over the current screen, moving the previous
    /// screen into the `return_screen` slot instead of cloning it. No-op
    /// when Help is already open.
    pub fn push_help_overlay(&mut self) {
        if matches!(self.screen, Screen::Help { .. }) {
            return;
        }
        log::debug!("[purple] screen: {} → Help", self.screen.variant_name());
        let old = std::mem::replace(&mut self.screen, Screen::HostList);
        self.screen = Screen::Help {
            return_screen: Box::new(old),
        };
    }

    /// Close the Help overlay and restore the previous screen. The boxed
    /// screen is moved back into place rather than cloned. No-op when the
    /// current screen is not Help.
    pub fn pop_help_overlay(&mut self) {
        let returned = {
            let Screen::Help { return_screen } = &mut self.screen else {
                return;
            };
            std::mem::replace(&mut **return_screen, Screen::HostList)
        };
        log::debug!("[purple] screen: Help → {}", returned.variant_name());
        self.screen = returned;
    }

    /// Cycle to the next top page. Logs the transition so Tab-cycle
    /// confusion ("I keep landing on the wrong page after Tab") leaves a
    /// breadcrumb in `~/.purple/purple.log`. Callers should prefer this
    /// over direct `app.top_page = app.top_page.next()` assignment.
    pub fn cycle_top_page_next(&mut self) {
        let old = self.top_page;
        self.top_page = self.top_page.next();
        log::debug!("[purple] top_page: {:?} → {:?} (Tab)", old, self.top_page);
    }

    /// Cycle to the previous top page. See `cycle_top_page_next`.
    pub fn cycle_top_page_prev(&mut self) {
        let old = self.top_page;
        self.top_page = self.top_page.prev();
        log::debug!(
            "[purple] top_page: {:?} → {:?} (Shift+Tab)",
            old,
            self.top_page
        );
    }

    /// Get the host index from the currently selected display list item.
    pub fn selected_host_index(&self) -> Option<usize> {
        if self.search.query.is_some() {
            // In search mode, list_state indexes into filtered_indices
            let sel = self.ui.list_state.selected()?;
            self.search.filtered_indices.get(sel).copied()
        } else {
            // In normal mode, list_state indexes into display_list
            let sel = self.ui.list_state.selected()?;
            match self.hosts_state.display_list.get(sel) {
                Some(HostListItem::Host { index }) => Some(*index),
                _ => None,
            }
        }
    }

    /// Get the currently selected host entry.
    pub fn selected_host(&self) -> Option<&HostEntry> {
        self.selected_host_index()
            .and_then(|i| self.hosts_state.list.get(i))
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
                    .and_then(|&i| self.hosts_state.patterns.get(i));
            }
            return None;
        }
        let sel = self.ui.list_state.selected()?;
        match self.hosts_state.display_list.get(sel) {
            Some(HostListItem::Pattern { index }) => self.hosts_state.patterns.get(*index),
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
            self.hosts_state.display_list.get(sel),
            Some(HostListItem::Pattern { .. })
        )
    }

    /// The selected Match block, if the cursor is on one. In search mode the
    /// Match rows follow the host rows and the pattern rows.
    pub fn selected_match_block(&self) -> Option<&MatchBlockEntry> {
        let sel = self.ui.list_state.selected()?;
        if self.search.query.is_some() {
            let offset =
                self.search.filtered_indices.len() + self.search.filtered_pattern_indices.len();
            return sel
                .checked_sub(offset)
                .and_then(|i| self.search.filtered_match_indices.get(i))
                .and_then(|&i| self.hosts_state.match_blocks.get(i));
        }
        match self.hosts_state.display_list.get(sel) {
            Some(HostListItem::MatchBlock { index }) => self.hosts_state.match_blocks.get(*index),
            _ => None,
        }
    }

    pub fn is_match_block_selected(&self) -> bool {
        self.selected_match_block().is_some()
    }

    /// A key for the selected Match block that survives a reload, see
    /// `MatchBlockKey`.
    pub fn selected_match_block_key(&self) -> Option<MatchBlockKey> {
        let sel = self.selected_match_block()?;
        let nth = self
            .hosts_state
            .match_blocks
            .iter()
            .filter(|m| m.source_file == sel.source_file && m.criteria_text == sel.criteria_text)
            .take_while(|m| m.order != sel.order)
            .count();
        Some(MatchBlockKey {
            source_file: sel.source_file.clone(),
            criteria_text: sel.criteria_text.clone(),
            nth,
        })
    }

    /// A pattern or a Match block: rows that take no host action.
    pub fn is_non_host_selected(&self) -> bool {
        self.is_pattern_selected() || self.is_match_block_selected()
    }

    /// Rows in search mode: hosts, then patterns, then Match blocks.
    pub fn search_result_count(&self) -> usize {
        self.search.filtered_indices.len()
            + self.search.filtered_pattern_indices.len()
            + self.search.filtered_match_indices.len()
    }

    /// Move selection up, skipping group headers.
    pub fn select_prev(&mut self) {
        self.ui.detail_scroll = 0;
        if self.search.query.is_some() {
            let total = self.search_result_count();
            super::cycle_selection(&mut self.ui.list_state, total, false);
        } else {
            self.select_prev_in_display_list();
        }
    }

    /// Move selection down, skipping group headers.
    pub fn select_next(&mut self) {
        self.ui.detail_scroll = 0;
        if self.search.query.is_some() {
            let total = self.search_result_count();
            super::cycle_selection(&mut self.ui.list_state, total, true);
        } else {
            self.select_next_in_display_list();
        }
    }

    fn select_next_in_display_list(&mut self) {
        if self.hosts_state.display_list.is_empty() {
            return;
        }
        let len = self.hosts_state.display_list.len();
        let current = self.ui.list_state.selected().unwrap_or(0);
        // Find next selectable item after current (always skip headers)
        for offset in 1..=len {
            let idx = (current + offset) % len;
            if self.hosts_state.display_list[idx].is_selectable() {
                self.ui.list_state.select(Some(idx));
                return;
            }
        }
    }

    fn select_prev_in_display_list(&mut self) {
        if self.hosts_state.display_list.is_empty() {
            return;
        }
        let len = self.hosts_state.display_list.len();
        let current = self.ui.list_state.selected().unwrap_or(0);
        // Find prev selectable item before current (always skip headers)
        for offset in 1..=len {
            let idx = (current + len - offset) % len;
            if self.hosts_state.display_list[idx].is_selectable() {
                self.ui.list_state.select(Some(idx));
                return;
            }
        }
    }

    /// Page down in the host list: jump PAGE_SIZE selectable items forward,
    /// skipping group headers and clamping at the last item. When already on
    /// the last item, wrap to the first, mirroring single-step navigation.
    pub fn page_down_host(&mut self) {
        self.ui.detail_scroll = 0;
        const PAGE_SIZE: usize = 10;
        let before = self.ui.list_state.selected();
        if self.search.query.is_some() {
            let total = self.search_result_count();
            super::page_down(&mut self.ui.list_state, total, PAGE_SIZE);
        } else {
            let current = self.ui.list_state.selected().unwrap_or(0);
            let mut target = current;
            let mut items_skipped = 0;
            let len = self.hosts_state.display_list.len();
            for i in (current + 1)..len {
                if self.hosts_state.display_list[i].is_selectable() {
                    target = i;
                    items_skipped += 1;
                    if items_skipped >= PAGE_SIZE {
                        break;
                    }
                }
            }
            self.ui.list_state.select(Some(target));
        }
        // Already at the bottom: wrap to the top, like j/Down does.
        if self.ui.list_state.selected() == before {
            self.select_next();
        }
    }

    /// Page up in the host list: jump PAGE_SIZE selectable items back, skipping
    /// group headers and clamping at the first item. When already on the first
    /// item, wrap to the last, mirroring single-step navigation.
    pub fn page_up_host(&mut self) {
        self.ui.detail_scroll = 0;
        const PAGE_SIZE: usize = 10;
        let before = self.ui.list_state.selected();
        if self.search.query.is_some() {
            let total = self.search_result_count();
            super::page_up(&mut self.ui.list_state, total, PAGE_SIZE);
        } else {
            let current = self.ui.list_state.selected().unwrap_or(0);
            let mut target = current;
            let mut items_skipped = 0;
            for i in (0..current).rev() {
                if self.hosts_state.display_list[i].is_selectable() {
                    target = i;
                    items_skipped += 1;
                    if items_skipped >= PAGE_SIZE {
                        break;
                    }
                }
            }
            self.ui.list_state.select(Some(target));
        }
        // Already at the top: wrap to the bottom, like k/Up does.
        if self.ui.list_state.selected() == before {
            self.select_prev();
        }
    }
    pub fn scan_keys(&mut self) {
        let paths = self.env().paths().cloned();
        let ssh_dir = paths.as_ref().map(crate::runtime::env::Paths::ssh_dir);
        if let Some(ssh_dir) = ssh_dir {
            self.keys.list = ssh_keys::discover_keys(
                paths.as_ref(),
                Path::new(&ssh_dir),
                &self.hosts_state.list,
            );
            if !self.keys.list.is_empty() && self.keys.list_state.selected().is_none() {
                self.keys.list_state.select(Some(0));
            }
            self.reload.keys_dir_mtime = crate::app::reload_state::get_mtime(&ssh_dir);
            self.reload.key_file_mtimes =
                crate::app::reload_state::snapshot_key_mtimes(&ssh_dir, &self.keys.list);
        }
    }

    /// Move key list selection up.
    pub fn select_prev_key(&mut self) {
        super::cycle_selection(&mut self.keys.list_state, self.keys.list.len(), false);
    }

    /// Move key list selection down.
    pub fn select_next_key(&mut self) {
        super::cycle_selection(&mut self.keys.list_state, self.keys.list.len(), true);
    }

    /// Move key picker selection up.
    pub fn select_prev_picker_key(&mut self) {
        super::cycle_selection(&mut self.ui.key_picker.list, self.keys.list.len(), false);
    }

    /// Move key picker selection down.
    pub fn select_next_picker_key(&mut self) {
        super::cycle_selection(&mut self.ui.key_picker.list, self.keys.list.len(), true);
    }

    /// Move password picker selection up.
    pub fn select_prev_password_source(&mut self) {
        super::cycle_selection(
            &mut self.ui.password_picker.list,
            crate::askpass::PASSWORD_SOURCES.len(),
            false,
        );
    }

    /// Move password picker selection down.
    pub fn select_next_password_source(&mut self) {
        super::cycle_selection(
            &mut self.ui.password_picker.list,
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
                .hosts_state
                .list
                .iter()
                .find(|h| h.alias == *alias)
                .map(|h| h.hostname.as_str()),
            _ => None,
        };
        let editing_suffix = editing_hostname.and_then(domain_suffix);

        let usage_counts = proxyjump_usage_counts(&self.hosts_state.list, editing_alias);
        let mut scored = score_proxyjump_candidates(
            &self.hosts_state.list,
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
        for host in &self.hosts_state.list {
            if let Some(ref role) = host.vault_ssh
                && seen.insert(role.clone())
            {
                roles.push(role.clone());
            }
        }
        // Also collect from provider configs.
        for section in &self.providers.config.sections {
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
        super::cycle_selection(&mut self.ui.vault_role_picker.list, len, false);
    }

    /// Move vault role picker selection down.
    pub fn select_next_vault_role(&mut self) {
        let len = self.vault_role_candidates().len();
        super::cycle_selection(&mut self.ui.vault_role_picker.list, len, true);
    }

    /// Collect all unique tags from hosts, sorted alphabetically.
    pub fn collect_unique_tags(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut tags = Vec::new();
        let mut has_stale = false;
        let mut has_vault_ssh = false;
        let mut has_vault_kv = false;
        for host in &self.hosts_state.list {
            for tag in host.provider_tags.iter().chain(host.tags.iter()) {
                if seen.insert(tag.clone()) {
                    tags.push(tag.clone());
                }
            }
            if let Some(ref provider) = host.provider
                && seen.insert(provider.clone())
            {
                tags.push(provider.clone());
            }
            if host.stale.is_some() {
                has_stale = true;
            }
            if crate::vault_ssh::resolve_vault_role(
                host.vault_ssh.as_deref(),
                host.provider.as_deref(),
                host.provider_label.as_deref(),
                &self.providers.config,
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
        for pattern in &self.hosts_state.patterns {
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
            for section in &self.providers.config.sections {
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
        for &idx in &self.hosts_state.multi_select {
            if let Some(host) = self.hosts_state.list.get(idx) {
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
        for host in &self.hosts_state.list {
            for tag in &host.tags {
                candidate_tags.insert(tag.clone());
            }
        }
        for pattern in &self.hosts_state.patterns {
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
                    .hosts_state
                    .list
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

        // Snapshot baseline actions for the dirty-check on Esc. Every row
        // starts at `Leave`; the snapshot is the same length as `rows` so
        // `is_dirty` short-circuits before scanning when nothing has changed.
        let initial_actions: Vec<BulkTagAction> = rows.iter().map(|r| r.action).collect();
        self.forms.bulk_tag_editor = BulkTagEditorState {
            rows,
            aliases,
            skipped_included: skipped,
            new_tag_input: None,
            new_tag_cursor: 0,
            initial_actions,
        };
        self.ui.bulk_tag_editor_state = ListState::default();
        if !self.forms.bulk_tag_editor.rows.is_empty() {
            self.ui.bulk_tag_editor_state.select(Some(0));
        }
        self.set_screen(Screen::BulkTagEditor);
        true
    }

    /// Move bulk tag editor selection down.
    pub fn bulk_tag_editor_next(&mut self) {
        super::cycle_selection(
            &mut self.ui.bulk_tag_editor_state,
            self.forms.bulk_tag_editor.rows.len(),
            true,
        );
    }

    /// Move bulk tag editor selection up.
    pub fn bulk_tag_editor_prev(&mut self) {
        super::cycle_selection(
            &mut self.ui.bulk_tag_editor_state,
            self.forms.bulk_tag_editor.rows.len(),
            false,
        );
    }

    /// Cycle the action on the currently selected row:
    /// `Leave` → `AddToAll` → `RemoveFromAll` → `Leave`.
    pub fn bulk_tag_editor_cycle_current(&mut self) {
        super::bulk_tag_cycle_current(&self.ui, &mut self.forms);
    }

    /// Append a freshly typed tag to the row list. The new row is marked
    /// `AddToAll` so the user's intent ("add this new tag to all selected
    /// hosts") is preserved without a second keystroke. No-op for empty
    /// input or duplicate tag names.
    pub fn bulk_tag_editor_commit_new_tag(&mut self) {
        super::bulk_tag_commit_new_tag(&mut self.ui, &mut self.forms);
    }

    /// Apply all pending actions from the bulk tag editor. Leaves the
    /// config untouched (and returns an error) if the write fails so the
    /// user can retry without losing state. On success, hosts are reloaded
    /// (which clears `multi_select`).
    pub fn bulk_tag_apply(&mut self) -> Result<BulkTagApplyResult, String> {
        // Compute + config write run on the host and form slices alone; the
        // returned result drives the caller's control flow inline. Only the
        // post-write whole-App tails (mtime refresh, reload) need full App,
        // and only when something actually changed on disk.
        let result = super::apply_bulk_tags(&mut self.hosts_state, &mut self.forms)?;
        if result.changed_hosts > 0 {
            self.update_last_modified();
            self.reload_hosts();
        }
        Ok(result)
    }

    /// Open the tag picker overlay.
    pub fn open_tag_picker(&mut self) {
        self.tags.list = self.collect_unique_tags();
        self.ui.tag_picker_state = ListState::default();
        if !self.tags.list.is_empty() {
            self.ui.tag_picker_state.select(Some(0));
        }
        self.set_screen(Screen::TagPicker);
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
        self.tunnels
            .load_directives(&self.hosts_state.ssh_config, alias);
    }

    /// Move tunnel list selection up.
    pub fn select_prev_tunnel(&mut self) {
        super::cycle_selection(
            &mut self.ui.tunnel_list_state,
            self.tunnels.list.len(),
            false,
        );
    }

    /// Move tunnel list selection down.
    pub fn select_next_tunnel(&mut self) {
        super::cycle_selection(
            &mut self.ui.tunnel_list_state,
            self.tunnels.list.len(),
            true,
        );
    }

    /// Move snippet picker selection up.
    pub fn select_prev_snippet(&mut self) {
        super::cycle_selection(
            &mut self.ui.snippet_picker_state,
            self.snippets.store.snippets.len(),
            false,
        );
    }

    /// Move snippet picker selection down.
    pub fn select_next_snippet(&mut self) {
        super::cycle_selection(
            &mut self.ui.snippet_picker_state,
            self.snippets.store.snippets.len(),
            true,
        );
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
        app.ui.proxyjump_picker.list.select(None);
        return;
    }
    // When no prior selection exists, seed `next` so the first modular
    // step lands on index 0 (forward) or len-1 (backward). Without this
    // seed a fresh picker with selected() == None would skip index 0 on
    // a Down press.
    let seed: usize = match app.ui.proxyjump_picker.list.selected() {
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
            app.ui.proxyjump_picker.list.select(Some(next));
            return;
        }
    }
}
