# purple. Architecture Reference

Detailed architecture, design decisions, testing invariants and distribution for purple. See [CLAUDE.md](CLAUDE.md) for rules and workflows.

## Architecture

```
src/
├── main.rs              # Entry point, clap CLI, event loop dispatch, first-launch init, auto-sync on startup
├── app.rs               # App struct (sub-structs: PingState, VaultState, UpdateState, TagState, UiSelection, SearchState, ReloadState, ConflictState), Screen enum, forms, search, sort, undo
├── app/
│   ├── ping.rs          # PingState sub-struct (status, generation, thresholds, auto_ping)
│   ├── vault.rs         # VaultState sub-struct (cert cache, signing thread, cancel flag, in-flight set)
│   ├── update.rs        # UpdateState sub-struct (available version, headline, hint)
│   ├── types.rs         # TagState, UiSelection, SearchState, ReloadState, ConflictState, Screen, SortMode, GroupBy, ViewMode, PingStatus, StatusMessage, FormBaseline variants
│   ├── forms.rs         # HostForm, ProviderFormFields, TunnelForm, SnippetForm, SnippetOutputState, SnippetParamFormState
│   ├── baselines.rs     # Form baseline snapshots for dirty-check
│   ├── display_list.rs  # Host list display generation (hosts + group headers)
│   ├── groups.rs        # Host grouping and tab navigation
│   ├── hosts.rs         # Host list building, sorting, cert cache refresh
│   ├── search.rs        # Search/filter operations
│   └── selection.rs     # Selection and navigation helpers
├── handler.rs           # Key event -> action dispatch per screen
├── handler/
│   ├── event_loop.rs    # Event arm handlers (extracted from run_tui): ping, sync, vault sign, file browser, snippet, container, cert check, tick
│   ├── host_list.rs     # Host list key handling (search, navigation, actions)
│   ├── host_list/actions.rs # Host list action dispatch (connect, clone, delete, file browser, containers)
│   ├── host_form.rs     # Add/edit host form handling
│   ├── host_detail.rs   # Host detail overlay and tag editing
│   ├── provider.rs      # Provider configuration form handling
│   ├── provider/region.rs # Region picker
│   ├── snippet.rs       # Snippet picker and execution
│   ├── file_browser.rs  # File browser navigation and SCP operations
│   ├── tunnel.rs        # Tunnel form handling
│   ├── containers.rs    # Container listing and action handling
│   ├── confirm.rs       # Confirmation dialogs (delete, vault bulk sign start)
│   ├── sync.rs          # Provider sync spawning
│   ├── ping.rs          # Ping task spawning
│   ├── command_palette.rs # Command palette key handling
│   ├── theme_picker.rs  # Theme selection handling
│   ├── tag_picker.rs    # Tag picker handling
│   ├── help.rs          # Help overlay handling
│   └── picker.rs        # Generic picker key handling (reusable)
├── ssh_context.rs       # SshContext (borrowed) and OwnedSshContext (owned) for remote operation thread spawning
├── tunnel.rs            # TunnelType/TunnelRule/ActiveTunnel, start_tunnel()
├── update.rs            # Self-update, version check, detect_install_method(), spawn_version_check()
├── lib.rs               # Re-exports for integration tests
├── fs_util.rs           # Shared atomic_write() (O_EXCL, PID-suffix, chmod 600, rename)
├── event.rs             # Crossterm event polling thread (sync mpsc), pause/resume for SSH
├── tui.rs               # Terminal setup/teardown (Drop safety)
├── connection.rs        # Spawn `ssh -F <config> -- <alias>`, stderr capture, host key error detection
├── animation.rs         # AnimationState (overlay scale-clip, detail width interpolation, braille ping spinner)
├── askpass.rs            # SSH_ASKPASS handler, password sources (keychain, 1Password, Bitwarden, pass, Vault KV)
├── vault_ssh.rs          # HashiCorp Vault SSH secrets engine: sign SSH pubkeys, cert cache (~/.purple/certs/), TTL checks, renewal
├── clipboard.rs         # Cross-platform clipboard (pbcopy, wl-copy, xclip, xsel)
├── history.rs           # Frecency scoring, connection timestamps, ~/.purple/history.tsv
├── import.rs            # Bulk import from hosts file or known_hosts, TUI import via I key, count_known_hosts_candidates()
├── ping.rs              # TCP connect check with RTT measurement in background threads
├── preferences.rs       # Sort mode, group_by, view_mode, theme persistence (~/.purple/preferences)
├── quick_add.rs         # Parse [user@]hostname[:port] with IPv6 support
├── ssh_keys.rs          # SSH key discovery, metadata, host linking
├── snippet.rs           # Snippet model, SnippetStore (INI parser/writer), run_snippet(), sends AppEvent directly to main loop
├── containers.rs        # Docker/Podman container model, runtime detection (sentinels), SSH fetch/action, JSONL cache, ID validation
├── demo.rs              # Built-in demo data for `--demo` mode (22 hosts, 3 providers, history, snippets, containers, keys)
├── demo_flag.rs         # Global AtomicBool guard: is_demo() checked by all disk-write functions
├── logging.rs           # Log init, rotation (5MB + 1 backup), startup banner, --verbose / PURPLE_LOG
├── file_browser.rs      # Dual-pane file explorer model, local/remote listing, BrowserSort, scp transfer, SSH warning filter
├── providers/
│   ├── mod.rs           # Provider trait, ProviderHost, registry, http_agent(), http_agent_insecure(), strip_cidr()
│   ├── config.rs        # ~/.purple/providers INI parser + writer
│   ├── sync.rs          # Diff engine: add/update/remove synced hosts, provider_tags sync, alias rename
│   ├── aws.rs           # AWS EC2 API (SigV4 signing, multi-region, XML pagination, AMI name resolution)
│   ├── digitalocean.rs  # DigitalOcean API (page pagination)
│   ├── vultr.rs         # Vultr API (cursor pagination)
│   ├── linode.rs        # Linode API (page pagination, private IP filtering)
│   ├── hetzner.rs       # Hetzner API (page pagination, labels->tags)
│   ├── i3d.rs           # i3D.net API (dedicated + FlexMetal, PRIVATE-TOKEN header, PAGE-TOKEN + RANGED-DATA pagination)
│   ├── leaseweb.rs      # Leaseweb API (dedicated servers + public cloud, X-Lsw-Auth header, offset pagination)
│   ├── scaleway.rs      # Scaleway API (multi-zone, page pagination, X-Total-Count header)
│   ├── gcp.rs           # GCP (Compute Engine) API (aggregatedList, JWT/OAuth2 service account auth, zone filtering)
│   ├── upcloud.rs       # UpCloud API (offset pagination, N+1 detail requests)
│   ├── proxmox.rs       # Proxmox VE API (N+1 detail requests, guest agent OS detection via get-osinfo + LXC interfaces, self-signed TLS)
│   ├── azure.rs         # Azure (Compute) API (ARM REST API, OAuth2 service principal auth, multi-subscription, batch NIC/IP resolution)
│   ├── tailscale.rs     # Tailscale API + CLI (dual mode: local `tailscale status --json` or HTTP API, Basic/Bearer auth, nodeId, connectedToControl)
│   ├── transip.rs       # TransIP API (RSA-SHA512 JWT auth or pre-generated Bearer token, single VPS list call)
│   ├── oracle.rs        # Oracle Cloud Infrastructure (OCI) API (RSA-SHA256 HTTP Signature signing, OCI config file auth, recursive compartment sync via Identity API, N+1 VNIC/image resolution)
│   └── ovh.rs           # OVHcloud Public Cloud API (SHA-1 signature auth, project-based, no pagination)
├── ssh_config/
│   ├── mod.rs           # Re-exports
│   ├── model.rs         # SshConfigFile, HostBlock, Directive, HostEntry (tags, provider_tags), tunnel/forward ops
│   ├── parser.rs        # Line-by-line parser, Include resolution, Match boundary detection
│   └── writer.rs        # Atomic write, symlink resolution, backups, blank line collapse
└── ui/
    ├── mod.rs           # Render dispatcher, render_footer_with_status(), render_footer_with_help()
    ├── theme.rs         # ThemeDef/ColorSlot structs, 11 built-in themes, custom TOML theme loader, data-driven theme::*() functions, OnceLock<RwLock> static, COLORTERM detection
    ├── theme_picker.rs  # Theme picker overlay with live preview and color swatches
    ├── host_list.rs     # Main host list with dual-encoded status dots (●▲✖○, braille spinner when checking), ADDRESS (hostname:port), max-2 tags (accent+muted), health summaries in group headers and title bar, sort indicator, detail_mode column suppression, vim nav, search, sort/group
    ├── host_form.rs     # Add/Edit form (divider-based layout), progressive disclosure (collapsed/expanded), Enter-to-pick for key and password source
    ├── provider_list.rs # Provider overlay (2-line per provider) + form (divider-based layout), progressive disclosure, sync status display
    ├── tunnel_list.rs   # Per-host tunnel overlay, start/stop, read-only for includes
    ├── tunnel_form.rs   # Tunnel add/edit overlay (divider-based layout, Local/Remote/Dynamic)
    ├── snippet_picker.rs # Snippet list overlay, single/multi-host mode
    ├── snippet_form.rs  # Snippet add/edit form (divider-based layout)
    ├── containers.rs    # Docker/Podman container overlay with start/stop/restart actions
    ├── detail_panel.rs  # Split-pane detail panel (connection, activity sparkline, tags, provider metadata, tunnels, snippets, containers)
    ├── host_detail.rs   # All directives overlay
    ├── confirm_dialog.rs # Delete confirmation (y/Esc), host key reset (y/Esc), import confirmation (y/Esc), welcome dialog (?/I/Enter)
    ├── help.rs          # Cheat sheet overlay (percentage-based sizing)
    ├── key_list.rs      # SSH key overlay with column headers
    ├── key_detail.rs    # SSH key detail overlay
    └── tag_picker.rs    # Tag filter overlay with host counts and footer
site/
├── worker.ts            # Bunny.net Edge Script (install script + landing page + llms.txt)
├── install.sh           # Shell installer (source of truth, embedded in worker.ts)
└── page.html            # Landing page
llms.txt                 # LLM context file (source of truth, embedded in worker.ts, served at getpurple.sh/llms.txt)
deny.toml                # cargo-deny config (license allowlist, advisory ignores)
fuzz/                    # cargo-fuzz harness for SSH config parser (corpus gitignored, seed_corpus/ committed)
tests/
├── proptest_ssh_config.rs    # Property-based round-trip and structural tests
├── proptest_mutations.rs     # Property-based mutation tests (add/delete/update/swap)
├── real_world_configs.rs     # Real-world SSH config test cases
├── roundtrip_fidelity.rs     # 530 round-trip parse/serialize fidelity tests
├── vault_ssh_config_safety.rs # Vault SSH cert-writing safety invariants (proptest)
└── mcp_e2e.rs                # End-to-end MCP server test (JSON-RPC handshake, tool calls)
```

