use std::collections::{HashMap, HashSet};

use ratatui::text::Span;

use crate::app::ping::PingStatus;
use crate::ssh_config::model::{
    ConfigElement, HostEntry, MatchBlockEntry, PatternEntry, SshConfigFile,
};
use crate::ui::theme;

/// Host, group, sort and view state grouped off the `App` god-struct. Holds
/// the parsed `~/.ssh/config`, the resolved host + pattern entries, the
/// display list built from them, the render cache, the undo stack for
/// deletions, the multi-select set for bulk snippet runs and all sort /
/// group / view UI-state. Pure state container.
pub struct HostState {
    pub(in crate::app) ssh_config: SshConfigFile,
    pub(in crate::app) list: Vec<HostEntry>,
    pub(in crate::app) patterns: Vec<PatternEntry>,
    pub(in crate::app) match_blocks: Vec<MatchBlockEntry>,
    pub(in crate::app) display_list: Vec<HostListItem>,
    pub(in crate::app) render_cache: HostListRenderCache,
    pub(in crate::app) undo_stack: Vec<DeletedHost>,
    /// Host indices selected for multi-host snippet execution (space to toggle).
    pub(in crate::app) multi_select: HashSet<usize>,
    pub(in crate::app) sort_mode: SortMode,
    pub(in crate::app) group_by: GroupBy,
    pub(in crate::app) view_mode: ViewMode,
    /// Currently active group filter. None = show all groups.
    pub(in crate::app) group_filter: Option<String>,
    /// Ordered list of group names from the current display list.
    pub(in crate::app) group_tab_order: Vec<String>,
    /// Host/pattern counts per group (computed before group filtering).
    pub(in crate::app) group_host_counts: HashMap<String, usize>,
}

impl HostState {
    /// Construct from a loaded config and pre-resolved host, pattern and
    /// Match block lists.
    pub fn from_config(
        ssh_config: SshConfigFile,
        hosts: Vec<HostEntry>,
        patterns: Vec<PatternEntry>,
        match_blocks: Vec<MatchBlockEntry>,
        display_list: Vec<HostListItem>,
    ) -> Self {
        Self {
            ssh_config,
            list: hosts,
            patterns,
            match_blocks,
            display_list,
            render_cache: HostListRenderCache::default(),
            undo_stack: Vec::new(),
            multi_select: HashSet::new(),
            sort_mode: SortMode::Original,
            group_by: GroupBy::None,
            view_mode: ViewMode::Compact,
            group_filter: None,
            group_tab_order: Vec::new(),
            group_host_counts: HashMap::new(),
        }
    }

    /// Change the group-by mode and reset any active group filter in
    /// lockstep. Callers that change `group_by` directly would leave a
    /// stale `group_filter` referring to a group that no longer exists.
    pub fn set_group_by(&mut self, by: GroupBy) {
        self.group_by = by;
        self.group_filter = None;
    }

