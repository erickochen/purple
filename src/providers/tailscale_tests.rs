use super::*;

// =========================================================================
// CLI parsing
// =========================================================================

#[test]
fn test_parse_cli_status_basic() {
    let json = r#"{
        "Peer": {
            "abc123": {
                "ID": "n12345",
                "HostName": "web-server",
                "TailscaleIPs": ["100.64.0.1", "fd7a:115c:a1e0::1"],
                "OS": "linux",
                "Online": true,
                "Tags": ["tag:server"]
            }
        }
    }"#;
    let status: CliStatus = serde_json::from_str(json).unwrap();
    let hosts = Tailscale::hosts_from_cli(status).unwrap();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].server_id, "n12345");
    assert_eq!(hosts[0].name, "web-server");
    assert_eq!(hosts[0].ip, "100.64.0.1");
    assert_eq!(hosts[0].tags, vec!["server"]);
    assert!(
        hosts[0]
            .metadata
            .iter()
            .any(|(k, v)| k == "os" && v == "linux")
    );
    assert!(
        hosts[0]
            .metadata
            .iter()
            .any(|(k, v)| k == "status" && v == "online")
    );
}

#[test]
fn test_parse_cli_status_no_peers() {
    let json = r#"{"Peer": {}}"#;
    let status: CliStatus = serde_json::from_str(json).unwrap();
    let hosts = Tailscale::hosts_from_cli(status).unwrap();
    assert!(hosts.is_empty());
}

#[test]
fn test_parse_cli_status_null_peer() {
    let json = r#"{}"#;
    let status: CliStatus = serde_json::from_str(json).unwrap();
    let hosts = Tailscale::hosts_from_cli(status).unwrap();
    assert!(hosts.is_empty());
}

#[test]
fn test_parse_cli_peer_no_ips_skipped() {
    let json = r#"{
        "Peer": {
            "abc": {
                "ID": "n1",
                "HostName": "no-ip",
                "TailscaleIPs": [],
                "OS": "linux",
                "Online": true,
                "Tags": []
            }
        }
    }"#;
    let status: CliStatus = serde_json::from_str(json).unwrap();
    let hosts = Tailscale::hosts_from_cli(status).unwrap();
    assert!(hosts.is_empty());
}

#[test]
fn test_parse_cli_peer_ipv4_preferred() {
    let json = r#"{
        "Peer": {
            "abc": {
                "ID": "n1",
                "HostName": "dual",
                "TailscaleIPs": ["fd7a:115c:a1e0::1", "100.64.0.5"],
                "OS": "",
                "Online": true,
                "Tags": []
            }
        }
    }"#;
    let status: CliStatus = serde_json::from_str(json).unwrap();
    let hosts = Tailscale::hosts_from_cli(status).unwrap();
    assert_eq!(hosts[0].ip, "100.64.0.5");
}

#[test]
fn test_parse_cli_peer_ipv6_fallback() {
    let json = r#"{
        "Peer": {
            "abc": {
                "ID": "n1",
                "HostName": "v6only",
                "TailscaleIPs": ["fd7a:115c:a1e0::1"],
                "OS": "",
                "Online": true,
                "Tags": []
            }
        }
    }"#;
    let status: CliStatus = serde_json::from_str(json).unwrap();
    let hosts = Tailscale::hosts_from_cli(status).unwrap();
    assert_eq!(hosts[0].ip, "fd7a:115c:a1e0::1");
}

#[test]
fn test_parse_cli_tags_stripped() {
    let json = r#"{
        "Peer": {
            "abc": {
                "ID": "n1",
                "HostName": "tagged",
                "TailscaleIPs": ["100.64.0.1"],
                "OS": "",
                "Online": true,
                "Tags": ["tag:server", "tag:prod", "notag"]
            }
        }
    }"#;
    let status: CliStatus = serde_json::from_str(json).unwrap();
    let hosts = Tailscale::hosts_from_cli(status).unwrap();
    assert_eq!(hosts[0].tags, vec!["server", "prod", "notag"]);
}

