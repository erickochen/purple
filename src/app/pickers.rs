//! Overlay picker lifecycle. Implements `impl App` continuation with
//! open/close domain actions for every `.open`-flag-based picker overlay
//! (password, key, proxyjump, vault_role, region). Screen-based pickers
//! (TagPicker, ThemePicker, SnippetPicker, etc.) live in `selection.rs`.

use ratatui::widgets::ListState;

use crate::app::App;
use crate::app::ui_state::{AwsProfileNote, AwsProfileRow};

impl App {
    /// Close the password picker overlay.
    pub fn close_password_picker(&mut self) {
        log::debug!("[purple] close_password_picker");
        self.ui.password_picker.open = false;
    }

    /// Close the key picker overlay.
    pub fn close_key_picker(&mut self) {
        log::debug!("[purple] close_key_picker");
        self.ui.key_picker.open = false;
    }

    /// Close the ProxyJump picker overlay.
    pub fn close_proxyjump_picker(&mut self) {
        log::debug!("[purple] close_proxyjump_picker");
        self.ui.proxyjump_picker.open = false;
    }

    /// Close the Vault SSH role picker overlay.
    pub fn close_vault_role_picker(&mut self) {
        log::debug!("[purple] close_vault_role_picker");
        self.ui.vault_role_picker.open = false;
    }

    /// Close the provider region picker overlay.
    pub fn close_region_picker(&mut self) {
        log::debug!("[purple] close_region_picker");
        self.ui.region_picker.open = false;
    }

    /// Open the password picker overlay focused on the first source.
    pub fn open_password_picker(&mut self) {
        log::debug!("[purple] open_password_picker");
        self.ui.password_picker.open = true;
        self.ui.password_picker.list = ListState::default();
        self.ui.password_picker.list.select(Some(0));
    }

    /// The AWS profile picker's rows: every profile found in `~/.aws/config`
    /// and `~/.aws/credentials`, sorted, each with what purple will do with
    /// it. A profile with nothing worth saying carries no note.
    ///
    /// Resolving the chain per profile is what makes the refusals visible. The
    /// shape that decides whether purple can use a profile lives in the file
    /// rather than in the name, so otherwise it only surfaces on the next
    /// sync.
    fn aws_profile_rows(&self) -> Vec<AwsProfileRow> {
        let env = self.env();
        let profiles = crate::providers::aws_profile::AwsProfiles::load(
            env.aws_config_file().as_deref(),
            env.aws_credentials_file().as_deref(),
        );
        profiles
            .names()
            .iter()
            .map(|name| {
                let note = match profiles.resolve_chain(name) {
                    Ok(chain) if !chain.roles.is_empty() => Some(AwsProfileNote {
                        text: crate::i18n::text(crate::messages::PROFILE_ASSUMES_ROLE),
                        usable: true,
                    }),
                    Ok(_) => None,
                    Err(e) => Some(AwsProfileNote {
                        text: crate::providers::aws::chain_error_note(&e),
                        usable: false,
                    }),
                };
                AwsProfileRow {
                    name: (*name).to_string(),
                    note,
                    region: profiles
                        .get(name)
                        .map(|p| p.region.trim().to_string())
                        .unwrap_or_default(),
                }
            })
            .collect()
    }

    /// Open the AWS profile picker overlay, reading `~/.aws` once. Returns
    /// false without opening when there is nothing to pick, so the caller can
    /// say why instead of showing an empty list.
    pub fn open_profile_picker(&mut self) -> bool {
        let rows = self.aws_profile_rows();
        if rows.is_empty() {
            return false;
        }
        log::debug!("[purple] open_profile_picker: {} profiles", rows.len());
        self.ui.aws_profile_rows = rows;
        self.ui.profile_picker.open = true;
        self.ui.profile_picker.list = ListState::default();
        self.ui.profile_picker.list.select(Some(0));
        true
    }

    /// Open the key picker overlay. Rescans `~/.ssh` first so the list
    /// reflects keys added since the form was opened, then selects the
    /// first key when at least one was discovered.
    pub fn open_key_picker(&mut self) {
        log::debug!("[purple] open_key_picker");
        self.scan_keys();
        self.ui.key_picker.open = true;
        self.ui.key_picker.list = ListState::default();
        if !self.keys.list.is_empty() {
            self.ui.key_picker.list.select(Some(0));
        }
    }

    /// Open the ProxyJump picker overlay. The opening cursor lands on the
    /// first host row rather than the first list entry, so separator/header
    /// rows above the host list do not steal initial focus.
    pub fn open_proxyjump_picker(&mut self) {
        log::debug!("[purple] open_proxyjump_picker");
        self.ui.proxyjump_picker.open = true;
        self.ui.proxyjump_picker.list = ListState::default();
        if let Some(idx) = self.proxyjump_first_host_index() {
            self.ui.proxyjump_picker.list.select(Some(idx));
        }
    }

    /// Open the Vault SSH role picker. The caller is responsible for
    /// guarding against an empty candidate list; this method assumes at
    /// least one role and selects the first.
    pub fn open_vault_role_picker(&mut self) {
        log::debug!("[purple] open_vault_role_picker");
        self.ui.vault_role_picker.open = true;
        self.ui.vault_role_picker.list = ListState::default();
        self.ui.vault_role_picker.list.select(Some(0));
    }

    /// Open the provider region picker overlay with the cursor on the
    /// first row. Region picker uses a `cursor: usize` rather than a
    /// ratatui `ListState` because its rows are a synthetic flat array.
    pub fn open_region_picker(&mut self) {
        log::debug!("[purple] open_region_picker");
        self.ui.region_picker.open = true;
        self.ui.region_picker.cursor = 0;
    }
}
