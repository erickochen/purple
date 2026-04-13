use super::*;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[test]
fn sparkline_empty_timestamps() {
    let result = activity_sparkline(&[], 40);
    assert!(result.is_empty());
}

fn directive(key: &str, value: &str) -> crate::ssh_config::model::Directive {
    crate::ssh_config::model::Directive {
        key: key.to_string(),
        value: value.to_string(),
        raw_line: format!("    {} {}", key, value),
        is_non_directive: false,
    }
}

fn host_element(
    alias: &str,
    directives: Vec<crate::ssh_config::model::Directive>,
) -> ConfigElement {
    ConfigElement::HostBlock(crate::ssh_config::model::HostBlock {
        host_pattern: alias.to_string(),
        raw_host_line: format!("Host {}", alias),
        directives,
    })
}

#[test]
fn tunnel_rules_format_local_forward_with_arrow() {
    let elements = vec![host_element(
        "db",
        vec![directive("LocalForward", "8200 10.30.0.3:8200")],
    )];
    let rules = find_tunnel_rules(&elements, "db");
    assert_eq!(rules, vec!["L 8200 \u{2192} 10.30.0.3:8200"]);
}

#[test]
fn tunnel_rules_format_remote_forward_with_arrow() {
    let elements = vec![host_element(
        "web",
        vec![directive("RemoteForward", "9090 127.0.0.1:9090")],
    )];
    let rules = find_tunnel_rules(&elements, "web");
    assert_eq!(rules, vec!["R 9090 \u{2192} 127.0.0.1:9090"]);
}

#[test]
fn tunnel_rules_dynamic_forward_has_no_arrow() {
    let elements = vec![host_element(
        "socks",
        vec![directive("DynamicForward", "1080")],
    )];
    let rules = find_tunnel_rules(&elements, "socks");
    assert_eq!(rules, vec!["D 1080"]);
}

#[test]
fn tunnel_rules_ipv6_bracketed_bind_address() {
    let elements = vec![host_element(
        "v6",
        vec![directive("LocalForward", "[::1]:8200 [::1]:8200")],
    )];
    let rules = find_tunnel_rules(&elements, "v6");
    assert_eq!(rules, vec!["L [::1]:8200 \u{2192} [::1]:8200"]);
}

#[test]
fn tunnel_rules_tab_separator_between_src_and_dst() {
    let elements = vec![host_element(
        "tabbed",
        vec![directive("LocalForward", "8200\t10.30.0.3:8200")],
    )];
    let rules = find_tunnel_rules(&elements, "tabbed");
    assert_eq!(rules, vec!["L 8200 \u{2192} 10.30.0.3:8200"]);
}

#[test]
fn sparkline_all_outside_range() {
    let old = now() - 400 * 86400; // older than max range (365d)
    let result = activity_sparkline(&[old], 40);
    assert!(result.is_empty());
}

#[test]
fn sparkline_single_timestamp() {
    let ts = now() - 86400;
    let lines = activity_sparkline(&[ts], 40);
    assert!(!lines.is_empty());
    // Bottom row + axis = at least 2 lines
    assert!(lines.len() >= 2);
}

#[test]
fn sparkline_multiple_buckets() {
    let n = now();
    let timestamps: Vec<u64> = (0..84).map(|day| n - day * 86400).collect();
    let lines = activity_sparkline(&timestamps, 40);
    assert!(lines.len() >= 2);
}

#[test]
fn sparkline_all_in_one_bucket() {
    let n = now();
    let timestamps: Vec<u64> = (0..10).map(|i| n - i * 60).collect();
    let lines = activity_sparkline(&timestamps, 20);
    assert!(lines.len() >= 2);
}

#[test]
fn sparkline_axis_labels() {
    let ts = now() - 86400; // 1 day ago → auto-scales to 5d range
    let lines = activity_sparkline(&[ts], 30);
    let axis = lines.last().unwrap();
    let text: String = axis.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("5d"));
    assert!(text.contains("now"));
}

#[test]
fn sparkline_auto_scales_to_data_range() {
    // 3 days of data → 5d range
    let lines_3d = activity_sparkline(&[now() - 3 * 86400], 30);
    let axis_3d: String = lines_3d
        .last()
        .unwrap()
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(axis_3d.contains("5d"));

    // 8 days of data → 10d range
    let lines_8d = activity_sparkline(&[now() - 8 * 86400], 30);
    let axis_8d: String = lines_8d
        .last()
        .unwrap()
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(axis_8d.contains("10d"));

    // 50 days of data → 2mo range
    let lines_50d = activity_sparkline(&[now() - 50 * 86400], 30);
    let axis_50d: String = lines_50d
        .last()
        .unwrap()
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(axis_50d.contains("2mo"));

    // 100 days of data → 6mo range
    let lines_100d = activity_sparkline(&[now() - 100 * 86400], 30);
    let axis_100d: String = lines_100d
        .last()
        .unwrap()
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(axis_100d.contains("6mo"));
}

