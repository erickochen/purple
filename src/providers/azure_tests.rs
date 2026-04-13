use super::*;

// =========================================================================
// Auth detection
// =========================================================================

// =========================================================================
// Subscription ID validation
// =========================================================================

#[test]
fn test_valid_subscription_id() {
    assert!(is_valid_subscription_id(
        "12345678-1234-1234-1234-123456789012"
    ));
    assert!(is_valid_subscription_id(
        "abcdef00-1234-5678-9abc-def012345678"
    ));
    assert!(is_valid_subscription_id(
        "ABCDEF00-1234-5678-9ABC-DEF012345678"
    ));
}

#[test]
fn test_invalid_subscription_id() {
    assert!(!is_valid_subscription_id(""));
    assert!(!is_valid_subscription_id("not-a-uuid"));
    assert!(!is_valid_subscription_id(
        "12345678-1234-1234-1234-12345678901"
    )); // too short last segment
    assert!(!is_valid_subscription_id(
        "12345678-1234-1234-1234-1234567890123"
    )); // too long
    assert!(!is_valid_subscription_id(
        "1234567g-1234-1234-1234-123456789012"
    )); // 'g' not hex
    assert!(!is_valid_subscription_id(
        "12345678123412341234123456789012"
    )); // no dashes
}

#[test]
fn test_azure_rejects_invalid_subscription_id() {
    let az = Azure {
        subscriptions: vec!["not-a-uuid".to_string()],
    };
    let result = az.fetch_hosts("fake-token");
    match result {
        Err(ProviderError::Http(msg)) => assert!(msg.contains("Invalid subscription ID")),
        other => panic!("Expected Http error, got: {:?}", other),
    }
}

// =========================================================================
// Auth detection
// =========================================================================

#[test]
fn test_is_sp_file() {
    assert!(is_sp_file("/path/to/sp.json"));
    assert!(is_sp_file("sp.JSON"));
    assert!(is_sp_file("credentials.Json"));
    assert!(!is_sp_file("some-access-token"));
    assert!(!is_sp_file(""));
}

#[test]
fn test_resolve_token_strips_bearer_prefix() {
    let result = resolve_token("Bearer eyJtoken").unwrap();
    assert_eq!(result, "eyJtoken");
}

#[test]
fn test_resolve_token_no_bearer_prefix() {
    let result = resolve_token("eyJtoken").unwrap();
    assert_eq!(result, "eyJtoken");
}

#[test]
fn test_resolve_token_bearer_only_rejects_empty() {
    let result = resolve_token("Bearer ");
    assert!(matches!(result, Err(ProviderError::AuthFailed)));
}

#[test]
fn test_parse_service_principal_camel_case() {
    let json = r#"{"tenantId":"t","clientId":"c","clientSecret":"s"}"#;
    let sp: ServicePrincipal = serde_json::from_str(json).unwrap();
    assert_eq!(sp.tenant_id, "t");
    assert_eq!(sp.client_id, "c");
    assert_eq!(sp.client_secret, "s");
}

#[test]
fn test_parse_service_principal_az_cli_format() {
    // Output format of `az ad sp create-for-rbac`
    let json = r#"{"appId":"a","password":"p","tenant":"t"}"#;
    let sp: ServicePrincipal = serde_json::from_str(json).unwrap();
    assert_eq!(sp.tenant_id, "t");
    assert_eq!(sp.client_id, "a");
    assert_eq!(sp.client_secret, "p");
}

#[test]
fn test_resolve_sp_token_file_not_found() {
    let result = resolve_sp_token("/nonexistent/path/sp.json");
    assert!(matches!(result, Err(ProviderError::Http(msg)) if msg.contains("Failed to read")));
}

#[test]
fn test_sp_file_missing_fields() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "purple_test_sp_incomplete_{}.json",
        std::process::id()
    ));
    std::fs::write(&path, r#"{"tenantId":"t"}"#).unwrap();
    let result = resolve_sp_token(path.to_str().unwrap());
    std::fs::remove_file(&path).ok();
    match result {
        Err(ProviderError::Http(msg)) => {
            assert!(msg.contains("appId/password/tenant"), "got: {}", msg);
        }
        other => panic!("Expected Http error, got: {:?}", other),
    }
}

#[test]
fn test_sp_file_invalid_json() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "purple_test_sp_invalid_{}.json",
        std::process::id()
    ));
    std::fs::write(&path, "not json at all").unwrap();
    let result = resolve_sp_token(path.to_str().unwrap());
    std::fs::remove_file(&path).ok();
    assert!(matches!(result, Err(ProviderError::Http(msg)) if msg.contains("Failed to parse")));
}

