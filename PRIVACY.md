# purple. Privacy Policy

Last updated: 2026-10-31

purple is an open-source SSH manager published by Eric Kochen as an individual. This policy covers the binary, the MCP server (`purple mcp`) and the Claude Desktop Extension (`.mcpb`).

## Collection

None. No backend, no telemetry, no analytics, no third-party processors.

## Usage

purple processes data only on your machine to serve the function you invoked.

## Storage

Everything stays local under `~/.ssh/config` and `~/.purple/`. Provider API tokens live in `~/.purple/providers/*.toml` as plaintext. The MCP audit log `~/.purple/mcp-audit.log` is created with mode `0o600` and records `ts`, `tool`, `args`, `outcome`, `reason` per call. The `args` of `run_command` are replaced with `<redacted>`.

## Sharing

purple does not share data with anyone. When invoked through an MCP client (for example Claude Desktop), tool results purple returns are passed back to that client and then handled under its policy. purple itself sees only the structured tool calls, not your prompts.

## Network

Outbound HTTPS only in these cases:

- TUI startup: background version check to `api.github.com` (or `github.com/erickochen/purple/releases/latest` when the API refuses), cached 1h in `~/.purple/last_version_check`. The MCP server and `.mcpb` bundle skip this check.
- `purple sync`: cloud APIs you configured (18 providers including AWS, GCP, Azure, Hetzner, Proxmox, OCI, NetBox).
- `purple update`: `api.github.com` (or `github.com/erickochen/purple/releases/latest` when the API refuses) and `github.com/erickochen/purple/releases/download/`.
- `purple vault sign`: your HashiCorp Vault server.
- SSH connections you trigger from the TUI or via `run_command` and `list_containers`.

No phone home, no crash reports, no feature flags.

## Retention

You control retention. `~/.purple/purple.log` is auto-rotated at 5 MB. The version check cache is overwritten on refresh. All other files are never auto-deleted by purple.

## Contact

<https://github.com/erickochen/purple/issues> or <hello@getpurple.sh>.