#[test]
fn sparkline_shown_at_threshold() {
    // 3 connections (= SPARKLINE_MIN_CONNECTIONS) → sparkline should render
    let n = now();
    let ts = vec![n - 86400, n - 2 * 86400, n - 3 * 86400];
    let lines = activity_sparkline(&ts, 30);
    assert!(
        !lines.is_empty(),
        "sparkline must render at {} connections",
        SPARKLINE_MIN_CONNECTIONS
    );
}

#[test]
fn sparkline_shown_above_threshold() {
    // 4 connections (above threshold) → sparkline should render
    let n = now();
    let ts = vec![n - 3600, n - 86400, n - 2 * 86400, n - 3 * 86400];
    let lines = activity_sparkline(&ts, 30);
    assert!(!lines.is_empty(), "sparkline must render at 4 connections");
}

#[test]
fn sparkline_rendered_with_dotted_baseline() {
    // Verify that empty buckets use · (middle dot) not spaces
    let n = now();
    // One connection at start of range → most buckets empty → dots visible
    let lines = activity_sparkline(&[n - 4 * 86400], 20);
    assert!(!lines.is_empty());
    // Bottom row (before axis) should contain · for empty buckets
    let bottom = &lines[lines.len() - 2]; // second to last = bottom row
    let text: String = bottom.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        text.contains('\u{00B7}'),
        "empty buckets should show · (middle dot), got: {:?}",
        text
    );
}

#[test]
fn sparkline_midpoint_label_shown_at_normal_width() {
    // At 30 cols, midpoint label should appear
    let lines = activity_sparkline(&[now() - 86400], 30);
    let axis: String = lines
        .last()
        .unwrap()
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(
        axis.contains("~2d"),
        "midpoint label missing at 30 cols, got: {:?}",
        axis
    );
}

#[test]
fn sparkline_midpoint_label_hidden_at_narrow_width() {
    // At 10 cols, midpoint label should NOT appear (too narrow)
    let lines = activity_sparkline(&[now() - 86400], 10);
    let axis: String = lines
        .last()
        .unwrap()
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(
        !axis.contains("~"),
        "midpoint label should be hidden at 10 cols, got: {:?}",
        axis
    );
}

#[test]
fn sparkline_365_day_boundary_selects_1y() {
    // Timestamp at exactly 364 days old → 1y range
    let lines_364 = activity_sparkline(&[now() - 364 * 86400], 30);
    assert!(!lines_364.is_empty(), "364-day-old data should render");
    let axis: String = lines_364
        .last()
        .unwrap()
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(
        axis.contains("1y"),
        "364 days should use 1y range, got: {axis:?}"
    );
}

#[test]
fn sparkline_narrow_width() {
    let ts = now() - 86400;
    let lines = activity_sparkline(&[ts], 10);
    assert!(lines.len() >= 2);
}

#[test]
fn sparkline_two_rows_for_high_variance() {
    let n = now();
    // One bucket with many hits, rest with few
    let mut timestamps: Vec<u64> = vec![n; 100];
    timestamps.push(n - 40 * 86400);
    let lines = activity_sparkline(&timestamps, 20);
    // Should have top row + bottom row + axis = 3 lines
    assert_eq!(lines.len(), 3);
}

// =========================================================================
// wrap_tags
// =========================================================================

fn tags(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
}

#[test]
fn wrap_tags_single_row() {
    let t = tags(&["prod", "web"]);
    let rows = wrap_tags(&t, 32);
    assert_eq!(rows, vec![vec!["prod", "web"]]);
}

#[test]
fn wrap_tags_wraps_to_second_row() {
    let t = tags(&["production", "web", "europe", "api"]);
    // "production web" = 14 cols, "europe" would make 21 > 20
    let rows = wrap_tags(&t, 20);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], vec!["production", "web"]);
    assert_eq!(rows[1], vec!["europe", "api"]);
}

