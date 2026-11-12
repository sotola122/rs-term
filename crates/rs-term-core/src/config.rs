//! `settings.toml` schema 1.
//!
//! A missing schema, a schema other than 1, an unknown key, a syntax error, or
//! an out-of-range number rejects the whole file. The file is not rewritten.
//! `ui.theme_mode` and `terminal.palette` are the exception the design calls
//! out: an unknown value falls back and is reported, and the rest of the file
//! still loads.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::codec::{Newline, ReceiveNewline, SessionEncoding, TransmitNewline};
use crate::screen::Rgb;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalPalette {
    pub name: String,
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub selection: Rgb,
    pub ansi: [Rgb; 16],
}

pub fn palette_by_name(name: &str) -> Option<TerminalPalette> {
    match name {
        "classic-dark" => Some(classic_dark()),
        "light" => Some(light_palette()),
        _ => None,
    }
}

pub fn classic_dark() -> TerminalPalette {
    TerminalPalette {
        name: "classic-dark".into(),
        foreground: Rgb {
            r: 0xD4,
            g: 0xD4,
            b: 0xD4,
        },
        background: Rgb { r: 0, g: 0, b: 0 },
        cursor: Rgb {
            r: 0x00,
            g: 0xC8,
            b: 0x00,
        },
        selection: Rgb {
            r: 0x26,
            g: 0x4F,
            b: 0x78,
        },
        ansi: ANSI_16,
    }
}

pub fn light_palette() -> TerminalPalette {
    TerminalPalette {
        name: "light".into(),
        foreground: Rgb {
            r: 0x1E,
            g: 0x1E,
            b: 0x1E,
        },
        background: Rgb {
            r: 0xFF,
            g: 0xFF,
            b: 0xFF,
        },
        cursor: Rgb {
            r: 0x00,
            g: 0x64,
            b: 0x00,
        },
        selection: Rgb {
            r: 0xAD,
            g: 0xD6,
            b: 0xFF,
        },
        ansi: ANSI_16,
    }
}

