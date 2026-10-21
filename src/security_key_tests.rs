use super::*;
use crossterm::event::{KeyEventState, KeyModifiers};

fn pin(key_path: &str, value: &str) -> SecurityKeyPin {
    SecurityKeyPin {
        key_path: key_path.to_string(),
        pin: value.to_string(),
    }
}

fn scratch_paths() -> (tempfile::TempDir, Paths) {
    let dir = tempfile::tempdir().expect("tempdir");
    let paths = Paths::new(dir.path());
    (dir, paths)
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

// --- parse_pin_prompt ---

#[test]
fn reads_the_prompt_ssh_shows_for_a_key_file() {
    let p = parse_pin_prompt("Enter PIN for ED25519-SK key /home/u/.ssh/id_ed25519_sk: ")
        .expect("pin prompt");
    assert_eq!(p.key_type, "ED25519-SK");
    assert_eq!(p.key_path, "/home/u/.ssh/id_ed25519_sk");
    let p = parse_pin_prompt("Enter PIN for ECDSA-SK key /Users/u/.ssh/personal_sk: ")
        .expect("pin prompt");
    assert_eq!(p.key_path, "/Users/u/.ssh/personal_sk");
}

#[test]
fn keeps_a_key_path_with_spaces_whole() {
    let p = parse_pin_prompt("Enter PIN for ECDSA-SK key /Users/a b/.ssh/my key: ")
        .expect("pin prompt");
    assert_eq!(p.key_path, "/Users/a b/.ssh/my key");
}

#[test]
fn reads_the_prompt_for_a_certificate_identity() {
    let p = parse_pin_prompt("Enter PIN for ED25519-SK-CERT key /home/u/.ssh/id_sk: ")
        .expect("pin prompt");
    assert_eq!(p.key_type, "ED25519-SK-CERT");
}

#[test]
fn a_keyboard_interactive_prompt_is_not_a_pin_prompt() {
    // OpenSSH 8.5+ prefixes every server prompt with `(user@host) `, so a
    // server cannot dress one up as a PIN prompt.
    assert!(
        parse_pin_prompt("(root@web) Enter PIN for ED25519-SK key /home/u/.ssh/id_sk: ").is_none()
    );
}

#[test]
fn the_agent_prompt_and_fingerprints_are_not_key_files() {
    assert!(
        parse_pin_prompt("Enter PIN and confirm user presence for ED25519-SK key SHA256:abc: ")
            .is_none()
    );
    assert!(parse_pin_prompt("Enter PIN for ED25519-SK key SHA256:abcdef: ").is_none());
    assert!(parse_pin_prompt("Enter PIN for ED25519-SK key MD5:ab:cd: ").is_none());
}

#[test]
fn other_prompts_are_not_pin_prompts() {
    for prompt in [
        "Enter PIN for ED25519 key /home/u/.ssh/id_ed25519: ",
        "Enter passphrase for key '/home/u/.ssh/id_ed25519': ",
        "u@host's password: ",
        "Enter PIN for ED25519-SK key : ",
        "Enter PIN for ED25519-SK key /home/u/x\nPassword: ",
        "",
    ] {
        assert!(parse_pin_prompt(prompt).is_none(), "{prompt:?}");
    }
}

// --- parse_sign_failure ---

#[test]
fn reads_a_wrong_pin_from_the_line_ssh_prints() {
    let f = parse_sign_failure(
        "sign_and_send_pubkey: signing failed for ECDSA-SK \"/Users/u/.ssh/personal_sk\": incorrect passphrase supplied to decrypt private key",
    )
    .expect("failure");
    assert_eq!(f.key_path, "/Users/u/.ssh/personal_sk");
    assert_eq!(f.reason, RefusalReason::WrongPin);
}

#[test]
fn tells_the_refusal_reasons_apart() {
    let line =
        |why: &str| format!("sign_and_send_pubkey: signing failed for ED25519-SK \"/k\": {why}");
    assert_eq!(
        parse_sign_failure(&line("invalid format")).map(|f| f.reason),
        Some(RefusalReason::NoAnswer)
    );
    assert_eq!(
        parse_sign_failure(&line("device not found")).map(|f| f.reason),
        Some(RefusalReason::NotFound)
    );
    assert_eq!(
        parse_sign_failure(&line("something else")).map(|f| f.reason),
        Some(RefusalReason::Other)
    );
}

#[test]
fn ignores_plain_keys_and_agent_keys() {
    assert!(
        parse_sign_failure(
            "sign_and_send_pubkey: signing failed for ED25519 \"/k\": incorrect passphrase supplied to decrypt private key"
        )
        .is_none()
    );
    assert!(
        parse_sign_failure(
            "sign_and_send_pubkey: signing failed for ED25519-SK \"/k\" from agent: agent refused operation"
        )
        .is_none()
    );
    assert!(parse_sign_failure("debug1: Offering public key: /k ED25519-SK").is_none());
}

#[test]
fn collects_every_failure_in_a_stderr_dump() {
    let stderr = "debug1: something\n\
        sign_and_send_pubkey: signing failed for ECDSA-SK \"/a\": incorrect passphrase supplied to decrypt private key\n\
        sign_and_send_pubkey: signing failed for ED25519-SK-CERT \"/b\": invalid format\n\
        user@host: Permission denied (publickey).";
    let failures = sign_failures(stderr);
    assert_eq!(failures.len(), 2);
    assert_eq!(failures[1].key_path, "/b");
}

// --- env wiring ---

fn env_of(cmd: &Command) -> Env {
    cmd.get_envs().fold(Env::empty(), |env, (k, v)| match v {
        Some(v) => env.with_var(k.to_string_lossy(), v.to_string_lossy()),
        None => env,
    })
}

#[test]
fn a_pin_round_trips_through_the_environment_for_its_own_key_only() {
    let mut cmd = Command::new("ssh");
    let run = SecurityKeyRun {
        pins: vec![pin("/a", "1111"), pin("/b", "2222")],
        flow: Some(PinFlow::FileBrowser),
    };
    configure(&mut cmd, &run);
    let env = env_of(&cmd);
    assert_eq!(pin_from_env(&env, "/a"), Some("1111"));
    assert_eq!(pin_from_env(&env, "/b"), Some("2222"));
    assert_eq!(pin_from_env(&env, "/c"), None);
    assert_eq!(flow_from_env(&env), Some(PinFlow::FileBrowser));
    assert!(cmd.get_args().all(|a| a != "1111" && a != "2222"));
}

#[test]
fn a_run_without_a_flow_sets_no_flow() {
    let mut cmd = Command::new("ssh");
    configure(&mut cmd, &SecurityKeyRun::default());
    assert_eq!(cmd.get_envs().count(), 0);
    assert_eq!(flow_from_env(&env_of(&cmd)), None);
}

#[test]
fn a_run_carries_a_bounded_number_of_pins() {
    let mut cmd = Command::new("ssh");
    let run = SecurityKeyRun {
        pins: (0..MAX_PINS_PER_RUN + 3)
            .map(|i| pin(&format!("/k{i}"), "1234"))
            .collect(),
        flow: None,
    };
    configure(&mut cmd, &run);
    assert_eq!(cmd.get_envs().count(), MAX_PINS_PER_RUN * 2);
}

#[test]
fn every_flow_survives_the_environment() {
    for flow in [PinFlow::Tunnel, PinFlow::FileBrowser, PinFlow::KeyPush] {
        let env = Env::empty().with_var(FLOW_VAR, flow.as_str());
        assert_eq!(flow_from_env(&env), Some(flow));
    }
    assert_eq!(flow_from_env(&Env::empty().with_var(FLOW_VAR, "x")), None);
}

#[test]
fn the_debug_output_never_shows_a_pin() {
    let text = format!(
        "{:?}",
        SecurityKeyRun {
            pins: vec![pin("/k", "98765")],
            flow: None,
        }
    );
    assert!(text.contains("/k"), "{text}");
    assert!(!text.contains("98765"), "{text}");
}

// --- OpenSSH gate ---

#[test]
fn only_an_openssh_that_prefixes_server_prompts_gets_pins() {
    assert!(ssh_version_allows_pins("OpenSSH_10.5p1, LibreSSL 3.3.6"));
    assert!(ssh_version_allows_pins("OpenSSH_8.5p1"));
    assert!(ssh_version_allows_pins(
        "OpenSSH_9.2p1 Debian-2+deb12u3, OpenSSL 3.0.15 3 Sep 2024"
    ));
    assert!(!ssh_version_allows_pins("OpenSSH_8.4p1 Debian-5+deb11u3"));
    assert!(!ssh_version_allows_pins("OpenSSH_7.9p1"));
    assert!(!ssh_version_allows_pins("unknown"));
    assert!(!ssh_version_allows_pins(""));
}

// --- markers ---

#[test]
fn a_refusal_is_recorded_read_and_cleared() {
    let (_dir, paths) = scratch_paths();
    assert_eq!(refusal_for(Some(&paths), "/k"), None);
    note_refusal(
        Some(&paths),
        &SignFailure {
            key_path: "/k".into(),
            reason: RefusalReason::NoAnswer,
        },
    );
    assert_eq!(
        refusal_for(Some(&paths), "/k"),
        Some(RefusalReason::NoAnswer)
    );
    assert_eq!(refusal_for(Some(&paths), "/other"), None);
    clear_refusal(Some(&paths), "/k");
    assert_eq!(refusal_for(Some(&paths), "/k"), None);
}

#[test]
fn only_a_refusal_of_a_carried_key_is_recorded() {
    let (_dir, paths) = scratch_paths();
    let run = SecurityKeyRun {
        pins: vec![pin("/carried", "1234")],
        flow: None,
    };
    let stderr = "sign_and_send_pubkey: signing failed for ED25519-SK \"/carried\": incorrect passphrase supplied to decrypt private key\n\
        sign_and_send_pubkey: signing failed for ED25519-SK \"/not-carried\": incorrect passphrase supplied to decrypt private key";
    let passed = scan_all(stderr, &paths, &run);
    assert_eq!(passed, stderr, "the stream passes through unchanged");
    assert_eq!(
        refusal_for(Some(&paths), "/carried"),
        Some(RefusalReason::WrongPin)
    );
    assert_eq!(refusal_for(Some(&paths), "/not-carried"), None);
}

#[test]
fn a_run_without_pins_records_nothing() {
    let (_dir, paths) = scratch_paths();
    let stderr = "sign_and_send_pubkey: signing failed for ED25519-SK \"/k\": incorrect passphrase supplied to decrypt private key\n";
    scan_all(stderr, &paths, &SecurityKeyRun::default());
    assert_eq!(refusal_for(Some(&paths), "/k"), None);
}

/// Read `stderr` through a refusal scan and return what came out.
fn scan_all(stderr: &str, paths: &Paths, run: &SecurityKeyRun) -> String {
    use std::io::Read;
    let mut scan = RefusalScan::new(stderr.as_bytes(), Some(paths), run);
    let mut out = String::new();
    scan.read_to_string(&mut out).expect("read");
    out
}

#[test]
fn a_refusal_is_on_record_as_soon_as_its_line_is_read() {
    // A jump host's refusal must land before the target asks for the PIN,
    // which happens while ssh is still writing.
    use std::io::Read;
    let (_dir, paths) = scratch_paths();
    let run = SecurityKeyRun {
        pins: vec![pin("/k", "1234")],
        flow: None,
    };
    let first = "sign_and_send_pubkey: signing failed for ED25519-SK \"/k\": incorrect passphrase supplied to decrypt private key\n";
    let stderr = format!("{first}debug1: next authentication method: password\n");
    let mut scan = RefusalScan::new(stderr.as_bytes(), Some(&paths), &run);
    let mut chunk = [0u8; 16];
    let mut read = 0;
    while read < first.len() {
        read += scan.read(&mut chunk).expect("read");
        if read < first.len() {
            assert_eq!(refusal_for(Some(&paths), "/k"), None, "line not done yet");
        }
    }
    assert!(read < stderr.len(), "the stream is not done yet");
    assert_eq!(
        refusal_for(Some(&paths), "/k"),
        Some(RefusalReason::WrongPin)
    );
}

#[test]
fn a_last_line_without_a_newline_is_scanned_at_the_end() {
    let (_dir, paths) = scratch_paths();
    let run = SecurityKeyRun {
        pins: vec![pin("/k", "1234")],
        flow: None,
    };
    let stderr = "sign_and_send_pubkey: signing failed for ECDSA-SK \"/k\": invalid format";
    scan_all(stderr, &paths, &run);
    assert_eq!(
        refusal_for(Some(&paths), "/k"),
        Some(RefusalReason::NoAnswer)
    );
}

#[test]
fn a_pin_request_is_taken_once_and_only_by_its_flow() {
    let (_dir, paths) = scratch_paths();
    arm_pin_request(Some(&paths), PinFlow::Tunnel, "web", "/k");
    assert_eq!(
        take_pin_request(Some(&paths), PinFlow::FileBrowser, "web"),
        None
    );
    assert_eq!(
        take_pin_request(Some(&paths), PinFlow::Tunnel, "web"),
        Some("/k".to_string())
    );
    assert_eq!(take_pin_request(Some(&paths), PinFlow::Tunnel, "web"), None);
}

#[test]
fn a_stale_pin_request_is_ignored() {
    let (_dir, paths) = scratch_paths();
    arm_pin_request(Some(&paths), PinFlow::Tunnel, "web", "/k");
    let marker = paths.askpass_pin_request_marker("tunnel", "web");
    let old = SystemTime::now() - MARKER_MAX_AGE - Duration::from_secs(1);
    std::fs::File::options()
        .write(true)
        .open(&marker)
        .and_then(|f| f.set_modified(old))
        .expect("age the marker");
    assert_eq!(take_pin_request(Some(&paths), PinFlow::Tunnel, "web"), None);
    assert!(!marker.exists(), "a stale marker is removed all the same");
}

#[test]
fn a_marker_from_a_clock_that_stepped_back_still_counts() {
    let (_dir, paths) = scratch_paths();
    record_hand_out(Some(&paths), "web", 100, "/k");
    let marker = paths.askpass_pin_used_marker("web");
    let ahead = SystemTime::now() + MARKER_MAX_AGE;
    std::fs::File::options()
        .write(true)
        .open(&marker)
        .and_then(|f| f.set_modified(ahead))
        .expect("date the marker ahead");
    assert!(
        handed_out(Some(&paths), "web", 100, "/k"),
        "the same ssh must not get the PIN twice"
    );
}

#[test]
fn a_hand_out_is_tracked_per_ssh_process() {
    let (_dir, paths) = scratch_paths();
    assert!(!handed_out(Some(&paths), "web", 100, "/k"));
    record_hand_out(Some(&paths), "web", 100, "/k");
    assert!(handed_out(Some(&paths), "web", 100, "/k"));
    assert!(!handed_out(Some(&paths), "web", 101, "/k"), "another ssh");
    assert!(
        !handed_out(Some(&paths), "web", 100, "/other"),
        "another key"
    );
    record_hand_out(Some(&paths), "web", 101, "/k b");
    let keys = take_handed_out_keys(Some(&paths), "web");
    assert_eq!(keys, vec!["/k".to_string(), "/k b".to_string()]);
    assert!(
        take_handed_out_keys(Some(&paths), "web").is_empty(),
        "taken"
    );
}

#[test]
fn a_second_hand_out_of_one_key_waits_and_backs_off_when_the_key_refused() {
    // Two hosts sharing a key ask at once. The first one finds the PIN wrong,
    // so the second must not spend another attempt on it.
    let (_dir, paths) = scratch_paths();
    // This test process stands in for the first ssh, still running.
    let first = std::process::id();
    let start = std::time::Instant::now();
    assert!(claim_hand_out(Some(&paths), "a", first, "/k"));
    assert!(handed_out(Some(&paths), "a", first, "/k"));
    note_refusal(
        Some(&paths),
        &SignFailure {
            key_path: "/k".into(),
            reason: RefusalReason::WrongPin,
        },
    );
    assert!(!claim_hand_out(Some(&paths), "b", 200, "/k"));
    assert!(!handed_out(Some(&paths), "b", 200, "/k"));
    // Filesystems that round mtimes down can shorten the wait by up to a
    // second, never more.
    assert!(
        start.elapsed() >= HAND_OUT_SPACING - Duration::from_secs(1),
        "the second hand-out waited"
    );
}

#[test]
fn a_hand_out_after_an_ssh_that_already_exited_barely_waits() {
    // The file browser looks up the home directory and then lists it, one
    // ssh after the other. The second one must not sit out the full gap.
    let (_dir, paths) = scratch_paths();
    let mut gone = std::process::Command::new("true")
        .spawn()
        .expect("spawn true");
    let gone_pid = gone.id();
    gone.wait().expect("reap");
    let start = std::time::Instant::now();
    assert!(claim_hand_out(Some(&paths), "a", gone_pid, "/k"));
    assert!(claim_hand_out(Some(&paths), "a", 200, "/k"));
    let elapsed = start.elapsed();
    assert!(elapsed >= EXITED_HOLDER_GRACE, "a short wait all the same");
    assert!(elapsed < HAND_OUT_SPACING);
}

#[test]
fn a_refusal_that_lands_just_after_its_ssh_exited_is_still_seen() {
    // The ssh got the PIN a while ago and exited only now; purple reads its
    // refusal line a moment later. The next hand-out must wait for that.
    let (_dir, paths) = scratch_paths();
    let mut gone = std::process::Command::new("true")
        .spawn()
        .expect("spawn true");
    let gone_pid = gone.id();
    gone.wait().expect("reap");
    assert!(claim_hand_out(Some(&paths), "a", gone_pid, "/k"));
    let last = paths.askpass_pin_last_marker(&key_digest("/k"));
    std::fs::File::options()
        .write(true)
        .open(&last)
        .and_then(|f| f.set_modified(SystemTime::now() - Duration::from_secs(1)))
        .expect("age the marker");
    assert_eq!(
        spacing_left(&last),
        Some(EXITED_HOLDER_GRACE),
        "the grace counts from now, not from the hand-out"
    );
    std::fs::File::options()
        .write(true)
        .open(&last)
        .and_then(|f| f.set_modified(SystemTime::now() - HAND_OUT_SPACING))
        .expect("age the marker past the spacing");
    assert_eq!(spacing_left(&last), None);
}

#[test]
fn a_live_or_unknown_process_counts_as_alive() {
    assert!(process_alive(std::process::id()));
    assert!(
        process_alive(u32::MAX),
        "out of range is not proof it is gone"
    );
}

#[test]
fn hand_outs_of_different_keys_do_not_wait_on_each_other() {
    let (_dir, paths) = scratch_paths();
    let start = std::time::Instant::now();
    assert!(claim_hand_out(Some(&paths), "a", 100, "/k1"));
    assert!(claim_hand_out(Some(&paths), "a", 100, "/k2"));
    assert!(start.elapsed() < HAND_OUT_SPACING);
}

#[test]
fn without_paths_nothing_is_recorded() {
    note_refusal(
        None,
        &SignFailure {
            key_path: "/k".into(),
            reason: RefusalReason::WrongPin,
        },
    );
    assert_eq!(refusal_for(None, "/k"), None);
    arm_pin_request(None, PinFlow::Tunnel, "web", "/k");
    assert_eq!(take_pin_request(None, PinFlow::Tunnel, "web"), None);
    record_hand_out(None, "web", 1, "/k");
    assert!(!handed_out(None, "web", 1, "/k"));
}

// --- terminal PIN input ---

#[test]
fn typing_erasing_and_submitting_a_pin() {
    let mut buf = String::new();
    for c in "12ab".chars() {
        assert_eq!(
            step_pin_input(&mut buf, &key(KeyCode::Char(c), KeyModifiers::NONE)),
            PinInput::Typed
        );
    }
    assert_eq!(
        step_pin_input(&mut buf, &key(KeyCode::Backspace, KeyModifiers::NONE)),
        PinInput::Erased
    );
    assert_eq!(buf, "12a");
    assert_eq!(
        step_pin_input(&mut buf, &key(KeyCode::Enter, KeyModifiers::NONE)),
        PinInput::Submit
    );
}

#[test]
fn esc_ctrl_c_and_ctrl_d_cancel() {
    let mut buf = String::from("12");
    assert_eq!(
        step_pin_input(&mut buf, &key(KeyCode::Esc, KeyModifiers::NONE)),
        PinInput::Cancel
    );
    assert_eq!(
        step_pin_input(&mut buf, &key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        PinInput::Cancel
    );
    assert_eq!(
        step_pin_input(&mut buf, &key(KeyCode::Char('d'), KeyModifiers::CONTROL)),
        PinInput::Cancel
    );
    assert_eq!(buf, "12", "a cancel types nothing");
}

#[test]
fn modified_keys_releases_and_an_empty_erase_are_ignored() {
    let mut buf = String::new();
    assert_eq!(
        step_pin_input(&mut buf, &key(KeyCode::Backspace, KeyModifiers::NONE)),
        PinInput::Ignored
    );
    assert_eq!(
        step_pin_input(&mut buf, &key(KeyCode::Char('x'), KeyModifiers::ALT)),
        PinInput::Ignored
    );
    let mut release = key(KeyCode::Char('x'), KeyModifiers::NONE);
    release.kind = KeyEventKind::Release;
    assert_eq!(step_pin_input(&mut buf, &release), PinInput::Ignored);
    assert!(buf.is_empty());
}

#[test]
fn the_pin_stops_at_the_ctap2_limit() {
    let mut buf = String::new();
    for _ in 0..PIN_MAX_BYTES + 5 {
        step_pin_input(&mut buf, &key(KeyCode::Char('7'), KeyModifiers::NONE));
    }
    assert_eq!(buf.len(), PIN_MAX_BYTES);
    // A two-byte character that would cross the limit is refused whole.
    let mut buf = "7".repeat(PIN_MAX_BYTES - 1);
    assert_eq!(
        step_pin_input(&mut buf, &key(KeyCode::Char('é'), KeyModifiers::NONE)),
        PinInput::Ignored
    );
}