## Key Design Decisions

- **Custom SSH config parser** for round-trip fidelity (preserves comments, formatting, unknown directives, CRLF, equals-syntax, inline comments). Match blocks stored as inert GlobalLines
- **Include support (read-only)**. Recursive (max depth 16, matching OpenSSH), tilde + glob + `${VAR}` env expansion. Included hosts displayed but not editable
- **Merge-based update**. `update_host` preserves unknown directives, indentation, raw lines when unchanged. `upsert_directive` only rebuilds when value changed
- **System `ssh` binary** (not `ssh2` crate). All invocations pass `-F <config_path>`
- **No tokio/async**. Event thread pauses during SSH sessions
- **Atomic writes**. SSH config writer also does symlink resolution, backups (last 5), blank line collapse
- **Tags via comments**. User tags in `# purple:tags prod,us-east`. Provider-synced tags in `# purple:provider_tags` (exact mirror of remote, always replaced on sync). Provider names as virtual tags. `tag:` fuzzy, `tag=` exact. **Bulk tag editor** (`t` with multi-select active) opens a tri-state checkbox overlay (`BulkTagAction`: `Leave`/`AddToAll`/`RemoveFromAll`, 3-way cycle via Space). Rows seeded from all user tags across the config. `+` key adds new tags. `bulk_tag_apply()` iterates selected aliases, computes per-host tag diffs, calls `set_host_tags` per host, then a single `config.write()`. Config backup + rollback on write failure. `bulk_tag_undo: Option<Vec<(alias, old_tags)>>` snapshot captured before write, restored by `u` key handler. Include-file hosts skipped and reported
- **Provider sync** (AWS EC2, DigitalOcean, Vultr, Linode, Hetzner, UpCloud, Proxmox VE, Scaleway, GCP, Azure, Tailscale, Oracle Cloud Infrastructure (OCI), OVHcloud, Leaseweb, i3D.net, TransIP). Background threads with cancel flags. Tracked via `# purple:provider name:id` comments. Provider tags stored separately in `# purple:provider_tags` (always replaced on sync). User tags in `# purple:tags` are never touched by sync. Providers return `PartialResult` on partial failures (per-VM for Proxmox/UpCloud, per-region for AWS, per-zone for Scaleway, per-subscription for Azure) so `--remove` is suppressed. AWS uses SigV4 request signing (HMAC-SHA256), multi-region sync, XML pagination (NextToken) and AMI name resolution (batched, max 100 per request). Credentials: `--profile` reads `~/.aws/credentials`, `--token AKID:SECRET` for inline keys, falls back to `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY` env vars. Region picker in TUI with geographic grouping. Profile credential errors return `AuthFailed` for immediate stop. Region validation before API calls. GCP uses service account JSON key file (JWT RS256 -> OAuth2 token exchange) or raw access token. Aggregated instance list API with pageToken pagination. Client-side zone filtering (empty zones = all). Requires `project` config field for GCP project ID. Dependencies: `rsa` (RSA-SHA256 signing) and `base64` (JWT encoding). Scaleway uses `X-Auth-Token` header auth, multi-zone sync (10 zones across fr-par, nl-ams, pl-waw, it-mil), page pagination with `X-Total-Count` response header, zone picker in TUI. Reuses the `regions` config field for zone storage. Azure uses ARM REST API (management.azure.com). Multi-subscription sync via `regions` config field (stores comma-separated subscription IDs). Auth: service principal JSON file (supports both az CLI format appId/password/tenant and portal format clientId/clientSecret/tenantId -> OAuth2 client credentials flow) or raw Bearer token. Batch IP resolution: 3 list calls per subscription (VMs with $expand=instanceView, NICs, Public IPs), joined in memory. IP priority: public > private. VM tags synced as key:value pairs. No region picker (subscription IDs are text input). Proxmox requires `https://` URLs (scheme validated case-insensitively); `--no-verify-tls` disables both cert and hostname verification. `provider add proxmox` without `--url` reuses the stored URL when updating an existing config. Per-provider `auto_sync` toggle (default `false` for Proxmox, `true` for others): skips startup auto-sync when disabled, manual sync via `s` always works. Written to config only when non-default. Provider metadata stored in `# purple:meta key=value,key=value` comments, displayed in detail panel under provider name heading. Metadata keys match provider terminology: `region` (AWS, DO, Vultr, Linode, Azure, OCI, OVH), `zone` (GCP, Scaleway, UpCloud), `location` (Hetzner, Leaseweb dedicated, i3D.net FlexMetal), `instance` (AWS instance type), `size` (DO), `plan` (Vultr, Linode, UpCloud), `type` (Hetzner, Scaleway server type, OVH flavor), `machine` (GCP machine type), `vm_size` (Azure), `specs` (Proxmox CPU/mem, Leaseweb dedicated CPU/RAM, i3D.net dedicated CPU), `shape` (OCI instance shape), `os` (AWS, Vultr, GCP, Proxmox, Tailscale, OCI), `image` (DO, Linode, Hetzner, Scaleway, Azure, UpCloud, OVH), `status` (all providers, volatile: excluded from sync diff), `node` (Proxmox only), `type` (Proxmox qemu/lxc). AWS pushes region, instance, os, status. DO pushes region, size, image, status. Vultr pushes region, plan, os, status. Linode pushes region, plan, image, status. Hetzner pushes location, type, image, status. Scaleway pushes zone, type, image, status. GCP pushes zone, machine, os, status. GCP also syncs network tags and labels. Azure pushes region, vm_size, image, status. Azure also syncs VM tags. UpCloud pushes zone, plan, image, status. Proxmox pushes node, type, specs, os, status. QEMU VMs with guest agent get real OS name via get-osinfo (e.g. "Debian GNU/Linux 13 (trixie)"), fallback to config ostype (e.g. "Linux 2.6-6.x"). Tailscale has two modes: empty token runs `tailscale status --json` via local CLI (peers only, excludes self-node), token filled uses HTTP API `GET /api/v2/tailnet/-/devices?fields=all`. API keys (`tskey-api-*`) use HTTP Basic auth (key as username, empty password). OAuth tokens use Bearer auth. Uses `nodeId` (preferred stable identifier) as server_id and `connectedToControl` for online/offline status. IPv4 (100.x) preferred over IPv6. Tags stripped of `tag:` prefix. Unauthorized devices skipped. CLI binary discovery via `command -v tailscale` with macOS app bundle fallback. Stdout read in background thread to avoid pipe deadlock. Tailscale pushes os, status. Oracle Cloud Infrastructure (OCI) reads ~/.oci/config for authentication (profile-based, RSA private key path), signs requests with RSA-SHA256 HTTP Signatures (reuses `rsa` crate), recursive compartment sync via Identity API ListCompartments (compartmentIdInSubtree=true, then per-compartment ListInstances/ListVnicAttachments), N+1 GetVnic and GetImage patterns for IP and OS resolution, freeform-only tag sync (defined_tags ignored), oci alias prefix. Required IAM: read instance-family, read virtual-network-family and inspect compartments in tenancy. OCI pushes region, shape, os, status. OVHcloud uses `$1$` SHA-1 signature auth with application key, application secret and consumer key. Project-based sync (one project per provider config). No pagination (single list call). `ovh` alias prefix. OVH pushes region, type, image, status. Leaseweb uses static API key (`X-Lsw-Auth` header). Syncs both dedicated servers (`bareMetals/v2/servers`) and public cloud instances (`publicCloud/v1/instances`). Offset-based pagination for both. Dedicated servers use `bm-` ID prefix, cloud instances use `cloud-` prefix. `lsw` alias prefix. Leaseweb dedicated pushes location, specs, status. Leaseweb cloud pushes region, type, image, status. i3D.net uses static API key (`PRIVATE-TOKEN` header). Syncs both dedicated/game servers (`/v3/host`) and FlexMetal on-demand servers (`/v3/flexMetal/servers`). Cursor-based pagination (`PAGE-TOKEN` header) for dedicated, offset-based (`RANGED-DATA` header) for FlexMetal. Dedicated servers use `host-` ID prefix, FlexMetal uses `flex-` prefix. `i3d` alias prefix. i3D.net dedicated pushes type (category), specs (CPU). i3D.net FlexMetal pushes location, type, os, status. FlexMetal tags synced as provider tags. TransIP uses RSA-SHA512 JWT auth (private key from key file) or pre-generated Bearer token. Single VPS list call (no pagination). `tip` alias prefix. TransIP pushes zone, plan, os, status. TransIP also syncs native VPS tags.
- **Soft-delete for disappeared hosts**. When a provider sync no longer returns a host, it is marked stale with a `# purple:stale <unix_timestamp>` comment. Stale hosts appear dimmed (DIM alias + hostname) and sort to the bottom. Purge with `X` key (shows host names in confirmation dialog, per-provider scoped from provider list). Stale hosts automatically clear when they reappear in the next sync (including stopped VMs with empty IP). Partial sync failures suppress stale marking. Editing a stale host clears the stale marker on save. Virtual `stale` tag for filtering (`tag:stale` fuzzy, `tag=stale` exact, appears in tag picker). Provider list shows per-provider stale count in error color. Stale warning (non-blocking, `is_error: true`) on connect, edit, delete, clone, tunnels, snippets and file browser. Active tunnels cleaned up on purge (after successful config write). CLI `--remove` still hard-deletes for scripts
- **Password management**. Per-host `# purple:askpass` comment stores the password source. Supported sources: OS Keychain (`keychain`), 1Password (`op://`), Bitwarden (`bw:`), pass (`pass:`), HashiCorp Vault KV secrets engine (`vault:`, labelled "HashiCorp Vault KV" in the askpass picker and "vault-kv" as a virtual tag. See the Vault SSH bullet below for the SEPARATE SSH signed-certs engine that must never be conflated with this password source), custom command. Purple acts as its own SSH_ASKPASS program. TUI: Enter on Password Source field opens picker, `Ctrl+D` in picker sets global default (saved to `~/.purple/preferences`). Keychain: auto-prompts on first connect if no password stored, auto-removes when source changes away from keychain. Bitwarden: auto-prompts for master password when vault locked, caches session. CLI: `purple password set/remove`. Keychain entries use `security` (macOS) / `secret-tool` (Linux) with service `purple-ssh`
- **Vault SSH signed certificates**. HashiCorp Vault SSH secrets engine for short-lived SSH certs. **This is a SEPARATE engine from the Vault KV password source above and the two must never be conflated in UI, CLI, docs or comments. Always use HashiCorp's own terminology ("Vault SSH secrets engine" vs "Vault KV secrets engine") so users can tell them apart at a glance.** Per-host role stored in `# purple:vault-ssh <mount>/sign/<role>` comment, with provider-level `vault_role` inheritance (host override > provider default). Signed certs cached under `~/.purple/certs/<alias>-cert.pub`, TTL shown in the detail panel under the `VAULT SSH` section with a `(press V to sign)` affordance hint when the cert is missing/expired/invalid. `V` key bulk-signs all hosts needing renewal. Press `V` again during a run to cancel. `needs_renewal` uses the fixed 5-min `RENEWAL_THRESHOLD_SECS` for normal TTLs but falls back to a proportional threshold (`total_secs / 2`) for short-TTL certs so a freshly signed sub-5-minute cert is not re-signed immediately. `check_cert_validity` rejects certs with a non-positive validity window (`to <= from`, e.g. a malformed cert) as `CertStatus::Invalid` before the negative `total_secs` can reach the renewal-threshold calculation. Background cert-status checks cache failures with a shorter `CERT_ERROR_BACKOFF_SECS` (30s) instead of the 5-min valid-cert TTL so transient errors recover quickly without hammering the check thread on every poll tick. CLI: `purple vault sign <alias>` or `purple vault sign --all` (nested subcommand group, matches `provider`/`theme` pattern). Signing shells out via `vault write -field=signed_key <role> public_key=@<pubkey>`; pubkey paths containing `=` are rejected up front because the Vault CLI would split them mid-argument. Virtual tags: `vault-ssh` (any host where `resolve_vault_role` returns Some) and `vault-kv` (askpass starts with `vault:`). `ensure_vault_ssh_if_needed` is called from every connect path AND from add/edit form submit so a freshly added host with a configured role gets a cert immediately. **Load-bearing invariant**: `should_write_certificate_file()` (in main.rs, used by every signing path: TUI bulk-sign `VaultSignResult` handler, `ensure_vault_ssh_if_needed`, CLI `purple vault sign <alias>` and `--all`, and both `add_host_from_form`/`edit_host_from_form` form mutators) returns true only when the host's existing `CertificateFile` is empty (whitespace-trimmed). A user-set custom `CertificateFile` is NEVER overwritten by purple's default cert path. The `VaultSignResult` event carries a `certificate_file` snapshot at sign time so the main loop never has to re-look up the host (avoiding O(n²) on bulk signs and TOCTOU under concurrent renames). The bulk-sign `signable` tuple is `(alias, role, certificate_file, pubkey_path)` so the pre-filter and TOCTOU re-check both call `resolve_cert_path(alias, &cert_file)` instead of `cert_path_for(alias)`. Cert cleanup on host delete/rename uses a `vault.cleanup_warning` side-channel field on `App` (NOT `eprintln!`, which would corrupt the ratatui screen in raw mode); the form-submit handler drains the field and overrides the success message when set. `App::Drop` cancels and joins `vault.sign_thread` so an in-flight bulk-sign worker cannot keep writing to `~/.purple/certs/` after teardown. **Vault address resolution**: purple never relied on users exporting `VAULT_ADDR` in the shell they launched purple from. The resolved address is passed as a `VAULT_ADDR` env var on the `vault` subprocess via `.env("VAULT_ADDR", addr)` on the `Command` in `vault_ssh::sign_certificate`. Resolve precedence (highest wins): CLI `--vault-addr <URL>` flag on `purple vault sign` > per-host `# purple:vault-addr <url>` comment > per-provider `vault_addr=` line in `~/.purple/providers` > inherited parent-process env (no override, current vault CLI default). `vault_ssh::resolve_vault_addr(host, provider, config) -> Option<String>` mirrors `resolve_vault_role`. Validation via `is_valid_vault_addr`: non-empty after trim, no control chars, no whitespace inside, max 512 bytes. The CLI flag is validated at the top of the handler and bails with an anyhow error on invalid input before any signing. The host comment is validated by `HostBlock::vault_addr()` at parse time (invalid values silently drop, identical to `vault_role` handling). The provider INI line is validated by `ProviderConfig::parse` (silent-drop on invalid, written by `save()` only when non-empty and valid). **Progressive disclosure of the Vault SSH Address form field**: `FormField::VaultAddr` and `ProviderFormField::VaultAddr` are always present in the schema (`ALL` array + every `*_FIELDS` static slice), but the host form and provider form renderers filter them out of the visible set when the corresponding `Vault SSH Role` field is empty on the same form. `HostForm::visible_fields()`/`focus_next_visible()`/`focus_prev_visible()` and `ProviderFormFields::visible_fields(provider)` provide the filtered list; the host form handler uses `focus_next_visible`/`focus_prev_visible` for Tab/Shift-Tab so focus cannot land on a hidden field. The stored value is preserved while hidden, so toggling the role back on restores the previous input. `HostForm::validate` only validates `vault_addr` when a role is set (leftover state when role is empty is benign because `to_entry` drops it), and `HostForm::from_entry_duplicate` clears `vault_addr` alongside `vault_ssh` because both belong to the original host's cert ecosystem. `set_host_vault_addr` is skipped entirely for pattern entries in `edit_host_from_form` (patterns cannot have wildcards in a concrete alias and the safety invariant refuses them). **Detail panel Vault SSH section** shows the role name (last path segment, e.g. `engineer` for `ssh-client-signer/sign/engineer`) with a `(from <provider>)` suffix when inherited. The address is not shown in the detail panel (it wastes space in a narrow column where `https://` dominates). The full role path and address are visible in the edit form (e). **Cert cache consolidation + mtime-based staleness**: `App::vault.cert_cache` value type is `HashMap<String, (Instant, CertStatus, Option<SystemTime>)>` where the third element is the cert file's on-disk mtime at check time. Every sign path funnels through `App::refresh_cert_cache(&mut self, alias)` which resolves the cert path, runs `check_cert_validity`, stats the file for mtime, and inserts the tuple. Called from: `VaultSignResult` event handler (replacing the prior manual inserts), `add_host_from_form`/`edit_host_from_form` at the end of a successful write (alias-rename also removes the entry under the old alias), and the main-loop connect path after `ensure_vault_ssh_if_needed` returns. `refresh_cert_cache` no-ops in demo mode and removes any stale entry when the host is missing or has no resolvable role, so the detail panel never shows a phantom status under a section that should not even render. The lazy cert-check block consults `cache_entry_is_stale(entry, current_mtime, elapsed_secs)` (a `pub(crate)` pure function in `src/main.rs` that takes an injected elapsed-closure for deterministic testing): the cache is stale if there is no entry, the current on-disk mtime differs from the cached mtime, or the TTL is exceeded. `CertStatus::Invalid` entries use `CERT_ERROR_BACKOFF_SECS` instead of `CERT_STATUS_CACHE_TTL_SECS` for transient-error fast-recovery. The mtime comparison catches external `purple vault sign` runs (from the CLI or another purple instance) within one render frame instead of waiting for the 5-minute TTL
- **Health status**. TCP ping with RTT measurement (`ping.rs`). `PingStatus` enum: `Checking`, `Reachable { rtt_ms }`, `Slow { rtt_ms }`, `Unreachable`, `Skipped`. Classification via `classify_ping()` in `app.rs` (threshold: `slow_threshold_ms`, default 200ms). Status dot before host alias (green/amber/red, animated braille spinner when checking, blank when unchecked). PING column shows RTT via `format_rtt()` in `host_list.rs` (42ms, 1.2s, 10s+). `theme::warning()` for amber/slow tier (Yellow ANSI 16, Rgb(234,179,8) truecolor). `SortMode::Status` ("down first") sorts unreachable to top via `ping_sort_key()`. `!` key toggles down-only filter (shows only unreachable hosts). Results expire after 60s TTL. `auto_ping` preference (default true) triggers `ping_all()` on startup. ProxyJump hosts are skipped. Concurrency limit 10. Generation-based invalidation
- **Tunnel management**. TUI (`T` key) and CLI (`purple tunnel`). Background `ssh -N` processes with poll_tunnels(). Active tunnels cleaned up on exit/panic via Drop
- **Self-update** (macOS and Linux, curl installs). Detects Homebrew/Cargo and redirects. Atomic binary replacement. 24h TTL version check cache. TUI update badge shows first changelog bullet as headline (cached from GitHub release body). `purple update` prints full release notes (markdown-stripped, control chars sanitized) after installation
- **Write-failure rollback** on all mutation paths
- **Form conflict detection** via mtime comparison (main config + includes for host forms, provider config for provider form)
- **Auto-reload** every 4s (stat config + includes + include dirs). Paused during forms/overlays
- **Welcome screen**. First launch shows a welcome dialog for all users. With existing hosts: shows host count. Without hosts but with `~/.ssh/known_hosts`: shows importable host count and offers `I` to import directly. Config backup noted if created. Only appears once (gated by `~/.purple/` directory creation)
- **Known_hosts import** (`I` key). Always available in host list. From welcome screen: imports directly (count already visible). From host list: shows confirmation dialog first. Uses `import::import_from_known_hosts()` with group header "known_hosts". Status shows imported/skipped counts with correct pluralization
- **Host key reset**. When SSH fails due to a changed host key (server reinstall), purple captures stderr, detects the error and offers to remove the old key via `ssh-keygen -R` and reconnect automatically
- **View modes**. `ViewMode::Detailed` (default, split-pane with detail panel) and `ViewMode::Compact` (full-width host list). Toggle with `v` key, persisted in `~/.purple/preferences`. Detail panel shows connection info, activity (history, sparkline, ping), tags, provider metadata, tunnels, snippets. Auto-fallback to compact when terminal < 95 columns wide. Detail panel width: 40 cols (46 at >= 140)
- **Snippets**. Saved commands stored in `~/.purple/snippets` (INI format, fields: name, command, description). TUI: `Ctrl+Space` to multi-select hosts, `r` runs on selected (or current) host(s), `R` runs on all visible hosts. CLI: `purple snippet list/add/remove/run`. Run-and-return model: pause TUI, execute via `ssh -F <config> -o ConnectTimeout=10 <alias> <command>`, show output, return. Multi-host execution runs sequentially with per-host output headers. Askpass integration for password-protected hosts
- **Container management** (Docker and Podman). TUI: `C` key opens container overlay for selected host. Agentless: runs commands via SSH (`docker ps -a --format '{{json .}}'` or `podman` equivalent). Runtime auto-detected in a single SSH call using `##purple:docker##`/`##purple:podman##`/`##purple:none##` sentinel markers. Container ID validated with ASCII alphanumeric allowlist before shell command construction (injection prevention). Actions: start/stop/restart via background threads. Cache: `~/.purple/container_cache.jsonl` (JSON lines, one line per host, loaded at startup, saved after each fetch). Detail panel shows cached container summary with `✓`/`✗` icons. `ContainerError` preserves detected runtime even on `ps` failure to avoid re-detection on refresh
- **Validation**: control chars rejected in all form fields, whitespace in hostname/user, `is_host_pattern()` for alias_prefix
- **Theme system**. 11 built-in themes + custom `~/.purple/themes/*.toml`. TUI picker (`m` key) with live preview. CLI: `--theme <name>`, `purple theme list/set`. Themes define 13 color slots (accent, success, warning, error, border, etc.) with per-tier values (NO_COLOR/ANSI 16/truecolor). Stored in `OnceLock<RwLock<ThemeDef>>` static. Custom TOML parser (flat key=value, no new dependencies)