// =========================================================================
// VM response parsing
// =========================================================================

#[test]
fn test_parse_vm_list_response() {
    let json = r#"{
        "value": [
            {
                "name": "web-01",
                "location": "eastus",
                "tags": {"env": "prod"},
                "properties": {
                    "vmId": "abc-123",
                    "hardwareProfile": {"vmSize": "Standard_B1s"},
                    "storageProfile": {
                        "imageReference": {
                            "offer": "UbuntuServer",
                            "sku": "22_04-lts"
                        }
                    },
                    "networkProfile": {
                        "networkInterfaces": [
                            {"id": "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Network/networkInterfaces/nic1"}
                        ]
                    },
                    "instanceView": {
                        "statuses": [
                            {"code": "ProvisioningState/succeeded"},
                            {"code": "PowerState/running"}
                        ]
                    }
                }
            }
        ]
    }"#;
    let resp: VmListResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.value.len(), 1);
    assert_eq!(resp.value[0].name, "web-01");
    assert_eq!(resp.value[0].properties.vm_id, "abc-123");
}

#[test]
fn test_parse_vm_list_with_next_link() {
    let json = r#"{"value": [], "nextLink": "https://management.azure.com/next?page=2"}"#;
    let resp: VmListResponse = serde_json::from_str(json).unwrap();
    assert_eq!(
        resp.next_link.as_deref(),
        Some("https://management.azure.com/next?page=2")
    );
}

#[test]
fn test_parse_vm_list_no_next_link() {
    let json = r#"{"value": []}"#;
    let resp: VmListResponse = serde_json::from_str(json).unwrap();
    assert!(resp.next_link.is_none());
}

// =========================================================================
// NIC response parsing
// =========================================================================

#[test]
fn test_parse_nic_response() {
    let json = r#"{
        "value": [
            {
                "id": "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Network/networkInterfaces/nic1",
                "properties": {
                    "ipConfigurations": [
                        {
                            "properties": {
                                "privateIPAddress": "10.0.0.4",
                                "publicIPAddress": {
                                    "id": "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Network/publicIPAddresses/pip1"
                                },
                                "primary": true
                            }
                        }
                    ]
                }
            }
        ]
    }"#;
    let resp: NicListResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.value.len(), 1);
    assert_eq!(
        resp.value[0].properties.ip_configurations[0]
            .properties
            .private_ip_address,
        Some("10.0.0.4".to_string())
    );
}

// =========================================================================
// Public IP response parsing
// =========================================================================

#[test]
fn test_parse_public_ip_response() {
    let json = r#"{
        "value": [
            {
                "id": "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Network/publicIPAddresses/pip1",
                "properties": {"ipAddress": "52.168.1.1"}
            }
        ]
    }"#;
    let resp: PublicIpListResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.value.len(), 1);
    assert_eq!(
        resp.value[0].properties.ip_address,
        Some("52.168.1.1".to_string())
    );
}

// =========================================================================
// IP selection
// =========================================================================

fn make_vm(nic_ids: Vec<(&str, bool)>) -> VirtualMachine {
    VirtualMachine {
        name: "test".to_string(),
        location: "eastus".to_string(),
        tags: None,
        properties: VmProperties {
            vm_id: "vm-123".to_string(),
            hardware_profile: None,
            storage_profile: None,
            network_profile: Some(NetworkProfile {
                network_interfaces: nic_ids
                    .into_iter()
                    .map(|(id, primary)| NetworkInterfaceRef {
                        id: id.to_string(),
                        properties: Some(NicRefProperties {
                            primary: Some(primary),
                        }),
                    })
                    .collect(),
            }),
            instance_view: None,
        },
    }
}

fn make_nic(id: &str, private_ip: &str, public_ip_id: Option<&str>) -> Nic {
    Nic {
        id: id.to_string(),
        properties: NicProperties {
            ip_configurations: vec![IpConfiguration {
                properties: IpConfigProperties {
                    private_ip_address: Some(private_ip.to_string()),
                    public_ip_address: public_ip_id.map(|pid| PublicIpRef {
                        id: pid.to_string(),
                    }),
                    primary: Some(true),
                },
            }],
        },
    }
}

