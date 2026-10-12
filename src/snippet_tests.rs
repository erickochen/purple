use super::*;

// =========================================================================
// Parse
// =========================================================================

#[test]
fn test_parse_empty() {
    let store = SnippetStore::parse("");
    assert!(store.snippets.is_empty());
}

#[test]
fn test_parse_single_snippet() {
    let content = "\
[check-disk]
command=df -h
description=Check disk usage
";
    let store = SnippetStore::parse(content);
    assert_eq!(store.snippets.len(), 1);
    let s = &store.snippets[0];
    assert_eq!(s.name, "check-disk");
    assert_eq!(s.command, "df -h");
    assert_eq!(s.description, "Check disk usage");
}

#[test]
fn test_parse_multiple_snippets() {
    let content = "\
[check-disk]
command=df -h

[uptime]
command=uptime
description=Check server uptime
";
    let store = SnippetStore::parse(content);
    assert_eq!(store.snippets.len(), 2);
    assert_eq!(store.snippets[0].name, "check-disk");
    assert_eq!(store.snippets[1].name, "uptime");
}

#[test]
fn test_parse_comments_and_blanks() {
    let content = "\
# Snippet config

[check-disk]
# Main command
command=df -h
";
    let store = SnippetStore::parse(content);
    assert_eq!(store.snippets.len(), 1);
    assert_eq!(store.snippets[0].command, "df -h");
}

#[test]
fn test_parse_duplicate_sections_first_wins() {
    let content = "\
[check-disk]
command=df -h

[check-disk]
command=du -sh *
";
    let store = SnippetStore::parse(content);
    assert_eq!(store.snippets.len(), 1);
    assert_eq!(store.snippets[0].command, "df -h");
}

#[test]
fn test_parse_snippet_without_command_skipped() {
    let content = "\
[empty]
description=No command here

[valid]
command=ls -la
";
    let store = SnippetStore::parse(content);
    assert_eq!(store.snippets.len(), 1);
    assert_eq!(store.snippets[0].name, "valid");
}

#[test]
fn test_parse_unknown_keys_ignored() {
    let content = "\
[check-disk]
command=df -h
unknown=value
foo=bar
";
    let store = SnippetStore::parse(content);
    assert_eq!(store.snippets.len(), 1);
    assert_eq!(store.snippets[0].command, "df -h");
}

#[test]
fn test_parse_whitespace_in_section_name() {
    let content = "[ check-disk ]\ncommand=df -h\n";
    let store = SnippetStore::parse(content);
    assert_eq!(store.snippets[0].name, "check-disk");
}

#[test]
fn test_parse_whitespace_around_key_value() {
    let content = "[check-disk]\n  command  =  df -h  \n";
    let store = SnippetStore::parse(content);
    assert_eq!(store.snippets[0].command, "df -h");
}

#[test]
fn test_parse_command_with_equals() {
    let content = "[env-check]\ncommand=env | grep HOME=\n";
    let store = SnippetStore::parse(content);
    assert_eq!(store.snippets[0].command, "env | grep HOME=");
}

#[test]
fn test_parse_line_without_equals_ignored() {
    let content = "[check]\ncommand=ls\ngarbage_line\n";
    let store = SnippetStore::parse(content);
    assert_eq!(store.snippets[0].command, "ls");
}

// =========================================================================
// Get / Set / Remove
// =========================================================================

#[test]
fn test_get_found() {
    let store = SnippetStore::parse("[check]\ncommand=ls\n");
    assert!(store.get("check").is_some());
}

#[test]
fn test_get_not_found() {
    let store = SnippetStore::parse("");
    assert!(store.get("nope").is_none());
}

#[test]
fn test_set_adds_new() {
    let mut store = SnippetStore::default();
    store.set(Snippet {
        name: "check".to_string(),
        command: "ls".to_string(),
        description: String::new(),
        interactive: false,
    });
    assert_eq!(store.snippets.len(), 1);
}

#[test]
fn test_set_replaces_existing() {
    let mut store = SnippetStore::parse("[check]\ncommand=ls\n");
    store.set(Snippet {
        name: "check".to_string(),
        command: "df -h".to_string(),
        description: String::new(),
        interactive: false,
    });
    assert_eq!(store.snippets.len(), 1);
    assert_eq!(store.snippets[0].command, "df -h");
}

#[test]
fn test_remove() {
    let mut store = SnippetStore::parse("[check]\ncommand=ls\n[uptime]\ncommand=uptime\n");
    store.remove("check");
    assert_eq!(store.snippets.len(), 1);
    assert_eq!(store.snippets[0].name, "uptime");
}

#[test]
fn test_remove_nonexistent_noop() {
    let mut store = SnippetStore::parse("[check]\ncommand=ls\n");
    store.remove("nope");
    assert_eq!(store.snippets.len(), 1);
}

// =========================================================================
// Validate name
// =========================================================================

#[test]
fn test_validate_name_valid() {
    assert!(validate_name("check-disk").is_ok());
    assert!(validate_name("restart_nginx").is_ok());
    assert!(validate_name("a").is_ok());
}

#[test]
fn test_validate_name_empty() {
    assert!(validate_name("").is_err());
}

#[test]
fn test_validate_name_whitespace() {
    assert!(validate_name("check disk").is_ok());
    assert!(validate_name("check\tdisk").is_err()); // tab is a control character
    assert!(validate_name("  ").is_err()); // only whitespace
    assert!(validate_name(" leading").is_err()); // leading whitespace
    assert!(validate_name("trailing ").is_err()); // trailing whitespace
}

#[test]
fn test_validate_name_special_chars() {
    assert!(validate_name("check#disk").is_err());
    assert!(validate_name("[check]").is_err());
}

#[test]
fn test_validate_name_control_chars() {
    assert!(validate_name("check\x00disk").is_err());
}

// =========================================================================
// Validate command
// =========================================================================

#[test]
fn test_validate_command_valid() {
    assert!(validate_command("df -h").is_ok());
    assert!(validate_command("cat /etc/hosts | grep localhost").is_ok());
    assert!(validate_command("echo 'hello\tworld'").is_ok()); // tab allowed
}

#[test]
fn test_validate_command_empty() {
    assert!(validate_command("").is_err());
}

#[test]
fn test_validate_command_whitespace_only() {
    assert!(validate_command("   ").is_err());
    assert!(validate_command(" \t ").is_err());
}