## Thread Topology

All threads are spawned via `std::thread::Builder::new().name(...)` for debuggability. No tokio/async. Communication is via `std::sync::mpsc`.

| Thread | Spawn location | Cancel mechanism | Channel direction |
|--------|---------------|-----------------|-------------------|
| Event poll | `event.rs` Event::new() | pause/resume via AtomicBool | EventThread → main (mpsc) |
| Provider sync (per provider) | `handler/sync.rs` spawn_provider_sync() | AtomicBool per provider | sync → main (AppEvent::SyncComplete/Error/Partial) |
| Vault bulk sign | `handler/confirm.rs` start_vault_bulk_sign() | AtomicBool (vault.signing_cancel) | sign → main (AppEvent::VaultSign*) |
| Ping (per host) | `ping.rs` ping_host() | None (short-lived) | ping → main (AppEvent::PingResult) |
| Container listing | `containers.rs` spawn_container_listing() | None | listing → main (AppEvent::ContainerListing) |
| Container action | `containers.rs` spawn_container_action() | None | action → main (AppEvent::ContainerActionComplete) |
| Snippet execution | `snippet.rs` spawn_snippet_execution() | AtomicBool (cancel) | snippet → main (AppEvent::Snippet*) |
| File browser remote listing | `file_browser.rs` spawn_remote_listing() | None | listing → main (AppEvent::FileBrowserListing) |
| SCP transfer | `handler/file_browser.rs` (confirm copy) | None | transfer → main (AppEvent::ScpComplete) |
| Version check | `update.rs` spawn_version_check() | None (short-lived) | check → main (AppEvent::VersionCheckResult) |
| Cert check | `main.rs` (startup) | None (short-lived) | check → main (AppEvent::CertCheckResult) |

