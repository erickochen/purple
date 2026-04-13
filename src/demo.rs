use std::path::PathBuf;

use crate::app::{App, GroupBy, PingStatus, SortMode, SyncRecord, ViewMode};
use crate::containers;
use crate::history::ConnectionHistory;
use crate::providers::config::ProviderConfig;
use crate::snippet::SnippetStore;
use crate::ssh_config::model::SshConfigFile;
use crate::ssh_keys::SshKeyInfo;

const DEMO_SSH_CONFIG: &str = "\
# Infrastructure

Host bastion-ams
  HostName 140.82.121.3
  User ops
  DynamicForward 1080
  LocalForward 8443 internal.corp:443
  IdentityFile ~/.ssh/id_ed25519
  # purple:tags production,vpn
  # purple:askpass keychain

Host db-primary
  HostName 10.30.1.5
  User postgres
  Port 5433
  ProxyJump bastion-ams
  LocalForward 5432 localhost:5432
  LocalForward 9090 localhost:9090
  # purple:tags production,database
  # purple:askpass op://vault/prod-db

Host monitoring
  HostName 10.30.2.10
  User admin
  ProxyJump bastion-ams
  LocalForward 3000 localhost:3000
  # purple:tags production,monitoring

Host gateway-vpn
  HostName 185.199.108.5
  User openvpn
  # purple:tags infra,vpn
  # purple:vault-ssh ssh-client-signer/sign/infra

# AWS EC2

Host aws-api-prod
  HostName 52.47.100.23
  User ec2-user
  ProxyJump bastion-ams
  IdentityFile ~/.ssh/id_ed25519
  # purple:tags production,api
  # purple:provider aws:i-0a1b2c3d4e5f60001
  # purple:meta region=us-east-1,instance=t3.medium,os=Amazon Linux 2023,status=running
  # purple:vault-ssh ssh-client-signer/sign/admin

Host aws-api-staging
  HostName 52.47.100.24
  User ec2-user
  # purple:tags staging,api
  # purple:provider aws:i-0a1b2c3d4e5f60002
  # purple:meta region=us-east-1,instance=t3.small,os=Amazon Linux 2023,status=running

Host aws-worker-eu
  HostName 3.120.55.17
  User ec2-user
  # purple:tags production,worker
  # purple:provider aws:i-0a1b2c3d4e5f60003
  # purple:meta region=eu-central-1,instance=c6i.large,os=Ubuntu 22.04,status=running

Host aws-batch-us
  HostName 52.47.100.25
  User ec2-user
  # purple:tags production,batch
  # purple:provider aws:i-0a1b2c3d4e5f60004
  # purple:meta region=us-east-1,instance=c6i.xlarge,os=Amazon Linux 2023,status=running

Host aws-ml-eu
  HostName 3.120.55.18
  User ec2-user
  # purple:tags ml
  # purple:provider aws:i-0a1b2c3d4e5f60005
  # purple:meta region=eu-central-1,instance=g5.xlarge,os=Ubuntu 22.04,status=running

Host aws-cache-eu
  HostName 3.120.55.19
  User ec2-user
  # purple:tags cache
  # purple:provider aws:i-0a1b2c3d4e5f60006
  # purple:meta region=eu-central-1,instance=r6i.large,os=Amazon Linux 2023,status=stopped
  # purple:stale 1743800000

# DigitalOcean

Host do-web-ams
  HostName 104.248.38.91
  User deploy
  # purple:tags production,web
  # purple:provider digitalocean:382010
  # purple:meta region=ams3,size=s-2vcpu-4gb,image=Ubuntu 22.04,status=active

Host do-staging-ams
  HostName 104.248.38.92
  User deploy
  LocalForward 5432 localhost:5432
  # purple:tags staging,web
  # purple:provider digitalocean:382011
  # purple:meta region=ams3,size=s-2vcpu-4gb,image=Ubuntu 22.04,status=active

Host do-worker-ams
  HostName 104.248.38.93
  User deploy
  # purple:tags worker
  # purple:provider digitalocean:382012
  # purple:meta region=ams3,size=s-1vcpu-2gb,image=Ubuntu 22.04,status=active

Host do-ci-runner
  HostName 104.248.38.94
  User gitlab
  ProxyJump bastion-ams
  # purple:tags ci
  # purple:provider digitalocean:382013
  # purple:meta region=ams3,size=s-4vcpu-8gb,image=Ubuntu 22.04,status=active

