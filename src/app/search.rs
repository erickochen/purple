//! Search and filter operations. Implements `impl App` continuation with
//! query mode entry/exit, fuzzy filter, scope computation, and the snippet
//! search helper.

use std::collections::HashSet;

use super::{HostListItem, PingStatus};
use crate::app::App;

impl App {
    /// Compute the search scope from the current display list when group-filtered.
    fn compute_search_scope(&self) -> Option<HashSet<usize>> {
        self.group_filter.as_ref()?;
        Some(
            self.display_list
                .iter()
                .filter_map(|item| {
                    if let HostListItem::Host { index } = item {
                        Some(*index)
                    } else {
                        None
                    }
                })
                .collect(),
        )
    }

    /// Enter search mode.
    pub fn start_search(&mut self) {
        self.search.pre_search_selection = self.ui.list_state.selected();
        self.search.scope_indices = self.compute_search_scope();
        self.search.query = Some(String::new());
        self.apply_filter();
    }

    /// Start search with an initial query (for positional arg).
    pub fn start_search_with(&mut self, query: &str) {
        self.search.pre_search_selection = self.ui.list_state.selected();
        self.search.scope_indices = self.compute_search_scope();
        self.search.query = Some(query.to_string());
        self.apply_filter();
    }

    /// Cancel search mode and restore normal view.
    pub fn cancel_search(&mut self) {
        self.filter_down_only = false;
        self.search.query = None;
        self.search.filtered_indices.clear();
        self.search.filtered_pattern_indices.clear();
        self.search.scope_indices = None;
        // Restore pre-search position (bounds-checked)
        if let Some(pos) = self.search.pre_search_selection.take() {
            if pos < self.display_list.len() {
                self.ui.list_state.select(Some(pos));
            } else if let Some(first) = self.display_list.iter().position(|item| {
                matches!(
                    item,
                    HostListItem::Host { .. } | HostListItem::Pattern { .. }
                )
            }) {
                self.ui.list_state.select(Some(first));
            }
        }
    }

