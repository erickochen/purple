//! UI selection substate: list cursors, picker overlays, scroll offsets.

use ratatui::widgets::ListState;

use crate::ui::theme::ThemeDef;

/// A picker overlay: open flag plus its list cursor.
#[derive(Debug, Default)]
pub struct PickerState {
    pub open: bool,
    pub list: ListState,
}

#[allow(dead_code)]
impl PickerState {
    /// Open the picker with the cursor positioned at `index`.
    pub fn open_at(&mut self, index: usize) {
        self.open = true;
        self.list.select(Some(index));
    }

    /// Close the picker and reset the cursor.
    pub fn close(&mut self) {
        self.open = false;
        self.list.select(None);
    }
}

/// Theme picker carries extra catalogue + preview state beyond a simple list.
#[derive(Debug, Default)]
pub struct ThemePickerState {
    pub list: ListState,
    pub builtins: Vec<ThemeDef>,
    pub custom: Vec<ThemeDef>,
    pub saved_name: String,
    pub original: Option<ThemeDef>,
}

/// Region picker uses a cursor index rather than a ListState because region
/// rows are a synthetic flat array (provider × region pairs) rather than a
/// ratatui-managed list.
#[derive(Debug, Default)]
pub struct RegionPickerState {
    pub open: bool,
    pub cursor: usize,
}

/// What the AWS profile picker says about one profile beyond its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwsProfileNote {
    pub text: &'static str,
    /// False when the chain does not resolve, so the row reads as a warning
    /// rather than as an ordinary annotation.
    pub usable: bool,
}

/// One row of the AWS profile picker, built once when the overlay opens.
///
/// Cached rather than derived per frame: building a row reads both AWS files
/// and resolves the profile's chain, and the overlay redraws on every tick.
/// Every sibling picker scans on open for the same reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwsProfileRow {
    pub name: String,
    pub note: Option<AwsProfileNote>,
    /// The profile's own `region`, so picking it can fill the Regions field
    /// without reading `~/.aws` a second time.
    pub region: String,
}

#[derive(Debug, Default)]
pub struct UiSelection {
    pub(in crate::app) list_state: ListState,
    pub(in crate::app) key_picker: PickerState,
    pub(in crate::app) password_picker: PickerState,
    pub(in crate::app) proxyjump_picker: PickerState,
    pub(in crate::app) vault_role_picker: PickerState,
    pub(in crate::app) profile_picker: PickerState,
    /// The profile picker's rows, read when it opens. The list cursor above
    /// indexes into this.
    pub(in crate::app) aws_profile_rows: Vec<AwsProfileRow>,
    pub(in crate::app) tag_picker_state: ListState,
    pub(in crate::app) bulk_tag_editor_state: ListState,
    pub(in crate::app) theme_picker: ThemePickerState,
    pub(in crate::app) provider_list_state: ListState,
    pub(in crate::app) tunnel_list_state: ListState,
    pub(in crate::app) tunnels_overview_state: ListState,
    pub(in crate::app) containers_overview_state: ListState,
    /// Cursor for the host picker reached from the tunnels overview when
    /// adding a new tunnel. Indexes into the editable-hosts slice built at
    /// render time (hosts from included files are excluded).
    pub(in crate::app) tunnel_host_picker_state: ListState,
    /// Live fuzzy-search query for the tunnel host picker. Always-on input
    /// mode: every printable keystroke appends to the query and shrinks the
    /// candidate set. Empty string means "show all".
    pub(in crate::app) tunnel_host_picker_query: String,
    /// Cursor + live query for the containers-tab `a` host picker.
    /// Mirrors the tunnel host picker pair; kept separate so the two
    /// pickers can be open back-to-back without state bleed.
    pub(in crate::app) container_host_picker_state: ListState,
    pub(in crate::app) container_host_picker_query: String,
    pub(in crate::app) snippet_picker_state: ListState,
    pub(in crate::app) snippet_search: Option<String>,
    pub(in crate::app) region_picker: RegionPickerState,
    pub(in crate::app) help_scroll: u16,
    pub(in crate::app) detail_scroll: u16,
    /// Set by handler, consumed by AnimationState to trigger detail panel transition.
    pub(in crate::app) detail_toggle_pending: bool,
    /// Tracks when the welcome screen was opened to auto-dismiss it.
    pub(in crate::app) welcome_opened: Option<std::time::Instant>,
    /// Set once the first time Esc-on-empty-list hint is shown per process.
    pub(in crate::app) esc_quit_hint_shown: bool,
    /// Welcome-screen heuristic: number of known hosts at last render.
    pub(in crate::app) known_hosts_count: usize,
    /// Pending SSH dispatch queued by connect actions; consumed by the event loop.
    pub(in crate::app) pending_connect: Option<(String, Option<String>)>,
    pub(crate) pending_bitwarden_rename: Option<(String, String)>,
}

