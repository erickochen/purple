use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;

use super::{Provider, ProviderError, ProviderHost, map_ureq_error};

pub struct I3d;

// --- Dedicated/Game servers ---

#[derive(Deserialize)]
struct Host {
    id: u64,
    #[serde(default, rename = "serverName")]
    server_name: String,
    #[serde(default)]
    category: String,
    #[serde(default, rename = "ipAddress")]
    ip_addresses: Vec<HostIp>,
    #[serde(default, rename = "numCpu")]
    num_cpu: Option<u32>,
    #[serde(default, rename = "cpuType")]
    cpu_type: String,
}

#[derive(Deserialize)]
struct HostIp {
    #[serde(rename = "ipAddress")]
    ip_address: String,
    #[serde(default)]
    version: u8,
    #[serde(default)]
    private: u8,
}

// --- FlexMetal servers ---

#[derive(Deserialize)]
struct FlexMetalServer {
    uuid: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    location: String,
    #[serde(default, rename = "instanceType")]
    instance_type: String,
    #[serde(default)]
    os: Option<FlexMetalOs>,
    #[serde(default, rename = "ipAddresses")]
    ip_addresses: Vec<FlexMetalIp>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct FlexMetalOs {
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct FlexMetalIp {
    #[serde(default)]
    ip: String,
    #[serde(default)]
    version: u8,
    #[serde(default)]
    public: bool,
}

/// Select best IP from dedicated host: public IPv4 > public IPv6 > any IPv4.
fn select_host_ip(ips: &[HostIp]) -> Option<String> {
    ips.iter()
        .find(|ip| ip.private == 0 && ip.version == 4)
        .or_else(|| ips.iter().find(|ip| ip.private == 0 && ip.version == 6))
        .or_else(|| ips.iter().find(|ip| ip.version == 4))
        .map(|ip| super::strip_cidr(&ip.ip_address).to_string())
}

/// Select best IP from FlexMetal server: public IPv4 > public IPv6 > any IPv4.
fn select_flex_ip(ips: &[FlexMetalIp]) -> Option<String> {
    ips.iter()
        .find(|ip| ip.public && ip.version == 4)
        .or_else(|| ips.iter().find(|ip| ip.public && ip.version == 6))
        .or_else(|| ips.iter().find(|ip| ip.version == 4))
        .map(|ip| super::strip_cidr(&ip.ip).to_string())
}

impl Provider for I3d {
    fn name(&self) -> &str {
        "i3d"
    }

    fn short_label(&self) -> &str {
        "i3d"
    }

    fn fetch_hosts_cancellable(
        &self,
        token: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        let agent = super::http_agent();
        let mut all_hosts = Vec::new();

        // Fetch dedicated/game hosts with PAGE-TOKEN pagination
        let mut page_token: Option<String> = None;
        let mut page_count = 0u32;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }
            page_count += 1;
            if page_count > 500 {
                break;
            }
            let mut req = agent
                .get("https://api.i3d.net/v3/host")
                .header("PRIVATE-TOKEN", token);
            if let Some(ref pt) = page_token {
                req = req.header("PAGE-TOKEN", pt);
            }
            let mut response = req.call().map_err(map_ureq_error)?;

            let next_token = response
                .headers()
                .get("PAGE-TOKEN")
                .and_then(|v| v.to_str().ok())
                .filter(|s| !s.is_empty())
                .map(String::from);

            let hosts: Vec<Host> = response
                .body_mut()
                .read_json()
                .map_err(|e| ProviderError::Parse(e.to_string()))?;

            for host in &hosts {
                if let Some(ip) = select_host_ip(&host.ip_addresses) {
                    let mut metadata = Vec::with_capacity(3);
                    if !host.category.is_empty() {
                        metadata.push(("type".to_string(), host.category.clone()));
                    }
                    if let Some(ncpu) = host.num_cpu {
                        if ncpu > 0 && !host.cpu_type.is_empty() {
                            metadata.push((
                                "specs".to_string(),
                                format!("{}x {}", ncpu, host.cpu_type),
                            ));
                        }
                    }
                    let name = if host.server_name.is_empty() {
                        host.id.to_string()
                    } else {
                        host.server_name.clone()
                    };
                    all_hosts.push(ProviderHost {
                        server_id: format!("host-{}", host.id),
                        name,
                        ip,
                        tags: Vec::new(),
                        metadata,
                    });
                }
            }

            match next_token {
                Some(t) => page_token = Some(t),
                None => break,
            }
        }

        // Fetch FlexMetal servers with RANGED-DATA pagination
        let mut offset = 0u32;
        let results_per_page = 50u32;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }
            let ranged = format!("start={},results={}", offset, results_per_page);
            let servers: Vec<FlexMetalServer> = agent
                .get("https://api.i3d.net/v3/flexMetal/servers")
                .header("PRIVATE-TOKEN", token)
                .header("RANGED-DATA", &ranged)
                .call()
                .map_err(map_ureq_error)?
                .body_mut()
                .read_json()
                .map_err(|e| ProviderError::Parse(e.to_string()))?;

