use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};
use log::{debug, error, warn};

use crate::ssh_config::model::SshConfigFile;

/// A password source option for the picker overlay.
pub struct PasswordSourceOption {
    pub label: &'static str,
    pub value: &'static str,
    pub hint: &'static str,
}

pub const PASSWORD_SOURCES: &[PasswordSourceOption] = &[
    PasswordSourceOption {
        label: "OS Keychain",
        value: "keychain",
        hint: "keychain",
    },
    PasswordSourceOption {
        label: "1Password",
        value: "op://",
        hint: "op://Vault/Item/field",
    },
    PasswordSourceOption {
        label: "Bitwarden",
        value: "bw:",
        hint: "bw:item-name",
    },
    PasswordSourceOption {
        label: "pass",
        value: "pass:",
        hint: "pass:path/to/entry",
    },
    // Vault KV secrets engine (key/value store). Distinct from the Vault SSH
    // secrets engine used for signed SSH certificates, which has its own
    // "Vault SSH role" field on the host form.
    PasswordSourceOption {
        label: "HashiCorp Vault KV",
        value: "vault:",
        hint: "vault:secret/path#field",
    },
    PasswordSourceOption {
        label: "Proton Pass",
        value: "proton:",
        hint: "proton:Vault/Item/field",
    },
    PasswordSourceOption {
        label: "Custom command",
        value: "cmd:",
        hint: "cmd %a %h",
    },
    PasswordSourceOption {
        label: "None",
        value: "",
        hint: "(remove)",
    },
];

/// OpenSSH retries use a different prompt without the initial yes/no suffix.
fn is_host_key_prompt(prompt: &str) -> bool {
    prompt.contains("yes/no") || prompt.starts_with("Please type 'yes'")
}

/// Never echo a partial fingerprint: OpenSSH would retry with a new prompt.
fn host_key_answer<'a>(prompt: &str, answer: &'a str) -> &'a str {
    let answer = answer.trim();
    if answer.eq_ignore_ascii_case("yes") {
        return "yes";
    }
    let fingerprint = prompt.split_whitespace().find_map(|word| {
        let word = word.trim_end_matches('.');
        let digest = word.strip_prefix("SHA256:")?;
        (digest.len() == 43
            && digest
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/'))
        .then_some(word)
    });
    if fingerprint == Some(answer) {
        answer
    } else {
        "no"
    }
}

/// Keep the full OpenSSH fingerprint prompt on the controlling terminal;
/// stdout is reserved for the answer consumed by SSH.
fn confirm_host_key_on_tty(prompt: &str) -> Result<()> {
    use std::io::{BufRead, Write};
    let mut tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .context("No terminal available to confirm the SSH host fingerprint")?;
    write!(tty, "\n{prompt} ")?;
    tty.flush()?;
    let mut answer = String::new();
    std::io::BufReader::new(tty).read_line(&mut answer)?;
    println!("{}", host_key_answer(prompt, &answer));
    Ok(())
}

