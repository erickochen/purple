//! Visual regression tests for every screen.
//!
//! Each test renders one screen into a `TestBackend` buffer using demo data,
//! serialises the buffer (characters plus per-cell style info) and compares
//! the result against a `.golden` baseline in `tests/visual_golden/`. Any
//! visual change to spacing, colors, text or borders fails the test.
//!
//! Regenerate baselines after intentional UI changes:
//!     ./scripts/update-golden.sh
//!
//! Implementation notes:
//! - Tests live in the binary crate (not in `tests/`) because they need
//!   access to private types (`App`, `ui::render`, `animation::AnimationState`).
//! - All tests pin the color mode to ANSI 16 (`init_with_mode(1)`) so output
//!   is deterministic across terminals (no truecolor RGB drift, no NO_COLOR
//!   stripping).
//! - Tests use a process-wide lock to serialise demo state mutations and
//!   theme initialisation across `cargo test` worker threads.

use std::path::PathBuf;
use std::sync::MutexGuard;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

use crate::animation::AnimationState;
use crate::app::{App, Screen};
use crate::demo;
use crate::demo_flag;
use crate::preferences;
use crate::ui;

const TERM_WIDTH: u16 = 100;
const TERM_HEIGHT: u16 = 30;

/// RAII guard returned by `setup()`. Holds the cross-suite lock for the
/// duration of the test and resets the demo flag on drop so subsequent
/// non-visual tests do not observe a sticky `demo_flag::is_demo() == true`.
struct VisualGuard {
    _lock: MutexGuard<'static, ()>,
}

impl Drop for VisualGuard {
    fn drop(&mut self) {
        demo_flag::disable();
    }
}

/// Acquire the cross-suite test lock, pin ANSI 16 colors and reset the
/// theme, and return a guard that releases the lock and resets the demo
/// flag on drop.
///
/// The lock is shared with `preferences::tests_helpers::with_temp_prefs`
/// because both suites mutate process-wide state (`demo_flag::DEMO_MODE`)
/// that would otherwise race when `cargo test` runs them concurrently.
///
/// Env-sensitivity audit: visual tests must be byte-identical on any host.
/// The consumed state is:
/// - `ui::theme` — pinned via `init_with_mode(1)`, ignores NO_COLOR/COLORTERM
/// - `preferences` — each test's `App` auto-sandboxes its `~/.purple` to a
///   fresh empty tempdir, so load_* returns None regardless of the host
/// - `CHANGELOG.md` — embedded via `include_str!` at compile time
/// - `CARGO_PKG_VERSION` / `PURPLE_BUILD_DATE` — compile-time env vars
///   (build date drifts by calendar day; accepted as known limitation)
#[must_use]
fn setup() -> VisualGuard {
    let lock = preferences::GLOBAL_TEST_IO_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    ui::theme::init_with_mode(1);
    // Reset the theme too. `init_with_mode` only sets the color mode; the
    // theme itself is `get_or_init` and never reset, so a non-Purple theme
    // left behind by another test would otherwise bleed into goldens.
    ui::theme::set_theme(ui::theme::ThemeDef::purple());
    VisualGuard { _lock: lock }
}

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/visual_golden")
}

fn golden_path(name: &str) -> PathBuf {
    golden_dir().join(format!("{name}.golden"))
}

fn color_name(c: Color) -> String {
    match c {
        Color::Reset => "Reset".into(),
        Color::Black => "Black".into(),
        Color::Red => "Red".into(),
        Color::Green => "Green".into(),
        Color::Yellow => "Yellow".into(),
        Color::Blue => "Blue".into(),
        Color::Magenta => "Magenta".into(),
        Color::Cyan => "Cyan".into(),
        Color::Gray => "Gray".into(),
        Color::DarkGray => "DarkGray".into(),
        Color::LightRed => "LightRed".into(),
        Color::LightGreen => "LightGreen".into(),
        Color::LightYellow => "LightYellow".into(),
        Color::LightBlue => "LightBlue".into(),
        Color::LightMagenta => "LightMagenta".into(),
        Color::LightCyan => "LightCyan".into(),
        Color::White => "White".into(),
        Color::Rgb(r, g, b) => format!("Rgb({r},{g},{b})"),
        Color::Indexed(i) => format!("Indexed({i})"),
    }
}

fn modifier_name(m: Modifier) -> String {
    if m.is_empty() {
        return "-".into();
    }
    let mut parts = Vec::new();
    if m.contains(Modifier::BOLD) {
        parts.push("BOLD");
    }
    if m.contains(Modifier::DIM) {
        parts.push("DIM");
    }
    if m.contains(Modifier::ITALIC) {
        parts.push("ITALIC");
    }
    if m.contains(Modifier::UNDERLINED) {
        parts.push("UNDERLINED");
    }
    if m.contains(Modifier::SLOW_BLINK) {
        parts.push("SLOW_BLINK");
    }
    if m.contains(Modifier::RAPID_BLINK) {
        parts.push("RAPID_BLINK");
    }
    if m.contains(Modifier::REVERSED) {
        parts.push("REVERSED");
    }
    if m.contains(Modifier::HIDDEN) {
        parts.push("HIDDEN");
    }
    if m.contains(Modifier::CROSSED_OUT) {
        parts.push("CROSSED_OUT");
    }
    parts.join("|")
}

/// Serialise a buffer to a deterministic string: a character grid followed by
/// a `---STYLES---` marker and one line per non-default cell with its style.
fn serialize_buffer(buf: &Buffer) -> String {
    let mut out = String::new();
    let area = buf.area;
    for y in 0..area.height {
        for x in 0..area.width {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out.push_str("---STYLES---\n");
    for y in 0..area.height {
        for x in 0..area.width {
            let cell = &buf[(x, y)];
            let is_default_fg = matches!(cell.fg, Color::Reset);
            let is_default_bg = matches!(cell.bg, Color::Reset);
            let is_default_mod = cell.modifier.is_empty();
            if is_default_fg && is_default_bg && is_default_mod {
                continue;
            }
            out.push_str(&format!(
                "({x},{y}) fg={} bg={} mod={}\n",
                color_name(cell.fg),
                color_name(cell.bg),
                modifier_name(cell.modifier),
            ));
        }
    }
    out
}

/// Compare actual output to the golden file. When `UPDATE_GOLDEN=1` is set,
/// overwrite the golden file instead of asserting.
fn assert_golden(name: &str, actual: &str) {
    let path = golden_path(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(golden_dir()).expect("create golden dir");
        std::fs::write(&path, actual).expect("write golden");
        return;
    }

    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "failed to read golden {}: {e}. Run UPDATE_GOLDEN=1 cargo test --bin purple visual_regression to create it.",
            path.display()
        )
    });

    if expected != actual {
        // Write the actual output next to the golden so the diff is easy to inspect.
        let actual_path = path.with_extension("actual");
        let _ = std::fs::write(&actual_path, actual);
        panic!(
            "visual regression: {name} differs from baseline.\n  golden: {}\n  actual: {}\nIf the change is intentional, run ./scripts/update-golden.sh and review the diff.",
            path.display(),
            actual_path.display(),
        );
    }
}

/// Render the given screen into a buffer and return the serialised result.
fn render_screen(app: &mut App) -> String {
    let backend = TestBackend::new(TERM_WIDTH, TERM_HEIGHT);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    let mut anim = AnimationState::default();
    terminal
        .draw(|frame| ui::render(frame, app, &mut anim))
        .expect("render frame");
    let buf = terminal.backend().buffer().clone();
    serialize_buffer(&buf)
}

// ---------------------------------------------------------------------------
// Tests. Each test pins ANSI-16 colors, builds a fresh demo app,
// switches to the target screen, renders it and compares against a golden.
// ---------------------------------------------------------------------------

#[test]
fn visual_host_list() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    let actual = render_screen(&mut app);
    assert_golden("host_list", &actual);
}

#[test]
fn visual_host_list_search() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.start_search_with("aws");
    let actual = render_screen(&mut app);
    assert_golden("host_list_search", &actual);
}

#[test]
fn visual_host_list_detail_panel() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    // Detail panel renders alongside the host list when view_mode is Detailed
    // and the terminal is wide enough (DETAIL_MIN_WIDTH).
    app.hosts_state
        .set_view_mode(crate::app::ViewMode::Detailed);
    let actual = render_screen(&mut app);
    assert_golden("host_list_detail_panel", &actual);
}

#[test]
fn visual_host_form_add() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::AddHost;
    let actual = render_screen(&mut app);
    assert_golden("host_form_add", &actual);
}

#[test]
fn visual_host_form_edit() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::EditHost {
        alias: "bastion-ams".to_string(),
    };
    let actual = render_screen(&mut app);
    assert_golden("host_form_edit", &actual);
}

