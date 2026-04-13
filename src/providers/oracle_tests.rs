use super::*;

// -----------------------------------------------------------------------
// Config parsing and compartment validation
// -----------------------------------------------------------------------

fn minimal_config() -> &'static str {
    "[DEFAULT]\ntenancy=ocid1.tenancy.oc1..aaa\nuser=ocid1.user.oc1..bbb\nfingerprint=aa:bb:cc\nregion=us-ashburn-1\nkey_file=~/.oci/key.pem\n"
}

#[test]
fn test_parse_oci_config_valid() {
    let creds = parse_oci_config(minimal_config(), "PEM_CONTENT").unwrap();
    assert_eq!(creds.tenancy, "ocid1.tenancy.oc1..aaa");
    assert_eq!(creds.user, "ocid1.user.oc1..bbb");
    assert_eq!(creds.fingerprint, "aa:bb:cc");
    assert_eq!(creds.region, "us-ashburn-1");
    assert_eq!(creds.key_pem, "PEM_CONTENT");
}

#[test]
fn test_parse_oci_config_missing_tenancy() {
    let cfg = "[DEFAULT]\nuser=ocid1.user.oc1..bbb\nfingerprint=aa:bb:cc\n";
    let err = parse_oci_config(cfg, "").unwrap_err();
    assert!(err.to_string().contains("tenancy"));
}

#[test]
fn test_parse_oci_config_missing_user() {
    let cfg = "[DEFAULT]\ntenancy=ocid1.tenancy.oc1..aaa\nfingerprint=aa:bb:cc\n";
    let err = parse_oci_config(cfg, "").unwrap_err();
    assert!(err.to_string().contains("user"));
}

#[test]
fn test_parse_oci_config_missing_fingerprint() {
    let cfg = "[DEFAULT]\ntenancy=ocid1.tenancy.oc1..aaa\nuser=ocid1.user.oc1..bbb\n";
    let err = parse_oci_config(cfg, "").unwrap_err();
    assert!(err.to_string().contains("fingerprint"));
}

#[test]
fn test_parse_oci_config_no_default_profile() {
    let cfg = "[OTHER]\ntenancy=ocid1.tenancy.oc1..aaa\nuser=u\nfingerprint=f\n";
    let err = parse_oci_config(cfg, "").unwrap_err();
    assert!(err.to_string().contains("tenancy"));
}

#[test]
fn test_parse_oci_config_multiple_profiles_reads_default() {
    let cfg = "[OTHER]\ntenancy=wrong\n[DEFAULT]\ntenancy=right\nuser=u\nfingerprint=f\n";
    let creds = parse_oci_config(cfg, "").unwrap();
    assert_eq!(creds.tenancy, "right");
}

#[test]
fn test_parse_oci_config_whitespace_trimmed() {
    let cfg = "[DEFAULT]\n tenancy = ocid1.tenancy.oc1..aaa \n user = u \n fingerprint = f \n";
    let creds = parse_oci_config(cfg, "").unwrap();
    assert_eq!(creds.tenancy, "ocid1.tenancy.oc1..aaa");
    assert_eq!(creds.user, "u");
    assert_eq!(creds.fingerprint, "f");
}

#[test]
fn test_parse_oci_config_crlf() {
    let cfg = "[DEFAULT]\r\ntenancy=ocid1.tenancy.oc1..aaa\r\nuser=u\r\nfingerprint=f\r\n";
    let creds = parse_oci_config(cfg, "").unwrap();
    assert_eq!(creds.tenancy, "ocid1.tenancy.oc1..aaa");
}

#[test]
fn test_parse_oci_config_empty_file() {
    let err = parse_oci_config("", "").unwrap_err();
    assert!(err.to_string().contains("tenancy"));
}

#[test]
fn test_validate_compartment_valid() {
    assert!(validate_compartment("ocid1.compartment.oc1..aaaaaaaa1234").is_ok());
}

#[test]
fn test_validate_compartment_tenancy_accepted() {
    assert!(validate_compartment("ocid1.tenancy.oc1..aaaaaaaa1234").is_ok());
}

#[test]
fn test_validate_compartment_invalid() {
    assert!(validate_compartment("ocid1.instance.oc1..xxx").is_err());
    assert!(validate_compartment("not-an-ocid").is_err());
    assert!(validate_compartment("").is_err());
}

// -----------------------------------------------------------------------
// RFC 7231 date formatting
// -----------------------------------------------------------------------

#[test]
fn test_format_rfc7231_known_vector() {
    // 1774526400 = Thu, 26 Mar 2026 12:00:00 GMT
    assert_eq!(
        format_rfc7231(1_774_526_400),
        "Thu, 26 Mar 2026 12:00:00 GMT"
    );
}

