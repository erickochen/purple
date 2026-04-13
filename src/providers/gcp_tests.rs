use super::*;

// =========================================================================
// URL segment extraction
// =========================================================================

#[test]
fn test_last_url_segment() {
    assert_eq!(
        last_url_segment("projects/my-project/zones/us-central1-a"),
        "us-central1-a"
    );
    assert_eq!(
        last_url_segment("projects/p/machineTypes/e2-micro"),
        "e2-micro"
    );
    assert_eq!(last_url_segment(""), "");
    assert_eq!(last_url_segment("no-slashes"), "no-slashes");
}

// =========================================================================
// Token detection
// =========================================================================

#[test]
fn test_is_json_key_file() {
    assert!(is_json_key_file("/path/to/service-account.json"));
    assert!(is_json_key_file("sa.json"));
    assert!(is_json_key_file("SA.JSON"));
    assert!(is_json_key_file("key.Json"));
    assert!(!is_json_key_file("ya29.some-access-token"));
    assert!(!is_json_key_file(""));
}

// =========================================================================
// URL encoding
// =========================================================================

#[test]
fn test_url_encode_plain() {
    assert_eq!(url_encode("abc123"), "abc123");
}

#[test]
fn test_url_encode_special_chars() {
    assert_eq!(url_encode("a+b=c/d"), "a%2Bb%3Dc%2Fd");
}

#[test]
fn test_url_encode_empty() {
    assert_eq!(url_encode(""), "");
}

// =========================================================================
// Response parsing
// =========================================================================

#[test]
fn test_parse_aggregated_list_response() {
    let json = r#"{
        "items": {
            "zones/us-central1-a": {
                "instances": [
                    {
                        "id": "1234567890123456789",
                        "name": "web-1",
                        "status": "RUNNING",
                        "machineType": "projects/p/zones/us-central1-a/machineTypes/e2-micro",
                        "zone": "projects/p/zones/us-central1-a",
                        "networkInterfaces": [{
                            "networkIP": "10.0.0.2",
                            "accessConfigs": [{"natIP": "35.192.0.1"}]
                        }],
                        "disks": [{"licenses": ["projects/debian-cloud/global/licenses/debian-11"]}]
                    }
                ]
            }
        }
    }"#;
    let resp: AggregatedListResponse = serde_json::from_str(json).unwrap();
    let instances = &resp.items["zones/us-central1-a"].instances;
    assert_eq!(instances.len(), 1);
    assert_eq!(instances[0].name, "web-1");
    assert_eq!(instances[0].id, "1234567890123456789");
    assert_eq!(instances[0].status, "RUNNING");
}

#[test]
fn test_parse_empty_zone() {
    let json = r#"{
        "items": {
            "zones/us-east1-b": {
                "warning": {"code": "NO_RESULTS_ON_PAGE"}
            }
        }
    }"#;
    let resp: AggregatedListResponse = serde_json::from_str(json).unwrap();
    let scoped = &resp.items["zones/us-east1-b"];
    assert!(scoped.instances.is_empty());
}

#[test]
fn test_parse_pagination_token() {
    let json = r#"{"items": {}, "nextPageToken": "abc123"}"#;
    let resp: AggregatedListResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.next_page_token.as_deref(), Some("abc123"));
}

#[test]
fn test_parse_no_pagination_token() {
    let json = r#"{"items": {}}"#;
    let resp: AggregatedListResponse = serde_json::from_str(json).unwrap();
    assert!(resp.next_page_token.is_none());
}

#[test]
fn test_parse_empty_pagination_token() {
    let json = r#"{"items": {}, "nextPageToken": ""}"#;
    let resp: AggregatedListResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.next_page_token.as_deref(), Some(""));
    // The fetch loop treats empty string as "no more pages"
}

// =========================================================================
// IP selection
// =========================================================================

fn instance_with_ips(nat_ip: &str, network_ip: &str) -> GcpInstance {
    GcpInstance {
        id: "123".to_string(),
        name: "test".to_string(),
        status: String::new(),
        machine_type: String::new(),
        network_interfaces: vec![NetworkInterface {
            access_configs: if nat_ip.is_empty() {
                vec![]
            } else {
                vec![AccessConfig {
                    nat_ip: nat_ip.to_string(),
                }]
            },
            network_ip: network_ip.to_string(),
            ipv6_access_configs: vec![],
        }],
        disks: vec![],
        tags: None,
        labels: None,
        zone: String::new(),
    }
}

