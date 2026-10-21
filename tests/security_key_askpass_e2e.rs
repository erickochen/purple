//! End-to-end test for security key PIN prompts in askpass mode.
//!
//! ssh asks for a FIDO2 key's PIN through askpass. This spawns the real
//! binary the way ssh would, with a cleared environment. It checks that a
//! PIN prompt never gets a password, that a PIN goes out only for its own
//! key and at most once per ssh process and that a touch notice starts no
//! password lookup.

#![cfg(unix)]

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const KEY: &str = "/home/ops/.ssh/id_ed25519_sk";
const PIN_PROMPT: &str = "Enter PIN for ED25519-SK key /home/ops/.ssh/id_ed25519_sk: ";
const SOURCE_SECRET: &str = "source-secret";

fn purple_bin() -> &'static str {
    env!("CARGO_BIN_EXE_purple")
}

struct Fixture {
    home: tempfile::TempDir,
    config_path: PathBuf,
    _config_dir: tempfile::TempDir,
}

impl Fixture {
    fn state_dir(&self) -> PathBuf {
        self.home.path().join(".purple")
    }

    fn marker(&self, name: &str) -> PathBuf {
        self.state_dir().join(name)
    }
}

/// A direct host whose password source is a command, so a password lookup
/// is visible without a keychain or a vault.
fn setup() -> Fixture {
    let home = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    let config_path = config_dir.path().join("config");
    std::fs::write(
        &config_path,
        format!(
            "Host target\n    HostName target.example.com\n    IdentityFile {KEY}\n    # purple:askpass echo {SOURCE_SECRET}\n"
        ),
    )
    .unwrap();
    Fixture {
        home,
        config_path,
        _config_dir: config_dir,
    }
}

/// purple in askpass mode as ssh starts it. `extra` adds the variables a
/// test needs. The child gets no controlling terminal, so a terminal-path
/// prompt can never wait for keys.
fn askpass(f: &Fixture, prompt: &str, extra: &[(&str, &str)]) -> std::process::Output {
    let mut cmd = Command::new(purple_bin());
    cmd.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("PURPLE_ASKPASS_MODE", "1")
        .env("PURPLE_HOST_ALIAS", "target")
        .env("PURPLE_CONFIG_PATH", &f.config_path)
        .env("HOME", f.home.path())
        .arg(prompt);
    for (k, v) in extra {
        cmd.env(k, v);
    }
    // SAFETY: setsid is async-signal-safe and only detaches the child from
    // the test runner's terminal.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd.output().expect("failed to spawn purple binary")
}