#[test]
fn test_format_rfc7231_epoch_zero() {
    assert_eq!(format_rfc7231(0), "Thu, 01 Jan 1970 00:00:00 GMT");
}

#[test]
fn test_format_rfc7231_leap_year() {
    // 1582934400 = Sat, 29 Feb 2020 00:00:00 GMT
    assert_eq!(
        format_rfc7231(1_582_934_400),
        "Sat, 29 Feb 2020 00:00:00 GMT"
    );
}

// -----------------------------------------------------------------------
// RSA signing
// -----------------------------------------------------------------------

fn load_test_key() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/test_oci_key.pem"
    ))
    .expect("test key fixture missing")
}

fn load_test_key_pkcs1() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/test_oci_key_pkcs1.pem"
    ))
    .expect("test pkcs1 key fixture missing")
}

fn make_creds(key_pem: String) -> OciCredentials {
    OciCredentials {
        tenancy: "ocid1.tenancy.oc1..aaa".into(),
        user: "ocid1.user.oc1..bbb".into(),
        fingerprint: "aa:bb:cc:dd".into(),
        key_pem,
        region: "us-ashburn-1".into(),
    }
}

#[test]
fn test_sign_request_authorization_header_format() {
    let creds = make_creds(load_test_key());
    let rsa_key = parse_private_key(&creds.key_pem).unwrap();
    let date = "Thu, 26 Mar 2026 12:00:00 GMT";
    let result = sign_request(
        &creds,
        &rsa_key,
        date,
        "iaas.us-ashburn-1.oraclecloud.com",
        "/20160918/instances",
    )
    .unwrap();
    assert!(result.starts_with("Signature version=\"1\",keyId="));
    assert!(result.contains("algorithm=\"rsa-sha256\""));
    // Exact match on the headers field
    assert!(result.contains("headers=\"date (request-target) host\""));
    assert!(result.contains("signature=\""));
    // Verify keyId format is exactly tenancy/user/fingerprint
    let expected_key_id = format!(
        "keyId=\"{}/{}/{}\"",
        creds.tenancy, creds.user, creds.fingerprint
    );
    assert!(
        result.contains(&expected_key_id),
        "keyId mismatch: expected {} in {}",
        expected_key_id,
        result
    );
}

#[test]
fn test_sign_request_deterministic() {
    let key = load_test_key();
    let creds1 = make_creds(key.clone());
    let creds2 = make_creds(key);
    let rsa_key = parse_private_key(&creds1.key_pem).unwrap();
    let date = "Thu, 26 Mar 2026 12:00:00 GMT";
    let host = "iaas.us-ashburn-1.oraclecloud.com";
    let path = "/20160918/instances";
    let r1 = sign_request(&creds1, &rsa_key, date, host, path).unwrap();
    let r2 = sign_request(&creds2, &rsa_key, date, host, path).unwrap();
    assert_eq!(r1, r2);
}

#[test]
fn test_sign_request_different_hosts_differ() {
    let key = load_test_key();
    let creds1 = make_creds(key.clone());
    let creds2 = make_creds(key);
    let rsa_key = parse_private_key(&creds1.key_pem).unwrap();
    let date = "Thu, 26 Mar 2026 12:00:00 GMT";
    let path = "/20160918/instances";
    let r1 = sign_request(
        &creds1,
        &rsa_key,
        date,
        "iaas.us-ashburn-1.oraclecloud.com",
        path,
    )
    .unwrap();
    let r2 = sign_request(
        &creds2,
        &rsa_key,
        date,
        "iaas.us-phoenix-1.oraclecloud.com",
        path,
    )
    .unwrap();
    assert_ne!(r1, r2);
}

#[test]
fn test_parse_private_key_pkcs1() {
    let pem = load_test_key_pkcs1();
    assert!(parse_private_key(&pem).is_ok());
}

#[test]
fn test_parse_private_key_pkcs8() {
    let pem = load_test_key();
    assert!(parse_private_key(&pem).is_ok());
}

#[test]
fn test_parse_private_key_encrypted_detected() {
    let fake_encrypted = "-----BEGIN RSA PRIVATE KEY-----\nProc-Type: 4,ENCRYPTED\nDEK-Info: ...\ndata\n-----END RSA PRIVATE KEY-----";
    let err = parse_private_key(fake_encrypted).unwrap_err();
    assert!(err.to_string().to_lowercase().contains("encrypt"));
}