impl UiSelection {
    /// Construct with all picker/list state defaulted and the host list
    /// selection pre-positioned at `initial` (the first selectable host or
    /// pattern in the display list).
    pub fn new_with_initial_selection(initial: Option<usize>) -> Self {
        let mut s = Self::default();
        if let Some(pos) = initial {
            s.list_state.select(Some(pos));
        }
        s
    }

    /// Queue an SSH connect for the event loop to pick up via the next
    /// `pending_connect.take()`. `askpass` carries the resolved per-host
    /// password source so the event loop can prepare a SSH_ASKPASS env
    /// before spawning the child.
    pub fn queue_connect(&mut self, alias: String, askpass: Option<String>) {
        self.pending_connect = Some((alias, askpass));
    }

    /// Enter snippet picker search mode with an empty query.
    pub fn open_snippet_search(&mut self) {
        self.snippet_search = Some(String::new());
    }

    /// Exit snippet picker search mode. Idempotent.
    pub fn close_snippet_search(&mut self) {
        self.snippet_search = None;
    }

    pub fn list_state(&self) -> &ListState {
        &self.list_state
    }

    pub fn list_state_mut(&mut self) -> &mut ListState {
        &mut self.list_state
    }

    pub fn key_picker(&self) -> &PickerState {
        &self.key_picker
    }

    pub fn key_picker_mut(&mut self) -> &mut PickerState {
        &mut self.key_picker
    }

    pub fn profile_picker(&self) -> &PickerState {
        &self.profile_picker
    }

    pub fn profile_picker_mut(&mut self) -> &mut PickerState {
        &mut self.profile_picker
    }

    pub fn aws_profile_rows(&self) -> &[AwsProfileRow] {
        &self.aws_profile_rows
    }

    pub fn password_picker(&self) -> &PickerState {
        &self.password_picker
    }

    pub fn password_picker_mut(&mut self) -> &mut PickerState {
        &mut self.password_picker
    }

    pub fn proxyjump_picker(&self) -> &PickerState {
        &self.proxyjump_picker
    }

    pub fn proxyjump_picker_mut(&mut self) -> &mut PickerState {
        &mut self.proxyjump_picker
    }

    pub fn vault_role_picker(&self) -> &PickerState {
        &self.vault_role_picker
    }

    pub fn vault_role_picker_mut(&mut self) -> &mut PickerState {
        &mut self.vault_role_picker
    }

    pub fn tag_picker_state(&self) -> &ListState {
        &self.tag_picker_state
    }

    pub fn tag_picker_state_mut(&mut self) -> &mut ListState {
        &mut self.tag_picker_state
    }

    pub fn bulk_tag_editor_state(&self) -> &ListState {
        &self.bulk_tag_editor_state
    }

    pub fn bulk_tag_editor_state_mut(&mut self) -> &mut ListState {
        &mut self.bulk_tag_editor_state
    }

    pub fn theme_picker(&self) -> &ThemePickerState {
        &self.theme_picker
    }

    pub fn theme_picker_mut(&mut self) -> &mut ThemePickerState {
        &mut self.theme_picker
    }