Drop semantics: `impl Drop for App` kills active tunnel child processes and joins the vault bulk-sign thread. The event poll thread is stopped via `Event::stop()` which sets an AtomicBool.

## Dependencies

ratatui, crossterm, clap (derive), clap_complete, dirs, glob, unicode-width, ureq (json, native-tls), serde, serde_json, anyhow, thiserror, sha2, sha1, hmac, quick-xml (serde, serialize), rsa (sha2), base64, libc, log, simplelog, proptest (dev), mockito (dev), tempfile (dev)

## Testing

```bash
cargo test    # ~6200+ tests (unit + integration + proptest + mockito HTTP)
cargo clippy  # Zero warnings policy
cargo fmt     # Enforced in CI
cargo deny    # License and vulnerability scanning (deny.toml)
```

CI (`.github/workflows/ci.yml`) runs on every push to `master` and every PR: fmt, clippy, test (macOS + Linux), cargo-deny and MSRV (1.86) check. Dependabot (`.github/dependabot.yml`) opens weekly PRs for cargo and github-actions updates.

### Fuzzing (manual, per release)

`fuzz/` is a cargo-fuzz harness with one target: `fuzz_ssh_config`. It exercises `parse_content`, `serialize`, round-trip idempotency, and every mutation API on `SshConfigFile` including `set_host_certificate_file`, `set_host_vault_ssh`, `set_host_vault_addr`, `set_host_tags`, `set_host_stale` and `set_host_meta`. The target lives at `fuzz/fuzz_targets/fuzz_ssh_config.rs`.