#[test]
fn test_select_ip_prefers_public() {
    let vm = make_vm(vec![("/nic1", true)]);
    let nic = make_nic("/nic1", "10.0.0.4", Some("/pip1"));
    let nic_map: HashMap<String, &Nic> = [("/nic1".to_string(), &nic)].into_iter().collect();
    let pip_map: HashMap<String, String> = [("/pip1".to_string(), "52.168.1.1".to_string())]
        .into_iter()
        .collect();

    assert_eq!(
        select_ip(&vm, &nic_map, &pip_map),
        Some("52.168.1.1".to_string())
    );
}

#[test]
fn test_select_ip_falls_back_to_private() {
    let vm = make_vm(vec![("/nic1", true)]);
    let nic = make_nic("/nic1", "10.0.0.4", None);
    let nic_map: HashMap<String, &Nic> = [("/nic1".to_string(), &nic)].into_iter().collect();
    let pip_map: HashMap<String, String> = HashMap::new();

    assert_eq!(
        select_ip(&vm, &nic_map, &pip_map),
        Some("10.0.0.4".to_string())
    );
}

#[test]
fn test_select_ip_no_network_profile() {
    let vm = VirtualMachine {
        name: "test".to_string(),
        location: "eastus".to_string(),
        tags: None,
        properties: VmProperties {
            vm_id: "vm-123".to_string(),
            hardware_profile: None,
            storage_profile: None,
            network_profile: None,
            instance_view: None,
        },
    };
    let nic_map = HashMap::new();
    let pip_map = HashMap::new();
    assert_eq!(select_ip(&vm, &nic_map, &pip_map), None);
}

#[test]
fn test_select_ip_primary_nic_selection() {
    let vm = make_vm(vec![("/nic-secondary", false), ("/nic-primary", true)]);
    let nic_secondary = make_nic("/nic-secondary", "10.0.0.5", None);
    let nic_primary = make_nic("/nic-primary", "10.0.0.4", Some("/pip1"));
    let nic_map: HashMap<String, &Nic> = [
        ("/nic-secondary".to_string(), &nic_secondary),
        ("/nic-primary".to_string(), &nic_primary),
    ]
    .into_iter()
    .collect();
    let pip_map: HashMap<String, String> = [("/pip1".to_string(), "52.168.1.1".to_string())]
        .into_iter()
        .collect();

    assert_eq!(
        select_ip(&vm, &nic_map, &pip_map),
        Some("52.168.1.1".to_string())
    );
}

#[test]
fn test_select_ip_case_insensitive_ids() {
    let vm = make_vm(vec![("/Subscriptions/Sub/NIC1", true)]);
    let nic = make_nic(
        "/subscriptions/sub/nic1",
        "10.0.0.4",
        Some("/Subscriptions/Sub/PIP1"),
    );
    let nic_map: HashMap<String, &Nic> = [("/subscriptions/sub/nic1".to_string(), &nic)]
        .into_iter()
        .collect();
    let pip_map: HashMap<String, String> = [(
        "/subscriptions/sub/pip1".to_string(),
        "52.168.1.1".to_string(),
    )]
    .into_iter()
    .collect();

    assert_eq!(
        select_ip(&vm, &nic_map, &pip_map),
        Some("52.168.1.1".to_string())
    );
}

// =========================================================================
// Power state extraction
// =========================================================================

#[test]
fn test_extract_power_state_running() {
    let iv = Some(InstanceView {
        statuses: vec![
            InstanceViewStatus {
                code: "ProvisioningState/succeeded".to_string(),
            },
            InstanceViewStatus {
                code: "PowerState/running".to_string(),
            },
        ],
    });
    assert_eq!(extract_power_state(&iv), Some("running".to_string()));
}

#[test]
fn test_extract_power_state_deallocated() {
    let iv = Some(InstanceView {
        statuses: vec![InstanceViewStatus {
            code: "PowerState/deallocated".to_string(),
        }],
    });
    assert_eq!(extract_power_state(&iv), Some("deallocated".to_string()));
}

#[test]
fn test_extract_power_state_none() {
    assert_eq!(extract_power_state(&None), None);
}

#[test]
fn test_extract_power_state_no_power_status() {
    let iv = Some(InstanceView {
        statuses: vec![InstanceViewStatus {
            code: "ProvisioningState/succeeded".to_string(),
        }],
    });
    assert_eq!(extract_power_state(&iv), None);
}

// =========================================================================
// OS string
// =========================================================================

#[test]
fn test_build_os_string_full() {
    let img = Some(ImageReference {
        offer: Some("UbuntuServer".to_string()),
        sku: Some("22_04-lts".to_string()),
        id: None,
    });
    assert_eq!(
        build_os_string(&img),
        Some("UbuntuServer-22_04-lts".to_string())
    );
}