    pub fn provider_list_state(&self) -> &ListState {
        &self.provider_list_state
    }

    pub fn provider_list_state_mut(&mut self) -> &mut ListState {
        &mut self.provider_list_state
    }

    pub fn tunnel_list_state(&self) -> &ListState {
        &self.tunnel_list_state
    }

    pub fn tunnel_list_state_mut(&mut self) -> &mut ListState {
        &mut self.tunnel_list_state
    }

    pub fn tunnels_overview_state(&self) -> &ListState {
        &self.tunnels_overview_state
    }

    pub fn tunnels_overview_state_mut(&mut self) -> &mut ListState {
        &mut self.tunnels_overview_state
    }

    pub fn containers_overview_state(&self) -> &ListState {
        &self.containers_overview_state
    }

    pub fn containers_overview_state_mut(&mut self) -> &mut ListState {
        &mut self.containers_overview_state
    }

    pub fn tunnel_host_picker_state(&self) -> &ListState {
        &self.tunnel_host_picker_state
    }

    pub fn tunnel_host_picker_state_mut(&mut self) -> &mut ListState {
        &mut self.tunnel_host_picker_state
    }

    pub fn tunnel_host_picker_query(&self) -> &String {
        &self.tunnel_host_picker_query
    }

    pub fn tunnel_host_picker_query_mut(&mut self) -> &mut String {
        &mut self.tunnel_host_picker_query
    }

    pub fn set_tunnel_host_picker_query(&mut self, query: String) {
        self.tunnel_host_picker_query = query;
    }

    pub fn container_host_picker_state(&self) -> &ListState {
        &self.container_host_picker_state
    }

    pub fn container_host_picker_state_mut(&mut self) -> &mut ListState {
        &mut self.container_host_picker_state
    }

    pub fn container_host_picker_query(&self) -> &String {
        &self.container_host_picker_query
    }

    pub fn container_host_picker_query_mut(&mut self) -> &mut String {
        &mut self.container_host_picker_query
    }

    pub fn set_container_host_picker_query(&mut self, query: String) {
        self.container_host_picker_query = query;
    }

    pub fn snippet_picker_state(&self) -> &ListState {
        &self.snippet_picker_state
    }

    pub fn snippet_picker_state_mut(&mut self) -> &mut ListState {
        &mut self.snippet_picker_state
    }

    pub fn snippet_search(&self) -> Option<&String> {
        self.snippet_search.as_ref()
    }

    pub fn snippet_search_mut(&mut self) -> Option<&mut String> {
        self.snippet_search.as_mut()
    }

    pub fn region_picker(&self) -> &RegionPickerState {
        &self.region_picker
    }

    pub fn region_picker_mut(&mut self) -> &mut RegionPickerState {
        &mut self.region_picker
    }

    pub fn help_scroll(&self) -> u16 {
        self.help_scroll
    }

    pub fn set_help_scroll(&mut self, scroll: u16) {
        self.help_scroll = scroll;
    }

    pub fn detail_scroll(&self) -> u16 {
        self.detail_scroll
    }

    pub fn set_detail_scroll(&mut self, scroll: u16) {
        self.detail_scroll = scroll;
    }

    pub fn detail_toggle_pending(&self) -> bool {
        self.detail_toggle_pending
    }

    pub fn set_detail_toggle_pending(&mut self, pending: bool) {
        self.detail_toggle_pending = pending;
    }

    pub fn welcome_opened(&self) -> Option<std::time::Instant> {
        self.welcome_opened
    }

    pub fn set_welcome_opened(&mut self, when: Option<std::time::Instant>) {
        self.welcome_opened = when;
    }

    pub fn esc_quit_hint_shown(&self) -> bool {
        self.esc_quit_hint_shown
    }

    pub fn set_esc_quit_hint_shown(&mut self, shown: bool) {
        self.esc_quit_hint_shown = shown;
    }

    pub fn known_hosts_count(&self) -> usize {
        self.known_hosts_count
    }

    pub fn set_known_hosts_count(&mut self, count: usize) {
        self.known_hosts_count = count;
    }

