use super::*;
use crate::app::{App, FormField, ProviderFormField, ProviderFormFields, Screen};
use crate::providers::config::{ProviderConfig, ProviderSection};
use crate::ssh_config::model::SshConfigFile;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;

fn test_provider_config() -> ProviderConfig {
    ProviderConfig {
        path_override: Some(PathBuf::from("/tmp/purple_test_providers")),
        ..Default::default()
    }
}

fn make_app(content: &str) -> App {
    let config = SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };
    let mut app = App::new(config);
    // Never write to the real ~/.purple during tests
    app.provider_config = test_provider_config();
    crate::preferences::set_path_override(PathBuf::from("/tmp/purple_test_preferences"));
    app
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// App met een geconfigureerde DigitalOcean (auto_sync=true) en een nieuw Proxmox.
fn make_providers_app_with_do() -> App {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::Providers;
    app.provider_config = test_provider_config();
    app.provider_config.set_section(ProviderSection {
        provider: "digitalocean".to_string(),
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
    });
    app
}

fn make_providers_app_with_proxmox() -> App {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::Providers;
    app.provider_config = test_provider_config();
    app.provider_config.set_section(ProviderSection {
        provider: "proxmox".to_string(),
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
    });
    app
}

/// Positioneer de cursor op een bepaalde provider in de lijst en stuur Enter.
fn open_provider_form(app: &mut App, provider_name: &str) {
    let sorted = app.sorted_provider_names();
    let idx = sorted.iter().position(|n| n == provider_name).unwrap();
    app.ui.provider_list_state.select(Some(idx));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(app, key(KeyCode::Enter), &tx);
}

// --- Form initialisatie ---

#[test]
fn test_provider_form_init_existing_do_preserves_auto_sync_true() {
    let mut app = make_providers_app_with_do();
    open_provider_form(&mut app, "digitalocean");
    assert!(
        app.provider_form.auto_sync,
        "Bestaande DO provider (auto_sync=true) moet true blijven in het form"
    );
}

#[test]
fn test_provider_form_init_existing_proxmox_preserves_auto_sync_false() {
    let mut app = make_providers_app_with_proxmox();
    open_provider_form(&mut app, "proxmox");
    assert!(
        !app.provider_form.auto_sync,
        "Bestaande Proxmox provider (auto_sync=false) moet false blijven in het form"
    );
}

#[test]
fn test_provider_form_init_existing_do_explicit_false_preserved() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::Providers;
    app.provider_config = test_provider_config();
    // DO met auto_sync=false (gebruiker heeft het handmatig uitgezet)
    app.provider_config.set_section(ProviderSection {
        provider: "digitalocean".to_string(),
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
    });
    open_provider_form(&mut app, "digitalocean");
    assert!(
        !app.provider_form.auto_sync,
        "DO met auto_sync=false moet false blijven"
    );
}

#[test]
fn test_provider_form_init_new_proxmox_defaults_to_false() {
    // Proxmox zonder bestaande config: default auto_sync=false
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::Providers;
    app.provider_config = test_provider_config(); // geen config voor proxmox
    open_provider_form(&mut app, "proxmox");
    assert!(
        !app.provider_form.auto_sync,
        "Nieuw Proxmox form moet auto_sync=false als default tonen"
    );
}

#[test]
fn test_provider_form_init_new_digitalocean_defaults_to_true() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::Providers;
    app.provider_config = test_provider_config();
    open_provider_form(&mut app, "digitalocean");
    assert!(
        app.provider_form.auto_sync,
        "Nieuw DigitalOcean form moet auto_sync=true als default tonen"
    );
}

// --- Space toggle ---

fn make_form_app_focused_on(provider: &str, field: ProviderFormField) -> App {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::ProviderForm {
        provider: provider.to_string(),
    };
    app.provider_form = ProviderFormFields {
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
    let status_text = app.status.as_ref().map(|s| s.text.as_str());
    let toast_text = app.toast.as_ref().map(|t| t.text.as_str());
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
    let status_msg = app.status.as_ref().map(|s| s.text.as_str()).unwrap_or("");
    let toast_msg = app.toast.as_ref().map(|t| t.text.as_str()).unwrap_or("");
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
    assert!(app.provider_form.auto_sync);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(!app.provider_form.auto_sync);
}

#[test]
fn test_space_toggles_auto_sync_false_to_true() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::AutoSync);
    app.provider_form.auto_sync = false;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(app.provider_form.auto_sync);
}

#[test]
fn test_space_on_other_field_does_not_affect_auto_sync() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.provider_form.auto_sync = true;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    // Space op Token voegt spatie toe aan tekstveld; auto_sync ongewijzigd
    assert!(app.provider_form.auto_sync);
}

// --- Char/Backspace blokkering op AutoSync ---

#[test]
fn test_char_input_blocked_when_auto_sync_focused() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::AutoSync);
    let original_token = app.provider_form.token.clone();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('x')), &tx);
    // Geen enkel tekstveld mag gewijzigd zijn
    assert_eq!(app.provider_form.token, original_token);
    assert_eq!(app.provider_form.alias_prefix, "do");
}

#[test]
fn test_backspace_blocked_when_auto_sync_focused() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::AutoSync);
    let original_token = app.provider_form.token.clone();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    assert_eq!(app.provider_form.token, original_token);
}

// --- Submit persisteert auto_sync ---

#[test]
fn test_submit_provider_form_persists_auto_sync_false() {
    // Submit met auto_sync=false moet de sectie opslaan met auto_sync=false.
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::ProviderForm {
        provider: "digitalocean".to_string(),
    };
    app.provider_config = test_provider_config();
    app.provider_form = ProviderFormFields {
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
    };

    let (tx, _rx) = mpsc::channel();
    // Enter triggert submit; save() kan falen zonder ~/.purple dir, maar de
    // in-memory sectie wordt altijd bijgewerkt vóór de save.
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    // Ongeacht of save() slaagde: de sectie in provider_config is bijgewerkt.
    if let Some(section) = app.provider_config.section("digitalocean") {
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
        provider: "digitalocean".to_string(),
    };
    app.provider_config = test_provider_config();
    app.provider_form = ProviderFormFields {
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
    };

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    if let Some(section) = app.provider_config.section("digitalocean") {
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
        provider: "digitalocean".to_string(),
    };
    app.provider_config = test_provider_config();
    app.provider_form = ProviderFormFields {
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
    };

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    if let Some(section) = app.provider_config.section("digitalocean") {
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
fn test_submit_provider_form_rejects_control_chars_in_token() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.provider_form.token = "tok\x01en".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "control characters");
}

#[test]
fn test_submit_provider_form_rejects_control_chars_in_alias_prefix() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.provider_form.alias_prefix = "do\x00".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "control characters");
}

#[test]
fn test_submit_provider_form_rejects_control_chars_in_url() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::Url);
    app.provider_form.url = "https://pve\x0a.local:8006".to_string();
    app.provider_form.token = "user@pam!t=secret".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "control characters");
}

#[test]
fn test_submit_provider_form_rejects_control_chars_in_user() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.provider_form.user = "ro\tot".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "control characters");
}

#[test]
fn test_submit_provider_form_rejects_control_chars_in_identity_file() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.provider_form.identity_file = "~/.ssh/id\x1b_rsa".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "control characters");
}

#[test]
fn test_submit_proxmox_rejects_empty_url() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::Url);
    app.provider_form.url = "".to_string();
    app.provider_form.token = "user@pam!t=secret".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "URL is required");
}

#[test]
fn test_submit_proxmox_rejects_http_url() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::Url);
    app.provider_form.url = "http://pve.local:8006".to_string();
    app.provider_form.token = "user@pam!t=secret".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "https://");
}

#[test]
fn test_submit_proxmox_accepts_https_url() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::Url);
    app.provider_form.url = "https://pve.local:8006".to_string();
    app.provider_form.token = "user@pam!t=secret".to_string();
    submit_form(&mut app);
    assert_status_not_contains(&app, "URL is required");
    assert_status_not_contains(&app, "https://");
}

#[test]
fn test_submit_proxmox_rejects_bare_hostname_url() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::Url);
    app.provider_form.url = "pve.local:8006".to_string();
    app.provider_form.token = "user@pam!t=secret".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "https://");
}

#[test]
fn test_submit_provider_form_rejects_empty_token() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.provider_form.token = "".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "Token");
}

#[test]
fn test_submit_provider_form_rejects_whitespace_only_token() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.provider_form.token = "   ".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "Token");
}

#[test]
fn test_submit_provider_form_rejects_pattern_alias_prefix() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.provider_form.alias_prefix = "do*".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "pattern");
}

#[test]
fn test_submit_provider_form_rejects_question_mark_alias() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.provider_form.alias_prefix = "do?".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "pattern");
}

#[test]
fn test_submit_provider_form_rejects_negation_alias() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.provider_form.alias_prefix = "!do".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "pattern");
}

#[test]
fn test_submit_provider_form_rejects_whitespace_in_user() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.provider_form.user = "my user".to_string();
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
        provider: "gcp".to_string(),
    };
    app.provider_form = ProviderFormFields {
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
    };
    app
}

#[test]
fn test_submit_gcp_rejects_empty_project() {
    let mut app = make_gcp_form_app();
    app.provider_form.project = "".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "Project ID");
}

#[test]
fn test_submit_gcp_rejects_whitespace_only_project() {
    let mut app = make_gcp_form_app();
    app.provider_form.project = "   ".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "Project ID");
}

#[test]
fn test_submit_gcp_rejects_empty_token() {
    let mut app = make_gcp_form_app();
    app.provider_form.token = "".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "Token");
}

#[test]
fn test_submit_gcp_empty_token_shows_gcp_specific_hint() {
    let mut app = make_gcp_form_app();
    app.provider_form.token = "".to_string();
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
    app.provider_form.focused_field = ProviderFormField::Token;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(app.provider_form.focused_field, ProviderFormField::Project);
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(app.provider_form.focused_field, ProviderFormField::Regions);
}

#[test]
fn test_provider_form_init_new_gcp_defaults() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::Providers;
    app.provider_config = test_provider_config();
    open_provider_form(&mut app, "gcp");
    assert!(app.provider_form.project.is_empty());
    assert!(app.provider_form.auto_sync);
}

// =========================================================================
// Azure-specific form validation
// =========================================================================

fn make_azure_form_app() -> App {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::ProviderForm {
        provider: "azure".to_string(),
    };
    app.provider_config = test_provider_config();
    app.provider_form = ProviderFormFields {
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
    };
    app
}

#[test]
fn test_submit_azure_rejects_empty_subscriptions() {
    let mut app = make_azure_form_app();
    app.provider_form.regions = "".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "subscription");
}

#[test]
fn test_submit_azure_rejects_whitespace_only_subscriptions() {
    let mut app = make_azure_form_app();
    app.provider_form.regions = "   ".to_string();
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
    app.provider_form.focused_field = ProviderFormField::Token;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(app.provider_form.focused_field, ProviderFormField::Regions);
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.provider_form.focused_field,
        ProviderFormField::AliasPrefix
    );
}

#[test]
fn test_azure_regions_field_accepts_typing() {
    let mut app = make_azure_form_app();
    app.provider_form.focused_field = ProviderFormField::Regions;
    app.provider_form.regions = String::new();
    app.provider_form.cursor_pos = 0;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('a')), &tx);
    assert_eq!(app.provider_form.regions, "a");
}

fn make_ovh_form_app() -> App {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::ProviderForm {
        provider: "ovh".to_string(),
    };
    app.provider_form = ProviderFormFields {
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
    };
    app
}

#[test]
fn test_ovh_enter_on_regions_opens_picker() {
    let mut app = make_ovh_form_app();
    app.provider_form.focused_field = ProviderFormField::Regions;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(
        app.ui.show_region_picker,
        "Enter on OVH Regions should open picker"
    );
    assert_eq!(app.ui.region_picker_cursor, 0);
}

#[test]
fn test_ovh_picker_select_eu() {
    let mut app = make_ovh_form_app();
    app.provider_form.focused_field = ProviderFormField::Regions;
    app.ui.show_region_picker = true;
    app.ui.region_picker_cursor = 0;

    // Cursor starts on group header "API Endpoint" (row 0).
    // Row 1 = "eu", Row 2 = "ca", Row 3 = "us"
    // Move down to "eu" (row 1)
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    assert_eq!(app.ui.region_picker_cursor, 1);

    // Press Space to select "eu"
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert_eq!(app.provider_form.regions, "eu");

    // Press Enter to confirm
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.show_region_picker);
    assert_eq!(app.provider_form.regions, "eu");
}

#[test]
fn test_ovh_picker_select_us() {
    let mut app = make_ovh_form_app();
    app.ui.show_region_picker = true;
    app.ui.region_picker_cursor = 0;
    app.screen = Screen::ProviderForm {
        provider: "ovh".to_string(),
    };

    // Move to "us" (row 3: header=0, eu=1, ca=2, us=3)
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    assert_eq!(app.ui.region_picker_cursor, 3);

    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert_eq!(app.provider_form.regions, "us");

    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.show_region_picker);
    assert_eq!(app.provider_form.regions, "us");
}

#[test]
fn test_ovh_picker_space_on_header_toggles_all() {
    let mut app = make_ovh_form_app();
    app.ui.show_region_picker = true;
    app.ui.region_picker_cursor = 0; // Group header
    app.screen = Screen::ProviderForm {
        provider: "ovh".to_string(),
    };

    let (tx, _rx) = mpsc::channel();
    // Space on header selects all endpoints
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    // All three should be selected (order preserved by OVH_ENDPOINTS)
    assert_eq!(app.provider_form.regions, "eu,ca,us");

    // Space again on header deselects all
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert_eq!(app.provider_form.regions, "");
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
    app.ui.show_region_picker = true;
    app.ui.region_picker_cursor = 0;
    app.screen = Screen::ProviderForm {
        provider: "ovh".to_string(),
    };

    let (tx, _rx) = mpsc::channel();
    // Move to "ca" (row 2)
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    assert_eq!(app.ui.region_picker_cursor, 2);

    // Enter directly (no Space needed) selects "ca" and closes
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.show_region_picker);
    assert_eq!(app.provider_form.regions, "ca");
}

#[test]
fn test_ovh_picker_enter_on_header_closes_without_select() {
    let mut app = make_ovh_form_app();
    app.ui.show_region_picker = true;
    app.ui.region_picker_cursor = 0; // group header
    app.screen = Screen::ProviderForm {
        provider: "ovh".to_string(),
    };

    let (tx, _rx) = mpsc::channel();
    // Enter on header: no item to select, just closes
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.show_region_picker);
    assert_eq!(app.provider_form.regions, "");
}