#[test]
fn wrap_tags_one_per_row_when_narrow() {
    let t = tags(&["production", "staging"]);
    // Each tag is 10 chars, panel only 10 wide — no room for two
    let rows = wrap_tags(&t, 10);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], vec!["production"]);
    assert_eq!(rows[1], vec!["staging"]);
}

#[test]
fn wrap_tags_empty() {
    let rows = wrap_tags(&[], 32);
    assert!(rows.is_empty());
}

#[test]
fn wrap_tags_exact_fit() {
    let t = tags(&["ab", "cd"]);
    // "ab cd" = 5 cols
    let rows = wrap_tags(&t, 5);
    assert_eq!(rows, vec![vec!["ab", "cd"]]);
}

#[test]
fn wrap_tags_exact_overflow() {
    let t = tags(&["ab", "cd"]);
    // "ab cd" = 5 cols, max 4 → wraps
    let rows = wrap_tags(&t, 4);
    assert_eq!(rows.len(), 2);
}

// --- resolve_proxy_chain tests ---

fn host(alias: &str, hostname: &str, proxy: &str) -> crate::ssh_config::model::HostEntry {
    crate::ssh_config::model::HostEntry {
        alias: alias.to_string(),
        hostname: hostname.to_string(),
        proxy_jump: proxy.to_string(),
        ..Default::default()
    }
}

#[test]
fn proxy_chain_single_hop() {
    let target = host("server", "10.0.0.1", "bastion");
    let bastion = host("bastion", "1.2.3.4", "");
    let hosts = vec![target.clone(), bastion];
    let chain = resolve_proxy_chain(&target, &hosts);
    assert_eq!(chain.len(), 1);
    assert_eq!(chain[0].0, "bastion");
    assert_eq!(chain[0].1, "1.2.3.4");
    assert!(chain[0].2); // in_config
}

#[test]
fn proxy_chain_multi_hop() {
    let target = host("server", "10.0.0.1", "jump1");
    let jump1 = host("jump1", "1.1.1.1", "jump2");
    let jump2 = host("jump2", "2.2.2.2", "");
    let hosts = vec![target.clone(), jump1, jump2];
    let chain = resolve_proxy_chain(&target, &hosts);
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0].0, "jump1");
    assert_eq!(chain[1].0, "jump2");
}

#[test]
fn proxy_chain_loop_detection() {
    let a = host("a", "1.1.1.1", "b");
    let b = host("b", "2.2.2.2", "a");
    let hosts = vec![a.clone(), b];
    let chain = resolve_proxy_chain(&a, &hosts);
    // Should stop after "b" because "a" was already seen
    assert_eq!(chain.len(), 1);
    assert_eq!(chain[0].0, "b");
}

#[test]
fn proxy_chain_comma_separated() {
    let target = host("server", "10.0.0.1", "hop1, hop2");
    let hop1 = host("hop1", "1.1.1.1", "");
    let hop2 = host("hop2", "2.2.2.2", "");
    let hosts = vec![target.clone(), hop1, hop2];
    let chain = resolve_proxy_chain(&target, &hosts);
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0].0, "hop1");
    assert_eq!(chain[1].0, "hop2");
}

#[test]
fn proxy_chain_host_not_in_config() {
    let target = host("server", "10.0.0.1", "unknown");
    let hosts = vec![target.clone()];
    let chain = resolve_proxy_chain(&target, &hosts);
    assert_eq!(chain.len(), 1);
    assert_eq!(chain[0].0, "unknown");
    assert_eq!(chain[0].1, "unknown"); // hostname == alias for unknown hosts
    assert!(!chain[0].2); // NOT in_config
}

#[test]
fn proxy_chain_empty_hops_in_comma_list() {
    let target = host("server", "10.0.0.1", "hop1,,hop2");
    let hop1 = host("hop1", "1.1.1.1", "");
    let hop2 = host("hop2", "2.2.2.2", "");
    let hosts = vec![target.clone(), hop1, hop2];
    let chain = resolve_proxy_chain(&target, &hosts);
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0].0, "hop1");
    assert_eq!(chain[1].0, "hop2");
}

#[test]
fn proxy_chain_mixed_known_unknown() {
    let target = host("server", "10.0.0.1", "known, mystery, also_known");
    let known = host("known", "1.1.1.1", "");
    let also_known = host("also_known", "3.3.3.3", "");
    let hosts = vec![target.clone(), known, also_known];
    let chain = resolve_proxy_chain(&target, &hosts);
    assert_eq!(chain.len(), 3);
    assert!(chain[0].2); // known: in_config
    assert!(!chain[1].2); // mystery: NOT in_config
    assert!(chain[2].2); // also_known: in_config
}

