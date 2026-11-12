//! One serial session and the terminal it feeds.
//!
//! `apply_rx` order is fixed: binary log, decode, receive newline, one
//! processor whose handler also feeds the text log, then raw `PtyWrite`
//! replies. The keyboard and paste enter only through `apply_input`.
//!
//! Opening a port is split in three so the blocking `open` can run off the
//! UI thread: `begin_connect` (UI thread), `PendingOpen::open` (any thread),
//! `finish_connect` (UI thread). Closing never waits on the UI thread: the
//! command sender is dropped and the thread is reaped by `tick`. Only
//! `close` (and `Drop`) wait, and only for `CLOSE_WAIT`, so the port is
//! really released before the process exits.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::Poll;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use futures_channel::mpsc::{Receiver, UnboundedReceiver};

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
    Opening {
        params: SerialParams,
    },
    Open {
        params: SerialParams,
    },
    Lost {
        params: SerialParams,
        error: PortError,
    },
}

/// Events of one open port. Status events (loss, failures, pins, dropped
/// count) come first and never wait behind a full data channel.
pub struct EventRx {
    data: Receiver<IoEvent>,
    status: UnboundedReceiver<IoEvent>,
    status_closed: bool,
    data_closed: bool,
}

impl EventRx {
    fn new(channels: serial::Channels) -> Self {
        Self {
            data: channels.data,
            status: channels.status,
            status_closed: false,
            data_closed: false,
        }
    }

    pub async fn recv(&mut self) -> Option<IoEvent> {
        std::future::poll_fn(|cx| {
            if !self.status_closed {
                match Pin::new(&mut self.status.recv()).poll(cx) {
                    Poll::Ready(Ok(event)) => return Poll::Ready(Some(event)),
                    Poll::Ready(Err(_)) => self.status_closed = true,
                    Poll::Pending => {}
                }
            }
            if !self.data_closed {
                match Pin::new(&mut self.data.recv()).poll(cx) {
                    Poll::Ready(Ok(event)) => return Poll::Ready(Some(event)),
                    Poll::Ready(Err(_)) => self.data_closed = true,
                    Poll::Pending => {}
                }
            }
            if self.status_closed && self.data_closed {
                Poll::Ready(None)
            } else {
                Poll::Pending
            }
        })
        .await
    }

    /// The next event without waiting. `None` means nothing is queued now.
    pub fn try_recv(&mut self) -> Option<IoEvent> {
        if let Ok(event) = self.status.try_recv() {
            return Some(event);
        }
        self.data.try_recv().ok()
    }

    /// Both channels are closed and empty.
    pub fn is_closed(&mut self) -> bool {
        if !self.status_closed {
            if let Err(error) = self.status.try_recv().map(|_| ()) {
                self.status_closed = error.is_closed();
            }
        }
        if !self.data_closed {
            if let Err(error) = self.data.try_recv().map(|_| ()) {
                self.data_closed = error.is_closed();
            }
        }
        self.status_closed && self.data_closed
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IoEvent {
    Data(Vec<u8>),
    PortLost(PortError),
    CommandFailed(PortError),
    /// The port refused a parameter change. The session reverts its
    /// parameters to the values that were in effect before.
    ApplyFailed(PortError),
    Pins(PinState),
    Dropped {
        bytes: u64,
    },
    /// The thread queued these bytes for the port but will never send them
    /// (a write error, a cancel, or the pending limit). The session takes
    /// them out of the TX count.
    TxDropped {
        bytes: u64,
    },
}

/// A port open that has been started on the UI thread. `open` blocks and
/// may run on any thread.
pub struct PendingOpen {
    params: SerialParams,
    generation: u64,
    previous: Vec<JoinHandle<()>>,
    opener: Opener,
}

type Opener = Box<dyn FnOnce(&SerialParams) -> Result<Box<dyn PortIo>, PortError> + Send>;

/// The result of `PendingOpen::open`, to be handed to `finish_connect`.
pub struct OpenedPort {
    params: SerialParams,
    generation: u64,
    result: Result<Box<dyn PortIo>, PortError>,
}

impl PendingOpen {
    pub fn params(&self) -> &SerialParams {
        &self.params
    }

    /// Blocking. Waits up to two seconds for a previous thread of the previous
    /// connection to release the port, then opens.
    pub fn open(self) -> OpenedPort {
        join_bounded(self.previous, SHUTDOWN_WAIT);
        let result = (self.opener)(&self.params);
        OpenedPort {
            params: self.params,
            generation: self.generation,
            result,
        }
    }
}

const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);
/// How long `close` waits for port threads. The worker wakes at least every
/// read timeout, so a healthy one ends in a few tens of milliseconds.
const CLOSE_WAIT: Duration = Duration::from_secs(1);

/// Join the threads that finish within `wait` in total. A thread still
/// running after that is given up (detached), never waited for. Returns how
/// many were given up.
fn join_bounded(handles: Vec<JoinHandle<()>>, wait: Duration) -> usize {
    let deadline = Instant::now() + wait;
    let mut given_up = 0;
    for handle in handles {
        while !handle.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        if handle.is_finished() {
            let _ = handle.join();
        } else {
            given_up += 1;
        }
    }
    given_up
}

/// How a status-line message is drawn (design §5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Plain text.
    Info,
    /// Yellow.
    Warn,
    /// Red. Stays until the next message or until its cause is resolved.
    Error,
}

/// One message for the status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub severity: Severity,
    pub text: String,
}

impl Message {
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            text: text.into(),
        }
    }

    pub fn warn(text: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warn,
            text: text.into(),
        }
    }

    pub fn info(text: impl Into<String>) -> Self {
        Self {
            severity: Severity::Info,
            text: text.into(),
        }
    }
}

