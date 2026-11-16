use super::*;

#[test]
fn auth_header_uses_bearer_for_v2_tokens() {
    assert_eq!(auth_header("nbt_abc.def"), "Bearer nbt_abc.def");
}

#[test]
fn auth_header_uses_token_scheme_for_classic_tokens() {
    assert_eq!(auth_header("0123456789abcdef"), "Token 0123456789abcdef");
}

#[test]
fn normalize_filter_defaults_to_active() {
    assert_eq!(normalize_filter(""), "status=active");
    assert_eq!(normalize_filter("   "), "status=active");
}

#[test]
fn normalize_filter_strips_leading_separators() {
    assert_eq!(normalize_filter("?tag=ssh"), "tag=ssh");
    assert_eq!(normalize_filter("&tag=ssh"), "tag=ssh");
    assert_eq!(normalize_filter("tag=ssh&site=ams1"), "tag=ssh&site=ams1");
}

fn device_json(id: u64, name: &str, ip4: Option<&str>) -> String {
    let ip = match ip4 {
        Some(a) => format!("{{\"address\": \"{a}\"}}"),
        None => "null".to_string(),
    };
    format!(
        "{{\"id\": {id}, \"name\": \"{name}\", \"status\": {{\"value\": \"active\"}}, \
         \"primary_ip4\": {ip}, \"primary_ip6\": null, \
         \"tags\": [{{\"slug\": \"prod\"}}, {{\"slug\": \"ansible\"}}], \
         \"site\": {{\"name\": \"ams1\"}}, \"role\": {{\"name\": \"server\"}}, \
         \"platform\": {{\"name\": \"ubuntu-24.04\"}}, \
         \"device_type\": {{\"model\": \"PowerEdge R650\"}}}}"
    )
}

#[test]
fn map_object_maps_a_device() {
    let obj: NetBoxObject =
        serde_json::from_str(&device_json(42, "db-01", Some("192.0.2.10/24"))).unwrap();
    let host = map_object(&obj, ObjectKind::Device).unwrap();
    assert_eq!(host.server_id, "device-42");
    assert_eq!(host.name, "db-01");
    assert_eq!(host.ip, "192.0.2.10");
    assert_eq!(host.port, None);
    assert_eq!(host.tags, vec!["ansible".to_string(), "prod".to_string()]);
    let meta: std::collections::HashMap<_, _> = host.metadata.into_iter().collect();
    assert_eq!(meta.get("location").map(String::as_str), Some("ams1"));
    assert_eq!(meta.get("role").map(String::as_str), Some("server"));
    assert_eq!(meta.get("os").map(String::as_str), Some("ubuntu-24.04"));
    assert_eq!(meta.get("type").map(String::as_str), Some("PowerEdge R650"));
    assert_eq!(meta.get("status").map(String::as_str), Some("active"));
}

#[test]
fn map_object_uses_the_selected_interface_ip() {
    let obj: NetBoxObject =
        serde_json::from_str(&device_json(42, "db-01", Some("192.0.2.10/24"))).unwrap();
    let host = map_object_with_ip(&obj, ObjectKind::Device, Some("198.51.100.10/24")).unwrap();
    assert_eq!(host.ip, "198.51.100.10");
}

#[test]
fn map_object_prefixes_vm_ids() {
    let obj: NetBoxObject =
        serde_json::from_str(&device_json(42, "app-01", Some("192.0.2.11/24"))).unwrap();
    let host = map_object(&obj, ObjectKind::Vm).unwrap();
    assert_eq!(host.server_id, "vm-42");
}

#[test]
fn map_object_skips_objects_without_primary_ip() {
    let obj: NetBoxObject = serde_json::from_str(&device_json(7, "no-ip", None)).unwrap();
    assert!(map_object(&obj, ObjectKind::Device).is_none());
}

#[test]
fn map_object_skips_unnamed_objects() {
    let json = r#"{"id": 9, "name": null, "primary_ip4": {"address": "192.0.2.9/24"}}"#;
    let obj: NetBoxObject = serde_json::from_str(json).unwrap();
    assert!(map_object(&obj, ObjectKind::Device).is_none());
}

