import * as BunnySDK from "@bunny.net/edgescript-sdk";

// Embedded copy of site/install.sh (source of truth).
// Must stay in sync — CI checks for drift on every PR and push (site.yml).
const INSTALL_SCRIPT = `#!/bin/sh
# Source of truth for the install script.
# Also embedded in worker.ts — keep both in sync.
# CI checks for drift on every PR and push (site.yml).
set -eu

REPO="erickochen/purple"
BINARY="purple"

main() {
    printf "\\n  \\033[1mpurple.\\033[0m installer\\n\\n"

    # Detect OS (before dependency checks so non-macOS gets a clear message)
    os="$(uname -s)"
    case "$os" in
        Darwin) ;;
        Linux)
            printf "  \\033[1m!\\033[0m Pre-built binaries are macOS only for now.\\n"
            printf "  Install via cargo instead:\\n\\n"
            printf "    cargo install purple-ssh\\n\\n"
            exit 1
            ;;
        *)
            printf "  \\033[1m!\\033[0m Unsupported OS: %s\\n" "$os"
            printf "  Install via cargo instead:\\n\\n"
            printf "    cargo install purple-ssh\\n\\n"
            exit 1
            ;;
    esac

    # Check dependencies (after OS detection so non-macOS exits with a clear message)
    need_cmd curl
    need_cmd tar
    need_cmd shasum

    # Detect architecture
    arch="$(uname -m)"
    case "$arch" in
        arm64|aarch64) target="aarch64-apple-darwin" ;;
        x86_64)        target="x86_64-apple-darwin" ;;
        *)
            printf "  \\033[1m!\\033[0m Unsupported architecture: %s\\n" "$arch"
            exit 1
            ;;
    esac

    # Get latest version
    printf "  Fetching latest release...\\n"
    version="$(curl -fsSL "https://api.github.com/repos/\${REPO}/releases/latest" \\
        | grep '"tag_name"' | head -1 | sed 's/.*"v\\(.*\\)".*/\\1/')"

    if [ -z "$version" ] || ! printf '%s' "$version" | grep -qE '^[0-9]+\\.[0-9]+\\.[0-9]+$'; then
        printf "  \\033[1m!\\033[0m Failed to fetch latest version.\\n"
        printf "  GitHub API may be rate-limited. Try again later or install via:\\n\\n"
        printf "    brew install erickochen/purple/purple\\n\\n"
        exit 1
    fi

    printf "  Found v%s for %s\\n" "$version" "$target"

    # Set up temp directory
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT

    tarball="purple-\${version}-\${target}.tar.gz"
    url="https://github.com/\${REPO}/releases/download/v\${version}/\${tarball}"
    sha_url="\${url}.sha256"

    # Download tarball and checksum
    printf "  Downloading...\\n"
    curl -fsSL "$url" -o "\${tmp}/\${tarball}"
    curl -fsSL "$sha_url" -o "\${tmp}/\${tarball}.sha256"

    # Verify checksum
    printf "  Verifying checksum...\\n"
    expected="$(awk '{print $1}' "\${tmp}/\${tarball}.sha256")"
    actual="$(shasum -a 256 "\${tmp}/\${tarball}" | awk '{print $1}')"

    if [ "$expected" != "$actual" ]; then
        printf "  \\033[1m!\\033[0m Checksum mismatch.\\n"
        printf "    Expected: %s\\n" "$expected"
        printf "    Got:      %s\\n" "$actual"
        exit 1
    fi

    # Extract
    tar -xzf "\${tmp}/\${tarball}" -C "$tmp"

    # Install
    install_dir="/usr/local/bin"
    if [ ! -w "$install_dir" ]; then
        install_dir="\${HOME}/.local/bin"
        mkdir -p "$install_dir"
    fi

    cp "\${tmp}/\${BINARY}" "\${install_dir}/\${BINARY}"
    chmod 755 "\${install_dir}/\${BINARY}"

    printf "\\n  \\033[1;35mpurple v%s\\033[0m installed to %s/%s\\n\\n" \\
        "$version" "$install_dir" "$BINARY"

    printf "  To update later, run: purple update\\n\\n"

    # Check PATH
    case ":\${PATH}:" in
        *":\${install_dir}:"*) ;;
        *)
            printf "  Add %s to your PATH:\\n\\n" "$install_dir"
            printf "    export PATH=\\"%s:\\$PATH\\"\\n\\n" "$install_dir"
            ;;
    esac
}

need_cmd() {
    if ! command -v "$1" > /dev/null 2>&1; then
        printf "  \\033[1m!\\033[0m Required command not found: %s\\n" "$1"
        exit 1
    fi
}

main "$@"
`;