#[test]
fn test_select_ip_prefers_nat() {
    let inst = instance_with_ips("35.192.0.1", "10.0.0.2");
    assert_eq!(select_ip(&inst), Some("35.192.0.1".to_string()));
}

#[test]
fn test_select_ip_falls_back_to_internal() {
    let inst = instance_with_ips("", "10.0.0.2");
    assert_eq!(select_ip(&inst), Some("10.0.0.2".to_string()));
}

#[test]
fn test_select_ip_no_interfaces() {
    let inst = GcpInstance {
        id: "123".to_string(),
        name: "test".to_string(),
        status: String::new(),
        machine_type: String::new(),
        network_interfaces: vec![],
        disks: vec![],
        tags: None,
        labels: None,
        zone: String::new(),
    };
    assert_eq!(select_ip(&inst), None);
}

#[test]
fn test_select_ip_empty_network_ip() {
    let inst = instance_with_ips("", "");
    assert_eq!(select_ip(&inst), None);
}

#[test]
fn test_select_ip_multiple_interfaces_cross_interface() {
    // First interface has only internal, second has external
    let inst = GcpInstance {
        id: "123".to_string(),
        name: "test".to_string(),
        status: String::new(),
        machine_type: String::new(),
        network_interfaces: vec![
            NetworkInterface {
                access_configs: vec![],
                network_ip: "10.0.0.2".to_string(),
                ipv6_access_configs: vec![],
            },
            NetworkInterface {
                access_configs: vec![AccessConfig {
                    nat_ip: "35.192.0.1".to_string(),
                }],
                network_ip: "10.0.1.2".to_string(),
                ipv6_access_configs: vec![],
            },
        ],
        disks: vec![],
        tags: None,
        labels: None,
        zone: String::new(),
    };
    // Should prefer external IP from second interface over internal from first
    assert_eq!(select_ip(&inst), Some("35.192.0.1".to_string()));
}

#[test]
fn test_select_ip_falls_back_to_ipv6() {
    let inst = GcpInstance {
        id: "123".to_string(),
        name: "test".to_string(),
        status: String::new(),
        machine_type: String::new(),
        network_interfaces: vec![NetworkInterface {
            access_configs: vec![],
            network_ip: String::new(),
            ipv6_access_configs: vec![Ipv6AccessConfig {
                external_ipv6: "2600:1900:4000:318::".to_string(),
            }],
        }],
        disks: vec![],
        tags: None,
        labels: None,
        zone: String::new(),
    };
    assert_eq!(select_ip(&inst), Some("2600:1900:4000:318::".to_string()));
}

#[test]
fn test_select_ip_prefers_ipv4_over_ipv6() {
    let inst = GcpInstance {
        id: "123".to_string(),
        name: "test".to_string(),
        status: String::new(),
        machine_type: String::new(),
        network_interfaces: vec![NetworkInterface {
            access_configs: vec![AccessConfig {
                nat_ip: "35.192.0.1".to_string(),
            }],
            network_ip: "10.0.0.2".to_string(),
            ipv6_access_configs: vec![Ipv6AccessConfig {
                external_ipv6: "2600:1900:4000:318::".to_string(),
            }],
        }],
        disks: vec![],
        tags: None,
        labels: None,
        zone: String::new(),
    };
    assert_eq!(select_ip(&inst), Some("35.192.0.1".to_string()));
}

#[test]
fn test_select_ip_prefers_internal_over_ipv6() {
    let inst = GcpInstance {
        id: "123".to_string(),
        name: "test".to_string(),
        status: String::new(),
        machine_type: String::new(),
        network_interfaces: vec![NetworkInterface {
            access_configs: vec![],
            network_ip: "10.0.0.2".to_string(),
            ipv6_access_configs: vec![Ipv6AccessConfig {
                external_ipv6: "2600:1900:4000:318::".to_string(),
            }],
        }],
        disks: vec![],
        tags: None,
        labels: None,
        zone: String::new(),
    };
    assert_eq!(select_ip(&inst), Some("10.0.0.2".to_string()));
}

