use std::io::Write;
use std::process::{Command, Stdio};

/// Try to find a working clipboard command by checking PATH.
fn clipboard_cmd() -> Result<&'static str, String> {
    let candidates = [
        ("pbcopy", &[][..]),                              // macOS
        ("wl-copy", &[][..]),                             // Wayland
        ("xclip", &["-selection", "clipboard"][..]),      // X11
        ("xsel", &["--clipboard", "--input"][..]),        // X11 alt
    ];

    for (cmd, _) in &candidates {
        let found = Command::new("sh")
            .args(["-c", &format!("command -v {} >/dev/null 2>&1", cmd)])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if found {
            return Ok(cmd);
        }
    }

    Err("No clipboard tool found. Install pbcopy (macOS), wl-copy (Wayland), or xclip/xsel (X11).".to_string())
}

/// Get the extra args needed for a clipboard command.
fn clipboard_args(cmd: &str) -> &'static [&'static str] {
    match cmd {
        "xclip" => &["-selection", "clipboard"],
        "xsel" => &["--clipboard", "--input"],
        _ => &[],
    }
}

/// Copy text to the system clipboard.
pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let cmd = clipboard_cmd()?;
    let args = clipboard_args(cmd);

    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| format!("Failed to run {}.", cmd))?;

    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| format!("Failed to write to {}.", cmd))?;
        stdin
            .write_all(text.as_bytes())
            .map_err(|_| format!("Failed to write to {}.", cmd))?;
    }

    child
        .wait()
        .map_err(|_| format!("{} exited unexpectedly.", cmd))?;

    Ok(())
}