#[test]
fn test_build_os_string_custom_image() {
    let img = Some(ImageReference {
        offer: None,
        sku: None,
        id: Some("/subscriptions/sub/images/custom".to_string()),
    });
    assert_eq!(build_os_string(&img), None);
}

#[test]
fn test_build_os_string_none() {
    assert_eq!(build_os_string(&None), None);
}

// =========================================================================
// Metadata
// =========================================================================

#[test]
fn test_build_metadata_full() {
    let vm = VirtualMachine {
        name: "web-01".to_string(),
        location: "EastUS".to_string(),
        tags: None,
        properties: VmProperties {
            vm_id: "vm-123".to_string(),
            hardware_profile: Some(HardwareProfile {
                vm_size: "Standard_B1s".to_string(),
            }),
            storage_profile: Some(StorageProfile {
                image_reference: Some(ImageReference {
                    offer: Some("UbuntuServer".to_string()),
                    sku: Some("22_04-lts".to_string()),
                    id: None,
                }),
            }),
            network_profile: None,
            instance_view: Some(InstanceView {
                statuses: vec![InstanceViewStatus {
                    code: "PowerState/running".to_string(),
                }],
            }),
        },
    };
    let meta = build_metadata(&vm);
    assert_eq!(
        meta,
        vec![
            ("region".to_string(), "eastus".to_string()),
            ("vm_size".to_string(), "Standard_B1s".to_string()),
            ("image".to_string(), "UbuntuServer-22_04-lts".to_string()),
            ("status".to_string(), "running".to_string()),
        ]
    );
}

#[test]
fn test_build_metadata_empty() {
    let vm = VirtualMachine {
        name: "bare".to_string(),
        location: String::new(),
        tags: None,
        properties: VmProperties {
            vm_id: "vm-123".to_string(),
            hardware_profile: None,
            storage_profile: None,
            network_profile: None,
            instance_view: None,
        },
    };
    assert!(build_metadata(&vm).is_empty());
}

// =========================================================================
// Tags
// =========================================================================

#[test]
fn test_build_tags_from_vm_tags() {
    let vm = VirtualMachine {
        name: "test".to_string(),
        location: "eastus".to_string(),
        tags: Some(
            [
                ("env".to_string(), "prod".to_string()),
                ("team".to_string(), "".to_string()),
            ]
            .into_iter()
            .collect(),
        ),
        properties: VmProperties {
            vm_id: "vm-123".to_string(),
            hardware_profile: None,
            storage_profile: None,
            network_profile: None,
            instance_view: None,
        },
    };
    let tags = build_tags(&vm);
    assert!(tags.contains(&"env:prod".to_string()));
    assert!(tags.contains(&"team".to_string()));
}

#[test]
fn test_build_tags_empty() {
    let vm = VirtualMachine {
        name: "test".to_string(),
        location: "eastus".to_string(),
        tags: None,
        properties: VmProperties {
            vm_id: "vm-123".to_string(),
            hardware_profile: None,
            storage_profile: None,
            network_profile: None,
            instance_view: None,
        },
    };
    assert!(build_tags(&vm).is_empty());
}

// =========================================================================
// Provider trait
// =========================================================================

#[test]
fn test_select_ip_nic_not_in_map() {
    let vm = make_vm(vec![("/nic-missing", true)]);
    let nic_map: HashMap<String, &Nic> = HashMap::new();
    let pip_map: HashMap<String, String> = HashMap::new();
    assert_eq!(select_ip(&vm, &nic_map, &pip_map), None);
}

#[test]
fn test_select_ip_pip_not_in_map_falls_back_to_private() {
    let vm = make_vm(vec![("/nic1", true)]);
    let nic = make_nic("/nic1", "10.0.0.4", Some("/pip-missing"));
    let nic_map: HashMap<String, &Nic> = [("/nic1".to_string(), &nic)].into_iter().collect();
    let pip_map: HashMap<String, String> = HashMap::new();
    assert_eq!(
        select_ip(&vm, &nic_map, &pip_map),
        Some("10.0.0.4".to_string())
    );
}