#[test]
fn proxy_chain_none_stops() {
    let target = host("server", "10.0.0.1", "none");
    let hosts = vec![target.clone()];
    let chain = resolve_proxy_chain(&target, &hosts);
    assert!(chain.is_empty());
}

#[test]
fn proxy_chain_empty_proxyjump() {
    let target = host("server", "10.0.0.1", "");
    let hosts = vec![target.clone()];
    let chain = resolve_proxy_chain(&target, &hosts);
    assert!(chain.is_empty());
}

#[test]
fn proxy_chain_max_depth() {
    // Create a chain of 12 hops (exceeds max 10)
    let mut hosts = Vec::new();
    for i in 0..12 {
        let proxy = if i < 11 {
            format!("h{}", i + 1)
        } else {
            String::new()
        };
        hosts.push(host(&format!("h{}", i), &format!("10.0.0.{}", i), &proxy));
    }
    let target = host("target", "10.0.0.99", "h0");
    hosts.push(target.clone());
    let chain = resolve_proxy_chain(&target, &hosts);
    assert!(chain.len() <= 10);
}

// =========================================================================
// password_label tests
// =========================================================================

#[test]
fn password_label_keychain() {
    assert_eq!(password_label("keychain"), "keychain");
}

#[test]
fn password_label_1password() {
    assert_eq!(password_label("op://vault/item"), "1password");
}

#[test]
fn password_label_bitwarden() {
    assert_eq!(password_label("bw:some-id"), "bitwarden");
}

#[test]
fn password_label_pass() {
    assert_eq!(password_label("pass:entry"), "pass");
}

#[test]
fn password_label_vault() {
    assert_eq!(password_label("vault:secret/path"), "vault-kv");
}

#[test]
fn password_label_custom() {
    assert_eq!(password_label("/usr/bin/my-askpass"), "custom");
}

// =========================================================================
// Detail panel section logic tests (via compute_detail_info)
// =========================================================================

use crate::ssh_config::model::SshConfigFile;

fn parse_config(s: &str) -> SshConfigFile {
    SshConfigFile {
        elements: SshConfigFile::parse_content(s),
        path: std::path::PathBuf::from("/tmp/test"),
        crlf: false,
        bom: false,
    }
}

#[test]
fn detail_pattern_match_alias_only() {
    // Pattern "web-*" should appear for host "web-prod" (alias match).
    let config =
        parse_config("Host web-*\n  ProxyJump bastion\n\nHost web-prod\n  Hostname 10.0.0.1\n");
    let hosts = config.host_entries();
    let info = compute_detail_info(&hosts[0], &hosts, &config);
    assert_eq!(info.pattern_matches, vec!["web-*"]);
    assert_eq!(
        info.pattern_proxy_jumps,
        vec![("web-*".to_string(), "bastion".to_string())]
    );
}

#[test]
fn detail_pattern_match_no_hostname_match() {
    // Pattern "10.30.0.*" should NOT appear for host "myserver" with Hostname 10.30.0.5.
    // SSH Host patterns match alias only, not resolved hostname.
    let config = parse_config(
        "Host 10.30.0.*\n  ProxyJump bastion\n\nHost myserver\n  Hostname 10.30.0.5\n",
    );
    let hosts = config.host_entries();
    let info = compute_detail_info(&hosts[0], &hosts, &config);
    assert!(info.pattern_matches.is_empty());
    assert!(info.pattern_proxy_jumps.is_empty());
}

#[test]
fn detail_pattern_match_star_applies() {
    // "Host *" matches every alias.
    let config = parse_config(
        "Host *\n  User admin\n  ProxyJump gw\n\nHost myserver\n  Hostname 10.0.0.1\n",
    );
    let hosts = config.host_entries();
    let info = compute_detail_info(&hosts[0], &hosts, &config);
    assert_eq!(info.pattern_matches, vec!["*"]);
    assert_eq!(
        info.pattern_proxy_jumps,
        vec![("*".to_string(), "gw".to_string())]
    );
}

#[test]
fn detail_pattern_match_negation_excludes() {
    // "Host * !bastion" should NOT match "bastion".
    let config =
        parse_config("Host * !bastion\n  ProxyJump gw\n\nHost bastion\n  Hostname 10.0.0.1\n");
    let hosts = config.host_entries();
    let info = compute_detail_info(&hosts[0], &hosts, &config);
    assert!(info.pattern_matches.is_empty());
}

