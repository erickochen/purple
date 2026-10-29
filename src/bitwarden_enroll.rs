//! First-login enrollment for an explicitly selected but unbound `bw:` source.
//! Secrets only travel over stdin/stdout pipes. The recovery journal holds IDs,
//! never passwords; ambiguous create outcomes are reconciled, never replayed.
use crate::{runtime::env::Env, ssh_config::model::SshConfigFile};
use anyhow::{Context, Result, bail, ensure};
use base64::Engine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{io::Write, path::Path, process::Stdio};

trait Vault {
    fn call(&self, args: &[&str], body: Option<&Value>) -> Result<Value>;
}
struct Cli<'a> {
    env: &'a Env,
    session: Option<&'a str>,
}
impl Vault for Cli<'_> {
    fn call(&self, args: &[&str], body: Option<&Value>) -> Result<Value> {
        let started = std::time::Instant::now();
        let mut command = self.env.command("bw");
        command
            .args(args)
            .env("BW_NOINTERACTION", "true")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(token) = self.session {
            command.env("BW_SESSION", token);
        }
        let mut child = command
            .spawn()
            .context(crate::messages::bitwarden::CLI_START)?;
        if let Some(body) = body {
            let encoded =
                base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(body)?);
            child
                .stdin
                .take()
                .context("Bitwarden stdin unavailable")?
                .write_all(encoded.as_bytes())?;
        } else {
            drop(child.stdin.take());
        }
        let output = child.wait_with_output()?;
        log::info!(
            "[bitwarden] operation={} elapsed_ms={} success={}",
            args.first().copied().unwrap_or("unknown"),
            started.elapsed().as_millis(),
            output.status.success()
        );
        // Never log CLI output: a create response includes the password.
        ensure!(
            output.status.success(),
            crate::messages::bitwarden::operation_failed(
                args.first().copied().unwrap_or("unknown"),
                output.status.code()
            )
        );
        if args == ["sync"] {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&output.stdout).context(crate::messages::bitwarden::INVALID_RESPONSE)
    }
}
fn id(value: &Value) -> Result<String> {
    let id = value["id"]
        .as_str()
        .context(crate::messages::bitwarden::MISSING_ID)?;
    ensure!(
        id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'),
        crate::messages::bitwarden::INVALID_ID
    );
    Ok(id.to_string())
}
fn journal_write(path: &Path, identity: &str, stage: &str, id: Option<&str>) -> Result<()> {
    crate::fs_util::write_json_pretty(path, &json!({"identity":identity,"stage":stage,"id":id}))?;
    Ok(())
}
fn enroll(
    vault: &dyn Vault,
    config_path: &Path,
    alias: &str,
    journal: &Path,
    prompt: impl FnOnce() -> Result<Option<String>>,
) -> Result<Option<String>> {
    ensure!(
        !crate::demo_flag::is_demo(),
        crate::messages::bitwarden::DEMO_DISABLED
    );
    let initial = std::fs::read(config_path)?;
    let mut config = SshConfigFile::parse(config_path)?;
    let host = config
        .host_entries()
        .into_iter()
        .find(|h| h.alias == alias)
        .context(crate::messages::bitwarden::HOST_MISSING)?;
    ensure!(
        host.askpass.as_deref().is_none_or(|s| s.trim() == "bw:"),
        crate::messages::bitwarden::SOURCE_CHANGED
    );
    let identity = serde_json::to_string(&json!([alias, host.hostname, host.user, host.port]))?;
    if let Some(parent) = journal.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _lock = crate::fs_util::FileLock::acquire(&journal.with_file_name("operations"))?;
    let recovery: Option<Value> = if journal.exists() {
        Some(serde_json::from_slice(&std::fs::read(journal)?)?)
    } else {
        None
    };
    if let Some(ref old) = recovery {
        ensure!(
            old["identity"] == identity,
            crate::messages::bitwarden::IDENTITY_CHANGED
        );
    }
    // The interactive caller already checked/unlocked the vault. Sync and
    // decrypted item lookup fail closed if that session is no longer usable.
    vault.call(&["sync"], None)?;
    let items = vault.call(&["list", "items", "--search", alias], None)?;
    let items = items
        .as_array()
        .context(crate::messages::bitwarden::INVALID_ITEMS)?;
    let matches: Vec<_> = items
        .iter()
        .filter(|i| {
            i["deletedDate"].is_null()
                && i["fields"].as_array().is_some_and(|fields| {
                    fields
                        .iter()
                        .any(|f| f["name"] == "purple_host_identity" && f["value"] == identity)
                })
        })
        .collect();
    ensure!(
        matches.len() <= 1,
        crate::messages::bitwarden::DUPLICATE_ITEMS
    );
    let item_id = if let Some(item) = matches.first() {
        ensure!(
            item["login"]["username"] == host.user
                && item["login"]["password"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty()),
            crate::messages::bitwarden::INCOMPLETE_ITEM
        );
        id(item)?
    } else if recovery
        .as_ref()
        .is_some_and(|r| r["stage"] != "creating_folder")
    {
        bail!(crate::messages::bitwarden::AMBIGUOUS_CREATE)
    } else {
        if recovery.is_some() {
            let folders = vault.call(&["list", "folders"], None)?;
            ensure!(
                folders.as_array().is_some_and(|f| f
                    .iter()
                    .filter(|x| x["name"] == "ssh/Purple")
                    .count()
                    == 1),
                crate::messages::bitwarden::AMBIGUOUS_FOLDER
            );
            std::fs::remove_file(journal)?;
        }
        ensure!(
            !host.user.is_empty() && !host.hostname.is_empty(),
            crate::messages::bitwarden::MISSING_HOST_FIELDS
        );
        let name = format!("Purple / {alias}");
        ensure!(
            !items.iter().any(|i| i["deletedDate"].is_null()
                && i["name"].as_str().is_some_and(|n| n.trim() == name)),
            crate::messages::bitwarden::NAME_COLLISION
        );
        let Some(password) = prompt()? else {
            return Ok(None);
        };
        ensure!(
            !password.is_empty(),
            crate::messages::bitwarden::EMPTY_PASSWORD
        );
        let folders = vault.call(&["list", "folders"], None)?;
        let folders: Vec<_> = folders
            .as_array()
            .context(crate::messages::bitwarden::INVALID_FOLDERS)?
            .iter()
            .filter(|f| f["name"] == "ssh/Purple")
            .collect();
        ensure!(
            folders.len() <= 1,
            crate::messages::bitwarden::DUPLICATE_FOLDERS
        );
        let folder_id = if let Some(folder) = folders.first() {
            id(folder)?
        } else {
            journal_write(journal, &identity, "creating_folder", None)?;
            let folder = vault.call(&["create", "folder"], Some(&json!({"name":"ssh/Purple"})))?;
            let folder_id = id(&folder)?;
            // Folder was confirmed, so no unknown operation remains.
            std::fs::remove_file(journal)?;
            folder_id
        };
        journal_write(journal, &identity, "creating_item", None)?;
        let item = vault.call(&["create","item"],Some(&json!({
            "type":1,"name":name,"folderId":folder_id,"organizationId":null,"favorite":false,"reprompt":0,
            "notes":"Created by Purple on first password login.",
            "fields":[{"name":"purple_host_identity","value":identity,"type":0}],
            "login":{"username":host.user,"password":password,"totp":null,"uris":[{"uri":format!("ssh://{}:{}",host.hostname,host.port),"match":null}]}
        })))?;
        let item_id = id(&item)?;
        journal_write(journal, &identity, "created", Some(&item_id))?;
        item_id
    };
    // `bw create item` waits for the server response and updates the CLI's
    // local vault. Re-read that saved item; a second full-vault sync adds a
    // network round trip without strengthening an acknowledged create.
    // Recovery still begins with sync above, and ambiguous creates never bind.
    let item = vault.call(&["get", "item", &item_id], None)?;
    ensure!(
        id(&item)? == item_id
            && item["deletedDate"].is_null()
            && item["login"]["username"] == host.user
            && item["fields"].as_array().is_some_and(|fields| fields
                .iter()
                .any(|f| f["name"] == "purple_host_identity" && f["value"] == identity))
            && item["login"]["password"]
                .as_str()
                .is_some_and(|p| !p.is_empty()),
        crate::messages::bitwarden::READBACK_FAILED
    );
    ensure!(
        std::fs::read(config_path)? == initial,
        crate::messages::bitwarden::CONFIG_CHANGED
    );
    let source = format!("bw:{item_id}");
    ensure!(
        config.set_host_askpass(alias, &source),
        crate::messages::bitwarden::BIND_FAILED
    );
    config.write()?;
    let saved = SshConfigFile::parse(config_path)?
        .host_entries()
        .into_iter()
        .find(|h| h.alias == alias);
    ensure!(
        saved.as_ref().and_then(|h| h.askpass.as_deref()) == Some(source.as_str()),
        crate::messages::bitwarden::BIND_VERIFY_FAILED
    );
    if journal.exists() {
        std::fs::remove_file(journal)?;
    }
    Ok(Some(source))
}
#[derive(serde::Serialize, serde::Deserialize)]
struct PendingRename {
    config: String,
    item_id: String,
    old_identity: String,
    new_identity: String,
    old_name: String,
    new_name: String,
    #[serde(default)]
    previous: Vec<(String, String)>,
}

