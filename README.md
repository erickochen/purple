<h1 align="center">🟣 purple.</h1>

<p align="center"><strong>Your SSH config, supercharged.</strong></p>

<p align="center">
  <a href="https://crates.io/crates/purple-ssh"><img src="https://img.shields.io/crates/v/purple-ssh.svg" alt="Crates.io"></a>
  <a href="https://crates.io/crates/purple-ssh"><img src="https://img.shields.io/crates/d/purple-ssh.svg" alt="Downloads"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License: MIT"></a>
</p>

<p align="center">
  A keyboard-driven TUI for managing your SSH hosts.<br>
  Search, tag, sync from cloud providers and connect. All without leaving the terminal.<br>
  Reads and writes <code>~/.ssh/config</code> with round-trip fidelity.<br>
  Your comments, formatting and unknown directives stay exactly where they are.
</p>

<p align="center"><img src="demo.gif" alt="purple SSH launcher TUI demo" width="700"></p>

---

## Features

🚀 **TUI host launcher** — navigation, search, filter, connect with Enter

🔄 **Round-trip SSH config** — reads and writes ~/.ssh/config without losing comments, formatting or unknown directives

🏷️ **Tags** — label hosts with #tags, filter with tag picker or search

☁️ **Cloud provider sync** — pull servers from DigitalOcean, Vultr, Linode and Hetzner into your SSH config

📂 **Include support** — displays hosts from Include files (read-only)

📡 **Ping** — TCP connectivity check per host or all at once

🔍 **Search** — fuzzy filter on alias, hostname, user, tags and provider

📊 **Connection history** — frecency-based sorting (most used / most recent)

📥 **Bulk import** — from hosts file or ~/.ssh/known_hosts

🔑 **SSH key management** — key browser with metadata and host linking

📋 **Clipboard** — copy SSH command or config block to clipboard

♻️ **Auto-reload** — detects external config changes and reloads automatically

---

## Safe by default

🔒 **Atomic writes** — temp file, chmod 600, rename. No half-written configs.

💾 **Automatic backups** — every write creates a timestamped backup. Keeps the last 5.

🎨 **Monochrome UI** — works in any terminal, any font. Respects [NO_COLOR](https://no-color.org/).

🐚 **Shell completions** — bash, zsh and fish.

---

## Install

**Homebrew (macOS)**

```bash
brew install erickochen/purple/purple
```

**Cargo**

```bash
cargo install purple-ssh
```

**From source**

```bash
git clone https://github.com/erickochen/purple.git && cd purple && cargo build --release
```

---

## Usage

```bash
purple                              # Launch the TUI
purple myserver                     # Connect or search
purple -c myserver                  # Direct connect
purple --list                       # List all hosts
purple add deploy@10.0.1.5:22      # Quick-add a host
purple import hosts.txt             # Bulk import from file
purple import --known-hosts         # Import from known_hosts
purple provider add digitalocean    # Configure cloud provider
purple sync                         # Sync all providers
purple sync --dry-run               # Preview sync changes
purple --completions zsh            # Shell completions
```

---

<details>
<summary><strong>Keybindings</strong> — press <code>?</code> in the TUI for the cheat sheet</summary>

<br>

**Host List**

| Key         | Action                           |
| ----------- | -------------------------------- |
| `j` / `k`   | Navigate up and down             |
| `Enter`     | Connect to selected host         |
| `a`         | Add new host                     |
| `e`         | Edit selected host               |
| `d`         | Delete selected host             |
| `c`         | Clone host                       |
| `y`         | Copy SSH command                 |
| `x`         | Export config block to clipboard |
| `/`         | Search and filter                |
| `#`         | Filter by tag                    |
| `t`         | Tag host                         |
| `s`         | Cycle sort mode                  |
| `i`         | Inspect host details             |
| `u`         | Undo last delete                 |
| `p`         | Ping selected host               |
| `P`         | Ping all hosts                   |
| `S`         | Cloud provider sync              |
| `K`         | SSH key list                     |
| `?`         | Help                             |
| `q` / `Esc` | Quit                             |

**Provider List**

| Key         | Action                 |
| ----------- | ---------------------- |
| `Enter`     | Configure provider     |
| `s`         | Sync selected provider |
| `d`         | Remove provider        |
| `q` / `Esc` | Back                   |

**Search**

| Key                 | Action                 |
| ------------------- | ---------------------- |
| Type                | Filter hosts           |
| `Enter`             | Connect to selected    |
| `Esc`               | Cancel search          |
| `Tab` / `Shift+Tab` | Next / previous result |

**Form**

| Key                 | Action                |
| ------------------- | --------------------- |
| `Tab` / `Shift+Tab` | Next / previous field |
| `K`                 | Pick SSH key          |
| `Enter`             | Save                  |
| `Esc`               | Cancel                |

</details>

<br>

<p align="center">
  💜 <a href="LICENSE">MIT License</a>
</p>