#[test]
fn map_object_falls_back_to_ipv6_and_strips_cidr() {
    let json = r#"{"id": 3, "name": "v6-only", "primary_ip4": null,
                   "primary_ip6": {"address": "2001:db8::1/64"}}"#;
    let obj: NetBoxObject = serde_json::from_str(json).unwrap();
    assert_eq!(
        map_object(&obj, ObjectKind::Device).unwrap().ip,
        "2001:db8::1"
    );
}

#[test]
fn object_accepts_legacy_device_role_key() {
    let json = r#"{"id": 5, "name": "old", "primary_ip4": {"address": "192.0.2.5/24"},
                   "device_role": {"name": "router"}}"#;
    let obj: NetBoxObject = serde_json::from_str(json).unwrap();
    let host = map_object(&obj, ObjectKind::Device).unwrap();
    assert!(
        host.metadata
            .contains(&("role".to_string(), "router".to_string()))
    );
}

#[test]
fn object_ignores_unknown_fields() {
    let json = r#"{"id": 1, "name": "x", "primary_ip4": {"address": "192.0.2.1/24"},
                   "some_future_field": {"nested": true}}"#;
    assert!(serde_json::from_str::<NetBoxObject>(json).is_ok());
}

#[test]
fn vm_maps_cluster_into_metadata() {
    let json = r#"{"id": 8, "name": "vm-a", "primary_ip4": {"address": "192.0.2.8/24"},
                   "cluster": {"name": "prod-vmw"}}"#;
    let obj: NetBoxObject = serde_json::from_str(json).unwrap();
    let host = map_object(&obj, ObjectKind::Vm).unwrap();
    assert!(
        host.metadata
            .contains(&("cluster".to_string(), "prod-vmw".to_string()))
    );
}

fn netbox() -> NetBox {
    NetBox {
        base_url: String::new(),
        verify_tls: true,
        filter: String::new(),
        interface: String::new(),
        ssh_port: None,
    }
}

fn empty_page() -> &'static str {
    r#"{"count": 0, "next": null, "previous": null, "results": []}"#
}

#[test]
fn fetch_from_walks_devices_then_vms() {
    let mut server = mockito::Server::new();
    let devices = server
        .mock("GET", "/api/dcim/devices/")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("limit".into(), "100".into()),
            mockito::Matcher::UrlEncoded("offset".into(), "0".into()),
            mockito::Matcher::UrlEncoded("status".into(), "active".into()),
        ]))
        .match_header("Authorization", "Token tk-1")
        .with_body(format!(
            r#"{{"count": 1, "next": null, "previous": null, "results": [{}]}}"#,
            device_json(42, "db-01", Some("192.0.2.10/24"))
        ))
        .create();
    let vms = server
        .mock("GET", "/api/virtualization/virtual-machines/")
        .match_query(mockito::Matcher::UrlEncoded("offset".into(), "0".into()))
        .match_header("Authorization", "Token tk-1")
        .with_body(
            r#"{"count": 1, "next": null, "previous": null, "results":
               [{"id": 42, "name": "app-01", "primary_ip4": {"address": "192.0.2.11/24"}}]}"#,
        )
        .create();

    let mut nb = netbox();
    nb.ssh_port = Some(2222);
    let hosts = nb
        .fetch_from(&server.url(), "tk-1", &AtomicBool::new(false))
        .unwrap();
    devices.assert();
    vms.assert();
    assert_eq!(hosts.len(), 2);
    assert_eq!(hosts[0].server_id, "device-42");
    assert_eq!(hosts[1].server_id, "vm-42");
    assert_eq!(hosts[0].port, Some(2222));
    assert_eq!(hosts[1].port, Some(2222));
}