#[test]
fn visual_host_detail() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::HostDetail { index: 0 };
    let actual = render_screen(&mut app);
    assert_golden("host_detail", &actual);
}

#[test]
fn visual_tunnel_list() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::TunnelList {
        alias: "bastion-ams".to_string(),
    };
    let actual = render_screen(&mut app);
    assert_golden("tunnel_list", &actual);
}

#[test]
fn visual_tunnels_overview() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Tunnels;
    let actual = render_screen(&mut app);
    assert_golden("tunnels_overview", &actual);
}

#[test]
fn visual_tunnels_overview_active() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Tunnels;
    // Seed a deterministic live snapshot for the host the cursor lands
    // on after `build_demo_app` (sorted alphabetically the first row is
    // bastion-ams). The detail panel reads from `demo_live_snapshots`
    // so the LIVE/CLIENTS/EVENTS cards render byte-stably.
    demo::seed_tunnel_live_snapshots(&mut app);
    app.ui.tunnels_overview_state_mut().select(Some(0));
    let actual = render_screen(&mut app);
    assert_golden("tunnels_overview_active", &actual);
}

#[test]
fn visual_tunnels_overview_active_tall() {
    // Verifies the adaptive layout: on a tall terminal the detail
    // panel grows the sparkline (up to 6 rows) and surfaces more
    // CLIENTS / EVENTS rows instead of leaving empty padding at the
    // bottom of the panel.
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Tunnels;
    demo::seed_tunnel_live_snapshots(&mut app);
    app.ui.tunnels_overview_state_mut().select(Some(0));
    let backend = TestBackend::new(120, 50);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    let mut anim = AnimationState::default();
    terminal
        .draw(|frame| ui::render(frame, &mut app, &mut anim))
        .expect("render frame");
    let buf = terminal.backend().buffer().clone();
    let actual = serialize_buffer(&buf);
    assert_golden("tunnels_overview_active_tall", &actual);
}

#[test]
fn visual_tunnel_form() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::TunnelForm {
        alias: "bastion-ams".to_string(),
        editing: None,
    };
    let actual = render_screen(&mut app);
    assert_golden("tunnel_form", &actual);
}

#[test]
fn visual_tunnel_host_picker() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Tunnels;
    app.screen = Screen::TunnelHostPicker;
    app.ui.tunnel_host_picker_state_mut().select(Some(0));
    let actual = render_screen(&mut app);
    assert_golden("tunnel_host_picker", &actual);
}

#[test]
fn visual_tunnel_host_picker_filtered() {
    // Picker with an active fuzzy query — title should switch to the
    // "X of Y" form and the list should shrink to matching hosts.
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Tunnels;
    app.screen = Screen::TunnelHostPicker;
    app.ui.set_tunnel_host_picker_query("db".to_string());
    app.ui.tunnel_host_picker_state_mut().select(Some(0));
    let actual = render_screen(&mut app);
    assert_golden("tunnel_host_picker_filtered", &actual);
}

#[test]
fn visual_container_host_picker() {
    // Picker shown after pressing `a` on the Containers tab. Mirrors the
    // tunnel-host-picker geometry but draws over the containers base
    // page instead of the tunnels base page.
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Containers;
    app.screen = Screen::ContainerHostPicker;
    app.ui.container_host_picker_state_mut().select(Some(0));
    let actual = render_screen(&mut app);
    assert_golden("container_host_picker", &actual);
}

/// Keys tab default layout: master list + Vault SSH TTL strip on top.
/// The demo config has at least one host with a `purple:vault-ssh` role
/// and a populated `vault.cert_cache` so the strip renders with several
/// gauge rows in distinct color tiers.
#[test]
fn visual_keys_overview() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Keys;
    app.keys.list_state_mut().select(Some(0));
    let actual = render_screen(&mut app);
    assert_golden("keys_overview", &actual);
}

/// Keys tab with the cursor on a hardware-bound `sk-ed25519` key. The
/// detail pane exercises the hardware-token Drunken Bishop variant and
/// the `sk-ed` algorithm badge styling.
#[test]
fn visual_keys_overview_hardware_key() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Keys;
    // Demo key #1 is yubikey_work (sk-ED25519).
    app.keys.list_state_mut().select(Some(1));
    let actual = render_screen(&mut app);
    assert_golden("keys_overview_hardware", &actual);
}

/// Keys tab without any configured Vault SSH role. The strip stays hidden
/// and the master pane takes the full vertical space minus the top bar
/// and footer. Built by clearing the demo host list's `vault_ssh` field.
#[test]
fn visual_keys_overview_no_vault() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Keys;
    app.keys.list_state_mut().select(Some(0));
    for host in app.hosts_state.list_mut() {
        host.vault_ssh = None;
    }
    app.vault.clear_cert_cache();
    let actual = render_screen(&mut app);
    assert_golden("keys_overview_no_vault", &actual);
}

/// Keys tab on a wide + tall terminal (200x40). At this size the hero
/// renders three side-by-side cards (Keys list / Randomart / Info) and
/// the Linked Hosts grid below. Pins the layout under visual regression
/// so a future refactor cannot silently regress it.
#[test]
fn visual_keys_overview_two_cards() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Keys;
    app.keys.list_state_mut().select(Some(0));
    let backend = TestBackend::new(200, 40);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    let mut anim = AnimationState::default();
    terminal
        .draw(|frame| ui::render(frame, &mut app, &mut anim))
        .expect("render frame");
    let buf = terminal.backend().buffer().clone();
    assert_golden("keys_overview_two_cards", &serialize_buffer(&buf));
}

/// Keys tab on a wide terminal with 31 synthetic linked hosts so the
/// Linked Hosts card distributes across three balanced columns and the
/// Drunken Bishop card renders at its large 25×13 size. Uses a real
/// 32-byte SHA256 fingerprint so the bishop walk fills the larger grid
/// with the same density as the canonical 17×9 OpenSSH art.
#[test]
fn visual_keys_overview_many_linked_hosts() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Keys;
    app.keys.list_state_mut().select(Some(0));
    app.keys.list_mut()[0].fingerprint =
        "SHA256:1LayGj+CVIvJfOnQqADAT52DoJHhSa30feF/23wbRuE".to_string();
    let synthetic: Vec<String> = (1..=31).map(|i| format!("host-{:02}", i)).collect();
    app.keys.list_mut()[0].linked_hosts = synthetic.clone();
    for alias in &synthetic {
        app.hosts_state
            .list_mut()
            .push(crate::ssh_config::model::HostEntry {
                alias: alias.clone(),
                hostname: format!("10.0.{}.{}", alias.len(), alias.len() * 3 % 250),
                ..Default::default()
            });
    }
    let backend = TestBackend::new(200, 40);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    let mut anim = AnimationState::default();
    terminal
        .draw(|frame| ui::render(frame, &mut app, &mut anim))
        .expect("render frame");
    let buf = terminal.backend().buffer().clone();
    assert_golden("keys_overview_many_linked_hosts", &serialize_buffer(&buf));
}

/// Keys tab on a narrow terminal (80 cols, below `HERO_MIN_WIDTH = 60`
/// after the Keys list eats its share). The hero falls back to a single
/// stacked card. Confirms the responsive collapse path renders without
/// the side-by-side info card.
#[test]
fn visual_keys_overview_narrow() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Keys;
    app.keys.list_state_mut().select(Some(0));
    let backend = TestBackend::new(80, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    let mut anim = AnimationState::default();
    terminal
        .draw(|frame| ui::render(frame, &mut app, &mut anim))
        .expect("render frame");
    let buf = terminal.backend().buffer().clone();
    assert_golden("keys_overview_narrow", &serialize_buffer(&buf));
}

/// Keys tab in search mode with a query that matches a subset of keys.
/// Confirms the search bar renders above the column header, the master
/// pane title shows `<query> (N/M)` with the filtered count, and the
/// list contains only matching rows.
#[test]
fn visual_keys_overview_search() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Keys;
    app.search.set_query(Some("rsa".to_string()));
    app.keys.list_state_mut().select(Some(0));
    let actual = render_screen(&mut app);
    assert_golden("keys_overview_search", &actual);
}

/// Keys tab in search mode with a query that matches nothing. Confirms
/// the master pane renders an empty list with the search bar's
/// filtered count showing 0.
#[test]
fn visual_keys_overview_search_no_match() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Keys;
    app.search.set_query(Some("xyzzy".to_string()));
    app.keys.list_state_mut().select(None);
    let actual = render_screen(&mut app);
    assert_golden("keys_overview_search_no_match", &actual);
}

