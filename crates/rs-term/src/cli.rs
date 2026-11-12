//! `rs-term [PORT] -b BAUD --data --parity --stop --flow -e sjis|utf8 --log PATH`.

use std::ffi::OsString;
use std::path::PathBuf;

use rs_term_core::{AmbiguousWidth, DataBits, Flow, Parity, SessionEncoding, StopBits};

use crate::keys::Prefix;

pub const HELP: &str = "\
rs-term - full-screen serial terminal
Copyright 2026 sotola122

Usage:
  rs-term [PORT] [OPTIONS]

Without PORT, pick a port or type a path inside the screen.
One process holds one connection. Splitting is left to herdr, tmux or
Windows Terminal.

Options:
  -b, --baud <N>              baud rate (1 to 4000000, default 115200)
      --data <5|6|7|8>        data bits (default 8)
      --parity <none|odd|even>
      --stop <1|2>
      --flow <none|xonxoff|rtscts>
  -e, --encoding <sjis|utf8>
      --log <PATH>            write a text log to this file from startup
      --prefix <ctrl-a>       prefix key (ctrl-LETTER, ctrl-\\, ctrl-]; default ctrl-a)
      --ambiguous-width <1|2|auto>
                              cells for ambiguous-width characters (default auto:
                              2 only on a conhost with a CJK code page)
  -h, --help

Keys after the prefix:
  c connect   d disconnect   r reconnect   b break
  l log   L stop log   s send   f file
  u baud   e encoding   p paste
  ? help   q quit   Esc cancel
  Press the prefix again to send its control byte.
";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Cli {
    pub port: Option<String>,
    pub baud: Option<u32>,
    pub data_bits: Option<DataBits>,
    pub parity: Option<Parity>,
    pub stop_bits: Option<StopBits>,
    pub flow: Option<Flow>,
    pub encoding: Option<SessionEncoding>,
    pub log: Option<PathBuf>,
    pub prefix: Prefix,
    pub ambiguous_width: Option<AmbiguousWidth>,
    pub help: bool,
}

/// `parse` for the raw process arguments. An argument that is not valid
/// Unicode is a usage error, not a panic.
pub fn parse_os<I>(args: I) -> Result<Cli, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut texts = Vec::new();
    for arg in args {
        match arg.into_string() {
            Ok(text) => texts.push(text),
            Err(raw) => {
                return Err(format!(
                    "Argument is not valid UTF-8: {}",
                    raw.to_string_lossy()
                ))
            }
        }
    }
    parse(texts)
}

pub fn parse<I, S>(args: I) -> Result<Cli, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args: Vec<String> = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect();
    let mut cli = Cli::default();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].clone();
        if arg == "--" {
            index += 1;
            while index < args.len() {
                take_port(&mut cli, &args[index])?;
                index += 1;
            }
            break;
        }
        if arg == "-h" || arg == "--help" {
            cli.help = true;
            index += 1;
            continue;
        }
        let (flag, inline) = split_flag(&arg);
        if is_option(&flag) {
            let value = match inline {
                Some(value) if value.is_empty() => {
                    return Err(format!("{flag} needs a value, but it is empty"));
                }
                Some(value) => {
                    index += 1;
                    value
                }
                None => {
                    index += 1;
                    if index >= args.len() || args[index].starts_with('-') {
                        return Err(format!("{flag} needs a value"));
                    }
                    let value = args[index].clone();
                    index += 1;
                    value
                }
            };
            apply(&mut cli, &flag, &value)?;
            continue;
        }
        if arg.starts_with('-') {
            return Err(format!("Unknown option: {arg}"));
        }
        take_port(&mut cli, &arg)?;
        index += 1;
    }
    Ok(cli)
}

fn take_port(cli: &mut Cli, port: &str) -> Result<(), String> {
    if cli.port.is_some() {
        return Err("Only one port can be given".into());
    }
    if port.is_empty() {
        return Err("Port name is empty".into());
    }
    cli.port = Some(port.to_string());
    Ok(())
}

fn is_option(flag: &str) -> bool {
    matches!(
        flag,
        "-b" | "--baud"
            | "--data"
            | "--parity"
            | "--stop"
            | "--flow"
            | "-e"
            | "--encoding"
            | "--log"
            | "--prefix"
            | "--ambiguous-width"
    )
}

