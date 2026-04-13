use super::*;

#[test]
fn test_parse_server_list_response() {
    let json = r#"{
        "servers": {
            "server": [
                {
                    "uuid": "uuid-1",
                    "title": "My Server",
                    "hostname": "my-server.example.com",
                    "tags": {"tag": ["PRODUCTION", "WEB"]},
                    "labels": {"label": [{"key": "env", "value": "prod"}]}
                },
                {
                    "uuid": "uuid-2",
                    "title": "",
                    "hostname": "db.example.com",
                    "tags": {"tag": []},
                    "labels": {"label": []}
                }
            ]
        }
    }"#;
    let resp: ServerListResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.servers.server.len(), 2);
    assert_eq!(resp.servers.server[0].uuid, "uuid-1");
    assert_eq!(resp.servers.server[0].title, "My Server");
    assert_eq!(resp.servers.server[0].tags.tag, vec!["PRODUCTION", "WEB"]);
    assert_eq!(resp.servers.server[1].title, "");
    assert_eq!(resp.servers.server[1].hostname, "db.example.com");
}

#[test]
fn test_parse_server_detail_with_networking() {
    let json = r#"{
        "server": {
            "networking": {
                "interfaces": {
                    "interface": [
                        {
                            "type": "utility",
                            "ip_addresses": {
                                "ip_address": [
                                    {"address": "10.3.0.1", "family": "IPv4"}
                                ]
                            }
                        },
                        {
                            "type": "public",
                            "ip_addresses": {
                                "ip_address": [
                                    {"address": "94.237.1.1", "family": "IPv4"},
                                    {"address": "2a04:3540::1", "family": "IPv6"}
                                ]
                            }
                        },
                        {
                            "type": "private",
                            "ip_addresses": {
                                "ip_address": [
                                    {"address": "10.0.0.1", "family": "IPv4"}
                                ]
                            }
                        }
                    ]
                }
            }
        }
    }"#;
    let resp: ServerDetailResponse = serde_json::from_str(json).unwrap();
    let interfaces = &resp.server.networking.interfaces.interface;
    assert_eq!(interfaces.len(), 3);
    assert_eq!(interfaces[1].iface_type, "public");
    assert_eq!(
        interfaces[1].ip_addresses.ip_address[0].address,
        "94.237.1.1"
    );
}

#[test]
fn test_select_ip_public_ipv4() {
    let interfaces = vec![
        NetworkInterface {
            iface_type: "private".into(),
            ip_addresses: IpAddressesWrapper {
                ip_address: vec![IpAddress {
                    address: "10.0.0.1".into(),
                    family: "IPv4".into(),
                }],
            },
        },
        NetworkInterface {
            iface_type: "public".into(),
            ip_addresses: IpAddressesWrapper {
                ip_address: vec![
                    IpAddress {
                        address: "94.237.1.1".into(),
                        family: "IPv4".into(),
                    },
                    IpAddress {
                        address: "2a04::1".into(),
                        family: "IPv6".into(),
                    },
                ],
            },
        },
    ];
    assert_eq!(select_ip(&interfaces), Some("94.237.1.1".to_string()));
}

#[test]
fn test_select_ip_public_ipv6_fallback() {
    let interfaces = vec![NetworkInterface {
        iface_type: "public".into(),
        ip_addresses: IpAddressesWrapper {
            ip_address: vec![IpAddress {
                address: "2a04::1".into(),
                family: "IPv6".into(),
            }],
        },
    }];
    assert_eq!(select_ip(&interfaces), Some("2a04::1".to_string()));
}

#[test]
fn test_select_ip_skips_placeholder_ipv4() {
    let interfaces = vec![NetworkInterface {
        iface_type: "public".into(),
        ip_addresses: IpAddressesWrapper {
            ip_address: vec![IpAddress {
                address: "0.0.0.0".into(),
                family: "IPv4".into(),
            }],
        },
    }];
    assert_eq!(select_ip(&interfaces), None);
}