/// Push-host picker freshly opened: no hosts selected yet. Vault-ssh
/// hosts render with `[-]` and a `(vault)` tag. Confirms the checkbox
/// column, vault-disabled style and overlay title.
#[test]
fn visual_keys_push_picker() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Keys;
    app.keys.list_state_mut().select(Some(0));
    app.screen = Screen::KeyPushPicker { key_index: 0 };
    app.keys.push_mut().list_state.select(Some(0));
    let actual = render_screen(&mut app);
    assert_golden("keys_push_picker", &actual);
}

/// Push-host picker with two hosts toggled on. Confirms the `[x]` mark
/// renders for selected non-vault hosts and the title shows the
/// "<N> selected of <total>" tally.
#[test]
fn visual_keys_push_picker_selected() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    // Lock the demo host count so the picker title `2 selected of 35
    // (N eligible)` is stable under demo-data drift. If a new host is
    // added or removed from the demo this assertion fails with a clear
    // message instead of an opaque golden diff.
    assert_eq!(
        app.hosts_state.list().len(),
        35,
        "demo host count drifted; update this assertion and regenerate the golden"
    );
    app.top_page = crate::app::TopPage::Keys;
    app.keys.list_state_mut().select(Some(0));
    app.screen = Screen::KeyPushPicker { key_index: 0 };
    app.keys.push_mut().list_state.select(Some(0));
    // Pick two non-vault hosts so the selected glyph renders.
    let to_select: Vec<String> = app
        .hosts_state
        .list()
        .iter()
        .filter(|h| h.vault_ssh.is_none())
        .take(2)
        .map(|h| h.alias.clone())
        .collect();
    for a in to_select {
        app.keys.push_mut().selected.insert(a);
    }
    let actual = render_screen(&mut app);
    assert_golden("keys_push_picker_selected", &actual);
}

/// Push confirm dialog with the selected aliases listed. Footer uses
/// the destructive action-verb pair `push` / `keep` via
/// `design::confirm_footer_destructive`.
#[test]
fn visual_keys_push_confirm() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Keys;
    let aliases: Vec<String> = app
        .hosts_state
        .list()
        .iter()
        .filter(|h| h.vault_ssh.is_none())
        .take(3)
        .map(|h| h.alias.clone())
        .collect();
    app.keys.push_mut().committed = aliases;
    app.screen = Screen::ConfirmKeyPush { key_index: 0 };
    let actual = render_screen(&mut app);
    assert_golden("keys_push_confirm", &actual);
}

/// Keys tab with zero keys discovered. Shows the empty-state hint inside
/// the master pane. Confirms the empty path renders the hint message and
/// not the table header.
#[test]
fn visual_keys_overview_empty() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Keys;
    app.keys.list_mut().clear();
    app.keys.list_state_mut().select(None);
    let actual = render_screen(&mut app);
    assert_golden("keys_overview_empty", &actual);
}

#[test]
fn visual_host_list_empty() {
    // Fresh install: no hosts in ~/.ssh/config. The Hosts tab must
    // render ONE centred TabEmpty card naming `a add` and `S sync`
    // as the next actions; the detail panel stays a quiet bordered
    // placeholder (no "Select a host to see details." floating top-right).
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.hosts_state.list_mut().clear();
    app.hosts_state.patterns_mut().clear();
    app.hosts_state.display_list_mut().clear();
    app.hosts_state.ssh_config_mut().elements = Vec::new();
    app.tunnels.clear_active();
    app.tunnels.demo_live_snapshots_mut().clear();
    let actual = render_screen(&mut app);
    assert_golden("host_list_empty", &actual);
}

#[test]
fn visual_containers_overview_empty() {
    // Fresh-install state: no container_cache entries. The Containers
    // tab must render ONE centred TabEmpty card (no duplicate "No
    // containers cached yet." in the detail panel).
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Containers;
    app.container_state.clear_cache();
    let actual = render_screen(&mut app);
    assert_golden("containers_overview_empty", &actual);
}

#[test]
fn visual_tunnels_overview_empty() {
    // No tunnel rules configured anywhere. The Tunnels tab must render
    // one centred TabEmpty card explaining how to add one. Easiest way
    // to force "no rules" without restructuring the demo SSH config is
    // to swap in a config that has no Host blocks at all.
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Tunnels;
    app.hosts_state.ssh_config_mut().elements = Vec::new();
    app.hosts_state.list_mut().clear();
    app.tunnels.clear_active();
    app.tunnels.demo_live_snapshots_mut().clear();
    let actual = render_screen(&mut app);
    assert_golden("tunnels_overview_empty", &actual);
}

#[test]
fn visual_containers_overview() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Containers;
    app.ui.containers_overview_state_mut().select(Some(0));
    let actual = render_screen(&mut app);
    assert_golden("containers_overview", &actual);
}

/// AlphaContainer sort mode: flat list, no host headers, HOST
/// column visible per-row. Specifically pins the regression where
/// `host_count` was being read from header items (zero in this mode)
/// and the stats title rendered "0 hosts".
#[test]
fn visual_containers_overview_alpha_container_mode() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Containers;
    app.containers_overview
        .set_sort_mode_ephemeral(crate::app::ContainersSortMode::AlphaContainer);
    app.ui.containers_overview_state_mut().select(Some(0));
    let actual = render_screen(&mut app);
    assert_golden("containers_overview_alpha_container", &actual);
}

/// At 200 cols the detail panel renders alongside the list. The panel
/// is 96 cols wide (twice the host-detail width) and the list takes the
/// remainder. Cursor placed on the first Container row of the first
/// non-folded host so the panel exercises the container branch rather
/// than the host-summary branch (covered separately by
/// `visual_containers_overview_host_detail`). Height set to 40 so the
/// LOGS card has visible inner rows beyond just open/close borders.
#[test]
fn visual_containers_overview_with_detail() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Containers;
    let items = crate::ui::containers_overview::visible_items(&app);
    let first_container = items
        .iter()
        .position(|i| i.as_container().is_some())
        .expect("demo cache has at least one container");
    app.ui
        .containers_overview_state_mut()
        .select(Some(first_container));
    let backend = TestBackend::new(200, 60);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    let mut anim = AnimationState::default();
    terminal
        .draw(|frame| ui::render(frame, &mut app, &mut anim))
        .expect("render frame");
    let buf = terminal.backend().buffer().clone();
    let actual = serialize_buffer(&buf);
    assert_golden("containers_overview_with_detail", &actual);
}

/// Podman snapshot. Cursor parked on the podman-edge group so the
/// detail panel shows the host summary for the only podman host in
/// the demo. Pins the empty-Status rendering and the inspect-driven
/// state-glyph fallback (loki exited with code 137 via inspect cache
/// only, since podman emits no Status string).
#[test]
fn visual_containers_overview_podman_host_detail() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Containers;
    let items = crate::ui::containers_overview::visible_items(&app);
    let podman_header = items
        .iter()
        .position(|i| match i {
            crate::ui::containers_overview::ContainerListItem::HostHeader { alias, .. } => {
                alias == "podman-edge"
            }
            _ => false,
        })
        .expect("podman-edge header in demo");
    app.ui
        .containers_overview_state_mut()
        .select(Some(podman_header));
    let backend = TestBackend::new(200, 60);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    let mut anim = AnimationState::default();
    terminal
        .draw(|frame| ui::render(frame, &mut app, &mut anim))
        .expect("render frame");
    let buf = terminal.backend().buffer().clone();
    let actual = serialize_buffer(&buf);
    assert_golden("containers_overview_podman_host_detail", &actual);
}

/// Sibling of the host-header variant: cursor parked on a running
/// podman container row so the detail panel shows the per-container
/// card with the docker.io/library/ image and empty docker-style
/// Status synthesized into the lower-case state label.
#[test]
fn visual_containers_overview_podman_container_detail() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Containers;
    let items = crate::ui::containers_overview::visible_items(&app);
    let caddy_row = items
        .iter()
        .position(|i| match i.as_container() {
            Some(row) => row.alias == "podman-edge" && row.name == "caddy",
            None => false,
        })
        .expect("podman-edge/caddy in demo");
    app.ui
        .containers_overview_state_mut()
        .select(Some(caddy_row));
    let backend = TestBackend::new(200, 60);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    let mut anim = AnimationState::default();
    terminal
        .draw(|frame| ui::render(frame, &mut app, &mut anim))
        .expect("render frame");
    let buf = terminal.backend().buffer().clone();
    let actual = serialize_buffer(&buf);
    assert_golden("containers_overview_podman_container_detail", &actual);
}