#[test]
fn test_validate_command_control_chars() {
    assert!(validate_command("ls\x00-la").is_err());
}

// =========================================================================
// Save / roundtrip
// =========================================================================

#[test]
fn test_save_roundtrip() {
    let mut store = SnippetStore::default();
    store.set(Snippet {
        name: "check-disk".to_string(),
        command: "df -h".to_string(),
        description: "Check disk usage".to_string(),
        interactive: false,
    });
    store.set(Snippet {
        name: "uptime".to_string(),
        command: "uptime".to_string(),
        description: String::new(),
        interactive: false,
    });

    // Serialize
    let mut content = String::new();
    for (i, snippet) in store.snippets.iter().enumerate() {
        if i > 0 {
            content.push('\n');
        }
        content.push_str(&format!("[{}]\n", snippet.name));
        content.push_str(&format!("command={}\n", snippet.command));
        if !snippet.description.is_empty() {
            content.push_str(&format!("description={}\n", snippet.description));
        }
    }

    // Re-parse
    let reparsed = SnippetStore::parse(&content);
    assert_eq!(reparsed.snippets.len(), 2);
    assert_eq!(reparsed.snippets[0].name, "check-disk");
    assert_eq!(reparsed.snippets[0].command, "df -h");
    assert_eq!(reparsed.snippets[0].description, "Check disk usage");
    assert_eq!(reparsed.snippets[1].name, "uptime");
    assert_eq!(reparsed.snippets[1].command, "uptime");
    assert!(reparsed.snippets[1].description.is_empty());
}

#[test]
fn test_save_to_temp_file() {
    // SnippetStore::save() is a no-op when the global demo flag is set, so
    // any parallel test that flips the flag could race this one and yield
    // a NotFound when we read back. Serialise via the cross-crate lock and
    // explicitly disable demo mode while we run, then restore the prior
    // value so subsequent tests that depend on demo mode are unaffected.
    let _guard = crate::demo_flag::GLOBAL_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let prior_demo = crate::demo_flag::is_demo();
    crate::demo_flag::disable();

    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().join("snippets");

    let mut store = SnippetStore {
        path_override: Some(path.clone()),
        ..Default::default()
    };
    store.set(Snippet {
        name: "test".to_string(),
        command: "echo hello".to_string(),
        description: "Test snippet".to_string(),
        interactive: false,
    });
    store.save().unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    let reloaded = SnippetStore::parse(&content);
    assert_eq!(reloaded.snippets.len(), 1);
    assert_eq!(reloaded.snippets[0].name, "test");
    assert_eq!(reloaded.snippets[0].command, "echo hello");

    if prior_demo {
        crate::demo_flag::enable();
    }
}

// =========================================================================
// Interactive shell
// =========================================================================

#[test]
fn test_parse_interactive_true() {
    let store = SnippetStore::parse("[pm2]\ncommand=pm2 status\ninteractive=true\n");
    assert!(store.snippets[0].interactive);
}

#[test]
fn test_parse_interactive_defaults_to_false() {
    let store = SnippetStore::parse("[pm2]\ncommand=pm2 status\n");
    assert!(!store.snippets[0].interactive);
}

#[test]
fn test_parse_interactive_only_accepts_true() {
    for value in ["false", "yes", "1", "TRUE", ""] {
        let content = format!("[pm2]\ncommand=pm2 status\ninteractive={value}\n");
        let store = SnippetStore::parse(&content);
        assert!(!store.snippets[0].interactive, "value {value:?}");
    }
}

#[test]
fn test_save_writes_interactive_only_when_set() {
    let _guard = crate::demo_flag::GLOBAL_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let prior_demo = crate::demo_flag::is_demo();
    crate::demo_flag::disable();

    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().join("snippets");
    let mut store = SnippetStore {
        path_override: Some(path.clone()),
        ..Default::default()
    };
    store.set(Snippet {
        name: "plain".to_string(),
        command: "uptime".to_string(),
        ..Snippet::default()
    });
    store.set(Snippet {
        name: "pm2".to_string(),
        command: "pm2 status".to_string(),
        interactive: true,
        ..Snippet::default()
    });
    store.save().unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    assert_eq!(content.matches("interactive=").count(), 1);
    assert!(content.contains("[pm2]\ncommand=pm2 status\ninteractive=true\n"));
    let reloaded = SnippetStore::parse(&content);
    assert!(!reloaded.get("plain").unwrap().interactive);
    assert!(reloaded.get("pm2").unwrap().interactive);

    if prior_demo {
        crate::demo_flag::enable();
    }
}

#[test]
fn test_remote_command_plain_is_the_command() {
    let s = Snippet {
        name: "a".into(),
        command: "pm2 status".into(),
        ..Snippet::default()
    };
    assert_eq!(s.remote_command(), "pm2 status");
}

#[test]
fn test_remote_command_interactive_wraps() {
    let s = Snippet {
        name: "a".into(),
        command: "pm2 status".into(),
        interactive: true,
        ..Snippet::default()
    };
    let wrapped = s.remote_command();
    assert!(wrapped.starts_with("exec /bin/sh -c '"), "{wrapped}");
    assert!(wrapped.contains(" sh 'pm2 status' "), "{wrapped}");
}

#[test]
fn test_portable_escape_keeps_quotes_backslashes_and_bangs_outside_quotes() {
    assert_eq!(portable_escape("plain"), "'plain'");
    assert_eq!(portable_escape("it's"), "'it'\\''s'");
    assert_eq!(portable_escape("a\\b"), "'a'\\\\'b'");
    assert_eq!(portable_escape("x!y"), "'x'\\!'y'");
}

#[test]
fn test_wrap_interactive_quotes_the_command_as_its_own_argument() {
    let wrapped = wrap_interactive("echo 'hi'");
    assert!(wrapped.contains(" sh 'echo '\\''hi'\\''' "), "{wrapped}");
}

/// POSIX login shells the wrapper is run through, as far as they exist on this
/// machine. bash and zsh take the interactive route, sh and dash the plain one.
fn local_shells() -> Vec<&'static str> {
    existing(&[
        "/bin/sh",
        "/bin/bash",
        "/bin/zsh",
        "/usr/bin/zsh",
        "/bin/dash",
    ])
}