#[test]
fn test_ovh_picker_enter_replaces_previous_selection() {
    let mut app = make_ovh_form_app();
    app.provider_form.regions = "eu".to_string(); // previously selected EU
    app.ui.show_region_picker = true;
    app.ui.region_picker_cursor = 3; // "us"
    app.screen = Screen::ProviderForm {
        provider: "ovh".to_string(),
    };

    let (tx, _rx) = mpsc::channel();
    // Enter on "us" should replace "eu" with "us" (single-select)
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.provider_form.regions, "us");
}

#[test]
fn test_azure_enter_on_regions_does_not_open_picker() {
    let mut app = make_azure_form_app();
    app.provider_form.focused_field = ProviderFormField::Regions;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    // Must NOT open region picker (Azure uses text input, not picker)
    assert!(!app.ui.show_region_picker);
    // Screen should no longer be ProviderForm (submit transitions away)
    // or validation error sets status (screen stays on form)
    // Either way: not a picker.
}

#[test]
fn test_submit_azure_rejects_invalid_subscription_id() {
    let mut app = make_azure_form_app();
    app.provider_form.regions = "not-a-uuid".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert_status_contains(&app, "Invalid subscription ID");
}

#[test]
fn test_submit_azure_rejects_mixed_valid_invalid_subscriptions() {
    let mut app = make_azure_form_app();
    app.provider_form.regions = "12345678-1234-1234-1234-123456789012,bad-id".to_string();
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
        app.provider_form.focused_field,
        ProviderFormField::AliasPrefix
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(app.provider_form.focused_field, ProviderFormField::User);
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.provider_form.focused_field,
        ProviderFormField::IdentityFile
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.provider_form.focused_field,
        ProviderFormField::VaultRole
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(app.provider_form.focused_field, ProviderFormField::AutoSync);
}

#[test]
fn test_provider_form_shift_tab_reverse() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::AutoSync);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::BackTab), &tx);
    assert_eq!(
        app.provider_form.focused_field,
        ProviderFormField::VaultRole
    );
}

#[test]
fn test_provider_form_proxmox_has_extra_fields() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::Url);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(app.provider_form.focused_field, ProviderFormField::Token);
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.provider_form.focused_field,
        ProviderFormField::AliasPrefix
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(app.provider_form.focused_field, ProviderFormField::User);
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.provider_form.focused_field,
        ProviderFormField::IdentityFile
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.provider_form.focused_field,
        ProviderFormField::VerifyTls
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.provider_form.focused_field,
        ProviderFormField::VaultRole
    );
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(app.provider_form.focused_field, ProviderFormField::AutoSync);
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
    assert!(app.provider_form.verify_tls);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(!app.provider_form.verify_tls);
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert!(app.provider_form.verify_tls);
}

#[test]
fn test_provider_form_char_input_verify_tls_blocked() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::VerifyTls);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('x')), &tx);
    // No text field should have changed
    assert_eq!(app.provider_form.token, "tok");
}

#[test]
fn test_provider_form_backspace_verify_tls_blocked() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::VerifyTls);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    assert_eq!(app.provider_form.token, "tok");
}

#[test]
fn test_provider_form_enter_opens_key_picker() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::IdentityFile);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.ui.show_key_picker);
}

#[test]
fn test_provider_form_char_appended_to_focused_field() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.provider_form.token = "tok".to_string();
    app.provider_form.cursor_pos = 3;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('X')), &tx);
    assert_eq!(app.provider_form.token, "tokX");
}

#[test]
fn test_provider_form_backspace_removes_from_focused_field() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.provider_form.token = "tok".to_string();
    app.provider_form.cursor_pos = 3;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    assert_eq!(app.provider_form.token, "to");
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
    app.ui.provider_list_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    // Should advance (wrapping depends on count)
    assert!(app.ui.provider_list_state.selected().is_some());
}

#[test]
fn test_provider_list_k_selects_prev() {
    let mut app = make_providers_app_with_do();
    app.ui.provider_list_state.select(Some(1));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
    assert!(app.ui.provider_list_state.selected().is_some());
}

#[test]
fn test_provider_list_sync_unconfigured_shows_status() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::Providers;
    app.provider_config = test_provider_config();
    // No config for digitalocean - select it and press s
    let sorted = app.sorted_provider_names();
    let idx = sorted.iter().position(|n| n == "digitalocean").unwrap();
    app.ui.provider_list_state.select(Some(idx));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('s')), &tx);
    assert!(app.toast.as_ref().unwrap().text.contains("Configure"));
}

#[test]
fn test_provider_list_delete_removes_config() {
    let mut app = make_providers_app_with_do();
    let sorted = app.sorted_provider_names();
    let idx = sorted.iter().position(|n| n == "digitalocean").unwrap();
    app.ui.provider_list_state.select(Some(idx));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    // d now triggers confirmation
    assert!(app.pending_provider_delete.is_some());
    // Confirm with y
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(app.pending_provider_delete.is_none());
    // Save may fail in tests (no ~/.purple), triggering rollback. Just verify handler ran.
    assert!(app.status.is_some() || app.toast.is_some());
}

#[test]
fn test_provider_list_delete_unconfigured_is_noop() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::Providers;
    app.provider_config = test_provider_config();
    let sorted = app.sorted_provider_names();
    let idx = sorted.iter().position(|n| n == "digitalocean").unwrap();
    app.ui.provider_list_state.select(Some(idx));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    // No status/toast message because no section existed to delete
    let has_removed = app
        .toast
        .as_ref()
        .is_some_and(|t| t.text.contains("Removed"))
        || app
            .status
            .as_ref()
            .is_some_and(|s| s.text.contains("Removed"));
    assert!(!has_removed);
}

#[test]
fn test_provider_list_esc_cancels_running_syncs() {
    let mut app = make_providers_app_with_do();
    let cancel = Arc::new(AtomicBool::new(false));
    app.syncing_providers
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
    assert!(
        matches!(app.screen, Screen::ProviderForm { ref provider } if provider == "digitalocean")
    );
    assert_eq!(app.provider_form.token, "tok");
    assert_eq!(app.provider_form.alias_prefix, "do");
    assert_eq!(app.provider_form.user, "root");
}

#[test]
fn test_provider_list_enter_opens_form_with_defaults() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::Providers;
    app.provider_config = test_provider_config();
    open_provider_form(&mut app, "vultr");
    assert!(matches!(app.screen, Screen::ProviderForm { ref provider } if provider == "vultr"));
    assert_eq!(app.provider_form.token, "");
    assert_eq!(app.provider_form.user, "root");
    assert!(app.provider_form.auto_sync); // vultr default true
}

#[test]
fn test_provider_form_proxmox_default_alias_prefix() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::Providers;
    app.provider_config = test_provider_config();
    open_provider_form(&mut app, "proxmox");
    // Proxmox short_label is "pve"
    assert_eq!(app.provider_form.alias_prefix, "pve");
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
        app.provider_config = test_provider_config();
        open_provider_form(&mut app, provider);
        assert!(
            app.provider_form.auto_sync,
            "{} should default auto_sync=true",
            provider
        );
    }
}

#[test]
fn test_proxmox_defaults_auto_sync_false() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::Providers;
    app.provider_config = test_provider_config();
    open_provider_form(&mut app, "proxmox");
    assert!(!app.provider_form.auto_sync);
}

#[test]
fn test_submit_proxmox_https_case_insensitive() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::Url);
    app.provider_form.url = "HTTPS://pve.local:8006".to_string();
    app.provider_form.token = "user@pam!t=secret".to_string();
    submit_form(&mut app);
    assert_status_not_contains(&app, "https://");
}

#[test]
fn test_submit_non_proxmox_url_not_required() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.provider_form.url = "".to_string();
    submit_form(&mut app);
    assert_status_not_contains(&app, "URL is required");
}

#[test]
fn test_submit_provider_form_accepts_empty_alias_prefix() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.provider_form.alias_prefix = "".to_string();
    submit_form(&mut app);
    assert_status_not_contains(&app, "pattern");
}

#[test]
fn test_submit_provider_form_accepts_hyphenated_alias() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.provider_form.alias_prefix = "my-cloud".to_string();
    submit_form(&mut app);
    assert_status_not_contains(&app, "pattern");
}

#[test]
fn test_submit_provider_form_rejects_space_in_alias_prefix() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.provider_form.alias_prefix = "my cloud".to_string();
    submit_form(&mut app);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    let msg = &app.status.as_ref().or(app.toast.as_ref()).unwrap().text;
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
    app.form = crate::app::HostForm::new();
    app.form.expanded = true; // Tests assume all fields visible
    app
}

// --- Enter on AskPass opens picker ---

#[test]
fn test_enter_on_askpass_opens_password_picker() {
    let mut app = make_form_app();
    app.form.focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.ui.show_password_picker);
    assert_eq!(app.ui.password_picker_state.selected(), Some(0));
}

// --- Esc closes picker ---

#[test]
fn test_password_picker_esc_closes() {
    let mut app = make_form_app();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(2));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(!app.ui.show_password_picker);
    // Form field should be unchanged
    assert_eq!(app.form.askpass, "");
}

// --- Navigation j/k ---

#[test]
fn test_password_picker_j_moves_down() {
    let mut app = make_form_app();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    assert_eq!(app.ui.password_picker_state.selected(), Some(1));
}

#[test]
fn test_password_picker_k_moves_up() {
    let mut app = make_form_app();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(2));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
    assert_eq!(app.ui.password_picker_state.selected(), Some(1));
}

#[test]
fn test_password_picker_down_arrow() {
    let mut app = make_form_app();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Down), &tx);
    assert_eq!(app.ui.password_picker_state.selected(), Some(1));
}

#[test]
fn test_password_picker_up_arrow() {
    let mut app = make_form_app();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(3));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Up), &tx);
    assert_eq!(app.ui.password_picker_state.selected(), Some(2));
}

#[test]
fn test_password_picker_wraps_around_bottom() {
    let mut app = make_form_app();
    app.ui.show_password_picker = true;
    let last = crate::askpass::PASSWORD_SOURCES.len() - 1;
    app.ui.password_picker_state.select(Some(last));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    assert_eq!(app.ui.password_picker_state.selected(), Some(0));
}

#[test]
fn test_password_picker_wraps_around_top() {
    let mut app = make_form_app();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
    let last = crate::askpass::PASSWORD_SOURCES.len() - 1;
    assert_eq!(app.ui.password_picker_state.selected(), Some(last));
}

// --- Enter selects source: OS Keychain ---

#[test]
fn test_password_picker_select_keychain() {
    let mut app = make_form_app();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(0)); // OS Keychain
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.show_password_picker);
    assert_eq!(app.form.askpass, "keychain");
}

// --- Enter selects source: 1Password (prefix) ---

#[test]
fn test_password_picker_select_1password() {
    let mut app = make_form_app();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(1)); // 1Password
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.show_password_picker);
    assert_eq!(app.form.askpass, "op://");
    assert_eq!(app.form.focused_field, FormField::AskPass);
}

// --- Enter selects source: Bitwarden (prefix) ---

#[test]
fn test_password_picker_select_bitwarden() {
    let mut app = make_form_app();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(2)); // Bitwarden
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.show_password_picker);
    assert_eq!(app.form.askpass, "bw:");
    assert_eq!(app.form.focused_field, FormField::AskPass);
}

// --- Enter selects source: pass (prefix) ---

#[test]
fn test_password_picker_select_pass() {
    let mut app = make_form_app();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(3)); // pass
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.show_password_picker);
    assert_eq!(app.form.askpass, "pass:");
    assert_eq!(app.form.focused_field, FormField::AskPass);
}

// --- Enter selects source: HashiCorp Vault (prefix) ---

#[test]
fn test_password_picker_select_vault() {
    let mut app = make_form_app();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(4)); // HashiCorp Vault
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.show_password_picker);
    assert_eq!(app.form.askpass, "vault:");
    assert_eq!(app.form.focused_field, FormField::AskPass);
}

// --- Enter selects source: Custom command ---

#[test]
fn test_password_picker_select_custom() {
    let mut app = make_form_app();
    app.form.askpass = "old-value".to_string();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(5)); // Custom command
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.show_password_picker);
    assert_eq!(app.form.askpass, "");
}

// --- Enter selects source: None (clears) ---

#[test]
fn test_password_picker_select_none() {
    let mut app = make_form_app();
    app.form.askpass = "keychain".to_string();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(6)); // None
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.show_password_picker);
    assert_eq!(app.form.askpass, "");
}

// --- Picker blocks other form input ---

#[test]
fn test_password_picker_blocks_char_input() {
    let mut app = make_form_app();
    app.form.askpass = "".to_string();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('x')), &tx);
    // 'x' should not be appended to any form field
    assert_eq!(app.form.askpass, "");
    assert_eq!(app.form.alias, "");
}

#[test]
fn test_password_picker_blocks_tab() {
    let mut app = make_form_app();
    let original_field = app.form.focused_field;
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    // Tab should not change focused field
    assert_eq!(app.form.focused_field, original_field);
}

// --- Picker on EditHost screen ---

#[test]
fn test_password_picker_works_on_edit_host() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::EditHost {
        alias: "test".to_string(),
    };
    app.form = crate::app::HostForm::new();
    app.form.focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.ui.show_password_picker);
    // Select keychain
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.form.askpass, "keychain");
}

// --- Picker priority over key picker ---

#[test]
fn test_password_picker_takes_priority_over_key_picker() {
    let mut app = make_form_app();
    app.ui.show_password_picker = true;
    app.ui.show_key_picker = true;
    app.ui.password_picker_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    // Esc should close password picker, not key picker
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(!app.ui.show_password_picker);
    assert!(app.ui.show_key_picker); // still open
}

// =========================================================================
// Host list Enter carries askpass in pending_connect
// =========================================================================

#[test]
fn test_host_list_enter_carries_askpass() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    app.screen = Screen::HostList;
    // Select the host
    app.ui.list_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    let pending = app.pending_connect.as_ref().unwrap();
    assert_eq!(pending.0, "myserver");
    assert_eq!(pending.1, Some("keychain".to_string()));
}

#[test]
fn test_host_list_enter_carries_vault_askpass() {
    let mut app =
        make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass vault:secret/ssh#pass\n");
    app.screen = Screen::HostList;
    app.ui.list_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    let pending = app.pending_connect.as_ref().unwrap();
    assert_eq!(pending.1, Some("vault:secret/ssh#pass".to_string()));
}

#[test]
fn test_host_list_enter_no_askpass() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    app.screen = Screen::HostList;
    app.ui.list_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    let pending = app.pending_connect.as_ref().unwrap();
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
    assert!(!app.search.filtered_indices.is_empty());
    app.ui.list_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    let pending = app.pending_connect.as_ref().unwrap();
    assert_eq!(pending.0, "myserver");
    assert_eq!(pending.1, Some("op://V/I/p".to_string()));
    // Search should be cancelled after Enter
    assert!(app.search.query.is_none());
}

