//! Копирование в системный буфер обмена: `pbcopy` на macOS, `wl-copy`/`xclip`/`xsel` на Linux, иначе — OSC 52
//! (терминал сам кладёт в буфер).

use std::io::Write;
use std::process::{Command, Stdio};

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn pipe_to(cmd: &str, args: &[&str], text: &str) -> bool {
    let Ok(mut child) = Command::new(cmd).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(text.as_bytes()).is_err() {
            return false;
        }
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

/// Linux: `wl-copy` (Wayland) или `xclip`/`xsel` (X11), если установлены; иначе остаётся OSC 52.
fn linux_clipboard(text: &str) -> bool {
    let has = |v: &str| std::env::var_os(v).map_or(false, |x| !x.is_empty());
    if has("WAYLAND_DISPLAY") && pipe_to("wl-copy", &[], text) {
        return true;
    }
    has("DISPLAY") && (pipe_to("xclip", &["-selection", "clipboard"], text) || pipe_to("xsel", &["--clipboard", "--input"], text))
}

/// Кладёт текст в буфер обмена. `true` — получилось (для OSC 52 это лишь «отправили терминалу»).
pub fn copy(text: &str) -> bool {
    if cfg!(target_os = "macos") && pipe_to("pbcopy", &[], text) {
        return true;
    }
    if cfg!(target_os = "linux") && linux_clipboard(text) {
        return true;
    }
    let mut out = std::io::stdout();
    write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes())).and_then(|_| out.flush()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_reference() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64("Привет".as_bytes()), "0J/RgNC40LLQtdGC");
    }
}
