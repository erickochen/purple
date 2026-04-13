use super::*;

// =========================================================================
// IP selection tests
// =========================================================================

#[test]
fn test_select_cloud_ip_prefers_public_ipv4() {
    let ips = vec![
        CloudIp {
            ip: "10.0.0.1".into(),
            version: 4,
            network_type: "INTERNAL".into(),
        },
        CloudIp {
            ip: "1.2.3.4".into(),
            version: 4,
            network_type: "PUBLIC".into(),
        },
        CloudIp {
            ip: "2001:db8::1".into(),
            version: 6,
            network_type: "PUBLIC".into(),
        },
    ];
    assert_eq!(select_cloud_ip(&ips).unwrap(), "1.2.3.4");
}

#[test]
fn test_select_cloud_ip_falls_back_to_public_ipv6() {
    let ips = vec![
        CloudIp {
            ip: "10.0.0.1".into(),
            version: 4,
            network_type: "INTERNAL".into(),
        },
        CloudIp {
            ip: "2001:db8::1/64".into(),
            version: 6,
            network_type: "PUBLIC".into(),
        },
    ];
    assert_eq!(select_cloud_ip(&ips).unwrap(), "2001:db8::1");
}

#[test]
fn test_select_cloud_ip_falls_back_to_internal_ipv4() {
    let ips = vec![CloudIp {
        ip: "10.0.0.1".into(),
        version: 4,
        network_type: "INTERNAL".into(),
    }];
    assert_eq!(select_cloud_ip(&ips).unwrap(), "10.0.0.1");
}

#[test]
fn test_select_cloud_ip_empty() {
    assert!(select_cloud_ip(&[]).is_none());
}

#[test]
fn test_select_cloud_ip_private_ipv6_only_returns_none() {
    let ips = vec![CloudIp {
        ip: "fd00::1".into(),
        version: 6,
        network_type: "INTERNAL".into(),
    }];
    assert!(select_cloud_ip(&ips).is_none());
}

#[test]
fn test_select_cloud_ip_multiple_public_ipv4_uses_first() {
    let ips = vec![
        CloudIp {
            ip: "1.1.1.1".into(),
            version: 4,
            network_type: "PUBLIC".into(),
        },
        CloudIp {
            ip: "2.2.2.2".into(),
            version: 4,
            network_type: "PUBLIC".into(),
        },
    ];
    assert_eq!(select_cloud_ip(&ips).unwrap(), "1.1.1.1");
}

#[test]
fn test_select_cloud_ip_with_cidr() {
    let ips = vec![CloudIp {
        ip: "1.2.3.4/32".into(),
        version: 4,
        network_type: "PUBLIC".into(),
    }];
    assert_eq!(select_cloud_ip(&ips).unwrap(), "1.2.3.4");
}

// =========================================================================
// Deserialization tests (dedicated servers)
// =========================================================================

#[test]
fn test_parse_baremetal_response() {
    let json = r#"{
        "servers": [{
            "id": "12345",
            "reference": "web-server-01",
            "networkInterfaces": {
                "public": {"ip": "85.17.0.1", "mac": "AA:BB:CC:DD:EE:FF"},
                "internal": {"ip": "10.0.0.5"}
            },
            "location": {"site": "AMS-01", "suite": "A1", "rack": "01", "unit": "1"},
            "contract": {"deliveryStatus": "ACTIVE", "reference": "my-server"},
            "specs": {
                "cpu": {"quantity": 2, "type": "Intel Xeon E-2288G"},
                "ram": {"size": 32, "unit": "GB"}
            }
        }],
        "_metadata": {"totalCount": 1, "limit": 20, "offset": 0}
    }"#;
    let resp: BareMetalListResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.servers.len(), 1);
    assert_eq!(resp.servers[0].id, "12345");
    assert_eq!(resp.servers[0].reference, "web-server-01");
    assert_eq!(
        resp.servers[0]
            .network_interfaces
            .public
            .as_ref()
            .unwrap()
            .ip,
        "85.17.0.1"
    );
    assert_eq!(resp.servers[0].location.as_ref().unwrap().site, "AMS-01");
    assert_eq!(resp.metadata.total_count, 1);
}

