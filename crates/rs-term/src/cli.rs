//! `rs-term [PORT] -b BAUD --data --parity --stop --flow -e sjis|utf8 --log PATH`.

use std::path::PathBuf;

use rs_term_core::{encode_key, DataBits, Flow, KeyInput, Parity, SessionEncoding, StopBits};

use crate::keys::Prefix;

pub const HELP: &str = "\
rs-term — 全画面のシリアル端末
著作権 2026 sotola122

使い方:
  rs-term [PORT] [オプション]

PORT を省くと、画面の中でポートを選ぶか、パスを入力する。
1 プロセスが 1 接続。分割は herdr、tmux、Windows Terminal が担当する。

オプション:
  -b, --baud <N>              ボーレート（1〜4000000、既定 115200）
      --data <5|6|7|8>        データビット（既定 8）
      --parity <none|odd|even>
      --stop <1|2>
      --flow <none|xonxoff|rtscts>
  -e, --encoding <sjis|utf8>
      --log <PATH>            起動時にテキストログをこのファイルへ書く
      --prefix <ctrl-a>       プレフィックス（既定 ctrl-a）
  -h, --help

プレフィックスの次のキー:
  c 接続   d 切断   r 再接続   b ブレーク
  l ログ開始   L ログ停止   s 文字列   f ファイル
  u ボーレート   e エンコーディング   p ペースト
  ? ヘルプ   q 終了
  プレフィックスをもう一度押すと、その制御バイトを送る。
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
    pub help: bool,
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
                    return Err(format!("{flag} の値が空"));
                }
                Some(value) => {
                    index += 1;
                    value
                }
                None => {
                    index += 1;
                    if index >= args.len() || args[index].starts_with('-') {
                        return Err(format!("{flag} の値が無い"));
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
            return Err(format!("不明なオプション: {arg}"));
        }
        take_port(&mut cli, &arg)?;
        index += 1;
    }
    Ok(cli)
}

fn take_port(cli: &mut Cli, port: &str) -> Result<(), String> {
    if cli.port.is_some() {
        return Err("ポートは1つだけ指定できる".into());
    }
    if port.is_empty() {
        return Err("ポート名が空".into());
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
        "--data" => cli.data_bits = Some(parse_data(value)?),
        "--parity" => cli.parity = Some(parse_parity(value)?),
        "--stop" => cli.stop_bits = Some(parse_stop(value)?),
        "--flow" => cli.flow = Some(parse_flow(value)?),
        "-e" | "--encoding" => cli.encoding = Some(parse_encoding(value)?),
        "--log" => cli.log = Some(PathBuf::from(value)),
        "--prefix" => cli.prefix = parse_prefix(value)?,
        _ => return Err(format!("不明なオプション: {flag}")),
    }
    Ok(())
}

pub fn parse_baud(text: &str) -> Result<u32, String> {
    let baud: u32 = text
        .parse()
        .map_err(|_| format!("ボーレート `{text}` が数値でない"))?;
    if !(1..=4_000_000).contains(&baud) {
        return Err("ボーレートが範囲外（1〜4000000）".into());
    }
    Ok(baud)
}

pub fn parse_encoding(text: &str) -> Result<SessionEncoding, String> {
    match text.to_ascii_lowercase().as_str() {
        "sjis" | "shift_jis" | "shift-jis" => Ok(SessionEncoding::ShiftJis),
        "utf8" | "utf-8" => Ok(SessionEncoding::Utf8),
        other => Err(format!("エンコーディング `{other}` は sjis か utf8 だけ")),
    }
}

fn parse_data(text: &str) -> Result<DataBits, String> {
    match text.to_ascii_lowercase().as_str() {
        "5" | "five" => Ok(DataBits::Five),
        "6" | "six" => Ok(DataBits::Six),
        "7" | "seven" => Ok(DataBits::Seven),
        "8" | "eight" => Ok(DataBits::Eight),
        other => Err(format!("データビット `{other}` は 5, 6, 7, 8 のどれか")),
    }
}

fn parse_parity(text: &str) -> Result<Parity, String> {
    match text.to_ascii_lowercase().as_str() {
        "n" | "none" => Ok(Parity::None),
        "o" | "odd" => Ok(Parity::Odd),
        "e" | "even" => Ok(Parity::Even),
        other => Err(format!("パリティ `{other}` は none, odd, even のどれか")),
    }
}

fn parse_stop(text: &str) -> Result<StopBits, String> {
    match text.to_ascii_lowercase().as_str() {
        "1" | "one" => Ok(StopBits::One),
        "2" | "two" => Ok(StopBits::Two),
        other => Err(format!("ストップビット `{other}` は 1 か 2")),
    }
}

fn parse_flow(text: &str) -> Result<Flow, String> {
    match text.to_ascii_lowercase().as_str() {
        "none" => Ok(Flow::None),
        "xonxoff" | "xon/xoff" => Ok(Flow::XonXoff),
        "rtscts" | "rts/cts" => Ok(Flow::RtsCts),
        other => Err(format!(
            "フロー `{other}` は none, xonxoff, rtscts のどれか"
        )),
    }
}

pub fn parse_prefix(text: &str) -> Result<Prefix, String> {
    let lower = text.to_ascii_lowercase();
    let body = lower
        .strip_prefix("ctrl-")
        .or_else(|| lower.strip_prefix("ctrl+"))
        .or_else(|| lower.strip_prefix("c-"))
        .or_else(|| lower.strip_prefix("c+"))
        .or_else(|| lower.strip_prefix('^'));
    let Some(body) = body else {
        return Err("プレフィックスは ctrl-英字 で指定する（例: ctrl-a）".into());
    };
    let mut chars = body.chars();
    let Some(ch) = chars.next() else {
        return Err("プレフィックスは ctrl-英字 で指定する（例: ctrl-a）".into());
    };
    if chars.next().is_some() {
        return Err("プレフィックスは ctrl-英字 で指定する（例: ctrl-a）".into());
    }
    if encode_key(&KeyInput::Ctrl(ch), false).is_err() {
        return Err(format!("ctrl-{ch} には制御コードが無い"));
    }
    Ok(Prefix {
        ctrl: ch.to_ascii_lowercase(),
    })
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
        assert!(parse(["--nope"]).unwrap_err().contains("不明"));
        assert!(parse(["COM1", "COM2"]).is_err());
        assert!(parse(["--prefix", "shift-a"]).is_err());
    }

    #[test]
    fn prefix_can_be_ctrl_b() {
        let cli = parse(["--prefix", "C-b"]).unwrap();
        assert_eq!(cli.prefix.ctrl, 'b');
        assert!(parse(["-h"]).unwrap().help);
    }
}
