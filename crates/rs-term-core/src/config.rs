//! `settings.toml` schema 1.
//!
//! Every table and key is optional; an omitted one takes the built-in default.
//! A schema other than 1, an unknown key, a syntax error, or an out-of-range
//! number rejects the whole file. The file is not rewritten.
//! Two things do not reject the file. An unknown `terminal.palette` falls back
//! and is reported. The GUI-era keys the TUI never used (`serial.reconnect`,
//! `serial.reopen_on_start`, `terminal.font_*`, the `[ui]` and `[layout]`
//! tables) are ignored
//! and reported in one warning.

use std::fs;
use std::path::{Path, PathBuf};

use serde::de::IgnoredAny;
use serde::Deserialize;

use crate::codec::{Newline, ReceiveNewline, SessionEncoding, TransmitNewline};
use crate::screen::{AmbiguousWidth, Rgb};
use crate::serial::{DataBits, Flow, Parity, StopBits};

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
    pub log: LogSettings,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SerialSettings {
    pub baud_rate: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
    pub flow: Flow,
    pub read_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TerminalSettings {
    pub columns: usize,
    pub rows: usize,
    pub scrollback: usize,
    pub local_echo: bool,
    pub palette: String,
    pub ambiguous_width: AmbiguousWidth,
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
                data_bits: DataBits::Eight,
                parity: Parity::None,
                stop_bits: StopBits::One,
                flow: Flow::None,
                read_timeout_ms: 20,
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
                ambiguous_width: AmbiguousWidth::Auto,
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

/// Every table and key is optional: an omitted one takes the built-in
/// default. The GUI-era keys the TUI never read are still accepted (and
/// ignored) so an old file keeps loading; they are listed in a warning.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSettings {
    schema: Option<u32>,
    #[serde(default)]
    serial: RawSerial,
    #[serde(default)]
    newline: RawNewline,
    #[serde(default)]
    encoding: RawEncoding,
    #[serde(default)]
    terminal: RawTerminal,
    #[serde(default)]
    log: RawLog,
    ui: Option<IgnoredAny>,
    layout: Option<IgnoredAny>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSerial {
    baud_rate: Option<u32>,
    data_bits: Option<String>,
    parity: Option<String>,
    stop_bits: Option<String>,
    flow: Option<String>,
    read_timeout_ms: Option<u64>,
    reconnect: Option<IgnoredAny>,
    reopen_on_start: Option<IgnoredAny>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNewline {
    receive: Option<String>,
    transmit: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEncoding {
    session: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTerminal {
    columns: Option<usize>,
    rows: Option<usize>,
    scrollback: Option<usize>,
    local_echo: Option<bool>,
    palette: Option<String>,
    ambiguous_width: Option<String>,
    font_family: Option<IgnoredAny>,
    font_size_px: Option<IgnoredAny>,
    font_fallbacks: Option<IgnoredAny>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLog {
    directory: Option<String>,
    name_template: Option<String>,
    rotate_bytes: Option<u64>,
    rotate_keep: Option<u32>,
    text_encoding: Option<String>,
}

/// Windows settings path. Empty or missing `APPDATA` is `None`.
pub fn windows_settings_path(appdata: Option<&str>) -> Option<PathBuf> {
    let appdata = appdata.filter(|value| !value.is_empty())?;
    Some(PathBuf::from(appdata).join("rs-term").join("settings.toml"))
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
        toml::from_str(text).map_err(|error| format!("Not valid TOML: {error}"))?;
    if let Some(schema) = raw.schema.filter(|schema| *schema != 1) {
        return Err(format!("schema {schema} is not supported (only 1)"));
    }
    interpret(raw)
}

pub fn load_settings_file(path: &Path) -> Result<Settings, String> {
    let text =
        fs::read_to_string(path).map_err(|error| format!("Cannot read the file (OS: {error})"))?;
    load_settings_str(&text)
}

/// An optional enum-valued key: parsed when given, else `default`.
fn pick<T>(
    text: Option<String>,
    default: T,
    key: &str,
    parse: fn(&str) -> Option<T>,
) -> Result<T, String> {
    match text {
        Some(text) => parse(&text).ok_or_else(|| format!("{key} `{text}` is not recognized")),
        None => Ok(default),
    }
}

fn interpret(raw: RawSettings) -> Result<Settings, String> {
    let d = Settings::default();
    let (serial, terminal, log) = (raw.serial, raw.terminal, raw.log);
    let baud_rate = serial.baud_rate.unwrap_or(d.serial.baud_rate);
    if !(1..=4_000_000).contains(&baud_rate) {
        return Err("serial.baud_rate is out of range (1 to 4000000)".into());
    }
    // Commands are taken between reads, so the timeout bounds key latency
    // and the stop time (review N13).
    let read_timeout_ms = serial.read_timeout_ms.unwrap_or(d.serial.read_timeout_ms);
    if read_timeout_ms == 0 || read_timeout_ms > 100 {
        return Err("serial.read_timeout_ms is out of range (1 to 100)".into());
    }
    let scrollback = terminal.scrollback.unwrap_or(d.terminal.scrollback);
    if scrollback > 100_000 {
        return Err("terminal.scrollback is out of range (0 to 100000)".into());
    }
    let columns = terminal.columns.unwrap_or(d.terminal.columns);
    let rows = terminal.rows.unwrap_or(d.terminal.rows);
    if !(1..=500).contains(&columns) || !(1..=500).contains(&rows) {
        return Err("terminal.columns or rows is out of range (1 to 500)".into());
    }
    let rotate_bytes = log.rotate_bytes.unwrap_or(d.log.rotate_bytes);
    if !(64 * 1024..=1024 * 1024 * 1024).contains(&rotate_bytes) {
        return Err("log.rotate_bytes is out of range (64 KiB to 1 GiB)".into());
    }
    let rotate_keep = log.rotate_keep.unwrap_or(d.log.rotate_keep);
    if !(1..=20).contains(&rotate_keep) {
        return Err("log.rotate_keep is out of range (1 to 20)".into());
    }
    if log
        .text_encoding
        .as_deref()
        .is_some_and(|name| name != "utf-8")
    {
        return Err("log.text_encoding must be utf-8".into());
    }
    let mut warnings = Vec::new();
    let legacy: Vec<&str> = [
        ("serial.reconnect", serial.reconnect.is_some()),
        ("serial.reopen_on_start", serial.reopen_on_start.is_some()),
        ("terminal.font_family", terminal.font_family.is_some()),
        ("terminal.font_size_px", terminal.font_size_px.is_some()),
        ("terminal.font_fallbacks", terminal.font_fallbacks.is_some()),
        ("ui", raw.ui.is_some()),
        ("layout", raw.layout.is_some()),
    ]
    .into_iter()
    .filter_map(|(name, present)| present.then_some(name))
    .collect();
    if !legacy.is_empty() {
        warnings.push(format!("Ignored unused settings: {}", legacy.join(", ")));
    }
    let mut palette = terminal.palette.unwrap_or(d.terminal.palette);
    if palette_by_name(&palette).is_none() {
        warnings.push(format!(
            "terminal.palette `{palette}` is not recognized; using classic-dark"
        ));
        palette = "classic-dark".into();
    }
    let ambiguous_width = pick(
        terminal.ambiguous_width,
        d.terminal.ambiguous_width,
        "terminal.ambiguous_width",
        AmbiguousWidth::parse,
    )?;
    Ok(Settings {
        serial: SerialSettings {
            baud_rate,
            data_bits: pick(
                serial.data_bits,
                d.serial.data_bits,
                "data_bits",
                DataBits::parse,
            )?,
            parity: pick(serial.parity, d.serial.parity, "parity", Parity::parse)?,
            stop_bits: pick(
                serial.stop_bits,
                d.serial.stop_bits,
                "stop_bits",
                StopBits::parse,
            )?,
            flow: pick(serial.flow, d.serial.flow, "flow", Flow::parse)?,
            read_timeout_ms,
        },
        newline: Newline {
            receive: pick(
                raw.newline.receive,
                d.newline.receive,
                "receive",
                parse_receive,
            )?,
            transmit: pick(
                raw.newline.transmit,
                d.newline.transmit,
                "transmit",
                parse_transmit,
            )?,
        },
        encoding: pick(
            raw.encoding.session,
            d.encoding,
            "encoding",
            SessionEncoding::parse,
        )?,
        terminal: TerminalSettings {
            columns,
            rows,
            scrollback,
            local_echo: terminal.local_echo.unwrap_or(d.terminal.local_echo),
            palette,
            ambiguous_width,
        },
        log: LogSettings {
            directory: log.directory.unwrap_or(d.log.directory),
            name_template: log.name_template.unwrap_or(d.log.name_template),
            rotate_bytes,
            rotate_keep,
        },
        warnings,
    })
}

fn parse_receive(text: &str) -> Option<ReceiveNewline> {
    match text.to_ascii_lowercase().as_str() {
        "cr" => Some(ReceiveNewline::Cr),
        "crlf" => Some(ReceiveNewline::CrLf),
        "lf" => Some(ReceiveNewline::Lf),
        "auto" => Some(ReceiveNewline::Auto),
        _ => None,
    }
}

fn parse_transmit(text: &str) -> Option<TransmitNewline> {
    match text.to_ascii_lowercase().as_str() {
        "cr" => Some(TransmitNewline::Cr),
        "crlf" => Some(TransmitNewline::CrLf),
        "lf" => Some(TransmitNewline::Lf),
        _ => None,
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
ambiguous_width = "auto"

[log]
directory = ""
name_template = "{port}_{date}_{time}.log"
rotate_bytes = 10485760
rotate_keep = 5
text_encoding = "utf-8"
"#;

    #[test]
    fn sample_loads() {
        let loaded = load_settings_str(SAMPLE).unwrap();
        assert_eq!(loaded.encoding, SessionEncoding::ShiftJis);
        assert!(loaded.warnings.is_empty());
        assert_eq!(loaded, Settings::default());
    }

    #[test]
    fn the_readme_settings_example_loads() {
        // A checkout may use CRLF line endings.
        let readme = include_str!("../../../README.md").replace("\r\n", "\n");
        let block = readme
            .split_once("```toml\n")
            .and_then(|(_, rest)| rest.split_once("```"))
            .map(|(block, _)| block)
            .expect("README has a toml block");
        let loaded = load_settings_str(block).expect("README example must load");
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert_eq!(loaded.serial.baud_rate, 115_200);
        assert_eq!(loaded.terminal.scrollback, 10_000);
        assert_eq!(loaded.terminal.ambiguous_width, AmbiguousWidth::Auto);
        // The README says these values are the built-in d.
        assert_eq!(loaded, Settings::default());
    }

    #[test]
    fn ambiguous_width_is_optional_and_validated() {
        let loaded = load_settings_str("[terminal]\nambiguous_width = \"2\"\n").unwrap();
        assert_eq!(loaded.terminal.ambiguous_width, AmbiguousWidth::Two);
        let loaded = load_settings_str("[terminal]\nambiguous_width = \"1\"\n").unwrap();
        assert_eq!(loaded.terminal.ambiguous_width, AmbiguousWidth::One);
        let loaded = load_settings_str("[terminal]\ncolumns = 80\n").unwrap();
        assert_eq!(loaded.terminal.ambiguous_width, AmbiguousWidth::Auto);
        let error = load_settings_str("[terminal]\nambiguous_width = \"3\"\n").unwrap_err();
        assert!(error.contains("ambiguous_width"), "{error}");
    }

    #[test]
    fn unknown_key_rejects_the_file() {
        let text = SAMPLE.replace("local_echo = false", "local_echo = false\nextra = 1");
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
    fn out_of_range_rejects_the_file() {
        let text = SAMPLE.replace("baud_rate = 115200", "baud_rate = 0");
        assert!(load_settings_str(&text).is_err());
        let text = SAMPLE.replace("scrollback = 10000", "scrollback = 100001");
        assert!(load_settings_str(&text).is_err());
    }

    #[test]
    fn unknown_palette_falls_back_without_rejecting() {
        let text = SAMPLE.replace("palette = \"classic-dark\"", "palette = \"nope\"");
        let loaded = load_settings_str(&text).unwrap();
        assert_eq!(loaded.terminal.palette, "classic-dark");
        assert_eq!(loaded.warnings.len(), 1);
    }

    #[test]
    fn empty_file_is_all_defaults_without_warnings() {
        let loaded = load_settings_str("").unwrap();
        assert_eq!(loaded, Settings::default());
        let loaded = load_settings_str(
            "schema = 1
",
        )
        .unwrap();
        assert_eq!(loaded, Settings::default());
    }

    #[test]
    fn partial_file_keeps_given_values_and_defaults_the_rest() {
        let text = "[serial]
baud_rate = 9600
parity = \"even\"

[terminal]
scrollback = 500

[encoding]
session = \"utf-8\"
";
        let loaded = load_settings_str(text).unwrap();
        let d = Settings::default();
        assert_eq!(loaded.serial.baud_rate, 9600);
        assert_eq!(loaded.serial.parity, Parity::Even);
        assert_eq!(loaded.serial.data_bits, d.serial.data_bits);
        assert_eq!(loaded.terminal.scrollback, 500);
        assert_eq!(loaded.terminal.columns, d.terminal.columns);
        assert_eq!(loaded.encoding, SessionEncoding::Utf8);
        assert_eq!(loaded.newline, d.newline);
        assert_eq!(loaded.log, d.log);
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn omitted_keys_do_not_skip_validation_of_given_ones() {
        assert!(load_settings_str(
            "[serial]
baud_rate = 0
"
        )
        .is_err());
        assert!(load_settings_str(
            "[serial]
parity = \"mark\"
"
        )
        .is_err());
        assert!(load_settings_str(
            "schema = 2
"
        )
        .is_err());
    }

    #[test]
    fn legacy_gui_keys_load_with_one_warning() {
        let text = format!(
            "{}
[ui]
theme_mode = \"blue\"
anything = 1
",
            SAMPLE
                .replace(
                    "read_timeout_ms = 20",
                    "read_timeout_ms = 20
reconnect = true
reopen_on_start = false"
                )
                .replace(
                    "local_echo = false",
                    "local_echo = false
font_family = \"x\"
font_size_px = 99.0
font_fallbacks = [\"a\"]"
                )
        );
        let loaded = load_settings_str(&text).unwrap();
        assert_eq!(loaded.warnings.len(), 1, "{:?}", loaded.warnings);
        let warning = &loaded.warnings[0];
        assert!(
            warning.starts_with("Ignored unused settings: "),
            "{warning}"
        );
        for key in [
            "serial.reconnect",
            "serial.reopen_on_start",
            "terminal.font_family",
            "terminal.font_size_px",
            "terminal.font_fallbacks",
            "ui",
        ] {
            assert!(warning.contains(key), "{warning}");
        }
        let expected = Settings {
            warnings: loaded.warnings.clone(),
            ..Settings::default()
        };
        assert_eq!(loaded, expected);
    }

    #[test]
    fn the_old_layout_tables_are_ignored_and_named() {
        let text = format!(
            "{}
[layout.tree]
kind = \"split\"

[[layout.panes]]
id = 1

[[layout.panes]]
id = 2
",
            SAMPLE.replace("baud_rate = 115200", "baud_rate = 9600")
        );
        let loaded = load_settings_str(&text).unwrap();
        assert_eq!(loaded.serial.baud_rate, 9600);
        assert_eq!(loaded.warnings.len(), 1, "{:?}", loaded.warnings);
        assert!(
            loaded.warnings[0].ends_with("layout"),
            "{:?}",
            loaded.warnings
        );
    }

    #[test]
    fn unknown_keys_still_reject_even_beside_legacy_ones() {
        assert!(load_settings_str(
            "[serial]
reconnect = true
bogus = 1
"
        )
        .is_err());
        assert!(load_settings_str(
            "bogus = 1
"
        )
        .is_err());
        assert!(load_settings_str(
            "[bogus]
"
        )
        .is_err());
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