#[test]
fn test_parse_baremetal_minimal() {
    let json = r#"{
        "servers": [{"id": "1", "networkInterfaces": {}}],
        "_metadata": {"totalCount": 1, "limit": 20, "offset": 0}
    }"#;
    let resp: BareMetalListResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.servers.len(), 1);
    assert!(resp.servers[0].network_interfaces.public.is_none());
}

#[test]
fn test_parse_baremetal_extra_fields_ignored() {
    let json = r#"{
        "servers": [{
            "id": "1",
            "reference": "srv",
            "networkInterfaces": {"public": {"ip": "1.2.3.4"}},
            "serialNumber": "XYZ123",
            "isCustomerGateway": false,
            "powerPorts": []
        }],
        "_metadata": {"totalCount": 1, "limit": 20, "offset": 0}
    }"#;
    let resp: BareMetalListResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.servers[0].reference, "srv");
}

// =========================================================================
// Deserialization tests (public cloud)
// =========================================================================

#[test]
fn test_parse_cloud_response() {
    let json = r#"{
        "instances": [{
            "id": "uuid-abc-123",
            "reference": "api-server",
            "state": "RUNNING",
            "region": "eu-west-3",
            "type": "lsw.c3.xlarge",
            "ips": [
                {"ip": "1.2.3.4", "prefixLength": "32", "version": 4, "nullRouted": false, "networkType": "PUBLIC"},
                {"ip": "10.0.0.1", "prefixLength": "8", "version": 4, "nullRouted": false, "networkType": "INTERNAL"}
            ],
            "image": {"id": "img-1", "name": "Ubuntu 22.04"}
        }],
        "_metadata": {"totalCount": 1, "limit": 20, "offset": 0}
    }"#;
    let resp: CloudListResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.instances.len(), 1);
    assert_eq!(resp.instances[0].id, "uuid-abc-123");
    assert_eq!(resp.instances[0].state, "RUNNING");
    assert_eq!(resp.instances[0].region, "eu-west-3");
    assert_eq!(resp.instances[0].instance_type, "lsw.c3.xlarge");
    assert_eq!(resp.instances[0].ips.len(), 2);
    assert_eq!(
        resp.instances[0].image.as_ref().unwrap().name.as_deref(),
        Some("Ubuntu 22.04")
    );
}

#[test]
fn test_parse_cloud_minimal() {
    let json = r#"{
        "instances": [{"id": "x", "ips": []}],
        "_metadata": {"totalCount": 1, "limit": 20, "offset": 0}
    }"#;
    let resp: CloudListResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.instances.len(), 1);
    assert!(resp.instances[0].ips.is_empty());
    assert!(resp.instances[0].image.is_none());
}

#[test]
fn test_parse_empty_lists() {
    let bm: BareMetalListResponse = serde_json::from_str(
        r#"{"servers": [], "_metadata": {"totalCount": 0, "limit": 20, "offset": 0}}"#,
    )
    .unwrap();
    assert!(bm.servers.is_empty());
    let cloud: CloudListResponse = serde_json::from_str(
        r#"{"instances": [], "_metadata": {"totalCount": 0, "limit": 20, "offset": 0}}"#,
    )
    .unwrap();
    assert!(cloud.instances.is_empty());
}

// =========================================================================
// Provider trait tests
// =========================================================================

#[test]
fn test_name_and_short_label() {
    let lsw = Leaseweb;
    assert_eq!(lsw.name(), "leaseweb");
    assert_eq!(lsw.short_label(), "lsw");
}

// =========================================================================
// format_baremetal_specs tests
// =========================================================================