#[test]
fn fetch_from_follows_next_with_offset() {
    let mut server = mockito::Server::new();
    let next_url = format!("{}/api/dcim/devices/?limit=100&offset=100", server.url());
    let page1 = server
        .mock("GET", "/api/dcim/devices/")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("offset".into(), "0".into()),
            mockito::Matcher::UrlEncoded("status".into(), "active".into()),
        ]))
        .with_body(format!(
            r#"{{"count": 101, "next": "{next_url}", "previous": null, "results": [{}]}}"#,
            device_json(1, "a", Some("192.0.2.1/24"))
        ))
        .create();
    let page2 = server
        .mock("GET", "/api/dcim/devices/")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("offset".into(), "100".into()),
            mockito::Matcher::UrlEncoded("status".into(), "active".into()),
        ]))
        .with_body(format!(
            r#"{{"count": 101, "next": null, "previous": null, "results": [{}]}}"#,
            device_json(2, "b", Some("192.0.2.2/24"))
        ))
        .create();
    let vms = server
        .mock("GET", "/api/virtualization/virtual-machines/")
        .match_query(mockito::Matcher::Any)
        .with_body(empty_page())
        .create();

    let hosts = netbox()
        .fetch_from(&server.url(), "tk-1", &AtomicBool::new(false))
        .unwrap();
    page1.assert();
    page2.assert();
    vms.assert();
    assert_eq!(hosts.len(), 2);
}

#[test]
fn fetch_from_sends_bearer_for_v2_tokens() {
    let mut server = mockito::Server::new();
    let devices = server
        .mock("GET", "/api/dcim/devices/")
        .match_query(mockito::Matcher::Any)
        .match_header("Authorization", "Bearer nbt_a.b")
        .with_body(empty_page())
        .create();
    let _vms = server
        .mock("GET", "/api/virtualization/virtual-machines/")
        .match_query(mockito::Matcher::Any)
        .with_body(empty_page())
        .create();
    netbox()
        .fetch_from(&server.url(), "nbt_a.b", &AtomicBool::new(false))
        .unwrap();
    devices.assert();
}

#[test]
fn fetch_from_passes_the_configured_filter() {
    let mut server = mockito::Server::new();
    let devices = server
        .mock("GET", "/api/dcim/devices/")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("tag".into(), "ssh".into()),
            mockito::Matcher::UrlEncoded("site".into(), "ams1".into()),
        ]))
        .with_body(empty_page())
        .create();
    let _vms = server
        .mock("GET", "/api/virtualization/virtual-machines/")
        .match_query(mockito::Matcher::Any)
        .with_body(empty_page())
        .create();
    let mut nb = netbox();
    nb.filter = "tag=ssh&site=ams1".to_string();
    nb.fetch_from(&server.url(), "tk-1", &AtomicBool::new(false))
        .unwrap();
    devices.assert();
}

#[test]
fn fetch_from_uses_the_selected_interface_for_devices_and_vms() {
    let mut server = mockito::Server::new();
    let devices = server
        .mock("GET", "/api/dcim/devices/")
        .match_query(mockito::Matcher::Any)
        .with_body(format!(
            r#"{{"count": 1, "next": null, "previous": null, "results": [{}]}}"#,
            device_json(42, "db-01", Some("192.0.2.10/24"))
        ))
        .create();
    let vms = server
        .mock("GET", "/api/virtualization/virtual-machines/")
        .match_query(mockito::Matcher::Any)
        .with_body(
            r#"{"count": 1, "next": null, "previous": null, "results":
               [{"id": 9, "name": "app-01", "primary_ip4": {"address": "192.0.2.9/24"}}]}"#,
        )
        .create();
    let device_ips = server
        .mock("GET", "/api/ipam/ip-addresses/")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("device_id".into(), "42".into()),
            mockito::Matcher::UrlEncoded("offset".into(), "0".into()),
        ]))
        .with_body(
            r#"{"count": 1, "next": null, "previous": null, "results":
                             [{"address": "198.51.100.42/24", "assigned_object":
                                 {"name": "ens192", "device": {"id": 42}}}]}"#,
        )
        .create();
    let vm_ips = server
        .mock("GET", "/api/ipam/ip-addresses/")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("virtual_machine_id".into(), "9".into()),
            mockito::Matcher::UrlEncoded("offset".into(), "0".into()),
        ]))
        .with_body(
            r#"{"count": 1, "next": null, "previous": null, "results":
                             [{"address": "203.0.113.9/24", "assigned_object":
                                 {"name": "ens192", "virtual_machine": {"id": 9}}}]}"#,
        )
        .create();

    let mut nb = netbox();
    nb.interface = "ens192".to_string();
    nb.ssh_port = Some(2222);
    let hosts = nb
        .fetch_from(&server.url(), "tk-1", &AtomicBool::new(false))
        .unwrap();
    devices.assert();
    vms.assert();
    device_ips.assert();
    vm_ips.assert();
    assert_eq!(hosts.len(), 2);
    assert_eq!(hosts[0].ip, "198.51.100.42");
    assert_eq!(hosts[1].ip, "203.0.113.9");
    assert_eq!(hosts[0].port, Some(2222));
    assert_eq!(hosts[1].port, Some(2222));
}