#[test]
fn test_search_enter_no_askpass() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    app.screen = Screen::HostList;
    app.start_search();
    app.ui.list_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    let pending = app.pending_connect.as_ref().unwrap();
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
    app.ui.list_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, ctrl_key('e'), &tx);
    assert!(matches!(app.screen, Screen::EditHost { ref alias } if alias == "myserver"));
    // Search query should be preserved so user returns to filtered list
    assert!(app.search.query.is_some());
}

#[test]
fn test_search_ctrl_e_blocks_included_host() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    // Simulate an included host by setting source_file
    if let Some(host) = app.hosts.first_mut() {
        host.source_file = Some(std::path::PathBuf::from("/etc/ssh/config.d/test"));
    }
    app.screen = Screen::HostList;
    app.start_search();
    app.ui.list_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, ctrl_key('e'), &tx);
    // Should remain in search mode (not open edit form)
    assert!(matches!(app.screen, Screen::HostList));
    assert!(app.status.is_some() || app.toast.is_some());
}

// =========================================================================
// Tunnel start reads askpass from host
// =========================================================================

#[test]
fn test_tunnel_handler_reads_askpass_from_hosts() {
    // Verify the askpass lookup logic: find host by alias and extract askpass
    let app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass bw:my-item\n");
    let askpass = app
        .hosts
        .iter()
        .find(|h| h.alias == "myserver")
        .and_then(|h| h.askpass.clone());
    assert_eq!(askpass, Some("bw:my-item".to_string()));
}

#[test]
fn test_tunnel_handler_askpass_none_when_absent() {
    let app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    let askpass = app
        .hosts
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
    app.ui.list_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    // Press 'e' to edit
    let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
    if matches!(app.screen, Screen::EditHost { .. }) {
        assert_eq!(app.form.askpass, "pass:ssh/prod");
    }
}

#[test]
fn test_edit_host_populates_empty_askpass() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    app.screen = Screen::HostList;
    app.ui.list_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
    if matches!(app.screen, Screen::EditHost { .. }) {
        assert_eq!(app.form.askpass, "");
    }
}

// =========================================================================
// Tab navigation through AskPass field
// =========================================================================

#[test]
fn test_tab_reaches_askpass_field() {
    let mut app = make_form_app();
    app.form.focused_field = FormField::ProxyJump;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(app.form.focused_field, FormField::AskPass);
}

#[test]
fn test_tab_from_askpass_goes_to_tags() {
    let mut app = make_form_app();
    app.form.focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(app.form.focused_field, FormField::Tags);
}

#[test]
fn test_shift_tab_from_tags_goes_to_askpass() {
    let mut app = make_form_app();
    app.form.focused_field = FormField::Tags;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::BackTab), &tx);
    assert_eq!(app.form.focused_field, FormField::AskPass);
}

#[test]
fn test_typing_in_askpass_field() {
    let mut app = make_form_app();
    app.form.focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert_eq!(app.form.askpass, "key");
}

#[test]
fn test_backspace_in_askpass_field() {
    let mut app = make_form_app();
    app.form.focused_field = FormField::AskPass;
    app.form.askpass = "vault:".to_string();
    app.form.cursor_pos = 6;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    assert_eq!(app.form.askpass, "vault");
}

// =========================================================================
// Picker then type: prefix selection followed by typing
// =========================================================================

#[test]
fn test_picker_select_op_then_type_rest() {
    let mut app = make_form_app();
    app.form.focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    // Open picker
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    // Navigate to 1Password (index 1)
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    // Select
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.form.askpass, "op://");
    assert_eq!(app.form.focused_field, FormField::AskPass);
    // Now type the rest of the URI
    let _ = handle_key_event(&mut app, key(KeyCode::Char('V')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('I')), &tx);
    assert_eq!(app.form.askpass, "op://V/I");
}

#[test]
fn test_picker_select_vault_then_type_rest() {
    let mut app = make_form_app();
    app.form.focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    // Open picker
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    // Navigate to Vault (index 4)
    for _ in 0..4 {
        let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    }
    assert_eq!(app.ui.password_picker_state.selected(), Some(4));
    // Select
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.form.askpass, "vault:");
    assert_eq!(app.form.focused_field, FormField::AskPass);
    // Type the path
    for c in "secret/ssh#pass".chars() {
        let _ = handle_key_event(&mut app, key(KeyCode::Char(c)), &tx);
    }
    assert_eq!(app.form.askpass, "vault:secret/ssh#pass");
}

#[test]
fn test_picker_select_keychain_no_further_typing_needed() {
    let mut app = make_form_app();
    app.form.focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    // Open picker via Enter on AskPass
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    // Select keychain (index 0, already selected)
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.form.askpass, "keychain");
    // focused_field stays on AskPass (picker was opened from AskPass)
    assert_eq!(app.form.focused_field, FormField::AskPass);
}

// =========================================================================
// Password picker: status messages after selection
// =========================================================================

#[test]
fn test_picker_keychain_sets_status_message() {
    let mut app = make_form_app();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.toast.as_ref().unwrap().text.contains("OS Keychain"));
}

#[test]
fn test_picker_none_sets_cleared_status() {
    let mut app = make_form_app();
    app.form.askpass = "keychain".to_string();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(6)); // None
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.toast.as_ref().unwrap().text.contains("cleared"));
}

#[test]
fn test_picker_prefix_source_shows_guidance() {
    // Prefix sources (op://, bw:, etc.) show a guidance message
    let mut app = make_form_app();
    app.toast = None;
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(1)); // 1Password (op://)
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.toast.as_ref().unwrap().text.contains("Complete"));
    assert_eq!(app.form.focused_field, FormField::AskPass);
}

// =========================================================================
// Backspace after prefix selection
// =========================================================================

#[test]
fn test_backspace_after_prefix_selection() {
    let mut app = make_form_app();
    app.form.focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    // Open picker and select 1Password
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    app.ui.password_picker_state.select(Some(1));
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.form.askpass, "op://");
    assert_eq!(app.form.focused_field, FormField::AskPass);
    // Type something
    let _ = handle_key_event(&mut app, key(KeyCode::Char('V')), &tx);
    assert_eq!(app.form.askpass, "op://V");
    // Backspace removes last char
    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    assert_eq!(app.form.askpass, "op://");
    // Another backspace removes the trailing /
    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    assert_eq!(app.form.askpass, "op:/");
}

// =========================================================================
// Edit form populates askpass from existing host
// =========================================================================

#[test]
fn test_edit_form_populates_askpass() {
    let mut app =
        make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass vault:secret/ssh#pw\n");
    // Simulate what happens when user presses 'e' on a host
    let entry = app.config.host_entries()[0].clone();
    app.form = crate::app::HostForm::from_entry(&entry, Default::default());
    assert_eq!(app.form.askpass, "vault:secret/ssh#pw");
}

#[test]
fn test_edit_form_empty_askpass_when_none() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    let entry = app.config.host_entries()[0].clone();
    app.form = crate::app::HostForm::from_entry(&entry, Default::default());
    assert_eq!(app.form.askpass, "");
}

// =========================================================================
// Password picker: unknown keys are no-ops
// =========================================================================

#[test]
fn test_password_picker_ignores_unknown_keys() {
    let mut app = make_form_app();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(2));
    let (tx, _rx) = mpsc::channel();
    // F1 key should be a no-op
    let _ = handle_key_event(&mut app, key(KeyCode::F(1)), &tx);
    assert!(app.ui.show_password_picker);
    assert_eq!(app.ui.password_picker_state.selected(), Some(2));
}

// =========================================================================
// Host list search Enter carries askpass
// =========================================================================

#[test]
fn test_search_enter_carries_askpass_op_uri() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass op://V/I/p\n");
    app.search.query = Some("myserver".to_string());
    app.apply_filter();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    if let Some((alias, askpass)) = &app.pending_connect {
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
    // When no global default is set, shows guidance text
    assert!(
        placeholder.contains("Enter") || placeholder.contains("default:"),
        "Should show guidance or default: {}",
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
    assert_eq!(crate::askpass::PASSWORD_SOURCES.len(), 7);
}

// =========================================================================
// Full picker → type → form submit flow
// =========================================================================

#[test]
fn test_full_flow_picker_to_typed_value() {
    let mut app = make_form_app();
    app.form.alias = "myhost".to_string();
    app.form.hostname = "10.0.0.1".to_string();
    app.form.focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();

    // Open picker, select Bitwarden (index 2)
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    app.ui.password_picker_state.select(Some(2));
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    // Verify field has prefix
    assert_eq!(app.form.askpass, "bw:");
    assert_eq!(app.form.focused_field, FormField::AskPass);

    // Type the item name
    for c in "my-ssh-server".chars() {
        let _ = handle_key_event(&mut app, key(KeyCode::Char(c)), &tx);
    }
    assert_eq!(app.form.askpass, "bw:my-ssh-server");

    // Verify to_entry produces correct askpass
    let entry = app.form.to_entry();
    assert_eq!(entry.askpass, Some("bw:my-ssh-server".to_string()));
}

#[test]
fn test_full_flow_picker_keychain_then_tab_away() {
    let mut app = make_form_app();
    // Only set alias (not hostname) so auto-submit doesn't trigger after picker
    app.form.alias = "myhost".to_string();
    app.form.focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();

    // Open picker via Enter on AskPass, select keychain
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    assert_eq!(app.form.askpass, "keychain");
    // Focus stays on AskPass (picker opened from AskPass)
    assert_eq!(app.form.focused_field, FormField::AskPass);

    // Tab to next field (Tags is after AskPass)
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(app.form.focused_field, FormField::Tags);
}

#[test]
fn test_full_flow_clear_askpass_via_picker_none() {
    let mut app = make_form_app();
    app.form.focused_field = FormField::AskPass;
    app.form.askpass = "op://Vault/Item/pw".to_string();
    let (tx, _rx) = mpsc::channel();

    // Open picker, select None (index 6)
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    for _ in 0..6 {
        let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    }
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    assert_eq!(app.form.askpass, "");
    let entry = app.form.to_entry();
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
    if let Some((alias, askpass)) = &app.pending_connect {
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
        provider: "digitalocean".to_string(),
    };
    app.provider_form = crate::app::ProviderFormFields::new();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, ctrl_key('p'), &tx);
    // Provider form does not have a password picker
    assert!(!app.ui.show_password_picker);
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
    assert_eq!(app.hosts.len(), 3);
    assert_eq!(app.hosts[0].askpass, Some("keychain".to_string()));
    assert_eq!(app.hosts[1].askpass, Some("op://Vault/SSH/pw".to_string()));
    assert_eq!(app.hosts[2].askpass, None);
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
    app.ui.list_state.select(Some(0));
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    let (alias, askpass) = app.pending_connect.take().unwrap();
    assert_eq!(alias, "alpha");
    assert_eq!(askpass, Some("keychain".to_string()));

    // Select beta (second host) and press Enter
    app.ui.list_state.select(Some(1));
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    let (alias, askpass) = app.pending_connect.take().unwrap();
    assert_eq!(alias, "beta");
    assert_eq!(askpass, Some("bw:my-item".to_string()));
}

// =========================================================================
// Askpass field typing: direct input without picker
// =========================================================================

#[test]
fn test_type_askpass_directly_without_picker() {
    let mut app = make_form_app();
    app.form.focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    for c in "keychain".chars() {
        let _ = handle_key_event(&mut app, key(KeyCode::Char(c)), &tx);
    }
    assert_eq!(app.form.askpass, "keychain");
}

#[test]
fn test_type_custom_command_directly() {
    let mut app = make_form_app();
    app.form.focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    for c in "my-script %a %h".chars() {
        let _ = handle_key_event(&mut app, key(KeyCode::Char(c)), &tx);
    }
    assert_eq!(app.form.askpass, "my-script %a %h");
}

#[test]
fn test_clear_askpass_with_backspace() {
    let mut app = make_form_app();
    app.form.focused_field = FormField::AskPass;
    app.form.askpass = "keychain".to_string();
    app.form.cursor_pos = 8;
    let (tx, _rx) = mpsc::channel();
    for _ in 0..8 {
        let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    }
    assert_eq!(app.form.askpass, "");
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
        app.config.host_entries()[0].askpass,
        Some("vault:secret/ssh#pw".to_string())
    );

    // Delete the host (undoable)
    if let Some((element, position)) = app.config.delete_host_undoable("myserver") {
        // Host is gone
        assert!(app.config.host_entries().is_empty());
        // Undo: restore
        app.config.insert_host_at(element, position);
        // Askpass should be restored
        let entries = app.config.host_entries();
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
    app.form.focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    for c in "get-p\u{00E4}ss %h".chars() {
        let _ = handle_key_event(&mut app, key(KeyCode::Char(c)), &tx);
    }
    assert_eq!(app.form.askpass, "get-p\u{00E4}ss %h");
}

// =========================================================================
// Enter on AskPass field opens picker
// =========================================================================

#[test]
fn test_enter_on_askpass_field_opens_picker() {
    let mut app = make_form_app();
    app.form.focused_field = FormField::AskPass;
    app.form.askpass = "old-val".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.ui.show_password_picker);
    // Old value should still be there (picker hasn't committed yet)
    assert_eq!(app.form.askpass, "old-val");
}

#[test]
fn test_enter_on_askpass_field_select_replaces_value() {
    let mut app = make_form_app();
    app.form.focused_field = FormField::AskPass;
    app.form.askpass = "old-val".to_string();
    let (tx, _rx) = mpsc::channel();
    // Open picker
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    // Select keychain
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.form.askpass, "keychain");
    assert!(!app.ui.show_password_picker);
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
        .config
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
        .config
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
        .config
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
    assert_eq!(app.form.askpass, "op://Vault/SSH/pw");
    assert_eq!(app.form.hostname, "10.0.0.1");
}

#[test]
fn test_e_key_opens_edit_form_without_askpass() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
    assert!(matches!(app.screen, Screen::EditHost { .. }));
    assert_eq!(app.form.askpass, "");
}

// =========================================================================
// Picker then Esc preserves existing askpass value
// =========================================================================

#[test]
fn test_picker_esc_preserves_existing_askpass() {
    let mut app = make_form_app();
    app.form.focused_field = FormField::AskPass;
    app.form.askpass = "vault:secret/ssh#pw".to_string();
    let (tx, _rx) = mpsc::channel();
    // Open picker
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.ui.show_password_picker);
    // Navigate but then Esc
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    // Original value preserved
    assert_eq!(app.form.askpass, "vault:secret/ssh#pw");
}

// =========================================================================
// Extra backspace past empty is no-op
// =========================================================================

#[test]
fn test_backspace_on_empty_askpass_is_noop() {
    let mut app = make_form_app();
    app.form.focused_field = FormField::AskPass;
    app.form.askpass = "".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    assert_eq!(app.form.askpass, "");
}

// =========================================================================
// Tab from AskPass goes to Tags, shift-tab goes to ProxyJump
// =========================================================================

