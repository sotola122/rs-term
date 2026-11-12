//! Serial sessions for rs-term.
//!
//! This crate does not depend on GPUI. The frontend (rs-term) owns the one session and all UI.

mod codec;
mod config;
mod error;
mod log;
mod screen;
mod serial;
mod session;

pub use codec::{Newline, ReceiveNewline, SessionEncoding, TransmitNewline};
pub use config::{
    classic_dark, light_palette, load_settings_file, load_settings_str, log_dir_from_env,
    palette_by_name, resolve_log_dir, settings_path, settings_path_from_env, windows_log_dir,
    windows_settings_path, DirEnv, LogSettings, OsKind, SerialSettings, Settings, TerminalPalette,
    TerminalSettings,
};

pub use error::{LogError, OpenError, PortError, SessionError};
pub use log::{
    expand_log_name, port_token, suggested_log_path, validate_log_name, LogCommand, LogEncoding,
    LogSession, LogTarget, Rotate, Timestamp,
};
pub use screen::{
    ambiguous_wide_auto, AmbiguousWidth, AnsiName, CellColor, CellFlags, ConsoleEnv, Rgb,
    ScreenCell, ScreenCursor, ScreenSnapshot,
};
pub use serial::{
    list_ports, DataBits, Flow, Parity, PortInfo, PortKind, SerialParams, StopBits, UsbIdentity,
    MAX_PENDING, READ_TIMEOUT,
};
pub use session::{
    encode_key, line_to_viewport, viewport_to_line, Direction, EventRx, Input, IoEvent, KeyInput,
    Link, Message, OpenedPort, PendingOpen, ScreenConfig, Session, SessionConfig, Severity,
};
