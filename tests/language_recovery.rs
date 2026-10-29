//! Broken optional catalogs must not disable English or preference recovery.
#[test]
fn invalid_external_catalog_keeps_cli_and_language_recovery_available() {
    for content in ["not json", r#"{"Edit: {}":"missing placeholder"}"#] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("locales")).unwrap();
        std::fs::write(dir.path().join("locales/zh-CN.json"), content).unwrap();
        std::fs::write(dir.path().join("language"), "zh-CN\n").unwrap();
        let config = dir.path().join("config");
        std::fs::write(&config, "").unwrap();
        for args in [
            vec!["--lang", "en", "--list"],
            vec!["--lang", "zh-CN", "--list"],
            vec!["--set-language", "en"],
        ] {
            let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_purple"));
            cmd.args(args).arg("--config").arg(&config);
            for category in ["CONFIG", "DATA", "STATE", "CACHE"] {
                cmd.env(format!("PURPLE_{category}_DIR"), dir.path());
            }
            let output = cmd.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                String::from_utf8_lossy(&output.stderr)
                    .contains("Ignoring invalid language override")
            );
        }
        assert_eq!(
            std::fs::read_to_string(dir.path().join("language")).unwrap(),
            "en\n"
        );
    }
}
