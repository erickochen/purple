//! Bitwarden enrollment and managed rename messages.
pub const CLI_START: &str = "Could not start Bitwarden CLI; install bw first";
pub const INVALID_RESPONSE: &str = "Invalid Bitwarden response; check the vault before retrying";
pub const MISSING_ID: &str = "Bitwarden did not return an item ID";
pub const INVALID_ID: &str = "Invalid Bitwarden ID";
pub const DEMO_DISABLED: &str = "Bitwarden enrollment is unavailable in demo mode";
pub const HOST_MISSING: &str = "Host no longer exists";
pub const SOURCE_CHANGED: &str = "Password source changed; reopen the host";
pub const IDENTITY_CHANGED: &str = "Host identity changed; check the existing Bitwarden item";
pub const INVALID_ITEMS: &str = "Invalid Bitwarden item list";
pub const DUPLICATE_ITEMS: &str =
    "Multiple matching Bitwarden items found; resolve duplicates first";
pub const INCOMPLETE_ITEM: &str =
    "Existing item is missing a username or password; check Bitwarden";
pub const AMBIGUOUS_CREATE: &str = "Previous creation is unconfirmed; stopped to avoid duplicates. Sync and inspect the ssh/Purple folder before retrying.";
pub const AMBIGUOUS_FOLDER: &str =
    "Previous folder creation is unconfirmed; inspect Bitwarden before retrying";
pub const MISSING_HOST_FIELDS: &str =
    "Set the hostname and username before saving a Bitwarden password";
pub const NAME_COLLISION: &str =
    "A Bitwarden item with this name exists; bind its UUID instead of overwriting it";
pub const EMPTY_PASSWORD: &str = "No password entered; enrollment cancelled";
pub const INVALID_FOLDERS: &str = "Invalid Bitwarden folder list";
pub const DUPLICATE_FOLDERS: &str = "Multiple ssh/Purple folders found; resolve duplicates first";
pub const READBACK_FAILED: &str = "Bitwarden item readback verification failed";
pub const CONFIG_CHANGED: &str =
    "SSH config changed externally; vault item preserved. Reconnect to finish binding";
pub const BIND_FAILED: &str = "Could not update the host password source; vault item preserved";
pub const BIND_VERIFY_FAILED: &str = "Host binding verification failed; vault item preserved";
pub const MISSING_PATHS: &str = "Purple configuration directory is unavailable";
pub const INVALID_SYNC_PATH: &str = "Invalid Bitwarden sync directory";
pub const RENAME_CONFLICT: &str = "Another host rename is pending; check the Bitwarden binding";
pub const ITEM_CHANGED: &str = "Bitwarden item changed or was deleted";
pub const OWNER_MISMATCH: &str = "Bitwarden ownership marker does not match; name left unchanged";
pub const NAME_EDITED: &str =
    "Bitwarden name was edited manually; name left unchanged. Review the pending rename";
pub const RENAME_VERIFY_FAILED: &str = "Bitwarden rename readback verification failed";
pub const BINDING_CHANGED: &str = "Local host binding changed; Bitwarden name left unchanged";
pub const SYNC_RENAME: &str = "Synchronizing the Bitwarden item name...";
pub const NAME_SYNCED: &str = "Bitwarden name synchronized";
pub const EXTERNAL_ITEM: &str = "Item is not managed by Purple; original Bitwarden name preserved";
pub const CHECKING_ITEMS: &str = "Synchronizing Bitwarden and checking existing items...";
pub const PASSWORD_PROMPT: &str = "Server login password (Esc to cancel): ";
pub const CANCELLED: &str = "Bitwarden enrollment cancelled";
pub const UNBOUND: &str = "Bitwarden is not bound; connect with Enter to save the password first";
pub const PASSWORD_BOUND: &str = "Bitwarden password saved and bound; connecting...";
pub fn rename_pending(e: &impl std::fmt::Display) -> String {
    format!("Bitwarden name not synchronized; retrying on the next connection: {e}")
}
pub fn host_renamed_pending(e: &impl std::fmt::Display) -> String {
    format!("Host renamed; {}", rename_pending(e))
}
pub fn saving(alias: &str) -> String {
    format!("Saving the login password for {alias} in Bitwarden folder ssh/Purple.")
}
pub fn keychain_preserved(message: &str) -> String {
    format!("{message}. Original Keychain entry preserved.")
}
pub fn rename_queue_failed(e: &impl std::fmt::Display) -> String {
    format!("Host renamed, but could not queue the Bitwarden rename: {e}")
}
pub fn rename_queued(alias: &str) -> String {
    format!("Host renamed; synchronizing Bitwarden: {alias}")
}

pub fn operation_failed(operation: &str, code: Option<i32>) -> String {
    format!(
        "Bitwarden {operation} failed (exit {code:?}); host binding was not written. Check CLI login, vault unlock and network access."
    )
}