#[test]
fn test_parse_cli_online_null() {
    let json = r#"{
        "Peer": {
            "abc": {
                "ID": "n1",
                "HostName": "unknown-state",
                "TailscaleIPs": ["100.64.0.1"],
                "OS": "",
                "Online": null,
                "Tags": []
            }
        }
    }"#;
    let status: CliStatus = serde_json::from_str(json).unwrap();
    let hosts = Tailscale::hosts_from_cli(status).unwrap();
    assert!(
        hosts[0]
            .metadata
            .iter()
            .any(|(k, v)| k == "status" && v == "unknown")
    );
}

#[test]
fn test_parse_cli_extra_fields_ignored() {
    let json = r#"{
        "Version": "1.50.0",
        "Self": {"ID": "self1", "HostName": "my-machine"},
        "MagicDNSSuffix": "tailnet.ts.net",
        "Peer": {
            "abc": {
                "ID": "n1",
                "HostName": "remote",
                "TailscaleIPs": ["100.64.0.1"],
                "OS": "linux",
                "Online": true,
                "Tags": [],
                "ExtraField": "ignored",
                "RxBytes": 12345
            }
        }
    }"#;
    let status: CliStatus = serde_json::from_str(json).unwrap();
    let hosts = Tailscale::hosts_from_cli(status).unwrap();
    assert_eq!(hosts.len(), 1);
}

// =========================================================================
// API parsing
// =========================================================================

#[test]
fn test_parse_api_response_basic() {
    let json = r#"{
        "devices": [
            {
                "nodeId": "nDEV1",
                "hostname": "api-server",
                "name": "api-server.tailnet.ts.net",
                "addresses": ["100.64.0.10", "fd7a:115c:a1e0::a"],
                "os": "linux",
                "authorized": true,
                "connectedToControl": true,
                "tags": ["tag:web"]
            }
        ]
    }"#;
    let resp: ApiResponse = serde_json::from_str(json).unwrap();
    let hosts = Tailscale::hosts_from_api(resp).unwrap();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].server_id, "nDEV1");
    assert_eq!(hosts[0].name, "api-server");
    assert_eq!(hosts[0].ip, "100.64.0.10");
    assert_eq!(hosts[0].tags, vec!["web"]);
    assert!(
        hosts[0]
            .metadata
            .iter()
            .any(|(k, v)| k == "os" && v == "linux")
    );
    assert!(
        hosts[0]
            .metadata
            .iter()
            .any(|(k, v)| k == "status" && v == "online")
    );
}

#[test]
fn test_parse_api_connected_to_control_false() {
    let json = r#"{
        "devices": [
            {
                "nodeId": "n1",
                "hostname": "offline-dev",
                "name": "offline-dev.ts.net",
                "addresses": ["100.64.0.1"],
                "os": "linux",
                "authorized": true,
                "connectedToControl": false,
                "tags": []
            }
        ]
    }"#;
    let resp: ApiResponse = serde_json::from_str(json).unwrap();
    let hosts = Tailscale::hosts_from_api(resp).unwrap();
    assert_eq!(hosts.len(), 1);
    assert!(
        hosts[0]
            .metadata
            .iter()
            .any(|(k, v)| k == "status" && v == "offline")
    );
}

#[test]
fn test_parse_api_extra_fields_ignored() {
    let json = r#"{
        "devices": [
            {
                "nodeId": "n1",
                "hostname": "full",
                "name": "full.ts.net",
                "addresses": ["100.64.0.1"],
                "os": "linux",
                "authorized": true,
                "connectedToControl": true,
                "tags": [],
                "lastSeen": "2025-01-01T00:00:00Z",
                "clientVersion": "1.50.0",
                "updateAvailable": false,
                "machineKey": "mkey:abc123",
                "nodeKey": "nodekey:xyz789",
                "user": "user@example.com",
                "keyExpiryDisabled": true,
                "isExternal": false
            }
        ]
    }"#;
    let resp: ApiResponse = serde_json::from_str(json).unwrap();
    let hosts = Tailscale::hosts_from_api(resp).unwrap();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].name, "full");
}

