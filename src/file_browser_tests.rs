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
// remote listing: real `ls -lnhAL` output from GNU coreutils and BusyBox
// =========================================================================

/// What GNU coreutils `ls -lnhAL` prints for a directory holding a name
/// with a leading space, a dangling symlink, a device file, a directory
/// with a space and a name with a trailing space. It exits 1.
const GNU_LISTING: &str = concat!(
    "total 16K\n",
    "-rw-r--r-- 1    0    0    2 Nov  3 17:25  leading\n",
    "l????????? ? ?    ?       ?            ? dangling\n",
    "crw-r--r-- 1    0    0 1, 3 Nov  3 17:25 nulldev\n",
    "-rw-r--r-- 1 1500 1500    3 Nov  3 17:25 small.txt\n",
    "drwxr-xr-x 2    0    0 4.0K Nov  3 17:25 sub dir\n",
    "-rw-r--r-- 1    0    0    2 Nov  3 17:25 trailing \n",
);

/// The same directory listed by BusyBox, which leaves the dangling
/// symlink out and also exits 1.
const BUSYBOX_LISTING: &str = concat!(
    "total 16K    \n",
    "-rw-r--r--    1 0        0              2 Nov  3 17:25  leading\n",
    "crw-r--r--    1 0        0           1,   3 Nov  3 17:25 nulldev\n",
    "-rw-r--r--    1 1500     1500           3 Nov  3 17:25 small.txt\n",
    "drwxr-xr-x    2 0        0           4.0K Nov  3 17:25 sub dir\n",
    "-rw-r--r--    1 0        0              2 Nov  3 17:25 trailing \n",
);

fn names(entries: &[FileEntry]) -> Vec<&str> {
    entries.iter().map(|e| e.name.as_str()).collect()
}

#[test]
fn remote_listing_asks_for_numeric_owners_and_literal_names() {
    assert_eq!(
        remote_listing_command("/home/my dir"),
        "LC_ALL=C QUOTING_STYLE=literal TIME_STYLE=locale ls -lnhAL '/home/my dir'"
    );
}

#[test]
fn gnu_and_busybox_listings_keep_every_name_intact() {
    for (label, listing) in [("gnu", GNU_LISTING), ("busybox", BUSYBOX_LISTING)] {
        let entries = parse_ls_output(listing, true, BrowserSort::Name);
        assert_eq!(
            names(&entries),
            vec!["sub dir", " leading", "nulldev", "small.txt", "trailing "],
            "{label}"
        );
        assert!(entries[0].is_dir, "{label}");
        let small = entries.iter().find(|e| e.name == "small.txt").unwrap();
        assert_eq!(small.size, Some(3), "{label}");
        assert!(small.modified.is_some(), "{label}");
    }
}

#[test]
fn a_device_file_takes_its_name_and_date_after_major_and_minor() {
    for listing in [GNU_LISTING, BUSYBOX_LISTING] {
        let entries = parse_ls_output(listing, true, BrowserSort::Name);
        let dev = entries.iter().find(|e| e.name == "nulldev").unwrap();
        assert!(!dev.is_dir);
        assert_eq!(dev.size, None);
        assert!(dev.modified.is_some());
    }
}

#[test]
fn a_broken_symlink_is_left_out_whatever_its_name() {
    let output = concat!(
        "total 8.0K\n",
        "l????????? ? ? ? ?            ? dangling link\n",
        "-rw-r--r-- 1 0 0 2 Jan  1  2020 old file\n",
    );
    let entries = parse_ls_output(output, true, BrowserSort::Name);
    assert_eq!(names(&entries), vec!["old file"]);
    assert_eq!(epoch_to_year(entries[0].modified.unwrap()), 2020);
}

#[test]
fn other_entries_ls_could_not_stat_keep_their_name() {
    // A stale network mount and a file in a folder without search permission.
    let output = concat!(
        "total 4.0K\n",
        "drwxr-xr-x 2 0 0 4.0K Nov  3 17:25 backup\n",
        "d????????? ? ? ? ?            ? nas share\n",
        "-????????? ? ? ? ?            ? secret\n",
    );
    let entries = parse_ls_output(output, true, BrowserSort::Name);
    assert_eq!(names(&entries), vec!["backup", "nas share", "secret"]);
    let nas = &entries[1];
    assert!(nas.is_dir);
    assert_eq!((nas.size, nas.modified), (None, None));
    assert!(!entries[2].is_dir);
}