# Proxmox VE

Host pve-web-01
  HostName 192.168.1.20
  User root
  # purple:tags web,internal
  # purple:provider proxmox:100
  # purple:meta node=pve1,type=qemu,specs=4c/8GiB,os=Debian 12,status=running

Host pve-web-02
  HostName 192.168.1.21
  User root
  # purple:tags web,internal
  # purple:provider proxmox:101
  # purple:meta node=pve1,type=qemu,specs=4c/8GiB,os=Debian 12,status=running

Host pve-db-01
  HostName 192.168.1.30
  User postgres
  LocalForward 5432 localhost:5432
  # purple:tags database,internal
  # purple:provider proxmox:102
  # purple:meta node=pve1,type=qemu,specs=8c/32GiB,os=Debian 12,status=running

Host pve-db-02
  HostName 192.168.1.31
  User postgres
  # purple:tags database,internal
  # purple:provider proxmox:103
  # purple:meta node=pve2,type=qemu,specs=8c/32GiB,os=Debian 12,status=running

Host pve-redis
  HostName 192.168.1.40
  User redis
  LocalForward 6379 localhost:6379
  # purple:tags cache,internal
  # purple:provider proxmox:104
  # purple:meta node=pve1,type=lxc,specs=2c/4GiB,os=Debian 12,status=running

Host pve-mail
  HostName 192.168.1.50
  User mail
  # purple:tags mail,internal
  # purple:provider proxmox:105
  # purple:meta node=pve2,type=lxc,specs=2c/4GiB,os=Debian 12,status=running

Host pve-monitor
  HostName 192.168.1.60
  User admin
  LocalForward 3000 localhost:3000
  LocalForward 9090 localhost:9090
  # purple:tags monitoring,internal
  # purple:provider proxmox:106
  # purple:meta node=pve2,type=qemu,specs=4c/8GiB,os=Ubuntu 22.04,status=running

Host pve-backup
  HostName 192.168.1.70
  User backup
  # purple:tags backup,internal
  # purple:provider proxmox:107
  # purple:meta node=pve2,type=lxc,specs=2c/8GiB,os=Debian 12,status=stopped
  # purple:stale 1743800000
";

const DEMO_SNIPPETS: &str = "\
[uptime]
command=uptime
description=Server uptime and load

[disk-usage]
command=df -h /
description=Root disk usage

[docker-ps]
command=docker ps --format 'table {{.Names}}\\t{{.Status}}'
description=Running containers

[tail-logs]
command=tail -n 50 /var/log/syslog
description=Last 50 syslog lines

[restart-nginx]
command=sudo systemctl restart nginx
description=Restart nginx service
";

const DEMO_PROVIDERS: &str = "\
[aws]
token=
alias_prefix=aws
user=ec2-user
profile=production
regions=us-east-1,eu-central-1
auto_sync=true
vault_role=ssh-client-signer/sign/engineer

[digitalocean]
token=dop_v1_demo
alias_prefix=do
user=deploy
auto_sync=true

[proxmox]
url=https://192.168.1.10:8006
token=root@pam!demo=xxx
alias_prefix=pve
user=root
vault_role=ssh-client-signer/sign/ops
vault_addr=http://localhost:8200
auto_sync=true
";

/// Generate demo history with timestamps relative to now.
/// Each entry: (alias, total_connections, spread_days).
const DEMO_HISTORY_SPEC: &[(&str, u32, u64)] = &[
    ("bastion-ams", 247, 300),
    ("db-primary", 142, 250),
    ("monitoring", 121, 280),
    ("gateway-vpn", 31, 300),
    ("aws-api-prod", 180, 300),
    ("aws-api-staging", 90, 200),
    ("aws-worker-eu", 65, 180),
    ("aws-batch-us", 160, 300),
    ("aws-ml-eu", 25, 80),
    ("aws-cache-eu", 8, 40),
    ("do-web-ams", 130, 250),
    ("do-staging-ams", 76, 180),
    ("do-worker-ams", 50, 150),
    ("do-ci-runner", 95, 200),
    ("pve-web-01", 110, 280),
    ("pve-web-02", 85, 250),
    ("pve-db-01", 70, 200),
    ("pve-db-02", 35, 120),
    ("pve-redis", 40, 100),
    ("pve-mail", 18, 90),
    ("pve-monitor", 60, 200),
    ("pve-backup", 6, 120),
];