const LANDING_PAGE = `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>purple. SSH config manager and host launcher for the terminal</title>
<meta name="description" content="Free, open-source SSH config manager, editor and host launcher. TUI with search, tags, tunnels, password management (keychain, 1Password, Bitwarden, pass, Vault), cloud provider sync (DigitalOcean, Vultr, Linode, Hetzner, UpCloud, Proxmox VE) and round-trip fidelity for ~/.ssh/config. Written in Rust. macOS and Linux.">
<meta name="keywords" content="SSH config manager, SSH launcher, terminal SSH, TUI SSH, SSH host manager, cloud SSH sync, DigitalOcean SSH, Vultr SSH, Linode SSH, Hetzner SSH, UpCloud SSH, Proxmox SSH, SSH tunnel manager, SSH config editor, Rust SSH tool, purple-ssh, SSH password manager, SSH askpass, SSH keychain, 1Password SSH, Bitwarden SSH">
<meta name="robots" content="index, follow">
<meta name="author" content="Erick Ochen">
<meta property="og:title" content="purple. SSH config manager and host launcher for the terminal">
<meta property="og:description" content="Free, open-source TUI that turns ~/.ssh/config into a searchable, taggable host launcher. Sync servers from 6 cloud providers. Manage SSH passwords. Written in Rust.">
<meta property="og:type" content="website">
<meta property="og:url" content="https://getpurple.sh">
<meta property="og:image" content="https://raw.githubusercontent.com/erickochen/purple/master/demo.gif">
<meta property="og:site_name" content="purple">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="purple. SSH config manager and host launcher">
<meta name="twitter:description" content="Free, open-source TUI for managing SSH configs. Search, tag, sync cloud providers, manage passwords. Written in Rust.">
<meta name="twitter:image" content="https://raw.githubusercontent.com/erickochen/purple/master/demo.gif">
<link rel="canonical" href="https://getpurple.sh">
<script type="application/ld+json">
{
  "@context": "https://schema.org",
  "@type": "SoftwareApplication",
  "name": "purple",
  "alternateName": "purple-ssh",
  "description": "SSH config manager, editor and host launcher for the terminal. TUI with search, tags, tunnels, password management and cloud provider sync for ~/.ssh/config.",
  "applicationCategory": "DeveloperApplication",
  "operatingSystem": "macOS, Linux",
  "url": "https://getpurple.sh",
  "downloadUrl": "https://getpurple.sh",
  "installUrl": "https://github.com/erickochen/purple/releases",
  "softwareVersion": "1.13.0",
  "programmingLanguage": "Rust",
  "license": "https://opensource.org/licenses/MIT",
  "codeRepository": "https://github.com/erickochen/purple",
  "offers": {
    "@type": "Offer",
    "price": "0",
    "priceCurrency": "USD"
  },
  "author": {
    "@type": "Person",
    "name": "Erick Ochen",
    "url": "https://github.com/erickochen"
  },
  "featureList": [
    "SSH config round-trip fidelity",
    "Fuzzy search across hosts",
    "Host tagging and filtering",
    "SSH tunnel management",
    "Cloud provider sync: DigitalOcean, Vultr, Linode, Hetzner, UpCloud, Proxmox VE",
    "Password management: OS Keychain, 1Password, Bitwarden, pass, HashiCorp Vault",
    "Bulk import from known_hosts",
    "SSH key management",
    "Atomic writes with automatic backups",
    "Shell completions for Bash, zsh, fish"
  ]
}
<\/script>
<script type="application/ld+json">
{
  "@context": "https://schema.org",
  "@type": "FAQPage",
  "mainEntity": [
    {
      "@type": "Question",
      "name": "Does purple modify my existing SSH config?",
      "acceptedAnswer": {
        "@type": "Answer",
        "text": "Only when you add, edit, delete or sync. All writes are atomic with automatic backups. Auto-sync runs on startup for providers that have it enabled (configurable per provider)."
      }
    },
    {
      "@type": "Question",
      "name": "Will purple break my SSH config comments or formatting?",
      "acceptedAnswer": {
        "@type": "Answer",
        "text": "No. purple preserves comments, indentation and unknown directives through every read-write cycle. Consecutive blank lines are collapsed to one."
      }
    },
    {
      "@type": "Question",
      "name": "Does purple need a daemon or background process?",
      "acceptedAnswer": {
        "@type": "Answer",
        "text": "No. purple is a single Rust binary. Run it, use it, close it. No runtime, no daemon, no async framework."
      }
    },
    {
      "@type": "Question",
      "name": "Does purple send my SSH config anywhere?",
      "acceptedAnswer": {
        "@type": "Answer",
        "text": "No. Your config never leaves your machine. Provider sync calls cloud APIs to fetch server lists. The TUI checks GitHub for new releases on startup (cached for 24 hours). No config data is transmitted."
      }
    },
    {
      "@type": "Question",
      "name": "How does SSH password management work in purple?",
      "acceptedAnswer": {
        "@type": "Answer",
        "text": "Set a password source per host via the TUI or a global default. When you connect, purple acts as SSH_ASKPASS and retrieves the password automatically. Supported sources: OS Keychain, 1Password, Bitwarden, pass, HashiCorp Vault and custom commands."
      }
    },
    {
      "@type": "Question",
      "name": "Can I use purple with SSH Include files?",
      "acceptedAnswer": {
        "@type": "Answer",
        "text": "Yes. Hosts from Include files are displayed in the TUI but never modified. purple resolves Include directives recursively (up to depth 5) with tilde and glob expansion."
      }
    }
  ]
}
<\/script>
<style>
* { margin: 0; padding: 0; box-sizing: border-box; }
body {
  background: #0a0a0a;
  color: #e0e0e0;
  font-family: "SF Mono", "Fira Code", "JetBrains Mono", "Cascadia Code", Menlo, Monaco, "Courier New", monospace;
  line-height: 1.6;
  min-height: 100vh;
  display: flex;
  flex-direction: column;
  align-items: center;
}
main {
  max-width: 720px;
  width: 100%;
  padding: 80px 24px 60px;
}
h1 {
  font-size: 3rem;
  font-weight: 700;
  letter-spacing: -0.02em;
  margin-bottom: 8px;
}
h1 span { color: #9333ea; }
.tagline {
  color: #888;
  font-size: 1rem;
  margin-bottom: 48px;
}
.install-box {
  background: #161616;
  border: 1px solid #2a2a2a;
  border-radius: 8px;
  padding: 20px 24px;
  margin-bottom: 16px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}
.install-box code {
  font-size: 1rem;
  color: #fff;
  white-space: nowrap;
}
.install-box code .dim { color: #777; }
.copy-btn {
  background: none;
  border: 1px solid #333;
  border-radius: 6px;
  color: #888;
  padding: 6px 12px;
  font-family: inherit;
  font-size: 0.8rem;
  cursor: pointer;
  transition: all 0.15s;
  white-space: nowrap;
}
.copy-btn:hover { border-color: #9333ea; color: #fff; }
.alt-methods {
  color: #555;
  font-size: 0.85rem;
  margin-bottom: 56px;
  line-height: 1.8;
}
.alt-methods a {
  color: #888;
  text-decoration: none;
  border-bottom: 1px solid #333;
  transition: all 0.15s;
}
.alt-methods a:hover { color: #9333ea; border-color: #9333ea; }
.demo {
  margin-bottom: 56px;
  margin-left: calc(50% - min(550px, 50vw));
  width: min(1100px, 100vw);
}
.demo img {
  width: 100%;
  border-radius: 8px;
  border: 1px solid #2a2a2a;
}
h2 {
  font-size: 1.2rem;
  font-weight: 600;
  margin-bottom: 16px;
  color: #e0e0e0;
}
.features {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px 32px;
  margin-bottom: 56px;
  font-size: 0.9rem;
}
.features div {
  color: #888;
}
.features div strong {
  color: #e0e0e0;
  font-weight: 600;
}
section {
  margin-bottom: 56px;
}
section p {
  color: #888;
  font-size: 0.9rem;
  margin-bottom: 12px;
}
section code {
  background: #161616;
  padding: 2px 6px;
  border-radius: 4px;
  font-size: 0.85rem;
}
.providers {
  display: grid;
  grid-template-columns: 1fr 1fr 1fr;
  gap: 8px;
  font-size: 0.85rem;
  color: #888;
  margin-bottom: 12px;
}
.providers div {
  background: #161616;
  border: 1px solid #2a2a2a;
  border-radius: 6px;
  padding: 8px 12px;
  text-align: center;
}
footer {
  color: #444;
  font-size: 0.8rem;
  padding-bottom: 40px;
}
footer a {
  color: #555;
  text-decoration: none;
  border-bottom: 1px solid #333;
  transition: all 0.15s;
}
footer a:hover { color: #9333ea; border-color: #9333ea; }
.faq dt {
  color: #e0e0e0;
  font-weight: 600;
  margin-bottom: 4px;
}
.faq dd {
  color: #888;
  font-size: 0.9rem;
  margin-bottom: 20px;
  margin-left: 0;
}
@media (max-width: 540px) {
  main { padding: 48px 16px 40px; }
  h1 { font-size: 2.2rem; }
  .install-box { padding: 16px; }
  .install-box code { font-size: 0.85rem; }
  .features { grid-template-columns: 1fr; }
  .providers { grid-template-columns: 1fr 1fr; }
}
</style>
</head>
<body>
<main>
  <h1>purple<span>.</span></h1>
  <p class="tagline">SSH config manager and host launcher for the terminal</p>

  <div class="install-box">
    <code><span class="dim">$</span> curl -fsSL getpurple.sh | sh</code>
    <button class="copy-btn" onclick="copy(this)">copy</button>
  </div>

  <div class="alt-methods">
    or via <a href="https://github.com/erickochen/homebrew-purple">Homebrew</a>:
    brew install erickochen/purple/purple<br>
    or via <a href="https://crates.io/crates/purple-ssh">cargo</a>:
    cargo install purple-ssh
  </div>

  <div class="demo">
    <img src="https://raw.githubusercontent.com/erickochen/purple/master/demo.gif"
         alt="purple SSH config manager TUI demo: searching hosts, connecting via SSH and syncing cloud providers in the terminal" loading="lazy" width="1100" height="600">
  </div>

  <h2>Features</h2>
  <div class="features">
    <div><strong>Search.</strong> Fuzzy find across aliases, hostnames, users and tags</div>
    <div><strong>Tags.</strong> Organize hosts with #tags and filter instantly</div>
    <div><strong>Tunnels.</strong> Manage SSH port forwards (local, remote, dynamic) per host</div>
    <div><strong>Ping.</strong> TCP connectivity check from the TUI</div>
    <div><strong>Round-trip fidelity.</strong> Preserves comments, formatting and unknown directives</div>
    <div><strong>Bulk import.</strong> From hosts files or ~/.ssh/known_hosts</div>
    <div><strong>Passwords.</strong> OS Keychain, 1Password, Bitwarden, pass, Vault integration</div>
    <div><strong>SSH keys.</strong> Browse keys with metadata and linked hosts</div>
    <div><strong>Cloud sync.</strong> Pull servers from 6 cloud providers into your config</div>
    <div><strong>Self-update.</strong> Run <code>purple update</code></div>
    <div><strong>Atomic writes.</strong> Temp file, chmod 600, rename. Automatic backups</div>
    <div><strong>Completions.</strong> Bash, zsh and fish via <code>purple --completions</code></div>
  </div>

  <section>
    <h2>Cloud provider sync</h2>
    <p>Pull servers from six cloud providers directly into your <code>~/.ssh/config</code>. Sync adds new hosts, updates changed IPs and merges tags. Tags you add manually are preserved across syncs.</p>
    <div class="providers">
      <div>DigitalOcean</div>
      <div>Vultr</div>
      <div>Linode</div>
      <div>Hetzner</div>
      <div>UpCloud</div>
      <div>Proxmox VE</div>
    </div>
    <p>Preview changes with <code>--dry-run</code>. Remove deleted hosts with <code>--remove</code>. Replace local tags with <code>--reset-tags</code>.</p>
  </section>

  <section>
    <h2>Your config, respected</h2>
    <p>purple reads and writes <code>~/.ssh/config</code> directly with full round-trip fidelity. Comments, indentation, unknown directives, CRLF line endings and Include files are all preserved. Every write is atomic (temp file, chmod 600, rename) with automatic backups.</p>
  </section>

  <section>
    <h2>Built with Rust</h2>
    <p>Single binary. No runtime, no daemon, no async framework. 2400+ tests. Zero clippy warnings. MIT licensed.</p>
  </section>

  <section>
    <h2>FAQ</h2>
    <dl class="faq">
      <dt>Does purple modify my existing SSH config?</dt>
      <dd>Only when you add, edit, delete or sync. All writes are atomic with automatic backups. Auto-sync runs on startup for providers that have it enabled (configurable per provider).</dd>
      <dt>Will purple break my comments or formatting?</dt>
      <dd>No. purple preserves comments, indentation and unknown directives through every read-write cycle.</dd>
      <dt>Does purple need a daemon or background process?</dt>
      <dd>No. It is a single Rust binary. Run it, use it, close it.</dd>
      <dt>Does purple send my SSH config anywhere?</dt>
      <dd>No. Your config never leaves your machine. Provider sync calls cloud APIs to fetch server lists. No config data is transmitted.</dd>
      <dt>How does password management work?</dt>
      <dd>Set a password source per host via the TUI or a global default. When you connect, purple acts as SSH_ASKPASS and retrieves the password automatically. Supported sources: OS Keychain, 1Password, Bitwarden, pass, HashiCorp Vault and custom commands.</dd>
      <dt>Can I use purple with Include files?</dt>
      <dd>Yes. Hosts from Include files are displayed in the TUI but never modified. purple resolves Include directives recursively (up to depth 5) with tilde and glob expansion.</dd>
    </dl>
  </section>

</main>

<footer>
  <a href="https://github.com/erickochen/purple">GitHub</a> \u00b7 <a href="https://crates.io/crates/purple-ssh">crates.io</a> \u00b7 MIT License
</footer>

<script>
function copy(btn) {
  navigator.clipboard.writeText("curl -fsSL getpurple.sh | sh").then(function() {
    btn.textContent = "copied";
    setTimeout(function() { btn.textContent = "copy"; }, 2000);
  }).catch(function() {});
}
<\/script>
</body>
</html>`;