#[test]
fn test_parse_private_key_proc_type_encrypted() {
    // Different wording but also contains ENCRYPTED
    let pem = "-----BEGIN RSA PRIVATE KEY-----\nProc-Type: 4,ENCRYPTED\nABC\n-----END RSA PRIVATE KEY-----";
    let err = parse_private_key(pem).unwrap_err();
    assert!(err.to_string().to_lowercase().contains("encrypt"));
}

#[test]
fn test_parse_private_key_malformed() {
    let err = parse_private_key("not a pem key at all").unwrap_err();
    assert!(err.to_string().contains("Failed to parse OCI private key"));
}

// -----------------------------------------------------------------------
// extract_key_file
// -----------------------------------------------------------------------

#[test]
fn test_extract_key_file_present() {
    let cfg = "[DEFAULT]\ntenancy=t\nkey_file=~/.oci/key.pem\n";
    assert_eq!(extract_key_file(cfg).unwrap(), "~/.oci/key.pem");
}

#[test]
fn test_extract_key_file_missing() {
    let cfg = "[DEFAULT]\ntenancy=t\n";
    assert!(extract_key_file(cfg).is_err());
}

// -----------------------------------------------------------------------
// JSON deserialization
// -----------------------------------------------------------------------

#[test]
fn test_deserialize_list_instances() {
    let json = r#"[
        {
            "id": "ocid1.instance.oc1..aaa",
            "displayName": "my-server",
            "lifecycleState": "RUNNING",
            "shape": "VM.Standard2.1",
            "imageId": "ocid1.image.oc1..img",
            "freeformTags": {"env": "prod", "team": "ops"}
        }
    ]"#;
    let items: Vec<OciInstance> = serde_json::from_str(json).unwrap();
    assert_eq!(items.len(), 1);
    let inst = &items[0];
    assert_eq!(inst.id, "ocid1.instance.oc1..aaa");
    assert_eq!(inst.display_name, "my-server");
    assert_eq!(inst.shape, "VM.Standard2.1");
    let tags = inst.freeform_tags.as_ref().unwrap();
    assert_eq!(tags.get("env").map(String::as_str), Some("prod"));
    assert_eq!(tags.get("team").map(String::as_str), Some("ops"));
}

#[test]
fn test_deserialize_list_instances_empty() {
    let json = r#"[]"#;
    let items: Vec<OciInstance> = serde_json::from_str(json).unwrap();
    assert_eq!(items.len(), 0);
}

#[test]
fn test_deserialize_list_instances_null_image_id() {
    let json = r#"[
        {
            "id": "ocid1.instance.oc1..bbb",
            "displayName": "no-image",
            "lifecycleState": "STOPPED",
            "shape": "VM.Standard2.1"
        }
    ]"#;
    let items: Vec<OciInstance> = serde_json::from_str(json).unwrap();
    assert_eq!(items.len(), 1);
    assert!(items[0].image_id.is_none());
    assert!(items[0].freeform_tags.is_none());
}

#[test]
fn test_deserialize_vnic_attachment_is_primary() {
    let json = r#"[
        {
            "instanceId": "ocid1.instance.oc1..aaa",
            "vnicId": "ocid1.vnic.oc1..vvv",
            "lifecycleState": "ATTACHED",
            "isPrimary": true
        }
    ]"#;
    let items: Vec<OciVnicAttachment> = serde_json::from_str(json).unwrap();
    assert_eq!(items.len(), 1);
    let att = &items[0];
    assert_eq!(att.instance_id, "ocid1.instance.oc1..aaa");
    assert_eq!(att.vnic_id.as_deref(), Some("ocid1.vnic.oc1..vvv"));
    assert_eq!(att.lifecycle_state, "ATTACHED");
    assert_eq!(att.is_primary, Some(true));
}

#[test]
fn test_deserialize_vnic_public_and_private() {
    let json = r#"{"publicIp": "1.2.3.4", "privateIp": "10.0.0.5"}"#;
    let vnic: OciVnic = serde_json::from_str(json).unwrap();
    assert_eq!(vnic.public_ip.as_deref(), Some("1.2.3.4"));
    assert_eq!(vnic.private_ip.as_deref(), Some("10.0.0.5"));
}

#[test]
fn test_deserialize_vnic_private_only() {
    let json = r#"{"privateIp": "10.0.0.5"}"#;
    let vnic: OciVnic = serde_json::from_str(json).unwrap();
    assert!(vnic.public_ip.is_none());
    assert_eq!(vnic.private_ip.as_deref(), Some("10.0.0.5"));
}

