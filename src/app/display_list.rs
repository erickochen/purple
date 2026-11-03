//! Display list construction, sorting and grouping. Implements `impl App`
//! continuation with the builders that turn host + pattern entries into
//! rendered list items, plus `apply_sort` and the group partitioning helpers.

use std::collections::HashMap;

use super::{GroupBy, HostListItem, SortMode};
use crate::app::App;
use crate::ssh_config::model::{
    ConfigElement, HostEntry, MatchBlockEntry, PatternEntry, SshConfigFile,
};

impl App {
    /// Build the display list with group headers from comments above host blocks.
    /// Comments are associated with the host block directly below them (no blank line between).
    /// Because the parser puts inter-block comments inside the preceding block's directives,
    /// we also extract trailing comments from each HostBlock.
    pub(crate) fn build_display_list_from(
        config: &SshConfigFile,
        hosts: &[HostEntry],
        patterns: &[PatternEntry],
        match_blocks: &[MatchBlockEntry],
    ) -> Vec<HostListItem> {
        let mut display_list = Vec::new();
        let mut host_index = 0;
        let mut pending_comment: Option<String> = None;

        for element in &config.elements {
            match element {
                ConfigElement::GlobalLine(line) => {
                    let trimmed = line.trim();
                    if let Some(rest) = trimmed.strip_prefix('#') {
                        if let Some(header) = Self::comment_header(rest) {
                            pending_comment = Some(header);
                        }
                    } else if trimmed.is_empty() {
                        // Blank line breaks the comment-to-host association
                        pending_comment = None;
                    } else {
                        pending_comment = None;
                    }
                }
                ConfigElement::HostBlock(block) => {
                    if crate::ssh_config::model::is_host_pattern(&block.host_pattern) {
                        pending_comment = None;
                        continue;
                    }

                    if host_index < hosts.len() {
                        if let Some(header) = pending_comment.take() {
                            display_list.push(HostListItem::GroupHeader(header));
                        }
                        display_list.push(HostListItem::Host { index: host_index });
                        host_index += 1;
                    }

                    // Extract trailing comments from this block for the next host
                    pending_comment = Self::extract_trailing_comment(&block.directives);
                }
                ConfigElement::Include(include) => {
                    pending_comment = None;
                    for file in &include.resolved_files {
                        Self::build_display_list_from_included(
                            &file.elements,
                            &file.path,
                            hosts,
                            &mut host_index,
                            &mut display_list,
                        );
                    }
                }
            }
        }

        // Append pattern group at the bottom
        if !patterns.is_empty() {
            let mut pattern_index = 0usize;
            display_list.push(HostListItem::GroupHeader("Patterns".to_string()));
            Self::append_pattern_items(&config.elements, &mut pattern_index, &mut display_list);
            debug_assert_eq!(
                pattern_index,
                patterns.len(),
                "append_pattern_items and collect_pattern_entries traversal mismatch"
            );
        }
        Self::append_match_block_items(match_blocks.len(), &mut display_list);

        display_list
    }

    /// Append the Match blocks group below the patterns, in reading order.
    fn append_match_block_items(count: usize, display_list: &mut Vec<HostListItem>) {
        if count == 0 {
            return;
        }
        let header = crate::messages::MATCH_BLOCKS_GROUP.to_string();
        display_list.push(HostListItem::GroupHeader(header));
        display_list.extend((0..count).map(|index| HostListItem::MatchBlock { index }));
    }

    fn append_pattern_items(
        elements: &[ConfigElement],
        pattern_index: &mut usize,
        display_list: &mut Vec<HostListItem>,
    ) {
        for e in elements {
            match e {
                ConfigElement::HostBlock(block) => {
                    if crate::ssh_config::model::is_host_pattern(&block.host_pattern) {
                        display_list.push(HostListItem::Pattern {
                            index: *pattern_index,
                        });
                        *pattern_index += 1;
                    }
                }
                ConfigElement::Include(include) => {
                    for file in &include.resolved_files {
                        Self::append_pattern_items(&file.elements, pattern_index, display_list);
                    }
                }
                ConfigElement::GlobalLine(_) => {}
            }
        }
    }