    /// Flip the host list between Compact and Detailed view.
    pub fn toggle_view_mode(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::Compact => ViewMode::Detailed,
            ViewMode::Detailed => ViewMode::Compact,
        };
    }

    /// Toggle multi-select membership for the host at `idx`. Returns
    /// `true` when `idx` is now selected (was inserted) and `false` when
    /// it is now unselected (was removed) so the caller can react
    /// without re-reading the set.
    pub fn toggle_multi_select(&mut self, idx: usize) -> bool {
        let inserted = !self.multi_select.contains(&idx);
        if inserted {
            self.multi_select.insert(idx);
        } else {
            self.multi_select.remove(&idx);
        }
        inserted
    }

    pub fn ssh_config(&self) -> &SshConfigFile {
        &self.ssh_config
    }

    pub fn ssh_config_mut(&mut self) -> &mut SshConfigFile {
        &mut self.ssh_config
    }

    pub fn set_ssh_config(&mut self, config: SshConfigFile) {
        self.ssh_config = config;
    }

    pub fn list(&self) -> &Vec<HostEntry> {
        &self.list
    }

    pub fn list_mut(&mut self) -> &mut Vec<HostEntry> {
        &mut self.list
    }

    pub fn patterns(&self) -> &Vec<PatternEntry> {
        &self.patterns
    }

    pub fn patterns_mut(&mut self) -> &mut Vec<PatternEntry> {
        &mut self.patterns
    }

    pub fn match_blocks(&self) -> &Vec<MatchBlockEntry> {
        &self.match_blocks
    }

    pub fn match_blocks_mut(&mut self) -> &mut Vec<MatchBlockEntry> {
        &mut self.match_blocks
    }

    pub fn display_list(&self) -> &Vec<HostListItem> {
        &self.display_list
    }

    pub fn display_list_mut(&mut self) -> &mut Vec<HostListItem> {
        &mut self.display_list
    }

    pub fn render_cache(&self) -> &HostListRenderCache {
        &self.render_cache
    }

    pub fn render_cache_mut(&mut self) -> &mut HostListRenderCache {
        &mut self.render_cache
    }

    /// Invalidate the host-list render cache after a mutation.
    pub fn invalidate_render_cache(&mut self) {
        self.render_cache.invalidate();
    }

    pub fn undo_stack(&self) -> &Vec<DeletedHost> {
        &self.undo_stack
    }

    pub fn undo_stack_mut(&mut self) -> &mut Vec<DeletedHost> {
        &mut self.undo_stack
    }

    /// Drop the most recent deletion off the undo stack, if any.
    pub fn pop_undo(&mut self) -> Option<DeletedHost> {
        self.undo_stack.pop()
    }

    /// Clear the undo stack. Positions may have shifted after a reload.
    pub fn clear_undo(&mut self) {
        self.undo_stack.clear();
    }

    pub fn multi_select(&self) -> &HashSet<usize> {
        &self.multi_select
    }

    pub fn multi_select_mut(&mut self) -> &mut HashSet<usize> {
        &mut self.multi_select
    }

    /// Clear the multi-select set. Idempotent.
    pub fn clear_multi_select(&mut self) {
        self.multi_select.clear();
    }

    pub fn sort_mode(&self) -> SortMode {
        self.sort_mode
    }

    pub fn set_sort_mode(&mut self, mode: SortMode) {
        self.sort_mode = mode;
    }

    /// Advance the sort mode to the next variant in the cycle.
    pub fn advance_sort_mode(&mut self) {
        self.sort_mode = self.sort_mode.next();
    }

    pub fn group_by(&self) -> &GroupBy {
        &self.group_by
    }

    /// Set the group-by mode without touching the active group filter.
    /// Use when restoring saved state. `set_group_by` resets the filter.
    pub fn set_group_by_raw(&mut self, by: GroupBy) {
        self.group_by = by;
    }

    pub fn view_mode(&self) -> ViewMode {
        self.view_mode
    }

    pub fn set_view_mode(&mut self, mode: ViewMode) {
        self.view_mode = mode;
    }

    pub fn group_filter(&self) -> Option<&String> {
        self.group_filter.as_ref()
    }

    pub fn set_group_filter(&mut self, filter: Option<String>) {
        self.group_filter = filter;
    }

    pub fn group_tab_order(&self) -> &Vec<String> {
        &self.group_tab_order
    }

    pub fn group_host_counts(&self) -> &HashMap<String, usize> {
        &self.group_host_counts
    }
}

#[cfg(test)]
impl Default for HostState {
    fn default() -> Self {
        Self {
            ssh_config: SshConfigFile {
                elements: Vec::new(),
                path: std::path::PathBuf::new(),
                crlf: false,
                bom: false,
            },
            list: Vec::new(),
            patterns: Vec::new(),
            match_blocks: Vec::new(),
            display_list: Vec::new(),
            render_cache: HostListRenderCache::default(),
            undo_stack: Vec::new(),
            multi_select: HashSet::new(),
            sort_mode: SortMode::Original,
            group_by: GroupBy::None,
            view_mode: ViewMode::Compact,
            group_filter: None,
            group_tab_order: Vec::new(),
            group_host_counts: HashMap::new(),
        }
    }
}

/// An item in the display list (hosts + group headers).
#[derive(Debug, Clone)]
pub enum HostListItem {
    GroupHeader(String),
    Host { index: usize },
    Pattern { index: usize },
    MatchBlock { index: usize },
}