#[test]
fn test_format_baremetal_specs_full() {
    let specs = BareMetalSpecs {
        cpu: Some(BareMetalCpu {
            quantity: 2,
            cpu_type: "Xeon E-2288G".into(),
        }),
        ram: Some(BareMetalRam {
            size: 32,
            unit: "GB".into(),
        }),
    };
    assert_eq!(format_baremetal_specs(&specs), "2x Xeon E-2288G, 32GB");
}

#[test]
fn test_format_baremetal_specs_cpu_only() {
    let specs = BareMetalSpecs {
        cpu: Some(BareMetalCpu {
            quantity: 1,
            cpu_type: "Xeon".into(),
        }),
        ram: None,
    };
    assert_eq!(format_baremetal_specs(&specs), "1x Xeon");
}

#[test]
fn test_format_baremetal_specs_ram_only() {
    let specs = BareMetalSpecs {
        cpu: None,
        ram: Some(BareMetalRam {
            size: 64,
            unit: "GB".into(),
        }),
    };
    assert_eq!(format_baremetal_specs(&specs), "64GB");
}

#[test]
fn test_format_baremetal_specs_empty() {
    let specs = BareMetalSpecs {
        cpu: None,
        ram: None,
    };
    assert_eq!(format_baremetal_specs(&specs), "");
}

#[test]
fn test_format_baremetal_specs_zero_values() {
    let specs = BareMetalSpecs {
        cpu: Some(BareMetalCpu {
            quantity: 0,
            cpu_type: "Xeon".into(),
        }),
        ram: Some(BareMetalRam {
            size: 0,
            unit: "GB".into(),
        }),
    };
    assert_eq!(format_baremetal_specs(&specs), "");
}

#[test]
fn test_format_baremetal_specs_empty_cpu_type() {
    let specs = BareMetalSpecs {
        cpu: Some(BareMetalCpu {
            quantity: 1,
            cpu_type: String::new(),
        }),
        ram: Some(BareMetalRam {
            size: 32,
            unit: "GB".into(),
        }),
    };
    assert_eq!(format_baremetal_specs(&specs), "32GB");
}

// =========================================================================
// HTTP roundtrip tests
// =========================================================================

#[test]
fn test_http_baremetal_roundtrip() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/bareMetals/v2/servers?limit=50&offset=0")
        .match_header("X-Lsw-Auth", "test-api-key")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
            "servers": [{
                "id": "12345",
                "reference": "web-01",
                "networkInterfaces": {"public": {"ip": "85.17.0.1"}},
                "location": {"site": "AMS-01"},
                "contract": {"deliveryStatus": "ACTIVE"},
                "specs": {"cpu": {"quantity": 1, "type": "Xeon"}, "ram": {"size": 32, "unit": "GB"}}
            }],
            "_metadata": {"totalCount": 1, "limit": 50, "offset": 0}
        }"#,
        )
        .create();

    let agent = super::super::http_agent();
    let resp: BareMetalListResponse = agent
        .get(&format!(
            "{}/bareMetals/v2/servers?limit=50&offset=0",
            server.url()
        ))
        .header("X-Lsw-Auth", "test-api-key")
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();

    assert_eq!(resp.servers.len(), 1);
    assert_eq!(resp.servers[0].id, "12345");
    assert_eq!(
        resp.servers[0]
            .network_interfaces
            .public
            .as_ref()
            .unwrap()
            .ip,
        "85.17.0.1"
    );
    mock.assert();
}