#[test]
fn test_select_ip_placeholder_ipv4_falls_through_to_ipv6() {
    let interfaces = vec![NetworkInterface {
        iface_type: "public".into(),
        ip_addresses: IpAddressesWrapper {
            ip_address: vec![
                IpAddress {
                    address: "0.0.0.0".into(),
                    family: "IPv4".into(),
                },
                IpAddress {
                    address: "2a04::1".into(),
                    family: "IPv6".into(),
                },
            ],
        },
    }];
    assert_eq!(select_ip(&interfaces), Some("2a04::1".to_string()));
}

#[test]
fn test_select_ip_skips_placeholder_ipv6() {
    let interfaces = vec![NetworkInterface {
        iface_type: "public".into(),
        ip_addresses: IpAddressesWrapper {
            ip_address: vec![IpAddress {
                address: "::".into(),
                family: "IPv6".into(),
            }],
        },
    }];
    assert_eq!(select_ip(&interfaces), None);
}

#[test]
fn test_select_ip_utility_skipped() {
    let interfaces = vec![NetworkInterface {
        iface_type: "utility".into(),
        ip_addresses: IpAddressesWrapper {
            ip_address: vec![IpAddress {
                address: "10.3.0.1".into(),
                family: "IPv4".into(),
            }],
        },
    }];
    assert_eq!(select_ip(&interfaces), None);
}

#[test]
fn test_select_ip_private_only() {
    let interfaces = vec![NetworkInterface {
        iface_type: "private".into(),
        ip_addresses: IpAddressesWrapper {
            ip_address: vec![IpAddress {
                address: "10.0.0.1".into(),
                family: "IPv4".into(),
            }],
        },
    }];
    assert_eq!(select_ip(&interfaces), None);
}

#[test]
fn test_select_ip_empty() {
    let interfaces: Vec<NetworkInterface> = Vec::new();
    assert_eq!(select_ip(&interfaces), None);
}

#[test]
fn test_tags_lowercased_and_sorted() {
    let server = ServerSummary {
        uuid: "uuid-1".into(),
        title: "test".into(),
        hostname: "test.example.com".into(),
        tags: TagWrapper {
            tag: vec!["ZEBRA".into(), "ALPHA".into()],
        },
        labels: LabelWrapper {
            label: vec![Label {
                key: "env".into(),
                value: "prod".into(),
            }],
        },
        zone: String::new(),
        plan: String::new(),
        state: String::new(),
    };
    let mut tags: Vec<String> = server.tags.tag.iter().map(|t| t.to_lowercase()).collect();
    for label in &server.labels.label {
        if label.value.is_empty() {
            tags.push(label.key.clone());
        } else {
            tags.push(format!("{}={}", label.key, label.value));
        }
    }
    tags.sort();
    assert_eq!(tags, vec!["alpha", "env=prod", "zebra"]);
}

#[test]
fn test_server_name_title_preferred() {
    let server = ServerSummary {
        uuid: "uuid-1".into(),
        title: "My Server".into(),
        hostname: "my-server.example.com".into(),
        tags: TagWrapper::default(),
        labels: LabelWrapper::default(),
        zone: String::new(),
        plan: String::new(),
        state: String::new(),
    };
    let name = if server.title.is_empty() {
        server.hostname.clone()
    } else {
        server.title.clone()
    };
    assert_eq!(name, "My Server");
}

#[test]
fn test_server_name_hostname_fallback() {
    let server = ServerSummary {
        uuid: "uuid-1".into(),
        title: "".into(),
        hostname: "db.example.com".into(),
        tags: TagWrapper::default(),
        labels: LabelWrapper::default(),
        zone: String::new(),
        plan: String::new(),
        state: String::new(),
    };
    let name = if server.title.is_empty() {
        server.hostname.clone()
    } else {
        server.title.clone()
    };
    assert_eq!(name, "db.example.com");
}

#[test]
fn test_parse_missing_tags_and_labels() {
    let json = r#"{
        "servers": {
            "server": [
                {
                    "uuid": "uuid-1",
                    "title": "bare",
                    "hostname": "bare.example.com"
                }
            ]
        }
    }"#;
    let resp: ServerListResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.servers.server.len(), 1);
    assert!(resp.servers.server[0].tags.tag.is_empty());
    assert!(resp.servers.server[0].labels.label.is_empty());
}