/// Non-POSIX login shells, which run the command in their own syntax.
fn other_local_shells() -> Vec<&'static str> {
    existing(&[
        "/bin/tcsh",
        "/usr/bin/tcsh",
        "/usr/bin/fish",
        "/opt/homebrew/bin/fish",
    ])
}

fn existing(paths: &[&'static str]) -> Vec<&'static str> {
    paths
        .iter()
        .copied()
        .filter(|s| std::path::Path::new(s).exists())
        .collect()
}

/// Run a wrapped command through a local shell the way sshd runs it on the
/// remote: `$SHELL -c <command>`, with HOME pointing at an empty directory
/// and `stdin` fed to the command.
fn run_wrapped_locally(shell: &str, command: &str, stdin: &str) -> std::process::Output {
    run_in_sandbox(shell, &wrap_interactive(command), stdin)
}

/// Run `script` with `shell -c` the way sshd starts a login shell (short name
/// as argv[0]), inside an empty HOME that is also the working directory, so
/// nothing from the developer's own dotfiles or the repo leaks in or out.
fn run_in_sandbox(shell: &str, script: &str, stdin: &str) -> std::process::Output {
    use std::io::Write;
    use std::os::unix::process::CommandExt;
    let home = tempfile::tempdir().expect("create tempdir");
    // Keeps Ubuntu's /etc/bash.bashrc from printing its sudo hint to stdout.
    std::fs::write(home.path().join(".sudo_as_admin_successful"), "").expect("write marker");
    let name = shell.rsplit('/').next().unwrap_or(shell);
    let mut child = std::process::Command::new(shell)
        .arg0(name)
        .arg("-c")
        .arg(script)
        .env("SHELL", shell)
        .env("HOME", home.path())
        .env_remove("ENV")
        .current_dir(home.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("run shell");
    // A shell that exits without reading stdin closes the pipe under the write.
    let written = child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin.as_bytes());
    if let Err(e) = written {
        assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe, "write stdin: {e}");
    }
    child.wait_with_output().expect("wait shell")
}

/// More than any default pipe buffer holds, so the write outlives a shell
/// that never reads stdin.
const PIPE_OVERFLOW_BYTES: usize = 1 << 20;

#[test]
fn test_sandbox_tolerates_a_shell_that_exits_without_reading_stdin() {
    let stdin = "x".repeat(PIPE_OVERFLOW_BYTES);
    let out = run_in_sandbox("/bin/sh", "exit 3", &stdin);
    assert_eq!(out.status.code(), Some(3));
}

#[test]
fn test_wrap_interactive_runs_with_quotes_stderr_and_exit_code() {
    for shell in local_shells() {
        let out = run_wrapped_locally(shell, "printf '%s\\n' \"it's\"; echo err >&2; exit 3", "");
        assert_eq!(String::from_utf8_lossy(&out.stdout), "it's\n", "{shell}");
        assert_eq!(String::from_utf8_lossy(&out.stderr), "err\n", "{shell}");
        assert_eq!(out.status.code(), Some(3), "{shell}");
    }
}

#[test]
fn test_wrap_interactive_passes_stdin_to_the_command() {
    for shell in local_shells() {
        let out = run_wrapped_locally(shell, "read x; echo \"got $x\"", "hello\n");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "got hello\n",
            "{shell}"
        );
    }
}

#[test]
fn test_wrap_interactive_never_runs_stdin_after_a_syntax_error() {
    for shell in local_shells() {
        let out = run_wrapped_locally(shell, "echo \"unterminated", "echo INJECTED\n");
        assert!(
            !String::from_utf8_lossy(&out.stdout).contains("INJECTED"),
            "{shell}"
        );
        assert_ne!(out.status.code(), Some(0), "{shell}");
        assert!(
            !out.stderr.is_empty(),
            "{shell}: the syntax error stays visible"
        );
    }
}

#[test]
fn test_wrap_interactive_runs_plainly_on_other_login_shells() {
    let plain_route = |s: &&str| !s.ends_with("/bash") && !s.ends_with("/zsh");
    let shells = local_shells().into_iter().filter(plain_route);
    for shell in shells.chain(other_local_shells()) {
        for command in ["echo \"x!y\"; echo it\\'s", "purple_no_such_command_xyz"] {
            let wrapped = run_wrapped_locally(shell, command, "");
            let plain = run_in_sandbox(shell, command, "");
            assert_eq!(wrapped.stdout, plain.stdout, "{shell}: {command}");
            assert_eq!(wrapped.stderr, plain.stderr, "{shell}: {command}");
            assert_eq!(
                wrapped.status.code(),
                plain.status.code(),
                "{shell}: {command}"
            );
        }
    }
}

#[test]
fn test_wrap_interactive_keeps_backslashes_and_quotes_in_every_shell() {
    for shell in local_shells() {
        let out = run_wrapped_locally(shell, "printf '%s\\n' 'a\\b' \"it's\"", "");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "a\\b\nit's\n",
            "{shell}"
        );
    }
}

#[test]
fn test_wrap_interactive_hides_shell_noise_after_a_signal() {
    // Only bash and zsh run interactively; the plain route reports a signal
    // the same way a plain snippet does.
    let interactive = |s: &&str| s.ends_with("/bash") || s.ends_with("/zsh");
    for shell in local_shells().into_iter().filter(interactive) {
        let out = run_wrapped_locally(shell, "sh -c 'kill -TERM $$'", "");
        assert!(out.stderr.is_empty(), "{shell}: {:?}", out.stderr);
    }
}

#[test]
fn test_wrap_interactive_survives_a_trailing_comment() {
    for shell in local_shells() {
        let out = run_wrapped_locally(shell, "echo ok # trailing comment", "");
        assert_eq!(String::from_utf8_lossy(&out.stdout), "ok\n", "{shell}");
        assert_eq!(out.status.code(), Some(0), "{shell}");
    }
}

#[test]
fn test_wrap_interactive_keeps_not_found_exit_code() {
    for shell in local_shells() {
        let out = run_wrapped_locally(shell, "purple_no_such_command_xyz", "");
        assert_eq!(out.status.code(), Some(EXIT_COMMAND_NOT_FOUND), "{shell}");
        assert!(
            !out.stderr.is_empty(),
            "{shell}: the command's own error stays visible"
        );
    }
}