#[test]
fn detail_route_from_inherited_proxy_jump() {
    // Host inherits ProxyJump via pattern. Route should show the bastion hop.
    let config = parse_config(
        "Host web-*\n  ProxyJump bastion\n\nHost bastion\n  Hostname 1.2.3.4\n\nHost web-prod\n  Hostname 10.0.0.1\n",
    );
    let hosts = config.host_entries();
    let web_prod = hosts.iter().find(|h| h.alias == "web-prod").unwrap();
    let info = compute_detail_info(web_prod, &hosts, &config);
    assert!(info.has_route);
    assert_eq!(info.route_hops, vec!["bastion"]);
}

#[test]
fn detail_no_route_without_proxy_jump() {
    let config = parse_config("Host myserver\n  Hostname 10.0.0.1\n");
    let hosts = config.host_entries();
    let info = compute_detail_info(&hosts[0], &hosts, &config);
    assert!(!info.has_route);
    assert!(info.route_hops.is_empty());
}

#[test]
fn detail_route_with_own_proxy_jump() {
    let config = parse_config(
        "Host bastion\n  Hostname 1.2.3.4\n\nHost myserver\n  Hostname 10.0.0.1\n  ProxyJump bastion\n",
    );
    let hosts = config.host_entries();
    let server = hosts.iter().find(|h| h.alias == "myserver").unwrap();
    let info = compute_detail_info(server, &hosts, &config);
    assert!(info.has_route);
    assert_eq!(info.route_hops, vec!["bastion"]);
}

#[test]
fn detail_multiple_pattern_matches() {
    // Two patterns match "web-prod": "web-*" and "*".
    let config = parse_config(
        "Host web-*\n  User team\n\nHost *\n  ServerAliveInterval 60\n\nHost web-prod\n  Hostname 10.0.0.1\n",
    );
    let hosts = config.host_entries();
    let info = compute_detail_info(&hosts[0], &hosts, &config);
    assert_eq!(info.pattern_matches, vec!["web-*", "*"]);
}

#[test]
fn detail_tags_shown_when_present() {
    let config = parse_config("Host myserver\n  Hostname 10.0.0.1\n  # purple:tags prod,web\n");
    let hosts = config.host_entries();
    let info = compute_detail_info(&hosts[0], &hosts, &config);
    assert!(info.has_tags);
}

#[test]
fn detail_tags_hidden_when_empty() {
    let config = parse_config("Host myserver\n  Hostname 10.0.0.1\n");
    let hosts = config.host_entries();
    let info = compute_detail_info(&hosts[0], &hosts, &config);
    assert!(!info.has_tags);
}

#[test]
fn detail_self_referencing_proxy_jump_shows_loop() {
    // "Host *" with "ProxyJump gateway" on gateway itself = loop.
    let config = parse_config(
        "Host *\n  ProxyJump gateway\n\n\
         Host gateway\n  Hostname 10.0.0.1\n\n\
         Host backend\n  Hostname 10.0.0.2\n",
    );
    let hosts = config.host_entries();
    let gateway = hosts.iter().find(|h| h.alias == "gateway").unwrap();
    let backend = hosts.iter().find(|h| h.alias == "backend").unwrap();
    let gw_info = compute_detail_info(gateway, &hosts, &config);
    assert!(gw_info.is_proxy_loop);
    assert!(!gw_info.has_route); // loop replaces route
    // backend is not a loop.
    let be_info = compute_detail_info(backend, &hosts, &config);
    assert!(!be_info.is_proxy_loop);
    assert!(be_info.has_route);
}

#[test]
fn detail_comma_proxy_jump_self_reference_shows_loop() {
    let config = parse_config(
        "Host *\n  ProxyJump hop1,gateway\n\n\
         Host gateway\n  Hostname 10.0.0.1\n",
    );
    let hosts = config.host_entries();
    let gateway = hosts.iter().find(|h| h.alias == "gateway").unwrap();
    let info = compute_detail_info(gateway, &hosts, &config);
    assert!(info.is_proxy_loop);
    assert!(!info.has_route);
}

// =========================================================================
// Pattern-selected detail view tests (via compute_pattern_detail_info)
// =========================================================================

#[test]
fn pattern_detail_matches_alias_only() {
    // "Host web-*" should match "web-prod" (alias match) but NOT "myserver"
    // even if myserver has Hostname matching the pattern.
    let config = parse_config(
        "Host web-*\n  ProxyJump bastion\n\n\
         Host web-prod\n  Hostname 10.0.0.1\n\n\
         Host myserver\n  Hostname web-staging.example.com\n",
    );
    let hosts = config.host_entries();
    let patterns = config.pattern_entries();
    let info = compute_pattern_detail_info(&patterns[0], &hosts);
    assert_eq!(info.matching_aliases, vec!["web-prod"]);
}