fn build_demo_history() -> ConnectionHistory {
    use std::collections::HashMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs();
    let day: u64 = 86400;
    let hour: u64 = 3600;

    let mut entries = HashMap::new();
    // Generate timestamps with realistic variation: bursts of activity
    // mixed with quiet periods, creating interesting sparkline shapes.
    // Uses a simple LCG (linear congruential generator) for determinism.
    // last_connected is set to now minus a small offset based on spec order,
    // so hosts with higher count (earlier in spec) sort first in frecency.
    for (spec_idx, &(alias, count, spread_days)) in DEMO_HISTORY_SPEC.iter().enumerate() {
        let mut timestamps = Vec::with_capacity(count as usize);
        // Seed from alias for per-host variation
        let mut rng: u64 = alias.bytes().fold(0u64, |acc, b| {
            acc.wrapping_mul(31).wrapping_add(u64::from(b))
        });
        for i in 0..count {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            // Base spread with jitter: clustered in recent weeks, sparse further back
            let base_days = (u64::from(i + 1) * spread_days) / u64::from(count + 1);
            let jitter_days = (rng >> 33) % (spread_days / 8 + 1);
            let days_ago = base_days.saturating_add(jitter_days).min(spread_days);
            // Working hours with variation
            let work_hour = 7 + (rng >> 40) % 13; // 7..19
            let minute = (rng >> 48) % 60;
            let ts = now.saturating_sub(days_ago * day) + work_hour * hour + minute * 60;
            let ts = ts.min(now);
            timestamps.push(ts);
        }
        timestamps.sort_unstable();
        timestamps.reverse();
        timestamps.dedup();
        // Override last_connected based on spec order so sort-by-recent is deterministic.
        // First entry in DEMO_HISTORY_SPEC = most recently connected.
        let last_connected = now - (spec_idx as u64) * hour;
        if let Some(first) = timestamps.first_mut() {
            *first = last_connected;
        }
        entries.insert(
            alias.to_string(),
            crate::history::HistoryEntry {
                alias: alias.to_string(),
                last_connected,
                count,
                timestamps,
            },
        );
    }
    ConnectionHistory::from_entries(entries)
}

fn build_demo_sync_history() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs();
    // All synced just now (within last few seconds)
    format!(
        "aws\t{now}\t0\tSynced 6 hosts (2 regions)\n\
         digitalocean\t{now}\t0\tSynced 4 hosts\n\
         proxmox\t{now}\t0\tSynced 8 VMs",
    )
}