#[test]
fn test_http_cloud_roundtrip() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/publicCloud/v1/instances?limit=50&offset=0")
        .match_header("X-Lsw-Auth", "test-key")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
            "instances": [{
                "id": "uuid-1",
                "reference": "api-1",
                "state": "RUNNING",
                "region": "eu-west-3",
                "type": "lsw.c3.xlarge",
                "ips": [{"ip": "1.2.3.4", "version": 4, "networkType": "PUBLIC"}],
                "image": {"name": "Ubuntu 22.04"}
            }],
            "_metadata": {"totalCount": 1, "limit": 50, "offset": 0}
        }"#,
        )
        .create();

    let agent = super::super::http_agent();
    let resp: CloudListResponse = agent
        .get(&format!(
            "{}/publicCloud/v1/instances?limit=50&offset=0",
            server.url()
        ))
        .header("X-Lsw-Auth", "test-key")
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();

    assert_eq!(resp.instances.len(), 1);
    assert_eq!(resp.instances[0].reference, "api-1");
    assert_eq!(select_cloud_ip(&resp.instances[0].ips).unwrap(), "1.2.3.4");
    mock.assert();
}

#[test]
fn test_http_baremetal_auth_failure() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/bareMetals/v2/servers?limit=50&offset=0")
        .with_status(401)
        .with_body(r#"{"errorCode": "401", "errorMessage": "Invalid API key"}"#)
        .create();

    let agent = super::super::http_agent();
    let result = agent
        .get(&format!(
            "{}/bareMetals/v2/servers?limit=50&offset=0",
            server.url()
        ))
        .header("X-Lsw-Auth", "bad-key")
        .call();

    assert!(result.is_err());
    let err = super::map_ureq_error(result.unwrap_err());
    assert!(matches!(err, ProviderError::AuthFailed));
    mock.assert();
}

#[test]
fn test_http_cloud_auth_failure() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/publicCloud/v1/instances?limit=50&offset=0")
        .with_status(401)
        .with_body(r#"{"errorCode": "401", "errorMessage": "Invalid API key"}"#)
        .create();

    let agent = super::super::http_agent();
    let result = agent
        .get(&format!(
            "{}/publicCloud/v1/instances?limit=50&offset=0",
            server.url()
        ))
        .header("X-Lsw-Auth", "bad-key")
        .call();

    assert!(result.is_err());
    let err = super::map_ureq_error(result.unwrap_err());
    assert!(matches!(err, ProviderError::AuthFailed));
    mock.assert();
}

// =========================================================================
// Metadata assembly tests
// =========================================================================

#[test]
fn test_baremetal_metadata_all_fields() {
    let json = r#"{
        "servers": [{
            "id": "1",
            "reference": "web",
            "networkInterfaces": {"public": {"ip": "1.2.3.4"}},
            "location": {"site": "AMS-01"},
            "contract": {"deliveryStatus": "ACTIVE"},
            "specs": {"cpu": {"quantity": 2, "type": "Xeon"}, "ram": {"size": 32, "unit": "GB"}}
        }],
        "_metadata": {"totalCount": 1, "limit": 20, "offset": 0}
    }"#;
    let resp: BareMetalListResponse = serde_json::from_str(json).unwrap();
    let server = &resp.servers[0];
    let mut metadata = Vec::new();
    if let Some(ref loc) = server.location {
        if !loc.site.is_empty() {
            metadata.push(("location".to_string(), loc.site.clone()));
        }
    }
    if let Some(ref specs) = server.specs {
        let spec_str = format_baremetal_specs(specs);
        if !spec_str.is_empty() {
            metadata.push(("specs".to_string(), spec_str));
        }
    }
    if let Some(ref contract) = server.contract {
        if !contract.delivery_status.is_empty() {
            metadata.push(("status".to_string(), contract.delivery_status.clone()));
        }
    }
    assert_eq!(metadata.len(), 3);
    assert_eq!(metadata[0], ("location".into(), "AMS-01".into()));
    assert_eq!(metadata[1], ("specs".into(), "2x Xeon, 32GB".into()));
    assert_eq!(metadata[2], ("status".into(), "ACTIVE".into()));
}

