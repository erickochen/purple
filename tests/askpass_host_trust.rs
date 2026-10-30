//! Trust and permission prompts must never read a configured password source.
use std::path::Path;
use std::process::{Command, Output};

const SECRET: &str = "TRUST_TEST_SECRET";

/// Run purple as askpass for host `probe`, whose source would print `SECRET`.
fn run_askpass(dir: &Path, prompt: &str, configure: impl FnOnce(&mut Command)) -> Output {
    let config = dir.join("config");
    std::fs::write(
        &config,
        format!("Host probe\n HostName example.test\n User root\n # purple:askpass cmd:printf {SECRET}\n"),
    )
    .unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_purple"));
    cmd.arg(prompt)
        .env("HOME", dir)
        .env("PURPLE_ASKPASS_MODE", "1")
        .env("PURPLE_HOST_ALIAS", "probe")
        .env("PURPLE_CONFIG_PATH", &config)
        .env_remove("SSH_ASKPASS_PROMPT");
    for category in ["CONFIG", "DATA", "STATE", "CACHE"] {
        cmd.env(format!("PURPLE_{category}_DIR"), dir);
    }
    configure(&mut cmd);
    cmd.output().unwrap()
}

#[test]
fn retry_prompts_reject_background_access_without_reading_passwords() {
    let dir = tempfile::tempdir().unwrap();
    for prompt in [
        "Continue (yes/no)?",
        "Please type 'yes', 'no' or the fingerprint: ",
        "Please type 'yes' or 'no': ",
    ] {
        let output = run_askpass(dir.path(), prompt, |cmd| {
            cmd.env("PURPLE_ASKPASS_SINGLE_ATTEMPT", "1");
        });
        assert!(
            !output.status.success(),
            "trust retry unexpectedly accepted"
        );
        assert!(
            output.stdout.is_empty(),
            "trust retry returned password data"
        );
    }
}

#[test]
fn other_yes_no_and_passphrase_prompts_are_refused_quietly() {
    let dir = tempfile::tempdir().unwrap();
    for prompt in [
        "Enter passphrase for key '/home/u/keys/yes/no/id_ed25519': ",
        "(root@probe) Accept the terms, yes/no? ",
    ] {
        let output = run_askpass(dir.path(), prompt, |cmd| {
            cmd.env("PURPLE_ASKPASS_SINGLE_ATTEMPT", "1");
        });
        assert!(!output.status.success(), "{prompt} was answered");
        assert!(output.stdout.is_empty(), "{prompt} returned password data");
        assert!(output.stderr.is_empty(), "{prompt} took the host key path");
    }
}

#[test]
fn password_prompts_still_read_the_source() {
    let dir = tempfile::tempdir().unwrap();
    let output = run_askpass(dir.path(), "root@example.test's password: ", |_| {});
    assert!(output.status.success(), "password prompt was refused");
    assert_eq!(output.stdout, SECRET.as_bytes());
}

#[test]
fn permission_prompts_are_refused_without_reading_passwords() {
    let dir = tempfile::tempdir().unwrap();
    let output = run_askpass(dir.path(), "Allow shared connection to probe? ", |cmd| {
        cmd.env("SSH_ASKPASS_PROMPT", "confirm");
    });
    assert!(
        !output.status.success(),
        "permission prompt unexpectedly accepted"
    );
    assert!(
        output.stdout.is_empty(),
        "permission prompt returned password data"
    );
}