#[test]
fn test_deserialize_image() {
    let json = r#"{"displayName": "Oracle-Linux-8.9"}"#;
    let img: OciImage = serde_json::from_str(json).unwrap();
    assert_eq!(img.display_name.as_deref(), Some("Oracle-Linux-8.9"));
}

#[test]
fn test_deserialize_error_body() {
    let json = r#"{"code": "NotAuthenticated", "message": "Missing or invalid credentials."}"#;
    let err: OciErrorBody = serde_json::from_str(json).unwrap();
    assert_eq!(err.code.as_deref(), Some("NotAuthenticated"));
    assert_eq!(
        err.message.as_deref(),
        Some("Missing or invalid credentials.")
    );
}

#[test]
fn test_deserialize_error_body_missing_fields() {
    let json = r#"{}"#;
    let err: OciErrorBody = serde_json::from_str(json).unwrap();
    assert!(err.code.is_none());
    assert!(err.message.is_none());
}

// -----------------------------------------------------------------------
// IP selection, VNIC mapping, tag extraction
// -----------------------------------------------------------------------

#[test]
fn test_select_ip_public_preferred() {
    let vnic = OciVnic {
        public_ip: Some("1.2.3.4".to_string()),
        private_ip: Some("10.0.0.1".to_string()),
    };
    assert_eq!(select_ip(&vnic), "1.2.3.4");
}

#[test]
fn test_select_ip_private_fallback() {
    let vnic = OciVnic {
        public_ip: None,
        private_ip: Some("10.0.0.1".to_string()),
    };
    assert_eq!(select_ip(&vnic), "10.0.0.1");
}

#[test]
fn test_select_ip_empty() {
    let vnic = OciVnic {
        public_ip: None,
        private_ip: None,
    };
    assert_eq!(select_ip(&vnic), "");
}

#[test]
fn test_select_primary_vnic() {
    let attachments = vec![
        OciVnicAttachment {
            instance_id: "inst-1".to_string(),
            vnic_id: Some("vnic-secondary".to_string()),
            lifecycle_state: "ATTACHED".to_string(),
            is_primary: Some(false),
        },
        OciVnicAttachment {
            instance_id: "inst-1".to_string(),
            vnic_id: Some("vnic-primary".to_string()),
            lifecycle_state: "ATTACHED".to_string(),
            is_primary: Some(true),
        },
    ];
    assert_eq!(
        select_vnic_for_instance(&attachments, "inst-1"),
        Some("vnic-primary".to_string())
    );
}

#[test]
fn test_select_vnic_no_primary_uses_first() {
    let attachments = vec![
        OciVnicAttachment {
            instance_id: "inst-1".to_string(),
            vnic_id: Some("vnic-first".to_string()),
            lifecycle_state: "ATTACHED".to_string(),
            is_primary: None,
        },
        OciVnicAttachment {
            instance_id: "inst-1".to_string(),
            vnic_id: Some("vnic-second".to_string()),
            lifecycle_state: "ATTACHED".to_string(),
            is_primary: None,
        },
    ];
    assert_eq!(
        select_vnic_for_instance(&attachments, "inst-1"),
        Some("vnic-first".to_string())
    );
}

#[test]
fn test_select_vnic_no_attachment() {
    let attachments: Vec<OciVnicAttachment> = vec![];
    assert_eq!(select_vnic_for_instance(&attachments, "inst-1"), None);
}

#[test]
fn test_select_vnic_filters_by_instance_id() {
    let attachments = vec![OciVnicAttachment {
        instance_id: "inst-other".to_string(),
        vnic_id: Some("vnic-other".to_string()),
        lifecycle_state: "ATTACHED".to_string(),
        is_primary: Some(true),
    }];
    assert_eq!(select_vnic_for_instance(&attachments, "inst-1"), None);
}

#[test]
fn test_extract_freeform_tags() {
    let mut map = std::collections::HashMap::new();
    map.insert("env".to_string(), "prod".to_string());
    map.insert("role".to_string(), "".to_string());
    map.insert("team".to_string(), "ops".to_string());
    let tags = extract_tags(&Some(map));
    // sorted
    assert_eq!(tags, vec!["env:prod", "role", "team:ops"]);
}

#[test]
fn test_extract_freeform_tags_empty() {
    let tags = extract_tags(&None);
    assert!(tags.is_empty());
}

// -----------------------------------------------------------------------
// Region constants and Provider trait
// -----------------------------------------------------------------------

#[test]
fn test_oci_regions_count() {
    assert_eq!(OCI_REGIONS.len(), 38);
}

#[test]
fn test_oci_regions_no_duplicates() {
    let mut ids: Vec<&str> = OCI_REGIONS.iter().map(|(id, _)| *id).collect();
    ids.sort_unstable();
    let before = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), before, "duplicate region IDs found");
}

