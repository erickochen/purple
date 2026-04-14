//! Contract snapshot tests for provider API response structures.
//!
//! Each test loads a golden fixture from `tests/api_contracts/` and verifies
//! that the JSON (or XML) structure contains all expected fields. These fixtures
//! are verified against current provider API documentation (see baseline
//! verification in `docs/provider-api-verification-plan.md`).
//!
//! If a provider changes their API response format, update the fixture file
//! and re-verify against the provider's current documentation.

use serde_json::Value;
use std::path::{Path, PathBuf};

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("api_contracts")
        .join(name)
}

/// Load a JSON fixture from `tests/api_contracts/`.
fn load_json(name: &str) -> Value {
    let path = fixture_path(name);
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {}", path.display(), e));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("failed to parse fixture {}: {}", name, e))
}

/// Load an XML fixture and return the raw string.
fn load_xml(name: &str) -> String {
    let path = fixture_path(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {}", path.display(), e))
}

/// Assert a JSON value has an object key at the given path.
fn assert_has_key(val: &Value, path: &str) {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = val;
    for (i, part) in parts.iter().enumerate() {
        // Handle array indexing: "key[0]"
        if let Some(idx_start) = part.find('[') {
            let key = &part[..idx_start];
            let idx: usize = part[idx_start + 1..part.len() - 1]
                .parse()
                .unwrap_or_else(|_| panic!("invalid array index in path '{}'", path));
            current = current
                .get(key)
                .unwrap_or_else(|| panic!("missing key '{}' at '{}'", key, path));
            current = current
                .get(idx)
                .unwrap_or_else(|| panic!("missing index [{}] at '{}'", idx, path));
        } else {
            current = current.get(part).unwrap_or_else(|| {
                let traversed = parts[..i].join(".");
                panic!(
                    "missing key '{}' in fixture (traversed: '{}')",
                    part, traversed
                );
            });
        }
    }
}

// ── AWS ──────────────────────────────────────────────────────────────

#[test]
fn contract_aws_describe_instances() {
    let xml = load_xml("aws_describe_instances.xml");
    // Verify key XML elements are present.
    assert!(xml.contains("<DescribeInstancesResponse"));
    assert!(xml.contains("<reservationSet>"));
    assert!(xml.contains("<instancesSet>"));
    assert!(xml.contains("<instanceId>"));
    assert!(xml.contains("<imageId>"));
    assert!(xml.contains("<instanceState>"));
    assert!(xml.contains("<instanceType>"));
    assert!(xml.contains("<ipAddress>"));
    assert!(xml.contains("<privateIpAddress>"));
    assert!(xml.contains("<tagSet>"));
    assert!(xml.contains("<placement>"));
    assert!(xml.contains("<availabilityZone>"));
}

#[test]
fn contract_aws_describe_images() {
    let xml = load_xml("aws_describe_images.xml");
    assert!(xml.contains("<DescribeImagesResponse"));
    assert!(xml.contains("<imagesSet>"));
    assert!(xml.contains("<imageId>"));
    assert!(xml.contains("<name>"));
}

// ── Azure ────────────────────────────────────────────────────────────

#[test]
fn contract_azure_token() {
    let v = load_json("azure_token.json");
    assert_has_key(&v, "access_token");
    assert_has_key(&v, "token_type");
    assert_has_key(&v, "expires_in");
}

#[test]
fn contract_azure_vms() {
    let v = load_json("azure_vms.json");
    assert_has_key(&v, "value");
    let vm = &v["value"][0];
    assert_has_key(vm, "name");
    assert_has_key(vm, "location");
    assert_has_key(vm, "properties.vmId");
    assert_has_key(vm, "properties.hardwareProfile.vmSize");
    assert_has_key(vm, "properties.storageProfile.imageReference.offer");
    assert_has_key(vm, "properties.storageProfile.imageReference.sku");
    assert_has_key(vm, "properties.networkProfile.networkInterfaces");
    assert_has_key(vm, "properties.instanceView.statuses");
}

#[test]
fn contract_azure_nics() {
    let v = load_json("azure_nics.json");
    assert_has_key(&v, "value");
    let nic = &v["value"][0];
    assert_has_key(nic, "id");
    assert_has_key(nic, "properties.ipConfigurations");
    let ip_config = &nic["properties"]["ipConfigurations"][0];
    assert_has_key(ip_config, "properties.privateIPAddress");
    assert_has_key(ip_config, "properties.publicIPAddress.id");
    assert_has_key(ip_config, "properties.primary");
}

#[test]
fn contract_azure_public_ips() {
    let v = load_json("azure_public_ips.json");
    assert_has_key(&v, "value");
    let pip = &v["value"][0];
    assert_has_key(pip, "id");
    assert_has_key(pip, "properties.ipAddress");
}

// ── DigitalOcean ─────────────────────────────────────────────────────

#[test]
fn contract_digitalocean_droplets() {
    let v = load_json("digitalocean_droplets.json");
    assert_has_key(&v, "droplets");
    assert_has_key(&v, "meta.total");
    let droplet = &v["droplets"][0];
    assert_has_key(droplet, "id");
    assert_has_key(droplet, "name");
    assert_has_key(droplet, "networks.v4");
    assert_has_key(droplet, "tags");
    let net = &droplet["networks"]["v4"][0];
    assert_has_key(net, "ip_address");
    assert_has_key(net, "type");
}

// ── GCP ──────────────────────────────────────────────────────────────

#[test]
fn contract_gcp_token() {
    let v = load_json("gcp_token.json");
    assert_has_key(&v, "access_token");
    assert_has_key(&v, "token_type");
    assert_has_key(&v, "expires_in");
}

#[test]
fn contract_gcp_aggregated_instances() {
    let v = load_json("gcp_aggregated_instances.json");
    assert_has_key(&v, "items");
    // Items is a map of zone -> scoped list.
    let items = v["items"]
        .as_object()
        .unwrap_or_else(|| panic!("expected 'items' to be an object in gcp_aggregated_instances"));
    assert!(!items.is_empty(), "items map should not be empty");
    let zone = items
        .values()
        .next()
        .expect("items map should have at least one zone");
    assert_has_key(zone, "instances");
    let instance = &zone["instances"][0];
    assert_has_key(instance, "id");
    assert_has_key(instance, "name");
    assert_has_key(instance, "status");
    assert_has_key(instance, "machineType");
    assert_has_key(instance, "zone");
    assert_has_key(instance, "networkInterfaces");
    let nic = &instance["networkInterfaces"][0];
    assert_has_key(nic, "networkIP");
    assert_has_key(nic, "accessConfigs");
}

// ── Hetzner ──────────────────────────────────────────────────────────

#[test]
fn contract_hetzner_servers() {
    let v = load_json("hetzner_servers.json");
    assert_has_key(&v, "servers");
    assert_has_key(&v, "meta.pagination.page");
    assert_has_key(&v, "meta.pagination.last_page");
    let server = &v["servers"][0];
    assert_has_key(server, "id");
    assert_has_key(server, "name");
    assert_has_key(server, "public_net.ipv4.ip");
    assert_has_key(server, "labels");
    assert_has_key(server, "status");
    assert_has_key(server, "server_type.name");
    assert_has_key(server, "location.name");
}

// ── Linode ───────────────────────────────────────────────────────────

#[test]
fn contract_linode_instances() {
    let v = load_json("linode_instances.json");
    assert_has_key(&v, "data");
    assert_has_key(&v, "page");
    assert_has_key(&v, "pages");
    let instance = &v["data"][0];
    assert_has_key(instance, "id");
    assert_has_key(instance, "label");
    assert_has_key(instance, "ipv4");
    assert_has_key(instance, "tags");
    assert_has_key(instance, "status");
    assert_has_key(instance, "region");
}

// ── Oracle Cloud ─────────────────────────────────────────────────────

#[test]
fn contract_oracle_compartments() {
    let v = load_json("oracle_compartments.json");
    assert!(v.is_array(), "compartments should be an array");
    let comp = &v[0];
    assert_has_key(comp, "id");
    assert_has_key(comp, "lifecycleState");
}

#[test]
fn contract_oracle_instances() {
    let v = load_json("oracle_instances.json");
    assert!(v.is_array(), "instances should be an array");
    let inst = &v[0];
    assert_has_key(inst, "id");
    assert_has_key(inst, "displayName");
    assert_has_key(inst, "lifecycleState");
    assert_has_key(inst, "shape");
    assert_has_key(inst, "imageId");
    assert_has_key(inst, "freeformTags");
}

#[test]
fn contract_oracle_vnic_attachments() {
    let v = load_json("oracle_vnic_attachments.json");
    assert!(v.is_array(), "vnic attachments should be an array");
    let att = &v[0];
    assert_has_key(att, "instanceId");
    assert_has_key(att, "vnicId");
    assert_has_key(att, "lifecycleState");
    assert_has_key(att, "isPrimary");
}

#[test]
fn contract_oracle_vnic() {
    let v = load_json("oracle_vnic.json");
    assert_has_key(&v, "publicIp");
    assert_has_key(&v, "privateIp");
}

#[test]
fn contract_oracle_image() {
    let v = load_json("oracle_image.json");
    assert_has_key(&v, "displayName");
}

// ── OVHcloud ─────────────────────────────────────────────────────────

#[test]
fn contract_ovh_instances() {
    let v = load_json("ovh_instances.json");
    assert!(v.is_array(), "OVH instances should be an array");
    let inst = &v[0];
    assert_has_key(inst, "id");
    assert_has_key(inst, "name");
    assert_has_key(inst, "status");
    assert_has_key(inst, "region");
    assert_has_key(inst, "ipAddresses");
    let ip = &inst["ipAddresses"][0];
    assert_has_key(ip, "ip");
    assert_has_key(ip, "type");
    assert_has_key(ip, "version");
}

// ── Leaseweb ─────────────────────────────────────────────────────────

#[test]
fn contract_leaseweb_baremetal() {
    let v = load_json("leaseweb_baremetal.json");
    assert_has_key(&v, "servers");
    assert_has_key(&v, "_metadata.totalCount");
    assert_has_key(&v, "_metadata.limit");
    assert_has_key(&v, "_metadata.offset");
    let server = &v["servers"][0];
    assert_has_key(server, "id");
    assert_has_key(server, "networkInterfaces.public.ip");
    assert_has_key(server, "contract.deliveryStatus");
}

#[test]
fn contract_leaseweb_cloud() {
    let v = load_json("leaseweb_cloud.json");
    assert_has_key(&v, "instances");
    assert_has_key(&v, "_metadata.totalCount");
    let inst = &v["instances"][0];
    assert_has_key(inst, "id");
    assert_has_key(inst, "state");
    assert_has_key(inst, "ips");
    let ip = &inst["ips"][0];
    assert_has_key(ip, "ip");
    assert_has_key(ip, "version");
    assert_has_key(ip, "networkType");
}

// ── i3D.net ──────────────────────────────────────────────────────────

#[test]
fn contract_i3d_hosts() {
    let v = load_json("i3d_hosts.json");
    assert!(v.is_array(), "i3D hosts should be an array");
    let host = &v[0];
    assert_has_key(host, "id");
    assert_has_key(host, "serverId");
    assert_has_key(host, "serverName");
    assert_has_key(host, "ipAddress");
    let ip = &host["ipAddress"][0];
    assert_has_key(ip, "ipAddress");
    assert_has_key(ip, "version");
    assert_has_key(ip, "private");
}

#[test]
fn contract_i3d_flexmetal() {
    let v = load_json("i3d_flexmetal.json");
    assert!(v.is_array(), "i3D FlexMetal should be an array");
    let server = &v[0];
    assert_has_key(server, "uuid");
    assert_has_key(server, "name");
    assert_has_key(server, "status");
    assert_has_key(server, "os");
    // os.slug is the preferred field (fixed in v2.38.1).
    assert_has_key(server, "os.slug");
    assert_has_key(server, "ipAddresses");
    let ip = &server["ipAddresses"][0];
    assert_has_key(ip, "ip");
    assert_has_key(ip, "version");
    assert_has_key(ip, "public");
}

// ── Proxmox VE ───────────────────────────────────────────────────────

#[test]
fn contract_proxmox_cluster_resources() {
    let v = load_json("proxmox_cluster_resources.json");
    assert_has_key(&v, "data");
    let resources = v["data"]
        .as_array()
        .unwrap_or_else(|| panic!("expected 'data' to be an array in proxmox_cluster_resources"));
    assert!(!resources.is_empty());
    // Find a qemu resource.
    let qemu = resources
        .iter()
        .find(|r| r["type"] == "qemu")
        .expect("should have a qemu resource");
    assert_has_key(qemu, "type");
    assert_has_key(qemu, "vmid");
    assert_has_key(qemu, "name");
    assert_has_key(qemu, "node");
    assert_has_key(qemu, "status");
    assert_has_key(qemu, "template");
}

#[test]
fn contract_proxmox_qemu_config() {
    let v = load_json("proxmox_qemu_config.json");
    assert_has_key(&v, "data");
    let data = &v["data"];
    // VmConfig uses flattened deserialization for ipconfig*/net* fields.
    assert_has_key(data, "cores");
    assert_has_key(data, "ipconfig0");
    assert_has_key(data, "net0");
}

#[test]
fn contract_proxmox_guest_agent_network() {
    let v = load_json("proxmox_guest_agent_network.json");
    // Double-wrapped: data.result is the actual interface list.
    assert_has_key(&v, "data.result");
    let ifaces = v["data"]["result"].as_array().unwrap_or_else(|| {
        panic!("expected 'data.result' to be an array in proxmox_guest_agent_network")
    });
    assert!(!ifaces.is_empty());
    let iface = &ifaces[0];
    assert_has_key(iface, "name");
    assert_has_key(iface, "ip-addresses");
    let addr = &iface["ip-addresses"][0];
    assert_has_key(addr, "ip-address");
    assert_has_key(addr, "ip-address-type");
}

#[test]
fn contract_proxmox_guest_os_info() {
    let v = load_json("proxmox_guest_os_info.json");
    assert_has_key(&v, "data.result.pretty-name");
}

#[test]
fn contract_proxmox_lxc_interfaces() {
    let v = load_json("proxmox_lxc_interfaces.json");
    assert_has_key(&v, "data");
    let ifaces = v["data"]
        .as_array()
        .unwrap_or_else(|| panic!("expected 'data' to be an array in proxmox_lxc_interfaces"));
    assert!(!ifaces.is_empty());
    let iface = &ifaces[0];
    assert_has_key(iface, "name");
    // Legacy PVE format uses inet/inet6 CIDR strings.
    assert!(
        iface.get("inet").is_some() || iface.get("ip-addresses").is_some(),
        "LXC interface fixture must have inet or ip-addresses"
    );
}

// ── Scaleway ─────────────────────────────────────────────────────────

#[test]
fn contract_scaleway_servers() {
    let v = load_json("scaleway_servers.json");
    assert_has_key(&v, "servers");
    let server = &v["servers"][0];
    assert_has_key(server, "id");
    assert_has_key(server, "name");
    assert_has_key(server, "state");
    assert_has_key(server, "commercial_type");
    assert_has_key(server, "tags");
    assert_has_key(server, "public_ips");
    let ip = &server["public_ips"][0];
    assert_has_key(ip, "address");
    assert_has_key(ip, "family");
}

// ── Tailscale ────────────────────────────────────────────────────────

#[test]
fn contract_tailscale_api_devices() {
    let v = load_json("tailscale_api_devices.json");
    assert_has_key(&v, "devices");
    let device = &v["devices"][0];
    assert_has_key(device, "nodeId");
    assert_has_key(device, "hostname");
    assert_has_key(device, "name");
    assert_has_key(device, "addresses");
    assert_has_key(device, "os");
    assert_has_key(device, "tags");
}

#[test]
fn contract_tailscale_cli_status() {
    let v = load_json("tailscale_cli_status.json");
    assert_has_key(&v, "Peer");
    let peers = v["Peer"]
        .as_object()
        .unwrap_or_else(|| panic!("expected 'Peer' to be an object in tailscale_cli_status"));
    assert!(!peers.is_empty());
    let peer = peers
        .values()
        .next()
        .expect("Peer map should have at least one entry");
    assert_has_key(peer, "HostName");
    assert_has_key(peer, "TailscaleIPs");
    assert_has_key(peer, "OS");
    assert_has_key(peer, "Online");
}

// ── TransIP ──────────────────────────────────────────────────────────

#[test]
fn contract_transip_token() {
    let v = load_json("transip_token.json");
    assert_has_key(&v, "token");
}

#[test]
fn contract_transip_vps() {
    let v = load_json("transip_vps.json");
    assert_has_key(&v, "vpss");
    let vps = &v["vpss"][0];
    assert_has_key(vps, "name");
    assert_has_key(vps, "uuid");
    assert_has_key(vps, "status");
    assert_has_key(vps, "ipAddress");
    assert_has_key(vps, "tags");
}

// ── UpCloud ──────────────────────────────────────────────────────────

#[test]
fn contract_upcloud_servers() {
    let v = load_json("upcloud_servers.json");
    assert_has_key(&v, "servers.server");
    let server = &v["servers"]["server"][0];
    assert_has_key(server, "uuid");
    assert_has_key(server, "title");
    assert_has_key(server, "hostname");
}

#[test]
fn contract_upcloud_server_detail() {
    let v = load_json("upcloud_server_detail.json");
    assert_has_key(&v, "server.networking.interfaces.interface");
    let iface = &v["server"]["networking"]["interfaces"]["interface"][0];
    assert_has_key(iface, "type");
    assert_has_key(iface, "ip_addresses.ip_address");
    let addr = &iface["ip_addresses"]["ip_address"][0];
    assert_has_key(addr, "address");
    assert_has_key(addr, "family");
}

// ── Vultr ────────────────────────────────────────────────────────────

#[test]
fn contract_vultr_instances() {
    let v = load_json("vultr_instances.json");
    assert_has_key(&v, "instances");
    assert_has_key(&v, "meta.links.next");
    let inst = &v["instances"][0];
    assert_has_key(inst, "id");
    assert_has_key(inst, "label");
    assert_has_key(inst, "main_ip");
    assert_has_key(inst, "tags");
}

// ── Fixture completeness ─────────────────────────────────────────────

/// Verify all expected fixture files exist.
#[test]
fn contract_all_fixtures_present() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("api_contracts");

    let expected = [
        // AWS (XML)
        "aws_describe_instances.xml",
        "aws_describe_images.xml",
        // Azure
        "azure_token.json",
        "azure_vms.json",
        "azure_nics.json",
        "azure_public_ips.json",
        // DigitalOcean
        "digitalocean_droplets.json",
        // GCP
        "gcp_token.json",
        "gcp_aggregated_instances.json",
        // Hetzner
        "hetzner_servers.json",
        // Linode
        "linode_instances.json",
        // Oracle
        "oracle_compartments.json",
        "oracle_instances.json",
        "oracle_vnic_attachments.json",
        "oracle_vnic.json",
        "oracle_image.json",
        // OVH
        "ovh_instances.json",
        // Leaseweb
        "leaseweb_baremetal.json",
        "leaseweb_cloud.json",
        // i3D
        "i3d_hosts.json",
        "i3d_flexmetal.json",
        // Proxmox
        "proxmox_cluster_resources.json",
        "proxmox_qemu_config.json",
        "proxmox_guest_agent_network.json",
        "proxmox_guest_os_info.json",
        "proxmox_lxc_interfaces.json",
        // Scaleway
        "scaleway_servers.json",
        // Tailscale
        "tailscale_api_devices.json",
        "tailscale_cli_status.json",
        // TransIP
        "transip_token.json",
        "transip_vps.json",
        // UpCloud
        "upcloud_servers.json",
        "upcloud_server_detail.json",
        // Vultr
        "vultr_instances.json",
    ];

    let mut missing = Vec::new();
    for name in &expected {
        if !dir.join(name).exists() {
            missing.push(*name);
        }
    }
    assert!(missing.is_empty(), "missing fixture files: {:?}", missing);
}
