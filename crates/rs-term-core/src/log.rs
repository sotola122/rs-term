//! Received-data log.
//!
//! Binary is the raw chunk before decode. Text is the handler callback stream
//! after decode and receive-newline conversion. A write error moves the
//! session to `Failed` and does not roll back the screen.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};

use crate::error::LogError;
use crate::screen::TextAtom;

const FLUSH_EVERY: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogSession {
    Stopped,
    Running(LogTarget),
    Paused(LogTarget),
    Failed {
        target: LogTarget,
        message: String,
        last_flushed: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogTarget {
    Binary {
        path: PathBuf,
        append: bool,
        rotate: Rotate,
    },
    Text {
        path: PathBuf,
        append: bool,
        rotate: Rotate,
        encoding: LogEncoding,
        plain: bool,
        bom: bool,
        timestamp: Option<Timestamp>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogEncoding {
    Utf8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timestamp {
    LocalFixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rotate {
    pub max_bytes: u64,
    pub keep: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogCommand {
    Start(LogTarget),
    Pause,
    Resume,
    Stop,
}

pub(crate) struct LogEngine {
    state: LogSession,
    file: Option<File>,
    lock_path: Option<PathBuf>,
    buffer: Vec<u8>,
    written: u64,
    last_flushed: u64,
    at_line_start: bool,
}

impl Default for LogEngine {
    fn default() -> Self {
        Self {
            state: LogSession::Stopped,
            file: None,
            lock_path: None,
            buffer: Vec::new(),
            written: 0,
            last_flushed: 0,
            at_line_start: true,
        }
    }
}

impl Drop for LogEngine {
    fn drop(&mut self) {
        self.release_files();
    }
}

impl LogEngine {
    pub(crate) fn state(&self) -> &LogSession {
        &self.state
    }

    pub(crate) fn command(&mut self, command: LogCommand) -> Result<(), LogError> {
        match command {
            LogCommand::Start(target) => self.start(target),
            LogCommand::Pause => match &self.state {
                LogSession::Stopped => Err(LogError::NotRunning),
                LogSession::Running(target) => {
                    let target = target.clone();
                    self.state = LogSession::Paused(target);
                    Ok(())
                }
                LogSession::Paused(_) | LogSession::Failed { .. } => Ok(()),
            },
            LogCommand::Resume => match &self.state {
                LogSession::Stopped => Err(LogError::NotRunning),
                LogSession::Paused(target) => {
                    let target = target.clone();
                    self.state = LogSession::Running(target);
                    self.flush_if_full();
                    Ok(())
                }
                LogSession::Running(_) | LogSession::Failed { .. } => Ok(()),
            },
            LogCommand::Stop => {
                let _ = self.flush();
                self.release_files();
                self.state = LogSession::Stopped;
                self.buffer.clear();
                self.written = 0;
                self.at_line_start = true;
                Ok(())
            }
        }
    }

    pub(crate) fn record_binary(&mut self, bytes: &[u8]) {
        if !matches!(self.state, LogSession::Running(LogTarget::Binary { .. })) {
            return;
        }
        self.push(bytes);
    }

    pub(crate) fn record_text(&mut self, atoms: &[TextAtom]) {
        let (plain, timestamp) = match &self.state {
            LogSession::Running(LogTarget::Text { plain, timestamp, .. }) => (*plain, *timestamp),
            _ => return,
        };
        let mut encoded = Vec::new();
        for atom in atoms {
            match atom {
                TextAtom::Char(ch) => {
                    if plain && is_dropped_plain(*ch) {
                        continue;
                    }
                    self.write_char(&mut encoded, *ch, timestamp);
                }
                TextAtom::Cr => {
                    encoded.push(b'\r');
                    self.at_line_start = true;
                }
                TextAtom::Lf => {
                    encoded.push(b'\n');
                    self.at_line_start = true;
                }
                TextAtom::Bs => {
                    if !plain {
                        self.write_char(&mut encoded, '\u{0008}', timestamp);
                    }
                }
                TextAtom::Tab => {
                    if !plain {
                        self.write_char(&mut encoded, '\t', timestamp);
                    }
                }
            }
        }
        self.push(&encoded);
    }

    fn write_char(&mut self, out: &mut Vec<u8>, ch: char, timestamp: Option<Timestamp>) {
        if self.at_line_start {
            if timestamp == Some(Timestamp::LocalFixed) {
                out.extend(local_stamp(Local::now()).as_bytes());
            }
            self.at_line_start = false;
        }
        let mut buf = [0u8; 4];
        out.extend(ch.encode_utf8(&mut buf).as_bytes());
    }

    fn start(&mut self, target: LogTarget) -> Result<(), LogError> {
        match &self.state {
            LogSession::Running(current) | LogSession::Paused(current) if same_target(current, &target) => {
                return Ok(());
            }
            LogSession::Running(_) | LogSession::Paused(_) | LogSession::Failed { .. } => {
                return Err(LogError::Busy);
            }
            LogSession::Stopped => {}
        }
        validate_log_name(&file_name(&target)?)?;
        let path = target_path(&target);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| LogError::Io {
                message: error.to_string(),
            })?;
        }
        let lock_path = lock_path_for(&path);
        let lock = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .map_err(|_| LogError::Busy)?;
        drop(lock);
        let append = target_append(&target);
        let mut options = OpenOptions::new();
        options.create(true).write(true);
        if append {
            options.append(true);
        } else {
            options.truncate(true);
        }
        let file = options.open(&path).map_err(|error| {
            let _ = fs::remove_file(&lock_path);
            LogError::Io {
                message: error.to_string(),
            }
        })?;
        self.written = if append {
            file.metadata().map(|meta| meta.len()).unwrap_or(0)
        } else {
            0
        };
        self.last_flushed = self.written;
        self.file = Some(file);
        self.lock_path = Some(lock_path);
        self.buffer.clear();
        self.at_line_start = true;
        self.state = LogSession::Running(target);
        Ok(())
    }

    fn push(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        if self.needs_rotate(bytes.len() as u64) {
            if let Err(error) = self.flush() {
                self.fail(error);
                return;
            }
            if let Err(error) = self.rotate() {
                self.fail(error);
                return;
            }
        }
        self.buffer.extend_from_slice(bytes);
        if matches!(self.state, LogSession::Running(_)) {
            self.flush_if_full();
        }
    }

    fn needs_rotate(&self, incoming: u64) -> bool {
        let max = match &self.state {
            LogSession::Running(target) | LogSession::Paused(target) => target_rotate(target).max_bytes,
            _ => return false,
        };
        self.written.saturating_add(self.buffer.len() as u64).saturating_add(incoming) > max
            && self.written > 0
    }

    fn flush_if_full(&mut self) {
        if self.buffer.len() >= FLUSH_EVERY {
            if let Err(error) = self.flush() {
                self.fail(error);
            }
        }
    }

    fn flush(&mut self) -> Result<(), String> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let Some(file) = self.file.as_mut() else {
            return Ok(());
        };
        file.write_all(&self.buffer).map_err(|error| error.to_string())?;
        let flushed = self.buffer.len() as u64;
        self.buffer.clear();
        self.written = self.written.saturating_add(flushed);
        self.last_flushed = self.written;
        file.flush().map_err(|error| error.to_string())?;
        Ok(())
    }

    fn rotate(&mut self) -> Result<(), String> {
        let (path, keep) = match &self.state {
            LogSession::Running(target) | LogSession::Paused(target) => {
                (target_path(target), target_rotate(target).keep)
            }
            _ => return Ok(()),
        };
        self.file.take();
        rotate_files(&path, keep)?;
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .map_err(|error| error.to_string())?;
        self.file = Some(file);
        self.written = 0;
        self.last_flushed = 0;
        Ok(())
    }

    fn fail(&mut self, message: String) {
        let target = match &self.state {
            LogSession::Running(target) | LogSession::Paused(target) => target.clone(),
            LogSession::Failed { target, .. } => target.clone(),
            LogSession::Stopped => return,
        };
        self.file.take();
        self.state = LogSession::Failed {
            target,
            message,
            last_flushed: self.last_flushed,
        };
    }

    fn release_files(&mut self) {
        self.file.take();
        if let Some(path) = self.lock_path.take() {
            let _ = fs::remove_file(path);
        }
    }
}

fn same_target(current: &LogTarget, next: &LogTarget) -> bool {
    matches!(
        (current, next),
        (LogTarget::Binary { path: a, .. }, LogTarget::Binary { path: b, .. })
        | (LogTarget::Text { path: a, .. }, LogTarget::Text { path: b, .. })
        if a == b
    )
}

fn target_path(target: &LogTarget) -> PathBuf {
    match target {
        LogTarget::Binary { path, .. } | LogTarget::Text { path, .. } => path.clone(),
    }
}

fn target_append(target: &LogTarget) -> bool {
    match target {
        LogTarget::Binary { append, .. } | LogTarget::Text { append, .. } => *append,
    }
}

fn target_rotate(target: &LogTarget) -> Rotate {
    match target {
        LogTarget::Binary { rotate, .. } | LogTarget::Text { rotate, .. } => *rotate,
    }
}

fn file_name(target: &LogTarget) -> Result<String, LogError> {
    target_path(target)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_string())
        .ok_or_else(|| LogError::InvalidName(target_path(target).display().to_string()))
}

pub fn validate_log_name(name: &str) -> Result<(), LogError> {
    if name.is_empty()
        || name.contains(['/', '\\', ':', '<', '>', '"', '|', '?', '*'])
        || name.contains("..")
        || name.ends_with(' ')
        || name.ends_with('.')
    {
        return Err(LogError::InvalidName(name.to_string()));
    }
    let stem = name.split('.').next().unwrap_or(name);
    let upper = stem.to_ascii_uppercase();
    const RESERVED: &[&str] = &["CON", "PRN", "AUX", "NUL"];
    if RESERVED.contains(&upper.as_str()) {
        return Err(LogError::InvalidName(name.to_string()));
    }
    if let Some(rest) = upper.strip_prefix("COM").or_else(|| upper.strip_prefix("LPT")) {
        if rest.len() == 1 && rest.as_bytes()[0].is_ascii_digit() && rest != "0" {
            return Err(LogError::InvalidName(name.to_string()));
        }
    }
    Ok(())
}

pub fn port_token(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_string()
}

pub fn expand_log_name(template: &str, port: &str, when: DateTime<Local>) -> String {
    template
        .replace("{port}", &port_token(port))
        .replace("{date}", &when.format("%Y%m%d").to_string())
        .replace("{time}", &when.format("%H%M%S").to_string())
}

fn local_stamp(when: DateTime<Local>) -> String {
    when.format("%Y-%m-%d %H:%M:%S ").to_string()
}

fn lock_path_for(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".lock");
    PathBuf::from(name)
}

fn rotate_files(path: &Path, keep: u32) -> Result<(), String> {
    if keep == 0 {
        return Err("rotate keep is 0".into());
    }
    let oldest = numbered(path, keep);
    let parked = numbered(path, keep + 1);
    if oldest.exists() {
        fs::rename(&oldest, &parked).map_err(|error| error.to_string())?;
    }
    for index in (1..=keep).rev() {
        let src = if index == 1 {
            path.to_path_buf()
        } else {
            numbered(path, index - 1)
        };
        let dst = numbered(path, index);
        if src.exists() {
            fs::rename(&src, &dst).map_err(|error| error.to_string())?;
        }
    }
    if parked.exists() {
        let _ = fs::remove_file(&parked);
    }
    Ok(())
}

fn numbered(path: &Path, index: u32) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".{index}"));
    PathBuf::from(name)
}