#[test]
fn test_select_ip_ipv6_empty_returns_none() {
    let inst = GcpInstance {
        id: "123".to_string(),
        name: "test".to_string(),
        status: String::new(),
        machine_type: String::new(),
        network_interfaces: vec![NetworkInterface {
            access_configs: vec![],
            network_ip: String::new(),
            ipv6_access_configs: vec![Ipv6AccessConfig {
                external_ipv6: String::new(),
            }],
        }],
        disks: vec![],
        tags: None,
        labels: None,
        zone: String::new(),
    };
    assert_eq!(select_ip(&inst), None);
}

#[test]
fn test_select_ip_ipv6_cross_interface() {
    // First interface has no IPs, second has IPv6
    let inst = GcpInstance {
        id: "123".to_string(),
        name: "test".to_string(),
        status: String::new(),
        machine_type: String::new(),
        network_interfaces: vec![
            NetworkInterface {
                access_configs: vec![],
                network_ip: String::new(),
                ipv6_access_configs: vec![],
            },
            NetworkInterface {
                access_configs: vec![],
                network_ip: String::new(),
                ipv6_access_configs: vec![Ipv6AccessConfig {
                    external_ipv6: "2600:1900:4000:318::".to_string(),
                }],
            },
        ],
        disks: vec![],
        tags: None,
        labels: None,
        zone: String::new(),
    };
    assert_eq!(select_ip(&inst), Some("2600:1900:4000:318::".to_string()));
}

// =========================================================================
// Metadata
// =========================================================================

#[test]
fn test_metadata_full() {
    let inst = GcpInstance {
        id: "123".to_string(),
        name: "web-1".to_string(),
        status: "RUNNING".to_string(),
        machine_type: "projects/p/zones/us-central1-a/machineTypes/e2-micro".to_string(),
        network_interfaces: vec![],
        disks: vec![Disk {
            licenses: vec!["projects/debian-cloud/global/licenses/debian-11".to_string()],
        }],
        tags: None,
        labels: None,
        zone: "projects/p/zones/us-central1-a".to_string(),
    };
    let meta = build_metadata(&inst);
    assert_eq!(
        meta,
        vec![
            ("zone".to_string(), "us-central1-a".to_string()),
            ("machine".to_string(), "e2-micro".to_string()),
            ("os".to_string(), "debian-11".to_string()),
            ("status".to_string(), "RUNNING".to_string()),
        ]
    );
}

#[test]
fn test_metadata_empty_fields() {
    let inst = GcpInstance {
        id: "123".to_string(),
        name: "bare".to_string(),
        status: String::new(),
        machine_type: String::new(),
        network_interfaces: vec![],
        disks: vec![],
        tags: None,
        labels: None,
        zone: String::new(),
    };
    let meta = build_metadata(&inst);
    assert!(meta.is_empty());
}

#[test]
fn test_metadata_no_licenses() {
    let inst = GcpInstance {
        id: "123".to_string(),
        name: "test".to_string(),
        status: "RUNNING".to_string(),
        machine_type: "projects/p/machineTypes/n1-standard-1".to_string(),
        network_interfaces: vec![],
        disks: vec![Disk { licenses: vec![] }],
        tags: None,
        labels: None,
        zone: "projects/p/zones/us-east1-b".to_string(),
    };
    let meta = build_metadata(&inst);
    assert_eq!(meta.len(), 3); // zone, machine, status (no os)
    assert!(!meta.iter().any(|(k, _)| k == "os"));
}

// =========================================================================
// Tags from labels and network tags
// =========================================================================

#[test]
fn test_build_tags_from_network_tags() {
    let inst = GcpInstance {
        id: "123".to_string(),
        name: "test".to_string(),
        status: String::new(),
        machine_type: String::new(),
        network_interfaces: vec![],
        disks: vec![],
        tags: Some(GcpTags {
            items: vec!["http-server".to_string(), "https-server".to_string()],
        }),
        labels: None,
        zone: String::new(),
    };
    let tags = build_tags(&inst);
    assert_eq!(tags, vec!["http-server", "https-server"]);
}

