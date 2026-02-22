use serde::Deserialize;

use super::{Provider, ProviderError, ProviderHost, map_ureq_error};

pub struct Hetzner;

#[derive(Deserialize)]
struct HetznerResponse {
    servers: Vec<HetznerServer>,
    meta: HetznerMeta,
}

#[derive(Deserialize)]
struct HetznerServer {
    id: u64,
    name: String,
    public_net: PublicNet,
    #[serde(default)]
    labels: std::collections::HashMap<String, String>,
}

#[derive(Deserialize)]
struct PublicNet {
    ipv4: Option<Ipv4Info>,
}

#[derive(Deserialize)]
struct Ipv4Info {
    ip: String,
}

#[derive(Deserialize)]
struct HetznerMeta {
    pagination: Pagination,
}

#[derive(Deserialize)]
struct Pagination {
    page: u64,
    last_page: u64,
}

impl Provider for Hetzner {
    fn name(&self) -> &str {
        "hetzner"
    }

    fn short_label(&self) -> &str {
        "hetzner"
    }

    fn fetch_hosts(&self, token: &str) -> Result<Vec<ProviderHost>, ProviderError> {
        let mut all_hosts = Vec::new();
        let mut page = 1u64;

        loop {
            let url = format!(
                "https://api.hetzner.cloud/v1/servers?page={}&per_page=50",
                page
            );
            let resp: HetznerResponse = ureq::get(&url)
                .set("Authorization", &format!("Bearer {}", token))
                .call()
                .map_err(map_ureq_error)?
                .into_json()
                .map_err(|e| ProviderError::Parse(e.to_string()))?;

            for server in &resp.servers {
                if let Some(ref ipv4) = server.public_net.ipv4 {
                    if !ipv4.ip.is_empty() {
                        let mut tags: Vec<String> = server
                            .labels
                            .iter()
                            .map(|(k, v)| {
                                if v.is_empty() {
                                    k.clone()
                                } else {
                                    format!("{}={}", k, v)
                                }
                            })
                            .collect();
                        tags.sort();
                        all_hosts.push(ProviderHost {
                            server_id: server.id.to_string(),
                            name: server.name.clone(),
                            ip: ipv4.ip.clone(),
                            tags,
                        });
                    }
                }
            }

            if resp.meta.pagination.page >= resp.meta.pagination.last_page {
                break;
            }
            page += 1;
        }

        Ok(all_hosts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hetzner_response() {
        let json = r#"{
            "servers": [
                {
                    "id": 42,
                    "name": "my-server",
                    "public_net": {
                        "ipv4": {"ip": "1.2.3.4"}
                    },
                    "labels": {"env": "prod", "team": ""}
                },
                {
                    "id": 43,
                    "name": "no-ip",
                    "public_net": {
                        "ipv4": null
                    },
                    "labels": {}
                }
            ],
            "meta": {"pagination": {"page": 1, "last_page": 1}}
        }"#;
        let resp: HetznerResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.servers.len(), 2);
        assert_eq!(resp.servers[0].name, "my-server");
        assert_eq!(resp.servers[0].public_net.ipv4.as_ref().unwrap().ip, "1.2.3.4");
        assert!(resp.servers[1].public_net.ipv4.is_none());
    }
}
