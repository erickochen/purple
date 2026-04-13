use super::*;

// =========================================================================
// shell_escape
// =========================================================================

#[test]
fn test_shell_escape_simple() {
    assert_eq!(shell_escape("/home/user"), "'/home/user'");
}

#[test]
fn test_shell_escape_with_single_quote() {
    assert_eq!(shell_escape("/home/it's"), "'/home/it'\\''s'");
}

#[test]
fn test_shell_escape_with_spaces() {
    assert_eq!(shell_escape("/home/my dir"), "'/home/my dir'");
}

// =========================================================================
// parse_ls_output
// =========================================================================

#[test]
fn test_parse_ls_basic() {
    let output = "\
total 24
drwxr-xr-x  2 user user 4096 Jan  1 12:00 subdir
-rw-r--r--  1 user user  512 Jan  1 12:00 file.txt
-rw-r--r--  1 user user 1.1K Jan  1 12:00 big.log
";
    let entries = parse_ls_output(output, true, BrowserSort::Name);
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].name, "subdir");
    assert!(entries[0].is_dir);
    assert_eq!(entries[0].size, None);
    // Files sorted alphabetically after dirs
    assert_eq!(entries[1].name, "big.log");
    assert!(!entries[1].is_dir);
    assert_eq!(entries[1].size, Some(1126)); // 1.1 * 1024
    assert_eq!(entries[2].name, "file.txt");
    assert!(!entries[2].is_dir);
    assert_eq!(entries[2].size, Some(512));
}

#[test]
fn test_parse_ls_hidden_filter() {
    let output = "\
total 8
-rw-r--r--  1 user user  100 Jan  1 12:00 .hidden
-rw-r--r--  1 user user  200 Jan  1 12:00 visible
";
    let entries = parse_ls_output(output, false, BrowserSort::Name);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "visible");

    let entries = parse_ls_output(output, true, BrowserSort::Name);
    assert_eq!(entries.len(), 2);
}

#[test]
fn test_parse_ls_symlink_to_file_dereferenced() {
    // With -L, symlink to file appears as regular file
    let output = "\
total 4
-rw-r--r--  1 user user   11 Jan  1 12:00 link
";
    let entries = parse_ls_output(output, true, BrowserSort::Name);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "link");
    assert!(!entries[0].is_dir);
}

#[test]
fn test_parse_ls_symlink_to_dir_dereferenced() {
    // With -L, symlink to directory appears as directory
    let output = "\
total 4
drwxr-xr-x  3 user user 4096 Jan  1 12:00 link
";
    let entries = parse_ls_output(output, true, BrowserSort::Name);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "link");
    assert!(entries[0].is_dir);
}

#[test]
fn test_parse_ls_filename_with_spaces() {
    let output = "\
total 4
-rw-r--r--  1 user user  100 Jan  1 12:00 my file name.txt
";
    let entries = parse_ls_output(output, true, BrowserSort::Name);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "my file name.txt");
}

#[test]
fn test_parse_ls_empty() {
    let output = "total 0\n";
    let entries = parse_ls_output(output, true, BrowserSort::Name);
    assert!(entries.is_empty());
}

// =========================================================================
// parse_human_size
// =========================================================================

#[test]
fn test_parse_human_size() {
    assert_eq!(parse_human_size("512"), 512);
    assert_eq!(parse_human_size("1.0K"), 1024);
    assert_eq!(parse_human_size("1.5M"), 1572864);
    assert_eq!(parse_human_size("2.0G"), 2147483648);
}

// =========================================================================
// format_size
// =========================================================================

#[test]
fn test_format_size() {
    assert_eq!(format_size(0), "0 B");
    assert_eq!(format_size(512), "512 B");
    assert_eq!(format_size(1024), "1.0 KB");
    assert_eq!(format_size(1536), "1.5 KB");
    assert_eq!(format_size(1048576), "1.0 MB");
    assert_eq!(format_size(1073741824), "1.0 GB");
}

