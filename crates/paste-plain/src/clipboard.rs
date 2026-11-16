//! Clipboard access through CLI tools: wl-clipboard on Wayland, xclip on X11.

use anyhow::{Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tracing::{debug, warn};

fn is_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// Look up an executable on `PATH`.
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|p| is_executable(p))
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Read raw clipboard bytes for a MIME type. Returns `None` if unavailable.
pub fn get_bytes(mime: &str) -> Option<Vec<u8>> {
    if is_wayland()
        && let Ok(out) = Command::new("wl-paste")
            .args(["--no-newline", "--type", mime])
            .stderr(Stdio::null())
            .output()
        && out.status.success()
    {
        return Some(out.stdout);
    }
    match Command::new("xclip")
        .args(["-selection", "clipboard", "-t", mime, "-o"])
        .stderr(Stdio::null())
        .output()
    {
        Ok(out) if out.status.success() && !out.stdout.is_empty() => Some(out.stdout),
        _ => None,
    }
}

/// MIME types currently offered by the clipboard owner.
pub fn list_types() -> Vec<String> {
    let out = if is_wayland() {
        Command::new("wl-paste")
            .arg("--list-types")
            .stderr(Stdio::null())
            .output()
    } else {
        Command::new("xclip")
            .args(["-selection", "clipboard", "-t", "TARGETS", "-o"])
            .stderr(Stdio::null())
            .output()
    };
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// Decode clipboard text that may be UTF-16 (some apps publish text/html that way).
pub fn decode_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes.len().is_multiple_of(2) {
        let le = bytes[0] == 0xFF && bytes[1] == 0xFE;
        let be = bytes[0] == 0xFE && bytes[1] == 0xFF;
        if le || be {
            let units: Vec<u16> = bytes[2..]
                .chunks_exact(2)
                .map(|c| {
                    if le {
                        u16::from_le_bytes([c[0], c[1]])
                    } else {
                        u16::from_be_bytes([c[0], c[1]])
                    }
                })
                .collect();
            return String::from_utf16_lossy(&units);
        }
    }
    let s = String::from_utf8_lossy(bytes);
    s.strip_prefix('\u{feff}').unwrap_or(&s).to_string()
}

/// Get plain text from the clipboard.
/// Tries wl-paste (Wayland) first, falls back to xclip (X11).
pub fn get_text() -> Result<String> {
    // Try Wayland first
    if let Ok(output) = Command::new("wl-paste")
        .arg("--no-newline")
        .arg("--type")
        .arg("text/plain")
        .stderr(Stdio::null())
        .output()
        && output.status.success()
    {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }

    // Fall back to xclip
    let output = Command::new("xclip")
        .args(["-selection", "clipboard", "-o"])
        .output()
        .context("failed to run xclip — is xclip or wl-clipboard installed?")?;

    if !output.status.success() {
        anyhow::bail!("xclip failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Get HTML from the clipboard (text/html), if offered.
pub fn get_html() -> Option<String> {
    get_bytes("text/html")
        .map(|b| decode_text(&b))
        .filter(|s| !s.trim().is_empty())
}

/// Get a PNG image from the clipboard (image/png), if offered.
pub fn get_png() -> Option<Vec<u8>> {
    get_bytes("image/png").filter(|b| b.starts_with(b"\x89PNG"))
}

/// Set plain text to the clipboard (replacing any rich content).
/// Tries wl-copy (Wayland) first, falls back to xclip (X11).
pub fn set_text(text: &str) -> Result<()> {
    set_bytes("text/plain", text.as_bytes())
}

/// Put a list of files on the clipboard as `text/uri-list`.
pub fn set_uri_list(paths: &[PathBuf]) -> Result<()> {
    set_bytes("text/uri-list", uri_list(paths).as_bytes())
}

/// Write bytes with the given MIME type to the clipboard.
pub fn set_bytes(mime: &str, data: &[u8]) -> Result<()> {
    if is_wayland() || which("xclip").is_none() {
        match pipe_to(Command::new("wl-copy").args(["--type", mime]), data) {
            Ok(()) => return Ok(()),
            Err(e) => debug!("wl-copy failed: {e}"),
        }
    }
    let mut cmd = Command::new("xclip");
    if mime == "text/plain" {
        // Plain `xclip -i` advertises UTF8_STRING/STRING/TEXT, which every app understands.
        cmd.args(["-selection", "clipboard", "-i"]);
    } else {
        cmd.args(["-selection", "clipboard", "-t", mime, "-i"]);
    }
    pipe_to(&mut cmd, data).context("failed to set clipboard — install wl-clipboard or xclip")
}

fn pipe_to(cmd: &mut Command, data: &[u8]) -> Result<()> {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(data)?;
    }
    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("clipboard tool exited with {status}");
    }
    Ok(())
}

/// Build a `text/uri-list` payload (RFC 2483: CRLF-terminated `file://` URIs).
pub fn uri_list(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| format!("file://{}\r\n", percent_encode_path(&p.to_string_lossy())))
        .collect()
}

