## 3.32.0 - 2026-10-28

- feat: Give purple its own SSH config file.
- feat: Pick it once in your preferences. No flag needed.
- change: Nothing changes until you pick one.
- change: Relative `Include` lines now read the same files as ssh.
- How to set it up: see the wiki or run `purple --help`.

## 3.31.1 - 2026-10-24

- feat: Containers show up on fish and tcsh hosts.
- feat: Listing and refresh work just like on bash and zsh.
- Thanks to @Parsifa1 for fixing this in #165

## 3.31.0 - 2026-10-21

- feat: Use your YubiKey with a PIN everywhere in purple.
- feat: A tunnel, a file browser listing or a key push that needs your security key PIN asks for it inside purple and carries on with it. Touch the key when it blinks. This needs OpenSSH 8.5 or newer.
- feat: purple keeps the PIN in memory per key until you quit and never writes it to disk. Once a connection proves it right, later tunnels, listings, transfers, container commands and snippets reuse it.
- feat: A wrong PIN costs your key one attempt. purple stops using a PIN the key refused, tells you why and asks again. Hosts that share a key take turns, so one wrong PIN never spends an attempt on every host at once.
- feat: Connecting from the host list asks for your security key PIN in the terminal, even when the host has a password source. Your password only ever answers a password prompt.
- change: Provider sync and the update check use current TLS root certificates and the latest certificate checks.

## 3.30.0 - 2026-09-22

- feat: Snippets can run with your full shell setup.
- feat: Turn on Interactive shell in a snippet and it runs with your `~/.bashrc` loaded, so tools like `pm2` installed through nvm work the same as when you log in. On the command line, `purple snippet add` takes `--interactive`.
- feat: A snippet that ends with "command not found" shows a tip that points you to the Interactive shell toggle.

## 3.29.2 - 2026-09-22

- change: purple opens instantly, however many keys you keep.
- change: The Keys tab, the key picker in the host form and every key rescan read your keys directly, so a `~/.ssh` with dozens of keys opens as fast as one with a single key.
- change: The fingerprint art on the Keys tab matches `ssh-keygen -lv` in every cell, the busiest ones included.

## 3.29.1 - 2026-09-22

- change: Your password stays on the host you typed it for.
- change: A jump host never receives the password or the vault entry meant for the host behind it. This applies to `ProxyCommand` as well as `ProxyJump`, including one inherited from a `Host *` or `Match` block. `ProxyCommand none` means direct.
- change: Background work (key push, file browser, transfers, container commands, tunnels) routes every hop's password prompt through purple. Nothing draws a prompt over the TUI. A jump host that refuses shows up in the connection error by name.
- change: A password question waits while you type in the search, the tag field or the jump bar. It also waits for an open confirm dialog.
- change: A `ProxyCommand` on a `Host *` or `Match` block now counts for the hosts that inherit it, so those read as proxied in the ping sweep instead of being probed directly. Teleport writes its hosts that way.
- change: A server behind a jump host that asks for a password without identifying itself gets an explanation instead of a prompt, since purple cannot tell it apart from the jump host asking.
- fix: `Esc` during a key push and quitting purple return immediately, even on a slow connection.

## 3.29.0 - 2026-09-21

- feat: Push keys and browse files on password-only hosts.
- feat: A host that wants a password asks you for it inside purple. Pick the OS keychain and every later connection finds it by itself, or pick this session only and it stays in memory until you quit. The push or the listing continues on your answer.
- feat: A host you have not reached before asks to be trusted first. Press `y` and purple records its key and carries on. A host key that changed still gets the security warning instead.
- feat: Select a dozen hosts and answer one question each, in the order you picked them. A question waits its turn while you are filling in a form and opens when you come back, so nothing takes the screen from what you are typing. A password you keep for the session carries into every later listing, transfer and container command on that host, so the file explorer asks once per host rather than once per directory.
- feat: Reach a host through a jump host and purple tells the two apart. It names whichever one refused or is unknown, so a password or a host key never lands on the wrong machine.
- change: Every background connection reports a refusal straight away and names the host and the reason. Key pushes, remote listings, transfers, container commands and tunnel starts all come back with something you can act on.
- change: `Esc` during a push stops the connection that is running, and quitting purple stays instant even when a host is slow to answer.
- change: A password purple hands to `ssh` goes out once, so a rejected one costs a single failed login on the server.

## 3.28.0 - 2026-09-17

- feat: Every AWS account, every instance, one host list.
- feat: Reach EC2 instances that have no public address. Set Session Manager to `auto` on your AWS provider and purple asks AWS which instances are reachable, then writes the proxy command for each one. `ssh`, `scp` and the file browser work as they always did, over a tunnel the instance opens outbound, with no inbound port and no bastion. Instances AWS does not report keep their IP address, so nothing you already use changes.
- feat: Run several AWS accounts side by side. Give each one a label and a profile, and purple keeps them in their own lanes with their own alias prefixes. A profile with `role_arn` and `source_profile` is assumed for you, so one key pair reaches every account under your Organization.
- feat: purple now reads `~/.aws/config`, not just `~/.aws/credentials`. Press Space on the Profile field to pick from the profiles you actually have, with the ones that reach another account marked and the ones purple cannot use saying why before you pick them. Picking one fills in that profile's region too.
- change: Adding a provider that already has a config updates it in place and names the route that keeps a second account beside it.
- fix: A second labeled config for the same provider saves on the first try, with its own alias prefix.

## 3.27.0 - 2026-08-26

- feat: Your NetBox inventory becomes your SSH host list.
- feat: Add NetBox with `S` in the TUI: fill URL and token and your devices and virtual machines sync into `~/.ssh/config`. Primary IPs become HostNames, tag slugs become searchable tags and site, role, platform and status show up in the detail panel.
- feat: Scope the sync with NetBox's own query language in the Filter field: `status=active` is the default and `tag=ssh&site=ams1` narrows it to exactly the machines you reach over SSH.
- feat: Both token forms work without configuration: new `nbt_` tokens and classic API tokens are detected automatically. Self-signed TLS is a toggle away.

## 3.26.1 - 2026-08-24

- fix: Every ssh session starts on a clean screen.
- fix: Connecting, opening a container shell and running a snippet clear the old frame before handing over your terminal, so remote output lands on a blank screen even when your terminal keeps the alternate screen disabled.

## 3.26.0 - 2026-08-16

