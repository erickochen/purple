# Purple. SSH Config Manager for Your Terminal

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

> Stop editing `~/.ssh/config` by hand. Life's too short.

**Purple** is a fast, interactive **SSH config manager** and **SSH config editor** for your terminal. Built as a keyboard-driven **terminal UI (TUI)**, Purple lets you add, edit, delete and connect to SSH hosts without opening a text editor.

Written in Rust. Works with your existing `~/.ssh/config`. No proprietary formats, no lock-in, no nonsense. If you need a **terminal SSH manager** that respects your workflow, Purple is it.

<p align="center"><img src="demo.gif" alt="Purple SSH config manager TUI demo" width="700"></p>

## What is Purple?

Purple is an open-source SSH config manager that runs in your terminal. It provides a keyboard-driven TUI (terminal user interface) for managing the hosts in your `~/.ssh/config` file. Instead of manually editing your SSH config with a text editor, Purple gives you a form-based interface to add, edit and delete SSH hosts, then connects to them with a single keypress.

Purple is written in Rust, works with your existing SSH config file (no proprietary format) and creates automatic backups before every change.

## Features

- **Interactive terminal interface (TUI)**. Navigate with keyboard, no GUI required
- **Add, edit and delete SSH hosts** with a simple form-based interface
- **Connect to any host** with a single keypress
- **Preserves your existing SSH config**. Formatting, comments and unknown directives stay intact
- **Works with any terminal theme**. Light, dark, Solarized, Dracula, Nord, Gruvbox, you name it
- **Vim-style navigation** (`j`/`k`) plus arrow keys
- **Zero config**. Reads your existing `~/.ssh/config` out of the box
- **Automatic backups**. Creates timestamped backups before every config change
- **Atomic writes**. No half-written configs if something goes wrong
- **Tiny binary**. Fast startup, minimal disk usage
- **Written in Rust**. Because life's too short for segfaults too

## Who is Purple For?

- **DevOps engineers** managing dozens or hundreds of SSH hosts across environments
- **Developers** who SSH into dev servers, staging and production daily
- **Sysadmins** who want a safer alternative to hand-editing `~/.ssh/config`
- **Anyone** tired of scrolling through a 200-line SSH config to find the right hostname

## Installation

### Homebrew (macOS)

```bash
brew install erickochen/purple/purple
```

Or:

```bash
brew tap erickochen/purple
brew install purple
```

### From source

```bash
cargo install purple-ssh
```

### Build from Git

```bash
git clone https://github.com/erickochen/purple.git
cd purple
cargo build --release
# Binary is at target/release/purple
```

## Quick Start

```bash
# Connect to a host directly
purple -c myserver

# List all your SSH hosts
purple --list

# Launch the full interactive TUI
purple
```

## Keybindings

### Host List

| Key           | Action                  |
|---------------|-------------------------|
| `j` / `Down`  | Move selection down     |
| `k` / `Up`    | Move selection up       |
| `Enter`       | Connect to selected host|
| `a`           | Add new host            |
| `e`           | Edit selected host      |
| `d`           | Delete selected host    |
| `?`           | Show help               |
| `q` / `Esc`   | Quit                    |
| `Ctrl+C`      | Quit (from any screen)  |

### Host Form (Add/Edit)

| Key              | Action          |
|------------------|-----------------|
| `Tab` / `Down`   | Next field      |
| `Shift+Tab` / `Up` | Previous field |
| `Enter`          | Save            |
| `Esc`            | Cancel          |

## Why Use Purple Instead of a Text Editor?

- **One typo in `~/.ssh/config` can lock you out of every server.** Purple validates input before writing.
- **SSH config syntax is fiddly.** Indentation matters, directive names are case-insensitive but values are not and there's no built-in way to check for errors. Purple handles the formatting for you.
- **No backups by default.** If you fat-finger a `sed` command or a bad editor save, your old config is gone. Purple creates timestamped backups before every write.
- **Jumping between hosts is slow.** With Purple, you see all your hosts and connect with one keypress.

## How It Works

Purple reads and writes the standard OpenSSH config file at `~/.ssh/config`. It parses the file while preserving:

- Comments (`# like this`)
- Blank lines and formatting
- Unknown or advanced directives (e.g., `ForwardAgent`, `LocalForward`)
- Wildcard entries (`Host *`)