// =========================================================================
// build_scp_args
// =========================================================================

#[test]
fn test_build_scp_args_upload() {
    let args = build_scp_args(
        "myhost",
        BrowserPane::Local,
        Path::new("/home/user/docs"),
        "/remote/path/",
        &["file.txt".to_string()],
        false,
    );
    assert_eq!(
        args,
        vec!["--", "/home/user/docs/file.txt", "myhost:/remote/path/",]
    );
}

#[test]
fn test_build_scp_args_download() {
    let args = build_scp_args(
        "myhost",
        BrowserPane::Remote,
        Path::new("/home/user/docs"),
        "/remote/path",
        &["file.txt".to_string()],
        false,
    );
    assert_eq!(
        args,
        vec!["--", "myhost:/remote/path/file.txt", "/home/user/docs",]
    );
}

#[test]
fn test_build_scp_args_spaces_in_path() {
    let args = build_scp_args(
        "myhost",
        BrowserPane::Remote,
        Path::new("/local"),
        "/remote/my path",
        &["my file.txt".to_string()],
        false,
    );
    // No shell escaping: Command::arg() passes paths literally
    assert_eq!(
        args,
        vec!["--", "myhost:/remote/my path/my file.txt", "/local",]
    );
}

#[test]
fn test_build_scp_args_with_dirs() {
    let args = build_scp_args(
        "myhost",
        BrowserPane::Local,
        Path::new("/local"),
        "/remote/",
        &["mydir".to_string()],
        true,
    );
    assert_eq!(args[0], "-r");
}

// =========================================================================
// list_local
// =========================================================================