#[test]
fn test_not_found_hint_applies_only_to_plain_127() {
    assert!(not_found_hint_applies(false, Some(EXIT_COMMAND_NOT_FOUND)));
    assert!(!not_found_hint_applies(true, Some(EXIT_COMMAND_NOT_FOUND)));
    assert!(!not_found_hint_applies(false, Some(0)));
    assert!(!not_found_hint_applies(false, Some(1)));
    assert!(!not_found_hint_applies(false, None));
}

// =========================================================================
// Edge cases
// =========================================================================

#[test]
fn test_set_multiple_then_remove_all() {
    let mut store = SnippetStore::default();
    for name in ["a", "b", "c"] {
        store.set(Snippet {
            name: name.to_string(),
            command: "cmd".to_string(),
            description: String::new(),
            interactive: false,
        });
    }
    assert_eq!(store.snippets.len(), 3);
    store.remove("a");
    store.remove("b");
    store.remove("c");
    assert!(store.snippets.is_empty());
}

#[test]
fn test_snippet_with_complex_command() {
    let content = "[complex]\ncommand=for i in $(seq 1 5); do echo $i; done\n";
    let store = SnippetStore::parse(content);
    assert_eq!(
        store.snippets[0].command,
        "for i in $(seq 1 5); do echo $i; done"
    );
}

#[test]
fn test_snippet_command_with_pipes_and_redirects() {
    let content = "[logs]\ncommand=tail -100 /var/log/syslog | grep error | head -20\n";
    let store = SnippetStore::parse(content);
    assert_eq!(
        store.snippets[0].command,
        "tail -100 /var/log/syslog | grep error | head -20"
    );
}

#[test]
fn test_description_optional() {
    let content = "[check]\ncommand=ls\n";
    let store = SnippetStore::parse(content);
    assert!(store.snippets[0].description.is_empty());
}

#[test]
fn test_description_with_equals() {
    let content = "[env]\ncommand=env\ndescription=Check HOME= and PATH= vars\n";
    let store = SnippetStore::parse(content);
    assert_eq!(store.snippets[0].description, "Check HOME= and PATH= vars");
}

#[test]
fn test_name_with_equals_roundtrip() {
    let mut store = SnippetStore::default();
    store.set(Snippet {
        name: "check=disk".to_string(),
        command: "df -h".to_string(),
        description: String::new(),
        interactive: false,
    });

    let mut content = String::new();
    for (i, snippet) in store.snippets.iter().enumerate() {
        if i > 0 {
            content.push('\n');
        }
        content.push_str(&format!("[{}]\n", snippet.name));
        content.push_str(&format!("command={}\n", snippet.command));
        if !snippet.description.is_empty() {
            content.push_str(&format!("description={}\n", snippet.description));
        }
    }

    let reparsed = SnippetStore::parse(&content);
    assert_eq!(reparsed.snippets.len(), 1);
    assert_eq!(reparsed.snippets[0].name, "check=disk");
}

#[test]
fn test_validate_name_with_equals() {
    assert!(validate_name("check=disk").is_ok());
}

#[test]
fn test_parse_only_comments_and_blanks() {
    let content = "# comment\n\n# another\n";
    let store = SnippetStore::parse(content);
    assert!(store.snippets.is_empty());
}

#[test]
fn test_parse_section_without_close_bracket() {
    let content = "[incomplete\ncommand=ls\n";
    let store = SnippetStore::parse(content);
    assert!(store.snippets.is_empty());
}

#[test]
fn test_parse_trailing_content_after_last_section() {
    let content = "[check]\ncommand=ls\n";
    let store = SnippetStore::parse(content);
    assert_eq!(store.snippets.len(), 1);
    assert_eq!(store.snippets[0].command, "ls");
}

#[test]
fn test_set_overwrite_preserves_order() {
    let mut store = SnippetStore::default();
    store.set(Snippet {
        name: "a".into(),
        command: "1".into(),
        description: String::new(),
        interactive: false,
    });
    store.set(Snippet {
        name: "b".into(),
        command: "2".into(),
        description: String::new(),
        interactive: false,
    });
    store.set(Snippet {
        name: "c".into(),
        command: "3".into(),
        description: String::new(),
        interactive: false,
    });
    store.set(Snippet {
        name: "b".into(),
        command: "updated".into(),
        description: String::new(),
        interactive: false,
    });
    assert_eq!(store.snippets.len(), 3);
    assert_eq!(store.snippets[0].name, "a");
    assert_eq!(store.snippets[1].name, "b");
    assert_eq!(store.snippets[1].command, "updated");
    assert_eq!(store.snippets[2].name, "c");
}

#[test]
fn test_validate_command_with_tab() {
    assert!(validate_command("echo\thello").is_ok());
}

#[test]
fn test_validate_command_with_newline() {
    assert!(validate_command("echo\nhello").is_err());
}

#[test]
fn test_validate_name_newline() {
    assert!(validate_name("check\ndisk").is_err());
}

// =========================================================================
// shell_escape
// =========================================================================

#[test]
fn test_shell_escape_simple() {
    assert_eq!(shell_escape("hello"), "'hello'");
}

#[test]
fn test_shell_escape_with_single_quote() {
    assert_eq!(shell_escape("it's"), "'it'\\''s'");
}

#[test]
fn test_shell_escape_with_spaces() {
    assert_eq!(shell_escape("hello world"), "'hello world'");
}

#[test]
fn test_shell_escape_with_semicolon() {
    assert_eq!(shell_escape("; rm -rf /"), "'; rm -rf /'");
}

#[test]
fn test_shell_escape_with_dollar() {
    assert_eq!(shell_escape("$(whoami)"), "'$(whoami)'");
}

#[test]
fn test_shell_escape_empty() {
    assert_eq!(shell_escape(""), "''");
}

// =========================================================================
// parse_params
// =========================================================================

#[test]
fn test_parse_params_none() {
    assert!(parse_params("df -h").is_empty());
}

#[test]
fn test_parse_params_single() {
    let params = parse_params("df -h {{path}}");
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name, "path");
    assert_eq!(params[0].default, None);
}

#[test]
fn test_parse_params_with_default() {
    let params = parse_params("df -h {{path:/var/log}}");
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name, "path");
    assert_eq!(params[0].default, Some("/var/log".to_string()));
}

#[test]
fn test_parse_params_multiple() {
    let params = parse_params("grep {{pattern}} {{file}}");
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].name, "pattern");
    assert_eq!(params[1].name, "file");
}