    pub fn pending_connect(&self) -> Option<&(String, Option<String>)> {
        self.pending_connect.as_ref()
    }

    /// Take the queued connect, leaving `None`. Consumed by the event loop.
    pub fn take_pending_connect(&mut self) -> Option<(String, Option<String>)> {
        self.pending_connect.take()
    }
}

impl ThemePickerState {
    /// Clear the catalogue lists and the saved-name input. Used by both
    /// picker-close paths. The `list` cursor and `original` are
    /// intentionally NOT touched here. The two callers handle `original`
    /// differently: the Esc/q path consumes it via `.take()` before
    /// calling reset (to restore the prior live theme); the Enter-save
    /// path leaves `original` intact through reset and clears it
    /// explicitly afterwards. Keeping `reset` orthogonal to `original`
    /// lets both flows share the same body.
    pub fn reset(&mut self) {
        self.builtins = Vec::new();
        self.custom = Vec::new();
        self.saved_name = String::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_connect_sets_pending_connect_to_some() {
        let mut s = UiSelection::default();
        s.queue_connect("web1".into(), Some("vault:foo".into()));
        assert_eq!(
            s.pending_connect,
            Some(("web1".to_string(), Some("vault:foo".to_string())))
        );
    }

    #[test]
    fn queue_connect_with_no_askpass_stores_none() {
        let mut s = UiSelection::default();
        s.queue_connect("web1".into(), None);
        assert_eq!(s.pending_connect, Some(("web1".to_string(), None)));
    }

    #[test]
    fn queue_connect_overwrites_existing_pending() {
        let mut s = UiSelection::default();
        s.queue_connect("first".into(), None);
        s.queue_connect("second".into(), Some("p".into()));
        assert_eq!(
            s.pending_connect,
            Some(("second".to_string(), Some("p".to_string())))
        );
    }

    #[test]
    fn open_snippet_search_sets_empty_query() {
        let mut s = UiSelection::default();
        s.open_snippet_search();
        assert_eq!(s.snippet_search.as_deref(), Some(""));
    }

    #[test]
    fn open_snippet_search_overwrites_existing_query_with_empty() {
        // The handler currently calls open only when search was inactive,
        // but the invariant should still hold: open is unconditional and
        // resets to an empty query. Pin the reset semantic so a future
        // caller cannot rely on a preserved query.
        let mut s = UiSelection {
            snippet_search: Some("old".to_string()),
            ..Default::default()
        };
        s.open_snippet_search();
        assert_eq!(s.snippet_search.as_deref(), Some(""));
    }

    #[test]
    fn close_snippet_search_clears_query() {
        let mut s = UiSelection {
            snippet_search: Some("query".to_string()),
            ..Default::default()
        };
        s.close_snippet_search();
        assert!(s.snippet_search.is_none());
    }

    #[test]
    fn close_snippet_search_is_idempotent() {
        let mut s = UiSelection::default();
        s.close_snippet_search();
        s.close_snippet_search();
        assert!(s.snippet_search.is_none());
    }

    #[test]
    fn theme_picker_reset_clears_lists_and_saved_name() {
        let mut t = ThemePickerState {
            builtins: vec![ThemeDef::purple_purple()],
            custom: vec![ThemeDef::purple_purple(), ThemeDef::purple_purple()],
            saved_name: "Solarized".to_string(),
            ..Default::default()
        };
        t.reset();
        assert!(t.builtins.is_empty());
        assert!(t.custom.is_empty());
        assert!(t.saved_name.is_empty());
    }

    #[test]
    fn theme_picker_reset_preserves_original_and_list_cursor() {
        let mut t = ThemePickerState {
            builtins: vec![ThemeDef::purple_purple()],
            original: Some(ThemeDef::purple_purple()),
            ..Default::default()
        };
        t.list.select(Some(2));
        t.reset();
        assert!(t.original.is_some(), "original must survive reset()");
        assert_eq!(t.list.selected(), Some(2));
    }
}
