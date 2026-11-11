//! Serial sessions for rs-term.
//!
//! This crate does not depend on GPUI. The pane tree lives in the shell.

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
    palette_by_name, resolve_log_dir, save_settings_file, settings_path, settings_path_from_env,
    windows_keymap_path, windows_log_dir, windows_settings_path, DirEnv, LogSettings, OsKind,
    SerialSettings, Settings, TerminalPalette, TerminalSettings, UiSettings,
};

/// TOML names of the domain enums, for files the UI owns (layout.toml).
pub mod names {
    pub use crate::config::{
        data_bits_name, encoding_name, flow_name, parity_name, parse_data_bits, parse_encoding,
        parse_flow, parse_parity, parse_receive, parse_stop, parse_transmit, receive_name,
        stop_name, transmit_name,
    };
}
pub use error::{LogError, OpenError, PortError, SessionError};
pub use log::{
    expand_log_name, port_token, suggested_log_path, validate_log_name, LogCommand, LogEncoding,
    LogSession, LogTarget, Rotate, Timestamp,
};
pub use screen::{AnsiName, CellColor, CellFlags, Rgb, ScreenCell, ScreenCursor, ScreenSnapshot};
pub use serial::{
    list_ports, usb_identity_matches, DataBits, Flow, Parity, PinState, PortInfo, PortKind,
    SerialParams, StopBits, UsbIdentity, MAX_PENDING, READ_TIMEOUT,
};
pub use session::{
    encode_key, key_to_bytes, line_to_viewport, viewport_to_line, Direction, EventRx, Input,
    IoEvent, KeyInput, Link, Message, OpenedPort, PendingOpen, ScreenConfig, Session,
    SessionConfig, Severity,
};
