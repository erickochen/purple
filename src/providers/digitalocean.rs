use serde::Deserialize;

use super::{Provider, ProviderError, ProviderHost, map_ureq_error};

pub struct DigitalOcean;

#[derive(Deserialize)]
struct DropletResponse {
    droplets: Vec<Droplet>,
    meta: Meta,
}

#[derive(Deserialize)]
struct Droplet {
    id: u64,
    name: String,
    networks: Networks,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct Networks {
    v4: Vec<NetworkV4>,
}

#[derive(Deserialize)]
struct NetworkV4 {
    ip_address: String,
    #[serde(rename = "type")]
    net_type: String,
}

#[derive(Deserialize)]
struct Meta {
    total: u64,
}

impl Provider for DigitalOcean {
    fn name(&self) -> &str {
        "digitalocean"
    }

    fn short_label(&self) -> &str {
        "do"
    }

    fn fetch_hosts(&self, token: &str) -> Result<Vec<ProviderHost>, ProviderError> {
        let mut all_hosts = Vec::new();
        let mut page = 1u64;
        let per_page = 200;

        loop {
            let url = format!(
                "https://api.digitalocean.com/v2/droplets?page={}&per_page={}",
                page, per_page
            );
            let resp: DropletResponse = ureq::get(&url)
                .set("Authorization", &format!("Bearer {}", token))
                .call()
                .map_err(map_ureq_error)?
                .into_json()
                .map_err(|e| ProviderError::Parse(e.to_string()))?;

            for droplet in &resp.droplets {
                let ip = droplet
                    .networks
                    .v4
                    .iter()
                    .find(|n| n.net_type == "public")
                    .map(|n| n.ip_address.clone());
                if let Some(ip) = ip {
                    all_hosts.push(ProviderHost {
                        server_id: droplet.id.to_string(),
                        name: droplet.name.clone(),
                        ip,
                        tags: droplet.tags.clone(),
                    });
                }
            }

            let fetched = page * per_page;
            if fetched >= resp.meta.total {
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
    fn test_parse_droplet_response() {
        let json = r#"{
            "droplets": [
                {
                    "id": 12345,
                    "name": "web-1",
                    "networks": {
                        "v4": [
                            {"ip_address": "10.0.0.1", "type": "private"},
                            {"ip_address": "1.2.3.4", "type": "public"}
                        ]
                    },
                    "tags": ["production"]
                },
                {
                    "id": 67890,
                    "name": "db-1",
                    "networks": {
                        "v4": [
                            {"ip_address": "10.0.0.2", "type": "private"}
                        ]
                    },
                    "tags": []
                }
            ],
            "meta": {"total": 2}
        }"#;
        let resp: DropletResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.droplets.len(), 2);
        assert_eq!(resp.droplets[0].name, "web-1");
        // web-1 has public IP
        let public_ip = resp.droplets[0]
            .networks
            .v4
            .iter()
            .find(|n| n.net_type == "public");
        assert!(public_ip.is_some());
        assert_eq!(public_ip.unwrap().ip_address, "1.2.3.4");
        // db-1 has no public IP (private only)
        let public_ip = resp.droplets[1]
            .networks
            .v4
            .iter()
            .find(|n| n.net_type == "public");
        assert!(public_ip.is_none());
    }
}