    /// Group header text for a comment line (text after `#`). purple metadata
    /// such as `purple:tags` in a Match block is not a header; `purple:group` is.
    fn comment_header(rest: &str) -> Option<String> {
        let text = rest.trim();
        if text.starts_with("purple:") && !text.starts_with("purple:group ") {
            return None;
        }
        let text = text.strip_prefix("purple:group ").unwrap_or(text);
        (!text.is_empty()).then(|| text.to_string())
    }

    /// Extract a trailing comment from a block's directives.
    /// If the last non-blank line in the directives is a comment, return it as
    /// a potential group header for the next host block.
    /// Strips `purple:group ` prefix so headers display as the provider name.
    fn extract_trailing_comment(
        directives: &[crate::ssh_config::model::Directive],
    ) -> Option<String> {
        let d = directives.last()?;
        if !d.is_non_directive {
            return None;
        }
        let trimmed = d.raw_line.trim();
        if trimmed.is_empty() {
            return None;
        }
        if let Some(rest) = trimmed.strip_prefix('#') {
            let text = rest.trim();
            // Skip purple metadata comments (purple:provider, purple:tags)
            // Only purple:group should produce a group header
            if text.starts_with("purple:") && !text.starts_with("purple:group ") {
                return None;
            }
            let text = text.strip_prefix("purple:group ").unwrap_or(text);
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
        None
    }

    fn build_display_list_from_included(
        elements: &[ConfigElement],
        file_path: &std::path::Path,
        hosts: &[HostEntry],
        host_index: &mut usize,
        display_list: &mut Vec<HostListItem>,
    ) {
        let mut pending_comment: Option<String> = None;
        let file_name = file_path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();

        // Add file header for included files
        if !file_name.is_empty() {
            let has_hosts = elements.iter().any(|e| {
                matches!(e, ConfigElement::HostBlock(b)
                    if !crate::ssh_config::model::is_host_pattern(&b.host_pattern)
                )
            });
            if has_hosts {
                display_list.push(HostListItem::GroupHeader(file_name));
            }
        }

        for element in elements {
            match element {
                ConfigElement::GlobalLine(line) => {
                    let trimmed = line.trim();
                    if let Some(rest) = trimmed.strip_prefix('#') {
                        if let Some(header) = Self::comment_header(rest) {
                            pending_comment = Some(header);
                        }
                    } else {
                        pending_comment = None;
                    }
                }
                ConfigElement::HostBlock(block) => {
                    if crate::ssh_config::model::is_host_pattern(&block.host_pattern) {
                        pending_comment = None;
                        continue;
                    }

                    if *host_index < hosts.len() {
                        if let Some(header) = pending_comment.take() {
                            display_list.push(HostListItem::GroupHeader(header));
                        }
                        display_list.push(HostListItem::Host { index: *host_index });
                        *host_index += 1;
                    }

                    // Extract trailing comments from this block for the next host
                    pending_comment = Self::extract_trailing_comment(&block.directives);
                }
                ConfigElement::Include(include) => {
                    pending_comment = None;
                    for file in &include.resolved_files {
                        Self::build_display_list_from_included(
                            &file.elements,
                            &file.path,
                            hosts,
                            host_index,
                            display_list,
                        );
                    }
                }
            }
        }
    }

    /// Rebuild the display list based on the current sort mode and group_by toggle.
    pub fn apply_sort(&mut self) {
        log::debug!(
            "[purple] apply_sort: mode={} group_by={:?} hosts={}",
            self.hosts_state.sort_mode.to_key(),
            self.hosts_state.group_by,
            self.hosts_state.list.len()
        );
        // Preserve the selected host, pattern or Match block across sort changes
        let selected_alias = self
            .selected_host()
            .map(|h| h.alias.clone())
            .or_else(|| self.selected_pattern().map(|p| p.pattern.clone()));
        let selected_match = self.selected_match_block_key();

        // Multi-select indices become visually misleading after reorder
        self.hosts_state.multi_select.clear();
        // display_list is about to be rebuilt; group_alias_map depends on it
        self.hosts_state.render_cache.invalidate();

        if self.hosts_state.sort_mode == SortMode::Original
            && matches!(self.hosts_state.group_by, GroupBy::None)
        {
            self.hosts_state.display_list = Self::build_display_list_from(
                &self.hosts_state.ssh_config,
                &self.hosts_state.list,
                &self.hosts_state.patterns,
                &self.hosts_state.match_blocks,
            );
        } else if self.hosts_state.sort_mode == SortMode::Original
            && !matches!(self.hosts_state.group_by, GroupBy::None)
        {
            // Original order but grouped: extract flat indices from config order
            let indices: Vec<usize> = (0..self.hosts_state.list.len()).collect();
            self.hosts_state.display_list = self.group_indices(&indices);
        } else {
            let mut indices: Vec<usize> = (0..self.hosts_state.list.len()).collect();
            match self.hosts_state.sort_mode {
                SortMode::AlphaAlias => {
                    indices.sort_by_cached_key(|&i| {
                        let stale = self.hosts_state.list[i].stale.is_some();
                        (stale, self.hosts_state.list[i].alias.to_ascii_lowercase())
                    });
                }
                SortMode::AlphaHostname => {
                    indices.sort_by_cached_key(|&i| {
                        let stale = self.hosts_state.list[i].stale.is_some();
                        (
                            stale,
                            self.hosts_state.list[i].hostname.to_ascii_lowercase(),
                        )
                    });
                }
                SortMode::Frecency => {
                    indices.sort_by(|a, b| {
                        let sa = self.hosts_state.list[*a].stale.is_some();
                        let sb = self.hosts_state.list[*b].stale.is_some();
                        sa.cmp(&sb).then_with(|| {
                            let score_a = self
                                .history
                                .frecency_score(&self.hosts_state.list[*a].alias);
                            let score_b = self
                                .history
                                .frecency_score(&self.hosts_state.list[*b].alias);
                            score_b.total_cmp(&score_a)
                        })
                    });
                }
                SortMode::MostRecent => {
                    indices.sort_by(|a, b| {
                        let sa = self.hosts_state.list[*a].stale.is_some();
                        let sb = self.hosts_state.list[*b].stale.is_some();
                        sa.cmp(&sb).then_with(|| {
                            let ts_a = self
                                .history
                                .last_connected(&self.hosts_state.list[*a].alias);
                            let ts_b = self
                                .history
                                .last_connected(&self.hosts_state.list[*b].alias);
                            ts_b.cmp(&ts_a)
                        })
                    });
                }
                SortMode::Status => {
                    indices.sort_by(|a, b| {
                        let sa = self.hosts_state.list[*a].stale.is_some();
                        let sb = self.hosts_state.list[*b].stale.is_some();
                        sa.cmp(&sb).then_with(|| {
                            let pa = self.ping.status.get(&self.hosts_state.list[*a].alias);
                            let pb = self.ping.status.get(&self.hosts_state.list[*b].alias);
                            super::ping_sort_key(pa).cmp(&super::ping_sort_key(pb))
                        })
                    });
                }
                _ => {}
            }
            self.hosts_state.display_list = self.group_indices(&indices);
        }

        // Append the pattern and Match block groups at the bottom (sorted and
        // grouped paths skip build_display_list_from, which already does this)
        let rebuilt_by_sort = self.hosts_state.sort_mode != SortMode::Original
            || !matches!(self.hosts_state.group_by, GroupBy::None);
        if rebuilt_by_sort && !self.hosts_state.patterns.is_empty() {
            self.hosts_state
                .display_list
                .push(HostListItem::GroupHeader("Patterns".to_string()));
            let mut pattern_index = 0usize;
            Self::append_pattern_items(
                &self.hosts_state.ssh_config.elements,
                &mut pattern_index,
                &mut self.hosts_state.display_list,
            );
        }
        if rebuilt_by_sort {
            Self::append_match_block_items(
                self.hosts_state.match_blocks.len(),
                &mut self.hosts_state.display_list,
            );
        }

        // Compute group host counts before group filtering
        {
            self.hosts_state.group_host_counts.clear();
            let mut current_group: Option<&str> = None;
            for item in &self.hosts_state.display_list {
                match item {
                    HostListItem::GroupHeader(text) => {
                        current_group = Some(text.as_str());
                    }
                    HostListItem::Host { .. }
                    | HostListItem::Pattern { .. }
                    | HostListItem::MatchBlock { .. } => {
                        if let Some(group) = current_group {
                            *self
                                .hosts_state
                                .group_host_counts
                                .entry(group.to_string())
                                .or_insert(0) += 1;
                        }
                    }
                }
            }
        }

        // Build group tab order. For tag mode, compute from host tags (matching
        // render_group_bar's tab list). For provider mode, extract from GroupHeaders.
        self.hosts_state.group_tab_order = match &self.hosts_state.group_by {
            GroupBy::Tag(_) => {
                let mut tag_counts: HashMap<String, usize> = HashMap::new();
                for host in &self.hosts_state.list {
                    for tag in host.tags.iter() {
                        *tag_counts.entry(tag.clone()).or_insert(0) += 1;
                    }
                }
                for pattern in &self.hosts_state.patterns {
                    for tag in &pattern.tags {
                        *tag_counts.entry(tag.clone()).or_insert(0) += 1;
                    }
                }
                for block in &self.hosts_state.match_blocks {
                    for tag in &block.tags {
                        *tag_counts.entry(tag.clone()).or_insert(0) += 1;
                    }
                }
                let mut sorted: Vec<(String, usize)> = tag_counts.into_iter().collect();
                sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                let top: Vec<(String, usize)> = sorted.into_iter().take(10).collect();
                self.hosts_state.group_host_counts =
                    top.iter().map(|(t, c)| (t.clone(), *c)).collect();
                top.into_iter().map(|(t, _)| t).collect()
            }
            _ => {
                let mut order = Vec::new();
                let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
                for item in &self.hosts_state.display_list {
                    if let HostListItem::GroupHeader(text) = item
                        && seen.insert(text.as_str())
                    {
                        order.push(text.clone());
                    }
                }
                order
            }
        };

        // Filter by group if active
        if let Some(ref filter) = self.hosts_state.group_filter {
            let is_tag_mode = matches!(self.hosts_state.group_by, GroupBy::Tag(_));
            let mut filtered = Vec::with_capacity(self.hosts_state.display_list.len());

            if is_tag_mode {
                // In tag mode, filter by host tags directly (GroupHeaders don't
                // cover all tags, only the active GroupBy tag).
                for item in std::mem::take(&mut self.hosts_state.display_list) {
                    match &item {
                        HostListItem::GroupHeader(_) => {} // skip all headers
                        HostListItem::Host { index } => {
                            if let Some(host) = self.hosts_state.list.get(*index)
                                && host
                                    .tags
                                    .iter()
                                    .chain(host.provider_tags.iter())
                                    .any(|t| t == filter)
                            {
                                filtered.push(item);
                            }
                        }
                        HostListItem::Pattern { index } => {
                            if let Some(pattern) = self.hosts_state.patterns.get(*index)
                                && pattern.tags.iter().any(|t| t == filter)
                            {
                                filtered.push(item);
                            }
                        }
                        HostListItem::MatchBlock { index } => {
                            if let Some(block) = self.hosts_state.match_blocks.get(*index)
                                && block.tags.iter().any(|t| t == filter)
                            {
                                filtered.push(item);
                            }
                        }
                    }
                }
            } else {
                // In provider/none mode, filter by GroupHeader matching
                let mut in_group = false;
                for item in std::mem::take(&mut self.hosts_state.display_list) {
                    match &item {
                        HostListItem::GroupHeader(text) => {
                            in_group = text == filter;
                        }
                        _ => {
                            if in_group {
                                filtered.push(item);
                            }
                        }
                    }
                }
            }

            self.hosts_state.display_list = filtered;
        }

        // Restore selection by alias or Match block, fall back to first host
        if let Some(alias) = selected_alias {
            self.select_host_by_alias(&alias);
            if self.selected_host().is_some() || self.selected_pattern().is_some() {
                return;
            }
        }
        if let Some(key) = selected_match {
            self.select_match_block_by_key(&key);
            if self.selected_match_block().is_some() {
                return;
            }
        }
        self.select_first_host();
    }

    /// Select the first selectable item in the display list (always skips headers).
    pub fn select_first_host(&mut self) {
        if let Some(pos) = self.hosts_state.display_list.iter().position(|item| {
            matches!(
                item,
                HostListItem::Host { .. }
                    | HostListItem::Pattern { .. }
                    | HostListItem::MatchBlock { .. }
            )
        }) {
            self.ui.list_state.select(Some(pos));
        }
    }

    /// Group EVERY host by the current `GroupBy`, ignoring the active search
    /// and group filters. For host-target pickers (e.g. running a snippet on a
    /// group) that must show every host, grouped exactly the way the host list
    /// is. Reuses the same `group_indices` dispatch as the list, so grouping
    /// stays in one place. Hosts keep config order within each group.
    pub(crate) fn grouped_all_hosts(&self) -> Vec<HostListItem> {
        let all: Vec<usize> = (0..self.hosts_state.list.len()).collect();
        self.group_indices(&all)
    }

    /// Partition sorted indices by provider, inserting group headers.
    /// Hosts without provider appear first (no header), then named provider
    /// groups (in first-appearance order) with headers.
    fn group_indices(&self, sorted_indices: &[usize]) -> Vec<HostListItem> {
        match &self.hosts_state.group_by {
            GroupBy::None => sorted_indices
                .iter()
                .map(|&i| HostListItem::Host { index: i })
                .collect(),
            GroupBy::Provider => {
                Self::group_indices_by_provider(&self.hosts_state.list, sorted_indices)
            }
            GroupBy::Tag(tag) => {
                Self::group_indices_by_tag(&self.hosts_state.list, sorted_indices, tag)
            }
        }
    }

    fn group_indices_by_provider(
        hosts: &[HostEntry],
        sorted_indices: &[usize],
    ) -> Vec<HostListItem> {
        let mut none_indices: Vec<usize> = Vec::new();
        let mut provider_groups: Vec<(&str, Vec<usize>)> = Vec::new();
        let mut provider_order: HashMap<&str, usize> = HashMap::new();

        for &idx in sorted_indices {
            match &hosts[idx].provider {
                None => none_indices.push(idx),
                Some(name) => {
                    if let Some(&group_idx) = provider_order.get(name.as_str()) {
                        provider_groups[group_idx].1.push(idx);
                    } else {
                        let group_idx = provider_groups.len();
                        provider_order.insert(name, group_idx);
                        provider_groups.push((name, vec![idx]));
                    }
                }
            }
        }

        let mut display_list = Vec::new();

        // Non-provider hosts first (no header)
        for idx in &none_indices {
            display_list.push(HostListItem::Host { index: *idx });
        }

        // Then provider groups with headers
        for (name, indices) in &provider_groups {
            let header = crate::providers::provider_display_name(name);
            display_list.push(HostListItem::GroupHeader(header.to_string()));
            for &idx in indices {
                display_list.push(HostListItem::Host { index: idx });
            }
        }
        display_list
    }

    /// Partition sorted indices by a user tag, inserting a group header.
    /// Hosts without the tag appear first (no header), then hosts with the
    /// tag appear under a single group header.
    fn group_indices_by_tag(
        hosts: &[HostEntry],
        sorted_indices: &[usize],
        tag: &str,
    ) -> Vec<HostListItem> {
        let mut without_tag: Vec<usize> = Vec::new();
        let mut with_tag: Vec<usize> = Vec::new();

        for &idx in sorted_indices {
            if hosts[idx].tags.iter().any(|t| t == tag) {
                with_tag.push(idx);
            } else {
                without_tag.push(idx);
            }
        }

        let mut display_list = Vec::new();

        for idx in &without_tag {
            display_list.push(HostListItem::Host { index: *idx });
        }

        if !with_tag.is_empty() {
            display_list.push(HostListItem::GroupHeader(tag.to_string()));
            for &idx in &with_tag {
                display_list.push(HostListItem::Host { index: idx });
            }
        }

        display_list
    }
}