const LLMS_TXT = `# purple

> SSH config manager and host launcher for the terminal

purple is a free, open-source TUI that turns ~/.ssh/config into a searchable, taggable host launcher with full round-trip fidelity. Single Rust binary. macOS and Linux. MIT licensed.

## What purple does

purple reads your existing ~/.ssh/config, gives you a terminal UI to search, filter, tag and connect to hosts, and writes changes back without touching your comments, formatting or unknown directives. It syncs servers from six cloud providers directly into your SSH config. No browser, no YAML files, no context switching.

## Key capabilities

- Reads, edits and writes ~/.ssh/config directly while preserving comments, formatting and unknown directives (round-trip fidelity)
- Fuzzy search across aliases, hostnames, users, tags and providers
- Host tagging via SSH config comments (# purple:tags)
- Cloud provider sync: DigitalOcean, Vultr, Linode (Akamai), Hetzner, UpCloud, Proxmox VE
- SSH tunnel management: LocalForward, RemoteForward, DynamicForward. Start/stop from TUI or CLI
- Password management: OS Keychain, 1Password (op://), Bitwarden (bw:), pass (pass:), HashiCorp Vault (vault:), custom command
- SSH key browsing with metadata (type, bits, fingerprint) and host linking
- Bulk import from hosts files or ~/.ssh/known_hosts
- Frecency-based connection history and sorting
- TCP ping / connectivity check per host or all at once
- Atomic writes with automatic backups (last 5). Temp file, chmod 600, rename
- Include file support (read-only, recursive up to depth 5, tilde + glob expansion)
- Shell completions (bash, zsh, fish)
- Self-update mechanism (macOS curl installs). Homebrew and cargo users update via their package manager
- Auto-reload: detects external config changes every 4 seconds
- Minimal UI with monochrome base and subtle color for status. Works in any terminal, respects NO_COLOR

## Install

curl -fsSL getpurple.sh | sh
brew install erickochen/purple/purple
cargo install purple-ssh

## CLI usage

purple                              # Launch the TUI
purple --config ~/other/ssh_config  # Use alternate config file
purple myserver                     # Connect if exact match, otherwise open TUI with search
purple -c myserver                  # Direct connect (skip the TUI)
purple --list                       # List all configured hosts
purple add deploy@10.0.1.5:22      # Quick-add a host
purple add user@host --alias name   # Quick-add with custom alias
purple add user@host --key ~/.ssh/id_ed25519  # Quick-add with key
purple import hosts.txt             # Bulk import from file
purple import --known-hosts         # Import from ~/.ssh/known_hosts
purple provider add digitalocean --token TOKEN
purple provider add proxmox --url https://pve:8006 --token user@pam!token=secret
purple provider add digitalocean --token TOKEN --no-auto-sync   # --auto-sync to re-enable
purple provider list                # List configured providers
purple provider remove digitalocean # Remove provider
purple sync                         # Sync all providers
purple sync digitalocean            # Sync single provider
purple sync --dry-run               # Preview changes
purple sync --remove                # Remove hosts deleted from provider
purple sync --reset-tags            # Replace local tags with provider tags
purple tunnel list                  # List all tunnels
purple tunnel list myserver         # List tunnels for a host
purple tunnel add myserver L:8080:localhost:80
purple tunnel remove myserver L:8080:localhost:80
purple tunnel start myserver        # Start tunnel (Ctrl+C to stop)
purple password set myserver        # Store password in OS keychain
purple password remove myserver     # Remove from keychain
purple update                       # Self-update
purple --completions zsh            # Generate shell completions

## Cloud provider sync

Sync servers from cloud providers into ~/.ssh/config. Each synced host is tracked via a comment (# purple:provider name:id) so purple knows which hosts belong to which provider.

Supported providers: DigitalOcean, Vultr, Linode (Akamai), Hetzner, UpCloud and Proxmox VE. Tags and labels from each provider are synced. Proxmox supports self-signed TLS certificates.

Per-provider auto_sync toggle controls startup sync. Default is true for all providers except Proxmox (default false). Manual sync via the TUI (s key) or CLI always works. Preview changes with --dry-run. Remove deleted hosts with --remove. Replace local tags with --reset-tags.

## Password management

purple can retrieve SSH passwords automatically on connect. Set a password source per host via the TUI form or a global default in ~/.purple/preferences. purple acts as its own SSH_ASKPASS program.

Supported password sources:
- OS Keychain (keychain): uses security command on macOS, secret-tool on Linux. Service name purple-ssh
- 1Password (op://): vault/item/field path
- Bitwarden (bw:): item name
- pass (pass:): entry path in the password store
- HashiCorp Vault (vault:): secret path
- Custom command: any shell command that outputs the password. Supports %a (alias) and %h (hostname) substitution. Optional cmd: prefix

## SSH tunnel management

Manage LocalForward, RemoteForward and DynamicForward rules per host. Start and stop background SSH tunnels from the TUI (T key) or CLI. Active tunnels run as ssh -N background processes and are cleaned up on exit.

## Tags

Tags are stored as SSH config comments (# purple:tags prod,us-east). Filter with tag: prefix in search (fuzzy match) or tag= prefix (exact match). Provider names appear as virtual tags. The tag picker (# key) shows all tags with host counts.

## Round-trip fidelity

purple preserves through every read-write cycle:
- Comments (including inline comments)
- Indentation (spaces, tabs)
- Unknown directives
- CRLF line endings
- Equals-syntax (Host = value)
- Match blocks (stored as inert global lines)
- Include file references

Consecutive blank lines are collapsed to one. Hosts from Include files are displayed but never modified.

## Technical details

- Language: Rust
- Platforms: macOS and Linux
- Binary name: purple (crate name: purple-ssh)
- Tests: 2400+ (unit + integration)
- No async runtime. Single binary, no daemon
- Atomic writes via temp file + chmod 600 + rename
- Uses system ssh binary with -F <config_path>
- License: MIT

## FAQ

Q: Does purple modify my existing SSH config?
A: Only when you add, edit, delete or sync. All writes are atomic with automatic backups. Auto-sync runs on startup for providers that have it enabled.

Q: Will purple break my comments or formatting?
A: No. Comments, indentation and unknown directives are preserved through every read-write cycle.

Q: Does purple need a daemon or background process?
A: No. It is a single binary. Run it, use it, close it.

Q: Does purple send my SSH config anywhere?
A: No. Your config never leaves your machine. Provider sync calls cloud APIs to fetch server lists. The TUI checks GitHub for new releases on startup (cached for 24 hours). No config data is transmitted.

Q: How does password management work?
A: Set a password source per host. When you connect, purple acts as SSH_ASKPASS and retrieves the password automatically. Supported sources: OS Keychain, 1Password, Bitwarden, pass, HashiCorp Vault and custom commands.

Q: Can I use purple with Include files?
A: Yes. Hosts from Include files are displayed in the TUI but never modified.

Q: How does provider sync handle name conflicts?
A: Synced hosts get an alias prefix (e.g. do-web-1 for DigitalOcean). If a name collides, purple appends a numeric suffix (do-web-1-2).

## Links

- Website: https://getpurple.sh
- GitHub: https://github.com/erickochen/purple
- Crate: https://crates.io/crates/purple-ssh
- License: MIT
`;

BunnySDK.net.http.serve(async (request: Request): Promise<Response> => {
  const url = new URL(request.url);

  // Redirect purple-ssh.com → getpurple.sh
  const host = request.headers.get("host") || "";
  if (host === "purple-ssh.com" || host === "www.purple-ssh.com" || host === "www.getpurple.sh") {
    return Response.redirect(`https://getpurple.sh${url.pathname}${url.search}`, 301);
  }
  if (url.pathname === "/llms.txt") {
    return new Response(LLMS_TXT, {
      headers: {
        "content-type": "text/plain; charset=utf-8",
        "cache-control": "public, max-age=3600",
      },
    });
  }

  const ua = (request.headers.get("user-agent") || "").toLowerCase();
  const isCli =
    ua.startsWith("curl") ||
    ua.startsWith("wget") ||
    ua.startsWith("fetch") ||
    ua.startsWith("httpie");

  if (isCli) {
    return new Response(INSTALL_SCRIPT, {
      headers: {
        "content-type": "text/plain; charset=utf-8",
        "cache-control": "public, max-age=300",
      },
    });
  }

  return new Response(LANDING_PAGE, {
    headers: {
      "content-type": "text/html; charset=utf-8",
      "cache-control": "public, max-age=3600",
    },
  });
});