Before every release, run for at least 5 minutes locally (not in CI — cargo-fuzz needs nightly and platform-specific sanitizers):

```bash
# One-time per checkout: seed the corpus from the committed seeds.
mkdir -p fuzz/corpus/fuzz_ssh_config
cp fuzz/seed_corpus/fuzz_ssh_config/* fuzz/corpus/fuzz_ssh_config/

# Run. Any crash means STOP and investigate — do NOT release.
cargo +nightly fuzz run fuzz_ssh_config -- -max_total_time=300
```

`fuzz/corpus/` is gitignored (libfuzzer-generated). `fuzz/seed_corpus/fuzz_ssh_config/` is committed — hand-crafted seeds for features added after v2.8.1 (Vault SSH comments, Match blocks, provider metadata, CRLF, tabs, patterns). When adding a new feature that touches the ssh config writer, add a seed file that demonstrates it.

### Vault SSH write-path invariants (CRITICAL)

The Vault SSH feature is the newest code that writes to `~/.ssh/config`. Any change to the Vault feature or the surrounding write paths MUST preserve every invariant below. Each is locked down by at least one test.

| Invariant | Test location |
|---|---|
| Sibling host blocks byte-identical after cert write | `tests/vault_ssh_config_safety.rs::vault_cert_write_leaves_sibling_hosts_byte_identical` |
| Property-based isolation across 512 random configs | `tests/vault_ssh_config_safety.rs::proptest_cert_write_leaves_siblings_byte_identical` |
| Match blocks treated as inert GlobalLines, never mutated | `vault_cert_write_does_not_touch_match_blocks` + `set_host_certificate_file_ignores_match_blocks` |
| Include files never written to disk | `vault_cert_write_never_touches_include_file` |
| Missing alias → `set_host_certificate_file` returns `false` and config bytes unchanged | `vault_cert_write_is_noop_when_alias_missing` + `proptest_cert_write_missing_alias_is_total_noop` |
| Wildcard/glob aliases (`*`, `?`, `[`, `]`, `!`, empty) refused | `vault_cert_write_refuses_wildcard_alias` |
| CRLF line endings preserved | `vault_cert_write_preserves_crlf_line_endings` |
| Write-failure (read-only parent dir) leaves disk byte-identical | `vault_cert_write_failure_leaves_disk_byte_identical` |
| User-set custom `CertificateFile` never overwritten | `should_write_certificate_file` + `test_edit_host_preserves_custom_certificate_file_with_vault_role` |
| Bulk-sign detects external `~/.ssh/config` edits via mtime and merges instead of overwriting | `App::external_config_changed` + `VaultSignAllDone` handler in `src/main.rs` |
| Sibling host blocks byte-identical after vault-addr write | `tests/vault_ssh_config_safety.rs::proptest_vault_addr_write_leaves_siblings_byte_identical` (1024 cases) + `vault_addr_write_does_not_touch_match_blocks` |
| Missing alias → `set_host_vault_addr` returns `false` and config bytes unchanged | `proptest_vault_addr_write_missing_alias_is_total_noop` + `vault_addr_write_is_noop_when_alias_missing` |
| Wildcard aliases refused by `set_host_vault_addr` | `vault_addr_write_refuses_wildcard_alias` |
| CRLF preserved across vault-addr write | `vault_addr_write_preserves_crlf_line_endings` |
| Match blocks stay inert during vault-addr write | `vault_addr_write_does_not_touch_match_blocks` + `set_vault_addr_*` tests in `src/ssh_config/model.rs` |
| `vault.cert_cache` picks up external cert rewrites within one frame via mtime comparison | `cache_stale_when_current_mtime_differs_from_cached` + `cache_stale_when_file_appears_after_missing_cache` + `cache_stale_when_file_disappears_after_cached_mtime` + `cache_stale_detects_external_cert_rewrite_via_mtime` (pure-fn tests in `src/main.rs`) |
| Every sign path funnels through `App::refresh_cert_cache` | `refresh_cert_cache_noop_when_alias_not_in_hosts` + `refresh_cert_cache_removes_entry_when_no_vault_role` + `refresh_cert_cache_inserts_missing_status_for_nonexistent_cert` (`src/app.rs`) |
| `sign_certificate` sets `VAULT_ADDR` env on the `vault` subprocess when resolved | `sign_certificate_sets_vault_addr_env_on_subprocess` + `sign_certificate_does_not_set_vault_addr_when_none` + `sign_certificate_rejects_invalid_vault_addr` (all use the `with_env_capturing_vault` helper in `src/vault_ssh.rs` which installs a mock `vault` binary writing `$VAULT_ADDR` to a capture file, gated by `PATH_LOCK` to serialize against `with_mock_vault`) |
| Host and provider form dirty detection tracks `vault_addr` | `host_form_is_dirty_detects_vault_addr_change` (`src/app.rs`) |
| Pattern entries never receive a `vault-addr` comment | `edit_host_from_form_does_not_write_vault_addr_for_pattern` (`src/app.rs`) |
| `edit_host_from_form` rollback restores `vault_addr` on write failure | `test_edit_vault_addr_rollback_restores_old_value` + `_restores_none` (`src/app.rs`, unit-level round-trip that exercises the exact `set_host_vault_addr` call the rollback block in `edit_host_from_form` makes; a higher-fidelity integration test that forces `config.write()` to fail via a read-only parent dir would be strictly better but has not been added yet) |

