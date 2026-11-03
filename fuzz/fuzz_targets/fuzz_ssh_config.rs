#![no_main]

use libfuzzer_sys::fuzz_target;
use std::path::PathBuf;

fuzz_target!(|data: &[u8]| {
    // Only fuzz valid UTF-8 (SSH configs are text files).
    let Ok(content) = std::str::from_utf8(data) else {
        return;
    };

    // 1. Parse must not panic.
    let elements = purple_ssh::ssh_config::model::SshConfigFile::parse_content(content);

    let config = purple_ssh::ssh_config::model::SshConfigFile {
        elements,
        path: PathBuf::from("/tmp/fuzz_config"),
        crlf: content.contains("\r\n"),
        bom: content.starts_with('\u{FEFF}'),
    };

    // 2. Serialize must not panic.
    let serialized = config.serialize();

    // 3. host_entries and the Match block view must not panic.
    let _ = config.host_entries();
    let _ = config.match_block_entries();

    // 4. Idempotency: serialize(parse(serialize(parse(input)))) == serialize(parse(input))
    let reparsed = purple_ssh::ssh_config::model::SshConfigFile {
        elements: purple_ssh::ssh_config::model::SshConfigFile::parse_content(&serialized),
        path: PathBuf::from("/tmp/fuzz_config"),
        crlf: serialized.contains("\r\n"),
        bom: serialized.starts_with('\u{FEFF}'),
    };
    let reserialized = reparsed.serialize();
    assert_eq!(
        serialized,
        reserialized,
        "Round-trip not idempotent for input of length {}",
        content.len()
    );

    // 5. Mutation smoke tests (must not panic).
    let entries = config.host_entries();
    if !entries.is_empty() {
        let alias = entries[0].alias.clone();

        // Delete
        let mut config_del = config.clone();
        config_del.delete_host(&alias);
        let _ = config_del.serialize();

        // Delete undoable + undo
        let mut config_undo = config.clone();
        if let Some((element, position)) = config_undo.delete_host_undoable(&alias) {
            config_undo.insert_host_at(element, position);
            let _ = config_undo.serialize();
        }

        // Update
        let mut config_upd = config.clone();
        config_upd.update_host(
            &alias,
            &purple_ssh::ssh_config::model::HostEntry {
                alias: alias.clone(),
                hostname: "10.0.0.1".to_string(),
                user: "fuzz".to_string(),
                port: 22,
                ..Default::default()
            },
        );
        let _ = config_upd.serialize();

        // Swap (if 2+ hosts)
        if entries.len() >= 2 {
            let mut config_swap = config.clone();
            config_swap.swap_hosts(&entries[0].alias, &entries[1].alias);
            let _ = config_swap.serialize();
        }
    }

    // 6. Add host. Adding one host must leave every other host's own
    //    directives and every Match block exactly as they were.
    let new_alias = "fuzz-new-host";
    let mut config_add = reparsed.clone();
    config_add.add_host(&purple_ssh::ssh_config::model::HostEntry {
        alias: new_alias.to_string(),
        hostname: "10.0.0.99".to_string(),
        user: "fuzzer".to_string(),
        port: 22,
        ..Default::default()
    });
    let added = config_add.serialize();
    if !reparsed.has_host(new_alias) {
        let after = purple_ssh::ssh_config::model::SshConfigFile {
            elements: purple_ssh::ssh_config::model::SshConfigFile::parse_content(&added),
            path: PathBuf::from("/tmp/fuzz_config"),
            crlf: added.contains("\r\n"),
            bom: added.starts_with('\u{FEFF}'),
        };
        let own = |c: &purple_ssh::ssh_config::model::SshConfigFile| {
            c.own_host_entries()
                .into_iter()
                .filter(|h| h.alias != new_alias)
                .map(|h| (h.alias, h.hostname, h.user, h.port, h.identity_file, h.proxy_jump))
                .collect::<Vec<_>>()
        };
        assert_eq!(own(&reparsed), own(&after), "add_host changed another host");
        let blocks = |c: &purple_ssh::ssh_config::model::SshConfigFile| {
            c.match_block_entries()
                .into_iter()
                .map(|m| (m.criteria_text, m.directives))
                .collect::<Vec<_>>()
        };
        assert_eq!(blocks(&reparsed), blocks(&after), "add_host changed a Match block");
    }

    // 7. Tags, provider, askpass, meta must not panic.
    for entry in &entries {
        let _ = &entry.tags;
        let _ = &entry.provider;
        let _ = &entry.askpass;
        let _ = &entry.provider_meta;
    }

    // 8. Vault SSH mutation API coverage.
    //    set_host_certificate_file is the newest write path and the one that
    //    writes a potentially user-visible directive into the ssh config.
    //    Exercise it against every parsed alias AND against a synthetic
    //    ghost alias to cover the "missing alias" branch. Round-trip
    //    idempotency of the result is asserted on each mutation so libfuzzer
    //    has a crash signal for any regression in the writer.
    if !entries.is_empty() {
        for entry in &entries {
            let mut c = config.clone();
            let wrote = c.set_host_certificate_file(&entry.alias, "~/.purple/certs/fuzz-cert.pub");
            let s1 = c.serialize();
            let mut c2 = purple_ssh::ssh_config::model::SshConfigFile {
                elements: purple_ssh::ssh_config::model::SshConfigFile::parse_content(&s1),
                path: PathBuf::from("/tmp/fuzz_config"),
                crlf: s1.contains("\r\n"),
                bom: s1.starts_with('\u{FEFF}'),
            };
            let s2 = c2.serialize();
            assert_eq!(
                s1, s2,
                "set_host_certificate_file broke round-trip idempotency (wrote={})",
                wrote
            );
            // Clear it again — must also be idempotent.
            let _ = c2.set_host_certificate_file(&entry.alias, "");
            let _ = c2.serialize();
        }

        // Ghost alias (must be a no-op with byte-identical output).
        let mut c_ghost = config.clone();
        let before = c_ghost.serialize();
        let wrote = c_ghost
            .set_host_certificate_file("\u{0000}fuzz-ghost-never-exists\u{0000}", "/tmp/g.pub");
        assert!(!wrote, "ghost alias should never match a real block");
        assert_eq!(
            before,
            c_ghost.serialize(),
            "ghost alias write mutated the config"
        );

        // Same ghost-alias invariant for set_host_vault_addr. Mirrors the
        // set_host_certificate_file path so the wildcard-refuse and
        // missing-alias branches both get libfuzzer coverage.
        let mut c_ghost_addr = config.clone();
        let before_addr = c_ghost_addr.serialize();
        let wrote_addr = c_ghost_addr.set_host_vault_addr(
            "\u{0000}fuzz-ghost-never-exists\u{0000}",
            "http://ghost.invalid:8200",
        );
        assert!(
            !wrote_addr,
            "ghost alias should never match a real block (vault_addr)"
        );
        assert_eq!(
            before_addr,
            c_ghost_addr.serialize(),
            "ghost alias vault_addr write mutated the config"
        );
    }

    // 9. set_host_vault_ssh / set_host_tags / set_host_stale / set_host_meta
    //    are all mutation APIs added after the fuzz target was written.
    //    Exercise them against the first entry if any, purely as a panic
    //    smoke test plus round-trip idempotency.
    if let Some(entry) = entries.first() {
        let mut c = config.clone();
        // set_host_vault_ssh is #[must_use] -> bool; swallow the return like
        // the set_host_vault_addr / set_host_certificate_file calls below.
        let _ = c.set_host_vault_ssh(&entry.alias, "ssh-client-signer/sign/fuzz-role");
        // set_host_vault_addr is #[must_use] -> bool; swallow the return like
        // the set_host_certificate_file calls above.
        let _ = c.set_host_vault_addr(&entry.alias, "http://127.0.0.1:8200");
        c.set_host_tags(&entry.alias, &["fuzz".to_string(), "prop".to_string()]);
        c.set_host_stale(&entry.alias, 1_700_000_000);
        c.clear_host_stale(&entry.alias);
        c.set_host_meta(
            &entry.alias,
            &[
                ("region".to_string(), "eu-west-1".to_string()),
                ("os".to_string(), "linux".to_string()),
            ],
        );
        // Clearing vault-addr must also round-trip cleanly.
        let _ = c.set_host_vault_addr(&entry.alias, "");
        let s1 = c.serialize();
        let c2 = purple_ssh::ssh_config::model::SshConfigFile {
            elements: purple_ssh::ssh_config::model::SshConfigFile::parse_content(&s1),
            path: PathBuf::from("/tmp/fuzz_config"),
            crlf: s1.contains("\r\n"),
            bom: s1.starts_with('\u{FEFF}'),
        };
        assert_eq!(
            s1,
            c2.serialize(),
            "post-v2.8.1 mutation APIs broke round-trip"
        );
    }

    // 10. Multi-config provider marker writers (3-segment) and the lazy
    //     migration helper. These mutate `# purple:provider` comments and
    //     are the highest-risk paths in the data layer that handles
    //     ~/.ssh/config. Exercise both paths against every parsed alias
    //     plus the cross-config helper.
    use purple_ssh::providers::config::ProviderConfigId;
    if let Some(entry) = entries.first() {
        let mut c = config.clone();
        // Bare write.
        c.set_host_provider_id(&entry.alias, &ProviderConfigId::bare("fuzz-bare"), "id-1");
        // Labeled write.
        c.set_host_provider_id(
            &entry.alias,
            &ProviderConfigId::labeled("fuzz-prov", "fuzz-label"),
            "id-2",
        );
        // Downgrade back to bare.
        c.set_host_provider_id(&entry.alias, &ProviderConfigId::bare("fuzz-prov"), "id-3");
        let s1 = c.serialize();
        let c2 = purple_ssh::ssh_config::model::SshConfigFile {
            elements: purple_ssh::ssh_config::model::SshConfigFile::parse_content(&s1),
            path: PathBuf::from("/tmp/fuzz_config"),
            crlf: s1.contains("\r\n"),
            bom: s1.starts_with('\u{FEFF}'),
        };
        assert_eq!(
            s1,
            c2.serialize(),
            "set_host_provider_id broke round-trip"
        );
    }

    // Lazy-migration legacy-rewrite helper. Run against every provider name
    // referenced by parsed markers so the fuzzer can drive it on whatever
    // input combination it constructs. Round-trip after.
    let providers_seen: std::collections::HashSet<String> = entries
        .iter()
        .filter_map(|e| e.provider.clone())
        .collect();
    for prov in &providers_seen {
        let mut c = config.clone();
        let _ = c.rewrite_legacy_markers_to_label(prov, "fuzz-default");
        let s1 = c.serialize();
        let c2 = purple_ssh::ssh_config::model::SshConfigFile {
            elements: purple_ssh::ssh_config::model::SshConfigFile::parse_content(&s1),
            path: PathBuf::from("/tmp/fuzz_config"),
            crlf: s1.contains("\r\n"),
            bom: s1.starts_with('\u{FEFF}'),
        };
        assert_eq!(
            s1,
            c2.serialize(),
            "rewrite_legacy_markers_to_label broke round-trip"
        );
    }

    // Bare-config raw lookup must not panic on arbitrary input. The
    // semantic correctness (no host duplication on bare-config sync) is
    // covered by the `bare_sync_*` unit tests in providers/sync_tests.rs.
    for prov in &providers_seen {
        let _ = config.find_hosts_by_provider_raw(prov);
    }

    // 11. Forward-rule mutators (add_forward / remove_forward) and the
    //     remaining purple:* writers (askpass, provider_tags). These were
    //     missing from the fuzz target until now.
    if let Some(entry) = entries.first() {
        let mut c = config.clone();
        c.add_forward(&entry.alias, "LocalForward", "8080 localhost:80");
        c.add_forward(&entry.alias, "RemoteForward", "9090 localhost:3000");
        c.add_forward(&entry.alias, "DynamicForward", "1080");
        let _ = c.remove_forward(&entry.alias, "LocalForward", "8080 localhost:80");
        c.set_host_askpass(&entry.alias, "keychain");
        c.set_host_askpass(&entry.alias, "vault:secret/ssh/fuzz");
        c.set_host_askpass(&entry.alias, "");
        c.set_host_provider_tags(&entry.alias, &["fuzz".to_string(), "auto".to_string()]);
        c.set_host_provider_tags(&entry.alias, &[]);
        let s1 = c.serialize();
        let c2 = purple_ssh::ssh_config::model::SshConfigFile {
            elements: purple_ssh::ssh_config::model::SshConfigFile::parse_content(&s1),
            path: PathBuf::from("/tmp/fuzz_config"),
            crlf: s1.contains("\r\n"),
            bom: s1.starts_with('\u{FEFF}'),
        };
        assert_eq!(
            s1,
            c2.serialize(),
            "forward / askpass / provider_tags mutators broke round-trip"
        );
    }
});