#[test]
fn test_oci_region_groups_cover_all() {
    use std::collections::HashSet;
    let group_indices: HashSet<usize> = OCI_REGION_GROUPS
        .iter()
        .flat_map(|(_, s, e)| *s..*e)
        .collect();
    let all_indices: HashSet<usize> = (0..OCI_REGIONS.len()).collect();
    assert_eq!(
        group_indices, all_indices,
        "region groups must cover all region indices exactly"
    );
    for (_, start, end) in OCI_REGION_GROUPS {
        assert!(*end <= OCI_REGIONS.len());
        assert!(start < end);
    }
}

#[test]
fn test_oracle_provider_name() {
    let oracle = Oracle {
        regions: Vec::new(),
        compartment: String::new(),
    };
    assert_eq!(oracle.name(), "oracle");
    assert_eq!(oracle.short_label(), "oci");
}

#[test]
fn test_oracle_empty_compartment_error() {
    let oracle = Oracle {
        regions: Vec::new(),
        compartment: String::new(),
    };
    let cancel = AtomicBool::new(false);
    let err = oracle
        .fetch_hosts_with_progress("some_token", &cancel, &|_| {})
        .unwrap_err();
    assert!(err.to_string().contains("compartment"));
}

// -----------------------------------------------------------------------
// Additional coverage
// -----------------------------------------------------------------------

#[test]
fn test_malformed_json_instance_list() {
    let result = serde_json::from_str::<Vec<OciInstance>>("not json");
    assert!(result.is_err());
}

#[test]
fn test_parse_private_key_empty_string() {
    let err = parse_private_key("").unwrap_err();
    assert!(
        err.to_string().contains("Failed to parse OCI private key"),
        "got: {}",
        err
    );
}

#[test]
fn test_parse_oci_config_missing_region_defaults_empty() {
    let cfg = "[DEFAULT]\ntenancy=ocid1.tenancy.oc1..aaa\nuser=u\nfingerprint=f\n";
    let creds = parse_oci_config(cfg, "").unwrap();
    assert_eq!(creds.region, "");
}

#[test]
fn test_sign_request_headers_exact() {
    let creds = make_creds(load_test_key());
    let rsa_key = parse_private_key(&creds.key_pem).unwrap();
    let date = "Thu, 26 Mar 2026 12:00:00 GMT";
    let result = sign_request(
        &creds,
        &rsa_key,
        date,
        "iaas.us-ashburn-1.oraclecloud.com",
        "/20160918/instances",
    )
    .unwrap();
    // Exact match on the headers= field value
    assert!(
        result.contains("headers=\"date (request-target) host\""),
        "headers field mismatch in: {}",
        result
    );
}

#[test]
fn test_sign_request_key_id_format() {
    let creds = make_creds(load_test_key());
    let rsa_key = parse_private_key(&creds.key_pem).unwrap();
    let date = "Thu, 26 Mar 2026 12:00:00 GMT";
    let result = sign_request(
        &creds,
        &rsa_key,
        date,
        "iaas.us-ashburn-1.oraclecloud.com",
        "/20160918/instances",
    )
    .unwrap();
    let expected = format!(
        "keyId=\"{}/{}/{}\"",
        creds.tenancy, creds.user, creds.fingerprint
    );
    assert!(
        result.contains(&expected),
        "expected keyId {} in: {}",
        expected,
        result
    );
}

#[test]
fn test_deserialize_multiple_instances() {
    let json = r#"[
        {
            "id": "ocid1.instance.oc1..aaa",
            "displayName": "server-1",
            "lifecycleState": "RUNNING",
            "shape": "VM.Standard2.1"
        },
        {
            "id": "ocid1.instance.oc1..bbb",
            "displayName": "server-2",
            "lifecycleState": "STOPPED",
            "shape": "VM.Standard.E4.Flex",
            "imageId": "ocid1.image.oc1..img2"
        }
    ]"#;
    let items: Vec<OciInstance> = serde_json::from_str(json).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].id, "ocid1.instance.oc1..aaa");
    assert_eq!(items[0].display_name, "server-1");
    assert_eq!(items[1].id, "ocid1.instance.oc1..bbb");
    assert_eq!(items[1].display_name, "server-2");
    assert_eq!(items[1].image_id.as_deref(), Some("ocid1.image.oc1..img2"));
}