/// Handle an SSH_ASKPASS invocation. Called when purple is invoked as an askpass program.
/// Reads the password source from the host's `# purple:askpass` comment and retrieves it.
pub fn handle(env: &crate::runtime::env::Env) -> Result<()> {
    // Initialize file-only logging for askpass subprocess
    // verbose is determined by PURPLE_LOG env var only (no CLI flag in subprocess)
    crate::logging::init(false, false, env);

    let alias = env.var("PURPLE_HOST_ALIAS").unwrap_or_default().to_string();
    let config_path = env
        .var("PURPLE_CONFIG_PATH")
        .unwrap_or_default()
        .to_string();

    // Check the prompt (argv[1]) to skip passphrase and host key verification prompts
    let prompt = std::env::args().nth(1).unwrap_or_default();

    // A notice that a security key wants a touch. ssh expects no answer and
    // stops this helper once the key responded.
    if env.var("SSH_ASKPASS_PROMPT") == Some("none") {
        if !crate::askpass_env::runs_in_background(env) {
            crate::security_key::show_on_tty(&prompt);
        }
        debug!("[purple] Askpass passed on a notice for {alias}");
        return Ok(());
    }

    // A security key PIN is never a password: no source and no typed
    // password may answer it.
    if let Some(pin_prompt) = crate::security_key::parse_pin_prompt(&prompt) {
        return handle_pin_prompt(env, &alias, &prompt, pin_prompt);
    }

    // force-mode askpass never falls back to the terminal on failure.
    // Host trust must be explicitly answered, never routed to a password vault.
    if is_host_key_prompt(&prompt) || env.var("SSH_ASKPASS_PROMPT") == Some("confirm") {
        anyhow::ensure!(
            !crate::askpass_env::runs_in_background(env),
            "Host key unknown. Connect first (Enter) to trust the host."
        );
        return confirm_host_key_on_tty(&prompt);
    }

    let prompt_lower = prompt.to_ascii_lowercase();
    if prompt_lower.contains("passphrase")
        || prompt_lower.contains("yes/no")
        || prompt_lower.contains("(yes/no/")
    {
        // Not a password prompt. Exit with error so SSH falls back to interactive.
        std::process::exit(1);
    }

    if alias.is_empty() || config_path.is_empty() {
        std::process::exit(1);
    }

    // Parse config first so we can resolve the prompt's host to the right entry.
    // With ProxyJump, ssh fires askpass for each hop. The prompt argv[1] tells us
    // which hop is being authenticated; PURPLE_HOST_ALIAS only knows the final
    // target. Resolving the prompt host to its own alias scopes the keychain
    // lookup to the correct entry per hop.
    let config = SshConfigFile::parse_with_env(&PathBuf::from(&config_path), env)
        .context("Failed to parse SSH config")?;

    // Restrict prompt-based resolution to PURPLE_HOST_ALIAS and the hosts
    // reachable via its ProxyJump chain. Without this scope, a malicious
    // server could send a keyboard-interactive prompt formatted like a
    // password prompt for an unrelated host (`attacker@victim's password:`)
    // and exfiltrate that host's credential. Chain membership ensures we
    // only ever supply credentials for hosts the user has wired into this
    // connection.
    let chain = build_proxy_chain(&config, &alias);
    let prompt_host = parse_password_prompt_host(&prompt);
    let prompt_hop = prompt_host.and_then(|h| find_alias_for_host(&config, h, &chain));
    let resolved_alias = prompt_hop.clone().unwrap_or_else(|| alias.clone());

    // The marker is keyed on the resolved alias so retries on one ProxyJump
    // hop do not block askpass on the next hop.
    let marker = marker_path(env.paths(), &resolved_alias);

    // Whether this prompt may be answered at all. Decided once, then applied
    // to the one-shot password and to a configured source alike, so both
    // channels release a credential on the same evidence.
    //
    // A prompt already naming a hop inside the chain settles itself. For the
    // rest, ssh answers the route question exactly, including for shapes the
    // model does not parse: a `Match` block, a directive above the first
    // `Host` line, a canonicalized name. Its reading is the one that counts,
    // because it is ssh that opens the connection. Asking costs a subprocess
    // that runs the user's own `Match exec` commands, so it stays behind the
    // cheap answer.
    let attributable = prompt_hop.is_some()
        || match ssh_resolves_a_proxy(env, &config_path, &alias) {
            Some(proxied) => !proxied,
            // ssh could not be asked. All that is left is the model, which
            // does not carry every shape ssh reads. A prompt naming a host
            // the model cannot place is refused on that uncertainty rather
            // than answered on a route that may be incomplete.
            None => prompt_host.is_none() && !chain_uses_jump_host(&config, &chain),
        };

    // One-shot password typed in the TUI for this one run, scoped to the
    // alias the parent named so a hop authenticating on the way to the
    // target never receives the target's password.
    //
    // No marker is armed here. Every command that carries the one-shot pair
    // also carries `-o NumberOfPasswordPrompts=1`, so ssh asks once per run
    // and a rejected password costs one login attempt. Arming a marker would
    // instead refuse the second ssh of a two-step operation, such as the
    // remote home lookup followed by its listing.
    if let (Some(oneshot_alias), Some(secret)) = (
        env.var(crate::askpass_env::ONESHOT_ALIAS_VAR),
        env.var(crate::askpass_env::ONESHOT_SECRET_VAR),
    ) && oneshot_alias == resolved_alias
        && attributable
    {
        debug!("[purple] Askpass one-shot password used for {resolved_alias}");
        print!("{}", secret);
        return Ok(());
    }

    // A prompt that cannot be placed inside this connection's own chain gets
    // nothing, from either channel. Exiting before the retry marker leaves no
    // trace of an attempt, because none was made.
    //
    // A marker of its own does go out, under the alias the parent named. The
    // TUI reads it to tell this case apart from an ordinary refusal: a
    // password typed here would meet the same wall, so it asks for none and
    // explains instead.
    if !attributable {
        debug!(
            "[purple] Askpass withheld for {resolved_alias}: prompt_host={:?} placed_in_chain=false chain_size={}",
            prompt_host,
            chain.len()
        );
        if let Some(path) = withheld_marker_path(env.paths(), &alias) {
            arm_marker(&path);
        }
        std::process::exit(1);
    }

    // Retry detection: if we've been called recently for this resolved alias,
    // the password was wrong. Exit with error so SSH falls back to interactive.
    //
    // A background run skips this. ssh there carries
    // `-o NumberOfPasswordPrompts=1`, so it asks once per hop and the loop
    // the marker exists to break cannot form. Keeping it would instead
    // refuse the second ssh of a two-step operation, such as the remote home
    // lookup followed by its listing.
    let single_attempt = env
        .var(crate::askpass_env::SINGLE_ATTEMPT_VAR)
        .is_some_and(|v| v == "1");
    if let Some(marker_path) = &marker
        && !single_attempt
    {
        if is_recent_marker(marker_path) {
            debug!("[purple] Askpass retry detected for {resolved_alias}");
            let _ = std::fs::remove_file(marker_path);
            std::process::exit(1);
        }
        arm_marker(marker_path);
    }

    let source = find_askpass_source(&config, env.paths(), &resolved_alias);

    let source = match source {
        Some(s) => s,
        None => std::process::exit(1),
    };

    debug!("[purple] Askpass invoked for alias={resolved_alias} source={source}");

    let hostname = find_hostname(&config, &resolved_alias);
    match retrieve_password(env, &source, &resolved_alias, &hostname) {
        Ok(password) => {
            debug!("[purple] Askpass retrieved password for {resolved_alias} via {source}");
            print!("{}", password);
            Ok(())
        }
        Err(err) => {
            warn!("[external] Password retrieval failed via {source}");
            debug!("[external] Password retrieval detail: {err}");
            if let Some(m) = &marker {
                let _ = std::fs::remove_file(m);
            }
            std::process::exit(1);
        }
    }
}