- feat: Keep $HOME clean and your dotfiles in sync.
- feat: Set `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME` or `XDG_CACHE_HOME` (or the `PURPLE_CONFIG_DIR`, `PURPLE_DATA_DIR`, `PURPLE_STATE_DIR` and `PURPLE_CACHE_DIR` overrides) and purple splits `~/.purple` by category. Preferences, snippets, providers and themes go to config, ready for your dotfiles repo. Signed certificates and the original SSH config backup go to data, history, recents and logs to state, the container cache and version check to cache. On the first run purple copies what it has into place and leaves `~/.purple` alone, so nothing moves until you ask.
- fix: The Teleport provider form opens on its User field and a canceled `tsh` sync takes everything `tsh` started down with it.
- fix: A provider whose credentials come from a profile or the environment saves without a token, from the form and from the CLI.

## 3.25.0 - 2026-08-16

- feat: Teleport nodes and kitty's ssh kitten join your host list.
- feat: Sync your Teleport cluster with `purple provider add teleport`. Every node from `tsh ls` lands in `~/.ssh/config` with the ProxyCommand and key paths that `tsh config` prescribes, so Enter connects through the proxy, node labels become tags you can extend and the cluster and address show up in the detail panel.
- feat: Pick the program that runs your logins. Set `ssh_command=kitten ssh` in `~/.purple/preferences` or export `PURPLE_SSH_COMMAND` and purple uses it for interactive connects, container shells and the copied ssh command, while tunnels, snippets and transfers stay on plain ssh.
- feat: Hosts behind a ProxyCommand show as proxied instead of offline, so a Teleport node or an `ssh -W` helper never reads as a red cross.

## 3.24.0 - 2026-08-13

- feat: AWS syncs with the credentials your company hands out.
- feat: Sync your EC2 hosts on temporary credentials from IAM Identity Center, assume-role or get-session-token. purple picks the session token up from your profile or from the token you paste in, then signs it into every request, so a fleet on short-lived keys stays as easy to sync as one on a permanent key pair.

## 3.23.0 - 2026-07-21

- feat: Change a provider's username once, every host follows.
- feat: Update the default username or SSH key on a cloud provider and purple carries it to every host from that provider on the next sync, so you never hand-edit them one by one.
- feat: Your per-host tweaks stay yours. If you set a different username or key on a specific host, changing the provider default leaves that host untouched.

## 3.22.1 - 2026-06-16

- change: Calmer detail panels that read at a glance.
- change: Section headers in the host, container, key and snippet detail panels now sit quietly in the card frame instead of competing with the values, so the details you came for stand out.

## 3.22.0 - 2026-06-06

- feat: Snippets get a tab to run, vet and track on hosts.
- feat: Browse and search every snippet, add, edit and delete in place, then run the one you want without leaving the keyboard.
- feat: Pick a snippet, select any set of hosts from a grouped picker and confirm. Purple runs the command on every host you chose, with output collected in one panel or streamed live in your terminal.
- feat: See a command's blast radius before you run it. Each snippet shows whether it is read-only and safe to fan out, writes state, needs care or is irreversible, with plain callouts like runs as root or fetches and runs remote code.
- feat: Know a snippet's track record at a glance. Purple records every run and shows a reliability percentage, a trend chart and the last outcome, so you can judge a command before trusting it across a fleet.
- feat: Save default hosts on a snippet so your routine runs come pre-selected. Open, confirm and go.

## 3.21.1 - 2026-05-31

- feat: Your cloud hosts stay grouped under their provider.
- feat: Every cloud provider keeps one clean group header in ~/.ssh/config and the purple host list, so your hosts read the same in the file and on screen.
- feat: Newly synced and imported hosts always land inside their provider's group, so your config stays organized as it grows.
- feat: Grouping holds whether your hosts live in your main config or an Included file.

## 3.21.0 - 2026-05-31

- feat: Tidy SSH config with a blank line between hosts.
- feat: Synced and imported hosts now line up cleanly, and your existing config tidies itself the next time purple saves.

## 3.20.1 - 2026-05-29

- change: Clearer logs make every issue faster to track down.
- change: Every logged event in your purple.log now carries a fault-domain tag ([external], [config] or [purple]), so you can grep straight to whether a problem comes from a remote tool, your SSH config or purple itself.
- change: Routine non-zero exit codes from remote commands no longer show up as errors. The error level is reserved for genuine SSH connection failures, so a real problem stands out instead of drowning in noise.
- change: Host deletes, provider syncs, undo and container actions now record what changed, so a support log tells the full story.

## 3.20.0 - 2026-05-28

- feat: Pattern SSH settings, grouped into clear cards.
- feat: Select a wildcard pattern and its directives now split into per-purpose cards. Connection, Authentication, Forwarding, Crypto, Keepalive and more, each shown only when the pattern sets it, so you read a pattern the way you read a host.
- feat: The host list wraps as you move. Scroll past the bottom and you land at the top, past the top and you land at the bottom, matching the Tunnels, Containers and Keys tabs. Page Up and Page Down wrap at the edges too.

## 3.19.0 - 2026-05-28

- feat: Type any action by name, on every tab.
- feat: Picking a tunnel from `:` starts or stops it in one Enter, the same one-shot rhythm a host already had.
- feat: Picking a container from `:` opens its shell in one Enter, no extra keypress on the overview.
- feat: Sort, group, view toggle, tag filter and multi-select are now reachable by name from `:` on the Hosts tab.
- feat: Exec, logs, restart, stop, refresh-host, fold-host and compose-stack restart are reachable by name from `:` on the Containers tab.
- feat: Modifier-bound shortcuts like Ctrl-a select-all and Ctrl-k stack-restart show up in `:` too, so they are findable instead of memorised.
- feat: `?` Help is one search away from anywhere.

## 3.18.7 - 2026-05-27

- change: Snappier overlays. Vault cert checks throttled.
- change: Opening or closing the container-logs, snippet and bulk-confirm overlays no longer copies the underlying payload, so transitions stay flat regardless of how many lines or rows the overlay carries.
- change: Vault cert freshness check throttled to once every 250ms while you sit on the selected host. External signs (CLI, another purple instance) still surface within a blink and the per-frame syscall load drops several-fold.
- change: Cloud provider sync internals share a Bearer auth helper and a typed metadata builder across all 16 providers, so a typo in one cannot silently drift from the rest.

## 3.18.6 - 2026-05-27