#[test]
fn test_select_ip_empty_nic_list() {
    let vm = VirtualMachine {
        name: "test".to_string(),
        location: "eastus".to_string(),
        tags: None,
        properties: VmProperties {
            vm_id: "vm-123".to_string(),
            hardware_profile: None,
            storage_profile: None,
            network_profile: Some(NetworkProfile {
                network_interfaces: vec![],
            }),
            instance_view: None,
        },
    };
    let nic_map = HashMap::new();
    let pip_map = HashMap::new();
    assert_eq!(select_ip(&vm, &nic_map, &pip_map), None);
}

#[test]
fn test_select_ip_no_primary_uses_first() {
    let vm = make_vm(vec![("/nic1", false), ("/nic2", false)]);
    let nic1 = make_nic("/nic1", "10.0.0.4", None);
    let nic2 = make_nic("/nic2", "10.0.0.5", None);
    let nic_map: HashMap<String, &Nic> =
        [("/nic1".to_string(), &nic1), ("/nic2".to_string(), &nic2)]
            .into_iter()
            .collect();
    let pip_map: HashMap<String, String> = HashMap::new();
    // No primary NIC, should fall back to first
    assert_eq!(
        select_ip(&vm, &nic_map, &pip_map),
        Some("10.0.0.4".to_string())
    );
}

#[test]
fn test_build_tags_empty_map() {
    let vm = VirtualMachine {
        name: "test".to_string(),
        location: "eastus".to_string(),
        tags: Some(HashMap::new()),
        properties: VmProperties {
            vm_id: "vm-123".to_string(),
            hardware_profile: None,
            storage_profile: None,
            network_profile: None,
            instance_view: None,
        },
    };
    assert!(build_tags(&vm).is_empty());
}

#[test]
fn test_build_metadata_normalizes_location_case() {
    let vm = VirtualMachine {
        name: "test".to_string(),
        location: "WestEurope".to_string(),
        tags: None,
        properties: VmProperties {
            vm_id: "vm-123".to_string(),
            hardware_profile: None,
            storage_profile: None,
            network_profile: None,
            instance_view: None,
        },
    };
    let meta = build_metadata(&vm);
    assert_eq!(meta[0], ("region".to_string(), "westeurope".to_string()));
}

#[test]
fn test_nextlink_domain_validation() {
    // Verify that a nextLink pointing to a different domain is rejected
    let body: serde_json::Value = serde_json::json!({
        "value": [],
        "nextLink": "https://evil.com/steal-token"
    });
    let next = body
        .get("nextLink")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .filter(|s| s.starts_with("https://management.azure.com/"))
        .map(|s| s.to_string());
    assert!(next.is_none());
}

#[test]
fn test_nextlink_valid_azure_url() {
    let body: serde_json::Value = serde_json::json!({
        "value": [],
        "nextLink": "https://management.azure.com/subscriptions/sub/providers/Microsoft.Compute/virtualMachines?$skiptoken=abc"
    });
    let next = body
        .get("nextLink")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .filter(|s| s.starts_with("https://management.azure.com/"))
        .map(|s| s.to_string());
    assert!(next.is_some());
}

// =========================================================================
// Empty/null response edge cases
// =========================================================================

#[test]
fn test_vm_with_empty_vmid_is_detected() {
    // VMs without a vmId have empty string from Default, which should be
    // skipped in fetch_subscription to prevent sync engine collisions.
    let vm = VirtualMachine {
        name: "test".to_string(),
        location: "eastus".to_string(),
        tags: None,
        properties: VmProperties::default(),
    };
    assert!(
        vm.properties.vm_id.is_empty(),
        "Default VmProperties should have empty vm_id"
    );
    // The actual skip happens in fetch_subscription:
    // if vm.properties.vm_id.is_empty() { continue; }
    // We verify the condition that triggers it.
}

#[test]
fn test_vm_with_valid_vmid_is_not_empty() {
    let json = r#"{"value": [{"name": "web", "properties": {"vmId": "abc-123"}}]}"#;
    let resp: VmListResponse = serde_json::from_str(json).unwrap();
    assert!(!resp.value[0].properties.vm_id.is_empty());
}

#[test]
fn test_parse_empty_vm_list() {
    let json = r#"{"value": []}"#;
    let resp: VmListResponse = serde_json::from_str(json).unwrap();
    assert!(resp.value.is_empty());
    assert!(resp.next_link.is_none());
}

#[test]
fn test_parse_vm_with_null_properties() {
    // VmProperties defaults to empty when null
    let json = r#"{"value": [{"name": "test"}]}"#;
    let resp: VmListResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.value.len(), 1);
    assert_eq!(resp.value[0].properties.vm_id, "");
}