const ANSI_16: [Rgb; 16] = [
    Rgb {
        r: 0x00,
        g: 0x00,
        b: 0x00,
    },
    Rgb {
        r: 0xCD,
        g: 0x00,
        b: 0x00,
    },
    Rgb {
        r: 0x00,
        g: 0xCD,
        b: 0x00,
    },
    Rgb {
        r: 0xCD,
        g: 0xCD,
        b: 0x00,
    },
    Rgb {
        r: 0x00,
        g: 0x00,
        b: 0xEE,
    },
    Rgb {
        r: 0xCD,
        g: 0x00,
        b: 0xCD,
    },
    Rgb {
        r: 0x00,
        g: 0xCD,
        b: 0xCD,
    },
    Rgb {
        r: 0xE5,
        g: 0xE5,
        b: 0xE5,
    },
    Rgb {
        r: 0x7F,
        g: 0x7F,
        b: 0x7F,
    },
    Rgb {
        r: 0xFF,
        g: 0x00,
        b: 0x00,
    },
    Rgb {
        r: 0x00,
        g: 0xFF,
        b: 0x00,
    },
    Rgb {
        r: 0xFF,
        g: 0xFF,
        b: 0x00,
    },
    Rgb {
        r: 0x5C,
        g: 0x5C,
        b: 0xFF,
    },
    Rgb {
        r: 0xFF,
        g: 0x00,
        b: 0xFF,
    },
    Rgb {
        r: 0x00,
        g: 0xFF,
        b: 0xFF,
    },
    Rgb {
        r: 0xFF,
        g: 0xFF,
        b: 0xFF,
    },
];

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub serial: SerialSettings,
    pub newline: Newline,
    pub encoding: SessionEncoding,
    pub terminal: TerminalSettings,
    pub ui: UiSettings,
    pub log: LogSettings,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SerialSettings {
    pub baud_rate: u32,
    pub data_bits: crate::DataBits,
    pub parity: crate::Parity,
    pub stop_bits: crate::StopBits,
    pub flow: crate::Flow,
    pub read_timeout_ms: u64,
    pub reconnect: bool,
    pub reopen_on_start: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TerminalSettings {
    pub columns: usize,
    pub rows: usize,
    pub scrollback: usize,
    pub local_echo: bool,
    pub palette: String,
    pub font_family: String,
    pub font_size_px: f32,
    pub font_fallbacks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiSettings {
    pub theme_mode: String,
    pub font_family: String,
    pub font_size_px: f32,
    pub ligatures: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogSettings {
    pub directory: String,
    pub name_template: String,
    pub rotate_bytes: u64,
    pub rotate_keep: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            serial: SerialSettings {
                baud_rate: 115_200,
                data_bits: crate::DataBits::Eight,
                parity: crate::Parity::None,
                stop_bits: crate::StopBits::One,
                flow: crate::Flow::None,
                read_timeout_ms: 20,
                reconnect: false,
                reopen_on_start: false,
            },
            newline: Newline {
                receive: ReceiveNewline::Auto,
                transmit: TransmitNewline::Cr,
            },
            encoding: SessionEncoding::ShiftJis,
            terminal: TerminalSettings {
                columns: 80,
                rows: 24,
                scrollback: 10_000,
                local_echo: false,
                palette: "classic-dark".into(),
                font_family: String::new(),
                font_size_px: 14.0,
                font_fallbacks: vec!["Yu Gothic".into(), "MS Gothic".into()],
            },
            ui: UiSettings {
                theme_mode: "dark".into(),
                font_family: String::new(),
                font_size_px: 16.0,
                ligatures: false,
            },
            log: LogSettings {
                directory: String::new(),
                name_template: "{port}_{date}_{time}.log".into(),
                rotate_bytes: 10_485_760,
                rotate_keep: 5,
            },
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSettings {
    schema: u32,
    serial: RawSerial,
    newline: RawNewline,
    encoding: RawEncoding,
    terminal: RawTerminal,
    ui: RawUi,
    log: RawLog,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSerial {
    baud_rate: u32,
    data_bits: String,
    parity: String,
    stop_bits: String,
    flow: String,
    read_timeout_ms: u64,
    reconnect: bool,
    reopen_on_start: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNewline {
    receive: String,
    transmit: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEncoding {
    session: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTerminal {
    columns: usize,
    rows: usize,
    scrollback: usize,
    local_echo: bool,
    palette: String,
    font_family: String,
    font_size_px: f32,
    font_fallbacks: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawUi {
    theme_mode: String,
    font_family: String,
    font_size_px: f32,
    ligatures: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLog {
    directory: String,
    name_template: String,
    rotate_bytes: u64,
    rotate_keep: u32,
    text_encoding: String,
}

#[derive(Debug, Serialize)]
struct SavedSettings {
    schema: u32,
    serial: SavedSerial,
    newline: SavedNewline,
    encoding: SavedEncoding,
    terminal: SavedTerminal,
    ui: SavedUi,
    log: SavedLog,
}

#[derive(Debug, Serialize)]
struct SavedSerial {
    baud_rate: u32,
    data_bits: &'static str,
    parity: &'static str,
    stop_bits: &'static str,
    flow: &'static str,
    read_timeout_ms: u64,
    reconnect: bool,
    reopen_on_start: bool,
}

#[derive(Debug, Serialize)]
struct SavedNewline {
    receive: &'static str,
    transmit: &'static str,
}

#[derive(Debug, Serialize)]
struct SavedEncoding {
    session: &'static str,
}

#[derive(Debug, Serialize)]
struct SavedTerminal {
    columns: usize,
    rows: usize,
    scrollback: usize,
    local_echo: bool,
    palette: String,
    font_family: String,
    font_size_px: f32,
    font_fallbacks: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SavedUi {
    theme_mode: String,
    font_family: String,
    font_size_px: f32,
    ligatures: bool,
}

#[derive(Debug, Serialize)]
struct SavedLog {
    directory: String,
    name_template: String,
    rotate_bytes: u64,
    rotate_keep: u32,
    text_encoding: &'static str,
}

/// Windows settings path. Empty or missing `APPDATA` is `None`.
pub fn windows_settings_path(appdata: Option<&str>) -> Option<PathBuf> {
    let appdata = appdata.filter(|value| !value.is_empty())?;
    Some(PathBuf::from(appdata).join("rs-term").join("settings.toml"))
}

/// Windows keymap path. Empty or missing `APPDATA` is `None`.
pub fn windows_keymap_path(appdata: Option<&str>) -> Option<PathBuf> {
    let appdata = appdata.filter(|value| !value.is_empty())?;
    Some(PathBuf::from(appdata).join("rs-term").join("keymap.toml"))
}

/// Windows log directory. Empty or missing `LOCALAPPDATA` is `None`.
pub fn windows_log_dir(local_appdata: Option<&str>) -> Option<PathBuf> {
    let local = local_appdata.filter(|value| !value.is_empty())?;
    Some(PathBuf::from(local).join("rs-term").join("logs"))
}

/// Which OS a path is resolved for. Tests pass this in so they do not depend
/// on the runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsKind {
    Windows,
    Macos,
    Linux,
}

/// Environment values used to place settings and logs. Empty strings are
/// treated as missing by [`DirEnv::from_process`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEnv {
    pub os: OsKind,
    pub home: Option<String>,
    pub appdata: Option<String>,
    pub local_appdata: Option<String>,
    pub xdg_config_home: Option<String>,
    pub xdg_state_home: Option<String>,
}

impl DirEnv {
    pub fn from_process() -> Self {
        Self {
            os: current_os(),
            home: env_nonempty("HOME").or_else(|| env_nonempty("USERPROFILE")),
            appdata: env_nonempty("APPDATA"),
            local_appdata: env_nonempty("LOCALAPPDATA"),
            xdg_config_home: env_nonempty("XDG_CONFIG_HOME"),
            xdg_state_home: env_nonempty("XDG_STATE_HOME"),
        }
    }
}

fn current_os() -> OsKind {
    if cfg!(windows) {
        OsKind::Windows
    } else if cfg!(target_os = "macos") {
        OsKind::Macos
    } else {
        OsKind::Linux
    }
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|value| !value.is_empty())
}

fn filled(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|text| !text.is_empty())
}

/// `settings.toml` for this environment. `None` when no home directory is known.
pub fn settings_path(env: &DirEnv) -> Option<PathBuf> {
    config_dir(env).map(|dir| dir.join("settings.toml"))
}

pub fn settings_path_from_env() -> Option<PathBuf> {
    settings_path(&DirEnv::from_process())
}

fn config_dir(env: &DirEnv) -> Option<PathBuf> {
    match env.os {
        OsKind::Windows => filled(&env.appdata).map(|path| PathBuf::from(path).join("rs-term")),
        OsKind::Macos => filled(&env.home)
            .map(|home| PathBuf::from(home).join("Library/Application Support/rs-term")),
        OsKind::Linux => {
            if let Some(config) = filled(&env.xdg_config_home) {
                return Some(PathBuf::from(config).join("rs-term"));
            }
            filled(&env.home).map(|home| PathBuf::from(home).join(".config/rs-term"))
        }
    }
}

/// Log directory. A non-empty `configured` value wins. Otherwise the platform
/// directory, then the process temp dir. Linux does not consult `LOCALAPPDATA`.
pub fn resolve_log_dir(configured: &str, env: &DirEnv) -> PathBuf {
    if !configured.is_empty() {
        return PathBuf::from(configured);
    }
    platform_log_dir(env).unwrap_or_else(|| std::env::temp_dir().join("rs-term").join("logs"))
}

pub fn log_dir_from_env(configured: &str) -> PathBuf {
    resolve_log_dir(configured, &DirEnv::from_process())
}

fn platform_log_dir(env: &DirEnv) -> Option<PathBuf> {
    match env.os {
        OsKind::Windows => windows_log_dir(filled(&env.local_appdata)),
        OsKind::Macos => {
            filled(&env.home).map(|home| PathBuf::from(home).join("Library/Logs/rs-term"))
        }
        OsKind::Linux => {
            if let Some(state) = filled(&env.xdg_state_home) {
                return Some(PathBuf::from(state).join("rs-term").join("logs"));
            }
            filled(&env.home).map(|home| PathBuf::from(home).join(".local/state/rs-term/logs"))
        }
    }
}

pub fn load_settings_str(text: &str) -> Result<Settings, String> {
    let raw: RawSettings =
        toml::from_str(text).map_err(|error| format!("TOML として読めない: {error}"))?;
    if raw.schema != 1 {
        return Err(format!("schema {} は読めない（1 だけ）", raw.schema));
    }
    interpret(raw)
}

pub fn load_settings_file(path: &Path) -> Result<Settings, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("読めない（OS: {error}）"))?;
    load_settings_str(&text)
}

/// Write `settings.toml.tmp`, `sync_all`, then rename over `path`.
pub fn save_settings_file(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let tmp = tmp_path(path);
    let body = toml::to_string_pretty(&saved(settings)).map_err(|error| error.to_string())?;
    let mut file = File::create(&tmp).map_err(|error| error.to_string())?;
    file.write_all(body.as_bytes())
        .map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    drop(file);
    fs::rename(&tmp, path).map_err(|error| {
        let _ = fs::remove_file(&tmp);
        error.to_string()
    })
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

fn interpret(raw: RawSettings) -> Result<Settings, String> {
    if !(1..=4_000_000).contains(&raw.serial.baud_rate) {
        return Err("serial.baud_rate が範囲外（1〜4000000）".into());
    }
    // Commands are taken between reads, so the timeout bounds key latency
    // and the stop time (review N13).
    if raw.serial.read_timeout_ms == 0 || raw.serial.read_timeout_ms > 100 {
        return Err("serial.read_timeout_ms が範囲外（1〜100）".into());
    }
    if raw.terminal.scrollback > 100_000 {
        return Err("terminal.scrollback が範囲外（0〜100000）".into());
    }
    if raw.terminal.columns == 0
        || raw.terminal.rows == 0
        || raw.terminal.columns > 500
        || raw.terminal.rows > 500
    {
        return Err("terminal.columns または rows が範囲外（1〜500）".into());
    }
    if !(1.0..=72.0).contains(&raw.terminal.font_size_px)
        || !(1.0..=72.0).contains(&raw.ui.font_size_px)
    {
        return Err("font_size_px が範囲外（1〜72）".into());
    }
    if !(64 * 1024..=1024 * 1024 * 1024).contains(&raw.log.rotate_bytes) {
        return Err("log.rotate_bytes が範囲外（64 KiB〜1 GiB）".into());
    }
    if !(1..=20).contains(&raw.log.rotate_keep) {
        return Err("log.rotate_keep が範囲外（1〜20）".into());
    }
    if raw.log.text_encoding != "utf-8" {
        return Err("log.text_encoding は utf-8 だけ".into());
    }
    let mut warnings = Vec::new();
    let mut palette = raw.terminal.palette;
    if palette_by_name(&palette).is_none() {
        warnings.push(format!(
            "terminal.palette `{palette}` は未知なので classic-dark"
        ));
        palette = "classic-dark".into();
    }
    let mut theme_mode = raw.ui.theme_mode;
    if theme_mode != "light" && theme_mode != "dark" {
        warnings.push(format!("ui.theme_mode `{theme_mode}` は未知なので dark"));
        theme_mode = "dark".into();
    }
    Ok(Settings {
        serial: SerialSettings {
            baud_rate: raw.serial.baud_rate,
            data_bits: parse_data_bits(&raw.serial.data_bits)?,
            parity: parse_parity(&raw.serial.parity)?,
            stop_bits: parse_stop(&raw.serial.stop_bits)?,
            flow: parse_flow(&raw.serial.flow)?,
            read_timeout_ms: raw.serial.read_timeout_ms,
            reconnect: raw.serial.reconnect,
            reopen_on_start: raw.serial.reopen_on_start,
        },
        newline: Newline {
            receive: parse_receive(&raw.newline.receive)?,
            transmit: parse_transmit(&raw.newline.transmit)?,
        },
        encoding: parse_encoding(&raw.encoding.session)?,
        terminal: TerminalSettings {
            columns: raw.terminal.columns,
            rows: raw.terminal.rows,
            scrollback: raw.terminal.scrollback,
            local_echo: raw.terminal.local_echo,
            palette,
            font_family: raw.terminal.font_family,
            font_size_px: raw.terminal.font_size_px,
            font_fallbacks: raw.terminal.font_fallbacks,
        },
        ui: UiSettings {
            theme_mode,
            font_family: raw.ui.font_family,
            font_size_px: raw.ui.font_size_px,
            ligatures: raw.ui.ligatures,
        },
        log: LogSettings {
            directory: raw.log.directory,
            name_template: raw.log.name_template,
            rotate_bytes: raw.log.rotate_bytes,
            rotate_keep: raw.log.rotate_keep,
        },
        warnings,
    })
}

fn saved(settings: &Settings) -> SavedSettings {
    SavedSettings {
        schema: 1,
        serial: SavedSerial {
            baud_rate: settings.serial.baud_rate,
            data_bits: data_bits_name(settings.serial.data_bits),
            parity: parity_name(settings.serial.parity),
            stop_bits: stop_name(settings.serial.stop_bits),
            flow: flow_name(settings.serial.flow),
            read_timeout_ms: settings.serial.read_timeout_ms,
            reconnect: settings.serial.reconnect,
            reopen_on_start: settings.serial.reopen_on_start,
        },
        newline: SavedNewline {
            receive: receive_name(settings.newline.receive),
            transmit: transmit_name(settings.newline.transmit),
        },
        encoding: SavedEncoding {
            session: encoding_name(settings.encoding),
        },
        terminal: SavedTerminal {
            columns: settings.terminal.columns,
            rows: settings.terminal.rows,
            scrollback: settings.terminal.scrollback,
            local_echo: settings.terminal.local_echo,
            palette: settings.terminal.palette.clone(),
            font_family: settings.terminal.font_family.clone(),
            font_size_px: settings.terminal.font_size_px,
            font_fallbacks: settings.terminal.font_fallbacks.clone(),
        },
        ui: SavedUi {
            theme_mode: settings.ui.theme_mode.clone(),
            font_family: settings.ui.font_family.clone(),
            font_size_px: settings.ui.font_size_px,
            ligatures: settings.ui.ligatures,
        },
        log: SavedLog {
            directory: settings.log.directory.clone(),
            name_template: settings.log.name_template.clone(),
            rotate_bytes: settings.log.rotate_bytes,
            rotate_keep: settings.log.rotate_keep,
            text_encoding: "utf-8",
        },
    }
}

pub fn parse_data_bits(text: &str) -> Result<crate::DataBits, String> {
    match text {
        "five" => Ok(crate::DataBits::Five),
        "six" => Ok(crate::DataBits::Six),
        "seven" => Ok(crate::DataBits::Seven),
        "eight" => Ok(crate::DataBits::Eight),
        other => Err(format!("data_bits `{other}` は未知")),
    }
}

pub fn parse_parity(text: &str) -> Result<crate::Parity, String> {
    match text {
        "none" => Ok(crate::Parity::None),
        "odd" => Ok(crate::Parity::Odd),
        "even" => Ok(crate::Parity::Even),
        other => Err(format!("parity `{other}` は未知")),
    }
}

pub fn parse_stop(text: &str) -> Result<crate::StopBits, String> {
    match text {
        "one" => Ok(crate::StopBits::One),
        "two" => Ok(crate::StopBits::Two),
        other => Err(format!("stop_bits `{other}` は未知")),
    }
}

pub fn parse_flow(text: &str) -> Result<crate::Flow, String> {
    match text {
        "none" => Ok(crate::Flow::None),
        "xonxoff" => Ok(crate::Flow::XonXoff),
        "rtscts" => Ok(crate::Flow::RtsCts),
        other => Err(format!("flow `{other}` は未知")),
    }
}

pub fn parse_receive(text: &str) -> Result<ReceiveNewline, String> {
    match text {
        "cr" => Ok(ReceiveNewline::Cr),
        "crlf" => Ok(ReceiveNewline::CrLf),
        "lf" => Ok(ReceiveNewline::Lf),
        "auto" => Ok(ReceiveNewline::Auto),
        other => Err(format!("receive `{other}` は未知")),
    }
}

pub fn parse_transmit(text: &str) -> Result<TransmitNewline, String> {
    match text {
        "cr" => Ok(TransmitNewline::Cr),
        "crlf" => Ok(TransmitNewline::CrLf),
        "lf" => Ok(TransmitNewline::Lf),
        other => Err(format!("transmit `{other}` は未知")),
    }
}

pub fn parse_encoding(text: &str) -> Result<SessionEncoding, String> {
    match text {
        "utf-8" => Ok(SessionEncoding::Utf8),
        "shift_jis" => Ok(SessionEncoding::ShiftJis),
        other => Err(format!("encoding `{other}` は未知")),
    }
}

pub fn data_bits_name(value: crate::DataBits) -> &'static str {
    match value {
        crate::DataBits::Five => "five",
        crate::DataBits::Six => "six",
        crate::DataBits::Seven => "seven",
        crate::DataBits::Eight => "eight",
    }
}

pub fn parity_name(value: crate::Parity) -> &'static str {
    match value {
        crate::Parity::None => "none",
        crate::Parity::Odd => "odd",
        crate::Parity::Even => "even",
    }
}

pub fn stop_name(value: crate::StopBits) -> &'static str {
    match value {
        crate::StopBits::One => "one",
        crate::StopBits::Two => "two",
    }
}

pub fn flow_name(value: crate::Flow) -> &'static str {
    match value {
        crate::Flow::None => "none",
        crate::Flow::XonXoff => "xonxoff",
        crate::Flow::RtsCts => "rtscts",
    }
}

pub fn receive_name(value: ReceiveNewline) -> &'static str {
    match value {
        ReceiveNewline::Cr => "cr",
        ReceiveNewline::CrLf => "crlf",
        ReceiveNewline::Lf => "lf",
        ReceiveNewline::Auto => "auto",
    }
}

pub fn transmit_name(value: TransmitNewline) -> &'static str {
    match value {
        TransmitNewline::Cr => "cr",
        TransmitNewline::CrLf => "crlf",
        TransmitNewline::Lf => "lf",
    }
}