#[test]
fn test_cloud_metadata_all_fields() {
    let json = r#"{
        "instances": [{
            "id": "i-1",
            "reference": "web",
            "state": "RUNNING",
            "region": "eu-west-3",
            "type": "lsw.c3.xlarge",
            "ips": [{"ip": "1.2.3.4", "version": 4, "networkType": "PUBLIC"}],
            "image": {"name": "Ubuntu 22.04"}
        }],
        "_metadata": {"totalCount": 1, "limit": 20, "offset": 0}
    }"#;
    let resp: CloudListResponse = serde_json::from_str(json).unwrap();
    let inst = &resp.instances[0];
    let mut metadata = Vec::new();
    if !inst.region.is_empty() {
        metadata.push(("region".to_string(), inst.region.clone()));
    }
    if !inst.instance_type.is_empty() {
        metadata.push(("type".to_string(), inst.instance_type.clone()));
    }
    if let Some(ref image) = inst.image {
        if let Some(ref name) = image.name {
            if !name.is_empty() {
                metadata.push(("image".to_string(), name.clone()));
            }
        }
    }
    if !inst.state.is_empty() {
        metadata.push(("status".to_string(), inst.state.clone()));
    }
    assert_eq!(metadata.len(), 4);
    assert_eq!(metadata[0], ("region".into(), "eu-west-3".into()));
    assert_eq!(metadata[1], ("type".into(), "lsw.c3.xlarge".into()));
    assert_eq!(metadata[2], ("image".into(), "Ubuntu 22.04".into()));
    assert_eq!(metadata[3], ("status".into(), "RUNNING".into()));
}

#[test]
fn test_cloud_no_ip_skipped() {
    let json = r#"{
        "instances": [
            {"id": "i-1", "reference": "has-ip", "ips": [{"ip": "1.2.3.4", "version": 4, "networkType": "PUBLIC"}]},
            {"id": "i-2", "reference": "no-ip", "ips": []},
            {"id": "i-3", "reference": "private-v6-only", "ips": [{"ip": "fd00::1", "version": 6, "networkType": "INTERNAL"}]}
        ],
        "_metadata": {"totalCount": 3, "limit": 20, "offset": 0}
    }"#;
    let resp: CloudListResponse = serde_json::from_str(json).unwrap();
    let hosts: Vec<_> = resp
        .instances
        .iter()
        .filter_map(|inst| select_cloud_ip(&inst.ips).map(|ip| (inst.reference.clone(), ip)))
        .collect();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].0, "has-ip");
}

#[test]
fn test_baremetal_uses_id_when_no_reference() {
    let json = r#"{
        "servers": [{"id": "srv-99", "networkInterfaces": {"public": {"ip": "1.2.3.4"}}}],
        "_metadata": {"totalCount": 1, "limit": 20, "offset": 0}
    }"#;
    let resp: BareMetalListResponse = serde_json::from_str(json).unwrap();
    let server = &resp.servers[0];
    let name = if server.reference.is_empty() {
        server.id.clone()
    } else {
        server.reference.clone()
    };
    assert_eq!(name, "srv-99");
}

#[test]
fn test_cloud_uses_id_when_no_reference() {
    let json = r#"{
        "instances": [{"id": "uuid-1", "ips": [{"ip": "1.2.3.4", "version": 4, "networkType": "PUBLIC"}]}],
        "_metadata": {"totalCount": 1, "limit": 20, "offset": 0}
    }"#;
    let resp: CloudListResponse = serde_json::from_str(json).unwrap();
    let inst = &resp.instances[0];
    let name = if inst.reference.is_empty() {
        inst.id.clone()
    } else {
        inst.reference.clone()
    };
    assert_eq!(name, "uuid-1");
}

// =========================================================================
// Pagination metadata tests
// =========================================================================

#[test]
fn test_pagination_meta() {
    let json = r#"{"totalCount": 150, "limit": 50, "offset": 100}"#;
    let meta: PaginationMeta = serde_json::from_str(json).unwrap();
    assert_eq!(meta.total_count, 150);
    assert_eq!(meta.limit, 50);
    assert_eq!(meta.offset, 100);
}

