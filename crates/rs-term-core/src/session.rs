//! One serial session and the terminal it feeds.
//!
//! `apply_rx` order is fixed: binary log, decode, receive newline, both
//! processors, text log, then raw `PtyWrite` replies. The keyboard and paste
//! enter only through `apply_input`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use futures_channel::mpsc::{Receiver, TryRecvError};

use crate::codec::{self, Newline, PastePiece, SessionEncoding};
use crate::error::{LogError, OpenError, PortError, SessionError};
use crate::log::{LogCommand, LogEngine, LogSession};
use crate::screen::{Screen, ScreenSnapshot};
use crate::serial::{self, Command, PinState, PortIo, SerialParams, UsbIdentity, WorkerHandle};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Key(KeyInput),
    Text(String),
    Bytes(Vec<u8>),
    Break { hold: Duration },
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyInput {
    Enter,
    Backspace,
    Delete,
    Tab,
    Escape,
    Char(char),
    Ctrl(char),
    Alt(char),
    Arrow(Direction),
    Home,
    End,
    Function(u8),
}

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub screen: ScreenConfig,
    pub encoding: SessionEncoding,
    pub newline: Newline,
    pub local_echo: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            screen: ScreenConfig {
                columns: 80,
                rows: 24,
                scrollback: 10_000,
            },
            encoding: SessionEncoding::Utf8,
            newline: Newline::default(),
            local_echo: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenConfig {
    pub columns: usize,
    pub rows: usize,
    pub scrollback: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Disconnected,
    Opening { params: SerialParams },
    Open { params: SerialParams },
    Lost { params: SerialParams, error: PortError },
}

pub struct EventRx {
    rx: Receiver<IoEvent>,
}

impl EventRx {
    pub async fn recv(&mut self) -> Option<IoEvent> {
        self.rx.recv().await.ok()
    }