/// Sibling of `visual_containers_overview_with_detail`: cursor parked
/// on the first host-divider row so the detail panel renders the
/// per-host summary (running/exited/total, runtime, last sync, fold
/// state, key reminder). Pins the header-detail render path against
/// drift now that dividers are first-class selection targets.
#[test]
fn visual_containers_overview_host_detail() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Containers;
    let items = crate::ui::containers_overview::visible_items(&app);
    let first_header = items
        .iter()
        .position(|i| i.is_header())
        .expect("AlphaHost mode emits a header before the first container");
    app.ui
        .containers_overview_state_mut()
        .select(Some(first_header));
    let backend = TestBackend::new(200, 60);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    let mut anim = AnimationState::default();
    terminal
        .draw(|frame| ui::render(frame, &mut app, &mut anim))
        .expect("render frame");
    let buf = terminal.backend().buffer().clone();
    let actual = serialize_buffer(&buf);
    assert_golden("containers_overview_host_detail", &actual);
}

#[test]
fn visual_container_logs() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Containers;
    app.container_state.set_logs_view(crate::app::LogsView {
        alias: "aws-api-staging".to_string(),
        container_id: "f9a0b1c2d3e4".to_string(),
        container_name: "api".to_string(),
        body: vec![
            "2026-05-09 19:41:58  10.0.0.42  GET /api/v1/health 200 17ms".to_string(),
            "2026-05-09 19:42:01  198.51.100.7  POST /webhooks/github 202 41ms".to_string(),
            "2026-05-09 19:42:05  upstream timed out (110: Operation timed out)".to_string(),
            "2026-05-09 19:42:11  10.0.0.42  GET /api/v1/health 200 16ms".to_string(),
        ],
        fetched_at: crate::demo_flag::now_secs() - 3,
        error: None,
        scroll: 0,
        last_render_height: 0,
        search: None,
    });
    app.screen = Screen::ContainerLogs;
    let actual = render_screen(&mut app);
    assert_golden("container_logs", &actual);
}

#[test]
fn visual_container_logs_search_active() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Containers;
    let body = vec![
        "2026-05-09 19:41:58  10.0.0.42  GET /api/v1/health 200 17ms".to_string(),
        "2026-05-09 19:42:01  198.51.100.7  POST /webhooks/github 202 41ms".to_string(),
        "2026-05-09 19:42:05  upstream timed out (110: Operation timed out)".to_string(),
        "2026-05-09 19:42:11  10.0.0.42  GET /api/v1/health 200 16ms".to_string(),
    ];
    // Hand-built post-Enter state: query confirmed, three hits across
    // /api/v1/health and the GET lines, cursor parked on the first.
    let matches: Vec<usize> = body
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| {
            if crate::handler::container_logs::matches_line(line, "api") {
                Some(idx)
            } else {
                None
            }
        })
        .collect();
    app.container_state.set_logs_view(crate::app::LogsView {
        alias: "aws-api-staging".to_string(),
        container_id: "f9a0b1c2d3e4".to_string(),
        container_name: "api".to_string(),
        body,
        fetched_at: crate::demo_flag::now_secs() - 3,
        error: None,
        scroll: 0,
        last_render_height: 0,
        search: Some(crate::app::ContainerLogsSearch {
            query: "api".to_string(),
            matches,
            current: 0,
            cursor_pos: 3,
        }),
    });
    app.screen = Screen::ContainerLogs;
    let actual = render_screen(&mut app);
    assert_golden("container_logs_search_active", &actual);
}

#[test]
fn visual_confirm_container_restart() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Containers;
    app.screen = Screen::ConfirmContainerRestart {
        alias: "aws-api-staging".to_string(),
        container_id: "f9a0b1c2d3e4".to_string(),
        container_name: "api".to_string(),
        project: Some("aws-api-staging".to_string()),
        uptime: Some("2d".to_string()),
    };
    let actual = render_screen(&mut app);
    assert_golden("confirm_container_restart", &actual);
}

#[test]
fn visual_confirm_container_stop() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Containers;
    app.screen = Screen::ConfirmContainerStop {
        alias: "aws-api-staging".to_string(),
        container_id: "f9a0b1c2d3e4".to_string(),
        container_name: "api".to_string(),
        project: Some("aws-api-staging".to_string()),
        uptime: Some("2d".to_string()),
    };
    let actual = render_screen(&mut app);
    assert_golden("confirm_container_stop", &actual);
}

#[test]
fn visual_confirm_stack_restart() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Containers;
    app.containers_overview
        .set_pending_bulk_confirm(crate::app::BulkConfirmContext {
            kind: crate::app::BulkConfirmKind::StackRestart,
            alias: "aws-api-staging".to_string(),
            project: Some("aws-api-staging".to_string()),
            members: vec![
                crate::app::StackMember {
                    container_id: "f9a0b1c2d3e4".to_string(),
                    container_name: "api".to_string(),
                    uptime: Some("2d".to_string()),
                },
                crate::app::StackMember {
                    container_id: "a1b2c3d4e5f6".to_string(),
                    container_name: "datadog-agent".to_string(),
                    uptime: Some("2d".to_string()),
                },
                crate::app::StackMember {
                    container_id: "11223344aabb".to_string(),
                    container_name: "nginx".to_string(),
                    uptime: Some("2d".to_string()),
                },
            ],
        });
    app.screen = Screen::ConfirmStackRestart;
    let actual = render_screen(&mut app);
    assert_golden("confirm_stack_restart", &actual);
}

#[test]
fn visual_confirm_host_restart_all() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Containers;
    app.containers_overview
        .set_pending_bulk_confirm(crate::app::BulkConfirmContext {
            kind: crate::app::BulkConfirmKind::HostRestartAll,
            alias: "aws-api-staging".to_string(),
            project: None,
            members: vec![
                crate::app::StackMember {
                    container_id: "f9a0b1c2d3e4".to_string(),
                    container_name: "api".to_string(),
                    uptime: Some("2d".to_string()),
                },
                crate::app::StackMember {
                    container_id: "a1b2c3d4e5f6".to_string(),
                    container_name: "datadog-agent".to_string(),
                    uptime: Some("2d".to_string()),
                },
                crate::app::StackMember {
                    container_id: "11223344aabb".to_string(),
                    container_name: "nginx".to_string(),
                    uptime: Some("2d".to_string()),
                },
            ],
        });
    app.screen = Screen::ConfirmHostRestartAll;
    let actual = render_screen(&mut app);
    assert_golden("confirm_host_restart_all", &actual);
}

#[test]
fn visual_confirm_host_stop_all() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Containers;
    app.containers_overview
        .set_pending_bulk_confirm(crate::app::BulkConfirmContext {
            kind: crate::app::BulkConfirmKind::HostStopAll,
            alias: "aws-api-staging".to_string(),
            project: None,
            members: vec![
                crate::app::StackMember {
                    container_id: "f9a0b1c2d3e4".to_string(),
                    container_name: "api".to_string(),
                    uptime: Some("2d".to_string()),
                },
                crate::app::StackMember {
                    container_id: "a1b2c3d4e5f6".to_string(),
                    container_name: "datadog-agent".to_string(),
                    uptime: Some("2d".to_string()),
                },
            ],
        });
    app.screen = Screen::ConfirmHostStopAll;
    let actual = render_screen(&mut app);
    assert_golden("confirm_host_stop_all", &actual);
}

/// Containers overview after the user pressed `v` to fold the detail
/// panel. Verifies the list takes the full body width and the footer
/// flips to `v detail`.
#[test]
fn visual_containers_overview_compact() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Containers;
    app.containers_overview
        .set_view_mode_ephemeral(crate::app::ViewMode::Compact);
    app.ui.containers_overview_state_mut().select(Some(0));
    let actual = render_screen(&mut app);
    assert_golden("containers_overview_compact", &actual);
}

/// Containers overview with one host group folded via Space. The
/// folded host's containers vanish from the list and the divider's
/// suffix flips to `(N hidden)`.
#[test]
fn visual_containers_overview_collapsed_group() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Containers;
    app.containers_overview
        .toggle_host_collapsed("aws-api-staging");
    app.ui.containers_overview_state_mut().select(Some(0));
    let actual = render_screen(&mut app);
    assert_golden("containers_overview_collapsed_group", &actual);
}

/// Container in `paused` state. Exercises the warning-tier glyph
/// (`ICON_PAUSED`) and colour path for transitional container states.
/// Counterpart of the existing running/exited goldens which cover the
/// online and stopped tiers.
#[test]
fn visual_containers_overview_paused() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Containers;
    if let Some(entry) = app.container_state.cache_entry_mut("bastion-ams")
        && let Some(first) = entry.containers.first_mut()
    {
        first.state = "paused".to_string();
        first.status = "Paused".to_string();
    }
    app.ui.containers_overview_state_mut().select(Some(0));
    let actual = render_screen(&mut app);
    assert_golden("containers_overview_paused", &actual);
}