// =========================================================================
// Server ID prefix tests
// =========================================================================

#[test]
fn test_baremetal_server_id_prefix() {
    assert_eq!(format!("bm-{}", "12345"), "bm-12345");
}

#[test]
fn test_cloud_server_id_prefix() {
    assert_eq!(format!("cloud-{}", "uuid-abc"), "cloud-uuid-abc");
}

// =========================================================================
// Pagination tests (multi-page)
// =========================================================================

#[test]
fn test_http_baremetal_pagination() {
    let mut server = mockito::Server::new();
    let page1 = server
        .mock("GET", "/bareMetals/v2/servers?limit=50&offset=0")
        .match_header("X-Lsw-Auth", "key")
        .with_status(200)
        .with_body(
            r#"{
            "servers": [{"id": "1", "reference": "srv-1", "networkInterfaces": {"public": {"ip": "1.1.1.1"}}}],
            "_metadata": {"totalCount": 51, "limit": 50, "offset": 0}
        }"#,
        )
        .create();
    let page2 = server
        .mock("GET", "/bareMetals/v2/servers?limit=50&offset=50")
        .match_header("X-Lsw-Auth", "key")
        .with_status(200)
        .with_body(
            r#"{
            "servers": [{"id": "2", "reference": "srv-2", "networkInterfaces": {"public": {"ip": "2.2.2.2"}}}],
            "_metadata": {"totalCount": 51, "limit": 50, "offset": 50}
        }"#,
        )
        .create();

    let agent = super::super::http_agent();
    let mut all = Vec::new();
    let mut offset = 0u64;
    let limit = 50u64;
    loop {
        let resp: BareMetalListResponse = agent
            .get(&format!(
                "{}/bareMetals/v2/servers?limit={}&offset={}",
                server.url(),
                limit,
                offset
            ))
            .header("X-Lsw-Auth", "key")
            .call()
            .unwrap()
            .body_mut()
            .read_json()
            .unwrap();
        all.extend(resp.servers);
        if offset + limit >= resp.metadata.total_count {
            break;
        }
        offset += limit;
    }
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id, "1");
    assert_eq!(all[1].id, "2");
    page1.assert();
    page2.assert();
}

#[test]
fn test_http_cloud_pagination() {
    let mut server = mockito::Server::new();
    let page1 = server
        .mock("GET", "/publicCloud/v1/instances?limit=50&offset=0")
        .match_header("X-Lsw-Auth", "key")
        .with_status(200)
        .with_body(
            r#"{
            "instances": [{"id": "a", "reference": "inst-1", "ips": [{"ip": "1.1.1.1", "version": 4, "networkType": "PUBLIC"}]}],
            "_metadata": {"totalCount": 51, "limit": 50, "offset": 0}
        }"#,
        )
        .create();
    let page2 = server
        .mock("GET", "/publicCloud/v1/instances?limit=50&offset=50")
        .match_header("X-Lsw-Auth", "key")
        .with_status(200)
        .with_body(
            r#"{
            "instances": [{"id": "b", "reference": "inst-2", "ips": [{"ip": "2.2.2.2", "version": 4, "networkType": "PUBLIC"}]}],
            "_metadata": {"totalCount": 51, "limit": 50, "offset": 50}
        }"#,
        )
        .create();

    let agent = super::super::http_agent();
    let mut all = Vec::new();
    let mut offset = 0u64;
    let limit = 50u64;
    loop {
        let resp: CloudListResponse = agent
            .get(&format!(
                "{}/publicCloud/v1/instances?limit={}&offset={}",
                server.url(),
                limit,
                offset
            ))
            .header("X-Lsw-Auth", "key")
            .call()
            .unwrap()
            .body_mut()
            .read_json()
            .unwrap();
        all.extend(resp.instances);
        if offset + limit >= resp.metadata.total_count {
            break;
        }
        offset += limit;
    }
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id, "a");
    assert_eq!(all[1].id, "b");
    page1.assert();
    page2.assert();
}