fn split_flag(arg: &str) -> (String, Option<String>) {
    if let Some(rest) = arg.strip_prefix("--") {
        if let Some((name, value)) = rest.split_once('=') {
            return (format!("--{name}"), Some(value.to_string()));
        }
        return (arg.to_string(), None);
    }
    if let Some(rest) = arg.strip_prefix('-') {
        let mut chars = rest.chars();
        if let Some(flag) = chars.next() {
            let value: String = chars.collect();
            if !value.is_empty() {
                return (format!("-{flag}"), Some(value));
            }
        }
    }
    (arg.to_string(), None)
}

fn apply(cli: &mut Cli, flag: &str, value: &str) -> Result<(), String> {
    match flag {
        "-b" | "--baud" => cli.baud = Some(parse_baud(value)?),
        "--data" => {
            cli.data_bits = Some(
                DataBits::parse(value)
                    .ok_or_else(|| format!("Data bits `{}` must be 5, 6, 7 or 8", lower(value)))?,
            )
        }
        "--parity" => {
            cli.parity =
                Some(Parity::parse(value).ok_or_else(|| {
                    format!("Parity `{}` must be none, odd or even", lower(value))
                })?)
        }
        "--stop" => {
            cli.stop_bits = Some(
                StopBits::parse(value)
                    .ok_or_else(|| format!("Stop bits `{}` must be 1 or 2", lower(value)))?,
            )
        }
        "--flow" => {
            cli.flow = Some(Flow::parse(value).ok_or_else(|| {
                format!(
                    "Flow control `{}` must be none, xonxoff or rtscts",
                    lower(value)
                )
            })?)
        }
        "-e" | "--encoding" => cli.encoding = Some(parse_encoding(value)?),
        "--log" => cli.log = Some(PathBuf::from(value)),
        "--prefix" => cli.prefix = parse_prefix(value)?,
        "--ambiguous-width" => {
            cli.ambiguous_width = Some(AmbiguousWidth::parse(value).ok_or_else(|| {
                format!("Ambiguous width `{}` must be 1, 2 or auto", lower(value))
            })?)
        }
        _ => return Err(format!("Unknown option: {flag}")),
    }
    Ok(())
}

fn lower(text: &str) -> String {
    text.to_ascii_lowercase()
}

/// The encoding dialog uses this too.
pub fn parse_encoding(text: &str) -> Result<SessionEncoding, String> {
    SessionEncoding::parse(text)
        .ok_or_else(|| format!("Encoding `{}` must be sjis or utf8", lower(text)))
}

pub fn parse_baud(text: &str) -> Result<u32, String> {
    let baud: u32 = text
        .parse()
        .map_err(|_| format!("Baud rate `{text}` is not a number"))?;
    if !(1..=4_000_000).contains(&baud) {
        return Err("Baud rate is out of range (1 to 4000000)".into());
    }
    Ok(baud)
}

const PREFIX_USAGE: &str = "The prefix must be ctrl-LETTER, ctrl-\\ or ctrl-] (for example ctrl-a). Other keys, such as Ctrl+[ which is Esc, cannot be told apart";