**Test depth note**: the `cache_entry_is_stale` tests listed above are pure-function tests. They cover the staleness comparator but NOT the on-disk `std::fs::metadata(path).modified()` read that the production lazy-check loop uses to obtain `current_mtime`. The production call-site for that read is `src/main.rs` inside the lazy-check block (searches for `resolve_cert_path` + `metadata` + `modified`). If a future refactor extracts the file-stat wrapper into a helper, that helper should get its own tempdir-backed integration test.

**Test env-capture helper invariant**: `with_env_capturing_vault` (`src/vault_ssh.rs`) and `with_mock_vault` (same file) BOTH acquire `PATH_LOCK` before mutating any process env. Tests running via `with_mock_vault` MUST NOT independently set `VAULT_ADDR` inside their closure — `with_env_capturing_vault` owns `VAULT_ADDR` within the same lock scope and a nested writer would race. New env-capture tests should extend `with_env_capturing_vault` rather than roll their own PATH/env manipulation.

`SshConfigFile::set_host_certificate_file` AND `SshConfigFile::set_host_vault_addr` are both annotated `#[must_use]` — every caller MUST check the `bool` return. Async callers (the bulk-sign worker, CLI `vault sign` paths, `ensure_vault_ssh_if_needed`) surface a visible warning when the mutation is silently skipped. Synchronous form paths use `debug_assert!` because the alias was just upserted. Pattern entries (where `form.is_pattern == true`) SKIP `set_host_vault_addr` entirely because the wildcard-refuse invariant would otherwise trip the `debug_assert!` in development builds. **Wildcard guard scope**: the guard in both setters refuses the five single-char glob metachars (`*?[]!`) and empty aliases, but does NOT explicitly reject space-separated multi-host patterns like `Host web-* db-*`. Those still degrade safely to `false` because the exact-match `block.host_pattern == alias` lookup will never find a single-token match for a multi-token alias. Audit trail lives in `model.rs::set_host_certificate_file` and `set_host_vault_addr`.