#[test]
fn unstat_marks_widen_when_another_entry_has_an_acl() {
    // GNU prints one more `?` when an entry carries an ACL or SELinux label.
    let output = concat!(
        "total 4.0K\n",
        "-rw-r--r--+ 1 0 0    2 Nov  3 17:25 acl file\n",
        "d?????????? ? ? ?    ?            ? nas share\n",
        "l?????????? ? ? ?    ?            ? gone\n",
    );
    let entries = parse_ls_output(output, true, BrowserSort::Name);
    assert_eq!(names(&entries), vec!["nas share", "acl file"]);
    assert!(entries[0].is_dir);
}

#[test]
fn an_entry_of_unknown_type_is_left_out() {
    // A broken symlink on a filesystem that gives no entry type.
    let output = "total 0\n?????????? ? ? ? ?            ? x\n";
    assert!(parse_ls_output(output, true, BrowserSort::Name).is_empty());
}

#[test]
fn a_broken_symlink_printed_as_a_link_is_left_out() {
    // What FreeBSD-family `ls -L` prints for broken links, with exit 0.
    let output = concat!(
        "total 0 B\n",
        "lrwxrwxrwx  1 0 0    7B Nov  3 18:35 abs -> /nope/x\n",
        "lrwxrwxrwx  1 0 0    7B Nov  3 18:35 rel -> missing\n",
    );
    assert!(listing_is_usable(Some(0), output, ""));
    assert!(parse_ls_output(output, true, BrowserSort::Name).is_empty());
}

#[test]
fn a_folder_that_is_itself_a_dangling_link_reads_as_an_error() {
    // What macOS prints when the listed path is a link to a removed folder.
    let stdout = "lrwxr-xr-x@ 1 501  0   122B Nov  3 19:21 /srv/current -> /srv/releases/v42\n";
    let stderr = "ls: current: No such file or directory\n";
    assert!(parse_ls_output(stdout, true, BrowserSort::Name).is_empty());
    assert!(!listing_is_usable(Some(1), stdout, stderr));
}

#[test]
fn a_bsd_device_with_a_hex_number_has_no_size() {
    // What macOS prints for a device with a large minor number.
    let output = "crw-rw-rw-  1 0    0    0x1d000000 Oct 27 22:11 aes_0\n";
    let entries = parse_ls_output(output, true, BrowserSort::Name);
    assert_eq!(names(&entries), vec!["aes_0"]);
    assert_eq!(entries[0].size, None);
}

#[test]
fn lines_that_are_not_entries_are_ignored() {
    let output = concat!(
        "total 4\n",
        "welcome to the build server, please be nice to it today ok\n",
        "-rw-r--r-- 1 0 0 2 Jan  1  2020 kept\n",
    );
    let entries = parse_ls_output(output, true, BrowserSort::Name);
    assert_eq!(names(&entries), vec!["kept"]);
}

#[test]
fn a_bsd_size_with_a_byte_suffix_parses() {
    let output = "-rw-r--r--@ 1 501  0     3B Nov  3 18:25 small.txt\n";
    let entries = parse_ls_output(output, true, BrowserSort::Name);
    assert_eq!(entries[0].size, Some(3));
}

#[test]
fn a_partial_listing_counts_when_it_holds_an_entry() {
    let stderr = "ls: cannot access '/probe/dangling': No such file or directory\n";
    assert!(listing_is_usable(Some(1), GNU_LISTING, stderr));
    assert!(listing_is_usable(Some(1), BUSYBOX_LISTING, stderr));
}

#[test]
fn a_directory_busybox_could_not_open_reads_as_an_error() {
    // BusyBox prints `total 0` and exits 1 for a directory it cannot open.
    assert!(!listing_is_usable(
        Some(1),
        "total 0      \n",
        "ls: can't open '/probe/locked': Permission denied\n"
    ));
}