#[test]
fn test_parse_api_unauthorized_skipped() {
    let json = r#"{
        "devices": [
            {
                "nodeId": "n1",
                "hostname": "authorized",
                "name": "authorized.ts.net",
                "addresses": ["100.64.0.1"],
                "os": "linux",
                "authorized": true,
                "tags": []
            },
            {
                "nodeId": "n2",
                "hostname": "unauthorized",
                "name": "unauthorized.ts.net",
                "addresses": ["100.64.0.2"],
                "os": "linux",
                "authorized": false,
                "tags": []
            }
        ]
    }"#;
    let resp: ApiResponse = serde_json::from_str(json).unwrap();
    let hosts = Tailscale::hosts_from_api(resp).unwrap();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].name, "authorized");
}

#[test]
fn test_parse_api_tags_null() {
    let json = r#"{
        "devices": [
            {
                "nodeId": "n1",
                "hostname": "notags",
                "name": "notags.ts.net",
                "addresses": ["100.64.0.1"],
                "os": "linux",
                "authorized": true
            }
        ]
    }"#;
    let resp: ApiResponse = serde_json::from_str(json).unwrap();
    let hosts = Tailscale::hosts_from_api(resp).unwrap();
    assert!(hosts[0].tags.is_empty());
}

#[test]
fn test_parse_api_tags_explicit_null() {
    // Tailscale API can return "tags": null (not just missing)
    let json = r#"{
        "devices": [
            {
                "nodeId": "n1",
                "hostname": "nulltags",
                "name": "nulltags.ts.net",
                "addresses": ["100.64.0.1"],
                "os": "linux",
                "authorized": true,
                "tags": null
            }
        ]
    }"#;
    let resp: ApiResponse = serde_json::from_str(json).unwrap();
    let hosts = Tailscale::hosts_from_api(resp).unwrap();
    assert_eq!(hosts.len(), 1);
    assert!(hosts[0].tags.is_empty());
}

#[test]
fn test_parse_api_hostname_from_name() {
    let json = r#"{
        "devices": [
            {
                "nodeId": "n1",
                "hostname": "",
                "name": "my-server.tailnet.ts.net",
                "addresses": ["100.64.0.1"],
                "os": "linux",
                "authorized": true,
                "tags": []
            }
        ]
    }"#;
    let resp: ApiResponse = serde_json::from_str(json).unwrap();
    let hosts = Tailscale::hosts_from_api(resp).unwrap();
    assert_eq!(hosts[0].name, "my-server");
}

#[test]
fn test_parse_cli_multiple_peers() {
    // Keys intentionally in reverse alphabetical order to verify sort
    let json = r#"{
        "Peer": {
            "zzz": {
                "ID": "n1",
                "HostName": "server-z",
                "TailscaleIPs": ["100.64.0.1"],
                "OS": "linux",
                "Online": true,
                "Tags": []
            },
            "aaa": {
                "ID": "n2",
                "HostName": "server-a",
                "TailscaleIPs": ["100.64.0.2"],
                "OS": "darwin",
                "Online": false,
                "Tags": ["tag:dev"]
            }
        }
    }"#;
    let status: CliStatus = serde_json::from_str(json).unwrap();
    let hosts = Tailscale::hosts_from_cli(status).unwrap();
    assert_eq!(hosts.len(), 2);
    // Sorted by peer key: "aaa" before "zzz"
    assert_eq!(hosts[0].name, "server-a");
    assert_eq!(hosts[1].name, "server-z");
}

#[test]
fn test_parse_cli_offline_peer_included() {
    let json = r#"{
        "Peer": {
            "abc": {
                "ID": "n1",
                "HostName": "offline-host",
                "TailscaleIPs": ["100.64.0.1"],
                "OS": "linux",
                "Online": false,
                "Tags": []
            }
        }
    }"#;
    let status: CliStatus = serde_json::from_str(json).unwrap();
    let hosts = Tailscale::hosts_from_cli(status).unwrap();
    assert_eq!(hosts.len(), 1);
    assert!(
        hosts[0]
            .metadata
            .iter()
            .any(|(k, v)| k == "status" && v == "offline")
    );
}