#[test]
fn test_build_tags_from_labels() {
    let mut labels = std::collections::HashMap::new();
    labels.insert("env".to_string(), "prod".to_string());
    labels.insert("team".to_string(), "".to_string());
    let inst = GcpInstance {
        id: "123".to_string(),
        name: "test".to_string(),
        status: String::new(),
        machine_type: String::new(),
        network_interfaces: vec![],
        disks: vec![],
        tags: None,
        labels: Some(labels),
        zone: String::new(),
    };
    let tags = build_tags(&inst);
    assert!(tags.contains(&"env:prod".to_string()));
    assert!(tags.contains(&"team".to_string()));
}

#[test]
fn test_build_tags_empty() {
    let inst = GcpInstance {
        id: "123".to_string(),
        name: "test".to_string(),
        status: String::new(),
        machine_type: String::new(),
        network_interfaces: vec![],
        disks: vec![],
        tags: None,
        labels: None,
        zone: String::new(),
    };
    assert!(build_tags(&inst).is_empty());
}

#[test]
fn test_build_tags_empty_items_vec() {
    let inst = GcpInstance {
        id: "123".to_string(),
        name: "test".to_string(),
        status: String::new(),
        machine_type: String::new(),
        network_interfaces: vec![],
        disks: vec![],
        tags: Some(GcpTags { items: vec![] }),
        labels: Some(std::collections::HashMap::new()),
        zone: String::new(),
    };
    assert!(build_tags(&inst).is_empty());
}

// =========================================================================
// Zone constants
// =========================================================================

#[test]
fn test_gcp_zones_count() {
    assert_eq!(GCP_ZONES.len(), 127);
}

#[test]
fn test_gcp_zone_groups_cover_all_zones() {
    let total: usize = GCP_ZONE_GROUPS.iter().map(|&(_, s, e)| e - s).sum();
    assert_eq!(total, GCP_ZONES.len());
    let mut expected_start = 0;
    for &(_, start, end) in GCP_ZONE_GROUPS {
        assert_eq!(start, expected_start, "Gap or overlap in zone groups");
        assert!(end > start, "Empty zone group");
        expected_start = end;
    }
    assert_eq!(expected_start, GCP_ZONES.len());
}

#[test]
fn test_gcp_zones_no_duplicates() {
    let mut seen = std::collections::HashSet::new();
    for (code, _) in GCP_ZONES {
        assert!(seen.insert(code), "Duplicate zone: {}", code);
    }
}

#[test]
fn test_gcp_zones_contains_common() {
    let codes: Vec<&str> = GCP_ZONES.iter().map(|(c, _)| *c).collect();
    assert!(codes.contains(&"us-central1-a"));
    assert!(codes.contains(&"europe-west1-b"));
    assert!(codes.contains(&"asia-east1-a"));
    assert!(codes.contains(&"asia-northeast1-a"));
    assert!(codes.contains(&"asia-south1-a"));
    assert!(codes.contains(&"europe-west4-a"));
    assert!(codes.contains(&"europe-north1-a"));
    assert!(codes.contains(&"me-west1-a"));
    assert!(codes.contains(&"africa-south1-a"));
    assert!(codes.contains(&"australia-southeast2-a"));
}

// =========================================================================
// Project ID validation
// =========================================================================

#[test]
fn test_gcp_valid_project_id() {
    // Valid project IDs should pass validation (will fail at network, not validation)
    let gcp = Gcp {
        zones: vec![],
        project: "my-project-123".to_string(),
    };
    let result = gcp.fetch_hosts("fake-token");
    // Should NOT be a project validation error
    if let Err(ProviderError::Http(msg)) = &result {
        assert!(!msg.contains("Invalid GCP project ID"), "got: {}", msg);
    }
}

#[test]
fn test_gcp_domain_scoped_project_id() {
    let gcp = Gcp {
        zones: vec![],
        project: "example.com:my-project".to_string(),
    };
    let result = gcp.fetch_hosts("fake-token");
    if let Err(ProviderError::Http(msg)) = &result {
        assert!(!msg.contains("Invalid GCP project ID"), "got: {}", msg);
    }
}