/// Container in `restarting` state. Exercises the warning-tier glyph
/// for the transitional restart path, distinct from `paused`.
#[test]
fn visual_containers_overview_restarting() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Containers;
    if let Some(entry) = app.container_state.cache_entry_mut("db-primary")
        && let Some(first) = entry.containers.first_mut()
    {
        first.state = "restarting".to_string();
        first.status = "Restarting (1) 2 seconds ago".to_string();
    }
    app.ui.containers_overview_state_mut().select(Some(0));
    let actual = render_screen(&mut app);
    assert_golden("containers_overview_restarting", &actual);
}

#[test]
fn visual_container_exec_prompt() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Containers;
    app.screen = Screen::ContainerExecPrompt {
        alias: "aws-api-staging".to_string(),
        container_id: "f9a0b1c2d3e4".to_string(),
        container_name: "api".to_string(),
        query: "tail -n 50 /var/log/app.log".to_string(),
    };
    let actual = render_screen(&mut app);
    assert_golden("container_exec_prompt", &actual);
}

#[test]
fn visual_tunnels_overview_delete_confirm() {
    // Pending-delete confirmation footer rendered over the overview.
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Tunnels;
    app.ui.tunnels_overview_state_mut().select(Some(0));
    app.tunnels.request_delete(0);
    let actual = render_screen(&mut app);
    assert_golden("tunnels_overview_delete_confirm", &actual);
}

#[test]
fn visual_key_list() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::KeyList;
    let actual = render_screen(&mut app);
    assert_golden("key_list", &actual);
}

#[test]
fn visual_key_detail() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::KeyDetail { index: 0 };
    let actual = render_screen(&mut app);
    assert_golden("key_detail", &actual);
}

#[test]
fn visual_help() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::HostList),
    };
    let actual = render_screen(&mut app);
    assert_golden("help", &actual);
}

#[test]
fn visual_confirm_delete() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::ConfirmDelete {
        alias: "bastion-ams".to_string(),
    };
    let actual = render_screen(&mut app);
    assert_golden("confirm_delete", &actual);
}

#[test]
fn visual_snippet_picker() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.snippets
        .set_flow_targets(vec!["bastion-ams".to_string()]);
    app.screen = Screen::SnippetPicker;
    let actual = render_screen(&mut app);
    assert_golden("snippet_picker", &actual);
}

#[test]
fn visual_snippet_host_picker() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.snippets.set_flow_snippet(Some(crate::snippet::Snippet {
        name: "deploy".into(),
        command: "make deploy".into(),
        description: "Ship the app".into(),
        interactive: false,
    }));
    app.snippets.host_pick_mut().list_state.select(Some(0));
    app.screen = Screen::SnippetHostPicker;
    let actual = render_screen(&mut app);
    assert_golden("snippet_host_picker", &actual);
}

#[test]
fn visual_snippet_host_picker_selected() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.snippets.set_flow_snippet(Some(crate::snippet::Snippet {
        name: "deploy".into(),
        command: "make deploy".into(),
        description: "Ship the app".into(),
        interactive: false,
    }));
    let first_two: Vec<String> = app
        .hosts_state
        .list()
        .iter()
        .take(2)
        .map(|h| h.alias.clone())
        .collect();
    for a in first_two {
        app.snippets.host_pick_mut().selected.insert(a);
    }
    app.snippets.host_pick_mut().list_state.select(Some(0));
    app.screen = Screen::SnippetHostPicker;
    let actual = render_screen(&mut app);
    assert_golden("snippet_host_picker_selected", &actual);
}

/// Opened from the edit form to choose default hosts: the title reads "Default
/// hosts for ..." and the footer commits with "save" (no run is launched).
#[test]
fn visual_snippet_host_picker_edit_default() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.snippets.set_flow_snippet(Some(crate::snippet::Snippet {
        name: "deploy".into(),
        command: "make deploy".into(),
        description: "Ship the app".into(),
        interactive: false,
    }));
    app.snippets.host_pick_mut().purpose = crate::app::SnippetHostPickPurpose::EditDefault;
    if let Some(alias) = app.hosts_state.list().first().map(|h| h.alias.clone()) {
        app.snippets.host_pick_mut().selected.insert(alias);
    }
    app.snippets.host_pick_mut().list_state.select(Some(0));
    app.screen = Screen::SnippetHostPicker;
    let actual = render_screen(&mut app);
    assert_golden("snippet_host_picker_edit_default", &actual);
}

/// With the host list grouped by provider, the picker shows group-header rows
/// (aggregate checkbox + name + count) with their hosts indented beneath, so a
/// whole group can be toggled at once.
#[test]
fn visual_snippet_host_picker_grouped() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.hosts_state.set_group_by(crate::app::GroupBy::Provider);
    app.snippets.set_flow_snippet(Some(crate::snippet::Snippet {
        name: "deploy".into(),
        command: "make deploy".into(),
        description: "Ship the app".into(),
        interactive: false,
    }));
    app.snippets.host_pick_mut().list_state.select(Some(0));
    app.screen = Screen::SnippetHostPicker;
    let actual = render_screen(&mut app);
    assert_golden("snippet_host_picker_grouped", &actual);
}

/// Type-to-filter active: a search line shows the live query and match count,
/// and the list is a flat fuzzy-ranked set of matching hosts (groups dropped).
#[test]
fn visual_snippet_host_picker_filtered() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.snippets.set_flow_snippet(Some(crate::snippet::Snippet {
        name: "deploy".into(),
        command: "make deploy".into(),
        description: "Ship the app".into(),
        interactive: false,
    }));
    app.snippets.host_pick_mut().query = "aws".to_string();
    app.snippets.host_pick_mut().filtering = true;
    app.snippets.host_pick_mut().list_state.select(Some(0));
    app.screen = Screen::SnippetHostPicker;
    let actual = render_screen(&mut app);
    assert_golden("snippet_host_picker_filtered", &actual);
}

#[test]
fn visual_confirm_run_snippet() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.snippets.set_flow_snippet(Some(crate::snippet::Snippet {
        name: "deploy".into(),
        command: "make deploy".into(),
        description: "Ship the app".into(),
        interactive: false,
    }));
    let targets: Vec<String> = app
        .hosts_state
        .list()
        .iter()
        .take(3)
        .map(|h| h.alias.clone())
        .collect();
    app.snippets.set_flow_targets(targets);
    app.screen = Screen::ConfirmRunSnippet;
    let actual = render_screen(&mut app);
    assert_golden("confirm_run_snippet", &actual);
}

#[test]
fn visual_snippets_overview() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Snippets;
    app.snippets.list_state_mut().select(Some(0));
    let actual = render_screen(&mut app);
    assert_golden("snippets_overview", &actual);
}

#[test]
fn visual_snippets_overview_empty() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Snippets;
    app.snippets.store_mut().snippets.clear();
    let actual = render_screen(&mut app);
    assert_golden("snippets_overview_empty", &actual);
}

#[test]
fn visual_snippets_overview_search() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Snippets;
    app.search.set_query(Some("deploy".to_string()));
    app.snippets.list_state_mut().select(Some(0));
    let actual = render_screen(&mut app);
    assert_golden("snippets_overview_search", &actual);
}

/// At 160 cols with the detail panel enabled (default Detailed view), the
/// command + parameters card renders alongside the list. Selects a
/// parametrised snippet so the Parameters section is exercised.
#[test]
fn visual_snippets_overview_with_detail() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Snippets;
    let deploy_idx = app
        .snippets
        .store()
        .snippets
        .iter()
        .position(|s| s.name == "deploy")
        .expect("demo snippets include deploy");
    app.snippets.list_state_mut().select(Some(deploy_idx));
    let backend = TestBackend::new(160, 40);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    let mut anim = AnimationState::default();
    terminal
        .draw(|frame| ui::render(frame, &mut app, &mut anim))
        .expect("render frame");
    let buf = terminal.backend().buffer().clone();
    let actual = serialize_buffer(&buf);
    assert_golden("snippets_overview_with_detail", &actual);
}

/// A destructive snippet with no run history: the IMPACT card flags `prune`
/// and no TRACK RECORD card renders (static cards only).
#[test]
fn visual_snippets_overview_destructive() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Snippets;
    let idx = app
        .snippets
        .store()
        .snippets
        .iter()
        .position(|s| s.name == "container-prune")
        .expect("demo snippets include container-prune");
    app.snippets.list_state_mut().select(Some(idx));
    let backend = TestBackend::new(160, 40);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    let mut anim = AnimationState::default();
    terminal
        .draw(|frame| ui::render(frame, &mut app, &mut anim))
        .expect("render frame");
    let buf = terminal.backend().buffer().clone();
    let actual = serialize_buffer(&buf);
    assert_golden("snippets_overview_destructive", &actual);
}