/// Answer ssh's security key PIN prompt. A background run hands out the PIN
/// the TUI put in its environment, at most once per ssh process and never
/// after the key refused it. Without one it tells the TUI and answers
/// nothing, which costs the key no attempt. A terminal run asks on the tty.
fn handle_pin_prompt(
    env: &crate::runtime::env::Env,
    alias: &str,
    prompt: &str,
    pin_prompt: crate::security_key::PinPrompt<'_>,
) -> Result<()> {
    use crate::security_key as sk;
    let key_path = pin_prompt.key_path;
    if !crate::askpass_env::runs_in_background(env) {
        match sk::read_pin_from_tty(prompt) {
            Ok(Some(pin)) => {
                debug!("[purple] Askpass read the security key PIN on the terminal for {key_path}");
                print!("{}", pin);
                return Ok(());
            }
            Ok(None) => debug!("[purple] Askpass security key PIN canceled for {key_path}"),
            Err(e) => debug!("[purple] Askpass could not ask for the security key PIN: {e}"),
        }
        std::process::exit(1);
    }
    let paths = env.paths();
    let ssh_pid = parent_pid();
    if let Some(pin) = sk::pin_from_env(env, key_path) {
        if let Some(reason) = sk::refusal_for(paths, key_path) {
            debug!("[purple] Askpass withheld the PIN for {key_path}: refused before ({reason:?})");
        } else if sk::handed_out(paths, alias, ssh_pid, key_path) {
            // The same ssh asking twice means its first try failed. A second
            // hand-out would cost the key one more attempt.
            debug!("[purple] Askpass withheld the PIN for {key_path}: this ssh already had it");
            std::process::exit(1);
        } else if sk::claim_hand_out(paths, alias, ssh_pid, key_path) {
            print!("{}", pin);
            return Ok(());
        } else {
            debug!("[purple] Askpass withheld the PIN for {key_path}: refused while it waited");
        }
    }
    if let Some(flow) = sk::flow_from_env(env)
        && !alias.is_empty()
    {
        sk::arm_pin_request(paths, flow, alias, key_path);
    }
    std::process::exit(1);
}

/// The ssh process that started this helper.
fn parent_pid() -> u32 {
    #[cfg(unix)]
    {
        std::os::unix::process::parent_id()
    }
    #[cfg(not(unix))]
    {
        0
    }
}

/// Extract the host being authenticated from an OpenSSH password prompt.
/// OpenSSH builds prompts as `<user>@<host>'s password:` (see `userauth_passwd`
/// in openssh-portable). IPv6 hosts are rendered with brackets (`user@[::1]`),
/// which we strip so the result matches a plain `HostName` entry. Returns
/// `None` for keyboard-interactive prompts or any other format we cannot parse,
/// so the caller falls back to PURPLE_HOST_ALIAS.
fn parse_password_prompt_host(prompt: &str) -> Option<&str> {
    let idx = prompt.find("'s password")?;
    let head = &prompt[..idx];
    let (_, host) = head.rsplit_once('@')?;
    let host = host.trim();
    let host = host
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(host);
    if host.is_empty() { None } else { Some(host) }
}

/// Find the alias whose entry matches `host` by alias or hostname, restricted
/// to entries in `permitted`. Alias match takes priority over hostname match
/// in a single pass. Used to map the SSH prompt's host (which may be a bastion
/// in a ProxyJump chain) back to the entry that owns its `# purple:askpass`
/// config. The `permitted` scope blocks malicious-server attempts to coax a
/// credential lookup for an unrelated host in `~/.ssh/config`.
fn find_alias_for_host(
    config: &SshConfigFile,
    host: &str,
    permitted: &HashSet<String>,
) -> Option<String> {
    let mut by_hostname: Option<String> = None;
    for entry in config.host_entries() {
        if !permitted.contains(&entry.alias) {
            continue;
        }
        if entry.alias.eq_ignore_ascii_case(host) {
            return Some(entry.alias.clone());
        }
        if by_hostname.is_none() && entry.hostname.eq_ignore_ascii_case(host) {
            by_hostname = Some(entry.alias.clone());
        }
    }
    by_hostname
}

