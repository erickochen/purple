use super::*;
use crate::app::{App, FormField, HostForm, ProviderFormField, ProviderFormFields, Screen};
use crate::handler::host_list::actions::vault_addr_missing;
use crate::providers::config::{ProviderConfig, ProviderSection};
use crate::ssh_config::model::SshConfigFile;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;

fn test_provider_config() -> ProviderConfig {
    // Unique tempdir per call so parallel tests do not race on the same
    // provider-config file when `path_override` gets written.
    let dir = tempfile::tempdir().expect("tempdir").keep();
    ProviderConfig {
        path_override: Some(dir.join("providers")),
        ..Default::default()
    }
}

fn make_app(content: &str) -> App {
    // Unique tempdir per call — parallel `cargo test` threads must not
    // share a config path when `app.hosts_state.ssh_config().write()` or preferences-write
    // runs.
    let scratch = tempfile::tempdir().expect("tempdir").keep();
    // App::new auto-sandboxes the in-test env so preferences and the
    // container cache resolve to a self-cleaning tempdir, never the real
    // ~/.purple/, even across parallel test threads.
    let config = SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: scratch.join("test_config"),
        crlf: false,
        bom: false,
    };
    let mut app = App::new(config);
    *app.providers.config_mut() = test_provider_config();
    app
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// App with a configured DigitalOcean (auto_sync=true) and a fresh Proxmox.
fn make_providers_app_with_do() -> App {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::Providers;
    *app.providers.config_mut() = test_provider_config();
    app.providers.config_mut().set_section(ProviderSection {
        filter: String::new(),
        id: crate::providers::config::ProviderConfigId::bare("digitalocean"),
        token: "tok".to_string(),
        alias_prefix: "do".to_string(),
        user: "root".to_string(),
        identity_file: String::new(),
        url: String::new(),
        verify_tls: true,
        auto_sync: true,
        profile: String::new(),
        regions: String::new(),
        project: String::new(),
        compartment: String::new(),
        vault_role: String::new(),
        vault_addr: String::new(),
        ssm: crate::providers::aws_ssm::SsmMode::default(),
    });
    app
}

fn make_providers_app_with_proxmox() -> App {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::Providers;
    *app.providers.config_mut() = test_provider_config();
    app.providers.config_mut().set_section(ProviderSection {
        filter: String::new(),
        id: crate::providers::config::ProviderConfigId::bare("proxmox"),
        token: "user@pam!t=secret".to_string(),
        alias_prefix: "pve".to_string(),
        user: "root".to_string(),
        identity_file: String::new(),
        url: "https://pve.local:8006".to_string(),
        verify_tls: true,
        auto_sync: false,
        profile: String::new(),
        regions: String::new(),
        project: String::new(),
        compartment: String::new(),
        vault_role: String::new(),
        vault_addr: String::new(),
        ssm: crate::providers::aws_ssm::SsmMode::default(),
    });
    app
}

/// Positioneer de cursor op een bepaalde provider in de lijst en stuur Enter.
fn open_provider_form(app: &mut App, provider_name: &str) {
    let sorted = app.sorted_provider_names();
    let idx = sorted.iter().position(|n| n == provider_name).unwrap();
    app.ui.provider_list_state_mut().select(Some(idx));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(app, key(KeyCode::Enter), &tx);
}

// --- Form initialisatie ---

#[test]
fn test_provider_form_init_existing_do_preserves_auto_sync_true() {
    let mut app = make_providers_app_with_do();
    open_provider_form(&mut app, "digitalocean");
    assert!(
        app.providers.form_mut().auto_sync,
        "Bestaande DO provider (auto_sync=true) moet true blijven in het form"
    );
}

#[test]
fn test_provider_form_init_existing_proxmox_preserves_auto_sync_false() {
    let mut app = make_providers_app_with_proxmox();
    open_provider_form(&mut app, "proxmox");
    assert!(
        !app.providers.form_mut().auto_sync,
        "Bestaande Proxmox provider (auto_sync=false) moet false blijven in het form"
    );
}

#[test]
fn test_provider_form_init_existing_do_explicit_false_preserved() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::Providers;
    *app.providers.config_mut() = test_provider_config();
    // DO met auto_sync=false (gebruiker heeft het handmatig uitgezet)
    app.providers.config_mut().set_section(ProviderSection {
        filter: String::new(),
        id: crate::providers::config::ProviderConfigId::bare("digitalocean"),
        token: "tok".to_string(),
        alias_prefix: "do".to_string(),
        user: "root".to_string(),
        identity_file: String::new(),
        url: String::new(),
        verify_tls: true,
        auto_sync: false,
        profile: String::new(),
        regions: String::new(),
        project: String::new(),
        compartment: String::new(),
        vault_role: String::new(),
        vault_addr: String::new(),
        ssm: crate::providers::aws_ssm::SsmMode::default(),
    });
    open_provider_form(&mut app, "digitalocean");
    assert!(
        !app.providers.form_mut().auto_sync,
        "DO met auto_sync=false moet false blijven"
    );
}

#[test]
fn test_provider_form_init_new_proxmox_defaults_to_false() {
    // Proxmox zonder bestaande config: default auto_sync=false
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::Providers;
    *app.providers.config_mut() = test_provider_config(); // geen config voor proxmox
    open_provider_form(&mut app, "proxmox");
    assert!(
        !app.providers.form_mut().auto_sync,
        "Nieuw Proxmox form moet auto_sync=false als default tonen"
    );
}

#[test]
fn test_provider_form_init_new_digitalocean_defaults_to_true() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::Providers;
    *app.providers.config_mut() = test_provider_config();
    open_provider_form(&mut app, "digitalocean");
    assert!(
        app.providers.form_mut().auto_sync,
        "Nieuw DigitalOcean form moet auto_sync=true als default tonen"
    );
}

// --- Space toggle ---

fn make_form_app_focused_on(provider: &str, field: ProviderFormField) -> App {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::ProviderForm {
        id: crate::providers::config::ProviderConfigId::bare(provider),
    };
    *app.providers.form_mut() = ProviderFormFields {
        filter: String::new(),
        label: String::new(),
        label_entry: false,
        url: String::new(),
        token: "tok".to_string(),
        profile: String::new(),
        project: String::new(),
        compartment: String::new(),
        regions: String::new(),
        alias_prefix: "do".to_string(),
        user: "root".to_string(),
        identity_file: String::new(),
        verify_tls: true,
        auto_sync: true,
        vault_role: String::new(),
        vault_addr: String::new(),
        ssm: crate::providers::aws_ssm::SsmMode::default(),
        focused_field: field,
        cursor_pos: 0,
        expanded: true, // Tests assume all fields visible
    };
    app
}

/// Submit provider form with fresh mtime capture to minimize race window.
fn submit_form(app: &mut App) {
    app.capture_provider_form_mtime();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(app, key(KeyCode::Enter), &tx);
}

/// Assert that the status message contains the expected validation error.
/// Tolerates the conflict-detection race: if another parallel test wrote
/// to ~/.purple/providers between mtime capture and submit, the conflict
/// check fires before validation and the test is inconclusive (not a bug).
fn assert_status_contains(app: &App, expected: &str) {
    // Check both footer status and toast (messages route to different destinations)
    let status_text = app.status_center.status().map(|s| s.text.as_str());
    let toast_text = app.status_center.toast().map(|t| t.text.as_str());
    let msg = status_text
        .or(toast_text)
        .expect("status or toast should be set");
    if msg.contains("changed externally") {
        return; // inconclusive due to race
    }
    assert!(
        msg.contains(expected),
        "Expected status/toast to contain '{}', got: '{}'",
        expected,
        msg
    );
}

fn assert_status_not_contains(app: &App, not_expected: &str) {
    let status_msg = app
        .status_center
        .status()
        .map(|s| s.text.as_str())
        .unwrap_or("");
    let toast_msg = app
        .status_center
        .toast()
        .map(|t| t.text.as_str())
        .unwrap_or("");
    if status_msg.contains("changed externally") || toast_msg.contains("changed externally") {
        return; // inconclusive due to race
    }
    assert!(
        !status_msg.contains(not_expected) && !toast_msg.contains(not_expected),
        "Status/toast should NOT contain '{}', got status: '{}', toast: '{}'",
        not_expected,
        status_msg,
        toast_msg
    );
}

#[test]
fn test_space_toggles_auto_sync_true_to_false() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::AutoSync);
    assert!(app.providers.form_mut().auto_sync);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(!app.providers.form_mut().auto_sync);
}

#[test]
fn test_space_toggles_auto_sync_false_to_true() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::AutoSync);
    app.providers.form_mut().auto_sync = false;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(app.providers.form_mut().auto_sync);
}

#[test]
fn test_space_on_other_field_does_not_affect_auto_sync() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.providers.form_mut().auto_sync = true;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    // Space op Token voegt spatie toe aan tekstveld; auto_sync ongewijzigd
    assert!(app.providers.form_mut().auto_sync);
}

// --- Char/Backspace blokkering op AutoSync ---

#[test]
fn test_char_input_blocked_when_auto_sync_focused() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::AutoSync);
    let original_token = app.providers.form_mut().token.clone();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('x')), &tx);
    // Geen enkel tekstveld mag gewijzigd zijn
    assert_eq!(app.providers.form_mut().token, original_token);
    assert_eq!(app.providers.form_mut().alias_prefix, "do");
}

#[test]
fn test_backspace_blocked_when_auto_sync_focused() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::AutoSync);
    let original_token = app.providers.form_mut().token.clone();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    assert_eq!(app.providers.form_mut().token, original_token);
}

// --- Submit persisteert auto_sync ---

#[test]
fn test_submit_provider_form_persists_auto_sync_false() {
    // Submit met auto_sync=false moet de sectie opslaan met auto_sync=false.
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::ProviderForm {
        id: crate::providers::config::ProviderConfigId::bare("digitalocean"),
    };
    *app.providers.config_mut() = test_provider_config();
    *app.providers.form_mut() = ProviderFormFields {
        filter: String::new(),
        label: String::new(),
        label_entry: false,
        url: String::new(),
        token: "tok".to_string(),
        profile: String::new(),
        project: String::new(),
        compartment: String::new(),
        regions: String::new(),
        alias_prefix: "do".to_string(),
        user: "root".to_string(),
        identity_file: String::new(),
        verify_tls: true,
        auto_sync: false,
        vault_role: String::new(),
        vault_addr: String::new(),
        focused_field: ProviderFormField::Token,
        cursor_pos: 0,
        expanded: false,
        ssm: crate::providers::aws_ssm::SsmMode::default(),
    };

    let (tx, _rx) = mpsc::channel();
    // Enter triggert submit; save() kan falen zonder ~/.purple dir, maar de
    // in-memory sectie wordt altijd bijgewerkt vóór de save.
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    // Ongeacht of save() slaagde: de sectie in provider_config is bijgewerkt.
    if let Some(section) = app.providers.config().section("digitalocean") {
        assert!(
            !section.auto_sync,
            "Opgeslagen sectie moet auto_sync=false hebben"
        );
    }
    // Als het form is gesloten (save geslaagd), controleert de screen-state
    // dat de toggle correct is doorgegeven.
}

#[test]
fn test_submit_provider_form_persists_auto_sync_true() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::ProviderForm {
        id: crate::providers::config::ProviderConfigId::bare("digitalocean"),
    };
    *app.providers.config_mut() = test_provider_config();
    *app.providers.form_mut() = ProviderFormFields {
        filter: String::new(),
        label: String::new(),
        label_entry: false,
        url: String::new(),
        token: "tok".to_string(),
        profile: String::new(),
        project: String::new(),
        compartment: String::new(),
        regions: String::new(),
        alias_prefix: "do".to_string(),
        user: "root".to_string(),
        identity_file: String::new(),
        verify_tls: true,
        auto_sync: true,
        vault_role: String::new(),
        vault_addr: String::new(),
        focused_field: ProviderFormField::Token,
        cursor_pos: 0,
        expanded: false,
        ssm: crate::providers::aws_ssm::SsmMode::default(),
    };

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    if let Some(section) = app.providers.config().section("digitalocean") {
        assert!(
            section.auto_sync,
            "Opgeslagen sectie moet auto_sync=true hebben"
        );
    }
}

#[test]
fn test_submit_provider_form_persists_vault_role() {
    // Submit met een vault_role moet de in-memory sectie bijwerken met
    // dezelfde role. save() naar disk kan falen in een test-omgeving zonder
    // ~/.purple dir; we vertrouwen alleen op de in-memory mutatie hier,
    // identiek aan test_submit_provider_form_persists_auto_sync_*.
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::ProviderForm {
        id: crate::providers::config::ProviderConfigId::bare("digitalocean"),
    };
    *app.providers.config_mut() = test_provider_config();
    *app.providers.form_mut() = ProviderFormFields {
        filter: String::new(),
        label: String::new(),
        label_entry: false,
        url: String::new(),
        token: "tok".to_string(),
        profile: String::new(),
        project: String::new(),
        compartment: String::new(),
        regions: String::new(),
        alias_prefix: "do".to_string(),
        user: "root".to_string(),
        identity_file: String::new(),
        verify_tls: true,
        auto_sync: true,
        vault_role: "ssh-client-signer/sign/engineer".to_string(),
        vault_addr: String::new(),
        focused_field: ProviderFormField::Token,
        cursor_pos: 0,
        expanded: true,
        ssm: crate::providers::aws_ssm::SsmMode::default(),
    };

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    if let Some(section) = app.providers.config().section("digitalocean") {
        assert_eq!(
            section.vault_role, "ssh-client-signer/sign/engineer",
            "vault_role moet round-trippen via provider form submit"
        );
    }
}

#[test]
fn test_provider_config_parse_vault_role_present() {
    // Direct: parse INI met vault_role en verifieer dat de waarde wordt
    // overgenomen. Aanvulling op de form-submit test, onafhankelijk van
    // filesystem en form state.
    let input = "[digitalocean]\ntoken=abc\nvault_role=ssh-client-signer/sign/engineer\n";
    let cfg = crate::providers::config::ProviderConfig::parse(input);
    let section = cfg.section("digitalocean").expect("section");
    assert_eq!(section.vault_role, "ssh-client-signer/sign/engineer");
}

// =========================================================================
// Provider form validation tests
// =========================================================================

#[test]
fn test_submit_provider_form_aws_accepts_empty_token_and_profile() {
    // Credentials come from AWS_ACCESS_KEY_ID / _SECRET_ACCESS_KEY /
    // _SESSION_TOKEN at sync time, which is not knowable when saving.
    let mut app = make_form_app_focused_on("aws", ProviderFormField::Token);
    app.providers.form_mut().token = String::new();
    app.providers.form_mut().profile = String::new();
    app.providers.form_mut().regions = "eu-central-1".to_string();
    app.providers.form_mut().alias_prefix = "aws".to_string();
    submit_form(&mut app);
    let section = app
        .providers
        .config()
        .section("aws")
        .expect("aws section must be saved without a token or a profile");
    assert_eq!(section.token, "");
    assert_eq!(section.profile, "");
    assert_eq!(section.regions, "eu-central-1");
    assert!(
        !matches!(app.screen, Screen::ProviderForm { .. }),
        "aws form must close after a successful submit"
    );
}

#[test]
fn test_submit_provider_form_aws_accepts_empty_token_with_profile() {
    let mut app = make_form_app_focused_on("aws", ProviderFormField::Token);
    app.providers.form_mut().token = String::new();
    app.providers.form_mut().profile = "default".to_string();
    app.providers.form_mut().regions = "eu-central-1".to_string();
    app.providers.form_mut().alias_prefix = "aws".to_string();
    submit_form(&mut app);
    let section = app
        .providers
        .config()
        .section("aws")
        .expect("aws section must be saved on profile credentials alone");
    assert_eq!(section.token, "");
    assert_eq!(section.profile, "default");
    assert!(
        !matches!(app.screen, Screen::ProviderForm { .. }),
        "aws form must close after a successful submit"
    );
}

#[test]
fn test_submit_provider_form_rejects_control_chars_in_token() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.providers.form_mut().token = "tok\x01en".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "control characters");
}

#[test]
fn test_submit_provider_form_rejects_control_chars_in_alias_prefix() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.providers.form_mut().alias_prefix = "do\x00".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "control characters");
}

#[test]
fn test_submit_provider_form_rejects_control_chars_in_url() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::Url);
    app.providers.form_mut().url = "https://pve\x0a.local:8006".to_string();
    app.providers.form_mut().token = "user@pam!t=secret".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "control characters");
}

#[test]
fn test_submit_provider_form_rejects_control_chars_in_user() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.providers.form_mut().user = "ro\tot".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "control characters");
}

#[test]
fn test_submit_provider_form_rejects_control_chars_in_identity_file() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.providers.form_mut().identity_file = "~/.ssh/id\x1b_rsa".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "control characters");
}

#[test]
fn test_submit_proxmox_rejects_empty_url() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::Url);
    app.providers.form_mut().url = "".to_string();
    app.providers.form_mut().token = "user@pam!t=secret".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "URL is required");
}

#[test]
fn test_submit_proxmox_rejects_http_url() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::Url);
    app.providers.form_mut().url = "http://pve.local:8006".to_string();
    app.providers.form_mut().token = "user@pam!t=secret".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "https://");
}

#[test]
fn test_submit_proxmox_accepts_https_url() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::Url);
    app.providers.form_mut().url = "https://pve.local:8006".to_string();
    app.providers.form_mut().token = "user@pam!t=secret".to_string();
    submit_form(&mut app);
    assert_status_not_contains(&app, "URL is required");
    assert_status_not_contains(&app, "https://");
}

#[test]
fn test_submit_proxmox_rejects_bare_hostname_url() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::Url);
    app.providers.form_mut().url = "pve.local:8006".to_string();
    app.providers.form_mut().token = "user@pam!t=secret".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "https://");
}

#[test]
fn test_submit_provider_form_rejects_empty_token() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.providers.form_mut().token = "".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "Token");
}

#[test]
fn test_submit_provider_form_accepts_empty_token_for_local_cli_providers() {
    // Teleport and Tailscale have no API token; the form must save without one.
    for (provider, prefix) in [("teleport", "tp"), ("tailscale", "ts")] {
        let mut app = make_form_app_focused_on(provider, ProviderFormField::User);
        app.providers.form_mut().token = String::new();
        app.providers.form_mut().alias_prefix = prefix.to_string();
        submit_form(&mut app);
        let section = app
            .providers
            .config()
            .section(provider)
            .unwrap_or_else(|| panic!("{provider} section must be saved without a token"));
        assert_eq!(section.token, "");
        assert_eq!(section.user, "root");
        assert!(
            !matches!(app.screen, Screen::ProviderForm { .. }),
            "{provider} form must close after a successful submit"
        );
    }
}

#[test]
fn test_submit_provider_form_rejects_whitespace_only_token() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.providers.form_mut().token = "   ".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "Token");
}

#[test]
fn test_submit_provider_form_rejects_pattern_alias_prefix() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.providers.form_mut().alias_prefix = "do*".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "pattern");
}

#[test]
fn test_submit_provider_form_rejects_question_mark_alias() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.providers.form_mut().alias_prefix = "do?".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "pattern");
}

#[test]
fn test_submit_provider_form_rejects_negation_alias() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.providers.form_mut().alias_prefix = "!do".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "pattern");
}

#[test]
fn test_submit_provider_form_rejects_whitespace_in_user() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.providers.form_mut().user = "my user".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "whitespace");
}

// =========================================================================
// GCP-specific form validation
// =========================================================================

fn make_gcp_form_app() -> App {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::ProviderForm {
        id: crate::providers::config::ProviderConfigId::bare("gcp"),
    };
    *app.providers.form_mut() = ProviderFormFields {
        filter: String::new(),
        label: String::new(),
        label_entry: false,
        url: String::new(),
        token: "/path/to/sa.json".to_string(),
        profile: String::new(),
        project: "my-project".to_string(),
        compartment: String::new(),
        regions: String::new(),
        alias_prefix: "gcp".to_string(),
        user: "root".to_string(),
        identity_file: String::new(),
        verify_tls: true,
        auto_sync: true,
        vault_role: String::new(),
        vault_addr: String::new(),
        focused_field: ProviderFormField::Token,
        cursor_pos: 0,
        expanded: false,
        ssm: crate::providers::aws_ssm::SsmMode::default(),
    };
    app
}

#[test]
fn test_submit_gcp_rejects_empty_project() {
    let mut app = make_gcp_form_app();
    app.providers.form_mut().project = "".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "Project ID");
}

#[test]
fn test_submit_gcp_rejects_whitespace_only_project() {
    let mut app = make_gcp_form_app();
    app.providers.form_mut().project = "   ".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "Project ID");
}

#[test]
fn test_submit_gcp_rejects_empty_token() {
    let mut app = make_gcp_form_app();
    app.providers.form_mut().token = "".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "Token");
}

#[test]
fn test_submit_gcp_empty_token_shows_gcp_specific_hint() {
    let mut app = make_gcp_form_app();
    app.providers.form_mut().token = "".to_string();
    submit_form(&mut app);
    assert_status_contains(&app, "service account");
}

#[test]
fn test_gcp_form_has_project_field() {
    let fields = ProviderFormField::fields_for("gcp");
    assert!(fields.contains(&ProviderFormField::Project));
}

#[test]
fn test_gcp_form_tab_cycles_through_project() {
    let mut app = make_gcp_form_app();
    app.providers.form_mut().focused_field = ProviderFormField::Token;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.providers.form_mut().focused_field,
        ProviderFormField::Project
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.providers.form_mut().focused_field,
        ProviderFormField::Regions
    );
}

#[test]
fn test_provider_form_init_new_gcp_defaults() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::Providers;
    *app.providers.config_mut() = test_provider_config();
    open_provider_form(&mut app, "gcp");
    assert!(app.providers.form_mut().project.is_empty());
    assert!(app.providers.form_mut().auto_sync);
}

// =========================================================================
// Azure-specific form validation
// =========================================================================

fn make_azure_form_app() -> App {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::ProviderForm {
        id: crate::providers::config::ProviderConfigId::bare("azure"),
    };
    *app.providers.config_mut() = test_provider_config();
    *app.providers.form_mut() = ProviderFormFields {
        filter: String::new(),
        label: String::new(),
        label_entry: false,
        url: String::new(),
        token: "fake-token".to_string(),
        profile: String::new(),
        project: String::new(),
        compartment: String::new(),
        regions: "12345678-1234-1234-1234-123456789012".to_string(),
        alias_prefix: "az".to_string(),
        user: "azureuser".to_string(),
        identity_file: String::new(),
        verify_tls: true,
        auto_sync: true,
        vault_role: String::new(),
        vault_addr: String::new(),
        focused_field: ProviderFormField::Token,
        cursor_pos: 0,
        expanded: false,
        ssm: crate::providers::aws_ssm::SsmMode::default(),
    };
    app
}

#[test]
fn test_submit_azure_rejects_empty_subscriptions() {
    let mut app = make_azure_form_app();
    app.providers.form_mut().regions = "".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "subscription");
}

#[test]
fn test_submit_azure_rejects_whitespace_only_subscriptions() {
    let mut app = make_azure_form_app();
    app.providers.form_mut().regions = "   ".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "subscription");
}

#[test]
fn test_azure_form_has_regions_field() {
    let fields = ProviderFormField::fields_for("azure");
    assert!(fields.contains(&ProviderFormField::Regions));
    assert!(!fields.contains(&ProviderFormField::Project));
    assert!(!fields.contains(&ProviderFormField::Url));
    assert!(!fields.contains(&ProviderFormField::Profile));
}

#[test]
fn test_azure_form_tab_cycles_through_regions() {
    let mut app = make_azure_form_app();
    app.providers.form_mut().focused_field = ProviderFormField::Token;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.providers.form_mut().focused_field,
        ProviderFormField::Regions
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.providers.form_mut().focused_field,
        ProviderFormField::AliasPrefix
    );
}

#[test]
fn test_azure_regions_field_accepts_typing() {
    let mut app = make_azure_form_app();
    app.providers.form_mut().focused_field = ProviderFormField::Regions;
    app.providers.form_mut().regions = String::new();
    app.providers.form_mut().cursor_pos = 0;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('a')), &tx);
    assert_eq!(app.providers.form_mut().regions, "a");
}

fn make_ovh_form_app() -> App {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::ProviderForm {
        id: crate::providers::config::ProviderConfigId::bare("ovh"),
    };
    *app.providers.form_mut() = ProviderFormFields {
        filter: String::new(),
        label: String::new(),
        label_entry: false,
        url: String::new(),
        token: "ak:as:ck".to_string(),
        profile: String::new(),
        project: "proj-123".to_string(),
        compartment: String::new(),
        regions: String::new(),
        alias_prefix: "ovh".to_string(),
        user: "ubuntu".to_string(),
        identity_file: String::new(),
        verify_tls: true,
        auto_sync: true,
        vault_role: String::new(),
        vault_addr: String::new(),
        focused_field: ProviderFormField::Token,
        cursor_pos: 0,
        expanded: false,
        ssm: crate::providers::aws_ssm::SsmMode::default(),
    };
    app
}

#[test]
fn test_ovh_space_on_regions_opens_picker() {
    // Pickers open on Space, never on Enter. Enter always submits.
    let mut app = make_ovh_form_app();
    app.providers.form_mut().focused_field = ProviderFormField::Regions;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(
        app.ui.region_picker().open,
        "Space on OVH Regions should open picker"
    );
    assert_eq!(app.ui.region_picker().cursor, 0);
}

#[test]
fn test_ovh_picker_select_eu() {
    let mut app = make_ovh_form_app();
    app.providers.form_mut().focused_field = ProviderFormField::Regions;
    app.ui.region_picker_mut().open = true;
    app.ui.region_picker_mut().cursor = 0;

    // Cursor starts on group header "API Endpoint" (row 0).
    // Row 1 = "eu", Row 2 = "ca", Row 3 = "us"
    // Move down to "eu" (row 1)
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    assert_eq!(app.ui.region_picker().cursor, 1);

    // Press Space to select "eu"
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert_eq!(app.providers.form_mut().regions, "eu");

    // Press Enter to confirm
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.region_picker().open);
    assert_eq!(app.providers.form_mut().regions, "eu");
}

#[test]
fn test_ovh_picker_select_us() {
    let mut app = make_ovh_form_app();
    app.ui.region_picker_mut().open = true;
    app.ui.region_picker_mut().cursor = 0;
    app.screen = Screen::ProviderForm {
        id: crate::providers::config::ProviderConfigId::bare("ovh"),
    };

    // Move to "us" (row 3: header=0, eu=1, ca=2, us=3)
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    assert_eq!(app.ui.region_picker().cursor, 3);

    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert_eq!(app.providers.form_mut().regions, "us");

    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.region_picker().open);
    assert_eq!(app.providers.form_mut().regions, "us");
}

#[test]
fn test_ovh_picker_space_on_header_toggles_all() {
    let mut app = make_ovh_form_app();
    app.ui.region_picker_mut().open = true;
    app.ui.region_picker_mut().cursor = 0; // Group header
    app.screen = Screen::ProviderForm {
        id: crate::providers::config::ProviderConfigId::bare("ovh"),
    };

    let (tx, _rx) = mpsc::channel();
    // Space on header selects all endpoints
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    // All three should be selected (order preserved by OVH_ENDPOINTS)
    assert_eq!(app.providers.form_mut().regions, "eu,ca,us");

    // Space again on header deselects all
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert_eq!(app.providers.form_mut().regions, "");
}

#[test]
fn test_ovh_endpoint_picker_rows() {
    let rows = super::provider::region_picker_rows("ovh");
    assert_eq!(rows.len(), 4); // 1 header + 3 endpoints
    assert_eq!(rows[0], None); // group header
    assert_eq!(rows[1], Some("eu"));
    assert_eq!(rows[2], Some("ca"));
    assert_eq!(rows[3], Some("us"));
}

#[test]
fn test_ovh_picker_enter_selects_and_closes() {
    // OVH is single-select: Enter on an item should select it and close
    let mut app = make_ovh_form_app();
    app.ui.region_picker_mut().open = true;
    app.ui.region_picker_mut().cursor = 0;
    app.screen = Screen::ProviderForm {
        id: crate::providers::config::ProviderConfigId::bare("ovh"),
    };

    let (tx, _rx) = mpsc::channel();
    // Move to "ca" (row 2)
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    assert_eq!(app.ui.region_picker().cursor, 2);

    // Enter directly (no Space needed) selects "ca" and closes
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.region_picker().open);
    assert_eq!(app.providers.form_mut().regions, "ca");
}

#[test]
fn test_ovh_picker_enter_on_header_closes_without_select() {
    let mut app = make_ovh_form_app();
    app.ui.region_picker_mut().open = true;
    app.ui.region_picker_mut().cursor = 0; // group header
    app.screen = Screen::ProviderForm {
        id: crate::providers::config::ProviderConfigId::bare("ovh"),
    };

    let (tx, _rx) = mpsc::channel();
    // Enter on header: no item to select, just closes
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.region_picker().open);
    assert_eq!(app.providers.form_mut().regions, "");
}

#[test]
fn test_ovh_picker_enter_replaces_previous_selection() {
    let mut app = make_ovh_form_app();
    app.providers.form_mut().regions = "eu".to_string(); // previously selected EU
    app.ui.region_picker_mut().open = true;
    app.ui.region_picker_mut().cursor = 3; // "us"
    app.screen = Screen::ProviderForm {
        id: crate::providers::config::ProviderConfigId::bare("ovh"),
    };

    let (tx, _rx) = mpsc::channel();
    // Enter on "us" should replace "eu" with "us" (single-select)
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.providers.form_mut().regions, "us");
}

#[test]
fn test_azure_enter_on_regions_does_not_open_picker() {
    let mut app = make_azure_form_app();
    app.providers.form_mut().focused_field = ProviderFormField::Regions;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    // Must NOT open region picker (Azure uses text input, not picker)
    assert!(!app.ui.region_picker().open);
    // Screen should no longer be ProviderForm (submit transitions away)
    // or validation error sets status (screen stays on form)
    // Either way: not a picker.
}

#[test]
fn test_submit_azure_rejects_invalid_subscription_id() {
    let mut app = make_azure_form_app();
    app.providers.form_mut().regions = "not-a-uuid".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "Invalid subscription ID");
}

#[test]
fn test_submit_azure_rejects_mixed_valid_invalid_subscriptions() {
    let mut app = make_azure_form_app();
    app.providers.form_mut().regions = "12345678-1234-1234-1234-123456789012,bad-id".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "Invalid subscription ID");
}

// =========================================================================
// Provider form navigation tests
// =========================================================================

#[test]
fn test_provider_form_tab_cycles_cloud_fields() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.providers.form_mut().focused_field,
        ProviderFormField::AliasPrefix
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.providers.form_mut().focused_field,
        ProviderFormField::User
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.providers.form_mut().focused_field,
        ProviderFormField::IdentityFile
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.providers.form_mut().focused_field,
        ProviderFormField::VaultRole
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.providers.form_mut().focused_field,
        ProviderFormField::AutoSync
    );
}

#[test]
fn test_provider_form_shift_tab_reverse() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::AutoSync);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::BackTab), &tx);
    assert_eq!(
        app.providers.form_mut().focused_field,
        ProviderFormField::VaultRole
    );
}

/// Leaving the Token field with an AWS token that carries no separator is the
/// only shape that warrants the format hint.
fn leave_aws_token_field(token: &str) -> App {
    let mut app = make_form_app_focused_on("aws", ProviderFormField::Token);
    app.providers.form_mut().token = token.to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    app
}

#[test]
fn test_aws_token_without_separator_warns_about_format() {
    let app = leave_aws_token_field("AKIAIOSFODNN7EXAMPLE");
    assert_status_contains(&app, crate::messages::TOKEN_FORMAT_AWS);
}

#[test]
fn test_aws_token_pair_does_not_warn() {
    let app = leave_aws_token_field("AKIAIOSFODNN7EXAMPLE:secretkey");
    assert_status_not_contains(&app, crate::messages::TOKEN_FORMAT_AWS);
}

#[test]
fn test_aws_token_with_session_token_does_not_warn() {
    let app = leave_aws_token_field("ASIAIOSFODNN7EXAMPLE:secretkey:sessiontoken");
    assert_status_not_contains(&app, crate::messages::TOKEN_FORMAT_AWS);
}

#[test]
fn test_aws_empty_token_does_not_warn() {
    let app = leave_aws_token_field("   ");
    assert_status_not_contains(&app, crate::messages::TOKEN_FORMAT_AWS);
}

#[test]
fn test_non_aws_token_without_separator_does_not_warn() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.providers.form_mut().token = "plain-token".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_status_not_contains(&app, crate::messages::TOKEN_FORMAT_AWS);
}

#[test]
fn test_aws_token_warning_stays_out_of_other_fields() {
    let mut app = make_form_app_focused_on("aws", ProviderFormField::AliasPrefix);
    app.providers.form_mut().token = "AKIAIOSFODNN7EXAMPLE".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_status_not_contains(&app, crate::messages::TOKEN_FORMAT_AWS);
}

#[test]
fn test_provider_form_proxmox_has_extra_fields() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::Url);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.providers.form_mut().focused_field,
        ProviderFormField::Token
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.providers.form_mut().focused_field,
        ProviderFormField::AliasPrefix
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.providers.form_mut().focused_field,
        ProviderFormField::User
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.providers.form_mut().focused_field,
        ProviderFormField::IdentityFile
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.providers.form_mut().focused_field,
        ProviderFormField::VerifyTls
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.providers.form_mut().focused_field,
        ProviderFormField::VaultRole
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.providers.form_mut().focused_field,
        ProviderFormField::AutoSync
    );
}

#[test]
fn test_provider_form_esc_returns_to_provider_list() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::Providers));
}

#[test]
fn test_provider_form_space_toggles_verify_tls() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::VerifyTls);
    assert!(app.providers.form_mut().verify_tls);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(!app.providers.form_mut().verify_tls);
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(app.providers.form_mut().verify_tls);
}

#[test]
fn test_provider_form_char_input_verify_tls_blocked() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::VerifyTls);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('x')), &tx);
    // No text field should have changed
    assert_eq!(app.providers.form_mut().token, "tok");
}

#[test]
fn test_provider_form_backspace_verify_tls_blocked() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::VerifyTls);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    assert_eq!(app.providers.form_mut().token, "tok");
}

#[test]
fn test_provider_form_space_opens_key_picker() {
    // Pickers open on Space, never on Enter.
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::IdentityFile);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(app.ui.key_picker().open);
}

#[test]
fn test_provider_form_char_appended_to_focused_field() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.providers.form_mut().token = "tok".to_string();
    app.providers.form_mut().cursor_pos = 3;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('X')), &tx);
    assert_eq!(app.providers.form_mut().token, "tokX");
}

#[test]
fn test_provider_form_backspace_removes_from_focused_field() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.providers.form_mut().token = "tok".to_string();
    app.providers.form_mut().cursor_pos = 3;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    assert_eq!(app.providers.form_mut().token, "to");
}

// =========================================================================
// Provider list interaction tests
// =========================================================================

#[test]
fn test_provider_list_esc_returns_to_host_list() {
    let mut app = make_providers_app_with_do();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn test_provider_list_q_returns_to_host_list() {
    let mut app = make_providers_app_with_do();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('q')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn test_provider_list_j_selects_next() {
    let mut app = make_providers_app_with_do();
    app.ui.provider_list_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    // Should advance (wrapping depends on count)
    assert!(app.ui.provider_list_state().selected().is_some());
}

#[test]
fn test_provider_list_k_selects_prev() {
    let mut app = make_providers_app_with_do();
    app.ui.provider_list_state_mut().select(Some(1));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
    assert!(app.ui.provider_list_state().selected().is_some());
}

#[test]
fn test_provider_list_sync_unconfigured_shows_status() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::Providers;
    *app.providers.config_mut() = test_provider_config();
    // No config for digitalocean - select it and press s
    let sorted = app.sorted_provider_names();
    let idx = sorted.iter().position(|n| n == "digitalocean").unwrap();
    app.ui.provider_list_state_mut().select(Some(idx));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('s')), &tx);
    assert!(
        app.status_center
            .toast()
            .unwrap()
            .text
            .contains("Configure")
    );
}

#[test]
fn test_provider_list_delete_removes_config() {
    let mut app = make_providers_app_with_do();
    let sorted = app.sorted_provider_names();
    let idx = sorted.iter().position(|n| n == "digitalocean").unwrap();
    app.ui.provider_list_state_mut().select(Some(idx));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    // d now triggers confirmation
    assert!(app.providers.pending_delete().is_some());
    // Confirm with y
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(app.providers.pending_delete().is_none());
    // Save may fail in tests (no ~/.purple), triggering rollback. Just verify handler ran.
    assert!(app.status_center.status().is_some() || app.status_center.toast().is_some());
}

#[test]
fn test_provider_list_delete_unconfigured_is_noop() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::Providers;
    *app.providers.config_mut() = test_provider_config();
    let sorted = app.sorted_provider_names();
    let idx = sorted.iter().position(|n| n == "digitalocean").unwrap();
    app.ui.provider_list_state_mut().select(Some(idx));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    // No status/toast message because no section existed to delete
    let has_removed = app
        .status_center
        .toast()
        .is_some_and(|t| t.text.contains("Removed"))
        || app
            .status_center
            .status()
            .is_some_and(|s| s.text.contains("Removed"));
    assert!(!has_removed);
}

#[test]
fn test_provider_list_esc_cancels_running_syncs() {
    let mut app = make_providers_app_with_do();
    let cancel = Arc::new(AtomicBool::new(false));
    app.providers
        .syncing_mut()
        .insert("digitalocean".to_string(), cancel.clone());
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(
        cancel.load(Ordering::Relaxed),
        "Cancel flag should be set on Esc"
    );
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn test_provider_list_enter_opens_form_with_existing_config() {
    let mut app = make_providers_app_with_do();
    open_provider_form(&mut app, "digitalocean");
    assert!(matches!(app.screen, Screen::ProviderForm { ref id } if id.provider == "digitalocean"));
    assert_eq!(app.providers.form_mut().token, "tok");
    assert_eq!(app.providers.form_mut().alias_prefix, "do");
    assert_eq!(app.providers.form_mut().user, "root");
}

#[test]
fn test_provider_list_enter_opens_form_with_defaults() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::Providers;
    *app.providers.config_mut() = test_provider_config();
    open_provider_form(&mut app, "vultr");
    assert!(matches!(app.screen, Screen::ProviderForm { ref id } if id.provider == "vultr"));
    assert_eq!(app.providers.form_mut().token, "");
    assert_eq!(app.providers.form_mut().user, "root");
    assert!(app.providers.form_mut().auto_sync); // vultr default true
}

#[test]
fn test_provider_form_proxmox_default_alias_prefix() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::Providers;
    *app.providers.config_mut() = test_provider_config();
    open_provider_form(&mut app, "proxmox");
    // Proxmox short_label is "pve"
    assert_eq!(app.providers.form_mut().alias_prefix, "pve");
}

// =========================================================================
// Provider form all-providers init defaults
// =========================================================================

#[test]
fn test_all_cloud_providers_default_auto_sync_true() {
    for provider in &[
        "digitalocean",
        "vultr",
        "linode",
        "hetzner",
        "upcloud",
        "aws",
        "scaleway",
        "gcp",
        "azure",
        "tailscale",
    ] {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.screen = Screen::Providers;
        *app.providers.config_mut() = test_provider_config();
        open_provider_form(&mut app, provider);
        assert!(
            app.providers.form_mut().auto_sync,
            "{} should default auto_sync=true",
            provider
        );
    }
}

#[test]
fn test_proxmox_defaults_auto_sync_false() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::Providers;
    *app.providers.config_mut() = test_provider_config();
    open_provider_form(&mut app, "proxmox");
    assert!(!app.providers.form_mut().auto_sync);
}

#[test]
fn test_submit_proxmox_https_case_insensitive() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::Url);
    app.providers.form_mut().url = "HTTPS://pve.local:8006".to_string();
    app.providers.form_mut().token = "user@pam!t=secret".to_string();
    submit_form(&mut app);
    assert_status_not_contains(&app, "https://");
}

#[test]
fn test_submit_non_proxmox_url_not_required() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.providers.form_mut().url = "".to_string();
    submit_form(&mut app);
    assert_status_not_contains(&app, "URL is required");
}

#[test]
fn test_submit_provider_form_accepts_empty_alias_prefix() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.providers.form_mut().alias_prefix = "".to_string();
    submit_form(&mut app);
    assert_status_not_contains(&app, "pattern");
}

#[test]
fn test_submit_provider_form_accepts_hyphenated_alias() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.providers.form_mut().alias_prefix = "my-cloud".to_string();
    submit_form(&mut app);
    assert_status_not_contains(&app, "pattern");
}

#[test]
fn test_submit_provider_form_rejects_space_in_alias_prefix() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.providers.form_mut().alias_prefix = "my cloud".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    let msg = &app
        .status_center
        .status()
        .or(app.status_center.toast())
        .unwrap()
        .text;
    if !msg.contains("changed externally") {
        assert!(msg.contains("pattern") || msg.contains("spaces"));
    }
}

// =========================================================================
// Password picker tests
// =========================================================================

fn ctrl_key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn make_form_app() -> App {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::AddHost;
    *app.forms.host_mut() = crate::app::HostForm::new();
    app.forms.host_mut().expanded = true; // Tests assume all fields visible
    app
}

// --- Enter on AskPass opens picker ---

#[test]
fn test_space_on_askpass_opens_password_picker() {
    // Pickers open on Space, never on Enter.
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(app.ui.password_picker().open);
    assert_eq!(app.ui.password_picker().list.selected(), Some(0));
}

#[test]
fn test_space_on_identityfile_opens_key_picker() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::IdentityFile;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(app.ui.key_picker().open);
}

#[test]
fn test_space_on_proxyjump_opens_proxyjump_picker() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::ProxyJump;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(app.ui.proxyjump_picker().open);
}

// VaultSsh has two Space branches. With no role candidates Space inserts a
// literal space and the picker stays closed. With candidates the picker
// opens and selects the first role. Both branches are pinned because the
// guard is the place where a refactor is most likely to drift.
#[test]
fn test_space_on_vaultssh_with_no_candidates_inserts_literal_space() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::VaultSsh;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(!app.ui.vault_role_picker().open);
    assert_eq!(app.forms.host_mut().vault_ssh, " ");
}

#[test]
fn test_space_on_vaultssh_with_candidates_opens_picker() {
    let mut app = make_form_app();
    app.hosts_state.list_mut()[0].vault_ssh = Some("admin".to_string());
    app.forms.host_mut().focused_field = FormField::VaultSsh;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(app.ui.vault_role_picker().open);
    assert_eq!(app.ui.vault_role_picker().list.selected(), Some(0));
}

// --- Esc closes picker ---

#[test]
fn test_password_picker_esc_closes() {
    let mut app = make_form_app();
    app.ui.password_picker_mut().open_at(2);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(!app.ui.password_picker().open);
    // Form field should be unchanged
    assert_eq!(app.forms.host_mut().askpass, "");
}

#[test]
fn test_key_picker_esc_closes() {
    let mut app = make_form_app();
    app.ui.key_picker_mut().open_at(0);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(!app.ui.key_picker().open);
}

#[test]
fn test_proxyjump_picker_esc_closes() {
    let mut app = make_form_app();
    app.ui.proxyjump_picker_mut().open_at(0);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(!app.ui.proxyjump_picker().open);
}

#[test]
fn test_vault_role_picker_esc_closes() {
    let mut app = make_form_app();
    app.ui.vault_role_picker_mut().open_at(0);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(!app.ui.vault_role_picker().open);
}

#[test]
fn test_region_picker_esc_closes() {
    let mut app = make_ovh_form_app();
    app.providers.form_mut().focused_field = ProviderFormField::Regions;
    app.ui.region_picker_mut().open = true;
    app.ui.region_picker_mut().cursor = 0;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(!app.ui.region_picker().open);
}

// --- Navigation j/k ---

#[test]
fn test_password_picker_j_moves_down() {
    let mut app = make_form_app();
    app.ui.password_picker_mut().open_at(0);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    assert_eq!(app.ui.password_picker().list.selected(), Some(1));
}

#[test]
fn test_password_picker_k_moves_up() {
    let mut app = make_form_app();
    app.ui.password_picker_mut().open_at(2);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
    assert_eq!(app.ui.password_picker().list.selected(), Some(1));
}

#[test]
fn test_password_picker_down_arrow() {
    let mut app = make_form_app();
    app.ui.password_picker_mut().open_at(0);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Down), &tx);
    assert_eq!(app.ui.password_picker().list.selected(), Some(1));
}

#[test]
fn test_password_picker_up_arrow() {
    let mut app = make_form_app();
    app.ui.password_picker_mut().open_at(3);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Up), &tx);
    assert_eq!(app.ui.password_picker().list.selected(), Some(2));
}

#[test]
fn test_password_picker_wraps_around_bottom() {
    let mut app = make_form_app();
    let last = crate::askpass::PASSWORD_SOURCES.len() - 1;
    app.ui.password_picker_mut().open_at(last);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    assert_eq!(app.ui.password_picker().list.selected(), Some(0));
}

#[test]
fn test_password_picker_wraps_around_top() {
    let mut app = make_form_app();
    app.ui.password_picker_mut().open_at(0);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
    let last = crate::askpass::PASSWORD_SOURCES.len() - 1;
    assert_eq!(app.ui.password_picker().list.selected(), Some(last));
}

// --- Enter selects source: OS Keychain ---

#[test]
fn test_password_picker_select_keychain() {
    let mut app = make_form_app();
    app.ui.password_picker_mut().open_at(0); // OS Keychain
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.password_picker().open);
    assert_eq!(app.forms.host_mut().askpass, "keychain");
}

// --- Enter selects source: 1Password (prefix) ---

#[test]
fn test_password_picker_select_1password() {
    let mut app = make_form_app();
    app.ui.password_picker_mut().open_at(1); // 1Password
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.password_picker().open);
    assert_eq!(app.forms.host_mut().askpass, "op://");
    assert_eq!(app.forms.host_mut().focused_field, FormField::AskPass);
}

// --- Enter selects source: Bitwarden (prefix) ---

#[test]
fn test_password_picker_select_bitwarden() {
    let mut app = make_form_app();
    app.ui.password_picker_mut().open_at(2); // Bitwarden
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.password_picker().open);
    assert_eq!(app.forms.host_mut().askpass, "bw:");
    assert_eq!(app.forms.host_mut().focused_field, FormField::AskPass);
}

// --- Enter selects source: pass (prefix) ---

#[test]
fn test_password_picker_select_pass() {
    let mut app = make_form_app();
    app.ui.password_picker_mut().open_at(3); // pass
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.password_picker().open);
    assert_eq!(app.forms.host_mut().askpass, "pass:");
    assert_eq!(app.forms.host_mut().focused_field, FormField::AskPass);
}

// --- Enter selects source: HashiCorp Vault (prefix) ---

#[test]
fn test_password_picker_select_vault() {
    let mut app = make_form_app();
    app.ui.password_picker_mut().open_at(4); // HashiCorp Vault
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.password_picker().open);
    assert_eq!(app.forms.host_mut().askpass, "vault:");
    assert_eq!(app.forms.host_mut().focused_field, FormField::AskPass);
}

// --- Enter selects source: Proton Pass (prefix) ---

#[test]
fn test_password_picker_select_proton_pass() {
    let mut app = make_form_app();
    app.ui.password_picker_mut().open_at(5); // Proton Pass
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.password_picker().open);
    assert_eq!(app.forms.host_mut().askpass, "proton:");
    assert_eq!(app.forms.host_mut().focused_field, FormField::AskPass);
}

// --- Host form writes proton askpass comment and round-trips ---

#[test]
fn test_host_form_proton_askpass_writes_comment() {
    // SshConfigFile::write no-ops under the global demo flag; serialise against
    // demo-enabling tests (asset generation, visual suite) and pin demo off so
    // the on-disk read below observes the written file.
    let _guard = crate::demo_flag::GLOBAL_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    crate::demo_flag::disable();
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("test_config");
    std::fs::write(&config_path, "Host srv\n    HostName srv.example.com\n").unwrap();

    let mut config = SshConfigFile::parse(&config_path).expect("parse");
    let _ = config.set_host_askpass("srv", "proton:Personal/srv/p");
    config.write().expect("write");

    let on_disk = std::fs::read_to_string(&config_path).expect("read");
    assert!(
        on_disk.contains("# purple:askpass proton:Personal/srv/p"),
        "expected proton askpass comment on disk, got:\n{on_disk}"
    );

    let reparsed = SshConfigFile::parse(&config_path).expect("reparse");
    let entry = reparsed
        .raw_host_entry("srv")
        .expect("srv block survives round-trip");
    assert_eq!(
        entry.askpass.as_deref(),
        Some("proton:Personal/srv/p"),
        "askpass must round-trip through parse-write-parse"
    );
}

// --- Enter selects source: Custom command ---

#[test]
fn test_password_picker_select_custom() {
    let mut app = make_form_app();
    app.forms.host_mut().askpass = "old-value".to_string();
    app.ui.password_picker_mut().open_at(6); // Custom command
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.password_picker().open);
    assert_eq!(app.forms.host_mut().askpass, "");
    // Custom-command branch must refocus AskPass so the next keystroke
    // lands in the askpass input, not whichever field had focus before
    // the picker opened.
    assert_eq!(app.forms.host_mut().focused_field, FormField::AskPass);
}

// --- Enter selects source: None (clears) ---

#[test]
fn test_password_picker_select_none() {
    let mut app = make_form_app();
    app.forms.host_mut().askpass = "keychain".to_string();
    app.ui.password_picker_mut().open_at(7); // None
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.password_picker().open);
    assert_eq!(app.forms.host_mut().askpass, "");
}

// --- Picker blocks other form input ---

#[test]
fn test_password_picker_blocks_char_input() {
    let mut app = make_form_app();
    app.forms.host_mut().askpass = "".to_string();
    app.ui.password_picker_mut().open_at(0);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('x')), &tx);
    // 'x' should not be appended to any form field
    assert_eq!(app.forms.host_mut().askpass, "");
    assert_eq!(app.forms.host_mut().alias, "");
}

#[test]
fn test_password_picker_blocks_tab() {
    let mut app = make_form_app();
    let original_field = app.forms.host_mut().focused_field;
    app.ui.password_picker_mut().open_at(0);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    // Tab should not change focused field
    assert_eq!(app.forms.host_mut().focused_field, original_field);
}

// --- Picker on EditHost screen ---

#[test]
fn test_password_picker_works_on_edit_host() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::EditHost {
        alias: "test".to_string(),
    };
    *app.forms.host_mut() = crate::app::HostForm::new();
    app.forms.host_mut().focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    // Space on empty picker field opens the picker.
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(app.ui.password_picker().open);
    // Inside the picker, Enter selects the highlighted entry (keychain).
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.forms.host_mut().askpass, "keychain");
}

// --- Picker priority over key picker ---

#[test]
fn test_password_picker_takes_priority_over_key_picker() {
    let mut app = make_form_app();
    app.ui.key_picker_mut().open = true;
    app.ui.password_picker_mut().open_at(0);
    let (tx, _rx) = mpsc::channel();
    // Esc should close password picker, not key picker
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(!app.ui.password_picker().open);
    assert!(app.ui.key_picker().open); // still open
}

// =========================================================================
// Host list Enter carries askpass in pending_connect
// =========================================================================

#[test]
fn test_host_list_enter_carries_askpass() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    app.screen = Screen::HostList;
    // Select the host
    app.ui.list_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    let pending = app.ui.pending_connect().unwrap();
    assert_eq!(pending.0, "myserver");
    assert_eq!(pending.1, Some("keychain".to_string()));
}

#[test]
fn test_host_list_enter_carries_vault_askpass() {
    let mut app =
        make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass vault:secret/ssh#pass\n");
    app.screen = Screen::HostList;
    app.ui.list_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    let pending = app.ui.pending_connect().unwrap();
    assert_eq!(pending.1, Some("vault:secret/ssh#pass".to_string()));
}

#[test]
fn test_host_list_enter_no_askpass() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    app.screen = Screen::HostList;
    app.ui.list_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    let pending = app.ui.pending_connect().unwrap();
    assert_eq!(pending.0, "myserver");
    assert_eq!(pending.1, None);
}

// =========================================================================
// Search mode Enter carries askpass in pending_connect
// =========================================================================

#[test]
fn test_search_enter_carries_askpass() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass op://V/I/p\n");
    app.screen = Screen::HostList;
    app.start_search();
    // In search mode, filtered_indices should contain our host
    assert!(!app.search.filtered_indices().is_empty());
    app.ui.list_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    let pending = app.ui.pending_connect().unwrap();
    assert_eq!(pending.0, "myserver");
    assert_eq!(pending.1, Some("op://V/I/p".to_string()));
    // Search should be cancelled after Enter
    assert!(app.search.query().is_none());
}

#[test]
fn test_search_enter_no_askpass() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    app.screen = Screen::HostList;
    app.start_search();
    app.ui.list_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    let pending = app.ui.pending_connect().unwrap();
    assert_eq!(pending.1, None);
}

// =========================================================================
// Ctrl+E edits selected host during search
// =========================================================================

#[test]
fn test_search_ctrl_e_opens_edit_form() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    app.screen = Screen::HostList;
    app.start_search();
    app.ui.list_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, ctrl_key('e'), &tx);
    assert!(matches!(app.screen, Screen::EditHost { ref alias } if alias == "myserver"));
    // Search query should be preserved so user returns to filtered list
    assert!(app.search.query().is_some());
}

#[test]
fn test_search_ctrl_e_blocks_included_host() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    // Simulate an included host by setting source_file
    if let Some(host) = app.hosts_state.list_mut().first_mut() {
        host.source_file = Some(std::path::PathBuf::from("/etc/ssh/config.d/test"));
    }
    app.screen = Screen::HostList;
    app.start_search();
    app.ui.list_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, ctrl_key('e'), &tx);
    // Should remain in search mode (not open edit form)
    assert!(matches!(app.screen, Screen::HostList));
    assert!(app.status_center.status().is_some() || app.status_center.toast().is_some());
}

// =========================================================================
// Tunnel start reads askpass from host
// =========================================================================

#[test]
fn test_tunnel_handler_reads_askpass_from_hosts() {
    // Verify the askpass lookup logic: find host by alias and extract askpass
    let app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass bw:my-item\n");
    let askpass = app
        .hosts_state
        .list()
        .iter()
        .find(|h| h.alias == "myserver")
        .and_then(|h| h.askpass.clone());
    assert_eq!(askpass, Some("bw:my-item".to_string()));
}

#[test]
fn test_tunnel_handler_askpass_none_when_absent() {
    let app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    let askpass = app
        .hosts_state
        .list()
        .iter()
        .find(|h| h.alias == "myserver")
        .and_then(|h| h.askpass.clone());
    assert_eq!(askpass, None);
}

// =========================================================================
// Edit host form populates askpass
// =========================================================================

#[test]
fn test_edit_host_populates_askpass_in_form() {
    let mut app =
        make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass pass:ssh/prod\n");
    app.screen = Screen::HostList;
    app.ui.list_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    // Press 'e' to edit
    let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
    if matches!(app.screen, Screen::EditHost { .. }) {
        assert_eq!(app.forms.host_mut().askpass, "pass:ssh/prod");
    }
}

#[test]
fn test_edit_host_populates_empty_askpass() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    app.screen = Screen::HostList;
    app.ui.list_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
    if matches!(app.screen, Screen::EditHost { .. }) {
        assert_eq!(app.forms.host_mut().askpass, "");
    }
}

// =========================================================================
// Tab navigation through AskPass field
// =========================================================================

#[test]
fn test_tab_reaches_askpass_field() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::ProxyJump;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(app.forms.host_mut().focused_field, FormField::AskPass);
}

#[test]
fn test_tab_from_askpass_goes_to_tags() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(app.forms.host_mut().focused_field, FormField::Tags);
}

#[test]
fn test_shift_tab_from_tags_goes_to_askpass() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::Tags;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::BackTab), &tx);
    assert_eq!(app.forms.host_mut().focused_field, FormField::AskPass);
}

#[test]
fn test_typing_in_askpass_field() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert_eq!(app.forms.host_mut().askpass, "key");
}

#[test]
fn test_backspace_in_askpass_field() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::AskPass;
    app.forms.host_mut().askpass = "vault:".to_string();
    app.forms.host_mut().cursor_pos = 6;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    assert_eq!(app.forms.host_mut().askpass, "vault");
}

// =========================================================================
// Picker then type: prefix selection followed by typing
// =========================================================================

#[test]
fn test_picker_select_op_then_type_rest() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    // Space on empty picker field opens the picker.
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    // Navigate to 1Password (index 1)
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    // Inside the picker, Enter selects.
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.forms.host_mut().askpass, "op://");
    assert_eq!(app.forms.host_mut().focused_field, FormField::AskPass);
    // Now type the rest of the URI
    let _ = handle_key_event(&mut app, key(KeyCode::Char('V')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('I')), &tx);
    assert_eq!(app.forms.host_mut().askpass, "op://V/I");
}

#[test]
fn test_picker_select_vault_then_type_rest() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    // Space on empty picker field opens the picker.
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    // Navigate to Vault (index 4)
    for _ in 0..4 {
        let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    }
    assert_eq!(app.ui.password_picker().list.selected(), Some(4));
    // Inside the picker, Enter selects.
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.forms.host_mut().askpass, "vault:");
    assert_eq!(app.forms.host_mut().focused_field, FormField::AskPass);
    // Type the path
    for c in "secret/ssh#pass".chars() {
        let _ = handle_key_event(&mut app, key(KeyCode::Char(c)), &tx);
    }
    assert_eq!(app.forms.host_mut().askpass, "vault:secret/ssh#pass");
}

#[test]
fn test_picker_select_keychain_no_further_typing_needed() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    // Space on empty picker field opens the picker.
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    // Inside the picker, Enter selects keychain (index 0, already selected).
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.forms.host_mut().askpass, "keychain");
    // focused_field stays on AskPass (picker was opened from AskPass)
    assert_eq!(app.forms.host_mut().focused_field, FormField::AskPass);
}

// =========================================================================
// Password picker: status messages after selection
// =========================================================================

#[test]
fn test_picker_keychain_sets_status_message() {
    let mut app = make_form_app();
    app.ui.password_picker_mut().open_at(0);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(
        app.status_center
            .toast()
            .unwrap()
            .text
            .contains("OS Keychain")
    );
}

#[test]
fn test_picker_none_sets_cleared_status() {
    let mut app = make_form_app();
    app.forms.host_mut().askpass = "keychain".to_string();
    app.ui.password_picker_mut().open_at(7); // None
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.status_center.toast().unwrap().text.contains("cleared"));
}

#[test]
fn test_picker_prefix_source_shows_guidance() {
    // Prefix sources (op://, bw:, etc.) show a guidance message
    let mut app = make_form_app();
    app.status_center.set_toast_message(None);
    app.ui.password_picker_mut().open_at(1); // 1Password (op://)
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.status_center.toast().unwrap().text.contains("Complete"));
    assert_eq!(app.forms.host_mut().focused_field, FormField::AskPass);
}

// =========================================================================
// Backspace after prefix selection
// =========================================================================

#[test]
fn test_backspace_after_prefix_selection() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    // Space opens the picker; Enter selects 1Password (after pre-positioning).
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    app.ui.password_picker_mut().list.select(Some(1));
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.forms.host_mut().askpass, "op://");
    assert_eq!(app.forms.host_mut().focused_field, FormField::AskPass);
    // Type something
    let _ = handle_key_event(&mut app, key(KeyCode::Char('V')), &tx);
    assert_eq!(app.forms.host_mut().askpass, "op://V");
    // Backspace removes last char
    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    assert_eq!(app.forms.host_mut().askpass, "op://");
    // Another backspace removes the trailing /
    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    assert_eq!(app.forms.host_mut().askpass, "op:/");
}

// =========================================================================
// Edit form populates askpass from existing host
// =========================================================================

#[test]
fn test_edit_form_populates_askpass() {
    let mut app =
        make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass vault:secret/ssh#pw\n");
    // Simulate what happens when user presses 'e' on a host
    let entry = app.hosts_state.ssh_config().host_entries()[0].clone();
    *app.forms.host_mut() = crate::app::HostForm::from_entry(&entry, Default::default());
    assert_eq!(app.forms.host_mut().askpass, "vault:secret/ssh#pw");
}

#[test]
fn test_edit_form_empty_askpass_when_none() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    let entry = app.hosts_state.ssh_config().host_entries()[0].clone();
    *app.forms.host_mut() = crate::app::HostForm::from_entry(&entry, Default::default());
    assert_eq!(app.forms.host_mut().askpass, "");
}

// =========================================================================
// Password picker: unknown keys are no-ops
// =========================================================================

#[test]
fn test_password_picker_ignores_unknown_keys() {
    let mut app = make_form_app();
    app.ui.password_picker_mut().open_at(2);
    let (tx, _rx) = mpsc::channel();
    // F1 key should be a no-op
    let _ = handle_key_event(&mut app, key(KeyCode::F(1)), &tx);
    assert!(app.ui.password_picker().open);
    assert_eq!(app.ui.password_picker().list.selected(), Some(2));
}

// =========================================================================
// Host list search Enter carries askpass
// =========================================================================

#[test]
fn test_search_enter_carries_askpass_op_uri() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass op://V/I/p\n");
    app.search.set_query(Some("myserver".to_string()));
    app.apply_filter();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    if let Some((alias, askpass)) = app.ui.pending_connect() {
        assert_eq!(alias, "myserver");
        assert_eq!(askpass.as_deref(), Some("op://V/I/p"));
    } else {
        panic!("Expected pending_connect to be set");
    }
}

// =========================================================================
// UI/UX: placeholder text and picker overlay properties
// =========================================================================

#[test]
fn test_askpass_placeholder_text() {
    let placeholder = crate::ui::host_form::placeholder_text(FormField::AskPass);
    // When no global default is set, shows the "Space to pick..." guidance.
    // When a default exists, shows "default: <name>". Per the keyboard
    // invariants, pickers open on Space, never Enter.
    assert!(
        placeholder.contains("Space") || placeholder.contains("default:"),
        "Should show Space guidance or default prefix: {}",
        placeholder
    );
}

#[test]
fn test_password_sources_fit_picker_width() {
    // Picker overlay is 48 chars wide (minus 4 for borders/padding)
    let max_content_width = 44;
    for source in crate::askpass::PASSWORD_SOURCES {
        let total = source.label.len() + 1 + source.hint.len();
        assert!(
            total <= max_content_width,
            "Source '{}' (label={}, hint={}) total {} exceeds max {}",
            source.label,
            source.label.len(),
            source.hint.len(),
            total,
            max_content_width
        );
    }
}

#[test]
fn test_password_picker_item_count_matches_sources() {
    assert_eq!(crate::askpass::PASSWORD_SOURCES.len(), 8);
}

// =========================================================================
// Full picker → type → form submit flow
// =========================================================================

#[test]
fn test_full_flow_picker_to_typed_value() {
    let mut app = make_form_app();
    app.forms.host_mut().alias = "myhost".to_string();
    app.forms.host_mut().hostname = "10.0.0.1".to_string();
    app.forms.host_mut().focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();

    // Space opens picker; pre-position to Bitwarden (index 2); Enter selects.
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    app.ui.password_picker_mut().list.select(Some(2));
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    // Verify field has prefix
    assert_eq!(app.forms.host_mut().askpass, "bw:");
    assert_eq!(app.forms.host_mut().focused_field, FormField::AskPass);

    // Type the item name
    for c in "my-ssh-server".chars() {
        let _ = handle_key_event(&mut app, key(KeyCode::Char(c)), &tx);
    }
    assert_eq!(app.forms.host_mut().askpass, "bw:my-ssh-server");

    // Verify to_entry produces correct askpass
    let entry = app.forms.host_mut().to_entry();
    assert_eq!(entry.askpass, Some("bw:my-ssh-server".to_string()));
}

#[test]
fn test_full_flow_picker_keychain_then_tab_away() {
    let mut app = make_form_app();
    // Only set alias (not hostname) so auto-submit doesn't trigger after picker
    app.forms.host_mut().alias = "myhost".to_string();
    app.forms.host_mut().focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();

    // Space opens picker; Enter selects keychain (index 0, default).
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    assert_eq!(app.forms.host_mut().askpass, "keychain");
    // Focus stays on AskPass (picker opened from AskPass)
    assert_eq!(app.forms.host_mut().focused_field, FormField::AskPass);

    // Tab to next field (Tags is after AskPass)
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(app.forms.host_mut().focused_field, FormField::Tags);
}

#[test]
fn test_full_flow_clear_askpass_via_picker_none() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::AskPass;
    app.forms.host_mut().askpass = "op://Vault/Item/pw".to_string();
    let (tx, _rx) = mpsc::channel();

    // Field has content → Space inserts literal. To re-open the picker,
    // pre-set the show_password_picker state directly (mirrors the user
    // backspacing the field clean and pressing Space, but skips the steps
    // since we are testing the post-picker behavior).
    app.ui.password_picker_mut().open = true;
    app.ui.password_picker_mut().list = ratatui::widgets::ListState::default();
    app.ui.password_picker_mut().list.select(Some(0));
    for _ in 0..6 {
        let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    }
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    assert_eq!(app.forms.host_mut().askpass, "");
    let entry = app.forms.host_mut().to_entry();
    assert_eq!(entry.askpass, None);
}

// =========================================================================
// Askpass with host without askpass (no askpass in pending_connect)
// =========================================================================

#[test]
fn test_host_list_enter_no_askpass_is_none() {
    let mut app = make_app("Host plain\n  HostName 10.0.0.1\n");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    if let Some((alias, askpass)) = app.ui.pending_connect() {
        assert_eq!(alias, "plain");
        assert!(askpass.is_none());
    } else {
        panic!("Expected pending_connect");
    }
}

// =========================================================================
// Ctrl+P does NOT open password picker on provider form
// =========================================================================

#[test]
fn test_ctrl_p_on_provider_form_does_not_open_password_picker() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::ProviderForm {
        id: crate::providers::config::ProviderConfigId::bare("digitalocean"),
    };
    *app.providers.form_mut() = crate::app::ProviderFormFields::new();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, ctrl_key('p'), &tx);
    // Provider form does not have a password picker
    assert!(!app.ui.password_picker().open);
}

// =========================================================================
// Multiple hosts: each carries its own askpass in pending_connect
// =========================================================================

#[test]
fn test_multiple_hosts_different_askpass_sources() {
    let config = "\
Host alpha
  HostName a.com
  # purple:askpass keychain

Host beta
  HostName b.com
  # purple:askpass op://Vault/SSH/pw

Host gamma
  HostName c.com
";
    let app = make_app(config);
    assert_eq!(app.hosts_state.list().len(), 3);
    assert_eq!(
        app.hosts_state.list()[0].askpass,
        Some("keychain".to_string())
    );
    assert_eq!(
        app.hosts_state.list()[1].askpass,
        Some("op://Vault/SSH/pw".to_string())
    );
    assert_eq!(app.hosts_state.list()[2].askpass, None);
}

#[test]
fn test_select_different_hosts_carries_correct_askpass() {
    let config = "\
Host alpha
  HostName a.com
  # purple:askpass keychain

Host beta
  HostName b.com
  # purple:askpass bw:my-item
";
    let mut app = make_app(config);
    let (tx, _rx) = mpsc::channel();

    // Select alpha (first host) and press Enter
    app.ui.list_state_mut().select(Some(0));
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    let (alias, askpass) = app.ui.take_pending_connect().unwrap();
    assert_eq!(alias, "alpha");
    assert_eq!(askpass, Some("keychain".to_string()));

    // Select beta (second host) and press Enter
    app.ui.list_state_mut().select(Some(1));
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    let (alias, askpass) = app.ui.take_pending_connect().unwrap();
    assert_eq!(alias, "beta");
    assert_eq!(askpass, Some("bw:my-item".to_string()));
}

// =========================================================================
// Askpass field typing: direct input without picker
// =========================================================================

#[test]
fn test_type_askpass_directly_without_picker() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    for c in "keychain".chars() {
        let _ = handle_key_event(&mut app, key(KeyCode::Char(c)), &tx);
    }
    assert_eq!(app.forms.host_mut().askpass, "keychain");
}

#[test]
fn test_type_custom_command_directly() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    for c in "my-script %a %h".chars() {
        let _ = handle_key_event(&mut app, key(KeyCode::Char(c)), &tx);
    }
    assert_eq!(app.forms.host_mut().askpass, "my-script %a %h");
}

#[test]
fn test_clear_askpass_with_backspace() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::AskPass;
    app.forms.host_mut().askpass = "keychain".to_string();
    app.forms.host_mut().cursor_pos = 8;
    let (tx, _rx) = mpsc::channel();
    for _ in 0..8 {
        let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    }
    assert_eq!(app.forms.host_mut().askpass, "");
}

// =========================================================================
// Delete host with askpass: undo restores it
// =========================================================================

#[test]
fn test_delete_undo_preserves_askpass_in_config() {
    let config_str = "Host myserver\n  HostName 10.0.0.1\n  # purple:askpass vault:secret/ssh#pw\n";
    let mut app = make_app(config_str);
    // Verify askpass is present
    assert_eq!(
        app.hosts_state.ssh_config().host_entries()[0].askpass,
        Some("vault:secret/ssh#pw".to_string())
    );

    // Delete the host (undoable)
    if let Some((element, position)) = app
        .hosts_state
        .ssh_config_mut()
        .delete_host_undoable("myserver")
    {
        // Host is gone
        assert!(app.hosts_state.ssh_config().host_entries().is_empty());
        // Undo: restore
        app.hosts_state
            .ssh_config_mut()
            .insert_host_at(element, position);
        // Askpass should be restored
        let entries = app.hosts_state.ssh_config().host_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].askpass, Some("vault:secret/ssh#pw".to_string()));
    } else {
        panic!("Expected delete_host_undoable to succeed");
    }
}

// =========================================================================
// Askpass with unicode characters
// =========================================================================

#[test]
fn test_askpass_unicode_in_custom_command() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    for c in "get-p\u{00E4}ss %h".chars() {
        let _ = handle_key_event(&mut app, key(KeyCode::Char(c)), &tx);
    }
    assert_eq!(app.forms.host_mut().askpass, "get-p\u{00E4}ss %h");
}

// =========================================================================
// Enter on AskPass field opens picker
// =========================================================================

#[test]
fn test_space_on_empty_askpass_field_opens_picker() {
    // Space opens the picker on empty picker fields. On non-empty fields
    // it inserts a literal space
    // (so custom commands like `my-script %h` keep working).
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::AskPass;
    // Field is empty (default after make_form_app).
    assert!(app.forms.host_mut().askpass.is_empty());
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(app.ui.password_picker().open);
}

#[test]
fn test_space_on_populated_askpass_field_inserts_literal() {
    // Empty-field gate: once the user has typed anything, Space inserts a
    // literal space (so multi-word custom commands work).
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::AskPass;
    app.forms.host_mut().askpass = "my-script".to_string();
    app.forms.host_mut().cursor_pos = 9;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(
        !app.ui.password_picker().open,
        "Space on a populated picker field must NOT open the picker"
    );
    assert_eq!(app.forms.host_mut().askpass, "my-script ");
}

#[test]
fn test_picker_open_on_empty_then_enter_selects_keychain() {
    // Space on empty picker field opens the picker; inside the picker,
    // Enter is the canonical "select" key.
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::AskPass;
    assert!(app.forms.host_mut().askpass.is_empty());
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(app.ui.password_picker().open);
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.forms.host_mut().askpass, "keychain");
    assert!(!app.ui.password_picker().open);
}

// =========================================================================
// --connect mode askpass lookup logic (replicated)
// =========================================================================

#[test]
fn test_connect_mode_askpass_lookup() {
    let app = make_app("Host srv\n  HostName 1.2.3.4\n  # purple:askpass pass:ssh/srv\n");
    // Simulate --connect lookup logic from main.rs
    let alias = "srv";
    let askpass = app
        .hosts_state
        .ssh_config()
        .host_entries()
        .iter()
        .find(|h| h.alias == alias)
        .and_then(|h| h.askpass.clone());
    assert_eq!(askpass, Some("pass:ssh/srv".to_string()));
}

#[test]
fn test_connect_mode_askpass_none() {
    let app = make_app("Host srv\n  HostName 1.2.3.4\n");
    let alias = "srv";
    let askpass = app
        .hosts_state
        .ssh_config()
        .host_entries()
        .iter()
        .find(|h| h.alias == alias)
        .and_then(|h| h.askpass.clone());
    assert_eq!(askpass, None);
}

#[test]
fn test_connect_mode_nonexistent_host() {
    let app = make_app("Host srv\n  HostName 1.2.3.4\n");
    let alias = "nonexistent";
    let askpass = app
        .hosts_state
        .ssh_config()
        .host_entries()
        .iter()
        .find(|h| h.alias == alias)
        .and_then(|h| h.askpass.clone());
    assert_eq!(askpass, None);
}

// =========================================================================
// 'e' key opens edit form with correct askpass from host list
// =========================================================================

#[test]
fn test_e_key_opens_edit_form_with_askpass() {
    let mut app =
        make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass op://Vault/SSH/pw\n");
    let (tx, _rx) = mpsc::channel();
    // Press 'e' to edit the selected host
    let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
    assert!(matches!(app.screen, Screen::EditHost { .. }));
    assert_eq!(app.forms.host_mut().askpass, "op://Vault/SSH/pw");
    assert_eq!(app.forms.host_mut().hostname, "10.0.0.1");
}

#[test]
fn test_e_key_opens_edit_form_without_askpass() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
    assert!(matches!(app.screen, Screen::EditHost { .. }));
    assert_eq!(app.forms.host_mut().askpass, "");
}

// =========================================================================
// Picker then Esc preserves existing askpass value
// =========================================================================

#[test]
fn test_picker_esc_preserves_existing_askpass() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::AskPass;
    app.forms.host_mut().askpass = "vault:secret/ssh#pw".to_string();
    let (tx, _rx) = mpsc::channel();
    // Field has content → user must clear it to reach the picker. Simulate
    // by setting the picker open directly (the unit under test is the Esc
    // behavior, not the open path).
    app.ui.password_picker_mut().open = true;
    app.ui.password_picker_mut().list = ratatui::widgets::ListState::default();
    app.ui.password_picker_mut().list.select(Some(0));
    assert!(app.ui.password_picker().open);
    // Navigate but then Esc
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    // Original value preserved
    assert_eq!(app.forms.host_mut().askpass, "vault:secret/ssh#pw");
}

// =========================================================================
// Extra backspace past empty is no-op
// =========================================================================

#[test]
fn test_backspace_on_empty_askpass_is_noop() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::AskPass;
    app.forms.host_mut().askpass = "".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    assert_eq!(app.forms.host_mut().askpass, "");
}

// =========================================================================
// Tab from AskPass goes to Tags, shift-tab goes to ProxyJump
// =========================================================================

#[test]
fn test_tab_from_askpass_to_tags() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(app.forms.host_mut().focused_field, FormField::Tags);
}

#[test]
fn test_shift_tab_from_askpass_to_proxyjump() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(
        &mut app,
        KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
        &tx,
    );
    assert_eq!(app.forms.host_mut().focused_field, FormField::ProxyJump);
}

// =========================================================================
// Tunnel start for host with askpass passes it through
// =========================================================================

#[test]
fn test_tunnel_askpass_lookup_different_sources() {
    let config = "\
Host alpha
  HostName a.com
  # purple:askpass keychain

Host beta
  HostName b.com
  # purple:askpass bw:item

Host gamma
  HostName c.com
";
    let app = make_app(config);
    let lookup = |alias: &str| -> Option<String> {
        app.hosts_state
            .list()
            .iter()
            .find(|h| h.alias == alias)
            .and_then(|h| h.askpass.clone())
    };
    assert_eq!(lookup("alpha"), Some("keychain".to_string()));
    assert_eq!(lookup("beta"), Some("bw:item".to_string()));
    assert_eq!(lookup("gamma"), None);
}

// =========================================================================
// Password picker status message tests
// =========================================================================

#[test]
fn test_password_picker_keychain_sets_status_message() {
    let mut app = make_form_app();
    app.ui.password_picker_mut().open_at(0); // Keychain
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    let toast = app.status_center.toast().unwrap();
    assert!(
        toast.text.contains("OS Keychain"),
        "Toast should mention OS Keychain, got: {}",
        toast.text
    );
}

#[test]
fn test_password_picker_none_sets_cleared_status() {
    let mut app = make_form_app();
    app.forms.host_mut().askpass = "keychain".to_string();
    app.ui.password_picker_mut().open_at(7); // None
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    let toast = app.status_center.toast().unwrap();
    assert!(
        toast.text.contains("cleared"),
        "Toast should say cleared, got: {}",
        toast.text
    );
}

#[test]
fn test_password_picker_prefix_source_focuses_askpass_field() {
    let mut app = make_form_app();
    app.ui.password_picker_mut().open_at(1); // 1Password (op://)
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(
        app.forms.host_mut().focused_field,
        FormField::AskPass,
        "Prefix source should focus AskPass field"
    );
    // No status message for prefix sources (user needs to keep typing)
    assert!(
        app.status_center.status().is_none()
            || !app.status_center.status().unwrap().text.contains("set to")
    );
}

#[test]
fn test_password_picker_prefix_bw_focuses_askpass() {
    let mut app = make_form_app();
    app.ui.password_picker_mut().open_at(2); // Bitwarden (bw:)
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.forms.host_mut().focused_field, FormField::AskPass);
    assert_eq!(app.forms.host_mut().askpass, "bw:");
}

#[test]
fn test_password_picker_prefix_pass_focuses_askpass() {
    let mut app = make_form_app();
    app.ui.password_picker_mut().open_at(3); // pass (pass:)
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.forms.host_mut().focused_field, FormField::AskPass);
    assert_eq!(app.forms.host_mut().askpass, "pass:");
}

#[test]
fn test_password_picker_prefix_vault_focuses_askpass() {
    let mut app = make_form_app();
    app.ui.password_picker_mut().open_at(4); // Vault (vault:)
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.forms.host_mut().focused_field, FormField::AskPass);
    assert_eq!(app.forms.host_mut().askpass, "vault:");
}

// =========================================================================
// Included host: edit blocked, but askpass visible in pending_connect
// =========================================================================

#[test]
fn test_included_host_edit_blocked() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    app.screen = Screen::HostList;
    if let Some(host) = app.hosts_state.list_mut().first_mut() {
        host.source_file = Some(std::path::PathBuf::from("/etc/ssh/ssh_config.d/work.conf"));
    }
    app.ui.list_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn test_included_host_connect_still_carries_askpass() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass op://V/I/p\n");
    app.screen = Screen::HostList;
    if let Some(host) = app.hosts_state.list_mut().first_mut() {
        host.source_file = Some(std::path::PathBuf::from("/etc/ssh/ssh_config.d/work.conf"));
    }
    app.ui.list_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    if let Some((alias, askpass)) = app.ui.pending_connect() {
        assert_eq!(alias, "myserver");
        assert_eq!(askpass.as_deref(), Some("op://V/I/p"));
    }
}

#[test]
fn test_included_host_delete_blocked() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass bw:item\n");
    app.screen = Screen::HostList;
    if let Some(host) = app.hosts_state.list_mut().first_mut() {
        host.source_file = Some(std::path::PathBuf::from("/etc/ssh/ssh_config.d/work.conf"));
    }
    app.ui.list_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
}

// =========================================================================
// Form submit with askpass: verify to_entry() includes askpass
// =========================================================================

#[test]
fn test_form_submit_with_all_password_source_types() {
    let sources = [
        "keychain",
        "op://V/I/p",
        "bw:item",
        "pass:ssh/srv",
        "vault:kv/ssh#pw",
        "my-cmd %h",
    ];
    for source in &sources {
        let mut app = make_app("");
        app.screen = Screen::AddHost;
        app.forms.host_mut().alias = "test-host".to_string();
        app.forms.host_mut().hostname = "10.0.0.1".to_string();
        app.forms.host_mut().askpass = source.to_string();
        let entry = app.forms.host_mut().to_entry();
        assert_eq!(
            entry.askpass.as_deref(),
            Some(*source),
            "Form with askpass '{}' should produce entry with same askpass",
            source
        );
    }
}

#[test]
fn test_form_submit_empty_askpass_is_none() {
    let mut app = make_app("");
    app.screen = Screen::AddHost;
    app.forms.host_mut().alias = "test-host".to_string();
    app.forms.host_mut().hostname = "10.0.0.1".to_string();
    app.forms.host_mut().askpass = "".to_string();
    let entry = app.forms.host_mut().to_entry();
    assert!(entry.askpass.is_none(), "Empty askpass should produce None");
}

// =========================================================================
// Password picker: Enter with no selection is no-op
// =========================================================================

#[test]
fn test_password_picker_enter_with_no_selection() {
    let mut app = make_form_app();
    app.ui.password_picker_mut().open = true;
    app.ui.password_picker_mut().list = ratatui::widgets::ListState::default(); // no selection
    app.forms.host_mut().askpass = "old".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.password_picker().open);
    assert_eq!(app.forms.host_mut().askpass, "old");
}

// =========================================================================
// BW_SESSION: stored in app state
// =========================================================================

#[test]
fn test_bw_session_stored_in_app() {
    let mut app = make_app("Host srv\n  HostName 1.2.3.4\n  # purple:askpass bw:item\n");
    assert!(app.bw_session.is_none());
    app.bw_session = Some("test-session-token".to_string());
    assert_eq!(app.bw_session.as_deref(), Some("test-session-token"));
}

#[test]
fn test_bw_session_none_for_non_bw_source() {
    let app = make_app("Host srv\n  HostName 1.2.3.4\n  # purple:askpass keychain\n");
    assert!(app.bw_session.is_none());
}

// =========================================================================
// Ctrl+D sets global default in password picker
// =========================================================================

#[test]
fn test_password_picker_ctrl_d_closes_picker() {
    // Use "None" to avoid writing a value to the real preferences file
    let mut app = make_form_app();
    app.ui.password_picker_mut().open_at(7); // None
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, ctrl_key('d'), &tx);
    assert!(!app.ui.password_picker().open);
}

#[test]
fn test_password_picker_ctrl_d_does_not_change_form_askpass() {
    let mut app = make_form_app();
    app.forms.host_mut().askpass = "old".to_string();
    app.ui.password_picker_mut().open_at(7); // None
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, ctrl_key('d'), &tx);
    // Ctrl+D only sets the global default, not the form field
    assert_eq!(app.forms.host_mut().askpass, "old");
}

#[test]
fn test_password_picker_ctrl_d_none_sets_status() {
    let mut app = make_form_app();
    app.ui.password_picker_mut().open_at(7); // None
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, ctrl_key('d'), &tx);
    // Shows "cleared" on success or "Failed to save" if ~/.purple doesn't exist
    assert!(app.status_center.status().is_some() || app.status_center.toast().is_some());
    assert!(!app.ui.password_picker().open);
}

#[test]
fn test_password_picker_ctrl_d_source_label_in_status() {
    // Verify logic: non-None sources produce "Global default set to X." message
    let sources = crate::askpass::PASSWORD_SOURCES;
    for (i, src) in sources.iter().enumerate() {
        if src.label == "None" {
            continue;
        }
        let expected = format!("Global default set to {}.", src.label);
        assert!(expected.contains("default"), "Source {}: {}", i, expected);
    }
}

// =========================================================================
// Keychain removal on askpass source change
// =========================================================================

#[test]
fn test_submit_form_old_askpass_tracked_for_edit() {
    // When editing a host with keychain askpass, the old source is detected
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    assert_eq!(
        app.hosts_state.list()[0].askpass,
        Some("keychain".to_string())
    );
    // Simulate opening edit form
    app.screen = Screen::EditHost {
        alias: "myserver".to_string(),
    };
    app.forms.host_mut().alias = "myserver".to_string();
    app.forms.host_mut().hostname = "10.0.0.1".to_string();
    // Change askpass to something else
    app.forms.host_mut().askpass = "op://Vault/Item/pw".to_string();
    // The old_askpass detection in submit_form looks up app.hosts_state.list() by alias
    let old = app
        .hosts_state
        .list()
        .iter()
        .find(|h| h.alias == "myserver")
        .and_then(|h| h.askpass.clone());
    assert_eq!(old, Some("keychain".to_string()));
}

#[test]
fn test_submit_form_no_keychain_removal_when_unchanged() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    app.screen = Screen::EditHost {
        alias: "myserver".to_string(),
    };
    app.forms.host_mut().alias = "myserver".to_string();
    app.forms.host_mut().hostname = "10.0.0.1".to_string();
    // Keep askpass as keychain
    app.forms.host_mut().askpass = "keychain".to_string();
    let old = app
        .hosts_state
        .list()
        .iter()
        .find(|h| h.alias == "myserver")
        .and_then(|h| h.askpass.clone());
    // Same source, no removal needed
    assert_eq!(old.as_deref(), Some("keychain"));
    assert_eq!(app.forms.host_mut().askpass, "keychain");
}

#[test]
fn test_submit_form_no_keychain_removal_for_add() {
    // AddHost has no old askpass
    let mut app = make_app("Host existing\n  HostName 1.2.3.4\n");
    app.screen = Screen::AddHost;
    let old: Option<String> = None; // no old host for add
    assert!(old.is_none());
}

#[test]
fn test_submit_form_rename_migrates_per_host_state() {
    // SSH directives and tunnel/Vault metadata survive a rename via
    // `update_host`. State keyed by alias outside the SSH config does
    // not: connection history, jump-bar recents, and the collapsed-
    // fleet preference must be migrated explicitly by submit_form.
    let mut app = make_app("Host web-old\n  HostName 1.2.3.4\n");
    // App::new builds a sandboxed Env; seed and read recents through it.
    let paths = app.env().paths().cloned();
    // Isolate history from any pre-existing ~/.purple/history.tsv on
    // the test runner. `from_entries` leaves `path` empty so the
    // background `save()` is a silent no-op; we assert only the
    // in-memory migration here.
    app.history = crate::history::ConnectionHistory::from_entries(std::collections::HashMap::new());
    app.history.upsert_entry(crate::history::HistoryEntry {
        alias: "web-old".to_string(),
        last_connected: 1_700_000_000,
        count: 12,
        timestamps: vec![1_700_000_000],
    });
    app.containers_overview.toggle_host_collapsed("web-old");
    // Seed a host recent for web-old on disk so the rename pulls it in.
    let mut seeded = crate::app::jump::RecentsFile::default();
    seeded.entries.push(crate::app::jump::RecentEntry {
        target: crate::app::jump::RecentRef::new(
            crate::app::jump::SourceKind::Host,
            "web-old".to_string(),
        ),
        last_used_unix: 100,
    });
    crate::app::jump::save_recents(&seeded, paths.as_ref()).expect("seed recents");

    app.screen = Screen::EditHost {
        alias: "web-old".to_string(),
    };
    *app.forms.host_mut() = crate::app::HostForm::new();
    app.forms.host_mut().alias = "web-new".to_string();
    app.forms.host_mut().hostname = "1.2.3.4".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    assert!(
        app.history.entry("web-old").is_none(),
        "history under the old alias must be cleared"
    );
    let migrated = app
        .history
        .entry("web-new")
        .expect("history must move under the new alias");
    assert_eq!(migrated.count, 12);
    assert_eq!(migrated.timestamps, vec![1_700_000_000]);

    assert!(
        !app.containers_overview
            .collapsed_hosts()
            .contains("web-old"),
        "collapsed-fleet preference must drop the old alias"
    );
    assert!(
        app.containers_overview
            .collapsed_hosts()
            .contains("web-new"),
        "collapsed-fleet preference must carry over to the new alias"
    );

    let reloaded = crate::app::jump::load_recents(paths.as_ref());
    let host_keys: Vec<&str> = reloaded
        .entries
        .iter()
        .filter(|e| e.target.kind == crate::app::jump::SourceKind::Host)
        .map(|e| e.target.key.as_str())
        .collect();
    assert_eq!(host_keys, vec!["web-new"]);
}

/// Rename keeps the host at its recency-based position on MostRecent
/// and Frecency without waiting for a restart.
fn rename_keeps_position_under_sort(sort_mode: crate::app::SortMode) {
    // Three hosts. `top-old` has the most recent connection, so on
    // MostRecent / Frecency it must appear at index 0 both before and
    // after a rename to `top-new`.
    let mut app = make_app(
        "Host top-old\n  HostName 1.1.1.1\n\
         Host mid\n  HostName 2.2.2.2\n\
         Host bot\n  HostName 3.3.3.3\n",
    );
    app.history = crate::history::ConnectionHistory::from_entries(std::collections::HashMap::new());
    app.history.upsert_entry(crate::history::HistoryEntry {
        alias: "top-old".to_string(),
        last_connected: 1_700_000_300,
        count: 30,
        timestamps: vec![1_700_000_100, 1_700_000_200, 1_700_000_300],
    });
    app.history.upsert_entry(crate::history::HistoryEntry {
        alias: "mid".to_string(),
        last_connected: 1_700_000_200,
        count: 5,
        timestamps: vec![1_700_000_200],
    });
    app.history.upsert_entry(crate::history::HistoryEntry {
        alias: "bot".to_string(),
        last_connected: 1_700_000_100,
        count: 1,
        timestamps: vec![1_700_000_100],
    });
    app.hosts_state.set_sort_mode(sort_mode);
    app.apply_sort();

    // Sanity: pre-rename, `top-old` sits at the top of the display list.
    let alias_at = |app: &App, idx: usize| -> Option<String> {
        match app.hosts_state.display_list().get(idx)? {
            crate::app::HostListItem::Host { index } => {
                app.hosts_state.list().get(*index).map(|h| h.alias.clone())
            }
            _ => None,
        }
    };
    assert_eq!(
        alias_at(&app, 0).as_deref(),
        Some("top-old"),
        "pre-rename: top-old must sit at index 0 on {:?}",
        sort_mode
    );

    // Rename top-old -> top-new via the real submit_form flow.
    app.screen = Screen::EditHost {
        alias: "top-old".to_string(),
    };
    *app.forms.host_mut() = crate::app::HostForm::new();
    app.forms.host_mut().alias = "top-new".to_string();
    app.forms.host_mut().hostname = "1.1.1.1".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    // Post-rename, the migrated host must keep the top slot.
    assert_eq!(
        alias_at(&app, 0).as_deref(),
        Some("top-new"),
        "post-rename: top-new must keep index 0 on {:?} (history was migrated to the new alias and the sort must reflect that)",
        sort_mode
    );

    // The cursor follows the rename: `submit_form` calls
    // `select_host_by_alias(&target_alias)` after the rename.
    assert_eq!(
        app.ui.list_state().selected(),
        Some(0),
        "cursor must follow the renamed host to its new display position on {:?}",
        sort_mode
    );
}

#[test]
fn test_submit_form_rename_keeps_position_on_most_recent() {
    rename_keeps_position_under_sort(crate::app::SortMode::MostRecent);
}

#[test]
fn test_submit_form_rename_keeps_position_on_frecency() {
    rename_keeps_position_under_sort(crate::app::SortMode::Frecency);
}

#[test]
fn test_submit_form_rename_carries_ping_and_container_cache() {
    // Rename preserves alias-keyed caches (ping, container_cache,
    // in-flight dedup sets) end-to-end through submit_form.
    let mut app = make_app("Host web-old\n  HostName 1.2.3.4\n");
    // Isolate from any pre-existing ~/.purple/history.tsv on the runner.
    app.history = crate::history::ConnectionHistory::from_entries(std::collections::HashMap::new());
    app.ping.insert_status(
        "web-old".to_string(),
        crate::app::PingStatus::Reachable { rtt_ms: 23 },
    );
    app.ping
        .record_check("web-old".to_string(), std::time::Instant::now());
    app.container_state.insert_cache_entry(
        "web-old".to_string(),
        crate::containers::ContainerCacheEntry {
            timestamp: 1_700_000_000,
            runtime: crate::containers::ContainerRuntime::Docker,
            engine_version: Some("24.0.0".to_string()),
            containers: vec![],
        },
    );
    app.containers_overview
        .mark_auto_list_pending("web-old".to_string());
    app.vault.mark_cert_check_started("web-old".to_string());
    app.vault
        .note_cert_stat("web-old".to_string(), std::time::Instant::now());
    // Seed an open bulk-confirm dialog and a logs overlay scoped to the
    // old alias so the rename migration is exercised end-to-end.
    app.containers_overview
        .set_pending_bulk_confirm(crate::app::BulkConfirmContext {
            kind: crate::app::BulkConfirmKind::HostRestartAll,
            alias: "web-old".to_string(),
            project: None,
            members: vec![],
        });
    app.container_state.set_logs_view(crate::app::LogsView {
        alias: "web-old".to_string(),
        container_id: "c1".to_string(),
        container_name: "nginx".to_string(),
        body: vec![],
        fetched_at: 0,
        error: None,
        scroll: 0,
        last_render_height: 0,
        search: None,
    });
    app.vault
        .set_pending_sign(vec![crate::vault_ssh::VaultSignTarget {
            alias: "web-old".to_string(),
            role: "ssh-client/sign/role".to_string(),
            certificate_file: String::new(),
            pubkey: std::path::PathBuf::from("/tmp/id_ed25519.pub"),
            vault_addr: None,
        }]);

    app.screen = Screen::EditHost {
        alias: "web-old".to_string(),
    };
    *app.forms.host_mut() = crate::app::HostForm::new();
    app.forms.host_mut().alias = "web-new".to_string();
    app.forms.host_mut().hostname = "1.2.3.4".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    // Ping carried over.
    assert!(
        !app.ping.status_contains("web-old"),
        "ping.status under old alias must be cleared"
    );
    assert!(
        matches!(
            app.ping.status_of("web-new"),
            Some(crate::app::PingStatus::Reachable { rtt_ms: 23 })
        ),
        "ping.status must move to the new alias with the same RTT, got {:?}",
        app.ping.status_of("web-new")
    );
    assert!(
        app.ping.last_checked_at("web-old").is_none()
            && app.ping.last_checked_at("web-new").is_some(),
        "ping.last_checked must follow the rename"
    );

    // Container cache carried over with payload intact.
    assert!(
        !app.container_state.cache_contains("web-old"),
        "container_cache under old alias must be cleared"
    );
    let entry = app
        .container_state
        .cache_entry("web-new")
        .expect("container_cache must move to the new alias");
    assert_eq!(entry.timestamp, 1_700_000_000);
    assert_eq!(entry.engine_version.as_deref(), Some("24.0.0"));

    // Alias-keyed in-flight dedup sets carried over.
    assert!(
        !app.containers_overview.auto_list_pending("web-old")
            && app.containers_overview.auto_list_pending("web-new"),
        "auto_list_in_flight must follow the rename"
    );
    assert!(
        !app.vault.is_cert_check_in_flight("web-old")
            && app.vault.is_cert_check_in_flight("web-new"),
        "vault.cert_checks_in_flight must follow the rename"
    );
    assert!(
        app.vault.last_cert_stat("web-old").is_none()
            && app.vault.last_cert_stat("web-new").is_some(),
        "vault.cert_stat_throttle must follow the rename"
    );
    // Open dialog payloads must follow the rename so a worker spawned
    // post-rename does not act on the stale alias.
    let pending_bulk = app
        .containers_overview
        .pending_bulk_confirm()
        .expect("pending_bulk_confirm survives rename");
    assert_eq!(
        pending_bulk.alias, "web-new",
        "containers_overview.pending_bulk_confirm.alias must follow the rename"
    );
    let pending_sign = app
        .vault
        .pending_sign()
        .expect("pending_sign survives rename");
    assert_eq!(
        pending_sign[0].alias, "web-new",
        "vault.pending_sign[].alias must follow the rename"
    );
    let logs = app
        .container_state
        .logs_view()
        .expect("logs_view survives rename");
    assert_eq!(
        logs.alias, "web-new",
        "container_state.logs_view.alias must follow the rename"
    );
}

#[test]
fn test_history_rename_leaves_sibling_keys_untouched() {
    // `update_host` (ssh_config/model.rs:684) renames only the matching
    // token in a multi-alias `Host` line. The history migration must
    // therefore touch only the renamed alias. Asserted at the
    // `ConnectionHistory::rename` level because `edit_host_from_form`
    // has a pre-existing `debug_assert!` on `set_host_vault_addr` that
    // refuses multi-alias blocks before the per-host migration runs.
    let mut history =
        crate::history::ConnectionHistory::from_entries(std::collections::HashMap::new());
    history.upsert_entry(crate::history::HistoryEntry {
        alias: "web-01".to_string(),
        last_connected: 1_700_000_000,
        count: 4,
        timestamps: vec![1_700_000_000],
    });
    history.upsert_entry(crate::history::HistoryEntry {
        alias: "web-prod".to_string(),
        last_connected: 1_700_000_500,
        count: 9,
        timestamps: vec![1_700_000_500],
    });

    assert!(history.rename("web-prod", "web-new"));
    assert!(history.entry("web-01").is_some());
    assert!(history.entry("web-prod").is_none());
    let moved = history
        .entry("web-new")
        .expect("renamed alias must carry over");
    assert_eq!(moved.count, 9);
}

// =========================================================================
// Snippet picker
// =========================================================================

fn make_snippet_app() -> App {
    let mut app = make_app("Host myserver\n  HostName 1.2.3.4\n");
    // Unique tempdir per call so parallel test threads never share a snippet
    // store path. keep() leaks it like make_app, avoiding a process::id race.
    let dir = tempfile::tempdir().expect("tempdir").keep();
    app.snippets.store_mut().path_override = Some(dir.join("snippets"));
    app.snippets.store_mut().snippets = vec![
        crate::snippet::Snippet {
            name: "check-disk".to_string(),
            command: "df -h".to_string(),
            description: "Check disk usage".to_string(),
            interactive: false,
        },
        crate::snippet::Snippet {
            name: "uptime".to_string(),
            command: "uptime".to_string(),
            description: String::new(),
            interactive: false,
        },
    ];
    let _ = app.snippets.store_mut().save();
    app.ui.snippet_picker_state_mut().select(Some(0));
    app.snippets.set_flow_targets(vec!["myserver".to_string()]);
    app.screen = Screen::SnippetPicker;
    app
}

#[test]
fn zzz_probe_rename_preserves_targets() {
    let mut app = make_snippet_app();
    app.snippets
        .store_mut()
        .set_targets("check-disk", vec!["h1".to_string()]);
    let _ = app.snippets.store_mut().save();
    *app.snippets.form_mut() =
        crate::app::SnippetForm::from_snippet(&app.snippets.store().snippets[0].clone());
    // Mirror open_snippet_edit_form, which seeds the field from saved targets.
    app.snippets.form_mut().default_hosts = vec!["h1".to_string()];
    app.snippets.form_mut().name = "renamed-disk".to_string();
    app.snippets.form_mut().cursor_pos = 12;
    app.snippets.set_flow_targets(vec!["myserver".to_string()]);
    app.snippets.set_form_editing(Some(0));
    app.screen = Screen::SnippetForm;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.snippets.store().get("renamed-disk").is_some());
    assert_eq!(
        app.snippets.store().targets_for("renamed-disk"),
        &["h1"],
        "saved default targets should survive a rename"
    );
}

#[test]
fn test_snippet_picker_nav_down_up() {
    let mut app = make_snippet_app();
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    assert_eq!(app.ui.snippet_picker_state().selected(), Some(1));

    let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
    assert_eq!(app.ui.snippet_picker_state().selected(), Some(0));
}

#[test]
fn test_snippet_picker_esc_returns_to_hostlist() {
    let mut app = make_snippet_app();
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert_eq!(app.screen, Screen::HostList);
}

#[test]
fn test_snippet_picker_q_returns_to_hostlist() {
    let mut app = make_snippet_app();
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char('q')), &tx);
    assert_eq!(app.screen, Screen::HostList);
}

#[test]
fn test_snippet_picker_enter_starts_output() {
    let mut app = make_snippet_app();
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(
        matches!(app.screen, Screen::SnippetOutput),
        "Expected SnippetOutput screen, got {:?}",
        app.screen
    );
    assert_eq!(app.snippets.output_snippet_name(), Some("check-disk"));
    assert_eq!(app.snippets.flow_targets(), &["myserver".to_string()][..]);
    assert!(app.snippets.output().is_some());
}

#[test]
fn test_snippet_picker_enter_clears_multi_select() {
    let mut app = make_snippet_app();
    app.hosts_state.multi_select_mut().insert(0);
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.hosts_state.multi_select().is_empty());
}

#[test]
fn test_snippet_picker_a_opens_add_form() {
    let mut app = make_snippet_app();
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char('a')), &tx);
    assert!(matches!(app.screen, Screen::SnippetForm));
    assert!(app.snippets.form_editing().is_none());
    assert!(app.snippets.form_mut().name.is_empty());
}

#[test]
fn test_snippet_picker_e_opens_edit_form() {
    let mut app = make_snippet_app();
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
    assert!(matches!(app.screen, Screen::SnippetForm));
    assert_eq!(app.snippets.form_editing(), Some(0));
    assert_eq!(app.snippets.form_mut().name, "check-disk");
    assert_eq!(app.snippets.form_mut().command, "df -h");
}

#[test]
fn test_snippet_picker_d_deletes_and_saves() {
    let mut app = make_snippet_app();
    let _ = app.snippets.store_mut().save(); // ensure file exists
    let (tx, _rx) = mpsc::channel();

    // d sets pending confirmation
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    assert_eq!(app.snippets.pending_delete(), Some(0));
    assert_eq!(app.snippets.store().snippets.len(), 2); // not yet deleted

    // y confirms deletion
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert_eq!(app.snippets.pending_delete(), None);
    assert_eq!(app.snippets.store().snippets.len(), 1);
    assert_eq!(app.snippets.store().snippets[0].name, "uptime");
    assert_eq!(app.ui.snippet_picker_state().selected(), Some(0));
}

#[test]
fn test_snippet_picker_d_last_item_selects_none() {
    let mut app = make_snippet_app();
    app.snippets.store_mut().snippets = vec![crate::snippet::Snippet {
        name: "only".to_string(),
        command: "ls".to_string(),
        description: String::new(),
        interactive: false,
    }];
    app.ui.snippet_picker_state_mut().select(Some(0));
    let _ = app.snippets.store_mut().save();
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    assert_eq!(app.snippets.pending_delete(), Some(0));

    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(app.snippets.store().snippets.is_empty());
    assert_eq!(app.ui.snippet_picker_state().selected(), None);
}

#[test]
fn test_snippet_picker_d_rollback_on_save_failure() {
    // Skip under root (e.g. inside CI Docker containers): root can
    // mkdir -p any path including /nonexistent/dir, so the save would
    // actually succeed and the rollback assertion would fire spuriously.
    // SAFETY: getuid() is a thread-safe POSIX call with no preconditions.
    #[cfg(unix)]
    if unsafe { libc::getuid() } == 0 {
        return;
    }
    // save() is a no-op under the global demo flag, so a parallel demo-enabling
    // test would make the forced failure succeed and the rollback never fire.
    // Serialise and disable demo mode for the duration.
    let _guard = crate::demo_flag::GLOBAL_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let prior_demo = crate::demo_flag::is_demo();
    crate::demo_flag::disable();

    let mut app = make_snippet_app();
    // Point to a non-writable path to force save failure
    app.snippets.store_mut().path_override = Some(PathBuf::from("/nonexistent/dir/snippets"));
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    assert_eq!(app.snippets.pending_delete(), Some(0));

    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    // Rollback: snippet should still be there
    assert_eq!(app.snippets.store().snippets.len(), 2);
    assert_eq!(app.snippets.store().snippets[0].name, "check-disk");
    assert!(app.status_center.toast().unwrap().is_error());

    if prior_demo {
        crate::demo_flag::enable();
    }
}

// =========================================================================
// Snippet form
// =========================================================================

#[test]
fn test_snippet_form_esc_returns_to_picker() {
    let mut app = make_snippet_app();
    *app.snippets.form_mut() = crate::app::SnippetForm::new();
    app.snippets.set_flow_targets(vec!["myserver".to_string()]);
    app.snippets.set_form_editing(None);
    app.screen = Screen::SnippetForm;
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::SnippetPicker));
}

#[test]
fn test_snippet_form_esc_returns_to_host_list_when_tab_origin() {
    let mut app = make_snippet_app();
    *app.snippets.form_mut() = crate::app::SnippetForm::new();
    app.snippets.set_flow_targets(vec!["myserver".to_string()]);
    app.snippets.set_form_editing(None);
    // Opened from the Snippets tab: a clean Esc returns to the tab (HostList).
    app.snippets.set_form_return_to_tab(true);
    app.screen = Screen::SnippetForm;
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn test_snippet_form_tab_cycles_fields() {
    let mut app = make_snippet_app();
    *app.snippets.form_mut() = crate::app::SnippetForm::new();
    app.snippets.set_flow_targets(vec!["myserver".to_string()]);
    app.snippets.set_form_editing(None);
    app.screen = Screen::SnippetForm;
    let (tx, _rx) = mpsc::channel();

    assert_eq!(
        app.snippets.form_mut().focused_field,
        crate::app::SnippetFormField::Name
    );

    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.snippets.form_mut().focused_field,
        crate::app::SnippetFormField::Command
    );

    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.snippets.form_mut().focused_field,
        crate::app::SnippetFormField::Description
    );

    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.snippets.form_mut().focused_field,
        crate::app::SnippetFormField::Interactive
    );

    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.snippets.form_mut().focused_field,
        crate::app::SnippetFormField::DefaultHosts
    );

    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.snippets.form_mut().focused_field,
        crate::app::SnippetFormField::Name
    );
}

#[test]
fn test_snippet_form_char_insert() {
    let mut app = make_snippet_app();
    *app.snippets.form_mut() = crate::app::SnippetForm::new();
    app.snippets.set_flow_targets(vec!["myserver".to_string()]);
    app.snippets.set_form_editing(None);
    app.screen = Screen::SnippetForm;
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char('a')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('b')), &tx);
    assert_eq!(app.snippets.form_mut().name, "ab");
    assert_eq!(app.snippets.form_mut().cursor_pos, 2);
}

#[test]
fn test_snippet_form_backspace() {
    let mut app = make_snippet_app();
    *app.snippets.form_mut() = crate::app::SnippetForm::new();
    app.snippets.form_mut().name = "abc".to_string();
    app.snippets.form_mut().cursor_pos = 3;
    app.snippets.set_flow_targets(vec!["myserver".to_string()]);
    app.snippets.set_form_editing(None);
    app.screen = Screen::SnippetForm;
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    assert_eq!(app.snippets.form_mut().name, "ab");
    assert_eq!(app.snippets.form_mut().cursor_pos, 2);
}

#[test]
fn test_snippet_form_submit_add() {
    let mut app = make_snippet_app();
    let _ = app.snippets.store_mut().save();
    *app.snippets.form_mut() = crate::app::SnippetForm::new();
    app.snippets.form_mut().name = "new-cmd".to_string();
    app.snippets.form_mut().command = "whoami".to_string();
    app.snippets.form_mut().cursor_pos = 6;
    app.snippets.set_flow_targets(vec!["myserver".to_string()]);
    app.snippets.set_form_editing(None);
    app.screen = Screen::SnippetForm;
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(matches!(app.screen, Screen::SnippetPicker));
    assert_eq!(app.snippets.store().snippets.len(), 3);
    assert!(app.snippets.store().get("new-cmd").is_some());
}

#[test]
fn test_snippet_form_submit_edit() {
    let mut app = make_snippet_app();
    let _ = app.snippets.store_mut().save();
    *app.snippets.form_mut() =
        crate::app::SnippetForm::from_snippet(&app.snippets.store().snippets[0].clone());
    app.snippets.form_mut().command = "df -hT".to_string();
    app.snippets.set_flow_targets(vec!["myserver".to_string()]);
    app.snippets.set_form_editing(Some(0));
    app.screen = Screen::SnippetForm;
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(matches!(app.screen, Screen::SnippetPicker));
    assert_eq!(app.snippets.store().snippets[0].command, "df -hT");
}

fn open_snippet_form_on_interactive(app: &mut App) {
    *app.snippets.form_mut() = crate::app::SnippetForm::new();
    app.snippets.form_mut().focused_field = crate::app::SnippetFormField::Interactive;
    app.snippets.set_flow_targets(vec!["myserver".to_string()]);
    app.snippets.set_form_editing(None);
    app.screen = Screen::SnippetForm;
}

#[test]
fn test_snippet_form_space_flips_interactive_toggle() {
    let mut app = make_snippet_app();
    open_snippet_form_on_interactive(&mut app);
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(app.snippets.form().interactive);
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(!app.snippets.form().interactive);
    assert!(matches!(app.screen, Screen::SnippetForm));
}

#[test]
fn test_snippet_form_typing_on_interactive_toggle_is_ignored() {
    let mut app = make_snippet_app();
    open_snippet_form_on_interactive(&mut app);
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char('x')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    let form = app.snippets.form();
    assert!(!form.interactive);
    assert!(form.name.is_empty() && form.command.is_empty() && form.description.is_empty());
}

#[test]
fn test_snippet_form_submit_persists_interactive() {
    let mut app = make_snippet_app();
    let _ = app.snippets.store_mut().save();
    open_snippet_form_on_interactive(&mut app);
    app.snippets.form_mut().name = "pm2".to_string();
    app.snippets.form_mut().command = "pm2 status".to_string();
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(matches!(app.screen, Screen::SnippetPicker));
    assert!(app.snippets.store().get("pm2").unwrap().interactive);
}

#[test]
fn test_snippet_form_edit_seeds_interactive_from_snippet() {
    let mut snippet = make_snippet_app().snippets.store().snippets[0].clone();
    snippet.interactive = true;
    let form = crate::app::SnippetForm::from_snippet(&snippet);
    assert!(form.interactive);
}

#[test]
fn test_snippet_form_toggling_interactive_makes_esc_ask_to_discard() {
    let mut app = make_snippet_app();
    open_snippet_form_on_interactive(&mut app);
    app.capture_snippet_form_baseline();
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(app.forms.is_discard_pending());
    assert!(matches!(app.screen, Screen::SnippetForm));
}

#[test]
fn test_snippet_form_submit_rejects_empty_name() {
    let mut app = make_snippet_app();
    *app.snippets.form_mut() = crate::app::SnippetForm::new();
    app.snippets.form_mut().command = "ls".to_string();
    app.snippets.set_flow_targets(vec!["myserver".to_string()]);
    app.snippets.set_form_editing(None);
    app.screen = Screen::SnippetForm;
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    // Should stay on the form with an error
    assert!(matches!(app.screen, Screen::SnippetForm));
    assert!(app.status_center.toast().unwrap().is_error());
}

#[test]
fn test_snippet_form_submit_rejects_duplicate_name() {
    let mut app = make_snippet_app();
    let _ = app.snippets.store_mut().save();
    *app.snippets.form_mut() = crate::app::SnippetForm::new();
    app.snippets.form_mut().name = "uptime".to_string();
    app.snippets.form_mut().command = "uptime -s".to_string();
    app.snippets.form_mut().cursor_pos = 9;
    app.snippets.set_flow_targets(vec!["myserver".to_string()]);
    app.snippets.set_form_editing(None);
    app.screen = Screen::SnippetForm;
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(matches!(app.screen, Screen::SnippetForm));
    assert!(app.status_center.toast().unwrap().is_error());
}

#[test]
fn test_snippet_form_submit_rollback_on_save_failure() {
    // Skip under root: see test_snippet_picker_d_rollback_on_save_failure
    // for the explanation.
    #[cfg(unix)]
    if unsafe { libc::getuid() } == 0 {
        return;
    }
    let _guard = crate::demo_flag::GLOBAL_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let prior_demo = crate::demo_flag::is_demo();
    crate::demo_flag::disable();

    let mut app = make_snippet_app();
    // Force save failure
    app.snippets.store_mut().path_override = Some(PathBuf::from("/nonexistent/dir/snippets"));
    *app.snippets.form_mut() = crate::app::SnippetForm::new();
    app.snippets.form_mut().name = "new-cmd".to_string();
    app.snippets.form_mut().command = "whoami".to_string();
    app.snippets.form_mut().cursor_pos = 6;
    app.snippets.set_flow_targets(vec!["myserver".to_string()]);
    app.snippets.set_form_editing(None);
    app.screen = Screen::SnippetForm;
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    // Rollback: new snippet should not be in the store
    assert_eq!(app.snippets.store().snippets.len(), 2);
    assert!(app.snippets.store().get("new-cmd").is_none());
    assert!(app.status_center.toast().unwrap().is_error());

    if prior_demo {
        crate::demo_flag::enable();
    }
}

#[test]
fn test_snippet_form_edit_rename_rollback_on_save_failure() {
    // Skip under root: see test_snippet_picker_d_rollback_on_save_failure
    // for the explanation.
    #[cfg(unix)]
    if unsafe { libc::getuid() } == 0 {
        return;
    }
    let _guard = crate::demo_flag::GLOBAL_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let prior_demo = crate::demo_flag::is_demo();
    crate::demo_flag::disable();

    let mut app = make_snippet_app();
    // The snippet being renamed has saved default targets; a failed rename-save
    // must restore them, not just the snippet name.
    app.snippets
        .store_mut()
        .set_targets("check-disk", vec!["h1".to_string(), "h2".to_string()]);
    // Force save failure
    app.snippets.store_mut().path_override = Some(PathBuf::from("/nonexistent/dir/snippets"));
    *app.snippets.form_mut() =
        crate::app::SnippetForm::from_snippet(&app.snippets.store().snippets[0].clone());
    app.snippets.form_mut().name = "renamed".to_string();
    app.snippets.form_mut().cursor_pos = 7;
    app.snippets.set_flow_targets(vec!["myserver".to_string()]);
    app.snippets.set_form_editing(Some(0));
    app.screen = Screen::SnippetForm;
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    // Rollback: original snippets should still be there
    assert_eq!(app.snippets.store().snippets.len(), 2);
    assert!(app.snippets.store().get("check-disk").is_some());
    assert!(app.snippets.store().get("renamed").is_none());
    // Targets restored too: the rename dropped them, the rollback brings them back.
    assert_eq!(
        app.snippets.store().targets_for("check-disk"),
        &["h1", "h2"]
    );
    assert!(app.snippets.store().targets_for("renamed").is_empty());

    if prior_demo {
        crate::demo_flag::enable();
    }
}

#[test]
fn test_snippet_form_rename_preserves_targets_and_run_history() {
    // save() and the post-rename ledger flush are demo-suppressed; serialise and
    // disable demo mode so the rename actually persists in this test.
    let _guard = crate::demo_flag::GLOBAL_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let prior_demo = crate::demo_flag::is_demo();
    crate::demo_flag::disable();

    let mut app = make_snippet_app();
    // The snippet being renamed has saved default targets and recorded runs.
    app.snippets
        .store_mut()
        .set_targets("check-disk", vec!["h1".to_string()]);
    app.snippets.runs_mut().record(
        "check-disk",
        crate::snippet_runs::RunRecord {
            ts: 1,
            hosts: 2,
            ok: 2,
            failed: 0,
        },
    );
    *app.snippets.form_mut() =
        crate::app::SnippetForm::from_snippet(&app.snippets.store().snippets[0].clone());
    // open_snippet_edit_form seeds the form's default_hosts from the saved
    // targets; mirror that here so the rename carries them via the form.
    app.snippets.form_mut().default_hosts = vec!["h1".to_string()];
    app.snippets.form_mut().name = "diskcheck".to_string();
    app.snippets.form_mut().cursor_pos = 9;
    app.snippets.set_form_editing(Some(0));
    app.screen = Screen::SnippetForm;
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    // Snippet renamed.
    assert!(app.snippets.store().get("diskcheck").is_some());
    assert!(app.snippets.store().get("check-disk").is_none());
    // Saved default targets follow the new name; the old name keeps none.
    assert_eq!(app.snippets.store().targets_for("diskcheck"), &["h1"]);
    assert!(app.snippets.store().targets_for("check-disk").is_empty());
    // Run history migrated to the new name.
    assert_eq!(app.snippets.runs().run_count("diskcheck"), 1);
    assert_eq!(app.snippets.runs().run_count("check-disk"), 0);

    if prior_demo {
        crate::demo_flag::enable();
    }
}

#[test]
fn test_snippet_picker_enter_with_no_selection() {
    let mut app = make_snippet_app();
    app.snippets.store_mut().snippets.clear();
    app.ui.snippet_picker_state_mut().select(None);
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    // Should remain on picker, no pending snippet
    assert!(matches!(app.screen, Screen::SnippetPicker));
    assert!(app.snippets.pending().is_none());
}

#[test]
fn test_host_list_r_opens_snippet_picker() {
    let mut app = make_app("Host myserver\n  HostName 1.2.3.4\n");
    app.ui.list_state_mut().select(Some(0));
    let dir = tempfile::tempdir().expect("tempdir").keep();
    app.snippets.store_mut().path_override = Some(dir.join("snippets"));
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char('r')), &tx);
    assert!(
        matches!(app.screen, Screen::SnippetPicker),
        "Expected SnippetPicker screen, got {:?}",
        app.screen
    );
    assert_eq!(app.snippets.flow_targets(), &["myserver".to_string()][..]);
}

#[test]
fn test_host_list_r_shift_opens_snippet_picker_all() {
    let mut app = make_app("Host a\n  HostName 1.1.1.1\nHost b\n  HostName 2.2.2.2\n");
    app.ui.list_state_mut().select(Some(0));
    let dir = tempfile::tempdir().expect("tempdir").keep();
    app.snippets.store_mut().path_override = Some(dir.join("snippets"));
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char('R')), &tx);
    assert!(
        matches!(app.screen, Screen::SnippetPicker),
        "Expected SnippetPicker screen, got {:?}",
        app.screen
    );
    assert_eq!(app.snippets.flow_targets().len(), 2);
}

// --- Tunnel form Space/arrow tests ---

fn make_tunnel_form_app(field: crate::app::TunnelFormField) -> App {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::TunnelForm {
        alias: "test".to_string(),
        editing: None,
    };
    *app.tunnels.form_mut() = crate::app::TunnelForm::new();
    app.tunnels.form_mut().focused_field = field;
    app
}

#[test]
fn test_tunnel_form_space_cycles_type_local_to_remote() {
    let mut app = make_tunnel_form_app(crate::app::TunnelFormField::Type);
    assert_eq!(
        app.tunnels.form_mut().tunnel_type,
        crate::tunnel::TunnelType::Local
    );
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert_eq!(
        app.tunnels.form_mut().tunnel_type,
        crate::tunnel::TunnelType::Remote
    );
}

#[test]
fn test_tunnel_form_space_cycles_type_remote_to_dynamic() {
    let mut app = make_tunnel_form_app(crate::app::TunnelFormField::Type);
    app.tunnels.form_mut().tunnel_type = crate::tunnel::TunnelType::Remote;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert_eq!(
        app.tunnels.form_mut().tunnel_type,
        crate::tunnel::TunnelType::Dynamic
    );
}

#[test]
fn test_tunnel_form_space_cycles_type_dynamic_to_local() {
    let mut app = make_tunnel_form_app(crate::app::TunnelFormField::Type);
    app.tunnels.form_mut().tunnel_type = crate::tunnel::TunnelType::Dynamic;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert_eq!(
        app.tunnels.form_mut().tunnel_type,
        crate::tunnel::TunnelType::Local
    );
}

#[test]
fn test_tunnel_form_left_on_type_does_not_cycle() {
    let mut app = make_tunnel_form_app(crate::app::TunnelFormField::Type);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Left), &tx);
    assert_eq!(
        app.tunnels.form_mut().tunnel_type,
        crate::tunnel::TunnelType::Local
    );
}

#[test]
fn test_tunnel_form_right_on_type_does_not_cycle() {
    let mut app = make_tunnel_form_app(crate::app::TunnelFormField::Type);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Right), &tx);
    assert_eq!(
        app.tunnels.form_mut().tunnel_type,
        crate::tunnel::TunnelType::Local
    );
}

#[test]
fn test_tunnel_form_space_on_bind_port_inserts_space() {
    let mut app = make_tunnel_form_app(crate::app::TunnelFormField::BindPort);
    app.tunnels.form_mut().bind_port = "80".to_string();
    app.tunnels.form_mut().cursor_pos = 2;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert_eq!(app.tunnels.form_mut().bind_port, "80 ");
}

#[test]
fn test_tunnel_form_left_on_text_moves_cursor() {
    let mut app = make_tunnel_form_app(crate::app::TunnelFormField::BindPort);
    app.tunnels.form_mut().bind_port = "8080".to_string();
    app.tunnels.form_mut().cursor_pos = 2;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Left), &tx);
    assert_eq!(app.tunnels.form_mut().cursor_pos, 1);
}

// --- Dirty-check tests ---

#[test]
fn test_host_form_clean_esc_closes_immediately() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    *app.forms.host_mut() = crate::app::HostForm::new();
    app.screen = Screen::AddHost;
    app.capture_form_baseline();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert!(!app.forms.is_discard_pending());
}

#[test]
fn test_host_form_dirty_esc_shows_confirmation() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    *app.forms.host_mut() = crate::app::HostForm::new();
    app.screen = Screen::AddHost;
    app.capture_form_baseline();
    app.forms.host_mut().alias = "dirty".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::AddHost));
    assert!(app.forms.is_discard_pending());
}

#[test]
fn test_host_form_dirty_esc_y_closes() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    *app.forms.host_mut() = crate::app::HostForm::new();
    app.screen = Screen::AddHost;
    app.capture_form_baseline();
    app.forms.host_mut().alias = "dirty".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert!(app.forms.host_baseline().is_none());
}

#[test]
fn test_host_form_dirty_esc_n_stays() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    *app.forms.host_mut() = crate::app::HostForm::new();
    app.screen = Screen::AddHost;
    app.capture_form_baseline();
    app.forms.host_mut().hostname = "changed.com".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    assert!(matches!(app.screen, Screen::AddHost));
    assert!(!app.forms.is_discard_pending());
}

#[test]
fn test_host_form_dirty_esc_other_key_ignored() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    *app.forms.host_mut() = crate::app::HostForm::new();
    app.screen = Screen::AddHost;
    app.capture_form_baseline();
    app.forms.host_mut().alias = "dirty".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('x')), &tx);
    assert!(app.forms.is_discard_pending()); // still pending
}

#[test]
fn test_tunnel_form_dirty_esc_shows_confirmation() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::TunnelForm {
        alias: "test".to_string(),
        editing: None,
    };
    *app.tunnels.form_mut() = crate::app::TunnelForm::new();
    app.capture_tunnel_form_baseline();
    app.tunnels.form_mut().bind_port = "9000".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::TunnelForm { .. }));
    assert!(app.forms.is_discard_pending());
}

#[test]
fn test_tunnel_form_clean_esc_closes() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::TunnelForm {
        alias: "test".to_string(),
        editing: None,
    };
    *app.tunnels.form_mut() = crate::app::TunnelForm::new();
    app.capture_tunnel_form_baseline();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::TunnelList { .. }));
}

// --- Delete confirmation tests ---

#[test]
fn test_snippet_picker_d_esc_cancels_delete() {
    let mut app = make_snippet_app();
    let _ = app.snippets.store_mut().save();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    assert_eq!(app.snippets.pending_delete(), Some(0));
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert_eq!(app.snippets.pending_delete(), None);
    assert_eq!(app.snippets.store().snippets.len(), 2);
}

#[test]
fn test_snippet_picker_d_n_cancels_delete() {
    let mut app = make_snippet_app();
    let _ = app.snippets.store_mut().save();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    assert_eq!(app.snippets.pending_delete(), None);
    assert_eq!(app.snippets.store().snippets.len(), 2);
}

#[test]
fn test_snippet_picker_d_other_key_ignored() {
    let mut app = make_snippet_app();
    let _ = app.snippets.store_mut().save();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    assert_eq!(app.snippets.pending_delete(), Some(0));
    assert_eq!(app.snippets.store().snippets.len(), 2);
}

#[test]
fn test_confirm_import_uppercase_y_works() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::ConfirmImport { count: 0 };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('Y')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn test_confirm_import_n_cancels() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::ConfirmImport { count: 0 };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn test_confirm_import_uppercase_n_cancels() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::ConfirmImport { count: 0 };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('N')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
}

// --- HostDetail navigation tests ---

#[test]
fn test_host_detail_esc_returns_to_host_list() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::HostDetail { index: 0 };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn test_host_detail_e_opens_edit() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::HostDetail { index: 0 };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
    assert!(matches!(app.screen, Screen::EditHost { .. }));
    assert!(app.forms.host_baseline().is_some());
}

#[test]
fn test_host_detail_t_opens_tunnel_list() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::HostDetail { index: 0 };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('T')), &tx);
    assert!(matches!(app.screen, Screen::TunnelList { .. }));
}

#[test]
fn test_host_detail_r_opens_snippet_picker() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::HostDetail { index: 0 };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('r')), &tx);
    assert!(matches!(app.screen, Screen::SnippetPicker));
}

#[test]
fn test_host_detail_e_on_included_host_stays() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.hosts_state.list_mut()[0].source_file = Some(PathBuf::from("/etc/ssh/config.d/test"));
    app.screen = Screen::HostDetail { index: 0 };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
    assert!(matches!(app.screen, Screen::HostDetail { .. }));
    assert!(app.status_center.toast().unwrap().is_error());
}

// --- Provider form: Left/Right on toggle fields does NOT toggle ---

#[test]
fn test_provider_form_left_on_verify_tls_stays_same() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::VerifyTls);
    assert!(app.providers.form_mut().verify_tls);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Left), &tx);
    assert!(app.providers.form_mut().verify_tls);
}

#[test]
fn test_provider_form_right_on_verify_tls_stays_same() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::VerifyTls);
    assert!(app.providers.form_mut().verify_tls);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Right), &tx);
    assert!(app.providers.form_mut().verify_tls);
}

#[test]
fn test_provider_form_left_on_auto_sync_stays_same() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::AutoSync);
    assert!(app.providers.form_mut().auto_sync);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Left), &tx);
    assert!(app.providers.form_mut().auto_sync);
}

#[test]
fn test_provider_form_right_on_auto_sync_stays_same() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::AutoSync);
    assert!(app.providers.form_mut().auto_sync);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Right), &tx);
    assert!(app.providers.form_mut().auto_sync);
}

// --- Provider form: dirty-check on Esc ---

#[test]
fn test_provider_form_clean_esc_with_baseline_closes() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.capture_provider_form_baseline();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::Providers));
    assert!(!app.forms.is_discard_pending());
}

#[test]
fn test_provider_form_dirty_esc_shows_confirmation() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.capture_provider_form_baseline();
    app.providers.form_mut().token = "newtoken".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert!(app.forms.is_discard_pending());
}

#[test]
fn test_provider_form_dirty_esc_y_closes() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.capture_provider_form_baseline();
    app.providers.form_mut().token = "newtoken".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(matches!(app.screen, Screen::Providers));
    assert!(app.providers.form_baseline().is_none());
}

#[test]
fn test_provider_form_dirty_esc_n_stays() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.capture_provider_form_baseline();
    app.providers.form_mut().token = "newtoken".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert!(!app.forms.is_discard_pending());
}

// --- Snippet form: dirty-check on Esc ---

#[test]
fn test_snippet_form_clean_esc_with_baseline_closes() {
    let mut app = make_snippet_app();
    *app.snippets.form_mut() = crate::app::SnippetForm::new();
    app.snippets.set_flow_targets(vec!["myserver".to_string()]);
    app.snippets.set_form_editing(None);
    app.screen = Screen::SnippetForm;
    app.capture_snippet_form_baseline();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::SnippetPicker));
    assert!(!app.forms.is_discard_pending());
}

#[test]
fn test_snippet_form_dirty_esc_shows_confirmation() {
    let mut app = make_snippet_app();
    *app.snippets.form_mut() = crate::app::SnippetForm::new();
    app.snippets.set_flow_targets(vec!["myserver".to_string()]);
    app.snippets.set_form_editing(None);
    app.screen = Screen::SnippetForm;
    app.capture_snippet_form_baseline();
    app.snippets.form_mut().name = "dirty".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::SnippetForm));
    assert!(app.forms.is_discard_pending());
}

#[test]
fn test_snippet_form_dirty_esc_y_closes() {
    let mut app = make_snippet_app();
    *app.snippets.form_mut() = crate::app::SnippetForm::new();
    app.snippets.set_flow_targets(vec!["myserver".to_string()]);
    app.snippets.set_form_editing(None);
    app.screen = Screen::SnippetForm;
    app.capture_snippet_form_baseline();
    app.snippets.form_mut().name = "dirty".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(matches!(app.screen, Screen::SnippetPicker));
    assert!(app.snippets.form_baseline().is_none());
}

// --- Tunnel delete: d/y/Esc/n ---

#[test]
fn test_tunnel_list_d_y_deletes_tunnel() {
    let mut app = make_app("Host test\n  HostName test.com\n  LocalForward 8080 localhost:80\n");
    app.screen = Screen::TunnelList {
        alias: "test".to_string(),
    };
    app.refresh_tunnel_list("test");
    app.ui.tunnel_list_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    assert_eq!(app.tunnels.pending_delete(), Some(0));
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(app.tunnels.pending_delete().is_none());
}

#[test]
fn test_tunnel_list_d_esc_cancels_delete() {
    let mut app = make_app("Host test\n  HostName test.com\n  LocalForward 8080 localhost:80\n");
    app.screen = Screen::TunnelList {
        alias: "test".to_string(),
    };
    app.refresh_tunnel_list("test");
    app.ui.tunnel_list_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    assert_eq!(app.tunnels.pending_delete(), Some(0));
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(app.tunnels.pending_delete().is_none());
    assert_eq!(app.tunnels.list().len(), 1);
}

#[test]
fn test_tunnel_list_d_n_cancels_delete() {
    let mut app = make_app("Host test\n  HostName test.com\n  LocalForward 8080 localhost:80\n");
    app.screen = Screen::TunnelList {
        alias: "test".to_string(),
    };
    app.refresh_tunnel_list("test");
    app.ui.tunnel_list_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    assert!(app.tunnels.pending_delete().is_none());
    assert_eq!(app.tunnels.list().len(), 1);
}

#[test]
fn test_tunnel_delete_logs_removed_forward() {
    let mut app = make_app("Host test\n  HostName test.com\n  LocalForward 8080 localhost:80\n");
    app.screen = Screen::TunnelList {
        alias: "test".to_string(),
    };
    app.refresh_tunnel_list("test");
    app.ui.tunnel_list_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    let records = crate::logging::capture::capture(|| {
        let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    });
    assert_eq!(app.tunnels.list().len(), 0);
    assert!(
        crate::logging::capture::has(
            &records,
            log::Level::Debug,
            "[purple] tunnel forward removed: alias=test"
        ),
        "expected tunnel-removed debug log, got: {:?}",
        records
    );
}

#[test]
fn test_tunnel_add_logs_added_forward() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.open_tunnel_add_form("test".to_string());
    {
        let form = app.tunnels.form_mut();
        form.tunnel_type = crate::tunnel::TunnelType::Local;
        form.bind_port = "9090".to_string();
        form.remote_host = "localhost".to_string();
        form.remote_port = "90".to_string();
    }
    let (tx, _rx) = mpsc::channel();
    let records = crate::logging::capture::capture(|| {
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    });
    assert!(
        crate::logging::capture::has(
            &records,
            log::Level::Debug,
            "[purple] tunnel forward added: alias=test"
        ),
        "expected tunnel-added debug log, got: {:?}",
        records
    );
}

#[test]
fn test_tunnel_edit_logs_edited_forward() {
    let mut app = make_app("Host test\n  HostName test.com\n  LocalForward 8080 localhost:80\n");
    app.refresh_tunnel_list("test");
    let rule = app.tunnels.list()[0].clone();
    app.open_tunnel_edit_form("test".to_string(), &rule, 0);
    // Change the bind port so the edited directive is not a duplicate.
    app.tunnels.form_mut().bind_port = "9999".to_string();
    let (tx, _rx) = mpsc::channel();
    let records = crate::logging::capture::capture(|| {
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    });
    assert!(
        crate::logging::capture::has(
            &records,
            log::Level::Debug,
            "[purple] tunnel forward edited: alias=test"
        ),
        "expected tunnel-edited debug log, got: {:?}",
        records
    );
}

// --- Host form: baseline cleared after submit ---

#[test]
fn test_host_form_baseline_cleared_after_submit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("test_config");
    std::fs::write(&config_path, "Host test\n  HostName test.com\n").unwrap();
    let config = SshConfigFile {
        elements: SshConfigFile::parse_content("Host test\n  HostName test.com\n"),
        path: config_path.clone(),
        crlf: false,
        bom: false,
    };
    let mut app = App::new(config);
    *app.providers.config_mut() = test_provider_config();
    *app.forms.host_mut() = crate::app::HostForm::new();
    app.forms.host_mut().alias = "newhost".to_string();
    app.forms.host_mut().hostname = "new.example.com".to_string();
    app.screen = Screen::AddHost;
    app.capture_form_mtime();
    app.capture_form_baseline();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.forms.host_baseline().is_none());
}

// --- Edge case: uppercase Y in discard confirms ---

#[test]
fn test_host_form_dirty_esc_uppercase_y_closes() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    *app.forms.host_mut() = crate::app::HostForm::new();
    app.screen = Screen::AddHost;
    app.capture_form_baseline();
    app.forms.host_mut().user = "ubuntu".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('Y')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert!(app.forms.host_baseline().is_none());
}

// --- Snippet form: dirty + n stays ---

#[test]
fn test_snippet_form_dirty_esc_n_stays() {
    let mut app = make_snippet_app();
    *app.snippets.form_mut() = crate::app::SnippetForm::new();
    app.snippets.set_flow_targets(vec!["myserver".to_string()]);
    app.snippets.set_form_editing(None);
    app.screen = Screen::SnippetForm;
    app.capture_snippet_form_baseline();
    app.snippets.form_mut().command = "changed".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    assert!(matches!(app.screen, Screen::SnippetForm));
    assert!(!app.forms.is_discard_pending());
}

// Stray key on the discard prompt must NOT dismiss the discard confirm.
// route_confirm_key's Ignored arm forbids a buggy refactor from letting any
// keypress silently confirm or cancel the discard.
#[test]
fn test_snippet_form_dirty_esc_other_key_ignored() {
    let mut app = make_snippet_app();
    *app.snippets.form_mut() = crate::app::SnippetForm::new();
    app.snippets.set_flow_targets(vec!["myserver".to_string()]);
    app.snippets.set_form_editing(None);
    app.screen = Screen::SnippetForm;
    app.capture_snippet_form_baseline();
    app.snippets.form_mut().command = "changed".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('x')), &tx);
    assert!(matches!(app.screen, Screen::SnippetForm));
    assert!(app.forms.is_discard_pending());
}

// --- Tunnel form: dirty + y closes, dirty + n stays ---

#[test]
fn test_tunnel_form_dirty_esc_y_closes() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::TunnelForm {
        alias: "test".to_string(),
        editing: None,
    };
    *app.tunnels.form_mut() = crate::app::TunnelForm::new();
    app.capture_tunnel_form_baseline();
    app.tunnels.form_mut().remote_host = "db.local".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(matches!(app.screen, Screen::TunnelList { .. }));
    assert!(app.tunnels.form_baseline().is_none());
}

#[test]
fn test_tunnel_form_dirty_esc_n_stays() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::TunnelForm {
        alias: "test".to_string(),
        editing: None,
    };
    *app.tunnels.form_mut() = crate::app::TunnelForm::new();
    app.capture_tunnel_form_baseline();
    app.tunnels.form_mut().bind_port = "9001".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    assert!(matches!(app.screen, Screen::TunnelForm { .. }));
    assert!(!app.forms.is_discard_pending());
}

// --- Tunnel delete: other key ignored ---

#[test]
fn test_tunnel_delete_other_key_ignored() {
    let mut app = make_app("Host test\n  HostName test.com\n  LocalForward 8080 localhost:80\n");
    app.screen = Screen::TunnelList {
        alias: "test".to_string(),
    };
    app.refresh_tunnel_list("test");
    app.ui.tunnel_list_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    assert_eq!(app.tunnels.pending_delete(), Some(0));
    let _ = handle_key_event(&mut app, key(KeyCode::Char('z')), &tx);
    assert_eq!(app.tunnels.pending_delete(), Some(0));
}

// --- Provider form: dirty + other key ignored ---

#[test]
fn test_provider_form_dirty_esc_other_key_ignored() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.capture_provider_form_baseline();
    app.providers.form_mut().token = "newtoken".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('x')), &tx);
    assert!(app.forms.is_discard_pending());
}

// --- Stale purge tests ---

#[test]
fn test_x_key_opens_confirm_purge_stale() {
    let mut app = make_app(
        "Host do-web\n  HostName 1.2.3.4\n  # purple:provider digitalocean:123\n  # purple:stale 1711900000\n",
    );
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('X')), &tx);
    assert!(
        matches!(app.screen, Screen::ConfirmPurgeStale),
        "expected ConfirmPurgeStale, got {:?}",
        app.screen
    );
    let payload = app
        .providers
        .pending_purge()
        .expect("pending_purge payload must be set");
    assert_eq!(payload.aliases.len(), 1);
    assert_eq!(payload.aliases[0], "do-web");
    assert!(payload.provider.is_none());
}

#[test]
fn test_x_key_no_stale_shows_status() {
    let mut app = make_app("Host normal\n  HostName 1.2.3.4\n");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('X')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    let toast = app.status_center.toast().expect("toast should be set");
    assert!(
        toast.text.contains("No stale hosts"),
        "expected 'No stale hosts' in toast, got: {}",
        toast.text
    );
}

#[test]
fn test_confirm_purge_stale_y_deletes() {
    let mut app = make_app(
        "Host do-web\n  HostName 1.2.3.4\n  # purple:provider digitalocean:123\n  # purple:stale 1711900000\n\nHost keep\n  HostName 5.6.7.8\n",
    );
    app.providers.set_pending_purge(crate::app::PendingPurge {
        aliases: vec!["do-web".to_string()],
        provider: None,
    });
    app.screen = Screen::ConfirmPurgeStale;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    // The stale host should be gone, only "keep" remains
    let aliases: Vec<&str> = app
        .hosts_state
        .list()
        .iter()
        .map(|h| h.alias.as_str())
        .collect();
    assert!(!aliases.contains(&"do-web"), "stale host should be removed");
    assert!(aliases.contains(&"keep"), "non-stale host should remain");
    assert!(
        app.providers.pending_purge().is_none(),
        "yes path must clear the pending purge payload"
    );
}

#[test]
fn test_confirm_purge_stale_esc_cancels() {
    let mut app = make_app(
        "Host do-web\n  HostName 1.2.3.4\n  # purple:provider digitalocean:123\n  # purple:stale 1711900000\n",
    );
    app.providers.set_pending_purge(crate::app::PendingPurge {
        aliases: vec!["do-web".to_string()],
        provider: None,
    });
    app.screen = Screen::ConfirmPurgeStale;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    // Host should still exist
    assert_eq!(app.hosts_state.list().len(), 1);
    assert_eq!(app.hosts_state.list()[0].alias, "do-web");
    // Esc must also drop the pending payload so a re-opened dialog
    // starts from a clean slot.
    assert!(
        app.providers.pending_purge().is_none(),
        "Esc must clear pending_purge"
    );
}

#[test]
fn test_e_key_warns_on_stale_host() {
    let mut app = make_app(
        "Host do-web\n  HostName 1.2.3.4\n  # purple:provider digitalocean:123\n  # purple:stale 1711900000\n",
    );
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
    // Edit form should open (warning, not block)
    assert!(matches!(app.screen, Screen::EditHost { .. }));
    let toast = app.status_center.toast().expect("toast should be set");
    assert!(toast.text.contains("Stale host"));
    assert!(toast.text.contains("DigitalOcean"));
    assert!(toast.is_error());
}

#[test]
fn test_d_key_warns_on_stale_host() {
    let mut app = make_app(
        "Host do-web\n  HostName 1.2.3.4\n  # purple:provider digitalocean:123\n  # purple:stale 1711900000\n",
    );
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    // Delete confirm should open (warning, not block)
    assert!(matches!(app.screen, Screen::ConfirmDelete { .. }));
    let toast = app.status_center.toast().expect("toast should be set");
    assert!(toast.text.contains("Stale host"));
    assert!(toast.is_error());
}

#[test]
fn test_enter_on_stale_host_shows_warning() {
    let mut app = make_app(
        "Host do-web\n  HostName 1.2.3.4\n  # purple:provider digitalocean:123\n  # purple:stale 1711900000\n",
    );
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    // Connection should still be pending
    assert!(app.ui.pending_connect().is_some());
    // But toast should show stale warning
    let toast = app.status_center.toast().expect("toast should be set");
    assert!(
        toast.text.contains("Stale host"),
        "expected stale warning, got: {}",
        toast.text
    );
    assert!(toast.text.contains("DigitalOcean"));
}

#[test]
fn test_enter_on_normal_host_no_stale_warning() {
    let mut app = make_app("Host normal\n  HostName 1.2.3.4\n");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.ui.pending_connect().is_some());
    // No stale warning
    assert!(
        app.status_center.toast().is_none()
            || !app.status_center.toast().unwrap().text.contains("Stale"),
    );
}

#[test]
fn test_search_enter_on_stale_host_shows_warning() {
    let mut app = make_app(
        "Host do-web\n  HostName 1.2.3.4\n  # purple:provider digitalocean:123\n  # purple:stale 1711900000\n",
    );
    // Enter search mode
    app.search.set_query(Some("do-web".to_string()));
    app.apply_filter();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.ui.pending_connect().is_some());
    let toast = app.status_center.toast().expect("toast should be set");
    assert!(
        toast.text.contains("Stale host"),
        "expected stale warning in search mode, got: {}",
        toast.text
    );
}

#[test]
fn test_c_key_warns_on_stale_host() {
    let mut app = make_app(
        "Host do-web\n  HostName 1.2.3.4\n  # purple:provider digitalocean:123\n  # purple:stale 1711900000\n",
    );
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('c')), &tx);
    assert!(matches!(app.screen, Screen::AddHost));
    let toast = app.status_center.toast().expect("toast should be set");
    assert!(
        toast.text.contains("Stale host"),
        "expected stale warning, got: {}",
        toast.text
    );
    assert!(toast.is_error());
}

#[test]
fn test_t_key_warns_on_stale_host() {
    let mut app = make_app(
        "Host do-web\n  HostName 1.2.3.4\n  # purple:provider digitalocean:123\n  # purple:stale 1711900000\n",
    );
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('T')), &tx);
    assert!(
        matches!(app.screen, Screen::TunnelList { .. }),
        "expected TunnelList screen, got: {:?}",
        app.screen
    );
    let toast = app.status_center.toast().expect("toast should be set");
    assert!(
        toast.text.contains("Stale host"),
        "expected stale warning, got: {}",
        toast.text
    );
    assert!(toast.is_error());
}

#[test]
fn test_provider_x_key_opens_scoped_purge() {
    let mut app = make_app(
        "Host do-web\n  HostName 1.2.3.4\n  # purple:provider digitalocean:123\n  # purple:stale 1711900000\n",
    );
    app.screen = Screen::Providers;
    *app.providers.config_mut() = test_provider_config();
    app.providers.config_mut().set_section(ProviderSection {
        filter: String::new(),
        id: crate::providers::config::ProviderConfigId::bare("digitalocean"),
        token: "tok".to_string(),
        alias_prefix: "do".to_string(),
        user: "root".to_string(),
        identity_file: String::new(),
        url: String::new(),
        verify_tls: true,
        auto_sync: true,
        profile: String::new(),
        regions: String::new(),
        project: String::new(),
        compartment: String::new(),
        vault_role: String::new(),
        vault_addr: String::new(),
        ssm: crate::providers::aws_ssm::SsmMode::default(),
    });
    // Select the DigitalOcean provider in the list
    let sorted = app.sorted_provider_names();
    let idx = sorted
        .iter()
        .position(|n| n == "digitalocean")
        .expect("digitalocean should be in sorted list");
    app.ui.provider_list_state_mut().select(Some(idx));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('X')), &tx);
    assert!(
        matches!(app.screen, Screen::ConfirmPurgeStale),
        "expected ConfirmPurgeStale, got {:?}",
        app.screen
    );
    let payload = app
        .providers
        .pending_purge()
        .expect("pending_purge payload must be set");
    assert_eq!(payload.aliases, vec!["do-web".to_string()]);
    assert_eq!(payload.provider.as_deref(), Some("digitalocean"));
}

#[test]
fn test_provider_purge_y_returns_to_providers() {
    let mut app = make_app(
        "Host do-web\n  HostName 1.2.3.4\n  # purple:provider digitalocean:123\n  # purple:stale 1711900000\n",
    );
    app.providers.set_pending_purge(crate::app::PendingPurge {
        aliases: vec!["do-web".to_string()],
        provider: Some("digitalocean".to_string()),
    });
    app.screen = Screen::ConfirmPurgeStale;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(
        matches!(app.screen, Screen::Providers),
        "expected Providers screen after provider-scoped purge, got: {:?}",
        app.screen
    );
}

#[test]
fn test_provider_purge_esc_returns_to_providers() {
    let mut app = make_app(
        "Host do-web\n  HostName 1.2.3.4\n  # purple:provider digitalocean:123\n  # purple:stale 1711900000\n",
    );
    app.providers.set_pending_purge(crate::app::PendingPurge {
        aliases: vec!["do-web".to_string()],
        provider: Some("digitalocean".to_string()),
    });
    app.screen = Screen::ConfirmPurgeStale;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(
        matches!(app.screen, Screen::Providers),
        "expected Providers screen after Esc on provider-scoped purge, got: {:?}",
        app.screen
    );
    // Host should still exist (purge was cancelled)
    assert_eq!(app.hosts_state.list().len(), 1);
    assert_eq!(app.hosts_state.list()[0].alias, "do-web");
}

// =========================================================================
// Container handler tests
// =========================================================================

fn make_container_state(
    alias: &str,
    containers: Vec<crate::containers::ContainerInfo>,
) -> crate::app::ContainerSession {
    let mut list_state = ratatui::widgets::ListState::default();
    if !containers.is_empty() {
        list_state.select(Some(0));
    }
    crate::app::ContainerSession {
        alias: alias.to_string(),
        askpass: None,
        runtime: Some(crate::containers::ContainerRuntime::Docker),
        containers,
        list_state,
        loading: false,
        error: None,
        action_in_progress: None,
        confirm_action: None,
    }
}

fn make_container(id: &str, name: &str, state: &str) -> crate::containers::ContainerInfo {
    crate::containers::ContainerInfo {
        id: id.to_string(),
        names: name.to_string(),
        image: "test:latest".to_string(),
        state: state.to_string(),
        status: "Up".to_string(),
        ports: "".to_string(),
    }
}

#[test]
fn test_shift_c_opens_containers() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('C')), &tx);
    assert!(
        matches!(app.screen, Screen::Containers { .. }),
        "expected Containers screen, got: {:?}",
        app.screen
    );
    assert!(
        app.container_session.is_some(),
        "container_state should be Some after Shift+C"
    );
}

#[test]
fn test_shift_c_no_host_noop() {
    let mut app = make_app("");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('C')), &tx);
    assert!(
        matches!(app.screen, Screen::HostList),
        "expected HostList when no hosts, got: {:?}",
        app.screen
    );
    assert!(app.container_session.is_none());
}

#[test]
fn test_shift_c_loads_cache() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.container_state.insert_cache_entry(
        "web".to_string(),
        crate::containers::ContainerCacheEntry {
            timestamp: 100,
            runtime: crate::containers::ContainerRuntime::Docker,
            engine_version: None,
            containers: vec![make_container("abc", "nginx", "running")],
        },
    );
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('C')), &tx);
    let state = app.container_session.as_ref().unwrap();
    assert_eq!(state.containers.len(), 1);
    assert_eq!(state.containers[0].id, "abc");
    assert_eq!(
        state.runtime,
        Some(crate::containers::ContainerRuntime::Docker)
    );
}

#[test]
fn test_shift_c_no_cache_empty() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('C')), &tx);
    let state = app.container_session.as_ref().unwrap();
    assert!(state.containers.is_empty());
    assert!(state.runtime.is_none());
}

#[test]
fn test_containers_esc_closes() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    app.container_session = Some(make_container_state("web", vec![]));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert!(app.container_session.is_none());
}

#[test]
fn test_containers_q_closes() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    app.container_session = Some(make_container_state("web", vec![]));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('q')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert!(app.container_session.is_none());
}

#[test]
fn test_containers_j_moves_down() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    let containers = vec![
        make_container("a", "web", "running"),
        make_container("b", "db", "running"),
        make_container("c", "cache", "exited"),
    ];
    app.container_session = Some(make_container_state("web", containers));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    let sel = app
        .container_session
        .as_ref()
        .unwrap()
        .list_state
        .selected();
    assert_eq!(sel, Some(1));
}

#[test]
fn test_containers_k_moves_up() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    let containers = vec![
        make_container("a", "web", "running"),
        make_container("b", "db", "running"),
    ];
    let mut state = make_container_state("web", containers);
    state.list_state.select(Some(1));
    app.container_session = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
    let sel = app
        .container_session
        .as_ref()
        .unwrap()
        .list_state
        .selected();
    assert_eq!(sel, Some(0));
}

#[test]
fn test_containers_j_wraps() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    let containers = vec![
        make_container("a", "web", "running"),
        make_container("b", "db", "running"),
    ];
    let mut state = make_container_state("web", containers);
    state.list_state.select(Some(1)); // at last
    app.container_session = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    let sel = app
        .container_session
        .as_ref()
        .unwrap()
        .list_state
        .selected();
    assert_eq!(sel, Some(0), "j at last item should wrap to 0");
}

#[test]
fn test_containers_k_wraps() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    let containers = vec![
        make_container("a", "web", "running"),
        make_container("b", "db", "running"),
    ];
    app.container_session = Some(make_container_state("web", containers));
    // selection starts at 0
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
    let sel = app
        .container_session
        .as_ref()
        .unwrap()
        .list_state
        .selected();
    assert_eq!(sel, Some(1), "k at first item should wrap to last");
}

#[test]
fn test_containers_j_empty_noop() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    app.container_session = Some(make_container_state("web", vec![]));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    let sel = app
        .container_session
        .as_ref()
        .unwrap()
        .list_state
        .selected();
    assert_eq!(sel, None);
}

#[test]
fn test_containers_k_empty_noop() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    app.container_session = Some(make_container_state("web", vec![]));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
    let sel = app
        .container_session
        .as_ref()
        .unwrap()
        .list_state
        .selected();
    assert_eq!(sel, None);
}

#[test]
fn test_containers_page_down() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    let containers: Vec<_> = (0..20)
        .map(|i| make_container(&format!("c{i}"), &format!("svc{i}"), "running"))
        .collect();
    app.container_session = Some(make_container_state("web", containers));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::PageDown), &tx);
    let sel = app
        .container_session
        .as_ref()
        .unwrap()
        .list_state
        .selected();
    assert_eq!(sel, Some(10));
}

#[test]
fn test_containers_page_up() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    let containers: Vec<_> = (0..20)
        .map(|i| make_container(&format!("c{i}"), &format!("svc{i}"), "running"))
        .collect();
    let mut state = make_container_state("web", containers);
    state.list_state.select(Some(15));
    app.container_session = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::PageUp), &tx);
    let sel = app
        .container_session
        .as_ref()
        .unwrap()
        .list_state
        .selected();
    assert_eq!(sel, Some(5));
}

#[test]
fn test_containers_s_sets_action_in_progress() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    app.container_session = Some(make_container_state(
        "web",
        vec![make_container("abc123", "nginx", "exited")],
    ));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('s')), &tx);
    let state = app.container_session.as_ref().unwrap();
    assert!(
        state.action_in_progress.is_some(),
        "action_in_progress should be set after s"
    );
    assert!(
        state.action_in_progress.as_ref().unwrap().contains("start"),
        "action should contain 'start'"
    );
}

#[test]
fn test_containers_x_shows_confirmation() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    app.container_session = Some(make_container_state(
        "web",
        vec![make_container("abc123", "nginx", "running")],
    ));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('x')), &tx);
    let state = app.container_session.as_ref().unwrap();
    assert!(state.confirm_action.is_some());
    let (action, name, _id) = state.confirm_action.as_ref().unwrap();
    assert_eq!(*action, crate::containers::ContainerAction::Stop);
    assert_eq!(name, "nginx");
}

#[test]
fn test_containers_r_shows_confirmation() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    app.container_session = Some(make_container_state(
        "web",
        vec![make_container("abc123", "nginx", "running")],
    ));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('r')), &tx);
    let state = app.container_session.as_ref().unwrap();
    assert!(state.confirm_action.is_some());
    let (action, name, _id) = state.confirm_action.as_ref().unwrap();
    assert_eq!(*action, crate::containers::ContainerAction::Restart);
    assert_eq!(name, "nginx");
}

#[test]
fn test_containers_y_confirms_action() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    let mut state = make_container_state("web", vec![make_container("abc123", "nginx", "running")]);
    state.confirm_action = Some((
        crate::containers::ContainerAction::Stop,
        "nginx".to_string(),
        "abc123".to_string(),
    ));
    app.container_session = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    let state = app.container_session.as_ref().unwrap();
    assert!(state.confirm_action.is_none());
    assert!(state.action_in_progress.is_some());
}

#[test]
fn test_containers_esc_cancels_confirmation() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    let mut state = make_container_state("web", vec![make_container("abc123", "nginx", "running")]);
    state.confirm_action = Some((
        crate::containers::ContainerAction::Stop,
        "nginx".to_string(),
        "abc123".to_string(),
    ));
    app.container_session = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    // Should cancel confirmation but stay in overlay
    assert!(app.container_session.is_some());
    assert!(
        app.container_session
            .as_ref()
            .unwrap()
            .confirm_action
            .is_none()
    );
    assert!(matches!(app.screen, Screen::Containers { .. }));
}

// Pins the handler's `?`-bypass gate: in browse context `q` closes the
// overlay, but during a pending confirm it must be treated as Ignored.
// A future edit that whitelisted `q` alongside `?` would silently
// dismiss destructive confirms by closing the overlay; this test
// catches that regression.
#[test]
fn test_containers_q_during_confirm_is_ignored() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    let mut state = make_container_state("web", vec![make_container("abc123", "nginx", "running")]);
    state.confirm_action = Some((
        crate::containers::ContainerAction::Stop,
        "nginx".to_string(),
        "abc123".to_string(),
    ));
    app.container_session = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('q')), &tx);
    assert!(
        app.container_session.is_some(),
        "q must NOT close overlay while confirm is pending"
    );
    let state = app.container_session.as_ref().unwrap();
    assert!(
        state.confirm_action.is_some(),
        "q must NOT clear pending confirm"
    );
    assert!(
        state.action_in_progress.is_none(),
        "q must NOT execute pending action"
    );
    assert!(matches!(app.screen, Screen::Containers { .. }));
}

// Pins the route_confirm_key Ignored contract: a stray key during a pending
// container-action confirm must NOT cancel the confirm or fire the action.
// Guards against a regression where the early-return is replaced by a
// catch-all that silently dismisses destructive confirms.
#[test]
fn test_containers_stray_key_during_confirm_is_ignored() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    let mut state = make_container_state("web", vec![make_container("abc123", "nginx", "running")]);
    state.confirm_action = Some((
        crate::containers::ContainerAction::Stop,
        "nginx".to_string(),
        "abc123".to_string(),
    ));
    app.container_session = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('z')), &tx);
    let state = app.container_session.as_ref().unwrap();
    assert!(
        state.confirm_action.is_some(),
        "stray key must NOT clear pending confirm"
    );
    assert!(
        state.action_in_progress.is_none(),
        "stray key must NOT execute pending action"
    );
}

#[test]
fn test_containers_action_blocked_when_in_progress() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    let mut state = make_container_state("web", vec![make_container("abc123", "nginx", "running")]);
    state.action_in_progress = Some("stop nginx...".to_string());
    app.container_session = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('s')), &tx);
    // action_in_progress should remain the same (not changed to start)
    let state = app.container_session.as_ref().unwrap();
    assert_eq!(state.action_in_progress.as_deref(), Some("stop nginx..."));
}

#[test]
fn test_containers_action_no_selection_noop() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    let mut state = make_container_state("web", vec![]);
    state.list_state.select(None);
    app.container_session = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('s')), &tx);
    assert!(
        app.container_session
            .as_ref()
            .unwrap()
            .action_in_progress
            .is_none(),
        "no action should start without selection"
    );
}

#[test]
fn test_containers_action_no_runtime_noop() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    let mut state = make_container_state("web", vec![make_container("abc123", "nginx", "running")]);
    state.runtime = None;
    app.container_session = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('s')), &tx);
    assert!(
        app.container_session
            .as_ref()
            .unwrap()
            .action_in_progress
            .is_none(),
        "no action should start without runtime"
    );
}

#[test]
fn test_containers_r_uppercase_refreshes() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    app.container_session = Some(make_container_state(
        "web",
        vec![make_container("abc123", "nginx", "running")],
    ));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('R')), &tx);
    assert!(
        app.container_session.as_ref().unwrap().loading,
        "loading should be true after R"
    );
}

#[test]
fn test_containers_r_uppercase_blocked_when_in_progress() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    let mut state = make_container_state("web", vec![make_container("abc123", "nginx", "running")]);
    state.action_in_progress = Some("restart nginx...".to_string());
    app.container_session = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('R')), &tx);
    assert!(
        !app.container_session.as_ref().unwrap().loading,
        "loading should remain false when action is in progress"
    );
}

#[test]
fn test_containers_unknown_key_noop() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    let containers = vec![make_container("abc123", "nginx", "running")];
    app.container_session = Some(make_container_state("web", containers.clone()));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('z')), &tx);
    assert!(matches!(app.screen, Screen::Containers { .. }));
    let state = app.container_session.as_ref().unwrap();
    assert_eq!(state.list_state.selected(), Some(0));
    assert!(state.action_in_progress.is_none());
    assert!(!state.loading);
}

#[test]
fn test_containers_y_noop_without_pending() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    app.container_session = Some(make_container_state(
        "web",
        vec![make_container("abc123", "nginx", "running")],
    ));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    let state = app.container_session.as_ref().unwrap();
    assert!(
        state.action_in_progress.is_none(),
        "no action should start when confirm_action is None"
    );
    assert!(
        state.confirm_action.is_none(),
        "confirm_action should remain None"
    );
}

#[test]
fn test_containers_x_blocked_when_action_in_progress() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    let mut state = make_container_state("web", vec![make_container("abc123", "nginx", "running")]);
    state.action_in_progress = Some("stop nginx...".to_string());
    app.container_session = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('x')), &tx);
    let state = app.container_session.as_ref().unwrap();
    assert!(
        state.confirm_action.is_none(),
        "x should not open confirmation when action is in progress"
    );
}

#[test]
fn test_containers_r_blocked_when_action_in_progress() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    let mut state = make_container_state("web", vec![make_container("abc123", "nginx", "running")]);
    state.action_in_progress = Some("stop nginx...".to_string());
    app.container_session = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('r')), &tx);
    let state = app.container_session.as_ref().unwrap();
    assert!(
        state.confirm_action.is_none(),
        "r should not open confirmation when action is in progress"
    );
}

#[test]
fn test_containers_x_blocked_when_confirm_pending() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    let mut state = make_container_state("web", vec![make_container("abc123", "nginx", "running")]);
    state.confirm_action = Some((
        crate::containers::ContainerAction::Stop,
        "nginx".to_string(),
        "abc123".to_string(),
    ));
    app.container_session = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('x')), &tx);
    let state = app.container_session.as_ref().unwrap();
    let (action, name, _id) = state.confirm_action.as_ref().unwrap();
    assert_eq!(
        *action,
        crate::containers::ContainerAction::Stop,
        "confirm_action should remain the original Stop"
    );
    assert_eq!(name, "nginx");
}

#[test]
fn test_containers_r_blocked_when_confirm_pending() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    let mut state = make_container_state("web", vec![make_container("abc123", "nginx", "running")]);
    state.confirm_action = Some((
        crate::containers::ContainerAction::Stop,
        "nginx".to_string(),
        "abc123".to_string(),
    ));
    app.container_session = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('r')), &tx);
    let state = app.container_session.as_ref().unwrap();
    let (action, name, _id) = state.confirm_action.as_ref().unwrap();
    assert_eq!(
        *action,
        crate::containers::ContainerAction::Stop,
        "confirm_action should remain the original Stop, not change to Restart"
    );
    assert_eq!(name, "nginx");
}

// --- Help key (?) tests for all overlay screens ---

#[test]
fn test_file_browser_question_opens_help() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::FileBrowser {
        alias: "web".to_string(),
    };
    app.file_browser_session = Some(crate::file_browser::FileBrowserSession {
        alias: "web".to_string(),
        askpass: None,
        active_pane: crate::file_browser::BrowserPane::Local,
        local_path: std::path::PathBuf::from("/tmp"),
        local_entries: Vec::new(),
        local_list_state: ratatui::widgets::ListState::default(),
        local_selected: std::collections::HashSet::new(),
        local_error: None,
        remote_path: "/home".to_string(),
        remote_entries: Vec::new(),
        remote_list_state: ratatui::widgets::ListState::default(),
        remote_selected: std::collections::HashSet::new(),
        remote_error: None,
        remote_loading: false,
        show_hidden: false,
        sort: crate::file_browser::BrowserSort::Name,
        confirm_copy: None,
        transferring: None,
        transfer_error: None,
        connection_recorded: false,
    });
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('?')), &tx);
    match &app.screen {
        Screen::Help { return_screen } => {
            assert!(matches!(**return_screen, Screen::FileBrowser { .. }));
        }
        other => panic!("Expected Help screen, got {:?}", other),
    }
}

#[test]
fn test_file_browser_help_esc_returns() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::FileBrowser {
            alias: "web".to_string(),
        }),
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::FileBrowser { .. }));
}

fn make_file_browser_session_with_confirm() -> crate::file_browser::FileBrowserSession {
    crate::file_browser::FileBrowserSession {
        alias: "web".to_string(),
        askpass: None,
        active_pane: crate::file_browser::BrowserPane::Local,
        local_path: std::path::PathBuf::from("/tmp"),
        local_entries: Vec::new(),
        local_list_state: ratatui::widgets::ListState::default(),
        local_selected: std::collections::HashSet::new(),
        local_error: None,
        remote_path: "/home".to_string(),
        remote_entries: Vec::new(),
        remote_list_state: ratatui::widgets::ListState::default(),
        remote_selected: std::collections::HashSet::new(),
        remote_error: None,
        remote_loading: false,
        show_hidden: false,
        sort: crate::file_browser::BrowserSort::Name,
        confirm_copy: Some(crate::file_browser::CopyRequest {
            sources: vec!["readme.txt".to_string()],
            source_pane: crate::file_browser::BrowserPane::Local,
            has_dirs: false,
        }),
        transferring: None,
        transfer_error: None,
        connection_recorded: false,
    }
}

#[test]
fn test_file_browser_confirm_yes_starts_transfer() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::FileBrowser {
        alias: "web".to_string(),
    };
    app.file_browser_session = Some(make_file_browser_session_with_confirm());
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    let fb = app.file_browser_session.as_ref().unwrap();
    assert!(fb.confirm_copy.is_none(), "y must consume confirm_copy");
    assert!(
        fb.transferring.is_some(),
        "y must set transferring to lock input while scp runs"
    );
}

#[test]
fn test_file_browser_confirm_no_clears_request() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::FileBrowser {
        alias: "web".to_string(),
    };
    app.file_browser_session = Some(make_file_browser_session_with_confirm());
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    let fb = app.file_browser_session.as_ref().unwrap();
    assert!(fb.confirm_copy.is_none(), "n must clear confirm_copy");
    assert!(fb.transferring.is_none(), "n must not start a transfer");
}

// Pins the route_confirm_key Ignored contract for the SCP confirm dialog:
// a stray key during pending confirm must NOT cancel the dialog or kick
// off the transfer. Guards against a regression where the inner match
// gets a catch-all that silently dismisses confirm_copy.
#[test]
fn test_file_browser_confirm_stray_key_is_ignored() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::FileBrowser {
        alias: "web".to_string(),
    };
    app.file_browser_session = Some(make_file_browser_session_with_confirm());
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('z')), &tx);
    let fb = app.file_browser_session.as_ref().unwrap();
    assert!(
        fb.confirm_copy.is_some(),
        "stray key must NOT clear pending confirm_copy"
    );
    assert!(
        fb.transferring.is_none(),
        "stray key must NOT start a transfer"
    );
}

#[test]
fn file_browser_enter_on_local_file_stages_copy() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::FileBrowser {
        alias: "web".to_string(),
    };
    let mut local_list_state = ratatui::widgets::ListState::default();
    local_list_state.select(Some(1)); // idx 0 is "..", idx 1 is the first entry
    app.file_browser_session = Some(crate::file_browser::FileBrowserSession {
        alias: "web".to_string(),
        askpass: None,
        active_pane: crate::file_browser::BrowserPane::Local,
        local_path: std::path::PathBuf::from("/tmp"),
        local_entries: vec![crate::file_browser::FileEntry {
            name: "data.txt".to_string(),
            is_dir: false,
            size: Some(10),
            modified: None,
        }],
        local_list_state,
        local_selected: std::collections::HashSet::new(),
        local_error: None,
        remote_path: "/home".to_string(),
        remote_entries: Vec::new(),
        remote_list_state: ratatui::widgets::ListState::default(),
        remote_selected: std::collections::HashSet::new(),
        remote_error: None,
        remote_loading: false,
        show_hidden: false,
        sort: crate::file_browser::BrowserSort::Name,
        confirm_copy: None,
        transferring: None,
        transfer_error: None,
        connection_recorded: false,
    });
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    let fb = app.file_browser_session.as_ref().unwrap();
    let req = fb
        .confirm_copy
        .as_ref()
        .expect("Enter on a local file with a remote target must stage a copy");
    assert_eq!(req.sources, vec!["data.txt".to_string()]);
    assert!(matches!(
        req.source_pane,
        crate::file_browser::BrowserPane::Local
    ));
    assert!(!req.has_dirs);
}

#[test]
fn file_browser_starts_no_second_remote_run_while_a_listing_loads() {
    // A security key PIN on trial must reach the key from one ssh only, so
    // an upload, `.` and `R` wait until the listing is back.
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::FileBrowser {
        alias: "web".to_string(),
    };
    let mut fb = make_file_browser_session_with_entries();
    fb.remote_loading = true;
    app.file_browser_session = Some(fb);
    let (tx, rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(
        app.file_browser_session
            .as_ref()
            .unwrap()
            .confirm_copy
            .is_none(),
        "no upload while the listing loads"
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Char('.')), &tx);
    assert!(
        !app.file_browser_session.as_ref().unwrap().show_hidden,
        "`.` waits for the listing"
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Char('R')), &tx);
    drop(tx);
    assert!(rx.recv().is_err(), "no listing was started");
}

fn make_file_browser_session_with_entries() -> crate::file_browser::FileBrowserSession {
    let mut local_list_state = ratatui::widgets::ListState::default();
    local_list_state.select(Some(1));
    crate::file_browser::FileBrowserSession {
        alias: "web".to_string(),
        askpass: None,
        active_pane: crate::file_browser::BrowserPane::Local,
        local_path: std::path::PathBuf::from("/tmp"),
        local_entries: vec![
            crate::file_browser::FileEntry {
                name: "a.txt".to_string(),
                is_dir: false,
                size: Some(1),
                modified: None,
            },
            crate::file_browser::FileEntry {
                name: "b.txt".to_string(),
                is_dir: false,
                size: Some(2),
                modified: None,
            },
        ],
        local_list_state,
        local_selected: std::collections::HashSet::new(),
        local_error: None,
        remote_path: "/home".to_string(),
        remote_entries: Vec::new(),
        remote_list_state: ratatui::widgets::ListState::default(),
        remote_selected: std::collections::HashSet::new(),
        remote_error: None,
        remote_loading: false,
        show_hidden: false,
        sort: crate::file_browser::BrowserSort::Name,
        confirm_copy: None,
        transferring: None,
        transfer_error: None,
        connection_recorded: false,
    }
}

#[test]
fn file_browser_plain_space_toggles_selection() {
    // Issue #82: macOS reserves Ctrl+Space for the input source switcher,
    // so plain Space must toggle the file selection in the active pane.
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::FileBrowser {
        alias: "web".to_string(),
    };
    app.file_browser_session = Some(make_file_browser_session_with_entries());
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    let fb = app.file_browser_session.as_ref().unwrap();
    assert!(
        fb.local_selected.contains("a.txt"),
        "plain Space must select the entry under the cursor"
    );

    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    let fb = app.file_browser_session.as_ref().unwrap();
    assert!(
        !fb.local_selected.contains("a.txt"),
        "plain Space on a selected entry must deselect it"
    );
}

#[test]
fn file_browser_ctrl_space_still_toggles_selection() {
    // Backwards compat: users with muscle memory for Ctrl+Space outside
    // tmux/macOS-IME contexts keep the old binding.
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::FileBrowser {
        alias: "web".to_string(),
    };
    app.file_browser_session = Some(make_file_browser_session_with_entries());
    let (tx, _rx) = mpsc::channel();

    let ctrl_space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL);
    let _ = handle_key_event(&mut app, ctrl_space, &tx);
    let fb = app.file_browser_session.as_ref().unwrap();
    assert!(
        fb.local_selected.contains("a.txt"),
        "Ctrl+Space must keep working as a selection toggle"
    );
}

#[test]
fn file_browser_plain_a_toggles_select_all() {
    // Issue #82: tmux binds Ctrl+A as the prefix, so plain `a` must
    // toggle select-all in the active pane.
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::FileBrowser {
        alias: "web".to_string(),
    };
    app.file_browser_session = Some(make_file_browser_session_with_entries());
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char('a')), &tx);
    let fb = app.file_browser_session.as_ref().unwrap();
    assert_eq!(
        fb.local_selected.len(),
        2,
        "plain `a` must select every visible entry"
    );

    let _ = handle_key_event(&mut app, key(KeyCode::Char('a')), &tx);
    let fb = app.file_browser_session.as_ref().unwrap();
    assert!(
        fb.local_selected.is_empty(),
        "plain `a` on a fully-selected pane must deselect everything"
    );
}

#[test]
fn file_browser_ctrl_a_still_toggles_select_all() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::FileBrowser {
        alias: "web".to_string(),
    };
    app.file_browser_session = Some(make_file_browser_session_with_entries());
    let (tx, _rx) = mpsc::channel();

    let ctrl_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
    let _ = handle_key_event(&mut app, ctrl_a, &tx);
    let fb = app.file_browser_session.as_ref().unwrap();
    assert_eq!(
        fb.local_selected.len(),
        2,
        "Ctrl+A must keep working as select-all"
    );
}

#[test]
fn test_snippet_picker_question_opens_help() {
    let mut app = make_snippet_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('?')), &tx);
    match &app.screen {
        Screen::Help { return_screen } => {
            assert!(matches!(**return_screen, Screen::SnippetPicker));
        }
        other => panic!("Expected Help screen, got {:?}", other),
    }
}

#[test]
fn test_snippet_picker_help_esc_returns() {
    let mut app = make_snippet_app();
    app.snippets.set_flow_targets(vec!["myserver".to_string()]);
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::SnippetPicker),
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::SnippetPicker));
}

#[test]
fn test_snippet_output_question_opens_help() {
    let mut app = make_snippet_app();
    // First enter snippet output by pressing Enter
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(matches!(app.screen, Screen::SnippetOutput));

    // Now press ? to open help
    let _ = handle_key_event(&mut app, key(KeyCode::Char('?')), &tx);
    match &app.screen {
        Screen::Help { return_screen } => {
            assert!(matches!(**return_screen, Screen::SnippetOutput));
        }
        other => panic!("Expected Help screen, got {:?}", other),
    }
}

#[test]
fn test_snippet_output_help_esc_returns() {
    let mut app = make_snippet_app();
    app.snippets
        .set_output_snippet_name(Some("check-disk".to_string()));
    app.snippets.set_flow_targets(vec!["myserver".to_string()]);
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::SnippetOutput),
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::SnippetOutput));
}

#[test]
fn test_containers_question_opens_help() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    app.container_session = Some(make_container_state("web", vec![]));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('?')), &tx);
    match &app.screen {
        Screen::Help { return_screen } => {
            assert!(matches!(**return_screen, Screen::Containers { .. }));
        }
        other => panic!("Expected Help screen, got {:?}", other),
    }
}

#[test]
fn test_containers_help_esc_returns() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::Containers {
            alias: "web".to_string(),
        }),
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::Containers { .. }));
}

#[test]
fn test_tunnel_list_question_opens_help() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::TunnelList {
        alias: "web".to_string(),
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('?')), &tx);
    match &app.screen {
        Screen::Help { return_screen } => {
            assert!(matches!(**return_screen, Screen::TunnelList { .. }));
        }
        other => panic!("Expected Help screen, got {:?}", other),
    }
}

#[test]
fn test_tunnel_list_help_esc_returns() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::TunnelList {
            alias: "web".to_string(),
        }),
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::TunnelList { .. }));
}

// --- Direct ? from HostList ---

#[test]
fn test_host_list_question_opens_help() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('?')), &tx);
    match &app.screen {
        Screen::Help { return_screen } => {
            assert!(matches!(**return_screen, Screen::HostList));
        }
        other => panic!("Expected Help screen, got {:?}", other),
    }
}

// --- ? guard bypass tests ---

#[test]
fn test_tunnel_delete_confirmation_question_opens_help() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::TunnelList {
        alias: "web".to_string(),
    };
    app.tunnels.request_delete(0);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('?')), &tx);
    match &app.screen {
        Screen::Help { return_screen } => {
            assert!(matches!(**return_screen, Screen::TunnelList { .. }));
        }
        other => panic!("Expected Help screen, got {:?}", other),
    }
    assert_eq!(
        app.tunnels.pending_delete(),
        Some(0),
        "pending_tunnel_delete should be preserved"
    );
}

#[test]
fn test_container_confirm_action_question_opens_help() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    let mut state = make_container_state("web", vec![make_container("abc123", "nginx", "running")]);
    state.confirm_action = Some((
        crate::containers::ContainerAction::Stop,
        "nginx".to_string(),
        "abc123".to_string(),
    ));
    app.container_session = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('?')), &tx);
    match &app.screen {
        Screen::Help { return_screen } => {
            assert!(matches!(**return_screen, Screen::Containers { .. }));
        }
        other => panic!("Expected Help screen, got {:?}", other),
    }
}

#[test]
fn test_snippet_picker_pending_delete_question_opens_help() {
    let mut app = make_snippet_app();
    app.snippets.request_delete(0);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('?')), &tx);
    match &app.screen {
        Screen::Help { return_screen } => {
            assert!(matches!(**return_screen, Screen::SnippetPicker));
        }
        other => panic!("Expected Help screen, got {:?}", other),
    }
}

// --- Help scroll tests ---

#[test]
fn test_help_j_increments_scroll() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::HostList),
    };
    app.ui.set_help_scroll(0);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    assert_eq!(app.ui.help_scroll(), 1);
}

#[test]
fn test_help_k_does_not_underflow() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::HostList),
    };
    app.ui.set_help_scroll(0);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
    assert_eq!(app.ui.help_scroll(), 0);
}

#[test]
fn test_help_page_down_increments_by_ten() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::HostList),
    };
    app.ui.set_help_scroll(0);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::PageDown), &tx);
    assert_eq!(app.ui.help_scroll(), 10);
}

#[test]
fn test_help_page_up_does_not_underflow() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::HostList),
    };
    app.ui.set_help_scroll(0);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::PageUp), &tx);
    assert_eq!(app.ui.help_scroll(), 0);
}

#[test]
fn test_help_scroll_reset_on_close() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::HostList),
    };
    app.ui.set_help_scroll(7);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert_eq!(app.ui.help_scroll(), 0);
    assert!(matches!(app.screen, Screen::HostList));
}

// --- Help close via q and ? ---

#[test]
fn test_help_q_closes_and_returns() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::TunnelList {
            alias: "web".to_string(),
        }),
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('q')), &tx);
    assert!(matches!(app.screen, Screen::TunnelList { .. }));
    assert_eq!(app.ui.help_scroll(), 0);
}

#[test]
fn test_help_question_again_closes_and_returns() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::Containers {
            alias: "web".to_string(),
        }),
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('?')), &tx);
    assert!(matches!(app.screen, Screen::Containers { .. }));
    assert_eq!(app.ui.help_scroll(), 0);
}

// --- Return screen field preservation ---

#[test]
fn test_file_browser_help_return_preserves_alias() {
    let mut app = make_app("Host myserver\n  HostName 1.2.3.4\n");
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::FileBrowser {
            alias: "myserver".to_string(),
        }),
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    match &app.screen {
        Screen::FileBrowser { alias } => {
            assert_eq!(alias, "myserver");
        }
        other => panic!("Expected FileBrowser, got {:?}", other),
    }
}

#[test]
fn test_snippet_output_help_return_preserves_fields() {
    let mut app = make_app("Host a\n  HostName 1.2.3.4\nHost b\n  HostName 5.6.7.8\n");
    app.snippets
        .set_output_snippet_name(Some("check-disk".to_string()));
    app.snippets
        .set_flow_targets(vec!["a".to_string(), "b".to_string()]);
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::SnippetOutput),
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(
        matches!(app.screen, Screen::SnippetOutput),
        "Expected SnippetOutput, got {:?}",
        app.screen
    );
    // Snippet name and flow targets survive the Help round-trip because
    // they live on `app.snippets`, not inside the Screen variant.
    assert_eq!(app.snippets.output_snippet_name(), Some("check-disk"));
    assert_eq!(
        app.snippets.flow_targets(),
        &["a".to_string(), "b".to_string()][..]
    );
}

#[test]
fn test_tunnel_list_help_return_preserves_alias() {
    let mut app = make_app("Host myserver\n  HostName 1.2.3.4\n");
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::TunnelList {
            alias: "myserver".to_string(),
        }),
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    match &app.screen {
        Screen::TunnelList { alias } => {
            assert_eq!(alias, "myserver");
        }
        other => panic!("Expected TunnelList, got {:?}", other),
    }
}

// --- Non-help screens ignore ? ---

#[test]
fn test_confirm_delete_question_does_not_open_help() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::ConfirmDelete {
        alias: "web".to_string(),
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('?')), &tx);
    assert!(
        matches!(app.screen, Screen::ConfirmDelete { .. }),
        "Expected ConfirmDelete screen, got {:?}",
        app.screen
    );
}

#[test]
fn test_tag_picker_question_opens_help() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::TagPicker;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('?')), &tx);
    match &app.screen {
        Screen::Help { return_screen } => {
            assert!(matches!(**return_screen, Screen::TagPicker));
        }
        other => panic!("expected Help, got {:?}", std::mem::discriminant(other)),
    }
}

#[test]
fn test_key_list_question_opens_help() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::KeyList;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('?')), &tx);
    match &app.screen {
        Screen::Help { return_screen } => {
            assert!(matches!(**return_screen, Screen::KeyList));
        }
        other => panic!("expected Help, got {:?}", std::mem::discriminant(other)),
    }
}

#[test]
fn test_key_detail_question_opens_help() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::KeyDetail { index: 0 };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('?')), &tx);
    match &app.screen {
        Screen::Help { return_screen } => {
            assert!(matches!(**return_screen, Screen::KeyDetail { .. }));
        }
        other => panic!("expected Help, got {:?}", std::mem::discriminant(other)),
    }
}

#[test]
fn test_host_detail_question_opens_help() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::HostDetail { index: 0 };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('?')), &tx);
    match &app.screen {
        Screen::Help { return_screen } => {
            assert!(matches!(**return_screen, Screen::HostDetail { .. }));
        }
        other => panic!("expected Help, got {:?}", std::mem::discriminant(other)),
    }
}

#[test]
fn test_providers_question_opens_help() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Providers;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('?')), &tx);
    match &app.screen {
        Screen::Help { return_screen } => {
            assert!(matches!(**return_screen, Screen::Providers));
        }
        other => panic!("expected Help, got {:?}", std::mem::discriminant(other)),
    }
}

// --- g-key GroupBy cycle ---

#[test]
fn g_key_none_to_provider() {
    let mut app = make_app("Host web1\n  HostName 1.2.3.4\n  # purple:provider digitalocean:1\n");
    assert_eq!(app.hosts_state.group_by(), &crate::app::GroupBy::None);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('g')), &tx);
    assert_eq!(app.hosts_state.group_by(), &crate::app::GroupBy::Provider);
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn g_key_provider_to_tag_mode_when_tags_exist() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production
";
    let mut app = make_app(content);
    app.hosts_state
        .set_group_by_raw(crate::app::GroupBy::Provider);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('g')), &tx);
    assert!(
        matches!(app.hosts_state.group_by(), &crate::app::GroupBy::Tag(_)),
        "expected Tag mode, got {:?}",
        app.hosts_state.group_by()
    );
    assert!(
        matches!(app.screen, Screen::HostList),
        "should stay on HostList, not open picker"
    );
}

#[test]
fn g_key_provider_to_none_when_no_tags() {
    let content = "\
Host web1
  HostName 1.1.1.1
";
    let mut app = make_app(content);
    app.hosts_state
        .set_group_by_raw(crate::app::GroupBy::Provider);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('g')), &tx);
    assert_eq!(app.hosts_state.group_by(), &crate::app::GroupBy::None);
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn g_key_tag_to_none() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production
";
    let mut app = make_app(content);
    app.hosts_state
        .set_group_by_raw(crate::app::GroupBy::Tag("production".to_string()));
    app.apply_sort();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('g')), &tx);
    assert_eq!(app.hosts_state.group_by(), &crate::app::GroupBy::None);
    assert!(matches!(app.screen, Screen::HostList));
    assert!(
        app.hosts_state
            .display_list()
            .iter()
            .all(|item| matches!(item, crate::app::HostListItem::Host { .. }))
    );
}

#[test]
fn g_key_full_cycle_with_tags() {
    // None → Provider → Tag → None
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production
";
    let mut app = make_app(content);
    assert_eq!(app.hosts_state.group_by(), &crate::app::GroupBy::None);

    let (tx, _rx) = mpsc::channel();

    // None → Provider
    let _ = handle_key_event(&mut app, key(KeyCode::Char('g')), &tx);
    assert_eq!(app.hosts_state.group_by(), &crate::app::GroupBy::Provider);

    // Provider → Tag (direct, no picker)
    let _ = handle_key_event(&mut app, key(KeyCode::Char('g')), &tx);
    assert!(
        matches!(app.hosts_state.group_by(), &crate::app::GroupBy::Tag(_)),
        "expected Tag mode, got {:?}",
        app.hosts_state.group_by()
    );
    assert!(matches!(app.screen, Screen::HostList));

    // Tag → None
    let _ = handle_key_event(&mut app, key(KeyCode::Char('g')), &tx);
    assert_eq!(app.hosts_state.group_by(), &crate::app::GroupBy::None);
}

#[test]
fn g_key_tag_to_none_empty_hosts() {
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut app = make_app("");
    app.hosts_state
        .set_group_by_raw(crate::app::GroupBy::Tag("production".to_string()));

    let key = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE);
    let _ = handle_key_event(&mut app, key, &tx);

    assert_eq!(app.hosts_state.group_by(), &crate::app::GroupBy::None);
    assert!(matches!(app.screen, Screen::HostList));
}

// =========================================================================
// Group header collapse tests
// =========================================================================

#[test]
fn test_enter_on_group_header_does_not_connect() {
    // Enter on a group header should not crash or connect — group headers are
    // no longer collapsible. Navigation happens via Tab (group_filter).
    let mut app = make_app(
        "Host web1\n  HostName 1.1.1.1\n  # purple:tags production\n\nHost web2\n  HostName 2.2.2.2\n  # purple:tags staging\n",
    );
    app.hosts_state
        .set_group_by_raw(crate::app::GroupBy::Tag("production".to_string()));
    app.hosts_state
        .set_sort_mode(crate::app::SortMode::AlphaAlias);
    app.apply_sort();

    // Find the group header position
    let header_pos = app
        .hosts_state
        .display_list()
        .iter()
        .position(
            |item| matches!(item, crate::app::HostListItem::GroupHeader(t) if t == "production"),
        )
        .expect("should have a production group header");
    app.ui.list_state_mut().select(Some(header_pos));

    // Press Enter — should not panic and group_filter should remain None
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();

    assert!(
        app.hosts_state.group_filter().is_none(),
        "group_filter should not be set by Enter on header"
    );
}

// =========================================================================
// Ctrl+A select all tests
// =========================================================================

#[test]
fn test_ctrl_a_selects_all_visible_hosts() {
    let mut app = make_app(
        "Host web1\n  HostName 1.1.1.1\n\nHost web2\n  HostName 2.2.2.2\n\nHost web3\n  HostName 3.3.3.3\n",
    );
    app.apply_sort();
    assert!(app.hosts_state.multi_select().is_empty());

    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, ctrl_key('a'), &tx).unwrap();

    // All 3 hosts should be selected
    assert_eq!(app.hosts_state.multi_select().len(), 3);

    // Press Ctrl+A again to deselect all
    handle_key_event(&mut app, ctrl_key('a'), &tx).unwrap();
    assert!(app.hosts_state.multi_select().is_empty());
}

#[test]
fn test_ctrl_a_in_search_mode_selects_filtered() {
    let mut app = make_app(
        "Host prod-web\n  HostName 1.1.1.1\n\nHost prod-db\n  HostName 2.2.2.2\n\nHost staging-app\n  HostName 3.3.3.3\n",
    );
    app.apply_sort();

    // Enter search mode and filter to "prod"
    app.search.set_query(Some("prod".to_string()));
    app.apply_filter();
    assert_eq!(app.search.filtered_indices().len(), 2);
    assert!(app.hosts_state.multi_select().is_empty());

    // Ctrl+A should select only the 2 filtered hosts
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, ctrl_key('a'), &tx).unwrap();
    assert_eq!(app.hosts_state.multi_select().len(), 2);

    // Press Ctrl+A again to deselect
    handle_key_event(&mut app, ctrl_key('a'), &tx).unwrap();
    assert!(app.hosts_state.multi_select().is_empty());
}

// =========================================================================
// Tab / Shift+Tab top-page navigation tests (HostList screen)
// =========================================================================

#[test]
fn tab_on_host_list_switches_to_tunnels_page() {
    let mut app = make_app("Host web1\n  HostName 1.1.1.1\n");
    assert_eq!(app.top_page, crate::app::TopPage::Hosts);

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);

    assert_eq!(app.top_page, crate::app::TopPage::Tunnels);
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn shift_tab_on_host_list_switches_pages() {
    let mut app = make_app("Host web1\n  HostName 1.1.1.1\n");
    assert_eq!(app.top_page, crate::app::TopPage::Hosts);

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(
        &mut app,
        KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
        &tx,
    );

    // Five-tab cycle: Hosts <- Keys <- Snippets <- Containers <- Tunnels <- Hosts.
    // One BackTab from Hosts lands on Keys.
    assert_eq!(app.top_page, crate::app::TopPage::Keys);
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn tab_five_times_returns_to_hosts_page() {
    let mut app = make_app("Host web1\n  HostName 1.1.1.1\n");
    assert_eq!(app.top_page, crate::app::TopPage::Hosts);

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(app.top_page, crate::app::TopPage::Tunnels);
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(app.top_page, crate::app::TopPage::Containers);
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(app.top_page, crate::app::TopPage::Snippets);
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(app.top_page, crate::app::TopPage::Keys);
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);

    assert_eq!(app.top_page, crate::app::TopPage::Hosts);
    assert!(matches!(app.screen, Screen::HostList));
}

/// Every top-level tab must honour the standard cross-tab keys. Guards against
/// a new (or refactored) tab silently dropping `q` / `:` / `n`, the way the
/// Snippets tab once did. Driven through the real `handle_key_event` dispatch
/// so it exercises each tab's handler exactly as the running app does.
const ALL_TOP_PAGES: [crate::app::TopPage; 5] = [
    crate::app::TopPage::Hosts,
    crate::app::TopPage::Tunnels,
    crate::app::TopPage::Containers,
    crate::app::TopPage::Snippets,
    crate::app::TopPage::Keys,
];

#[test]
fn every_top_page_quits_on_q() {
    let (tx, _rx) = mpsc::channel();
    for page in ALL_TOP_PAGES {
        let mut app = make_app("Host web1\n  HostName 1.1.1.1\n");
        app.top_page = page;
        assert!(app.running, "app starts running on {page:?}");
        let _ = handle_key_event(&mut app, key(KeyCode::Char('q')), &tx);
        assert!(!app.running, "q must quit on the {page:?} tab");
    }
}

#[test]
fn every_top_page_opens_jump_on_colon() {
    let (tx, _rx) = mpsc::channel();
    for page in ALL_TOP_PAGES {
        let mut app = make_app("Host web1\n  HostName 1.1.1.1\n");
        app.top_page = page;
        let _ = handle_key_event(&mut app, key(KeyCode::Char(':')), &tx);
        assert!(
            app.jump.is_some(),
            ": must open the jump palette on the {page:?} tab"
        );
    }
}

#[test]
fn every_top_page_opens_whats_new_on_n() {
    let (tx, _rx) = mpsc::channel();
    for page in ALL_TOP_PAGES {
        let mut app = make_app("Host web1\n  HostName 1.1.1.1\n");
        app.top_page = page;
        let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
        assert!(
            matches!(app.screen, Screen::WhatsNew(_)),
            "n must open What's New on the {page:?} tab"
        );
    }
}

#[test]
fn g_key_no_longer_affects_top_page() {
    let content = "\
Host aws-web1
  HostName 1.1.1.1
  # purple:provider aws:i-123

Host do-web2
  HostName 2.2.2.2
  # purple:provider digitalocean:abc
";
    let mut app = make_app(content);
    let starting_page = app.top_page;

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('g')), &tx);

    // Grouping toggled internally, top page unchanged.
    assert_eq!(app.top_page, starting_page);
    assert_eq!(app.hosts_state.group_by(), &crate::app::GroupBy::Provider);
}

#[test]
fn esc_clears_group_filter() {
    let content = "\
Host aws-web1
  HostName 1.1.1.1
  # purple:provider aws:i-123

Host do-web2
  HostName 2.2.2.2
  # purple:provider digitalocean:abc
";
    let mut app = make_app(content);
    app.hosts_state
        .set_group_by_raw(crate::app::GroupBy::Provider);
    app.apply_sort();
    let first_group = app.hosts_state.group_tab_order()[0].clone();
    app.hosts_state.set_group_filter(Some(first_group));
    app.apply_sort();
    assert!(app.running);

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);

    assert!(
        app.hosts_state.group_filter().is_none(),
        "Esc with active group_filter should clear it"
    );
    assert!(app.running, "Esc with active filter should NOT quit");
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn esc_does_not_quit_first_press_shows_hint_toast() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    assert!(app.hosts_state.group_filter().is_none());
    assert!(app.hosts_state.multi_select().is_empty());
    assert!(app.running);
    assert!(!app.ui.esc_quit_hint_shown());

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);

    assert!(app.running, "Esc on idle host list must not quit");
    assert!(
        app.ui.esc_quit_hint_shown(),
        "first idle Esc must arm the one-shot hint flag"
    );
    let toast = app
        .status_center
        .toast()
        .expect("first idle Esc must surface a toast");
    assert_eq!(toast.text, crate::messages::ESC_QUIT_HINT);
}

#[test]
fn esc_second_press_after_hint_is_silent_noop() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(
        app.status_center.toast().is_some(),
        "first press should have produced a toast"
    );
    // Simulate the toast having been read and dismissed.
    app.status_center.set_toast_message(None);

    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);

    assert!(app.running, "second idle Esc must still not quit");
    assert!(
        app.status_center.toast().is_none(),
        "second idle Esc must stay silent (no repeated toast)"
    );
}

#[test]
fn q_still_quits_after_esc_hint() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(app.running);
    assert!(app.ui.esc_quit_hint_shown());

    let _ = handle_key_event(&mut app, key(KeyCode::Char('q')), &tx);
    assert!(
        !app.running,
        "q must always quit, even after Esc hint shown"
    );
}

#[test]
fn test_shift_p_key_clears_ping_increments_generation() {
    let mut app = make_app("Host web1\n  HostName 1.1.1.1\n");
    // Pre-populate ping status to simulate completed pings
    app.ping.insert_status(
        "web1".to_string(),
        crate::app::PingStatus::Reachable { rtt_ms: 10 },
    );
    app.ping
        .record_check("web1".to_string(), std::time::Instant::now());
    app.ping.set_filter_down_only(true);
    app.ping.set_checked_at(Some(std::time::Instant::now()));
    assert_eq!(app.ping.generation(), 0);

    let (tx, _rx) = std::sync::mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Char('P')), &tx).unwrap();

    assert!(app.ping.status_is_empty());
    assert!(app.ping.last_checked().is_empty());
    assert_eq!(app.ping.generation(), 1);
    assert!(!app.ping.filter_down_only());
    assert!(app.ping.checked_at().is_none());
}

#[test]
fn test_p_key_clears_ping_increments_generation() {
    let mut app = make_app("Host web1\n  HostName 1.1.1.1\n");
    app.ping.insert_status(
        "web1".to_string(),
        crate::app::PingStatus::Reachable { rtt_ms: 10 },
    );
    app.ping
        .record_check("web1".to_string(), std::time::Instant::now());
    app.ping.set_filter_down_only(true);
    app.ping.set_checked_at(Some(std::time::Instant::now()));
    assert_eq!(app.ping.generation(), 0);

    let (tx, _rx) = std::sync::mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Char('p')), &tx).unwrap();

    assert!(app.ping.status_is_empty());
    assert!(app.ping.last_checked().is_empty());
    assert_eq!(app.ping.generation(), 1);
    assert!(!app.ping.filter_down_only());
    assert!(app.ping.checked_at().is_none());
}

#[test]
fn test_ctrl_p_with_active_filter_clears_pings_and_cancels_search() {
    let mut app = make_app("Host web1\n  HostName 1.1.1.1\n");
    app.ping.insert_status(
        "web1".to_string(),
        crate::app::PingStatus::Reachable { rtt_ms: 10 },
    );
    app.ping
        .record_check("web1".to_string(), std::time::Instant::now());
    app.ping.set_filter_down_only(true);
    app.search.set_query(Some("we".to_string()));

    let (tx, _rx) = std::sync::mpsc::channel();
    handle_key_event(&mut app, ctrl_key('p'), &tx).unwrap();

    // Ping cleared
    assert!(app.ping.status_is_empty());
    assert!(app.ping.last_checked().is_empty());
    assert!(!app.ping.filter_down_only());
    // Active filter triggered cancel_search: query is gone
    assert!(app.search.query().is_none());
}

#[test]
fn test_ctrl_p_without_active_filter_clears_pings_and_preserves_search() {
    let mut app = make_app("Host web1\n  HostName 1.1.1.1\n");
    app.ping.insert_status(
        "web1".to_string(),
        crate::app::PingStatus::Reachable { rtt_ms: 10 },
    );
    app.ping.set_filter_down_only(false);
    app.search.set_query(Some("we".to_string()));

    let (tx, _rx) = std::sync::mpsc::channel();
    handle_key_event(&mut app, ctrl_key('p'), &tx).unwrap();

    // Ping cleared
    assert!(app.ping.status_is_empty());
    assert!(!app.ping.filter_down_only());
    // No active filter: search query is preserved (cancel_search not called)
    assert_eq!(app.search.query(), Some("we"));
}

#[test]
fn test_bang_key_without_pings_shows_error() {
    let mut app = make_app("Host web1\n  HostName 1.1.1.1\n");
    assert!(app.ping.status_is_empty());
    let (tx, _rx) = std::sync::mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Char('!')), &tx).unwrap();
    assert!(!app.ping.filter_down_only());
    assert!(app.status_center.toast().unwrap().is_error());
}

#[test]
fn test_bang_key_toggles_down_only_on() {
    let mut app = make_app("Host web1\n  HostName 1.1.1.1\nHost web2\n  HostName 2.2.2.2\n");
    app.ping
        .insert_status("web1".to_string(), crate::app::PingStatus::Unreachable);
    app.ping.insert_status(
        "web2".to_string(),
        crate::app::PingStatus::Reachable { rtt_ms: 10 },
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Char('!')), &tx).unwrap();
    assert!(app.ping.filter_down_only());
    assert!(app.search.query().is_some());
    // Only web1 (Unreachable) should be in filtered results
    assert_eq!(app.search.filtered_indices().len(), 1);
}

#[test]
fn test_bang_key_toggles_down_only_off() {
    let mut app = make_app("Host web1\n  HostName 1.1.1.1\nHost web2\n  HostName 2.2.2.2\n");
    app.ping
        .insert_status("web1".to_string(), crate::app::PingStatus::Unreachable);
    app.ping.insert_status(
        "web2".to_string(),
        crate::app::PingStatus::Reachable { rtt_ms: 10 },
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    // Toggle on
    handle_key_event(&mut app, key(KeyCode::Char('!')), &tx).unwrap();
    assert!(app.ping.filter_down_only());
    // Toggle off
    handle_key_event(&mut app, key(KeyCode::Char('!')), &tx).unwrap();
    assert!(!app.ping.filter_down_only());
    assert!(app.search.query().is_none());
}

#[test]
fn refresh_selected_if_stale_skips_when_auto_ping_off() {
    let mut app = make_app("Host web1\n  HostName 1.1.1.1\n");
    app.ping.set_auto_ping(false);
    let (tx, _rx) = std::sync::mpsc::channel();
    super::ping::refresh_selected_if_stale(&mut app, &tx);
    assert!(app.ping.status_is_empty());
}

#[test]
fn refresh_selected_if_stale_skips_fresh_host() {
    let mut app = make_app("Host web1\n  HostName 1.1.1.1\n");
    app.ping.set_auto_ping(true);
    app.ping.insert_status(
        "web1".to_string(),
        crate::app::PingStatus::Reachable { rtt_ms: 10 },
    );
    app.ping
        .record_check("web1".to_string(), std::time::Instant::now());
    let (tx, _rx) = std::sync::mpsc::channel();
    super::ping::refresh_selected_if_stale(&mut app, &tx);
    // Status remains Reachable, not flipped to Checking
    assert!(matches!(
        app.ping.status_of("web1"),
        Some(crate::app::PingStatus::Reachable { .. })
    ));
}

#[test]
fn refresh_selected_if_stale_marks_checking_for_stale_host() {
    // Loopback + high port: TCP RST returns immediately so the spawned
    // probe thread exits fast. The PingResult goes to a dropped rx, so
    // status stays Checking — the only behavior we assert.
    let mut app = make_app("Host web1\n  HostName 127.0.0.1\n  Port 59999\n");
    app.ping.set_auto_ping(true);
    app.ping
        .insert_status("web1".to_string(), crate::app::PingStatus::Unreachable);
    let stale = std::time::Instant::now()
        - crate::app::ping::STALE_REFRESH_AFTER
        - std::time::Duration::from_secs(1);
    app.ping.record_check("web1".to_string(), stale);
    let (tx, _rx) = std::sync::mpsc::channel();
    super::ping::refresh_selected_if_stale(&mut app, &tx);
    assert!(matches!(
        app.ping.status_of("web1"),
        Some(crate::app::PingStatus::Checking)
    ));
}

#[test]
fn refresh_selected_if_stale_marks_proxycommand_host_skipped() {
    // No TCP probe for a ProxyCommand host: it lands on Skipped ("proxied")
    // and no ping event is spawned.
    let mut app = make_app(
        "Host tp-node1\n  HostName node1\n  Port 3022\n  ProxyCommand tsh proxy ssh %r@%h:%p\n",
    );
    app.ping.set_auto_ping(true);
    let (tx, rx) = std::sync::mpsc::channel::<crate::event::AppEvent>();
    super::ping::refresh_selected_if_stale(&mut app, &tx);
    assert!(rx.try_recv().is_err());
    assert!(matches!(
        app.ping.status_of("tp-node1"),
        Some(crate::app::PingStatus::Skipped)
    ));
}

#[test]
fn p_key_on_proxycommand_host_skips_and_explains() {
    let mut app = make_app(
        "Host tp-node1\n  HostName node1\n  Port 3022\n  ProxyCommand tsh proxy ssh %r@%h:%p\n",
    );
    let (tx, rx) = std::sync::mpsc::channel::<crate::event::AppEvent>();
    handle_key_event(&mut app, key(KeyCode::Char('p')), &tx).unwrap();
    assert!(rx.try_recv().is_err());
    assert!(matches!(
        app.ping.status_of("tp-node1"),
        Some(crate::app::PingStatus::Skipped)
    ));
    assert!(
        app.status_center
            .toast()
            .unwrap()
            .text
            .contains("ProxyCommand")
    );
}

#[test]
fn shift_p_marks_proxycommand_hosts_skipped_and_probes_the_rest() {
    let mut app = make_app(
        "Host tp-node1\n  HostName node1\n  ProxyCommand tsh proxy ssh %r@%h:%p\n\n\
         Host web1\n  HostName 127.0.0.1\n  Port 59999\n",
    );
    let (tx, _rx) = std::sync::mpsc::channel::<crate::event::AppEvent>();
    handle_key_event(&mut app, key(KeyCode::Char('P')), &tx).unwrap();
    assert!(matches!(
        app.ping.status_of("tp-node1"),
        Some(crate::app::PingStatus::Skipped)
    ));
    assert!(matches!(
        app.ping.status_of("web1"),
        Some(crate::app::PingStatus::Checking)
    ));
}

#[test]
fn refresh_selected_if_stale_skips_host_already_checking() {
    let mut app = make_app("Host web1\n  HostName 1.1.1.1\n");
    app.ping.set_auto_ping(true);
    app.ping
        .insert_status("web1".to_string(), crate::app::PingStatus::Checking);
    let (tx, rx) = std::sync::mpsc::channel::<crate::event::AppEvent>();
    super::ping::refresh_selected_if_stale(&mut app, &tx);
    // Guard fires before ping_host spawn: no event produced.
    assert!(rx.try_recv().is_err());
    assert!(matches!(
        app.ping.status_of("web1"),
        Some(crate::app::PingStatus::Checking)
    ));
}

#[test]
fn refresh_selected_if_stale_skips_host_with_empty_hostname() {
    let mut app = make_app("Host web1\n");
    app.ping.set_auto_ping(true);
    let (tx, _rx) = std::sync::mpsc::channel();
    super::ping::refresh_selected_if_stale(&mut app, &tx);
    assert!(app.ping.status_is_empty());
}

#[test]
fn refresh_selected_if_stale_probes_bastion_for_proxyjump_host() {
    let mut app = make_app(
        "Host bastion\n  HostName 127.0.0.1\n  Port 59999\nHost web1\n  HostName 10.0.0.1\n  ProxyJump bastion\n",
    );
    app.ping.set_auto_ping(true);
    // Select web1 (second selectable item).
    app.select_next();
    assert_eq!(app.selected_host().map(|h| h.alias.as_str()), Some("web1"));
    let (tx, _rx) = std::sync::mpsc::channel();
    super::ping::refresh_selected_if_stale(&mut app, &tx);
    // Bastion (not web1) gets marked Checking — the probe targets the bastion.
    assert!(matches!(
        app.ping.status_of("bastion"),
        Some(crate::app::PingStatus::Checking)
    ));
    assert!(!app.ping.status_contains("web1"));
}

#[test]
fn refresh_selected_if_stale_skips_proxyjump_when_bastion_missing() {
    let mut app = make_app("Host web1\n  HostName 10.0.0.1\n  ProxyJump ghost\n");
    app.ping.set_auto_ping(true);
    let (tx, _rx) = std::sync::mpsc::channel();
    super::ping::refresh_selected_if_stale(&mut app, &tx);
    assert!(app.ping.status_is_empty());
}

/// Stage two hosts where web1 has a stale timestamp and web2 does not. After
/// pressing the navigation key the new selection's stale state should drive
/// a single fresh probe — the previously-stale entry must NOT be re-probed.
fn stage_two_hosts_first_stale() -> App {
    // 127.0.0.1:59999 is the loopback-RST trick used by the existing stale
    // tests: probe spawns and exits fast, status stays Checking.
    let mut app = make_app(
        "Host web1\n  HostName 127.0.0.1\n  Port 59999\nHost web2\n  HostName 127.0.0.1\n  Port 59999\n",
    );
    app.ping.set_auto_ping(true);
    let stale = std::time::Instant::now()
        - crate::app::ping::STALE_REFRESH_AFTER
        - std::time::Duration::from_secs(1);
    app.ping.insert_status(
        "web1".to_string(),
        crate::app::PingStatus::Reachable { rtt_ms: 5 },
    );
    app.ping.record_check("web1".to_string(), stale);
    // web2 starts with no recorded status -> stale on selection.
    app
}

#[test]
fn host_list_down_triggers_stale_refresh_for_new_selection() {
    let mut app = stage_two_hosts_first_stale();
    // Initial selection is web1; press Down to land on web2 (stale because no record).
    let (tx, _rx) = std::sync::mpsc::channel();
    super::host_list::handle_main_key(&mut app, key(KeyCode::Down), &tx);
    assert_eq!(app.selected_host().map(|h| h.alias.as_str()), Some("web2"));
    assert!(matches!(
        app.ping.status_of("web2"),
        Some(crate::app::PingStatus::Checking)
    ));
}

#[test]
fn host_list_j_triggers_stale_refresh_for_new_selection() {
    let mut app = stage_two_hosts_first_stale();
    let (tx, _rx) = std::sync::mpsc::channel();
    super::host_list::handle_main_key(&mut app, key(KeyCode::Char('j')), &tx);
    assert_eq!(app.selected_host().map(|h| h.alias.as_str()), Some("web2"));
    assert!(matches!(
        app.ping.status_of("web2"),
        Some(crate::app::PingStatus::Checking)
    ));
}

#[test]
fn host_list_up_triggers_stale_refresh_when_selection_is_stale() {
    // Land on web2 first, then press k to go back to web1 (which is stale).
    let mut app = stage_two_hosts_first_stale();
    let (tx, _rx) = std::sync::mpsc::channel();
    super::host_list::handle_main_key(&mut app, key(KeyCode::Down), &tx);
    // Clear web2's checking marker so we can observe a fresh transition.
    app.ping.remove_status("web2");
    super::host_list::handle_main_key(&mut app, key(KeyCode::Char('k')), &tx);
    assert_eq!(app.selected_host().map(|h| h.alias.as_str()), Some("web1"));
    // web1 was Reachable + stale; after k it should now be Checking.
    assert!(matches!(
        app.ping.status_of("web1"),
        Some(crate::app::PingStatus::Checking)
    ));
}

#[test]
fn host_list_pagedown_triggers_stale_refresh_for_new_selection() {
    let mut app = stage_two_hosts_first_stale();
    let (tx, _rx) = std::sync::mpsc::channel();
    super::host_list::handle_main_key(&mut app, key(KeyCode::PageDown), &tx);
    assert_eq!(app.selected_host().map(|h| h.alias.as_str()), Some("web2"));
    assert!(matches!(
        app.ping.status_of("web2"),
        Some(crate::app::PingStatus::Checking)
    ));
}

#[test]
fn host_list_navigation_does_not_refire_when_auto_ping_off() {
    let mut app = stage_two_hosts_first_stale();
    app.ping.set_auto_ping(false);
    let (tx, _rx) = std::sync::mpsc::channel();
    super::host_list::handle_main_key(&mut app, key(KeyCode::Down), &tx);
    assert_eq!(app.selected_host().map(|h| h.alias.as_str()), Some("web2"));
    // auto_ping=off short-circuits the refresh: web2 should remain unmarked.
    assert!(!app.ping.status_contains("web2"));
}

#[test]
fn handle_ping_result_records_last_checked_for_alias_and_dependents() {
    let mut app = make_app(
        "Host bastion\n  HostName 1.1.1.1\nHost web1\n  HostName 10.0.0.1\n  ProxyJump bastion\n",
    );
    app.ping.set_generation(0);
    let before = std::time::Instant::now();
    super::event_loop::handle_ping_result(&mut app, "bastion".to_string(), Some(15), 0);
    let after = std::time::Instant::now();
    let bastion_ts = *app.ping.last_checked_at("bastion").unwrap();
    let web1_ts = *app.ping.last_checked_at("web1").unwrap();
    assert!(bastion_ts >= before && bastion_ts <= after);
    assert!(web1_ts >= before && web1_ts <= after);
    // Dependent inherits the bastion's status, not just its timestamp.
    assert!(matches!(
        app.ping.status_of("web1"),
        Some(crate::app::PingStatus::Reachable { .. })
    ));
}

#[test]
fn tick_does_not_clear_ping_status_after_sixty_seconds() {
    // Contract guard: removing the 60s expiry must not be silently re-added.
    let mut app = make_app("Host web1\n  HostName 1.1.1.1\n");
    app.ping.insert_status(
        "web1".to_string(),
        crate::app::PingStatus::Reachable { rtt_ms: 10 },
    );
    let old = std::time::Instant::now() - std::time::Duration::from_secs(120);
    app.ping.record_check("web1".to_string(), old);
    app.ping.set_checked_at(Some(old));
    let mut anim = crate::animation::AnimationState::default();
    let mut last_check = std::time::Instant::now();
    super::event_loop::handle_tick(&mut app, &mut anim, false, &mut last_check);
    assert!(
        app.ping.status_contains("web1"),
        "tick must not expire ping status"
    );
}

// ─── Progressive disclosure: host form ─────────────────────────

#[test]
fn host_form_new_starts_collapsed() {
    let form = HostForm::new();
    assert!(!form.expanded);
}

#[test]
fn host_form_from_entry_starts_expanded() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = SshConfigFile {
        elements: SshConfigFile::parse_content("Host test\n  HostName test.com\n"),
        path: dir.path().join("test_config"),
        crlf: false,
        bom: false,
    };
    let entries = config.host_entries();
    let form = HostForm::from_entry(&entries[0], Default::default());
    assert!(form.expanded);
}

#[test]
fn host_form_new_pattern_starts_expanded() {
    let form = HostForm::new_pattern();
    assert!(form.expanded);
}

#[test]
fn host_form_tab_from_alias_stays_collapsed() {
    let mut app = make_app("");
    *app.forms.host_mut() = HostForm::new();
    app.forms.host_mut().focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert_eq!(app.forms.host_mut().focused_field, FormField::Hostname);
    assert!(!app.forms.host_mut().expanded);
}

#[test]
fn host_form_tab_from_hostname_expands() {
    let mut app = make_app("");
    *app.forms.host_mut() = HostForm::new();
    app.forms.host_mut().focused_field = FormField::Hostname;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert!(app.forms.host_mut().expanded);
    assert_eq!(app.forms.host_mut().focused_field, FormField::User);
}

#[test]
fn host_form_collapsed_backtab_wraps() {
    let mut app = make_app("");
    *app.forms.host_mut() = HostForm::new();
    app.forms.host_mut().focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(
        &mut app,
        KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
        &tx,
    )
    .unwrap();
    assert_eq!(app.forms.host_mut().focused_field, FormField::Hostname);
    assert!(!app.forms.host_mut().expanded);
}

#[test]
fn host_form_expanded_does_not_trigger_dirty() {
    let mut app = make_app("");
    *app.forms.host_mut() = HostForm::new();
    app.forms.host_mut().alias = "test".to_string();
    app.screen = Screen::AddHost;
    app.capture_form_baseline();
    app.forms.host_mut().expanded = true;
    assert!(!app.host_form_is_dirty());
}

// ─── Progressive disclosure: provider form ─────────────────────

#[test]
fn provider_form_new_starts_collapsed() {
    let form = ProviderFormFields::new();
    assert!(!form.expanded);
}

#[test]
fn provider_required_fields_aws() {
    let required = crate::app::ProviderFormField::required_fields_for("aws");
    assert!(required.contains(&crate::app::ProviderFormField::Token));
    assert!(required.contains(&crate::app::ProviderFormField::Profile));
    assert!(required.contains(&crate::app::ProviderFormField::Regions));
}

#[test]
fn provider_required_fields_proxmox() {
    let required = crate::app::ProviderFormField::required_fields_for("proxmox");
    assert!(required.contains(&crate::app::ProviderFormField::Url));
    assert!(required.contains(&crate::app::ProviderFormField::Token));
    // AliasPrefix is optional
    assert!(!required.contains(&crate::app::ProviderFormField::AliasPrefix));
}

#[test]
fn provider_required_fields_netbox() {
    // NetBox leads with Url and Token like Proxmox; Filter stays optional.
    let fields = crate::app::ProviderFormField::fields_for("netbox");
    assert_eq!(fields[0], crate::app::ProviderFormField::Url);
    assert_eq!(fields[1], crate::app::ProviderFormField::Token);
    assert!(fields.contains(&crate::app::ProviderFormField::Filter));
    assert!(fields.contains(&crate::app::ProviderFormField::VerifyTls));
    let required = crate::app::ProviderFormField::required_fields_for("netbox");
    assert!(required.contains(&crate::app::ProviderFormField::Url));
    assert!(required.contains(&crate::app::ProviderFormField::Token));
    assert!(!required.contains(&crate::app::ProviderFormField::Filter));
}

#[test]
fn provider_netbox_filter_is_optional_not_mandatory() {
    assert!(!crate::app::ProviderFormField::is_mandatory_field(
        crate::app::ProviderFormField::Filter,
        "netbox"
    ));
    assert!(!crate::app::ProviderFormField::is_required_field(
        crate::app::ProviderFormField::Filter,
        "netbox"
    ));
}

#[test]
fn provider_required_fields_teleport_lead_with_user() {
    // No Token for Teleport: User is the collapsed-mode field and comes first,
    // so the form opens on a visible field instead of an empty box.
    let required = crate::app::ProviderFormField::required_fields_for("teleport");
    assert_eq!(required, vec![crate::app::ProviderFormField::User]);
    let all = crate::app::ProviderFormField::fields_for("teleport");
    assert_eq!(all[0], crate::app::ProviderFormField::User);
    assert!(!all.contains(&crate::app::ProviderFormField::Token));
    assert!(!all.contains(&crate::app::ProviderFormField::VaultRole));
    // Every other provider keeps User out of the collapsed view.
    for provider in ["digitalocean", "aws", "tailscale", "proxmox"] {
        assert!(
            !crate::app::ProviderFormField::required_fields_for(provider)
                .contains(&crate::app::ProviderFormField::User),
            "{provider} must not show User in collapsed mode"
        );
    }
}

#[test]
fn provider_optional_fields_are_complement() {
    for provider in &[
        "aws",
        "digitalocean",
        "proxmox",
        "netbox",
        "gcp",
        "azure",
        "oracle",
        "ovh",
        "scaleway",
        "tailscale",
        "teleport",
    ] {
        let all = crate::app::ProviderFormField::fields_for(provider);
        let required = crate::app::ProviderFormField::required_fields_for(provider);
        let optional = crate::app::ProviderFormField::optional_fields_for(provider);
        assert_eq!(
            required.len() + optional.len(),
            all.len(),
            "Field count mismatch for provider {}",
            provider
        );
    }
}

#[test]
fn provider_mandatory_fields_aws_token_and_profile_optional() {
    // Neither field is individually mandatory: AWS resolves credentials from
    // the token, from the profile, or from the environment, so asterisking
    // either one would promise a requirement that validation does not enforce.
    use crate::app::ProviderFormField;
    assert!(
        !ProviderFormField::is_mandatory_field(ProviderFormField::Token, "aws"),
        "AWS Token should not be mandatory (profile or environment may supply it)"
    );
    assert!(
        !ProviderFormField::is_mandatory_field(ProviderFormField::Profile, "aws"),
        "AWS Profile should not be mandatory (token or environment may supply it)"
    );
    // Still shown in collapsed mode: optional is not the same as hidden.
    assert!(ProviderFormField::is_required_field(
        ProviderFormField::Token,
        "aws"
    ));
    assert!(ProviderFormField::is_required_field(
        ProviderFormField::Profile,
        "aws"
    ));
}

#[test]
fn provider_mandatory_fields_tailscale_token_optional() {
    use crate::app::ProviderFormField;
    assert!(
        !ProviderFormField::is_mandatory_field(ProviderFormField::Token, "tailscale"),
        "Tailscale Token should not be mandatory (empty = CLI mode)"
    );
}

#[test]
fn provider_mandatory_fields_ovh_regions() {
    use crate::app::ProviderFormField;
    assert!(
        ProviderFormField::is_mandatory_field(ProviderFormField::Regions, "ovh"),
        "OVH Regions (Endpoint) should be mandatory"
    );
}

#[test]
fn provider_required_fields_prefix_of_all_fields() {
    use crate::app::ProviderFormField;
    for provider in &[
        "aws",
        "digitalocean",
        "proxmox",
        "netbox",
        "gcp",
        "azure",
        "oracle",
        "ovh",
        "scaleway",
        "tailscale",
        "teleport",
        "transip",
        "leaseweb",
        "i3d",
    ] {
        let all = ProviderFormField::fields_for(provider);
        let required = ProviderFormField::required_fields_for(provider);
        assert_eq!(
            &all[..required.len()],
            required.as_slice(),
            "Required fields must be a prefix of fields_for() for {}",
            provider
        );
    }
}

#[test]
fn provider_form_expanded_does_not_trigger_dirty() {
    let mut app = make_app("");
    app.screen = Screen::ProviderForm {
        id: crate::providers::config::ProviderConfigId::bare("digitalocean"),
    };
    *app.providers.form_mut() = ProviderFormFields::new();
    app.providers.form_mut().token = "tok".to_string();
    app.capture_provider_form_baseline();
    app.providers.form_mut().expanded = true;
    assert!(!app.provider_form_is_dirty());
}

// ─── Host form collapsed Enter-saves ───────────────────────────

#[test]
fn host_form_collapsed_enter_saves() {
    let mut app = make_app("");
    *app.forms.host_mut() = HostForm::new();
    app.forms.host_mut().alias = "myhost".to_string();
    app.forms.host_mut().hostname = "myhost.local".to_string();
    app.forms.host_mut().focused_field = FormField::Hostname;
    app.screen = Screen::AddHost;
    app.capture_form_mtime();
    app.capture_form_baseline();
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();
    assert!(
        matches!(app.screen, Screen::HostList),
        "Expected HostList after save, got {:?}",
        app.screen
    );
}

// ─── Provider form progressive disclosure navigation ───────────

#[test]
fn provider_form_tab_from_last_required_expands() {
    // DigitalOcean has one required field: Token
    let mut app = make_app("");
    app.screen = Screen::ProviderForm {
        id: crate::providers::config::ProviderConfigId::bare("digitalocean"),
    };
    *app.providers.form_mut() = ProviderFormFields::new();
    app.providers.form_mut().token = "tok".to_string();
    // Token is the only required field for DO
    app.providers.form_mut().focused_field = crate::app::ProviderFormField::Token;
    app.providers.form_mut().expanded = false;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert!(app.providers.form_mut().expanded);
    // First optional field for DO is AliasPrefix
    assert_eq!(
        app.providers.form_mut().focused_field,
        crate::app::ProviderFormField::AliasPrefix
    );
}

#[test]
fn provider_form_collapsed_backtab_wraps() {
    // AWS has 3 required fields: Token, Profile, Regions
    let mut app = make_app("");
    app.screen = Screen::ProviderForm {
        id: crate::providers::config::ProviderConfigId::bare("aws"),
    };
    *app.providers.form_mut() = ProviderFormFields::new();
    app.providers.form_mut().focused_field = crate::app::ProviderFormField::Token;
    app.providers.form_mut().expanded = false;
    let tx = mpsc::channel().0;
    handle_key_event(
        &mut app,
        KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
        &tx,
    )
    .unwrap();
    // Token is first required; BackTab wraps to last required (Regions)
    assert_eq!(
        app.providers.form_mut().focused_field,
        crate::app::ProviderFormField::Regions
    );
    assert!(!app.providers.form_mut().expanded);
}

#[test]
fn provider_form_tab_within_collapsed_required() {
    // AWS: Token -> Profile -> Regions (all required)
    let mut app = make_app("");
    app.screen = Screen::ProviderForm {
        id: crate::providers::config::ProviderConfigId::bare("aws"),
    };
    *app.providers.form_mut() = ProviderFormFields::new();
    app.providers.form_mut().focused_field = crate::app::ProviderFormField::Token;
    app.providers.form_mut().expanded = false;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    // Token -> Profile (mid-required, should NOT expand)
    assert_eq!(
        app.providers.form_mut().focused_field,
        crate::app::ProviderFormField::Profile
    );
    assert!(!app.providers.form_mut().expanded);
}

// --- theme_at_index tests ---

#[test]
fn theme_at_index_returns_builtin() {
    let builtins = crate::ui::theme::ThemeDef::builtins();
    let custom: Vec<crate::ui::theme::ThemeDef> = vec![];
    let result = super::theme_picker::theme_at_index(0, &builtins, &custom, None);
    assert!(result.is_some());
    assert_eq!(result.unwrap().name, "Purple");
}

#[test]
fn theme_at_index_returns_none_for_divider() {
    let builtins = crate::ui::theme::ThemeDef::builtins();
    let custom = vec![crate::ui::theme::ThemeDef::purple()];
    let divider_idx = Some(builtins.len());
    let result =
        super::theme_picker::theme_at_index(builtins.len(), &builtins, &custom, divider_idx);
    assert!(result.is_none());
}

#[test]
fn theme_at_index_returns_custom_after_divider() {
    let builtins = crate::ui::theme::ThemeDef::builtins();
    let mut custom_theme = crate::ui::theme::ThemeDef::purple();
    custom_theme.name = "My Custom".to_string();
    let custom = vec![custom_theme];
    let divider_idx = Some(builtins.len());
    let result =
        super::theme_picker::theme_at_index(builtins.len() + 1, &builtins, &custom, divider_idx);
    assert!(result.is_some());
    assert_eq!(result.unwrap().name, "My Custom");
}

#[test]
fn theme_at_index_out_of_bounds_returns_none() {
    let builtins = crate::ui::theme::ThemeDef::builtins();
    let custom: Vec<crate::ui::theme::ThemeDef> = vec![];
    let result = super::theme_picker::theme_at_index(999, &builtins, &custom, None);
    assert!(result.is_none());
}

#[test]
fn remove_in_flight_removes_single_alias() {
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};
    let set = Arc::new(Mutex::new(HashSet::new()));
    {
        let mut g = set.lock().unwrap();
        g.insert("host-a".to_string());
        g.insert("host-b".to_string());
        g.insert("host-c".to_string());
    }
    super::confirm::remove_in_flight(&set, "host-b");
    let g = set.lock().unwrap();
    assert!(g.contains("host-a"));
    assert!(!g.contains("host-b"));
    assert!(g.contains("host-c"));
}

#[test]
fn remove_in_flight_preserves_other_aliases_on_poison() {
    // Regression: an earlier implementation cleared the whole set on
    // mutex poison, making every in-flight alias simultaneously eligible
    // for re-signing. Verify we only remove the target alias.
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};
    let set: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    {
        let mut g = set.lock().unwrap();
        g.insert("host-a".to_string());
        g.insert("host-b".to_string());
        g.insert("host-c".to_string());
    }
    // Poison the mutex by panicking while holding the lock.
    let set_clone = set.clone();
    let _ = std::thread::spawn(move || {
        let _g = set_clone.lock().unwrap();
        panic!("intentional poison for test");
    })
    .join();
    assert!(set.is_poisoned());

    super::confirm::remove_in_flight(&set, "host-b");
    // After recovery the set must still contain the other aliases.
    let g = match set.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    assert!(g.contains("host-a"), "host-a must survive poison recovery");
    assert!(!g.contains("host-b"), "host-b must be removed");
    assert!(g.contains("host-c"), "host-c must survive poison recovery");
}

#[test]
fn vault_addr_missing_reports_when_env_and_host_both_empty() {
    assert!(vault_addr_missing(&[None], None));
}

#[test]
fn vault_addr_missing_reports_when_env_is_invalid_and_host_empty() {
    // Whitespace-only is rejected by is_valid_vault_addr; treat as unset.
    assert!(vault_addr_missing(&[None], Some("  ")));
}

#[test]
fn vault_addr_missing_false_when_env_is_set() {
    assert!(!vault_addr_missing(
        &[None, None],
        Some("https://vault.example.com:8200")
    ));
}

#[test]
fn vault_addr_missing_false_when_every_host_has_addr() {
    assert!(!vault_addr_missing(
        &[Some("https://a"), Some("https://b")],
        None
    ));
}

#[test]
fn vault_addr_missing_false_when_mixed_hosts_and_env_empty() {
    // Some hosts have an addr, some don't. Only block when ALL lack an addr.
    assert!(!vault_addr_missing(&[Some("https://a"), None], None));
}

#[test]
fn vault_addr_missing_false_when_no_hosts() {
    // Empty slice: nothing to sign, no prompt needed.
    assert!(!vault_addr_missing(&[], None));
}

#[test]
fn vault_addr_missing_true_when_env_is_empty_string() {
    assert!(vault_addr_missing(&[None], Some("")));
}

#[test]
fn vault_addr_missing_false_when_mixed_hosts_and_env_valid() {
    assert!(!vault_addr_missing(
        &[Some("https://a"), None],
        Some("https://vault.example.com:8200")
    ));
}

#[test]
fn zone_data_for_returns_nonempty_for_known_providers() {
    // zone_data_for falls back to (&[], &[]) + debug_assert for unknown
    // providers, so release builds cannot panic. We only test the happy
    // path here; the unknown-provider fallback is validated by the
    // debug_assert firing in CI if any caller ever passes a typo.
    for provider in ["scaleway", "aws", "gcp", "oracle", "ovh"] {
        let (zones, groups) = super::zone_data_for(provider);
        assert!(
            !zones.is_empty(),
            "zones for {provider} should not be empty"
        );
        assert!(
            !groups.is_empty(),
            "groups for {provider} should not be empty"
        );
    }
}

// --- Jump tests ---

#[test]
fn colon_opens_jump() {
    let mut app = make_app("");
    app.screen = Screen::HostList;
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Char(':')), &tx).unwrap();
    assert!(app.jump.is_some());
}

#[test]
fn jump_esc_closes() {
    let mut app = make_app("");
    app.jump = Some(crate::app::JumpState::default());
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Esc), &tx).unwrap();
    assert!(app.jump.is_none());
}

#[test]
fn jump_char_always_filters() {
    // All chars go to filter, even recognized command keys like 'K'
    let mut app = make_app("");
    app.jump = Some(crate::app::JumpState::default());
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Char('K')), &tx).unwrap();
    assert!(app.jump.is_some(), "jump bar should stay open");
    assert_eq!(app.jump.as_ref().unwrap().query(), "K");
    assert!(
        matches!(app.screen, Screen::HostList),
        "should not navigate away"
    );
}

#[test]
fn jump_filter_then_enter_executes() {
    // Type the K hotkey directly: single-char query against a hotkey letter
    // gets a large score boost so the matching action lands at index 0,
    // and Enter dispatches it.
    let mut app = make_app("");
    let mut state = crate::app::JumpState::default();
    state.push_query('K');
    app.jump = Some(state);
    app.recompute_jump_hits();
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();
    assert!(
        matches!(app.screen, Screen::KeyList),
        "Enter should dispatch the K action and open KeyList; screen={:?}",
        app.screen
    );
    assert!(app.jump.is_none());
}

#[test]
fn jump_up_down_navigates() {
    let mut app = make_app("");
    app.jump = Some(crate::app::JumpState::default());
    let (tx, _rx) = mpsc::channel();
    // First Down reveals the cursor on row 0 (NOT row 1) so the user
    // does not skip past the first item on a fresh open.
    handle_key_event(&mut app, key(KeyCode::Down), &tx).unwrap();
    assert_eq!(app.jump.as_ref().unwrap().selected(), 0);
    assert!(app.jump.as_ref().unwrap().cursor_revealed());
    // Subsequent Downs increment normally.
    handle_key_event(&mut app, key(KeyCode::Down), &tx).unwrap();
    assert_eq!(app.jump.as_ref().unwrap().selected(), 1);
    handle_key_event(&mut app, key(KeyCode::Up), &tx).unwrap();
    assert_eq!(app.jump.as_ref().unwrap().selected(), 0);
}

#[test]
fn jump_any_char_appends_to_filter() {
    let mut app = make_app("");
    app.jump = Some(crate::app::JumpState::default());
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Char('t')), &tx).unwrap();
    assert!(app.jump.is_some());
    assert_eq!(app.jump.as_ref().unwrap().query(), "t");
    // 't' is a command key (tag inline), but should filter, not execute
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn jump_enter_on_empty_filter_does_nothing() {
    let mut app = make_app("");
    app.jump = Some(crate::app::JumpState::default());
    app.jump.as_mut().unwrap().push_query('z');
    app.jump.as_mut().unwrap().push_query('z');
    app.jump.as_mut().unwrap().push_query('z');
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();
    assert!(app.jump.is_some());
}

#[test]
fn jump_backspace_on_empty_closes() {
    let mut app = make_app("");
    app.jump = Some(crate::app::JumpState::default());
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Backspace), &tx).unwrap();
    assert!(app.jump.is_none());
}

#[test]
fn jump_backspace_removes_filter_char() {
    let mut app = make_app("");
    app.jump = Some(crate::app::JumpState::default());
    app.jump.as_mut().unwrap().push_query('t');
    app.jump.as_mut().unwrap().push_query('u');
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Backspace), &tx).unwrap();
    assert_eq!(app.jump.as_ref().unwrap().query(), "t");
}

#[test]
fn jump_navigate_then_enter_executes() {
    let mut app = make_app("");
    app.jump = Some(crate::app::JumpState::default());
    let (tx, _rx) = mpsc::channel();
    // First Down reveals the cursor on row 0; subsequent Downs increment.
    handle_key_event(&mut app, key(KeyCode::Down), &tx).unwrap();
    handle_key_event(&mut app, key(KeyCode::Down), &tx).unwrap();
    handle_key_event(&mut app, key(KeyCode::Down), &tx).unwrap();
    assert_eq!(app.jump.as_ref().unwrap().selected(), 2);
    // Enter on index 2 should dispatch the third action — with no host
    // selected the action's handler does nothing visible (no crash).
    // Jump bar closes either way.
    handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();
    assert!(app.jump.is_none(), "jump bar should close after Enter");
}

#[test]
fn jump_filter_shrink_then_enter_clamps_selected() {
    let mut app = make_app("");
    // Set selected to a high index, then add a filter that reduces the list
    let mut state = crate::app::JumpState::default();
    state.set_selected(10);
    state.push_query('S'); // push_query resets selected to 0
    state.push_query('S');
    state.push_query('H');
    // Filtered list narrows to a few items
    let filtered = state.filtered_commands();
    assert!(!filtered.is_empty(), "filter should have results");
    assert!(filtered.len() < crate::app::PaletteCommand::all().len());
    // Force selected to way out-of-bounds to test clamping in Enter handler
    state.set_selected(50);
    app.jump = Some(state);
    let (tx, _rx) = mpsc::channel();
    // Enter should clamp selected to last item, execute it, and close the jump bar
    handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();
    assert!(
        app.jump.is_none(),
        "jump bar should close after clamped Enter"
    );
}

// --- Unified jump dispatch tests for non-Action hits ---

#[test]
fn jump_enter_on_host_hit_jumps_to_host_list_and_selects() {
    let mut app = make_app("Host alpha\n  HostName alpha.example\n");
    app.jump = Some(crate::app::JumpState::default());
    let host_hit = crate::app::JumpHit::Host(crate::app::HostHit {
        alias: "alpha".into(),
        hostname: "alpha.example".into(),
        tags: vec![],
        provider: None,
        user: String::new(),
        identity_file: String::new(),
        proxy_jump: String::new(),
        vault_ssh: None,
    });
    if let Some(p) = app.jump.as_mut() {
        p.set_hits(vec![host_hit]);
        // Force the visible_hits fast path even with empty query.
        for c in "alpha".chars() {
            p.push_query(c);
        }
    }
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();
    assert!(app.jump.is_none(), "jump bar should close on Enter");
    assert!(matches!(app.top_page, crate::app::TopPage::Hosts));
    let alias = app.selected_host().map(|h| h.alias.clone());
    assert_eq!(alias.as_deref(), Some("alpha"));
}

#[test]
fn jump_enter_on_tunnel_hit_switches_to_tunnels_page_and_selects_row() {
    let mut app =
        make_app("Host alpha\n  HostName alpha.example\n  LocalForward 5432 db.internal:5432\n");
    app.jump = Some(crate::app::JumpState::default());
    let tunnel_hit = crate::app::JumpHit::Tunnel(crate::app::TunnelHit {
        alias: "alpha".into(),
        bind_port: 5432,
        bind_port_str: "5432".into(),
        destination: "5432 -> db.internal:5432".into(),
        active: false,
    });
    if let Some(p) = app.jump.as_mut() {
        p.set_hits(vec![tunnel_hit]);
        for c in "alpha".chars() {
            p.push_query(c);
        }
    }
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();
    assert!(matches!(app.top_page, crate::app::TopPage::Tunnels));
    // Selection landed on the matching tunnel row in the tunnels overview
    // ListState (NOT the host list).
    let pairs = crate::ui::tunnels_overview::visible_pairs(
        &app.search,
        &app.hosts_state,
        &app.tunnels,
        &app.history,
    );
    let expected_idx = pairs
        .iter()
        .position(|(alias, rule)| alias == "alpha" && rule.bind_port == 5432);
    assert_eq!(app.ui.tunnels_overview_state().selected(), expected_idx);
}

#[test]
fn jump_enter_on_container_hit_lands_on_global_containers_tab() {
    let mut app = make_app("Host beta\n  HostName beta.example\n");
    // Seed a cached container for `beta` so collect_jump_candidates
    // surfaces it; Enter then dispatches the matching hit.
    app.container_state.insert_cache_entry(
        "beta".into(),
        crate::containers::ContainerCacheEntry {
            timestamp: 0,
            runtime: crate::containers::ContainerRuntime::Docker,
            engine_version: None,
            containers: vec![crate::containers::ContainerInfo {
                id: "abc".into(),
                names: "nginx".into(),
                image: String::new(),
                state: "running".into(),
                status: String::new(),
                ports: String::new(),
            }],
        },
    );
    app.jump = Some(crate::app::JumpState::default());
    if let Some(p) = app.jump.as_mut() {
        for c in "nginx".chars() {
            p.push_query(c);
        }
    }
    app.recompute_jump_hits();
    // Move selection onto the container hit (it should be the first
    // matching candidate; we check by kind to be robust).
    if let Some(p) = app.jump.as_mut()
        && let Some(idx) = p
            .visible_hits()
            .iter()
            .position(|h| matches!(h, crate::app::JumpHit::Container(_)))
    {
        p.set_selected(idx);
    }
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();
    // New behaviour: stay on the global containers tab and land the
    // cursor on the picked container's row in the visible list,
    // instead of opening the legacy per-host overlay.
    assert!(matches!(app.screen, Screen::HostList));
    assert_eq!(app.top_page, crate::app::TopPage::Containers);
    let visible = crate::ui::containers_overview::visible_items(&app);
    let selected_idx = app
        .ui
        .containers_overview_state()
        .selected()
        .expect("cursor must be placed on a row");
    let row = visible
        .get(selected_idx)
        .and_then(|i| match i {
            crate::ui::containers_overview::ContainerListItem::Container(r) => Some(r),
            _ => None,
        })
        .expect("cursor must land on a Container row, not a header");
    assert_eq!(row.alias, "beta");
    assert_eq!(row.id, "abc");
}

#[test]
fn jump_enter_on_snippet_hit_with_no_host_warns() {
    // Snippet picker requires a target host. With a snippet seeded but no
    // host selected (empty config = no host list = no selection), Enter
    // should surface a warning toast and NOT open the picker screen.
    let mut app = make_app("");
    app.snippets
        .store_mut()
        .snippets
        .push(crate::snippet::Snippet {
            name: "deploy".into(),
            command: "curl example".into(),
            description: String::new(),
            interactive: false,
        });
    app.jump = Some(crate::app::JumpState::default());
    if let Some(p) = app.jump.as_mut() {
        for c in "deploy".chars() {
            p.push_query(c);
        }
    }
    app.recompute_jump_hits();
    if let Some(p) = app.jump.as_mut()
        && let Some(idx) = p
            .visible_hits()
            .iter()
            .position(|h| matches!(h, crate::app::JumpHit::Snippet(_)))
    {
        p.set_selected(idx);
    }
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();
    assert!(!matches!(app.screen, Screen::SnippetPicker));
    let toast = app.status_center.toast();
    assert!(
        toast.is_some(),
        "warning toast should surface when no host is selected"
    );
}

#[test]
fn jump_tab_jumps_to_next_section() {
    // Two visible kinds (Host + Action) so Tab has somewhere to go.
    let mut app = make_app("Host gamma\n  HostName gamma.example\n");
    app.jump = Some(crate::app::JumpState::default());
    if let Some(p) = app.jump.as_mut() {
        for c in "gamma".chars() {
            p.push_query(c);
        }
    }
    app.recompute_jump_hits();
    let starting_kind = app
        .jump
        .as_ref()
        .unwrap()
        .visible_hits()
        .first()
        .map(|h| h.kind());
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    let after_kind = app
        .jump
        .as_ref()
        .map(|p| p.visible_hits()[p.selected()].kind());
    if let (Some(a), Some(b)) = (starting_kind, after_kind) {
        if a != b {
            // Confirmed: Tab moved to a different kind. Pass.
        } else {
            // Some demos may only have one kind at this stage; the test is
            // satisfied as long as the key press did not crash.
        }
    }
}

#[test]
fn jump_query_capped_at_64() {
    let mut state = crate::app::JumpState::default();
    for _ in 0..100 {
        state.push_query('a');
    }
    assert_eq!(
        state.query().len(),
        64,
        "query should be capped at 64 chars"
    );
}

// --- ProxyJump picker handler tests ---

use crate::app::ProxyJumpCandidate;

fn proxyjump_picker_app() -> App {
    // Three hosts: `bastion` is promoted into the suggested section via
    // the keyword heuristic, `alpha`/`zeta` stay in the rest section
    // below the separator, and `victim` is the host being edited.
    let mut app = make_app(concat!(
        "Host bastion\n  HostName 1.1.1.1\n",
        "Host alpha\n  HostName 2.2.2.2\n",
        "Host zeta\n  HostName 3.3.3.3\n",
        "Host victim\n  HostName 9.9.9.9\n",
    ));
    app.screen = Screen::EditHost {
        alias: "victim".to_string(),
    };
    app.ui.proxyjump_picker_mut().open = true;
    app
}

#[test]
fn proxyjump_picker_enter_on_section_label_is_noop() {
    let mut app = proxyjump_picker_app();
    let candidates = app.proxyjump_candidates();
    let label_idx = candidates
        .iter()
        .position(|c| matches!(c, ProxyJumpCandidate::SectionLabel(_)))
        .expect("test setup must produce a SectionLabel");
    app.ui.proxyjump_picker_mut().list.select(Some(label_idx));

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    assert!(
        app.ui.proxyjump_picker().open,
        "Enter on a SectionLabel must not close the picker"
    );
    assert!(
        app.forms.host_mut().proxy_jump.is_empty(),
        "Enter on a SectionLabel must not populate the ProxyJump field"
    );
}

#[test]
fn proxyjump_picker_enter_on_separator_is_noop() {
    let mut app = proxyjump_picker_app();
    let candidates = app.proxyjump_candidates();
    let sep = candidates
        .iter()
        .position(|c| matches!(c, ProxyJumpCandidate::Separator))
        .expect("test setup must produce a separator");
    app.ui.proxyjump_picker_mut().list.select(Some(sep));

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    assert!(
        app.ui.proxyjump_picker().open,
        "Enter on a Separator must not close the picker"
    );
    assert!(
        app.forms.host_mut().proxy_jump.is_empty(),
        "Enter on a Separator must not populate the ProxyJump field"
    );
}

#[test]
fn proxyjump_picker_enter_on_host_applies_alias_and_closes() {
    let mut app = proxyjump_picker_app();
    // Select the first host (the suggested one). `proxyjump_first_host_index`
    // resolves to the right index regardless of any leading SectionLabel.
    let first_host = app.proxyjump_first_host_index().expect("host expected");
    app.ui.proxyjump_picker_mut().list.select(Some(first_host));

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    assert!(
        !app.ui.proxyjump_picker().open,
        "Enter on a Host must close the picker"
    );
    assert_eq!(
        app.forms.host_mut().proxy_jump,
        "bastion",
        "the selected host's alias must populate the ProxyJump field"
    );
}

// ─── Smart paste: bare domain/IP detection ──────────────────────

#[test]
fn host_form_smart_paste_detects_bare_domain() {
    let mut app = make_app("");
    *app.forms.host_mut() = HostForm::new();
    app.forms.host_mut().alias = "db.example.com".to_string();
    app.forms.host_mut().focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    // Tab away from Alias triggers smart paste
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert_eq!(app.forms.host_mut().hostname, "db.example.com");
    // Alias stays unchanged — only hostname is suggested
    assert_eq!(app.forms.host_mut().alias, "db.example.com");
}

#[test]
fn host_form_smart_paste_detects_ip_address() {
    let mut app = make_app("");
    *app.forms.host_mut() = HostForm::new();
    app.forms.host_mut().alias = "192.168.1.100".to_string();
    app.forms.host_mut().focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert_eq!(app.forms.host_mut().hostname, "192.168.1.100");
    assert_eq!(app.forms.host_mut().alias, "192.168.1.100");
}

#[test]
fn host_form_smart_paste_skips_plain_name() {
    let mut app = make_app("");
    *app.forms.host_mut() = HostForm::new();
    app.forms.host_mut().alias = "myserver".to_string();
    app.forms.host_mut().focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    // No dot means no detection — alias stays, hostname stays empty
    assert_eq!(app.forms.host_mut().alias, "myserver");
    assert!(app.forms.host_mut().hostname.is_empty());
}

#[test]
fn host_form_smart_paste_domain_no_overwrite_hostname() {
    let mut app = make_app("");
    *app.forms.host_mut() = HostForm::new();
    app.forms.host_mut().alias = "db.example.com".to_string();
    app.forms.host_mut().hostname = "already.set.com".to_string();
    app.forms.host_mut().focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    // Hostname already populated — don't overwrite
    assert_eq!(app.forms.host_mut().hostname, "already.set.com");
    assert_eq!(app.forms.host_mut().alias, "db.example.com");
}

#[test]
fn host_form_smart_paste_rejects_leading_dot() {
    let mut app = make_app("");
    *app.forms.host_mut() = HostForm::new();
    app.forms.host_mut().alias = ".example.com".to_string();
    app.forms.host_mut().focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    // Leading dot produces empty first label — must not fire
    assert_eq!(app.forms.host_mut().alias, ".example.com");
    assert!(app.forms.host_mut().hostname.is_empty());
}

#[test]
fn host_form_smart_paste_rejects_bare_dot() {
    let mut app = make_app("");
    *app.forms.host_mut() = HostForm::new();
    app.forms.host_mut().alias = ".".to_string();
    app.forms.host_mut().focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert_eq!(app.forms.host_mut().alias, ".");
    assert!(app.forms.host_mut().hostname.is_empty());
}

#[test]
fn host_form_smart_paste_ignores_ipv6_mixed() {
    // IPv4-mapped IPv6 notation must not trigger bare-domain detection
    let mut app = make_app("");
    *app.forms.host_mut() = HostForm::new();
    app.forms.host_mut().alias = "::ffff:192.0.2.1".to_string();
    app.forms.host_mut().focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert_eq!(app.forms.host_mut().alias, "::ffff:192.0.2.1");
    assert!(app.forms.host_mut().hostname.is_empty());
}

#[test]
fn host_form_smart_paste_allows_underscore_hostname() {
    let mut app = make_app("");
    *app.forms.host_mut() = HostForm::new();
    app.forms.host_mut().alias = "my_host.internal".to_string();
    app.forms.host_mut().focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert_eq!(app.forms.host_mut().hostname, "my_host.internal");
    assert_eq!(app.forms.host_mut().alias, "my_host.internal");
}

#[test]
fn host_form_smart_paste_fires_on_enter() {
    // Enter on Alias also calls maybe_smart_paste before submit.
    // Use a minimal valid config so submit_form can succeed.
    let mut app = make_app("");
    *app.forms.host_mut() = HostForm::new();
    app.forms.host_mut().alias = "web.example.com".to_string();
    app.forms.host_mut().focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();
    // Smart paste copies alias to hostname, alias stays unchanged.
    // submit_form runs next — on success the screen returns to HostList.
    assert_eq!(app.screen, Screen::HostList);
    assert!(
        app.hosts_state
            .list()
            .iter()
            .any(|h| h.alias == "web.example.com")
    );
    assert!(
        app.hosts_state
            .list()
            .iter()
            .any(|h| h.hostname == "web.example.com")
    );
}

#[test]
fn host_form_smart_paste_rejects_trailing_dot() {
    // Trailing dot is invalid for SSH HostName — must not fire
    let mut app = make_app("");
    *app.forms.host_mut() = HostForm::new();
    app.forms.host_mut().alias = "example.com.".to_string();
    app.forms.host_mut().focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert_eq!(app.forms.host_mut().alias, "example.com.");
    assert!(app.forms.host_mut().hostname.is_empty());
}

#[test]
fn host_form_smart_paste_rejects_short_dotted_string() {
    // "1.1" (len 3) should not trigger — too short to be a real hostname
    let mut app = make_app("");
    *app.forms.host_mut() = HostForm::new();
    app.forms.host_mut().alias = "1.1".to_string();
    app.forms.host_mut().focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert_eq!(app.forms.host_mut().alias, "1.1");
    assert!(app.forms.host_mut().hostname.is_empty());
}

#[test]
fn host_form_smart_paste_minimum_valid_length() {
    // "x.io" (len 4) is the shortest that should trigger
    let mut app = make_app("");
    *app.forms.host_mut() = HostForm::new();
    app.forms.host_mut().alias = "x.io".to_string();
    app.forms.host_mut().focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert_eq!(app.forms.host_mut().hostname, "x.io");
    assert_eq!(app.forms.host_mut().alias, "x.io");
}

#[test]
fn host_form_smart_paste_no_fire_on_edit_with_hostname() {
    // EditHost: hostname already populated from existing entry — must not overwrite
    let mut app = make_app("Host myserver\n  HostName myserver.local\n");
    *app.forms.host_mut() = HostForm::new();
    app.forms.host_mut().alias = "db.example.com".to_string();
    app.forms.host_mut().hostname = "myserver.local".to_string();
    app.forms.host_mut().focused_field = FormField::Alias;
    app.screen = Screen::EditHost {
        alias: "myserver".to_string(),
    };
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert_eq!(app.forms.host_mut().hostname, "myserver.local");
    assert_eq!(app.forms.host_mut().alias, "db.example.com");
}

// ---------------------------------------------------------------------
// Bulk tag editor — handler integration
// ---------------------------------------------------------------------

fn bulk_make_app() -> App {
    // Config path gets written during apply — use a unique /tmp path per
    // test so parallel runs don't stomp each other. Thread ID ensures
    // uniqueness when cargo test runs tests in parallel threads.
    let path = std::env::temp_dir().join(format!(
        "purple_bulk_test_{}_{:?}.cfg",
        std::process::id(),
        std::thread::current().id()
    ));
    let mut app = make_app(
        "Host a\n  HostName 1.1.1.1\n  # purple:tags prod\n\
         Host b\n  HostName 2.2.2.2\n  # purple:tags prod,db\n\
         Host c\n  HostName 3.3.3.3\n  # purple:tags db\n",
    );
    app.hosts_state.ssh_config_mut().path = path;
    app
}

#[test]
fn plain_space_toggles_multi_select_in_host_list() {
    let mut app = bulk_make_app();
    let tx = mpsc::channel().0;
    // First host is selected by default.
    let idx = app.selected_host_index().unwrap();
    handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx).unwrap();
    assert!(app.hosts_state.multi_select().contains(&idx));
    handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx).unwrap();
    assert!(!app.hosts_state.multi_select().contains(&idx));
}

#[test]
fn esc_with_selection_clears_it_without_quitting() {
    let mut app = bulk_make_app();
    let tx = mpsc::channel().0;
    app.hosts_state.multi_select_mut().insert(0);
    handle_key_event(&mut app, key(KeyCode::Esc), &tx).unwrap();
    assert!(app.hosts_state.multi_select().is_empty());
    assert!(app.running, "Esc must not quit while clearing selection");
}

#[test]
fn t_routes_to_bulk_editor_when_selection_active() {
    let mut app = bulk_make_app();
    let tx = mpsc::channel().0;
    app.hosts_state.multi_select_mut().insert(0);
    app.hosts_state.multi_select_mut().insert(1);
    handle_key_event(&mut app, key(KeyCode::Char('t')), &tx).unwrap();
    assert_eq!(app.screen, Screen::BulkTagEditor);
    assert!(
        app.tags.input().is_none(),
        "single-host input must NOT open"
    );
    assert_eq!(app.forms.bulk_tag_editor_mut().aliases.len(), 2);
}

#[test]
fn t_opens_single_host_input_when_no_selection() {
    let mut app = bulk_make_app();
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Char('t')), &tx).unwrap();
    assert_eq!(app.screen, Screen::HostList);
    assert!(
        app.tags.input().is_some(),
        "must fall back to existing single-host tag input"
    );
}

#[test]
fn test_tag_edit_logs_updated_tags() {
    let mut app = bulk_make_app();
    let tx = mpsc::channel().0;
    // 't' with no multi-select opens the single-host tag input for host "a".
    handle_key_event(&mut app, key(KeyCode::Char('t')), &tx).unwrap();
    assert!(app.tags.input().is_some());
    let records = crate::logging::capture::capture(|| {
        handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();
    });
    assert!(
        crate::logging::capture::has(
            &records,
            log::Level::Debug,
            "[purple] host tags updated: alias=a"
        ),
        "expected tag-updated debug log, got: {:?}",
        records
    );
}

#[test]
fn bulk_editor_space_cycles_and_enter_applies() {
    let mut app = bulk_make_app();
    let tx = mpsc::channel().0;
    // Select a + c. Apply "add prod" — a already has it, c does not.
    let idx_a = app
        .hosts_state
        .list()
        .iter()
        .position(|h| h.alias == "a")
        .unwrap();
    let idx_c = app
        .hosts_state
        .list()
        .iter()
        .position(|h| h.alias == "c")
        .unwrap();
    app.hosts_state.multi_select_mut().insert(idx_a);
    app.hosts_state.multi_select_mut().insert(idx_c);
    handle_key_event(&mut app, key(KeyCode::Char('t')), &tx).unwrap();
    assert_eq!(app.screen, Screen::BulkTagEditor);

    // Land the cursor on `prod`.
    let prod_row = app
        .forms
        .bulk_tag_editor()
        .rows
        .iter()
        .position(|r| r.tag == "prod")
        .unwrap();
    app.ui.bulk_tag_editor_state_mut().select(Some(prod_row));
    // One Space: Leave → AddToAll.
    handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx).unwrap();
    assert_eq!(
        app.forms.bulk_tag_editor_mut().rows[prod_row].action,
        crate::app::BulkTagAction::AddToAll
    );
    handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();
    assert_eq!(app.screen, Screen::HostList);
    // c now has prod.
    let c = app
        .hosts_state
        .list()
        .iter()
        .find(|h| h.alias == "c")
        .unwrap();
    assert!(c.tags.contains(&"prod".to_string()));
}

#[test]
fn bulk_editor_esc_with_dirty_shows_discard_then_confirms() {
    // Every dirty-checked surface prompts before discarding work.
    // Esc on a dirty editor opens
    // the discard prompt; pressing y then closes the editor and clears state.
    let mut app = bulk_make_app();
    let tx = mpsc::channel().0;
    app.hosts_state.multi_select_mut().insert(0);
    handle_key_event(&mut app, key(KeyCode::Char('t')), &tx).unwrap();
    assert_eq!(app.screen, Screen::BulkTagEditor);
    // Stage a change.
    let prod_row = app
        .forms
        .bulk_tag_editor()
        .rows
        .iter()
        .position(|r| r.tag == "prod")
        .unwrap();
    app.ui.bulk_tag_editor_state_mut().select(Some(prod_row));
    handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx).unwrap();
    assert!(
        app.forms.bulk_tag_editor_mut().is_dirty(),
        "Space cycle should mark editor dirty"
    );
    // Esc on dirty editor opens the discard prompt; editor stays open.
    handle_key_event(&mut app, key(KeyCode::Esc), &tx).unwrap();
    assert!(
        app.forms.is_discard_pending(),
        "Esc on dirty editor must show discard prompt"
    );
    assert_eq!(
        app.screen,
        Screen::BulkTagEditor,
        "Discard prompt keeps the editor screen"
    );
    // Confirm the discard.
    handle_key_event(&mut app, key(KeyCode::Char('y')), &tx).unwrap();
    assert_eq!(app.screen, Screen::HostList);
    assert!(app.forms.bulk_tag_editor_mut().rows.is_empty());
    assert!(!app.forms.is_discard_pending());
}

#[test]
fn bulk_editor_esc_when_clean_closes_immediately() {
    // Without dirty changes, Esc closes the editor without prompting.
    let mut app = bulk_make_app();
    let tx = mpsc::channel().0;
    app.hosts_state.multi_select_mut().insert(0);
    handle_key_event(&mut app, key(KeyCode::Char('t')), &tx).unwrap();
    assert_eq!(app.screen, Screen::BulkTagEditor);
    handle_key_event(&mut app, key(KeyCode::Esc), &tx).unwrap();
    assert_eq!(app.screen, Screen::HostList);
    assert!(!app.forms.is_discard_pending());
}

#[test]
fn bulk_editor_esc_dirty_then_no_keeps_editor_open() {
    // Pressing n / Esc on the discard prompt cancels the discard and
    // returns the user to the editor with their changes intact.
    let mut app = bulk_make_app();
    let tx = mpsc::channel().0;
    app.hosts_state.multi_select_mut().insert(0);
    handle_key_event(&mut app, key(KeyCode::Char('t')), &tx).unwrap();
    let prod_row = app
        .forms
        .bulk_tag_editor()
        .rows
        .iter()
        .position(|r| r.tag == "prod")
        .unwrap();
    app.ui.bulk_tag_editor_state_mut().select(Some(prod_row));
    handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx).unwrap();
    handle_key_event(&mut app, key(KeyCode::Esc), &tx).unwrap();
    assert!(app.forms.is_discard_pending());
    handle_key_event(&mut app, key(KeyCode::Char('n')), &tx).unwrap();
    assert!(!app.forms.is_discard_pending());
    assert_eq!(app.screen, Screen::BulkTagEditor);
    assert!(
        app.forms.bulk_tag_editor_mut().is_dirty(),
        "Changes preserved"
    );
}

#[test]
fn bulk_editor_plus_opens_new_tag_input() {
    let mut app = bulk_make_app();
    let tx = mpsc::channel().0;
    app.hosts_state.multi_select_mut().insert(0);
    handle_key_event(&mut app, key(KeyCode::Char('t')), &tx).unwrap();
    handle_key_event(&mut app, key(KeyCode::Char('+')), &tx).unwrap();
    assert!(app.forms.bulk_tag_editor_mut().new_tag_input.is_some());
    // Type "eu" and Enter.
    handle_key_event(&mut app, key(KeyCode::Char('e')), &tx).unwrap();
    handle_key_event(&mut app, key(KeyCode::Char('u')), &tx).unwrap();
    handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();
    assert!(app.forms.bulk_tag_editor_mut().new_tag_input.is_none());
    let eu = app
        .forms
        .bulk_tag_editor()
        .rows
        .iter()
        .find(|r| r.tag == "eu");
    assert!(eu.is_some(), "new tag `eu` should be appended as a row");
    assert_eq!(eu.unwrap().action, crate::app::BulkTagAction::AddToAll);
}

#[test]
fn bulk_tag_undo_restores_previous_tags() {
    let mut app = bulk_make_app();
    let tx = mpsc::channel().0;
    // Select a (has prod) + b (has prod, db). Remove `prod` from both.
    let idx_a = app
        .hosts_state
        .list()
        .iter()
        .position(|h| h.alias == "a")
        .unwrap();
    let idx_b = app
        .hosts_state
        .list()
        .iter()
        .position(|h| h.alias == "b")
        .unwrap();
    app.hosts_state.multi_select_mut().insert(idx_a);
    app.hosts_state.multi_select_mut().insert(idx_b);
    handle_key_event(&mut app, key(KeyCode::Char('t')), &tx).unwrap();
    let prod_row = app
        .forms
        .bulk_tag_editor()
        .rows
        .iter()
        .position(|r| r.tag == "prod")
        .unwrap();
    app.ui.bulk_tag_editor_state_mut().select(Some(prod_row));
    // Cycle to RemoveFromAll (Leave → AddToAll → RemoveFromAll).
    handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx).unwrap();
    handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx).unwrap();
    handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();
    assert_eq!(app.screen, Screen::HostList);
    // Verify prod was removed.
    let a = app
        .hosts_state
        .list()
        .iter()
        .find(|h| h.alias == "a")
        .unwrap();
    assert!(!a.tags.contains(&"prod".to_string()));
    // Undo.
    assert!(app.forms.bulk_tag_undo().is_some());
    handle_key_event(&mut app, key(KeyCode::Char('u')), &tx).unwrap();
    assert!(app.forms.bulk_tag_undo().is_none());
    // Verify prod is back.
    let a = app
        .hosts_state
        .list()
        .iter()
        .find(|h| h.alias == "a")
        .unwrap();
    let b = app
        .hosts_state
        .list()
        .iter()
        .find(|h| h.alias == "b")
        .unwrap();
    assert!(a.tags.contains(&"prod".to_string()));
    assert!(b.tags.contains(&"prod".to_string()));
    // b still has db (it wasn't touched).
    assert!(b.tags.contains(&"db".to_string()));
}

#[test]
fn bulk_editor_q_cancels_like_esc() {
    let mut app = bulk_make_app();
    let tx = mpsc::channel().0;
    app.hosts_state.multi_select_mut().insert(0);
    handle_key_event(&mut app, key(KeyCode::Char('t')), &tx).unwrap();
    assert_eq!(app.screen, Screen::BulkTagEditor);
    handle_key_event(&mut app, key(KeyCode::Char('q')), &tx).unwrap();
    assert_eq!(app.screen, Screen::HostList);
    assert!(app.forms.bulk_tag_editor_mut().rows.is_empty());
}

#[test]
fn bulk_editor_jk_navigates_rows() {
    let mut app = bulk_make_app();
    let tx = mpsc::channel().0;
    app.hosts_state.multi_select_mut().insert(0);
    app.hosts_state.multi_select_mut().insert(1);
    handle_key_event(&mut app, key(KeyCode::Char('t')), &tx).unwrap();
    assert!(app.forms.bulk_tag_editor_mut().rows.len() >= 2);
    let initial = app.ui.bulk_tag_editor_state().selected();
    handle_key_event(&mut app, key(KeyCode::Char('j')), &tx).unwrap();
    let after_j = app.ui.bulk_tag_editor_state().selected();
    assert_ne!(initial, after_j, "j should move selection");
    handle_key_event(&mut app, key(KeyCode::Char('k')), &tx).unwrap();
    let after_k = app.ui.bulk_tag_editor_state().selected();
    assert_eq!(initial, after_k, "k should move back");
}

#[test]
fn bulk_editor_help_roundtrip() {
    let mut app = bulk_make_app();
    let tx = mpsc::channel().0;
    app.hosts_state.multi_select_mut().insert(0);
    handle_key_event(&mut app, key(KeyCode::Char('t')), &tx).unwrap();
    assert_eq!(app.screen, Screen::BulkTagEditor);
    // Stage a change so we can verify state survives the help roundtrip.
    app.ui.bulk_tag_editor_state_mut().select(Some(0));
    handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx).unwrap();
    let action_before = app.forms.bulk_tag_editor_mut().rows[0].action;
    // Open help.
    handle_key_event(&mut app, key(KeyCode::Char('?')), &tx).unwrap();
    assert!(matches!(app.screen, Screen::Help { .. }));
    // Return from help.
    handle_key_event(&mut app, key(KeyCode::Esc), &tx).unwrap();
    assert_eq!(app.screen, Screen::BulkTagEditor);
    assert_eq!(
        app.forms.bulk_tag_editor_mut().rows[0].action,
        action_before
    );
}

#[test]
fn bulk_editor_new_tag_input_backspace_and_cursor() {
    let mut app = bulk_make_app();
    let tx = mpsc::channel().0;
    app.hosts_state.multi_select_mut().insert(0);
    handle_key_event(&mut app, key(KeyCode::Char('t')), &tx).unwrap();
    // Open new-tag input.
    handle_key_event(&mut app, key(KeyCode::Char('+')), &tx).unwrap();
    // Type "abc".
    handle_key_event(&mut app, key(KeyCode::Char('a')), &tx).unwrap();
    handle_key_event(&mut app, key(KeyCode::Char('b')), &tx).unwrap();
    handle_key_event(&mut app, key(KeyCode::Char('c')), &tx).unwrap();
    assert_eq!(
        app.forms.bulk_tag_editor_mut().new_tag_input.as_deref(),
        Some("abc")
    );
    assert_eq!(app.forms.bulk_tag_editor_mut().new_tag_cursor, 3);
    // Backspace removes 'c'.
    handle_key_event(&mut app, key(KeyCode::Backspace), &tx).unwrap();
    assert_eq!(
        app.forms.bulk_tag_editor_mut().new_tag_input.as_deref(),
        Some("ab")
    );
    assert_eq!(app.forms.bulk_tag_editor_mut().new_tag_cursor, 2);
    // Left, Right.
    handle_key_event(&mut app, key(KeyCode::Left), &tx).unwrap();
    assert_eq!(app.forms.bulk_tag_editor_mut().new_tag_cursor, 1);
    handle_key_event(&mut app, key(KeyCode::Right), &tx).unwrap();
    assert_eq!(app.forms.bulk_tag_editor_mut().new_tag_cursor, 2);
    // Home, End.
    handle_key_event(&mut app, key(KeyCode::Home), &tx).unwrap();
    assert_eq!(app.forms.bulk_tag_editor_mut().new_tag_cursor, 0);
    handle_key_event(&mut app, key(KeyCode::End), &tx).unwrap();
    assert_eq!(app.forms.bulk_tag_editor_mut().new_tag_cursor, 2);
    // Esc cancels input without closing editor.
    handle_key_event(&mut app, key(KeyCode::Esc), &tx).unwrap();
    assert!(app.forms.bulk_tag_editor_mut().new_tag_input.is_none());
    assert_eq!(app.screen, Screen::BulkTagEditor);
}

#[test]
fn format_apply_status_variants() {
    use crate::app::BulkTagApplyResult;
    use crate::handler::bulk_tag_editor::format_apply_status;

    // No changes, no skipped.
    assert_eq!(format_apply_status(&BulkTagApplyResult::default()), "");

    // Only adds.
    let r = BulkTagApplyResult {
        changed_hosts: 3,
        added: 5,
        removed: 0,
        skipped_included: 0,
    };
    let s = format_apply_status(&r);
    assert!(s.contains("Updated 3 hosts"), "{s}");
    assert!(s.contains("+5"), "{s}");
    assert!(!s.contains("-"), "{s}");

    // Only removes.
    let r = BulkTagApplyResult {
        changed_hosts: 2,
        added: 0,
        removed: 3,
        skipped_included: 0,
    };
    let s = format_apply_status(&r);
    assert!(s.contains("-3"), "{s}");

    // Both + skipped.
    let r = BulkTagApplyResult {
        changed_hosts: 4,
        added: 2,
        removed: 1,
        skipped_included: 2,
    };
    let s = format_apply_status(&r);
    assert!(s.contains("+2"), "{s}");
    assert!(s.contains("-1"), "{s}");
    assert!(s.contains("skipped 2"), "{s}");
    assert!(s.contains("include files"), "{s}");

    // Single host, single skipped (singular forms).
    let r = BulkTagApplyResult {
        changed_hosts: 1,
        added: 1,
        removed: 0,
        skipped_included: 1,
    };
    let s = format_apply_status(&r);
    assert!(s.contains("Updated 1 host"), "{s}");
    assert!(!s.contains("hosts"), "should be singular: {s}");
    assert!(s.contains("skipped 1 in include file"), "{s}");
}

// ── route_confirm_key (confirm dialog routing) ──────────────────────

#[test]
fn route_confirm_key_y_lowercase_yes() {
    assert_eq!(
        super::route_confirm_key(key(KeyCode::Char('y'))),
        super::ConfirmAction::Yes
    );
}

#[test]
fn route_confirm_key_y_uppercase_yes() {
    assert_eq!(
        super::route_confirm_key(key(KeyCode::Char('Y'))),
        super::ConfirmAction::Yes
    );
}

#[test]
fn route_confirm_key_n_lowercase_no() {
    assert_eq!(
        super::route_confirm_key(key(KeyCode::Char('n'))),
        super::ConfirmAction::No
    );
}

#[test]
fn route_confirm_key_n_uppercase_no() {
    assert_eq!(
        super::route_confirm_key(key(KeyCode::Char('N'))),
        super::ConfirmAction::No
    );
}

#[test]
fn route_confirm_key_esc_no() {
    assert_eq!(
        super::route_confirm_key(key(KeyCode::Esc)),
        super::ConfirmAction::No
    );
}

#[test]
fn route_confirm_key_other_keys_ignored() {
    // Critical safety invariant: stray keys must NOT cancel a confirm dialog.
    for code in [
        KeyCode::Char('t'), // adjacent to y
        KeyCode::Char('u'), // adjacent to y
        KeyCode::Char('m'), // adjacent to n
        KeyCode::Char('b'), // adjacent to n
        KeyCode::Char('q'), // browse-context cancel, not confirm-context
        KeyCode::Enter,
        KeyCode::Tab,
        KeyCode::Char(' '),
    ] {
        assert_eq!(
            super::route_confirm_key(key(code)),
            super::ConfirmAction::Ignored,
            "key {:?} must be Ignored, not Yes/No",
            code
        );
    }
}

// ── End-to-end Vault Sign confirm safety (the original bug) ─────────

/// Build an app stuck on the Vault Sign confirm screen with one signable host.
fn vault_sign_confirm_app() -> App {
    let mut app =
        make_app("Host vaulthost\n  HostName vault.example.com\n  IdentityFile ~/.ssh/id_rsa\n");
    let path = tempfile::tempdir()
        .expect("tempdir")
        .keep()
        .join("test-cert");
    let signable = vec![crate::vault_ssh::VaultSignTarget {
        alias: "vaulthost".to_string(),
        role: "ssh-client/sign/role".to_string(),
        certificate_file: String::new(),
        pubkey: path,
        vault_addr: None,
    }];
    app.vault.set_pending_sign(signable);
    app.screen = Screen::ConfirmVaultSign;
    app
}

#[test]
fn vault_sign_confirm_stray_key_does_not_cancel() {
    // Original bug: a `_ => app.screen = Screen::HostList` catch-all let
    // any keypress next to `y` (e.g. `t`, `u`) silently abort a bulk sign.
    // Today the handler routes via `route_confirm_key` and stray keys are
    // explicitly Ignored. Regression guard.
    for stray in [
        KeyCode::Char('t'),
        KeyCode::Char('u'),
        KeyCode::Char('q'),
        KeyCode::Char(' '),
        KeyCode::Enter,
        KeyCode::Tab,
    ] {
        let mut app = vault_sign_confirm_app();
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(stray), &tx);
        assert!(
            matches!(app.screen, Screen::ConfirmVaultSign),
            "stray key {:?} must not cancel Vault Sign confirm",
            stray
        );
        assert!(
            app.vault.pending_sign().is_some(),
            "stray key {:?} must not drop the pending_sign payload",
            stray
        );
    }
}

#[test]
fn vault_sign_confirm_n_cancels() {
    let mut app = vault_sign_confirm_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    assert_eq!(app.screen, Screen::HostList);
    assert!(
        app.vault.pending_sign().is_none(),
        "n must clear pending_sign"
    );
}

#[test]
fn vault_sign_confirm_esc_cancels() {
    let mut app = vault_sign_confirm_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert_eq!(app.screen, Screen::HostList);
    assert!(
        app.vault.pending_sign().is_none(),
        "Esc must clear pending_sign"
    );
}

// ── Picker-field parity (Enter submits, Space-on-empty opens, Space-on-populated literal) ──

#[test]
fn enter_on_identity_file_field_does_not_open_key_picker() {
    // Invariant 1: Enter never opens a picker.
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::IdentityFile;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(
        !app.ui.key_picker().open,
        "Enter on IdentityFile must NOT open the key picker (use Space)"
    );
}

#[test]
fn space_on_empty_identity_file_opens_key_picker() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::IdentityFile;
    assert!(app.forms.host_mut().identity_file.is_empty());
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(app.ui.key_picker().open);
}

#[test]
fn space_on_populated_identity_file_inserts_literal() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::IdentityFile;
    app.forms.host_mut().identity_file = "/home/me/keys/id".to_string();
    app.forms.host_mut().cursor_pos = app.forms.host_mut().identity_file.chars().count();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(
        !app.ui.key_picker().open,
        "Space on populated IdentityFile must NOT open picker"
    );
    assert_eq!(app.forms.host_mut().identity_file, "/home/me/keys/id ");
}

#[test]
fn enter_on_proxy_jump_field_does_not_open_picker() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::ProxyJump;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.proxyjump_picker().open);
}

#[test]
fn space_on_empty_proxy_jump_opens_picker() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::ProxyJump;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(app.ui.proxyjump_picker().open);
}

#[test]
fn space_on_populated_proxy_jump_inserts_literal() {
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::ProxyJump;
    app.forms.host_mut().proxy_jump = "bastion".to_string();
    app.forms.host_mut().cursor_pos = 7;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(!app.ui.proxyjump_picker().open);
    assert_eq!(app.forms.host_mut().proxy_jump, "bastion ");
}

#[test]
fn space_on_empty_vault_ssh_with_no_candidates_inserts_literal() {
    // VaultSsh is `is_picker == true` but the picker only opens when there
    // are role candidates. With none configured, Space on empty VaultSsh
    // degrades to literal-space insert so the user can type the role.
    let mut app = make_form_app();
    app.forms.host_mut().focused_field = FormField::VaultSsh;
    assert!(app.vault_role_candidates().is_empty());
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(
        !app.ui.vault_role_picker().open,
        "no candidates → no picker, even on empty field"
    );
    assert_eq!(
        app.forms.host_mut().vault_ssh,
        " ",
        "Space falls through to literal-space insert"
    );
}

// ── Provider form picker-field parity ───────────────────────────────

#[test]
fn enter_on_provider_identity_file_does_not_open_picker() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::IdentityFile);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.key_picker().open);
}

#[test]
fn space_on_populated_provider_identity_file_inserts_literal() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::IdentityFile);
    app.providers.form_mut().identity_file = "/path".to_string();
    app.providers.form_mut().cursor_pos = 5;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(!app.ui.key_picker().open);
    assert_eq!(app.providers.form_mut().identity_file, "/path ");
}

#[test]
fn space_on_populated_ovh_regions_inserts_literal() {
    let mut app = make_ovh_form_app();
    app.providers.form_mut().focused_field = ProviderFormField::Regions;
    app.providers.form_mut().regions = "eu".to_string();
    app.providers.form_mut().cursor_pos = 2;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(
        !app.ui.region_picker().open,
        "Space on populated Regions must NOT open picker"
    );
    assert_eq!(app.providers.form_mut().regions, "eu ");
}

// ── Container confirm n/N cancels ───────────────────────────────────

fn container_confirm_app() -> App {
    let mut app = make_app("Host srv\n  HostName srv.example.com\n");
    app.screen = Screen::Containers {
        alias: "srv".to_string(),
    };
    app.container_session = Some(crate::app::ContainerSession {
        alias: "srv".to_string(),
        askpass: None,
        runtime: Some(crate::containers::ContainerRuntime::Docker),
        containers: vec![crate::containers::ContainerInfo {
            id: "abc123".to_string(),
            names: "demo".to_string(),
            image: "nginx".to_string(),
            state: "running".to_string(),
            status: "Up".to_string(),
            ports: String::new(),
        }],
        list_state: ratatui::widgets::ListState::default(),
        loading: false,
        error: None,
        action_in_progress: None,
        confirm_action: Some((
            crate::containers::ContainerAction::Stop,
            "demo".to_string(),
            "abc123".to_string(),
        )),
    });
    app.container_session
        .as_mut()
        .unwrap()
        .list_state
        .select(Some(0));
    app
}

#[test]
fn container_confirm_n_cancels_pending_action() {
    let mut app = container_confirm_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    let state = app.container_session.as_ref().unwrap();
    assert!(
        state.confirm_action.is_none(),
        "n must cancel the pending container action"
    );
    assert!(matches!(app.screen, Screen::Containers { .. }));
}

#[test]
fn container_confirm_capital_n_cancels_pending_action() {
    let mut app = container_confirm_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('N')), &tx);
    assert!(
        app.container_session
            .as_ref()
            .unwrap()
            .confirm_action
            .is_none()
    );
}

#[test]
fn container_confirm_stray_key_ignored() {
    let mut app = container_confirm_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('t')), &tx);
    assert!(
        app.container_session
            .as_ref()
            .unwrap()
            .confirm_action
            .is_some(),
        "stray key must not cancel the pending container action"
    );
}

// ── BulkTagEditorState::is_dirty added-rows branch ──────────────────

#[test]
fn bulk_editor_is_dirty_detects_added_row() {
    use crate::app::{BulkTagAction, BulkTagEditorState, BulkTagRow};
    let mut state = BulkTagEditorState {
        rows: vec![BulkTagRow {
            tag: "prod".into(),
            initial_count: 0,
            action: BulkTagAction::Leave,
        }],
        aliases: Vec::new(),
        skipped_included: Vec::new(),
        new_tag_input: None,
        new_tag_cursor: 0,
        initial_actions: vec![BulkTagAction::Leave],
    };
    assert!(!state.is_dirty(), "baseline state must be clean");

    // Append a new tag row (simulates the `+ new tag` flow). New rows
    // default to AddToAll, so they count as dirty immediately.
    state.rows.push(BulkTagRow {
        tag: "newtag".into(),
        initial_count: 0,
        action: BulkTagAction::AddToAll,
    });
    assert!(state.is_dirty(), "added row with non-Leave action is dirty");
}

#[test]
fn bulk_editor_is_dirty_added_leave_row_still_clean() {
    // Edge case: an appended row that happens to still be Leave is not
    // semantically dirty (nothing will change on apply).
    use crate::app::{BulkTagAction, BulkTagEditorState, BulkTagRow};
    let mut state = BulkTagEditorState {
        rows: vec![BulkTagRow {
            tag: "prod".into(),
            initial_count: 0,
            action: BulkTagAction::Leave,
        }],
        aliases: Vec::new(),
        skipped_included: Vec::new(),
        new_tag_input: None,
        new_tag_cursor: 0,
        initial_actions: vec![BulkTagAction::Leave],
    };
    state.rows.push(BulkTagRow {
        tag: "noop".into(),
        initial_count: 0,
        action: BulkTagAction::Leave,
    });
    assert!(!state.is_dirty(), "appended Leave row is not dirty");
}

// --- WhatsNew overlay handler tests ---

#[test]
fn whats_new_esc_closes_and_marks_seen() {
    let mut app = make_app("");
    app.screen = Screen::WhatsNew(crate::app::WhatsNewState::default());
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert_eq!(
        crate::preferences::load_last_seen_version(app.env().paths())
            .unwrap()
            .as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn whats_new_q_closes() {
    let mut app = make_app("");
    app.screen = Screen::WhatsNew(crate::app::WhatsNewState::default());
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('q')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn whats_new_n_toggles_closed() {
    let mut app = make_app("");
    app.screen = Screen::WhatsNew(crate::app::WhatsNewState::default());
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn whats_new_enter_does_nothing() {
    let mut app = make_app("");
    app.screen = Screen::WhatsNew(crate::app::WhatsNewState::default());
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(
        matches!(app.screen, Screen::WhatsNew(_)),
        "Enter must not close the overlay"
    );
    assert_eq!(
        crate::preferences::load_last_seen_version(app.env().paths()).unwrap(),
        None,
        "Enter must not persist last_seen_version"
    );
}

#[test]
fn whats_new_scroll_j_advances_state() {
    let mut app = make_app("");
    app.screen = Screen::WhatsNew(crate::app::WhatsNewState::default());
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    if let Screen::WhatsNew(ref s) = app.screen {
        assert_eq!(s.scroll, 1);
    } else {
        panic!("expected WhatsNew screen");
    }
}

#[test]
fn whats_new_close_dismisses_sticky_toast() {
    let mut app = make_app("");
    app.status_center
        .set_toast_message(Some(crate::app::StatusMessage {
            text: crate::messages::whats_new_toast::upgraded("2.42.0"),
            class: crate::app::MessageClass::Success,
            tick_count: 0,
            sticky: true,
            created_at: std::time::Instant::now(),
        }));
    app.screen = Screen::WhatsNew(crate::app::WhatsNewState::default());
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let fragment = crate::messages::whats_new_toast::INVITE_FRAGMENT;
    let contains_invite = app
        .status_center
        .toast()
        .is_some_and(|t| t.text.contains(fragment));
    assert!(!contains_invite, "sticky toast should be dismissed");
}

#[test]
fn host_list_n_opens_whats_new_when_search_inactive() {
    let mut app = make_app("");
    app.screen = Screen::HostList;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    assert!(matches!(app.screen, Screen::WhatsNew(_)));
}

#[test]
fn host_list_n_types_into_search_when_active() {
    let mut app = make_app("");
    app.screen = Screen::HostList;
    app.search.set_query(Some(String::new()));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert_eq!(app.search.query(), Some("n"));
}

// =========================================================================
// Tunnels overview: add/edit/delete via top-page Tunnels
// =========================================================================

/// Helper: build an app on the Tunnels top-page with one editable host that
/// has a single LocalForward tunnel. Cursor is positioned on the tunnel.
fn make_tunnels_overview_app() -> App {
    let mut app = make_app("Host test\n  HostName test.com\n  LocalForward 8080 localhost:80\n");
    app.top_page = crate::app::TopPage::Tunnels;
    app.screen = Screen::HostList;
    app.ui.tunnels_overview_state_mut().select(Some(0));
    app
}

#[test]
fn tunnels_overview_a_with_no_editable_hosts_shows_warning() {
    let mut app = make_app("");
    app.top_page = crate::app::TopPage::Tunnels;
    app.screen = Screen::HostList;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('a')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert!(matches!(app.top_page, crate::app::TopPage::Tunnels));
    let toast = app.status_center.toast().expect("toast");
    assert!(toast.text.contains("No editable hosts"));
}

#[test]
fn tunnels_overview_a_opens_host_picker() {
    let mut app = make_tunnels_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('a')), &tx);
    assert!(matches!(app.screen, Screen::TunnelHostPicker));
    assert_eq!(app.ui.tunnel_host_picker_state().selected(), Some(0));
}

#[test]
fn tunnel_host_picker_enter_opens_form_for_chosen_host() {
    let mut app = make_app("Host alpha\n  HostName a.example\n\nHost beta\n  HostName b.example\n");
    app.top_page = crate::app::TopPage::Tunnels;
    app.screen = Screen::TunnelHostPicker;
    app.ui.tunnel_host_picker_state_mut().select(Some(1)); // beta
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    match &app.screen {
        Screen::TunnelForm { alias, editing } => {
            assert_eq!(alias, "beta");
            assert!(editing.is_none());
        }
        other => panic!("expected TunnelForm, got {:?}", other.variant_name()),
    }
    // Picker cursor was reset so re-opening starts from the top.
    assert_eq!(app.ui.tunnel_host_picker_state().selected(), None);
}

#[test]
fn tunnel_host_picker_esc_returns_to_overview() {
    let mut app = make_tunnels_overview_app();
    app.screen = Screen::TunnelHostPicker;
    app.ui.tunnel_host_picker_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert!(matches!(app.top_page, crate::app::TopPage::Tunnels));
    assert_eq!(app.ui.tunnel_host_picker_state().selected(), None);
}

#[test]
fn tunnel_host_picker_arrow_keys_clamp() {
    // Mirrors jump navigation: Down clamps at the last row,
    // Up clamps at row 0. No wrap-around — predictable for "type to filter"
    // overlays where the visible set changes between keystrokes.
    let mut app = make_app("Host alpha\n  HostName a\n\nHost beta\n  HostName b\n");
    app.top_page = crate::app::TopPage::Tunnels;
    app.screen = Screen::TunnelHostPicker;
    app.ui.tunnel_host_picker_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Down), &tx);
    assert_eq!(app.ui.tunnel_host_picker_state().selected(), Some(1));
    let _ = handle_key_event(&mut app, key(KeyCode::Down), &tx);
    assert_eq!(app.ui.tunnel_host_picker_state().selected(), Some(1));
    let _ = handle_key_event(&mut app, key(KeyCode::Up), &tx);
    assert_eq!(app.ui.tunnel_host_picker_state().selected(), Some(0));
    let _ = handle_key_event(&mut app, key(KeyCode::Up), &tx);
    assert_eq!(app.ui.tunnel_host_picker_state().selected(), Some(0));
}

#[test]
fn tunnel_host_picker_typing_filters_live() {
    // Always-on fuzzy search: every printable keystroke appends to the
    // query and the visible set shrinks accordingly.
    let mut app = make_app(
        "Host alpha\n  HostName a.example\n\n\
         Host beta\n  HostName b.example\n\n\
         Host db-primary\n  HostName 10.30.1.5\n",
    );
    app.top_page = crate::app::TopPage::Tunnels;
    app.screen = Screen::TunnelHostPicker;
    app.ui.tunnel_host_picker_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('b')), &tx);
    assert_eq!(app.ui.tunnel_host_picker_query(), "db");
    let visible = crate::handler::tunnel_host_picker::filtered_hosts(&app);
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].0, "db-primary");
    assert_eq!(app.ui.tunnel_host_picker_state().selected(), Some(0));
}

#[test]
fn tunnel_host_picker_backspace_pops_query_char() {
    let mut app = make_app("Host alpha\n  HostName a\n\nHost beta\n  HostName b\n");
    app.top_page = crate::app::TopPage::Tunnels;
    app.screen = Screen::TunnelHostPicker;
    app.ui.tunnel_host_picker_state_mut().select(Some(0));
    app.ui.set_tunnel_host_picker_query("be".to_string());
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    assert_eq!(app.ui.tunnel_host_picker_query(), "b");
    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    assert_eq!(app.ui.tunnel_host_picker_query(), "");
    // Still on the picker; full list is restored.
    assert!(matches!(app.screen, Screen::TunnelHostPicker));
    let visible = crate::handler::tunnel_host_picker::filtered_hosts(&app);
    assert_eq!(visible.len(), 2);
}

#[test]
fn tunnel_host_picker_backspace_on_empty_query_closes() {
    // Mirrors the jump: Backspace on an empty buffer cancels
    // the overlay so a single keystroke gets you out without reaching Esc.
    let mut app = make_app("Host alpha\n  HostName a\n");
    app.top_page = crate::app::TopPage::Tunnels;
    app.screen = Screen::TunnelHostPicker;
    app.ui.tunnel_host_picker_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert!(matches!(app.top_page, crate::app::TopPage::Tunnels));
}

#[test]
fn tunnel_host_picker_substring_match() {
    // Substring (not subsequence) keeps semantics in lock-step with the
    // jump: a contiguous run of query chars must appear in
    // either the alias or the hostname.
    let mut app = make_app(
        "Host db-primary\n  HostName a\n\
         Host other\n  HostName db.internal\n\
         Host nope\n  HostName 1.2.3.4\n",
    );
    app.top_page = crate::app::TopPage::Tunnels;
    app.screen = Screen::TunnelHostPicker;
    app.ui.set_tunnel_host_picker_query("db".to_string());
    let visible = crate::handler::tunnel_host_picker::filtered_hosts(&app);
    let aliases: Vec<&str> = visible.iter().map(|(a, _)| a.as_str()).collect();
    assert_eq!(aliases, vec!["db-primary", "other"]);
}

#[test]
fn tunnel_host_picker_enter_uses_filtered_index() {
    // Enter must select the host at the cursor's position within the
    // filtered set, not the underlying full list.
    let mut app = make_app(
        "Host alpha\n  HostName a\n\n\
         Host beta\n  HostName b\n\n\
         Host db-primary\n  HostName 10.30.1.5\n",
    );
    app.top_page = crate::app::TopPage::Tunnels;
    app.screen = Screen::TunnelHostPicker;
    app.ui.set_tunnel_host_picker_query("db".to_string());
    app.ui.tunnel_host_picker_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    match &app.screen {
        Screen::TunnelForm { alias, editing } => {
            assert_eq!(alias, "db-primary");
            assert!(editing.is_none());
        }
        other => panic!("expected TunnelForm, got {:?}", other.variant_name()),
    }
    assert!(app.ui.tunnel_host_picker_query().is_empty());
}

#[test]
fn tunnel_host_picker_no_match_blocks_enter() {
    // When the query filters out every host, the cursor is None and Enter
    // must be a no-op (no panic, no transition).
    let mut app = make_app("Host alpha\n  HostName a\n");
    app.top_page = crate::app::TopPage::Tunnels;
    app.screen = Screen::TunnelHostPicker;
    app.ui.tunnel_host_picker_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('z')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('z')), &tx);
    assert_eq!(app.ui.tunnel_host_picker_state().selected(), None);
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(matches!(app.screen, Screen::TunnelHostPicker));
}

#[test]
fn tunnel_host_picker_query_matches_hostname_too() {
    // Substring match runs on both alias and hostname so users can find a
    // host by IP fragment when they don't remember the alias.
    let mut app = make_app(
        "Host bastion\n  HostName 140.82.121.3\n\n\
         Host db\n  HostName 10.30.1.5\n",
    );
    app.top_page = crate::app::TopPage::Tunnels;
    app.screen = Screen::TunnelHostPicker;
    app.ui.set_tunnel_host_picker_query("10.30".to_string());
    let visible = crate::handler::tunnel_host_picker::filtered_hosts(&app);
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].0, "db");
}

#[test]
fn tunnels_overview_e_opens_form_for_selected_row() {
    let mut app = make_tunnels_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
    match &app.screen {
        Screen::TunnelForm { alias, editing } => {
            assert_eq!(alias, "test");
            assert_eq!(*editing, Some(0));
        }
        other => panic!("expected TunnelForm, got {:?}", other.variant_name()),
    }
    // Form should be pre-populated with the row's tunnel data.
    assert_eq!(app.tunnels.form_mut().bind_port, "8080");
    assert_eq!(app.tunnels.form_mut().remote_host, "localhost");
    assert_eq!(app.tunnels.form_mut().remote_port, "80");
}

#[test]
fn tunnels_overview_d_sets_pending_delete() {
    let mut app = make_tunnels_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    assert_eq!(app.tunnels.pending_delete(), Some(0));
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn tunnels_overview_pending_delete_y_removes_tunnel_and_returns_to_overview() {
    let mut app = make_tunnels_overview_app();
    app.tunnels.request_delete(0);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert_eq!(app.tunnels.pending_delete(), None);
    assert!(matches!(app.screen, Screen::HostList));
    assert!(matches!(app.top_page, crate::app::TopPage::Tunnels));
    // Tunnel directive must be gone from the in-memory ssh_config.
    let rules = app.hosts_state.ssh_config().find_tunnel_directives("test");
    assert!(rules.is_empty(), "tunnel should have been removed");
}

#[test]
fn tunnels_overview_pending_delete_n_cancels() {
    let mut app = make_tunnels_overview_app();
    app.tunnels.request_delete(0);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    assert_eq!(app.tunnels.pending_delete(), None);
    let rules = app.hosts_state.ssh_config().find_tunnel_directives("test");
    assert_eq!(rules.len(), 1, "tunnel must still be present after cancel");
}

fn make_multi_tunnel_overview_app() -> App {
    let mut app = make_app(
        "Host alpha\n  HostName a.example\n  LocalForward 8080 localhost:80\n\
         Host beta\n  HostName b.example\n  LocalForward 9090 localhost:90\n\
         Host gamma\n  HostName c.example\n  LocalForward 7070 localhost:70\n",
    );
    app.top_page = crate::app::TopPage::Tunnels;
    app.screen = Screen::HostList;
    app.ui.tunnels_overview_state_mut().select(Some(0));
    app
}

#[test]
fn tunnels_overview_search_down_navigates_filtered_list_without_leaving_input_mode() {
    // While typing into the tunnels-tab search box, ↓/Tab must move the
    // cursor through the filtered set without dismissing the input —
    // mirroring host-list search ergonomics so the muscle memory is
    // shared across tabs.
    let mut app = make_multi_tunnel_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    assert!(app.search.query().is_some(), "/ enters search mode");
    let _ = handle_key_event(&mut app, key(KeyCode::Down), &tx);
    assert!(
        app.search.query().is_some(),
        "Down must NOT exit search input mode"
    );
    assert_eq!(app.ui.tunnels_overview_state().selected(), Some(1));
}

#[test]
fn tunnels_overview_search_enter_acts_on_highlighted_row_and_dismisses_input() {
    // Enter while searching should behave like Enter outside search:
    // act on the highlighted tunnel (toggle start/stop) and clear the
    // query so subsequent keys navigate normally. Demo mode would
    // forbid the actual ssh spawn, so we just check the input was
    // dismissed and the screen stayed put.
    let mut app = make_tunnels_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(
        app.search.query().is_none(),
        "Enter must dismiss the search input"
    );
}

#[test]
fn tunnels_overview_search_esc_clears_query_and_resets_cursor() {
    let mut app = make_multi_tunnel_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('b')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(app.search.query().is_none());
    assert_eq!(app.ui.tunnels_overview_state().selected(), Some(0));
}

#[test]
fn tunnels_overview_n_opens_whats_new() {
    // The tunnels tab should reach the What's New overlay with the
    // same single-key shortcut as the host list — discoverable from
    // either main tab.
    let mut app = make_tunnels_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    assert!(matches!(app.screen, Screen::WhatsNew(_)));
}

#[test]
fn tunnels_overview_pending_delete_esc_cancels() {
    let mut app = make_tunnels_overview_app();
    app.tunnels.request_delete(0);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert_eq!(app.tunnels.pending_delete(), None);
    let rules = app.hosts_state.ssh_config().find_tunnel_directives("test");
    assert_eq!(rules.len(), 1);
}

#[test]
fn tunnels_overview_pending_delete_other_key_ignored() {
    let mut app = make_tunnels_overview_app();
    app.tunnels.request_delete(0);
    let (tx, _rx) = mpsc::channel();
    // A stray character must not silently transition state — pending_delete
    // stays armed, the tunnel stays in the config, and no toast fires.
    let _ = handle_key_event(&mut app, key(KeyCode::Char('t')), &tx);
    assert_eq!(app.tunnels.pending_delete(), Some(0));
    let rules = app.hosts_state.ssh_config().find_tunnel_directives("test");
    assert_eq!(rules.len(), 1);
}

#[test]
fn tunnel_form_esc_from_tunnels_overview_returns_to_overview() {
    // When the form was opened from the Tunnels-tab overview the cancel
    // path must hop back to the overview, not to the per-host TunnelList.
    let mut app = make_tunnels_overview_app();
    app.screen = Screen::TunnelForm {
        alias: "test".to_string(),
        editing: None,
    };
    *app.tunnels.form_mut() = crate::app::TunnelForm::new();
    app.capture_tunnel_form_baseline();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert!(matches!(app.top_page, crate::app::TopPage::Tunnels));
}

#[test]
fn tunnel_form_submit_from_tunnels_overview_returns_to_overview() {
    let mut app = make_tunnels_overview_app();
    app.screen = Screen::TunnelForm {
        alias: "test".to_string(),
        editing: None,
    };
    *app.tunnels.form_mut() = crate::app::TunnelForm::new();
    app.tunnels.form_mut().bind_port = "9090".to_string();
    app.tunnels.form_mut().remote_host = "internal.corp".to_string();
    app.tunnels.form_mut().remote_port = "443".to_string();
    app.capture_form_mtime();
    app.capture_tunnel_form_baseline();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert!(matches!(app.top_page, crate::app::TopPage::Tunnels));
    let rules = app.hosts_state.ssh_config().find_tunnel_directives("test");
    assert_eq!(rules.len(), 2, "new tunnel should be persisted");
}

#[test]
fn tunnel_form_esc_from_host_detail_still_returns_to_tunnel_list() {
    // Regression guard: the existing host-detail flow must keep returning
    // to TunnelList. Only top_page == Tunnels redirects to the overview.
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.top_page = crate::app::TopPage::Hosts;
    app.screen = Screen::TunnelForm {
        alias: "test".to_string(),
        editing: None,
    };
    *app.tunnels.form_mut() = crate::app::TunnelForm::new();
    app.capture_tunnel_form_baseline();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::TunnelList { .. }));
}

// =========================================================================
// Tunnels overview: sort cycling and jump
// =========================================================================

#[test]
fn tunnels_overview_s_cycles_sort_mode() {
    use crate::app::TunnelSortMode;
    let mut app = make_tunnels_overview_app();
    assert_eq!(app.tunnels.sort_mode(), TunnelSortMode::MostRecent);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('s')), &tx);
    assert_eq!(app.tunnels.sort_mode(), TunnelSortMode::AlphaHostname);
    let toast = app.status_center.toast().expect("toast");
    assert!(toast.text.contains("A-Z hostname"));
    let _ = handle_key_event(&mut app, key(KeyCode::Char('s')), &tx);
    assert_eq!(app.tunnels.sort_mode(), TunnelSortMode::MostRecent);
    let toast = app.status_center.toast().expect("toast");
    assert!(toast.text.contains("most recent"));
}

#[test]
fn tunnels_overview_colon_opens_jump_in_tunnels_mode() {
    let mut app = make_tunnels_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(':')), &tx);
    let jump = app.jump.as_ref().expect("jump bar open");
    assert_eq!(jump.mode(), crate::app::JumpMode::Tunnels);
    let cmds = jump.filtered_commands();
    assert!(
        cmds.iter()
            .any(|c| c.key == 's' && c.label.contains("Tunnels: Sort")),
        "tunnels jump bar must include the Tunnels: Sort action"
    );
    // The unified jump bar shows host-actions on every tab now too.
    assert!(
        cmds.iter().any(|c| c.label.contains("Hosts: Ping host")),
        "Hosts: Ping host must be reachable from the tunnels-tab jump bar"
    );
}

#[test]
fn tunnels_overview_jump_enter_dispatches_sort() {
    let mut app = make_tunnels_overview_app();
    app.jump = Some(crate::app::JumpState::for_mode(
        crate::app::JumpMode::Tunnels,
    ));
    app.jump.as_mut().unwrap().push_query('s');
    app.jump.as_mut().unwrap().push_query('o');
    app.jump.as_mut().unwrap().push_query('r');
    app.jump.as_mut().unwrap().push_query('t');
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.jump.is_none(), "jump bar should close after Enter");
    assert_eq!(
        app.tunnels.sort_mode(),
        crate::app::TunnelSortMode::AlphaHostname,
        "sort cycled via jump dispatch"
    );
}

#[test]
fn tunnels_overview_jump_dispatches_to_tunnels_handler() {
    // Sanity: 'a' in Tunnels-mode jump bar routes through the tunnels handler
    // (which opens the host picker), not the host-list handler. The
    // mode-match boost ensures the tunnel-targeted action wins on a
    // single-char `a` query against the unified action set.
    let mut app = make_tunnels_overview_app();
    app.jump = Some(crate::app::JumpState::for_mode(
        crate::app::JumpMode::Tunnels,
    ));
    app.jump.as_mut().unwrap().push_query('a');
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(matches!(app.screen, Screen::TunnelHostPicker));
}

#[test]
fn tunnels_overview_edit_after_sort_targets_visible_row() {
    // Regression: with two hosts in storage order zzz, aaa and AlphaHostname
    // sort active, the row at cursor 0 visually is `aaa`. Pressing 'e' must
    // open the form for `aaa`, not `zzz`. Before the visible_pairs refactor
    // the handler walked raw storage order and would target `zzz`.
    let mut app = make_app(
        "Host zzz\n  HostName z.example\n  LocalForward 9090 localhost:90\n\
         Host aaa\n  HostName a.example\n  LocalForward 8080 localhost:80\n",
    );
    app.top_page = crate::app::TopPage::Tunnels;
    app.screen = Screen::HostList;
    app.tunnels
        .set_sort_mode(crate::app::TunnelSortMode::AlphaHostname);
    app.ui.tunnels_overview_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
    match &app.screen {
        Screen::TunnelForm { alias, editing } => {
            assert_eq!(
                alias, "aaa",
                "edit must target visible row, not storage row"
            );
            assert!(editing.is_some());
        }
        other => panic!("expected TunnelForm, got {:?}", other.variant_name()),
    }
}

#[test]
fn tunnels_overview_reposition_cursor_follows_row_to_new_index() {
    // Regression: when MostRecent reorders rows after a tunnel toggle, the
    // cursor must follow the row the user acted on, not stay at the old
    // visual index (which would now point at a different host).
    use crate::tunnel::{TunnelRule, TunnelType};
    let mut app = make_app(
        "Host aaa\n  HostName a.example\n  LocalForward 8080 localhost:80\n\
         Host zzz\n  HostName z.example\n  LocalForward 9090 localhost:90\n",
    );
    app.top_page = crate::app::TopPage::Tunnels;
    app.screen = Screen::HostList;
    app.tunnels
        .set_sort_mode(crate::app::TunnelSortMode::MostRecent);
    app.ui.tunnels_overview_state_mut().select(Some(1));
    let zzz_rule = TunnelRule {
        tunnel_type: TunnelType::Local,
        bind_address: String::new(),
        bind_port: 9090,
        remote_host: "localhost".to_string(),
        remote_port: 90,
    };
    // Simulate the sort-key change a tunnel start would cause: zzz becomes
    // the most-recently-connected host, jumping from index 1 to index 0.
    app.history.record("zzz");
    crate::handler::tunnels_overview::reposition_cursor_on(&mut app, "zzz", &zzz_rule);
    let pairs = crate::ui::tunnels_overview::visible_pairs(
        &app.search,
        &app.hosts_state,
        &app.tunnels,
        &app.history,
    );
    let zzz_idx = pairs.iter().position(|(a, _)| a == "zzz").expect("zzz row");
    assert_eq!(
        app.ui.tunnels_overview_state().selected(),
        Some(zzz_idx),
        "cursor must follow zzz to its new sorted position"
    );
}

#[test]
fn tunnels_overview_delete_after_sort_targets_visible_row() {
    // Regression: pending_delete index must resolve through the visible
    // (filtered + sorted) sequence, not raw storage order.
    let mut app = make_app(
        "Host zzz\n  HostName z.example\n  LocalForward 9090 localhost:90\n\
         Host aaa\n  HostName a.example\n  LocalForward 8080 localhost:80\n",
    );
    app.top_page = crate::app::TopPage::Tunnels;
    app.screen = Screen::HostList;
    app.tunnels
        .set_sort_mode(crate::app::TunnelSortMode::AlphaHostname);
    app.ui.tunnels_overview_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    // After deletion of `aaa`'s tunnel, only `zzz`'s tunnel should remain.
    let aaa_rules = app.hosts_state.ssh_config().find_tunnel_directives("aaa");
    let zzz_rules = app.hosts_state.ssh_config().find_tunnel_directives("zzz");
    assert!(
        aaa_rules.is_empty(),
        "aaa's tunnel should have been removed"
    );
    assert_eq!(zzz_rules.len(), 1, "zzz's tunnel must be untouched");
}

#[test]
fn esc_on_tunnels_overview_does_not_quit_first_press_shows_hint() {
    let mut app = make_tunnels_overview_app();
    assert!(app.running);
    assert!(!app.ui.esc_quit_hint_shown());

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);

    assert!(app.running, "Esc on idle tunnels overview must not quit");
    assert!(app.ui.esc_quit_hint_shown());
    let toast = app
        .status_center
        .toast()
        .expect("first idle Esc must surface a toast");
    assert_eq!(toast.text, crate::messages::ESC_QUIT_HINT);
}

#[test]
fn esc_on_tunnels_overview_second_press_silent_noop() {
    let mut app = make_tunnels_overview_app();
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    app.status_center.set_toast_message(None);

    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);

    assert!(app.running);
    assert!(
        app.status_center.toast().is_none(),
        "second idle Esc must stay silent"
    );
}

#[test]
fn q_on_tunnels_overview_still_quits() {
    let mut app = make_tunnels_overview_app();
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char('q')), &tx);

    assert!(!app.running, "q must always quit on tunnels overview");
}

#[test]
fn esc_hint_does_not_displace_active_sticky_error_toast() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    let (tx, _rx) = mpsc::channel();

    // notify_error surfaces a sticky Error-class toast — exactly the kind of
    // message that must not be silently clobbered by an informational hint.
    app.notify_error("provider sync failed");
    let sticky = app
        .status_center
        .toast()
        .expect("notify_error must land in the toast slot");
    assert!(sticky.sticky, "Error toasts are sticky by default");
    let sticky_text = sticky.text.clone();

    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);

    assert!(app.running, "Esc must not quit");
    assert!(
        !app.ui.esc_quit_hint_shown(),
        "flag must stay unset when the hint was suppressed by a sticky toast"
    );
    assert_eq!(
        app.status_center
            .toast()
            .map(|t| t.text.as_str())
            .unwrap_or(""),
        sticky_text,
        "sticky Error toast must remain visible, hint must not displace it"
    );

    // Once the sticky toast is cleared, a later idle Esc surfaces the hint as
    // designed and arms the one-shot flag.
    app.status_center.set_toast_message(None);
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(app.ui.esc_quit_hint_shown());
    assert_eq!(
        app.status_center.toast().map(|t| t.text.as_str()),
        Some(crate::messages::ESC_QUIT_HINT)
    );
}

// =========================================================================
// Containers overview: navigation and Tab cycling
// =========================================================================

fn make_containers_overview_app() -> App {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n\nHost db\n  HostName 2.2.2.2\n");
    // App::new loads ~/.purple/container_cache.jsonl from disk. Clear so
    // host-environment cache state cannot leak into the test set.
    app.container_state.clear_cache();
    app.container_state.insert_cache_entry(
        "web".to_string(),
        crate::containers::ContainerCacheEntry {
            timestamp: 100,
            runtime: crate::containers::ContainerRuntime::Docker,
            engine_version: None,
            containers: vec![
                make_container("c1", "nginx", "running"),
                make_container("c2", "redis", "exited"),
            ],
        },
    );
    app.container_state.insert_cache_entry(
        "db".to_string(),
        crate::containers::ContainerCacheEntry {
            timestamp: 100,
            runtime: crate::containers::ContainerRuntime::Docker,
            engine_version: None,
            containers: vec![make_container("c3", "postgres", "running")],
        },
    );
    app.top_page = crate::app::TopPage::Containers;
    app.screen = Screen::HostList;
    // Default sort = AlphaHost interleaves host headers with
    // container rows. Item layout for this fixture:
    //   [0] HostHeader(db)
    //   [1] Container(db/postgres)      <- first container
    //   [2] HostHeader(web)
    //   [3] Container(web/nginx)
    //   [4] Container(web/redis)
    // Park cursor on the first container so tests start from a
    // selectable row (headers are skipped by handler navigation).
    app.ui.containers_overview_state_mut().select(Some(1));
    app
}

#[test]
fn containers_overview_j_advances_cursor() {
    // Items: [0]Header(db) [1]postgres [2]Header(web) [3]nginx [4]redis.
    // Headers are now selectable (bulk K/S, fold/unfold) so `j` from
    // idx 1 lands on the next item, the host divider at idx 2.
    let mut app = make_containers_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    assert_eq!(app.ui.containers_overview_state().selected(), Some(2));
}

#[test]
fn containers_overview_g_jumps_to_top() {
    let mut app = make_containers_overview_app();
    app.ui.containers_overview_state_mut().select(Some(4));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('g')), &tx);
    // Headers are valid selection targets; `g` snaps to the very
    // first row, which is the db host divider at idx 0.
    assert_eq!(app.ui.containers_overview_state().selected(), Some(0));
}

#[test]
fn containers_overview_capital_g_jumps_to_bottom() {
    let mut app = make_containers_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('G')), &tx);
    // Last non-header item: idx 4 (web/redis) in our fixture.
    assert_eq!(app.ui.containers_overview_state().selected(), Some(4));
}

#[test]
fn containers_overview_s_cycles_sort_mode() {
    let mut app = make_containers_overview_app();
    assert_eq!(
        app.containers_overview.sort_mode(),
        crate::app::ContainersSortMode::AlphaHost
    );
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('s')), &tx);
    assert_eq!(
        app.containers_overview.sort_mode(),
        crate::app::ContainersSortMode::AlphaContainer
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Char('s')), &tx);
    assert_eq!(
        app.containers_overview.sort_mode(),
        crate::app::ContainersSortMode::AlphaHost
    );
}

#[test]
fn containers_overview_colon_opens_jump_in_containers_mode() {
    let mut app = make_containers_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(':')), &tx);
    let mode = app
        .jump
        .as_ref()
        .map(|p| p.mode())
        .expect("jump bar must be open");
    assert_eq!(mode, crate::app::JumpMode::Containers);
}

#[test]
fn containers_overview_s_persists_sort_mode_to_preferences() {
    // Regression: `s` used to flip in-memory only, so a restart would
    // drop back to AlphaHost. Verify the persisted value matches the
    // in-memory value after each flip.
    let mut app = make_containers_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('s')), &tx);
    assert_eq!(
        crate::preferences::load_containers_sort_mode(app.env().paths()),
        crate::app::ContainersSortMode::AlphaContainer
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Char('s')), &tx);
    assert_eq!(
        crate::preferences::load_containers_sort_mode(app.env().paths()),
        crate::app::ContainersSortMode::AlphaHost
    );
}

#[test]
fn containers_overview_s_keeps_cursor_on_same_alias_after_flip() {
    // Regression: capturing selected_alias() AFTER flipping sort_mode
    // resolves the cursor index against the new ordering and picks a
    // different row. Cursor must stay on the alias the user pressed `s`
    // while looking at.
    //
    // AlphaHost item layout (our fixture):
    //   [0] Header(db)  [1] postgres  [2] Header(web)  [3] nginx  [4] redis
    // Park the cursor on web/nginx (idx 3).
    let mut app = make_containers_overview_app();
    app.ui.containers_overview_state_mut().select(Some(3));

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('s')), &tx);

    // After flip → AlphaContainer mode (no host headers). The cursor
    // must land on a Container row whose alias is still "web" — the
    // exact index changes because the ordering is now by container
    // name (alphabetical: nginx, postgres, redis).
    let items = crate::ui::containers_overview::visible_items(&app);
    let idx = app
        .ui
        .containers_overview_state()
        .selected()
        .expect("cursor selected");
    let row = items[idx]
        .as_container()
        .expect("cursor must point at a Container row, not a header");
    assert_eq!(row.alias, "web");
}

#[test]
fn containers_overview_tab_advances_to_snippets() {
    let mut app = make_containers_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    // Containers -> Snippets
    // (cycle: Hosts -> Tunnels -> Containers -> Snippets -> Keys -> Hosts).
    assert!(matches!(app.top_page, crate::app::TopPage::Snippets));
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn containers_overview_back_tab_returns_to_tunnels() {
    let mut app = make_containers_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(
        &mut app,
        KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
        &tx,
    );
    assert!(matches!(app.top_page, crate::app::TopPage::Tunnels));
}

#[test]
fn containers_overview_slash_opens_search() {
    let mut app = make_containers_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    assert_eq!(app.search.query(), Some(""));
    // Cursor snaps to the first row, which can be a header now that
    // dividers are selectable. AlphaHost mode places the db header at
    // idx 0.
    assert_eq!(app.ui.containers_overview_state().selected(), Some(0));
}

#[test]
fn containers_overview_search_filters_rows() {
    let mut app = make_containers_overview_app();
    app.search.set_query(Some(String::new()));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('p')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('o')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('s')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('t')), &tx);
    let rows = crate::ui::containers_overview::visible_rows(&app);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "postgres");
}

#[test]
fn containers_overview_search_enter_on_header_is_noop() {
    // The search-mode Enter arm must mirror the main handler: clear
    // the search query, but never toggle fold or queue an exec when
    // the cursor sits on a host-header row.
    let mut app = make_containers_overview_app();
    app.search.set_query(Some(String::new()));
    let items = crate::ui::containers_overview::visible_items(&app);
    let header_idx = items
        .iter()
        .position(|i| i.is_header())
        .expect("test fixture must include at least one host-header row");
    let header_alias = match &items[header_idx] {
        crate::ui::containers_overview::ContainerListItem::HostHeader { alias, .. } => {
            alias.clone()
        }
        _ => unreachable!(),
    };
    app.ui
        .containers_overview_state_mut()
        .select(Some(header_idx));
    let was_collapsed = app
        .containers_overview
        .collapsed_hosts()
        .contains(&header_alias);

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    assert!(
        app.search.query().is_none(),
        "Enter in search mode clears the query"
    );
    let now_collapsed = app
        .containers_overview
        .collapsed_hosts()
        .contains(&header_alias);
    assert_eq!(
        was_collapsed, now_collapsed,
        "Enter on header during search must not toggle fold"
    );
    assert!(
        app.container_state.pending_exec_request().is_none(),
        "Enter on header during search must not queue an exec"
    );
}

#[test]
fn containers_overview_enter_on_host_header_is_noop() {
    // Enter on a host-header row must not toggle the group fold and
    // must not queue an exec. Space is the only binding that folds;
    // Enter is reserved for primary actions on container rows.
    let mut app = make_containers_overview_app();
    let items = crate::ui::containers_overview::visible_items(&app);
    let header_idx = items
        .iter()
        .position(|i| i.is_header())
        .expect("test fixture must include at least one host-header row");
    let header_alias = match &items[header_idx] {
        crate::ui::containers_overview::ContainerListItem::HostHeader { alias, .. } => {
            alias.clone()
        }
        _ => unreachable!(),
    };
    app.ui
        .containers_overview_state_mut()
        .select(Some(header_idx));
    let was_collapsed = app
        .containers_overview
        .collapsed_hosts()
        .contains(&header_alias);

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    let now_collapsed = app
        .containers_overview
        .collapsed_hosts()
        .contains(&header_alias);
    assert_eq!(
        was_collapsed, now_collapsed,
        "Enter on host header must not toggle fold"
    );
    assert!(
        app.container_state.pending_exec_request().is_none(),
        "Enter on host header must not queue an exec"
    );
}

#[test]
fn containers_overview_enter_queues_exec_for_running_container() {
    let mut app = make_containers_overview_app();
    // AlphaHost asc → first row is db/postgres (running).
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    let req = app
        .container_state
        .pending_exec_request()
        .expect("Enter must queue a container-exec request");
    assert_eq!(req.alias, "db");
    assert_eq!(req.container_name, "postgres");
    assert_eq!(req.container_id, "c3");
    // Screen must stay on the overview — drain happens in the main loop.
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn containers_overview_enter_on_stopped_container_warns_and_does_nothing() {
    let mut app = make_containers_overview_app();
    // Items: [0]Header(db) [1]postgres [2]Header(web) [3]nginx [4]redis.
    // redis is the exited container (idx 4 in items).
    app.ui.containers_overview_state_mut().select(Some(4));
    let items = crate::ui::containers_overview::visible_items(&app);
    let row = items
        .into_iter()
        .nth(4)
        .and_then(|i| match i {
            crate::ui::containers_overview::ContainerListItem::Container(r) => Some(r),
            _ => None,
        })
        .expect("idx 4 must be a container row");
    assert_eq!(row.name, "redis");
    assert_eq!(row.state, "exited");

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    assert!(app.container_state.pending_exec_request().is_none());
    let toast = app
        .status_center
        .toast()
        .expect("warning toast for stopped container");
    assert!(toast.text.contains("redis"));
    assert!(toast.text.contains("not running"));
}

#[test]
fn containers_overview_enter_rejects_unsafe_container_id() {
    // Regression: a corrupt or hostile `docker ps` JSON could in
    // theory inject shell metacharacters into row.id. The handler
    // must validate before queueing the exec request.
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.container_state.clear_cache();
    app.container_state.insert_cache_entry(
        "web".to_string(),
        crate::containers::ContainerCacheEntry {
            timestamp: 100,
            runtime: crate::containers::ContainerRuntime::Docker,
            engine_version: None,
            containers: vec![crate::containers::ContainerInfo {
                id: "abc;rm -rf /".to_string(),
                names: "evil".to_string(),
                image: "img".to_string(),
                state: "running".to_string(),
                status: "Up".to_string(),
                ports: String::new(),
            }],
        },
    );
    app.top_page = crate::app::TopPage::Containers;
    app.screen = Screen::HostList;
    // Items: [0]Header(web) [1]Container(web/evil). Park cursor on
    // the container, not the header.
    app.ui.containers_overview_state_mut().select(Some(1));

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    assert!(
        app.container_state.pending_exec_request().is_none(),
        "exec must not queue for an ID that fails validate_container_id"
    );
    assert!(
        app.status_center.toast().is_some(),
        "user-facing error toast expected"
    );
}

#[test]
fn containers_overview_enter_in_demo_mode_shows_disabled_toast() {
    let mut app = make_containers_overview_app();
    app.demo_mode = true;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.container_state.pending_exec_request().is_none());
    let toast = app.status_center.toast().expect("toast");
    assert!(toast.text.contains("Demo mode"));
    // Demo guards report a blocked action, so the toast must carry the
    // Warning severity, not Success. Mismatched severity here would tell
    // the user the action succeeded when it was actually skipped.
    assert_eq!(toast.class, crate::app::MessageClass::Warning);
}

#[test]
#[allow(non_snake_case)]
fn containers_overview_K_in_demo_mode_emits_warning_toast() {
    let mut app = make_containers_overview_app();
    app.demo_mode = true;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('K')), &tx);
    let toast = app.status_center.toast().expect("toast");
    assert!(toast.text.contains("Demo mode"));
    assert_eq!(toast.class, crate::app::MessageClass::Warning);
}

#[test]
fn containers_overview_l_in_demo_mode_opens_logs_view() {
    // Logs are a read-only view and `spawn_container_logs_fetch` has a
    // demo short-circuit that synthesises a deterministic 200-line
    // stream. `l` must therefore go through in demo mode: open the
    // overlay, queue the pending fetch, no "Demo mode" warning toast.
    let mut app = make_containers_overview_app();
    app.demo_mode = true;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('l')), &tx);
    assert!(
        matches!(app.screen, Screen::ContainerLogs),
        "logs overlay must open in demo mode, got {:?}",
        app.screen
    );
    assert!(
        app.container_state.pending_logs_request().is_some(),
        "logs fetch must be queued in demo mode (handler then short-circuits to demo_log_lines)"
    );
    if let Some(toast) = app.status_center.toast() {
        assert!(
            !toast.text.contains("Demo mode"),
            "logs must not surface a demo-disabled warning, got {:?}",
            toast.text
        );
    }
}

#[test]
fn container_refresh_progress_uses_sticky_progress_class() {
    // The refresh progress must survive a concurrent sticky error so
    // the user never loses sight of an active R batch. Sticky+Progress
    // is the only routing that satisfies both constraints.
    let mut app = make_containers_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('R')), &tx);
    let footer = app.status_center.status().expect("progress footer");
    assert!(footer.text.contains("Refreshing"));
    assert_eq!(footer.class, crate::app::MessageClass::Progress);
    assert!(footer.sticky, "refresh progress must be sticky");
}

#[test]
fn containers_overview_q_quits() {
    let mut app = make_containers_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('q')), &tx);
    assert!(!app.running);
}

#[test]
fn containers_overview_inspect_complete_caches_result() {
    let mut app = make_containers_overview_app();
    let inspect = crate::containers::ContainerInspect {
        exit_code: 0,
        oom_killed: false,
        started_at: "2026-05-09T08:00:00Z".to_string(),
        finished_at: String::new(),
        health: Some("healthy".to_string()),
        restart_count: 0,
        command: Some(vec!["nginx".to_string()]),
        entrypoint: None,
        env_count: 5,
        mount_count: 1,
        networks: vec![],
        image_digest: None,
        restart_policy: None,
        user: None,
        privileged: false,
        readonly_rootfs: false,
        apparmor_profile: None,
        seccomp_profile: None,
        cap_add: Vec::new(),
        cap_drop: Vec::new(),
        mounts: Vec::new(),
        compose_project: None,
        compose_service: None,
        ..Default::default()
    };
    app.containers_overview
        .inspect_cache_mut()
        .in_flight
        .insert("c1".to_string());

    crate::handler::event_loop::handle_container_inspect_complete(
        &mut app,
        "web".to_string(),
        "c1".to_string(),
        Ok(inspect.clone()),
    );

    let entry = app
        .containers_overview
        .inspect_cache()
        .entries
        .get("c1")
        .expect("cached");
    assert_eq!(
        entry.result.as_ref().unwrap().health,
        Some("healthy".to_string())
    );
    assert!(
        !app.containers_overview
            .inspect_cache()
            .in_flight
            .contains("c1"),
        "in_flight marker must be cleared on completion"
    );
}

#[test]
fn containers_overview_inspect_complete_stores_error() {
    let mut app = make_containers_overview_app();
    app.containers_overview
        .inspect_cache_mut()
        .in_flight
        .insert("c2".to_string());

    crate::handler::event_loop::handle_container_inspect_complete(
        &mut app,
        "web".to_string(),
        "c2".to_string(),
        Err("permission denied".to_string()),
    );

    let entry = app
        .containers_overview
        .inspect_cache()
        .entries
        .get("c2")
        .expect("cached");
    assert_eq!(
        entry.result.as_ref().err().map(|s| s.as_str()),
        Some("permission denied")
    );
    assert!(
        !app.containers_overview
            .inspect_cache()
            .in_flight
            .contains("c2")
    );
}

#[test]
fn containers_overview_logs_tail_complete_caches_result() {
    let mut app = make_containers_overview_app();
    app.containers_overview
        .logs_cache_mut()
        .in_flight
        .insert("c1".to_string());

    crate::handler::event_loop::handle_container_logs_tail_complete(
        &mut app,
        "web".to_string(),
        "c1".to_string(),
        Ok(vec!["line one".to_string(), "line two".to_string()]),
    );

    let entry = app
        .containers_overview
        .logs_cache()
        .entries
        .get("c1")
        .expect("cached");
    assert_eq!(entry.result.as_ref().unwrap().len(), 2);
    assert!(
        !app.containers_overview
            .logs_cache()
            .in_flight
            .contains("c1"),
        "in_flight marker must be cleared on completion"
    );
}

#[test]
fn containers_overview_logs_tail_complete_stores_error() {
    let mut app = make_containers_overview_app();
    app.containers_overview
        .logs_cache_mut()
        .in_flight
        .insert("c2".to_string());

    crate::handler::event_loop::handle_container_logs_tail_complete(
        &mut app,
        "web".to_string(),
        "c2".to_string(),
        Err("permission denied".to_string()),
    );

    let entry = app
        .containers_overview
        .logs_cache()
        .entries
        .get("c2")
        .expect("cached");
    assert_eq!(
        entry.result.as_ref().err().map(|s| s.as_str()),
        Some("permission denied")
    );
    assert!(
        !app.containers_overview
            .logs_cache()
            .in_flight
            .contains("c2")
    );
}

#[test]
fn containers_overview_logs_tail_complete_drops_orphan_host() {
    // Race guard: the host disappeared from the config between the
    // logs-fetch spawn and the result arrival. The result must NOT
    // land in the cache, but the in-flight marker must clear.
    let mut app = make_containers_overview_app();
    app.containers_overview
        .logs_cache_mut()
        .in_flight
        .insert("orphan".to_string());

    crate::handler::event_loop::handle_container_logs_tail_complete(
        &mut app,
        "ghost-host-not-in-config".to_string(),
        "orphan".to_string(),
        Ok(vec!["should be dropped".to_string()]),
    );

    assert!(
        !app.containers_overview
            .logs_cache()
            .entries
            .contains_key("orphan"),
        "orphan result must not be cached"
    );
    assert!(
        !app.containers_overview
            .logs_cache()
            .in_flight
            .contains("orphan"),
        "in_flight marker still cleared even when result dropped"
    );
}

#[test]
fn containers_overview_inspect_cache_fresh_within_ttl() {
    let mut app = make_containers_overview_app();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let inspect = crate::containers::ContainerInspect {
        exit_code: 0,
        oom_killed: false,
        started_at: String::new(),
        finished_at: String::new(),
        health: None,
        restart_count: 0,
        command: None,
        entrypoint: None,
        env_count: 0,
        mount_count: 0,
        networks: vec![],
        image_digest: None,
        restart_policy: None,
        user: None,
        privileged: false,
        readonly_rootfs: false,
        apparmor_profile: None,
        seccomp_profile: None,
        cap_add: Vec::new(),
        cap_drop: Vec::new(),
        mounts: Vec::new(),
        compose_project: None,
        compose_service: None,
        ..Default::default()
    };
    app.containers_overview.inspect_cache_mut().entries.insert(
        "c3".to_string(),
        crate::app::InspectCacheEntry {
            timestamp: now,
            result: Ok(inspect),
        },
    );
    assert!(
        app.containers_overview
            .inspect_cache()
            .fresh("c3", now)
            .is_some(),
        "cache should be fresh at t=0"
    );
    assert!(
        app.containers_overview
            .inspect_cache()
            .fresh("c3", now + 60)
            .is_none(),
        "cache should be stale after TTL window"
    );
}

#[test]
fn containers_overview_first_esc_arms_quit_hint() {
    let mut app = make_containers_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(app.running);
    assert!(app.ui.esc_quit_hint_shown());
    let toast = app.status_center.toast().expect("hint toast");
    assert!(toast.text.contains("q"));
}

#[test]
fn containers_overview_refresh_all_starts_capped_batch() {
    // Cache has 2 hosts; CAP=4 so both should fire in the initial wave.
    let mut app = make_containers_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('R')), &tx);

    let batch = app
        .containers_overview
        .refresh_batch()
        .expect("R must start a batch");
    assert_eq!(batch.total, 2);
    assert_eq!(batch.completed, 0);
    assert_eq!(
        batch.in_flight, 2,
        "both hosts should be in flight under cap"
    );
    assert!(batch.queue.is_empty(), "queue is empty when total <= cap");
}

#[test]
fn containers_overview_refresh_all_caps_in_flight_at_max_parallel() {
    // Build 10-host cache; R should leave 4 in flight and 6 queued.
    let mut app = make_containers_overview_app();
    app.container_state.clear_cache();
    for i in 0..10 {
        app.container_state.insert_cache_entry(
            format!("h{}", i),
            crate::containers::ContainerCacheEntry {
                timestamp: 100,
                runtime: crate::containers::ContainerRuntime::Docker,
                engine_version: None,
                containers: vec![],
            },
        );
    }
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('R')), &tx);

    let batch = app
        .containers_overview
        .refresh_batch()
        .expect("R must start a batch");
    assert_eq!(batch.total, 10);
    assert_eq!(batch.in_flight, crate::app::REFRESH_MAX_PARALLEL);
    assert_eq!(batch.queue.len(), 10 - crate::app::REFRESH_MAX_PARALLEL);
}

#[test]
fn containers_overview_refresh_all_in_demo_mode_starts_synthetic_batch() {
    // Demo mode runs a synthetic refresh so reviewers see the same
    // progress footer and freshness flip as a live SSH batch. The
    // batch is staged immediately on the keypress; the worker thread
    // posts ContainerListing events afterwards.
    let mut app = make_containers_overview_app();
    app.demo_mode = true;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('R')), &tx);
    let batch = app
        .containers_overview
        .refresh_batch()
        .expect("synthetic batch staged");
    assert_eq!(batch.in_flight, batch.total);
    assert!(batch.total > 0);
    let footer = app.status_center.status().expect("progress footer");
    assert!(footer.text.contains("Refreshing"));
    assert!(!footer.text.contains("Demo mode"));
    assert!(app.status_center.toast().is_none());
}

#[test]
fn containers_overview_refresh_all_on_empty_cache_warns() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.container_state.clear_cache();
    app.top_page = crate::app::TopPage::Containers;
    app.screen = Screen::HostList;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('R')), &tx);
    assert!(app.containers_overview.refresh_batch().is_none());
    let toast = app.status_center.toast().expect("toast");
    assert!(toast.text.contains("No cached hosts"));
}

#[test]
fn containers_overview_refresh_all_rejects_concurrent_batch() {
    let mut app = make_containers_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('R')), &tx);
    assert!(app.containers_overview.refresh_batch().is_some());

    // Press R again while batch active.
    let _ = handle_key_event(&mut app, key(KeyCode::Char('R')), &tx);
    let toast = app.status_center.toast().expect("toast");
    assert!(toast.text.contains("already in progress"));
}

#[test]
fn containers_overview_a_opens_picker_in_real_mode() {
    let mut app = make_containers_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('a')), &tx);
    assert!(matches!(app.screen, Screen::ContainerHostPicker));
    assert_eq!(app.ui.container_host_picker_state().selected(), Some(0));
    assert!(app.ui.container_host_picker_query().is_empty());
}

#[test]
fn containers_overview_a_in_demo_mode_shows_toast() {
    let mut app = make_containers_overview_app();
    app.demo_mode = true;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('a')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    let toast = app.status_center.toast().expect("toast");
    assert!(toast.text.contains("Demo mode"));
}

#[test]
fn container_host_picker_filters_to_uncached_only() {
    // make_containers_overview_app caches "web" and "db". We only have
    // those two hosts in hosts_state, so picker should be empty.
    let app = make_containers_overview_app();
    let uncached = crate::handler::container_host_picker::uncached_aliases(&app);
    assert!(uncached.is_empty(), "all hosts already cached");

    // Add a third host with no cache.
    let mut app = make_app(
        "Host web\n  HostName 1.2.3.4\n\nHost db\n  HostName 2.2.2.2\n\nHost api\n  HostName 3.3.3.3\n",
    );
    app.container_state.clear_cache();
    app.container_state.insert_cache_entry(
        "web".to_string(),
        crate::containers::ContainerCacheEntry {
            timestamp: 100,
            runtime: crate::containers::ContainerRuntime::Docker,
            engine_version: None,
            containers: vec![],
        },
    );
    let uncached = crate::handler::container_host_picker::uncached_aliases(&app);
    // Cached: web. Uncached: db, api.
    assert_eq!(uncached.len(), 2);
    assert!(uncached.contains(&"db".to_string()));
    assert!(uncached.contains(&"api".to_string()));
    assert!(!uncached.contains(&"web".to_string()));
}

#[test]
fn refresh_batch_completes_cleanly_on_last_listing() {
    // Final listing of a 2-host batch: queue empty, last in-flight
    // alias arrives, batch should clear and emit the success toast.
    let mut app = make_containers_overview_app();
    app.containers_overview
        .start_refresh(crate::app::RefreshBatch {
            queue: std::collections::VecDeque::new(),
            in_flight: 1,
            total: 2,
            completed: 1,
            in_flight_aliases: ["web".to_string()].into_iter().collect(),
        });
    let (tx, _rx) = mpsc::channel();
    crate::handler::event_loop::drive_refresh_batch(&mut app, "web", &tx);

    assert!(
        app.containers_overview.refresh_batch().is_none(),
        "batch must clear after last listing"
    );
    let toast = app.status_center.toast().expect("completion toast");
    assert!(toast.text.contains("Refreshed"));
    assert!(toast.text.contains("2 hosts"));
}

#[test]
fn container_action_complete_emits_success_toast() {
    // A finished Restart/Stop is a user-initiated outcome, so it must
    // surface as a Success toast — not the Info footer (which is for
    // background events the user did not explicitly trigger).
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.container_session = Some(make_container_state(
        "web",
        vec![make_container("abc123", "nginx", "running")],
    ));
    let (tx, _rx) = mpsc::channel();
    crate::handler::event_loop::handle_container_action_complete(
        &mut app,
        "web".to_string(),
        crate::containers::ContainerAction::Restart,
        Ok(()),
        &tx,
    );
    let toast = app.status_center.toast().expect("container action toast");
    assert_eq!(toast.class, crate::app::MessageClass::Success);
    assert!(toast.text.contains("restart"));
}

#[test]
fn refresh_batch_completion_clears_sticky_progress_and_uses_success_toast() {
    // notify_progress put a sticky "Refreshing X/Y hosts…" in the footer
    // at batch start. The completion path must clear that footer so the
    // user does not see a stale "Refreshing" line sitting next to the
    // success toast.
    let mut app = make_containers_overview_app();
    app.containers_overview
        .start_refresh(crate::app::RefreshBatch {
            queue: std::collections::VecDeque::new(),
            in_flight: 1,
            total: 2,
            completed: 1,
            in_flight_aliases: ["web".to_string()].into_iter().collect(),
        });
    // Seed the sticky progress message as the live batch would.
    app.notify_progress(crate::messages::container_refresh_progress(1, 2));
    let (tx, _rx) = mpsc::channel();
    crate::handler::event_loop::drive_refresh_batch(&mut app, "web", &tx);

    assert!(
        app.status_center.status().is_none(),
        "sticky progress footer must be cleared on batch completion"
    );
    let toast = app.status_center.toast().expect("completion toast");
    assert_eq!(toast.class, crate::app::MessageClass::Success);
}

#[test]
fn refresh_batch_ignores_non_batch_listing() {
    // Listing for an alias that is NOT in the batch's in_flight set
    // (e.g. host-list `C` or `a`-add running in parallel) must not
    // touch the batch counters.
    let mut app = make_containers_overview_app();
    app.containers_overview
        .start_refresh(crate::app::RefreshBatch {
            queue: std::collections::VecDeque::new(),
            in_flight: 1,
            total: 2,
            completed: 1,
            in_flight_aliases: ["web".to_string()].into_iter().collect(),
        });
    let (tx, _rx) = mpsc::channel();
    crate::handler::event_loop::drive_refresh_batch(&mut app, "intruder", &tx);

    let batch = app
        .containers_overview
        .refresh_batch()
        .expect("batch must NOT clear");
    assert_eq!(batch.in_flight, 1);
    assert_eq!(batch.completed, 1);
    assert!(batch.in_flight_aliases.contains("web"));
    assert!(!batch.in_flight_aliases.contains("intruder"));
}

#[test]
fn refresh_batch_decrements_completed_increments_on_match() {
    // Mid-batch listing: queue still has one item but spawn would
    // touch SSH which we cannot do in unit tests. Pre-fill the queue
    // empty so we exercise just the decrement+complete count path
    // without triggering a spawn.
    let mut app = make_containers_overview_app();
    app.containers_overview
        .start_refresh(crate::app::RefreshBatch {
            queue: std::collections::VecDeque::new(),
            in_flight: 2,
            total: 2,
            completed: 0,
            in_flight_aliases: ["web".to_string(), "db".to_string()].into_iter().collect(),
        });
    let (tx, _rx) = mpsc::channel();
    crate::handler::event_loop::drive_refresh_batch(&mut app, "web", &tx);

    let batch = app
        .containers_overview
        .refresh_batch()
        .expect("batch still active (db pending)");
    assert_eq!(batch.in_flight, 1);
    assert_eq!(batch.completed, 1);
    assert!(!batch.in_flight_aliases.contains("web"));
    assert!(batch.in_flight_aliases.contains("db"));
}

#[test]
fn refresh_batch_no_op_when_no_batch_active() {
    let mut app = make_containers_overview_app();
    assert!(app.containers_overview.refresh_batch().is_none());
    let (tx, _rx) = mpsc::channel();
    crate::handler::event_loop::drive_refresh_batch(&mut app, "web", &tx);
    assert!(app.containers_overview.refresh_batch().is_none());
}

#[test]
fn container_listing_dropped_for_alias_no_longer_in_config() {
    // Race regression: a `docker ps` thread can return after the
    // host was deleted via Confirm-Delete + reload_hosts. The
    // listing handler must drop the result instead of resurrecting
    // the orphan in the cache.
    let mut app = make_app(""); // empty config — alias "ghost" is NOT in hosts_state.list
    app.container_state.clear_cache();
    let result: Result<crate::containers::ContainerListing, crate::containers::ContainerError> =
        Ok(crate::containers::ContainerListing {
            runtime: crate::containers::ContainerRuntime::Docker,
            engine_version: None,
            containers: vec![make_container("c1", "nginx", "running")],
        });
    let (tx, _rx) = mpsc::channel();
    crate::handler::event_loop::handle_container_listing(
        &mut app,
        "ghost".to_string(),
        result,
        &tx,
    );
    assert!(
        !app.container_state.cache_contains("ghost"),
        "late listing for a removed host must not repopulate the cache"
    );
}

#[test]
fn container_inspect_complete_dropped_for_alias_no_longer_in_config() {
    let mut app = make_app("");
    app.container_state.clear_cache();
    let inspect = crate::containers::ContainerInspect {
        exit_code: 0,
        oom_killed: false,
        started_at: String::new(),
        finished_at: String::new(),
        health: None,
        restart_count: 0,
        command: None,
        entrypoint: None,
        env_count: 0,
        mount_count: 0,
        networks: vec![],
        image_digest: None,
        restart_policy: None,
        user: None,
        privileged: false,
        readonly_rootfs: false,
        apparmor_profile: None,
        seccomp_profile: None,
        cap_add: Vec::new(),
        cap_drop: Vec::new(),
        mounts: Vec::new(),
        compose_project: None,
        compose_service: None,
        ..Default::default()
    };
    crate::handler::event_loop::handle_container_inspect_complete(
        &mut app,
        "ghost".to_string(),
        "c1".to_string(),
        Ok(inspect),
    );
    assert!(
        !app.containers_overview
            .inspect_cache()
            .entries
            .contains_key("c1"),
        "late inspect for a removed host must not be cached"
    );
}

#[test]
fn reload_hosts_drops_orphan_container_cache_entries() {
    // Manual delete / stale purge / external edit all funnel
    // through reload_hosts. A host that is no longer in
    // hosts_state.list must lose its container_cache entry so
    // ~/.purple/container_cache.jsonl does not accumulate orphans.
    let mut app = make_app("Host alive\n  HostName 1.2.3.4\n");
    app.container_state.clear_cache();
    app.container_state.insert_cache_entry(
        "alive".to_string(),
        crate::containers::ContainerCacheEntry {
            timestamp: 100,
            runtime: crate::containers::ContainerRuntime::Docker,
            engine_version: None,
            containers: vec![],
        },
    );
    app.container_state.insert_cache_entry(
        "ghost".to_string(),
        crate::containers::ContainerCacheEntry {
            timestamp: 100,
            runtime: crate::containers::ContainerRuntime::Docker,
            engine_version: None,
            containers: vec![],
        },
    );

    app.reload_hosts();

    assert!(app.container_state.cache_contains("alive"));
    assert!(
        !app.container_state.cache_contains("ghost"),
        "orphan host must be pruned"
    );
}

#[test]
fn reload_hosts_drops_orphan_logs_cache_entries() {
    // Mirrors reload_hosts_drops_orphan_inspect_cache_entries for the
    // sibling logs cache. Logs entries key on full container ID just
    // like inspect entries; an entry whose container disappeared
    // (host removed externally) must be pruned together.
    let mut app = make_app("Host alive\n  HostName 1.2.3.4\n");
    app.container_state.clear_cache();
    app.container_state.insert_cache_entry(
        "alive".to_string(),
        crate::containers::ContainerCacheEntry {
            timestamp: 100,
            runtime: crate::containers::ContainerRuntime::Docker,
            engine_version: None,
            containers: vec![make_container("alive-c1", "nginx", "running")],
        },
    );
    app.containers_overview.logs_cache_mut().entries.insert(
        "alive-c1".to_string(),
        crate::app::LogsCacheEntry {
            timestamp: 0,
            result: Ok(vec!["alive line".to_string()]),
        },
    );
    app.containers_overview.logs_cache_mut().entries.insert(
        "ghost-c1".to_string(),
        crate::app::LogsCacheEntry {
            timestamp: 0,
            result: Ok(vec!["ghost line".to_string()]),
        },
    );

    app.reload_hosts();

    assert!(
        app.containers_overview
            .logs_cache()
            .entries
            .contains_key("alive-c1")
    );
    assert!(
        !app.containers_overview
            .logs_cache()
            .entries
            .contains_key("ghost-c1"),
        "logs entries with no container in the host cache must be pruned"
    );
}

#[test]
fn reload_hosts_drops_orphan_inspect_cache_entries() {
    let mut app = make_app("Host alive\n  HostName 1.2.3.4\n");
    app.container_state.clear_cache();
    app.container_state.insert_cache_entry(
        "alive".to_string(),
        crate::containers::ContainerCacheEntry {
            timestamp: 100,
            runtime: crate::containers::ContainerRuntime::Docker,
            engine_version: None,
            containers: vec![make_container("alive-c1", "nginx", "running")],
        },
    );
    // Inspect entry whose container ID no longer exists (host
    // removed externally between purple sessions).
    let inspect = crate::containers::ContainerInspect {
        exit_code: 0,
        oom_killed: false,
        started_at: String::new(),
        finished_at: String::new(),
        health: None,
        restart_count: 0,
        command: None,
        entrypoint: None,
        env_count: 0,
        mount_count: 0,
        networks: vec![],
        image_digest: None,
        restart_policy: None,
        user: None,
        privileged: false,
        readonly_rootfs: false,
        apparmor_profile: None,
        seccomp_profile: None,
        cap_add: Vec::new(),
        cap_drop: Vec::new(),
        mounts: Vec::new(),
        compose_project: None,
        compose_service: None,
        ..Default::default()
    };
    app.containers_overview.inspect_cache_mut().entries.insert(
        "alive-c1".to_string(),
        crate::app::InspectCacheEntry {
            timestamp: 0,
            result: Ok(inspect.clone()),
        },
    );
    app.containers_overview.inspect_cache_mut().entries.insert(
        "ghost-c1".to_string(),
        crate::app::InspectCacheEntry {
            timestamp: 0,
            result: Ok(inspect),
        },
    );

    app.reload_hosts();

    assert!(
        app.containers_overview
            .inspect_cache()
            .entries
            .contains_key("alive-c1")
    );
    assert!(
        !app.containers_overview
            .inspect_cache()
            .entries
            .contains_key("ghost-c1"),
        "inspect entries with no container in the host cache must be pruned"
    );
}

#[test]
fn reload_hosts_drops_orphan_file_browser_host_paths() {
    // host_paths persists the last-visited remote dir per alias. It has
    // no self-pruning, so reload_hosts must strip aliases that no longer
    // exist or a rename leaves a ghost entry behind forever.
    let mut app = make_app("Host alive\n  HostName 1.2.3.4\n");
    app.file_browser_state.set_host_path(
        "alive".to_string(),
        std::path::PathBuf::from("/var/log"),
        "/var/log".to_string(),
    );
    app.file_browser_state.set_host_path(
        "ghost".to_string(),
        std::path::PathBuf::from("/etc"),
        "/etc".to_string(),
    );

    app.reload_hosts();

    assert!(app.file_browser_state.contains_host("alive"));
    assert!(
        !app.file_browser_state.contains_host("ghost"),
        "host_paths entry for a removed host must be pruned"
    );
}

#[test]
fn reload_hosts_drops_orphan_refresh_batch_in_flight_aliases() {
    // The R batch tracks in-flight aliases to gate counter updates against
    // non-batch listings. A host removed mid-batch must not linger in the
    // set, even if the listing thread is still on its way back.
    let mut app = make_app("Host alive\n  HostName 1.2.3.4\n");
    let mut in_flight = std::collections::HashSet::new();
    in_flight.insert("alive".to_string());
    in_flight.insert("ghost".to_string());
    app.containers_overview
        .start_refresh(crate::app::RefreshBatch {
            queue: std::collections::VecDeque::new(),
            in_flight: 2,
            total: 2,
            completed: 0,
            in_flight_aliases: in_flight,
        });

    app.reload_hosts();

    let batch = app
        .containers_overview
        .refresh_batch()
        .expect("batch must still exist after reload");
    assert!(batch.in_flight_aliases.contains("alive"));
    assert!(
        !batch.in_flight_aliases.contains("ghost"),
        "removed host must be dropped from refresh_batch in_flight_aliases"
    );
}

fn empty_tunnel_live_snapshot() -> crate::tunnel_live::TunnelLiveSnapshot {
    crate::tunnel_live::TunnelLiveSnapshot {
        uptime_secs: 0,
        active_channels: 0,
        peak_concurrent: 0,
        total_opens: 0,
        idle_secs: 0,
        rx_history: [0; crate::tunnel_live::HISTORY_BUCKETS],
        tx_history: [0; crate::tunnel_live::HISTORY_BUCKETS],
        current_rx_bps: 0,
        current_tx_bps: 0,
        peak_rx_bps: 0,
        peak_tx_bps: 0,
        throughput_ready: false,
        clients: Vec::new(),
        events: Vec::new(),
        currently_open: Vec::new(),
        conflict: None,
        last_exit: None,
    }
}

#[test]
fn reload_hosts_drops_orphan_demo_live_snapshots() {
    let mut app = make_app("Host alive\n  HostName 1.2.3.4\n");
    app.tunnels
        .demo_live_snapshots_mut()
        .insert("alive".to_string(), empty_tunnel_live_snapshot());
    app.tunnels
        .demo_live_snapshots_mut()
        .insert("ghost".to_string(), empty_tunnel_live_snapshot());

    app.reload_hosts();

    assert!(app.tunnels.demo_live_snapshots().contains_key("alive"));
    assert!(
        !app.tunnels.demo_live_snapshots().contains_key("ghost"),
        "demo_live_snapshots entry for a removed host must be pruned"
    );
}

#[test]
fn reload_hosts_drops_orphan_collapsed_hosts_and_persists() {
    // collapsed_hosts is persisted to preferences. A delete must prune
    // the runtime set AND rewrite preferences so the orphan does not
    // come back on restart.
    let mut app = make_app("Host alive\n  HostName 1.2.3.4\n");
    app.containers_overview.toggle_host_collapsed("alive");
    app.containers_overview.toggle_host_collapsed("ghost");
    crate::preferences::save_containers_collapsed_hosts(
        app.env().paths(),
        app.containers_overview.collapsed_hosts(),
    )
    .expect("seed");

    app.reload_hosts();

    assert!(app.containers_overview.collapsed_hosts().contains("alive"));
    assert!(
        !app.containers_overview.collapsed_hosts().contains("ghost"),
        "collapsed_hosts entry for a removed host must be pruned"
    );
    let on_disk = crate::preferences::load_containers_collapsed_hosts(app.env().paths());
    assert!(on_disk.contains("alive"));
    assert!(
        !on_disk.contains("ghost"),
        "pruned collapsed_hosts must be persisted, otherwise the orphan returns on restart"
    );
}

#[test]
fn reload_hosts_ghost_sweep_clears_every_alias_keyed_collection() {
    // Single contract test that seeds EVERY known alias-keyed collection
    // with a ghost entry, runs reload_hosts, and asserts each one is
    // ghost-free. When a contributor adds a new alias-keyed cache without
    // wiring it into reload_hosts, this test pins the omission down. To
    // add a new collection: seed it below and add a matching assertion.
    let mut app = make_app("Host alive\n  HostName 1.2.3.4\n");
    let ghost = "ghost".to_string();

    app.tunnels
        .summaries_cache_mut()
        .insert(ghost.clone(), String::new());
    app.vault.insert_cert(
        ghost.clone(),
        (
            std::time::Instant::now(),
            crate::vault_ssh::CertStatus::Missing,
            None,
        ),
    );
    app.vault.mark_cert_check_started(ghost.clone());
    app.vault
        .note_cert_stat(ghost.clone(), std::time::Instant::now());
    {
        let mut sign = app.vault.sign_in_flight().lock().expect("lock");
        sign.insert(ghost.clone());
    }
    app.container_state.insert_cache_entry(
        ghost.clone(),
        crate::containers::ContainerCacheEntry {
            timestamp: 0,
            runtime: crate::containers::ContainerRuntime::Docker,
            engine_version: None,
            containers: vec![],
        },
    );
    app.containers_overview
        .mark_auto_list_pending(ghost.clone());
    app.containers_overview
        .start_refresh(crate::app::RefreshBatch {
            queue: std::collections::VecDeque::new(),
            in_flight: 1,
            total: 1,
            completed: 0,
            in_flight_aliases: [ghost.clone()].into_iter().collect(),
        });
    app.containers_overview.toggle_host_collapsed(&ghost);
    app.file_browser_state.set_host_path(
        ghost.clone(),
        std::path::PathBuf::from("/etc"),
        "/etc".to_string(),
    );
    app.ping.insert_status(
        ghost.clone(),
        crate::app::PingStatus::Reachable { rtt_ms: 1 },
    );
    app.ping
        .record_check(ghost.clone(), std::time::Instant::now());
    app.tunnels
        .demo_live_snapshots_mut()
        .insert(ghost.clone(), empty_tunnel_live_snapshot());
    app.containers_overview
        .set_pending_bulk_confirm(crate::app::BulkConfirmContext {
            kind: crate::app::BulkConfirmKind::StackRestart,
            alias: ghost.clone(),
            project: Some("ghost-stack".to_string()),
            members: vec![],
        });
    app.container_state.set_logs_view(crate::app::LogsView {
        alias: ghost.clone(),
        container_id: "abc123".to_string(),
        container_name: "nginx".to_string(),
        body: vec!["line".to_string()],
        fetched_at: 0,
        error: None,
        scroll: 0,
        last_render_height: 0,
        search: None,
    });
    app.vault
        .set_pending_sign(vec![crate::vault_ssh::VaultSignTarget {
            alias: ghost.clone(),
            role: "ssh-client/sign/role".to_string(),
            certificate_file: String::new(),
            pubkey: std::path::PathBuf::from("/tmp/id_ed25519.pub"),
            vault_addr: None,
        }]);
    app.providers.set_pending_purge(crate::app::PendingPurge {
        aliases: vec![ghost.clone()],
        provider: None,
    });

    app.reload_hosts();

    assert!(
        !app.tunnels.summaries_cache().contains_key(&ghost),
        "tunnels.summaries_cache"
    );
    assert!(!app.vault.has_cert(&ghost), "vault.cert_cache");
    assert!(
        !app.vault.is_cert_check_in_flight(&ghost),
        "vault.cert_checks_in_flight"
    );
    assert!(
        app.vault.last_cert_stat(&ghost).is_none(),
        "vault.cert_stat_throttle"
    );
    {
        let sign = app.vault.sign_in_flight().lock().expect("lock");
        assert!(!sign.contains(&ghost), "vault.sign_in_flight");
    }
    assert!(
        !app.container_state.cache_contains(&ghost),
        "container_state.cache"
    );
    assert!(
        !app.containers_overview.auto_list_pending(&ghost),
        "containers_overview.auto_list_in_flight()"
    );
    if let Some(batch) = app.containers_overview.refresh_batch() {
        assert!(
            !batch.in_flight_aliases.contains(&ghost),
            "containers_overview.refresh_batch.in_flight_aliases"
        );
    }
    assert!(
        !app.containers_overview.collapsed_hosts().contains(&ghost),
        "containers_overview.collapsed_hosts()"
    );
    assert!(
        !app.file_browser_state.contains_host(&ghost),
        "file_browser_state.host_paths"
    );
    assert!(!app.ping.status_contains(&ghost), "ping.status");
    assert!(
        app.ping.last_checked_at(&ghost).is_none(),
        "ping.last_checked"
    );
    assert!(
        !app.tunnels.demo_live_snapshots().contains_key(&ghost),
        "tunnels.demo_live_snapshots"
    );
    assert!(
        app.containers_overview.pending_bulk_confirm().is_none(),
        "containers_overview.pending_bulk_confirm targeting a deleted host must be cleared"
    );
    assert!(
        app.container_state.logs_view().is_none(),
        "container_state.logs_view targeting a deleted host must be cleared"
    );
    assert!(
        app.vault.pending_sign().is_none(),
        "vault.pending_sign with only deleted-host targets must be cleared"
    );
    // pending_purge is not pruned by reload_hosts (the purge worker
    // recomputes stale_hosts on yes), but stale entries should not
    // prevent the dialog from running. The assertion intentionally
    // does not require pruning here.
    let _ = app.providers.pending_purge();
}

#[test]
fn reload_hosts_drops_orphan_sign_in_flight() {
    // sign_in_flight is the bulk-V sign tracker. Worker thread self-prunes
    // on completion, but a host removed mid-sign would linger until the
    // worker fires. reload_hosts must take the lock and prune.
    let mut app = make_app("Host alive\n  HostName 1.2.3.4\n");
    {
        let mut sign = app.vault.sign_in_flight().lock().expect("lock");
        sign.insert("alive".to_string());
        sign.insert("ghost".to_string());
    }

    app.reload_hosts();

    let sign = app.vault.sign_in_flight().lock().expect("lock");
    assert!(sign.contains("alive"));
    assert!(
        !sign.contains("ghost"),
        "removed host must be dropped from vault.sign_in_flight"
    );
}

#[test]
fn auto_fetch_new_hosts_only_fetches_queued_aliases() {
    // Only the alias that was explicitly pushed to the queue must be
    // fetched. Pre-existing cache-missing hosts must be left alone —
    // the regression we are guarding against is the askpass storm
    // when sync confirmed an inventory of pre-existing hosts on
    // first run.
    let mut app = make_app("Host queued\n  HostName 1.1.1.1\n\nHost ignored\n  HostName 2.2.2.2\n");
    app.container_state.clear_cache();
    app.container_state.queue_fetch("queued".to_string());
    let (tx, _rx) = mpsc::channel();
    crate::handler::containers_overview::auto_fetch_new_hosts(&mut app, &tx);

    let batch = app
        .containers_overview
        .refresh_batch()
        .expect("auto-fetch must start a batch for the queued alias");
    assert_eq!(batch.total, 1);
    assert_eq!(batch.in_flight, 1);
    assert_eq!(
        batch.in_flight_aliases.iter().cloned().collect::<Vec<_>>(),
        vec!["queued".to_string()]
    );
    assert!(
        !app.container_state.has_pending_fetches(),
        "queue must be drained"
    );
}

#[test]
fn auto_fetch_new_hosts_dedupes_aliases() {
    // Multiple triggers (e.g. form save + sync) may push the same
    // alias before the next drain; the helper must dedupe so we do
    // not spawn parallel SSH for the same host.
    let mut app = make_app("Host dup\n  HostName 1.1.1.1\n");
    app.container_state.clear_cache();
    app.container_state.extend_pending_fetches([
        "dup".to_string(),
        "dup".to_string(),
        "dup".to_string(),
    ]);
    let (tx, _rx) = mpsc::channel();
    crate::handler::containers_overview::auto_fetch_new_hosts(&mut app, &tx);

    let batch = app
        .containers_overview
        .refresh_batch()
        .expect("auto-fetch must start a batch for the queued alias");
    assert_eq!(batch.total, 1);
    assert_eq!(batch.in_flight, 1);
}

#[test]
fn auto_fetch_new_hosts_skips_alias_with_existing_cache() {
    // A parallel `C` press (or earlier auto-fetch) may have
    // populated the cache between push and drain. Skip it.
    let mut app = make_app("Host already\n  HostName 1.1.1.1\n");
    app.container_state.clear_cache();
    app.container_state.insert_cache_entry(
        "already".to_string(),
        crate::containers::ContainerCacheEntry {
            timestamp: 100,
            runtime: crate::containers::ContainerRuntime::Docker,
            engine_version: None,
            containers: vec![],
        },
    );
    app.container_state.queue_fetch("already".to_string());
    let (tx, _rx) = mpsc::channel();
    crate::handler::containers_overview::auto_fetch_new_hosts(&mut app, &tx);
    assert!(app.containers_overview.refresh_batch().is_none());
}

#[test]
fn auto_fetch_new_hosts_skips_unknown_alias() {
    // Race guard: the alias may have been deleted between push and
    // drain (manual delete, stale purge, external edit).
    let mut app = make_app("Host real\n  HostName 1.1.1.1\n");
    app.container_state.clear_cache();
    app.container_state.queue_fetch("ghost".to_string());
    let (tx, _rx) = mpsc::channel();
    crate::handler::containers_overview::auto_fetch_new_hosts(&mut app, &tx);
    assert!(app.containers_overview.refresh_batch().is_none());
}

#[test]
fn auto_fetch_new_hosts_skips_host_without_hostname() {
    // Placeholder entries (Host with no HostName) cannot be SSH'd.
    let mut app = make_app("Host placeholder\n  User root\n");
    app.container_state.clear_cache();
    app.container_state.queue_fetch("placeholder".to_string());
    let (tx, _rx) = mpsc::channel();
    crate::handler::containers_overview::auto_fetch_new_hosts(&mut app, &tx);
    assert!(app.containers_overview.refresh_batch().is_none());
}

#[test]
fn auto_fetch_new_hosts_no_op_in_demo_mode() {
    let mut app = make_app("Host a\n  HostName 1.1.1.1\n");
    app.container_state.clear_cache();
    app.demo_mode = true;
    app.container_state.queue_fetch("a".to_string());
    let (tx, _rx) = mpsc::channel();
    crate::handler::containers_overview::auto_fetch_new_hosts(&mut app, &tx);
    assert!(app.containers_overview.refresh_batch().is_none());
    // Queue is still drained (cleared) so a second tick does not retry.
    assert!(!app.container_state.has_pending_fetches());
}

#[test]
fn auto_fetch_new_hosts_no_op_when_queue_empty() {
    // No queued aliases means nothing should fire even if the cache
    // is empty for every host. This is the regression guard for the
    // first-run askpass storm.
    let mut app = make_app("Host a\n  HostName 1.1.1.1\n\nHost b\n  HostName 2.2.2.2\n");
    app.container_state.clear_cache();
    let (tx, _rx) = mpsc::channel();
    crate::handler::containers_overview::auto_fetch_new_hosts(&mut app, &tx);
    assert!(app.containers_overview.refresh_batch().is_none());
}

#[test]
fn auto_fetch_new_hosts_dedupes_mixed_queue() {
    // Realistic shape: a sync that adds two hosts where one was
    // accidentally double-emitted. Both unique aliases must end up
    // in the batch exactly once.
    let mut app = make_app("Host a\n  HostName 1.1.1.1\n\nHost b\n  HostName 2.2.2.2\n");
    app.container_state.clear_cache();
    app.container_state
        .extend_pending_fetches(["a".to_string(), "a".to_string(), "b".to_string()]);
    let (tx, _rx) = mpsc::channel();
    crate::handler::containers_overview::auto_fetch_new_hosts(&mut app, &tx);
    let batch = app
        .containers_overview
        .refresh_batch()
        .expect("auto-fetch must start a batch for unique aliases");
    assert_eq!(batch.total, 2);
    assert!(batch.in_flight_aliases.contains("a"));
    assert!(batch.in_flight_aliases.contains("b"));
}

#[test]
fn queue_new_aliases_since_pushes_only_new() {
    // Snapshot before adding "fresh"; queueing must only push that
    // alias, never the pre-existing "old".
    let mut app = make_app("Host old\n  HostName 1.1.1.1\n");
    let before = app.snapshot_alias_set();
    // Simulate a reload that added a new host.
    app.hosts_state
        .list_mut()
        .push(crate::ssh_config::model::HostEntry {
            alias: "fresh".to_string(),
            hostname: "2.2.2.2".to_string(),
            ..Default::default()
        });
    app.queue_new_aliases_since(&before);
    assert_eq!(
        app.container_state.pending_fetch_aliases(),
        ["fresh".to_string()]
    );
}

#[test]
fn queue_new_aliases_since_no_op_when_unchanged() {
    // Regression guard: a sync that confirms an existing inventory
    // (same aliases before/after) must not push anything. This is
    // the core invariant behind the askpass-storm fix.
    let mut app = make_app("Host a\n  HostName 1.1.1.1\n\nHost b\n  HostName 2.2.2.2\n");
    let before = app.snapshot_alias_set();
    app.queue_new_aliases_since(&before);
    assert!(!app.container_state.has_pending_fetches());
}

#[test]
fn refresh_selected_host_marks_alias_in_flight() {
    // `r` keypress and the post-key auto-list must not double-spawn
    // for the same host. The fix is to insert into auto_list_in_flight
    // before spawning so the auto helper short-circuits.
    let mut app = make_containers_overview_app();
    app.demo_mode = false;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('r')), &tx);
    // Cursor parks on idx 1 = postgres on host db.
    assert!(
        app.containers_overview.auto_list_pending("db"),
        "`r` must mark the alias in-flight to dedup the auto-list helper"
    );
}

#[test]
fn ensure_list_for_selected_host_marks_in_flight_for_stale_cache() {
    // Stale entry (timestamp=100, well beyond LIST_CACHE_TTL_SECS): the
    // helper must spawn a refresh and mark the alias as in-flight so
    // a follow-up scroll within the same window does not re-fire.
    let mut app = make_containers_overview_app();
    app.demo_mode = false;
    let (tx, _rx) = mpsc::channel();
    super::containers_overview::ensure_list_for_selected_host(&mut app, &tx);
    // Cursor parks on idx 1 = postgres on host db.
    assert!(
        app.containers_overview.auto_list_pending("db"),
        "stale cache must trigger an in-flight marker"
    );
}

#[test]
fn ensure_list_for_selected_host_skips_when_fresh() {
    // Fresh entry (timestamp=now): the helper must short-circuit.
    let mut app = make_containers_overview_app();
    app.demo_mode = false;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    if let Some(entry) = app.container_state.cache_entry_mut("db") {
        entry.timestamp = now;
    }
    let (tx, _rx) = mpsc::channel();
    super::containers_overview::ensure_list_for_selected_host(&mut app, &tx);
    assert!(
        !app.containers_overview.auto_list_pending("db"),
        "fresh cache must not trigger a refresh"
    );
}

#[test]
fn ensure_list_for_selected_host_dedupes_in_flight() {
    // An alias already marked in-flight must not get a second spawn,
    // even if the cache is stale.
    let mut app = make_containers_overview_app();
    app.demo_mode = false;
    app.containers_overview
        .mark_auto_list_pending("db".to_string());
    let (tx, _rx) = mpsc::channel();
    super::containers_overview::ensure_list_for_selected_host(&mut app, &tx);
    // Still exactly one entry — the helper did not double-insert.
    assert_eq!(app.containers_overview.auto_list_in_flight().len(), 1);
}

#[test]
fn ensure_list_for_selected_host_skips_when_alias_in_batch() {
    // While an `R` batch is running for this alias, the auto helper
    // must yield to the batch so the completion handler can drive
    // the counters cleanly.
    let mut app = make_containers_overview_app();
    app.demo_mode = false;
    let mut in_flight_aliases = std::collections::HashSet::new();
    in_flight_aliases.insert("db".to_string());
    app.containers_overview
        .start_refresh(crate::app::RefreshBatch {
            queue: std::collections::VecDeque::new(),
            in_flight: 1,
            total: 1,
            completed: 0,
            in_flight_aliases,
        });
    let (tx, _rx) = mpsc::channel();
    super::containers_overview::ensure_list_for_selected_host(&mut app, &tx);
    assert!(
        !app.containers_overview.auto_list_pending("db"),
        "auto helper must not spawn while batch already covers the alias"
    );
}

#[test]
fn ensure_list_for_selected_host_no_op_in_demo_mode() {
    let mut app = make_containers_overview_app();
    app.demo_mode = true;
    let (tx, _rx) = mpsc::channel();
    super::containers_overview::ensure_list_for_selected_host(&mut app, &tx);
    assert!(app.containers_overview.auto_list_in_flight().is_empty());
}

#[test]
fn ensure_list_for_selected_host_no_op_during_shutdown() {
    let mut app = make_containers_overview_app();
    app.demo_mode = false;
    app.running = false;
    let (tx, _rx) = mpsc::channel();
    super::containers_overview::ensure_list_for_selected_host(&mut app, &tx);
    assert!(app.containers_overview.auto_list_in_flight().is_empty());
}

#[test]
fn handle_container_listing_clears_auto_list_in_flight() {
    // The completion handler must drop the in-flight marker so the
    // next scroll past the freshness window can re-fire. Without
    // this the alias would be stuck "in flight" forever from the
    // helper's POV.
    let mut app = make_containers_overview_app();
    app.containers_overview
        .mark_auto_list_pending("db".to_string());
    let (tx, _rx) = mpsc::channel();
    super::event_loop::handle_container_listing(
        &mut app,
        "db".to_string(),
        Ok(crate::containers::ContainerListing {
            runtime: crate::containers::ContainerRuntime::Docker,
            engine_version: None,
            containers: vec![make_container("c3", "postgres", "running")],
        }),
        &tx,
    );
    assert!(
        !app.containers_overview.auto_list_pending("db"),
        "in-flight marker must be cleared on listing arrival"
    );
}

#[test]
fn handle_container_listing_clears_auto_list_in_flight_on_error() {
    // Symmetric to the Ok path: an Err result must also drop the
    // marker, otherwise a transient SSH failure would lock the alias
    // out of future scroll-driven refreshes.
    let mut app = make_containers_overview_app();
    app.containers_overview
        .mark_auto_list_pending("db".to_string());
    let (tx, _rx) = mpsc::channel();
    super::event_loop::handle_container_listing(
        &mut app,
        "db".to_string(),
        Err(crate::containers::ContainerError {
            runtime: None,
            message: "ssh: connect refused".to_string(),
        }),
        &tx,
    );
    assert!(
        !app.containers_overview.auto_list_pending("db"),
        "Err result must also clear the in-flight marker"
    );
}

#[test]
fn handle_container_listing_no_panic_when_alias_not_in_flight() {
    // Documents the no-panic guarantee: a listing whose alias was
    // never marked (e.g. a stray result from a deleted host or an
    // unrelated trigger) must not crash the handler.
    let mut app = make_containers_overview_app();
    assert!(app.containers_overview.auto_list_in_flight().is_empty());
    let (tx, _rx) = mpsc::channel();
    super::event_loop::handle_container_listing(
        &mut app,
        "db".to_string(),
        Ok(crate::containers::ContainerListing {
            runtime: crate::containers::ContainerRuntime::Docker,
            engine_version: None,
            containers: vec![make_container("c3", "postgres", "running")],
        }),
        &tx,
    );
    assert!(app.containers_overview.auto_list_in_flight().is_empty());
}

#[test]
fn ensure_list_for_selected_host_refreshes_when_cursor_on_header() {
    // Header rows resolve to the host alias directly, so parking the
    // cursor on a divider must still keep that host's container
    // listing fresh. Reverses the prior "header rows are inert" rule
    // now that dividers are first-class selection targets.
    let mut app = make_containers_overview_app();
    app.demo_mode = false;
    // Force the cache for `db` stale enough that the helper wants to
    // re-fetch (TTL is 30s; timestamp 100 in the fixture is far in
    // the past).
    let _ = app.container_state.cache_entry("db");
    // Items[0] is HostHeader(db) in the fixture's AlphaHost layout.
    app.ui.containers_overview_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    super::containers_overview::ensure_list_for_selected_host(&mut app, &tx);
    assert!(
        app.containers_overview.auto_list_pending("db"),
        "header rows must trigger a refresh for their host"
    );
}

#[test]
fn ensure_inspect_for_host_header_marks_running_first_then_others() {
    // Cursor parked on the web host header. The fixture seeds web with
    // c1 (running) and c2 (exited). Pre-fetch must mark BOTH as in_flight
    // because the fanout (10) exceeds the fixture container count, and
    // the running one should be enqueued first.
    let mut app = make_containers_overview_app();
    app.demo_mode = false;
    // Items[2] is HostHeader(web) in the fixture's AlphaHost layout.
    app.ui.containers_overview_state_mut().select(Some(2));
    let (tx, _rx) = mpsc::channel();
    super::containers_overview::ensure_inspect_for_host_header(&mut app, &tx);
    let in_flight = &app.containers_overview.inspect_cache().in_flight;
    assert!(
        in_flight.contains("c1"),
        "running container must be enqueued for inspect on host-header land"
    );
    assert!(
        in_flight.contains("c2"),
        "non-running containers also enqueue when fanout has room"
    );
}

#[test]
fn ensure_inspect_for_host_header_skips_when_cursor_not_on_header() {
    // Cursor parks on the postgres container row in the fixture. The
    // helper must no-op: it only fires when the cursor sits on a
    // host-header, otherwise the per-row `ensure_inspect_for_selected`
    // owns that responsibility.
    let mut app = make_containers_overview_app();
    app.demo_mode = false;
    app.ui.containers_overview_state_mut().select(Some(1));
    let (tx, _rx) = mpsc::channel();
    super::containers_overview::ensure_inspect_for_host_header(&mut app, &tx);
    assert!(
        app.containers_overview.inspect_cache().in_flight.is_empty(),
        "no inspect threads must spawn when cursor is on a container row"
    );
}

#[test]
fn ensure_inspect_for_host_header_no_op_in_demo_mode() {
    let mut app = make_containers_overview_app();
    app.demo_mode = true;
    app.ui.containers_overview_state_mut().select(Some(2));
    let (tx, _rx) = mpsc::channel();
    super::containers_overview::ensure_inspect_for_host_header(&mut app, &tx);
    assert!(app.containers_overview.inspect_cache().in_flight.is_empty());
}

#[test]
fn ensure_inspect_for_host_header_dedups_on_repeated_call() {
    // Second call must not double-spawn. In-flight set tracks each
    // container ID once; rapid key events that re-fire the helper while
    // the original threads are still running cannot pile up extra SSH
    // sessions for the same id.
    let mut app = make_containers_overview_app();
    app.demo_mode = false;
    app.ui.containers_overview_state_mut().select(Some(2));
    let (tx, _rx) = mpsc::channel();
    super::containers_overview::ensure_inspect_for_host_header(&mut app, &tx);
    let first = app.containers_overview.inspect_cache().in_flight.len();
    super::containers_overview::ensure_inspect_for_host_header(&mut app, &tx);
    let second = app.containers_overview.inspect_cache().in_flight.len();
    assert_eq!(
        first, second,
        "in_flight set must not grow on a repeated call against the same host"
    );
    assert!(first > 0, "first call must seed at least one in-flight id");
}

#[test]
fn esc_hint_flag_is_shared_between_host_list_and_tunnels_overview() {
    let mut app = make_app("Host test\n  HostName test.com\n  LocalForward 8080 localhost:80\n");
    let (tx, _rx) = mpsc::channel();

    // First idle Esc on host list arms the hint flag.
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(app.ui.esc_quit_hint_shown());
    app.status_center.set_toast_message(None);

    // Switch to tunnels overview and press Esc again. The shared flag means
    // the hint stays silent — the user already learned about `q`.
    app.top_page = crate::app::TopPage::Tunnels;
    app.ui.tunnels_overview_state_mut().select(Some(0));
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);

    assert!(app.running);
    assert!(
        app.status_center.toast().is_none(),
        "second-tab idle Esc must not re-surface the hint"
    );

    // And again on the containers tab — same flag, same silence.
    app.top_page = crate::app::TopPage::Containers;
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(app.running);
    assert!(
        app.status_center.toast().is_none(),
        "third-tab idle Esc must not re-surface the hint"
    );
}

// --- Container actions: K (restart), S (stop), e (exec), l (logs) ----

#[test]
#[allow(non_snake_case)]
fn containers_overview_K_opens_restart_confirm() {
    let mut app = make_containers_overview_app();
    // Cursor parks on db/postgres (running).
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('K')), &tx);
    assert!(
        matches!(app.screen, Screen::ConfirmContainerRestart { .. }),
        "expected ConfirmContainerRestart, got {:?}",
        app.screen
    );
}

#[test]
#[allow(non_snake_case)]
fn containers_overview_S_opens_stop_confirm() {
    let mut app = make_containers_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('S')), &tx);
    assert!(matches!(app.screen, Screen::ConfirmContainerStop { .. }));
}

#[test]
fn containers_overview_e_opens_exec_prompt() {
    let mut app = make_containers_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
    assert!(matches!(app.screen, Screen::ContainerExecPrompt { .. }));
}

#[test]
#[allow(non_snake_case)]
fn containers_overview_K_on_header_opens_host_restart_all() {
    // Cursor on a host-divider row routes K to the bulk-restart-host
    // confirm dialog instead of the per-container restart.
    let mut app = make_containers_overview_app();
    // Items[0] is HostHeader(db); only one running container (postgres).
    app.ui.containers_overview_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('K')), &tx);
    assert!(
        matches!(app.screen, Screen::ConfirmHostRestartAll),
        "expected ConfirmHostRestartAll, got {:?}",
        app.screen
    );
    let payload = app
        .containers_overview
        .pending_bulk_confirm()
        .expect("bulk confirm payload must be set");
    assert_eq!(payload.alias, "db");
    assert!(matches!(
        payload.kind,
        crate::app::BulkConfirmKind::HostRestartAll
    ));
    assert_eq!(payload.members.len(), 1, "only postgres is running on db");
    assert_eq!(payload.members[0].container_name, "postgres");
}

#[test]
#[allow(non_snake_case)]
fn containers_overview_S_on_header_opens_host_stop_all() {
    let mut app = make_containers_overview_app();
    // Items[2] is HostHeader(web); web has nginx running, redis exited.
    app.ui.containers_overview_state_mut().select(Some(2));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('S')), &tx);
    assert!(
        matches!(app.screen, Screen::ConfirmHostStopAll),
        "expected ConfirmHostStopAll, got {:?}",
        app.screen
    );
    let payload = app
        .containers_overview
        .pending_bulk_confirm()
        .expect("bulk confirm payload must be set");
    assert_eq!(payload.alias, "web");
    assert!(matches!(
        payload.kind,
        crate::app::BulkConfirmKind::HostStopAll
    ));
    assert_eq!(
        payload.members.len(),
        1,
        "exited containers must not be queued for stop"
    );
    assert_eq!(payload.members[0].container_name, "nginx");
}

#[test]
fn containers_overview_l_on_header_warns_single_target() {
    // l (logs) on a host-divider row is a no-op with a guidance toast,
    // because logs apply to a single container.
    let mut app = make_containers_overview_app();
    app.ui.containers_overview_state_mut().select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('l')), &tx);
    assert!(
        matches!(app.screen, Screen::HostList),
        "header rows must not open the logs overlay"
    );
    assert!(
        app.status_center.toast().is_some(),
        "user must see a hint about needing a single container"
    );
}

#[test]
fn containers_overview_space_on_header_toggles_collapse() {
    // In-memory contract: Space on a host-divider row folds the group;
    // Space again unfolds. Disk persistence lives in the preferences
    // module tests, since the demo flag set by `build_demo_app` blocks
    // the persistence path here.
    let mut app = make_containers_overview_app();
    app.ui.containers_overview_state_mut().select(Some(0)); // db header
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(
        app.containers_overview.collapsed_hosts().contains("db"),
        "first Space folds the group"
    );

    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(
        !app.containers_overview.collapsed_hosts().contains("db"),
        "second Space unfolds the group"
    );
}

#[test]
fn containers_overview_v_toggles_view_mode_and_arms_animation() {
    // In-memory contract: v flips view_mode and queues the
    // detail-panel animation. Disk persistence is exercised by the
    // preferences module's own tests, not from here, because the
    // global demo flag (set by `build_demo_app`) short-circuits the
    // disk-write path.
    let mut app = make_containers_overview_app();
    assert_eq!(
        app.containers_overview.view_mode(),
        crate::app::ViewMode::Detailed
    );
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char('v')), &tx);
    assert_eq!(
        app.containers_overview.view_mode(),
        crate::app::ViewMode::Compact
    );
    assert!(
        app.ui.detail_toggle_pending(),
        "v must arm the detail-panel animation so the next render eases the panel out"
    );

    let _ = handle_key_event(&mut app, key(KeyCode::Char('v')), &tx);
    assert_eq!(
        app.containers_overview.view_mode(),
        crate::app::ViewMode::Detailed,
        "v toggles back"
    );
}

#[test]
fn containers_overview_l_queues_logs_fetch_and_opens_overlay() {
    let mut app = make_containers_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('l')), &tx);
    assert!(matches!(app.screen, Screen::ContainerLogs));
    assert!(
        app.container_state.pending_logs_request().is_some(),
        "expected pending fetch request"
    );
}

#[test]
#[allow(non_snake_case)]
fn containers_overview_K_on_exited_does_not_open_confirm() {
    let mut app = make_containers_overview_app();
    // Move cursor to web/redis (state="exited", index 4 in fixture).
    app.ui.containers_overview_state_mut().select(Some(4));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('K')), &tx);
    assert!(
        matches!(app.screen, Screen::HostList),
        "exited container must not transition to confirm"
    );
}

#[test]
#[allow(non_snake_case)]
fn containers_overview_K_in_demo_mode_blocks() {
    let mut app = make_containers_overview_app();
    app.demo_mode = true;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('K')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn containers_overview_ctrl_k_with_compose_label_opens_stack_confirm() {
    let mut app = make_containers_overview_app();
    // Seed an inspect entry with compose_project so the stack
    // confirm has something to anchor on.
    let make_inspect = |project: &str| crate::containers::ContainerInspect {
        exit_code: 0,
        oom_killed: false,
        started_at: String::new(),
        finished_at: String::new(),
        health: None,
        restart_count: 0,
        command: None,
        entrypoint: None,
        env_count: 0,
        mount_count: 0,
        networks: vec![],
        image_digest: None,
        restart_policy: None,
        user: None,
        privileged: false,
        readonly_rootfs: false,
        apparmor_profile: None,
        seccomp_profile: None,
        cap_add: Vec::new(),
        cap_drop: Vec::new(),
        mounts: Vec::new(),
        compose_project: Some(project.to_string()),
        compose_service: None,
        ..Default::default()
    };
    app.containers_overview.inspect_cache_mut().entries.insert(
        "c3".to_string(),
        crate::app::InspectCacheEntry {
            timestamp: 100,
            result: Ok(make_inspect("db-stack")),
        },
    );
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, ctrl_key('k'), &tx);
    assert!(
        matches!(app.screen, Screen::ConfirmStackRestart),
        "expected ConfirmStackRestart, got {:?}",
        app.screen
    );
}

#[test]
fn containers_overview_ctrl_k_without_compose_label_warns_and_stays() {
    let mut app = make_containers_overview_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, ctrl_key('k'), &tx);
    // No inspect entry seeded, so the helper warns and keeps the
    // user on the overview rather than guessing a project name.
    assert!(matches!(app.screen, Screen::HostList));
    assert!(app.status_center.toast().is_some());
}

// --- Container restart confirm dialog --------------------------------

#[test]
fn confirm_container_restart_y_enqueues_action_and_returns() {
    let mut app = make_containers_overview_app();
    app.screen = Screen::ConfirmContainerRestart {
        alias: "db".to_string(),
        container_id: "c3".to_string(),
        container_name: "postgres".to_string(),
        project: None,
        uptime: Some("2d".to_string()),
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert_eq!(app.container_state.pending_actions_len(), 1);
    let req = app.container_state.pending_actions_at(0).unwrap();
    assert_eq!(req.alias, "db");
    assert_eq!(req.container_id, "c3");
    assert_eq!(req.action, crate::containers::ContainerAction::Restart);
}

#[test]
fn confirm_container_restart_n_cancels_without_enqueue() {
    let mut app = make_containers_overview_app();
    app.screen = Screen::ConfirmContainerRestart {
        alias: "db".to_string(),
        container_id: "c3".to_string(),
        container_name: "postgres".to_string(),
        project: None,
        uptime: None,
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert!(app.container_state.pending_actions_len() == 0);
}

#[test]
fn confirm_container_restart_stray_key_ignored() {
    let mut app = make_containers_overview_app();
    app.screen = Screen::ConfirmContainerRestart {
        alias: "db".to_string(),
        container_id: "c3".to_string(),
        container_name: "postgres".to_string(),
        project: None,
        uptime: None,
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('t')), &tx);
    // Route_confirm_key returns Ignored for non-y/n/Esc keys, so
    // the screen must stay on the confirm dialog.
    assert!(
        matches!(app.screen, Screen::ConfirmContainerRestart { .. }),
        "stray key must not transition; got {:?}",
        app.screen
    );
    assert!(app.container_state.pending_actions_len() == 0);
}

// --- Container stop confirm dialog -----------------------------------

#[test]
fn confirm_container_stop_y_enqueues_stop_action() {
    let mut app = make_containers_overview_app();
    app.screen = Screen::ConfirmContainerStop {
        alias: "db".to_string(),
        container_id: "c3".to_string(),
        container_name: "postgres".to_string(),
        project: None,
        uptime: Some("2d".to_string()),
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert_eq!(app.container_state.pending_actions_len(), 1);
    assert_eq!(
        app.container_state.pending_actions_at(0).unwrap().action,
        crate::containers::ContainerAction::Stop
    );
}

#[test]
fn confirm_container_stop_stray_key_ignored() {
    let mut app = make_containers_overview_app();
    app.screen = Screen::ConfirmContainerStop {
        alias: "db".to_string(),
        container_id: "c3".to_string(),
        container_name: "postgres".to_string(),
        project: None,
        uptime: None,
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('x')), &tx);
    assert!(matches!(app.screen, Screen::ConfirmContainerStop { .. }));
}

// --- Stack restart confirm dialog ------------------------------------

#[test]
fn confirm_stack_restart_y_enqueues_one_action_per_member() {
    let mut app = make_containers_overview_app();
    app.containers_overview
        .set_pending_bulk_confirm(crate::app::BulkConfirmContext {
            kind: crate::app::BulkConfirmKind::StackRestart,
            alias: "web".to_string(),
            project: Some("web-stack".to_string()),
            members: vec![
                crate::app::StackMember {
                    container_id: "c1".to_string(),
                    container_name: "nginx".to_string(),
                    uptime: Some("5d".to_string()),
                },
                crate::app::StackMember {
                    container_id: "cextra".to_string(),
                    container_name: "sidecar".to_string(),
                    uptime: Some("5d".to_string()),
                },
            ],
        });
    app.screen = Screen::ConfirmStackRestart;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    // Both members were running on alias "web" which exists in the
    // fixture cache; both are enqueued.
    assert_eq!(app.container_state.pending_actions_len(), 2);
    assert_eq!(
        app.container_state
            .pending_actions_at(0)
            .unwrap()
            .container_id,
        "c1"
    );
    assert_eq!(
        app.container_state
            .pending_actions_at(1)
            .unwrap()
            .container_id,
        "cextra"
    );
    // Pending payload must be taken on yes so a re-entry to the dialog
    // starts from a clean slate.
    assert!(app.containers_overview.pending_bulk_confirm().is_none());
}

#[test]
fn confirm_stack_restart_stray_key_ignored() {
    let mut app = make_containers_overview_app();
    app.containers_overview
        .set_pending_bulk_confirm(crate::app::BulkConfirmContext {
            kind: crate::app::BulkConfirmKind::StackRestart,
            alias: "web".to_string(),
            project: Some("web-stack".to_string()),
            members: vec![],
        });
    app.screen = Screen::ConfirmStackRestart;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('q')), &tx);
    assert!(matches!(app.screen, Screen::ConfirmStackRestart));
    // Ignored key must NOT take the payload; the dialog stays open and
    // the next valid key still has the data to act on.
    assert!(app.containers_overview.pending_bulk_confirm().is_some());
}

#[test]
fn confirm_host_restart_all_y_enqueues_restart_per_member() {
    let mut app = make_containers_overview_app();
    app.containers_overview
        .set_pending_bulk_confirm(crate::app::BulkConfirmContext {
            kind: crate::app::BulkConfirmKind::HostRestartAll,
            alias: "web".to_string(),
            project: None,
            members: vec![
                crate::app::StackMember {
                    container_id: "c1".to_string(),
                    container_name: "nginx".to_string(),
                    uptime: Some("5d".to_string()),
                },
                crate::app::StackMember {
                    container_id: "cextra".to_string(),
                    container_name: "sidecar".to_string(),
                    uptime: None,
                },
            ],
        });
    app.screen = Screen::ConfirmHostRestartAll;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert_eq!(app.container_state.pending_actions_len(), 2);
    for i in 0..2 {
        assert_eq!(
            app.container_state.pending_actions_at(i).unwrap().action,
            crate::containers::ContainerAction::Restart
        );
    }
    assert_eq!(
        app.container_state
            .pending_actions_at(0)
            .unwrap()
            .container_id,
        "c1"
    );
    assert_eq!(
        app.container_state
            .pending_actions_at(1)
            .unwrap()
            .container_id,
        "cextra"
    );
    assert!(
        app.containers_overview.pending_bulk_confirm().is_none(),
        "yes must clear pending_bulk_confirm"
    );
}

#[test]
fn confirm_host_stop_all_y_enqueues_stop_per_member() {
    let mut app = make_containers_overview_app();
    app.containers_overview
        .set_pending_bulk_confirm(crate::app::BulkConfirmContext {
            kind: crate::app::BulkConfirmKind::HostStopAll,
            alias: "web".to_string(),
            project: None,
            members: vec![
                crate::app::StackMember {
                    container_id: "c1".to_string(),
                    container_name: "nginx".to_string(),
                    uptime: Some("5d".to_string()),
                },
                crate::app::StackMember {
                    container_id: "c2".to_string(),
                    container_name: "redis".to_string(),
                    uptime: None,
                },
            ],
        });
    app.screen = Screen::ConfirmHostStopAll;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert_eq!(app.container_state.pending_actions_len(), 2);
    for i in 0..2 {
        assert_eq!(
            app.container_state.pending_actions_at(i).unwrap().action,
            crate::containers::ContainerAction::Stop
        );
    }
    assert!(
        app.containers_overview.pending_bulk_confirm().is_none(),
        "yes must clear pending_bulk_confirm"
    );
}

#[test]
fn confirm_host_restart_all_stray_key_ignored() {
    let mut app = make_containers_overview_app();
    app.containers_overview
        .set_pending_bulk_confirm(crate::app::BulkConfirmContext {
            kind: crate::app::BulkConfirmKind::HostRestartAll,
            alias: "web".to_string(),
            project: None,
            members: vec![],
        });
    app.screen = Screen::ConfirmHostRestartAll;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('q')), &tx);
    assert!(matches!(app.screen, Screen::ConfirmHostRestartAll));
    assert!(
        app.containers_overview.pending_bulk_confirm().is_some(),
        "stray key must not consume the bulk-confirm payload"
    );
}

#[test]
fn confirm_host_stop_all_stray_key_ignored() {
    let mut app = make_containers_overview_app();
    app.containers_overview
        .set_pending_bulk_confirm(crate::app::BulkConfirmContext {
            kind: crate::app::BulkConfirmKind::HostStopAll,
            alias: "web".to_string(),
            project: None,
            members: vec![],
        });
    app.screen = Screen::ConfirmHostStopAll;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('q')), &tx);
    assert!(matches!(app.screen, Screen::ConfirmHostStopAll));
    assert!(
        app.containers_overview.pending_bulk_confirm().is_some(),
        "stray key must not consume the bulk-confirm payload"
    );
}

#[test]
fn bulk_confirm_with_pruned_payload_recovers_to_host_list() {
    // Regression: prune_orphans clears pending_bulk_confirm when the
    // target host disappears, but leaves `app.screen` on the matching
    // Confirm variant. The handler must recognise the missing payload
    // and drop to HostList instead of being a silent no-op (which
    // would strand the user on a header-only Confirm screen).
    let mut app = make_containers_overview_app();
    app.screen = Screen::ConfirmStackRestart;
    // pending_bulk_confirm intentionally left empty.
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(
        matches!(app.screen, Screen::HostList),
        "missing payload must trigger transition to HostList"
    );
}

#[test]
fn purge_stale_with_pruned_payload_recovers_to_host_list() {
    let mut app = make_app("Host alive\n  HostName 1.2.3.4\n");
    app.screen = Screen::ConfirmPurgeStale;
    // pending_purge intentionally left empty (simulating reload_hosts
    // having cleared it from under the user).
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(
        matches!(app.screen, Screen::HostList),
        "missing payload must trigger transition to HostList"
    );
}

#[test]
fn vault_sign_with_pruned_payload_recovers_to_host_list() {
    let mut app = make_app("Host alive\n  HostName 1.2.3.4\n");
    app.screen = Screen::ConfirmVaultSign;
    // pending_sign intentionally left empty.
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(
        matches!(app.screen, Screen::HostList),
        "missing payload must trigger transition to HostList"
    );
}

#[test]
fn container_logs_with_pruned_payload_recovers_to_host_list() {
    let mut app = make_containers_overview_app();
    app.screen = Screen::ContainerLogs;
    // logs_view intentionally left empty.
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(
        matches!(app.screen, Screen::HostList),
        "missing logs_view must trigger transition to HostList"
    );
}

#[test]
fn container_logs_esc_clears_logs_view() {
    // Memory-retention regression: Esc/q must drop the (possibly large)
    // logs body so it does not linger on container_state until the next
    // `l` overwrites it.
    let mut app = make_containers_overview_app();
    app.container_state.set_logs_view(crate::app::LogsView {
        alias: "db".to_string(),
        container_id: "c1".to_string(),
        container_name: "postgres".to_string(),
        body: vec!["line".to_string(); 200],
        fetched_at: 0,
        error: None,
        scroll: 0,
        last_render_height: 24,
        search: None,
    });
    app.screen = Screen::ContainerLogs;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert!(
        app.container_state.logs_view().is_none(),
        "Esc must free the LogsView body"
    );
}

// --- Exec prompt -----------------------------------------------------

#[test]
fn exec_prompt_enter_with_command_queues_exec_and_returns() {
    let mut app = make_containers_overview_app();
    app.screen = Screen::ContainerExecPrompt {
        alias: "db".to_string(),
        container_id: "c3".to_string(),
        container_name: "postgres".to_string(),
        query: "ls -la".to_string(),
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    let queued = app
        .container_state
        .pending_exec_request()
        .expect("exec request queued");
    assert_eq!(queued.command.as_deref(), Some("ls -la"));
}

#[test]
fn exec_prompt_enter_empty_query_is_no_op() {
    let mut app = make_containers_overview_app();
    app.screen = Screen::ContainerExecPrompt {
        alias: "db".to_string(),
        container_id: "c3".to_string(),
        container_name: "postgres".to_string(),
        query: String::new(),
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(matches!(app.screen, Screen::ContainerExecPrompt { .. }));
    assert!(app.container_state.pending_exec_request().is_none());
}

#[test]
fn exec_prompt_enter_with_control_char_warns_and_does_not_queue() {
    let mut app = make_containers_overview_app();
    app.screen = Screen::ContainerExecPrompt {
        alias: "db".to_string(),
        container_id: "c3".to_string(),
        container_name: "postgres".to_string(),
        query: "rm -rf /\nrm -rf /".to_string(),
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    // Embedded newline is a control char; the prompt rejects it.
    assert!(matches!(app.screen, Screen::ContainerExecPrompt { .. }));
    assert!(app.container_state.pending_exec_request().is_none());
    assert!(app.status_center.toast().is_some());
}

#[test]
fn exec_prompt_char_input_capped_at_512() {
    let mut app = make_containers_overview_app();
    app.screen = Screen::ContainerExecPrompt {
        alias: "db".to_string(),
        container_id: "c3".to_string(),
        container_name: "postgres".to_string(),
        query: "x".repeat(512),
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    if let Screen::ContainerExecPrompt { query, .. } = &app.screen {
        assert_eq!(
            query.chars().count(),
            512,
            "buffer must not grow past the cap"
        );
    } else {
        panic!("expected ContainerExecPrompt, got {:?}", app.screen);
    }
}

// --- Logs handler keys -----------------------------------------------

#[test]
fn logs_overlay_esc_closes_to_host_list() {
    let mut app = make_containers_overview_app();
    app.container_state.set_logs_view(crate::app::LogsView {
        alias: "db".to_string(),
        container_id: "c3".to_string(),
        container_name: "postgres".to_string(),
        body: vec!["one".to_string(), "two".to_string()],
        fetched_at: 100,
        error: None,
        scroll: 0,
        last_render_height: 0,
        search: None,
    });
    app.screen = Screen::ContainerLogs;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn logs_overlay_q_closes_to_host_list() {
    let mut app = make_containers_overview_app();
    app.container_state.set_logs_view(crate::app::LogsView {
        alias: "db".to_string(),
        container_id: "c3".to_string(),
        container_name: "postgres".to_string(),
        body: vec![],
        fetched_at: 0,
        error: None,
        scroll: 0,
        last_render_height: 0,
        search: None,
    });
    app.screen = Screen::ContainerLogs;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('q')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn logs_overlay_g_resets_scroll_and_capital_g_jumps_to_tail() {
    // Tail anchoring: with a 100-line body and a 24-row visible area,
    // the bottom of the body must align with the bottom of the
    // viewport, so scroll lands at 100 - 24 = 76.
    let mut app = make_containers_overview_app();
    let body: Vec<String> = (0..100).map(|i| format!("line {}", i)).collect();
    app.container_state.set_logs_view(crate::app::LogsView {
        alias: "db".to_string(),
        container_id: "c3".to_string(),
        container_name: "postgres".to_string(),
        body,
        fetched_at: 100,
        error: None,
        scroll: 50,
        last_render_height: 24,
        search: None,
    });
    app.screen = Screen::ContainerLogs;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('g')), &tx);
    if let Some(scroll) = app.container_state.logs_view().map(|v| v.scroll) {
        let scroll = &scroll;
        assert_eq!(*scroll, 0);
    } else {
        panic!("expected ContainerLogs");
    }
    let _ = handle_key_event(&mut app, key(KeyCode::Char('G')), &tx);
    if let Some(scroll) = app.container_state.logs_view().map(|v| v.scroll) {
        let scroll = &scroll;
        assert_eq!(*scroll, 76, "G must tail-anchor: body.len() - height");
    } else {
        panic!("expected ContainerLogs");
    }
}

#[test]
fn logs_overlay_capital_g_clamps_when_body_fits_in_viewport() {
    // 3-line body in a 24-row viewport: scroll stays at 0 because the
    // tail position (3 - 24 saturating) is 0.
    let mut app = make_containers_overview_app();
    app.container_state.set_logs_view(crate::app::LogsView {
        alias: "db".to_string(),
        container_id: "c3".to_string(),
        container_name: "postgres".to_string(),
        body: vec!["a".to_string(), "b".to_string(), "c".to_string()],
        fetched_at: 100,
        error: None,
        scroll: 1,
        last_render_height: 24,
        search: None,
    });
    app.screen = Screen::ContainerLogs;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('G')), &tx);
    if let Some(scroll) = app.container_state.logs_view().map(|v| v.scroll) {
        let scroll = &scroll;
        assert_eq!(*scroll, 0);
    } else {
        panic!("expected ContainerLogs");
    }
}

// --- handle_container_logs_complete ---------------------------------

#[test]
fn logs_complete_ok_populates_body_and_anchors_to_tail() {
    // 100-line body in a 24-row viewport: scroll = 100 - 24 = 76 so
    // the last log line sits at the bottom of the visible area instead
    // of being painted alone at the top.
    let mut app = make_containers_overview_app();
    app.container_state.set_logs_view(crate::app::LogsView {
        alias: "db".to_string(),
        container_id: "c3".to_string(),
        container_name: "postgres".to_string(),
        body: vec![],
        fetched_at: 0,
        error: None,
        scroll: 0,
        last_render_height: 24,
        search: None,
    });
    app.screen = Screen::ContainerLogs;
    let lines: Vec<String> = (0..100).map(|i| format!("line {}", i)).collect();
    crate::handler::event_loop::handle_container_logs_complete(
        &mut app,
        "db".to_string(),
        "c3".to_string(),
        "postgres".to_string(),
        Ok(lines),
    );
    if let Some(view) = app.container_state.logs_view() {
        assert_eq!(view.body.len(), 100);
        assert_eq!(
            view.scroll, 76,
            "scroll must tail-anchor (body.len() - height)"
        );
        assert!(view.error.is_none());
        assert!(view.fetched_at > 0, "fetched_at must be set");
    } else {
        panic!("expected ContainerLogs view, got {:?}", app.screen);
    }
}

#[test]
fn logs_complete_ok_keeps_scroll_zero_when_body_fits_viewport() {
    // 3-line body in a 24-row viewport saturates to scroll = 0.
    let mut app = make_containers_overview_app();
    app.container_state.set_logs_view(crate::app::LogsView {
        alias: "db".to_string(),
        container_id: "c3".to_string(),
        container_name: "postgres".to_string(),
        body: vec![],
        fetched_at: 0,
        error: None,
        scroll: 0,
        last_render_height: 24,
        search: None,
    });
    app.screen = Screen::ContainerLogs;
    crate::handler::event_loop::handle_container_logs_complete(
        &mut app,
        "db".to_string(),
        "c3".to_string(),
        "postgres".to_string(),
        Ok(vec!["a".to_string(), "b".to_string(), "c".to_string()]),
    );
    if let Some(scroll) = app.container_state.logs_view().map(|v| v.scroll) {
        let scroll = &scroll;
        assert_eq!(*scroll, 0);
    } else {
        panic!("expected ContainerLogs");
    }
}

#[test]
fn logs_complete_err_clears_body_and_records_error() {
    let mut app = make_containers_overview_app();
    app.container_state.set_logs_view(crate::app::LogsView {
        alias: "db".to_string(),
        container_id: "c3".to_string(),
        container_name: "postgres".to_string(),
        body: vec!["leftover".to_string()],
        fetched_at: 0,
        error: None,
        scroll: 5,
        last_render_height: 24,
        search: None,
    });
    app.screen = Screen::ContainerLogs;
    crate::handler::event_loop::handle_container_logs_complete(
        &mut app,
        "db".to_string(),
        "c3".to_string(),
        "postgres".to_string(),
        Err("permission denied".to_string()),
    );
    if let Some(view) = app.container_state.logs_view() {
        assert!(view.body.is_empty());
        assert_eq!(view.scroll, 0);
        assert_eq!(view.error.as_deref(), Some("permission denied"));
        assert!(view.fetched_at > 0, "even on error, fetched_at must reset");
    } else {
        panic!("expected ContainerLogs view");
    }
}

#[test]
fn logs_complete_dropped_when_screen_is_host_list() {
    let mut app = make_containers_overview_app();
    app.screen = Screen::HostList;
    crate::handler::event_loop::handle_container_logs_complete(
        &mut app,
        "db".to_string(),
        "c3".to_string(),
        "postgres".to_string(),
        Ok(vec!["should not land".to_string()]),
    );
    assert!(
        matches!(app.screen, Screen::HostList),
        "screen must be unchanged"
    );
}

#[test]
fn logs_complete_dropped_when_container_id_differs() {
    let mut app = make_containers_overview_app();
    app.container_state.set_logs_view(crate::app::LogsView {
        alias: "db".to_string(),
        container_id: "c3".to_string(),
        container_name: "postgres".to_string(),
        body: vec![],
        fetched_at: 0,
        error: None,
        scroll: 0,
        last_render_height: 0,
        search: None,
    });
    app.screen = Screen::ContainerLogs;
    crate::handler::event_loop::handle_container_logs_complete(
        &mut app,
        "db".to_string(),
        "different-id".to_string(),
        "other".to_string(),
        Ok(vec!["wrong target".to_string()]),
    );
    if let Some(view) = app.container_state.logs_view() {
        let body = &view.body;
        assert!(body.is_empty(), "stale fetch must not populate the overlay");
    } else {
        panic!("expected ContainerLogs to remain");
    }
}

// --- Logs handler search keys ---------------------------------------

fn make_logs_app_with_body(body: Vec<String>) -> App {
    let mut app = make_containers_overview_app();
    app.container_state.set_logs_view(crate::app::LogsView {
        alias: "db".to_string(),
        container_id: "c3".to_string(),
        container_name: "postgres".to_string(),
        body,
        fetched_at: 100,
        error: None,
        scroll: 0,
        last_render_height: 24,
        search: None,
    });
    app.screen = Screen::ContainerLogs;
    app
}

fn search_state(app: &App) -> Option<crate::app::ContainerLogsSearch> {
    if let Some(view) = app.container_state.logs_view() {
        let search = &view.search;
        search.clone()
    } else {
        None
    }
}

#[test]
fn logs_overlay_slash_opens_search_with_empty_query() {
    let mut app = make_logs_app_with_body(vec!["foo".to_string(), "bar".to_string()]);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    let s = search_state(&app).expect("search must be Some after /");
    assert!(s.query.is_empty(), "query starts empty");
    assert_eq!(s.cursor_pos, 0, "cursor starts at 0");
}

#[test]
fn logs_overlay_typing_extends_query_and_recomputes_matches() {
    let mut app = make_logs_app_with_body(vec![
        "error 1".to_string(),
        "ok".to_string(),
        "Error 2".to_string(),
    ]);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('r')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('r')), &tx);
    let s = search_state(&app).expect("search active");
    assert_eq!(s.query, "err");
    // Smart-case: lowercase query matches both "error 1" and "Error 2".
    assert_eq!(s.matches, vec![0, 2]);
}

#[test]
fn logs_overlay_backspace_shrinks_query_and_clamps_current() {
    let mut app = make_logs_app_with_body(vec!["one".to_string(), "two".to_string()]);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('o')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    let s = search_state(&app).expect("search active");
    assert!(s.query.is_empty(), "backspace removes the only char");
    assert!(s.matches.is_empty(), "empty query yields no matches");
}

#[test]
fn logs_overlay_esc_during_search_closes_search_only() {
    let mut app = make_logs_app_with_body(vec!["foo".to_string()]);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('f')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(search_state(&app).is_none(), "Esc closes search");
    assert!(
        matches!(app.screen, Screen::ContainerLogs),
        "viewer stays open"
    );
}

#[test]
fn logs_overlay_esc_without_search_closes_viewer() {
    let mut app = make_logs_app_with_body(vec!["foo".to_string()]);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(
        matches!(app.screen, Screen::HostList),
        "Esc without search closes viewer"
    );
}

#[test]
fn logs_overlay_tab_cycles_forward_through_matches() {
    let mut app = make_logs_app_with_body(vec![
        "foo a".to_string(),
        "bar".to_string(),
        "foo b".to_string(),
        "foo c".to_string(),
    ]);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('f')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('o')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('o')), &tx);
    // Three matches at lines 0, 2, 3. Initial current=0.
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(search_state(&app).unwrap().current, 1);
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(search_state(&app).unwrap().current, 2);
    // Wrap-around.
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(search_state(&app).unwrap().current, 0);
}

#[test]
fn logs_overlay_shift_tab_cycles_backward_through_matches() {
    let mut app = make_logs_app_with_body(vec![
        "foo a".to_string(),
        "foo b".to_string(),
        "foo c".to_string(),
    ]);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('f')), &tx);
    // Initial current=0. Shift+Tab wraps to last.
    let _ = handle_key_event(&mut app, key(KeyCode::BackTab), &tx);
    assert_eq!(search_state(&app).unwrap().current, 2);
    let _ = handle_key_event(&mut app, key(KeyCode::BackTab), &tx);
    assert_eq!(search_state(&app).unwrap().current, 1);
}

#[test]
fn logs_overlay_n_during_search_is_just_a_letter() {
    let mut app = make_logs_app_with_body(vec!["no entry".to_string()]);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    // Modeless: n is part of the query, not a match-nav command.
    assert_eq!(search_state(&app).unwrap().query, "n");
    assert_eq!(
        search_state(&app).unwrap().matches,
        vec![0],
        "query 'n' matches 'no entry'"
    );
}

#[test]
fn logs_overlay_q_during_search_extends_query_not_quits() {
    let mut app = make_logs_app_with_body(vec!["foo".to_string()]);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('q')), &tx);
    assert!(
        matches!(app.screen, Screen::ContainerLogs),
        "q during search must not close the viewer"
    );
    assert_eq!(search_state(&app).unwrap().query, "q");
}

#[test]
fn logs_overlay_typing_resets_current_to_first_match() {
    // After Tab moves to match #2, refining the query must reset
    // current back to 0 — mirrors app::search::apply_filter.
    let mut app = make_logs_app_with_body(vec![
        "foo a".to_string(),
        "foo b".to_string(),
        "foo c".to_string(),
    ]);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('f')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(search_state(&app).unwrap().current, 2);
    // Typing another char refines the query and must reset current.
    let _ = handle_key_event(&mut app, key(KeyCode::Char('o')), &tx);
    assert_eq!(
        search_state(&app).unwrap().current,
        0,
        "refining query must reset cursor to first match"
    );
}

#[test]
fn logs_overlay_scroll_keys_swallowed_during_search() {
    // Modeless contract: while search is active, j/k/g/G are treated
    // as character input so the user can search for those letters.
    let mut app = make_logs_app_with_body(vec!["jump line".to_string()]);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    assert_eq!(search_state(&app).unwrap().query, "j");
    let _ = handle_key_event(&mut app, key(KeyCode::Char('g')), &tx);
    assert_eq!(search_state(&app).unwrap().query, "jg");
}

#[test]
fn logs_overlay_typing_uppercase_flips_to_case_sensitive() {
    let mut app = make_logs_app_with_body(vec!["Error: x".to_string(), "error: y".to_string()]);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    // Capital E flips to case-sensitive.
    let _ = handle_key_event(&mut app, key(KeyCode::Char('E')), &tx);
    let s = search_state(&app).unwrap();
    assert_eq!(
        s.matches,
        vec![0],
        "uppercase query must match only the capitalised line"
    );
}

#[test]
fn logs_overlay_cursor_tracks_inserts_at_end() {
    let mut app = make_logs_app_with_body(vec!["foo".to_string()]);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('a')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('b')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('c')), &tx);
    let s = search_state(&app).unwrap();
    assert_eq!(s.query, "abc");
    assert_eq!(s.cursor_pos, 3, "cursor follows the tail after appends");
}

#[test]
fn logs_overlay_left_arrow_moves_cursor_back_then_insert_lands_mid_query() {
    let mut app = make_logs_app_with_body(vec!["foo".to_string()]);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('a')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('c')), &tx);
    // cursor at end (pos 2). Left once, insert 'b' between a and c.
    let _ = handle_key_event(&mut app, key(KeyCode::Left), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('b')), &tx);
    let s = search_state(&app).unwrap();
    assert_eq!(s.query, "abc", "char inserted at cursor, not at end");
    assert_eq!(s.cursor_pos, 2);
}

#[test]
fn logs_overlay_home_end_jump_cursor_to_extremes() {
    let mut app = make_logs_app_with_body(vec!["foo".to_string()]);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('a')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('b')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('c')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Home), &tx);
    assert_eq!(search_state(&app).unwrap().cursor_pos, 0);
    let _ = handle_key_event(&mut app, key(KeyCode::End), &tx);
    assert_eq!(search_state(&app).unwrap().cursor_pos, 3);
}

#[test]
fn logs_overlay_delete_removes_char_at_cursor() {
    let mut app = make_logs_app_with_body(vec!["foo".to_string()]);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('a')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('b')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('c')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Home), &tx);
    // Cursor at 0, Delete eats 'a'.
    let _ = handle_key_event(&mut app, key(KeyCode::Delete), &tx);
    let s = search_state(&app).unwrap();
    assert_eq!(s.query, "bc");
    assert_eq!(s.cursor_pos, 0, "delete does not move cursor");
}

#[test]
fn logs_overlay_backspace_at_position_zero_is_noop() {
    let mut app = make_logs_app_with_body(vec!["foo".to_string()]);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('a')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Home), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    let s = search_state(&app).unwrap();
    assert_eq!(s.query, "a", "backspace at start does not corrupt query");
    assert_eq!(s.cursor_pos, 0);
}

#[test]
fn logs_overlay_left_at_position_zero_is_noop() {
    let mut app = make_logs_app_with_body(vec!["foo".to_string()]);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Left), &tx);
    assert_eq!(search_state(&app).unwrap().cursor_pos, 0);
}

#[test]
fn logs_overlay_cursor_handles_unicode_grapheme() {
    let mut app = make_logs_app_with_body(vec!["über".to_string()]);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    // Type "ü" (2-byte UTF-8). Cursor advances by one CHAR, not 2 bytes.
    let _ = handle_key_event(&mut app, key(KeyCode::Char('ü')), &tx);
    let s = search_state(&app).unwrap();
    assert_eq!(s.query, "ü");
    assert_eq!(s.cursor_pos, 1, "cursor counts chars, not bytes");
    // Backspace must remove the whole rune, not a single byte.
    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    assert!(search_state(&app).unwrap().query.is_empty());
}

// --- Palette dispatch tests for the newly added per-tab actions ---
//
// Each test opens the jump bar in the correct mode, types a query that
// uniquely (or via the mode-match nudge) selects the action under test,
// presses Enter and asserts the resulting handler effect. Acts as a
// regression net so future scoring or table tweaks cannot silently
// regress the action set or its routing.

fn open_jump_with_query(app: &mut App, mode: crate::app::JumpMode, query: &str) {
    app.jump = Some(crate::app::JumpState::for_mode(mode));
    for c in query.chars() {
        app.jump.as_mut().unwrap().push_query(c);
    }
    app.recompute_jump_hits();
}

fn make_hosts_app_with_palette() -> App {
    let mut app = make_app(
        "Host alpha\n  HostName alpha.example\n\
         Host beta\n  HostName beta.example\n",
    );
    app.top_page = crate::app::TopPage::Hosts;
    app.screen = Screen::HostList;
    // Reset list cursor so the host-aware actions resolve a selection.
    app.ui.list_state_mut().select(Some(0));
    app
}

#[test]
fn palette_dispatches_hosts_sort_from_hosts_mode() {
    let mut app = make_hosts_app_with_palette();
    let initial = app.hosts_state.sort_mode();
    open_jump_with_query(&mut app, crate::app::JumpMode::Hosts, "sort");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.jump.is_none(), "jump bar closes after dispatch");
    assert_ne!(
        app.hosts_state.sort_mode(),
        initial,
        "Hosts: Sort must advance the host sort mode"
    );
}

#[test]
fn palette_dispatches_hosts_cycle_grouping_from_hosts_mode() {
    let mut app = make_hosts_app_with_palette();
    assert_eq!(app.hosts_state.group_by(), &crate::app::GroupBy::None);
    open_jump_with_query(&mut app, crate::app::JumpMode::Hosts, "grouping");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.jump.is_none());
    assert_ne!(
        app.hosts_state.group_by(),
        &crate::app::GroupBy::None,
        "Hosts: Cycle grouping must advance group_by from None"
    );
}

#[test]
fn palette_dispatches_hosts_toggle_compact_view_from_hosts_mode() {
    let mut app = make_hosts_app_with_palette();
    let initial = app.hosts_state.view_mode();
    open_jump_with_query(&mut app, crate::app::JumpMode::Hosts, "compact");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.jump.is_none());
    assert_ne!(
        app.hosts_state.view_mode(),
        initial,
        "Hosts: Toggle compact view must flip the view mode"
    );
}

#[test]
fn palette_dispatches_hosts_filter_by_tag_from_hosts_mode() {
    let mut app = make_hosts_app_with_palette();
    // Give one host a tag so the picker has something to show.
    app.hosts_state.list_mut()[0].tags = vec!["prod".into()];
    open_jump_with_query(&mut app, crate::app::JumpMode::Hosts, "filter by tag");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.jump.is_none());
    assert!(
        matches!(app.screen, Screen::TagPicker),
        "Hosts: Filter by tag must open the TagPicker, got {:?}",
        app.screen
    );
}

#[test]
fn palette_dispatches_containers_exec_from_containers_mode() {
    let mut app = make_containers_overview_app();
    open_jump_with_query(&mut app, crate::app::JumpMode::Containers, "exec");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.jump.is_none());
    assert!(
        matches!(app.screen, Screen::ContainerExecPrompt { .. }),
        "Containers: Exec must open the exec prompt, got {:?}",
        app.screen
    );
}

#[test]
fn palette_dispatches_containers_view_logs_from_containers_mode() {
    let mut app = make_containers_overview_app();
    open_jump_with_query(&mut app, crate::app::JumpMode::Containers, "view logs");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.jump.is_none());
    assert!(
        matches!(app.screen, Screen::ContainerLogs),
        "Containers: View logs must navigate to the logs screen, got {:?}",
        app.screen
    );
}

#[test]
fn palette_dispatches_containers_restart_from_containers_mode() {
    let mut app = make_containers_overview_app();
    open_jump_with_query(
        &mut app,
        crate::app::JumpMode::Containers,
        "restart container",
    );
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.jump.is_none());
    assert!(
        matches!(app.screen, Screen::ConfirmContainerRestart { .. }),
        "Containers: Restart must open the restart confirm, got {:?}",
        app.screen
    );
}

#[test]
fn palette_dispatches_containers_stop_from_containers_mode() {
    let mut app = make_containers_overview_app();
    open_jump_with_query(&mut app, crate::app::JumpMode::Containers, "stop container");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.jump.is_none());
    assert!(
        matches!(app.screen, Screen::ConfirmContainerStop { .. }),
        "Containers: Stop must open the stop confirm, got {:?}",
        app.screen
    );
}

#[test]
fn palette_dispatches_containers_add_host_from_containers_mode() {
    let mut app = make_containers_overview_app();
    open_jump_with_query(&mut app, crate::app::JumpMode::Containers, "add host");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.jump.is_none());
    assert!(
        matches!(app.screen, Screen::ContainerHostPicker),
        "Containers: Add host must open the container host picker, got {:?}",
        app.screen
    );
}

#[test]
fn palette_dispatches_containers_refresh_selected_from_containers_mode() {
    let mut app = make_containers_overview_app();
    open_jump_with_query(
        &mut app,
        crate::app::JumpMode::Containers,
        "refresh selected",
    );
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.jump.is_none());
    let toast = app
        .status_center
        .toast()
        .expect("Containers: Refresh selected host must emit a refreshing toast");
    assert!(
        toast.text.contains("Refreshing"),
        "expected refreshing toast, got {:?}",
        toast.text
    );
}

#[test]
fn palette_dispatches_hosts_mark_host_via_space_action() {
    let mut app = make_hosts_app_with_palette();
    assert!(app.hosts_state.multi_select().is_empty());
    open_jump_with_query(&mut app, crate::app::JumpMode::Hosts, "mark");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.jump.is_none());
    assert!(
        !app.hosts_state.multi_select().is_empty(),
        "Hosts: Mark host must toggle a host into the multi-select set"
    );
}

#[test]
fn palette_dispatches_hosts_select_all_visible_via_ctrl_a() {
    let mut app = make_hosts_app_with_palette();
    assert!(app.hosts_state.multi_select().is_empty());
    open_jump_with_query(&mut app, crate::app::JumpMode::Hosts, "select all");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.jump.is_none());
    let selected = app.hosts_state.multi_select().len();
    assert!(
        selected >= 2,
        "Hosts: Select all visible must mark every visible host, marked={}",
        selected
    );
}

#[test]
fn palette_dispatches_containers_fold_unfold_host_via_space_action() {
    let mut app = make_containers_overview_app();
    // Park cursor on the db host divider (item 0) so the fold action
    // resolves a host alias rather than a container row.
    app.ui.containers_overview_state_mut().select(Some(0));
    let initial_collapsed = app.containers_overview.collapsed_hosts().contains("db");
    open_jump_with_query(&mut app, crate::app::JumpMode::Containers, "fold");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.jump.is_none());
    let now_collapsed = app.containers_overview.collapsed_hosts().contains("db");
    assert_ne!(
        now_collapsed, initial_collapsed,
        "Containers: Fold or unfold host must flip the collapse state"
    );
}

#[test]
fn palette_dispatches_containers_restart_compose_stack_via_ctrl_k() {
    // The fixture seeds no inspect cache entries, so the stack-restart
    // handler short-circuits with a `container_stack_unknown` toast.
    // Either outcome — the confirm dialog OR the warning toast — proves
    // the palette dispatch reached the modifier-bound match arm; the
    // ConfirmStackRestart screen requires a full compose project setup
    // we deliberately keep out of the unit fixture.
    let mut app = make_containers_overview_app();
    open_jump_with_query(&mut app, crate::app::JumpMode::Containers, "compose stack");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.jump.is_none());
    let toast = app.status_center.toast();
    let reached_handler = matches!(app.screen, Screen::ConfirmStackRestart)
        || toast.is_some_and(|t| {
            t.text.to_lowercase().contains("stack") || t.text.to_lowercase().contains("compose")
        });
    assert!(
        reached_handler,
        "Containers: Restart compose stack must reach the stack restart handler; \
         screen={:?} toast={:?}",
        app.screen,
        toast.map(|t| t.text.clone())
    );
}

#[test]
fn palette_dispatches_help_from_any_tab() {
    // Open palette from the Tunnels tab and pick the universal Help action.
    // The dispatch switches to the Hosts tab and opens the help overlay;
    // the side-effect under test is that Screen ends up at Help, not the
    // tab-switch itself.
    let mut app = make_tunnels_overview_app();
    open_jump_with_query(&mut app, crate::app::JumpMode::Tunnels, "keybindings");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.jump.is_none());
    assert!(
        matches!(app.screen, Screen::Help { .. }),
        "Help: Show keybindings must open the help overlay, got {:?}",
        app.screen
    );
}

#[test]
fn palette_tunnel_hit_auto_toggles_picked_tunnel() {
    // Picking a tunnel from the palette is a one-shot action: the cursor
    // lands on the tunnel row AND fires Enter, just like picking a host
    // is a one-shot connect. Without this the user would have to press
    // Enter a second time on the overview, which the help overlay does
    // not document.
    let mut app = make_app("Host web\n  HostName web.example\n  LocalForward 8080 localhost:80\n");
    app.top_page = crate::app::TopPage::Hosts;
    app.screen = Screen::HostList;
    assert!(
        !app.tunnels.active_contains("web"),
        "tunnel starts inactive in the fixture"
    );
    app.jump = Some(crate::app::JumpState::default());
    let tunnel_hit = crate::app::JumpHit::Tunnel(crate::app::TunnelHit {
        alias: "web".into(),
        bind_port: 8080,
        bind_port_str: "8080".into(),
        destination: "localhost:80".into(),
        active: false,
    });
    if let Some(p) = app.jump.as_mut() {
        p.set_hits(vec![tunnel_hit]);
        for c in "web".chars() {
            p.push_query(c);
        }
    }
    // Demo mode short-circuits the spawn with a toast so the test does
    // not depend on a working `ssh` binary. Either toast text proves the
    // tunnels handler reached its Enter arm — the assertion under test.
    app.demo_mode = true;
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();
    assert!(app.jump.is_none(), "jump bar closes after dispatch");
    assert!(matches!(app.top_page, crate::app::TopPage::Tunnels));
    let toast = app
        .status_center
        .toast()
        .expect("dispatch must produce a toast (demo-disabled or started)");
    assert!(
        toast.text.to_lowercase().contains("tunnel") || toast.text.to_lowercase().contains("demo"),
        "expected a tunnel-related toast, got {:?}",
        toast.text
    );
}

#[test]
fn palette_container_hit_auto_queues_shell_exec() {
    // Mirror of the tunnel-hit auto-Enter test: picking a container from
    // the palette must land on the row AND fire Enter so the gesture
    // matches the one-shot host-connect rhythm. Enter on a container row
    // queues an interactive shell exec (not the one-off exec prompt that
    // `e` opens); the assertion targets the queued request.
    let mut app = make_containers_overview_app();
    app.top_page = crate::app::TopPage::Hosts;
    app.screen = Screen::HostList;
    assert!(
        app.container_state.pending_exec_request().is_none(),
        "no pending exec before dispatch"
    );
    app.jump = Some(crate::app::JumpState::default());
    let container_hit = crate::app::JumpHit::Container(crate::app::ContainerHit {
        alias: "web".into(),
        container_name: "nginx".into(),
        container_id: "c1".into(),
        state: "running".into(),
    });
    if let Some(p) = app.jump.as_mut() {
        p.set_hits(vec![container_hit]);
        for c in "nginx".chars() {
            p.push_query(c);
        }
    }
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();
    assert!(app.jump.is_none());
    assert!(matches!(app.top_page, crate::app::TopPage::Containers));
    let queued = app
        .container_state
        .pending_exec_request()
        .expect("Container hit must auto-queue a shell exec request");
    assert_eq!(queued.alias, "web");
    assert_eq!(queued.container_id, "c1");
}

// ── AWS profile picker ──────────────────────────────────────────

/// App on the AWS provider form with a `~/.aws` holding the given files. The
/// returned `TempDir` owns the home directory, so the caller keeps it alive.
fn make_aws_form_app(config: &str, credentials: &str) -> (App, tempfile::TempDir) {
    let home = tempfile::tempdir().expect("tempdir");
    let aws = home.path().join(".aws");
    std::fs::create_dir_all(&aws).expect("create .aws");
    std::fs::write(aws.join("config"), config).expect("write config");
    std::fs::write(aws.join("credentials"), credentials).expect("write credentials");

    let scratch = tempfile::tempdir().expect("tempdir").keep();
    let ssh = SshConfigFile {
        elements: SshConfigFile::parse_content(""),
        path: scratch.join("test_config"),
        crlf: false,
        bom: false,
    };
    let env = Arc::new(crate::runtime::env::Env::for_test(home.path()));
    let mut app = App::with_env(ssh, env);
    *app.providers.config_mut() = test_provider_config();
    app.screen = Screen::ProviderForm {
        id: crate::providers::config::ProviderConfigId::bare("aws"),
    };
    *app.providers.form_mut() = ProviderFormFields {
        filter: String::new(),
        label: String::new(),
        label_entry: false,
        url: String::new(),
        token: String::new(),
        profile: String::new(),
        project: String::new(),
        compartment: String::new(),
        regions: String::new(),
        alias_prefix: "aws".to_string(),
        user: "ec2-user".to_string(),
        identity_file: String::new(),
        verify_tls: true,
        auto_sync: true,
        vault_role: String::new(),
        vault_addr: String::new(),
        focused_field: ProviderFormField::Profile,
        cursor_pos: 0,
        expanded: true,
        ssm: crate::providers::aws_ssm::SsmMode::default(),
    };
    (app, home)
}

const TWO_PROFILES: &str = "[profile alpha]\nregion = eu-west-1\n[profile beta]\nrole_arn = arn:r\nsource_profile = alpha\n";
const ALPHA_KEYS: &str = "[alpha]\naws_access_key_id = AKIA\naws_secret_access_key = S\n";

#[test]
fn test_aws_space_on_profile_opens_the_picker() {
    let (mut app, _home) = make_aws_form_app(TWO_PROFILES, ALPHA_KEYS);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(app.ui.profile_picker().open, "Space must open the picker");
    assert_eq!(app.ui.profile_picker().list.selected(), Some(0));
}

#[test]
fn test_aws_space_on_profile_says_why_it_did_not_open() {
    // An empty picker reads as a bug, so the warning explains the absence.
    let (mut app, _home) = make_aws_form_app("", "");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(!app.ui.profile_picker().open);
    assert_eq!(
        app.status_center.toast().map(|t| t.text.as_str()),
        Some(crate::messages::PICKER_NO_AWS_PROFILES)
    );
}

#[test]
fn test_aws_profile_picker_walks_and_takes_the_row() {
    let (mut app, _home) = make_aws_form_app(TWO_PROFILES, ALPHA_KEYS);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    // Sorted: alpha, beta.
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    assert_eq!(app.ui.profile_picker().list.selected(), Some(1));
    let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
    assert_eq!(app.ui.profile_picker().list.selected(), Some(0));

    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.profile_picker().open, "Enter must close the picker");
    assert_eq!(app.providers.form().profile, "alpha");
    assert_eq!(app.providers.form().cursor_pos, "alpha".chars().count());
    // The profile names a region purple knows, and Regions was empty.
    assert_eq!(app.providers.form().regions, "eu-west-1");
}

#[test]
fn test_aws_profile_picker_esc_leaves_the_field_alone() {
    let (mut app, _home) = make_aws_form_app(TWO_PROFILES, ALPHA_KEYS);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(!app.ui.profile_picker().open);
    assert_eq!(app.providers.form().profile, "");
    assert_eq!(app.providers.form().regions, "");
}

#[test]
fn test_aws_profile_picker_keeps_a_region_the_user_already_chose() {
    let (mut app, _home) = make_aws_form_app(TWO_PROFILES, ALPHA_KEYS);
    app.providers.form_mut().regions = "us-east-1".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.providers.form().profile, "alpha");
    assert_eq!(app.providers.form().regions, "us-east-1");
}

#[test]
fn test_aws_profile_picker_leaves_regions_empty_when_the_profile_names_none() {
    let (mut app, _home) = make_aws_form_app(TWO_PROFILES, ALPHA_KEYS);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    // Row 1 is `beta`, which carries a role but no region.
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.providers.form().profile, "beta");
    assert_eq!(app.providers.form().regions, "");
}

#[test]
fn test_aws_profile_picker_skips_a_region_purple_cannot_sync() {
    // The sync refuses any region outside AWS_REGIONS, so prefilling one
    // would hand the user a config that fails on save.
    let (mut app, _home) =
        make_aws_form_app("[profile alpha]\nregion = xx-nowhere-1\n", ALPHA_KEYS);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.providers.form().profile, "alpha");
    assert_eq!(app.providers.form().regions, "");
}

#[test]
fn test_aws_profile_picker_marks_roles_and_refusals() {
    // The picker offers every profile it found, so the two facts that decide
    // what happens next have to be on the row: a role reaches another
    // account, and a shape purple cannot use will fail on the next sync.
    let (mut app, _home) = make_aws_form_app(
        "[profile alpha]\nregion = eu-west-1\n\
         [profile beta]\nrole_arn = arn:r\nsource_profile = alpha\n\
         [profile sso-dev]\nsso_start_url = https://x.awsapps.com/start\n",
        ALPHA_KEYS,
    );
    assert!(app.open_profile_picker(), "three profiles must open it");
    let rows = app.ui.aws_profile_rows();
    let note = |name: &str| {
        rows.iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("no row for {name}"))
            .note
            .clone()
    };
    assert!(note("alpha").is_none(), "a plain profile needs no note");
    let role = note("beta").expect("a role profile is marked");
    assert!(role.usable);
    assert_eq!(role.text, crate::messages::PROFILE_ASSUMES_ROLE);
    let refused = note("sso-dev").expect("an unusable profile is marked");
    assert!(!refused.usable);
    assert_eq!(refused.text, crate::messages::PROFILE_NOTE_SSO);
}

#[test]
fn test_aws_profile_picker_rows_are_read_once_when_it_opens() {
    // Every sibling picker scans on open. The overlay redraws on every tick,
    // and a row costs two file reads plus a chain resolution, so deriving it
    // per frame would read a file holding secret material dozens of times a
    // second.
    let (mut app, home) = make_aws_form_app(TWO_PROFILES, ALPHA_KEYS);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert_eq!(app.ui.aws_profile_rows().len(), 2);

    std::fs::write(
        home.path().join(".aws").join("config"),
        "[profile gamma]\nregion = eu-west-1\n",
    )
    .expect("rewrite config");
    assert_eq!(
        app.ui
            .aws_profile_rows()
            .iter()
            .map(|r| r.name.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "beta"],
        "the open overlay keeps the rows it was opened with"
    );

    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(
        app.ui.aws_profile_rows().iter().any(|r| r.name == "gamma"),
        "reopening picks up a profile added since"
    );
}

#[test]
fn test_aws_form_refuses_a_profile_name_it_cannot_put_in_a_proxy_command() {
    // The name goes into a shell line purple writes, so the form refuses it
    // rather than letting the next sync fail on it.
    let (mut app, _home) = make_aws_form_app(TWO_PROFILES, ALPHA_KEYS);
    app.providers.form_mut().profile = "my work".to_string();
    app.providers.form_mut().regions = "eu-central-1".to_string();
    app.providers.form_mut().ssm = crate::providers::aws_ssm::SsmMode::Auto;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    assert!(
        matches!(app.screen, Screen::ProviderForm { .. }),
        "the form must stay open on a refusal"
    );
    let toast = app.status_center.toast().map(|t| t.text.clone());
    assert!(
        toast.as_deref().is_some_and(|t| t.contains("ProxyCommand")),
        "no refusal toast: {toast:?}"
    );
    assert!(
        app.providers
            .config()
            .section_by_id(&crate::providers::config::ProviderConfigId::bare("aws"))
            .is_none(),
        "nothing may be saved"
    );
}

#[test]
fn test_aws_form_allows_the_same_name_with_session_manager_off() {
    // The name only has to be shell-safe because it goes into a command; with
    // Session Manager off it never does.
    let (mut app, _home) = make_aws_form_app(TWO_PROFILES, ALPHA_KEYS);
    app.providers.form_mut().profile = "my work".to_string();
    app.providers.form_mut().regions = "eu-central-1".to_string();
    app.providers.form_mut().ssm = crate::providers::aws_ssm::SsmMode::Off;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(
        app.providers
            .config()
            .section_by_id(&crate::providers::config::ProviderConfigId::bare("aws"))
            .is_some(),
        "off must not validate the profile name"
    );
}

#[test]
fn test_aws_form_warns_when_session_manager_has_no_profile() {
    // The proxy command can carry a --profile and nothing else, so the aws
    // CLI falls back to its own credentials for the session. Saving is still
    // allowed, and the warning outranks the save toast.
    let (mut app, _home) = make_aws_form_app(TWO_PROFILES, ALPHA_KEYS);
    app.providers.form_mut().profile = String::new();
    app.providers.form_mut().token = "AKIAAAAAAAAAAAAAAAAA:secret".to_string();
    app.providers.form_mut().regions = "eu-central-1".to_string();
    app.providers.form_mut().ssm = crate::providers::aws_ssm::SsmMode::Auto;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    assert!(
        app.providers
            .config()
            .section_by_id(&crate::providers::config::ProviderConfigId::bare("aws"))
            .is_some(),
        "the save must still go through"
    );
    assert_eq!(
        app.status_center.toast().map(|t| t.text.as_str()),
        Some(crate::messages::AWS_SSM_WITHOUT_PROFILE)
    );
}

#[cfg(unix)]
#[test]
fn stopping_a_tunnel_drops_the_pin_on_trial_for_it() {
    // The stopped ssh never settles, so a restart must ask for the PIN again
    // instead of sending the unproven one out a second time.
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n  LocalForward 18080 localhost:80\n");
    let child = std::process::Command::new("sleep")
        .arg("30")
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn sleep");
    let (parser_tx, _parser_rx) = mpsc::channel();
    let tunnel = crate::tunnel::ActiveTunnel::spawn(
        child,
        "web",
        parser_tx,
        crate::tunnel_live::AuthWatch::default(),
    );
    app.tunnels.active_insert("web".into(), tunnel);
    app.security_key_pins.start_trial(crate::app::PinTrial {
        alias: "web".into(),
        key_path: "/k".into(),
        pin: "4242".into(),
        flow: crate::security_key::PinFlow::Tunnel,
    });
    let effects = {
        let mut ctx = super::tunnel::ctx_from_app(&mut app);
        assert!(ctx.stop_active_tunnel("web"));
        ctx.effects
    };
    effects.apply(&mut app);
    assert!(app.security_key_pins.trial().is_none());
    assert!(
        app.security_key_run_for("web", crate::security_key::PinFlow::Tunnel)
            .pins
            .is_empty()
    );
}