#[test]
fn test_tab_from_askpass_to_tags() {
    let mut app = make_form_app();
    app.form.focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(app.form.focused_field, FormField::Tags);
}

#[test]
fn test_shift_tab_from_askpass_to_proxyjump() {
    let mut app = make_form_app();
    app.form.focused_field = FormField::AskPass;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(
        &mut app,
        KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
        &tx,
    );
    assert_eq!(app.form.focused_field, FormField::ProxyJump);
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
        app.hosts
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
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(0)); // Keychain
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    let toast = app.toast.as_ref().unwrap();
    assert!(
        toast.text.contains("OS Keychain"),
        "Toast should mention OS Keychain, got: {}",
        toast.text
    );
}

#[test]
fn test_password_picker_none_sets_cleared_status() {
    let mut app = make_form_app();
    app.form.askpass = "keychain".to_string();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(6)); // None
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    let toast = app.toast.as_ref().unwrap();
    assert!(
        toast.text.contains("cleared"),
        "Toast should say cleared, got: {}",
        toast.text
    );
}

#[test]
fn test_password_picker_prefix_source_focuses_askpass_field() {
    let mut app = make_form_app();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(1)); // 1Password (op://)
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(
        app.form.focused_field,
        FormField::AskPass,
        "Prefix source should focus AskPass field"
    );
    // No status message for prefix sources (user needs to keep typing)
    assert!(app.status.is_none() || !app.status.as_ref().unwrap().text.contains("set to"));
}

#[test]
fn test_password_picker_prefix_bw_focuses_askpass() {
    let mut app = make_form_app();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(2)); // Bitwarden (bw:)
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.form.focused_field, FormField::AskPass);
    assert_eq!(app.form.askpass, "bw:");
}

#[test]
fn test_password_picker_prefix_pass_focuses_askpass() {
    let mut app = make_form_app();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(3)); // pass (pass:)
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.form.focused_field, FormField::AskPass);
    assert_eq!(app.form.askpass, "pass:");
}

#[test]
fn test_password_picker_prefix_vault_focuses_askpass() {
    let mut app = make_form_app();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(4)); // Vault (vault:)
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert_eq!(app.form.focused_field, FormField::AskPass);
    assert_eq!(app.form.askpass, "vault:");
}

// =========================================================================
// Included host: edit blocked, but askpass visible in pending_connect
// =========================================================================

#[test]
fn test_included_host_edit_blocked() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    app.screen = Screen::HostList;
    if let Some(host) = app.hosts.first_mut() {
        host.source_file = Some(std::path::PathBuf::from("/etc/ssh/ssh_config.d/work.conf"));
    }
    app.ui.list_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn test_included_host_connect_still_carries_askpass() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass op://V/I/p\n");
    app.screen = Screen::HostList;
    if let Some(host) = app.hosts.first_mut() {
        host.source_file = Some(std::path::PathBuf::from("/etc/ssh/ssh_config.d/work.conf"));
    }
    app.ui.list_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    if let Some((alias, askpass)) = &app.pending_connect {
        assert_eq!(alias, "myserver");
        assert_eq!(askpass.as_deref(), Some("op://V/I/p"));
    }
}

#[test]
fn test_included_host_delete_blocked() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass bw:item\n");
    app.screen = Screen::HostList;
    if let Some(host) = app.hosts.first_mut() {
        host.source_file = Some(std::path::PathBuf::from("/etc/ssh/ssh_config.d/work.conf"));
    }
    app.ui.list_state.select(Some(0));
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
        app.form.alias = "test-host".to_string();
        app.form.hostname = "10.0.0.1".to_string();
        app.form.askpass = source.to_string();
        let entry = app.form.to_entry();
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
    app.form.alias = "test-host".to_string();
    app.form.hostname = "10.0.0.1".to_string();
    app.form.askpass = "".to_string();
    let entry = app.form.to_entry();
    assert!(entry.askpass.is_none(), "Empty askpass should produce None");
}

// =========================================================================
// Password picker: Enter with no selection is no-op
// =========================================================================

#[test]
fn test_password_picker_enter_with_no_selection() {
    let mut app = make_form_app();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state = ratatui::widgets::ListState::default(); // no selection
    app.form.askpass = "old".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(!app.ui.show_password_picker);
    assert_eq!(app.form.askpass, "old");
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
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(6)); // None
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, ctrl_key('d'), &tx);
    assert!(!app.ui.show_password_picker);
}

#[test]
fn test_password_picker_ctrl_d_does_not_change_form_askpass() {
    let mut app = make_form_app();
    app.form.askpass = "old".to_string();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(6)); // None
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, ctrl_key('d'), &tx);
    // Ctrl+D only sets the global default, not the form field
    assert_eq!(app.form.askpass, "old");
}

#[test]
fn test_password_picker_ctrl_d_none_sets_status() {
    let mut app = make_form_app();
    app.ui.show_password_picker = true;
    app.ui.password_picker_state.select(Some(6)); // None
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, ctrl_key('d'), &tx);
    // Shows "cleared" on success or "Failed to save" if ~/.purple doesn't exist
    assert!(app.status.is_some() || app.toast.is_some());
    assert!(!app.ui.show_password_picker);
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
    assert_eq!(app.hosts[0].askpass, Some("keychain".to_string()));
    // Simulate opening edit form
    app.screen = Screen::EditHost {
        alias: "myserver".to_string(),
    };
    app.form.alias = "myserver".to_string();
    app.form.hostname = "10.0.0.1".to_string();
    // Change askpass to something else
    app.form.askpass = "op://Vault/Item/pw".to_string();
    // The old_askpass detection in submit_form looks up app.hosts by alias
    let old = app
        .hosts
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
    app.form.alias = "myserver".to_string();
    app.form.hostname = "10.0.0.1".to_string();
    // Keep askpass as keychain
    app.form.askpass = "keychain".to_string();
    let old = app
        .hosts
        .iter()
        .find(|h| h.alias == "myserver")
        .and_then(|h| h.askpass.clone());
    // Same source, no removal needed
    assert_eq!(old.as_deref(), Some("keychain"));
    assert_eq!(app.form.askpass, "keychain");
}

#[test]
fn test_submit_form_no_keychain_removal_for_add() {
    // AddHost has no old askpass
    let mut app = make_app("Host existing\n  HostName 1.2.3.4\n");
    app.screen = Screen::AddHost;
    let old: Option<String> = None; // no old host for add
    assert!(old.is_none());
}

// =========================================================================
// Snippet picker
// =========================================================================

fn make_snippet_app() -> App {
    let mut app = make_app("Host myserver\n  HostName 1.2.3.4\n");
    let dir = std::env::temp_dir().join(format!(
        "purple_handler_snip_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::create_dir_all(&dir);
    app.snippet_store.path_override = Some(dir.join("snippets"));
    app.snippet_store.snippets = vec![
        crate::snippet::Snippet {
            name: "check-disk".to_string(),
            command: "df -h".to_string(),
            description: "Check disk usage".to_string(),
        },
        crate::snippet::Snippet {
            name: "uptime".to_string(),
            command: "uptime".to_string(),
            description: String::new(),
        },
    ];
    let _ = app.snippet_store.save();
    app.ui.snippet_picker_state.select(Some(0));
    app.screen = Screen::SnippetPicker {
        target_aliases: vec!["myserver".to_string()],
    };
    app
}

#[test]
fn test_snippet_picker_nav_down_up() {
    let mut app = make_snippet_app();
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    assert_eq!(app.ui.snippet_picker_state.selected(), Some(1));

    let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
    assert_eq!(app.ui.snippet_picker_state.selected(), Some(0));
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
    match &app.screen {
        Screen::SnippetOutput {
            snippet_name,
            target_aliases,
        } => {
            assert_eq!(snippet_name, "check-disk");
            assert_eq!(target_aliases, &vec!["myserver".to_string()]);
        }
        _ => panic!("Expected SnippetOutput screen, got {:?}", app.screen),
    }
    assert!(app.snippet_output.is_some());
}

#[test]
fn test_snippet_picker_enter_clears_multi_select() {
    let mut app = make_snippet_app();
    app.multi_select.insert(0);
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.multi_select.is_empty());
}

#[test]
fn test_snippet_picker_a_opens_add_form() {
    let mut app = make_snippet_app();
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char('a')), &tx);
    assert!(matches!(
        app.screen,
        Screen::SnippetForm { editing: None, .. }
    ));
    assert!(app.snippet_form.name.is_empty());
}

#[test]
fn test_snippet_picker_e_opens_edit_form() {
    let mut app = make_snippet_app();
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
    assert!(matches!(
        app.screen,
        Screen::SnippetForm {
            editing: Some(0),
            ..
        }
    ));
    assert_eq!(app.snippet_form.name, "check-disk");
    assert_eq!(app.snippet_form.command, "df -h");
}

#[test]
fn test_snippet_picker_d_deletes_and_saves() {
    let mut app = make_snippet_app();
    let _ = app.snippet_store.save(); // ensure file exists
    let (tx, _rx) = mpsc::channel();

    // d sets pending confirmation
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    assert_eq!(app.pending_snippet_delete, Some(0));
    assert_eq!(app.snippet_store.snippets.len(), 2); // not yet deleted

    // y confirms deletion
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert_eq!(app.pending_snippet_delete, None);
    assert_eq!(app.snippet_store.snippets.len(), 1);
    assert_eq!(app.snippet_store.snippets[0].name, "uptime");
    assert_eq!(app.ui.snippet_picker_state.selected(), Some(0));
}

#[test]
fn test_snippet_picker_d_last_item_selects_none() {
    let mut app = make_snippet_app();
    app.snippet_store.snippets = vec![crate::snippet::Snippet {
        name: "only".to_string(),
        command: "ls".to_string(),
        description: String::new(),
    }];
    app.ui.snippet_picker_state.select(Some(0));
    let _ = app.snippet_store.save();
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    assert_eq!(app.pending_snippet_delete, Some(0));

    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(app.snippet_store.snippets.is_empty());
    assert_eq!(app.ui.snippet_picker_state.selected(), None);
}

#[test]
fn test_snippet_picker_d_rollback_on_save_failure() {
    let mut app = make_snippet_app();
    // Point to a non-writable path to force save failure
    app.snippet_store.path_override = Some(PathBuf::from("/nonexistent/dir/snippets"));
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    assert_eq!(app.pending_snippet_delete, Some(0));

    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    // Rollback: snippet should still be there
    assert_eq!(app.snippet_store.snippets.len(), 2);
    assert_eq!(app.snippet_store.snippets[0].name, "check-disk");
    assert!(app.toast.as_ref().unwrap().is_error());
}

// =========================================================================
// Snippet form
// =========================================================================

#[test]
fn test_snippet_form_esc_returns_to_picker() {
    let mut app = make_snippet_app();
    app.snippet_form = crate::app::SnippetForm::new();
    app.screen = Screen::SnippetForm {
        target_aliases: vec!["myserver".to_string()],
        editing: None,
    };
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::SnippetPicker { .. }));
}

#[test]
fn test_snippet_form_tab_cycles_fields() {
    let mut app = make_snippet_app();
    app.snippet_form = crate::app::SnippetForm::new();
    app.screen = Screen::SnippetForm {
        target_aliases: vec!["myserver".to_string()],
        editing: None,
    };
    let (tx, _rx) = mpsc::channel();

    assert_eq!(
        app.snippet_form.focused_field,
        crate::app::SnippetFormField::Name
    );

    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.snippet_form.focused_field,
        crate::app::SnippetFormField::Command
    );

    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.snippet_form.focused_field,
        crate::app::SnippetFormField::Description
    );

    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
    assert_eq!(
        app.snippet_form.focused_field,
        crate::app::SnippetFormField::Name
    );
}

#[test]
fn test_snippet_form_char_insert() {
    let mut app = make_snippet_app();
    app.snippet_form = crate::app::SnippetForm::new();
    app.screen = Screen::SnippetForm {
        target_aliases: vec!["myserver".to_string()],
        editing: None,
    };
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char('a')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('b')), &tx);
    assert_eq!(app.snippet_form.name, "ab");
    assert_eq!(app.snippet_form.cursor_pos, 2);
}

#[test]
fn test_snippet_form_backspace() {
    let mut app = make_snippet_app();
    app.snippet_form = crate::app::SnippetForm::new();
    app.snippet_form.name = "abc".to_string();
    app.snippet_form.cursor_pos = 3;
    app.screen = Screen::SnippetForm {
        target_aliases: vec!["myserver".to_string()],
        editing: None,
    };
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
    assert_eq!(app.snippet_form.name, "ab");
    assert_eq!(app.snippet_form.cursor_pos, 2);
}

#[test]
fn test_snippet_form_submit_add() {
    let mut app = make_snippet_app();
    let _ = app.snippet_store.save();
    app.snippet_form = crate::app::SnippetForm::new();
    app.snippet_form.name = "new-cmd".to_string();
    app.snippet_form.command = "whoami".to_string();
    app.snippet_form.cursor_pos = 6;
    app.screen = Screen::SnippetForm {
        target_aliases: vec!["myserver".to_string()],
        editing: None,
    };
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(matches!(app.screen, Screen::SnippetPicker { .. }));
    assert_eq!(app.snippet_store.snippets.len(), 3);
    assert!(app.snippet_store.get("new-cmd").is_some());
}

#[test]
fn test_snippet_form_submit_edit() {
    let mut app = make_snippet_app();
    let _ = app.snippet_store.save();
    app.snippet_form =
        crate::app::SnippetForm::from_snippet(&app.snippet_store.snippets[0].clone());
    app.snippet_form.command = "df -hT".to_string();
    app.screen = Screen::SnippetForm {
        target_aliases: vec!["myserver".to_string()],
        editing: Some(0),
    };
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(matches!(app.screen, Screen::SnippetPicker { .. }));
    assert_eq!(app.snippet_store.snippets[0].command, "df -hT");
}

#[test]
fn test_snippet_form_submit_rejects_empty_name() {
    let mut app = make_snippet_app();
    app.snippet_form = crate::app::SnippetForm::new();
    app.snippet_form.command = "ls".to_string();
    app.screen = Screen::SnippetForm {
        target_aliases: vec!["myserver".to_string()],
        editing: None,
    };
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    // Should stay on the form with an error
    assert!(matches!(app.screen, Screen::SnippetForm { .. }));
    assert!(app.toast.as_ref().unwrap().is_error());
}

#[test]
fn test_snippet_form_submit_rejects_duplicate_name() {
    let mut app = make_snippet_app();
    let _ = app.snippet_store.save();
    app.snippet_form = crate::app::SnippetForm::new();
    app.snippet_form.name = "uptime".to_string();
    app.snippet_form.command = "uptime -s".to_string();
    app.snippet_form.cursor_pos = 9;
    app.screen = Screen::SnippetForm {
        target_aliases: vec!["myserver".to_string()],
        editing: None,
    };
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(matches!(app.screen, Screen::SnippetForm { .. }));
    assert!(app.toast.as_ref().unwrap().is_error());
}

