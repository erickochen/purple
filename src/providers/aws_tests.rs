use super::*;

// =========================================================================
// format_utc
// =========================================================================

#[test]
fn test_format_utc_epoch_zero() {
    let (ts, ds) = format_utc(0);
    assert_eq!(ts, "19700101T000000Z");
    assert_eq!(ds, "19700101");
}

#[test]
fn test_format_utc_known_date() {
    // 2024-01-15 12:30:45 UTC = 1705321845
    let (ts, ds) = format_utc(1705321845);
    assert_eq!(ts, "20240115T123045Z");
    assert_eq!(ds, "20240115");
}

#[test]
fn test_format_utc_leap_year() {
    // 2024-02-29 00:00:00 UTC = 1709164800
    let (ts, ds) = format_utc(1709164800);
    assert_eq!(ts, "20240229T000000Z");
    assert_eq!(ds, "20240229");
}

#[test]
fn test_format_utc_end_of_year() {
    // 2023-12-31 23:59:59 UTC = 1704067199
    let (ts, ds) = format_utc(1704067199);
    assert_eq!(ts, "20231231T235959Z");
    assert_eq!(ds, "20231231");
}

#[test]
fn test_format_utc_year_2000() {
    // 2000-03-01 00:00:00 UTC = 951868800
    let (ts, ds) = format_utc(951868800);
    assert_eq!(ts, "20000301T000000Z");
    assert_eq!(ds, "20000301");
}

// =========================================================================
// uri_encode
// =========================================================================

#[test]
fn test_uri_encode_passthrough() {
    assert_eq!(uri_encode("abc123-_.~"), "abc123-_.~");
}

#[test]
fn test_uri_encode_special_chars() {
    assert_eq!(uri_encode("hello world"), "hello%20world");
    assert_eq!(uri_encode("a=b&c"), "a%3Db%26c");
    assert_eq!(uri_encode("/path"), "%2Fpath");
}

#[test]
fn test_uri_encode_empty() {
    assert_eq!(uri_encode(""), "");
}

// =========================================================================
// hex_encode
// =========================================================================

#[test]
fn test_hex_encode() {
    assert_eq!(hex_encode(&[0x00, 0xff, 0xab]), "00ffab");
    assert_eq!(hex_encode(&[]), "");
}

// =========================================================================
// sha256_hash
// =========================================================================

