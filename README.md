<h1 align="center">🟣 Purple.</h1>

<p align="center"><strong>Your SSH config, managed.</strong></p>

<p align="center">
  <a href="https://crates.io/crates/purple-ssh"><img src="https://img.shields.io/crates/v/purple-ssh.svg" alt="Crates.io"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License: MIT"></a>
</p>

<p align="center">
  A fast, keyboard-driven TUI for managing SSH hosts.<br>
  Add, edit, delete and connect. All from your terminal.<br>
  Written in Rust. Single binary. No lock-in.
</p>

<p align="center"><img src="demo.gif" alt="Purple SSH config manager TUI demo" width="700"></p>

---

## 😩 The Problem

Your `~/.ssh/config` has 47 hosts and counting. You edit it by hand. One typo locks you out of production. You have no backups. You `grep` for hostnames like a caveman. Sound familiar?

## ✨ What Purple Does

Purple reads your SSH config, gives you a proper interface and writes it back. Byte-for-byte. Your comments, formatting and unknown directives survive every edit. No proprietary formats. No surprises.

### 🖥️ Manage Hosts

Add, edit and delete hosts through a form interface. Connect to any host by pressing Enter. Quick-add from the command line with `purple add user@host:port`. Paste `user@host:port` into the alias field and watch it auto-fill the rest. Smart paste. Because life's too short to fill out forms.

### 🔍 Find Things Fast

Search and filter by alias, hostname or user. Vim-style navigation with `j`/`k` and arrow keys. Group headers turn comments above Host blocks into visual sections. Your config stays organized without you trying.

### 📡 Know What's Alive

Ping hosts with a TCP reachability check before you connect. One host or all of them. No more SSHing into the void.

### 🔑 SSH Key Management

Browse your keys, see fingerprints and linked hosts. Pick a key with Ctrl+K right from the form. No more guessing which key goes where.

### 📂 Include Support

Reads Include directives recursively with glob expansion. Included hosts show up in the list but stay read-only. Your multi-file setup just works.

### 📋 Copy to Clipboard

Press `y` to copy the SSH command. Works on macOS, Wayland and X11.

---

## 🛡️ Built for the Paranoid

| | |
|---|---|
| **Round-trip fidelity** | Comments, formatting, unknown directives. All preserved. |
| **Atomic writes** | Temp file, chmod 600, rename. No half-written configs. |
| **Automatic backups** | Every write creates a backup. Keeps the last 5. |
| **Shell completions** | Bash, zsh and fish. |
| **Works everywhere** | ANSI 16 colors. Any terminal theme, any monospace font. No Nerd Font needed. |

---

## 📦 Install

**Homebrew (macOS)**
```bash
brew install erickochen/purple/purple
```

**Cargo**
```bash
cargo install purple-ssh
```

**From Source**
```bash
git clone https://github.com/erickochen/purple.git && cd purple && cargo build --release
```

---

## 🚀 Usage

```bash
purple                          # Launch the TUI
purple myserver                 # Connect or search
purple -c myserver              # Direct connect
purple --list                   # List all hosts
purple add deploy@10.0.1.5:22  # Quick-add a host
purple --completions zsh        # Generate shell completions
```

---

## ⌨️ Keybindings

### Host List

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate up and down |
| `Enter` | Connect to selected host |
| `a` | Add new host |
| `e` | Edit selected host |
| `d` | Delete selected host |
| `y` | Copy SSH command to clipboard |
| `/` | Search and filter |
| `p` | Ping selected host |
| `P` | Ping all hosts |
| `K` | SSH key list |
| `?` | Help |
| `q` / `Esc` | Quit |

### Search

| Key | Action |
|-----|--------|
| Type | Filter hosts |
| `Enter` | Connect to selected |
| `Esc` | Cancel search |
| `Tab` / `Shift+Tab` | Next / previous result |

### Form

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Next / previous field |
| `Ctrl+K` | Pick SSH key |
| `Enter` | Save |
| `Esc` | Cancel |

---

## 💜 Why "Purple"?

Every project needs a name. This one got picked because purple is the creator's favorite color. No acronym, no grand metaphor. Just vibes. 😉

## License

[MIT](LICENSE)
