//! Domain errors. Every `Display` is the English text the status line
//! shows. OS text, when there is any, stays in parentheses after it.

use std::fmt;

/// Failure while opening a port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    Port(PortError),
    AlreadyOpen,
    /// A disconnect or a newer connect overtook this open.
    Cancelled,
}

/// Failure of a port operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortError {
    Io { message: String },
    Unsupported(&'static str),
    Closed,
}

/// Failure of a session operation that is not a log transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    NotOpen,
    Port(PortError),
    Unmappable { ch: char },
    Busy,
    TooLarge { bytes: usize, limit: usize },
    NoControlCode { ch: char },
}

/// Failure of a log command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogError {
    /// Another process holds the file.
    Busy,
    /// This session already logs to a different file.
    AlreadyRunning,
    InvalidName(String),
    Io {
        message: String,
    },
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::AlreadyOpen => {
                write!(f, "Already connected; disconnect first (prefix, then d)")
            }
            OpenError::Cancelled => write!(f, "Connection cancelled"),
            OpenError::Port(error) => write!(f, "{error}"),
        }
    }
}

impl fmt::Display for PortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PortError::Io { message } => write!(f, "{message}"),
            PortError::Unsupported(what) => write!(f, "{what}"),
            PortError::Closed => write!(f, "Port is closed"),
        }
    }
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::NotOpen => {
                write!(f, "Not connected; nothing sent (prefix, then c to connect)")
            }
            SessionError::Port(error) => write!(f, "{error}"),
            SessionError::Unmappable { ch } => write!(
                f,
                "'{ch}' (U+{:04X}) cannot be encoded in the current encoding; not sent",
                *ch as u32
            ),
            SessionError::Busy => write!(f, "Send queue is full; not sent"),
            SessionError::TooLarge { bytes, limit } => write!(
                f,
                "{bytes} bytes exceeds the {} MiB limit; not sent",
                limit / (1024 * 1024)
            ),
            SessionError::NoControlCode { ch } => {
                write!(f, "No control code for Ctrl+{ch}; not sent")
            }
        }
    }
}

impl fmt::Display for LogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogError::Busy => {
                write!(f, "Log file is in use by another process")
            }
            LogError::AlreadyRunning => {
                write!(f, "Already logging to another file; stop logging first")
            }
            LogError::InvalidName(reason) => write!(f, "Invalid log file name: {reason}"),
            LogError::Io { message } => write!(f, "Log write failed: {message}"),
        }
    }
}

impl std::error::Error for OpenError {}
impl std::error::Error for PortError {}
impl std::error::Error for SessionError {}
impl std::error::Error for LogError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_is_non_empty_english() {
        let messages = [
            OpenError::AlreadyOpen.to_string(),
            OpenError::Cancelled.to_string(),
            PortError::Closed.to_string(),
            SessionError::NotOpen.to_string(),
            SessionError::Unmappable { ch: '😀' }.to_string(),
            SessionError::Busy.to_string(),
            SessionError::TooLarge {
                bytes: 20_000_000,
                limit: 16 * 1024 * 1024,
            }
            .to_string(),
            SessionError::NoControlCode { ch: 'é' }.to_string(),
            LogError::Busy.to_string(),
            LogError::AlreadyRunning.to_string(),
            LogError::InvalidName("..".into()).to_string(),
            LogError::Io {
                message: "x".into(),
            }
            .to_string(),
        ];
        for message in messages {
            assert!(!message.is_empty());
            // The rejected character is quoted as typed; all else is ASCII.
            let text = message.replace(['😀', 'é'], "?");
            assert!(text.is_ascii(), "{message}");
        }
    }

    #[test]
    fn messages_point_at_tui_controls_not_gui_ones() {
        let text = OpenError::AlreadyOpen.to_string();
        assert!(text.contains("prefix, then d"), "{text}");
        for word in ["pane", "palette", "menu", "button"] {
            assert!(!text.contains(word), "{text}");
        }
    }
}