impl HostListItem {
    /// Rows the cursor can land on; group headers are skipped.
    pub fn is_selectable(&self) -> bool {
        !matches!(self, HostListItem::GroupHeader(_))
    }
}

/// View mode for the host list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViewMode {
    Compact,
    Detailed,
}

/// Sort mode for the host list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortMode {
    Original,
    AlphaAlias,
    AlphaHostname,
    Frecency,
    MostRecent,
    Status,
}

impl SortMode {
    pub fn next(self) -> Self {
        match self {
            SortMode::Original => SortMode::AlphaAlias,
            SortMode::AlphaAlias => SortMode::AlphaHostname,
            SortMode::AlphaHostname => SortMode::Frecency,
            SortMode::Frecency => SortMode::MostRecent,
            SortMode::MostRecent => SortMode::Status,
            SortMode::Status => SortMode::Original,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SortMode::Original => "config order",
            SortMode::AlphaAlias => "A-Z alias",
            SortMode::AlphaHostname => "A-Z hostname",
            SortMode::Frecency => "most used",
            SortMode::MostRecent => "most recent",
            SortMode::Status => "down first",
        }
    }

    pub fn to_key(self) -> &'static str {
        match self {
            SortMode::Original => "original",
            SortMode::AlphaAlias => "alpha_alias",
            SortMode::AlphaHostname => "alpha_hostname",
            SortMode::Frecency => "frecency",
            SortMode::MostRecent => "most_recent",
            SortMode::Status => "status",
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s {
            "original" => SortMode::Original,
            "alpha_alias" => SortMode::AlphaAlias,
            "alpha_hostname" => SortMode::AlphaHostname,
            "frecency" => SortMode::Frecency,
            "most_recent" => SortMode::MostRecent,
            "status" => SortMode::Status,
            _ => SortMode::MostRecent,
        }
    }
}

/// Build health summary spans: ●23 ▲2 ✖1 ○1
/// Only includes states with count > 0. Returns empty vec if no pings.
pub fn health_summary_spans(
    ping_status: &HashMap<String, PingStatus>,
    hosts: &[HostEntry],
) -> Vec<Span<'static>> {
    health_summary_spans_for(ping_status, hosts.iter().map(|h| h.alias.as_str()))
}

/// Build health summary spans for a subset of host aliases.
/// Only includes states with count > 0. Returns empty vec if no pings.
pub fn health_summary_spans_for<'a>(
    ping_status: &HashMap<String, PingStatus>,
    aliases: impl Iterator<Item = &'a str>,
) -> Vec<Span<'static>> {
    if ping_status.is_empty() {
        return vec![];
    }
    let mut online = 0u32;
    let mut slow = 0u32;
    let mut down = 0u32;
    let mut unchecked = 0u32;
    for alias in aliases {
        match ping_status.get(alias) {
            Some(PingStatus::Reachable { .. }) => online += 1,
            Some(PingStatus::Slow { .. }) => slow += 1,
            Some(PingStatus::Unreachable) => down += 1,
            Some(PingStatus::Checking) | None => unchecked += 1,
            Some(PingStatus::Skipped) => {} // ProxyJump, excluded
        }
    }
    let mut spans = Vec::new();
    if online > 0 {
        spans.push(Span::styled(
            format!("\u{25CF}{online}"),
            theme::online_dot(),
        ));
    }
    if slow > 0 {
        if !spans.is_empty() {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(format!("\u{25B2}{slow}"), theme::warning()));
    }
    if down > 0 {
        if !spans.is_empty() {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(format!("\u{2716}{down}"), theme::error()));
    }
    if unchecked > 0 {
        if !spans.is_empty() {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(format!("\u{25CB}{unchecked}"), theme::muted()));
    }
    spans
}

/// Group mode for the host list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupBy {
    None,
    Provider,
    Tag(String),
}