fn host_identity(host: &crate::ssh_config::model::HostEntry) -> Result<String> {
    Ok(serde_json::to_string(&json!([
        host.alias,
        host.hostname,
        host.user,
        host.port
    ]))?)
}

fn rename_path(env: &Env, item_id: &str) -> Result<std::path::PathBuf> {
    id(&json!({"id": item_id}))?;
    Ok(env
        .paths()
        .context(crate::messages::bitwarden::MISSING_PATHS)?
        .state_dir()
        .join("bitwarden-renames")
        .join(format!("{item_id}.json")))
}

/// Queue metadata only; never copy passwords into the recovery journal.
pub fn queue_rename(
    env: &Env,
    config: &Path,
    old: &crate::ssh_config::model::HostEntry,
    new: &crate::ssh_config::model::HostEntry,
) -> Result<bool> {
    if old.alias == new.alias || old.askpass != new.askpass {
        return Ok(false);
    }
    let Some(item_id) = old.askpass.as_deref().and_then(|s| s.strip_prefix("bw:")) else {
        return Ok(false);
    };
    if item_id.is_empty() {
        return Ok(false);
    }
    let path = rename_path(env, item_id)?;
    std::fs::create_dir_all(
        path.parent()
            .context(crate::messages::bitwarden::INVALID_SYNC_PATH)?,
    )?;
    let _lock = crate::fs_util::FileLock::acquire(&path.with_extension("lock"))?;
    let mut pending = if path.exists() {
        let mut pending: PendingRename = serde_json::from_slice(&std::fs::read(&path)?)?;
        ensure!(
            pending.config == config.to_string_lossy()
                && pending.new_identity == host_identity(old)?,
            crate::messages::bitwarden::RENAME_CONFLICT
        );
        pending
            .previous
            .push((pending.new_identity.clone(), pending.new_name.clone()));
        pending
    } else {
        PendingRename {
            config: config.to_string_lossy().into_owned(),
            item_id: item_id.into(),
            old_identity: host_identity(old)?,
            new_identity: String::new(),
            old_name: format!("Purple / {}", old.alias),
            new_name: String::new(),
            previous: Vec::new(),
        }
    };
    pending.new_identity = host_identity(new)?;
    pending.new_name = format!("Purple / {}", new.alias);
    crate::fs_util::write_json_pretty(&path, &pending)?;
    Ok(true)
}