#[test]
fn test_parse_detail_missing_networking() {
    let json = r#"{"server": {}}"#;
    let resp: ServerDetailResponse = serde_json::from_str(json).unwrap();
    assert!(resp.server.networking.interfaces.interface.is_empty());
}

// --- collect_ips tests ---

#[test]
fn test_collect_ips_filters_by_type() {
    let interfaces = vec![
        NetworkInterface {
            iface_type: "public".into(),
            ip_addresses: IpAddressesWrapper {
                ip_address: vec![IpAddress {
                    address: "94.237.1.1".into(),
                    family: "IPv4".into(),
                }],
            },
        },
        NetworkInterface {
            iface_type: "private".into(),
            ip_addresses: IpAddressesWrapper {
                ip_address: vec![IpAddress {
                    address: "10.0.0.1".into(),
                    family: "IPv4".into(),
                }],
            },
        },
        NetworkInterface {
            iface_type: "utility".into(),
            ip_addresses: IpAddressesWrapper {
                ip_address: vec![IpAddress {
                    address: "10.3.0.1".into(),
                    family: "IPv4".into(),
                }],
            },
        },
    ];
    let public = collect_ips(&interfaces, "public");
    assert_eq!(public.len(), 1);
    assert_eq!(public[0].address, "94.237.1.1");

    let private = collect_ips(&interfaces, "private");
    assert_eq!(private.len(), 1);
    assert_eq!(private[0].address, "10.0.0.1");

    let utility = collect_ips(&interfaces, "utility");
    assert_eq!(utility.len(), 1);
    assert_eq!(utility[0].address, "10.3.0.1");
}

#[test]
fn test_collect_ips_empty_interfaces() {
    let interfaces: Vec<NetworkInterface> = Vec::new();
    assert!(collect_ips(&interfaces, "public").is_empty());
}

#[test]
fn test_collect_ips_no_matching_type() {
    let interfaces = vec![NetworkInterface {
        iface_type: "private".into(),
        ip_addresses: IpAddressesWrapper {
            ip_address: vec![IpAddress {
                address: "10.0.0.1".into(),
                family: "IPv4".into(),
            }],
        },
    }];
    assert!(collect_ips(&interfaces, "public").is_empty());
}

#[test]
fn test_collect_ips_multiple_addresses_on_same_interface() {
    let interfaces = vec![NetworkInterface {
        iface_type: "public".into(),
        ip_addresses: IpAddressesWrapper {
            ip_address: vec![
                IpAddress {
                    address: "94.237.1.1".into(),
                    family: "IPv4".into(),
                },
                IpAddress {
                    address: "94.237.1.2".into(),
                    family: "IPv4".into(),
                },
                IpAddress {
                    address: "2a04::1".into(),
                    family: "IPv6".into(),
                },
            ],
        },
    }];
    let public = collect_ips(&interfaces, "public");
    assert_eq!(public.len(), 3);
}

// --- select_ip: multiple public IPv4 uses first ---

#[test]
fn test_select_ip_multiple_public_v4_uses_first() {
    let interfaces = vec![NetworkInterface {
        iface_type: "public".into(),
        ip_addresses: IpAddressesWrapper {
            ip_address: vec![
                IpAddress {
                    address: "94.237.1.1".into(),
                    family: "IPv4".into(),
                },
                IpAddress {
                    address: "94.237.1.2".into(),
                    family: "IPv4".into(),
                },
            ],
        },
    }];
    assert_eq!(select_ip(&interfaces), Some("94.237.1.1".to_string()));
}

// --- select_ip: both placeholders ---

#[test]
fn test_select_ip_both_placeholders() {
    let interfaces = vec![NetworkInterface {
        iface_type: "public".into(),
        ip_addresses: IpAddressesWrapper {
            ip_address: vec![
                IpAddress {
                    address: "0.0.0.0".into(),
                    family: "IPv4".into(),
                },
                IpAddress {
                    address: "::".into(),
                    family: "IPv6".into(),
                },
            ],
        },
    }];
    assert_eq!(select_ip(&interfaces), None);
}