fn percent_encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Tool used to synthesize Ctrl+V.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteTool {
    Xdotool,
    Wtype,
    Ydotool,
}

/// Pick the paste tool: on Wayland prefer wtype, then ydotool, then xdotool
/// (XWayland apps only); on X11 prefer xdotool, then ydotool.
pub fn choose_paste_tool(wayland: bool, available: impl Fn(&str) -> bool) -> Option<PasteTool> {
    let order: &[PasteTool] = if wayland {
        &[PasteTool::Wtype, PasteTool::Ydotool, PasteTool::Xdotool]
    } else {
        &[PasteTool::Xdotool, PasteTool::Ydotool]
    };
    order.iter().copied().find(|t| {
        available(match t {
            PasteTool::Xdotool => "xdotool",
            PasteTool::Wtype => "wtype",
            PasteTool::Ydotool => "ydotool",
        })
    })
}

/// Simulate a Ctrl+V keypress (xdotool on X11, wtype/ydotool on Wayland).
pub fn simulate_paste() -> Result<()> {
    let tool = choose_paste_tool(is_wayland(), |n| which(n).is_some()).ok_or_else(|| {
        anyhow::anyhow!(
            "no key injection tool found — install xdotool (X11) or wtype/ydotool (Wayland)"
        )
    })?;
    let status = match tool {
        PasteTool::Xdotool => Command::new("xdotool")
            .args(["key", "--clearmodifiers", "ctrl+v"])
            .status(),
        PasteTool::Wtype => Command::new("wtype")
            .args(["-M", "ctrl", "-k", "v", "-m", "ctrl"])
            .status(),
        // Linux input event codes: 29 = KEY_LEFTCTRL, 47 = KEY_V.
        PasteTool::Ydotool => Command::new("ydotool")
            .args(["key", "29:1", "47:1", "47:0", "29:0"])
            .status(),
    }
    .with_context(|| format!("failed to run {tool:?}"))?;

    if !status.success() {
        warn!("{tool:?} exited with {status}");
        anyhow::bail!("{tool:?} failed to simulate paste");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_list_encodes_spaces() {
        let s = uri_list(&[PathBuf::from("/tmp/a b/c.txt")]);
        assert_eq!(s, "file:///tmp/a%20b/c.txt\r\n");
    }

    #[test]
    fn decode_utf16le_with_bom() {
        let bytes = [0xFF, 0xFE, b'h', 0, b'i', 0];
        assert_eq!(decode_text(&bytes), "hi");
        assert_eq!(decode_text("\u{feff}x".as_bytes()), "x");
    }

    #[test]
    fn paste_tool_preference() {
        let all = |_: &str| true;
        assert_eq!(choose_paste_tool(true, all), Some(PasteTool::Wtype));
        assert_eq!(choose_paste_tool(false, all), Some(PasteTool::Xdotool));
        let only_ydo = |n: &str| n == "ydotool";
        assert_eq!(choose_paste_tool(true, only_ydo), Some(PasteTool::Ydotool));
        assert_eq!(choose_paste_tool(false, |_| false), None);
    }
}