    pub fn try_recv(&mut self) -> Result<IoEvent, TryRecvError> {
        self.rx.try_recv()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IoEvent {
    Data(Vec<u8>),
    PortLost(PortError),
    CommandFailed(PortError),
    Pins(PinState),
    Dropped { bytes: u64 },
}

pub struct Session {
    screen: Screen,
    encoding: SessionEncoding,
    newline: Newline,
    local_echo: bool,
    link: Link,
    params: SerialParams,
    worker: Option<WorkerHandle>,
    generation: Arc<AtomicU64>,
    log: LogEngine,
    note: String,
    pins: Option<PinState>,
    usb: Option<UsbIdentity>,
    reconnect: bool,
}

impl Session {
    pub fn new(config: SessionConfig) -> Self {
        let screen = Screen::new(
            config.screen.columns,
            config.screen.rows,
            config.screen.scrollback,
            config.encoding,
            config.newline,
        );
        Self {
            screen,
            encoding: config.encoding,
            newline: config.newline,
            local_echo: config.local_echo,
            link: Link::Disconnected,
            params: SerialParams::default(),
            worker: None,
            generation: Arc::new(AtomicU64::new(1)),
            log: LogEngine::default(),
            note: String::new(),
            pins: None,
            usb: None,
            reconnect: false,
        }
    }

    pub fn connect(&mut self, serial: SerialParams) -> Result<EventRx, OpenError> {
        self.connect_with(serial, serial::open_port)
    }

    pub(crate) fn connect_with(
        &mut self,
        serial: SerialParams,
        open: impl FnOnce(&SerialParams) -> Result<Box<dyn PortIo>, PortError>,
    ) -> Result<EventRx, OpenError> {
        if matches!(self.link, Link::Opening { .. } | Link::Open { .. }) {
            return Err(OpenError::AlreadyOpen);
        }
        self.shutdown_worker();
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        self.link = Link::Opening {
            params: serial.clone(),
        };
        match open(&serial) {
            Ok(port) => {
                let (worker, rx) = serial::spawn(port, generation);
                self.worker = Some(worker);
                self.params = serial.clone();
                self.link = Link::Open { params: serial };
                self.pins = None;
                Ok(EventRx { rx })
            }
            Err(error) => {
                self.link = Link::Disconnected;
                self.note = error.to_string();
                Err(OpenError::Port(error))
            }
        }
    }

    pub fn disconnect(&mut self) {
        let _ = self.screen.finish_codec();
        self.shutdown_worker();
        self.link = Link::Disconnected;
        self.pins = None;
    }

    pub fn close(&mut self) {
        self.disconnect();
    }

    pub fn link(&self) -> &Link {
        &self.link
    }

    pub fn params(&self) -> &SerialParams {
        &self.params
    }

    pub fn encoding(&self) -> SessionEncoding {
        self.encoding
    }

    pub fn newline(&self) -> Newline {
        self.newline
    }

    pub fn local_echo(&self) -> bool {
        self.local_echo
    }

    pub fn note(&self) -> &str {
        &self.note
    }

    pub fn pins(&self) -> Option<PinState> {
        self.pins
    }

    pub fn set_usb_identity(&mut self, identity: Option<UsbIdentity>) {
        self.usb = identity;
    }

    pub fn usb_identity(&self) -> Option<&UsbIdentity> {
        self.usb.as_ref()
    }

    pub fn set_reconnect_enabled(&mut self, enabled: bool) {
        self.reconnect = enabled;
    }

    pub fn reconnect_enabled(&self) -> bool {
        self.reconnect
    }

    /// Reconnect stays off unless the setting is on and vid, pid, and serial
    /// all match. The UI does not expose the setting.
    pub fn reconnect_matches(&self, current: Option<&UsbIdentity>) -> bool {
        match (self.reconnect, self.usb.as_ref(), current) {
            (true, Some(previous), Some(current)) => serial::usb_identity_matches(previous, current),
            _ => false,
        }
    }

    pub fn apply_rx(&mut self, bytes: &[u8]) {
        self.log.record_binary(bytes);
        let outcome = self.screen.apply_bytes(bytes);
        self.log.record_text(&outcome.text);
        for note in outcome.notes {
            self.note = note;
        }
        for reply in outcome.replies {
            if let Err(error) = self.queue_bytes(reply) {
                self.note = error.to_string();
            }
        }
    }

    pub fn apply_input(&mut self, input: Input) -> Result<(), SessionError> {
        match input {
            Input::Key(KeyInput::Char(ch)) => {
                let text = ch.to_string();
                let bytes = codec::encode(self.encoding, &text)
                    .map_err(|ch| SessionError::Unmappable { ch })?;
                self.queue_encoded(&text, &bytes)
            }
            Input::Key(KeyInput::Alt(ch)) => {
                let encoded = codec::encode(self.encoding, &ch.to_string())
                    .map_err(|ch| SessionError::Unmappable { ch })?;
                let mut bytes = vec![0x1B];
                bytes.extend(encoded);
                self.queue_bytes(bytes)
            }
            Input::Key(KeyInput::Enter) => {
                let bytes = codec::transmit_newline(self.newline.transmit).to_vec();
                self.queue_bytes(bytes)?;
                if self.local_echo {
                    self.screen.echo_newline();
                }
                Ok(())
            }
            Input::Key(key) => {
                let bytes = key_bytes(&key, self.screen.app_cursor())?;
                self.queue_bytes(bytes)
            }
            Input::Text(text) => self.send_text(&text),
            Input::Bytes(bytes) => self.queue_bytes(bytes),
            Input::Break { hold } => self.send_command(Command::Break { hold }),
            Input::Cancel => {
                let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
                self.send_command(Command::Cancel { generation })
            }
        }
    }

    pub fn note_port_lost(&mut self, error: PortError) {
        if let Link::Open { params } = &self.link {
            self.link = Link::Lost {
                params: params.clone(),
                error: error.clone(),
            };
            self.note = error.to_string();
        }
    }

    pub fn note_command_failed(&mut self, error: PortError) {
        self.note = error.to_string();
    }

    pub fn note_pins(&mut self, pins: PinState) {
        self.pins = Some(pins);
    }

    pub fn note_dropped(&mut self, bytes: u64) {
        self.note = format!("受信を {bytes} バイト落とした");
    }

    /// Stop the port thread and keep `Link::Lost`. `disconnect` is the
    /// transition back to `Disconnected`.
    pub fn release_port(&mut self) {
        self.shutdown_worker();
    }

    pub fn resize(&mut self, columns: usize, rows: usize) {
        self.screen.resize(columns, rows);
    }

    pub fn screen_content(&self) -> ScreenSnapshot {
        self.screen.snapshot()
    }

    pub fn set_encoding(&mut self, encoding: SessionEncoding) {
        let outcome = self.screen.set_encoding(encoding);
        self.encoding = encoding;
        self.log.record_text(&outcome.text);
        for note in outcome.notes {
            self.note = note;
        }
    }

    pub fn set_newline(&mut self, newline: Newline) {
        self.screen.set_newline(newline);
        self.newline = newline;
    }

    pub fn set_local_echo(&mut self, on: bool) {
        self.local_echo = on;
    }

    pub fn set_serial_params(&mut self, params: SerialParams) -> Result<(), SessionError> {
        if matches!(self.link, Link::Open { .. } | Link::Opening { .. })
            && params.path != self.params.path
        {
            return Err(SessionError::Port(PortError::Unsupported(
                "path の変更は切断してから",
            )));
        }
        if matches!(self.link, Link::Open { .. }) {
            self.send_command(Command::Apply(params.clone()))?;
            self.params = params.clone();
            self.link = Link::Open { params };
            Ok(())
        } else if matches!(self.link, Link::Opening { .. }) {
            Err(SessionError::Busy)
        } else {
            self.params = params;
            Ok(())
        }
    }

    pub fn set_dtr(&mut self, on: bool) -> Result<(), SessionError> {
        self.params.dtr_on_open = if on { Some(true) } else { None };
        if matches!(self.link, Link::Open { .. }) {
            self.send_command(Command::Dtr(on))?;
        }
        Ok(())
    }

    pub fn set_rts(&mut self, on: bool) -> Result<(), SessionError> {
        if self.params.flow == serial::Flow::RtsCts {
            return Err(SessionError::Port(PortError::Unsupported(
                "CTS による送信制御。受信の自動 RTS はしない",
            )));
        }
        self.params.rts = on;
        if matches!(self.link, Link::Open { .. }) {
            self.send_command(Command::Rts(on))?;
        }
        Ok(())
    }

    pub fn log(&mut self, command: LogCommand) -> Result<(), LogError> {
        self.log.command(command)
    }

    pub fn log_state(&self) -> &LogSession {
        self.log.state()
    }

    pub fn scroll_lines(&mut self, delta: i32) {
        self.screen.scroll_lines(delta);
    }

    pub fn scroll_page(&mut self, up: bool) {
        self.screen.scroll_page(up);
    }

    pub fn scroll_top(&mut self) {
        self.screen.scroll_top();
    }

    pub fn scroll_bottom(&mut self) {
        self.screen.scroll_bottom();
    }

    pub fn select_clear(&mut self) {
        self.screen.select_clear();
    }

    pub fn select_start(&mut self, line: i32, column: usize) {
        self.screen.select_start(line, column);
    }

    pub fn select_update(&mut self, line: i32, column: usize) {
        self.screen.select_update(line, column);
    }

    pub fn selected_text(&self) -> Option<String> {
        self.screen.selected_text()
    }

    fn send_text(&mut self, text: &str) -> Result<(), SessionError> {
        let mut chunks: Vec<Vec<u8>> = Vec::new();
        let mut echo: Vec<Echo> = Vec::new();
        if self.screen.bracketed_paste() {
            chunks.push(b"\x1b[200~".to_vec());
        }
        for piece in codec::split_paste(text) {
            match piece {
                PastePiece::Text(part) => {
                    let bytes = codec::encode(self.encoding, &part).map_err(|ch| SessionError::Unmappable { ch })?;
                    echo.push(Echo::Text(part));
                    push_chunks(&mut chunks, bytes);
                }
                PastePiece::Newline => {
                    echo.push(Echo::Newline);
                    push_chunks(&mut chunks, codec::transmit_newline(self.newline.transmit).to_vec());
                }
            }
        }
        if self.screen.bracketed_paste() {
            chunks.push(b"\x1b[201~".to_vec());
        }
        self.queue_chunks(chunks)?;
        if self.local_echo {
            for item in echo {
                match item {
                    Echo::Text(text) => self.screen.echo_text(&text),
                    Echo::Newline => self.screen.echo_newline(),
                }
            }
        }
        Ok(())
    }

    fn queue_encoded(&mut self, text: &str, bytes: &[u8]) -> Result<(), SessionError> {
        self.queue_bytes(bytes.to_vec())?;
        if self.local_echo {
            self.screen.echo_text(text);
        }
        Ok(())
    }

    fn queue_bytes(&mut self, bytes: Vec<u8>) -> Result<(), SessionError> {
        let mut chunks = Vec::new();
        push_chunks(&mut chunks, bytes);
        self.queue_chunks(chunks)
    }

    fn queue_chunks(&mut self, chunks: Vec<Vec<u8>>) -> Result<(), SessionError> {
        if !matches!(self.link, Link::Open { .. }) {
            return Err(SessionError::NotOpen);
        }
        let generation = self.generation.load(Ordering::Relaxed);
        for bytes in chunks {
            self.send_command(Command::Write { generation, bytes })?;
        }
        Ok(())
    }

    fn send_command(&mut self, command: Command) -> Result<(), SessionError> {
        let Some(worker) = self.worker.as_ref() else {
            return Err(SessionError::NotOpen);
        };
        worker.commands.try_send(command).map_err(|_| SessionError::Busy)
    }

    fn shutdown_worker(&mut self) {
        let Some(worker) = self.worker.take() else {
            return;
        };
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = worker.commands.try_send(Command::Cancel { generation });
        let _ = worker.commands.try_send(Command::Shutdown);
        let started = Instant::now();
        loop {
            if worker.join.is_finished() {
                let _ = worker.join.join();
                return;
            }
            if started.elapsed() >= Duration::from_secs(2) {
                self.note = "スレッドが残っている".into();
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
            let _ = worker.commands.try_send(Command::Cancel { generation });
            let _ = worker.commands.try_send(Command::Shutdown);
        }
    }
}

enum Echo {
    Text(String),
    Newline,
}

fn push_chunks(chunks: &mut Vec<Vec<u8>>, bytes: Vec<u8>) {
    if bytes.is_empty() {
        return;
    }
    for piece in bytes.chunks(4096) {
        chunks.push(piece.to_vec());
    }
}

fn key_bytes(key: &KeyInput, app_cursor: bool) -> Result<Vec<u8>, SessionError> {
    let bytes: &[u8] = match key {
        KeyInput::Enter | KeyInput::Char(_) | KeyInput::Alt(_) => b"",
        KeyInput::Backspace => &[0x08],
        KeyInput::Tab => &[0x09],
        KeyInput::Escape => &[0x1B],
        KeyInput::Delete => b"\x1b[3~",
        KeyInput::Home => b"\x1b[H",
        KeyInput::End => b"\x1b[F",
        KeyInput::Arrow(direction) => arrow_bytes(*direction, app_cursor),
        KeyInput::Function(1) => b"\x1bOP",
        KeyInput::Function(2) => b"\x1bOQ",
        KeyInput::Function(3) => b"\x1bOR",
        KeyInput::Function(4) => b"\x1bOS",
        KeyInput::Function(5) => b"\x1b[15~",
        KeyInput::Function(6) => b"\x1b[17~",
        KeyInput::Function(7) => b"\x1b[18~",
        KeyInput::Function(8) => b"\x1b[19~",
        KeyInput::Function(9) => b"\x1b[20~",
        KeyInput::Function(10) => b"\x1b[21~",
        KeyInput::Function(11) => b"\x1b[23~",
        KeyInput::Function(12) => b"\x1b[24~",
        KeyInput::Function(_) => b"",
        KeyInput::Ctrl(ch) => return Ok(vec![ctrl_byte(*ch)]),
    };
    Ok(bytes.to_vec())
}

fn ctrl_byte(ch: char) -> u8 {
    match ch.to_ascii_lowercase() {
        'c' => 0x03,
        'd' => 0x04,
        'z' => 0x1A,
        other if other.is_ascii_lowercase() => (other as u8) - b'a' + 1,
        other if other.is_ascii() => other as u8,
        _ => 0,
    }
}

fn arrow_bytes(direction: Direction, app_cursor: bool) -> &'static [u8] {
    match (app_cursor, direction) {
        (false, Direction::Up) => b"\x1b[A",
        (false, Direction::Down) => b"\x1b[B",
        (false, Direction::Right) => b"\x1b[C",
        (false, Direction::Left) => b"\x1b[D",
        (true, Direction::Up) => b"\x1bOA",
        (true, Direction::Down) => b"\x1bOB",
        (true, Direction::Right) => b"\x1bOC",
        (true, Direction::Left) => b"\x1bOD",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{ReceiveNewline, TransmitNewline};
    use crate::log::{LogEncoding, LogTarget, Rotate};
    use crate::serial::{DataBits, Flow, Parity, StopBits};
    use std::collections::VecDeque;
    use std::io;
    use std::sync::{Mutex, MutexGuard};

    struct SharedPort {
        inner: Arc<Mutex<PortState>>,
    }

    struct PortState {
        reads: VecDeque<io::Result<Vec<u8>>>,
        written: Vec<u8>,
        partial: Option<usize>,
        allow_write: bool,
        applied: Vec<SerialParams>,
        dtr: Vec<bool>,
        rts: Vec<bool>,
        break_on: bool,
        break_count: u32,
        clear_count: u32,
        opened_with: Option<SerialParams>,
        pins: PinState,
        fail_rts: bool,
    }

    impl PortIo for SharedPort {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let mut state = lock(&self.inner);
            match state.reads.pop_front() {
                Some(Ok(bytes)) => {
                    let count = bytes.len().min(buf.len());
                    buf[..count].copy_from_slice(&bytes[..count]);
                    Ok(count)
                }
                Some(Err(error)) => Err(error),
                None => Err(io::Error::new(io::ErrorKind::TimedOut, "idle")),
            }
        }

        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let mut state = lock(&self.inner);
            if !state.allow_write {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "write held"));
            }
            let count = state.partial.unwrap_or(buf.len()).min(buf.len());
            if count == 0 {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "write wait"));
            }
            state.written.extend_from_slice(&buf[..count]);
            Ok(count)
        }

        fn apply(&mut self, params: &SerialParams) -> Result<(), PortError> {
            lock(&self.inner).applied.push(params.clone());
            Ok(())
        }

        fn set_dtr(&mut self, on: bool) -> Result<(), PortError> {
            lock(&self.inner).dtr.push(on);
            Ok(())
        }

        fn set_rts(&mut self, on: bool) -> Result<(), PortError> {
            let mut state = lock(&self.inner);
            if state.fail_rts {
                return Err(PortError::Unsupported("rts"));
            }
            state.rts.push(on);
            Ok(())
        }

        fn set_break(&mut self, on: bool) -> Result<(), PortError> {
            let mut state = lock(&self.inner);
            state.break_on = on;
            if on {
                state.break_count += 1;
            } else {
                state.clear_count += 1;
            }
            Ok(())
        }

        fn pins(&mut self) -> Result<PinState, PortError> {
            Ok(lock(&self.inner).pins)
        }
    }

    fn lock(inner: &Arc<Mutex<PortState>>) -> MutexGuard<'_, PortState> {
        inner.lock().unwrap_or_else(|error| error.into_inner())
    }

    type SharedOpen = Box<dyn FnOnce(&SerialParams) -> Result<Box<dyn PortIo>, PortError>>;

    fn open_shared(state: PortState) -> (Arc<Mutex<PortState>>, SharedOpen) {
        let inner = Arc::new(Mutex::new(state));
        let opened = Arc::clone(&inner);
        let open = move |params: &SerialParams| {
            opened.lock().unwrap().opened_with = Some(params.clone());
            let port: Box<dyn PortIo> = Box::new(SharedPort {
                inner: Arc::clone(&opened),
            });
            Ok(port)
        };
        (inner, Box::new(open))
    }

    fn session() -> Session {
        Session::new(SessionConfig {
            newline: Newline {
                receive: ReceiveNewline::Cr,
                transmit: TransmitNewline::Cr,
            },
            ..SessionConfig::default()
        })
    }

    #[test]
    fn hello_is_visible_without_a_port() {
        let mut session = session();
        session.apply_rx(b"hello\r\n");
        let row = visible(&session);
        assert_eq!(row, "hello");
    }

    #[test]
    fn timeout_is_not_disconnect_and_other_errors_are() {
        let (state, open) = open_shared(PortState {
            reads: VecDeque::from([
                Err(io::Error::new(io::ErrorKind::TimedOut, "quiet")),
                Ok(b"hi".to_vec()),
            ]),
            ..empty_state()
        });
        let mut session = session();
        let mut rx = session
            .connect_with(SerialParams { path: "COM3".into(), ..SerialParams::default() }, open)
            .unwrap();
        let event = wait_data(&mut rx);
        assert!(matches!(event, Some(IoEvent::Data(bytes)) if bytes == b"hi"));
        session.apply_rx(b"hi");
        assert!(matches!(session.link(), Link::Open { .. }));
        let _ = state;
        session.disconnect();
        assert!(matches!(session.link(), Link::Disconnected));
        session.disconnect();
        assert!(matches!(session.link(), Link::Disconnected));
    }

    #[test]
    fn read_error_is_reported_as_port_lost() {
        let (_state, open) = open_shared(PortState {
            reads: VecDeque::from([Err(io::Error::new(io::ErrorKind::BrokenPipe, "gone"))]),
            ..empty_state()
        });
        let mut session = session();
        let mut rx = session
            .connect_with(SerialParams::default(), open)
            .unwrap();
        let event = wait_event(&mut rx);
        match event {
            Some(IoEvent::PortLost(error)) => session.note_port_lost(error),
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(session.link(), Link::Lost { .. }));
        let error = match session.connect(SerialParams::default()) {
            Err(error) => error,
            Ok(_) => panic!("opening an empty path should fail"),
        };
        // Lost is not Open, so a new connect is attempted.
        assert!(!matches!(error, OpenError::AlreadyOpen));
    }

    #[test]
    fn connect_while_open_is_already_open() {
        let (_state, open) = open_shared(empty_state());
        let mut session = session();
        let _rx = session.connect_with(SerialParams::default(), open).unwrap();
        let (_state, open) = open_shared(empty_state());
        let error = match session.connect_with(SerialParams::default(), open) {
            Err(error) => error,
            Ok(_) => panic!("expected AlreadyOpen"),
        };
        assert_eq!(error, OpenError::AlreadyOpen);
    }

    #[test]
    fn seven_even_one_is_recorded_and_partial_writes_resume() {
        let (state, open) = open_shared(PortState {
            partial: Some(1),
            ..empty_state()
        });
        let mut session = session();
        let params = SerialParams {
            path: "COM4".into(),
            data_bits: DataBits::Seven,
            parity: Parity::Even,
            stop_bits: StopBits::One,
            ..SerialParams::default()
        };
        let _rx = session.connect_with(params.clone(), open).unwrap();
        let opened = lock(&state).opened_with.clone().unwrap();
        assert_eq!(opened.data_bits, DataBits::Seven);
        assert_eq!(opened.parity, Parity::Even);
        assert_eq!(opened.stop_bits, StopBits::One);
        assert_eq!(opened.read_timeout, Duration::from_millis(20));
        assert_eq!(opened.dtr_on_open, None);
        session.apply_input(Input::Bytes(vec![1, 2, 3])).unwrap();
        let written = wait_written(&state, 3);
        assert_eq!(written, vec![1, 2, 3]);
    }

    #[test]
    fn dtr_on_is_some_true_and_rtscts_rejects_manual_rts() {
        let (state, open) = open_shared(empty_state());
        let mut session = session();
        let params = SerialParams {
            dtr_on_open: Some(true),
            flow: Flow::RtsCts,
            rts: true,
            ..SerialParams::default()
        };
        let _rx = session.connect_with(params, open).unwrap();
        assert_eq!(lock(&state).opened_with.as_ref().unwrap().dtr_on_open, Some(true));
        let error = session.set_rts(false).unwrap_err();
        assert!(matches!(error, SessionError::Port(PortError::Unsupported(_))));
        assert!(lock(&state).rts.is_empty());
    }

    #[test]
    fn commands_run_after_a_successful_read() {
        let (state, open) = open_shared(PortState {
            reads: VecDeque::from([Ok(b"x".to_vec())]),
            ..empty_state()
        });
        let mut session = session();
        let mut rx = session.connect_with(SerialParams::default(), open).unwrap();
        let _ = wait_data(&mut rx);
        let next = SerialParams {
            baud_rate: 9600,
            ..session.params().clone()
        };
        session.set_serial_params(next).unwrap();
        let applied = wait_applied(&state);
        assert_eq!(applied.baud_rate, 9600);
    }

    #[test]
    fn break_clears_without_a_sleep_inside_the_port() {
        let (state, open) = open_shared(empty_state());
        let mut session = session();
        let _rx = session.connect_with(SerialParams::default(), open).unwrap();
        session
            .apply_input(Input::Break { hold: Duration::from_millis(30) })
            .unwrap();
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            let guard = lock(&state);
            if guard.break_count >= 1 && guard.clear_count >= 1 {
                assert!(!guard.break_on);
                return;
            }
            drop(guard);
            thread::sleep(Duration::from_millis(5));
        }
        panic!("break was not cleared");
    }

    #[test]
    fn keys_match_the_table() {
        let (state, open) = open_shared(empty_state());
        let mut session = session();
        let _rx = session.connect_with(SerialParams::default(), open).unwrap();
        session.apply_input(Input::Key(KeyInput::Ctrl('c'))).unwrap();
        session.apply_input(Input::Key(KeyInput::Ctrl('D'))).unwrap();
        session.apply_input(Input::Key(KeyInput::Ctrl('z'))).unwrap();
        session.apply_input(Input::Key(KeyInput::Backspace)).unwrap();
        session.apply_input(Input::Key(KeyInput::Delete)).unwrap();
        session.apply_input(Input::Key(KeyInput::Home)).unwrap();
        session.apply_input(Input::Key(KeyInput::End)).unwrap();
        session.apply_input(Input::Key(KeyInput::Function(1))).unwrap();
        session.apply_input(Input::Key(KeyInput::Arrow(Direction::Up))).unwrap();
        let written = wait_written(&state, 20);
        assert!(written.windows(1).any(|w| w == [0x03]));
        assert!(written.windows(1).any(|w| w == [0x04]));
        assert!(written.windows(1).any(|w| w == [0x1A]));
        assert!(written.windows(1).any(|w| w == [0x08]));
        assert!(written.windows(4).any(|w| w == *b"\x1b[3~"));
        assert!(written.windows(3).any(|w| w == *b"\x1b[H" || w == *b"\x1b[F"));
        assert!(written.windows(3).any(|w| w == *b"\x1bOP"));
        assert!(written.windows(3).any(|w| w == *b"\x1b[A"));
    }

    #[test]
    fn cancel_drops_bytes_not_yet_written() {
        let (state, open) = open_shared(PortState {
            partial: Some(1),
            allow_write: false,
            reads: VecDeque::from([Err(io::Error::new(io::ErrorKind::TimedOut, "hold"))]),
            ..empty_state()
        });
        let mut session = session();
        let _rx = session.connect_with(SerialParams::default(), open).unwrap();
        session.apply_input(Input::Bytes(vec![9, 9, 9, 9])).unwrap();
        thread::sleep(Duration::from_millis(30));
        session.apply_input(Input::Cancel).unwrap();
        thread::sleep(Duration::from_millis(30));
        lock(&state).allow_write = true;
        thread::sleep(Duration::from_millis(40));
        let written = lock(&state).written.clone();
        assert!(written.is_empty(), "{written:?}");
    }

    #[test]
    fn text_log_follows_handler_order_and_pause_discards() {
        let dir = std::env::temp_dir().join(format!("rs-term-session-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("text.log");
        let mut session = session();
        session
            .log(LogCommand::Start(LogTarget::Text {
                path: path.clone(),
                append: false,
                rotate: Rotate { max_bytes: 64 * 1024, keep: 2 },
                encoding: LogEncoding::Utf8,
                plain: true,
                bom: false,
                timestamp: None,
            }))
            .unwrap();
        session.apply_rx(b"foo\rbar\x1b[31mZ");
        session.log(LogCommand::Pause).unwrap();
        session.apply_rx(b"NO");
        session.log(LogCommand::Stop).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("foo\rbar"), "{text}");
        assert!(!text.contains("NO"), "{text}");
        assert!(!text.contains('\u{1b}'), "{text}");
        assert!(text.contains('Z'), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn empty_state() -> PortState {
        PortState {
            reads: VecDeque::new(),
            written: Vec::new(),
            partial: None,
            allow_write: true,
            applied: Vec::new(),
            dtr: Vec::new(),
            rts: Vec::new(),
            break_on: false,
            break_count: 0,
            clear_count: 0,
            opened_with: None,
            pins: PinState { cts: false, dsr: false, cd: false, ri: false },
            fail_rts: false,
        }
    }

    fn visible(session: &Session) -> String {
        let snap = session.screen_content();
        let mut row = String::new();
        for cell in snap.cells.iter().take(snap.columns) {
            if cell.width > 0 {
                row.push_str(&cell.text);
            }
        }
        row.trim_end().to_string()
    }

    fn wait_data(rx: &mut EventRx) -> Option<IoEvent> {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            match rx.try_recv() {
                Ok(event @ IoEvent::Data(_)) => return Some(event),
                Ok(IoEvent::Pins(_)) | Ok(IoEvent::Dropped { .. }) => {}
                Ok(other) => return Some(other),
                Err(TryRecvError::Empty) => thread::sleep(Duration::from_millis(5)),
                Err(TryRecvError::Closed) => return None,
            }
        }
        None
    }

    fn wait_event(rx: &mut EventRx) -> Option<IoEvent> {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            match rx.try_recv() {
                Ok(IoEvent::Pins(_)) => {}
                Ok(event) => return Some(event),
                Err(TryRecvError::Empty) => thread::sleep(Duration::from_millis(5)),
                Err(TryRecvError::Closed) => return None,
            }
        }
        None
    }

    fn wait_written(state: &Arc<Mutex<PortState>>, min: usize) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let written = lock(state).written.clone();
            if written.len() >= min {
                return written;
            }
            thread::sleep(Duration::from_millis(5));
        }
        lock(state).written.clone()
    }

    fn wait_applied(state: &Arc<Mutex<PortState>>) -> SerialParams {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Some(params) = lock(state).applied.last().cloned() {
                return params;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("apply was not called");
    }
}