            let count = servers.len();
            for server in &servers {
                if let Some(ip) = select_flex_ip(&server.ip_addresses) {
                    let mut metadata = Vec::with_capacity(4);
                    if !server.location.is_empty() {
                        metadata.push(("location".to_string(), server.location.clone()));
                    }
                    if !server.instance_type.is_empty() {
                        metadata.push(("type".to_string(), server.instance_type.clone()));
                    }
                    if let Some(ref os) = server.os {
                        let os_val = os
                            .slug
                            .as_deref()
                            .filter(|s| !s.is_empty())
                            .or_else(|| os.name.as_deref().filter(|s| !s.is_empty()));
                        if let Some(val) = os_val {
                            metadata.push(("os".to_string(), val.to_string()));
                        }
                    }
                    if !server.status.is_empty() {
                        metadata.push(("status".to_string(), server.status.clone()));
                    }
                    let name = if server.name.is_empty() {
                        server.uuid.clone()
                    } else {
                        server.name.clone()
                    };
                    all_hosts.push(ProviderHost {
                        server_id: format!("flex-{}", server.uuid),
                        name,
                        ip,
                        tags: server.tags.clone(),
                        metadata,
                    });
                }
            }

            if count < results_per_page as usize {
                break;
            }
            offset += results_per_page;
            if offset / results_per_page >= 500 {
                break;
            }
        }

        Ok(all_hosts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Name/label tests ---

    #[test]
    fn test_name_and_short_label() {
        let i3d = I3d;
        assert_eq!(i3d.name(), "i3d");
        assert_eq!(i3d.short_label(), "i3d");
    }

    // --- IP selection tests ---

    #[test]
    fn test_select_host_ip_prefers_public_ipv4() {
        let ips = vec![
            HostIp {
                ip_address: "10.0.0.1".into(),
                version: 4,
                private: 1,
            },
            HostIp {
                ip_address: "31.204.131.39".into(),
                version: 4,
                private: 0,
            },
            HostIp {
                ip_address: "2001:db8::1".into(),
                version: 6,
                private: 0,
            },
        ];
        assert_eq!(select_host_ip(&ips).unwrap(), "31.204.131.39");
    }

    #[test]
    fn test_select_host_ip_falls_back_to_public_ipv6() {
        let ips = vec![
            HostIp {
                ip_address: "10.0.0.1".into(),
                version: 4,
                private: 1,
            },
            HostIp {
                ip_address: "2001:db8::1/64".into(),
                version: 6,
                private: 0,
            },
        ];
        assert_eq!(select_host_ip(&ips).unwrap(), "2001:db8::1");
    }

    #[test]
    fn test_select_host_ip_falls_back_to_private_ipv4() {
        let ips = vec![HostIp {
            ip_address: "10.0.0.1".into(),
            version: 4,
            private: 1,
        }];
        assert_eq!(select_host_ip(&ips).unwrap(), "10.0.0.1");
    }

    #[test]
    fn test_select_host_ip_empty() {
        assert!(select_host_ip(&[]).is_none());
    }

    #[test]
    fn test_select_flex_ip_prefers_public_ipv4() {
        let ips = vec![
            FlexMetalIp {
                ip: "10.0.0.1".into(),
                version: 4,
                public: false,
            },
            FlexMetalIp {
                ip: "1.2.3.4".into(),
                version: 4,
                public: true,
            },
        ];
        assert_eq!(select_flex_ip(&ips).unwrap(), "1.2.3.4");
    }

    #[test]
    fn test_select_flex_ip_falls_back_to_public_ipv6() {
        let ips = vec![
            FlexMetalIp {
                ip: "10.0.0.1".into(),
                version: 4,
                public: false,
            },
            FlexMetalIp {
                ip: "2001:db8::1".into(),
                version: 6,
                public: true,
            },
        ];
        assert_eq!(select_flex_ip(&ips).unwrap(), "2001:db8::1");
    }

    #[test]
    fn test_select_flex_ip_falls_back_to_private_ipv4() {
        let ips = vec![FlexMetalIp {
            ip: "10.0.0.1".into(),
            version: 4,
            public: false,
        }];
        assert_eq!(select_flex_ip(&ips).unwrap(), "10.0.0.1");
    }

    #[test]
    fn test_select_flex_ip_empty() {
        assert!(select_flex_ip(&[]).is_none());
    }

    // --- Deserialization tests ---

    #[test]
    fn test_parse_host_response() {
        let json = r#"[{
            "id": 12345,
            "serverId": 67890,
            "serverName": "game-server-01",
            "category": "Dedicated Game Servers",
            "dcLocationId": 1,
            "ipAddress": [
                {"ipAddress": "31.204.131.39", "version": 4, "type": 1, "private": 0},
                {"ipAddress": "10.0.0.5", "version": 4, "type": 1, "private": 1}
            ],
            "numCpu": 8,
            "cpuType": "Intel Xeon E-2288G"
        }]"#;
        let hosts: Vec<Host> = serde_json::from_str(json).unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].id, 12345);
        assert_eq!(hosts[0].server_name, "game-server-01");
        assert_eq!(hosts[0].ip_addresses.len(), 2);
        assert_eq!(hosts[0].ip_addresses[0].ip_address, "31.204.131.39");
        assert_eq!(hosts[0].num_cpu, Some(8));
        assert_eq!(hosts[0].cpu_type, "Intel Xeon E-2288G");
    }

    #[test]
    fn test_parse_host_minimal() {
        let json = r#"[{"id": 1, "ipAddress": []}]"#;
        let hosts: Vec<Host> = serde_json::from_str(json).unwrap();
        assert_eq!(hosts.len(), 1);
        assert!(hosts[0].ip_addresses.is_empty());
        assert_eq!(hosts[0].server_name, "");
    }

    #[test]
    fn test_parse_host_empty_list() {
        let hosts: Vec<Host> = serde_json::from_str("[]").unwrap();
        assert!(hosts.is_empty());
    }

    #[test]
    fn test_parse_flexmetal_response() {
        let json = r#"[{
            "uuid": "abc-123-def",
            "name": "flex-web-01",
            "status": "delivered",
            "location": "Amsterdam",
            "instanceType": "bm.general1.small",
            "os": {"slug": "ubuntu-2204-lts"},
            "ipAddresses": [
                {"ip": "1.2.3.4", "version": 4, "public": true},
                {"ip": "10.0.0.1", "version": 4, "public": false}
            ],
            "tags": ["production", "web"]
        }]"#;
        let servers: Vec<FlexMetalServer> = serde_json::from_str(json).unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].uuid, "abc-123-def");
        assert_eq!(servers[0].name, "flex-web-01");
        assert_eq!(servers[0].status, "delivered");
        assert_eq!(servers[0].location, "Amsterdam");
        assert_eq!(servers[0].instance_type, "bm.general1.small");
        assert_eq!(
            servers[0].os.as_ref().unwrap().slug.as_deref(),
            Some("ubuntu-2204-lts")
        );
        assert_eq!(servers[0].ip_addresses.len(), 2);
        assert_eq!(servers[0].tags, vec!["production", "web"]);
    }

    #[test]
    fn test_parse_flexmetal_os_name_fallback() {
        let json = r#"[{
            "uuid": "abc",
            "os": {"name": "Ubuntu 22.04"},
            "ipAddresses": [{"ip": "1.2.3.4", "version": 4, "public": true}]
        }]"#;
        let servers: Vec<FlexMetalServer> = serde_json::from_str(json).unwrap();
        let os = servers[0].os.as_ref().unwrap();
        assert!(os.slug.is_none());
        assert_eq!(os.name.as_deref(), Some("Ubuntu 22.04"));
    }

    #[test]
    fn test_parse_flexmetal_os_slug_preferred_over_name() {
        let json = r#"[{
            "uuid": "abc",
            "os": {"slug": "ubuntu-2204-lts", "name": "Ubuntu 22.04"},
            "ipAddresses": [{"ip": "1.2.3.4", "version": 4, "public": true}]
        }]"#;
        let servers: Vec<FlexMetalServer> = serde_json::from_str(json).unwrap();
        let os = servers[0].os.as_ref().unwrap();
        assert_eq!(os.slug.as_deref(), Some("ubuntu-2204-lts"));
        assert_eq!(os.name.as_deref(), Some("Ubuntu 22.04"));
        // Verify slug wins in metadata assembly
        let mut metadata = Vec::new();
        let os_val = os
            .slug
            .as_deref()
            .filter(|s| !s.is_empty())
            .or_else(|| os.name.as_deref().filter(|s| !s.is_empty()));
        if let Some(val) = os_val {
            metadata.push(("os".to_string(), val.to_string()));
        }
        assert_eq!(
            metadata,
            [("os".to_string(), "ubuntu-2204-lts".to_string())]
        );
    }

    #[test]
    fn test_parse_flexmetal_os_empty_object() {
        let json = r#"[{"uuid": "abc", "os": {}, "ipAddresses": []}]"#;
        let servers: Vec<FlexMetalServer> = serde_json::from_str(json).unwrap();
        let os = servers[0].os.as_ref().unwrap();
        assert!(os.slug.is_none());
        assert!(os.name.is_none());
    }

    #[test]
    fn test_parse_flexmetal_os_empty_slug_falls_back_to_name() {
        let json = r#"[{
            "uuid": "abc",
            "os": {"slug": "", "name": "Ubuntu 22.04"},
            "ipAddresses": [{"ip": "1.2.3.4", "version": 4, "public": true}]
        }]"#;
        let servers: Vec<FlexMetalServer> = serde_json::from_str(json).unwrap();
        let os = servers[0].os.as_ref().unwrap();
        let os_val = os
            .slug
            .as_deref()
            .filter(|s| !s.is_empty())
            .or_else(|| os.name.as_deref().filter(|s| !s.is_empty()));
        assert_eq!(os_val, Some("Ubuntu 22.04"));
    }

    #[test]
    fn test_parse_flexmetal_os_both_empty_strings() {
        let json = r#"[{
            "uuid": "abc",
            "os": {"slug": "", "name": ""},
            "ipAddresses": [{"ip": "1.2.3.4", "version": 4, "public": true}]
        }]"#;
        let servers: Vec<FlexMetalServer> = serde_json::from_str(json).unwrap();
        let os = servers[0].os.as_ref().unwrap();
        let os_val = os
            .slug
            .as_deref()
            .filter(|s| !s.is_empty())
            .or_else(|| os.name.as_deref().filter(|s| !s.is_empty()));
        assert!(os_val.is_none());
    }

    #[test]
    fn test_parse_flexmetal_minimal() {
        let json = r#"[{"uuid": "x", "ipAddresses": []}]"#;
        let servers: Vec<FlexMetalServer> = serde_json::from_str(json).unwrap();
        assert_eq!(servers.len(), 1);
        assert!(servers[0].ip_addresses.is_empty());
        assert!(servers[0].tags.is_empty());
    }

    #[test]
    fn test_parse_flexmetal_empty_list() {
        let servers: Vec<FlexMetalServer> = serde_json::from_str("[]").unwrap();
        assert!(servers.is_empty());
    }

    // --- HTTP roundtrip tests ---

    #[test]
    fn test_http_host_list_roundtrip() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/v3/host")
            .match_header("PRIVATE-TOKEN", "test-api-key")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"[{
                "id": 12345,
                "serverName": "game-01",
                "category": "Dedicated",
                "ipAddress": [{"ipAddress": "31.204.131.39", "version": 4, "type": 1, "private": 0}],
                "numCpu": 4,
                "cpuType": "Xeon"
            }]"#)
            .create();

        let agent = super::super::http_agent();
        let hosts: Vec<Host> = agent
            .get(&format!("{}/v3/host", server.url()))
            .header("PRIVATE-TOKEN", "test-api-key")
            .call()
            .unwrap()
            .body_mut()
            .read_json()
            .unwrap();

        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].server_name, "game-01");
        assert_eq!(
            select_host_ip(&hosts[0].ip_addresses).unwrap(),
            "31.204.131.39"
        );
        mock.assert();
    }

    #[test]
    fn test_http_flexmetal_roundtrip() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/v3/flexMetal/servers")
            .match_header("PRIVATE-TOKEN", "test-key")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"[{
                "uuid": "flex-uuid-1",
                "name": "flex-web",
                "status": "delivered",
                "location": "Amsterdam",
                "instanceType": "bm.general1.small",
                "os": {"slug": "ubuntu-2204-lts"},
                "ipAddresses": [{"ip": "1.2.3.4", "version": 4, "public": true}],
                "tags": ["prod"]
            }]"#,
            )
            .create();

        let agent = super::super::http_agent();
        let servers: Vec<FlexMetalServer> = agent
            .get(&format!("{}/v3/flexMetal/servers", server.url()))
            .header("PRIVATE-TOKEN", "test-key")
            .call()
            .unwrap()
            .body_mut()
            .read_json()
            .unwrap();

        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "flex-web");
        assert_eq!(select_flex_ip(&servers[0].ip_addresses).unwrap(), "1.2.3.4");
        mock.assert();
    }

    #[test]
    fn test_http_host_auth_failure() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/v3/host")
            .with_status(401)
            .with_body(r#"{"error": "Unauthorized"}"#)
            .create();

        let agent = super::super::http_agent();
        let result = agent
            .get(&format!("{}/v3/host", server.url()))
            .header("PRIVATE-TOKEN", "bad-key")
            .call();

        assert!(result.is_err());
        let err = super::map_ureq_error(result.unwrap_err());
        assert!(matches!(err, ProviderError::AuthFailed));
        mock.assert();
    }

    #[test]
    fn test_http_flexmetal_auth_failure() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/v3/flexMetal/servers")
            .with_status(401)
            .with_body(r#"{"error": "Unauthorized"}"#)
            .create();

        let agent = super::super::http_agent();
        let result = agent
            .get(&format!("{}/v3/flexMetal/servers", server.url()))
            .header("PRIVATE-TOKEN", "bad-key")
            .call();

        assert!(result.is_err());
        let err = super::map_ureq_error(result.unwrap_err());
        assert!(matches!(err, ProviderError::AuthFailed));
        mock.assert();
    }

    // --- Metadata tests ---

    #[test]
    fn test_host_metadata_all_fields() {
        let host = Host {
            id: 1,
            server_name: "game-01".into(),
            category: "Dedicated Game Servers".into(),
            ip_addresses: vec![HostIp {
                ip_address: "1.2.3.4".into(),
                version: 4,
                private: 0,
            }],
            num_cpu: Some(8),
            cpu_type: "Intel Xeon E-2288G".into(),
        };
        let ip = select_host_ip(&host.ip_addresses).unwrap();
        let mut metadata = Vec::new();
        if !host.category.is_empty() {
            metadata.push(("type".to_string(), host.category.clone()));
        }
        if let Some(ncpu) = host.num_cpu {
            if ncpu > 0 && !host.cpu_type.is_empty() {
                metadata.push(("specs".to_string(), format!("{}x {}", ncpu, host.cpu_type)));
            }
        }
        assert_eq!(ip, "1.2.3.4");
        assert_eq!(metadata.len(), 2);
        assert_eq!(
            metadata[0],
            ("type".to_string(), "Dedicated Game Servers".to_string())
        );
        assert_eq!(
            metadata[1],
            ("specs".to_string(), "8x Intel Xeon E-2288G".to_string())
        );
    }

    #[test]
    fn test_flexmetal_metadata_all_fields() {
        let server = FlexMetalServer {
            uuid: "abc".into(),
            name: "flex-01".into(),
            status: "delivered".into(),
            location: "Amsterdam".into(),
            instance_type: "bm.general1.small".into(),
            os: Some(FlexMetalOs {
                slug: Some("ubuntu-2204-lts".into()),
                name: None,
            }),
            ip_addresses: vec![FlexMetalIp {
                ip: "1.2.3.4".into(),
                version: 4,
                public: true,
            }],
            tags: vec!["prod".into()],
        };
        let ip = select_flex_ip(&server.ip_addresses).unwrap();
        let mut metadata = Vec::new();
        if !server.location.is_empty() {
            metadata.push(("location".to_string(), server.location.clone()));
        }
        if !server.instance_type.is_empty() {
            metadata.push(("type".to_string(), server.instance_type.clone()));
        }
        if let Some(ref os) = server.os {
            let os_val = os
                .slug
                .as_deref()
                .filter(|s| !s.is_empty())
                .or_else(|| os.name.as_deref().filter(|s| !s.is_empty()));
            if let Some(val) = os_val {
                metadata.push(("os".to_string(), val.to_string()));
            }
        }
        if !server.status.is_empty() {
            metadata.push(("status".to_string(), server.status.clone()));
        }
        assert_eq!(ip, "1.2.3.4");
        assert_eq!(metadata.len(), 4);
        assert_eq!(
            metadata[0],
            ("location".to_string(), "Amsterdam".to_string())
        );
        assert_eq!(
            metadata[1],
            ("type".to_string(), "bm.general1.small".to_string())
        );
        assert_eq!(
            metadata[2],
            ("os".to_string(), "ubuntu-2204-lts".to_string())
        );
        assert_eq!(metadata[3], ("status".to_string(), "delivered".to_string()));
    }

    // --- Name fallback tests ---

    #[test]
    fn test_host_uses_id_when_name_empty() {
        let host = Host {
            id: 12345,
            server_name: String::new(),
            category: String::new(),
            ip_addresses: vec![HostIp {
                ip_address: "1.2.3.4".into(),
                version: 4,
                private: 0,
            }],
            num_cpu: None,
            cpu_type: String::new(),
        };
        let name = if host.server_name.is_empty() {
            host.id.to_string()
        } else {
            host.server_name.clone()
        };
        assert_eq!(name, "12345");
    }

    #[test]
    fn test_flexmetal_uses_uuid_when_name_empty() {
        let server = FlexMetalServer {
            uuid: "abc-123".into(),
            name: String::new(),
            status: String::new(),
            location: String::new(),
            instance_type: String::new(),
            os: None,
            ip_addresses: vec![FlexMetalIp {
                ip: "1.2.3.4".into(),
                version: 4,
                public: true,
            }],
            tags: Vec::new(),
        };
        let name = if server.name.is_empty() {
            server.uuid.clone()
        } else {
            server.name.clone()
        };
        assert_eq!(name, "abc-123");
    }

    // --- No-IP skip tests ---

    #[test]
    fn test_host_skipped_without_valid_ip() {
        // Host with only IPv6 private IPs -> no valid IP -> skipped
        let ips = vec![HostIp {
            ip_address: "fe80::1".into(),
            version: 6,
            private: 1,
        }];
        assert!(select_host_ip(&ips).is_none());
    }

    #[test]
    fn test_flexmetal_skipped_without_valid_ip() {
        // FlexMetal with only private IPv6 -> no valid IP -> skipped
        let ips = vec![FlexMetalIp {
            ip: "fe80::1".into(),
            version: 6,
            public: false,
        }];
        assert!(select_flex_ip(&ips).is_none());
    }

    // --- Server ID prefix tests ---

    #[test]
    fn test_host_server_id_prefix() {
        let id = format!("host-{}", 12345u64);
        assert_eq!(id, "host-12345");
        assert!(id.starts_with("host-"));
    }

    #[test]
    fn test_flexmetal_server_id_prefix() {
        let id = format!("flex-{}", "abc-123");
        assert_eq!(id, "flex-abc-123");
        assert!(id.starts_with("flex-"));
    }

    // --- CIDR stripping tests ---

    #[test]
    fn test_host_ip_cidr_stripped() {
        let ips = vec![HostIp {
            ip_address: "31.204.131.39/24".into(),
            version: 4,
            private: 0,
        }];
        assert_eq!(select_host_ip(&ips).unwrap(), "31.204.131.39");
    }

    #[test]
    fn test_flex_ip_cidr_stripped() {
        let ips = vec![FlexMetalIp {
            ip: "1.2.3.4/32".into(),
            version: 4,
            public: true,
        }];
        assert_eq!(select_flex_ip(&ips).unwrap(), "1.2.3.4");
    }

    // --- Cancellation test ---

    #[test]
    fn test_cancellation_returns_cancelled() {
        let cancel = AtomicBool::new(true);
        let provider = I3d;
        let result = provider.fetch_hosts_cancellable("any-token", &cancel);
        assert!(matches!(result, Err(ProviderError::Cancelled)));
    }

    // --- Pagination tests ---

    #[test]
    fn test_http_host_list_pagination() {
        let mut server = mockito::Server::new();
        let page1 = server
            .mock("GET", "/v3/host")
            .match_header("PRIVATE-TOKEN", "key")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("PAGE-TOKEN", "next-cursor")
            .with_body(
                r#"[{
                "id": 1,
                "serverName": "host-1",
                "ipAddress": [{"ipAddress": "1.1.1.1", "version": 4, "type": 1, "private": 0}]
            }]"#,
            )
            .expect(1)
            .create();
        let page2 = server
            .mock("GET", "/v3/host")
            .match_header("PRIVATE-TOKEN", "key")
            .match_header("PAGE-TOKEN", "next-cursor")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"[{
                "id": 2,
                "serverName": "host-2",
                "ipAddress": [{"ipAddress": "2.2.2.2", "version": 4, "type": 1, "private": 0}]
            }]"#,
            )
            .expect(1)
            .create();

        let agent = super::super::http_agent();

        // Page 1
        let mut resp = agent
            .get(&format!("{}/v3/host", server.url()))
            .header("PRIVATE-TOKEN", "key")
            .call()
            .unwrap();
        let next = resp
            .headers()
            .get("PAGE-TOKEN")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let hosts: Vec<Host> = resp.body_mut().read_json().unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].server_name, "host-1");
        assert!(next.is_some());

        // Page 2
        let mut resp2 = agent
            .get(&format!("{}/v3/host", server.url()))
            .header("PRIVATE-TOKEN", "key")
            .header("PAGE-TOKEN", next.as_deref().unwrap())
            .call()
            .unwrap();
        let next2 = resp2
            .headers()
            .get("PAGE-TOKEN")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let hosts2: Vec<Host> = resp2.body_mut().read_json().unwrap();
        assert_eq!(hosts2.len(), 1);
        assert_eq!(hosts2[0].server_name, "host-2");
        assert!(next2.is_none());

        page1.assert();
        page2.assert();
    }

    #[test]
    fn test_http_flexmetal_pagination() {
        let mut server = mockito::Server::new();
        let page1 = server
            .mock("GET", "/v3/flexMetal/servers")
            .match_header("PRIVATE-TOKEN", "key")
            .match_header("RANGED-DATA", "start=0,results=50")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                // Return exactly 50 items to trigger next page
                format!(
                    "[{}]",
                    (0..50)
                        .map(|i| format!(
                            r#"{{"uuid":"uuid-{}","name":"flex-{}","ipAddresses":[{{"ip":"10.0.0.{}","version":4,"public":true}}]}}"#,
                            i, i, i % 256
                        ))
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            )
            .expect(1)
            .create();
        let page2 = server
            .mock("GET", "/v3/flexMetal/servers")
            .match_header("PRIVATE-TOKEN", "key")
            .match_header("RANGED-DATA", "start=50,results=50")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"[{
                "uuid": "uuid-last",
                "name": "flex-last",
                "ipAddresses": [{"ip": "9.9.9.9", "version": 4, "public": true}]
            }]"#,
            )
            .expect(1)
            .create();

        let agent = super::super::http_agent();

        // Page 1 - 50 results
        let servers1: Vec<FlexMetalServer> = agent
            .get(&format!("{}/v3/flexMetal/servers", server.url()))
            .header("PRIVATE-TOKEN", "key")
            .header("RANGED-DATA", "start=0,results=50")
            .call()
            .unwrap()
            .body_mut()
            .read_json()
            .unwrap();
        assert_eq!(servers1.len(), 50);

        // Page 2 - 1 result (less than 50 = last page)
        let servers2: Vec<FlexMetalServer> = agent
            .get(&format!("{}/v3/flexMetal/servers", server.url()))
            .header("PRIVATE-TOKEN", "key")
            .header("RANGED-DATA", "start=50,results=50")
            .call()
            .unwrap()
            .body_mut()
            .read_json()
            .unwrap();
        assert_eq!(servers2.len(), 1);
        assert_eq!(servers2[0].name, "flex-last");

        page1.assert();
        page2.assert();
    }
}