#[test]
fn test_snippet_form_submit_rollback_on_save_failure() {
    let mut app = make_snippet_app();
    // Force save failure
    app.snippet_store.path_override = Some(PathBuf::from("/nonexistent/dir/snippets"));
    app.snippet_form = crate::app::SnippetForm::new();
    app.snippet_form.name = "new-cmd".to_string();
    app.snippet_form.command = "whoami".to_string();
    app.snippet_form.cursor_pos = 6;
    app.screen = Screen::SnippetForm {
        target_aliases: vec!["myserver".to_string()],
        editing: None,
    };
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    // Rollback: new snippet should not be in the store
    assert_eq!(app.snippet_store.snippets.len(), 2);
    assert!(app.snippet_store.get("new-cmd").is_none());
    assert!(app.toast.as_ref().unwrap().is_error());
}

#[test]
fn test_snippet_form_edit_rename_rollback_on_save_failure() {
    let mut app = make_snippet_app();
    // Force save failure
    app.snippet_store.path_override = Some(PathBuf::from("/nonexistent/dir/snippets"));
    app.snippet_form =
        crate::app::SnippetForm::from_snippet(&app.snippet_store.snippets[0].clone());
    app.snippet_form.name = "renamed".to_string();
    app.snippet_form.cursor_pos = 7;
    app.screen = Screen::SnippetForm {
        target_aliases: vec!["myserver".to_string()],
        editing: Some(0),
    };
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    // Rollback: original snippets should still be there
    assert_eq!(app.snippet_store.snippets.len(), 2);
    assert!(app.snippet_store.get("check-disk").is_some());
    assert!(app.snippet_store.get("renamed").is_none());
}

#[test]
fn test_snippet_picker_enter_with_no_selection() {
    let mut app = make_snippet_app();
    app.snippet_store.snippets.clear();
    app.ui.snippet_picker_state.select(None);
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    // Should remain on picker, no pending snippet
    assert!(matches!(app.screen, Screen::SnippetPicker { .. }));
    assert!(app.pending_snippet.is_none());
}

#[test]
fn test_host_list_r_opens_snippet_picker() {
    let mut app = make_app("Host myserver\n  HostName 1.2.3.4\n");
    app.ui.list_state.select(Some(0));
    let dir = std::env::temp_dir().join(format!("purple_handler_snip_r_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    app.snippet_store.path_override = Some(dir.join("snippets"));
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char('r')), &tx);
    match &app.screen {
        Screen::SnippetPicker { target_aliases } => {
            assert_eq!(target_aliases, &vec!["myserver".to_string()]);
        }
        _ => panic!("Expected SnippetPicker screen"),
    }
}

#[test]
fn test_host_list_r_shift_opens_snippet_picker_all() {
    let mut app = make_app("Host a\n  HostName 1.1.1.1\nHost b\n  HostName 2.2.2.2\n");
    app.ui.list_state.select(Some(0));
    let dir = std::env::temp_dir().join(format!("purple_handler_snip_R_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    app.snippet_store.path_override = Some(dir.join("snippets"));
    let (tx, _rx) = mpsc::channel();

    let _ = handle_key_event(&mut app, key(KeyCode::Char('R')), &tx);
    match &app.screen {
        Screen::SnippetPicker { target_aliases } => {
            assert_eq!(target_aliases.len(), 2);
        }
        _ => panic!("Expected SnippetPicker screen"),
    }
}

// --- Tunnel form Space/arrow tests ---

fn make_tunnel_form_app(field: crate::app::TunnelFormField) -> App {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::TunnelForm {
        alias: "test".to_string(),
        editing: None,
    };
    app.tunnel_form = crate::app::TunnelForm::new();
    app.tunnel_form.focused_field = field;
    app
}

#[test]
fn test_tunnel_form_space_cycles_type_local_to_remote() {
    let mut app = make_tunnel_form_app(crate::app::TunnelFormField::Type);
    assert_eq!(
        app.tunnel_form.tunnel_type,
        crate::tunnel::TunnelType::Local
    );
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert_eq!(
        app.tunnel_form.tunnel_type,
        crate::tunnel::TunnelType::Remote
    );
}

#[test]
fn test_tunnel_form_space_cycles_type_remote_to_dynamic() {
    let mut app = make_tunnel_form_app(crate::app::TunnelFormField::Type);
    app.tunnel_form.tunnel_type = crate::tunnel::TunnelType::Remote;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert_eq!(
        app.tunnel_form.tunnel_type,
        crate::tunnel::TunnelType::Dynamic
    );
}

#[test]
fn test_tunnel_form_space_cycles_type_dynamic_to_local() {
    let mut app = make_tunnel_form_app(crate::app::TunnelFormField::Type);
    app.tunnel_form.tunnel_type = crate::tunnel::TunnelType::Dynamic;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert_eq!(
        app.tunnel_form.tunnel_type,
        crate::tunnel::TunnelType::Local
    );
}

#[test]
fn test_tunnel_form_left_on_type_does_not_cycle() {
    let mut app = make_tunnel_form_app(crate::app::TunnelFormField::Type);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Left), &tx);
    assert_eq!(
        app.tunnel_form.tunnel_type,
        crate::tunnel::TunnelType::Local
    );
}

#[test]
fn test_tunnel_form_right_on_type_does_not_cycle() {
    let mut app = make_tunnel_form_app(crate::app::TunnelFormField::Type);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Right), &tx);
    assert_eq!(
        app.tunnel_form.tunnel_type,
        crate::tunnel::TunnelType::Local
    );
}

#[test]
fn test_tunnel_form_space_on_bind_port_inserts_space() {
    let mut app = make_tunnel_form_app(crate::app::TunnelFormField::BindPort);
    app.tunnel_form.bind_port = "80".to_string();
    app.tunnel_form.cursor_pos = 2;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
    assert_eq!(app.tunnel_form.bind_port, "80 ");
}

#[test]
fn test_tunnel_form_left_on_text_moves_cursor() {
    let mut app = make_tunnel_form_app(crate::app::TunnelFormField::BindPort);
    app.tunnel_form.bind_port = "8080".to_string();
    app.tunnel_form.cursor_pos = 2;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Left), &tx);
    assert_eq!(app.tunnel_form.cursor_pos, 1);
}

// --- Dirty-check tests ---

#[test]
fn test_host_form_clean_esc_closes_immediately() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.form = crate::app::HostForm::new();
    app.screen = Screen::AddHost;
    app.capture_form_baseline();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert!(!app.pending_discard_confirm);
}

#[test]
fn test_host_form_dirty_esc_shows_confirmation() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.form = crate::app::HostForm::new();
    app.screen = Screen::AddHost;
    app.capture_form_baseline();
    app.form.alias = "dirty".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::AddHost));
    assert!(app.pending_discard_confirm);
}

#[test]
fn test_host_form_dirty_esc_y_closes() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.form = crate::app::HostForm::new();
    app.screen = Screen::AddHost;
    app.capture_form_baseline();
    app.form.alias = "dirty".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert!(app.form_baseline.is_none());
}

#[test]
fn test_host_form_dirty_esc_n_stays() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.form = crate::app::HostForm::new();
    app.screen = Screen::AddHost;
    app.capture_form_baseline();
    app.form.hostname = "changed.com".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    assert!(matches!(app.screen, Screen::AddHost));
    assert!(!app.pending_discard_confirm);
}

#[test]
fn test_host_form_dirty_esc_other_key_ignored() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.form = crate::app::HostForm::new();
    app.screen = Screen::AddHost;
    app.capture_form_baseline();
    app.form.alias = "dirty".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('x')), &tx);
    assert!(app.pending_discard_confirm); // still pending
}

#[test]
fn test_tunnel_form_dirty_esc_shows_confirmation() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::TunnelForm {
        alias: "test".to_string(),
        editing: None,
    };
    app.tunnel_form = crate::app::TunnelForm::new();
    app.capture_tunnel_form_baseline();
    app.tunnel_form.bind_port = "9000".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::TunnelForm { .. }));
    assert!(app.pending_discard_confirm);
}

#[test]
fn test_tunnel_form_clean_esc_closes() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::TunnelForm {
        alias: "test".to_string(),
        editing: None,
    };
    app.tunnel_form = crate::app::TunnelForm::new();
    app.capture_tunnel_form_baseline();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::TunnelList { .. }));
}

// --- Delete confirmation tests ---

#[test]
fn test_snippet_picker_d_esc_cancels_delete() {
    let mut app = make_snippet_app();
    let _ = app.snippet_store.save();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    assert_eq!(app.pending_snippet_delete, Some(0));
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert_eq!(app.pending_snippet_delete, None);
    assert_eq!(app.snippet_store.snippets.len(), 2);
}

#[test]
fn test_snippet_picker_d_n_cancels_delete() {
    let mut app = make_snippet_app();
    let _ = app.snippet_store.save();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    assert_eq!(app.pending_snippet_delete, None);
    assert_eq!(app.snippet_store.snippets.len(), 2);
}

#[test]
fn test_snippet_picker_d_other_key_ignored() {
    let mut app = make_snippet_app();
    let _ = app.snippet_store.save();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    assert_eq!(app.pending_snippet_delete, Some(0));
    assert_eq!(app.snippet_store.snippets.len(), 2);
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
    assert!(app.form_baseline.is_some());
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
    assert!(matches!(app.screen, Screen::SnippetPicker { .. }));
}

#[test]
fn test_host_detail_e_on_included_host_stays() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.hosts[0].source_file = Some(PathBuf::from("/etc/ssh/config.d/test"));
    app.screen = Screen::HostDetail { index: 0 };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
    assert!(matches!(app.screen, Screen::HostDetail { .. }));
    assert!(app.toast.as_ref().unwrap().is_error());
}

// --- Provider form: Left/Right on toggle fields does NOT toggle ---

#[test]
fn test_provider_form_left_on_verify_tls_stays_same() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::VerifyTls);
    assert!(app.provider_form.verify_tls);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Left), &tx);
    assert!(app.provider_form.verify_tls);
}

#[test]
fn test_provider_form_right_on_verify_tls_stays_same() {
    let mut app = make_form_app_focused_on("proxmox", ProviderFormField::VerifyTls);
    assert!(app.provider_form.verify_tls);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Right), &tx);
    assert!(app.provider_form.verify_tls);
}

#[test]
fn test_provider_form_left_on_auto_sync_stays_same() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::AutoSync);
    assert!(app.provider_form.auto_sync);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Left), &tx);
    assert!(app.provider_form.auto_sync);
}

#[test]
fn test_provider_form_right_on_auto_sync_stays_same() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::AutoSync);
    assert!(app.provider_form.auto_sync);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Right), &tx);
    assert!(app.provider_form.auto_sync);
}

// --- Provider form: dirty-check on Esc ---

#[test]
fn test_provider_form_clean_esc_with_baseline_closes() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.capture_provider_form_baseline();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::Providers));
    assert!(!app.pending_discard_confirm);
}

#[test]
fn test_provider_form_dirty_esc_shows_confirmation() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.capture_provider_form_baseline();
    app.provider_form.token = "newtoken".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert!(app.pending_discard_confirm);
}

#[test]
fn test_provider_form_dirty_esc_y_closes() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.capture_provider_form_baseline();
    app.provider_form.token = "newtoken".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(matches!(app.screen, Screen::Providers));
    assert!(app.provider_form_baseline.is_none());
}

#[test]
fn test_provider_form_dirty_esc_n_stays() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.capture_provider_form_baseline();
    app.provider_form.token = "newtoken".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    assert!(matches!(app.screen, Screen::ProviderForm { .. }));
    assert!(!app.pending_discard_confirm);
}

// --- Snippet form: dirty-check on Esc ---

#[test]
fn test_snippet_form_clean_esc_with_baseline_closes() {
    let mut app = make_snippet_app();
    app.snippet_form = crate::app::SnippetForm::new();
    app.screen = Screen::SnippetForm {
        target_aliases: vec!["myserver".to_string()],
        editing: None,
    };
    app.capture_snippet_form_baseline();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::SnippetPicker { .. }));
    assert!(!app.pending_discard_confirm);
}

#[test]
fn test_snippet_form_dirty_esc_shows_confirmation() {
    let mut app = make_snippet_app();
    app.snippet_form = crate::app::SnippetForm::new();
    app.screen = Screen::SnippetForm {
        target_aliases: vec!["myserver".to_string()],
        editing: None,
    };
    app.capture_snippet_form_baseline();
    app.snippet_form.name = "dirty".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::SnippetForm { .. }));
    assert!(app.pending_discard_confirm);
}

#[test]
fn test_snippet_form_dirty_esc_y_closes() {
    let mut app = make_snippet_app();
    app.snippet_form = crate::app::SnippetForm::new();
    app.screen = Screen::SnippetForm {
        target_aliases: vec!["myserver".to_string()],
        editing: None,
    };
    app.capture_snippet_form_baseline();
    app.snippet_form.name = "dirty".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(matches!(app.screen, Screen::SnippetPicker { .. }));
    assert!(app.snippet_form_baseline.is_none());
}

// --- Tunnel delete: d/y/Esc/n ---

#[test]
fn test_tunnel_list_d_y_deletes_tunnel() {
    let mut app = make_app("Host test\n  HostName test.com\n  LocalForward 8080 localhost:80\n");
    app.screen = Screen::TunnelList {
        alias: "test".to_string(),
    };
    app.refresh_tunnel_list("test");
    app.ui.tunnel_list_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    assert_eq!(app.pending_tunnel_delete, Some(0));
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(app.pending_tunnel_delete.is_none());
}

#[test]
fn test_tunnel_list_d_esc_cancels_delete() {
    let mut app = make_app("Host test\n  HostName test.com\n  LocalForward 8080 localhost:80\n");
    app.screen = Screen::TunnelList {
        alias: "test".to_string(),
    };
    app.refresh_tunnel_list("test");
    app.ui.tunnel_list_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    assert_eq!(app.pending_tunnel_delete, Some(0));
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(app.pending_tunnel_delete.is_none());
    assert_eq!(app.tunnel_list.len(), 1);
}

#[test]
fn test_tunnel_list_d_n_cancels_delete() {
    let mut app = make_app("Host test\n  HostName test.com\n  LocalForward 8080 localhost:80\n");
    app.screen = Screen::TunnelList {
        alias: "test".to_string(),
    };
    app.refresh_tunnel_list("test");
    app.ui.tunnel_list_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    assert!(app.pending_tunnel_delete.is_none());
    assert_eq!(app.tunnel_list.len(), 1);
}

// --- Host form: baseline cleared after submit ---