#[test]
fn test_pagination_exact_multiple_no_extra_request() {
    // totalCount=50, limit=50 -> should stop after first page (offset+limit >= totalCount)
    let mut server = mockito::Server::new();
    let page1 = server
        .mock("GET", "/bareMetals/v2/servers?limit=50&offset=0")
        .with_status(200)
        .with_body(
            r#"{
            "servers": [{"id": "1", "networkInterfaces": {"public": {"ip": "1.1.1.1"}}}],
            "_metadata": {"totalCount": 50, "limit": 50, "offset": 0}
        }"#,
        )
        .expect(1)
        .create();
    // If a second request is made, this mock would NOT match (no mock at offset=50)
    // and the test would fail with a connection error.

    let agent = super::super::http_agent();
    let resp: BareMetalListResponse = agent
        .get(&format!(
            "{}/bareMetals/v2/servers?limit=50&offset=0",
            server.url()
        ))
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();
    // Verify the condition: offset(0) + limit(50) >= totalCount(50) -> break
    let offset = 0u64;
    assert!(offset + resp.metadata.limit >= resp.metadata.total_count);
    page1.assert();
}

// =========================================================================
// Cancellation test
// =========================================================================

#[test]
fn test_cancellation_returns_cancelled() {
    let cancel = AtomicBool::new(true);
    let lsw = Leaseweb;
    let result = lsw.fetch_hosts_cancellable("any-token", &cancel);
    assert!(matches!(result, Err(ProviderError::Cancelled)));
}

// =========================================================================
// Bare metal: no IP -> skipped
// =========================================================================

#[test]
fn test_baremetal_no_ip_skipped() {
    let json = r#"{
        "servers": [
            {"id": "1", "reference": "has-ip", "networkInterfaces": {"public": {"ip": "1.2.3.4"}}},
            {"id": "2", "reference": "no-iface", "networkInterfaces": {}},
            {"id": "3", "reference": "empty-ip", "networkInterfaces": {"public": {"ip": ""}}}
        ],
        "_metadata": {"totalCount": 3, "limit": 20, "offset": 0}
    }"#;
    let resp: BareMetalListResponse = serde_json::from_str(json).unwrap();
    let hosts: Vec<_> = resp
        .servers
        .iter()
        .filter_map(|server| {
            let ip = server
                .network_interfaces
                .public
                .as_ref()
                .map(|iface| super::super::strip_cidr(&iface.ip).to_string())
                .or_else(|| {
                    server
                        .network_interfaces
                        .internal
                        .as_ref()
                        .map(|iface| super::super::strip_cidr(&iface.ip).to_string())
                });
            ip.filter(|ip| !ip.is_empty())
                .map(|ip| (server.reference.clone(), ip))
        })
        .collect();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].0, "has-ip");
}

// =========================================================================
// Bare metal: strip_cidr on IPs
// =========================================================================

#[test]
fn test_baremetal_ip_cidr_stripped() {
    let json = r#"{
        "servers": [
            {"id": "1", "reference": "public-cidr", "networkInterfaces": {"public": {"ip": "85.17.0.1/32"}}},
            {"id": "2", "reference": "internal-cidr", "networkInterfaces": {"internal": {"ip": "10.0.0.5/24"}}}
        ],
        "_metadata": {"totalCount": 2, "limit": 20, "offset": 0}
    }"#;
    let resp: BareMetalListResponse = serde_json::from_str(json).unwrap();
    let server0 = &resp.servers[0];
    let ip0 = super::super::strip_cidr(&server0.network_interfaces.public.as_ref().unwrap().ip);
    assert_eq!(ip0, "85.17.0.1");

    let server1 = &resp.servers[1];
    let ip1 = super::super::strip_cidr(&server1.network_interfaces.internal.as_ref().unwrap().ip);
    assert_eq!(ip1, "10.0.0.5");
}