impl GroupBy {
    pub fn to_key(&self) -> String {
        match self {
            GroupBy::None => "none".to_string(),
            GroupBy::Provider => "provider".to_string(),
            GroupBy::Tag(tag) => format!("tag:{}", tag),
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s {
            "none" => GroupBy::None,
            "provider" => GroupBy::Provider,
            s if s.starts_with("tag:") => match s.strip_prefix("tag:") {
                Some(tag) => GroupBy::Tag(tag.to_string()),
                _ => GroupBy::None,
            },
            _ => GroupBy::None,
        }
    }

    pub fn label(&self) -> String {
        match self {
            GroupBy::None => "ungrouped".to_string(),
            GroupBy::Provider => "provider".to_string(),
            GroupBy::Tag(tag) => format!("tag: {}", tag),
        }
    }
}

/// Stores a deleted host for undo.
#[derive(Debug, Clone)]
pub struct DeletedHost {
    pub element: ConfigElement,
    pub position: usize,
}

/// Item in the ProxyJump picker list. Scored hosts (used elsewhere as
/// ProxyJump, matching a jump-host name pattern, or sharing the editing
/// host's domain suffix) are promoted above a visual separator so the
/// likely pick is at the top and the rest stays alphabetical below.
/// `SectionLabel` renders a non-selectable heading (e.g. "Suggestions")
/// above the scored section. Navigation skips both `SectionLabel` and
/// `Separator`.
#[derive(Debug, Clone, PartialEq)]
pub enum ProxyJumpCandidate {
    Host {
        alias: String,
        hostname: String,
        suggested: bool,
    },
    SectionLabel(&'static str),
    Separator,
}

/// Lazily-computed derived state that feeds the host-list renderer.
///
/// The renderer runs on every keystroke and every animation tick. Rebuilding
/// these from `hosts`/`display_list`/`history` per frame allocates thousands
/// of short-lived `String`s on hosts lists in the 500+ range. Fields are
/// `None` when dirty; the renderer populates them on first use after an
/// invalidation and subsequent frames reuse the values until the next
/// mutation calls `invalidate()`.
#[derive(Default)]
pub struct HostListRenderCache {
    /// Max width of formatted "last connected" strings across all hosts.
    /// Caches the `format_time_ago` allocations.
    pub history_width: Option<usize>,
    /// Group-header text -> host aliases in that group. Built from
    /// `display_list`, so invalidates on every sort/filter/reload.
    pub group_alias_map: Option<HashMap<String, Vec<String>>>,
}

impl HostListRenderCache {
    pub fn invalidate(&mut self) {
        self.history_width = None;
        self.group_alias_map = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_empty() {
        let s = HostState::default();
        assert!(s.list.is_empty());
        assert!(s.patterns.is_empty());
        assert!(s.match_blocks.is_empty());
        assert!(s.display_list.is_empty());
        assert!(s.undo_stack.is_empty());
        assert!(s.multi_select.is_empty());
        assert!(s.group_filter.is_none());
        assert!(s.group_tab_order.is_empty());
        assert!(s.group_host_counts.is_empty());
    }

    #[test]
    fn set_group_by_provider_clears_filter() {
        let mut s = HostState {
            group_filter: Some("acme".to_string()),
            ..Default::default()
        };
        s.set_group_by(GroupBy::Provider);
        assert!(matches!(s.group_by, GroupBy::Provider));
        assert!(s.group_filter.is_none());
    }

    #[test]
    fn set_group_by_none_clears_filter() {
        let mut s = HostState {
            group_by: GroupBy::Provider,
            group_filter: Some("acme".to_string()),
            ..Default::default()
        };
        s.set_group_by(GroupBy::None);
        assert!(matches!(s.group_by, GroupBy::None));
        assert!(s.group_filter.is_none());
    }

    #[test]
    fn set_group_by_tag_clears_filter() {
        let mut s = HostState {
            group_filter: Some("prod".to_string()),
            ..Default::default()
        };
        s.set_group_by(GroupBy::Tag("staging".to_string()));
        match &s.group_by {
            GroupBy::Tag(t) => assert_eq!(t, "staging"),
            _ => panic!("expected Tag, got {:?}", s.group_by),
        }
        assert!(s.group_filter.is_none());
    }

    #[test]
    fn set_group_by_overwrites_existing() {
        let mut s = HostState {
            group_by: GroupBy::Provider,
            ..Default::default()
        };
        s.set_group_by(GroupBy::None);
        assert!(matches!(s.group_by, GroupBy::None));
    }

    #[test]
    fn toggle_view_mode_compact_to_detailed() {
        let mut s = HostState::default();
        assert_eq!(s.view_mode, ViewMode::Compact);
        s.toggle_view_mode();
        assert_eq!(s.view_mode, ViewMode::Detailed);
    }

    #[test]
    fn toggle_view_mode_detailed_to_compact() {
        let mut s = HostState {
            view_mode: ViewMode::Detailed,
            ..Default::default()
        };
        s.toggle_view_mode();
        assert_eq!(s.view_mode, ViewMode::Compact);
    }

    #[test]
    fn toggle_multi_select_inserts_when_absent_and_returns_true() {
        let mut s = HostState::default();
        let now_selected = s.toggle_multi_select(3);
        assert!(now_selected);
        assert!(s.multi_select.contains(&3));
    }

    #[test]
    fn toggle_multi_select_removes_when_present_and_returns_false() {
        let mut s = HostState::default();
        s.multi_select.insert(3);
        let now_selected = s.toggle_multi_select(3);
        assert!(!now_selected);
        assert!(!s.multi_select.contains(&3));
    }

    #[test]
    fn toggle_multi_select_does_not_touch_other_indices() {
        let mut s = HostState::default();
        s.multi_select.insert(1);
        s.multi_select.insert(2);
        s.toggle_multi_select(3);
        assert!(s.multi_select.contains(&1));
        assert!(s.multi_select.contains(&2));
        assert!(s.multi_select.contains(&3));
    }

    #[test]
    fn advance_sort_mode_steps_to_next_variant() {
        let mut s = HostState::default();
        assert_eq!(s.sort_mode, SortMode::Original);
        s.advance_sort_mode();
        assert_eq!(s.sort_mode, SortMode::AlphaAlias);
    }

    #[test]
    fn advance_sort_mode_wraps_from_last_to_first() {
        let mut s = HostState {
            sort_mode: SortMode::Status,
            ..Default::default()
        };
        s.advance_sort_mode();
        assert_eq!(s.sort_mode, SortMode::Original);
    }

    #[test]
    fn set_group_by_raw_keeps_active_filter() {
        let mut s = HostState {
            group_filter: Some("acme".to_string()),
            ..Default::default()
        };
        s.set_group_by_raw(GroupBy::Provider);
        assert!(matches!(s.group_by, GroupBy::Provider));
        assert_eq!(s.group_filter.as_deref(), Some("acme"));
    }

    #[test]
    fn clear_multi_select_empties_the_set() {
        let mut s = HostState::default();
        s.multi_select.insert(1);
        s.multi_select.insert(2);
        s.clear_multi_select();
        assert!(s.multi_select.is_empty());
    }

    #[test]
    fn pop_undo_returns_most_recent_deletion() {
        let mut s = HostState::default();
        s.undo_stack.push(DeletedHost {
            element: ConfigElement::GlobalLine(String::new()),
            position: 0,
        });
        s.undo_stack.push(DeletedHost {
            element: ConfigElement::GlobalLine(String::new()),
            position: 7,
        });
        let popped = s.pop_undo().expect("undo entry present");
        assert_eq!(popped.position, 7);
        assert_eq!(s.undo_stack.len(), 1);
    }

    #[test]
    fn pop_undo_returns_none_when_empty() {
        let mut s = HostState::default();
        assert!(s.pop_undo().is_none());
    }

    #[test]
    fn clear_undo_empties_the_stack() {
        let mut s = HostState::default();
        s.undo_stack.push(DeletedHost {
            element: ConfigElement::GlobalLine(String::new()),
            position: 0,
        });
        s.clear_undo();
        assert!(s.undo_stack.is_empty());
    }

    #[test]
    fn invalidate_render_cache_clears_cached_fields() {
        let mut s = HostState::default();
        s.render_cache.history_width = Some(12);
        s.render_cache.group_alias_map = Some(HashMap::new());
        s.invalidate_render_cache();
        assert!(s.render_cache.history_width.is_none());
        assert!(s.render_cache.group_alias_map.is_none());
    }
}