pub fn parse_prefix(text: &str) -> Result<Prefix, String> {
    let lower = text.to_ascii_lowercase();
    let body = lower
        .strip_prefix("ctrl-")
        .or_else(|| lower.strip_prefix("ctrl+"))
        .or_else(|| lower.strip_prefix("c-"))
        .or_else(|| lower.strip_prefix("c+"))
        .or_else(|| lower.strip_prefix('^'));
    let mut chars = body.unwrap_or("").chars();
    match (chars.next(), chars.next()) {
        (Some(ch), None) if ch.is_ascii_lowercase() || ch == '\\' || ch == ']' => {
            Ok(Prefix { ctrl: ch })
        }
        _ => Err(PREFIX_USAGE.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_serial_command_line() {
        let cli = parse([
            "COM3", "-b", "9600", "--data", "7", "--parity", "even", "--stop", "2", "--flow",
            "rtscts", "-e", "sjis", "--log", "run.log",
        ])
        .unwrap();
        assert_eq!(cli.port.as_deref(), Some("COM3"));
        assert_eq!(cli.baud, Some(9600));
        assert_eq!(cli.data_bits, Some(DataBits::Seven));
        assert_eq!(cli.parity, Some(Parity::Even));
        assert_eq!(cli.stop_bits, Some(StopBits::Two));
        assert_eq!(cli.flow, Some(Flow::RtsCts));
        assert_eq!(cli.encoding, Some(SessionEncoding::ShiftJis));
        assert_eq!(cli.log.unwrap(), PathBuf::from("run.log"));
        assert_eq!(cli.prefix.ctrl, 'a');
    }

    #[test]
    fn parses_the_ambiguous_width_option() {
        assert_eq!(parse(["COM1"]).unwrap().ambiguous_width, None);
        for (text, expect) in [
            ("1", AmbiguousWidth::One),
            ("2", AmbiguousWidth::Two),
            ("auto", AmbiguousWidth::Auto),
            ("AUTO", AmbiguousWidth::Auto),
        ] {
            let cli = parse(["--ambiguous-width", text]).unwrap();
            assert_eq!(cli.ambiguous_width, Some(expect), "{text}");
        }
        let cli = parse(["--ambiguous-width=2", "COM1"]).unwrap();
        assert_eq!(cli.ambiguous_width, Some(AmbiguousWidth::Two));
        let error = parse(["--ambiguous-width", "3"]).unwrap_err();
        assert!(error.contains("must be 1, 2 or auto"), "{error}");
        assert!(parse(["--ambiguous-width"]).is_err());
    }

    #[test]
    fn flags_may_precede_the_port_and_utf8_is_accepted() {
        let cli = parse(["--baud=19200", "-e", "utf8", "/dev/ttyUSB0"]).unwrap();
        assert_eq!(cli.port.as_deref(), Some("/dev/ttyUSB0"));
        assert_eq!(cli.baud, Some(19_200));
        assert_eq!(cli.encoding, Some(SessionEncoding::Utf8));
    }

    #[test]
    fn missing_port_is_the_picker() {
        let cli = parse(["--flow", "xonxoff"]).unwrap();
        assert!(cli.port.is_none());
        assert_eq!(cli.flow, Some(Flow::XonXoff));
    }

    #[test]
    fn unknown_flag_and_a_second_port_fail() {
        assert!(parse(["--nope"]).unwrap_err().contains("Unknown option"));
        assert!(parse(["COM1", "COM2"]).is_err());
        assert!(parse(["--prefix", "shift-a"]).is_err());
    }

    #[test]
    fn prefix_can_be_ctrl_b() {
        let cli = parse(["--prefix", "C-b"]).unwrap();
        assert_eq!(cli.prefix.ctrl, 'b');
        assert!(parse(["-h"]).unwrap().help);
    }

    #[test]
    fn prefix_accepts_letters_backslash_and_bracket_only() {
        for (spec, ctrl) in [
            ("ctrl-a", 'a'),
            ("ctrl+a", 'a'),
            ("C-z", 'z'),
            ("C-Z", 'z'),
            ("ctrl+\\", '\\'),
            ("ctrl-]", ']'),
        ] {
            assert_eq!(parse_prefix(spec).unwrap().ctrl, ctrl, "{spec}");
            assert!(parse(["--prefix", spec]).is_ok(), "{spec}");
        }
        for spec in [
            "ctrl-3", "ctrl-[", "ctrl-?", "ctrl-@", "ctrl-^", "ctrl-_", "ctrl-1", "ctrl- ",
            "ctrl--", "ctrl-", "ctrl-ab", "a",
        ] {
            let error = parse(["--prefix", spec]).unwrap_err();
            assert!(error.contains("prefix"), "{spec}: {error}");
        }
    }

    #[cfg(unix)]
    fn not_unicode() -> OsString {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(vec![b'/', b'd', 0xFF, 0xFE])
    }

    #[cfg(windows)]
    fn not_unicode() -> OsString {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[0x2F, 0xD800])
    }

    #[test]
    fn a_non_unicode_argument_is_a_usage_error() {
        let error = parse_os([OsString::from("-b"), not_unicode()]).unwrap_err();
        assert!(error.contains("UTF-8"), "{error}");
        let error = parse_os([not_unicode()]).unwrap_err();
        assert!(error.contains("UTF-8"), "{error}");
    }

    #[test]
    fn help_and_usage_errors_are_ascii_english() {
        assert!(HELP.is_ascii() && PREFIX_USAGE.is_ascii());
        for args in [
            vec!["--nope"],
            vec!["--data", "9"],
            vec!["--parity", "x"],
            vec!["--stop", "3"],
            vec!["--flow", "x"],
            vec!["-e", "x"],
            vec!["-b", "abc"],
            vec!["-b", "0"],
            vec!["-b"],
            vec!["COM1", "COM2"],
            vec!["--prefix", "x"],
            vec!["--ambiguous-width", "3"],
        ] {
            let error = parse(args.clone()).unwrap_err();
            assert!(error.is_ascii(), "{args:?}: {error}");
        }
    }

    #[test]
    fn plain_os_arguments_parse_like_strings() {
        let cli = parse_os(["--baud=9600", "COM3"].map(OsString::from)).unwrap();
        assert_eq!(cli.baud, Some(9600));
        assert_eq!(cli.port.as_deref(), Some("COM3"));
    }
}