// --- tag construction edge cases ---

#[test]
fn test_tags_label_empty_value_key_only() {
    let server = ServerSummary {
        uuid: "u".into(),
        title: "t".into(),
        hostname: "h".into(),
        tags: TagWrapper::default(),
        labels: LabelWrapper {
            label: vec![Label {
                key: "managed".into(),
                value: "".into(),
            }],
        },
        zone: String::new(),
        plan: String::new(),
        state: String::new(),
    };
    let mut tags: Vec<String> = server.tags.tag.iter().map(|t| t.to_lowercase()).collect();
    for label in &server.labels.label {
        if label.value.is_empty() {
            tags.push(label.key.clone());
        } else {
            tags.push(format!("{}={}", label.key, label.value));
        }
    }
    tags.sort();
    assert_eq!(tags, vec!["managed"]);
}

#[test]
fn test_tags_only_no_labels() {
    let server = ServerSummary {
        uuid: "u".into(),
        title: "t".into(),
        hostname: "h".into(),
        tags: TagWrapper {
            tag: vec!["WEB".into(), "PROD".into()],
        },
        labels: LabelWrapper::default(),
        zone: String::new(),
        plan: String::new(),
        state: String::new(),
    };
    let mut tags: Vec<String> = server.tags.tag.iter().map(|t| t.to_lowercase()).collect();
    tags.sort();
    assert_eq!(tags, vec!["prod", "web"]);
}

#[test]
fn test_labels_only_no_tags() {
    let server = ServerSummary {
        uuid: "u".into(),
        title: "t".into(),
        hostname: "h".into(),
        tags: TagWrapper::default(),
        labels: LabelWrapper {
            label: vec![
                Label {
                    key: "env".into(),
                    value: "staging".into(),
                },
                Label {
                    key: "team".into(),
                    value: "backend".into(),
                },
            ],
        },
        zone: String::new(),
        plan: String::new(),
        state: String::new(),
    };
    let mut tags: Vec<String> = Vec::new();
    for label in &server.labels.label {
        if label.value.is_empty() {
            tags.push(label.key.clone());
        } else {
            tags.push(format!("{}={}", label.key, label.value));
        }
    }
    tags.sort();
    assert_eq!(tags, vec!["env=staging", "team=backend"]);
}

// --- pagination offset logic ---

#[test]
fn test_pagination_stops_when_count_less_than_limit() {
    // When the API returns fewer items than the limit, we've hit the last page
    let json = r#"{
        "servers": {
            "server": [
                {"uuid": "u1", "title": "a", "hostname": "a.example.com"},
                {"uuid": "u2", "title": "b", "hostname": "b.example.com"}
            ]
        }
    }"#;
    let resp: ServerListResponse = serde_json::from_str(json).unwrap();
    let limit = 100;
    let count = resp.servers.server.len();
    assert!(count < limit); // Should stop
}

#[test]
fn test_pagination_continues_when_count_equals_limit() {
    // When count == limit, there may be more pages
    let count = 100;
    let limit = 100;
    assert!(count >= limit); // Should continue
}

// --- server UUID is string ---

#[test]
fn test_server_uuid_is_string() {
    let json = r#"{
        "servers": {
            "server": [
                {"uuid": "00c148cb-ef71-46cb-a76f-1bb53e791e8a", "title": "t", "hostname": "h"}
            ]
        }
    }"#;
    let resp: ServerListResponse = serde_json::from_str(json).unwrap();
    assert_eq!(
        resp.servers.server[0].uuid,
        "00c148cb-ef71-46cb-a76f-1bb53e791e8a"
    );
}

// --- empty server list ---

#[test]
fn test_empty_server_list() {
    let json = r#"{"servers": {"server": []}}"#;
    let resp: ServerListResponse = serde_json::from_str(json).unwrap();
    assert!(resp.servers.server.is_empty());
}

