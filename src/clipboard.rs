use std::io::Write;
use std::process::{Command, Stdio};

/// Copy text to the system clipboard using pbcopy (macOS).
pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let mut child = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Clipboard not available. Are you on macOS?")?;

    // Write to stdin and drop it explicitly so pbcopy sees EOF
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or("Clipboard not available. Are you on macOS?")?;
        stdin
            .write_all(text.as_bytes())
            .map_err(|_| "Clipboard not available. Are you on macOS?")?;
    }

    child
        .wait()
        .map_err(|_| "Clipboard not available. Are you on macOS?")?;

    Ok(())
}