#[test]
fn test_extract_freeform_tags_special_chars() {
    let mut map = std::collections::HashMap::new();
    map.insert("path".to_string(), "/usr/local/bin".to_string());
    map.insert("env:tier".to_string(), "prod/us-east".to_string());
    let tags = extract_tags(&Some(map));
    assert!(tags.contains(&"env:tier:prod/us-east".to_string()));
    assert!(tags.contains(&"path:/usr/local/bin".to_string()));
}

// -----------------------------------------------------------------------
// HTTP roundtrip tests (mockito)
// -----------------------------------------------------------------------

#[test]
fn test_http_list_instances_roundtrip() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/20160918/instances")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded(
                "compartmentId".into(),
                "ocid1.compartment.oc1..aaa".into(),
            ),
            mockito::Matcher::UrlEncoded("limit".into(), "100".into()),
        ]))
        .match_header("Authorization", mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
                {
                    "id": "ocid1.instance.oc1..inst1",
                    "displayName": "web-prod-1",
                    "lifecycleState": "RUNNING",
                    "shape": "VM.Standard2.1",
                    "imageId": "ocid1.image.oc1..img1",
                    "freeformTags": {"env": "prod", "team": "web"}
                }
            ]"#,
        )
        .create();

    let agent = super::super::http_agent();
    let url = format!(
        "{}/20160918/instances?compartmentId=ocid1.compartment.oc1..aaa&limit=100",
        server.url()
    );
    let items: Vec<OciInstance> = agent
        .get(&url)
        .header("Authorization", "Signature version=\"1\",keyId=\"fake\"")
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "ocid1.instance.oc1..inst1");
    assert_eq!(items[0].display_name, "web-prod-1");
    assert_eq!(items[0].lifecycle_state, "RUNNING");
    assert_eq!(items[0].shape, "VM.Standard2.1");
    assert_eq!(items[0].image_id.as_deref(), Some("ocid1.image.oc1..img1"));
    let tags = items[0].freeform_tags.as_ref().unwrap();
    assert_eq!(tags.get("env").unwrap(), "prod");
    assert_eq!(tags.get("team").unwrap(), "web");
    mock.assert();
}

#[test]
fn test_http_list_instances_pagination() {
    let mut server = mockito::Server::new();
    let page1 = server
        .mock("GET", "/20160918/instances")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("compartmentId".into(), "ocid1.compartment.oc1..aaa".into()),
            mockito::Matcher::UrlEncoded("limit".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_header("opc-next-page", "page2-token")
        .with_body(
            r#"[{"id": "ocid1.instance.oc1..a", "displayName": "srv-1", "lifecycleState": "RUNNING", "shape": "VM.Standard2.1"}]"#,
        )
        .create();
    let page2 = server
        .mock("GET", "/20160918/instances")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("compartmentId".into(), "ocid1.compartment.oc1..aaa".into()),
            mockito::Matcher::UrlEncoded("limit".into(), "100".into()),
            mockito::Matcher::UrlEncoded("page".into(), "page2-token".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[{"id": "ocid1.instance.oc1..b", "displayName": "srv-2", "lifecycleState": "STOPPED", "shape": "VM.Standard.E4.Flex"}]"#,
        )
        .create();

    let agent = super::super::http_agent();
    // Page 1
    let resp1 = agent
        .get(&format!(
            "{}/20160918/instances?compartmentId=ocid1.compartment.oc1..aaa&limit=100",
            server.url()
        ))
        .header("Authorization", "Signature version=\"1\",keyId=\"fake\"")
        .call()
        .unwrap();
    let next_page = resp1
        .headers()
        .get("opc-next-page")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let items1: Vec<OciInstance> =
        serde_json::from_str(&resp1.into_body().read_to_string().unwrap()).unwrap();
    assert_eq!(items1.len(), 1);
    assert_eq!(items1[0].id, "ocid1.instance.oc1..a");
    assert_eq!(next_page.as_deref(), Some("page2-token"));

    // Page 2
    let items2: Vec<OciInstance> = agent
        .get(&format!(
            "{}/20160918/instances?compartmentId=ocid1.compartment.oc1..aaa&limit=100&page=page2-token",
            server.url()
        ))
        .header("Authorization", "Signature version=\"1\",keyId=\"fake\"")
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();
    assert_eq!(items2.len(), 1);
    assert_eq!(items2[0].id, "ocid1.instance.oc1..b");

    page1.assert();
    page2.assert();
}

#[test]
fn test_http_list_instances_auth_failure() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/20160918/instances")
        .match_query(mockito::Matcher::Any)
        .with_status(401)
        .with_body(r#"{"code": "NotAuthenticated", "message": "The required information to complete authentication was not provided."}"#)
        .create();

    let agent = super::super::http_agent();
    let result = agent
        .get(&format!(
            "{}/20160918/instances?compartmentId=ocid1.compartment.oc1..aaa&limit=100",
            server.url()
        ))
        .header("Authorization", "Signature version=\"1\",keyId=\"bad\"")
        .call();

    match result {
        Err(ureq::Error::StatusCode(401)) => {} // expected
        other => panic!("expected 401 error, got {:?}", other),
    }
    mock.assert();
}

#[test]
fn test_http_vnic_attachments_roundtrip() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/20160918/vnicAttachments")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded(
                "compartmentId".into(),
                "ocid1.compartment.oc1..aaa".into(),
            ),
            mockito::Matcher::UrlEncoded("limit".into(), "100".into()),
        ]))
        .match_header("Authorization", mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
                {
                    "instanceId": "ocid1.instance.oc1..inst1",
                    "vnicId": "ocid1.vnic.oc1..vnic1",
                    "lifecycleState": "ATTACHED",
                    "isPrimary": true
                },
                {
                    "instanceId": "ocid1.instance.oc1..inst1",
                    "vnicId": "ocid1.vnic.oc1..vnic2",
                    "lifecycleState": "ATTACHED",
                    "isPrimary": false
                }
            ]"#,
        )
        .create();

    let agent = super::super::http_agent();
    let url = format!(
        "{}/20160918/vnicAttachments?compartmentId=ocid1.compartment.oc1..aaa&limit=100",
        server.url()
    );
    let items: Vec<OciVnicAttachment> = agent
        .get(&url)
        .header("Authorization", "Signature version=\"1\",keyId=\"fake\"")
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();

    assert_eq!(items.len(), 2);
    assert_eq!(items[0].instance_id, "ocid1.instance.oc1..inst1");
    assert_eq!(items[0].vnic_id.as_deref(), Some("ocid1.vnic.oc1..vnic1"));
    assert_eq!(items[0].lifecycle_state, "ATTACHED");
    assert_eq!(items[0].is_primary, Some(true));
    assert_eq!(items[1].is_primary, Some(false));
    mock.assert();
}

