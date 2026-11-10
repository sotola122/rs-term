use std::fmt;

/// Failure while opening a port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    Port(PortError),
    AlreadyOpen,
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
}

/// Failure of a log command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogError {
    NotRunning,
    Busy,
    InvalidName(String),
    Io { message: String },
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::AlreadyOpen => write!(f, "already open"),
            OpenError::Port(error) => write!(f, "{error}"),
        }
    }
}

impl fmt::Display for PortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PortError::Io { message } => write!(f, "{message}"),
            PortError::Unsupported(what) => write!(f, "unsupported: {what}"),
            PortError::Closed => write!(f, "closed"),
        }
    }
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::NotOpen => write!(f, "not open"),
            SessionError::Port(error) => write!(f, "{error}"),
            SessionError::Unmappable { ch } => write!(f, "unmappable U+{:04X}", *ch as u32),
            SessionError::Busy => write!(f, "busy"),
        }
    }
}

impl fmt::Display for LogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogError::NotRunning => write!(f, "log is not running"),
            LogError::Busy => write!(f, "log is busy"),
            LogError::InvalidName(name) => write!(f, "invalid log name: {name}"),
            LogError::Io { message } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for OpenError {}
impl std::error::Error for PortError {}
impl std::error::Error for SessionError {}
impl std::error::Error for LogError {}
