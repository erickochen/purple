use serde::Deserialize;

use super::{Provider, ProviderError, ProviderHost, map_ureq_error};

pub struct Vultr;

#[derive(Deserialize)]
struct InstanceResponse {
    instances: Vec<Instance>,
    meta: VultrMeta,
}

#[derive(Deserialize)]
struct Instance {
    id: String,
    label: String,
    main_ip: String,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct VultrMeta {
    links: VultrLinks,
}

#[derive(Deserialize)]
struct VultrLinks {
    next: String,
}

impl Provider for Vultr {
    fn name(&self) -> &str {
        "vultr"
    }

    fn short_label(&self) -> &str {
        "vultr"
    }

    fn fetch_hosts(&self, token: &str) -> Result<Vec<ProviderHost>, ProviderError> {
        let mut all_hosts = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let url = match &cursor {
                None => "https://api.vultr.com/v2/instances?per_page=500".to_string(),
                Some(c) => format!(
                    "https://api.vultr.com/v2/instances?per_page=500&cursor={}",
                    c
                ),
            };
            let resp: InstanceResponse = ureq::get(&url)
                .set("Authorization", &format!("Bearer {}", token))
                .call()
                .map_err(map_ureq_error)?
                .into_json()
                .map_err(|e| ProviderError::Parse(e.to_string()))?;

            for instance in &resp.instances {
                if instance.main_ip.is_empty() || instance.main_ip == "0.0.0.0" {
                    continue;
                }
                all_hosts.push(ProviderHost {
                    server_id: instance.id.clone(),
                    name: instance.label.clone(),
                    ip: instance.main_ip.clone(),
                    tags: instance.tags.clone(),
                });
            }

            if resp.meta.links.next.is_empty() {
                break;
            }
            cursor = Some(resp.meta.links.next.clone());
        }

        Ok(all_hosts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_instance_response() {
        let json = r#"{
            "instances": [
                {
                    "id": "abc-123",
                    "label": "my-server",
                    "main_ip": "5.6.7.8",
                    "tags": ["web"]
                },
                {
                    "id": "def-456",
                    "label": "pending-server",
                    "main_ip": "0.0.0.0",
                    "tags": []
                }
            ],
            "meta": {"links": {"next": ""}}
        }"#;
        let resp: InstanceResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.instances.len(), 2);
        assert_eq!(resp.instances[0].label, "my-server");
        assert_eq!(resp.instances[0].main_ip, "5.6.7.8");
        // Second instance has 0.0.0.0 (should be skipped)
        assert_eq!(resp.instances[1].main_ip, "0.0.0.0");
    }
}