#[test]
fn test_http_vnic_attachments_auth_failure() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/20160918/vnicAttachments")
        .match_query(mockito::Matcher::Any)
        .with_status(401)
        .with_body(r#"{"code": "NotAuthenticated", "message": "Invalid credentials"}"#)
        .create();

    let agent = super::super::http_agent();
    let result = agent
        .get(&format!(
            "{}/20160918/vnicAttachments?compartmentId=ocid1.compartment.oc1..aaa&limit=100",
            server.url()
        ))
        .header("Authorization", "Signature version=\"1\",keyId=\"bad\"")
        .call();

    match result {
        Err(ureq::Error::StatusCode(401)) => {} // expected
        other => panic!("expected 401 error, got {:?}", other),
    }
    mock.assert();
}

#[test]
fn test_http_get_vnic_roundtrip() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/20160918/vnics/ocid1.vnic.oc1..vnic1")
        .match_header("Authorization", mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"publicIp": "129.146.10.1", "privateIp": "10.0.0.5"}"#)
        .create();

    let agent = super::super::http_agent();
    let url = format!("{}/20160918/vnics/ocid1.vnic.oc1..vnic1", server.url());
    let vnic: OciVnic = agent
        .get(&url)
        .header("Authorization", "Signature version=\"1\",keyId=\"fake\"")
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();

    assert_eq!(vnic.public_ip.as_deref(), Some("129.146.10.1"));
    assert_eq!(vnic.private_ip.as_deref(), Some("10.0.0.5"));
    mock.assert();
}

#[test]
fn test_http_get_vnic_auth_failure() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/20160918/vnics/ocid1.vnic.oc1..vnic1")
        .match_query(mockito::Matcher::Any)
        .with_status(401)
        .with_body(r#"{"code": "NotAuthenticated", "message": "Invalid credentials"}"#)
        .create();

    let agent = super::super::http_agent();
    let result = agent
        .get(&format!(
            "{}/20160918/vnics/ocid1.vnic.oc1..vnic1",
            server.url()
        ))
        .header("Authorization", "Signature version=\"1\",keyId=\"bad\"")
        .call();

    match result {
        Err(ureq::Error::StatusCode(401)) => {} // expected
        other => panic!("expected 401 error, got {:?}", other),
    }
    mock.assert();
}