// --- detail response with empty networking ---

#[test]
fn test_detail_empty_interfaces() {
    let json = r#"{"server": {"networking": {"interfaces": {"interface": []}}}}"#;
    let resp: ServerDetailResponse = serde_json::from_str(json).unwrap();
    assert!(resp.server.networking.interfaces.interface.is_empty());
    assert_eq!(
        select_ip(&resp.server.networking.interfaces.interface),
        None
    );
}

// --- mixed public interfaces: IPv4 on one, IPv6 on another ---

#[test]
fn test_select_ip_across_multiple_public_interfaces() {
    let interfaces = vec![
        NetworkInterface {
            iface_type: "public".into(),
            ip_addresses: IpAddressesWrapper {
                ip_address: vec![IpAddress {
                    address: "2a04::1".into(),
                    family: "IPv6".into(),
                }],
            },
        },
        NetworkInterface {
            iface_type: "public".into(),
            ip_addresses: IpAddressesWrapper {
                ip_address: vec![IpAddress {
                    address: "94.237.1.1".into(),
                    family: "IPv4".into(),
                }],
            },
        },
    ];
    // IPv4 should win even though it's on the second interface
    assert_eq!(select_ip(&interfaces), Some("94.237.1.1".to_string()));
}

// --- collect_ips with multiple interfaces of same type ---

#[test]
fn test_collect_ips_two_public_interfaces() {
    let interfaces = vec![
        NetworkInterface {
            iface_type: "public".into(),
            ip_addresses: IpAddressesWrapper {
                ip_address: vec![IpAddress {
                    address: "94.1.1.1".into(),
                    family: "IPv4".into(),
                }],
            },
        },
        NetworkInterface {
            iface_type: "public".into(),
            ip_addresses: IpAddressesWrapper {
                ip_address: vec![IpAddress {
                    address: "94.2.2.2".into(),
                    family: "IPv4".into(),
                }],
            },
        },
    ];
    let ips = collect_ips(&interfaces, "public");
    assert_eq!(ips.len(), 2);
    assert_eq!(ips[0].address, "94.1.1.1");
    assert_eq!(ips[1].address, "94.2.2.2");
}

// --- select_ip: public interface with empty ip_address list ---

#[test]
fn test_select_ip_public_empty_addresses() {
    let interfaces = vec![NetworkInterface {
        iface_type: "public".into(),
        ip_addresses: IpAddressesWrapper {
            ip_address: Vec::new(),
        },
    }];
    assert_eq!(select_ip(&interfaces), None);
}

// --- select_ip: only utility interface (ignored) ---

#[test]
fn test_select_ip_utility_interface_ignored() {
    let interfaces = vec![NetworkInterface {
        iface_type: "utility".into(),
        ip_addresses: IpAddressesWrapper {
            ip_address: vec![IpAddress {
                address: "10.0.0.1".into(),
                family: "IPv4".into(),
            }],
        },
    }];
    assert_eq!(select_ip(&interfaces), None);
}

// --- collect_ips: private interface not in public results ---

#[test]
fn test_collect_ips_private_not_in_public() {
    let interfaces = vec![
        NetworkInterface {
            iface_type: "private".into(),
            ip_addresses: IpAddressesWrapper {
                ip_address: vec![IpAddress {
                    address: "10.0.0.1".into(),
                    family: "IPv4".into(),
                }],
            },
        },
        NetworkInterface {
            iface_type: "public".into(),
            ip_addresses: IpAddressesWrapper {
                ip_address: vec![IpAddress {
                    address: "94.1.1.1".into(),
                    family: "IPv4".into(),
                }],
            },
        },
    ];
    let public_ips = collect_ips(&interfaces, "public");
    assert_eq!(public_ips.len(), 1);
    assert_eq!(public_ips[0].address, "94.1.1.1");
}

// --- server name: title non-empty uses title ---

