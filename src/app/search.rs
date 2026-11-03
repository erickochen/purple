//! Search and filter operations. Implements `impl App` continuation with
//! query mode entry/exit, fuzzy filter, scope computation, and the snippet
//! search helper.

use std::collections::HashSet;

use super::{HostListItem, PingStatus};
use crate::app::App;

/// Search mode state.
#[derive(Default)]
pub struct SearchState {
    pub(in crate::app) query: Option<String>,
    pub(in crate::app) filtered_indices: Vec<usize>,
    pub(in crate::app) filtered_pattern_indices: Vec<usize>,
    pub(in crate::app) filtered_match_indices: Vec<usize>,
    pub(in crate::app) pre_search_selection: Option<usize>,
    /// When a group tab is active, holds the host indices visible in that group.
    /// Search results are intersected with this set to scope the search.
    pub(in crate::app) scope_indices: Option<HashSet<usize>>,
}

impl SearchState {
    pub fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }

    pub fn filtered_indices(&self) -> &[usize] {
        &self.filtered_indices
    }

    pub fn filtered_pattern_indices(&self) -> &[usize] {
        &self.filtered_pattern_indices
    }

    pub fn filtered_match_indices(&self) -> &[usize] {
        &self.filtered_match_indices
    }

    pub fn scope_indices(&self) -> Option<&HashSet<usize>> {
        self.scope_indices.as_ref()
    }

    pub fn set_query(&mut self, value: Option<String>) {
        self.query = value;
    }

    pub fn clear_filtered_indices(&mut self) {
        self.filtered_indices.clear();
    }

    pub fn clear_filtered_pattern_indices(&mut self) {
        self.filtered_pattern_indices.clear();
    }

    pub fn clear_filtered_match_indices(&mut self) {
        self.filtered_match_indices.clear();
    }

    /// Append a char to the query string. No-op when the query is inactive.
    pub fn push_query_char(&mut self, c: char) {
        if let Some(q) = self.query.as_mut() {
            q.push(c);
        }
    }

    /// Pop the trailing char from the query string. No-op when the query is inactive.
    pub fn pop_query_char(&mut self) {
        if let Some(q) = self.query.as_mut() {
            q.pop();
        }
    }
}