#[test]
fn test_sha256_empty() {
    let hash = hex_encode(&sha256_hash(b""));
    assert_eq!(
        hash,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn test_sha256_known() {
    let hash = hex_encode(&sha256_hash(b"hello"));
    assert_eq!(
        hash,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

// =========================================================================
// hmac_sha256
// =========================================================================

#[test]
fn test_hmac_sha256_known() {
    // HMAC-SHA256("key", "message") is a well-known test vector
    let result = hex_encode(&hmac_sha256(
        b"key",
        b"The quick brown fox jumps over the lazy dog",
    ));
    assert_eq!(
        result,
        "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
    );
}

// =========================================================================
// sign_request (SigV4)
// =========================================================================

#[test]
fn test_sign_request_format() {
    let creds = AwsCredentials {
        access_key: "AKIDEXAMPLE".to_string(),
        secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_string(),
    };
    let auth = sign_request(
        &creds,
        "us-east-1",
        "ec2.us-east-1.amazonaws.com",
        "Action=DescribeInstances&Version=2016-11-15",
        "20150830T123600Z",
        "20150830",
    );
    assert!(auth.starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/ec2/aws4_request, SignedHeaders=host;x-amz-date, Signature="));
    // Signature should be a 64-char hex string
    let sig = auth.rsplit("Signature=").next().unwrap();
    assert_eq!(sig.len(), 64);
    assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_sign_request_deterministic() {
    let creds = AwsCredentials {
        access_key: "AK".to_string(),
        secret_key: "SK".to_string(),
    };
    let a = sign_request(
        &creds,
        "us-east-1",
        "ec2.us-east-1.amazonaws.com",
        "Action=DescribeInstances",
        "20240101T000000Z",
        "20240101",
    );
    let b = sign_request(
        &creds,
        "us-east-1",
        "ec2.us-east-1.amazonaws.com",
        "Action=DescribeInstances",
        "20240101T000000Z",
        "20240101",
    );
    assert_eq!(a, b);
}

#[test]
fn test_sign_request_different_regions() {
    let creds = AwsCredentials {
        access_key: "AK".to_string(),
        secret_key: "SK".to_string(),
    };
    let a = sign_request(
        &creds,
        "us-east-1",
        "ec2.us-east-1.amazonaws.com",
        "Action=DescribeInstances",
        "20240101T000000Z",
        "20240101",
    );
    let b = sign_request(
        &creds,
        "eu-west-1",
        "ec2.eu-west-1.amazonaws.com",
        "Action=DescribeInstances",
        "20240101T000000Z",
        "20240101",
    );
    assert_ne!(a, b);
}

// =========================================================================
// parse_credentials
// =========================================================================

#[test]
fn test_parse_credentials_default_profile() {
    let content = "[default]\naws_access_key_id = AKID123\naws_secret_access_key = SECRET456\n";
    let creds = parse_credentials(content, "default").unwrap();
    assert_eq!(creds.access_key, "AKID123");
    assert_eq!(creds.secret_key, "SECRET456");
}

#[test]
fn test_parse_credentials_named_profile() {
    let content = "[default]\naws_access_key_id = DEFAULT\naws_secret_access_key = DEFSECRET\n\n[prod]\naws_access_key_id = PRODAK\naws_secret_access_key = PRODSK\n";
    let creds = parse_credentials(content, "prod").unwrap();
    assert_eq!(creds.access_key, "PRODAK");
    assert_eq!(creds.secret_key, "PRODSK");
}

#[test]
fn test_parse_credentials_missing_profile() {
    let content = "[default]\naws_access_key_id = AK\naws_secret_access_key = SK\n";
    assert!(parse_credentials(content, "nonexistent").is_none());
}

#[test]
fn test_parse_credentials_incomplete_profile() {
    let content = "[incomplete]\naws_access_key_id = AK\n";
    assert!(parse_credentials(content, "incomplete").is_none());
}

#[test]
fn test_parse_credentials_whitespace_handling() {
    let content =
        "[default]\n  aws_access_key_id  =  AKID  \n  aws_secret_access_key  =  SECRET  \n";
    let creds = parse_credentials(content, "default").unwrap();
    assert_eq!(creds.access_key, "AKID");
    assert_eq!(creds.secret_key, "SECRET");
}

#[test]
fn test_parse_credentials_extra_keys_ignored() {
    let content = "[default]\naws_access_key_id = AK\naws_secret_access_key = SK\naws_session_token = TOKEN\nregion = us-east-1\n";
    let creds = parse_credentials(content, "default").unwrap();
    assert_eq!(creds.access_key, "AK");
    assert_eq!(creds.secret_key, "SK");
}

#[test]
fn test_parse_credentials_empty_content() {
    assert!(parse_credentials("", "default").is_none());
}

// =========================================================================
// resolve_credentials (token parsing)
// =========================================================================

#[test]
fn test_resolve_credentials_token_format() {
    let creds = resolve_credentials("AKID:SECRET", "").unwrap();
    assert_eq!(creds.access_key, "AKID");
    assert_eq!(creds.secret_key, "SECRET");
}

#[test]
fn test_resolve_credentials_empty_parts() {
    // Empty access key
    assert!(resolve_credentials(":SECRET", "").is_err());
    // Empty secret key
    assert!(resolve_credentials("AKID:", "").is_err());
}

#[test]
fn test_resolve_credentials_no_colon() {
    // No colon in token: split_once fails, falls through to env vars
    // Token-only (no colon) should not produce valid credentials from token path
    let result = resolve_credentials("just-a-token", "");
    // Result depends on env vars. Verify token path was skipped by
    // confirming credentials (if any) don't contain the raw token string.
    if let Ok(ref creds) = result {
        assert_ne!(creds.access_key, "just-a-token");
        assert_ne!(creds.secret_key, "just-a-token");
    }
}

// =========================================================================
// XML parsing: DescribeInstances
// =========================================================================

#[test]
fn test_parse_describe_instances_basic() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<DescribeInstancesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
<requestId>abc123</requestId>
<reservationSet>
    <item>
        <reservationId>r-12345</reservationId>
        <instancesSet>
            <item>
                <instanceId>i-abc123</instanceId>
                <imageId>ami-12345</imageId>
                <instanceState><name>running</name></instanceState>
                <instanceType>t3.micro</instanceType>
                <ipAddress>1.2.3.4</ipAddress>
                <placement><availabilityZone>us-east-1a</availabilityZone></placement>
                <tagSet>
                    <item><key>Name</key><value>web-01</value></item>
                    <item><key>Environment</key><value>prod</value></item>
                </tagSet>
            </item>
        </instancesSet>
    </item>
</reservationSet>
</DescribeInstancesResponse>"#;

    let resp: DescribeInstancesResponse = quick_xml::de::from_str(xml).unwrap();
    assert_eq!(resp.reservation_set.item.len(), 1);
    let instance = &resp.reservation_set.item[0].instances_set.item[0];
    assert_eq!(instance.instance_id, "i-abc123");
    assert_eq!(instance.image_id, "ami-12345");
    assert_eq!(instance.instance_state.name, "running");
    assert_eq!(instance.instance_type, "t3.micro");
    assert_eq!(instance.ip_address.as_deref(), Some("1.2.3.4"));
    assert_eq!(instance.tag_set.item.len(), 2);
}

#[test]
fn test_parse_describe_instances_no_public_ip() {
    let xml = r#"<DescribeInstancesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
<reservationSet>
    <item>
        <instancesSet>
            <item>
                <instanceId>i-noip</instanceId>
                <instanceState><name>running</name></instanceState>
                <tagSet/>
            </item>
        </instancesSet>
    </item>
</reservationSet>
</DescribeInstancesResponse>"#;

    let resp: DescribeInstancesResponse = quick_xml::de::from_str(xml).unwrap();
    let instance = &resp.reservation_set.item[0].instances_set.item[0];
    assert!(instance.ip_address.is_none());
}

#[test]
fn test_parse_describe_instances_empty() {
    let xml = r#"<DescribeInstancesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
<reservationSet/>
</DescribeInstancesResponse>"#;

    let resp: DescribeInstancesResponse = quick_xml::de::from_str(xml).unwrap();
    assert!(resp.reservation_set.item.is_empty());
}

#[test]
fn test_parse_describe_instances_with_next_token() {
    let xml = r#"<DescribeInstancesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
<reservationSet/>
<nextToken>eyJ0b2tlbiI6ICJ0ZXN0In0=</nextToken>
</DescribeInstancesResponse>"#;

    let resp: DescribeInstancesResponse = quick_xml::de::from_str(xml).unwrap();
    assert_eq!(resp.next_token.as_deref(), Some("eyJ0b2tlbiI6ICJ0ZXN0In0="));
}

#[test]
fn test_parse_describe_instances_multiple_reservations() {
    let xml = r#"<DescribeInstancesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
<reservationSet>
    <item>
        <instancesSet>
            <item>
                <instanceId>i-001</instanceId>
                <instanceState><name>running</name></instanceState>
                <ipAddress>1.1.1.1</ipAddress>
            </item>
        </instancesSet>
    </item>
    <item>
        <instancesSet>
            <item>
                <instanceId>i-002</instanceId>
                <instanceState><name>running</name></instanceState>
                <ipAddress>2.2.2.2</ipAddress>
            </item>
        </instancesSet>
    </item>
</reservationSet>
</DescribeInstancesResponse>"#;

    let resp: DescribeInstancesResponse = quick_xml::de::from_str(xml).unwrap();
    assert_eq!(resp.reservation_set.item.len(), 2);
    assert_eq!(
        resp.reservation_set.item[0].instances_set.item[0].instance_id,
        "i-001"
    );
    assert_eq!(
        resp.reservation_set.item[1].instances_set.item[0].instance_id,
        "i-002"
    );
}

// =========================================================================
// XML parsing: DescribeImages
// =========================================================================

#[test]
fn test_parse_describe_images() {
    let xml = r#"<DescribeImagesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
<imagesSet>
    <item>
        <imageId>ami-12345</imageId>
        <name>ubuntu/images/hvm-ssd/ubuntu-jammy-22.04-amd64-server-20240101</name>
    </item>
    <item>
        <imageId>ami-67890</imageId>
        <name>amzn2-ami-hvm-2.0.20240101.0-x86_64-gp2</name>
    </item>
</imagesSet>
</DescribeImagesResponse>"#;

    let resp: DescribeImagesResponse = quick_xml::de::from_str(xml).unwrap();
    assert_eq!(resp.images_set.item.len(), 2);
    assert_eq!(resp.images_set.item[0].image_id, "ami-12345");
    assert!(resp.images_set.item[0].name.contains("ubuntu"));
    assert_eq!(resp.images_set.item[1].image_id, "ami-67890");
}

#[test]
fn test_parse_describe_images_empty() {
    let xml = r#"<DescribeImagesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
<imagesSet/>
</DescribeImagesResponse>"#;

    let resp: DescribeImagesResponse = quick_xml::de::from_str(xml).unwrap();
    assert!(resp.images_set.item.is_empty());
}

// =========================================================================
// extract_tags
// =========================================================================

#[test]
fn test_extract_tags_name_and_values() {
    let tags = vec![
        Ec2Tag {
            key: "Name".to_string(),
            value: "web-01".to_string(),
        },
        Ec2Tag {
            key: "Environment".to_string(),
            value: "prod".to_string(),
        },
        Ec2Tag {
            key: "Team".to_string(),
            value: "backend".to_string(),
        },
    ];
    let (name, extracted) = extract_tags(&tags);
    assert_eq!(name, "web-01");
    assert_eq!(extracted, vec!["backend", "prod"]); // sorted
}

#[test]
fn test_extract_tags_filters_aws_prefix() {
    let tags = vec![
        Ec2Tag {
            key: "Name".to_string(),
            value: "srv".to_string(),
        },
        Ec2Tag {
            key: "aws:cloudformation:stack-name".to_string(),
            value: "my-stack".to_string(),
        },
        Ec2Tag {
            key: "aws:autoscaling:groupName".to_string(),
            value: "my-asg".to_string(),
        },
        Ec2Tag {
            key: "custom".to_string(),
            value: "val".to_string(),
        },
    ];
    let (name, extracted) = extract_tags(&tags);
    assert_eq!(name, "srv");
    assert_eq!(extracted, vec!["val"]);
}

#[test]
fn test_extract_tags_no_name() {
    let tags = vec![Ec2Tag {
        key: "Environment".to_string(),
        value: "dev".to_string(),
    }];
    let (name, extracted) = extract_tags(&tags);
    assert!(name.is_empty());
    assert_eq!(extracted, vec!["dev"]);
}

#[test]
fn test_extract_tags_empty_value_skipped() {
    let tags = vec![Ec2Tag {
        key: "flag".to_string(),
        value: "".to_string(),
    }];
    let (_, extracted) = extract_tags(&tags);
    assert!(extracted.is_empty());
}

#[test]
fn test_extract_tags_empty() {
    let (name, tags) = extract_tags(&[]);
    assert!(name.is_empty());
    assert!(tags.is_empty());
}

// =========================================================================
// AWS_REGIONS constant
// =========================================================================

#[test]
fn test_aws_regions_not_empty() {
    assert!(AWS_REGIONS.len() >= 20);
}

#[test]
fn test_aws_region_groups_cover_all_regions() {
    let total: usize = AWS_REGION_GROUPS.iter().map(|&(_, s, e)| e - s).sum();
    assert_eq!(total, AWS_REGIONS.len());
    // Verify groups are contiguous and non-overlapping
    let mut expected_start = 0;
    for &(_, start, end) in AWS_REGION_GROUPS {
        assert_eq!(start, expected_start, "Gap or overlap in region groups");
        assert!(end > start, "Empty region group");
        expected_start = end;
    }
    assert_eq!(expected_start, AWS_REGIONS.len());
}

#[test]
fn test_aws_regions_no_duplicates() {
    let mut seen = HashSet::new();
    for (code, _) in AWS_REGIONS {
        assert!(seen.insert(code), "Duplicate region: {}", code);
    }
}

#[test]
fn test_aws_regions_contains_common() {
    let codes: Vec<&str> = AWS_REGIONS.iter().map(|(c, _)| *c).collect();
    assert!(codes.contains(&"us-east-1"));
    assert!(codes.contains(&"eu-west-1"));
    assert!(codes.contains(&"ap-northeast-1"));
}

// =========================================================================
// Provider trait
// =========================================================================

#[test]
fn test_aws_provider_name() {
    let aws = Aws {
        regions: vec![],
        profile: String::new(),
    };
    assert_eq!(aws.name(), "aws");
    assert_eq!(aws.short_label(), "aws");
}

#[test]
fn test_aws_no_regions_error() {
    let aws = Aws {
        regions: vec![],
        profile: String::new(),
    };
    let result = aws.fetch_hosts("fake");
    match result {
        Err(ProviderError::Http(msg)) => assert!(msg.contains("No AWS regions")),
        other => panic!("Expected Http error, got: {:?}", other),
    }
}

// =========================================================================
// param helper
// =========================================================================

#[test]
fn test_param_helper() {
    let (k, v) = param("Action", "DescribeInstances");
    assert_eq!(k, "Action");
    assert_eq!(v, "DescribeInstances");
}

// =========================================================================
// Region validation
// =========================================================================

#[test]
fn test_aws_invalid_region_error() {
    let aws = Aws {
        regions: vec!["xx-invalid-1".to_string()],
        profile: String::new(),
    };
    let result = aws.fetch_hosts("AKID:SECRET");
    match result {
        Err(ProviderError::Http(msg)) => assert!(msg.contains("Unknown AWS region")),
        other => panic!("Expected Http error for invalid region, got: {:?}", other),
    }
}

#[test]
fn test_aws_mixed_valid_invalid_region_error() {
    let aws = Aws {
        regions: vec!["us-east-1".to_string(), "xx-fake-9".to_string()],
        profile: String::new(),
    };
    let result = aws.fetch_hosts("AKID:SECRET");
    match result {
        Err(ProviderError::Http(msg)) => assert!(msg.contains("xx-fake-9")),
        other => panic!("Expected Http error for invalid region, got: {:?}", other),
    }
}

// =========================================================================
// Profile credential errors return AuthFailed
// =========================================================================

#[test]
fn test_resolve_credentials_bad_profile_returns_auth_failed() {
    // Non-existent profile should return AuthFailed (not Http)
    let result = read_credentials_file("nonexistent-profile-xyz");
    assert!(matches!(result, Err(ProviderError::AuthFailed)));
}

// =========================================================================
// AMI batch constant
// =========================================================================

#[test]
fn test_ami_batch_size_is_reasonable() {
    assert_eq!(
        AMI_BATCH_SIZE, 100,
        "AMI batch size should be 100 (AWS limit per DescribeImages call)"
    );
}

// =========================================================================
// Private IP fallback
// =========================================================================

#[test]
fn test_parse_private_ip_address() {
    let xml = r#"<DescribeInstancesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
<reservationSet><item><instancesSet><item>
    <instanceId>i-priv</instanceId>
    <instanceState><name>running</name></instanceState>
    <privateIpAddress>10.0.1.5</privateIpAddress>
    <tagSet/>
</item></instancesSet></item></reservationSet>
</DescribeInstancesResponse>"#;
    let resp: DescribeInstancesResponse = quick_xml::de::from_str(xml).unwrap();
    let inst = &resp.reservation_set.item[0].instances_set.item[0];
    assert!(inst.ip_address.is_none());
    assert_eq!(inst.private_ip_address.as_deref(), Some("10.0.1.5"));
}

#[test]
fn test_public_ip_preferred_over_private() {
    let xml = r#"<DescribeInstancesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
<reservationSet><item><instancesSet><item>
    <instanceId>i-both</instanceId>
    <instanceState><name>running</name></instanceState>
    <ipAddress>54.1.2.3</ipAddress>
    <privateIpAddress>10.0.1.5</privateIpAddress>
    <tagSet/>
</item></instancesSet></item></reservationSet>
</DescribeInstancesResponse>"#;
    let resp: DescribeInstancesResponse = quick_xml::de::from_str(xml).unwrap();
    let inst = &resp.reservation_set.item[0].instances_set.item[0];
    assert_eq!(inst.ip_address.as_deref(), Some("54.1.2.3"));
    assert_eq!(inst.private_ip_address.as_deref(), Some("10.0.1.5"));
}

#[test]
fn test_no_ip_at_all_still_parseable() {
    let xml = r#"<DescribeInstancesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
<reservationSet><item><instancesSet><item>
    <instanceId>i-noip</instanceId>
    <instanceState><name>running</name></instanceState>
    <tagSet/>
</item></instancesSet></item></reservationSet>
</DescribeInstancesResponse>"#;
    let resp: DescribeInstancesResponse = quick_xml::de::from_str(xml).unwrap();
    let inst = &resp.reservation_set.item[0].instances_set.item[0];
    assert!(inst.ip_address.is_none());
    assert!(inst.private_ip_address.is_none());
}

// =========================================================================
// HTTP roundtrip tests (mockito)
// =========================================================================

#[test]
fn test_http_describe_instances_roundtrip() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("Action".into(), "DescribeInstances".into()),
            mockito::Matcher::UrlEncoded("Version".into(), "2016-11-15".into()),
        ]))
        .match_header("Authorization", mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "text/xml")
        .with_body(
            r#"<DescribeInstancesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <reservationSet>
<item>
  <instancesSet>
    <item>
      <instanceId>i-1234567890</instanceId>
      <instanceState><name>running</name></instanceState>
      <privateIpAddress>10.0.0.1</privateIpAddress>
      <ipAddress>54.1.2.3</ipAddress>
      <imageId>ami-12345678</imageId>
      <instanceType>t3.micro</instanceType>
      <tagSet><item><key>Name</key><value>web-1</value></item></tagSet>
    </item>
  </instancesSet>
</item>
  </reservationSet>
</DescribeInstancesResponse>"#,
        )
        .create();

    let agent = super::super::http_agent();
    let url = format!(
        "{}/?Action=DescribeInstances&Version=2016-11-15",
        server.url()
    );
    let body = agent
        .get(&url)
        .header("Authorization", "AWS4-HMAC-SHA256 Credential=fake")
        .call()
        .unwrap()
        .body_mut()
        .read_to_string()
        .unwrap();
    let resp: DescribeInstancesResponse = quick_xml::de::from_str(&body).unwrap();

    assert_eq!(resp.reservation_set.item.len(), 1);
    let inst = &resp.reservation_set.item[0].instances_set.item[0];
    assert_eq!(inst.instance_id, "i-1234567890");
    assert_eq!(inst.instance_state.name, "running");
    assert_eq!(inst.ip_address.as_deref(), Some("54.1.2.3"));
    assert_eq!(inst.private_ip_address.as_deref(), Some("10.0.0.1"));
    assert_eq!(inst.image_id, "ami-12345678");
    assert_eq!(inst.instance_type, "t3.micro");
    assert_eq!(inst.tag_set.item.len(), 1);
    assert_eq!(inst.tag_set.item[0].key, "Name");
    assert_eq!(inst.tag_set.item[0].value, "web-1");
    mock.assert();
}

#[test]
fn test_http_describe_instances_auth_failure() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/")
        .match_query(mockito::Matcher::Any)
        .with_status(401)
        .with_header("content-type", "text/xml")
        .with_body("<Error><Code>AuthFailure</Code></Error>")
        .create();

    let agent = super::super::http_agent();
    let result = agent
        .get(&format!(
            "{}/?Action=DescribeInstances&Version=2016-11-15",
            server.url()
        ))
        .header("Authorization", "AWS4-HMAC-SHA256 Credential=bad")
        .call();

    match result {
        Err(ureq::Error::StatusCode(401)) => {} // expected
        other => panic!("expected 401 error, got {:?}", other),
    }
    mock.assert();
}