#[test]
fn test_parse_nic_with_empty_ip_configs() {
    let json = r#"{
        "value": [{
            "id": "/nic1",
            "properties": {"ipConfigurations": []}
        }]
    }"#;
    let resp: NicListResponse = serde_json::from_str(json).unwrap();
    assert!(resp.value[0].properties.ip_configurations.is_empty());
}

#[test]
fn test_parse_public_ip_with_no_address() {
    let json = r#"{
        "value": [{
            "id": "/pip1",
            "properties": {}
        }]
    }"#;
    let resp: PublicIpListResponse = serde_json::from_str(json).unwrap();
    assert!(resp.value[0].properties.ip_address.is_none());
}

#[test]
fn test_select_ip_pip_empty_address_falls_back_to_private() {
    let vm = make_vm(vec![("/nic1", true)]);
    let nic = make_nic("/nic1", "10.0.0.4", Some("/pip1"));
    let nic_map: HashMap<String, &Nic> = [("/nic1".to_string(), &nic)].into_iter().collect();
    // Public IP exists in map but has empty address
    let pip_map: HashMap<String, String> =
        [("/pip1".to_string(), String::new())].into_iter().collect();
    assert_eq!(
        select_ip(&vm, &nic_map, &pip_map),
        Some("10.0.0.4".to_string())
    );
}

#[test]
fn test_select_ip_nic_with_no_ip_configs() {
    let vm = make_vm(vec![("/nic1", true)]);
    let nic = Nic {
        id: "/nic1".to_string(),
        properties: NicProperties {
            ip_configurations: vec![],
        },
    };
    let nic_map: HashMap<String, &Nic> = [("/nic1".to_string(), &nic)].into_iter().collect();
    let pip_map: HashMap<String, String> = HashMap::new();
    assert_eq!(select_ip(&vm, &nic_map, &pip_map), None);
}

#[test]
fn test_select_ip_ip_config_no_primary_uses_first() {
    let vm = make_vm(vec![("/nic1", true)]);
    let nic = Nic {
        id: "/nic1".to_string(),
        properties: NicProperties {
            ip_configurations: vec![
                IpConfiguration {
                    properties: IpConfigProperties {
                        private_ip_address: Some("10.0.0.4".to_string()),
                        public_ip_address: None,
                        primary: Some(false),
                    },
                },
                IpConfiguration {
                    properties: IpConfigProperties {
                        private_ip_address: Some("10.0.0.5".to_string()),
                        public_ip_address: None,
                        primary: Some(false),
                    },
                },
            ],
        },
    };
    let nic_map: HashMap<String, &Nic> = [("/nic1".to_string(), &nic)].into_iter().collect();
    let pip_map: HashMap<String, String> = HashMap::new();
    // No primary IP config, should fall back to first
    assert_eq!(
        select_ip(&vm, &nic_map, &pip_map),
        Some("10.0.0.4".to_string())
    );
}

// =========================================================================
// Provider trait
// =========================================================================

#[test]
fn test_azure_provider_name() {
    let az = Azure {
        subscriptions: vec![],
    };
    assert_eq!(az.name(), "azure");
    assert_eq!(az.short_label(), "az");
}

#[test]
fn test_azure_no_subscriptions_error() {
    let az = Azure {
        subscriptions: vec![],
    };
    let result = az.fetch_hosts("fake-token");
    match result {
        Err(ProviderError::Http(msg)) => assert!(msg.contains("No Azure subscriptions")),
        other => panic!("Expected Http error, got: {:?}", other),
    }
}

// =========================================================================
// Token response deserialization (simulates read_json for OAuth2 exchange)
// =========================================================================

#[test]
fn test_azure_token_response_deserialize() {
    let json =
        r#"{"access_token": "eyJ0eXAi.abc.def", "token_type": "Bearer", "expires_in": 3599}"#;
    let resp: TokenResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.access_token, "eyJ0eXAi.abc.def");
}

#[test]
fn test_azure_token_response_missing_token_fails() {
    let json = r#"{"token_type": "Bearer", "expires_in": 3599}"#;
    assert!(serde_json::from_str::<TokenResponse>(json).is_err());
}

// =========================================================================
// ureq v3 send_form API pattern test
// =========================================================================