#[test]
fn test_parse_api_device_no_addresses_skipped() {
    let json = r#"{
        "devices": [
            {
                "nodeId": "n1",
                "hostname": "no-addr",
                "name": "no-addr.ts.net",
                "addresses": [],
                "os": "linux",
                "authorized": true,
                "tags": []
            }
        ]
    }"#;
    let resp: ApiResponse = serde_json::from_str(json).unwrap();
    let hosts = Tailscale::hosts_from_api(resp).unwrap();
    assert!(hosts.is_empty());
}

#[test]
fn test_parse_api_missing_authorized_defaults_true() {
    // Devices without "authorized" field should NOT be silently skipped.
    let json = r#"{
        "devices": [
            {
                "nodeId": "n1",
                "hostname": "implicit-auth",
                "name": "implicit-auth.ts.net",
                "addresses": ["100.64.0.1"],
                "os": "linux",
                "tags": []
            }
        ]
    }"#;
    let resp: ApiResponse = serde_json::from_str(json).unwrap();
    let hosts = Tailscale::hosts_from_api(resp).unwrap();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].name, "implicit-auth");
}

#[test]
fn test_parse_api_multiple_devices() {
    let json = r#"{
        "devices": [
            {
                "nodeId": "n1",
                "hostname": "web",
                "name": "web.ts.net",
                "addresses": ["100.64.0.1"],
                "os": "linux",
                "authorized": true,
                "tags": []
            },
            {
                "nodeId": "n2",
                "hostname": "db",
                "name": "db.ts.net",
                "addresses": ["100.64.0.2"],
                "os": "linux",
                "authorized": true,
                "tags": ["tag:prod"]
            }
        ]
    }"#;
    let resp: ApiResponse = serde_json::from_str(json).unwrap();
    let hosts = Tailscale::hosts_from_api(resp).unwrap();
    assert_eq!(hosts.len(), 2);
}

// =========================================================================
// Helpers
// =========================================================================

#[test]
fn test_select_ip_prefers_ipv4() {
    let ips = vec!["fd7a:115c:a1e0::1".to_string(), "100.64.0.5".to_string()];
    assert_eq!(select_ip(&ips), Some("100.64.0.5".to_string()));
}

#[test]
fn test_select_ip_ipv6_fallback() {
    let ips = vec!["fd7a:115c:a1e0::1".to_string()];
    assert_eq!(select_ip(&ips), Some("fd7a:115c:a1e0::1".to_string()));
}

#[test]
fn test_select_ip_strips_cidr() {
    let ips = vec!["100.64.0.1/32".to_string()];
    assert_eq!(select_ip(&ips), Some("100.64.0.1".to_string()));
}

#[test]
fn test_select_ip_empty() {
    let ips: Vec<String> = vec![];
    assert_eq!(select_ip(&ips), None);
}

#[test]
fn test_strip_tag_prefix() {
    assert_eq!(strip_tag_prefix("tag:server"), "server");
    assert_eq!(strip_tag_prefix("tag:prod"), "prod");
    assert_eq!(strip_tag_prefix("notag"), "notag");
    assert_eq!(strip_tag_prefix(""), "");
}

// =========================================================================
// Trait
// =========================================================================

#[test]
fn test_tailscale_name() {
    let ts = Tailscale;
    assert_eq!(ts.name(), "tailscale");
}

#[test]
fn test_tailscale_short_label() {
    let ts = Tailscale;
    assert_eq!(ts.short_label(), "ts");
}

// =========================================================================
// Token validation
// =========================================================================

#[test]
fn test_auth_key_rejected() {
    let ts = Tailscale;
    let cancel = AtomicBool::new(false);
    let result = ts.fetch_hosts_cancellable("tskey-auth-abc123", &cancel);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("device auth key"),
        "Error should mention device auth key: {}",
        err
    );
}

// =========================================================================
// HTTP roundtrip tests (mockito)
// =========================================================================