pub struct Session {
    screen: Screen,
    encoding: SessionEncoding,
    newline: Newline,
    local_echo: bool,
    link: Link,
    params: SerialParams,
    worker: Option<WorkerHandle>,
    reaping: Vec<(JoinHandle<()>, Instant)>,
    reap_warned: bool,
    applied: Option<SerialParams>,
    generation: Arc<AtomicU64>,
    log: LogEngine,
    note: Option<Message>,
    pins: Option<PinState>,
    usb: Option<UsbIdentity>,
    reconnect: bool,
    rx_bytes: u64,
    tx_bytes: u64,
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
            reaping: Vec::new(),
            reap_warned: false,
            applied: None,
            generation: Arc::new(AtomicU64::new(1)),
            log: LogEngine::default(),
            note: None,
            pins: None,
            usb: None,
            reconnect: false,
            rx_bytes: 0,
            tx_bytes: 0,
        }
    }

    pub fn rx_bytes(&self) -> u64 {
        self.rx_bytes
    }

    pub fn tx_bytes(&self) -> u64 {
        self.tx_bytes
    }

    /// Open on the calling thread. The UI uses `begin_connect` instead.
    pub fn connect(&mut self, serial: SerialParams) -> Result<EventRx, OpenError> {
        let pending = self.begin_connect(serial)?;
        let opened = pending.open();
        self.finish_connect(opened)
    }

    /// Move to `Opening` and return the blocking part.
    pub fn begin_connect(&mut self, serial: SerialParams) -> Result<PendingOpen, OpenError> {
        self.begin_connect_with(serial, Box::new(serial::open_port))
    }

    pub(crate) fn begin_connect_with(
        &mut self,
        serial: SerialParams,
        opener: Opener,
    ) -> Result<PendingOpen, OpenError> {
        if matches!(self.link, Link::Opening { .. } | Link::Open { .. }) {
            return Err(OpenError::AlreadyOpen);
        }
        self.stop_worker();
        self.reset_stream();
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        self.link = Link::Opening {
            params: serial.clone(),
        };
        self.params = serial.clone();
        let previous = self.reaping.drain(..).map(|(handle, _)| handle).collect();
        self.reap_warned = false;
        Ok(PendingOpen {
            params: serial,
            generation,
            previous,
            opener,
        })
    }

    /// Finish an open on the UI thread. An open that was overtaken by a
    /// disconnect or a newer connect is closed and reported as cancelled.
    pub fn finish_connect(&mut self, opened: OpenedPort) -> Result<EventRx, OpenError> {
        let current = self.generation.load(Ordering::Relaxed);
        if opened.generation != current || !matches!(self.link, Link::Opening { .. }) {
            return Err(OpenError::Cancelled);
        }
        match opened.result {
            Ok(port) => {
                let (worker, channels) = serial::spawn(port, current);
                self.worker = Some(worker);
                // A line setting changed while the port was opening (the
                // baud dialog) waits in `self.params`; the port was opened
                // with the older values, so apply the wanted ones now.
                let mut effective = opened.params.clone();
                let wanted = &self.params;
                if wanted.path == effective.path && !same_line(wanted, &effective) {
                    effective.baud_rate = wanted.baud_rate;
                    effective.data_bits = wanted.data_bits;
                    effective.parity = wanted.parity;
                    effective.stop_bits = wanted.stop_bits;
                    effective.flow = wanted.flow;
                    let _ = self.send_command(Command::Apply(effective.clone()));
                }
                self.params = effective.clone();
                self.applied = Some(opened.params);
                self.link = Link::Open { params: effective };
                self.pins = None;
                self.note = None;
                Ok(EventRx::new(channels))
            }
            Err(error) => {
                self.link = Link::Disconnected;
                let error = OpenError::Port(error);
                self.note = Some(Message::error(format!(
                    "{} を開けない: {error}",
                    opened.params.path
                )));
                Err(error)
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn connect_with(
        &mut self,
        serial: SerialParams,
        open: impl FnOnce(&SerialParams) -> Result<Box<dyn PortIo>, PortError> + Send + 'static,
    ) -> Result<EventRx, OpenError> {
        let pending = self.begin_connect_with(serial, Box::new(open))?;
        let opened = pending.open();
        self.finish_connect(opened)
    }

    /// Close the port. The receive stream is finished: an incomplete
    /// Shift_JIS lead becomes U+FFFD, a held CR is completed, and the parser
    /// forgets any half sequence, so nothing leaks into the next connection.
    pub fn disconnect(&mut self) {
        if matches!(self.link, Link::Opening { .. }) {
            self.generation.fetch_add(1, Ordering::Relaxed);
        }
        self.stop_worker();
        self.reset_stream();
        self.link = Link::Disconnected;
        self.pins = None;
    }

    /// Reset to the state of a new session: disconnected, no port name, log
    /// stopped. The screen stays (§5.3: the user clears it).
    pub fn reset_to_unconnected(&mut self) {
        self.disconnect();
        let _ = self.log.command(LogCommand::Stop);
        self.params.path.clear();
        self.usb = None;
        self.note = None;
    }

    /// Disconnect and wait (up to `CLOSE_WAIT`) until the port thread has
    /// dropped the port. Use it when the process is about to exit: without
    /// the wait the OS closes the descriptor first, and on Linux a port that
    /// was never closed by `serialport` keeps `TIOCEXCL` set. A thread that
    /// does not finish in time is given up.
    pub fn close(&mut self) {
        self.disconnect();
        self.join_reaping(CLOSE_WAIT);
    }

    fn join_reaping(&mut self, wait: Duration) -> usize {
        let handles = self.reaping.drain(..).map(|(handle, _)| handle).collect();
        join_bounded(handles, wait)
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

    pub fn note(&self) -> Option<&Message> {
        self.note.as_ref()
    }

    pub fn clear_note(&mut self) {
        self.note = None;
    }

    /// Replace the status-line message with a typed one.
    pub fn set_message(&mut self, message: Message) {
        self.note = Some(message);
    }

    pub fn set_note(&mut self, note: impl Into<String>) {
        self.note = Some(Message::error(note));
    }

    /// Periodic work, about once a second: flush a quiet log and reap port
    /// threads that were asked to stop.
    pub fn tick(&mut self) {
        self.log.tick();
        let now = Instant::now();
        let mut still = Vec::new();
        for (handle, since) in self.reaping.drain(..) {
            if handle.is_finished() {
                let _ = handle.join();
            } else {
                if now.duration_since(since) >= SHUTDOWN_WAIT && !self.reap_warned {
                    self.reap_warned = true;
                    self.note = Some(Message::error(
                        "シリアルのスレッドが 2 秒以内に終わらない（ポートが応答していない可能性）",
                    ));
                }
                still.push((handle, since));
            }
        }
        self.reaping = still;
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
            (true, Some(previous), Some(current)) => {
                serial::usb_identity_matches(previous, current)
            }
            _ => false,
        }
    }

    pub fn apply_rx(&mut self, bytes: &[u8]) {
        self.rx_bytes = self.rx_bytes.saturating_add(bytes.len() as u64);
        self.log.record_binary(bytes);
        let outcome = self.screen.apply_bytes(bytes);
        self.log.record_text(&outcome.text);
        for note in outcome.notes {
            self.note = Some(Message::error(note));
        }
        for reply in outcome.replies {
            if let Err(error) = self.queue_bytes(reply) {
                self.note = Some(Message::error(error.to_string()));
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
            self.note = Some(Message::error(error.to_string()));
        }
    }

    pub fn note_command_failed(&mut self, error: PortError) {
        self.note = Some(Message::error(error.to_string()));
    }

    /// The port refused new parameters. Show the reason and go back to the
    /// parameters the port still has (review N25).
    pub fn note_apply_failed(&mut self, error: PortError) {
        if let Some(applied) = self.applied.clone() {
            let path = self.params.path.clone();
            self.params = SerialParams { path, ..applied };
            if let Link::Open { .. } = self.link {
                self.link = Link::Open {
                    params: self.params.clone(),
                };
            }
        }
        self.note = Some(Message::error(format!("設定を戻した: {error}")));
    }

    /// Route one event from `EventRx`.
    pub fn apply_event(&mut self, event: IoEvent) {
        match event {
            IoEvent::Data(bytes) => self.apply_rx(&bytes),
            IoEvent::PortLost(error) => {
                self.note_port_lost(error);
                self.release_port();
            }
            IoEvent::CommandFailed(error) => self.note_command_failed(error),
            IoEvent::ApplyFailed(error) => self.note_apply_failed(error),
            IoEvent::Pins(pins) => self.note_pins(pins),
            IoEvent::Dropped { bytes } => self.note_dropped(bytes),
            IoEvent::TxDropped { bytes } => self.tx_bytes = self.tx_bytes.saturating_sub(bytes),
        }
    }

    pub fn note_pins(&mut self, pins: PinState) {
        self.pins = Some(pins);
    }

    pub fn note_dropped(&mut self, bytes: u64) {
        self.note = Some(Message::warn(format!(
            "受信が速すぎて計 {bytes} バイトを捨てた"
        )));
    }

    /// Stop the port thread and keep `Link::Lost`. `disconnect` is the
    /// transition back to `Disconnected`.
    pub fn release_port(&mut self) {
        self.stop_worker();
        self.reset_stream();
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
            self.note = Some(Message::error(note));
        }
    }

    pub fn set_newline(&mut self, newline: Newline) {
        let outcome = self.screen.set_newline(newline);
        self.log.record_text(&outcome.text);
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
            self.applied = Some(self.params.clone());
            self.params = params.clone();
            self.link = Link::Open { params };
            Ok(())
        } else if matches!(self.link, Link::Opening { .. }) {
            // Applied by `finish_connect` once the port is open.
            self.params = params.clone();
            self.link = Link::Opening { params };
            Ok(())
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
        if self.params.flow != serial::Flow::None {
            return Err(SessionError::Port(PortError::Unsupported(
                "RTS を手で動かせるのはフロー制御がなしのときだけ",
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
                    let bytes = codec::encode(self.encoding, &part)
                        .map_err(|ch| SessionError::Unmappable { ch })?;
                    echo.push(Echo::Text(part));
                    push_chunks(&mut chunks, bytes);
                }
                PastePiece::Newline => {
                    echo.push(Echo::Newline);
                    push_chunks(
                        &mut chunks,
                        codec::transmit_newline(self.newline.transmit).to_vec(),
                    );
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

    /// One `Input` is one command, so it is queued whole or not at all
    /// (review N4).
    fn queue_chunks(&mut self, chunks: Vec<Vec<u8>>) -> Result<(), SessionError> {
        if !matches!(self.link, Link::Open { .. }) {
            return Err(SessionError::NotOpen);
        }
        let bytes: Vec<u8> = chunks.concat();
        if bytes.len() > serial::MAX_PENDING {
            return Err(SessionError::TooLarge {
                bytes: bytes.len(),
                limit: serial::MAX_PENDING,
            });
        }
        let generation = self.generation.load(Ordering::Relaxed);
        let queued = bytes.len() as u64;
        self.send_command(Command::Write { generation, bytes })?;
        self.tx_bytes = self.tx_bytes.saturating_add(queued);
        Ok(())
    }

    fn send_command(&mut self, command: Command) -> Result<(), SessionError> {
        let Some(worker) = self.worker.as_ref() else {
            return Err(SessionError::NotOpen);
        };
        worker
            .commands
            .try_send(command)
            .map_err(|_| SessionError::Busy)
    }

    /// Drop the command sender. The thread sees the channel disconnect and
    /// returns; nothing here waits (reviews R41, N8). `tick` joins it.
    fn stop_worker(&mut self) {
        let Some(worker) = self.worker.take() else {
            return;
        };
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = worker.commands.try_send(Command::Cancel { generation });
        let _ = worker.commands.try_send(Command::Shutdown);
        drop(worker.commands);
        self.reaping.push((worker.join, Instant::now()));
    }

    fn reset_stream(&mut self) {
        let outcome = self.screen.finish_stream();
        self.log.record_text(&outcome.text);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop_worker();
        self.join_reaping(CLOSE_WAIT);
        let _ = self.log.command(LogCommand::Stop);
    }
}

enum Echo {
    Text(String),
    Newline,
}

/// The settings `Command::Apply` changes on an open port.
fn same_line(a: &SerialParams, b: &SerialParams) -> bool {
    a.baud_rate == b.baud_rate
        && a.data_bits == b.data_bits
        && a.parity == b.parity
        && a.stop_bits == b.stop_bits
        && a.flow == b.flow
}

fn push_chunks(chunks: &mut Vec<Vec<u8>>, bytes: Vec<u8>) {
    if bytes.is_empty() {
        return;
    }
    for piece in bytes.chunks(4096) {
        chunks.push(piece.to_vec());
    }
}

/// Bytes a key becomes on the wire. `app_cursor` selects SS3 arrows.
/// Character and Enter keys are empty here; use [`key_to_bytes`] for those.
pub fn encode_key(key: &KeyInput, app_cursor: bool) -> Result<Vec<u8>, SessionError> {
    key_bytes(key, app_cursor)
}

/// Wire bytes for one key, including encoded characters and the transmit newline.
pub fn key_to_bytes(
    key: &KeyInput,
    encoding: SessionEncoding,
    newline: crate::codec::TransmitNewline,
    app_cursor: bool,
) -> Result<Vec<u8>, SessionError> {
    match key {
        KeyInput::Char(ch) => {
            let text = ch.to_string();
            codec::encode(encoding, &text).map_err(|ch| SessionError::Unmappable { ch })
        }
        KeyInput::Alt(ch) => {
            let encoded = codec::encode(encoding, &ch.to_string())
                .map_err(|ch| SessionError::Unmappable { ch })?;
            let mut bytes = vec![0x1B];
            bytes.extend(encoded);
            Ok(bytes)
        }
        KeyInput::Enter => Ok(codec::transmit_newline(newline).to_vec()),
        other => encode_key(other, app_cursor),
    }
}

/// Visual row 0 is the top of the viewport. Alacritty line 0 is the top of
/// the live screen; lines increase downward, and history is negative once
/// `display_offset` moves the viewport up.
pub fn viewport_to_line(row: usize, rows: usize, display_offset: usize) -> i32 {
    let row = if rows == 0 { 0 } else { row.min(rows - 1) };
    row as i32 - display_offset as i32
}

pub fn line_to_viewport(line: i32, rows: usize, display_offset: usize) -> Option<usize> {
    let rows = rows.max(1);
    let visual = line + display_offset as i32;
    if visual >= 0 && (visual as usize) < rows {
        Some(visual as usize)
    } else {
        None
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
        KeyInput::Ctrl(ch) => {
            return ctrl_byte(*ch)
                .map(|byte| vec![byte])
                .ok_or(SessionError::NoControlCode { ch: *ch })
        }
    };
    Ok(bytes.to_vec())
}

/// Ctrl plus a key, as a C0 byte. `None` for keys without a control code.
pub(crate) fn ctrl_byte(ch: char) -> Option<u8> {
    match ch.to_ascii_lowercase() {
        other if other.is_ascii_lowercase() => Some((other as u8) - b'a' + 1),
        '@' | ' ' | '2' => Some(0x00),
        '[' | '3' => Some(0x1B),
        '\\' | '4' => Some(0x1C),
        ']' | '5' => Some(0x1D),
        '^' | '6' => Some(0x1E),
        '_' | '-' | '7' => Some(0x1F),
        '?' | '8' => Some(0x7F),
        _ => None,
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
    use std::thread;

    struct SharedPort {
        inner: Arc<Mutex<PortState>>,
    }

    impl Drop for SharedPort {
        fn drop(&mut self) {
            lock(&self.inner).closed = true;
        }
    }

    struct PortState {
        reads: VecDeque<io::Result<Vec<u8>>>,
        written: Vec<u8>,
        partial: Option<usize>,
        allow_write: bool,
        write_error: bool,
        applied: Vec<SerialParams>,
        dtr: Vec<bool>,
        rts: Vec<bool>,
        break_on: bool,
        break_count: u32,
        clear_count: u32,
        opened_with: Option<SerialParams>,
        pins: PinState,
        fail_rts: bool,
        fail_apply: bool,
        closed: bool,
        read_delay: Duration,
    }

    impl PortIo for SharedPort {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let delay = lock(&self.inner).read_delay;
            if !delay.is_zero() {
                thread::sleep(delay);
            }
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
            if state.write_error {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "write failed"));
            }
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
            let mut state = lock(&self.inner);
            if state.fail_apply {
                return Err(PortError::Io {
                    message: "ボーレートをポートが受け付けない".into(),
                });
            }
            state.applied.push(params.clone());
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

    type SharedOpen = Box<dyn FnOnce(&SerialParams) -> Result<Box<dyn PortIo>, PortError> + Send>;

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
            .connect_with(
                SerialParams {
                    path: "COM3".into(),
                    ..SerialParams::default()
                },
                open,
            )
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
        let mut rx = session.connect_with(SerialParams::default(), open).unwrap();
        let event = wait_event(&mut rx);
        match event {
            Some(IoEvent::PortLost(error)) => session.note_port_lost(error),
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(session.link(), Link::Lost { .. }));
        // Lost is not Open, so a new connect is attempted. The opener fails
        // here without touching a real device.
        let error = match session.connect_with(SerialParams::default(), |_: &SerialParams| {
            Err(PortError::Io {
                message: "ポートが見つからない".into(),
            })
        }) {
            Err(error) => error,
            Ok(_) => panic!("the failing opener must fail"),
        };
        assert!(matches!(error, OpenError::Port(_)));
        assert!(matches!(session.link(), Link::Disconnected));
        assert!(
            session
                .note()
                .is_some_and(|note| note.text.contains("を開けない")),
            "{:?}",
            session.note()
        );
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
        assert_eq!(
            lock(&state).opened_with.as_ref().unwrap().dtr_on_open,
            Some(true)
        );
        let error = session.set_rts(false).unwrap_err();
        assert!(matches!(
            error,
            SessionError::Port(PortError::Unsupported(_))
        ));
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
            .apply_input(Input::Break {
                hold: Duration::from_millis(30),
            })
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
        let keys = [
            KeyInput::Ctrl('c'),
            KeyInput::Ctrl('D'),
            KeyInput::Ctrl('z'),
            KeyInput::Ctrl('['),
            KeyInput::Ctrl('@'),
            KeyInput::Backspace,
            KeyInput::Delete,
            KeyInput::Home,
            KeyInput::End,
            KeyInput::Function(1),
            KeyInput::Arrow(Direction::Up),
            KeyInput::Arrow(Direction::Left),
        ];
        for key in keys {
            session.apply_input(Input::Key(key)).unwrap();
        }
        let expected: Vec<u8> = [
            &[0x03u8][..],
            &[0x04],
            &[0x1A],
            &[0x1B],
            &[0x00],
            &[0x08],
            b"\x1b[3~",
            b"\x1b[H",
            b"\x1b[F",
            b"\x1bOP",
            b"\x1b[A",
            b"\x1b[D",
        ]
        .concat();
        let written = wait_written(&state, expected.len());
        assert_eq!(written, expected);
    }

    #[test]
    fn app_cursor_mode_sends_ss3_arrows() {
        let (state, open) = open_shared(empty_state());
        let mut session = session();
        let _rx = session.connect_with(SerialParams::default(), open).unwrap();
        session.apply_rx(b"\x1b[?1h");
        session
            .apply_input(Input::Key(KeyInput::Arrow(Direction::Up)))
            .unwrap();
        session
            .apply_input(Input::Key(KeyInput::Arrow(Direction::Right)))
            .unwrap();
        assert_eq!(wait_written(&state, 6), b"\x1bOA\x1bOC");
    }

    #[test]
    fn ctrl_without_a_control_code_is_an_error() {
        let (_state, open) = open_shared(empty_state());
        let mut session = session();
        let _rx = session.connect_with(SerialParams::default(), open).unwrap();
        let error = session
            .apply_input(Input::Key(KeyInput::Ctrl('é')))
            .unwrap_err();
        assert_eq!(error, SessionError::NoControlCode { ch: 'é' });
    }

    #[test]
    fn a_full_data_channel_keeps_every_byte_and_port_lost() {
        // 200 chunks then a broken pipe. The channel is shallower than that,
        // so the worker blocks instead of dropping. PortLost still arrives
        // (review N2, design v7 §7.5).
        let mut reads: VecDeque<io::Result<Vec<u8>>> =
            (0..200).map(|_| Ok(vec![b'x'; 100])).collect();
        reads.push_back(Err(io::Error::new(io::ErrorKind::BrokenPipe, "gone")));
        let (_state, open) = open_shared(PortState {
            reads,
            ..empty_state()
        });
        let mut session = session();
        let mut rx = session.connect_with(SerialParams::default(), open).unwrap();
        let mut lost = false;
        let mut data = 0u64;
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && !(lost && data == 200 * 100) {
            match rx.try_recv() {
                Some(IoEvent::PortLost(_)) => lost = true,
                Some(IoEvent::Dropped { .. }) => panic!("receive path dropped bytes"),
                Some(IoEvent::Data(bytes)) => data += bytes.len() as u64,
                Some(_) => {}
                None => thread::sleep(Duration::from_millis(1)),
            }
        }
        assert!(lost, "PortLost was swallowed");
        assert_eq!(data, 200 * 100);
    }

    #[test]
    fn a_megabyte_burst_is_not_dropped() {
        const TOTAL: usize = 1024 * 1024;
        let mut reads = VecDeque::new();
        let mut next = 0u8;
        let mut remaining = TOTAL;
        while remaining > 0 {
            let count = remaining.min(100);
            let mut chunk = Vec::with_capacity(count);
            for _ in 0..count {
                chunk.push(next);
                next = next.wrapping_add(1);
            }
            reads.push_back(Ok(chunk));
            remaining -= count;
        }
        let (_state, open) = open_shared(PortState {
            reads,
            ..empty_state()
        });
        let mut session = session();
        let mut rx = session.connect_with(SerialParams::default(), open).unwrap();
        let mut got = Vec::with_capacity(TOTAL);
        let deadline = Instant::now() + Duration::from_secs(20);
        while got.len() < TOTAL && Instant::now() < deadline {
            match rx.try_recv() {
                Some(IoEvent::Data(bytes)) => got.extend(bytes),
                Some(IoEvent::Dropped { .. }) => panic!("receive path dropped bytes"),
                Some(IoEvent::Pins(_)) => {}
                Some(other) => panic!("unexpected {other:?}"),
                None => thread::sleep(Duration::from_millis(1)),
            }
        }
        assert_eq!(got.len(), TOTAL, "short by {}", TOTAL - got.len());
        let mut expect = 0u8;
        for (index, byte) in got.iter().enumerate() {
            assert_eq!(*byte, expect, "mismatch at {index}");
            expect = expect.wrapping_add(1);
        }
    }

    #[test]
    fn scrollback_shows_earlier_lines() {
        let mut session = Session::new(SessionConfig {
            screen: ScreenConfig {
                columns: 20,
                rows: 4,
                scrollback: 100,
            },
            newline: Newline {
                receive: ReceiveNewline::Cr,
                transmit: TransmitNewline::Cr,
            },
            ..SessionConfig::default()
        });
        for index in 0..12 {
            session.apply_rx(format!("L{index:02}\r\n").as_bytes());
        }
        let before = visible_rows(&session);
        assert!(before.iter().any(|row| row.contains("L11")), "{before:?}");
        assert!(!before.iter().any(|row| row.contains("L00")), "{before:?}");
        session.scroll_lines(50);
        let snap = session.screen_content();
        assert!(snap.display_offset > 0, "offset {}", snap.display_offset);
        let after = rows_of(&snap);
        assert!(after.iter().any(|row| row.contains("L00")), "{after:?}");
    }

    #[test]
    fn top_row_selection_matches_viewport_lines() {
        let mut session = Session::new(SessionConfig {
            screen: ScreenConfig {
                columns: 20,
                rows: 4,
                scrollback: 100,
            },
            newline: Newline {
                receive: ReceiveNewline::Cr,
                transmit: TransmitNewline::Cr,
            },
            ..SessionConfig::default()
        });
        session.apply_rx(b"hello\r\n");
        for _ in 0..8 {
            session.apply_rx(b"xxxx\r\n");
        }
        session.scroll_lines(50);
        let snap = session.screen_content();
        assert!(snap.display_offset > 0, "offset {}", snap.display_offset);
        let line = viewport_to_line(0, snap.rows, snap.display_offset);
        assert_eq!(line, -(snap.display_offset as i32));
        session.select_start(line, 0);
        session.select_update(line, 19);
        let selected = session.selected_text().unwrap_or_default();
        let top = rows_of(&snap).into_iter().next().unwrap_or_default();
        assert!(
            selected.contains(top.trim()),
            "selected {selected:?} top {top:?} line {line}"
        );
        assert_eq!(
            line_to_viewport(line, snap.rows, snap.display_offset),
            Some(0)
        );
        let bottom = viewport_to_line(snap.rows - 1, snap.rows, snap.display_offset);
        assert_eq!(
            line_to_viewport(bottom, snap.rows, snap.display_offset),
            Some(snap.rows - 1)
        );
    }

    #[test]
    fn counters_follow_rx_and_tx() {
        let (state, open) = open_shared(empty_state());
        let mut session = session();
        let _rx = session.connect_with(SerialParams::default(), open).unwrap();
        session.apply_rx(b"abc");
        assert_eq!(session.rx_bytes(), 3);
        session.apply_input(Input::Bytes(vec![1, 2, 3, 4])).unwrap();
        let written = wait_written(&state, 4);
        assert_eq!(written, vec![1, 2, 3, 4]);
        assert_eq!(session.tx_bytes(), 4);
    }

    #[test]
    fn read_error_releases_the_port_without_shutdown() {
        let (state, open) = open_shared(PortState {
            reads: VecDeque::from([Err(io::Error::new(io::ErrorKind::BrokenPipe, "gone"))]),
            ..empty_state()
        });
        let mut session = session();
        let _rx = session.connect_with(SerialParams::default(), open).unwrap();
        // No disconnect: the thread closes the handle by itself (review N3).
        wait_until(|| lock(&state).closed);
    }

    #[test]
    fn eof_is_port_lost() {
        let (_state, open) = open_shared(PortState {
            reads: VecDeque::from([Ok(Vec::new())]),
            ..empty_state()
        });
        let mut session = session();
        let mut rx = session.connect_with(SerialParams::default(), open).unwrap();
        match wait_event(&mut rx) {
            Some(IoEvent::PortLost(error)) => session.apply_event(IoEvent::PortLost(error)),
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(session.link(), Link::Lost { .. }));
    }

    #[test]
    fn a_large_paste_is_sent_whole() {
        let (state, open) = open_shared(empty_state());
        let mut session = session();
        let _rx = session.connect_with(SerialParams::default(), open).unwrap();
        // 300 KiB used to overflow the 32-deep command channel (review N4).
        let text = "a".repeat(300 * 1024);
        session.apply_input(Input::Text(text)).unwrap();
        let written = wait_written(&state, 300 * 1024);
        assert_eq!(written.len(), 300 * 1024);
    }

    #[test]
    fn a_paste_over_the_limit_sends_nothing() {
        let (state, open) = open_shared(empty_state());
        let mut session = session();
        let _rx = session.connect_with(SerialParams::default(), open).unwrap();
        let text = "a".repeat(serial::MAX_PENDING + 1);
        let error = session.apply_input(Input::Text(text)).unwrap_err();
        assert!(matches!(error, SessionError::TooLarge { .. }));
        thread::sleep(Duration::from_millis(30));
        assert!(lock(&state).written.is_empty());
    }

    #[test]
    fn disconnect_does_not_wait_for_a_slow_read() {
        let (state, open) = open_shared(PortState {
            read_delay: Duration::from_millis(300),
            ..empty_state()
        });
        let mut session = session();
        let _rx = session.connect_with(SerialParams::default(), open).unwrap();
        thread::sleep(Duration::from_millis(20));
        let started = Instant::now();
        session.disconnect();
        assert!(started.elapsed() < Duration::from_millis(100));
        assert!(matches!(session.link(), Link::Disconnected));
        wait_until(|| {
            session.tick();
            lock(&state).closed
        });
    }

    #[test]
    fn open_is_split_and_a_disconnect_cancels_it() {
        let (state, open) = open_shared(empty_state());
        let mut session = session();
        let pending = session
            .begin_connect_with(
                SerialParams {
                    path: "COM9".into(),
                    ..SerialParams::default()
                },
                Box::new(open),
            )
            .unwrap();
        assert!(matches!(session.link(), Link::Opening { .. }));
        let opened = thread::spawn(move || pending.open()).join().unwrap();
        session.disconnect();
        assert_eq!(
            session.finish_connect(opened).err(),
            Some(OpenError::Cancelled)
        );
        assert!(matches!(session.link(), Link::Disconnected));
        // The port opened in the background is closed again.
        wait_until(|| lock(&state).closed);
    }

    #[test]
    fn refused_parameters_are_reverted() {
        let (_state, open) = open_shared(PortState {
            fail_apply: true,
            ..empty_state()
        });
        let mut session = session();
        let mut rx = session
            .connect_with(
                SerialParams {
                    baud_rate: 115_200,
                    ..SerialParams::default()
                },
                open,
            )
            .unwrap();
        let next = SerialParams {
            baud_rate: 12_345,
            ..session.params().clone()
        };
        session.set_serial_params(next).unwrap();
        assert_eq!(session.params().baud_rate, 12_345);
        match wait_event(&mut rx) {
            Some(event @ IoEvent::ApplyFailed(_)) => session.apply_event(event),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(session.params().baud_rate, 115_200);
        assert!(
            session
                .note()
                .is_some_and(|note| note.text.contains("設定を戻した")),
            "{:?}",
            session.note()
        );
    }

    #[test]
    fn manual_rts_only_without_flow_control() {
        for flow in [Flow::XonXoff, Flow::RtsCts] {
            let (state, open) = open_shared(empty_state());
            let mut session = session();
            let _rx = session
                .connect_with(
                    SerialParams {
                        flow,
                        ..SerialParams::default()
                    },
                    open,
                )
                .unwrap();
            assert!(session.set_rts(true).is_err(), "{flow:?}");
            thread::sleep(Duration::from_millis(20));
            assert!(lock(&state).rts.is_empty());
        }
    }

    #[test]
    fn disconnect_finishes_the_receive_stream() {
        let mut session = Session::new(SessionConfig {
            encoding: SessionEncoding::ShiftJis,
            newline: Newline {
                receive: ReceiveNewline::Cr,
                transmit: TransmitNewline::Cr,
            },
            ..SessionConfig::default()
        });
        // A half escape sequence and a lone Shift_JIS lead byte.
        session.apply_rx(b"A\x1b[");
        session.disconnect();
        session.apply_rx(b"31mB\x82");
        session.disconnect();
        session.apply_rx(b"C");
        let row = visible(&session);
        // The parser forgot `ESC [`, so `31m` is printed; the lead became
        // U+FFFD instead of joining `C` (reviews N11, N15).
        assert_eq!(row, "A31mB\u{FFFD}C");
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
                rotate: Rotate {
                    max_bytes: 64 * 1024,
                    keep: 2,
                },
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

    #[test]
    fn text_log_maps_line_drawing_like_the_screen() {
        // One parser feeds both, so `ESC ( 0` changes the log too (R19).
        let dir = std::env::temp_dir().join(format!("rs-term-dec-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dec.log");
        let mut session = session();
        session
            .log(LogCommand::Start(LogTarget::Text {
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
        session.apply_rx(b"\x1b(0lqk\x1b(Bq");
        assert_eq!(visible(&session), "┌─┐q");
        session.log(LogCommand::Stop).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "┌─┐q");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn empty_state() -> PortState {
        PortState {
            reads: VecDeque::new(),
            written: Vec::new(),
            partial: None,
            allow_write: true,
            write_error: false,
            applied: Vec::new(),
            dtr: Vec::new(),
            rts: Vec::new(),
            break_on: false,
            break_count: 0,
            clear_count: 0,
            opened_with: None,
            pins: PinState {
                cts: false,
                dsr: false,
                cd: false,
                ri: false,
            },
            fail_rts: false,
            fail_apply: false,
            closed: false,
            read_delay: Duration::ZERO,
        }
    }

    fn visible(session: &Session) -> String {
        rows_of(&session.screen_content())
            .into_iter()
            .next()
            .unwrap_or_default()
    }

    fn visible_rows(session: &Session) -> Vec<String> {
        rows_of(&session.screen_content())
    }

    fn rows_of(snap: &crate::screen::ScreenSnapshot) -> Vec<String> {
        let mut rows = vec![String::new(); snap.rows];
        for (index, cell) in snap.cells.iter().enumerate() {
            let row = index / snap.columns.max(1);
            if row >= rows.len() || cell.width == 0 {
                continue;
            }
            rows[row].push_str(&cell.text);
        }
        rows.into_iter()
            .map(|row| row.trim_end().to_string())
            .collect()
    }

    fn wait_data(rx: &mut EventRx) -> Option<IoEvent> {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            match rx.try_recv() {
                Some(event @ IoEvent::Data(_)) => return Some(event),
                Some(IoEvent::Pins(_)) | Some(IoEvent::Dropped { .. }) => {}
                Some(other) => return Some(other),
                None if rx.is_closed() => return None,
                None => thread::sleep(Duration::from_millis(5)),
            }
        }
        None
    }

    fn wait_event(rx: &mut EventRx) -> Option<IoEvent> {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            match rx.try_recv() {
                Some(IoEvent::Pins(_)) => {}
                Some(event) => return Some(event),
                None if rx.is_closed() => return None,
                None => thread::sleep(Duration::from_millis(5)),
            }
        }
        None
    }

    fn wait_until(mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if done() {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("condition not reached in 3 s");
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

    #[test]
    fn close_releases_the_port_without_tick() {
        let (state, open) = open_shared(empty_state());
        let mut session = session();
        let _rx = session.connect_with(SerialParams::default(), open).unwrap();
        assert!(!lock(&state).closed);
        session.close();
        // No tick, no waiting here: close itself joined the thread.
        assert!(lock(&state).closed, "the port was not dropped by close");
        assert!(session.reaping.is_empty());
    }

    #[test]
    fn drop_releases_the_port_without_tick() {
        let (state, open) = open_shared(empty_state());
        let mut session = session();
        let _rx = session.connect_with(SerialParams::default(), open).unwrap();
        drop(session);
        assert!(lock(&state).closed, "the port was not dropped by Drop");
    }

    #[test]
    fn disconnect_stays_non_blocking_and_tick_reaps() {
        let (state, open) = open_shared(PortState {
            read_delay: Duration::from_millis(300),
            ..empty_state()
        });
        let mut session = session();
        let _rx = session.connect_with(SerialParams::default(), open).unwrap();
        let started = Instant::now();
        session.disconnect();
        assert!(started.elapsed() < Duration::from_millis(200));
        assert_eq!(session.reaping.len(), 1);
        wait_until(|| {
            session.tick();
            session.reaping.is_empty()
        });
        assert!(lock(&state).closed);
    }

    #[test]
    fn close_gives_up_on_a_stuck_thread() {
        let (release, hold) = std::sync::mpsc::channel::<()>();
        let stuck = thread::spawn(move || {
            let _ = hold.recv();
        });
        let done = thread::spawn(|| {});
        let started = Instant::now();
        let given_up = join_bounded(vec![done, stuck], Duration::from_millis(100));
        assert_eq!(given_up, 1);
        assert!(started.elapsed() < Duration::from_secs(1));
        let _ = release.send(());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn close_frees_a_pty_slave_that_was_opened_exclusively() {
        use std::os::fd::{FromRawFd, OwnedFd};

        // The master stays open, so TTY_EXCLUSIVE set on the slave survives
        // until the slave is closed by `serialport` (TIOCNXCL on drop).
        let mut master = 0;
        let mut slave = 0;
        let mut name = [0 as libc::c_char; 128];
        let rc = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                name.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        assert_eq!(rc, 0, "openpty");
        let _master = unsafe { OwnedFd::from_raw_fd(master) };
        drop(unsafe { OwnedFd::from_raw_fd(slave) });
        let path = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        let params = SerialParams {
            path: path.clone(),
            ..SerialParams::default()
        };

        let mut session = session();
        let _rx = session.connect(params.clone()).expect("first open");
        let second = serial::open_port(&params);
        assert!(second.is_err(), "a second open must be refused");
        drop(second);

        session.close();
        let again = serial::open_port(&params);
        assert!(again.is_ok(), "the slave stayed busy after close");
    }

    /// Apply events until `done` holds or two seconds pass.
    fn pump_events(session: &mut Session, rx: &mut EventRx, done: impl Fn(&Session) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && !done(session) {
            match rx.try_recv() {
                Some(event) => session.apply_event(event),
                None => thread::sleep(Duration::from_millis(5)),
            }
        }
    }

    #[test]
    fn tx_counts_what_is_queued_and_a_write_error_takes_it_back() {
        let (state, open) = open_shared(PortState {
            write_error: true,
            ..empty_state()
        });
        let mut session = session();
        let mut rx = session.connect_with(SerialParams::default(), open).unwrap();
        session
            .apply_input(Input::Bytes(b"abcde".to_vec()))
            .unwrap();
        assert_eq!(session.tx_bytes(), 5);
        pump_events(&mut session, &mut rx, |s| s.tx_bytes() == 0);
        assert_eq!(session.tx_bytes(), 0);
        assert!(lock(&state).written.is_empty());
        assert!(session.note().is_some());
    }

    #[test]
    fn tx_does_not_count_bytes_a_cancel_discarded() {
        let (state, open) = open_shared(PortState {
            allow_write: false,
            ..empty_state()
        });
        let mut session = session();
        let mut rx = session.connect_with(SerialParams::default(), open).unwrap();
        session.apply_input(Input::Bytes(b"abc".to_vec())).unwrap();
        session.apply_input(Input::Bytes(b"de".to_vec())).unwrap();
        assert_eq!(session.tx_bytes(), 5);
        session.apply_input(Input::Cancel).unwrap();
        pump_events(&mut session, &mut rx, |s| s.tx_bytes() == 0);
        assert_eq!(session.tx_bytes(), 0);
        assert!(lock(&state).written.is_empty());
    }

    #[test]
    fn tx_does_not_count_bytes_the_pending_limit_refused() {
        let (_state, open) = open_shared(PortState {
            allow_write: false,
            ..empty_state()
        });
        let mut session = session();
        let mut rx = session.connect_with(SerialParams::default(), open).unwrap();
        let big = serial::MAX_PENDING / 2 + 1;
        session.apply_input(Input::Bytes(vec![b'a'; big])).unwrap();
        session.apply_input(Input::Bytes(vec![b'b'; big])).unwrap();
        assert_eq!(session.tx_bytes(), 2 * big as u64);
        pump_events(&mut session, &mut rx, |s| s.tx_bytes() == big as u64);
        assert_eq!(session.tx_bytes(), big as u64);
    }

    #[test]
    fn tx_stays_counted_when_the_bytes_are_written() {
        let (state, open) = open_shared(empty_state());
        let mut session = session();
        let mut rx = session.connect_with(SerialParams::default(), open).unwrap();
        session.apply_input(Input::Bytes(b"abc".to_vec())).unwrap();
        wait_until(|| lock(&state).written == b"abc");
        thread::sleep(Duration::from_millis(100));
        while let Some(event) = rx.try_recv() {
            session.apply_event(event);
        }
        assert_eq!(session.tx_bytes(), 3);
    }

    fn begin_opening(session: &mut Session, open: SharedOpen) -> PendingOpen {
        session
            .begin_connect_with(
                SerialParams {
                    path: "COM9".into(),
                    baud_rate: 115_200,
                    ..SerialParams::default()
                },
                open,
            )
            .unwrap()
    }

    #[test]
    fn a_baud_change_while_opening_is_applied_when_the_open_finishes() {
        let (state, open) = open_shared(empty_state());
        let mut session = session();
        let pending = begin_opening(&mut session, open);
        let next = SerialParams {
            baud_rate: 9_600,
            ..session.params().clone()
        };
        session.set_serial_params(next).unwrap();
        assert_eq!(session.params().baud_rate, 9_600);
        assert!(matches!(session.link(), Link::Opening { params } if params.baud_rate == 9_600));
        let opened = pending.open();
        let _rx = session.finish_connect(opened).unwrap();
        assert_eq!(session.params().baud_rate, 9_600);
        assert!(matches!(session.link(), Link::Open { params } if params.baud_rate == 9_600));
        wait_until(|| lock(&state).applied.last().map(|p| p.baud_rate) == Some(9_600));
    }

    #[test]
    fn a_refused_baud_from_the_opening_period_is_reverted() {
        let (_state, open) = open_shared(PortState {
            fail_apply: true,
            ..empty_state()
        });
        let mut session = session();
        let pending = begin_opening(&mut session, open);
        let next = SerialParams {
            baud_rate: 9_600,
            ..session.params().clone()
        };
        session.set_serial_params(next).unwrap();
        let opened = pending.open();
        let mut rx = session.finish_connect(opened).unwrap();
        match wait_event(&mut rx) {
            Some(event @ IoEvent::ApplyFailed(_)) => session.apply_event(event),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(session.params().baud_rate, 115_200);
    }

    #[test]
    fn a_path_change_while_opening_is_still_refused() {
        let (_state, open) = open_shared(empty_state());
        let mut session = session();
        let _pending = begin_opening(&mut session, open);
        let other = SerialParams {
            path: "COM10".into(),
            ..session.params().clone()
        };
        assert!(session.set_serial_params(other).is_err());
        assert_eq!(session.params().path, "COM9");
    }
}
