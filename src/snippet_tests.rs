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
    });
    store.set(Snippet {
        name: "uptime".to_string(),
        command: "uptime".to_string(),
        description: String::new(),
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
    let dir = std::env::temp_dir().join(format!("purple_snippet_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("snippets");

    let mut store = SnippetStore {
        path_override: Some(path.clone()),
        ..Default::default()
    };
    store.set(Snippet {
        name: "test".to_string(),
        command: "echo hello".to_string(),
        description: "Test snippet".to_string(),
    });
    store.save().unwrap();

    // Read back
    let content = std::fs::read_to_string(&path).unwrap();
    let reloaded = SnippetStore::parse(&content);
    assert_eq!(reloaded.snippets.len(), 1);
    assert_eq!(reloaded.snippets[0].name, "test");
    assert_eq!(reloaded.snippets[0].command, "echo hello");

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
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
    });
    store.set(Snippet {
        name: "b".into(),
        command: "2".into(),
        description: String::new(),
    });
    store.set(Snippet {
        name: "c".into(),
        command: "3".into(),
        description: String::new(),
    });
    store.set(Snippet {
        name: "b".into(),
        command: "updated".into(),
        description: String::new(),
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
