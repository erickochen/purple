## 2.39.0

- Contract snapshot tests for API drift detection
- 32 golden fixture files verified against current provider API docs cover all 16 providers and every documented endpoint. 35 integration tests run on every push to catch response structure changes before they break real syncs

## 2.38.1

- Fix i3D.net FlexMetal OS metadata not showing
- The FlexMetal API returns `os.slug` (e.g. `ubuntu-2204-lts`), not `os.name`. Purple now reads `slug` first with `name` as fallback, so OS info appears in host details and metadata

## 2.38.0

- Bulk tag editor for multi-host tagging
- Select hosts with Space (or Ctrl+Space), press t to open the bulk tag overlay. Tri-state checkboxes: [x] add to all, [ ] remove from all, [~] leave as-is. Space cycles through all three states
- Press + to add a brand-new tag. Enter applies, Esc cancels. u undoes the entire operation in one keystroke
- Plain Space now toggles host selection (Ctrl+Space still works). Esc clears the selection
- Footer shows bulk actions (t bulk tag, r run, Esc clear, ? help) when hosts are selected
- Include-file hosts are detected and skipped with a visible warning. Mixed-state rows show a "mixed" label for accessibility on NO_COLOR terminals
- Closes [#15](https://github.com/erickochen/purple/issues/15)

## 2.37.0

- Smarter forms, jump host suggestions and polished overlays
- Typing a domain or IP in the Name field auto-suggests it as the Host value. Works on Tab, Down and Enter, skips when Host is already populated
- Tags in host and pattern detail panels separated by commas (`prod, web, europe`) instead of spaces, so tags containing spaces aren't ambiguous
- Confirmation toasts now stay visible for 4 seconds instead of 3
- ProxyJump picker promotes likely jump hosts into a Suggestions section, ranked by usage count, keyword match (`jump`/`bastion`/`gateway`/`proxy`/`gw`) and shared domain suffix ([#14](https://github.com/erickochen/purple/issues/14))
- Help overlay redesigned: essentials only on the host list, centered layout, clearer labels, wiki link points to the full command reference

## 2.36.1

- Host indicators stay visible when the detail panel is open
- Tunnel (⇄) and proxy jump (↗) indicators now appear next to the host name in compact view instead of disappearing with the address column. Active tunnels show in purple, configured tunnels and jumps in muted style
- Vault SSH sign dialog spacing improved. Host list is visually separated from the question and the skip note

## 2.36.0

- Vault SSH role picker and smarter defaults
- Vault SSH Role field offers a picker when roles are already configured on other hosts or providers. Press Enter to pick or type one manually ([#26](https://github.com/erickochen/purple/issues/26))
- Scheme-aware default ports for Vault address: `:443` for https, `:80` for http, `:8200` for bare hostnames
- Vault SSH placeholder hints explain authentication flow ("auth via vault login")
- SSH disconnect reasons shown in toast instead of just "exited with code 255". Full stderr context joined with pipe separators so you see why the connection was closed
- Vault batch signing completion status now properly clears the sticky progress footer
- Tags and alias placeholder hints clarified to avoid comma confusion
- Tag input bar shows placeholder with cursor before hint text when empty
- Updated dependencies to latest compatible versions

## 2.35.1

- Internal cleanup. Leaner App struct and event loop
- App state grouped into sub-structs: PingState, VaultState, UpdateState, TagState ([#25](https://github.com/erickochen/purple/issues/25))
- Event loop arms extracted from run_tui (1300 lines) into handler/event_loop.rs
- CLI subcommand handlers extracted from main() into cli.rs
- SshContext struct eliminates repeated function arguments across remote operations
- SnippetEvent bridge thread removed. Snippets send events directly to the main loop
- TUI smoke test: automated tmux-based test navigates all 22 screens in demo mode
- Fixed 6 flaky test races (hardcoded temp paths and global state interference)
- Fixed 9 rustdoc warnings (unclosed HTML tags and bare URLs in doc comments)

## 2.35.0

- Command palette. Press `:` to search and run any action
- 24 searchable actions: add, edit, delete, file explorer, tunnels, containers, SSH keys, providers and more ([#21](https://github.com/erickochen/purple/issues/21))
- Type to filter by name, press Enter to execute. Case-insensitive matching
- Up/Down to navigate, Esc to close, Backspace to clear filter
- Shortcuts shown next to each command so you learn the direct keys over time
- Footer shows `: cmds` hint. Help overlay lists `:` in the TOOLS section

## 2.34.0

- Toast notifications for user action feedback
- Copy, sort, delete and error messages now appear as a bordered overlay box in the bottom-right corner instead of blending into the footer
- Toast queue buffers rapid actions so no feedback is lost (max 5 queued)
- Keyboard hints in the footer stay visible at all times. Status messages no longer replace the "? more" help hint
- Four message classes: Confirmation (toast, 1.5s), Info (footer, 3s), Alert (toast, 5s) and Progress (footer, sticky)
- Background events like provider sync and ping go to the footer. Direct user actions go to toast
- Debug logging for all status message routing decisions

## 2.33.5

- Debug logging for silent error paths across the codebase
- Pipe read failures in MCP tool execution, Vault SSH signing and Tailscale provider now surface in the log instead of being silently swallowed
- First-launch init (directory creation, SSH config backup, permission setting) logs warnings on failure
- Connection history read/write errors, preferences file access failures and SSH config backup pruning failures are now logged
- Thread panics in stderr capture (SSH connection) and stdout/stderr readers (snippet execution) log a warning instead of silently returning empty strings
- Tunnel and process cleanup failures (kill/wait) log at debug level
- Proxmox guest OS info and VM config API calls log the specific failure reason instead of returning None silently
- Version check cache write failures log at debug level

## 2.33.4

- Internal: extract CLI handlers from main.rs, move test modules into dedicated files across the codebase, document unsafe block invariants, replace fragile unwrap patterns with let-else guards, route TUI-context error messages through the log system instead of stderr

## 2.33.3

- Safer render paths: recover from poisoned mutex, remove unwrap panics from container overlay, replace unreachable! with safe fallbacks in provider and tunnel forms
- Faster host sorting: cache lowercased keys instead of allocating per comparison. HashSet dedup for group tab ordering
- Allocation-free host width calculation in the host list renderer
- Container cache write errors now surface in the debug log instead of being silently swallowed
- Internal: split handler.rs (11k lines) and app.rs (11k lines) into focused submodules. No behavior change

## 2.33.2

- Active tunnels now render green in the detail panel
- Tunnel rules in the detail panel TUNNELS section use a `→` arrow between the local and remote endpoints, so `LocalForward 8200 10.30.0.3:8200` displays as `L 8200 → 10.30.0.3:8200`. DynamicForward entries keep their single-value form (`D 1080`)
- Active tunnel lines use the theme success color instead of bold, matching the green convention already used for active hosts in the host list

## 2.33.1

- Askpass now works from headless ssh sessions on Linux
- Switch `SSH_ASKPASS_REQUIRE` from `prefer` to `force` on every ssh and scp launch (connect, tunnel, file browser, snippet). OpenSSH's `prefer` mode silently no-ops when both `DISPLAY` and `WAYLAND_DISPLAY` are empty and falls back to the TTY prompt, which broke Bitwarden, 1Password, Vault KV, pass and keychain lookups for users running purple inside a remote shell on Arch, Hyprland and other headless Linux setups
- Extract the SSH_ASKPASS env wiring into `askpass_env::configure_ssh_command` so all four call sites share one regression test that inspects `Command::get_envs()` directly. A future change back to `prefer` now fails CI
- Resolves #19

## 2.33.0

- Tmux-aware SSH: new window instead of TUI suspend
- When purple runs inside a tmux session, pressing Enter on a host opens SSH in a new tmux window named after the alias. The purple TUI stays alive in the original window so you can switch between sessions with `prefix + n/p` and keep navigating other hosts
- Detection via `$TMUX` env var. No tmux means the current suspend-and-restore behavior is unchanged
- Hosts with an askpass source (keychain, 1Password, Bitwarden, Vault KV, pass, cmd) keep the suspend-TUI flow because the askpass relay needs inherited stdin
- Vault SSH cert signing still runs before the tmux window opens, so short-lived certs are refreshed exactly as before. Signing status messages surface via the purple status bar
- Resolves #18

## 2.32.1

- Remove vault sign from host list footer to reduce clutter

## 2.32.0

- Structured logging to ~/.purple/purple.log
- `--verbose` flag enables debug-level logging. `PURPLE_LOG` env var for finer control (trace/debug/info/warn/error/off)
- `purple logs` subcommand: `--tail` to follow in real time, `--clear` to delete
- Log entries carry fault domain prefixes: `[external]` for remote/tool errors, `[config]` for local config issues, `[purple]` for internal errors
- Startup banner records purple version, SSH version, terminal, providers and askpass sources for diagnostics
- Automatic log rotation at 5 MB

## 2.31.0

- HashiCorp Vault SSH certificate signing
- Short-lived SSH certificates signed via the HashiCorp Vault SSH secrets engine. Per-host role in `# purple:vault-ssh <mount>/sign/<role>`, per-provider default in `vault_role=`. Host overrides win over provider defaults
- `V` key bulk-signs every host needing renewal. Press `V` again to cancel. Detail panel shows cert TTL under the `VAULT SSH` section with a "(press V to sign)" affordance when missing, expired or invalid
- Automatic renewal on connect via `ensure_vault_ssh_if_needed`, so an expired cert is re-signed before the SSH session starts
- Cert cache under `~/.purple/certs/<alias>-cert.pub`. Background status checks with 5 minute TTL, shorter 30 second backoff on errors
- Detail panel reflects external `purple vault sign` runs (CLI or another purple instance) within one render frame via mtime-based cache invalidation
- Vault SSH address configurable per host (`# purple:vault-addr`), per provider (`vault_addr=`) or per CLI invocation (`purple vault sign --vault-addr`). Purple exports the resolved value as `VAULT_ADDR` on the `vault` subprocess, so you no longer need to export it in every shell you launch purple from
- New "Vault SSH Role" and "Vault SSH Address" fields in the host and provider forms. Progressive disclosure: Address appears when Role is set, with provider inheritance hint
- CLI: `purple vault sign <alias>` and `purple vault sign --all`, both accepting `--vault-addr <url>`. Shells out to `vault write -field=signed_key` so existing Vault authentication (VAULT_TOKEN, token helpers, OIDC, etc.) applies
- Bulk sign detects concurrent external `~/.ssh/config` edits via mtime and merges instead of overwriting, so edits in another editor are preserved
- Virtual tags `vault-ssh` (any host with a resolved role) and `vault-kv` (any host using the `vault:` askpass prefix) for filtering
- Distinct from the HashiCorp Vault KV secrets engine used as a password source via the `vault:` askpass prefix. UI, CLI and docs keep the two engines strictly separated
- Vault SSH address normalization: bare IP or hostname auto-expands to `https://IP:8200`. Explicit `http://` for dev-mode Vault servers
- 30 second timeout on vault CLI subprocess. Previously hung indefinitely when the Vault server was unreachable
- Friendly error messages for common Vault SSH failures: connection refused, connection timed out, host not found, TLS mismatch (HTTP vs HTTPS), permission denied, token expired
- Signing progress shows animated spinner. Error messages stay visible until the next action (sticky status)
- Pre-check on `V`: warns immediately when no Vault address is configured instead of failing silently after the confirm dialog
- Detail panel Vault SSH section: shows role name instead of full mount path. Address moved to edit form (e) to save space
- 1000+ new tests covering the Vault SSH write paths, wildcard safety invariants (proptest across 512 random configs), Match block inertness, CRLF preservation, rollback on write failure, mtime cache staleness, subprocess env propagation and CLI flag parsing

## 2.30.1

- Fix pattern tags missing from tag grouping tabs and counts
- Fix tag picker showing (0) for tags that only exist on patterns
- Fix generic search not matching pattern tags
- Fix group-by-tag clearing when tag only exists on a pattern

## 2.30.0

- Color themes. 11 built-in themes with live preview (`m` key)
- Custom themes from `~/.purple/themes/*.toml`
- CLI: `purple theme list`, `purple theme set <name>` and `--theme <name>` session override
- Pattern inheritance: ProxyJump, User and IdentityFile from pattern blocks (e.g. `Host *`, `Host web-*`) now inherited by matching hosts. ↗ indicator and ping logic reflect inherited ProxyJump
- Edit form shows inherited values as dimmed placeholders with source pattern (e.g. `gateway  ← *`)
- Self-referencing ProxyJump loop detection: ↗ in error color, ROUTE warning in detail panel and fix hint in edit form when a pattern assigns a host as its own jump host
- Fix detail panel PATTERN MATCH section no longer shows hostname-matched patterns that SSH would not apply
- Fix error messages now show in overlay footers instead of behind dimmed background
- Fix editing multi-host patterns (e.g. `Host web-* db-*`) failing with false "no longer exists" error

## 2.29.0

- Progressive disclosure in host and provider forms. Required fields shown first, arrow down reveals optional fields
- Demo mode (`purple --demo`) with synthetic data for screenshots and recordings

## 2.28.0

- Animation state separated from App into dedicated AnimationState module
- Animated braille spinner for ping checking status

## 2.27.3

- New welcome screen logo (ANSI Shadow style)
- Lowercase README badges

## 2.27.2

- Record connection history when running snippets on hosts via TUI

## 2.27.1

- Fix group bar selector not following selected host in tagged view
- Tagged view group bar now shows only user tags. Provider tags no longer appear as tabs

## 2.27.0

- Cleaner host list and keycap-style footer keys
- Streamlined columns: NAME, ADDRESS, TAGS (up to 3) and LAST. Auth, tunnel and ping moved to detail panel
- ADDRESS column shows hostname:port (user moved to detail panel). Hidden when detail panel is open
- ProxyJump hosts now inherit ping status from their bastion host instead of showing "can't ping directly"
- Tags displayed without # prefix throughout the UI
- Snippet parameter form shows discard confirmation when pressing Esc with modified values
- Detail panel shows friendly password source labels (keychain, 1password, bitwarden, pass, vault) and ping RTT
- Proxmox no longer injects resource type (qemu/lxc) as provider tag. Type is already in metadata
- SSH key detail overlay now shows Esc/close footer
- Online status dots use dimmed green to distinguish from success messages
- Health summary spans (●23 ▲2 ✖1) available for group headers

## 2.26.0

- Live host health status with RTT measurement and auto-ping on startup
- Status dots before each host: ● online, ▲ slow, ✖ down, ○ unchecked (color + shape for accessibility)
- PING column shows response time (42ms, 1.2s, 10s+). Detail panel shows RTT under CONNECTION
- Slow host detection with configurable threshold (default 200ms, set slow_threshold_ms in preferences)
- Sort by health status: press s to cycle to "down first" (unreachable hosts at top)
- Down-only filter: press ! to show only unreachable hosts
- Ping results expire after 60 seconds with automatic cleanup
- Auto-ping enabled by default (disable with auto_ping=false in preferences)
- Provider sync status no longer flickers between updates

## 2.25.0

- Visual redesign with tab navigation, section cards and scoped search
- Tab/Shift+Tab cycles through provider groups or tags. Esc resets to All
- Search respects active tab: filters within the selected group only
- Tag mode in navigation bar: when grouped by tag, top tags shown as tabs
- Bordered host list and detail panel with section cards
- Status indicators per host row: reachable, unreachable, checking
- Green highlight for recent connections in Last column
- Detail panel sections as visual cards with rounded borders
- Last section stretches to fill panel height
- Pattern match sections clearly labeled
- All containers and tunnels shown (no more truncation)
- Space-separated footer (cleaner look)
- File explorer shortcut changed from f to F for consistency
- Help screen updated with Tab/Shift+Tab, scoped search, aligned columns
- Proxmox stopped VMs no longer marked stale. Stopped and no-IP VMs are included in sync with empty IP to prevent false stale detection
- Collapsible groups removed; Tab/Shift+Tab tab navigation replaces Enter-to-collapse

## 2.24.0

- Collapsible groups, Ctrl+A select all and smarter Proxmox OS detection
- Navigate to a group header and press Enter to collapse or expand it. Collapsed state is saved between sessions
- Ctrl+A to select or deselect all visible hosts. Works with search filters and collapsed groups
- Proxmox VMs with QEMU guest agent now show actual OS (e.g. "Debian GNU/Linux 13 (trixie)") instead of generic "Linux 2.6-6.x"
- Ping results no longer persist after clearing. Background ping threads are properly discarded
- Column hide priority improved: tags stay visible longest on narrow terminals
- Detail panel wider (40/46 columns) and shown by default
- Group headers are bold with collapse indicators
- Pinned "? more" help hint in footer
- Breadcrumb navigation in overlay titles (e.g. "Providers > AWS")
- Sync completion shows change summary (e.g. "+3 ~1 -2")
- Refreshed welcome screen with new logo and "Press ? anytime for help"
- Help screen shows issue tracker link

## 2.23.0

- Group hosts by any tag. Press g to cycle: ungrouped, provider or tag.
- Tag grouping opens a picker to choose which tag to group by. Thanks [@AciDCooL](https://github.com/AciDCooL) for the [feature request](https://github.com/erickochen/purple/issues/10)
- Group preference persisted across sessions and migrated automatically from the old provider-only setting
- Tag pickers close gracefully on config reload during provider sync
- Stale tag preferences cleared on startup when the tag no longer exists

## 2.22.0

- Dimmed background behind overlays for better visual depth and focus
- Form divider lines rendered dim to establish clear visual hierarchy (border > labels > dividers)
- Three-tier color support: truecolor (dark grey fg), ANSI 16 (DarkGray), NO_COLOR (DIM modifier only)
- Consistent dimming across open animation, steady-state and close animation

## 2.21.0

- Host patterns support
- Wildcard blocks like `Host *.example.com` and `Host 10.30.0.*` now appear in a dedicated Patterns group at the bottom of the host list
- Add, edit, clone and delete pattern blocks directly from the TUI (`A` to add a new pattern)
- Full SSH glob matching engine: `*`, `?`, `[charset]`, `[!charset]`/`[^charset]`, ranges, `!negation` and multi-pattern (space-separated)
- Detail panel shows inherited directives from matching patterns for each host. Pattern detail view lists all matching hosts
- Patterns included in search, tag filtering (`tag:` fuzzy and `tag=` exact) and tag picker
- Context-aware footer: pattern-specific actions shown when a pattern is selected
- Dynamic column widths in key list and key picker overlay. Columns now adapt to content width instead of using fixed sizes

## 2.20.0

- TransIP provider. 16 cloud providers now supported
- RSA-SHA512 signed JWT authentication or pre-generated Bearer token
- Syncs VPS name, IP, availability zone, product plan, OS and status as provider metadata
- Native TransIP tags synced as provider tags
- Single API call fetches all VPS instances (no pagination needed for typical fleets)

## 2.19.0

- i3D.net provider. 15 cloud providers now supported
- Syncs both dedicated/game servers and FlexMetal on-demand bare metal in a single sync
- Static API key authentication via PRIVATE-TOKEN header
- Dedicated hosts: server category and CPU specs as provider metadata
- FlexMetal servers: location, instance type, OS and status as provider metadata. FlexMetal tags synced as provider tags
- Cursor-based pagination for dedicated hosts, offset-based for FlexMetal

## 2.18.0

- Leaseweb provider. 14 cloud providers now supported
- Syncs both dedicated (bare metal) servers and public cloud instances in a single sync
- Static API key authentication via X-Lsw-Auth header
- Dedicated servers: location, specs (CPU and RAM) and delivery status as provider metadata
- Public cloud instances: region, instance type, OS image and state as provider metadata
- Offset-based pagination for both product lines

## 2.17.0

- OVHcloud Public Cloud provider. 13 cloud providers now supported
- Custom OVH API signature authentication (SHA-1 signing with application key, secret and consumer key)
- EU, CA and US API endpoint picker (defaults to EU)
- Syncs instance name, IP, region, flavor type, OS image and status as provider metadata
- Public IPv4 preferred, falls back to public IPv6 then private IPv4. CIDR suffixes stripped automatically
- HTTPS enforced on all OVH API calls. Project ID validated before any network request

## 2.16.0

- Smooth animations for overlays, detail panel and welcome screen
- Overlay screens open and close with a zoom scale effect (150ms ease-out cubic, 350ms for welcome)
- Detail panel slides open and closes with a smooth width transition. Pressing `v` mid-animation reverses direction seamlessly
- Welcome screen redesigned with block-art logo (line-by-line reveal), typewriter text and larger dialog
- `Ctrl+E` edits the selected host directly from search mode without leaving the search (thanks [@UnspecifiedId](https://www.reddit.com/user/UnspecifiedId/))
- Form borders use purple accent color for host, provider, tunnel, snippet and parameter forms
- Animation loop runs at ~60fps during transitions, returns to blocking event wait when idle

## 2.15.0

- MCP server for AI agents
- `purple mcp` lets Claude Code, Cursor and other AI assistants list hosts, run commands and manage containers via JSON-RPC 2.0
- Five MCP tools: list_hosts, get_host, run_command, list_containers and container_action
- Alias validation against SSH config before any SSH execution (prevents connections to hosts outside your config)
- SSH timeout protection on all MCP tool calls including container operations
- Container ID injection prevention via ASCII allowlist validation

## 2.14.2

- Add fuzz-equivalent property tests for SSH config parser
- Arbitrary Unicode and raw byte inputs now verify idempotency and mutation safety (delete, undo, update, swap, add) in CI

## 2.14.1

- Fix Oracle Cloud group header cleanup. Orphaned headers were not removed when all Oracle hosts disappeared
- Shell-escape alias and hostname in custom askpass commands to prevent metacharacter injection
- Strip control characters from provider config values to prevent INI format corruption
- Terminal cleanup on TUI init failure. Raw mode and alternate screen are now restored if cursor hide or clear fails
- Safe PID conversion in snippet process guard
- Deduplicate percent-encoding and date formatting across provider modules
- Cache clipboard command detection to avoid repeated subprocess spawns
- URL-encode Vultr pagination cursor in query parameters
- Replace tautological connection tests with real integration tests

## 2.14.0

- Oracle Cloud Infrastructure (OCI) provider. Sync Compute instances via OCI REST API with RSA-SHA256 request signing
- Multi-region support with TUI region picker (38 regions across Americas, EMEA and Asia Pacific)
- Recursive compartment sync. Point at a tenancy or compartment OCID and all sub-compartments are included
- Authenticate with your existing OCI config file (~/.oci/config). No extra credentials to manage
- Metadata: region, shape, os and status shown in detail panel. Freeform tags synced as provider tags
- Help overlay layout refined. Narrower width, rebalanced columns and tighter key alignment
- Region picker spacing fix in provider form

## 2.13.0

- Context-sensitive help. Press `?` on any screen to see its shortcuts
- Help works in host list, file browser, snippets, containers, tunnels, providers, SSH keys, tag picker and host detail
- Improved visual hierarchy. Section headers are bold, descriptions are dim, keys are right-aligned
- Host list help reorganized into task-based groups: Navigate, View, Forms, Manage Hosts, Connect and Run, Tools
- Smaller help overlays for sub-screens. No duplicate headers, compact sizing
- Missing shortcuts added across all screens (q/Esc, PgDn/PgUp, j/k)
- Help accessible through confirmation guards and search mode

## 2.12.0

- Container management over SSH. Works with Docker and Podman
- Press `C` on any host to see all containers. Start, stop and restart without leaving purple
- Auto-detects Docker or Podman on the remote host. No agent. No web UI. No extra ports
- Cached container data shown in the detail panel after first fetch

## 2.11.1

- Consistent footer spacing across all overlay screens
- Spacer row between content and footer in all overlay screens for cleaner visual separation
- Startup sort now selects the first host in sorted order instead of the first host in config order
- Rebranded from "SSH config manager" to "terminal SSH client" across all user-facing text
- 1500+ new parser robustness tests covering malformed input, quoting edge cases, Match blocks and mutation sequences (4200+ total)

## 2.11.0

- Soft-delete for disappeared provider hosts
- Hosts that vanish from cloud sync are marked stale instead of silently kept or hard-deleted. Stale hosts appear dimmed in the host list and sort to the bottom
- Purge stale hosts with X. Confirmation dialog shows host names (up to 6) before deletion
- Per-provider purge from the provider list (X key scoped to the selected provider)
- Provider list shows per-provider stale count in red with X key hint in footer
- Detail panel shows "Stale" field with relative timestamp in red
- Virtual "stale" tag for filtering (tag:stale fuzzy, tag=stale exact, appears in tag picker)
- Stale connection warning on Enter, edit, delete, clone, tunnels, snippets and file browser
- Editing a stale host clears the stale marker on save
- Stale hosts automatically un-stale when they reappear in the next provider sync (including stopped VMs with empty IP)
- Partial sync failures suppress stale marking to prevent false positives
- Active tunnels cleaned up on purge (after successful config write)
- CLI: `purple sync` prints "Marked N stale." per provider
- Footer separators between every action (consistency fix across all screens)
- Delete confirmation dialog widened to 52 columns (consistent with other dialogs)
- Detail panel route visualization uses display width instead of byte length for Unicode correctness
- Fix missing blank line when adding a provider host before another provider's group header
- 143 new tests covering stale marking, clearing, purge, sort, filter, config integrity and round-trip fidelity (4111 total)

## 2.10.1

- Sparkline now shows your full connection history
- Fix timestamp retention to match sparkline range. History now keeps 365 days instead of 90 so the auto-scaling sparkline can show up to 1 year of connection activity

## 2.10.0

- Smarter forms, visual routes and a sparkline that fits your data
- ProxyJump chain visualization in detail panel. Shows the full hop route (○ you → ● bastion → ● target) with validation for missing hosts in red
- ProxyJump arrow indicator (→) in host list for hosts using a jump host
- Activity sparkline auto-scales to your data. Ranges from 5 days to 1 year based on connection history
- Sparkline shows dotted baseline (·) for empty periods and a midpoint time label for orientation
- Fewer than 3 connections show a compact text list instead of a sparkline
- Dirty-check on Esc. All four form types now ask "Discard changes?" when you press Esc with unsaved edits
- Auto-submit after picker selection. Pick a key, proxy host or password source and the form submits if ready
- Space bar toggles and cycles. Tunnel type and provider booleans now use Space instead of arrow keys
- Arrow keys are cursor-only in all forms. Left/Right never toggle or cycle values
- HostDetail overlay is no longer a dead end. Press e to edit, T for tunnels, r for snippets
- Signal safety during SSH. Ctrl+C reaches SSH normally but no longer kills purple
- Tunnel processes run in their own process group for clean signal isolation
- Context-aware mode badges in title bar (TAGGING, N SELECTED)
- Search footer shows tag syntax hints (tag: fuzzy, tag= exact) and improved match count (N of M)
- Import confirmation accepts both y and Y
- Consistent footer separators (│) across all screens with shared helper functions
- Help screen updated with Space toggle, detail panel scroll, snippet output navigation and smart-paste hint
- Smart-paste placeholder in Alias field shows user@host:port format
- Edit form title shows the host alias being edited
- 62 new tests covering dirty-check, delete confirmations, navigation, ProxyJump chain resolution and sparkline behavior (3968 total)

## 2.9.0

- Redesigned host list with smarter column layout and provider tag separation
- Provider tags are now stored in a dedicated comment and always mirror the remote. Your own tags are never touched by sync
- Two-cluster column system. Left cluster (name and host) and right cluster (auth, tags, last) separated by a flexible gap
- Add header underline and bold column headers for better scannability
- Add sort indicator next to the active sort column name
- Add selection indicator on the left edge of the selected row
- Show dash for empty auth and last cells instead of blank space
- Show read-only provider tags in the tag edit bar
- Group headers show a horizontal leader line after the label
- Tighter column gaps (2-3 fixed) for a more compact and professional look
- Shorten time labels in the last column (5m instead of 5m ago)
- Sanitize tag values: strip control characters, commas, bidi overrides and enforce max length
- Remove --reset-tags CLI flag (no longer needed)

## 2.8.1

- Add CI workflow with format, clippy, test, cargo-deny and MSRV checks
- Fix parser handling of lone \r line endings breaking round-trip idempotency
- Add property-based and fuzz testing for SSH config parser
- Add Dependabot for weekly cargo and GitHub Actions updates
- Add cargo-deny for license and vulnerability scanning
- Update GitHub Actions to latest versions (checkout v6, upload-artifact v7, download-artifact v8)
- Update rustls-webpki to 0.103.10 (security fix)

## 2.8.0

- Welcome screen shows host count and offers known_hosts import on first launch
- Import hosts from ~/.ssh/known_hosts with I key

## 2.7.1

- Detail panel tags wrap across multiple lines to fit panel width
- Update badge headline truncates with ellipsis instead of being clipped

## 2.7.0

- Provider metadata uses provider-specific terminology (instance, vm_size, zone, location, image, specs)
- Improved SSH config compatibility: UTF-8 BOM, Host= syntax, ${VAR} in includes, quoted paths, depth 16
- Automatic repair of absorbed group comments and orphaned group headers
- Synced hosts insert adjacent to existing provider group for consistent grouping
- Multi-level undo for host deletion (up to 50 levels)
- Welcome screen with one-time backup of original SSH config to ~/.purple/config.original
- Advisory file locking prevents concurrent write corruption
- New hosts insert before trailing Host \* blocks to preserve SSH first-match-wins ordering
- Inline comments preserved when updating directives
- UpCloud boot disk preferred over first storage device for image metadata
- Scaleway pagination via response body instead of X-Total-Count header
- Proxmox QEMU OS type labels match qm.conf(5) manpage
- Atomic writes call fsync before rename and clean up temp files on failure

## 2.6.0

- Added release notes to update flow and GitHub releases
- TUI update badge shows changelog headline from GitHub release body
- Full release notes displayed after `purple update` with markdown stripping
- Release workflow extracts changelog section as GitHub release body
- Added CHANGELOG.md with full release history

## 2.5.0

- Improved Hetzner location migration and GCP zones/IPv6 support
- Added provider metadata (region, plan, os, status) to sync and detail panel
- Added Tailscale to provider badges on landing pages

## 2.4.0

- Added Tailscale provider with local CLI and HTTP API support

## 2.3.0

- Added Linux support for pre-built binaries, installer and self-update

## 2.2.0

- Improved snippet picker: column headers, aligned layout, allow spaces in names, rename raw to terminal

## 2.1.0

- Added in-TUI snippet output, parameterized snippets, snippet search, parallel execution and terminal fallback

## 2.0.4

- Fixed status message leaking into overlay footers

## 2.0.3

- Added file browser sort directions and sync history persistence
- Improved footer and help overlays

## 2.0.2

- Fixed symlink handling in file browser
- Rewritten product messaging for TUI-first positioning

## 2.0.1

- Fixed Include equals-sign parsing, stale multi-select, tag input cursor and sort selection persistence

## 2.0.0

- Added remote file explorer with scp transfer

## 1.28.2

- Fixed 9 bugs found during code review

## 1.28.1

- Redesigned help overlay with two-column layout
- Added getpurple.sh label to host list

## 1.28.0

- Added Azure cloud provider sync

## 1.27.0

- Added GCP Compute Engine cloud provider sync

## 1.26.1

- Dimmed group headers in host list for better visual hierarchy

## 1.26.0

- Redesigned host list with composite host column, purple accent theme and active tunnel visibility

## 1.25.1

- Fixed UI/UX consistency across footers, forms, lists and delete confirmations

## 1.25.0

- Added Scaleway cloud provider sync

## 1.24.0

- Added AWS EC2 cloud provider sync

## 1.23.1

- Fixed keychain migration safety on alias rename

## 1.23.0

- Added activity sparkline, history timestamps and detail scroll
- Improved form clarity and performance

## 1.22.0

- Stream snippet output in real-time instead of buffering

## 1.21.0

- Added full provider metadata and volatile sync
- Improved UI consistency and help attribution

## 1.20.0

- Added provider metadata sync with detail panel display

## 1.19.0

- Added command snippets with multi-host execution

## 1.18.0

- Added ProxyJump picker to host form

## 1.17.0

- Redesigned UI with rounded borders, column layout and compact forms

## 1.16.0

- Added split-pane detail panel with v key toggle

## 1.15.0

- Detect changed host keys and offer to remove old key and reconnect

## 1.14.2

- Fixed ping indicator colored space on selection
- Preserved host selection after SSH

## 1.14.1

- Fixed Left/Right toggle for VerifyTls and AutoSync fields

## 1.14.0

- Added cursor navigation in forms

## 1.13.1

- Fixed tests overwriting ~/.purple/providers
- Preserved selection after host edit

## 1.13.0

- Added SSH password management (keychain, 1Password, Bitwarden, pass, Vault)

## 1.12.0

- Hardened Proxmox provider deserialization
- Added --auto-sync/--no-auto-sync CLI flags

## 1.11.0

- Added Proxmox VE provider

## 1.10.0

- Added per-provider auto_sync toggle

## 1.9.1

- Fixed self-update failing on GitHub redirect

## 1.9.0

- Sort provider list by last sync
- Show footer shortcuts with status on all screens

## 1.8.2

- Fixed redirect following, key_detail height, width-aware truncate, deduplicate providers, validate parse_target

## 1.8.1

- Fixed missing space before update notification in title bar

## 1.8.0

- Added sync history in provider list

## 1.7.0

- Fixed tag selection reset, merged sync tags
- Added --reset-tags flag

## 1.6.0

- Added self-update and TUI version check
- Added getpurple.sh landing page, install script and Bunny edge worker

## 1.5.0

- Added tunnel management

## 1.4.2

- Fixed sync write-failure rollback, cancel-flag replacement, provider config dedup, scoped IPv6 detection

## 1.4.1

- Fixed alias_prefix validation, sync rename stability, tag-edit reload guard, equals-syntax preservation

## 1.4.0

- Added sync cancellation, token env var and atomic_write extraction

## 1.3.0

- Added group-by-provider and form conflict detection
- Hardened parser and improved sync

## 1.2.0

- Added UpCloud provider

## 1.1.1

- Fixed provider CLI config dependency, known_hosts port validation, hex hostname skip, import duplicate counter, token masking UTF-8 panic

## 1.1.0

- Added cloud provider sync (DigitalOcean, Vultr, Linode, Hetzner)

## 1.0.2

- Fixed parser = splitting, shell-quote clipboard, throttle ping-all, quote-aware comments, import reporting, symlink writer, auto-reload guard

## 1.0.1

- Fixed ping dual-stack, event thread hang, known_hosts parser, add-host rollback

## 1.0.0

- Fixed known_hosts wildcard import
- Preserved inline comments on edit

## 0.11.1

- Fixed alias whitespace validation, tab multi-pattern filtering, edit tag rollback, import group headers, included file trailing comments

## 0.11.0

- Added tags to form
- Fixed Include-in-block parsing, inline comments, wildcard validation, search restore and rollback formatting

## 0.10.5

- Fixed broken undo, write-failure rollback, include dir reload, CLI alias validation

## 0.10.4

- Fixed Unicode panic, tab parsing, CRLF preservation, include reload, import errors

## 0.10.3

- Fixed stale edit index, clipboard exit check, search ping guard, known_hosts markers, IPv6 validation

## 0.10.2

- Fixed DNS timeout, undo-on-reload, raw mode guard, deterministic history

## 0.10.1

- Fixed zombie processes, stale delete index, UTF-8 panics, Unicode width, key casing, permission races, panic hook ordering, IPv6 parsing

## 0.10.0

- Columnar layout, key picker comments, simpler keybindings

## 0.9.0

- Added tag picker, search-by-tag and key list improvements

## 0.8.0

- Monochrome theme with purple brand badge

## 0.7.0

- Most recent sort mode and purple accent colors

## 0.6.1

- Reverted Magenta borders, fixed title text readability

## 0.6.0

- Added sort mode persistence and Magenta borders

## 0.5.2

- Fixed brand badge readability across terminal themes

## 0.5.1

- Brand badge title and lowercase branding

## 0.5.0

- Design, UX and accessibility improvements

## 0.4.2

- Fixed round-trip formatting: Include parsing, indentation, blank lines, tags and swap

## 0.4.1

- Fixed SSH connection delay, terminal robustness and atomic write improvements

## 0.4.0

- Added clone, sort, tags, import, inspect, export, undo, auto-reload and connection history

## 0.3.1

- Fixed zombie processes in clipboard detection, search position restore, ProxyJump ping, Linux clipboard support

## 0.3.0

- Added search, ping, grouping, clipboard, quick-add, Include support and shell completions

## 0.2.0

- Added SSH key management (key list, key detail, key picker)

## 0.1.0

- Initial release
