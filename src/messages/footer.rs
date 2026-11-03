//! Canonical footer keycap labels.
//!
//! Single source of truth for the text shown next to every keycap in TUI
//! footers. Inline strings in `src/ui/*.rs` are forbidden. every footer
//! must reference these constants so the same key in different screens
//! shows the same label.
//!
//! Categories follow the design-system-reference.md "Footer keycap" rule:
//! - `back`  . leaves a navigated overlay and returns to its parent
//! - `cancel`. discards form/picker input or selection state
//! - `close` . closes a self-contained info overlay (no state)
//!
//! All values include the surrounding spaces the `Footer` builder expects
//! (` select `, not `select`).

// --- Esc category ---

/// Back to parent screen (used for navigated lists: providers, tunnels, keys).
pub const ESC_BACK: &str = " back";

/// Cancel pending input (forms, pickers with selection state).
pub const ESC_CANCEL: &str = " cancel";

/// Close a self-contained info overlay (help, what's-new, jump, key_detail).
pub const ESC_CLOSE: &str = " close";

/// Clear the active filter inside a search/tag-input mode (host_list).
pub const ESC_CLEAR: &str = " clear";

// --- Enter category ---

/// Open the selected item's detail / edit form.
pub const ENTER_SELECT: &str = " select ";

/// Save and submit a form.
pub const ENTER_SAVE: &str = " save ";

/// Connect over SSH (host_list).
pub const ENTER_CONNECT: &str = " connect ";

/// Apply a multi-select choice (region picker, bulk tag editor).
pub const ENTER_APPLY: &str = " apply ";

/// Edit the selected row (provider list).
pub const ENTER_EDIT: &str = " edit ";

/// Toggle: tunnel start.
pub const ENTER_START: &str = " start ";

/// Toggle: tunnel stop.
pub const ENTER_STOP: &str = " stop ";

/// Run a snippet.
pub const ENTER_RUN: &str = " run ";

/// File-browser copy.
pub const ENTER_COPY: &str = " copy ";

/// Drop into an interactive shell on the selected resource (containers
/// overview Enter. `ssh -t … docker exec -it … sh`).
pub const ENTER_SHELL: &str = " shell ";

/// Provider list: expand a multi-config row.
pub const ENTER_EXPAND: &str = " expand ";

/// Provider list: collapse a multi-config row.
pub const ENTER_COLLAPSE: &str = " collapse ";

/// Bulk tag editor: add row.
pub const ENTER_ADD: &str = " add ";

/// Password prompt: submit the password and resume the push or listing
/// that asked for it. Distinct from `ENTER_SAVE`, which would misdescribe
/// a submit with "remember" off.
pub const ENTER_CONTINUE: &str = " continue ";

// --- Action keys (single-letter shortcuts) ---

pub const ACTION_ADD: &str = " add ";
pub const ACTION_DEL: &str = " del ";
pub const ACTION_EDIT: &str = " edit ";
pub const ACTION_SYNC: &str = " sync ";
pub const ACTION_SORT: &str = " sort ";
pub const ACTION_SEARCH: &str = " search ";
pub const ACTION_TAG: &str = " tag ";
pub const ACTION_DETAIL: &str = " detail ";
pub const ACTION_COMPACT: &str = " compact ";
pub const ACTION_JUMP: &str = " jump ";
pub const ACTION_HELP: &str = " help ";
pub const ACTION_BULK_TAG: &str = " bulk tag ";
pub const ACTION_RUN: &str = " run ";
pub const ACTION_TUNNELS: &str = " tunnels ";
pub const ACTION_SNIPPET: &str = " snippet ";
pub const ACTION_TERMINAL: &str = " terminal ";
pub const ACTION_NEW: &str = " new ";
pub const ACTION_HIDDEN: &str = " hidden ";
pub const ACTION_REFRESH: &str = " refresh ";
pub const ACTION_RESTART: &str = " restart ";
pub const ACTION_START: &str = " start ";
pub const ACTION_STOP: &str = " stop ";
pub const ACTION_LOGS: &str = " logs ";

/// Keys tab footer labels. Centralised here so the design-system gate
/// (`./scripts/check-design-system.sh`) does not flag inline literals
/// at the keys_overview call sites.
pub const ACTION_PUSH: &str = " push ";

/// Bulk Vault SSH sign trigger, shared between the host list and the
/// Keys tab so the wording stays in sync with the actual handler.
pub const ACTION_VAULT_SIGN: &str = " sign cert ";

/// Key-push picker primary action. Enter advances to the confirm dialog.
pub const ENTER_CONFIRM: &str = " confirm ";

/// Logs overlay footer keycaps. Kept here so the design-system gate
/// (`./scripts/check-design-system.sh`) does not flag inline literals
/// at the call sites.
pub const ACTION_BACK: &str = " back ";
pub const ACTION_TOP: &str = " top ";
pub const ACTION_BOTTOM: &str = " bottom ";
pub const ACTION_SCROLL: &str = " scroll ";
pub const ACTION_PAGE: &str = " page ";
pub const ACTION_ALL: &str = " all ";
pub const ACTION_CLOSE: &str = " close ";
pub const ACTION_MATCH: &str = " match ";
pub const ACTION_MOVE: &str = " move ";

// --- Tab / arrow / Space ---

/// Next field/pane. Used in every form and the file_browser.
pub const TAB_NEXT: &str = " next ";

/// Cycle through items in a list (arrow keys).
pub const ARROWS_SELECT: &str = " select ";

/// Toggle a focused boolean field.
pub const SPACE_TOGGLE: &str = " toggle ";

/// Open the focused picker (form picker fields).
pub const SPACE_PICK: &str = " pick ";

/// Cycle through tag states (bulk tag editor: include / exclude / no-op).
pub const SPACE_CYCLE: &str = " cycle ";

// --- Modifier shortcuts ---

pub const CTRL_C_CANCEL: &str = " cancel ";

// --- Snippet / file-browser specific ---

pub const SNIPPET_OUTPUT_COPY: &str = " copy ";
pub const FB_SELECT: &str = " select ";

// --- Help / scroll keys ---

pub const KEYS_SCROLL: &str = "j/k";
pub const LABEL_SCROLL: &str = " scroll ";
pub const KEYS_TOP_BOTTOM: &str = "g/G";
pub const LABEL_TOP_BOTTOM: &str = " top/bottom";
pub const KEYS_NEXT_PREV_HOST: &str = "n/N";
pub const LABEL_NEXT_PREV_HOST: &str = " next/prev host ";