pub fn encoding_name(value: SessionEncoding) -> &'static str {
    match value {
        SessionEncoding::Utf8 => "utf-8",
        SessionEncoding::ShiftJis => "shift_jis",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
schema = 1

[serial]
baud_rate = 115200
data_bits = "eight"
parity = "none"
stop_bits = "one"
flow = "none"
read_timeout_ms = 20
reconnect = false
reopen_on_start = false

[newline]
receive = "auto"
transmit = "cr"

[encoding]
session = "shift_jis"

[terminal]
columns = 80
rows = 24
scrollback = 10000
local_echo = false
palette = "classic-dark"
font_family = ""
font_size_px = 14.0
font_fallbacks = ["Yu Gothic", "MS Gothic"]

[ui]
theme_mode = "dark"
font_family = ""
font_size_px = 16.0
ligatures = false

[log]
directory = ""
name_template = "{port}_{date}_{time}.log"
rotate_bytes = 10485760
rotate_keep = 5
text_encoding = "utf-8"
"#;

    #[test]
    fn sample_loads_and_round_trips() {
        let loaded = load_settings_str(SAMPLE).unwrap();
        assert_eq!(loaded.encoding, SessionEncoding::ShiftJis);
        assert!(loaded.warnings.is_empty());
        let dir = std::env::temp_dir().join(format!("rs-term-settings-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.toml");
        save_settings_file(&path, &loaded).unwrap();
        let again = load_settings_file(&path).unwrap();
        assert_eq!(again.serial.baud_rate, 115_200);
        assert_eq!(again.terminal.palette, "classic-dark");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_key_rejects_the_file() {
        let text = SAMPLE.replace("ligatures = false", "ligatures = false\nextra = 1");
        assert!(load_settings_str(&text).is_err());
    }

    #[test]
    fn schema_above_one_rejects_the_file() {
        let text = SAMPLE.replace("schema = 1", "schema = 2");
        assert!(load_settings_str(&text).is_err());
    }

    #[test]
    fn read_timeout_above_100_ms_rejects_the_file() {
        let text = SAMPLE.replace("read_timeout_ms = 20", "read_timeout_ms = 101");
        assert!(load_settings_str(&text).is_err());
        let text = SAMPLE.replace("read_timeout_ms = 20", "read_timeout_ms = 100");
        assert!(load_settings_str(&text).is_ok());
    }

    #[test]
    fn layout_is_not_a_core_setting() {
        // Layout belongs to the terminal multiplexer, not to rs-term.
        let text = format!("{SAMPLE}\n[layout]\n");
        assert!(load_settings_str(&text).is_err());
    }

    #[test]
    fn out_of_range_rejects_the_file() {
        let text = SAMPLE.replace("baud_rate = 115200", "baud_rate = 0");
        assert!(load_settings_str(&text).is_err());
        let text = SAMPLE.replace("scrollback = 10000", "scrollback = 100001");
        assert!(load_settings_str(&text).is_err());
    }

    #[test]
    fn unknown_theme_falls_back_without_rejecting() {
        let text = SAMPLE.replace("theme_mode = \"dark\"", "theme_mode = \"blue\"");
        let loaded = load_settings_str(&text).unwrap();
        assert_eq!(loaded.ui.theme_mode, "dark");
        assert!(!loaded.warnings.is_empty());
        let text = SAMPLE.replace("palette = \"classic-dark\"", "palette = \"nope\"");
        let loaded = load_settings_str(&text).unwrap();
        assert_eq!(loaded.terminal.palette, "classic-dark");
    }

    #[test]
    fn empty_appdata_is_not_a_guessed_path() {
        assert!(windows_settings_path(None).is_none());
        assert!(windows_settings_path(Some("")).is_none());
        assert!(windows_log_dir(Some("")).is_none());
        let path = windows_settings_path(Some("C:\\Users\\a\\AppData\\Roaming")).unwrap();
        assert!(
            path.ends_with("rs-term/settings.toml") || path.ends_with("rs-term\\settings.toml")
        );
    }

    fn env_linux() -> DirEnv {
        DirEnv {
            os: OsKind::Linux,
            home: None,
            appdata: None,
            local_appdata: None,
            xdg_config_home: None,
            xdg_state_home: None,
        }
    }

    #[test]
    fn linux_log_dir_uses_xdg_state_and_ignores_localappdata() {
        let mut env = env_linux();
        env.local_appdata = Some(r"C:\Users\a\AppData\Local".into());
        env.xdg_state_home = Some("/var/state".into());
        assert_eq!(
            resolve_log_dir("", &env),
            PathBuf::from("/var/state/rs-term/logs")
        );
    }

    #[test]
    fn linux_log_dir_falls_back_to_home_state() {
        let mut env = env_linux();
        env.home = Some("/home/a".into());
        env.local_appdata = Some(r"C:\Users\a\AppData\Local".into());
        assert_eq!(
            resolve_log_dir("", &env),
            PathBuf::from("/home/a/.local/state/rs-term/logs")
        );
    }

    #[test]
    fn configured_log_directory_wins() {
        let env = env_linux();
        assert_eq!(
            resolve_log_dir("/custom/logs", &env),
            PathBuf::from("/custom/logs")
        );
    }

    #[test]
    fn missing_linux_dirs_fall_back_to_temp_not_localappdata() {
        let path = resolve_log_dir("", &env_linux());
        assert_eq!(path, std::env::temp_dir().join("rs-term").join("logs"));
        assert!(!path.display().to_string().contains("LOCALAPPDATA"));
    }

    #[test]
    fn windows_log_dir_uses_localappdata() {
        let env = DirEnv {
            os: OsKind::Windows,
            home: Some(r"C:\Users\a".into()),
            appdata: Some(r"C:\Users\a\AppData\Roaming".into()),
            local_appdata: Some(r"C:\Users\a\AppData\Local".into()),
            xdg_config_home: None,
            xdg_state_home: None,
        };
        let path = resolve_log_dir("", &env);
        assert!(
            path.ends_with("rs-term/logs") || path.ends_with(r"rs-term\logs"),
            "{path:?}"
        );
        let text = path.display().to_string();
        assert!(text.contains("Local"), "{text}");
    }

    #[test]
    fn linux_settings_path_prefers_xdg_config_home() {
        let mut env = env_linux();
        env.home = Some("/home/a".into());
        env.xdg_config_home = Some("/cfg".into());
        assert_eq!(
            settings_path(&env),
            Some(PathBuf::from("/cfg/rs-term/settings.toml"))
        );
        env.xdg_config_home = None;
        assert_eq!(
            settings_path(&env),
            Some(PathBuf::from("/home/a/.config/rs-term/settings.toml"))
        );
    }
}
