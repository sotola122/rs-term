//! OSC 52, plus the OS clipboard when it can be opened.

use std::io::{self, Write};

pub fn copy_text(text: &str) -> io::Result<()> {
    let mut out = io::stdout();
    write!(out, "{}", osc52(text))?;
    out.flush()?;
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        let _ = clipboard.set_text(text.to_string());
    }
    Ok(())
}

pub fn clipboard_text() -> Option<String> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    clipboard.get_text().ok()
}

pub fn osc52(text: &str) -> String {
    format!("\u{1b}]52;c;{}\u{7}", base64_encode(text.as_bytes()))
}

fn base64_encode(data: &[u8]) -> String {
    let mut out = String::new();
    let mut index = 0;
    while index + 3 <= data.len() {
        let n =
            ((data[index] as u32) << 16) | ((data[index + 1] as u32) << 8) | data[index + 2] as u32;
        push_group(&mut out, n, 4);
        index += 3;
    }
    match data.len() - index {
        1 => {
            let n = (data[index] as u32) << 16;
            push_group(&mut out, n, 2);
            out.push('=');
            out.push('=');
        }
        2 => {
            let n = ((data[index] as u32) << 16) | ((data[index + 1] as u32) << 8);
            push_group(&mut out, n, 3);
            out.push('=');
        }
        _ => {}
    }
    out
}

fn push_group(out: &mut String, n: u32, chars: usize) {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let shifts = [18, 12, 6, 0];
    for shift in shifts.into_iter().take(chars) {
        out.push(TABLE[((n >> shift) & 63) as usize] as char);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc52_carries_standard_base64() {
        let seq = osc52("hi");
        assert_eq!(seq, "\u{1b}]52;c;aGk=\u{7}");
    }
}