fn build_demo_container_cache() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs();
    let ts1 = now - 1200;
    let ts2 = now - 900;
    let ts3 = now - 600;
    let ts4 = now - 1500;
    let ts5 = now - 1000;
    let ts6 = now - 800;
    let ts7 = now - 1100;
    format!(
        r#"{{"alias":"bastion-ams","timestamp":{},"runtime":"Docker","containers":[{{"ID":"f1a2b3c4d5e6","Names":"nginx-proxy","Image":"nginx:1.25-alpine","State":"running","Status":"Up 12 days","Ports":"0.0.0.0:80->80/tcp, 0.0.0.0:443->443/tcp"}},{{"ID":"a2b3c4d5e6f7","Names":"app-backend","Image":"myapp:v2.14.1","State":"running","Status":"Up 12 days","Ports":"127.0.0.1:8080->8080/tcp"}},{{"ID":"b3c4d5e6f7a8","Names":"redis","Image":"redis:7-alpine","State":"running","Status":"Up 12 days","Ports":"127.0.0.1:6379->6379/tcp"}},{{"ID":"c4d5e6f7a8b9","Names":"postgres","Image":"postgres:16-alpine","State":"running","Status":"Up 12 days","Ports":"127.0.0.1:5432->5432/tcp"}},{{"ID":"d5e6f7a8b9c0","Names":"prometheus","Image":"prom/prometheus:v2.48","State":"running","Status":"Up 5 days","Ports":"127.0.0.1:9090->9090/tcp"}},{{"ID":"e6f7a8b9c0d1","Names":"grafana","Image":"grafana/grafana:10.2","State":"running","Status":"Up 5 days","Ports":"127.0.0.1:3000->3000/tcp"}},{{"ID":"f7a8b9c0d1e2","Names":"certbot","Image":"certbot/certbot:v2.7","State":"exited","Status":"Exited (0) 2 days ago","Ports":""}}]}}
{{"alias":"db-primary","timestamp":{},"runtime":"Docker","containers":[{{"ID":"a8b9c0d1e2f3","Names":"postgres-primary","Image":"postgres:16-alpine","State":"running","Status":"Up 30 days","Ports":"127.0.0.1:5432->5432/tcp"}},{{"ID":"b9c0d1e2f3a4","Names":"pgbouncer","Image":"pgbouncer:1.21","State":"running","Status":"Up 30 days","Ports":"127.0.0.1:6432->6432/tcp"}},{{"ID":"c0d1e2f3a4b5","Names":"pg-exporter","Image":"prometheuscommunity/postgres-exporter:0.15","State":"running","Status":"Up 30 days","Ports":"127.0.0.1:9187->9187/tcp"}}]}}
{{"alias":"do-web-ams","timestamp":{},"runtime":"Docker","containers":[{{"ID":"d1e2f3a4b5c6","Names":"nginx","Image":"nginx:1.25","State":"running","Status":"Up 8 days","Ports":"0.0.0.0:80->80/tcp, 0.0.0.0:443->443/tcp"}},{{"ID":"e2f3a4b5c6d7","Names":"app","Image":"myapp:3.2.1","State":"running","Status":"Up 8 days","Ports":"8080/tcp"}},{{"ID":"f3a4b5c6d7e8","Names":"worker","Image":"myapp:3.2.1","State":"running","Status":"Up 8 days","Ports":""}},{{"ID":"a4b5c6d7e8f9","Names":"redis","Image":"redis:7-alpine","State":"running","Status":"Up 8 days","Ports":"6379/tcp"}},{{"ID":"b5c6d7e8f9a0","Names":"sidekiq","Image":"myapp:3.2.1","State":"exited","Status":"Exited (1) 3 hours ago","Ports":""}}]}}
{{"alias":"pve-web-01","timestamp":{},"runtime":"Docker","containers":[{{"ID":"c6d7e8f9a0b1","Names":"nginx","Image":"nginx:1.25","State":"running","Status":"Up 20 days","Ports":"0.0.0.0:80->80/tcp, 0.0.0.0:443->443/tcp"}},{{"ID":"d7e8f9a0b1c2","Names":"webapp","Image":"internal/webapp:1.8.3","State":"running","Status":"Up 20 days","Ports":"127.0.0.1:3000->3000/tcp"}},{{"ID":"e8f9a0b1c2d3","Names":"celery","Image":"internal/webapp:1.8.3","State":"running","Status":"Up 20 days","Ports":""}}]}}
{{"alias":"aws-api-staging","timestamp":{},"runtime":"Docker","containers":[{{"ID":"f9a0b1c2d3e4","Names":"api","Image":"myteam/api:v4.1.0-rc2","State":"running","Status":"Up 2 days","Ports":"0.0.0.0:8080->8080/tcp"}},{{"ID":"a0b1c2d3e4f5","Names":"nginx","Image":"nginx:1.25-alpine","State":"running","Status":"Up 2 days","Ports":"0.0.0.0:443->443/tcp"}},{{"ID":"b1c2d3e4f5a6","Names":"datadog-agent","Image":"datadog/agent:7","State":"running","Status":"Up 2 days","Ports":""}},{{"ID":"c2d3e4f5a6b7","Names":"redis","Image":"redis:7-alpine","State":"running","Status":"Up 2 days","Ports":"127.0.0.1:6379->6379/tcp"}}]}}
{{"alias":"aws-batch-us","timestamp":{},"runtime":"Docker","containers":[{{"ID":"d3e4f5a6b7c8","Names":"scheduler","Image":"myteam/batch:2.9.0","State":"running","Status":"Up 14 days","Ports":"127.0.0.1:8080->8080/tcp"}},{{"ID":"e4f5a6b7c8d9","Names":"worker-1","Image":"myteam/batch:2.9.0","State":"running","Status":"Up 14 days","Ports":""}},{{"ID":"f5a6b7c8d9e0","Names":"worker-2","Image":"myteam/batch:2.9.0","State":"running","Status":"Up 14 days","Ports":""}},{{"ID":"a6b7c8d9e0f1","Names":"rabbitmq","Image":"rabbitmq:3.13-management","State":"running","Status":"Up 14 days","Ports":"127.0.0.1:5672->5672/tcp, 127.0.0.1:15672->15672/tcp"}},{{"ID":"b7c8d9e0f1a2","Names":"flower","Image":"mher/flower:2.0","State":"running","Status":"Up 14 days","Ports":"127.0.0.1:5555->5555/tcp"}}]}}
{{"alias":"gateway-vpn","timestamp":{},"runtime":"Docker","containers":[{{"ID":"c8d9e0f1a2b3","Names":"wireguard","Image":"linuxserver/wireguard:1.0","State":"running","Status":"Up 45 days","Ports":"0.0.0.0:51820->51820/udp"}},{{"ID":"d9e0f1a2b3c4","Names":"pihole","Image":"pihole/pihole:2024.07","State":"running","Status":"Up 45 days","Ports":"0.0.0.0:53->53/tcp, 0.0.0.0:53->53/udp, 127.0.0.1:8080->80/tcp"}},{{"ID":"e0f1a2b3c4d5","Names":"unbound","Image":"mvance/unbound:1.20","State":"running","Status":"Up 45 days","Ports":"127.0.0.1:5335->5335/tcp"}}]}}"#,
        ts1, ts2, ts3, ts4, ts5, ts6, ts7,
    )
}