impl App {
    /// Compute the search scope from the current display list when group-filtered.
    fn compute_search_scope(&self) -> Option<HashSet<usize>> {
        self.hosts_state.group_filter.as_ref()?;
        Some(
            self.hosts_state
                .display_list
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
        self.ping.filter_down_only = false;
        self.search.query = None;
        self.search.filtered_indices.clear();
        self.search.filtered_pattern_indices.clear();
        self.search.filtered_match_indices.clear();
        self.search.scope_indices = None;
        // Restore pre-search position (bounds-checked)
        if let Some(pos) = self.search.pre_search_selection.take() {
            if pos < self.hosts_state.display_list.len() {
                self.ui.list_state.select(Some(pos));
            } else if let Some(first) = self
                .hosts_state
                .display_list
                .iter()
                .position(HostListItem::is_selectable)
            {
                self.ui.list_state.select(Some(first));
            }
        }
    }

    /// Apply the current search query to filter hosts.
    pub fn apply_filter(&mut self) {
        log::debug!(
            "[purple] apply_filter: query={:?} down_only={} scope={}",
            self.search.query.as_deref().unwrap_or(""),
            self.ping.filter_down_only,
            self.search.scope_indices.as_ref().map_or(0, |s| s.len())
        );
        // Filtered index lists drive the search-mode render path which also
        // consumes the render cache; recompute fresh.
        self.hosts_state.render_cache.invalidate();
        let query = match &self.search.query {
            Some(q) if !q.is_empty() => q.clone(),
            Some(_) => {
                self.search.filtered_indices = (0..self.hosts_state.list.len()).collect();
                self.search.filtered_pattern_indices =
                    (0..self.hosts_state.patterns.len()).collect();
                self.search.filtered_match_indices =
                    (0..self.hosts_state.match_blocks.len()).collect();
                // Scope to group if active
                if let Some(ref scope) = self.search.scope_indices {
                    self.search.filtered_indices.retain(|i| scope.contains(i));
                }
                if !self.ping.filter_down_only {
                    let total = self.search_result_count();
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
                if !self.ping.filter_down_only {
                    return;
                }
                // No search query but down-only is active: start with all hosts
                self.search.filtered_indices = (0..self.hosts_state.list.len()).collect();
                self.search.filtered_pattern_indices = Vec::new();
                self.search.filtered_match_indices = Vec::new();
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
            let provider_config = &self.providers.config;
            self.search.filtered_indices = self
                .hosts_state
                .list
                .iter()
                .enumerate()
                .filter(|(_, host)| {
                    (super::eq_ci("stale", tag_exact) && host.stale.is_some())
                        || (super::eq_ci("vault-ssh", tag_exact)
                            && crate::vault_ssh::resolve_vault_role(
                                host.vault_ssh.as_deref(),
                                host.provider.as_deref(),
                                host.provider_label.as_deref(),
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
                .hosts_state
                .patterns
                .iter()
                .enumerate()
                .filter(|(_, p)| p.tags.iter().any(|t| super::eq_ci(t, tag_exact)))
                .map(|(i, _)| i)
                .collect();
            self.search.filtered_match_indices = self
                .hosts_state
                .match_blocks
                .iter()
                .enumerate()
                .filter(|(_, m)| m.tags.iter().any(|t| super::eq_ci(t, tag_exact)))
                .map(|(i, _)| i)
                .collect();
        } else if let Some(tag_query) = query.strip_prefix("tag:") {
            // Fuzzy tag match (manual search), includes provider name and virtual "stale"/"vault".
            // Space-separated terms are ANDed: every term must hit a tag/provider field.
            let provider_config = &self.providers.config;
            let terms: Vec<&str> = tag_query.split_whitespace().collect();
            self.search.filtered_indices = self
                .hosts_state
                .list
                .iter()
                .enumerate()
                .filter(|(_, host)| {
                    terms.iter().all(|term| {
                        (super::contains_ci("stale", term) && host.stale.is_some())
                            || (super::contains_ci("vault-ssh", term)
                                && crate::vault_ssh::resolve_vault_role(
                                    host.vault_ssh.as_deref(),
                                    host.provider.as_deref(),
                                    host.provider_label.as_deref(),
                                    provider_config,
                                )
                                .is_some())
                            || (super::contains_ci("vault-kv", term)
                                && host
                                    .askpass
                                    .as_deref()
                                    .map(|s| s.starts_with("vault:"))
                                    .unwrap_or(false))
                            || host
                                .provider_tags
                                .iter()
                                .chain(host.tags.iter())
                                .any(|t| super::contains_ci(t, term))
                            || host
                                .provider
                                .as_ref()
                                .is_some_and(|p| super::contains_ci(p, term))
                    })
                })
                .map(|(i, _)| i)
                .collect();
            self.search.filtered_pattern_indices = self
                .hosts_state
                .patterns
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    terms
                        .iter()
                        .all(|term| p.tags.iter().any(|t| super::contains_ci(t, term)))
                })
                .map(|(i, _)| i)
                .collect();
            self.search.filtered_match_indices = self
                .hosts_state
                .match_blocks
                .iter()
                .enumerate()
                .filter(|(_, m)| {
                    terms
                        .iter()
                        .all(|term| m.tags.iter().any(|t| super::contains_ci(t, term)))
                })
                .map(|(i, _)| i)
                .collect();
        } else {
            // Space-separated terms are ANDed: every term must match at least
            // one field. Split once, not per host. No terms (whitespace-only
            // query) matches everything, so a trailing space while typing does
            // not blank the list.
            let terms: Vec<&str> = query.split_whitespace().collect();
            self.search.filtered_indices = self
                .hosts_state
                .list
                .iter()
                .enumerate()
                .filter(|(_, host)| {
                    terms.iter().all(|term| {
                        super::contains_ci(&host.alias, term)
                            || super::contains_ci(&host.hostname, term)
                            || super::contains_ci(&host.user, term)
                            || host
                                .provider_tags
                                .iter()
                                .chain(host.tags.iter())
                                .any(|t| super::contains_ci(t, term))
                            || host
                                .provider
                                .as_ref()
                                .is_some_and(|p| super::contains_ci(p, term))
                    })
                })
                .map(|(i, _)| i)
                .collect();
            self.search.filtered_pattern_indices = self
                .hosts_state
                .patterns
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    terms.iter().all(|term| {
                        super::contains_ci(&p.pattern, term)
                            || p.tags.iter().any(|t| super::contains_ci(t, term))
                    })
                })
                .map(|(i, _)| i)
                .collect();
            self.search.filtered_match_indices = self
                .hosts_state
                .match_blocks
                .iter()
                .enumerate()
                .filter(|(_, m)| {
                    terms.iter().all(|term| {
                        super::contains_ci(&m.criteria_text, term)
                            || m.tags.iter().any(|t| super::contains_ci(t, term))
                    })
                })
                .map(|(i, _)| i)
                .collect();
        }

        // Scope results to the active group if set
        if let Some(ref scope) = self.search.scope_indices {
            self.search.filtered_indices.retain(|i| scope.contains(i));
        }

        // Post-filter: keep only unreachable hosts when down-only mode is active
        if self.ping.filter_down_only {
            self.search.filtered_indices.retain(|&idx| {
                let alias = &self.hosts_state.list[idx].alias;
                matches!(self.ping.status.get(alias), Some(PingStatus::Unreachable))
            });
            // Patterns and Match blocks can't be pinged, so hide them in down-only mode
            self.search.filtered_pattern_indices.clear();
            self.search.filtered_match_indices.clear();
        }

        // Reset selection
        let total_results = self.search_result_count();
        log::debug!(
            "[purple] apply_filter matched: hosts={} patterns={} match_blocks={}",
            self.search.filtered_indices.len(),
            self.search.filtered_pattern_indices.len(),
            self.search.filtered_match_indices.len()
        );
        if total_results == 0 {
            self.ui.list_state.select(None);
        } else {
            self.ui.list_state.select(Some(0));
        }
    }
    /// Return indices of snippets matching the search query.
    pub fn filtered_snippet_indices(&self) -> Vec<usize> {
        crate::snippet::filtered_indices(self.snippets.store(), self.ui.snippet_search.as_deref())
    }
}