- fix: Cleaner, balanced margins on every overlay.
- fix: What's New, the Transfer dialog, command output, help and the Vault SSH strip now reserve the same breathing room on both sides of the border, so prose and tables read as a tidy frame instead of skimming the right edge.
- change: The design system check that protects this margin now covers split-render overlays too, so the polish carries over to every new screen as it lands.

## 3.18.5 - 2026-05-27

- feat: Special characters in your SSH config save reliably.
- feat: Paths, hostnames and proxy jumps that contain spaces, "#" or quotes are written and read back exactly, whether you edit them yourself or provider sync writes them.
- change: Provider sync is now verified end to end for every supported cloud, so a broken request, header or page cursor is caught before it ever reaches your host list.

## 3.18.4 - 2026-05-27

- change: Safer foundations under every sync, sign and connect.
- change: purple now captures your environment and file locations once at startup and threads them through every operation, instead of reaching for them ad hoc across the app. Provider sync, Vault signing, askpass and file transfer all read the same consistent source.
- change: Every keystroke and screen works exactly as before. This release is internal hardening that makes purple safer to build on, with no change to your daily flow.

## 3.18.3 - 2026-05-26

- change: Every screen now runs on purple's safer core.
- change: The bulk tag editor and the Tunnels overview now work only on their own slice of purple's state, finishing the per-screen isolation from 3.18.2. Every key and screen behaves exactly as before.

## 3.18.2 - 2026-05-26

- change: A tighter, safer core behind every keystroke.
- change: Your edits now stay contained to the screen you're on. Each screen works only on its own part of purple's state, so a whole class of state bugs is caught before it ships.
- change: The "discard unsaved changes?" prompt on host, provider, snippet and tunnel forms is now backed by one shared check per form, so it can't drift out of sync and silently drop your edits as new fields get added.
- change: Every key and screen works exactly as you expect, so your daily flow is untouched while purple's internals get safer to build on.

## 3.18.1 - 2026-05-24

- change: Sturdier internals across host rename and removal.
- change: Each kind of per-host state (containers, ping, vault, tunnels, last-visited paths) now owns its own cleanup when a host disappears or gets renamed, so future features plug into the same contract instead of being touched from a central place.
- change: New tests pin down both halves of the cleanup, including the inspect and logs caches, so a future refactor cannot drop part of the orphan sweep silently.

## 3.18.0 - 2026-05-24

- feat: Pick files with plain Space and a in the file explorer.
- feat: Plain Space toggles the file under the cursor and plain a toggles every visible entry, no modifier keys needed.
- feat: The file explorer works cleanly inside tmux and on macOS keyboard-switching setups where Ctrl chords get intercepted before reaching purple.
- change: Ctrl+Space and Ctrl+A still toggle the same selection for users with the old muscle memory.
- Thanks to @muhlymak for raising this in #82

## 3.17.1 - 2026-05-24

- change: Safer, more reliable releases.
- change: Reworked how purple locates its settings and key files internally so the test suite runs fully isolated and reliably catches regressions before a release reaches you. Nothing changes in how purple looks or behaves.

## 3.17.0 - 2026-05-24

- feat: Vault SSH certs stay fresh for files and containers.
- feat: Opening the file browser, transferring files or working with containers on a host that uses the Vault SSH secrets engine now signs a fresh short-lived certificate first, just like connecting already did.
- feat: This covers the file browser, file transfers, the per-host container view and the Containers tab, including listing, logs, inspect, start, stop, restart and exec.
- change: It all happens automatically in the background, so an expired certificate no longer blocks you mid-task.

## 3.16.0 - 2026-05-24

- feat: Search your hosts by several words at once.
- feat: Type words separated by spaces and the list keeps only hosts that match all of them, in any field and any order. "web prod" finds your production web hosts even when the two words live in different fields.
- feat: The tag: filter works the same way, so tag:web prod narrows to hosts carrying both tags.
- Thanks to @9d4 for suggesting this in #79

## 3.15.20 - 2026-05-23

- change: Single-source versioning across the site, docs and binary.
- change: The version lives only in Cargo.toml and is injected into the landing page and llms.txt at deploy, so the site, docs and binary can never show different versions.
- change: The landing page, installer and LLM doc build from their source files at deploy, with no hand-maintained embedded copy to drift.
- change: Version bumps no longer disturb the visual test goldens, and theme-mutating tests are serialized against the snapshot suite for steadier parallel runs.

## 3.15.19 - 2026-05-23

- change: Sturdier internals across containers and key handling.
- change: Restart or stop a single container or a whole host's containers at once, with the same consistent, reliable behaviour.
- change: The Containers overview manages its caches and view state through named operations, so future changes to that view stay consistent.
- change: Host-list and file-browser key handling split into focused operations, so changes land in one place and stay predictable.
- change: Release checks pin the provider count to the code and derive their own test counts, so docs and coverage cannot quietly fall out of step.

## 3.15.18 - 2026-05-23

- feat: Provider sync keeps your hosts when a cloud API hiccups.
- feat: A provider that fails partway through a sync still hands back every server it reached.
- fix: Your existing hosts stay put on a partial sync, with removals and stale marking held back until a clean, complete fetch.
- change: The same resilience covers DigitalOcean, Hetzner, Linode, Vultr, TransIP, Leaseweb and i3D.net.

## 3.15.17 - 2026-05-23

