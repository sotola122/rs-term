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
    classic_dark, light_palette, load_settings_file, load_settings_str, palette_by_name,
    save_settings_file, windows_keymap_path, windows_log_dir, windows_settings_path, Axis,
    LayoutNode, LayoutSettings, LogSettings, PaneSettings, SerialSettings, Settings,
    TerminalPalette, TerminalSettings, UiSettings,
};
pub use error::{LogError, OpenError, PortError, SessionError};
pub use log::{
    expand_log_name, port_token, validate_log_name, LogCommand, LogEncoding, LogSession, LogTarget,
    Rotate, Timestamp,
};
pub use screen::{AnsiName, CellColor, CellFlags, Rgb, ScreenCell, ScreenCursor, ScreenSnapshot};
pub use serial::{
    list_ports, usb_identity_matches, DataBits, Flow, Parity, PinState, PortInfo, PortKind,
    SerialParams, StopBits, UsbIdentity, READ_TIMEOUT,
};
pub use session::{
    Direction, EventRx, Input, IoEvent, KeyInput, Link, ScreenConfig, Session, SessionConfig,
};