#[test]
fn test_send_form_array_syntax_with_owned_strings() {
    // Verify the v3 send_form([...]) syntax with .as_str() on owned Strings
    let client_id = "app-id-123".to_string();
    let client_secret = "secret-456".to_string();
    let form_data: [(&str, &str); 4] = [
        ("grant_type", "client_credentials"),
        ("client_id", client_id.as_str()),
        ("client_secret", client_secret.as_str()),
        ("scope", "https://management.azure.com/.default"),
    ];
    assert_eq!(form_data.len(), 4);
    assert_eq!(form_data[1].1, "app-id-123");
    assert_eq!(form_data[2].1, "secret-456");
}

// =========================================================================
// HTTP roundtrip tests (mockito)
// =========================================================================

#[test]
fn test_http_oauth2_token_roundtrip() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/test-tenant/oauth2/v2.0/token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"access_token": "test-token-abc", "token_type": "Bearer", "expires_in": 3600}"#,
        )
        .create();

    let agent = super::super::http_agent();
    let url = format!("{}/test-tenant/oauth2/v2.0/token", server.url());
    let mut resp = agent
        .post(&url)
        .send_form([
            ("grant_type", "client_credentials"),
            ("client_id", "app-id"),
            ("client_secret", "secret"),
            ("scope", "https://management.azure.com/.default"),
        ])
        .unwrap();
    let token_resp: TokenResponse = resp.body_mut().read_json().unwrap();

    assert_eq!(token_resp.access_token, "test-token-abc");
    mock.assert();
}

#[test]
fn test_http_oauth2_token_failure() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/test-tenant/oauth2/v2.0/token")
        .with_status(401)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error": "invalid_client"}"#)
        .create();

    let agent = super::super::http_agent();
    let url = format!("{}/test-tenant/oauth2/v2.0/token", server.url());
    let result = agent.post(&url).send_form([
        ("grant_type", "client_credentials"),
        ("client_id", "bad-id"),
        ("client_secret", "bad-secret"),
        ("scope", "https://management.azure.com/.default"),
    ]);

    assert!(result.is_err());
    mock.assert();
}

#[test]
fn test_http_list_vms_roundtrip() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock(
            "GET",
            "/subscriptions/sub-123/providers/Microsoft.Compute/virtualMachines",
        )
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("api-version".into(), "2024-07-01".into()),
            mockito::Matcher::UrlEncoded("$expand".into(), "instanceView".into()),
        ]))
        .match_header("Authorization", "Bearer test-token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "value": [
                    {
                        "name": "web-01",
                        "location": "eastus",
                        "tags": {"env": "prod"},
                        "properties": {
                            "vmId": "abc-123",
                            "hardwareProfile": {"vmSize": "Standard_B1s"},
                            "storageProfile": {
                                "imageReference": {
                                    "offer": "UbuntuServer",
                                    "sku": "22_04-lts"
                                }
                            },
                            "networkProfile": {
                                "networkInterfaces": [
                                    {"id": "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Network/networkInterfaces/nic1"}
                                ]
                            },
                            "instanceView": {
                                "statuses": [
                                    {"code": "ProvisioningState/succeeded"},
                                    {"code": "PowerState/running"}
                                ]
                            }
                        }
                    }
                ],
                "nextLink": null
            }"#,
        )
        .create();

    let agent = super::super::http_agent();
    let url = format!(
        "{}/subscriptions/sub-123/providers/Microsoft.Compute/virtualMachines?api-version=2024-07-01&$expand=instanceView",
        server.url()
    );
    let resp: VmListResponse = agent
        .get(&url)
        .header("Authorization", "Bearer test-token")
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();

    assert_eq!(resp.value.len(), 1);
    let vm = &resp.value[0];
    assert_eq!(vm.name, "web-01");
    assert_eq!(vm.location, "eastus");
    assert_eq!(vm.properties.vm_id, "abc-123");
    assert_eq!(
        vm.properties.hardware_profile.as_ref().unwrap().vm_size,
        "Standard_B1s"
    );
    assert_eq!(
        vm.properties
            .storage_profile
            .as_ref()
            .unwrap()
            .image_reference
            .as_ref()
            .unwrap()
            .offer
            .as_deref(),
        Some("UbuntuServer")
    );
    assert!(resp.next_link.is_none());
    mock.assert();
}