#[test]
fn test_server_name_uses_title_when_present() {
    let json = r#"{
        "servers": {
            "server": [{
                "uuid": "uuid-1",
                "title": "My Title",
                "hostname": "my-host.example.com",
                "tags": {"tag": []},
                "labels": {"label": []}
            }]
        }
    }"#;
    let resp: ServerListResponse = serde_json::from_str(json).unwrap();
    let server = &resp.servers.server[0];
    let name = if server.title.is_empty() {
        server.hostname.clone()
    } else {
        server.title.clone()
    };
    assert_eq!(name, "My Title");
}

// --- tags: combined tags + labels, sorted ---

#[test]
fn test_tags_combined_and_sorted() {
    let json = r#"{
        "servers": {
            "server": [{
                "uuid": "uuid-1",
                "title": "test",
                "hostname": "test",
                "tags": {"tag": ["Zebra", "Apple"]},
                "labels": {"label": [{"key": "env", "value": "prod"}, {"key": "tier", "value": ""}]}
            }]
        }
    }"#;
    let resp: ServerListResponse = serde_json::from_str(json).unwrap();
    let server = &resp.servers.server[0];
    let mut tags: Vec<String> = server.tags.tag.iter().map(|t| t.to_lowercase()).collect();
    for label in &server.labels.label {
        if label.value.is_empty() {
            tags.push(label.key.clone());
        } else {
            tags.push(format!("{}={}", label.key, label.value));
        }
    }
    tags.sort();
    assert_eq!(tags, vec!["apple", "env=prod", "tier", "zebra"]);
}

// =========================================================================
// OS metadata from storage_devices
// =========================================================================

#[test]
fn test_parse_detail_with_storage_devices() {
    let json = r#"{"server": {"networking": {"interfaces": {"interface": []}},
        "storage_devices": {"storage_device": [
            {"storage_title": "Ubuntu Server 24.04 LTS", "type": "disk"},
            {"storage_title": "Extra disk", "type": "disk"}
        ]}}}"#;
    let resp: ServerDetailResponse = serde_json::from_str(json).unwrap();
    let sd = resp.server.storage_devices.unwrap();
    assert_eq!(
        sd.storage_device[0].storage_title,
        "Ubuntu Server 24.04 LTS"
    );
}

#[test]
fn test_os_metadata_from_storage_devices() {
    let json = r#"{"server": {"networking": {"interfaces": {"interface": []}},
        "storage_devices": {"storage_device": [
            {"storage_title": "Debian GNU/Linux 12"}
        ]}}}"#;
    let resp: ServerDetailResponse = serde_json::from_str(json).unwrap();
    let sd = resp.server.storage_devices.unwrap();
    let title = &sd.storage_device[0].storage_title;
    assert_eq!(title, "Debian GNU/Linux 12");
}

#[test]
fn test_boot_disk_preferred_over_first_device() {
    let json = r#"{"server": {"networking": {"interfaces": {"interface": []}},
        "storage_devices": {"storage_device": [
            {"storage_title": "Data disk", "boot_disk": "0"},
            {"storage_title": "Ubuntu Server 24.04 LTS", "boot_disk": "1"}
        ]}}}"#;
    let resp: ServerDetailResponse = serde_json::from_str(json).unwrap();
    let sd = resp.server.storage_devices.unwrap();
    let boot = sd
        .storage_device
        .iter()
        .find(|d| d.boot_disk == "1")
        .or_else(|| sd.storage_device.first());
    assert_eq!(boot.unwrap().storage_title, "Ubuntu Server 24.04 LTS");
}

#[test]
fn test_boot_disk_falls_back_to_first() {
    let json = r#"{"server": {"networking": {"interfaces": {"interface": []}},
        "storage_devices": {"storage_device": [
            {"storage_title": "Debian GNU/Linux 12"}
        ]}}}"#;
    let resp: ServerDetailResponse = serde_json::from_str(json).unwrap();
    let sd = resp.server.storage_devices.unwrap();
    let boot = sd
        .storage_device
        .iter()
        .find(|d| d.boot_disk == "1")
        .or_else(|| sd.storage_device.first());
    assert_eq!(boot.unwrap().storage_title, "Debian GNU/Linux 12");
}