#[test]
fn test_http_devices_roundtrip_bearer() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/api/v2/tailnet/-/devices")
        .match_query(mockito::Matcher::UrlEncoded("fields".into(), "all".into()))
        .match_header("Authorization", "Bearer oauth-token-abc123")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "devices": [
                    {
                        "nodeId": "nDEV123456",
                        "hostname": "web-prod-1",
                        "name": "web-prod-1.tailnet-abc.ts.net",
                        "addresses": ["100.64.0.10", "fd7a:115c:a1e0::a"],
                        "os": "linux",
                        "authorized": true,
                        "connectedToControl": true,
                        "tags": ["tag:server", "tag:production"]
                    }
                ]
            }"#,
        )
        .create();

    let agent = super::super::http_agent();
    let url = format!("{}/api/v2/tailnet/-/devices?fields=all", server.url());
    let resp: ApiResponse = agent
        .get(&url)
        .header("Authorization", "Bearer oauth-token-abc123")
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();

    assert_eq!(resp.devices.len(), 1);
    let d = &resp.devices[0];
    assert_eq!(d.node_id, "nDEV123456");
    assert_eq!(d.hostname, "web-prod-1");
    assert_eq!(d.os, "linux");
    assert!(d.authorized);
    assert!(d.connected_to_control);
    assert_eq!(d.addresses, vec!["100.64.0.10", "fd7a:115c:a1e0::a"]);
    assert_eq!(d.tags, vec!["tag:server", "tag:production"]);

    let hosts = Tailscale::hosts_from_api(resp).unwrap();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].server_id, "nDEV123456");
    assert_eq!(hosts[0].name, "web-prod-1");
    assert_eq!(hosts[0].ip, "100.64.0.10");
    assert_eq!(hosts[0].tags, vec!["server", "production"]);
    mock.assert();
}

#[test]
fn test_http_devices_roundtrip_basic_auth() {
    let mut server = mockito::Server::new();
    let api_key = "tskey-api-kABC123-CNTRL";
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{}:", api_key));
    let expected_auth = format!("Basic {}", encoded);

    let mock = server
        .mock("GET", "/api/v2/tailnet/-/devices")
        .match_query(mockito::Matcher::UrlEncoded("fields".into(), "all".into()))
        .match_header("Authorization", expected_auth.as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "devices": [
                    {
                        "nodeId": "nBASIC789",
                        "hostname": "db-replica",
                        "name": "db-replica.ts.net",
                        "addresses": ["100.64.1.20"],
                        "os": "linux",
                        "authorized": true,
                        "connectedToControl": false,
                        "tags": []
                    }
                ]
            }"#,
        )
        .create();

    let agent = super::super::http_agent();
    let url = format!("{}/api/v2/tailnet/-/devices?fields=all", server.url());
    let resp: ApiResponse = agent
        .get(&url)
        .header("Authorization", &expected_auth)
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();

    assert_eq!(resp.devices.len(), 1);
    assert_eq!(resp.devices[0].node_id, "nBASIC789");
    assert_eq!(resp.devices[0].hostname, "db-replica");
    assert!(!resp.devices[0].connected_to_control);

    let hosts = Tailscale::hosts_from_api(resp).unwrap();
    assert_eq!(hosts[0].ip, "100.64.1.20");
    assert!(
        hosts[0]
            .metadata
            .iter()
            .any(|(k, v)| k == "status" && v == "offline")
    );
    mock.assert();
}

#[test]
fn test_http_devices_auth_failure() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/api/v2/tailnet/-/devices")
        .match_query(mockito::Matcher::Any)
        .with_status(401)
        .with_body(r#"{"message": "Unauthorized"}"#)
        .create();

    let agent = super::super::http_agent();
    let result = agent
        .get(&format!(
            "{}/api/v2/tailnet/-/devices?fields=all",
            server.url()
        ))
        .header("Authorization", "Bearer bad-token")
        .call();

    match result {
        Err(ureq::Error::StatusCode(401)) => {} // expected
        other => panic!("expected 401 error, got {:?}", other),
    }
    mock.assert();
}