/// True when any host in `chain` routes through a jump host, written either
/// as `ProxyJump` or as a `ProxyCommand`. Neither form is guaranteed to put
/// the bastion in the chain: a `ProxyJump` naming a host with no `Host` block
/// of its own contributes no alias, and a `ProxyCommand` names its bastion
/// inside a command line purple does not parse. So chain size alone cannot
/// tell a direct connection from one behind a bastion. This can.
fn chain_uses_jump_host(config: &SshConfigFile, chain: &HashSet<String>) -> bool {
    config.host_entries().iter().any(|e| {
        chain.contains(&e.alias) && (!e.proxy_jump.trim().is_empty() || e.has_proxy_command)
    })
}

/// How long `ssh -G` gets before its answer is given up on. Reading a config
/// is instant, but `CanonicalizeHostname` makes it resolve names and a
/// `Match exec` block runs a command of the user's own, either of which can
/// stall. ssh is waiting on this askpass while that happens, so the wait is
/// bounded and the model answers instead.
const PROXY_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Ask ssh whether it resolves a route through another machine for `alias`.
/// `ssh -G` prints the configuration it will actually use, so this covers
/// every shape the model does not carry: a `Match` block, a directive above
/// the first `Host` line, a canonicalized name. It omits both keywords
/// entirely when nothing proxies, and `ProxyCommand none` prints no line at
/// all. `None` when ssh could not be run, took too long or refused the
/// alias, which leaves the answer to the caller.
fn ssh_resolves_a_proxy(
    env: &crate::runtime::env::Env,
    config_path: &str,
    alias: &str,
) -> Option<bool> {
    let mut child = env
        .command("ssh")
        .arg("-G")
        .arg("-F")
        .arg(config_path)
        .arg("--")
        .arg(alias)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    // Read on a thread rather than after the wait, so a chatty config cannot
    // fill the pipe and park both sides. EOF on the pipe means ssh is done.
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let read = std::io::Read::read_to_string(&mut stdout, &mut text);
        let _ = tx.send(read.ok().map(|_| text));
    });
    let Ok(Some(text)) = rx.recv_timeout(PROXY_PROBE_TIMEOUT) else {
        debug!("[purple] Askpass proxy probe gave no answer for {alias}");
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    if !child.wait().ok()?.success() {
        return None;
    }
    Some(
        text.lines()
            .any(|l| l.starts_with("proxycommand ") || l.starts_with("proxyjump ")),
    )
}

/// Build the set of aliases reachable from `target` via its ProxyJump chain,
/// including `target` itself. ProxyJump values can be comma-separated and
/// formatted `[user@]host[:port]`, including bracketed IPv6 hosts. Cycles are
/// broken by the visited-set; entries that reference unknown hosts contribute
/// nothing to the chain.
fn build_proxy_chain(config: &SshConfigFile, target: &str) -> HashSet<String> {
    let entries = config.host_entries();
    let mut chain: HashSet<String> = HashSet::new();
    let mut queue: Vec<String> = vec![target.to_string()];
    while let Some(current) = queue.pop() {
        if !chain.insert(current.clone()) {
            continue;
        }
        let Some(entry) = entries.iter().find(|e| e.alias == current) else {
            continue;
        };
        if entry.proxy_jump.is_empty() {
            continue;
        }
        for jump in entry.proxy_jump.split(',') {
            let host = parse_proxy_jump_host(jump);
            if host.is_empty() {
                continue;
            }
            for e in &entries {
                if e.alias.eq_ignore_ascii_case(host) || e.hostname.eq_ignore_ascii_case(host) {
                    queue.push(e.alias.clone());
                }
            }
        }
    }
    chain
}

/// Extract the host portion from a single ProxyJump entry of the form
/// `[user@]host[:port]`. Handles bracketed IPv6 hosts (`[::1]:22`).
fn parse_proxy_jump_host(jump: &str) -> &str {
    let trimmed = jump.trim();
    let after_user = trimmed.rsplit_once('@').map(|(_, h)| h).unwrap_or(trimmed);
    if let Some(rest) = after_user.strip_prefix('[')
        && let Some(end) = rest.find(']')
    {
        return &rest[..end];
    }
    after_user.split(':').next().unwrap_or(after_user)
}

/// Find the askpass source for a host. Checks per-host config, then global default.
fn find_askpass_source(
    config: &SshConfigFile,
    paths: Option<&crate::runtime::env::Paths>,
    alias: &str,
) -> Option<String> {
    // Per-host source
    for entry in config.host_entries() {
        if entry.alias == alias
            && let Some(ref source) = entry.askpass
        {
            return Some(source.clone());
        }
    }
    // Global default from preferences file
    load_askpass_default_direct(paths)
}