fn apply_rename(vault: &dyn Vault, pending: &PendingRename) -> Result<bool> {
    vault.call(&["sync"], None)?;
    let mut item = vault.call(&["get", "item", &pending.item_id], None)?;
    ensure!(
        id(&item)? == pending.item_id && item["deletedDate"].is_null(),
        crate::messages::bitwarden::ITEM_CHANGED
    );
    let marker = item["fields"].as_array().and_then(|fields| {
        fields
            .iter()
            .position(|f| f["name"] == "purple_host_identity")
    });
    let Some(marker) = marker else {
        return Ok(false);
    };
    let identity = item["fields"][marker]["value"].as_str().unwrap_or("");
    ensure!(
        identity == pending.old_identity
            || identity == pending.new_identity
            || pending.previous.iter().any(|(value, _)| value == identity),
        crate::messages::bitwarden::OWNER_MISMATCH
    );
    if identity == pending.new_identity && item["name"] == pending.new_name {
        return Ok(true);
    }
    ensure!(
        item["name"] == pending.old_name
            || item["name"] == pending.new_name
            || pending
                .previous
                .iter()
                .any(|(_, name)| item["name"] == *name),
        crate::messages::bitwarden::NAME_EDITED
    );
    item["name"] = json!(pending.new_name);
    item["fields"][marker]["value"] = json!(pending.new_identity);
    // Edit the freshly synced complete item, preserving every credential field.
    vault.call(&["edit", "item", &pending.item_id], Some(&item))?;
    let saved = vault.call(&["get", "item", &pending.item_id], None)?;
    ensure!(
        id(&saved)? == pending.item_id
            && saved["name"] == pending.new_name
            && saved["login"] == item["login"]
            && saved["fields"] == item["fields"],
        crate::messages::bitwarden::RENAME_VERIFY_FAILED
    );
    Ok(true)
}