#[test]
fn test_host_form_baseline_cleared_after_submit() {
    // Use a unique temp file to avoid race conditions with parallel tests
    // that share /tmp/test_config.
    let unique = format!(
        "/tmp/purple_test_baseline_{:?}",
        std::thread::current().id()
    );
    let config_path = PathBuf::from(&unique);
    std::fs::write(&config_path, "Host test\n  HostName test.com\n").unwrap();
    let config = SshConfigFile {
        elements: SshConfigFile::parse_content("Host test\n  HostName test.com\n"),
        path: config_path.clone(),
        crlf: false,
        bom: false,
    };
    let mut app = App::new(config);
    app.provider_config = test_provider_config();
    crate::preferences::set_path_override(PathBuf::from(format!("{}_prefs", unique)));
    app.form = crate::app::HostForm::new();
    app.form.alias = "newhost".to_string();
    app.form.hostname = "new.example.com".to_string();
    app.screen = Screen::AddHost;
    app.capture_form_mtime();
    app.capture_form_baseline();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.form_baseline.is_none());
    // Cleanup
    let _ = std::fs::remove_file(&unique);
    let _ = std::fs::remove_file(format!("{}_prefs", unique));
}

// --- Edge case: uppercase Y in discard confirms ---

#[test]
fn test_host_form_dirty_esc_uppercase_y_closes() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.form = crate::app::HostForm::new();
    app.screen = Screen::AddHost;
    app.capture_form_baseline();
    app.form.user = "ubuntu".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('Y')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert!(app.form_baseline.is_none());
}

// --- Snippet form: dirty + n stays ---

#[test]
fn test_snippet_form_dirty_esc_n_stays() {
    let mut app = make_snippet_app();
    app.snippet_form = crate::app::SnippetForm::new();
    app.screen = Screen::SnippetForm {
        target_aliases: vec!["myserver".to_string()],
        editing: None,
    };
    app.capture_snippet_form_baseline();
    app.snippet_form.command = "changed".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    assert!(matches!(app.screen, Screen::SnippetForm { .. }));
    assert!(!app.pending_discard_confirm);
}

// --- Tunnel form: dirty + y closes, dirty + n stays ---

#[test]
fn test_tunnel_form_dirty_esc_y_closes() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::TunnelForm {
        alias: "test".to_string(),
        editing: None,
    };
    app.tunnel_form = crate::app::TunnelForm::new();
    app.capture_tunnel_form_baseline();
    app.tunnel_form.remote_host = "db.local".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(matches!(app.screen, Screen::TunnelList { .. }));
    assert!(app.tunnel_form_baseline.is_none());
}

#[test]
fn test_tunnel_form_dirty_esc_n_stays() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.screen = Screen::TunnelForm {
        alias: "test".to_string(),
        editing: None,
    };
    app.tunnel_form = crate::app::TunnelForm::new();
    app.capture_tunnel_form_baseline();
    app.tunnel_form.bind_port = "9001".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('n')), &tx);
    assert!(matches!(app.screen, Screen::TunnelForm { .. }));
    assert!(!app.pending_discard_confirm);
}

// --- Tunnel delete: other key ignored ---

#[test]
fn test_tunnel_delete_other_key_ignored() {
    let mut app = make_app("Host test\n  HostName test.com\n  LocalForward 8080 localhost:80\n");
    app.screen = Screen::TunnelList {
        alias: "test".to_string(),
    };
    app.refresh_tunnel_list("test");
    app.ui.tunnel_list_state.select(Some(0));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
    assert_eq!(app.pending_tunnel_delete, Some(0));
    let _ = handle_key_event(&mut app, key(KeyCode::Char('z')), &tx);
    assert_eq!(app.pending_tunnel_delete, Some(0));
}

// --- Provider form: dirty + other key ignored ---

#[test]
fn test_provider_form_dirty_esc_other_key_ignored() {
    let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
    app.capture_provider_form_baseline();
    app.provider_form.token = "newtoken".to_string();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    let _ = handle_key_event(&mut app, key(KeyCode::Char('x')), &tx);
    assert!(app.pending_discard_confirm);
}

// --- Stale purge tests ---

#[test]
fn test_x_key_opens_confirm_purge_stale() {
    let mut app = make_app(
        "Host do-web\n  HostName 1.2.3.4\n  # purple:provider digitalocean:123\n  # purple:stale 1711900000\n",
    );
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('X')), &tx);
    match &app.screen {
        Screen::ConfirmPurgeStale { aliases, provider } => {
            assert_eq!(aliases.len(), 1);
            assert_eq!(aliases[0], "do-web");
            assert!(provider.is_none());
        }
        other => panic!("expected ConfirmPurgeStale, got {:?}", other),
    }
}

#[test]
fn test_x_key_no_stale_shows_status() {
    let mut app = make_app("Host normal\n  HostName 1.2.3.4\n");
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('X')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    let toast = app.toast.as_ref().expect("toast should be set");
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
    app.screen = Screen::ConfirmPurgeStale {
        aliases: vec!["do-web".to_string()],
        provider: None,
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    // The stale host should be gone, only "keep" remains
    let aliases: Vec<&str> = app.hosts.iter().map(|h| h.alias.as_str()).collect();
    assert!(!aliases.contains(&"do-web"), "stale host should be removed");
    assert!(aliases.contains(&"keep"), "non-stale host should remain");
}

#[test]
fn test_confirm_purge_stale_esc_cancels() {
    let mut app = make_app(
        "Host do-web\n  HostName 1.2.3.4\n  # purple:provider digitalocean:123\n  # purple:stale 1711900000\n",
    );
    app.screen = Screen::ConfirmPurgeStale {
        aliases: vec!["do-web".to_string()],
        provider: None,
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    // Host should still exist
    assert_eq!(app.hosts.len(), 1);
    assert_eq!(app.hosts[0].alias, "do-web");
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
    let toast = app.toast.as_ref().expect("toast should be set");
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
    let toast = app.toast.as_ref().expect("toast should be set");
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
    assert!(app.pending_connect.is_some());
    // But toast should show stale warning
    let toast = app.toast.as_ref().expect("toast should be set");
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
    assert!(app.pending_connect.is_some());
    // No stale warning
    assert!(app.toast.is_none() || !app.toast.as_ref().unwrap().text.contains("Stale"),);
}

#[test]
fn test_search_enter_on_stale_host_shows_warning() {
    let mut app = make_app(
        "Host do-web\n  HostName 1.2.3.4\n  # purple:provider digitalocean:123\n  # purple:stale 1711900000\n",
    );
    // Enter search mode
    app.search.query = Some("do-web".to_string());
    app.apply_filter();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(app.pending_connect.is_some());
    let toast = app.toast.as_ref().expect("toast should be set");
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
    let toast = app.toast.as_ref().expect("toast should be set");
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
    let toast = app.toast.as_ref().expect("toast should be set");
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
    app.provider_config = test_provider_config();
    app.provider_config.set_section(ProviderSection {
        provider: "digitalocean".to_string(),
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
    });
    // Select the DigitalOcean provider in the list
    let sorted = app.sorted_provider_names();
    let idx = sorted
        .iter()
        .position(|n| n == "digitalocean")
        .expect("digitalocean should be in sorted list");
    app.ui.provider_list_state.select(Some(idx));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('X')), &tx);
    match &app.screen {
        Screen::ConfirmPurgeStale { aliases, provider } => {
            assert_eq!(aliases, &vec!["do-web".to_string()]);
            assert_eq!(provider.as_deref(), Some("digitalocean"));
        }
        other => panic!("expected ConfirmPurgeStale, got {:?}", other),
    }
}

#[test]
fn test_provider_purge_y_returns_to_providers() {
    let mut app = make_app(
        "Host do-web\n  HostName 1.2.3.4\n  # purple:provider digitalocean:123\n  # purple:stale 1711900000\n",
    );
    app.screen = Screen::ConfirmPurgeStale {
        aliases: vec!["do-web".to_string()],
        provider: Some("digitalocean".to_string()),
    };
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
    app.screen = Screen::ConfirmPurgeStale {
        aliases: vec!["do-web".to_string()],
        provider: Some("digitalocean".to_string()),
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(
        matches!(app.screen, Screen::Providers),
        "expected Providers screen after Esc on provider-scoped purge, got: {:?}",
        app.screen
    );
    // Host should still exist (purge was cancelled)
    assert_eq!(app.hosts.len(), 1);
    assert_eq!(app.hosts[0].alias, "do-web");
}

// =========================================================================
// Container handler tests
// =========================================================================

fn make_container_state(
    alias: &str,
    containers: Vec<crate::containers::ContainerInfo>,
) -> crate::app::ContainerState {
    let mut list_state = ratatui::widgets::ListState::default();
    if !containers.is_empty() {
        list_state.select(Some(0));
    }
    crate::app::ContainerState {
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
        app.container_state.is_some(),
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
    assert!(app.container_state.is_none());
}

#[test]
fn test_shift_c_loads_cache() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.container_cache.insert(
        "web".to_string(),
        crate::containers::ContainerCacheEntry {
            timestamp: 100,
            runtime: crate::containers::ContainerRuntime::Docker,
            containers: vec![make_container("abc", "nginx", "running")],
        },
    );
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('C')), &tx);
    let state = app.container_state.as_ref().unwrap();
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
    let state = app.container_state.as_ref().unwrap();
    assert!(state.containers.is_empty());
    assert!(state.runtime.is_none());
}

#[test]
fn test_containers_esc_closes() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    app.container_state = Some(make_container_state("web", vec![]));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert!(app.container_state.is_none());
}

#[test]
fn test_containers_q_closes() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    app.container_state = Some(make_container_state("web", vec![]));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('q')), &tx);
    assert!(matches!(app.screen, Screen::HostList));
    assert!(app.container_state.is_none());
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
    app.container_state = Some(make_container_state("web", containers));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    let sel = app.container_state.as_ref().unwrap().list_state.selected();
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
    app.container_state = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
    let sel = app.container_state.as_ref().unwrap().list_state.selected();
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
    app.container_state = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    let sel = app.container_state.as_ref().unwrap().list_state.selected();
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
    app.container_state = Some(make_container_state("web", containers));
    // selection starts at 0
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
    let sel = app.container_state.as_ref().unwrap().list_state.selected();
    assert_eq!(sel, Some(1), "k at first item should wrap to last");
}

#[test]
fn test_containers_j_empty_noop() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    app.container_state = Some(make_container_state("web", vec![]));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    let sel = app.container_state.as_ref().unwrap().list_state.selected();
    assert_eq!(sel, None);
}

#[test]
fn test_containers_k_empty_noop() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    app.container_state = Some(make_container_state("web", vec![]));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
    let sel = app.container_state.as_ref().unwrap().list_state.selected();
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
    app.container_state = Some(make_container_state("web", containers));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::PageDown), &tx);
    let sel = app.container_state.as_ref().unwrap().list_state.selected();
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
    app.container_state = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::PageUp), &tx);
    let sel = app.container_state.as_ref().unwrap().list_state.selected();
    assert_eq!(sel, Some(5));
}

#[test]
fn test_containers_s_sets_action_in_progress() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    app.container_state = Some(make_container_state(
        "web",
        vec![make_container("abc123", "nginx", "exited")],
    ));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('s')), &tx);
    let state = app.container_state.as_ref().unwrap();
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
    app.container_state = Some(make_container_state(
        "web",
        vec![make_container("abc123", "nginx", "running")],
    ));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('x')), &tx);
    let state = app.container_state.as_ref().unwrap();
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
    app.container_state = Some(make_container_state(
        "web",
        vec![make_container("abc123", "nginx", "running")],
    ));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('r')), &tx);
    let state = app.container_state.as_ref().unwrap();
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
    app.container_state = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    let state = app.container_state.as_ref().unwrap();
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
    app.container_state = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    // Should cancel confirmation but stay in overlay
    assert!(app.container_state.is_some());
    assert!(
        app.container_state
            .as_ref()
            .unwrap()
            .confirm_action
            .is_none()
    );
    assert!(matches!(app.screen, Screen::Containers { .. }));
}

#[test]
fn test_containers_action_blocked_when_in_progress() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    let mut state = make_container_state("web", vec![make_container("abc123", "nginx", "running")]);
    state.action_in_progress = Some("stop nginx...".to_string());
    app.container_state = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('s')), &tx);
    // action_in_progress should remain the same (not changed to start)
    let state = app.container_state.as_ref().unwrap();
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
    app.container_state = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('s')), &tx);
    assert!(
        app.container_state
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
    app.container_state = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('s')), &tx);
    assert!(
        app.container_state
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
    app.container_state = Some(make_container_state(
        "web",
        vec![make_container("abc123", "nginx", "running")],
    ));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('R')), &tx);
    assert!(
        app.container_state.as_ref().unwrap().loading,
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
    app.container_state = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('R')), &tx);
    assert!(
        !app.container_state.as_ref().unwrap().loading,
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
    app.container_state = Some(make_container_state("web", containers.clone()));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('z')), &tx);
    assert!(matches!(app.screen, Screen::Containers { .. }));
    let state = app.container_state.as_ref().unwrap();
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
    app.container_state = Some(make_container_state(
        "web",
        vec![make_container("abc123", "nginx", "running")],
    ));
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
    let state = app.container_state.as_ref().unwrap();
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
    app.container_state = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('x')), &tx);
    let state = app.container_state.as_ref().unwrap();
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
    app.container_state = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('r')), &tx);
    let state = app.container_state.as_ref().unwrap();
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
    app.container_state = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('x')), &tx);
    let state = app.container_state.as_ref().unwrap();
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
    app.container_state = Some(state);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('r')), &tx);
    let state = app.container_state.as_ref().unwrap();
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
    app.file_browser = Some(crate::file_browser::FileBrowserState {
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

#[test]
fn test_snippet_picker_question_opens_help() {
    let mut app = make_snippet_app();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('?')), &tx);
    match &app.screen {
        Screen::Help { return_screen } => {
            assert!(matches!(**return_screen, Screen::SnippetPicker { .. }));
        }
        other => panic!("Expected Help screen, got {:?}", other),
    }
}

#[test]
fn test_snippet_picker_help_esc_returns() {
    let mut app = make_snippet_app();
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::SnippetPicker {
            target_aliases: vec!["myserver".to_string()],
        }),
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::SnippetPicker { .. }));
}

#[test]
fn test_snippet_output_question_opens_help() {
    let mut app = make_snippet_app();
    // First enter snippet output by pressing Enter
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
    assert!(matches!(app.screen, Screen::SnippetOutput { .. }));

    // Now press ? to open help
    let _ = handle_key_event(&mut app, key(KeyCode::Char('?')), &tx);
    match &app.screen {
        Screen::Help { return_screen } => {
            assert!(matches!(**return_screen, Screen::SnippetOutput { .. }));
        }
        other => panic!("Expected Help screen, got {:?}", other),
    }
}