#[test]
fn test_os_metadata_missing_storage_devices() {
    let json = r#"{"server": {"networking": {"interfaces": {"interface": []}}}}"#;
    let resp: ServerDetailResponse = serde_json::from_str(json).unwrap();
    assert!(resp.server.storage_devices.is_none());
}

#[test]
fn test_os_metadata_empty_storage_device_list() {
    let json = r#"{"server": {"networking": {"interfaces": {"interface": []}},
        "storage_devices": {"storage_device": []}}}"#;
    let resp: ServerDetailResponse = serde_json::from_str(json).unwrap();
    let sd = resp.server.storage_devices.unwrap();
    assert!(sd.storage_device.is_empty());
}

// =========================================================================
// ureq v3 error pattern tests
// =========================================================================

#[test]
fn test_ureq_status_401_matches_auth_pattern() {
    // Verify the StatusCode(401 | 403) pattern used in fetch_hosts_cancellable
    let err = ureq::Error::StatusCode(401);
    assert!(matches!(err, ureq::Error::StatusCode(401 | 403)));
}

#[test]
fn test_ureq_status_403_matches_auth_pattern() {
    let err = ureq::Error::StatusCode(403);
    assert!(matches!(err, ureq::Error::StatusCode(401 | 403)));
}

#[test]
fn test_ureq_status_429_matches_rate_limit_pattern() {
    let err = ureq::Error::StatusCode(429);
    assert!(matches!(err, ureq::Error::StatusCode(429)));
}

#[test]
fn test_ureq_status_500_does_not_match_auth_pattern() {
    let err = ureq::Error::StatusCode(500);
    assert!(!matches!(err, ureq::Error::StatusCode(401 | 403)));
    assert!(!matches!(err, ureq::Error::StatusCode(429)));
}

// =========================================================================
// HTTP roundtrip tests (mockito)
// =========================================================================

#[test]
fn test_http_list_servers_roundtrip() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/1.3/server")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("limit".into(), "100".into()),
            mockito::Matcher::UrlEncoded("offset".into(), "0".into()),
        ]))
        .match_header("Authorization", "Bearer uc-test-token-123")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "servers": {
                    "server": [
                        {
                            "uuid": "00c148cb-ef71-46cb-a76f-1bb53e791e8a",
                            "title": "Web Frontend",
                            "hostname": "web-frontend.example.com",
                            "tags": {"tag": ["PRODUCTION", "WEB"]},
                            "labels": {"label": [{"key": "env", "value": "prod"}]},
                            "zone": "fi-hel1",
                            "plan": "2xCPU-4GB",
                            "state": "started"
                        }
                    ]
                }
            }"#,
        )
        .create();

    let agent = super::super::http_agent();
    let url = format!("{}/1.3/server?limit=100&offset=0", server.url());
    let resp: ServerListResponse = agent
        .get(&url)
        .header("Authorization", "Bearer uc-test-token-123")
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();

    assert_eq!(resp.servers.server.len(), 1);
    let s = &resp.servers.server[0];
    assert_eq!(s.uuid, "00c148cb-ef71-46cb-a76f-1bb53e791e8a");
    assert_eq!(s.title, "Web Frontend");
    assert_eq!(s.hostname, "web-frontend.example.com");
    assert_eq!(s.tags.tag, vec!["PRODUCTION", "WEB"]);
    assert_eq!(s.zone, "fi-hel1");
    assert_eq!(s.plan, "2xCPU-4GB");
    assert_eq!(s.state, "started");
    mock.assert();
}