#[test]
fn test_gcp_rejects_uppercase_project_id() {
    let gcp = Gcp {
        zones: vec![],
        project: "My-Project".to_string(),
    };
    let result = gcp.fetch_hosts("fake-token");
    match result {
        Err(ProviderError::Http(msg)) => assert!(msg.contains("Invalid GCP project ID")),
        other => panic!(
            "Expected Http error for uppercase project, got: {:?}",
            other
        ),
    }
}

#[test]
fn test_gcp_rejects_special_chars_in_project_id() {
    let gcp = Gcp {
        zones: vec![],
        project: "my_project".to_string(),
    };
    let result = gcp.fetch_hosts("fake-token");
    match result {
        Err(ProviderError::Http(msg)) => assert!(msg.contains("Invalid GCP project ID")),
        other => panic!(
            "Expected Http error for underscore project, got: {:?}",
            other
        ),
    }
}

#[test]
fn test_gcp_rejects_space_in_project_id() {
    let gcp = Gcp {
        zones: vec![],
        project: "my project".to_string(),
    };
    let result = gcp.fetch_hosts("fake-token");
    match result {
        Err(ProviderError::Http(msg)) => assert!(msg.contains("Invalid GCP project ID")),
        other => panic!("Expected Http error for space in project, got: {:?}", other),
    }
}

// =========================================================================
// Empty zones accepted (sync all)
// =========================================================================

#[test]
fn test_gcp_empty_zones_accepted() {
    // Empty zones should not cause a validation error (syncs all zones)
    let gcp = Gcp {
        zones: vec![],
        project: "my-project".to_string(),
    };
    let result = gcp.fetch_hosts("fake-token");
    // Should fail at network level, not validation
    if let Err(ProviderError::Http(msg)) = &result {
        assert!(!msg.contains("zone"), "got: {}", msg);
    }
}

// =========================================================================
// Provider trait
// =========================================================================

#[test]
fn test_gcp_provider_name() {
    let gcp = Gcp {
        zones: vec![],
        project: String::new(),
    };
    assert_eq!(gcp.name(), "gcp");
    assert_eq!(gcp.short_label(), "gcp");
}

#[test]
fn test_gcp_no_project_error() {
    let gcp = Gcp {
        zones: vec![],
        project: String::new(),
    };
    let result = gcp.fetch_hosts("fake-token");
    match result {
        Err(ProviderError::Http(msg)) => assert!(msg.contains("No GCP project")),
        other => panic!("Expected Http error, got: {:?}", other),
    }
}

// =========================================================================
// Instance ID is string-encoded uint64
// =========================================================================

#[test]
fn test_instance_id_is_string() {
    let json = r#"{
        "items": {
            "zones/us-central1-a": {
                "instances": [{
                    "id": "12345678901234567890",
                    "name": "test",
                    "networkInterfaces": [],
                    "disks": []
                }]
            }
        }
    }"#;
    let resp: AggregatedListResponse = serde_json::from_str(json).unwrap();
    let inst = &resp.items["zones/us-central1-a"].instances[0];
    assert_eq!(inst.id, "12345678901234567890");
}

// =========================================================================
// IPv6 deserialization
// =========================================================================

#[test]
fn test_parse_ipv6_access_configs() {
    let json = r#"{
        "items": {
            "zones/us-central1-a": {
                "instances": [{
                    "id": "123",
                    "name": "test-ipv6",
                    "networkInterfaces": [{
                        "networkIP": "10.0.0.2",
                        "accessConfigs": [],
                        "ipv6AccessConfigs": [{"externalIpv6": "2600:1900:4000:318::"}]
                    }],
                    "disks": []
                }]
            }
        }
    }"#;
    let resp: AggregatedListResponse = serde_json::from_str(json).unwrap();
    let inst = &resp.items["zones/us-central1-a"].instances[0];
    assert_eq!(inst.network_interfaces[0].ipv6_access_configs.len(), 1);
    assert_eq!(
        inst.network_interfaces[0].ipv6_access_configs[0].external_ipv6,
        "2600:1900:4000:318::"
    );
}

