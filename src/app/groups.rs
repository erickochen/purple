//! Group tab navigation. Implements `impl App` continuation with group-related
//! methods: cycling through group tabs, clearing group filters, and keeping the
//! tab selection in sync with the currently highlighted host.

use super::{GroupBy, HostListItem};
use crate::app::App;

impl App {
    /// Auto-follow: update group_tab_index based on selected host's group.
    pub(crate) fn update_group_tab_follow(&mut self) {
        if self.group_filter.is_some() {
            return;
        }
        let selected = self.ui.list_state.selected().unwrap_or(0);

        // In tag mode, match the selected host's tags against the tab order
        // directly, because the display list only has one GroupHeader (the
        // active GroupBy tag) while the tab bar shows the top-10 tags.
        if matches!(self.group_by, GroupBy::Tag(_)) {
            let tags: Option<&[String]> = match self.display_list.get(selected) {
                Some(HostListItem::Host { index }) => {
                    self.hosts.get(*index).map(|h| h.tags.as_slice())
                }
                Some(HostListItem::Pattern { index }) => {
                    self.patterns.get(*index).map(|p| p.tags.as_slice())
                }
                _ => None,
            };
            if let Some(item_tags) = tags {
                for (idx, tab_tag) in self.group_tab_order.iter().enumerate() {
                    if item_tags.iter().any(|t| t == tab_tag) {
                        self.group_tab_index = idx + 1;
                        return;
                    }
                }
            }
            self.group_tab_index = 0;
            return;
        }

        // Provider/none mode: walk backwards to find the nearest GroupHeader
        for i in (0..=selected).rev() {
            if let HostListItem::GroupHeader(name) = &self.display_list[i] {
                self.group_tab_index = self
                    .group_tab_order
                    .iter()
                    .position(|g| g == name)
                    .map(|idx| idx + 1)
                    .unwrap_or(0);
                return;
            }
        }
        self.group_tab_index = 0;
    }

    /// Cycle to the next group tab (Tab key). All -> group1 -> ... -> groupN -> All.
    pub fn next_group_tab(&mut self) {
        let group_count = self.group_tab_order.len();
        if group_count == 0 {
            return;
        }
        match &self.group_filter {
            None => {
                self.group_filter = Some(self.group_tab_order[0].clone());
                self.group_tab_index = 1;
            }
            Some(current) => {
                let pos = self
                    .group_tab_order
                    .iter()
                    .position(|g| g == current)
                    .unwrap_or(0);
                let next = pos + 1;
                if next >= group_count {
                    // Wrap back to "All"
                    self.group_filter = None;
                    self.group_tab_index = 0;
                } else {
                    self.group_filter = Some(self.group_tab_order[next].clone());
                    self.group_tab_index = next + 1;
                }
            }
        }
        self.apply_sort();
        // Select first host in list
        for (i, item) in self.display_list.iter().enumerate() {
            if matches!(item, HostListItem::Host { .. }) {
                self.ui.list_state.select(Some(i));
                break;
            }
        }
    }

    /// Cycle to the previous group tab (Shift+Tab key). All <- group1 <- ... <- groupN.
    pub fn prev_group_tab(&mut self) {
        let group_count = self.group_tab_order.len();
        if group_count == 0 {
            return;
        }
        match &self.group_filter {
            None => {
                // From All, go to last group
                let last = group_count - 1;
                self.group_filter = Some(self.group_tab_order[last].clone());
                self.group_tab_index = last + 1;
            }
            Some(current) => {
                let pos = self
                    .group_tab_order
                    .iter()
                    .position(|g| g == current)
                    .unwrap_or(0);
                if pos == 0 {
                    // Wrap back to "All"
                    self.group_filter = None;
                    self.group_tab_index = 0;
                } else {
                    let prev = pos - 1;
                    self.group_filter = Some(self.group_tab_order[prev].clone());
                    self.group_tab_index = prev + 1;
                }
            }
        }
        self.apply_sort();
        for (i, item) in self.display_list.iter().enumerate() {
            if matches!(item, HostListItem::Host { .. }) {
                self.ui.list_state.select(Some(i));
                break;
            }
        }
    }

    /// Clear group filter (Esc from filtered mode).
    pub fn clear_group_filter(&mut self) {
        if self.group_filter.is_none() {
            return;
        }
        self.group_filter = None;
        self.group_tab_index = 0;
        self.apply_sort();
        for (i, item) in self.display_list.iter().enumerate() {
            if matches!(item, HostListItem::Host { .. }) {
                self.ui.list_state.select(Some(i));
                break;
            }
        }
    }
}