/// A piped snippet with a mixed run history: IMPACT flags pipe + redirect, the
/// TRACK RECORD verdict sits in the amber tier and the trend chart dips to the
/// 0 baseline on the failed run.
#[test]
fn visual_snippets_overview_mixed_runs() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Snippets;
    let idx = app
        .snippets
        .store()
        .snippets
        .iter()
        .position(|s| s.name == "backup-db")
        .expect("demo snippets include backup-db");
    app.snippets.list_state_mut().select(Some(idx));
    let backend = TestBackend::new(160, 40);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    let mut anim = AnimationState::default();
    terminal
        .draw(|frame| ui::render(frame, &mut app, &mut anim))
        .expect("render frame");
    let buf = terminal.backend().buffer().clone();
    let actual = serialize_buffer(&buf);
    assert_golden("snippets_overview_mixed_runs", &actual);
}

/// At 160 cols with the detail toggled off (Compact), the list spans full
/// width. Contrasts with `visual_snippets_overview_with_detail`.
#[test]
fn visual_snippets_overview_compact() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Snippets;
    app.snippets.toggle_view_mode(); // Detailed -> Compact
    app.snippets.list_state_mut().select(Some(0));
    let backend = TestBackend::new(160, 40);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    let mut anim = AnimationState::default();
    terminal
        .draw(|frame| ui::render(frame, &mut app, &mut anim))
        .expect("render frame");
    let buf = terminal.backend().buffer().clone();
    let actual = serialize_buffer(&buf);
    assert_golden("snippets_overview_compact", &actual);
}

/// The add form opened from the tab renders over the tab (not the host-list
/// picker) because `form_return_to_tab` is set.
#[test]
fn visual_snippets_overview_add_form() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Snippets;
    app.snippets.set_form_return_to_tab(true);
    app.snippets.set_form_editing(None);
    app.screen = Screen::SnippetForm;
    let actual = render_screen(&mut app);
    assert_golden("snippets_overview_add_form", &actual);
}

/// The delete confirmation popup renders over the Snippets tab.
#[test]
fn visual_snippets_overview_delete_confirm() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Snippets;
    app.snippets.list_state_mut().select(Some(0));
    app.snippets.request_delete(0);
    let actual = render_screen(&mut app);
    assert_golden("snippets_overview_delete_confirm", &actual);
}

#[test]
fn visual_snippet_form() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.snippets
        .set_flow_targets(vec!["bastion-ams".to_string()]);
    app.snippets.set_form_editing(None);
    app.screen = Screen::SnippetForm;
    let actual = render_screen(&mut app);
    assert_golden("snippet_form", &actual);
}

/// The Default hosts field focused: it shows the chosen aliases and the footer
/// carries the Space-pick hint (the field opens the host picker, it takes no
/// typed text).
#[test]
fn visual_snippet_form_default_hosts() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    *app.snippets.form_mut() = crate::app::SnippetForm::from_snippet(&crate::snippet::Snippet {
        name: "deploy".into(),
        command: "make deploy".into(),
        description: "Ship the app".into(),
        interactive: false,
    });
    app.snippets.form_mut().default_hosts = vec!["bastion-ams".into(), "db-primary".into()];
    app.snippets.form_mut().focused_field = crate::app::SnippetFormField::DefaultHosts;
    app.snippets.set_form_editing(Some(0));
    app.screen = Screen::SnippetForm;
    let actual = render_screen(&mut app);
    assert_golden("snippet_form_default_hosts", &actual);
}

#[test]
fn visual_snippet_output() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.snippets
        .set_output(Some(crate::app::SnippetOutputState {
            run_id: 1,
            results: vec![crate::app::SnippetHostOutput {
                alias: "bastion-ams".to_string(),
                stdout: "load average: 0.12 0.18 0.21\n".to_string(),
                stderr: String::new(),
                exit_code: Some(0),
            }],
            scroll_offset: 0,
            completed: 1,
            total: 1,
            all_done: true,
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            interactive: false,
        }));
    app.snippets
        .set_output_snippet_name(Some("uptime".to_string()));
    app.snippets
        .set_flow_targets(vec!["bastion-ams".to_string()]);
    app.screen = Screen::SnippetOutput;
    let actual = render_screen(&mut app);
    assert_golden("snippet_output", &actual);
}

#[test]
fn visual_snippet_param_form() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    let snippet = crate::snippet::Snippet {
        name: "uptime".to_string(),
        command: "uptime".to_string(),
        description: "Server uptime and load".to_string(),
        interactive: false,
    };
    // Param form requires state populated with the snippet's params (none here),
    // so build an empty SnippetParamFormState matching the snippet.
    let params: Vec<crate::snippet::SnippetParam> = Vec::new();
    app.snippets
        .set_param_form(Some(crate::app::SnippetParamFormState::new(&params)));
    app.snippets.set_param_snippet(Some(snippet));
    app.snippets
        .set_flow_targets(vec!["bastion-ams".to_string()]);
    app.screen = Screen::SnippetParamForm;
    let actual = render_screen(&mut app);
    assert_golden("snippet_param_form", &actual);
}

#[test]
fn visual_tag_picker() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::TagPicker;
    let actual = render_screen(&mut app);
    assert_golden("tag_picker", &actual);
}

#[test]
fn visual_theme_picker() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.ui.theme_picker_mut().builtins = ui::theme::ThemeDef::builtins();
    app.ui.theme_picker_mut().custom = Vec::new();
    app.ui.theme_picker_mut().saved_name = "Purple".to_string();
    app.ui.theme_picker_mut().list.select(Some(0));
    app.screen = Screen::ThemePicker;
    let actual = render_screen(&mut app);
    assert_golden("theme_picker", &actual);
}

#[test]
fn visual_containers() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    // Containers screen requires container_state. Populate from the demo cache.
    let alias = "bastion-ams".to_string();
    let cached = app
        .container_state
        .cache_entry(&alias)
        .map(|c| c.containers.clone())
        .unwrap_or_default();
    app.container_session = Some(crate::app::ContainerSession {
        alias: alias.clone(),
        askpass: None,
        runtime: Some(crate::containers::ContainerRuntime::Docker),
        containers: cached,
        list_state: ratatui::widgets::ListState::default(),
        loading: false,
        error: None,
        action_in_progress: None,
        confirm_action: None,
    });
    app.screen = Screen::Containers { alias };
    let actual = render_screen(&mut app);
    assert_golden("containers", &actual);
}

#[test]
fn visual_file_browser() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    let alias = "bastion-ams".to_string();
    // Use a deterministic empty browser state. remote_loading=true skips remote
    // I/O and local entries are intentionally empty so output is host-agnostic.
    app.file_browser_session = Some(crate::file_browser::FileBrowserSession {
        alias: alias.clone(),
        askpass: None,
        active_pane: crate::file_browser::BrowserPane::Local,
        local_path: std::path::PathBuf::from("/demo"),
        local_entries: Vec::new(),
        local_list_state: ratatui::widgets::ListState::default(),
        local_selected: std::collections::HashSet::new(),
        local_error: None,
        remote_path: String::new(),
        remote_entries: Vec::new(),
        remote_list_state: ratatui::widgets::ListState::default(),
        remote_selected: std::collections::HashSet::new(),
        remote_error: None,
        remote_loading: true,
        show_hidden: false,
        sort: crate::file_browser::BrowserSort::Name,
        confirm_copy: None,
        transferring: None,
        transfer_error: None,
        connection_recorded: true,
    });
    app.screen = Screen::FileBrowser { alias };
    let actual = render_screen(&mut app);
    assert_golden("file_browser", &actual);
}

#[test]
fn visual_file_browser_transfer_dialog() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    let alias = "bastion-ams".to_string();
    app.file_browser_session = Some(crate::file_browser::FileBrowserSession {
        alias: alias.clone(),
        askpass: None,
        active_pane: crate::file_browser::BrowserPane::Local,
        local_path: std::path::PathBuf::from("/demo"),
        local_entries: Vec::new(),
        local_list_state: ratatui::widgets::ListState::default(),
        local_selected: std::collections::HashSet::new(),
        local_error: None,
        remote_path: String::new(),
        remote_entries: Vec::new(),
        remote_list_state: ratatui::widgets::ListState::default(),
        remote_selected: std::collections::HashSet::new(),
        remote_error: None,
        remote_loading: true,
        show_hidden: false,
        sort: crate::file_browser::BrowserSort::Name,
        confirm_copy: None,
        transferring: Some(
            "/demo/very-long-archive-name-that-exercises-the-right-margin.tar.gz".to_string(),
        ),
        transfer_error: None,
        connection_recorded: true,
    });
    app.screen = Screen::FileBrowser { alias };
    let actual = render_screen(&mut app);
    assert_golden("file_browser_transfer_dialog", &actual);
}