#[test]
fn test_parse_params_deduplicate() {
    let params = parse_params("echo {{name}} {{name}}");
    assert_eq!(params.len(), 1);
}

#[test]
fn test_parse_params_invalid_name_skipped() {
    let params = parse_params("echo {{valid}} {{bad name}} {{ok}}");
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].name, "valid");
    assert_eq!(params[1].name, "ok");
}

#[test]
fn test_parse_params_unclosed_brace() {
    let params = parse_params("echo {{unclosed");
    assert!(params.is_empty());
}

#[test]
fn test_parse_params_max_20() {
    let cmd: String = (0..25)
        .map(|i| format!("{{{{p{}}}}}", i))
        .collect::<Vec<_>>()
        .join(" ");
    let params = parse_params(&cmd);
    assert_eq!(params.len(), 20);
}

// =========================================================================
// validate_param_name
// =========================================================================

#[test]
fn test_validate_param_name_valid() {
    assert!(validate_param_name("path").is_ok());
    assert!(validate_param_name("my-param").is_ok());
    assert!(validate_param_name("my_param").is_ok());
    assert!(validate_param_name("param1").is_ok());
}

#[test]
fn test_validate_param_name_empty() {
    assert!(validate_param_name("").is_err());
}

#[test]
fn test_validate_param_name_rejects_braces() {
    assert!(validate_param_name("a{b").is_err());
    assert!(validate_param_name("a}b").is_err());
}

#[test]
fn test_validate_param_name_rejects_quote() {
    assert!(validate_param_name("it's").is_err());
}

#[test]
fn test_validate_param_name_rejects_whitespace() {
    assert!(validate_param_name("a b").is_err());
}

// =========================================================================
// substitute_params
// =========================================================================

#[test]
fn test_substitute_simple() {
    let mut values = std::collections::HashMap::new();
    values.insert("path".to_string(), "/var/log".to_string());
    let result = substitute_params("df -h {{path}}", &values);
    assert_eq!(result, "df -h '/var/log'");
}

#[test]
fn test_substitute_with_default() {
    let values = std::collections::HashMap::new();
    let result = substitute_params("df -h {{path:/tmp}}", &values);
    assert_eq!(result, "df -h '/tmp'");
}

#[test]
fn test_substitute_overrides_default() {
    let mut values = std::collections::HashMap::new();
    values.insert("path".to_string(), "/home".to_string());
    let result = substitute_params("df -h {{path:/tmp}}", &values);
    assert_eq!(result, "df -h '/home'");
}

#[test]
fn test_substitute_escapes_injection() {
    let mut values = std::collections::HashMap::new();
    values.insert("name".to_string(), "; rm -rf /".to_string());
    let result = substitute_params("echo {{name}}", &values);
    assert_eq!(result, "echo '; rm -rf /'");
}

#[test]
fn test_substitute_no_recursive_expansion() {
    let mut values = std::collections::HashMap::new();
    values.insert("a".to_string(), "{{b}}".to_string());
    values.insert("b".to_string(), "gotcha".to_string());
    let result = substitute_params("echo {{a}}", &values);
    assert_eq!(result, "echo '{{b}}'");
}

#[test]
fn test_substitute_default_also_escaped() {
    let values = std::collections::HashMap::new();
    let result = substitute_params("echo {{x:$(whoami)}}", &values);
    assert_eq!(result, "echo '$(whoami)'");
}

// =========================================================================
// sanitize_output
// =========================================================================

#[test]
fn test_sanitize_plain_text() {
    assert_eq!(sanitize_output("hello world"), "hello world");
}

#[test]
fn test_sanitize_preserves_newlines_tabs() {
    assert_eq!(sanitize_output("line1\nline2\tok"), "line1\nline2\tok");
}

#[test]
fn test_sanitize_strips_csi() {
    assert_eq!(sanitize_output("\x1b[31mred\x1b[0m"), "red");
}

#[test]
fn test_sanitize_strips_osc_bel() {
    assert_eq!(sanitize_output("\x1b]0;title\x07text"), "text");
}

#[test]
fn test_sanitize_strips_osc_st() {
    assert_eq!(sanitize_output("\x1b]52;c;dGVzdA==\x1b\\text"), "text");
}

#[test]
fn test_sanitize_strips_c1_range() {
    assert_eq!(sanitize_output("a\u{0090}b\u{009C}c"), "abc");
}

#[test]
fn test_sanitize_strips_control_chars() {
    assert_eq!(sanitize_output("a\x01b\x07c"), "abc");
}

#[test]
fn test_sanitize_strips_dcs() {
    assert_eq!(sanitize_output("\x1bPdata\x1b\\text"), "text");
}

// =========================================================================
// shell_escape (edge cases)
// =========================================================================

#[test]
fn test_shell_escape_only_single_quotes() {
    assert_eq!(shell_escape("'''"), "''\\'''\\'''\\'''");
}

#[test]
fn test_shell_escape_consecutive_single_quotes() {
    assert_eq!(shell_escape("a''b"), "'a'\\'''\\''b'");
}

// =========================================================================
// parse_params (edge cases)
// =========================================================================

#[test]
fn test_parse_params_adjacent() {
    let params = parse_params("{{a}}{{b}}");
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].name, "a");
    assert_eq!(params[1].name, "b");
}

#[test]
fn test_parse_params_command_is_only_param() {
    let params = parse_params("{{cmd}}");
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name, "cmd");
}

#[test]
fn test_parse_params_nested_braces_rejected() {
    // {{{a}}} -> inner is "{a" which fails validation
    let params = parse_params("{{{a}}}");
    assert!(params.is_empty());
}

#[test]
fn test_parse_params_colon_empty_default() {
    let params = parse_params("echo {{name:}}");
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name, "name");
    assert_eq!(params[0].default, Some("".to_string()));
}

#[test]
fn test_parse_params_empty_inner() {
    let params = parse_params("echo {{}}");
    assert!(params.is_empty());
}

#[test]
fn test_parse_params_single_braces_ignored() {
    let params = parse_params("echo {notaparam}");
    assert!(params.is_empty());
}

#[test]
fn test_parse_params_default_with_colons() {
    let params = parse_params("{{url:http://localhost:8080}}");
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name, "url");
    assert_eq!(params[0].default, Some("http://localhost:8080".to_string()));
}