    /// Apply the current search query to filter hosts.
    pub fn apply_filter(&mut self) {
        let query = match &self.search.query {
            Some(q) if !q.is_empty() => q.clone(),
            Some(_) => {
                self.search.filtered_indices = (0..self.hosts.len()).collect();
                self.search.filtered_pattern_indices = (0..self.patterns.len()).collect();
                // Scope to group if active
                if let Some(ref scope) = self.search.scope_indices {
                    self.search.filtered_indices.retain(|i| scope.contains(i));
                }
                if !self.filter_down_only {
                    let total = self.search.filtered_indices.len()
                        + self.search.filtered_pattern_indices.len();
                    if total == 0 {
                        self.ui.list_state.select(None);
                    } else {
                        self.ui.list_state.select(Some(0));
                    }
                    return;
                }
                // Fall through to down-only filtering below
                String::new()
            }
            None => {
                if !self.filter_down_only {
                    return;
                }
                // No search query but down-only is active: start with all hosts
                self.search.filtered_indices = (0..self.hosts.len()).collect();
                self.search.filtered_pattern_indices = Vec::new();
                // Scope to group if active
                if let Some(ref scope) = self.search.scope_indices {
                    self.search.filtered_indices.retain(|i| scope.contains(i));
                }
                // Fall through to down-only filtering below
                String::new()
            }
        };

        if let Some(tag_exact) = query.strip_prefix("tag=") {
            // Exact tag match (from tag picker), includes provider name and virtual "stale"/"vault"
            let provider_config = &self.provider_config;
            self.search.filtered_indices = self
                .hosts
                .iter()
                .enumerate()
                .filter(|(_, host)| {
                    (super::eq_ci("stale", tag_exact) && host.stale.is_some())
                        || (super::eq_ci("vault-ssh", tag_exact)
                            && crate::vault_ssh::resolve_vault_role(
                                host.vault_ssh.as_deref(),
                                host.provider.as_deref(),
                                provider_config,
                            )
                            .is_some())
                        || (super::eq_ci("vault-kv", tag_exact)
                            && host
                                .askpass
                                .as_deref()
                                .map(|s| s.starts_with("vault:"))
                                .unwrap_or(false))
                        || host
                            .provider_tags
                            .iter()
                            .chain(host.tags.iter())
                            .any(|t| super::eq_ci(t, tag_exact))
                        || host
                            .provider
                            .as_ref()
                            .is_some_and(|p| super::eq_ci(p, tag_exact))
                })
                .map(|(i, _)| i)
                .collect();
            self.search.filtered_pattern_indices = self
                .patterns
                .iter()
                .enumerate()
                .filter(|(_, p)| p.tags.iter().any(|t| super::eq_ci(t, tag_exact)))
                .map(|(i, _)| i)
                .collect();
        } else if let Some(tag_query) = query.strip_prefix("tag:") {
            // Fuzzy tag match (manual search), includes provider name and virtual "stale"/"vault"
            let provider_config = &self.provider_config;
            self.search.filtered_indices = self
                .hosts
                .iter()
                .enumerate()
                .filter(|(_, host)| {
                    (super::contains_ci("stale", tag_query) && host.stale.is_some())
                        || (super::contains_ci("vault-ssh", tag_query)
                            && crate::vault_ssh::resolve_vault_role(
                                host.vault_ssh.as_deref(),
                                host.provider.as_deref(),
                                provider_config,
                            )
                            .is_some())
                        || (super::contains_ci("vault-kv", tag_query)
                            && host
                                .askpass
                                .as_deref()
                                .map(|s| s.starts_with("vault:"))
                                .unwrap_or(false))
                        || host
                            .provider_tags
                            .iter()
                            .chain(host.tags.iter())
                            .any(|t| super::contains_ci(t, tag_query))
                        || host
                            .provider
                            .as_ref()
                            .is_some_and(|p| super::contains_ci(p, tag_query))
                })
                .map(|(i, _)| i)
                .collect();
            self.search.filtered_pattern_indices = self
                .patterns
                .iter()
                .enumerate()
                .filter(|(_, p)| p.tags.iter().any(|t| super::contains_ci(t, tag_query)))
                .map(|(i, _)| i)
                .collect();
        } else {
            self.search.filtered_indices = self
                .hosts
                .iter()
                .enumerate()
                .filter(|(_, host)| {
                    super::contains_ci(&host.alias, &query)
                        || super::contains_ci(&host.hostname, &query)
                        || super::contains_ci(&host.user, &query)
                        || host
                            .provider_tags
                            .iter()
                            .chain(host.tags.iter())
                            .any(|t| super::contains_ci(t, &query))
                        || host
                            .provider
                            .as_ref()
                            .is_some_and(|p| super::contains_ci(p, &query))
                })
                .map(|(i, _)| i)
                .collect();
            self.search.filtered_pattern_indices = self
                .patterns
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    super::contains_ci(&p.pattern, &query)
                        || p.tags.iter().any(|t| super::contains_ci(t, &query))
                })
                .map(|(i, _)| i)
                .collect();
        }

        // Scope results to the active group if set
        if let Some(ref scope) = self.search.scope_indices {
            self.search.filtered_indices.retain(|i| scope.contains(i));
        }

        // Post-filter: keep only unreachable hosts when down-only mode is active
        if self.filter_down_only {
            self.search.filtered_indices.retain(|&idx| {
                let alias = &self.hosts[idx].alias;
                matches!(self.ping_status.get(alias), Some(PingStatus::Unreachable))
            });
            // Patterns can't be pinged, so hide them in down-only mode
            self.search.filtered_pattern_indices.clear();
        }

        // Reset selection
        let total_results =
            self.search.filtered_indices.len() + self.search.filtered_pattern_indices.len();
        if total_results == 0 {
            self.ui.list_state.select(None);
        } else {
            self.ui.list_state.select(Some(0));
        }
    }
    /// Return indices of snippets matching the search query.
    pub fn filtered_snippet_indices(&self) -> Vec<usize> {
        match &self.ui.snippet_search {
            None => (0..self.snippet_store.snippets.len()).collect(),
            Some(query) if query.is_empty() => (0..self.snippet_store.snippets.len()).collect(),
            Some(query) => self
                .snippet_store
                .snippets
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    super::contains_ci(&s.name, query)
                        || super::contains_ci(&s.command, query)
                        || super::contains_ci(&s.description, query)
                })
                .map(|(i, _)| i)
                .collect(),
        }
    }
}