#[test]
fn fetch_from_skips_objects_without_the_selected_interface_ip() {
    let mut server = mockito::Server::new();
    let _devices = server
        .mock("GET", "/api/dcim/devices/")
        .match_query(mockito::Matcher::Any)
        .with_body(format!(
            r#"{{"count": 1, "next": null, "previous": null, "results": [{}]}}"#,
            device_json(42, "db-01", Some("192.0.2.10/24"))
        ))
        .create();
    let _vms = server
        .mock("GET", "/api/virtualization/virtual-machines/")
        .match_query(mockito::Matcher::Any)
        .with_body(empty_page())
        .create();
    let _ips = server
        .mock("GET", "/api/ipam/ip-addresses/")
        .match_query(mockito::Matcher::UrlEncoded(
            "device_id".into(),
            "42".into(),
        ))
        .with_body(
            r#"{"count": 1, "next": null, "previous": null, "results":
                             [{"address": "198.51.100.42/24", "assigned_object":
                                 {"name": "eth0", "device": {"id": 42}}}]}"#,
        )
        .create();

    let mut nb = netbox();
    nb.interface = "ens192".to_string();
    let hosts = nb
        .fetch_from(&server.url(), "tk-1", &AtomicBool::new(false))
        .unwrap();
    assert!(hosts.is_empty());
}

#[test]
fn fetch_from_maps_auth_failure() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", "/api/dcim/devices/")
        .match_query(mockito::Matcher::Any)
        .with_status(401)
        .create();
    let err = netbox()
        .fetch_from(&server.url(), "bad", &AtomicBool::new(false))
        .unwrap_err();
    assert!(matches!(err, ProviderError::AuthFailed));
}

#[test]
fn devices_fixture_deserializes_and_maps() {
    let json = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/api_contracts/netbox_devices.json"
    ));
    let resp: NetBoxListResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.results.len(), 2);
    let hosts: Vec<ProviderHost> = resp
        .results
        .iter()
        .filter_map(|o| map_object(o, ObjectKind::Device))
        .collect();
    assert_eq!(hosts.len(), 2);
    assert_eq!(hosts[0].ip, "192.0.2.10");
    assert_eq!(hosts[1].ip, "2001:db8::10");
}

#[test]
fn vms_fixture_deserializes_and_maps() {
    let json = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/api_contracts/netbox_virtual_machines.json"
    ));
    let resp: NetBoxListResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.results.len(), 2);
    let hosts: Vec<ProviderHost> = resp
        .results
        .iter()
        .filter_map(|o| map_object(o, ObjectKind::Vm))
        .collect();
    assert_eq!(hosts.len(), 2);
    assert_eq!(hosts[0].server_id, "vm-2044");
    assert!(
        hosts[0]
            .metadata
            .contains(&("cluster".to_string(), "prod-vmw".to_string()))
    );
}