#[test]
fn test_http_list_vms_pagination() {
    let mut server = mockito::Server::new();
    let page1 = server
        .mock(
            "GET",
            "/subscriptions/sub-123/providers/Microsoft.Compute/virtualMachines",
        )
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("api-version".into(), "2024-07-01".into()),
            mockito::Matcher::UrlEncoded("$expand".into(), "instanceView".into()),
        ]))
        .match_header("Authorization", "Bearer tk")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "value": [{"name": "vm-a", "properties": {"vmId": "id-a"}}],
                "nextLink": "NEXT_URL_PLACEHOLDER"
            }"#,
        )
        .create();

    let page2 = server
        .mock(
            "GET",
            "/subscriptions/sub-123/providers/Microsoft.Compute/virtualMachines",
        )
        .match_query(mockito::Matcher::AllOf(vec![mockito::Matcher::UrlEncoded(
            "page".into(),
            "2".into(),
        )]))
        .match_header("Authorization", "Bearer tk")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "value": [{"name": "vm-b", "properties": {"vmId": "id-b"}}]
            }"#,
        )
        .create();

    let agent = super::super::http_agent();
    // Page 1
    let r1: VmListResponse = agent
        .get(&format!(
            "{}/subscriptions/sub-123/providers/Microsoft.Compute/virtualMachines?api-version=2024-07-01&$expand=instanceView",
            server.url()
        ))
        .header("Authorization", "Bearer tk")
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();
    assert_eq!(r1.value.len(), 1);
    assert_eq!(r1.value[0].name, "vm-a");
    assert!(r1.next_link.is_some());

    // Page 2
    let r2: VmListResponse = agent
        .get(&format!(
            "{}/subscriptions/sub-123/providers/Microsoft.Compute/virtualMachines?page=2",
            server.url()
        ))
        .header("Authorization", "Bearer tk")
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();
    assert_eq!(r2.value.len(), 1);
    assert_eq!(r2.value[0].name, "vm-b");
    assert!(r2.next_link.is_none());

    page1.assert();
    page2.assert();
}

#[test]
fn test_http_list_nics_roundtrip() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock(
            "GET",
            "/subscriptions/sub-123/providers/Microsoft.Network/networkInterfaces",
        )
        .match_query(mockito::Matcher::UrlEncoded(
            "api-version".into(),
            "2024-05-01".into(),
        ))
        .match_header("Authorization", "Bearer test-token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "value": [
                    {
                        "id": "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Network/networkInterfaces/nic1",
                        "properties": {
                            "ipConfigurations": [
                                {
                                    "properties": {
                                        "privateIPAddress": "10.0.0.4",
                                        "publicIPAddress": {
                                            "id": "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Network/publicIPAddresses/pip1"
                                        },
                                        "primary": true
                                    }
                                }
                            ]
                        }
                    }
                ]
            }"#,
        )
        .create();

    let agent = super::super::http_agent();
    let url = format!(
        "{}/subscriptions/sub-123/providers/Microsoft.Network/networkInterfaces?api-version=2024-05-01",
        server.url()
    );
    let resp: NicListResponse = agent
        .get(&url)
        .header("Authorization", "Bearer test-token")
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();

    assert_eq!(resp.value.len(), 1);
    let nic = &resp.value[0];
    assert!(nic.id.contains("nic1"));
    let ip_config = &nic.properties.ip_configurations[0];
    assert_eq!(
        ip_config.properties.private_ip_address,
        Some("10.0.0.4".to_string())
    );
    assert!(ip_config.properties.public_ip_address.is_some());
    assert_eq!(ip_config.properties.primary, Some(true));
    mock.assert();
}

#[test]
fn test_http_list_public_ips_roundtrip() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock(
            "GET",
            "/subscriptions/sub-123/providers/Microsoft.Network/publicIPAddresses",
        )
        .match_query(mockito::Matcher::UrlEncoded(
            "api-version".into(),
            "2024-05-01".into(),
        ))
        .match_header("Authorization", "Bearer test-token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "value": [
                    {
                        "id": "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Network/publicIPAddresses/pip1",
                        "properties": {"ipAddress": "52.168.1.1"}
                    },
                    {
                        "id": "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Network/publicIPAddresses/pip2",
                        "properties": {"ipAddress": "52.168.1.2"}
                    }
                ]
            }"#,
        )
        .create();

    let agent = super::super::http_agent();
    let url = format!(
        "{}/subscriptions/sub-123/providers/Microsoft.Network/publicIPAddresses?api-version=2024-05-01",
        server.url()
    );
    let resp: PublicIpListResponse = agent
        .get(&url)
        .header("Authorization", "Bearer test-token")
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();

    assert_eq!(resp.value.len(), 2);
    assert_eq!(
        resp.value[0].properties.ip_address,
        Some("52.168.1.1".to_string())
    );
    assert_eq!(
        resp.value[1].properties.ip_address,
        Some("52.168.1.2".to_string())
    );
    mock.assert();
}