#[test]
fn a_directory_holding_only_broken_symlinks_lists_as_empty() {
    // GNU and uutils print the link, BusyBox leaves it out. Both exit 1.
    let gnu = (
        "total 0\nl????????? ? ? ? ?            ? gone\n",
        "ls: cannot access '/only/gone': No such file or directory\n",
    );
    let busybox = (
        "total 0      \n",
        "ls: /only/gone: No such file or directory\n",
    );
    for (stdout, stderr) in [gnu, busybox] {
        assert!(listing_is_usable(Some(1), stdout, stderr), "{stdout}");
        assert!(parse_ls_output(stdout, true, BrowserSort::Name).is_empty());
    }
}

#[test]
fn a_folder_without_search_permission_shows_names_or_the_error() {
    // What GNU prints to a user who may read the folder but not search it.
    let with_file = concat!(
        "total 0\n",
        "l????????? ? ? ? ?            ? gone\n",
        "-????????? ? ? ? ?            ? secret\n",
    );
    let only_links = "total 0\nl????????? ? ? ? ?            ? gone\n";
    let denied = "ls: cannot access '/rx/gone': Permission denied\n";
    assert!(listing_is_usable(Some(1), with_file, denied));
    assert_eq!(
        names(&parse_ls_output(with_file, true, BrowserSort::Name)),
        vec!["secret"]
    );
    assert!(!listing_is_usable(Some(1), only_links, denied));
}

#[test]
fn a_folder_holding_only_symlink_loops_lists_as_empty() {
    // glibc and musl word the loop error differently.
    let gnu = (
        "total 0\nl????????? ? ? ? ?            ? loop\n",
        "ls: cannot access '/d/loop': Too many levels of symbolic links\n",
    );
    let busybox = ("total 0      \n", "ls: /d/loop: Symbolic link loop\n");
    let uutils = (
        "total 0\nl????????? ? ? ? ?            ? a\n",
        concat!(
            "ls: unknown io error: '/tmp/w/loop/a', 'Os { code: 40, kind: FilesystemLoop, ",
            "message: \"Too many levels of symbolic links\" }'\n",
        ),
    );
    for (stdout, stderr) in [gnu, busybox, uutils] {
        assert!(listing_is_usable(Some(1), stdout, stderr), "{stderr}");
    }
}

#[test]
fn a_folder_holding_only_links_through_a_file_lists_as_empty() {
    let gnu = (
        "total 0\nl????????? ? ? ? ?            ? nd\n",
        "ls: cannot access '/w/through/nd': Not a directory\n",
    );
    let busybox = ("total 0      \n", "ls: /w/through/nd: Not a directory\n");
    let uutils = (
        "total 0\nl????????? ? ? ? ?            ? nd\n",
        concat!(
            "ls: unknown io error: '/w/through/nd', 'Os { code: 20, kind: NotADirectory, ",
            "message: \"Not a directory\" }'\n",
        ),
    );
    for (stdout, stderr) in [gnu, busybox, uutils] {
        assert!(listing_is_usable(Some(1), stdout, stderr), "{stderr}");
    }
}

#[test]
fn rc_file_noise_on_stderr_does_not_hide_a_broken_symlink_folder() {
    let stderr = concat!(
        "stty: 'standard input': Inappropriate ioctl for device\n",
        "ls: cannot access '/only/gone': No such file or directory\n",
    );
    assert!(listing_is_usable(Some(1), "total 0\n", stderr));
}

#[test]
fn ssh_warnings_do_not_hide_a_broken_symlink_folder() {
    let stderr = concat!(
        "** WARNING: connection is not using a post-quantum key exchange algorithm.\n",
        "ls: cannot access '/only/gone': No such file or directory\n",
    );
    assert!(listing_is_usable(Some(1), "total 0\n", stderr));
    assert!(!listing_is_usable(Some(1), "total 0\n", "** WARNING: x\n"));
}

#[test]
fn solaris_doors_and_mandatory_locks_still_list() {
    let output = concat!(
        "Dr--r--r--   1 0 0 0 Nov  3 17:25 name_service_door\n",
        "-rw-r-lr--   1 0 0 5 Nov  3 17:25 locked.db\n",
    );
    let entries = parse_ls_output(output, true, BrowserSort::Name);
    assert_eq!(names(&entries), vec!["locked.db", "name_service_door"]);
}

#[test]
fn a_clean_run_counts_even_when_empty() {
    assert!(listing_is_usable(Some(0), "total 0\n", ""));
}