#[test]
fn test_http_get_image_roundtrip() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/20160918/images/ocid1.image.oc1..img1")
        .match_header("Authorization", mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"displayName": "Oracle-Linux-8.8-2024.01.26-0"}"#)
        .create();

    let agent = super::super::http_agent();
    let url = format!("{}/20160918/images/ocid1.image.oc1..img1", server.url());
    let image: OciImage = agent
        .get(&url)
        .header("Authorization", "Signature version=\"1\",keyId=\"fake\"")
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();

    assert_eq!(
        image.display_name.as_deref(),
        Some("Oracle-Linux-8.8-2024.01.26-0")
    );
    mock.assert();
}

#[test]
fn test_http_get_image_auth_failure() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/20160918/images/ocid1.image.oc1..img1")
        .match_query(mockito::Matcher::Any)
        .with_status(401)
        .with_body(r#"{"code": "NotAuthenticated", "message": "Invalid credentials"}"#)
        .create();

    let agent = super::super::http_agent();
    let result = agent
        .get(&format!(
            "{}/20160918/images/ocid1.image.oc1..img1",
            server.url()
        ))
        .header("Authorization", "Signature version=\"1\",keyId=\"bad\"")
        .call();

    match result {
        Err(ureq::Error::StatusCode(401)) => {} // expected
        other => panic!("expected 401 error, got {:?}", other),
    }
    mock.assert();
}

// ── ListCompartments HTTP tests ─────────────────────────────────

#[test]
fn test_deserialize_compartment() {
    let json = r#"[
        {"id": "ocid1.compartment.oc1..child1", "lifecycleState": "ACTIVE"},
        {"id": "ocid1.compartment.oc1..child2", "lifecycleState": "DELETED"}
    ]"#;
    let items: Vec<OciCompartment> = serde_json::from_str(json).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].id, "ocid1.compartment.oc1..child1");
    assert_eq!(items[0].lifecycle_state, "ACTIVE");
    assert_eq!(items[1].lifecycle_state, "DELETED");
}

#[test]
fn test_deserialize_compartment_empty() {
    let json = r#"[]"#;
    let items: Vec<OciCompartment> = serde_json::from_str(json).unwrap();
    assert_eq!(items.len(), 0);
}

#[test]
fn test_http_list_compartments_roundtrip() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/20160918/compartments")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("compartmentId".into(), "ocid1.tenancy.oc1..root".into()),
            mockito::Matcher::UrlEncoded("compartmentIdInSubtree".into(), "true".into()),
            mockito::Matcher::UrlEncoded("lifecycleState".into(), "ACTIVE".into()),
            mockito::Matcher::UrlEncoded("limit".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
                {"id": "ocid1.compartment.oc1..prod", "lifecycleState": "ACTIVE"},
                {"id": "ocid1.compartment.oc1..staging", "lifecycleState": "ACTIVE"},
                {"id": "ocid1.compartment.oc1..old", "lifecycleState": "DELETED"}
            ]"#,
        )
        .create();

    let agent = super::super::http_agent();
    let url = format!(
        "{}/20160918/compartments?compartmentId={}&compartmentIdInSubtree=true&lifecycleState=ACTIVE&limit=100",
        server.url(),
        "ocid1.tenancy.oc1..root"
    );
    let items: Vec<OciCompartment> = agent
        .get(&url)
        .header("date", "Thu, 27 Mar 2026 12:00:00 GMT")
        .header("Authorization", "Signature version=\"1\",keyId=\"test\"")
        .call()
        .unwrap()
        .body_mut()
        .read_json()
        .unwrap();

    // Only ACTIVE compartments should be kept by caller
    let active: Vec<_> = items
        .iter()
        .filter(|c| c.lifecycle_state == "ACTIVE")
        .collect();
    assert_eq!(active.len(), 2);
    assert_eq!(active[0].id, "ocid1.compartment.oc1..prod");
    assert_eq!(active[1].id, "ocid1.compartment.oc1..staging");
    mock.assert();
}

#[test]
fn test_http_list_compartments_auth_failure() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/20160918/compartments")
        .match_query(mockito::Matcher::Any)
        .with_status(401)
        .with_body(r#"{"code": "NotAuthenticated", "message": "Not authenticated"}"#)
        .create();

    let agent = super::super::http_agent();
    let result = agent
        .get(&format!(
            "{}/20160918/compartments?compartmentId=x&compartmentIdInSubtree=true&lifecycleState=ACTIVE&limit=100",
            server.url()
        ))
        .header("date", "Thu, 27 Mar 2026 12:00:00 GMT")
        .header("Authorization", "Signature version=\"1\",keyId=\"bad\"")
        .call();

    match result {
        Err(ureq::Error::StatusCode(401)) => {} // expected
        other => panic!("expected 401 error, got {:?}", other),
    }
    mock.assert();
}