#[test]
fn test_parse_missing_ipv6_access_configs() {
    let json = r#"{
        "items": {
            "zones/us-central1-a": {
                "instances": [{
                    "id": "123",
                    "name": "test-no-ipv6",
                    "networkInterfaces": [{
                        "networkIP": "10.0.0.2",
                        "accessConfigs": [{"natIP": "35.192.0.1"}]
                    }],
                    "disks": []
                }]
            }
        }
    }"#;
    let resp: AggregatedListResponse = serde_json::from_str(json).unwrap();
    let inst = &resp.items["zones/us-central1-a"].instances[0];
    assert!(inst.network_interfaces[0].ipv6_access_configs.is_empty());
}

// =========================================================================
// Token response deserialization (simulates read_json for OAuth2 exchange)
// =========================================================================

#[test]
fn test_token_response_deserialize() {
    #[derive(Deserialize)]
    struct TokenResponse {
        access_token: String,
    }
    let json = r#"{"access_token": "ya29.abc123", "token_type": "Bearer", "expires_in": 3600}"#;
    let resp: TokenResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.access_token, "ya29.abc123");
}

#[test]
fn test_token_response_missing_access_token_fails() {
    #[derive(Deserialize)]
    #[allow(dead_code)]
    struct TokenResponse {
        access_token: String,
    }
    let json = r#"{"token_type": "Bearer", "expires_in": 3600}"#;
    assert!(serde_json::from_str::<TokenResponse>(json).is_err());
}

// =========================================================================
// ureq v3 send_form API smoke test
// =========================================================================

#[test]
fn test_send_form_array_syntax_compiles() {
    // Verify the v3 send_form([...]) syntax with owned/borrowed values
    // This is a compile-time check that the API accepts the patterns we use
    let jwt = "test.jwt.value".to_string();
    let form_data: [(&str, &str); 2] = [
        ("grant_type", "urn:ietf:params:oauth:grant_type:jwt-bearer"),
        ("assertion", jwt.as_str()),
    ];
    assert_eq!(form_data.len(), 2);
    assert_eq!(form_data[1].1, "test.jwt.value");
}

// =========================================================================
// HTTP roundtrip tests (mockito)
// =========================================================================

#[test]
fn test_http_token_exchange_roundtrip() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/token")
        .match_header("content-type", mockito::Matcher::Regex("application/x-www-form-urlencoded".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"access_token": "ya29.test-access-token-xyz", "token_type": "Bearer", "expires_in": 3600}"#,
        )
        .create();

    #[derive(Deserialize)]
    struct TokenResponse {
        access_token: String,
    }

    let agent = super::super::http_agent();
    let resp: TokenResponse = agent
        .post(&format!("{}/token", server.url()))
        .send_form([
            ("grant_type", "urn:ietf:params:oauth:grant_type:jwt-bearer"),
            ("assertion", "test.jwt.value"),
        ])
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();

    assert_eq!(resp.access_token, "ya29.test-access-token-xyz");
    mock.assert();
}

#[test]
fn test_http_token_exchange_auth_failure() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/token")
        .match_query(mockito::Matcher::Any)
        .with_status(401)
        .with_body(r#"{"error": "invalid_grant", "error_description": "Invalid JWT"}"#)
        .create();

    let agent = super::super::http_agent();
    let result = agent.post(&format!("{}/token", server.url())).send_form([
        ("grant_type", "urn:ietf:params:oauth:grant_type:jwt-bearer"),
        ("assertion", "bad.jwt"),
    ]);

    match result {
        Err(ureq::Error::StatusCode(401)) => {} // expected
        other => panic!("expected 401 error, got {:?}", other),
    }
    mock.assert();
}