#[test]
fn pattern_detail_ip_pattern_no_hostname_match() {
    // "Host 10.30.0.*" should NOT list "myserver" even though its Hostname
    // is 10.30.0.5. SSH matches alias only.
    let config = parse_config(
        "Host 10.30.0.*\n  ProxyJump bastion\n\n\
         Host myserver\n  Hostname 10.30.0.5\n\n\
         Host 10.30.0.5\n  User root\n",
    );
    let hosts = config.host_entries();
    let patterns = config.pattern_entries();
    let info = compute_pattern_detail_info(&patterns[0], &hosts);
    // Only "10.30.0.5" (alias match), NOT "myserver" (hostname match).
    assert_eq!(info.matching_aliases, vec!["10.30.0.5"]);
}

#[test]
fn pattern_detail_star_matches_all_hosts() {
    let config = parse_config(
        "Host *\n  ServerAliveInterval 60\n\n\
         Host alpha\n  Hostname 1.1.1.1\n\n\
         Host beta\n  Hostname 2.2.2.2\n",
    );
    let hosts = config.host_entries();
    let patterns = config.pattern_entries();
    let info = compute_pattern_detail_info(&patterns[0], &hosts);
    assert_eq!(info.matching_aliases.len(), 2);
    assert!(info.matching_aliases.contains(&"alpha".to_string()));
    assert!(info.matching_aliases.contains(&"beta".to_string()));
}

#[test]
fn pattern_detail_negation_excludes() {
    // "Host * !bastion" should match "web" but NOT "bastion".
    let config = parse_config(
        "Host * !bastion\n  ProxyJump gw\n\n\
         Host web\n  Hostname 10.0.0.1\n\n\
         Host bastion\n  Hostname 10.0.0.99\n",
    );
    let hosts = config.host_entries();
    let patterns = config.pattern_entries();
    let info = compute_pattern_detail_info(&patterns[0], &hosts);
    assert_eq!(info.matching_aliases, vec!["web"]);
}

#[test]
fn pattern_detail_no_matches() {
    // Pattern "staging-*" matches no concrete hosts.
    let config =
        parse_config("Host staging-*\n  User deploy\n\nHost web-prod\n  Hostname 10.0.0.1\n");
    let hosts = config.host_entries();
    let patterns = config.pattern_entries();
    let info = compute_pattern_detail_info(&patterns[0], &hosts);
    assert!(info.matching_aliases.is_empty());
}

#[test]
fn pattern_detail_has_directives() {
    let config = parse_config("Host web-*\n  ProxyJump bastion\n  User team\n");
    let patterns = config.pattern_entries();
    let info = compute_pattern_detail_info(&patterns[0], &[]);
    assert!(info.has_directives);
}

#[test]
fn pattern_detail_empty_directives() {
    // A pattern block with no directives (only the Host line).
    let config = parse_config("Host web-*\n");
    let patterns = config.pattern_entries();
    let info = compute_pattern_detail_info(&patterns[0], &[]);
    assert!(!info.has_directives);
}

#[test]
fn pattern_detail_has_tags() {
    let config = parse_config("Host web-*\n  User team\n  # purple:tags internal,vpn\n");
    let patterns = config.pattern_entries();
    let info = compute_pattern_detail_info(&patterns[0], &[]);
    assert!(info.has_tags);
}

#[test]
fn pattern_detail_no_tags() {
    let config = parse_config("Host web-*\n  User team\n");
    let patterns = config.pattern_entries();
    let info = compute_pattern_detail_info(&patterns[0], &[]);
    assert!(!info.has_tags);
}

#[test]
fn pattern_detail_multiple_negations() {
    // "Host * !web-* !bastion" should exclude both web-prod and bastion.
    let config = parse_config(
        "Host * !web-* !bastion\n  ServerAliveInterval 60\n\n\
         Host web-prod\n  Hostname 10.0.0.1\n\n\
         Host bastion\n  Hostname 10.0.0.99\n\n\
         Host db-01\n  Hostname 10.0.0.2\n",
    );
    let hosts = config.host_entries();
    let patterns = config.pattern_entries();
    let info = compute_pattern_detail_info(&patterns[0], &hosts);
    assert_eq!(info.matching_aliases, vec!["db-01"]);
}