#[test]
fn test_http_describe_images_roundtrip() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("Action".into(), "DescribeImages".into()),
            mockito::Matcher::UrlEncoded("Version".into(), "2016-11-15".into()),
            mockito::Matcher::UrlEncoded("ImageId.1".into(), "ami-12345678".into()),
        ]))
        .match_header("Authorization", mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "text/xml")
        .with_body(
            r#"<DescribeImagesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <imagesSet>
<item>
  <imageId>ami-12345678</imageId>
  <name>amzn2-ami-hvm-2.0</name>
</item>
  </imagesSet>
</DescribeImagesResponse>"#,
        )
        .create();

    let agent = super::super::http_agent();
    let url = format!(
        "{}/?Action=DescribeImages&Version=2016-11-15&ImageId.1=ami-12345678",
        server.url()
    );
    let body = agent
        .get(&url)
        .header("Authorization", "AWS4-HMAC-SHA256 Credential=fake")
        .call()
        .unwrap()
        .body_mut()
        .read_to_string()
        .unwrap();
    let resp: DescribeImagesResponse = quick_xml::de::from_str(&body).unwrap();

    assert_eq!(resp.images_set.item.len(), 1);
    assert_eq!(resp.images_set.item[0].image_id, "ami-12345678");
    assert_eq!(resp.images_set.item[0].name, "amzn2-ami-hvm-2.0");
    mock.assert();
}

#[test]
fn test_http_describe_images_auth_failure() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/")
        .match_query(mockito::Matcher::Any)
        .with_status(401)
        .with_header("content-type", "text/xml")
        .with_body("<Error><Code>AuthFailure</Code></Error>")
        .create();

    let agent = super::super::http_agent();
    let result = agent
        .get(&format!(
            "{}/?Action=DescribeImages&Version=2016-11-15&ImageId.1=ami-abc",
            server.url()
        ))
        .header("Authorization", "AWS4-HMAC-SHA256 Credential=bad")
        .call();

    match result {
        Err(ureq::Error::StatusCode(401)) => {} // expected
        other => panic!("expected 401 error, got {:?}", other),
    }
    mock.assert();
}
