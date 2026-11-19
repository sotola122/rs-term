//! Encoding and newline conversion.
//!
//! Shift_JIS is decoded as a whole chunk by `encoding_rs`. C0 bytes are not
//! peeled off first: a lead byte followed by ESC is the library's malformed
//! sequence, not a CP932 character.

use encoding_rs::{Decoder, EncoderResult, SHIFT_JIS, UTF_8};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEncoding {
    Utf8,
    ShiftJis,
}

impl SessionEncoding {
    /// The spellings the command line, the encoding dialog and
    /// `settings.toml` accept. Case does not matter.
    pub fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "sjis" | "shift_jis" | "shift-jis" => Some(Self::ShiftJis),
            "utf8" | "utf-8" => Some(Self::Utf8),
            _ => None,
        }
    }

    /// The short name the encoding dialog starts with (`parse` takes it back).
    pub fn token(self) -> &'static str {
        match self {
            Self::ShiftJis => "sjis",
            Self::Utf8 => "utf8",
        }
    }

    /// The name the status line shows.
    pub fn label(self) -> &'static str {
        match self {
            Self::ShiftJis => "Shift_JIS",
            Self::Utf8 => "UTF-8",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Newline {
    pub receive: ReceiveNewline,
    pub transmit: TransmitNewline,
}

impl Default for Newline {
    fn default() -> Self {
        Self {
            receive: ReceiveNewline::Auto,
            transmit: TransmitNewline::Cr,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiveNewline {
    Cr,
    CrLf,
    Lf,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransmitNewline {
    Cr,
    CrLf,
    Lf,
}

pub(crate) struct Codec {
    encoding: SessionEncoding,
    decoder: Option<Decoder>,
}

impl Codec {
    pub(crate) fn new(encoding: SessionEncoding) -> Self {
        Self {
            decoder: decoder_for(encoding),
            encoding,
        }
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) -> Vec<u8> {
        let Some(decoder) = self.decoder.as_mut() else {
            return bytes.to_vec();
        };
        decode_all(decoder, bytes, false)
    }

    /// Finish the decoder so a dangling lead byte becomes U+FFFD.
    /// Finish a dangling lead as U+FFFD and start a fresh decoder.
    pub(crate) fn reset(&mut self) -> Vec<u8> {
        let flushed = self.finish();
        self.decoder = decoder_for(self.encoding);
        flushed
    }

    pub(crate) fn set_encoding(&mut self, encoding: SessionEncoding) -> Vec<u8> {
        let flushed = self.finish();
        self.encoding = encoding;
        self.decoder = decoder_for(encoding);
        flushed
    }

    fn finish(&mut self) -> Vec<u8> {
        let Some(decoder) = self.decoder.as_mut() else {
            return Vec::new();
        };
        decode_all(decoder, b"", true)
    }
}

fn decoder_for(encoding: SessionEncoding) -> Option<Decoder> {
    match encoding {
        SessionEncoding::Utf8 => None,
        SessionEncoding::ShiftJis => Some(SHIFT_JIS.new_decoder()),
    }
}

fn decode_all(decoder: &mut Decoder, bytes: &[u8], last: bool) -> Vec<u8> {
    let mut rest = bytes;
    let mut out = String::new();
    loop {
        let needed = decoder
            .max_utf8_buffer_length(rest.len().max(1))
            .unwrap_or(rest.len().saturating_mul(3).saturating_add(16));
        let start = out.len();
        out.reserve(needed);
        let (result, read, _replaced) = decoder.decode_to_string(rest, &mut out, last);
        rest = &rest[read.min(rest.len())..];
        match result {
            encoding_rs::CoderResult::InputEmpty => break,
            encoding_rs::CoderResult::OutputFull => {
                if out.len() == start {
                    // Nothing was written. Grow and retry rather than spin.
                    out.reserve(needed.max(8));
                }
            }
        }
        if rest.is_empty() && !matches!(result, encoding_rs::CoderResult::OutputFull) {
            break;
        }
    }
    out.into_bytes()
}

/// Encode one input. An unmappable scalar fails the whole input.
pub(crate) fn encode(encoding: SessionEncoding, text: &str) -> Result<Vec<u8>, char> {
    let encoding = match encoding {
        SessionEncoding::Utf8 => UTF_8,
        SessionEncoding::ShiftJis => SHIFT_JIS,
    };
    let mut encoder = encoding.new_encoder();
    let mut src = text;
    let mut out = Vec::new();
    loop {
        let needed = encoder
            .max_buffer_length_from_utf8_without_replacement(src.len())
            .unwrap_or(src.len().saturating_mul(3).saturating_add(16));
        let start = out.len();
        out.resize(start + needed.max(8), 0);
        let (result, read, written) =
            encoder.encode_from_utf8_without_replacement(src, &mut out[start..], true);
        out.truncate(start + written);
        src = &src[read.min(src.len())..];
        match result {
            EncoderResult::InputEmpty => return Ok(out),
            EncoderResult::OutputFull => {
                if written == 0 && read == 0 {
                    out.resize(out.len() + 16, 0);
                }
            }
            EncoderResult::Unmappable(ch) => return Err(ch),
        }
    }
}

pub(crate) fn transmit_newline(mode: TransmitNewline) -> &'static [u8] {
    match mode {
        TransmitNewline::Cr => b"\r",
        TransmitNewline::CrLf => b"\r\n",
        TransmitNewline::Lf => b"\n",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PastePiece {
    Text(String),
    Newline,
}

/// Split paste text. `\n`, `\r\n`, and `\r` are one newline. No extra trailing newline.
pub(crate) fn split_paste(text: &str) -> Vec<PastePiece> {
    let mut pieces = Vec::new();
    let mut buf = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if !buf.is_empty() {
                pieces.push(PastePiece::Text(std::mem::take(&mut buf)));
            }
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            pieces.push(PastePiece::Newline);
        } else if ch == '\n' {
            if !buf.is_empty() {
                pieces.push(PastePiece::Text(std::mem::take(&mut buf)));
            }
            pieces.push(PastePiece::Newline);
        } else {
            buf.push(ch);
        }
    }
    if !buf.is_empty() {
        pieces.push(PastePiece::Text(buf));
    }
    pieces
}

pub(crate) struct NewlineConverter {
    mode: ReceiveNewline,
    /// Auto mode only: the last byte was a CR, already passed on. An LF right
    /// after it completes a CRLF and is passed as is; any other LF gets a CR
    /// first. Nothing is held back, so a CR at the end of a chunk is shown at
    /// once.
    after_cr: bool,
}

impl NewlineConverter {
    pub(crate) fn new(mode: ReceiveNewline) -> Self {
        Self {
            mode,
            after_cr: false,
        }
    }

    pub(crate) fn set_mode(&mut self, mode: ReceiveNewline) {
        self.mode = mode;
        self.after_cr = false;
    }

    pub(crate) fn push(&mut self, utf8: &[u8]) -> Vec<u8> {
        match self.mode {
            ReceiveNewline::Cr => utf8.to_vec(),
            ReceiveNewline::CrLf => {
                let mut out = Vec::with_capacity(utf8.len() + 8);
                for byte in utf8 {
                    out.push(*byte);
                    if *byte == b'\r' {
                        out.push(b'\n');
                    }
                }
                out
            }
            ReceiveNewline::Lf => {
                let mut out = Vec::with_capacity(utf8.len() + 8);
                for byte in utf8 {
                    if *byte == b'\n' {
                        out.extend_from_slice(b"\r\n");
                    } else {
                        out.push(*byte);
                    }
                }
                out
            }
            ReceiveNewline::Auto => self.push_auto(utf8),
        }
    }

    /// Auto detects the device's line ending: CRLF stays, a bare LF becomes
    /// CRLF, and a lone CR is only a carriage return (line editors and
    /// progress bars redraw with it).
    fn push_auto(&mut self, input: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(input.len() + 8);
        for &byte in input {
            match byte {
                b'\n' if !self.after_cr => out.extend_from_slice(b"\r\n"),
                other => out.push(other),
            }
            self.after_cr = byte == b'\r';
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_names_round_trip() {
        for encoding in [SessionEncoding::ShiftJis, SessionEncoding::Utf8] {
            assert_eq!(SessionEncoding::parse(encoding.token()), Some(encoding));
        }
        assert_eq!(
            SessionEncoding::parse("Shift_JIS"),
            Some(SessionEncoding::ShiftJis)
        );
        assert_eq!(SessionEncoding::parse("UTF-8"), Some(SessionEncoding::Utf8));
        assert_eq!(SessionEncoding::parse("euc-jp"), None);
        assert_eq!(SessionEncoding::ShiftJis.label(), "Shift_JIS");
        assert_eq!(SessionEncoding::Utf8.label(), "UTF-8");
    }

    #[test]
    fn utf8_chunks_concatenate_to_the_original() {
        let mut codec = Codec::new(SessionEncoding::Utf8);
        let first = codec.push(&[0xE3, 0x81]);
        let second = codec.push(&[0x82]);
        let mut all = first;
        all.extend(second);
        assert_eq!(all, "あ".as_bytes());
    }

    #[test]
    fn shift_jis_split_lead_still_decodes_a() {
        let full = decode_shift_jis(&[0x82, 0xA0]);
        for split in 0..=2 {
            let mut codec = Codec::new(SessionEncoding::ShiftJis);
            let mut out = codec.push(&([0x82, 0xA0])[..split]);
            out.extend(codec.push(&([0x82, 0xA0])[split..]));
            assert_eq!(out, full, "split at {split}");
        }
        assert_eq!(String::from_utf8(full).unwrap(), "あ");
    }

    #[test]
    fn malformed_lead_before_esc_is_replacement_not_prolonged_sound() {
        let bytes = decode_shift_jis(&[0x81, 0x1B, 0x5B, 0x48]);
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.starts_with('\u{FFFD}'), "{text:?}");
        assert!(!text.contains('ー'), "{text:?} bytes {bytes:?}");
        assert!(bytes.windows(3).any(|w| w == [0x1B, 0x5B, 0x48]));
    }

    #[test]
    fn cr_and_lf_after_a_lead_stay_newlines() {
        let cr = decode_shift_jis(&[0x82, b'\r']);
        let text = String::from_utf8(cr).unwrap();
        assert!(text.contains('\u{FFFD}'));
        assert!(text.contains('\r'));
        let lf = String::from_utf8(decode_shift_jis(&[0x82, b'\n'])).unwrap();
        assert!(lf.contains('\n'));
    }

    #[test]
    fn reset_flushes_a_dangling_lead() {
        let mut codec = Codec::new(SessionEncoding::ShiftJis);
        assert!(codec.push(&[0x82]).is_empty());
        let flushed = String::from_utf8(codec.reset()).unwrap();
        assert_eq!(flushed, "\u{FFFD}");
        let next = String::from_utf8(codec.push(&[0xA0])).unwrap();
        assert_ne!(next, "あ");
    }

    #[test]
    fn receive_newline_matches_the_manual() {
        let sample = b"a\r\nb\rc\nd";
        assert_eq!(convert(ReceiveNewline::Cr, sample), b"a\r\nb\rc\nd");
        assert_eq!(convert(ReceiveNewline::CrLf, sample), b"a\r\n\nb\r\nc\nd");
        assert_eq!(convert(ReceiveNewline::Lf, sample), b"a\r\r\nb\rc\r\nd");
        assert_eq!(convert(ReceiveNewline::Auto, sample), b"a\r\nb\rc\r\nd");
    }

    #[test]
    fn auto_tracks_cr_across_chunks() {
        let mut crlf = NewlineConverter::new(ReceiveNewline::Auto);
        assert_eq!(crlf.push(b"a\r"), b"a\r");
        assert_eq!(crlf.push(b"\nb"), b"\nb");
        let mut lone = NewlineConverter::new(ReceiveNewline::Auto);
        assert_eq!(lone.push(b"a\r"), b"a\r");
        assert_eq!(lone.push(b"b"), b"b");
        let mut lf = NewlineConverter::new(ReceiveNewline::Auto);
        assert_eq!(lf.push(b"\nx"), b"\r\nx");
        assert_eq!(lf.push(b"\n"), b"\r\n");
        let mut double = NewlineConverter::new(ReceiveNewline::Auto);
        assert_eq!(double.push(b"\r\r\n"), b"\r\r\n");
    }

    #[test]
    fn auto_keeps_a_linenoise_redraw_on_one_line() {
        let redraw = b"\rnexia>  se\x1b[0K\r\x1b[9C";
        assert_eq!(convert(ReceiveNewline::Auto, redraw), redraw);
        let mut nl = NewlineConverter::new(ReceiveNewline::Auto);
        let mut out = Vec::new();
        for byte in redraw {
            out.extend(nl.push(&[*byte]));
        }
        assert_eq!(out, redraw);
    }

    #[test]
    fn unmappable_character_encodes_nothing() {
        let err = encode(SessionEncoding::ShiftJis, "a😀").unwrap_err();
        assert_eq!(err, '😀');
        let ok = encode(SessionEncoding::ShiftJis, "あ").unwrap();
        assert_eq!(ok, vec![0x82, 0xA0]);
    }

    #[test]
    fn paste_newlines_are_one_break_without_a_trailing_extra() {
        assert_eq!(
            split_paste("a\r\nb\nc\r"),
            vec![
                PastePiece::Text("a".into()),
                PastePiece::Newline,
                PastePiece::Text("b".into()),
                PastePiece::Newline,
                PastePiece::Text("c".into()),
                PastePiece::Newline,
            ]
        );
        assert_eq!(split_paste("ab"), vec![PastePiece::Text("ab".into())]);
    }

    fn decode_shift_jis(bytes: &[u8]) -> Vec<u8> {
        let mut decoder = SHIFT_JIS.new_decoder();
        decode_all(&mut decoder, bytes, true)
    }

    fn convert(mode: ReceiveNewline, bytes: &[u8]) -> Vec<u8> {
        NewlineConverter::new(mode).push(bytes)
    }
}