/// Read askpass default directly from ~/.purple/preferences without depending on the
/// preferences module (which requires crate::app and isn't available in askpass subprocess).
fn load_askpass_default_direct(paths: Option<&crate::runtime::env::Paths>) -> Option<String> {
    let path = paths?.preferences();
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once('=')
            && k.trim() == "askpass"
        {
            let val = v.trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

/// Find the hostname for an alias (for %h substitution).
fn find_hostname(config: &SshConfigFile, alias: &str) -> String {
    for entry in config.host_entries() {
        if entry.alias == alias {
            return entry.hostname.clone();
        }
    }
    alias.to_string()
}

/// Retrieve a password from the given source.
fn retrieve_password(
    env: &crate::runtime::env::Env,
    source: &str,
    alias: &str,
    hostname: &str,
) -> Result<String> {
    if source == "keychain" {
        return retrieve_from_keychain(env, alias);
    }
    if let Some(uri) = source.strip_prefix("op://") {
        return retrieve_from_1password(env, &format!("op://{}", uri));
    }
    if let Some(entry) = source.strip_prefix("pass:") {
        return retrieve_from_pass(env, entry);
    }
    if let Some(item_id) = source.strip_prefix("bw:") {
        return retrieve_from_bitwarden(env, item_id);
    }
    if let Some(rest) = source.strip_prefix("vault:") {
        return retrieve_from_vault(env, rest);
    }
    if let Some(spec) = source.strip_prefix("proton:") {
        return retrieve_from_proton_pass(env, spec);
    }
    // Custom command (with or without cmd: prefix)
    let cmd = source.strip_prefix("cmd:").unwrap_or(source);
    retrieve_from_command(env, cmd, alias, hostname)
}

/// Retrieve from OS keychain (macOS: Keychain, Linux: secret-tool).
fn retrieve_from_keychain(env: &crate::runtime::env::Env, alias: &str) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let output = env
            .command("security")
            .args([
                "find-generic-password",
                "-a",
                alias,
                "-s",
                "purple-ssh",
                "-w",
            ])
            .output()
            .context("Failed to run security command")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::warn!(
                "[external] askpass keychain lookup failed: alias={} exit={} stderr={}",
                alias,
                output.status.code().unwrap_or(-1),
                stderr.trim().lines().next().unwrap_or("<empty>"),
            );
            anyhow::bail!("Keychain lookup failed");
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let output = env
            .command("secret-tool")
            .args(["lookup", "application", "purple-ssh", "host", alias])
            .output()
            .context("Failed to run secret-tool")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::warn!(
                "[external] askpass secret-tool lookup failed: alias={} exit={} stderr={}",
                alias,
                output.status.code().unwrap_or(-1),
                stderr.trim().lines().next().unwrap_or("<empty>"),
            );
            anyhow::bail!("Secret-tool lookup failed");
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

/// Check if a password exists in the OS keychain for this alias.
pub fn keychain_has_password(env: &crate::runtime::env::Env, alias: &str) -> bool {
    retrieve_from_keychain(env, alias).is_ok()
}

/// Retrieve a password from the OS keychain. Public for keychain migration on alias rename.
pub fn retrieve_keychain_password(env: &crate::runtime::env::Env, alias: &str) -> Result<String> {
    retrieve_from_keychain(env, alias)
}

/// Store a password in the OS keychain.
pub fn store_in_keychain(
    env: &crate::runtime::env::Env,
    alias: &str,
    password: &str,
) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let status = env
            .command("security")
            .args([
                "add-generic-password",
                "-U",
                "-a",
                alias,
                "-s",
                "purple-ssh",
                "-w",
                password,
            ])
            .status()
            .context("Failed to run security command")?;
        if !status.success() {
            anyhow::bail!("Failed to store password in Keychain");
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mut child = env
            .command("secret-tool")
            .args([
                "store",
                "--label",
                &format!("purple-ssh: {}", alias),
                "application",
                "purple-ssh",
                "host",
                alias,
            ])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("Failed to run secret-tool")?;
        if let Some(ref mut stdin) = child.stdin {
            use std::io::Write;
            stdin.write_all(password.as_bytes())?;
        }
        let status = child.wait()?;
        if !status.success() {
            anyhow::bail!("Failed to store password with secret-tool");
        }
        Ok(())
    }
}

/// Remove a password from the OS keychain.
pub fn remove_from_keychain(env: &crate::runtime::env::Env, alias: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let status = env
            .command("security")
            .args(["delete-generic-password", "-a", alias, "-s", "purple-ssh"])
            .status()
            .context("Failed to run security command")?;
        if !status.success() {
            anyhow::bail!("No password found for '{}' in Keychain", alias);
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let status = env
            .command("secret-tool")
            .args(["clear", "application", "purple-ssh", "host", alias])
            .status()
            .context("Failed to run secret-tool")?;
        if !status.success() {
            anyhow::bail!("Failed to remove password with secret-tool");
        }
        Ok(())
    }
}

/// Retrieve from 1Password CLI.
fn retrieve_from_1password(env: &crate::runtime::env::Env, uri: &str) -> Result<String> {
    let result = env
        .command("op")
        .args(["read", uri, "--no-newline"])
        .output();
    let output = match result {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            error!("[config] Password manager binary not found: op");
            return Err(e).context("Failed to run 1Password CLI (op)");
        }
        other => other.context("Failed to run 1Password CLI (op)")?,
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::warn!(
            "[external] askpass 1Password lookup failed: uri={} exit={} stderr={}",
            uri,
            output.status.code().unwrap_or(-1),
            stderr.trim().lines().next().unwrap_or("<empty>"),
        );
        anyhow::bail!("1Password lookup failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Retrieve from pass (password-store). Returns the first line.
fn retrieve_from_pass(env: &crate::runtime::env::Env, entry: &str) -> Result<String> {
    let result = env.command("pass").args(["show", entry]).output();
    let output = match result {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            error!("[config] Password manager binary not found: pass");
            return Err(e).context("Failed to run pass");
        }
        other => other.context("Failed to run pass")?,
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::warn!(
            "[external] askpass pass lookup failed: entry={} exit={} stderr={}",
            entry,
            output.status.code().unwrap_or(-1),
            stderr.trim().lines().next().unwrap_or("<empty>"),
        );
        anyhow::bail!("pass lookup failed");
    }
    let full = String::from_utf8_lossy(&output.stdout);
    Ok(full.lines().next().unwrap_or("").to_string())
}

/// Retrieve from Bitwarden CLI. The item_id can be an item ID or search term.
/// Uses `bw get password <item_id>` which requires an unlocked vault (BW_SESSION).
fn retrieve_from_bitwarden(env: &crate::runtime::env::Env, item_id: &str) -> Result<String> {
    let result = env
        .command("bw")
        .args(["get", "password", item_id])
        .output();
    let output = match result {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            error!("[config] Password manager binary not found: bw");
            return Err(e).context("Failed to run Bitwarden CLI (bw)");
        }
        other => other.context("Failed to run Bitwarden CLI (bw)")?,
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::warn!(
            "[external] askpass Bitwarden lookup failed: item={} exit={} stderr={}",
            item_id,
            output.status.code().unwrap_or(-1),
            stderr.trim().lines().next().unwrap_or("<empty>"),
        );
        anyhow::bail!("Bitwarden lookup failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Retrieve from the HashiCorp Vault KV secrets engine via the `vault` CLI.
/// Spec format: `path#field` or just `path` (defaults to `password`).
/// Distinct from the Vault SSH secrets engine (see src/vault_ssh.rs), which
/// signs SSH certificates rather than storing passwords.
fn retrieve_from_vault(env: &crate::runtime::env::Env, spec: &str) -> Result<String> {
    let (path, field) = match spec.rsplit_once('#') {
        Some((p, f)) => (p, f),
        None => (spec, "password"),
    };
    let result = env
        .command("vault")
        .args(["kv", "get", &format!("-field={}", field), path])
        .output();
    let output = match result {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            error!("[config] Password manager binary not found: vault");
            return Err(e).context("Failed to run vault CLI");
        }
        other => other.context("Failed to run vault CLI")?,
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::warn!(
            "[external] askpass Vault KV lookup failed: path={} field={} exit={} stderr={}",
            path,
            field,
            output.status.code().unwrap_or(-1),
            stderr.trim().lines().next().unwrap_or("<empty>"),
        );
        anyhow::bail!("Vault lookup failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Retrieve via custom command. Supports %h (hostname) and %a (alias) substitution.
/// Values are shell-escaped to prevent metacharacter injection.
fn retrieve_from_command(
    env: &crate::runtime::env::Env,
    cmd: &str,
    alias: &str,
    hostname: &str,
) -> Result<String> {
    let safe_alias = crate::snippet::shell_escape(alias);
    let safe_hostname = crate::snippet::shell_escape(hostname);
    let expanded = cmd.replace("%a", &safe_alias).replace("%h", &safe_hostname);
    let output = env
        .command("sh")
        .args(["-c", &expanded])
        .output()
        .context("Failed to run custom askpass command")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::warn!(
            "[external] askpass custom command failed: alias={} exit={} stderr={}",
            alias,
            output.status.code().unwrap_or(-1),
            stderr.trim().lines().next().unwrap_or("<empty>"),
        );
        anyhow::bail!("Custom askpass command failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Get the path for the retry marker file.
/// Sanitizes the alias to prevent path traversal (replaces `/` and `\` with `_`).
fn marker_path(paths: Option<&crate::runtime::env::Paths>, alias: &str) -> Option<PathBuf> {
    paths.map(|p| p.askpass_marker(alias))
}

/// Filename prefix of the marker that records a prompt a background run
/// could not place. The dot after `withheld` keeps it clear of a retry
/// marker, whose own sanitizer turns every dot into an underscore.
pub(crate) const WITHHELD_MARKER_PREFIX: &str = ".askpass_withheld.";

/// Path of the marker that records a prompt this connection could not place.
/// Keyed on the alias the TUI acted on, because that is the name it has to
/// look up afterwards. Cleared by `cleanup_marker` along with the rest.
fn withheld_marker_path(
    paths: Option<&crate::runtime::env::Paths>,
    alias: &str,
) -> Option<PathBuf> {
    paths.map(|p| p.askpass_withheld_marker(alias))
}

/// True when the last background run for `alias` met a prompt it could not
/// place inside the connection's own chain. A password typed in the TUI
/// would meet the same wall, so the caller asks for none.
///
/// Reading takes the marker with it. One withheld run then accounts for one
/// dialog and never for a later, unrelated one, which matters because the
/// runs that arm a marker do not all have a place to clear it.
pub fn take_withheld(paths: Option<&crate::runtime::env::Paths>, alias: &str) -> bool {
    let Some(path) = withheld_marker_path(paths, alias) else {
        return false;
    };
    let recent = is_recent_marker(&path);
    let _ = std::fs::remove_file(&path);
    recent
}

/// Write the retry marker. Its presence within the next minute means this
/// alias was already answered once, so the caller refuses a second answer.
fn arm_marker(path: &Path) {
    if let Err(e) = std::fs::create_dir_all(path.parent().unwrap()) {
        debug!("[purple] Failed to create askpass marker directory: {e}");
    }
    if let Err(e) = crate::fs_util::atomic_write(path, b"") {
        debug!("[purple] Failed to write askpass marker: {e}");
    }
}

/// Check if a marker file exists and is recent (< 60 seconds old).
fn is_recent_marker(path: &PathBuf) -> bool {
    if let Ok(meta) = std::fs::metadata(path)
        && let Ok(modified) = meta.modified()
        && let Ok(elapsed) = SystemTime::now().duration_since(modified)
    {
        return elapsed.as_secs() < 60;
    }
    false
}

/// Clean up retry markers after a successful connection. ProxyJump connections
/// create one marker per hop and the parent process only knows the final
/// target alias, so we clear every `.askpass_*` marker in the state directory
/// on success.
/// Each marker has a 60s expiry; this just keeps rapid reconnects snappy and
/// prevents a stranded bastion marker from blocking the next attempt.
pub fn cleanup_marker(paths: Option<&crate::runtime::env::Paths>, _alias: &str) {
    let Some(paths) = paths else {
        return;
    };
    let Ok(read) = std::fs::read_dir(paths.state_dir()) else {
        return;
    };
    for entry in read.flatten() {
        // Retry markers only. A withheld or PIN marker belongs to the run it
        // was armed for and is taken by whoever reads it, so a sweep running
        // for another host has no business removing it.
        if entry.file_name().to_str().is_some_and(|s| {
            s.starts_with(".askpass_")
                && !s.starts_with(WITHHELD_MARKER_PREFIX)
                && !s.starts_with(crate::security_key::PIN_MARKER_PREFIX)
        }) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Parse an askpass source string and return a description for display.
#[allow(dead_code)]
pub fn describe_source(source: &str) -> &str {
    if source == "keychain" {
        "OS Keychain"
    } else if source.starts_with("op://") {
        "1Password"
    } else if source.starts_with("proton:") {
        "Proton Pass"
    } else if source.starts_with("pass:") {
        "pass"
    } else if source.starts_with("bw:") {
        "Bitwarden"
    } else if source.starts_with("vault:") {
        "HashiCorp Vault KV"
    } else {
        "Custom command"
    }
}

/// Bitwarden vault status.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BwStatus {
    Unlocked,
    Locked,
    NotAuthenticated,
    NotInstalled,
}

/// Parse the Bitwarden vault status from `bw status` JSON output.
fn parse_bw_status(stdout: &str) -> BwStatus {
    if let Some(status) = stdout
        .split("\"status\":")
        .nth(1)
        .and_then(|s| s.split('"').nth(1))
    {
        match status {
            "unlocked" => BwStatus::Unlocked,
            "locked" => BwStatus::Locked,
            "unauthenticated" => BwStatus::NotAuthenticated,
            _ => BwStatus::Locked,
        }
    } else {
        BwStatus::NotInstalled
    }
}

/// Check the Bitwarden vault status by running `bw status`.
pub fn bw_vault_status(env: &crate::runtime::env::Env) -> BwStatus {
    let output = match env.command("bw").arg("status").output() {
        Ok(o) => o,
        Err(_) => return BwStatus::NotInstalled,
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_bw_status(&stdout)
}

/// Unlock the Bitwarden vault with the given master password.
/// Passes the password via env var to avoid exposure in `ps` output.
/// Returns the session token on success.
pub fn bw_unlock(env: &crate::runtime::env::Env, password: &str) -> Result<String> {
    let output = env
        .command("bw")
        .args(["unlock", "--passwordenv", "PURPLE_BW_MASTER", "--raw"])
        .env("PURPLE_BW_MASTER", password)
        .output()
        .context("Failed to run Bitwarden CLI (bw)")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Bitwarden unlock failed: {}", stderr.trim());
    }
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if token.is_empty() {
        anyhow::bail!("Bitwarden unlock returned empty session token");
    }
    Ok(token)
}

/// Proton Pass CLI authentication status.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProtonStatus {
    Authenticated,
    NotAuthenticated,
    NotInstalled,
}

/// Check whether `pass-cli` is installed and the user is logged in. Uses
/// `pass-cli test` (not `info`) because in pass-cli 2.x `info` exits 0 even
/// without a session and only reports the error on stderr. `test` is the
/// command that actually exits non-zero when authentication is missing.
pub fn proton_status(env: &crate::runtime::env::Env) -> ProtonStatus {
    let result = env.command("pass-cli").arg("test").output();
    let status = match result {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => ProtonStatus::NotInstalled,
        Err(_) => ProtonStatus::NotAuthenticated,
        Ok(out) if out.status.success() => ProtonStatus::Authenticated,
        Ok(_) => ProtonStatus::NotAuthenticated,
    };
    debug!("[external] Proton Pass status: {status:?}");
    status
}

/// Log in to Proton Pass with a Personal Access Token.
/// PAT is supplied via the `PROTON_PASS_PERSONAL_ACCESS_TOKEN` env var so it
/// never appears in argv. Returns an error wrapping pass-cli's stderr on
/// non-zero exit so the prompt loop can surface it.
pub fn proton_login(env: &crate::runtime::env::Env, pat: &str) -> Result<()> {
    if pat.is_empty() {
        anyhow::bail!("empty PAT");
    }
    let output = env
        .command("pass-cli")
        .arg("login")
        .env("PROTON_PASS_PERSONAL_ACCESS_TOKEN", pat)
        .output()
        .context("Failed to run Proton Pass CLI (pass-cli)")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        debug!("[external] Proton Pass login failed: {}", stderr.trim());
        anyhow::bail!("{}", stderr.trim());
    }
    debug!("[external] Proton Pass login succeeded");
    Ok(())
}

/// Parse a `proton:Vault/Item/field` askpass spec into its three components.
/// Vault and item segments cannot contain `/`; the field segment is everything
/// after the second `/`. All three segments must be non-empty.
fn parse_proton_spec(spec: &str) -> Result<(&str, &str, &str)> {
    let (vault, rest) = spec
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("Proton Pass spec must be Vault/Item/field"))?;
    let (item, field) = rest
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("Proton Pass spec must be Vault/Item/field"))?;
    if vault.is_empty() || item.is_empty() || field.is_empty() {
        anyhow::bail!("Proton Pass spec segments must be non-empty");
    }
    Ok((vault, item, field))
}

/// Retrieve a secret from Proton Pass via `pass-cli item view`. The askpass
/// spec `proton:Vault/Item/field` is mapped to name-based lookup flags
/// (`--vault-name`, `--item-title`, `--field`) rather than the URI form, so
/// purple users can refer to their vaults and items by human-readable names
/// instead of opaque share/item IDs.
fn retrieve_from_proton_pass(env: &crate::runtime::env::Env, spec: &str) -> Result<String> {
    let (vault, item, field) = parse_proton_spec(spec)?;
    let result = env
        .command("pass-cli")
        .args([
            "item",
            "view",
            "--vault-name",
            vault,
            "--item-title",
            item,
            "--field",
            field,
        ])
        .output();
    let output = match result {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            error!("[config] Password manager binary not found: pass-cli");
            return Err(e).context("Failed to run Proton Pass CLI (pass-cli)");
        }
        other => other.context("Failed to run Proton Pass CLI (pass-cli)")?,
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("[external] Proton Pass lookup failed: {}", stderr.trim());
        anyhow::bail!("Proton Pass lookup failed");
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        warn!("[external] Proton Pass returned empty secret");
        anyhow::bail!("Proton Pass returned empty secret");
    }
    debug!("[external] Proton Pass lookup succeeded");
    Ok(value)
}

#[cfg(test)]
#[path = "askpass_tests.rs"]
mod tests;

#[cfg(test)]
mod host_trust_tests {
    use super::*;

    #[test]
    fn fingerprint_confirmation_requires_the_complete_digest() {
        let fingerprint = format!("SHA256:{}", "A".repeat(43));
        let prompt =
            format!("ED25519 key fingerprint is {fingerprint}.\nContinue (yes/no/[fingerprint])? ");
        assert_eq!(host_key_answer(&prompt, &fingerprint), fingerprint);
        for input in ["SHA256:", "SHA256:AAAA", "no", "NO", "", "yes please"] {
            assert_eq!(host_key_answer(&prompt, input), "no");
        }
        for input in ["yes", "Yes", "YES", " yes "] {
            assert_eq!(host_key_answer(&prompt, input), "yes");
        }
        assert_eq!(
            host_key_answer("Please type 'yes' or 'no': ", &fingerprint),
            "no"
        );
    }

    #[test]
    fn trust_retries_are_never_password_prompts() {
        for prompt in [
            "Continue (yes/no)?",
            "Please type 'yes', 'no' or the fingerprint: ",
            "Please type 'yes' or 'no': ",
        ] {
            assert!(is_host_key_prompt(prompt));
        }
        assert!(!is_host_key_prompt("root@example.test's password: "));
    }
}