/// Retry only explicitly queued metadata changes, after a fresh binding check.
pub fn sync_pending_rename(
    env: &Env,
    config: &Path,
    alias: &str,
    session: Option<&str>,
) -> Result<Option<&'static str>> {
    let host = SshConfigFile::parse(config)?
        .host_entries()
        .into_iter()
        .find(|h| h.alias == alias)
        .context(crate::messages::bitwarden::HOST_MISSING)?;
    let Some(item_id) = host.askpass.as_deref().and_then(|s| s.strip_prefix("bw:")) else {
        return Ok(None);
    };
    if item_id.is_empty() {
        return Ok(None);
    }
    let path = rename_path(env, item_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let _lock = crate::fs_util::FileLock::acquire(&path.with_extension("lock"))?;
    let pending: PendingRename = serde_json::from_slice(&std::fs::read(&path)?)?;
    ensure!(
        pending.item_id == item_id
            && pending.config == config.to_string_lossy()
            && pending.new_identity == host_identity(&host)?,
        crate::messages::bitwarden::BINDING_CHANGED
    );
    eprintln!("{}", crate::messages::bitwarden::SYNC_RENAME);
    let managed = apply_rename(&Cli { env, session }, &pending)?;
    std::fs::remove_file(path)?;
    Ok(Some(if managed {
        crate::messages::bitwarden::NAME_SYNCED
    } else {
        crate::messages::bitwarden::EXTERNAL_ITEM
    }))
}