#[test]
fn ip_addresses_fixture_deserializes_assigned_interfaces() {
    let json = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/api_contracts/netbox_ip_addresses.json"
    ));
    let resp: NetBoxIpListResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.results.len(), 2);
    let device = resp.results[0].assigned_object.as_ref().unwrap();
    assert_eq!(device.name, "ens192");
    assert_eq!(device.device.as_ref().unwrap().id, 101);
    let vm = resp.results[1].assigned_object.as_ref().unwrap();
    assert_eq!(vm.virtual_machine.as_ref().unwrap().id, 2044);
}

#[test]
fn map_object_treats_empty_address_as_missing() {
    let json = r#"{"id": 4, "name": "x", "primary_ip4": {"address": ""},
                   "primary_ip6": {"address": "2001:db8::4/64"}}"#;
    let obj: NetBoxObject = serde_json::from_str(json).unwrap();
    assert_eq!(
        map_object(&obj, ObjectKind::Device).unwrap().ip,
        "2001:db8::4"
    );
}

#[test]
fn fetch_from_returns_partial_when_vm_stage_fails() {
    // Devices land, then the VM endpoint breaks: the devices must survive as
    // a PartialResult so sync still adds and updates while remove and stale
    // marking stay suppressed upstream.
    let mut server = mockito::Server::new();
    let _devices = server
        .mock("GET", "/api/dcim/devices/")
        .match_query(mockito::Matcher::Any)
        .with_body(format!(
            r#"{{"count": 1, "next": null, "previous": null, "results": [{}]}}"#,
            device_json(1, "a", Some("192.0.2.1/24"))
        ))
        .create();
    let _vms = server
        .mock("GET", "/api/virtualization/virtual-machines/")
        .match_query(mockito::Matcher::Any)
        .with_status(500)
        .create();
    let err = netbox()
        .fetch_from(&server.url(), "tk", &AtomicBool::new(false))
        .unwrap_err();
    match err {
        ProviderError::PartialResult { hosts, .. } => {
            assert_eq!(hosts.len(), 1);
            assert_eq!(hosts[0].server_id, "device-1");
        }
        other => panic!("expected PartialResult, got {other:?}"),
    }
}

#[test]
fn fetch_from_fails_hard_on_malformed_first_page() {
    // A parse failure with nothing collected is a hard error: the provider is
    // skipped and the user's config stays untouched.
    let mut server = mockito::Server::new();
    let _devices = server
        .mock("GET", "/api/dcim/devices/")
        .match_query(mockito::Matcher::Any)
        .with_body("this is not json")
        .create();
    let err = netbox()
        .fetch_from(&server.url(), "tk", &AtomicBool::new(false))
        .unwrap_err();
    assert!(matches!(err, ProviderError::Parse(_)));
}

#[test]
fn fetch_from_tolerates_a_trailing_slash_in_the_base_url() {
    let mut server = mockito::Server::new();
    let devices = server
        .mock("GET", "/api/dcim/devices/")
        .match_query(mockito::Matcher::Any)
        .with_body(empty_page())
        .create();
    let _vms = server
        .mock("GET", "/api/virtualization/virtual-machines/")
        .match_query(mockito::Matcher::Any)
        .with_body(empty_page())
        .create();
    let base = format!("{}/", server.url());
    netbox()
        .fetch_from(&base, "tk", &AtomicBool::new(false))
        .unwrap();
    devices.assert();
}