#[test]
fn other_exit_codes_read_as_an_error() {
    // GNU exits 2 for a missing directory, ssh exits 255 when it drops.
    let missing = "ls: cannot access '/x': No such file or directory\n";
    assert!(!listing_is_usable(Some(2), "", missing));
    assert!(!listing_is_usable(Some(255), GNU_LISTING, ""));
    assert!(!listing_is_usable(None, GNU_LISTING, ""));
}

#[test]
fn a_missing_directory_with_exit_one_reads_as_an_error() {
    // BusyBox and BSD exit 1 here, with nothing on stdout.
    let stderr = "ls: /probe/missing: No such file or directory\n";
    assert!(!listing_is_usable(Some(1), "", stderr));
}

/// Run the listing command through a local `sh`, with the format variables
/// a remote shell might export set to values that would break the parser.
fn run_listing_locally(path: &Path) -> std::process::Output {
    Command::new("sh")
        .arg("-c")
        .arg(remote_listing_command(&path.to_string_lossy()))
        .env("QUOTING_STYLE", "shell-escape")
        .env("TIME_STYLE", "long-iso")
        .output()
        .expect("run sh")
}

#[test]
fn the_local_ls_lists_odd_names_and_skips_a_dangling_symlink() {
    let dir = tempfile::tempdir().expect("tempdir");
    let base = dir.path();
    std::fs::create_dir(base.join("sub dir")).unwrap();
    std::fs::write(base.join(" leading"), "x").unwrap();
    std::fs::write(base.join("trailing "), "x").unwrap();
    std::fs::write(base.join("small.txt"), "abc").unwrap();
    std::os::unix::fs::symlink(base.join("missing target"), base.join("dangling link")).unwrap();

    let out = run_listing_locally(base);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        listing_is_usable(out.status.code(), &stdout, &stderr),
        "{:?}: {stdout}{stderr}",
        out.status
    );
    let entries = parse_ls_output(&stdout, true, BrowserSort::Name);
    assert_eq!(
        names(&entries),
        vec!["sub dir", " leading", "small.txt", "trailing "]
    );
    assert!(entries[0].is_dir);
    assert_eq!(entries[2].size, Some(3));
    assert!(entries[2].modified.is_some());
}

#[test]
fn the_local_ls_lists_a_directory_of_broken_symlinks_as_empty() {
    let dir = tempfile::tempdir().expect("tempdir");
    let base = dir.path();
    std::os::unix::fs::symlink(base.join("missing target"), base.join("gone")).unwrap();

    let out = run_listing_locally(base);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        listing_is_usable(out.status.code(), &stdout, &stderr),
        "{:?}: {stdout}{stderr}",
        out.status
    );
    assert!(parse_ls_output(&stdout, true, BrowserSort::Name).is_empty());
}

#[test]
fn the_local_ls_reports_a_dangling_link_to_a_folder_as_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let link = dir.path().join("current");
    std::os::unix::fs::symlink(dir.path().join("removed release"), &link).unwrap();
    let out = run_listing_locally(&link);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !listing_is_usable(out.status.code(), &stdout, &stderr),
        "{:?}: {stdout}{stderr}",
        out.status
    );
}

#[test]
fn the_local_ls_reports_a_missing_directory_as_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run_listing_locally(&dir.path().join("missing"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !listing_is_usable(out.status.code(), &stdout, &stderr),
        "{stdout}{stderr}"
    );
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
    assert_eq!(parse_human_size("512B"), 512);
    assert_eq!(parse_human_size("0B"), 0);
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

// =========================================================================
// build_scp_command: the transport options and auth env of a transfer.
// A transfer runs in the background, so it must never be able to reach the
// controlling tty for a password or a host-key question.
// =========================================================================

fn scp_ctx<'a>(
    env: &'a crate::runtime::env::Env,
    askpass: Option<&'a str>,
    session_password: Option<&'a str>,
    trust_new_host_key: bool,
) -> crate::ssh_context::SshContext<'a> {
    crate::ssh_context::SshContext {
        alias: "web1",
        config_path: Path::new("/tmp/cfg"),
        askpass,
        session_password,
        bw_session: None,
        has_tunnel: false,
        trust_new_host_key,
        security_key: &crate::security_key::NO_SECURITY_KEY,
        env,
    }
}