/// What a background run sets, as opposed to the terminal path.
const BACKGROUND: (&str, &str) = ("PURPLE_ASKPASS_SINGLE_ATTEMPT", "1");

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// The refusal marker name for `key_path`, as purple derives it.
fn digest(key_path: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(key_path.as_bytes())
        .iter()
        .take(16)
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn a_pin_prompt_never_gets_the_host_password() {
    // Each wrong PIN costs the key an attempt, so the password a source or
    // a typed session password holds must never go out as one.
    let f = setup();
    let out = askpass(
        &f,
        PIN_PROMPT,
        &[
            BACKGROUND,
            ("PURPLE_ASKPASS_ONESHOT_ALIAS", "target"),
            ("PURPLE_ASKPASS_ONESHOT", "hunter2"),
        ],
    );
    assert!(!out.status.success());
    assert_eq!(stdout(&out), "");
}

#[test]
fn a_pin_prompt_on_the_terminal_path_never_gets_the_host_password() {
    let f = setup();
    let out = askpass(&f, PIN_PROMPT, &[]);
    assert!(!out.status.success(), "no terminal to ask on");
    assert_eq!(stdout(&out), "");
}

#[test]
fn a_flow_that_can_ask_leaves_a_request_naming_the_key() {
    let f = setup();
    let out = askpass(
        &f,
        PIN_PROMPT,
        &[BACKGROUND, ("PURPLE_ASKPASS_SK_FLOW", "tunnel")],
    );
    assert!(!out.status.success());
    assert_eq!(
        read(&f.marker(".askpass_pin.request.tunnel.target")).as_deref(),
        Some(KEY)
    );
}

#[test]
fn a_run_that_cannot_ask_leaves_no_request() {
    let f = setup();
    let out = askpass(&f, PIN_PROMPT, &[BACKGROUND]);
    assert!(!out.status.success());
    assert!(read(&f.marker(".askpass_pin.request.tunnel.target")).is_none());
}

#[test]
fn the_pin_goes_out_for_its_own_key_once_per_ssh_process() {
    let f = setup();
    let env = [
        BACKGROUND,
        ("PURPLE_ASKPASS_SK_KEY_0", KEY),
        ("PURPLE_ASKPASS_SK_PIN_0", "4242"),
    ];
    let out = askpass(&f, PIN_PROMPT, &env);
    assert!(out.status.success());
    assert_eq!(stdout(&out), "4242");
    let used = read(&f.marker(".askpass_pin.used.target")).expect("hand-out recorded");
    assert_eq!(used, format!("{}\t{KEY}\n", std::process::id()));

    // The same ssh asking again means its first try failed.
    let again = askpass(&f, PIN_PROMPT, &env);
    assert!(!again.status.success());
    assert_eq!(stdout(&again), "");
}

#[test]
fn a_pin_for_another_key_is_not_handed_out() {
    let f = setup();
    let out = askpass(
        &f,
        PIN_PROMPT,
        &[
            BACKGROUND,
            ("PURPLE_ASKPASS_SK_KEY_0", "/home/ops/.ssh/other_sk"),
            ("PURPLE_ASKPASS_SK_PIN_0", "4242"),
        ],
    );
    assert!(!out.status.success());
    assert_eq!(stdout(&out), "");
}

#[test]
fn a_pin_the_key_refused_is_withheld_and_asked_for_again() {
    let f = setup();
    std::fs::create_dir_all(f.state_dir()).unwrap();
    std::fs::write(
        f.marker(&format!(".askpass_pin.refused.{}", digest(KEY))),
        format!("wrong_pin\n{KEY}\n"),
    )
    .unwrap();
    let out = askpass(
        &f,
        PIN_PROMPT,
        &[
            BACKGROUND,
            ("PURPLE_ASKPASS_SK_KEY_0", KEY),
            ("PURPLE_ASKPASS_SK_PIN_0", "4242"),
            ("PURPLE_ASKPASS_SK_FLOW", "key_push"),
        ],
    );
    assert!(!out.status.success());
    assert_eq!(stdout(&out), "");
    assert_eq!(
        read(&f.marker(".askpass_pin.request.key_push.target")).as_deref(),
        Some(KEY)
    );
}

#[test]
fn a_touch_notice_runs_no_password_lookup() {
    let f = setup();
    let touched = f.home.path().join("lookup-ran");
    std::fs::write(
        &f.config_path,
        format!(
            "Host target\n    HostName target.example.com\n    # purple:askpass touch {}\n",
            touched.display()
        ),
    )
    .unwrap();
    let out = askpass(
        &f,
        "Confirm user presence for key ED25519-SK SHA256:abcdef",
        &[BACKGROUND, ("SSH_ASKPASS_PROMPT", "none")],
    );
    assert!(out.status.success());
    assert_eq!(stdout(&out), "");
    assert!(!touched.exists(), "the source ran for a notice");
}

#[test]
fn a_keyboard_interactive_prompt_is_not_taken_for_a_pin_prompt() {
    // OpenSSH 8.5+ prefixes server prompts, so this is the server talking.
    // It goes down the password path like any other server prompt.
    let f = setup();
    let out = askpass(
        &f,
        &format!("(ops@target.example.com) {PIN_PROMPT}"),
        &[BACKGROUND, ("PURPLE_ASKPASS_SK_FLOW", "tunnel")],
    );
    assert!(read(&f.marker(".askpass_pin.request.tunnel.target")).is_none());
    assert_eq!(stdout(&out), SOURCE_SECRET);
}

#[test]
fn a_pin_prompt_leaves_the_password_prompt_after_it_untouched() {
    // On the terminal path the retry marker refuses a second answer within
    // a minute. A PIN prompt earlier in the same run must not arm it.
    let f = setup();
    let pin = askpass(&f, PIN_PROMPT, &[]);
    assert!(!pin.status.success());
    let password = askpass(&f, "ops@target.example.com's password: ", &[]);
    assert!(password.status.success());
    assert_eq!(stdout(&password), SOURCE_SECRET);
}