#[test]
fn hostile_netbox_payloads_cannot_corrupt_the_ssh_config() {
    use crate::providers::config::{ProviderConfigId, ProviderSection};
    use crate::providers::sync::sync_provider;
    use crate::ssh_config::model::SshConfigFile;

    // This test writes and re-reads an on-disk config. `SshConfigFile::write`
    // no-ops in demo mode, so serialize against demo-flag mutators and pin
    // the flag off, mirroring the writer's own on-disk tests.
    let _g = crate::demo_flag::GLOBAL_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::demo_flag::disable();

    struct MockNetBox;
    impl Provider for MockNetBox {
        fn name(&self) -> &str {
            "netbox"
        }
        fn short_label(&self) -> &str {
            "nb"
        }
        fn fetch_hosts_cancellable(
            &self,
            _token: &str,
            _cancel: &AtomicBool,
            _env: &crate::runtime::env::Env,
        ) -> Result<Vec<ProviderHost>, ProviderError> {
            Ok(Vec::new())
        }
    }

    // Every provider-supplied string carries an injection attempt: newlines
    // that would open new directives or Host blocks, plus delimiter noise.
    let json = r#"{"id": 666,
        "name": "evil\n  ProxyJump attacker.example",
        "status": {"value": "active\nHost *"},
        "primary_ip4": {"address": "192.0.2.66/24\n  IdentityFile /tmp/evil"},
        "primary_ip6": null,
        "tags": [{"slug": "bad\ntag,with=delims"}],
        "site": {"name": "ams1\n  ProxyCommand evil"},
        "role": {"name": "server,role=fake"},
        "device_type": {"model": "Model\nHost injected"}}"#;
    let obj: NetBoxObject = serde_json::from_str(json).unwrap();
    let host = map_object(&obj, ObjectKind::Device).unwrap();

    let path = tempfile::tempdir().unwrap().keep().join("config");
    let mut config = SshConfigFile {
        elements: Vec::new(),
        path: path.clone(),
        crlf: false,
        bom: false,
    };
    let section = ProviderSection {
        id: ProviderConfigId::bare("netbox"),
        token: "tk".to_string(),
        alias_prefix: "nb".to_string(),
        user: "root".to_string(),
        identity_file: String::new(),
        url: "https://netbox.example.com".to_string(),
        verify_tls: true,
        auto_sync: false,
        profile: String::new(),
        regions: String::new(),
        project: String::new(),
        compartment: String::new(),
        filter: String::new(),
        interface: String::new(),
        ssh_port: None,
        vault_role: String::new(),
        vault_addr: String::new(),
        ssm: crate::providers::aws_ssm::SsmMode::default(),
    };
    let result = sync_provider(
        &mut config,
        &MockNetBox,
        &[host],
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.added, 1);
    config.write().unwrap();

    // Structural assertions on the written file: the payloads may survive as
    // inert text inside a quoted value or comment, but never as a directive
    // or Host line of their own.
    let written = std::fs::read_to_string(&path).unwrap();
    let host_lines: Vec<&str> = written
        .lines()
        .filter(|l| l.trim_start().starts_with("Host "))
        .collect();
    assert_eq!(host_lines.len(), 1, "exactly one Host line: {written}");
    assert!(
        host_lines[0].starts_with("Host nb-evil-"),
        "alias must come from sanitize_name: {written}"
    );
    for line in written.lines() {
        let first = line.split_whitespace().next().unwrap_or("");
        assert!(
            !matches!(first, "ProxyJump" | "ProxyCommand" | "IdentityFile"),
            "injected directive became real: {line:?}"
        );
    }

    // The file must survive a full re-parse as exactly one intact host.
    let reparsed = SshConfigFile {
        elements: SshConfigFile::parse_content(&written),
        path,
        crlf: false,
        bom: false,
    };
    let entries = reparsed.host_entries();
    assert_eq!(entries.len(), 1, "re-parse must yield one host: {written}");
    assert!(!entries[0].hostname.contains('\n'));
    assert!(
        entries[0].hostname.starts_with("192.0.2.66"),
        "hostname mangled: {:?}",
        entries[0].hostname
    );
    assert!(entries[0].proxy_jump.is_empty());
    assert!(entries[0].identity_file.is_empty());
}

#[test]
fn fetch_hosts_rejects_missing_and_non_https_urls() {
    let env = crate::runtime::env::Env::for_test("/tmp/x");
    let missing = netbox();
    assert!(matches!(
        missing.fetch_hosts_cancellable("tk", &AtomicBool::new(false), &env),
        Err(ProviderError::Http(_))
    ));
    let http_only = NetBox {
        base_url: "http://netbox.example.com".to_string(),
        verify_tls: true,
        filter: String::new(),
        interface: String::new(),
        ssh_port: None,
    };
    assert!(matches!(
        http_only.fetch_hosts_cancellable("tk", &AtomicBool::new(false), &env),
        Err(ProviderError::Http(_))
    ));
}
