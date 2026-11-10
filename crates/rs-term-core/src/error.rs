//! Domain errors. Every `Display` is the Japanese text the pane status line
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
    NotRunning,
    /// Another pane or process holds the file.
    Busy,
    /// This pane already logs to a different file.
    AlreadyRunning,
    InvalidName(String),
    Io {
        message: String,
    },
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::AlreadyOpen => write!(f, "すでに接続している。先に「切断」"),
            OpenError::Cancelled => write!(f, "接続を取り消した"),
            OpenError::Port(error) => write!(f, "{error}"),
        }
    }
}

impl fmt::Display for PortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PortError::Io { message } => write!(f, "{message}"),
            PortError::Unsupported(what) => write!(f, "{what}"),
            PortError::Closed => write!(f, "ポートが閉じている"),
        }
    }
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::NotOpen => write!(f, "未接続なので送らなかった（Ctrl+Shift+K で接続）"),
            SessionError::Port(error) => write!(f, "{error}"),
            SessionError::Unmappable { ch } => write!(
                f,
                "「{ch}」(U+{:04X}) は今のエンコーディングで表せないので送らなかった",
                *ch as u32
            ),
            SessionError::Busy => write!(f, "送信の待ち行列が一杯なので送らなかった"),
            SessionError::TooLarge { bytes, limit } => write!(
                f,
                "{bytes} バイトは上限 {} MiB を超えるので送らなかった",
                limit / (1024 * 1024)
            ),
            SessionError::NoControlCode { ch } => {
                write!(f, "Ctrl+{ch} に対応する制御コードが無いので送らなかった")
            }
        }
    }
}

impl fmt::Display for LogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogError::NotRunning => write!(f, "ログは開始していない"),
            LogError::Busy => {
                write!(f, "そのログファイルは別のペインかアプリが使用中")
            }
            LogError::AlreadyRunning => {
                write!(f, "このペインは別のファイルへログ中。先に「ログ停止」")
            }
            LogError::InvalidName(reason) => write!(f, "ログのファイル名が使えない: {reason}"),
            LogError::Io { message } => write!(f, "ログの書き込みに失敗: {message}"),
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

    fn has_japanese(text: &str) -> bool {
        text.chars()
            .any(|ch| matches!(ch, '\u{3040}'..='\u{30FF}' | '\u{4E00}'..='\u{9FFF}'))
    }

    #[test]
    fn every_message_is_japanese() {
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
            LogError::NotRunning.to_string(),
            LogError::Busy.to_string(),
            LogError::AlreadyRunning.to_string(),
            LogError::InvalidName("..".into()).to_string(),
            LogError::Io {
                message: "x".into(),
            }
            .to_string(),
        ];
        for message in messages {
            assert!(has_japanese(&message), "{message}");
        }
    }
}