**Form inheritance hint limitation (pre-existing, applies to both `vault_ssh` and `vault_addr`)**: the in-form provider inheritance hints rendered by `src/ui/host_form.rs` (`vault_provider_hint` and `vault_addr_provider_hint`) are both gated on `Screen::EditHost { alias }` — the AddHost screen does NOT show "inherits X from provider" hints for these fields. This is because a new host has no provider context bound to the form state yet. Users in the AddHost flow only see the generic placeholder until they save and re-open the form. If you change this, fix BOTH hints together and ensure `app.form.provider_context` or equivalent is plumbed through from the caller.

The bulk-sign `config.write()` in `VaultSignAllDone` calls `app.external_config_changed()` first. If the on-disk config was touched by another editor, purple reloads the fresh file, re-applies `CertificateFile` directives only for hosts that still exist and still have no custom path, and writes that merged result — external edits are preserved.

## Demo recording

`demo-record.py` generates `demo.gif` and `demo.webm` for the README and getpurple.sh.

Two-phase workflow:
1. Script launches `target/release/purple --demo` in a new Ghostty window (built-in demo data: 22 hosts, 3 providers, history, snippets, containers)
2. Script captures that window via `screencapture -v -l<windowid>` and sends keystrokes via AppleScript

Uses your real Ghostty config (Victor Mono, font-thicken, display-p3) for pixel-perfect native GPU font rendering. Auto-crops window chrome and titlebar via ffmpeg cropdetect. Converts to GIF (256-color palette, gifsicle optimization) and WebM (VP9 CRF 18). Does NOT close other Ghostty windows. The `demo.tape` file is legacy (VHS/ttyd, inferior browser-based font rendering).

```bash
cargo build --release && python3 demo-record.py
```

Requires: Ghostty, ffmpeg, gifsicle, swift (macOS), accessibility permission for osascript (`System Preferences > Privacy > Accessibility`).

## Distribution

- **getpurple.sh**: `curl -fsSL getpurple.sh | sh` (Bunny.net Edge Script, `.github/workflows/site.yml`)
- **Homebrew**: `brew install erickochen/purple/purple` (tap: `erickochen/homebrew-purple`)
- **crates.io**: `cargo install purple-ssh`