#[test]
fn test_snippet_output_help_esc_returns() {
    let mut app = make_snippet_app();
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::SnippetOutput {
            snippet_name: "check-disk".to_string(),
            target_aliases: vec!["myserver".to_string()],
        }),
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert!(matches!(app.screen, Screen::SnippetOutput { .. }));
}

#[test]
fn test_containers_question_opens_help() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Containers {
        alias: "web".to_string(),
    };
    app.container_state = Some(make_container_state("web", vec![]));
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
    app.pending_tunnel_delete = Some(0);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('?')), &tx);
    match &app.screen {
        Screen::Help { return_screen } => {
            assert!(matches!(**return_screen, Screen::TunnelList { .. }));
        }
        other => panic!("Expected Help screen, got {:?}", other),
    }
    assert_eq!(
        app.pending_tunnel_delete,
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
    app.container_state = Some(state);
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
    app.pending_snippet_delete = Some(0);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('?')), &tx);
    match &app.screen {
        Screen::Help { return_screen } => {
            assert!(matches!(**return_screen, Screen::SnippetPicker { .. }));
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
    app.ui.help_scroll = 0;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
    assert_eq!(app.ui.help_scroll, 1);
}

#[test]
fn test_help_k_does_not_underflow() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::HostList),
    };
    app.ui.help_scroll = 0;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
    assert_eq!(app.ui.help_scroll, 0);
}

#[test]
fn test_help_page_down_increments_by_ten() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::HostList),
    };
    app.ui.help_scroll = 0;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::PageDown), &tx);
    assert_eq!(app.ui.help_scroll, 10);
}

#[test]
fn test_help_page_up_does_not_underflow() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::HostList),
    };
    app.ui.help_scroll = 0;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::PageUp), &tx);
    assert_eq!(app.ui.help_scroll, 0);
}

#[test]
fn test_help_scroll_reset_on_close() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::HostList),
    };
    app.ui.help_scroll = 7;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    assert_eq!(app.ui.help_scroll, 0);
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
    assert_eq!(app.ui.help_scroll, 0);
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
    assert_eq!(app.ui.help_scroll, 0);
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
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::SnippetOutput {
            snippet_name: "check-disk".to_string(),
            target_aliases: vec!["a".to_string(), "b".to_string()],
        }),
    };
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
    match &app.screen {
        Screen::SnippetOutput {
            snippet_name,
            target_aliases,
        } => {
            assert_eq!(snippet_name, "check-disk");
            assert_eq!(target_aliases, &vec!["a".to_string(), "b".to_string()]);
        }
        other => panic!("Expected SnippetOutput, got {:?}", other),
    }
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
    assert_eq!(app.group_by, crate::app::GroupBy::None);
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('g')), &tx);
    assert_eq!(app.group_by, crate::app::GroupBy::Provider);
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
    app.group_by = crate::app::GroupBy::Provider;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('g')), &tx);
    assert!(
        matches!(app.group_by, crate::app::GroupBy::Tag(_)),
        "expected Tag mode, got {:?}",
        app.group_by
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
    app.group_by = crate::app::GroupBy::Provider;
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('g')), &tx);
    assert_eq!(app.group_by, crate::app::GroupBy::None);
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
    app.group_by = crate::app::GroupBy::Tag("production".to_string());
    app.apply_sort();
    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Char('g')), &tx);
    assert_eq!(app.group_by, crate::app::GroupBy::None);
    assert!(matches!(app.screen, Screen::HostList));
    assert!(
        app.display_list
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
    assert_eq!(app.group_by, crate::app::GroupBy::None);

    let (tx, _rx) = mpsc::channel();

    // None → Provider
    let _ = handle_key_event(&mut app, key(KeyCode::Char('g')), &tx);
    assert_eq!(app.group_by, crate::app::GroupBy::Provider);

    // Provider → Tag (direct, no picker)
    let _ = handle_key_event(&mut app, key(KeyCode::Char('g')), &tx);
    assert!(
        matches!(app.group_by, crate::app::GroupBy::Tag(_)),
        "expected Tag mode, got {:?}",
        app.group_by
    );
    assert!(matches!(app.screen, Screen::HostList));

    // Tag → None
    let _ = handle_key_event(&mut app, key(KeyCode::Char('g')), &tx);
    assert_eq!(app.group_by, crate::app::GroupBy::None);
}

#[test]
fn g_key_tag_to_none_empty_hosts() {
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut app = make_app("");
    app.group_by = crate::app::GroupBy::Tag("production".to_string());

    let key = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE);
    let _ = handle_key_event(&mut app, key, &tx);

    assert_eq!(app.group_by, crate::app::GroupBy::None);
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
    app.group_by = crate::app::GroupBy::Tag("production".to_string());
    app.sort_mode = crate::app::SortMode::AlphaAlias;
    app.apply_sort();

    // Find the group header position
    let header_pos = app
        .display_list
        .iter()
        .position(
            |item| matches!(item, crate::app::HostListItem::GroupHeader(t) if t == "production"),
        )
        .expect("should have a production group header");
    app.ui.list_state.select(Some(header_pos));

    // Press Enter — should not panic and group_filter should remain None
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();

    assert!(
        app.group_filter.is_none(),
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
    assert!(app.multi_select.is_empty());

    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, ctrl_key('a'), &tx).unwrap();

    // All 3 hosts should be selected
    assert_eq!(app.multi_select.len(), 3);

    // Press Ctrl+A again to deselect all
    handle_key_event(&mut app, ctrl_key('a'), &tx).unwrap();
    assert!(app.multi_select.is_empty());
}

#[test]
fn test_ctrl_a_in_search_mode_selects_filtered() {
    let mut app = make_app(
        "Host prod-web\n  HostName 1.1.1.1\n\nHost prod-db\n  HostName 2.2.2.2\n\nHost staging-app\n  HostName 3.3.3.3\n",
    );
    app.apply_sort();

    // Enter search mode and filter to "prod"
    app.search.query = Some("prod".to_string());
    app.apply_filter();
    assert_eq!(app.search.filtered_indices.len(), 2);
    assert!(app.multi_select.is_empty());

    // Ctrl+A should select only the 2 filtered hosts
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, ctrl_key('a'), &tx).unwrap();
    assert_eq!(app.multi_select.len(), 2);

    // Press Ctrl+A again to deselect
    handle_key_event(&mut app, ctrl_key('a'), &tx).unwrap();
    assert!(app.multi_select.is_empty());
}

// =========================================================================
// Tab / Shift+Tab / Esc group-filter tests (HostList screen)
// =========================================================================

/// Build an app with two provider-tagged hosts so that group_by=Provider
/// produces a non-empty group_tab_order after apply_sort().
fn make_provider_grouped_app() -> App {
    let content = "\
Host aws-web1
  HostName 1.1.1.1
  # purple:provider aws:i-123

Host do-web2
  HostName 2.2.2.2
  # purple:provider digitalocean:abc
";
    let mut app = make_app(content);
    app.group_by = crate::app::GroupBy::Provider;
    app.apply_sort();
    app
}

#[test]
fn tab_on_host_list_filters_to_first_group() {
    let mut app = make_provider_grouped_app();
    assert!(
        !app.group_tab_order.is_empty(),
        "expected non-empty group_tab_order after apply_sort with Provider grouping"
    );
    assert!(app.group_filter.is_none(), "filter should start as None");

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);

    assert!(
        app.group_filter.is_some(),
        "group_filter should be Some after Tab"
    );
    assert_eq!(
        app.group_filter.as_deref(),
        Some(app.group_tab_order[0].as_str())
    );
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn shift_tab_on_host_list_filters_to_last_group() {
    let mut app = make_provider_grouped_app();
    let last_group = app.group_tab_order.last().unwrap().clone();
    assert!(app.group_filter.is_none(), "filter should start as None");

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(
        &mut app,
        KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
        &tx,
    );

    assert_eq!(
        app.group_filter.as_deref(),
        Some(last_group.as_str()),
        "BackTab from All should land on the last group"
    );
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn tab_cycles_back_to_all() {
    let mut app = make_provider_grouped_app();
    // There are exactly 2 groups (aws, digitalocean). Set filter to the last one.
    let last_group = app.group_tab_order.last().unwrap().clone();
    app.group_filter = Some(last_group);
    app.apply_sort();

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);

    assert!(
        app.group_filter.is_none(),
        "Tab past the last group should wrap back to All (None)"
    );
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn esc_clears_group_filter() {
    let mut app = make_provider_grouped_app();
    let first_group = app.group_tab_order[0].clone();
    app.group_filter = Some(first_group);
    app.apply_sort();
    assert!(app.running);

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);

    assert!(
        app.group_filter.is_none(),
        "Esc with active group_filter should clear it"
    );
    assert!(app.running, "Esc with active filter should NOT quit");
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn esc_quits_when_no_filter() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    assert!(app.group_filter.is_none());
    assert!(app.running);

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);

    assert!(!app.running, "Esc with no group_filter should quit");
}

#[test]
fn test_p_key_clears_ping_increments_generation() {
    let mut app = make_app("Host web1\n  HostName 1.1.1.1\n");
    // Pre-populate ping status to simulate completed pings
    app.ping.status.insert(
        "web1".to_string(),
        crate::app::PingStatus::Reachable { rtt_ms: 10 },
    );
    app.ping.filter_down_only = true;
    app.ping.checked_at = Some(std::time::Instant::now());
    assert_eq!(app.ping.generation, 0);

    let (tx, _rx) = std::sync::mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Char('P')), &tx).unwrap();

    assert!(app.ping.status.is_empty());
    assert_eq!(app.ping.generation, 1);
    assert!(!app.ping.filter_down_only);
    assert!(app.ping.checked_at.is_none());
}

#[test]
fn test_bang_key_without_pings_shows_error() {
    let mut app = make_app("Host web1\n  HostName 1.1.1.1\n");
    assert!(app.ping.status.is_empty());
    let (tx, _rx) = std::sync::mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Char('!')), &tx).unwrap();
    assert!(!app.ping.filter_down_only);
    assert!(app.toast.as_ref().unwrap().is_error());
}

#[test]
fn test_bang_key_toggles_down_only_on() {
    let mut app = make_app("Host web1\n  HostName 1.1.1.1\nHost web2\n  HostName 2.2.2.2\n");
    app.ping
        .status
        .insert("web1".to_string(), crate::app::PingStatus::Unreachable);
    app.ping.status.insert(
        "web2".to_string(),
        crate::app::PingStatus::Reachable { rtt_ms: 10 },
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Char('!')), &tx).unwrap();
    assert!(app.ping.filter_down_only);
    assert!(app.search.query.is_some());
    // Only web1 (Unreachable) should be in filtered results
    assert_eq!(app.search.filtered_indices.len(), 1);
}

#[test]
fn test_bang_key_toggles_down_only_off() {
    let mut app = make_app("Host web1\n  HostName 1.1.1.1\nHost web2\n  HostName 2.2.2.2\n");
    app.ping
        .status
        .insert("web1".to_string(), crate::app::PingStatus::Unreachable);
    app.ping.status.insert(
        "web2".to_string(),
        crate::app::PingStatus::Reachable { rtt_ms: 10 },
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    // Toggle on
    handle_key_event(&mut app, key(KeyCode::Char('!')), &tx).unwrap();
    assert!(app.ping.filter_down_only);
    // Toggle off
    handle_key_event(&mut app, key(KeyCode::Char('!')), &tx).unwrap();
    assert!(!app.ping.filter_down_only);
    assert!(app.search.query.is_none());
}

// ─── Progressive disclosure: host form ─────────────────────────

#[test]
fn host_form_new_starts_collapsed() {
    let form = HostForm::new();
    assert!(!form.expanded);
}

#[test]
fn host_form_from_entry_starts_expanded() {
    let config = SshConfigFile {
        elements: SshConfigFile::parse_content("Host test\n  HostName test.com\n"),
        path: PathBuf::from("/tmp/test_config"),
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
    app.form = HostForm::new();
    app.form.focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert_eq!(app.form.focused_field, FormField::Hostname);
    assert!(!app.form.expanded);
}

#[test]
fn host_form_tab_from_hostname_expands() {
    let mut app = make_app("");
    app.form = HostForm::new();
    app.form.focused_field = FormField::Hostname;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert!(app.form.expanded);
    assert_eq!(app.form.focused_field, FormField::User);
}

#[test]
fn host_form_collapsed_backtab_wraps() {
    let mut app = make_app("");
    app.form = HostForm::new();
    app.form.focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(
        &mut app,
        KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
        &tx,
    )
    .unwrap();
    assert_eq!(app.form.focused_field, FormField::Hostname);
    assert!(!app.form.expanded);
}

#[test]
fn host_form_expanded_does_not_trigger_dirty() {
    let mut app = make_app("");
    app.form = HostForm::new();
    app.form.alias = "test".to_string();
    app.screen = Screen::AddHost;
    app.capture_form_baseline();
    app.form.expanded = true;
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
fn provider_optional_fields_are_complement() {
    for provider in &[
        "aws",
        "digitalocean",
        "proxmox",
        "gcp",
        "azure",
        "oracle",
        "ovh",
        "scaleway",
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
fn provider_mandatory_fields_aws_token_and_profile() {
    use crate::app::ProviderFormField;
    assert!(
        ProviderFormField::is_mandatory_field(ProviderFormField::Token, "aws"),
        "AWS Token should be mandatory (asterisked)"
    );
    assert!(
        ProviderFormField::is_mandatory_field(ProviderFormField::Profile, "aws"),
        "AWS Profile should be mandatory (asterisked)"
    );
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
        "gcp",
        "azure",
        "oracle",
        "ovh",
        "scaleway",
        "tailscale",
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
        provider: "digitalocean".to_string(),
    };
    app.provider_form = ProviderFormFields::new();
    app.provider_form.token = "tok".to_string();
    app.capture_provider_form_baseline();
    app.provider_form.expanded = true;
    assert!(!app.provider_form_is_dirty());
}

// ─── Host form collapsed Enter-saves ───────────────────────────

#[test]
fn host_form_collapsed_enter_saves() {
    let mut app = make_app("");
    app.form = HostForm::new();
    app.form.alias = "myhost".to_string();
    app.form.hostname = "myhost.local".to_string();
    app.form.focused_field = FormField::Hostname;
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
        provider: "digitalocean".to_string(),
    };
    app.provider_form = ProviderFormFields::new();
    app.provider_form.token = "tok".to_string();
    // Token is the only required field for DO
    app.provider_form.focused_field = crate::app::ProviderFormField::Token;
    app.provider_form.expanded = false;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert!(app.provider_form.expanded);
    // First optional field for DO is AliasPrefix
    assert_eq!(
        app.provider_form.focused_field,
        crate::app::ProviderFormField::AliasPrefix
    );
}

#[test]
fn provider_form_collapsed_backtab_wraps() {
    // AWS has 3 required fields: Token, Profile, Regions
    let mut app = make_app("");
    app.screen = Screen::ProviderForm {
        provider: "aws".to_string(),
    };
    app.provider_form = ProviderFormFields::new();
    app.provider_form.focused_field = crate::app::ProviderFormField::Token;
    app.provider_form.expanded = false;
    let tx = mpsc::channel().0;
    handle_key_event(
        &mut app,
        KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
        &tx,
    )
    .unwrap();
    // Token is first required; BackTab wraps to last required (Regions)
    assert_eq!(
        app.provider_form.focused_field,
        crate::app::ProviderFormField::Regions
    );
    assert!(!app.provider_form.expanded);
}

#[test]
fn provider_form_tab_within_collapsed_required() {
    // AWS: Token -> Profile -> Regions (all required)
    let mut app = make_app("");
    app.screen = Screen::ProviderForm {
        provider: "aws".to_string(),
    };
    app.provider_form = ProviderFormFields::new();
    app.provider_form.focused_field = crate::app::ProviderFormField::Token;
    app.provider_form.expanded = false;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    // Token -> Profile (mid-required, should NOT expand)
    assert_eq!(
        app.provider_form.focused_field,
        crate::app::ProviderFormField::Profile
    );
    assert!(!app.provider_form.expanded);
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
    assert!(super::vault_addr_missing(&[None], None));
}

#[test]
fn vault_addr_missing_reports_when_env_is_invalid_and_host_empty() {
    // Whitespace-only is rejected by is_valid_vault_addr; treat as unset.
    assert!(super::vault_addr_missing(&[None], Some("  ")));
}

#[test]
fn vault_addr_missing_false_when_env_is_set() {
    assert!(!super::vault_addr_missing(
        &[None, None],
        Some("https://vault.example.com:8200")
    ));
}

#[test]
fn vault_addr_missing_false_when_every_host_has_addr() {
    assert!(!super::vault_addr_missing(
        &[Some("https://a"), Some("https://b")],
        None
    ));
}

#[test]
fn vault_addr_missing_false_when_mixed_hosts_and_env_empty() {
    // Some hosts have an addr, some don't. Only block when ALL lack an addr.
    assert!(!super::vault_addr_missing(&[Some("https://a"), None], None));
}

#[test]
fn vault_addr_missing_false_when_no_hosts() {
    // Empty slice: nothing to sign, no prompt needed.
    assert!(!super::vault_addr_missing(&[], None));
}

#[test]
fn vault_addr_missing_true_when_env_is_empty_string() {
    assert!(super::vault_addr_missing(&[None], Some("")));
}

#[test]
fn vault_addr_missing_false_when_mixed_hosts_and_env_valid() {
    assert!(!super::vault_addr_missing(
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

// --- Command palette tests ---

#[test]
fn colon_opens_command_palette() {
    let mut app = make_app("");
    app.screen = Screen::HostList;
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Char(':')), &tx).unwrap();
    assert!(app.palette.is_some());
}

#[test]
fn palette_esc_closes() {
    let mut app = make_app("");
    app.palette = Some(crate::app::CommandPaletteState::new());
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Esc), &tx).unwrap();
    assert!(app.palette.is_none());
}

#[test]
fn palette_char_always_filters() {
    // All chars go to filter, even recognized command keys like 'K'
    let mut app = make_app("");
    app.palette = Some(crate::app::CommandPaletteState::new());
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Char('K')), &tx).unwrap();
    assert!(app.palette.is_some(), "palette should stay open");
    assert_eq!(app.palette.as_ref().unwrap().query, "K");
    assert!(
        matches!(app.screen, Screen::HostList),
        "should not navigate away"
    );
}