#[test]
fn test_list_local_sorts_dirs_first() {
    let base = std::env::temp_dir().join(format!("purple_fb_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    std::fs::create_dir(base.join("zdir")).unwrap();
    std::fs::write(base.join("afile.txt"), "hello").unwrap();
    std::fs::write(base.join("bfile.txt"), "world").unwrap();

    let entries = list_local(&base, true, BrowserSort::Name).unwrap();
    assert_eq!(entries.len(), 3);
    assert!(entries[0].is_dir);
    assert_eq!(entries[0].name, "zdir");
    assert_eq!(entries[1].name, "afile.txt");
    assert_eq!(entries[2].name, "bfile.txt");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn test_list_local_hidden() {
    let base = std::env::temp_dir().join(format!("purple_fb_hidden_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join(".hidden"), "").unwrap();
    std::fs::write(base.join("visible"), "").unwrap();

    let entries = list_local(&base, false, BrowserSort::Name).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "visible");

    let entries = list_local(&base, true, BrowserSort::Name).unwrap();
    assert_eq!(entries.len(), 2);

    let _ = std::fs::remove_dir_all(&base);
}

// =========================================================================
// filter_ssh_warnings
// =========================================================================

#[test]
fn test_filter_ssh_warnings_filters_warnings() {
    let stderr = "\
** WARNING: connection is not using a post-quantum key exchange algorithm.
** This session may be vulnerable to \"store now, decrypt later\" attacks.
** The server may need to be upgraded. See https://openssh.com/pq.html
scp: '/root/file.rpm': No such file or directory";
    assert_eq!(
        filter_ssh_warnings(stderr),
        "scp: '/root/file.rpm': No such file or directory"
    );
}

#[test]
fn test_filter_ssh_warnings_keeps_plain_error() {
    let stderr = "scp: /etc/shadow: Permission denied\n";
    assert_eq!(
        filter_ssh_warnings(stderr),
        "scp: /etc/shadow: Permission denied"
    );
}

#[test]
fn test_filter_ssh_warnings_empty() {
    assert_eq!(filter_ssh_warnings(""), "");
    assert_eq!(filter_ssh_warnings("  \n  \n"), "");
}

#[test]
fn test_filter_ssh_warnings_warning_prefix() {
    let stderr = "Warning: Permanently added '10.0.0.1' to the list of known hosts.\nPermission denied (publickey).";
    assert_eq!(
        filter_ssh_warnings(stderr),
        "Permission denied (publickey)."
    );
}

#[test]
fn test_filter_ssh_warnings_lowercase_see_https() {
    let stderr = "For details, see https://openssh.com/legacy.html\nConnection refused";
    assert_eq!(filter_ssh_warnings(stderr), "Connection refused");
}

#[test]
fn test_filter_ssh_warnings_only_warnings() {
    let stderr = "** WARNING: connection is not using a post-quantum key exchange algorithm.\n** This session may be vulnerable to \"store now, decrypt later\" attacks.\n** The server may need to be upgraded. See https://openssh.com/pq.html";
    assert_eq!(filter_ssh_warnings(stderr), "");
}

// =========================================================================
// approximate_epoch (known dates)
// =========================================================================

#[test]
fn test_approximate_epoch_known_dates() {
    // 2024-01-01 00:00 UTC = 1704067200
    let ts = approximate_epoch(2024, 0, 1, 0, 0);
    assert_eq!(ts, 1704067200);
    // 2000-01-01 00:00 UTC = 946684800
    let ts = approximate_epoch(2000, 0, 1, 0, 0);
    assert_eq!(ts, 946684800);
    // 1970-01-01 00:00 UTC = 0
    assert_eq!(approximate_epoch(1970, 0, 1, 0, 0), 0);
}

#[test]
fn test_approximate_epoch_leap_year() {
    // 2024-02-29 should differ from 2024-03-01 by 86400
    let feb29 = approximate_epoch(2024, 1, 29, 0, 0);
    let mar01 = approximate_epoch(2024, 2, 1, 0, 0);
    assert_eq!(mar01 - feb29, 86400);
}

// =========================================================================
// epoch_to_year
// =========================================================================

#[test]
fn test_epoch_to_year() {
    assert_eq!(epoch_to_year(0), 1970);
    // 2023-01-01 00:00 UTC = 1672531200
    assert_eq!(epoch_to_year(1672531200), 2023);
    // 2024-01-01 00:00 UTC = 1704067200
    assert_eq!(epoch_to_year(1704067200), 2024);
    // 2024-12-31 23:59:59
    assert_eq!(epoch_to_year(1735689599), 2024);
    // 2025-01-01 00:00:00
    assert_eq!(epoch_to_year(1735689600), 2025);
}

// =========================================================================
// parse_ls_date
// =========================================================================

#[test]
fn test_parse_ls_date_recent_format() {
    // "Jan 15 12:34" - should return a timestamp
    let ts = parse_ls_date("Jan", "15", "12:34");
    assert!(ts.is_some());
    let ts = ts.unwrap();
    // Should be within the last year
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    assert!(ts <= now + 86400);
    assert!(ts > now - 366 * 86400);
}

#[test]
fn test_parse_ls_date_old_format() {
    let ts = parse_ls_date("Mar", "5", "2023");
    assert!(ts.is_some());
    let ts = ts.unwrap();
    // Should be in 2023
    assert_eq!(epoch_to_year(ts), 2023);
}

#[test]
fn test_parse_ls_date_invalid_month() {
    assert!(parse_ls_date("Foo", "1", "12:00").is_none());
}

#[test]
fn test_parse_ls_date_invalid_day() {
    assert!(parse_ls_date("Jan", "0", "12:00").is_none());
    assert!(parse_ls_date("Jan", "32", "12:00").is_none());
}

#[test]
fn test_parse_ls_date_invalid_year() {
    assert!(parse_ls_date("Jan", "1", "1969").is_none());
}

// =========================================================================
// format_relative_time
// =========================================================================

#[test]
fn test_format_relative_time_ranges() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    assert_eq!(format_relative_time(now), "just now");
    assert_eq!(format_relative_time(now - 30), "just now");
    assert_eq!(format_relative_time(now - 120), "2m ago");
    assert_eq!(format_relative_time(now - 7200), "2h ago");
    assert_eq!(format_relative_time(now - 86400 * 3), "3d ago");
}

#[test]
fn test_format_relative_time_old_date() {
    // A date far in the past should show short date format
    let old = approximate_epoch(2020, 5, 15, 0, 0);
    let result = format_relative_time(old);
    assert!(
        result.contains("2020"),
        "Expected year in '{}' for old date",
        result
    );
}

#[test]
fn test_format_relative_time_future() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    // Future timestamp should not panic and should show date
    let result = format_relative_time(now + 86400 * 30);
    assert!(!result.is_empty());
}

// =========================================================================
// format_short_date
// =========================================================================

#[test]
fn test_format_short_date_different_year() {
    let ts = approximate_epoch(2020, 2, 15, 0, 0); // Mar 15 2020
    let result = format_short_date(ts);
    assert!(result.contains("2020"), "Expected year in '{}'", result);
    assert!(result.starts_with("Mar"), "Expected Mar in '{}'", result);
}

#[test]
fn test_format_short_date_leap_year() {
    // Mar 1 2024 (leap year, different year) should show "Mar 2024"
    let ts = approximate_epoch(2024, 2, 1, 0, 0);
    let result = format_short_date(ts);
    assert!(result.starts_with("Mar"), "Expected Mar in '{}'", result);
    assert!(result.contains("2024"), "Expected 2024 in '{}'", result);
    // Verify Feb 29 and Mar 1 are distinct days (86400 apart)
    let feb29 = approximate_epoch(2024, 1, 29, 12, 0);
    let mar01 = approximate_epoch(2024, 2, 1, 12, 0);
    let feb29_date = format_short_date(feb29);
    let mar01_date = format_short_date(mar01);
    assert!(
        feb29_date.starts_with("Feb"),
        "Expected Feb in '{}'",
        feb29_date
    );
    assert!(
        mar01_date.starts_with("Mar"),
        "Expected Mar in '{}'",
        mar01_date
    );
}

// =========================================================================
// sort_entries (date mode)
// =========================================================================

#[test]
fn test_sort_entries_date_dirs_first_newest_first() {
    let mut entries = vec![
        FileEntry {
            name: "old.txt".into(),
            is_dir: false,
            size: Some(100),
            modified: Some(1000),
        },
        FileEntry {
            name: "new.txt".into(),
            is_dir: false,
            size: Some(200),
            modified: Some(3000),
        },
        FileEntry {
            name: "mid.txt".into(),
            is_dir: false,
            size: Some(150),
            modified: Some(2000),
        },
        FileEntry {
            name: "adir".into(),
            is_dir: true,
            size: None,
            modified: Some(500),
        },
    ];
    sort_entries(&mut entries, BrowserSort::Date);
    assert!(entries[0].is_dir);
    assert_eq!(entries[0].name, "adir");
    assert_eq!(entries[1].name, "new.txt");
    assert_eq!(entries[2].name, "mid.txt");
    assert_eq!(entries[3].name, "old.txt");
}

#[test]
fn test_sort_entries_name_mode() {
    let mut entries = vec![
        FileEntry {
            name: "zebra.txt".into(),
            is_dir: false,
            size: Some(100),
            modified: Some(3000),
        },
        FileEntry {
            name: "alpha.txt".into(),
            is_dir: false,
            size: Some(200),
            modified: Some(1000),
        },
        FileEntry {
            name: "mydir".into(),
            is_dir: true,
            size: None,
            modified: Some(2000),
        },
    ];
    sort_entries(&mut entries, BrowserSort::Name);
    assert!(entries[0].is_dir);
    assert_eq!(entries[1].name, "alpha.txt");
    assert_eq!(entries[2].name, "zebra.txt");
}

// =========================================================================
// parse_ls_output with modified field
// =========================================================================

#[test]
fn test_parse_ls_output_populates_modified() {
    let output = "\
total 4
-rw-r--r--  1 user user  512 Jan  1 12:00 file.txt
";
    let entries = parse_ls_output(output, true, BrowserSort::Name);
    assert_eq!(entries.len(), 1);
    assert!(
        entries[0].modified.is_some(),
        "modified should be populated"
    );
}

#[test]
fn test_parse_ls_output_date_sort() {
    // Use year format to avoid ambiguity with current date
    let output = "\
total 12
-rw-r--r--  1 user user  100 Jan  1  2020 old.txt
-rw-r--r--  1 user user  200 Jun 15  2023 new.txt
-rw-r--r--  1 user user  150 Mar  5  2022 mid.txt
";
    let entries = parse_ls_output(output, true, BrowserSort::Date);
    assert_eq!(entries.len(), 3);
    // Should be sorted newest first (2023 > 2022 > 2020)
    assert_eq!(entries[0].name, "new.txt");
    assert_eq!(entries[1].name, "mid.txt");
    assert_eq!(entries[2].name, "old.txt");
}

// =========================================================================
// list_local with modified field
// =========================================================================

#[test]
fn test_list_local_populates_modified() {
    let base = std::env::temp_dir().join(format!("purple_fb_mtime_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("test.txt"), "hello").unwrap();

    let entries = list_local(&base, true, BrowserSort::Name).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(
        entries[0].modified.is_some(),
        "modified should be populated for local files"
    );

    let _ = std::fs::remove_dir_all(&base);
}

// =========================================================================
// epoch_to_year boundary
// =========================================================================

#[test]
fn test_epoch_to_year_2100_boundary() {
    let ts_2100 = approximate_epoch(2100, 0, 1, 0, 0);
    assert_eq!(epoch_to_year(ts_2100), 2100);
    assert_eq!(epoch_to_year(ts_2100 - 1), 2099);
    let mid_2100 = approximate_epoch(2100, 5, 15, 12, 0);
    assert_eq!(epoch_to_year(mid_2100), 2100);
}

// =========================================================================
// parse_ls_date edge cases
// =========================================================================

#[test]
fn test_parse_ls_date_midnight() {
    let ts = parse_ls_date("Jan", "1", "00:00");
    assert!(ts.is_some(), "00:00 should parse successfully");
    let ts = ts.unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    assert!(ts <= now + 86400);
    assert!(ts > now - 366 * 86400);
}

// =========================================================================
// sort_entries edge cases
// =========================================================================

#[test]
fn test_sort_entries_date_with_none_modified() {
    let mut entries = vec![
        FileEntry {
            name: "known.txt".into(),
            is_dir: false,
            size: Some(100),
            modified: Some(5000),
        },
        FileEntry {
            name: "unknown.txt".into(),
            is_dir: false,
            size: Some(200),
            modified: None,
        },
        FileEntry {
            name: "recent.txt".into(),
            is_dir: false,
            size: Some(300),
            modified: Some(9000),
        },
    ];
    sort_entries(&mut entries, BrowserSort::Date);
    assert_eq!(entries[0].name, "recent.txt");
    assert_eq!(entries[1].name, "known.txt");
    assert_eq!(entries[2].name, "unknown.txt");
}

#[test]
fn test_sort_entries_date_asc_oldest_first() {
    let mut entries = vec![
        FileEntry {
            name: "old.txt".into(),
            is_dir: false,
            size: Some(100),
            modified: Some(1000),
        },
        FileEntry {
            name: "new.txt".into(),
            is_dir: false,
            size: Some(200),
            modified: Some(3000),
        },
        FileEntry {
            name: "mid.txt".into(),
            is_dir: false,
            size: Some(150),
            modified: Some(2000),
        },
        FileEntry {
            name: "adir".into(),
            is_dir: true,
            size: None,
            modified: Some(500),
        },
    ];
    sort_entries(&mut entries, BrowserSort::DateAsc);
    assert!(entries[0].is_dir);
    assert_eq!(entries[0].name, "adir");
    assert_eq!(entries[1].name, "old.txt");
    assert_eq!(entries[2].name, "mid.txt");
    assert_eq!(entries[3].name, "new.txt");
}

#[test]
fn test_sort_entries_date_asc_none_modified_sorts_to_end() {
    let mut entries = vec![
        FileEntry {
            name: "known.txt".into(),
            is_dir: false,
            size: Some(100),
            modified: Some(5000),
        },
        FileEntry {
            name: "unknown.txt".into(),
            is_dir: false,
            size: Some(200),
            modified: None,
        },
        FileEntry {
            name: "old.txt".into(),
            is_dir: false,
            size: Some(300),
            modified: Some(1000),
        },
    ];
    sort_entries(&mut entries, BrowserSort::DateAsc);
    assert_eq!(entries[0].name, "old.txt");
    assert_eq!(entries[1].name, "known.txt");
    assert_eq!(entries[2].name, "unknown.txt"); // None sorts to end
}

#[test]
fn test_parse_ls_output_date_asc_sort() {
    let output = "\
total 12
-rw-r--r--  1 user user  100 Jan  1  2020 old.txt
-rw-r--r--  1 user user  200 Jun 15  2023 new.txt
-rw-r--r--  1 user user  150 Mar  5  2022 mid.txt
";
    let entries = parse_ls_output(output, true, BrowserSort::DateAsc);
    assert_eq!(entries.len(), 3);
    // Should be sorted oldest first (2020 < 2022 < 2023)
    assert_eq!(entries[0].name, "old.txt");
    assert_eq!(entries[1].name, "mid.txt");
    assert_eq!(entries[2].name, "new.txt");
}

#[test]
fn test_sort_entries_date_multiple_dirs() {
    let mut entries = vec![
        FileEntry {
            name: "old_dir".into(),
            is_dir: true,
            size: None,
            modified: Some(1000),
        },
        FileEntry {
            name: "new_dir".into(),
            is_dir: true,
            size: None,
            modified: Some(3000),
        },
        FileEntry {
            name: "mid_dir".into(),
            is_dir: true,
            size: None,
            modified: Some(2000),
        },
        FileEntry {
            name: "file.txt".into(),
            is_dir: false,
            size: Some(100),
            modified: Some(5000),
        },
    ];
    sort_entries(&mut entries, BrowserSort::Date);
    assert!(entries[0].is_dir);
    assert_eq!(entries[0].name, "new_dir");
    assert_eq!(entries[1].name, "mid_dir");
    assert_eq!(entries[2].name, "old_dir");
    assert_eq!(entries[3].name, "file.txt");
}

// =========================================================================
// format_relative_time boundaries
// =========================================================================

#[test]
fn test_format_relative_time_exactly_60s() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    assert_eq!(format_relative_time(now - 60), "1m ago");
    assert_eq!(format_relative_time(now - 59), "just now");
}

// =========================================================================
// parse_ls_output date sort with dirs
// =========================================================================

#[test]
fn test_parse_ls_output_date_sort_with_dirs() {
    let output = "\
total 16
drwxr-xr-x  2 user user 4096 Jan  1  2020 old_dir
-rw-r--r--  1 user user  200 Jun 15  2023 new_file.txt
drwxr-xr-x  2 user user 4096 Dec  1  2023 new_dir
-rw-r--r--  1 user user  100 Mar  5  2022 old_file.txt
";
    let entries = parse_ls_output(output, true, BrowserSort::Date);
    assert_eq!(entries.len(), 4);
    assert!(entries[0].is_dir);
    assert_eq!(entries[0].name, "new_dir");
    assert!(entries[1].is_dir);
    assert_eq!(entries[1].name, "old_dir");
    assert_eq!(entries[2].name, "new_file.txt");
    assert_eq!(entries[3].name, "old_file.txt");
}