pub fn build_demo_app() -> App {
    crate::demo_flag::enable();

    let config = SshConfigFile::from_content(DEMO_SSH_CONFIG, PathBuf::from("/demo/ssh/config"));
    let mut app = App::new(config);
    app.demo_mode = true;

    // History (timestamps relative to now for sparkline visibility)
    app.history = build_demo_history();

    // Provider config — replace disk-loaded config with demo-only providers
    app.provider_config = ProviderConfig::parse(DEMO_PROVIDERS);

    // Sync history (timestamps relative to now)
    app.sync_history = SyncRecord::load_from_content(&build_demo_sync_history());

    // Snippets
    app.snippet_store = SnippetStore::parse(DEMO_SNIPPETS);

    // Container cache (timestamps relative to now)
    app.container_cache = containers::parse_container_cache_content(&build_demo_container_cache());

    // Ping status (deterministic)
    let reachable = |ms| PingStatus::Reachable { rtt_ms: ms };
    // Ungrouped hosts
    app.ping.status.insert("bastion-ams".into(), reachable(7));
    app.ping.status.insert("gateway-vpn".into(), reachable(11));
    // ProxyJump hosts (normally skipped by pinger, forced reachable for demo)
    app.ping.status.insert("db-primary".into(), reachable(5));
    app.ping.status.insert("monitoring".into(), reachable(8));
    // AWS
    app.ping.status.insert("aws-api-prod".into(), reachable(89));
    app.ping
        .status
        .insert("aws-api-staging".into(), reachable(92));
    app.ping
        .status
        .insert("aws-worker-eu".into(), reachable(23));
    app.ping.status.insert("aws-batch-us".into(), reachable(18));
    app.ping.status.insert("aws-ml-eu".into(), reachable(25));
    app.ping
        .status
        .insert("aws-cache-eu".into(), PingStatus::Unreachable);
    // DigitalOcean
    app.ping.status.insert("do-web-ams".into(), reachable(12));
    app.ping
        .status
        .insert("do-staging-ams".into(), reachable(14));
    app.ping
        .status
        .insert("do-worker-ams".into(), reachable(15));
    app.ping.status.insert("do-ci-runner".into(), reachable(42));
    // Proxmox
    app.ping.status.insert("pve-web-01".into(), reachable(3));
    app.ping.status.insert("pve-web-02".into(), reachable(3));
    app.ping.status.insert("pve-db-01".into(), reachable(2));
    app.ping.status.insert("pve-db-02".into(), reachable(2));
    app.ping.status.insert("pve-redis".into(), reachable(2));
    app.ping.status.insert("pve-mail".into(), reachable(3));
    app.ping.status.insert("pve-monitor".into(), reachable(3));
    app.ping
        .status
        .insert("pve-backup".into(), PingStatus::Unreachable);

    app.ping.has_pinged = true;
    app.ping.checked_at = Some(std::time::Instant::now());

    // Vault SSH cert status (deterministic demo data)
    {
        use crate::vault_ssh::CertStatus;
        let now = std::time::Instant::now();
        // aws-api-prod: valid cert, 6h remaining out of 8h total
        app.vault.cert_cache.insert(
            "aws-api-prod".into(),
            (
                now,
                CertStatus::Valid {
                    expires_at: 0,
                    remaining_secs: 21600,
                    total_secs: 28800,
                },
                None,
            ),
        );
        // aws-worker-eu: valid cert, 45m remaining out of 8h (warning tier)
        app.vault.cert_cache.insert(
            "aws-worker-eu".into(),
            (
                now,
                CertStatus::Valid {
                    expires_at: 0,
                    remaining_secs: 2700,
                    total_secs: 28800,
                },
                None,
            ),
        );
        // aws-batch-us: valid cert, 4h remaining out of 8h
        app.vault.cert_cache.insert(
            "aws-batch-us".into(),
            (
                now,
                CertStatus::Valid {
                    expires_at: 0,
                    remaining_secs: 14400,
                    total_secs: 28800,
                },
                None,
            ),
        );
        // gateway-vpn: valid cert, 7h remaining out of 8h
        app.vault.cert_cache.insert(
            "gateway-vpn".into(),
            (
                now,
                CertStatus::Valid {
                    expires_at: 0,
                    remaining_secs: 25200,
                    total_secs: 28800,
                },
                None,
            ),
        );
        // pve-web-01: valid cert, 3h remaining out of 8h
        app.vault.cert_cache.insert(
            "pve-web-01".into(),
            (
                now,
                CertStatus::Valid {
                    expires_at: 0,
                    remaining_secs: 10800,
                    total_secs: 28800,
                },
                None,
            ),
        );
        // Others left as Missing (Not signed) for variety
    }

    // SSH keys (fake metadata)
    app.keys = vec![
        SshKeyInfo {
            name: "id_ed25519".into(),
            display_path: "~/.ssh/id_ed25519".into(),
            key_type: "ED25519".into(),
            bits: "256".into(),
            fingerprint: "SHA256:dGVzdGRlbW9rZXlmb3JwdXJwbGVzc2g".into(),
            comment: "ops@bastion".into(),
            linked_hosts: vec![
                "bastion-ams".into(),
                "aws-api-prod".into(),
                "aws-api-staging".into(),
            ],
        },
        SshKeyInfo {
            name: "id_rsa".into(),
            display_path: "~/.ssh/id_rsa".into(),
            key_type: "RSA".into(),
            bits: "4096".into(),
            fingerprint: "SHA256:cnNhdGVzdGtleWZvcnB1cnBsZXNzaGRl".into(),
            comment: "deploy@legacy".into(),
            linked_hosts: vec![],
        },
    ];

    // Preferences
    app.view_mode = ViewMode::Compact;
    app.sort_mode = SortMode::MostRecent;
    app.group_by = GroupBy::None;
    app.ping.auto_ping = true;

    // Rebuild display list with sort/group applied
    app.apply_sort();
    app.select_first_host();

    app
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::MutexGuard;

    /// Serialise all demo tests so the global `DEMO_MODE` AtomicBool never
    /// leaks into a concurrent test that exercises disk-write paths.
    static DEMO_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// RAII guard that holds the serialisation lock and resets the global demo
    /// flag on drop (including panics). The MutexGuard is held (not read) to
    /// keep the lock alive for the duration of the test.
    struct DemoGuard(#[allow(dead_code)] MutexGuard<'static, ()>);

    impl Drop for DemoGuard {
        fn drop(&mut self) {
            crate::demo_flag::disable();
        }
    }

    /// Build demo app with serialisation lock + RAII guard to reset global flag.
    fn demo_app() -> (App, DemoGuard) {
        let lock = DEMO_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let app = build_demo_app();
        (app, DemoGuard(lock))
    }

    #[test]
    fn demo_app_has_expected_hosts() {
        let (app, _guard) = demo_app();
        assert_eq!(app.hosts.len(), 22);
    }

    #[test]
    fn demo_app_has_providers() {
        let (app, _guard) = demo_app();
        assert_eq!(app.provider_config.configured_providers().len(), 3);
    }

    #[test]
    fn demo_app_has_history() {
        let (app, _guard) = demo_app();
        assert_eq!(app.history.entries.len(), 22);
    }

    #[test]
    fn demo_app_has_snippets() {
        let (app, _guard) = demo_app();
        assert_eq!(app.snippet_store.snippets.len(), 5);
    }

    #[test]
    fn demo_app_has_containers() {
        let (app, _guard) = demo_app();
        assert_eq!(app.container_cache.len(), 7);
        assert!(app.container_cache.contains_key("bastion-ams"));
        assert!(app.container_cache.contains_key("db-primary"));
        assert!(app.container_cache.contains_key("do-web-ams"));
        assert!(app.container_cache.contains_key("pve-web-01"));
        assert!(app.container_cache.contains_key("aws-api-staging"));
        assert!(app.container_cache.contains_key("aws-batch-us"));
        assert!(app.container_cache.contains_key("gateway-vpn"));
    }

    #[test]
    fn demo_app_has_ping_status() {
        let (app, _guard) = demo_app();
        assert!(app.ping.has_pinged);
        assert!(app.ping.checked_at.is_some());
        assert_eq!(
            app.ping.status.get("bastion-ams"),
            Some(&PingStatus::Reachable { rtt_ms: 7 })
        );
        assert_eq!(
            app.ping.status.get("aws-cache-eu"),
            Some(&PingStatus::Unreachable)
        );
        assert_eq!(
            app.ping.status.get("pve-backup"),
            Some(&PingStatus::Unreachable)
        );
        assert_eq!(
            app.ping.status.get("monitoring"),
            Some(&PingStatus::Reachable { rtt_ms: 8 })
        );
    }

    #[test]
    fn demo_app_has_keys() {
        let (app, _guard) = demo_app();
        assert_eq!(app.keys.len(), 2);
    }

    #[test]
    fn demo_app_has_sync_history() {
        let (app, _guard) = demo_app();
        assert_eq!(app.sync_history.len(), 3);
    }

    #[test]
    fn demo_mode_flag_is_set() {
        let (app, _guard) = demo_app();
        assert!(app.demo_mode);
    }

    #[test]
    fn demo_app_has_vault_ssh_config() {
        let (app, _guard) = demo_app();
        // Two providers have vault_role (inheritance for their hosts).
        let aws = app.provider_config.section("aws").expect("aws section");
        assert!(
            !aws.vault_role.is_empty(),
            "aws provider should have vault_role set"
        );
        let pve = app
            .provider_config
            .section("proxmox")
            .expect("proxmox section");
        assert!(
            !pve.vault_role.is_empty(),
            "proxmox provider should have vault_role set"
        );
        assert!(
            !pve.vault_addr.is_empty(),
            "proxmox provider should have vault_addr set"
        );
        // At least one host has a per-host vault_ssh override.
        let override_host = app
            .hosts
            .iter()
            .find(|h| h.vault_ssh.as_deref().is_some_and(|s| !s.is_empty()));
        assert!(
            override_host.is_some(),
            "demo should have a host with a vault_ssh override"
        );
    }

    #[test]
    fn demo_app_has_stale_hosts() {
        let (app, _guard) = demo_app();
        let cache = app.hosts.iter().find(|h| h.alias == "aws-cache-eu");
        assert!(cache.is_some());
        assert!(cache.unwrap().stale.is_some());
        let backup = app.hosts.iter().find(|h| h.alias == "pve-backup");
        assert!(backup.is_some());
        assert!(backup.unwrap().stale.is_some());
    }

    #[test]
    fn demo_sorted_provider_names() {
        let (app, _guard) = demo_app();
        let names = app.sorted_provider_names();
        // First 3 should be our configured providers (with sync history)
        let configured: Vec<&str> = names.iter().take(3).map(|s| s.as_str()).collect();
        assert!(
            configured.contains(&"aws"),
            "aws missing from top 3: {:?}",
            configured
        );
        assert!(
            configured.contains(&"digitalocean"),
            "digitalocean missing from top 3: {:?}",
            configured
        );
        assert!(
            configured.contains(&"proxmox"),
            "proxmox missing from top 3: {:?}",
            configured
        );
        // No other provider should have a checkmark (be configured)
        for name in &names[3..] {
            assert!(
                app.provider_config.section(name).is_none(),
                "unexpected configured provider: {}",
                name
            );
        }
    }

    #[test]
    fn demo_app_has_correct_preferences() {
        let (app, _guard) = demo_app();
        assert_eq!(app.view_mode, ViewMode::Compact);
        assert_eq!(app.sort_mode, SortMode::MostRecent);
        assert_eq!(app.group_by, GroupBy::None);
        assert!(app.ping.auto_ping);
        assert!(!app.display_list.is_empty());
    }
}