/// Called only from interactive connection paths, after the normal vault unlock.
pub fn prepare(
    env: &Env,
    config: &Path,
    alias: &str,
    source: &mut Option<String>,
    session: Option<&str>,
) -> Result<bool> {
    if source.as_deref().is_none_or(|s| s.trim() != "bw:") {
        if source.as_deref().is_some_and(|s| s.starts_with("bw:")) {
            match sync_pending_rename(env, config, alias, session) {
                Ok(Some(message)) => eprintln!("{message}"),
                Err(e) => eprintln!("{}", crate::messages::bitwarden::rename_pending(&e)),
                Ok(None) => {}
            }
        }
        return Ok(true);
    }
    let paths = env
        .paths()
        .context(crate::messages::bitwarden::MISSING_PATHS)?;
    let key = format!(
        "{:x}",
        Sha256::digest(format!("{}:{alias}", config.display()).as_bytes())
    );
    let journal = paths
        .state_dir()
        .join("bitwarden-enrollment")
        .join(format!("{key}.json"));
    let vault = Cli { env, session };
    eprintln!("{}", crate::messages::bitwarden::CHECKING_ITEMS);
    let result = enroll(&vault, config, alias, &journal, || {
        eprintln!("{}", crate::messages::bitwarden::saving(alias));
        crate::cli::prompt_hidden_input(crate::messages::bitwarden::PASSWORD_PROMPT)
    })?;
    if let Some(binding) = result {
        *source = Some(binding);
        eprintln!("{}", crate::messages::bitwarden::PASSWORD_BOUND);
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    const ITEM: &str = "11111111-1111-4111-8111-111111111111";
    const FOLDER: &str = "22222222-2222-4222-8222-222222222222";
    struct Fake {
        replies: RefCell<VecDeque<(&'static str, Result<Value>)>>,
        bodies: RefCell<Vec<Value>>,
    }
    impl Fake {
        fn new(replies: Vec<(&'static str, Result<Value>)>) -> Self {
            Self {
                replies: RefCell::new(replies.into()),
                bodies: RefCell::new(vec![]),
            }
        }
    }
    impl Vault for Fake {
        fn call(&self, args: &[&str], body: Option<&Value>) -> Result<Value> {
            let (expected, result) = self
                .replies
                .borrow_mut()
                .pop_front()
                .expect("unexpected vault call");
            assert_eq!(args.join(" "), expected);
            if let Some(b) = body {
                self.bodies.borrow_mut().push(b.clone());
            }
            result
        }
    }
    fn setup() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        std::fs::write(
            &config,
            "Host test\n  HostName example.test\n  User root\n  # purple:askpass bw:\n",
        )
        .unwrap();
        let journal = dir.path().join("journal.json");
        (dir, config, journal)
    }
    fn initial(items: Value) -> Vec<(&'static str, Result<Value>)> {
        vec![
            ("sync", Ok(Value::Null)),
            ("list items --search test", Ok(items)),
        ]
    }
    fn item() -> Value {
        json!({"id":ITEM,"name":"Purple / test","login":{"username":"root","password":"secret-test-value"},"fields":[{"name":"purple_host_identity","value":"[\"test\",\"example.test\",\"root\",22]"}]})
    }
    fn create_steps() -> Vec<(&'static str, Result<Value>)> {
        let mut steps = initial(json!([]));
        steps.extend([
            (
                "list folders",
                Ok(json!([{"id":FOLDER,"name":"ssh/Purple"}])),
            ),
            ("create item", Ok(item())),
            ("get item 11111111-1111-4111-8111-111111111111", Ok(item())),
        ]);
        steps
    }
    #[test]
    fn create_and_bind_only_after_verified_save() {
        let _guard = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::demo_flag::disable();
        let (_d, c, j) = setup();
        let vault = Fake::new(create_steps());
        assert_eq!(
            enroll(&vault, &c, "test", &j, || Ok(Some(
                "secret-test-value".into()
            )))
            .unwrap(),
            Some(format!("bw:{ITEM}"))
        );
        assert!(std::fs::read_to_string(&c).unwrap().contains(ITEM));
        assert!(!j.exists());
        assert!(
            !std::fs::read_to_string(&c)
                .unwrap()
                .contains("secret-test-value")
        );
        let bodies = vault.bodies.borrow();
        assert_eq!(bodies[0]["folderId"], FOLDER);
        assert_eq!(bodies[0]["login"]["username"], "root");
    }
    #[test]
    fn cancel_does_not_create_or_edit() {
        let _guard = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::demo_flag::disable();
        let (_d, c, j) = setup();
        let before = std::fs::read(&c).unwrap();
        let vault = Fake::new(initial(json!([])));
        assert!(
            enroll(&vault, &c, "test", &j, || Ok(None))
                .unwrap()
                .is_none()
        );
        assert_eq!(before, std::fs::read(c).unwrap());
        assert!(!j.exists());
        assert!(vault.bodies.borrow().is_empty());
    }
    #[test]
    fn same_name_without_identity_is_not_overwritten() {
        let _guard = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::demo_flag::disable();
        let (_d, c, j) = setup();
        let vault = Fake::new(initial(json!([{"name":"Purple / test"}])));
        assert!(enroll(&vault, &c, "test", &j, || panic!("must not prompt")).is_err());
        assert!(vault.bodies.borrow().is_empty());
    }
    #[test]
    fn unknown_create_outcome_is_not_replayed() {
        let _guard = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::demo_flag::disable();
        let (_d, c, j) = setup();
        let before = std::fs::read(&c).unwrap();
        let mut steps = initial(json!([]));
        steps.extend([
            (
                "list folders",
                Ok(json!([{"id":FOLDER,"name":"ssh/Purple"}])),
            ),
            ("create item", Err(anyhow::anyhow!("network disconnected"))),
        ]);
        assert!(
            enroll(&Fake::new(steps), &c, "test", &j, || Ok(Some(
                "secret-test-value".into()
            )))
            .is_err()
        );
        assert!(
            !std::fs::read_to_string(&j)
                .unwrap()
                .contains("secret-test-value")
        );
        let retry = Fake::new(initial(json!([])));
        assert!(enroll(&retry, &c, "test", &j, || panic!("no replay")).is_err());
        assert_eq!(before, std::fs::read(&c).unwrap());
        let mut recovered = initial(json!([item()]));
        recovered.extend([("get item 11111111-1111-4111-8111-111111111111", Ok(item()))]);
        assert!(
            enroll(&Fake::new(recovered), &c, "test", &j, || panic!(
                "already stored"
            ))
            .unwrap()
            .is_some()
        );
    }
    #[test]
    fn external_edit_is_preserved_after_remote_save() {
        let _guard = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::demo_flag::disable();
        let (_d, c, j) = setup();
        let vault = Fake::new(create_steps());
        let edited = "Host test\n HostName changed.test\n User root\n # purple:askpass bw:\n";
        assert!(
            enroll(&vault, &c, "test", &j, || {
                std::fs::write(&c, edited).unwrap();
                Ok(Some("secret-test-value".into()))
            })
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(c).unwrap(), edited);
        assert!(j.exists());
    }
    #[test]
    fn locked_vault_never_asks_for_server_password() {
        let _guard = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::demo_flag::disable();
        let (_d, c, j) = setup();
        let vault = Fake::new(vec![
            ("sync", Ok(Value::Null)),
            (
                "list items --search test",
                Err(anyhow::anyhow!("vault locked")),
            ),
        ]);
        assert!(enroll(&vault, &c, "test", &j, || panic!("locked")).is_err());
        assert!(vault.bodies.borrow().is_empty());
    }
    #[test]
    fn sync_failure_keeps_config_unbound() {
        let _guard = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::demo_flag::disable();
        let (_d, c, j) = setup();
        let before = std::fs::read(&c).unwrap();
        let mut steps = create_steps();
        steps.clear();
        steps.push(("sync", Err(anyhow::anyhow!("offline"))));
        assert!(
            enroll(&Fake::new(steps), &c, "test", &j, || Ok(Some(
                "secret-test-value".into()
            )))
            .is_err()
        );
        assert_eq!(before, std::fs::read(c).unwrap());
        assert!(!j.exists());
    }
    #[test]
    fn invalid_readback_preserves_recovery_without_binding() {
        let _guard = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::demo_flag::disable();
        for field in ["password", "username", "identity"] {
            let (_d, c, j) = setup();
            let before = std::fs::read(&c).unwrap();
            let mut bad = item();
            match field {
                "password" => bad["login"]["password"] = json!(""),
                "username" => bad["login"]["username"] = json!("other"),
                _ => bad["fields"] = json!([]),
            }
            let mut steps = create_steps();
            steps.last_mut().unwrap().1 = Ok(bad);
            assert!(
                enroll(&Fake::new(steps), &c, "test", &j, || Ok(Some(
                    "secret-test-value".into()
                )))
                .is_err()
            );
            assert_eq!(before, std::fs::read(&c).unwrap());
            assert!(j.exists());
        }
    }

    fn pending_rename() -> PendingRename {
        PendingRename {
            config: "/tmp/test-config".into(),
            item_id: ITEM.into(),
            old_identity: item()["fields"][0]["value"].as_str().unwrap().into(),
            new_identity: "[\"renamed\",\"example.test\",\"root\",22]".into(),
            old_name: "Purple / test".into(),
            new_name: "Purple / renamed".into(),
            previous: Vec::new(),
        }
    }

    #[test]
    fn rename_preserves_credentials_and_retries_idempotently() {
        let pending = pending_rename();
        let mut before = item();
        before["login"]["totp"] = json!("synthetic-totp");
        before["notes"] = json!("keep this note");
        let mut after = before.clone();
        after["name"] = json!(pending.new_name);
        after["fields"][0]["value"] = json!(pending.new_identity);
        let vault = Fake::new(vec![
            ("sync", Ok(Value::Null)),
            (
                "get item 11111111-1111-4111-8111-111111111111",
                Ok(before.clone()),
            ),
            (
                "edit item 11111111-1111-4111-8111-111111111111",
                Ok(after.clone()),
            ),
            (
                "get item 11111111-1111-4111-8111-111111111111",
                Ok(after.clone()),
            ),
        ]);
        assert!(apply_rename(&vault, &pending).unwrap());
        let written = &vault.bodies.borrow()[0];
        assert_eq!(written["login"], before["login"]);
        assert_eq!(written["notes"], before["notes"]);
        let retry = Fake::new(vec![
            ("sync", Ok(Value::Null)),
            ("get item 11111111-1111-4111-8111-111111111111", Ok(after)),
        ]);
        assert!(apply_rename(&retry, &pending).unwrap());
        assert!(retry.bodies.borrow().is_empty());
    }

    #[test]
    fn chained_rename_accepts_an_ambiguous_intermediate_save() {
        let mut pending = pending_rename();
        let intermediate = "[\"middle\",\"example.test\",\"root\",22]";
        pending
            .previous
            .push((intermediate.into(), "Purple / middle".into()));
        let mut before = item();
        before["name"] = json!("Purple / middle");
        before["fields"][0]["value"] = json!(intermediate);
        let mut after = before.clone();
        after["name"] = json!(pending.new_name);
        after["fields"][0]["value"] = json!(pending.new_identity);
        let vault = Fake::new(vec![
            ("sync", Ok(Value::Null)),
            ("get item 11111111-1111-4111-8111-111111111111", Ok(before)),
            (
                "edit item 11111111-1111-4111-8111-111111111111",
                Ok(after.clone()),
            ),
            ("get item 11111111-1111-4111-8111-111111111111", Ok(after)),
        ]);
        assert!(apply_rename(&vault, &pending).unwrap());
    }

    #[test]
    fn rename_never_overwrites_foreign_or_manually_renamed_items() {
        for kind in ["unmanaged", "foreign", "manual"] {
            let mut value = item();
            match kind {
                "unmanaged" => value["fields"] = json!([]),
                "foreign" => value["fields"][0]["value"] = json!("unrelated"),
                _ => value["name"] = json!("My manual name"),
            }
            let vault = Fake::new(vec![
                ("sync", Ok(Value::Null)),
                ("get item 11111111-1111-4111-8111-111111111111", Ok(value)),
            ]);
            let result = apply_rename(&vault, &pending_rename());
            if kind == "unmanaged" {
                assert!(!result.unwrap());
            } else {
                assert!(result.is_err());
            }
            assert!(vault.bodies.borrow().is_empty());
        }
    }

    #[test]
    fn pending_rename_coalesces_and_contains_no_password() {
        let (dir, config, _) = setup();
        let env = Env::for_test(dir.path());
        let mut old = SshConfigFile::parse(&config)
            .unwrap()
            .host_entries()
            .remove(0);
        old.askpass = Some(format!("bw:{ITEM}"));
        let mut next = old.clone();
        next.alias = "second".into();
        assert!(queue_rename(&env, &config, &old, &next).unwrap());
        let mut latest = next.clone();
        latest.alias = "third".into();
        assert!(queue_rename(&env, &config, &next, &latest).unwrap());
        let raw = std::fs::read_to_string(rename_path(&env, ITEM).unwrap()).unwrap();
        let pending: PendingRename = serde_json::from_str(&raw).unwrap();
        assert_eq!(pending.old_name, "Purple / test");
        assert_eq!(pending.new_name, "Purple / third");
        assert!(!raw.contains("password"));
        let mut other = old.clone();
        other.alias = "other".into();
        assert!(queue_rename(&env, &config, &other, &latest).is_err());
    }

    #[test]
    fn invalid_item_id_never_becomes_config() {
        assert!(id(&json!({"id":"x\nHost evil"})).is_err());
    }
}