#[test]
fn visual_jump() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.jump = Some(crate::app::JumpState::default());
    app.recompute_jump_hits();
    let actual = render_screen(&mut app);
    assert_golden("jump", &actual);
}

#[test]
fn visual_jump_query() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.jump = Some(crate::app::JumpState::default());
    if let Some(p) = app.jump.as_mut() {
        for c in "files".chars() {
            p.push_query(c);
        }
    }
    app.recompute_jump_hits();
    let actual = render_screen(&mut app);
    assert_golden("jump_query", &actual);
}

#[test]
fn visual_jump_no_results() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.jump = Some(crate::app::JumpState::default());
    if let Some(p) = app.jump.as_mut() {
        for c in "zzzqqq".chars() {
            p.push_query(c);
        }
    }
    app.recompute_jump_hits();
    let actual = render_screen(&mut app);
    assert_golden("jump_no_results", &actual);
}

#[test]
fn visual_jump_with_recents() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    let mut state = crate::app::JumpState::default();
    // Seed two recents: one host, one action. Exercises the
    // RECENT-section render path that visual_jump cannot.
    let n_action = crate::app::JumpAction::all()
        .iter()
        .find(|a| a.key == 'n')
        .copied()
        .expect("'n' (What's new) action present");
    state.set_recents(vec![
        crate::app::JumpHit::Host(crate::app::HostHit {
            alias: "bastion-ams".into(),
            hostname: "bastion.ams.example".into(),
            tags: vec![],
            provider: None,
            user: String::new(),
            identity_file: String::new(),
            proxy_jump: String::new(),
            vault_ssh: None,
        }),
        crate::app::JumpHit::Action(n_action),
    ]);
    app.jump = Some(state);
    app.recompute_jump_hits();
    let actual = render_screen(&mut app);
    assert_golden("jump_with_recents", &actual);
}

#[test]
fn visual_jump_over_tunnels() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Tunnels;
    app.jump = Some(crate::app::JumpState::for_mode(
        crate::app::JumpMode::Tunnels,
    ));
    app.recompute_jump_hits();
    let actual = render_screen(&mut app);
    assert_golden("jump_over_tunnels", &actual);
}

#[test]
fn visual_jump_over_containers() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Containers;
    app.jump = Some(crate::app::JumpState::for_mode(
        crate::app::JumpMode::Containers,
    ));
    app.recompute_jump_hits();
    let actual = render_screen(&mut app);
    assert_golden("jump_over_containers", &actual);
}

#[test]
fn visual_jump_over_snippets() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.top_page = crate::app::TopPage::Snippets;
    app.jump = Some(crate::app::JumpState::for_mode(
        crate::app::JumpMode::Snippets,
    ));
    app.recompute_jump_hits();
    let actual = render_screen(&mut app);
    assert_golden("jump_over_snippets", &actual);
}

#[test]
fn visual_bulk_tag_editor() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    // Bulk tag editor operates on multi_select. Populate it with a couple of demo hosts.
    app.hosts_state.multi_select_mut().insert(0);
    app.hosts_state.multi_select_mut().insert(1);
    app.screen = Screen::BulkTagEditor;
    let actual = render_screen(&mut app);
    assert_golden("bulk_tag_editor", &actual);
}

#[test]
fn visual_provider_list() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::Providers;
    let actual = render_screen(&mut app);
    assert_golden("provider_list", &actual);
}

#[test]
fn visual_provider_form() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::ProviderForm {
        id: crate::providers::config::ProviderConfigId::bare("aws"),
    };
    let actual = render_screen(&mut app);
    assert_golden("provider_form", &actual);
}

#[test]
fn visual_provider_form_teleport() {
    // Teleport has no Token field. The collapsed form must still show its
    // lead field (User, the Teleport login) with focus on it.
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.open_provider_form(crate::providers::config::ProviderConfigId::bare("teleport"));
    assert_eq!(
        app.providers.form().focused_field,
        crate::app::ProviderFormField::User
    );
    let actual = render_screen(&mut app);
    assert_golden("provider_form_teleport", &actual);
}

#[test]
fn visual_provider_form_netbox() {
    // NetBox leads with Url like Proxmox and adds the Filter field.
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.open_provider_form(crate::providers::config::ProviderConfigId::bare("netbox"));
    assert_eq!(
        app.providers.form().focused_field,
        crate::app::ProviderFormField::Url
    );
    let actual = render_screen(&mut app);
    assert_golden("provider_form_netbox", &actual);
}

#[test]
fn visual_provider_form_label_entry() {
    // Issue #51: when the user adds an N-th labeled config (with at least one
    // labeled section already present), the form opens with a `Label` input
    // prepended. The golden locks in the visual placement so a future field
    // reshuffle or omission of the prepend in visible_fields is caught.
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::ProviderForm {
        id: crate::providers::config::ProviderConfigId::labeled("aws", ""),
    };
    // Replicate what open_add_config_flow's `_ =>` branch would do, without
    // depending on demo data layout. Label is the focused field; the form
    // sits in collapsed mode like a freshly opened add flow.
    *app.providers.form_mut() = crate::app::ProviderFormFields {
        filter: String::new(),
        label: String::new(),
        label_entry: true,
        url: String::new(),
        token: String::new(),
        profile: String::new(),
        project: String::new(),
        compartment: String::new(),
        regions: String::new(),
        alias_prefix: "aws".to_string(),
        user: "root".to_string(),
        identity_file: String::new(),
        verify_tls: true,
        auto_sync: true,
        vault_role: String::new(),
        vault_addr: String::new(),
        focused_field: crate::app::ProviderFormField::Label,
        cursor_pos: 0,
        expanded: false,
        ssm: crate::providers::aws_ssm::SsmMode::default(),
    };
    let actual = render_screen(&mut app);
    assert_golden("provider_form_label_entry", &actual);
}

#[test]
fn visual_provider_form_aws_expanded() {
    // The collapsed golden never showed Profile or Session Manager. Expanded,
    // Profile has to draw the picker arrow its footer advertises, the way
    // Identity File and Regions do.
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.open_provider_form(crate::providers::config::ProviderConfigId::bare("aws"));
    app.providers.form_mut().expanded = true;
    app.providers.form_mut().profile = String::new();
    app.providers.form_mut().focused_field = crate::app::ProviderFormField::Profile;
    app.providers.form_mut().cursor_pos = 0;
    let actual = render_screen(&mut app);
    assert_golden("provider_form_aws_expanded", &actual);
}

#[test]
fn visual_aws_profile_picker() {
    // Three rows, three outcomes: plain, reaches another account and one
    // purple will refuse at sync time.
    let _g = setup();
    let home = tempfile::tempdir().expect("tempdir");
    let aws = home.path().join(".aws");
    std::fs::create_dir_all(&aws).expect("create .aws");
    std::fs::write(
        aws.join("config"),
        "[profile alpha]\nregion = eu-west-1\n\
         [profile org-prod]\nrole_arn = arn:aws:iam::1:role/Admin\nsource_profile = alpha\n\
         [profile sso-dev]\nsso_start_url = https://x.awsapps.com/start\n",
    )
    .expect("write config");
    std::fs::write(
        aws.join("credentials"),
        "[alpha]\naws_access_key_id = AKIA\naws_secret_access_key = S\n",
    )
    .expect("write credentials");

    let mut app = demo::build_demo_app();
    app.env = std::sync::Arc::new(crate::runtime::env::Env::for_test(home.path()));
    app.open_provider_form(crate::providers::config::ProviderConfigId::bare("aws"));
    app.providers.form_mut().expanded = true;
    app.providers.form_mut().focused_field = crate::app::ProviderFormField::Profile;
    app.open_profile_picker();
    let actual = render_screen(&mut app);
    assert_golden("aws_profile_picker", &actual);
}

#[test]
fn visual_provider_label_migration() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    // Seed pending state as if the user just hit `a` on a single-bare-config
    // provider; the screen prompts for both labels with focus on the new one.
    app.providers
        .set_pending_label_migration(Some(crate::app::PendingLabelMigration {
            provider: "hetzner".to_string(),
            existing_label: "default".to_string(),
            new_label: String::new(),
            focused: crate::app::LabelMigrationField::Existing,
            cursor_pos: "default".chars().count(),
        }));
    app.screen = Screen::ProviderLabelMigration {
        provider: "hetzner".to_string(),
    };
    let actual = render_screen(&mut app);
    assert_golden("provider_label_migration", &actual);
}