- feat: The command palette runs exactly the row you highlight.
- fix: Searches that mix hosts and actions now highlight and run the same row.
- Thanks to @AndreyOkh for reporting this (#76).

## 3.15.16 - 2026-05-23

- change: Sturdier internals behind the Containers tab.
- change: The Containers overview screen is split into focused modules, so future changes to that view stay consistent and land in one place.

## 3.15.15 - 2026-05-23

- change: Sturdier internals across hosts, keys, forms and UI.
- change: Host list sorting, grouping, multi-select and undo route through single operations, so your filters and selections stay consistent mid-action.
- change: Provider sync progress and diff totals update through one path, so the counter never jumps out of step.
- change: Form edit, discard and delete prompts share guarded operations, so unsaved-change warnings and rollbacks behave the same on every screen.
- change: Keys tab, pickers and overlays manage their cursors and scroll state through named operations, keeping navigation predictable.

## 3.15.14 - 2026-05-23

- change: Steadier internals across host, tunnel and snippet state.
- change: Tag input, search and jump bar overlays manage state through named operations, so future tweaks to those flows land in one place.
- change: Ping refreshes, container fetches and tunnel start/stop share one entry per domain, keeping behaviour uniform across the Hosts, Tunnels and Containers tabs.
- change: Snippet store edits route through a single update path, so a save failure and its in-memory rollback stay in sync.

## 3.15.13 - 2026-05-22

- change: Tighter App state seams across more sub-systems.
- change: Update notice, file browser paths, connection history and Vault SSH state move behind named operations so write paths are inspectable from one spot.
- change: Vault SSH cert cache, in-flight checks and bulk-sign state share one set of operations, so they cannot drift apart mid-action.

## 3.15.12 - 2026-05-22

- change: Sturdier overlay, vault and status internals.
- change: Most App state now sits behind explicit module boundaries; future tweaks to overlays, vault and status land more predictably.
- change: Each overlay (file browser, jump bar, tag input) has a single open/close entry point in the codebase, making state lifecycle inspectable from one spot.
- change: Vault SSH cert status checks land their result and clear the in-flight reservation atomically.
- change: Vault SSH bulk sign teardown is split between the active-cancel path and the post-done cleanup, keeping log signatures distinct per phase.

## 3.15.11 - 2026-05-22

- change: Container request queues and host renames get sturdier.
- change: A logs or exec request that displaces a previous unstarted one is now logged, so dropped intents stay traceable.
- change: Host renames guard against accidental name collisions on container caches, refresh batches and folded-group state.
- change: Containers tab preferences hydrate through a single startup path that cannot silently revert in-session edits.

## 3.15.10 - 2026-05-22

- change: Sturdier internals, with no UI changes.
- change: Every module now lives in a shared library crate, so the bootstrap, CLI dispatch and direct-connect flows are reachable from tests for the first time.
- change: Startup banner proxy detection is locked down by a unit test, so support bundles keep showing reliable environment context.
- change: Internal seams that used to hide behind a thousand-line entry file now sit behind explicit module boundaries, making future tweaks safer to land.

## 3.15.9 - 2026-05-21

- feat: Sturdier delete, toggle and picker internals across the app.
- change: Cancel out of any delete confirmation, on hosts, tunnels, snippets or providers, and the state lands cleanly every time.
- change: Multi-select, view-mode and group-by toggles stay consistent so your filters and selections never drift mid-action.
- change: Key push and provider label flows recover from cancel and failure without leaking state into the next attempt.
- change: Snippet search and theme picker close share the same teardown so a stray Esc no longer leaves stale state behind.

## 3.15.8 - 2026-05-20

- change: Tighter form internals, sturdier picker handoffs.
- change: Custom command and prefix selections in the password picker refocus the password field for your follow-up keystrokes.
- change: Smart paste of `user@host:port` into the host form collapses four field writes into one cohesive update, keeping every value you have already typed intact.
- change: Discard prompts across host, tunnel, snippet, provider and bulk-tag forms route through one shared lifecycle, so future tweaks land in one place.

## 3.15.7 - 2026-05-20

- change: Tighter confirm and routing internals, with no UI changes.
- change: Container actions and SCP file copies confirm through the same y/n/Esc router as every other prompt in purple, so a stray key during a destructive confirm is reliably ignored.
- change: Background event handling, from ticks and pings through snippet runs and Vault signing, lives in focused domain modules. New event types land in a clear home.
- change: User-facing copy lives next to the feature it serves, so wording fixes touch one short file instead of scrolling through everything.

## 3.15.6 - 2026-05-20

- change: Sturdier picker and confirm internals, with no UI changes.
- change: Every overlay picker (password, key, ProxyJump, Vault role, region) opens and closes through one shared routine, so behaviour stays uniform when handler logic evolves.
- change: The snippet discard prompt follows the same y/n/Esc contract as every other confirm in the app, dropping the last raw key-match in the handler tree.
- change: Stale-host hint rendering routes through one helper. Future tweaks to the "Gone from <provider>" wording land in one place instead of twelve.

## 3.15.5 - 2026-05-20

- change: Sturdier form open and close across all edit screens.
- change: Host, tunnel, snippet and provider forms share a single open and close routine, so a fix to one path benefits all four.
- change: Vault config writes after a save are reliably persisted before the form closes.
- change: Per-provider sync defaults (DigitalOcean on, Proxmox off) are pinned by tests so a future refactor cannot silently flip them.

## 3.15.4 - 2026-05-19

- change: Sturdier event handling, with no UI changes.
- change: Adding a new background-job event now fails to compile until you declare whether it survives an SSH launch. No silent fall-through to a dropped result.
- change: Sync, ping, Vault and container results follow the same replay rule, expressed once on the event type itself instead of a hand-maintained whitelist.

## 3.15.3 - 2026-05-19

- feat: Activity chart shows from your first connect.

The host detail panel sparkline used to wait for three recorded connections before appearing. Now a single recorded SSH is enough to start drawing your activity timeline.

## 3.15.2 - 2026-05-19

- feat: Rename or delete a host without leaving stale views.
- feat: File Browser opens at your last folder on a host, even after a rename.
- feat: Container-overview group folds survive renames and deletions.
- fix: Container refresh batches keep accurate counts when you remove a host mid-run.
- fix: Vault bulk sign tracks aliases cleanly through a rename.

## 3.15.1 - 2026-05-19

- fix: Multiple cloud accounts on one provider work end to end.
- fix: Cloud sync now finds the right provider when an account has a name suffix like `digitalocean:work` or `proxmox:server2`. Its hosts land in your SSH config instead of triggering an "Unknown provider" toast.
- fix: Adding a third account on a provider opens the form with a Name field at the top. Type a short label using lowercase letters, digits and hyphens (max 32 characters). Empty or already-used names are caught while you type.
- fix: Removing one named account leaves the rest in place. Press `a` afterwards to add any account back, including with the same name as the one you removed.
- Reported by @AndreyOkh (#51).

## 3.15.0 - 2026-05-19

- feat: Provider tags follow your hosts in every view.
- feat: Proxmox PVE tags and other provider-synced tags stay visible whether you group by provider, by tag or not at all. The group header no longer eats them.
- feat: Confirm dialogs hold their shape at every prose length, with the blank row above the bottom border preserved even when long sentences wrap.
- feat: Wrapped lines in dialogs share the leading indent of the line above, so multi-line explanations read as one paragraph instead of jumping flush-left.
- change: One design system helper now backs every confirm popup, so future dialogs inherit the same rhythm without per-screen tuning.

## 3.14.5 - 2026-05-18

- feat: Vault SSH signs through HAProxy and other HTTPS proxies.
- fix: Typing `https://vault.corp.example.com` reaches your server on the standard HTTPS port without a stray `:443` ending up in the `Host` header, so strict ACLs on HAProxy, nginx and other HTTP/1.1 proxies route the signing request to the right backend.

## 3.14.4 - 2026-05-18

- feat: Safer logs and sturdier internals, with no UI changes.
- feat: Your provider API tokens and Vault server hostnames are redacted in every debug output, log line, panic message and test failure. You can share a log without scrubbing it first.
- feat: Every screen transition lands in the debug log. When a bug needs support, the screen path is traceable instead of guessed.
- feat: Container paused and restarting states are pinned by regression goldens, alongside five new goldens for Vault TTL boundaries, proxy chains and other detail-panel edges. The visual contract stops drifting between releases.
- change: Adding a new cloud provider now fails to compile until every required form-field and capability site is wired. No silent fall-through to generic defaults.
- change: Internal architecture decoupled across the App struct, key dispatch, the detail panel and provider identity checks. Future feature work lands faster and with fewer regressions.

## 3.14.3 - 2026-05-17

- change: Tighter design system across the binary and the docs.
- feat: Every container reads the same on every screen, so a paused or dead one never reads as a live one again.
- feat: The brand-purple cursor inside every form is visible on the default theme again.
- feat: Snippet output and Keys tab help list every shortcut your fingers already know, including Ctrl+C and Enter/c.
- feat: Custom theme TOML files surface a clear warning per malformed hex value instead of silently falling back to the default.
- feat: Dialogs, help screens and tunnel detail rows now keep a clean margin against the right border, with wrap or shorter labels chosen per surface so nothing clips and nothing touches the edge.
- feat: The Hosts, Containers, Tunnels and Keys tabs greet a fresh install with one centred card naming what is missing and the one key that fills it, instead of repeating "nothing here" twice at different heights.
- feat: Tunnel, snippet and provider deletes now ask for confirmation in the same centred popup style as host delete and Vault SSH sign, instead of a tiny footer prompt under the parent overlay.
- fix: Pressing `a` on Containers or `p` on Keys without any hosts configured now surfaces a "No hosts yet. Add a host first." toast instead of opening an empty picker.
- change: Stable healthy attributes (encrypted, agent loaded, TTL good) now read as their own visual tier, separate from live-now indicators.
- change: Pre-commit gates catch a wider class of design slip-ups before they ship, so future releases land tighter by default.

## 3.14.2 - 2026-05-17

- feat: More ssh_config edge cases covered by round-trip safety.
- feat: Duplicate `HostName` / `User` / `Port` / `ProxyJump` lines resolve first-match-wins, exactly the way `ssh -G` does.
- feat: `Match`, `Include` and `Host` keywords start a new section even when indented, with `=` as a separator everywhere OpenSSH accepts it.
- feat: Deleting a host preserves every section header above the next block and leaves no silent re-route through a token-only sibling.
- feat: Tags, askpass, vault roles, provider metadata and stale flags refuse shared `Host A B` lines, so a sibling alias never inherits a credential or ownership it was not granted.
- feat: Background sync and deferred vault writes step aside when the config changed under them on disk, so edits made in another editor survive.
- feat: Backups land next to the canonical file even through a symlinked `~/.ssh/config`, with mode preserved on every save.

## 3.14.1 - 2026-05-17

- feat: Tighter safety on every ~/.ssh/config write.
- feat: Multi-key hosts keep every IdentityFile line when you edit one of them.
- feat: Vault SSH signing leaves your user-set CertificateFile lines exactly where they are.
- feat: Vault roles stay on the single host you targeted, even on shared `Host A B` lines.
- feat: Provider sync can never smuggle extra directives into your config through a crafted server name.
- feat: Self-including or mutually-including SSH config files terminate cleanly instead of hanging the parser.
- feat: Your most recent save survives a sudden power loss thanks to a durable rename on the underlying write.

## 3.14.0 - 2026-05-16

- feat: Browse, push and inspect every SSH key from one tab.
- feat: A new Keys tab joins Hosts, Tunnels and Containers in the Tab cycle. Every key in `~/.ssh/` lands there with strength score, encryption status, randomart fingerprint and last-touch.
- feat: Press `p` on a key to push it to a multi-host picker. Vault-managed hosts skip the prompt so cert-managed hosts stay cert-managed.
- feat: Per-key activity sparkline auto-scales the time window as your usage history grows, from a tight 5 days out to a full year.
- feat: A Vault SSH strip at the top of the tab shows time-to-expiry for every active signed cert, colour-coded green to red as the clock runs out.
- feat: Search keys with `/`, copy the highlighted public key with `Enter` or `c`, sign Vault SSH certs in bulk with `V`.

## 3.13.1 - 2026-05-15

- feat: Long hostnames render in full on every terminal.
- feat: The NAME column scales to your longest alias instead of stopping at 32 characters.
- feat: Whether your aliases are short labels or 50-character FQDNs, they appear unabbreviated when there is room.
- feat: On narrow terminals the column shrinks last, after LAST and TAGS step aside, so meaningful detail stays in view.
- Reported by @matthew-pakulski (#67).

## 3.13.0 - 2026-05-14

- feat: Pull SSH passwords from your Proton Pass vault.
- feat: Pick Proton Pass on any host and purple resolves the secret from your vault on every connect.
- feat: One pass-cli login carries you across reconnects. A quick re-auth prompt catches expired sessions.
- feat: Your Personal Access Token rides an env var into the login call, never argv or any later subprocess.
- Proton Pass support suggested by @W4ff1e (#66).

## 3.12.3 - 2026-05-12

- feat: Twice the release history and calmer detail panels.
- feat: Browse the last ten releases at a glance with n, double what fit before.
- feat: Container detail cards keep clean breathing room on every side, so CMD and host-header text never collide with a border.
- change: Space folds container groups and Enter opens containers, no overlap.

## 3.12.2 - 2026-05-12

- feat: Renames keep your status, containers and tunnels in step.
- feat: A host edit carries its ping status, last-checked timer and cached container list straight over to the new alias, so the row never flashes to "unknown" mid-edit.
- feat: Active SSH tunnels and in-flight container or Vault checks all follow the rename, so a relabel never strands work that was already in motion.
- feat: `purple sync` migrates the same per-host caches when a cloud provider rewrites an alias, so the list stays warm across an automatic rename.
- feat: Stale Vault SSH cert files clean up after themselves on rename, so an alias change never leaves a forgotten certificate behind on disk.

## 3.12.1 - 2026-05-12

- feat: Renamed hosts hold their place on Most recent.
- feat: Most recent and Frecency reorder the moment you save, so the host you just renamed stays in its rightful slot without a restart.
- feat: Provider sync renames update the list ordering just as fast, so a cloud relabel never parks a busy host at the bottom.

## 3.12.0 - 2026-05-12

- feat: Renames carry history, recents and your fleet view.
- feat: Edit a host's alias and your connection history follows it, so your top hosts stay top after a typo fix or a provider relabel.
- feat: Jump-bar recents move with the rename, so the universal `:` bar still surfaces the host you actually use.
- feat: Collapsed fleets in the containers tab survive a rename, so big fleets stay tidy across edits.
- feat: `purple sync` migrates the same per-host state when a cloud provider rewrites an alias, so a relabel never orphans your context.
- feat: Container logs (`l`) open in demo mode, so `--demo` shows off the search and filter UI without a remote shell.
- change: SSH config rename keeps `# purple:*` metadata, unknown directives, CRLF endings and the UTF-8 BOM byte-for-byte. New round-trip tests lock it in.

## 3.11.0 - 2026-05-11

- feat: Live-search container logs without leaving purple.
- feat: Press `/` in the logs viewer to filter as you type. Tab and Shift+Tab step through matches, `←`/`→`/`Home`/`End`/`Backspace`/`Delete` edit the query, `Esc` returns to the logs.
- feat: Smart-case search matches like you would expect. Lowercase finds anything, an uppercase character locks the search to exact case.
- feat: Demo mode now ships realistic container logs, so `--demo` exercises the new search without a remote shell.
- change: Container detail cards line up consistently. ACTIONS verbs share the same value column as FLEET state, HOST address and STATUS rows, so the cards read as one grid.
- change: UPTIME stays anchored to the right edge of the container list, even when the detail panel is folded. IMAGE flexes to fill the space.
- change: Mounts table in the container detail card has breathing room. The `rw`/`ro` tag no longer presses against the right border.

## 3.10.1 - 2026-05-11

- feat: Cleaner Podman fleets, snappier containers, safer exec.
- fix: Podman 3.x and 5.x JSON parses across the board. Array `Names`, array `Ports`, the docker-shim runtime relabel and Podman 3.x's `OomKilled` field all land in the same columns Docker already uses.
- fix: A non-zero exit lights up the row and the ATTENTION card on Podman too. The state glyph turns warning when inspect reports the code, even with an empty Status string.
- feat: SSH command output caps at 16 MB. A runaway remote can no longer balloon purple's memory or freeze a refresh while a snippet streams.
- feat: The containers tab paints sooner on big fleets. The row build is memoized, so sort, search and collapse stay smooth past a hundred hosts.
- change: Container exec validates every container ID before it spawns ssh. Future entry points (MCP, jump-bar paste) cannot smuggle a shell-meta character through.
- fix: A warning toast no longer waits behind a held success message. The warning takes the slot the moment it lands.

## 3.10.0 - 2026-05-10

- feat: Smoother prompts, sharper logs, easier troubleshooting.
- feat: Container confirms use action verbs both sides ("restart"/"keep", "stop"/"keep") so the choice is unambiguous before you press y or n.
- feat: Container restart and stop land as a green success toast, just like every other user-initiated action.
- feat: Demo-mode hints render as yellow warnings, so a blocked action looks blocked.
- feat: `purple --verbose` records every state change. Preferences saves, CLI flow, SSH config writes, form submits, filter and sort, vault thread and config reload all show up in `~/.purple/purple.log`.
- feat: A complete Troubleshooting wiki page documents every fault domain, every log string and every recovery step.
- change: Every prompt now reads in purple's voice, including form validation and stale-host warnings.
- change: Pre-commit gates catch footer-keycap drift and generic confirm verbs ("yes"/"no"/"cancel") before they ship.

## 3.9.0 - 2026-05-10

- feat: Manage every container in your fleet from one tab.
- feat: Press Tab past Hosts and Tunnels to land in Containers. Every Docker and Podman container groups under each host. Park the cursor on a host divider and the detail panel summarises engine version, runtime, sync age and running/exited counts.
- feat: Enter shells in. `l` streams the last 200 log lines. `K` restarts. `S` stops. `e` runs a one-off command. Confirms always show before anything destructive.
- feat: `Ctrl-K` cycles a whole compose stack member by member. `K` or `S` on a host divider sweeps every running container on that host in order.
- feat: `r` refreshes one host. `R` refreshes all in parallel, four at a time. `a` adds a host to the cache from a picker.
- feat: The detail panel reads `docker inspect` once and remembers, so image digest, restart policy, user, privileged flag, mounts, network and recent logs render instantly as you arrow through.
- feat: Sync brings freshly added hosts into the cache automatically. Jump finds containers by name, image or host and lands you on the right row.

## 3.8.0 - 2026-05-08

- feat: Esc cancels, never quits. Press q to quit.
- feat: Esc on the host list clears your group filter or your bulk selection and stops there. A stray Esc on an empty host list keeps you in the app instead of closing it.
- feat: The first idle Esc per session surfaces a one-shot toast pointing to `q`, then stays a silent no-op. If a sticky error is up, the hint waits so it never displaces a message you still need to see.
- change: Help footer and wiki spell out `q` as the only quit key on the host list and tunnels page.
- Esc-as-cancel suggested by @BrahmingWu (#63).

## 3.7.0 - 2026-05-08

- feat: Multiple accounts per cloud provider, no juggling.
- feat: Add a second AWS, DigitalOcean or any cloud account in the providers list. Press `a` on the row, give both configs a label and they live side by side. Existing hosts stay yours.
- feat: Each labeled config syncs in its own lane. Markers carry the label, so two accounts never claim each other's hosts and a `--remove` on one doesn't touch the other.
- feat: Vault SSH role and address resolve per labeled config, so work and personal can sign with their own roles in their own Vaults.
- feat: `purple sync digitalocean:work` targets one config; the bare provider name still syncs all configs of that provider.
- feat: `purple add --label work` and `purple provider remove digitalocean:work` round out the CLI.
- change: Faster Nix release builds. CI's `nix flake check` now whitelists the source files it needs and skips LTO, saving ~20 minutes per release.
- change: `~/.purple/sync_history.tsv` first column now reads `provider[:label]` (e.g. `digitalocean:work`) for labeled configs. Bare configs keep the plain provider name. Scripts grepping the file may need updating.
- Thanks to @AndreyOkh for suggesting multi-account support.

## 3.6.0 - 2026-05-06

- feat: Find any host, tunnel or action in one keystroke.
- feat: Press `:` to open Jump, a universal fuzzy bar across hosts, tunnels, containers, snippets and actions. Like `Cmd+K`, in your terminal.
- feat: Type your SSH username to find every server you log in as. Jump searches User, ProxyJump and Vault SSH role too, not just aliases and hostnames.
- feat: Field prefixes (`user:`, `proxy:`, `vault:`, `tag:`) scope your search to a single SSH directive when you need precision.
- feat: Recent picks lead the bar across sessions, so the second time is always faster than the first.
- change: Footer keys land in one canonical order across every overlay, so muscle memory survives between screens.
- change: Action labels carry their category up front (`Hosts: Add host`, `Tunnels: Manage tunnels`) so you can search by what you want to do.

## 3.5.0 - 2026-05-05

- feat: Vault SSH connects through bastions on the first try.
- feat: Active tab stays crisply underlined on every terminal, including macOS Terminal where the highlight used to bleed across tabs.
- feat: The selected host's ping dot self-refreshes as you arrow through the list, so freshness follows your cursor.
- feat: When Vault signing fails, the toast tells you the actual cause instead of being shadowed by the generic SSH error.
- change: Proxmox sync breaks down partial failures by category (timeout, HTTP 5xx, HTTP 4xx, transport, parse) in the summary line.
- change: Pre-commit gates catch hardcoded user-facing text and convention drift before they ship, so error wording stays consistent.
- change: Smoke tests verify each screen actually renders, not just that the binary stays alive.

## 3.4.1 - 2026-05-05

- change: See exactly which key cleared your ping dots.
- change: Every ping refresh, clear and config-driven reset now appears in --verbose so your bug reports include the right context out of the box.

## 3.4.0 - 2026-05-05

- feat: Online dots stay accurate without manual refreshes.
- feat: Ping results no longer expire. Once you press P, dots stay green, amber or red until you choose to clear them.
- feat: The detail panel now shows when each host was last checked, so you can tell fresh data from yesterday at a glance.
- feat: Selecting a host whose result is older than 2 minutes triggers a quiet single-host re-ping in the background. Dots stay current as you browse, even on configs with hundreds of hosts, without flooding the network.
- change: ProxyJump dependents inherit both status and last-checked time from their bastion.

## 3.3.0 - 2026-05-04

- feat: Use SSH keys stored anywhere via symlinks.
- feat: purple follows symlinks in ~/.ssh, so keys synced from password managers, secure storage or central key directories appear in the picker exactly like a regular file you placed there yourself.
- change: Tunnels page mirrors the Hosts layout. Column titles line up with the rows below, getpurple.sh anchors the bottom-right corner, and the inactive tab fades to muted so your eye lands on the active page first.
- Symlinked SSH keys reported by @Nab0y (#58).

## 3.2.0 - 2026-05-04

- feat: Install purple on NixOS, or with Nix on Linux and macOS.
- feat: `nix profile install github:erickochen/purple` adds purple to your Nix profile in one command. Same single binary as every other channel.
- feat: `nix develop github:erickochen/purple` drops you into a pinned Rust toolchain with cargo-audit and cargo-deny ready, so contributors get the exact same environment as CI.
- feat: Hash-locked flake inputs for reproducible builds. NixOS users can embed purple as a flake input with the follows-pattern recipe in the wiki.
- Initial flake foundation contributed by @ArisoN-ext (#54).

## 3.1.0 - 2026-05-03

- feat: live tunnel throughput on Linux, parity with macOS.
- feat: detail panel on Linux now shows real bytes-per-second per client process via a direct kernel netlink call (NETLINK_SOCK_DIAG), alongside the channel-events swimlane and per-client roster you already had.
- change: ROUTE card uses a dotted vertical connector and a fisheye marker on the destination so multi-hop chains read clearly without relying on colour.
- fix: long PATTERN MATCH header lists wrap across multiple rows instead of overflowing the detail panel border.

## 3.0.0 - 2026-05-03

- feat: hosts and live tunnels in one cockpit, Tab between.
- feat: tunnels page shows every SSH forward with live throughput, channel events and the apps moving bytes. Local, Remote and Dynamic SOCKS.
- feat: detail panel reveals which client process owns each open channel, with a 60-second swimlane of opens and closes.
- feat: add a tunnel from the tunnels page itself, with a fuzzy host picker that filters as you type.
- change: Tab and Shift+Tab now switch between Hosts and Tunnels pages instead of cycling group tabs.

## 2.45.2 - 2026-04-25

- feat: ProxyJump bastions get their own credentials.
- feat: Each hop in your jump chain reads its own keychain, 1Password, Bitwarden, pass or Vault KV entry, so multi-hop SSH connects without typing.
- feat: Every hop is scoped to its own retry window, so a stale bastion marker no longer blocks the next reconnect.
- change: Prompt-based credential lookup is restricted to the hosts in your ProxyJump chain, so a malicious server cannot phish credentials for other entries in your config.

## 2.45.1 - 2026-04-20

- fix: Tighter releases. Build verification stays stable across calendar days and CI runners.

## 2.45.0 - 2026-04-19

- feat: Claude Desktop one-click install with safe defaults.
- feat: Download the `.mcpb` bundle from any release and double-click. Your agent gets `list_hosts`, `get_host` and `list_containers` out of the box. No shell, no container control.
- feat: `purple mcp --read-only` exposes the same safe trio when wiring up Claude Code, Cursor or any other MCP client by hand.
- feat: Every MCP tool call lands in `~/.purple/mcp-audit.log` as JSON. Timestamp, tool, args, outcome, reason. Owner-only file mode. `run_command` arguments redacted so passwords on shell flags never hit disk. Redirect with `--audit-log <PATH>` or turn it off with `--no-audit`.
- feat: Audit log refuses to open through a pre-existing symlink, so a writable-directory attacker cannot redirect writes elsewhere. `run_command` switched off busy-polling and clamps oversized timeouts, so a runaway agent cannot pin the server.
- feat: Linux musl builds for `x86_64` and `aarch64` ship next to the existing glibc binaries. Drop the static binary on any distro and run.
- change: Wordmark refreshed with cleaner box-drawing strokes. Same cadence, still a cyan period at the end.

## 2.44.0 - 2026-04-18

- feat: Live progress, live hosts, untruncated IPs.
- feat: The sync footer shows which provider is in flight, how many providers are done and a running diff, so you can tell at a glance what is changing in your host list.
- feat: Online indicators pulse gently so a quick scan of the host list tells you which hosts are alive right now.
- feat: IP addresses in the address column always render in full, even next to a ProxyJump or tunnel indicator, so you can read and copy them without squinting.
- change: Welcome, Help and What's New overlays share one clean logotype with the cyan-dot accent from the landing page, giving web and TUI one consistent look.
- fix: Upgrade toasts fire reliably after a brew or curl install when the previous run was a dev build, so you actually see release notes for the version you just installed.

## 2.43.2 - 2026-04-18

- feat: Cleaner internals, steadier releases.
- change: Domain types now live with the state they belong to. Screens, pickers, tags, sync records and reload tracking each move to their own module, so the next change you make lands in one obvious place.
- change: The `UiSelection` god-struct becomes a small set of focused picker states, making overlay behaviour easier to read and extend.
- change: `App::new` shrinks from 124 lines to 48 by delegating to per-substate constructors that make startup initialisation explicit.

## 2.43.1 - 2026-04-18

- feat: Tighter foundations for faster, safer releases.
- change: App state is split into six focused domains (hosts, tunnels, snippets, providers, forms, status) so changes land in one place and are easier to reason about.
- change: New behaviour tests cover status routing, toast expiry and provider ordering, catching regressions earlier.
- change: Fewer surprise file reads at startup. Provider state loads explicitly in one place, so tests and tools run predictably.

## 2.43.0 - 2026-04-18

- feat: Confident edits on every host, shared lines included.
- feat: Multi-alias `Host` lines like `Host web-01 web-01.prod` are first-class in the TUI. Edit, rename and delete them from any alias and the on-disk config keeps up.
- feat: Deleting one alias from a shared line keeps the surviving aliases and their shared directives in place, ready to use.
- feat: The delete confirm dialog spells out the sibling aliases that will survive, so you press y knowing exactly what stays.
- change: Vault SSH certificates and Vault HTTP endpoints scope themselves per-alias automatically, keeping shared lines clean.
- change: OpenSSH itself is now a test oracle. Purple's parse and serialize cycle is cross-validated against `ssh -G` on 16 curated configs plus every fuzz seed, on top of 637k-run fuzzing.

## 2.42.3 - 2026-04-18

- change: Codebase tune-up: sharper debug trail, snappier renders.
- change: Debug logs now show every screen navigation, so support bundles explain exactly how you got into a state
- change: Cloud sync thread-spawn failures log the underlying OS error instead of a generic "failed to start" toast
- change: Preference-save errors (view mode, sort order) now surface in the debug log instead of being silently dropped
- fix: Remove a theoretical crash path in toast-timeout calculation when a new message class is added

## 2.42.2 - 2026-04-18

- change: Snappier host list. Safer, tighter releases.
- change: Large inventories render faster. Column widths and group health summaries are cached across frames instead of rebuilt on every render tick
- change: Editing the installer or landing page no longer risks breaking getpurple.sh. One command regenerates the embedded copies from source
- change: Full pre-commit suite now runs in CI. Format, clippy, build, test, deny, MSRV, doc, site sync, smoke test, design system, messages, keybindings and visual regression all gate every push
- change: Adding a new cloud provider is one entry in one table instead of edits to four parallel lookups
- fix: Update, tunnel and snippet CLI output routed through the centralized message module, keeping wording consistent

## 2.42.1 - 2026-04-17

- fix: SSH prompts no longer bleed into the TUI
- fix: Opening the container overlay on an untrusted host now shows a clear "host key unknown" message instead of raw SSH prompt text leaking across the screen
- fix: Background SSH fetches (containers, file browser listings) refuse untrusted hosts cleanly. Connect once with Enter to trust a host, then the overlays work as expected
- change: Captured SSH calls no longer inherit stdin, so nothing can escape into the terminal while purple is drawing

## 2.42.0 - 2026-04-17

- feat: What's New catches up even if you skipped releases
- feat: Press n on the host list or command palette to reopen the overlay
- fix: Overlay bullets render without raw markdown and wrap cleanly on narrow terminals
- fix: Version-check cache writes atomically and survives long release headlines

## 2.41.1 - 2026-04-16

- fix: Picker hints now say Space, matching the key you actually press
- fix: Password Source, ProxyJump, SSH Key, Vault SSH Role and provider Regions/Endpoint fields all show the correct Space hint
- fix: Flaky preferences test race eliminated. Handler and preferences tests no longer collide
- change: Form hint copy centralized in one place so wording stays consistent across forms
- change: Docs caught up. llms.txt, landing page and wiki now say Space for picker opens

## 2.41.0 - 2026-04-16

- feat: One consistent look, feel and keyboard everywhere
- feat: Enter always submits. Space activates the focused field. No more guessing which key does what
- feat: Confirm dialogs accept only y, n or Esc. Labels spell out the stakes before you commit
- feat: Toast notifications show a shrinking bar. Errors stay until acknowledged. Warnings auto-expire
- feat: Focused form fields tell you what Space does. Toggles, pickers and text fields each show their own hint
- fix: A stray keypress no longer silently triggers destructive actions. Closes [#27](https://github.com/erickochen/purple/issues/27)
- fix: Closing the bulk tag editor with unsaved changes now asks before discarding them

## 2.40.0 - 2026-04-15

- feat: Provider sync catches API drift before your config breaks
- fix: Security update. rustls-webpki CVE patched
- feat: All 16 providers validated against upstream OpenAPI specs on every push. See [#28](https://github.com/erickochen/purple/issues/28)
- feat: Daily GitHub Actions watches 12 provider changelog pages for deprecation keywords
- fix: Flaky bulk_tag_undo test from a shared temp file path

## 2.39.0 - 2026-04-14

- feat: API contract tests catch provider drift before it breaks your syncs
- feat: 32 golden fixtures cover all 16 providers and every documented endpoint
- feat: 35 integration tests run on every push to detect response structure changes early

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