When you add, edit, or delete a host, Purple writes the changes back atomically (temp file + rename) and creates a timestamped backup first. Editing a host preserves all existing directives that Purple doesn't manage. Your `ForwardAgent`, `Compression` and other settings stay exactly where they are.

### Supported Host Directives

Purple's form supports these commonly used directives:

| Field         | SSH Directive  | Example                    |
|---------------|----------------|----------------------------|
| Alias         | `Host`         | `production`               |
| Hostname      | `HostName`     | `192.168.1.10`             |
| User          | `User`         | `deploy`                   |
| Port          | `Port`         | `2222`                     |
| Identity File | `IdentityFile` | `~/.ssh/id_ed25519`        |
| ProxyJump     | `ProxyJump`    | `bastion`                  |

All other directives in your config are preserved as-is.

## Alternatives & Comparison

Looking for an SSH config manager? Here's how Purple compares:

| Feature | Purple | sshs | storm |
|---|---|---|---|
| Terminal UI (TUI) | Yes | Yes | No (CLI only) |
| Edit SSH config in-place | Yes | No (read-only) | Yes |
| Preserves comments & formatting | Yes | N/A | No |
| Automatic backups | Yes | N/A | No |
| Atomic writes | Yes | N/A | No |
| Connect to hosts | Yes | Yes | No |
| Vim keybindings | Yes | Yes | No |
| Written in | Rust | Rust | Python |

Purple is designed for developers who want to **manage and edit** their SSH hosts from the terminal without sacrificing safety. If you just need a host picker, [sshs](https://github.com/quantumsheep/sshs) is a great choice. If you want full config management with backups and atomic writes, Purple is the tool for that.

## Terminal Compatibility

Purple uses only the 16 standard ANSI colors, which means it automatically adapts to your terminal's color scheme. It looks great on:

- **Dark themes**: iTerm2 default, Terminal.app dark, Alacritty dark
- **Light themes**: Solarized Light, Terminal.app default light
- **Popular schemes**: Dracula, Nord, Gruvbox, One Dark, Catppuccin
- **High contrast**: Accessible themes for visual impairment

No emoji or Nerd Font icons required. Purple uses standard Unicode box-drawing characters that work with any monospace font (SF Mono, JetBrains Mono, Menlo, Fira Code, etc.).

Minimum terminal size: 60 columns x 15 rows.

## FAQ

**Q: Will Purple mess up my SSH config?**
A: No. Purple creates a timestamped backup before every write and uses atomic file operations (write to temp file, then rename). Your original formatting, comments and unknown directives are preserved. Editing a host preserves all directives Purple doesn't know about (like `ForwardAgent`, `Compression`, etc.).

**Q: How is Purple different from sshs?**
A: sshs is a host picker. It reads your SSH config and lets you select a host to connect to. Purple is a full SSH config manager: you can add, edit and delete hosts, not just connect to them. Purple also creates backups and uses atomic writes.

**Q: Can Purple handle hundreds of SSH hosts?**
A: Yes. Purple's TUI renders a scrollable list and handles large configs without issues.

**Q: Does Purple support ProxyJump / bastion hosts?**
A: Yes. Purple's host form includes a ProxyJump field for jump host / bastion host configurations.

**Q: Is Purple safe to use on production SSH configs?**
A: Yes. Purple creates a timestamped backup before every write and uses atomic file operations. If anything goes wrong, your previous config is preserved.

**Q: Can I use Purple with SSH keys and IdentityFile?**
A: Yes. You can specify the path to your SSH key (e.g., `~/.ssh/id_ed25519`) when adding or editing a host.

**Q: Does it work with `Include` directives?**
A: Purple currently reads the main `~/.ssh/config` file. `Include` directives are preserved but not followed. Support for included files is planned.

**Q: Can I use a custom config path?**
A: Yes! Use `purple --config /path/to/config`.

**Q: Does Purple work with tmux / screen?**
A: Yes. Purple uses standard ANSI escape codes and works correctly inside tmux, screen and other terminal multiplexers.

**Q: Does it work on Linux?**
A: Purple is built for macOS but should work on Linux too. Homebrew installation is macOS-only; use `cargo install` on Linux.

## License

[MIT](LICENSE)