#[test]
fn visual_confirm_host_key_reset() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::ConfirmHostKeyReset {
        alias: "bastion-ams".to_string(),
        hostname: "bastion.example.com".to_string(),
        known_hosts_path: "/demo/.ssh/known_hosts".to_string(),
        askpass: None,
    };
    let actual = render_screen(&mut app);
    assert_golden("confirm_host_key_reset", &actual);
}

#[test]
fn visual_confirm_host_key_trust() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::ConfirmHostKeyTrust {
        alias: "bastion-ams".to_string(),
        hostname: "bastion.example.com".to_string(),
        retry: crate::app::PendingRetry::KeyPush {
            key_path: "~/.ssh/id_ed25519".to_string(),
            alias: "bastion-ams".to_string(),
        },
    };
    let actual = render_screen(&mut app);
    assert_golden("confirm_host_key_trust", &actual);
}

#[test]
fn visual_password_prompt() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    let mut state = crate::app::PasswordPromptState::new(
        "bastion-ams".to_string(),
        false,
        crate::app::PendingRetry::KeyPush {
            key_path: "~/.ssh/id_ed25519".to_string(),
            alias: "bastion-ams".to_string(),
        },
    );
    state.input = "hunter2".to_string();
    app.password_prompt = Some(state);
    app.screen = Screen::PasswordPrompt;
    let actual = render_screen(&mut app);
    assert_golden("password_prompt", &actual);
}

#[test]
fn visual_security_key_pin_prompt() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    let mut state = crate::app::PasswordPromptState::security_key_pin(
        "bastion-ams".to_string(),
        "/home/ops/.ssh/id_ed25519_sk".to_string(),
        crate::app::PendingRetry::Tunnel {
            alias: "bastion-ams".to_string(),
        },
    );
    state.input = "4242".to_string();
    app.password_prompt = Some(state);
    app.screen = Screen::PasswordPrompt;
    let actual = render_screen(&mut app);
    assert_golden("security_key_pin_prompt", &actual);
}

#[test]
fn visual_confirm_import() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::ConfirmImport { count: 5 };
    let actual = render_screen(&mut app);
    assert_golden("confirm_import", &actual);
}

#[test]
fn visual_confirm_purge_stale() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.providers.set_pending_purge(crate::app::PendingPurge {
        aliases: vec!["aws-old-1".to_string(), "aws-old-2".to_string()],
        provider: Some("aws".to_string()),
    });
    app.screen = Screen::ConfirmPurgeStale;
    let actual = render_screen(&mut app);
    assert_golden("confirm_purge_stale", &actual);
}

#[test]
fn visual_confirm_vault_sign() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.vault.set_pending_sign(Vec::new());
    app.screen = Screen::ConfirmVaultSign;
    let actual = render_screen(&mut app);
    assert_golden("confirm_vault_sign", &actual);
}

#[test]
fn visual_welcome() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::Welcome {
        has_backup: true,
        host_count: 22,
        known_hosts_count: 47,
    };
    let actual = render_screen(&mut app);
    assert_golden("welcome", &actual);
}

#[test]
fn visual_whats_new() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::WhatsNew(crate::app::WhatsNewState::default());
    let fixture = std::fs::read_to_string("tests/fixtures/changelog/simple.md").unwrap();
    let _override = crate::changelog::set_test_override(fixture);
    let actual = render_screen(&mut app);
    assert_golden("whats_new", &actual);
}

// ---------------------------------------------------------------------------
// Detail-panel branch coverage tests. Each test targets a sub-section of
// detail_panel::render that is not exercised by the existing
// host_list_detail_panel golden.
// ---------------------------------------------------------------------------

/// Select a host by alias in the main list so detail_panel renders it.
fn select_host_by_alias(app: &mut App, alias: &str) {
    use crate::app::HostListItem;
    let pos = app.hosts_state.display_list().iter().position(|item| {
        if let HostListItem::Host { index } = item {
            app.hosts_state
                .list()
                .get(*index)
                .map(|h| h.alias == alias)
                .unwrap_or(false)
        } else {
            false
        }
    });
    app.ui.list_state_mut().select(pos);
}

#[test]
fn visual_host_detail_vault_expired() {
    // Exercises the VAULT SSH section with CertStatus::Expired:
    // shows "Expired" in error style and "(press V to sign)" affordance.
    // gateway-vpn has a direct vault-ssh role (not inherited) and no provider
    // so the role line takes the non-inherited path.
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.vault.insert_cert(
        "gateway-vpn".to_string(),
        (
            std::time::Instant::now(),
            crate::vault_ssh::CertStatus::Expired,
            None,
        ),
    );
    select_host_by_alias(&mut app, "gateway-vpn");
    app.hosts_state
        .set_view_mode(crate::app::ViewMode::Detailed);
    let actual = render_screen(&mut app);
    assert_golden("host_detail_vault_expired", &actual);
}

#[test]
fn visual_host_detail_long_proxy_chain() {
    // Exercises the ROUTE section with a 3-hop ProxyJump chain.
    // customer-db-1 already jumps via customer-jump. We extend its
    // proxy_jump to include two additional known hosts so the route
    // visualisation renders three intermediate hop lines.
    let _g = setup();
    let mut app = demo::build_demo_app();
    // Inject a 3-hop chain: customer-jump, bastion-ams, gateway-vpn
    // All three are in the demo host list, so they render as known hops.
    if let Some(h) = app
        .hosts_state
        .list_mut()
        .iter_mut()
        .find(|h| h.alias == "customer-db-1")
    {
        h.proxy_jump = "customer-jump,bastion-ams,gateway-vpn".to_string();
    }
    select_host_by_alias(&mut app, "customer-db-1");
    app.hosts_state
        .set_view_mode(crate::app::ViewMode::Detailed);
    let actual = render_screen(&mut app);
    assert_golden("host_detail_long_proxy_chain", &actual);
}

#[test]
fn visual_host_detail_no_provider_tag() {
    // Exercises the detail panel for a host with no provider, no provider
    // metadata and no vault role. The Tags section still renders (user tags
    // present) but the Provider metadata and Vault SSH sections are absent.
    // prod-eu2 has user tags, vault-ssh, no provider. We clear the vault
    // cert cache for it so the status falls back to "Not signed".
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.vault.remove_cert("prod-eu2");
    select_host_by_alias(&mut app, "prod-eu2");
    app.hosts_state
        .set_view_mode(crate::app::ViewMode::Detailed);
    let actual = render_screen(&mut app);
    assert_golden("host_detail_no_provider_tag", &actual);
}

#[test]
fn visual_host_detail_no_containers() {
    // Exercises the detail panel for a host that has no entry in the
    // container_state cache. The CONTAINERS section must be absent.
    // prod-eu1 has user tags, vault-ssh (with an inherited provider role
    // absent since there is no provider), yubikey identity. The demo
    // container cache has no entry for it.
    let _g = setup();
    let mut app = demo::build_demo_app();
    // Ensure there is definitely no container cache entry.
    app.container_state.remove_cache_entry("prod-eu1");
    select_host_by_alias(&mut app, "prod-eu1");
    app.hosts_state
        .set_view_mode(crate::app::ViewMode::Detailed);
    let actual = render_screen(&mut app);
    assert_golden("host_detail_no_containers", &actual);
}

#[test]
fn visual_host_detail_with_tags() {
    // Exercises the Tags section together with the Provider metadata section.
    // aws-api-prod carries user tags, provider tags (via provider comment),
    // provider metadata (region, instance, os, status) and an inherited
    // vault role from the aws provider config. This is the densest path
    // through the lower half of detail_panel::render.
    let _g = setup();
    let mut app = demo::build_demo_app();
    select_host_by_alias(&mut app, "aws-api-prod");
    app.hosts_state
        .set_view_mode(crate::app::ViewMode::Detailed);
    let actual = render_screen(&mut app);
    assert_golden("host_detail_with_tags", &actual);
}

/// Select the pve-* pattern so the detail panel renders the per-category
/// directive cards (CONNECTION, AUTHENTICATION, ... MULTIPLEXING) instead of a
/// single flat DIRECTIVES card. Exercises the long-keyword row layout too.
#[test]
fn visual_pattern_detail() {
    use crate::app::HostListItem;
    let _g = setup();
    let mut app = demo::build_demo_app();
    let pos = app
        .hosts_state
        .display_list()
        .iter()
        .position(|item| matches!(item, HostListItem::Pattern { .. }));
    assert!(
        pos.is_some(),
        "demo config must contain a selectable pattern"
    );
    app.ui.list_state_mut().select(pos);
    let actual = render_screen(&mut app);
    assert_golden("pattern_detail", &actual);
}