fn scp_args_of(cmd: &Command) -> Vec<String> {
    cmd.get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

fn scp_env_names(cmd: &Command) -> Vec<String> {
    cmd.get_envs()
        .filter_map(|(k, v)| v.map(|_| k.to_string_lossy().into_owned()))
        .collect()
}

fn scp_has_opt(args: &[String], value: &str) -> bool {
    args.windows(2).any(|w| w[0] == "-o" && w[1] == value)
}

#[test]
fn scp_without_auth_sets_batch_mode_and_strict_checking() {
    let env = crate::runtime::env::Env::for_test("/home/u");
    let ctx = scp_ctx(&env, None, None, false);
    let args = scp_args_of(&build_scp_command(&ctx, &["--".to_string()]));
    assert!(scp_has_opt(&args, "BatchMode=yes"), "got: {args:?}");
    assert!(
        scp_has_opt(&args, "StrictHostKeyChecking=yes"),
        "got: {args:?}"
    );
}

#[test]
fn scp_with_a_source_omits_batch_mode() {
    // BatchMode would disable the very auth method askpass feeds.
    let env = crate::runtime::env::Env::for_test("/home/u");
    let ctx = scp_ctx(&env, Some("keychain"), None, false);
    let args = scp_args_of(&build_scp_command(&ctx, &["--".to_string()]));
    assert!(!scp_has_opt(&args, "BatchMode=yes"), "got: {args:?}");
}

#[test]
fn scp_with_a_session_password_wires_the_one_shot_channel() {
    let env = crate::runtime::env::Env::for_test("/home/u");
    let ctx = scp_ctx(&env, None, Some("hunter2"), false);
    let cmd = build_scp_command(&ctx, &["--".to_string()]);
    let args = scp_args_of(&cmd);
    assert!(!scp_has_opt(&args, "BatchMode=yes"), "got: {args:?}");
    assert!(args.iter().all(|a| a != "hunter2"), "secret in argv");
    let names = scp_env_names(&cmd);
    assert!(names.iter().any(|n| n == "SSH_ASKPASS"));
    assert!(names.iter().any(|n| n == "PURPLE_ASKPASS_ONESHOT"));
}

#[test]
fn scp_always_limits_ssh_to_one_password_attempt() {
    let env = crate::runtime::env::Env::for_test("/home/u");
    for (askpass, session) in [
        (None, None),
        (Some("keychain"), None),
        (None, Some("hunter2")),
    ] {
        let ctx = scp_ctx(&env, askpass, session, false);
        let args = scp_args_of(&build_scp_command(&ctx, &["--".to_string()]));
        assert!(
            scp_has_opt(&args, "NumberOfPasswordPrompts=1"),
            "got: {args:?}"
        );
    }
}

#[test]
fn scp_on_a_trust_retry_uses_accept_new() {
    let env = crate::runtime::env::Env::for_test("/home/u");
    let ctx = scp_ctx(&env, None, None, true);
    let args = scp_args_of(&build_scp_command(&ctx, &["--".to_string()]));
    assert!(
        scp_has_opt(&args, "StrictHostKeyChecking=accept-new"),
        "got: {args:?}"
    );
    assert!(!scp_has_opt(&args, "StrictHostKeyChecking=yes"));
}

#[test]
fn scp_transfer_arguments_follow_the_transport_options() {
    // scp stops option parsing at `--`, so every -o must precede it.
    let env = crate::runtime::env::Env::for_test("/home/u");
    let ctx = scp_ctx(&env, None, None, false);
    let transfer = vec![
        "--".to_string(),
        "/tmp/a.txt".to_string(),
        "web1:/home/u/".to_string(),
    ];
    let args = scp_args_of(&build_scp_command(&ctx, &transfer));
    let sep = args.iter().position(|a| a == "--").expect("-- present");
    let last_opt = args.iter().rposition(|a| a == "-o").expect("-o present");
    assert!(last_opt < sep, "got: {args:?}");
    assert_eq!(&args[sep..], &transfer[..]);
}

#[test]
fn scp_reuses_an_open_tunnel_by_clearing_forwardings() {
    let env = crate::runtime::env::Env::for_test("/home/u");
    let mut ctx = scp_ctx(&env, None, None, false);
    ctx.has_tunnel = true;
    let args = scp_args_of(&build_scp_command(&ctx, &["--".to_string()]));
    assert!(
        scp_has_opt(&args, "ClearAllForwardings=yes"),
        "got: {args:?}"
    );
}