// =========================================================================
// validate_param_name (edge cases)
// =========================================================================

#[test]
fn test_validate_param_name_unicode() {
    assert!(validate_param_name("caf\u{00e9}").is_ok());
}

#[test]
fn test_validate_param_name_hyphen_only() {
    assert!(validate_param_name("-").is_ok());
}

#[test]
fn test_validate_param_name_underscore_only() {
    assert!(validate_param_name("_").is_ok());
}

#[test]
fn test_validate_param_name_rejects_dot() {
    assert!(validate_param_name("a.b").is_err());
}

// =========================================================================
// substitute_params (edge cases)
// =========================================================================

#[test]
fn test_substitute_no_params_passthrough() {
    let values = std::collections::HashMap::new();
    let result = substitute_params("df -h /tmp", &values);
    assert_eq!(result, "df -h /tmp");
}

#[test]
fn test_substitute_missing_param_no_default() {
    let values = std::collections::HashMap::new();
    let result = substitute_params("echo {{name}}", &values);
    assert_eq!(result, "echo ''");
}

#[test]
fn test_substitute_empty_value_falls_to_default() {
    let mut values = std::collections::HashMap::new();
    values.insert("name".to_string(), "".to_string());
    let result = substitute_params("echo {{name:fallback}}", &values);
    assert_eq!(result, "echo 'fallback'");
}

#[test]
fn test_substitute_non_ascii_around_params() {
    let mut values = std::collections::HashMap::new();
    values.insert("x".to_string(), "val".to_string());
    let result = substitute_params("\u{00e9}cho {{x}} \u{2603}", &values);
    assert_eq!(result, "\u{00e9}cho 'val' \u{2603}");
}

#[test]
fn test_substitute_adjacent_params() {
    let mut values = std::collections::HashMap::new();
    values.insert("a".to_string(), "x".to_string());
    values.insert("b".to_string(), "y".to_string());
    let result = substitute_params("{{a}}{{b}}", &values);
    assert_eq!(result, "'x''y'");
}

// =========================================================================
// sanitize_output (edge cases)
// =========================================================================

#[test]
fn test_sanitize_empty() {
    assert_eq!(sanitize_output(""), "");
}

#[test]
fn test_sanitize_only_escapes() {
    assert_eq!(sanitize_output("\x1b[31m\x1b[0m\x1b[1m"), "");
}

#[test]
fn test_sanitize_lone_esc_at_end() {
    assert_eq!(sanitize_output("hello\x1b"), "hello");
}

#[test]
fn test_sanitize_truncated_csi_no_terminator() {
    assert_eq!(sanitize_output("hello\x1b[123"), "hello");
}

#[test]
fn test_sanitize_apc_sequence() {
    assert_eq!(sanitize_output("\x1b_payload\x1b\\visible"), "visible");
}

#[test]
fn test_sanitize_pm_sequence() {
    assert_eq!(sanitize_output("\x1b^payload\x1b\\visible"), "visible");
}

#[test]
fn test_sanitize_dcs_terminated_by_bel() {
    assert_eq!(sanitize_output("\x1bPdata\x07text"), "text");
}

#[test]
fn test_sanitize_lone_esc_plus_letter() {
    assert_eq!(sanitize_output("a\x1bMb"), "ab");
}

#[test]
fn test_sanitize_multiple_mixed_sequences() {
    // \x01 (SOH) is stripped but "gone" text after it is preserved
    let input = "\x1b[1mbold\x1b[0m \x1b]0;title\x07normal \x01gone";
    assert_eq!(sanitize_output(input), "bold normal gone");
}

// =========================================================================
// base_ssh_command: non_interactive flag
// =========================================================================

fn base_ssh_args(non_interactive: bool) -> Vec<String> {
    base_ssh_args_with(None, None, non_interactive, false)
}