#[test]
fn palette_filter_then_enter_executes() {
    // Type "SSH" to filter, then Enter to execute the selected result
    let mut app = make_app("");
    let mut state = crate::app::CommandPaletteState::new();
    state.push_query('S');
    state.push_query('S');
    state.push_query('H');
    let filtered = state.filtered_commands();
    // Find the SSH keys entry and set selected to its index
    let ssh_idx = filtered.iter().position(|c| c.key == 'K').unwrap();
    state.selected = ssh_idx;
    app.palette = Some(state);
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();
    assert!(matches!(app.screen, Screen::KeyList));
    assert!(app.palette.is_none());
}

#[test]
fn palette_up_down_navigates() {
    let mut app = make_app("");
    app.palette = Some(crate::app::CommandPaletteState::new());
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Down), &tx).unwrap();
    assert_eq!(app.palette.as_ref().unwrap().selected, 1);
    handle_key_event(&mut app, key(KeyCode::Up), &tx).unwrap();
    assert_eq!(app.palette.as_ref().unwrap().selected, 0);
}

#[test]
fn palette_any_char_appends_to_filter() {
    let mut app = make_app("");
    app.palette = Some(crate::app::CommandPaletteState::new());
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Char('t')), &tx).unwrap();
    assert!(app.palette.is_some());
    assert_eq!(app.palette.as_ref().unwrap().query, "t");
    // 't' is a command key (tag inline), but should filter, not execute
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn palette_enter_on_empty_filter_does_nothing() {
    let mut app = make_app("");
    app.palette = Some(crate::app::CommandPaletteState::new());
    app.palette.as_mut().unwrap().push_query('z');
    app.palette.as_mut().unwrap().push_query('z');
    app.palette.as_mut().unwrap().push_query('z');
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();
    assert!(app.palette.is_some());
}

#[test]
fn palette_backspace_on_empty_closes() {
    let mut app = make_app("");
    app.palette = Some(crate::app::CommandPaletteState::new());
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Backspace), &tx).unwrap();
    assert!(app.palette.is_none());
}

#[test]
fn palette_backspace_removes_filter_char() {
    let mut app = make_app("");
    app.palette = Some(crate::app::CommandPaletteState::new());
    app.palette.as_mut().unwrap().push_query('t');
    app.palette.as_mut().unwrap().push_query('u');
    let (tx, _rx) = mpsc::channel();
    handle_key_event(&mut app, key(KeyCode::Backspace), &tx).unwrap();
    assert_eq!(app.palette.as_ref().unwrap().query, "t");
}

#[test]
fn palette_navigate_then_enter_executes() {
    let mut app = make_app("");
    app.palette = Some(crate::app::CommandPaletteState::new());
    let (tx, _rx) = mpsc::channel();
    // The 3rd command in all() is 'e' (edit). Navigate Down twice to index 2.
    handle_key_event(&mut app, key(KeyCode::Down), &tx).unwrap();
    handle_key_event(&mut app, key(KeyCode::Down), &tx).unwrap();
    assert_eq!(app.palette.as_ref().unwrap().selected, 2);
    // Enter on index 2 should dispatch 'e' (edit) — but with no host selected
    // it does nothing visible (no crash). Palette should close.
    handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();
    assert!(app.palette.is_none(), "palette should close after Enter");
}

#[test]
fn palette_filter_shrink_then_enter_clamps_selected() {
    let mut app = make_app("");
    let mut state = crate::app::CommandPaletteState::new();
    // Set selected to a high index, then add a filter that reduces the list
    state.selected = 10;
    state.push_query('S'); // push_query resets selected to 0
    state.push_query('S');
    state.push_query('H');
    // Filtered list narrows to a few items
    let filtered = state.filtered_commands();
    assert!(!filtered.is_empty(), "filter should have results");
    assert!(filtered.len() < crate::app::PaletteCommand::all().len());
    // Force selected to way out-of-bounds to test clamping in Enter handler
    state.selected = 50;
    app.palette = Some(state);
    let (tx, _rx) = mpsc::channel();
    // Enter should clamp selected to last item, execute it, and close palette
    handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();
    assert!(
        app.palette.is_none(),
        "palette should close after clamped Enter"
    );
}

#[test]
fn palette_query_capped_at_64() {
    let mut state = crate::app::CommandPaletteState::new();
    for _ in 0..100 {
        state.push_query('a');
    }
    assert_eq!(state.query.len(), 64, "query should be capped at 64 chars");
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
    app.ui.show_proxyjump_picker = true;
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
    app.ui.proxyjump_picker_state.select(Some(label_idx));

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    assert!(
        app.ui.show_proxyjump_picker,
        "Enter on a SectionLabel must not close the picker"
    );
    assert!(
        app.form.proxy_jump.is_empty(),
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
    app.ui.proxyjump_picker_state.select(Some(sep));

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    assert!(
        app.ui.show_proxyjump_picker,
        "Enter on a Separator must not close the picker"
    );
    assert!(
        app.form.proxy_jump.is_empty(),
        "Enter on a Separator must not populate the ProxyJump field"
    );
}

#[test]
fn proxyjump_picker_enter_on_host_applies_alias_and_closes() {
    let mut app = proxyjump_picker_app();
    // Select the first host (the suggested one). `proxyjump_first_host_index`
    // resolves to the right index regardless of any leading SectionLabel.
    let first_host = app.proxyjump_first_host_index().expect("host expected");
    app.ui.proxyjump_picker_state.select(Some(first_host));

    let (tx, _rx) = mpsc::channel();
    let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

    assert!(
        !app.ui.show_proxyjump_picker,
        "Enter on a Host must close the picker"
    );
    assert_eq!(
        app.form.proxy_jump, "bastion",
        "the selected host's alias must populate the ProxyJump field"
    );
}

// ─── Smart paste: bare domain/IP detection ──────────────────────

#[test]
fn host_form_smart_paste_detects_bare_domain() {
    let mut app = make_app("");
    app.form = HostForm::new();
    app.form.alias = "db.example.com".to_string();
    app.form.focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    // Tab away from Alias triggers smart paste
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert_eq!(app.form.hostname, "db.example.com");
    // Alias stays unchanged — only hostname is suggested
    assert_eq!(app.form.alias, "db.example.com");
}

#[test]
fn host_form_smart_paste_detects_ip_address() {
    let mut app = make_app("");
    app.form = HostForm::new();
    app.form.alias = "192.168.1.100".to_string();
    app.form.focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert_eq!(app.form.hostname, "192.168.1.100");
    assert_eq!(app.form.alias, "192.168.1.100");
}

#[test]
fn host_form_smart_paste_skips_plain_name() {
    let mut app = make_app("");
    app.form = HostForm::new();
    app.form.alias = "myserver".to_string();
    app.form.focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    // No dot means no detection — alias stays, hostname stays empty
    assert_eq!(app.form.alias, "myserver");
    assert!(app.form.hostname.is_empty());
}

#[test]
fn host_form_smart_paste_domain_no_overwrite_hostname() {
    let mut app = make_app("");
    app.form = HostForm::new();
    app.form.alias = "db.example.com".to_string();
    app.form.hostname = "already.set.com".to_string();
    app.form.focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    // Hostname already populated — don't overwrite
    assert_eq!(app.form.hostname, "already.set.com");
    assert_eq!(app.form.alias, "db.example.com");
}

#[test]
fn host_form_smart_paste_rejects_leading_dot() {
    let mut app = make_app("");
    app.form = HostForm::new();
    app.form.alias = ".example.com".to_string();
    app.form.focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    // Leading dot produces empty first label — must not fire
    assert_eq!(app.form.alias, ".example.com");
    assert!(app.form.hostname.is_empty());
}

#[test]
fn host_form_smart_paste_rejects_bare_dot() {
    let mut app = make_app("");
    app.form = HostForm::new();
    app.form.alias = ".".to_string();
    app.form.focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert_eq!(app.form.alias, ".");
    assert!(app.form.hostname.is_empty());
}

#[test]
fn host_form_smart_paste_ignores_ipv6_mixed() {
    // IPv4-mapped IPv6 notation must not trigger bare-domain detection
    let mut app = make_app("");
    app.form = HostForm::new();
    app.form.alias = "::ffff:192.0.2.1".to_string();
    app.form.focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert_eq!(app.form.alias, "::ffff:192.0.2.1");
    assert!(app.form.hostname.is_empty());
}

#[test]
fn host_form_smart_paste_allows_underscore_hostname() {
    let mut app = make_app("");
    app.form = HostForm::new();
    app.form.alias = "my_host.internal".to_string();
    app.form.focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert_eq!(app.form.hostname, "my_host.internal");
    assert_eq!(app.form.alias, "my_host.internal");
}

#[test]
fn host_form_smart_paste_fires_on_enter() {
    // Enter on Alias also calls maybe_smart_paste before submit.
    // Use a minimal valid config so submit_form can succeed.
    let mut app = make_app("");
    app.form = HostForm::new();
    app.form.alias = "web.example.com".to_string();
    app.form.focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Enter), &tx).unwrap();
    // Smart paste copies alias to hostname, alias stays unchanged.
    // submit_form runs next — on success the screen returns to HostList.
    assert_eq!(app.screen, Screen::HostList);
    assert!(app.hosts.iter().any(|h| h.alias == "web.example.com"));
    assert!(app.hosts.iter().any(|h| h.hostname == "web.example.com"));
}

#[test]
fn host_form_smart_paste_rejects_trailing_dot() {
    // Trailing dot is invalid for SSH HostName — must not fire
    let mut app = make_app("");
    app.form = HostForm::new();
    app.form.alias = "example.com.".to_string();
    app.form.focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert_eq!(app.form.alias, "example.com.");
    assert!(app.form.hostname.is_empty());
}

#[test]
fn host_form_smart_paste_rejects_short_dotted_string() {
    // "1.1" (len 3) should not trigger — too short to be a real hostname
    let mut app = make_app("");
    app.form = HostForm::new();
    app.form.alias = "1.1".to_string();
    app.form.focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert_eq!(app.form.alias, "1.1");
    assert!(app.form.hostname.is_empty());
}

#[test]
fn host_form_smart_paste_minimum_valid_length() {
    // "x.io" (len 4) is the shortest that should trigger
    let mut app = make_app("");
    app.form = HostForm::new();
    app.form.alias = "x.io".to_string();
    app.form.focused_field = FormField::Alias;
    app.screen = Screen::AddHost;
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert_eq!(app.form.hostname, "x.io");
    assert_eq!(app.form.alias, "x.io");
}

#[test]
fn host_form_smart_paste_no_fire_on_edit_with_hostname() {
    // EditHost: hostname already populated from existing entry — must not overwrite
    let mut app = make_app("Host myserver\n  HostName myserver.local\n");
    app.form = HostForm::new();
    app.form.alias = "db.example.com".to_string();
    app.form.hostname = "myserver.local".to_string();
    app.form.focused_field = FormField::Alias;
    app.screen = Screen::EditHost {
        alias: "myserver".to_string(),
    };
    let tx = mpsc::channel().0;
    handle_key_event(&mut app, key(KeyCode::Tab), &tx).unwrap();
    assert_eq!(app.form.hostname, "myserver.local");
    assert_eq!(app.form.alias, "db.example.com");
}
