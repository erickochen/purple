//! Trust retries must never read a configured password source.
#[test]
fn retry_prompts_reject_background_access_without_reading_passwords() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    std::fs::write(&config, "Host probe\n HostName example.test\n User root\n # purple:askpass cmd:printf TRUST_TEST_SECRET\n").unwrap();
    for prompt in [
        "Continue (yes/no)?",
        "Please type 'yes', 'no' or the fingerprint: ",
        "Please type 'yes' or 'no': ",
    ] {
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_purple"));
        cmd.arg(prompt)
            .env("PURPLE_ASKPASS_MODE", "1")
            .env("PURPLE_ASKPASS_SINGLE_ATTEMPT", "1")
            .env("PURPLE_HOST_ALIAS", "probe")
            .env("PURPLE_CONFIG_PATH", &config)
            .env_remove("SSH_ASKPASS_PROMPT");
        for category in ["CONFIG", "DATA", "STATE", "CACHE"] {
            cmd.env(format!("PURPLE_{category}_DIR"), dir.path());
        }
        let output = cmd.output().unwrap();
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