/// Args of a `base_ssh_command` built with the given auth inputs and flags.
fn base_ssh_args_with(
    askpass: Option<&str>,
    session_password: Option<&str>,
    non_interactive: bool,
    trust_new_host_key: bool,
) -> Vec<String> {
    use std::path::Path;
    let cmd = base_ssh_command(
        "host1",
        Path::new("/tmp/cfg"),
        "true",
        askpass,
        session_password,
        None,
        false,
        non_interactive,
        trust_new_host_key,
    );
    cmd.get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

fn has_opt(args: &[String], value: &str) -> bool {
    args.windows(2).any(|w| w[0] == "-o" && w[1] == value)
}

#[test]
fn base_ssh_non_interactive_without_auth_sets_batch_mode() {
    // No source and no session password: ssh must fail at once instead
    // of asking on the tty behind the TUI.
    let args = base_ssh_args_with(None, None, true, false);
    assert!(has_opt(&args, "BatchMode=yes"), "got: {args:?}");
}

#[test]
fn base_ssh_non_interactive_with_source_omits_batch_mode() {
    // BatchMode would disable the very auth method askpass feeds.
    let args = base_ssh_args_with(Some("keychain"), None, true, false);
    assert!(!has_opt(&args, "BatchMode=yes"), "got: {args:?}");
}

#[test]
fn base_ssh_non_interactive_with_session_password_omits_batch_mode() {
    let args = base_ssh_args_with(None, Some("hunter2"), true, false);
    assert!(!has_opt(&args, "BatchMode=yes"), "got: {args:?}");
    // The secret travels in the environment, never in argv.
    assert!(args.iter().all(|a| a != "hunter2"));
}

#[test]
fn base_ssh_interactive_never_sets_batch_mode() {
    // The terminal run (capture=false) keeps ssh's own prompt.
    let args = base_ssh_args_with(None, None, false, false);
    assert!(!has_opt(&args, "BatchMode=yes"), "got: {args:?}");
}

#[test]
fn base_ssh_non_interactive_limits_ssh_to_one_password_attempt() {
    let args = base_ssh_args_with(Some("keychain"), None, true, false);
    assert!(has_opt(&args, "NumberOfPasswordPrompts=1"), "got: {args:?}");
}

#[test]
fn base_ssh_interactive_keeps_the_default_password_attempts() {
    // At a real terminal the user can type a different password, so ssh
    // keeps its own three prompts.
    let args = base_ssh_args_with(None, None, false, false);
    assert!(
        !has_opt(&args, "NumberOfPasswordPrompts=1"),
        "got: {args:?}"
    );
}

#[test]
fn base_ssh_trust_retry_uses_accept_new() {
    let args = base_ssh_args_with(None, None, true, true);
    assert!(
        has_opt(&args, "StrictHostKeyChecking=accept-new"),
        "got: {args:?}"
    );
    assert!(!has_opt(&args, "StrictHostKeyChecking=yes"));
}

#[test]
fn base_ssh_options_precede_the_alias_separator() {
    // ssh stops option parsing at `--`, so every -o must come before it.
    let args = base_ssh_args_with(None, None, true, true);
    let sep = args.iter().position(|a| a == "--").expect("-- present");
    let last_opt = args
        .iter()
        .rposition(|a| a == "-o")
        .expect("at least one -o");
    assert!(last_opt < sep, "got: {args:?}");
}

#[test]
fn base_ssh_non_interactive_sets_strict_host_key_checking() {
    let args = base_ssh_args(true);
    let pairs: Vec<(&String, &String)> = args.windows(2).map(|w| (&w[0], &w[1])).collect();
    assert!(
        pairs
            .iter()
            .any(|(k, v)| *k == "-o" && *v == "StrictHostKeyChecking=yes"),
        "non-interactive ssh must include StrictHostKeyChecking=yes, got: {args:?}"
    );
}

#[test]
fn base_ssh_interactive_omits_strict_host_key_checking() {
    let args = base_ssh_args(false);
    assert!(
        !args.iter().any(|a| a.contains("StrictHostKeyChecking")),
        "interactive ssh must NOT set StrictHostKeyChecking (let user confirm TOFU), got: {args:?}"
    );
}

// =========================================================================
// read_bounded: cap enforcement on captured SSH stdout/stderr.
// The 16 MB cap is the only memory protection against a hostile or
// malfunctioning SSH remote that streams unbounded output. These tests
// pin the boundary behaviour so a refactor cannot silently regress it.
// =========================================================================

#[test]
fn read_bounded_under_cap_returns_all_bytes() {
    let data = b"hello world";
    let mut cursor = std::io::Cursor::new(data.to_vec());
    let out = super::read_bounded(&mut cursor, 100, "test-alias", "stdout");
    assert_eq!(out, b"hello world");
}

#[test]
fn read_bounded_exactly_at_cap_returns_all_bytes() {
    // Boundary: cap == data length must not truncate.
    let data = vec![0xAAu8; 32];
    let mut cursor = std::io::Cursor::new(data.clone());
    let out = super::read_bounded(&mut cursor, 32, "alias", "stdout");
    assert_eq!(out.len(), 32);
    assert_eq!(out, data);
}

#[test]
fn read_bounded_one_over_cap_truncates_to_cap() {
    let max = 16usize;
    let data = vec![0xBBu8; max + 1];
    let mut cursor = std::io::Cursor::new(data);
    let out = super::read_bounded(&mut cursor, max, "alias", "stdout");
    assert_eq!(out.len(), max);
    assert!(out.iter().all(|&b| b == 0xBBu8));
}

#[test]
fn read_bounded_massive_overflow_stays_at_cap() {
    // Defends against a hostile remote streaming 64 KB when the cap
    // is 1 KB: total captured memory must equal the cap, never the
    // payload size.
    let max = 1024usize;
    let data = vec![0xCCu8; 64 * 1024];
    let mut cursor = std::io::Cursor::new(data);
    let out = super::read_bounded(&mut cursor, max, "alias", "stdout");
    assert_eq!(out.len(), max);
}

// =========================================================================
// filtered_indices
// =========================================================================

#[test]
fn filtered_indices_returns_all_when_query_is_none_or_empty() {
    let store = SnippetStore {
        snippets: vec![
            Snippet {
                name: "a".into(),
                command: "ls".into(),
                description: String::new(),
                interactive: false,
            },
            Snippet {
                name: "b".into(),
                command: "df".into(),
                description: String::new(),
                interactive: false,
            },
        ],
        ..Default::default()
    };
    assert_eq!(filtered_indices(&store, None), vec![0, 1]);
    assert_eq!(filtered_indices(&store, Some("")), vec![0, 1]);
}

#[test]
fn filtered_indices_matches_name_command_or_description_case_insensitively() {
    let store = SnippetStore {
        snippets: vec![
            Snippet {
                name: "Deploy".into(),
                command: "make".into(),
                description: "ship".into(),
                interactive: false,
            },
            Snippet {
                name: "uptime".into(),
                command: "UPTIME -p".into(),
                description: String::new(),
                interactive: false,
            },
            Snippet {
                name: "disk".into(),
                command: "df".into(),
                description: "Free space".into(),
                interactive: false,
            },
        ],
        ..Default::default()
    };
    assert_eq!(filtered_indices(&store, Some("deploy")), vec![0]);
    assert_eq!(filtered_indices(&store, Some("uptime")), vec![1]);
    assert_eq!(filtered_indices(&store, Some("free")), vec![2]);
    assert_eq!(filtered_indices(&store, Some("zzz")), Vec::<usize>::new());
}

// =========================================================================
// analyze_command (IMPACT card)
// =========================================================================

// =========================================================================
// Per-snippet saved targets (hosts= line)
// =========================================================================

#[test]
fn parse_reads_hosts_line_into_targets() {
    let store = SnippetStore::parse("[deploy]\ncommand=make\nhosts=web1, web2 ,web3\n");
    assert_eq!(store.targets_for("deploy"), &["web1", "web2", "web3"]);
}

#[test]
fn parse_without_hosts_has_no_targets() {
    let store = SnippetStore::parse("[deploy]\ncommand=make\n");
    assert!(store.targets_for("deploy").is_empty());
}

#[test]
fn set_targets_empty_clears() {
    let mut store = SnippetStore::parse("[d]\ncommand=x\nhosts=a,b\n");
    assert_eq!(store.targets_for("d").len(), 2);
    store.set_targets("d", vec![]);
    assert!(store.targets_for("d").is_empty());
}

#[test]
fn orphan_targets_dropped_for_snippet_without_command() {
    // [empty] carries hosts= but no command, so it is skipped and leaves no
    // orphan target entry.
    let store = SnippetStore::parse("[empty]\nhosts=a,b\n\n[valid]\ncommand=ls\n");
    assert!(store.get("empty").is_none());
    assert!(store.targets_for("empty").is_empty());
}

#[test]
fn remove_drops_targets() {
    let mut store = SnippetStore::parse("[d]\ncommand=x\nhosts=a,b\n");
    store.remove("d");
    assert!(store.targets_for("d").is_empty());
}

#[test]
fn targets_round_trip_through_save_and_reload() {
    // save() is demo-suppressed; hold the demo lock and force non-demo so the
    // write actually lands, without racing a parallel demo-mode test.
    let _guard = crate::demo_flag::GLOBAL_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::demo_flag::disable();

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("snippets");
    let mut store = SnippetStore::parse("[deploy]\ncommand=make\ndescription=ship\n");
    store.path_override = Some(path.clone());
    store.set_targets("deploy", vec!["web1".into(), "web2".into()]);
    store.save().expect("save");

    let reloaded = SnippetStore::parse(&std::fs::read_to_string(&path).expect("read"));
    assert_eq!(reloaded.targets_for("deploy"), &["web1", "web2"]);
    // command + description survive alongside the hosts line.
    let s = reloaded.get("deploy").expect("snippet");
    assert_eq!(s.command, "make");
    assert_eq!(s.description, "ship");
}

#[test]
fn targets_round_trip_preserves_comma_in_alias() {
    // A host whose SSH `Host` line uses a comma (e.g. `Host web1,web2`) is a
    // single concrete alias containing a comma. The comma-joined hosts= line
    // must not shred it into phantom aliases on reload.
    let _guard = crate::demo_flag::GLOBAL_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let prior_demo = crate::demo_flag::is_demo();
    crate::demo_flag::disable();

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("snippets");
    let mut store = SnippetStore::parse("[deploy]\ncommand=make\n");
    store.path_override = Some(path.clone());
    // An alias containing a comma and one containing a literal percent sign.
    store.set_targets("deploy", vec!["web1,web2".into(), "ab%2Ccd".into()]);
    store.save().expect("save");

    let reloaded = SnippetStore::parse(&std::fs::read_to_string(&path).expect("read"));
    assert_eq!(reloaded.targets_for("deploy"), &["web1,web2", "ab%2Ccd"]);

    if prior_demo {
        crate::demo_flag::enable();
    }
}

#[test]
fn count_params_matches_parse_params_len() {
    for cmd in [
        "",
        "echo hi",
        "echo {{name}}",
        "{{a}} {{b}} {{a}}",                   // dedup
        "{{x:default}} {{y}}",                 // with default
        "{{bad name}} {{ok}}",                 // invalid name skipped
        "{{a}} {{b}} {{c}} {{d}} {{e}} {{f}}", // several
    ] {
        assert_eq!(
            count_params(cmd),
            parse_params(cmd).len(),
            "count_params disagreed with parse_params for {cmd:?}"
        );
    }
}

// analyze_command / CommandImpact is covered in src/snippet_impact_tests.rs.

// =========================================================================
// substitute_params / parse_params agreement on what is a parameter
// =========================================================================

#[test]
fn test_substitute_leaves_invalid_placeholder_literal() {
    let values = std::collections::HashMap::new();
    // A space makes the name invalid: parse_params skips it, so substitute must
    // leave the literal text intact instead of rewriting it to ''.
    assert_eq!(substitute_params("echo {{a b}}", &values), "echo {{a b}}");
    // '!' is not a valid name char either.
    assert_eq!(substitute_params("echo {{a!b}}", &values), "echo {{a!b}}");
}

#[test]
fn test_parse_and_substitute_agree_on_parameters() {
    // parse_params reports no parameter for an invalid name, and substitute
    // must not silently rewrite that placeholder.
    assert!(parse_params("echo {{a b}}").is_empty());
    let values = std::collections::HashMap::new();
    assert_eq!(substitute_params("echo {{a b}}", &values), "echo {{a b}}");
    // A valid name is still substituted (and shell-escaped).
    assert_eq!(parse_params("echo {{name}}").len(), 1);
    let mut v = std::collections::HashMap::new();
    v.insert("name".to_string(), "hi there".to_string());
    assert_eq!(substitute_params("echo {{name}}", &v), "echo 'hi there'");
}

#[test]
fn test_validate_param_name_uses_message_module() {
    assert_eq!(
        validate_param_name("").unwrap_err(),
        crate::messages::SNIPPET_PARAM_NAME_EMPTY
    );
    assert_eq!(
        validate_param_name("a b").unwrap_err(),
        crate::messages::snippet_param_name_invalid("a b")
    );
}

// =========================================================================
// sanitize_output: 8-bit C1 control sequences
// =========================================================================

#[test]
fn test_sanitize_strips_8bit_csi() {
    // U+009B is the single-char (8-bit) CSI introducer. Its parameter bytes
    // (31m / 0m) must be consumed, not leaked into the TUI as literal text.
    assert_eq!(sanitize_output("\u{009b}31mred\u{009b}0m"), "red");
}

// =========================================================================
// read_pipe_capped: byte cap and line cap
// =========================================================================

/// Yields `remaining` bytes of `b'x'` with no newline, to exercise the byte cap
/// against a single unterminated megastream without allocating it up front.
struct NoNewlineReader {
    remaining: usize,
}

impl std::io::Read for NoNewlineReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.remaining == 0 {
            return Ok(0);
        }
        let n = buf.len().min(self.remaining);
        for b in &mut buf[..n] {
            *b = b'x';
        }
        self.remaining -= n;
        Ok(n)
    }
}

#[test]
fn read_pipe_capped_bounds_single_unterminated_line() {
    // A hostile remote streaming one huge no-newline line must not grow the
    // buffer past the byte cap (the line cap alone would never fire).
    let reader = NoNewlineReader {
        remaining: SSH_OUTPUT_MAX_BYTES + 1_000_000,
    };
    let out = read_pipe_capped(reader, "host", "stdout");
    assert_eq!(out.len(), SSH_OUTPUT_MAX_BYTES);
    assert!(!out.contains("truncated"));
}

#[test]
fn read_pipe_capped_truncates_at_line_cap() {
    let input = "a\n".repeat(10_050);
    let out = read_pipe_capped(input.as_bytes(), "host", "stdout");
    assert!(out.contains("[Output truncated at 10,000 lines]"));
    // 10,000 kept "a" lines + the truncation marker line.
    assert_eq!(out.matches('\n').count(), 10_000);
}