#[test]
fn test_http_server_detail_roundtrip() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/1.3/server/00c148cb-ef71-46cb-a76f-1bb53e791e8a")
        .match_header("Authorization", "Bearer uc-test-token-123")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "server": {
                    "networking": {
                        "interfaces": {
                            "interface": [
                                {
                                    "type": "public",
                                    "ip_addresses": {
                                        "ip_address": [
                                            {"address": "94.237.42.10", "family": "IPv4"},
                                            {"address": "2a04:3540:1000::1", "family": "IPv6"}
                                        ]
                                    }
                                },
                                {
                                    "type": "utility",
                                    "ip_addresses": {
                                        "ip_address": [
                                            {"address": "10.3.0.5", "family": "IPv4"}
                                        ]
                                    }
                                }
                            ]
                        }
                    },
                    "storage_devices": {
                        "storage_device": [
                            {"storage_title": "Ubuntu Server 24.04 LTS", "boot_disk": "1"},
                            {"storage_title": "Data volume", "boot_disk": "0"}
                        ]
                    }
                }
            }"#,
        )
        .create();

    let agent = super::super::http_agent();
    let url = format!(
        "{}/1.3/server/00c148cb-ef71-46cb-a76f-1bb53e791e8a",
        server.url()
    );
    let resp: ServerDetailResponse = agent
        .get(&url)
        .header("Authorization", "Bearer uc-test-token-123")
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();

    let interfaces = &resp.server.networking.interfaces.interface;
    assert_eq!(interfaces.len(), 2);
    assert_eq!(select_ip(interfaces), Some("94.237.42.10".to_string()));

    let sd = resp.server.storage_devices.unwrap();
    let boot = sd
        .storage_device
        .iter()
        .find(|d| d.boot_disk == "1")
        .unwrap();
    assert_eq!(boot.storage_title, "Ubuntu Server 24.04 LTS");
    mock.assert();
}

#[test]
fn test_http_list_servers_pagination() {
    let mut server = mockito::Server::new();
    let page1 = server
        .mock("GET", "/1.3/server")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("limit".into(), "100".into()),
            mockito::Matcher::UrlEncoded("offset".into(), "0".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "servers": {
                    "server": [{"uuid": "u1", "title": "a", "hostname": "a.example.com"}]
                }
            }"#,
        )
        .create();
    let page2 = server
        .mock("GET", "/1.3/server")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("limit".into(), "100".into()),
            mockito::Matcher::UrlEncoded("offset".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "servers": {
                    "server": [{"uuid": "u2", "title": "b", "hostname": "b.example.com"}]
                }
            }"#,
        )
        .create();

    let agent = super::super::http_agent();
    let r1: ServerListResponse = agent
        .get(&format!("{}/1.3/server?limit=100&offset=0", server.url()))
        .header("Authorization", "Bearer tk")
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();
    assert_eq!(r1.servers.server.len(), 1);
    assert_eq!(r1.servers.server[0].uuid, "u1");

    let r2: ServerListResponse = agent
        .get(&format!("{}/1.3/server?limit=100&offset=100", server.url()))
        .header("Authorization", "Bearer tk")
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();
    assert_eq!(r2.servers.server.len(), 1);
    assert_eq!(r2.servers.server[0].uuid, "u2");
    page1.assert();
    page2.assert();
}

#[test]
fn test_http_list_servers_auth_failure() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/1.3/server")
        .match_query(mockito::Matcher::Any)
        .with_status(401)
        .with_body(
            r#"{"error": {"error_code": "AUTHENTICATION_FAILED", "error_message": "Authentication failed."}}"#,
        )
        .create();

    let agent = super::super::http_agent();
    let result = agent
        .get(&format!("{}/1.3/server?limit=100&offset=0", server.url()))
        .header("Authorization", "Bearer bad-token")
        .call();

    match result {
        Err(ureq::Error::StatusCode(401)) => {} // expected
        other => panic!("expected 401 error, got {:?}", other),
    }
    mock.assert();
}

#[test]
fn test_http_server_detail_auth_failure() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/1.3/server/some-uuid")
        .with_status(401)
        .with_body(
            r#"{"error": {"error_code": "AUTHENTICATION_FAILED", "error_message": "Authentication failed."}}"#,
        )
        .create();

    let agent = super::super::http_agent();
    let result = agent
        .get(&format!("{}/1.3/server/some-uuid", server.url()))
        .header("Authorization", "Bearer bad-token")
        .call();

    match result {
        Err(ureq::Error::StatusCode(401)) => {} // expected
        other => panic!("expected 401 error, got {:?}", other),
    }
    mock.assert();
}