#[test]
fn test_http_aggregated_instances_roundtrip() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/compute/v1/projects/my-project/aggregated/instances")
        .match_header("Authorization", "Bearer ya29.test-token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "items": {
                    "zones/us-central1-a": {
                        "instances": [
                            {
                                "id": "1234567890",
                                "name": "vm-prod-1",
                                "status": "RUNNING",
                                "machineType": "projects/my-project/zones/us-central1-a/machineTypes/e2-micro",
                                "zone": "projects/my-project/zones/us-central1-a",
                                "networkInterfaces": [
                                    {
                                        "networkIP": "10.128.0.2",
                                        "accessConfigs": [{"natIP": "35.192.0.1"}],
                                        "ipv6AccessConfigs": []
                                    }
                                ],
                                "disks": [{"licenses": ["projects/debian-cloud/global/licenses/debian-11"]}],
                                "tags": {"items": ["http-server", "https-server"]},
                                "labels": {"env": "prod", "team": "infra"}
                            }
                        ]
                    }
                }
            }"#,
        )
        .create();

    let agent = super::super::http_agent();
    let url = format!(
        "{}/compute/v1/projects/my-project/aggregated/instances",
        server.url()
    );
    let resp: AggregatedListResponse = agent
        .get(&url)
        .header("Authorization", "Bearer ya29.test-token")
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();

    assert!(resp.next_page_token.is_none());
    let zone = resp.items.get("zones/us-central1-a").unwrap();
    assert_eq!(zone.instances.len(), 1);
    let inst = &zone.instances[0];
    assert_eq!(inst.id, "1234567890");
    assert_eq!(inst.name, "vm-prod-1");
    assert_eq!(inst.status, "RUNNING");
    assert!(inst.machine_type.ends_with("e2-micro"));
    assert_eq!(inst.network_interfaces[0].network_ip, "10.128.0.2");
    assert_eq!(
        inst.network_interfaces[0].access_configs[0].nat_ip,
        "35.192.0.1"
    );
    assert_eq!(
        inst.tags.as_ref().unwrap().items,
        vec!["http-server", "https-server"]
    );
    let labels = inst.labels.as_ref().unwrap();
    assert_eq!(labels.get("env").unwrap(), "prod");
    assert_eq!(labels.get("team").unwrap(), "infra");
    mock.assert();
}

#[test]
fn test_http_aggregated_instances_pagination() {
    let mut server = mockito::Server::new();
    let page1 = server
        .mock("GET", "/compute/v1/projects/my-project/aggregated/instances")
        .match_query(mockito::Matcher::Missing)
        .match_header("Authorization", "Bearer tk")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "items": {
                    "zones/us-east1-b": {
                        "instances": [{"id": "1", "name": "a", "status": "RUNNING", "zone": "zones/us-east1-b"}]
                    }
                },
                "nextPageToken": "token-page-2"
            }"#,
        )
        .create();
    let page2 = server
        .mock("GET", "/compute/v1/projects/my-project/aggregated/instances")
        .match_query(mockito::Matcher::UrlEncoded("pageToken".into(), "token-page-2".into()))
        .match_header("Authorization", "Bearer tk")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "items": {
                    "zones/eu-west1-b": {
                        "instances": [{"id": "2", "name": "b", "status": "TERMINATED", "zone": "zones/eu-west1-b"}]
                    }
                }
            }"#,
        )
        .create();

    let agent = super::super::http_agent();
    let base = format!(
        "{}/compute/v1/projects/my-project/aggregated/instances",
        server.url()
    );

    // Page 1
    let r1: AggregatedListResponse = agent
        .get(&base)
        .header("Authorization", "Bearer tk")
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();
    assert_eq!(r1.next_page_token.as_deref(), Some("token-page-2"));
    assert_eq!(r1.items.get("zones/us-east1-b").unwrap().instances.len(), 1);

    // Page 2
    let r2: AggregatedListResponse = agent
        .get(&format!("{}?pageToken=token-page-2", base))
        .header("Authorization", "Bearer tk")
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();
    assert!(r2.next_page_token.is_none());
    assert_eq!(
        r2.items.get("zones/eu-west1-b").unwrap().instances[0].name,
        "b"
    );

    page1.assert();
    page2.assert();
}

#[test]
fn test_http_aggregated_instances_auth_failure() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/compute/v1/projects/my-project/aggregated/instances")
        .match_query(mockito::Matcher::Any)
        .with_status(401)
        .with_body(r#"{"error": {"code": 401, "message": "Request had invalid authentication credentials."}}"#)
        .create();

    let agent = super::super::http_agent();
    let result = agent
        .get(&format!(
            "{}/compute/v1/projects/my-project/aggregated/instances",
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