fn is_dropped_plain(ch: char) -> bool {
    ch.is_ascii() && (ch.is_ascii_control() || ch == '\u{7f}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_reject_traversal_and_reserved_devices() {
        assert!(validate_log_name("com10.log").is_ok());
        assert!(validate_log_name("COM10.log").is_ok());
        for bad in ["COM1.log", "aux.txt", "a/b", r"a\b", "..", "a..b", "ends.", "ends "] {
            assert!(validate_log_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn template_uses_the_port_leaf_and_no_colon() {
        let name = expand_log_name("{port}_{date}_{time}.log", r"\\.\COM10", Local::now());
        assert!(name.starts_with("COM10_"), "{name}");
        assert!(!name.contains(':'));
        assert_eq!(port_token("/dev/ttyUSB0"), "ttyUSB0");
    }

    #[test]
    fn pause_does_not_grow_the_file_and_stop_flushes() {
        let dir = std::env::temp_dir().join(format!("rs-term-log-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("run.log");
        let mut log = LogEngine::default();
        log.command(LogCommand::Start(LogTarget::Text {
            path: path.clone(),
            append: false,
            rotate: Rotate {
                max_bytes: 64 * 1024,
                keep: 2,
            },
            encoding: LogEncoding::Utf8,
            plain: false,
            bom: false,
            timestamp: None,
        }))
        .unwrap();
        log.record_text(&[TextAtom::Char('a'), TextAtom::Char('b')]);
        log.command(LogCommand::Pause).unwrap();
        let paused_len = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        log.record_text(&[TextAtom::Char('c')]);
        let still = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        assert_eq!(paused_len, still);
        log.command(LogCommand::Resume).unwrap();
        log.command(LogCommand::Stop).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text, "ab");
        assert!(!lock_path_for(&path).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn second_open_of_the_same_path_is_busy() {
        let dir = std::env::temp_dir().join(format!("rs-term-log-busy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("one.log");
        let target = LogTarget::Binary {
            path: path.clone(),
            append: false,
            rotate: Rotate {
                max_bytes: 64 * 1024,
                keep: 1,
            },
        };
        let mut first = LogEngine::default();
        first.command(LogCommand::Start(target.clone())).unwrap();
        let mut second = LogEngine::default();
        assert_eq!(
            second.command(LogCommand::Start(target)),
            Err(LogError::Busy)
        );
        drop(first);
        let _ = fs::remove_dir_all(&dir);
    }
}
