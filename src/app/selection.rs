//! Selection and navigation helpers: keys, tags, tunnels, snippets, and the
//! background tunnel polling that updates status when active tunnels exit.

use std::path::Path;

use ratatui::widgets::ListState;

use super::{HostListItem, Screen};
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

    /// Get hosts available as ProxyJump targets (excludes the host being edited).
    pub fn proxyjump_candidates(&self) -> Vec<(String, String)> {
        let editing_alias = match &self.screen {
            Screen::EditHost { alias, .. } => Some(alias.as_str()),
            _ => None,
        };
        self.hosts
            .iter()
            .filter(|h| {
                if let Some(alias) = editing_alias {
                    h.alias != alias
                } else {
                    true
                }
            })
            .map(|h| (h.alias.clone(), h.hostname.clone()))
            .collect()
    }

    /// Move proxyjump picker selection up.
    pub fn select_prev_proxyjump(&mut self) {
        let len = self.proxyjump_candidates().len();
        super::cycle_selection(&mut self.ui.proxyjump_picker_state, len, false);
    }

    /// Move proxyjump picker selection down.
    pub fn select_next_proxyjump(&mut self) {
        let len = self.proxyjump_candidates().len();
        super::cycle_selection(&mut self.ui.proxyjump_picker_state, len, true);
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

    /// Open the tag picker overlay.
    pub fn open_tag_picker(&mut self) {
        self.tag_list = self.collect_unique_tags();
        self.ui.tag_picker_state = ListState::default();
        if !self.tag_list.is_empty() {
            self.ui.tag_picker_state.select(Some(0));
        }
        self.screen = Screen::TagPicker;
    }

    /// Move tag picker selection up.
    pub fn select_prev_tag(&mut self) {
        super::cycle_selection(&mut self.ui.tag_picker_state, self.tag_list.len(), false);
    }

    /// Move tag picker selection down.
    pub fn select_next_tag(&mut self) {
        super::cycle_selection(&mut self.ui.tag_picker_state, self.tag_list.len(), true);
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
