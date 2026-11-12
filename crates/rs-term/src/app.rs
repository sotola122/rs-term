//! One serial session and the modal commands around it.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use rs_term_core::{
    classic_dark, list_ports, load_settings_file, log_dir_from_env, palette_by_name,
    settings_path_from_env, suggested_log_path, viewport_to_line, DataBits, EventRx, Flow, Input,
    IoEvent, KeyInput, Link, LogCommand, LogEncoding, LogSession, LogTarget, Message, OpenError,
    OpenedPort, Parity, ScreenConfig, SerialParams, Session, SessionConfig, SessionError, Settings,
    Severity, StopBits, TerminalPalette, MAX_PENDING,
};

use crate::cli::{self, Cli};
use crate::copy::{clipboard_text, copy_text};
use crate::keys::{self, Action, Command, Focus, Prefix};
use crate::status::{LinkState, StatusView};

const BREAK_HOLD: Duration = Duration::from_millis(250);
const DRAIN_BUDGET: usize = 512 * 1024;
const WHEEL_LINES: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modal {
    Picker,
    SendString,
    SendFile,
    LogPath,
    Baud,
    Encoding,
}

pub struct ModalBody {
    pub title: &'static str,
    pub lines: Vec<(String, bool)>,
    /// How many leading lines stay on screen when the rest scrolls to keep
    /// the selected line visible (the picker's input and notes).
    pub pinned: usize,
}

pub struct App {
    session: Session,
    events: Option<EventRx>,
    opening: Option<Receiver<OpenedPort>>,
    settings: Settings,
    palette: TerminalPalette,
    prefix: Prefix,
    prefix_armed: bool,
    help: bool,
    help_scroll: usize,
    modal: Option<Modal>,
    input: String,
    modal_error: Option<String>,
    ports: Vec<String>,
    port_note: Option<String>,
    picker_index: usize,
    quit: bool,
    size: (u16, u16),
    last_tick: Instant,
    /// Startup reasons (broken settings, warnings, a `--log` failure) that a
    /// successful connect must not wipe from the status line.
    startup: Option<Message>,
}

impl App {
    pub fn new(cli: Cli) -> Self {
        let (settings, load_note) = load_settings();
        Self::with_settings(cli, settings, load_note)
    }

    fn with_settings(cli: Cli, settings: Settings, load_note: Option<String>) -> Self {
        let encoding = cli.encoding.unwrap_or(settings.encoding);
        let palette = palette_by_name(&settings.terminal.palette).unwrap_or_else(classic_dark);
        let mut session = Session::new(SessionConfig {
            screen: ScreenConfig {
                columns: settings.terminal.columns,
                rows: settings.terminal.rows,
                scrollback: settings.terminal.scrollback,
            },
            encoding,
            newline: settings.newline,
            local_echo: settings.terminal.local_echo,
        });
        let params = params_from(&settings, &cli, cli.port.clone().unwrap_or_default());
        let _ = session.set_serial_params(params.clone());
        let mut startup = Vec::new();
        if let Some(note) = load_note {
            startup.push(Message::error(note));
        } else if !settings.warnings.is_empty() {
            startup.push(Message::warn(settings.warnings.join(" ")));
        }
        let mut app = Self {
            session,
            events: None,
            opening: None,
            settings,
            palette,
            prefix: cli.prefix,
            prefix_armed: false,
            help: false,
            help_scroll: 0,
            modal: None,
            input: String::new(),
            modal_error: None,
            ports: Vec::new(),
            port_note: None,
            picker_index: 0,
            quit: false,
            size: (80, 24),
            last_tick: Instant::now(),
            startup: None,
        };
        if let Some(path) = cli.log {
            startup.push(app.start_log(path));
        }
        app.startup = combine_startup(startup);
        if let Some(message) = &app.startup {
            app.session.set_message(message.clone());
        }
        if params.path.is_empty() {
            app.open_picker();
        } else {
            app.start_connect(params);
        }
        app
    }

    /// Prefix `q` or a termination signal (SIGTERM / SIGHUP on Unix).
    pub fn should_quit(&self) -> bool {
        self.quit || crate::signal::requested()
    }

    /// Quit path: stop the log and the port, and wait (bounded) until the
    /// port thread has dropped the port, so the OS never closes a port that
    /// is still marked exclusive. Run it before the terminal is restored.
    pub fn shutdown(&mut self) {
        self.events = None;
        self.opening = None;
        self.session.close();
    }

    pub fn show_help(&self) -> bool {
        self.help
    }

    pub fn help_scroll(&self) -> usize {
        self.help_scroll
    }

    fn scroll_help(&mut self, up: bool, page: bool) {
        let area = ratatui::layout::Rect::new(0, 0, self.size.0, self.size.1);
        let step = if page {
            crate::view::help_geometry(area).1.max(1)
        } else {
            1
        };
        let max = crate::view::help_max_scroll(area);
        self.help_scroll = if up {
            self.help_scroll.saturating_sub(step)
        } else {
            (self.help_scroll + step).min(max)
        };
    }

    pub fn palette(&self) -> &TerminalPalette {
        &self.palette
    }

    pub fn snapshot(&self) -> rs_term_core::ScreenSnapshot {
        self.session.screen_content()
    }

    pub fn status_view(&self) -> StatusView {
        let params = self.session.params();
        StatusView {
            prefix: self.prefix_armed,
            port: if params.path.is_empty() {
                "未選択".into()
            } else {
                params.path.clone()
            },
            frame: frame_text(params),
            encoding: self.session.encoding().label().into(),
            link: link_state(self.session.link()),
            log: log_word(self.session.log_state()),
            rx: self.session.rx_bytes(),
            tx: self.session.tx_bytes(),
            message: self
                .session
                .note()
                .map(|note| (note.severity, note.text.clone())),
        }
    }

    pub fn modal_body(&self) -> Option<ModalBody> {
        let modal = self.modal?;
        let mut lines = Vec::new();
        if let Some(error) = &self.modal_error {
            lines.push((error.clone(), false));
        }
        let mut pinned = None;
        let title = match modal {
            Modal::Picker => {
                lines.push((format!("> {}", self.input), false));
                if let Some(note) = &self.port_note {
                    lines.push((note.clone(), false));
                }
                let paths = self.filtered_ports();
                if paths.is_empty() && self.input.is_empty() {
                    lines.push(("ポートが無い。パスを入力して Enter".into(), false));
                }
                pinned = Some(lines.len());
                for (index, path) in paths.iter().enumerate() {
                    lines.push((
                        path.clone(),
                        index == self.picker_index.min(paths.len().saturating_sub(1)),
                    ));
                }
                "ポート"
            }
            Modal::SendString => {
                lines.push(("送る文字列。Enter で送信改行を足す".into(), false));
                lines.push((format!("> {}█", self.input), false));
                "文字列を送る"
            }
            Modal::SendFile => {
                lines.push(("送るファイルのパス".into(), false));
                lines.push((format!("> {}█", self.input), false));
                "ファイルを送る"
            }
            Modal::LogPath => {
                lines.push(("ログファイルのパス".into(), false));
                lines.push((format!("> {}█", self.input), false));
                "ログ開始"
            }
            Modal::Baud => {
                lines.push(("ボーレート（1〜4000000）".into(), false));
                lines.push((format!("> {}█", self.input), false));
                "ボーレート"
            }
            Modal::Encoding => {
                lines.push(("sjis または utf8".into(), false));
                lines.push((format!("> {}█", self.input), false));
                "エンコーディング"
            }
        };
        let pinned = pinned.unwrap_or(lines.len());
        Some(ModalBody {
            title,
            lines,
            pinned,
        })
    }

    pub fn on_resize(&mut self, width: u16, height: u16) {
        self.size = (width.max(1), height.max(1));
        let columns = self.size.0 as usize;
        let rows = self.size.1.saturating_sub(1).max(1) as usize;
        self.session.resize(columns, rows);
    }

    pub fn drain_serial(&mut self) -> bool {
        self.poll_open();
        if self.last_tick.elapsed() >= Duration::from_secs(1) {
            self.session.tick();
            self.last_tick = Instant::now();
        }
        let mut taken = 0usize;
        let mut any = false;
        while taken < DRAIN_BUDGET {
            let event = {
                let Some(events) = self.events.as_mut() else {
                    break;
                };
                events.try_recv()
            };
            let Some(event) = event else {
                break;
            };
            if let IoEvent::Data(bytes) = &event {
                taken += bytes.len();
            }
            any = true;
            self.session.apply_event(event);
        }
        any
    }

    pub fn on_paste(&mut self, text: String) {
        if self.help {
            return;
        }
        if self.modal.is_some() {
            for ch in text.chars() {
                if ch != '\n' && ch != '\r' {
                    self.input.push(ch);
                }
            }
            self.filter_changed();
            return;
        }
        self.prefix_armed = false;
        self.send_input(Input::Text(text));
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        if self.help {
            match keys::classify(&key, self.prefix, Focus::Help) {
                Action::Run(Command::Quit) => self.quit = true,
                Action::Run(Command::Cancel) => self.help = false,
                Action::Scroll { up, page } => self.scroll_help(up, page),
                _ => {}
            }
            return;
        }
        if self.modal.is_some() {
            self.on_modal_key(key);
            return;
        }
        let focus = if self.prefix_armed {
            Focus::Prefix
        } else {
            Focus::Terminal
        };
        match keys::classify(&key, self.prefix, focus) {
            Action::ArmPrefix => self.prefix_armed = true,
            Action::LiteralPrefix => {
                self.prefix_armed = false;
                self.send_input(Input::Key(KeyInput::Ctrl(self.prefix.ctrl)));
            }
            Action::Run(command) => {
                self.prefix_armed = false;
                self.run(command);
            }
            Action::Input(input) => self.send_input(input),
            Action::Scroll { up, page } => {
                if page {
                    self.session.scroll_page(up);
                } else {
                    self.session
                        .scroll_lines(if up { WHEEL_LINES } else { -WHEEL_LINES });
                }
            }
            Action::Ignore => {
                if self.prefix_armed {
                    self.prefix_armed = false;
                }
            }
        }
    }

    pub fn on_mouse(&mut self, mouse: MouseEvent) {
        if self.modal.is_some() || self.help {
            return;
        }
        let term_rows = self.size.1.saturating_sub(1);
        if mouse.row >= term_rows {
            return;
        }
        match mouse.kind {
            MouseEventKind::ScrollUp => self.session.scroll_lines(WHEEL_LINES),
            MouseEventKind::ScrollDown => self.session.scroll_lines(-WHEEL_LINES),
            MouseEventKind::Down(MouseButton::Left) => {
                let (line, column) = self.cell_at(mouse.column, mouse.row);
                self.session.select_start(line, column);
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let (line, column) = self.cell_at(mouse.column, mouse.row);
                self.session.select_update(line, column);
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some(text) = self.session.selected_text() {
                    if !text.is_empty() {
                        let _ = copy_text(&text);
                    }
                }
            }
            _ => {}
        }
    }

    fn cell_at(&self, column: u16, row: u16) -> (i32, usize) {
        let snap = self.session.screen_content();
        let column = (column as usize).min(snap.columns.saturating_sub(1));
        let line = viewport_to_line(row as usize, snap.rows, snap.display_offset);
        (line, column)
    }

    fn on_modal_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let picker = self.modal == Some(Modal::Picker);
        match key.code {
            KeyCode::Esc => {
                self.modal = None;
                self.input.clear();
                self.modal_error = None;
            }
            KeyCode::Enter => self.confirm_modal(),
            KeyCode::Backspace if !ctrl => {
                self.input.pop();
                self.filter_changed();
            }
            KeyCode::Up if picker => self.move_picker(-1),
            KeyCode::Down if picker => self.move_picker(1),
            KeyCode::Char(ch) if !ctrl && !alt => {
                self.input.push(ch);
                self.filter_changed();
            }
            _ => {}
        }
    }

    /// The input text changed. In the picker it is the filter, so the
    /// selection goes back to the top of the new list.
    fn filter_changed(&mut self) {
        if self.modal == Some(Modal::Picker) {
            self.picker_index = 0;
        }
    }

    fn run(&mut self, command: Command) {
        match command {
            Command::Connect => self.connect_command(),
            Command::Disconnect => {
                self.disconnect();
                self.session.set_message(Message::info("切断した"));
            }
            Command::Reconnect => self.reconnect(),
            Command::Break => self.send_input(Input::Break { hold: BREAK_HOLD }),
            Command::LogStart => self.ask_log(),
            Command::LogStop => self.stop_log(),
            Command::SendString => self.open_text(Modal::SendString, String::new()),
            Command::SendFile => self.open_text(Modal::SendFile, String::new()),
            Command::Baud => {
                self.open_text(Modal::Baud, self.session.params().baud_rate.to_string())
            }
            Command::Encoding => {
                self.open_text(Modal::Encoding, self.session.encoding().token().into())
            }
            Command::Help => {
                self.help = true;
                self.help_scroll = 0;
            }
            Command::Quit => self.quit = true,
            Command::Paste => self.paste_clipboard(),
            Command::Cancel => {
                self.prefix_armed = false;
                self.help = false;
            }
        }
    }

    fn connect_command(&mut self) {
        if matches!(
            self.session.link(),
            Link::Open { .. } | Link::Opening { .. }
        ) {
            self.session
                .set_message(Message::error(OpenError::AlreadyOpen.to_string()));
            return;
        }
        if self.session.params().path.is_empty() {
            self.open_picker();
        } else {
            let params = self.session.params().clone();
            self.start_connect(params);
        }
    }

    fn reconnect(&mut self) {
        if self.session.params().path.is_empty() {
            self.open_picker();
            return;
        }
        let params = self.session.params().clone();
        self.disconnect();
        self.start_connect(params);
    }

    fn disconnect(&mut self) {
        self.session.disconnect();
        self.events = None;
    }

    fn ask_log(&mut self) {
        if matches!(self.session.log_state(), LogSession::Running(_)) {
            self.session
                .set_message(Message::error("すでにログ中。先にログを停止"));
            return;
        }
        let dir = log_dir_from_env(&self.settings.log.directory);
        let path = suggested_log_path(
            &dir,
            &self.settings.log.name_template,
            &self.session.params().path,
        );
        self.open_text(Modal::LogPath, path.display().to_string());
    }

    fn stop_log(&mut self) {
        match self.session.log(LogCommand::Stop) {
            Ok(()) => self.session.set_message(Message::info("ログを停止した")),
            Err(error) => self.session.set_message(Message::error(error.to_string())),
        }
    }

    fn paste_clipboard(&mut self) {
        match clipboard_text() {
            Some(text) => self.send_input(Input::Text(text)),
            None => self
                .session
                .set_message(Message::error("クリップボードを読めない")),
        }
    }

    fn open_picker(&mut self) {
        match list_ports() {
            Ok(ports) => {
                self.ports = ports.into_iter().map(|port| port.path).collect();
                self.port_note = None;
            }
            Err(error) => {
                self.ports.clear();
                self.port_note = Some(error.to_string());
            }
        }
        self.picker_index = 0;
        self.open_text(Modal::Picker, String::new());
    }

    fn open_text(&mut self, modal: Modal, initial: String) {
        self.modal = Some(modal);
        self.input = initial;
        self.modal_error = None;
        self.prefix_armed = false;
        self.help = false;
    }

    fn confirm_modal(&mut self) {
        let Some(modal) = self.modal else {
            return;
        };
        let input = self.input.clone();
        match modal {
            Modal::Picker => self.confirm_picker(&input),
            Modal::SendString => {
                self.close_modal();
                self.send_input(Input::Text(format!("{input}\n")));
            }
            Modal::SendFile => {
                if input.is_empty() {
                    self.modal_error = Some("パスが空".into());
                    return;
                }
                self.close_modal();
                self.send_file(&input);
            }
            Modal::LogPath => {
                if input.is_empty() {
                    self.modal_error = Some("パスが空".into());
                    return;
                }
                self.close_modal();
                let message = self.start_log(PathBuf::from(input));
                self.session.set_message(message);
            }
            Modal::Baud => match cli::parse_baud(&input) {
                Ok(baud) => {
                    self.close_modal();
                    self.set_baud(baud);
                }
                Err(error) => self.modal_error = Some(error),
            },
            Modal::Encoding => match cli::parse_encoding(&input) {
                Ok(encoding) => {
                    self.close_modal();
                    self.session.set_encoding(encoding);
                    self.session
                        .set_message(Message::info("エンコーディングを変えた"));
                }
                Err(error) => self.modal_error = Some(error),
            },
        }
    }

    fn confirm_picker(&mut self, input: &str) {
        let path = if !input.is_empty() {
            input.to_string()
        } else {
            let paths = self.filtered_ports();
            let Some(path) = paths
                .get(self.picker_index.min(paths.len().saturating_sub(1)))
                .cloned()
            else {
                // Nothing listed and nothing typed: nothing to do (§6).
                return;
            };
            path
        };
        self.close_modal();
        self.connect_path(path);
    }

    fn close_modal(&mut self) {
        self.modal = None;
        self.input.clear();
        self.modal_error = None;
    }

    fn connect_path(&mut self, path: String) {
        let mut params = self.session.params().clone();
        params.path = path;
        if let Err(error) = self.session.set_serial_params(params.clone()) {
            self.session.set_message(Message::error(error.to_string()));
            return;
        }
        self.start_connect(params);
    }

    fn start_connect(&mut self, params: SerialParams) {
        let pending = match self.session.begin_connect(params) {
            Ok(pending) => pending,
            Err(error) => {
                self.session.set_message(Message::error(error.to_string()));
                return;
            }
        };
        let (tx, rx) = mpsc::channel();
        match thread::Builder::new()
            .name("rs-term-open".into())
            .spawn(move || {
                let _ = tx.send(pending.open());
            }) {
            Ok(_) => self.opening = Some(rx),
            Err(error) => self.session.set_message(Message::error(format!(
                "接続スレッドを作れない（OS: {error}）"
            ))),
        }
    }

    fn poll_open(&mut self) {
        let ready = match self.opening.as_ref() {
            Some(rx) => rx.try_recv(),
            None => return,
        };
        match ready {
            Ok(opened) => {
                self.opening = None;
                let keep = keep_startup(self.session.note(), self.startup.as_ref());
                match self.session.finish_connect(opened) {
                    Ok(events) => {
                        self.events = Some(events);
                        if let Some(message) = keep {
                            self.session.set_message(message);
                        }
                    }
                    Err(OpenError::Cancelled) => {}
                    Err(error) => {
                        if self.session.note().is_none() {
                            self.session.set_message(Message::error(error.to_string()));
                        }
                    }
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.opening = None;
                self.session
                    .set_message(Message::error("接続スレッドが終了した"));
            }
        }
    }

    fn set_baud(&mut self, baud: u32) {
        let mut params = self.session.params().clone();
        params.baud_rate = baud;
        match self.session.set_serial_params(params) {
            Ok(()) => self
                .session
                .set_message(Message::info(baud_changed_text(baud))),
            Err(error) => self.session.set_message(Message::error(error.to_string())),
        }
    }

    /// Starts the text log and reports the outcome; the caller shows it.
    fn start_log(&mut self, path: PathBuf) -> Message {
        let result = self.session.log(LogCommand::Start(LogTarget::Text {
            path,
            append: false,
            rotate: rs_term_core::Rotate {
                max_bytes: self.settings.log.rotate_bytes,
                keep: self.settings.log.rotate_keep,
            },
            encoding: LogEncoding::Utf8,
            plain: true,
            bom: false,
            timestamp: None,
        }));
        match result {
            Ok(()) => Message::info("ログを開始した"),
            Err(error) => Message::error(error.to_string()),
        }
    }

    fn send_file(&mut self, path: &str) {
        match read_send_file(Path::new(path)) {
            Ok(bytes) => self.send_input(Input::Bytes(bytes)),
            Err(message) => self.session.set_message(Message::error(message)),
        }
    }

    fn send_input(&mut self, input: Input) {
        if let Err(error) = self.session.apply_input(input) {
            self.session.set_message(Message::error(error.to_string()));
        }
    }

    fn filtered_ports(&self) -> Vec<String> {
        let query = self.input.to_ascii_lowercase();
        self.ports
            .iter()
            .filter(|path| query.is_empty() || path.to_ascii_lowercase().contains(&query))
            .cloned()
            .collect()
    }

    fn move_picker(&mut self, delta: isize) {
        let len = self.filtered_ports().len();
        if len == 0 {
            self.picker_index = 0;
            return;
        }
        let current = self.picker_index.min(len - 1) as isize;
        let next = (current + delta).clamp(0, len as isize - 1);
        self.picker_index = next as usize;
    }
}

/// The startup messages as one: texts joined, colored by the most severe.
fn combine_startup(messages: Vec<Message>) -> Option<Message> {
    let rank = |severity: Severity| match severity {
        Severity::Info => 0,
        Severity::Warn => 1,
        Severity::Error => 2,
    };
    let severity = messages
        .iter()
        .map(|message| message.severity)
        .max_by_key(|severity| rank(*severity))?;
    let text: Vec<&str> = messages
        .iter()
        .map(|message| message.text.as_str())
        .collect();
    Some(Message {
        severity,
        text: text.join("  "),
    })
}

/// A successful connect clears the note. The startup message survives it
/// only while it is still the one on screen (no later action replaced it).
fn keep_startup(current: Option<&Message>, startup: Option<&Message>) -> Option<Message> {
    match (current, startup) {
        (Some(current), Some(startup)) if current == startup => Some(startup.clone()),
        _ => None,
    }
}

fn load_settings() -> (Settings, Option<String>) {
    let Some(path) = settings_path_from_env() else {
        return (Settings::default(), None);
    };
    if !path.exists() {
        return (Settings::default(), None);
    }
    match load_settings_file(&path) {
        Ok(settings) => (settings, None),
        Err(error) => (Settings::default(), Some(error)),
    }
}

fn params_from(settings: &Settings, cli: &Cli, path: String) -> SerialParams {
    SerialParams {
        path,
        baud_rate: cli.baud.unwrap_or(settings.serial.baud_rate),
        data_bits: cli.data_bits.unwrap_or(settings.serial.data_bits),
        parity: cli.parity.unwrap_or(settings.serial.parity),
        stop_bits: cli.stop_bits.unwrap_or(settings.serial.stop_bits),
        flow: cli.flow.unwrap_or(settings.serial.flow),
        read_timeout: Duration::from_millis(settings.serial.read_timeout_ms),
    }
}

/// Reads a file to send. Only a regular file of at most `MAX_PENDING` bytes
/// is accepted: FIFOs and devices report a length of 0 and never end, and a
/// file can grow after the length check, so the read itself is capped too.
fn read_send_file(path: &Path) -> Result<Vec<u8>, String> {
    let read_error = |error: std::io::Error| format!("ファイルを読めない（OS: {error}）");
    let meta = std::fs::metadata(path).map_err(read_error)?;
    if !meta.is_file() {
        return Err("通常のファイルではないので送らなかった".into());
    }
    if meta.len() > MAX_PENDING as u64 {
        return Err(too_large(meta.len() as usize));
    }
    let file = std::fs::File::open(path).map_err(read_error)?;
    read_at_most(file, MAX_PENDING)?.ok_or_else(|| too_large(MAX_PENDING + 1))
}

/// Everything `reader` yields, or `None` if it yields more than `limit` bytes.
fn read_at_most(reader: impl Read, limit: usize) -> Result<Option<Vec<u8>>, String> {
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("ファイルを読めない（OS: {error}）"))?;
    Ok((bytes.len() <= limit).then_some(bytes))
}

fn too_large(bytes: usize) -> String {
    SessionError::TooLarge {
        bytes,
        limit: MAX_PENDING,
    }
    .to_string()
}

fn baud_changed_text(baud: u32) -> String {
    format!("ボーレートを {baud} にした")
}

fn frame_text(params: &SerialParams) -> String {
    let mut text = format!(
        "{} {}{}{}",
        params.baud_rate,
        data_label(params.data_bits),
        parity_label(params.parity),
        stop_label(params.stop_bits)
    );
    match params.flow {
        Flow::None => {}
        Flow::XonXoff => text.push_str(" XON/XOFF"),
        Flow::RtsCts => text.push_str(" RTS/CTS"),
    }
    text
}

fn data_label(bits: DataBits) -> &'static str {
    match bits {
        DataBits::Five => "5",
        DataBits::Six => "6",
        DataBits::Seven => "7",
        DataBits::Eight => "8",
    }
}

fn parity_label(parity: Parity) -> &'static str {
    match parity {
        Parity::None => "N",
        Parity::Odd => "O",
        Parity::Even => "E",
    }
}

fn stop_label(stop: StopBits) -> &'static str {
    match stop {
        StopBits::One => "1",
        StopBits::Two => "2",
    }
}

fn link_state(link: &Link) -> LinkState {
    match link {
        Link::Opening { .. } => LinkState::Opening,
        Link::Open { .. } => LinkState::Open,
        Link::Lost { .. } => LinkState::Lost,
        Link::Disconnected => LinkState::Disconnected,
    }
}

fn log_word(log: &LogSession) -> String {
    match log {
        LogSession::Stopped => "LOG off".into(),
        LogSession::Running(LogTarget::Text { .. }) => "LOG on".into(),
        LogSession::Running(LogTarget::Binary { .. }) => "LOG bin".into(),
        LogSession::Failed { .. } => "LOG 失敗".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rs-term-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn send_file_reads_a_regular_file() {
        let dir = temp_dir("send-ok");
        let path = dir.join("a.bin");
        std::fs::write(&path, b"hello\0").unwrap();
        assert_eq!(read_send_file(&path).unwrap(), b"hello\0");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn send_file_rejects_a_directory_and_a_missing_path() {
        let dir = temp_dir("send-dir");
        let error = read_send_file(&dir).unwrap_err();
        assert!(error.contains("通常のファイルではない"), "{error}");
        let error = read_send_file(&dir.join("none")).unwrap_err();
        assert!(error.contains("ファイルを読めない"), "{error}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn send_file_rejects_an_oversized_file_before_reading() {
        let dir = temp_dir("send-big");
        let path = dir.join("big.bin");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_PENDING as u64 + 1).unwrap();
        drop(file);
        let error = read_send_file(&path).unwrap_err();
        assert!(error.contains("上限 16 MiB"), "{error}");
        assert!(error.contains(&(MAX_PENDING + 1).to_string()), "{error}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_read_stops_at_the_limit_even_for_an_endless_source() {
        assert_eq!(read_at_most(&b"abc"[..], 3).unwrap(), Some(b"abc".to_vec()));
        assert_eq!(read_at_most(&b"abcd"[..], 3).unwrap(), None);
        assert_eq!(read_at_most(std::io::repeat(0), 1000).unwrap(), None);
    }

    #[test]
    fn baud_change_has_a_confirmation_text() {
        assert_eq!(baud_changed_text(9600), "ボーレートを 9600 にした");
    }

    #[test]
    fn startup_messages_are_joined_with_the_highest_severity() {
        let combined = combine_startup(vec![
            Message::info("ログを開始した"),
            Message::warn("注意の理由"),
            Message::error("壊れている"),
        ])
        .unwrap();
        assert_eq!(combined.severity, Severity::Error);
        assert_eq!(combined.text, "ログを開始した  注意の理由  壊れている");
        assert!(combine_startup(Vec::new()).is_none());
        let only_info = combine_startup(vec![Message::info("ログを開始した")]).unwrap();
        assert_eq!(only_info.severity, Severity::Info);
    }

    #[test]
    fn a_connect_keeps_the_startup_message_unless_something_replaced_it() {
        let startup = Message::error("設定が壊れている");
        assert_eq!(
            keep_startup(Some(&startup), Some(&startup)),
            Some(startup.clone())
        );
        let open_error = Message::error("COM1 を開けない");
        assert_eq!(keep_startup(Some(&open_error), Some(&startup)), None);
        assert_eq!(keep_startup(None, Some(&startup)), None);
        assert_eq!(keep_startup(Some(&startup), None), None);
    }

    #[test]
    fn startup_reasons_all_reach_the_status_line() {
        let dir = std::env::temp_dir();
        let cli = Cli {
            log: Some(dir),
            ..Cli::default()
        };
        let settings = Settings {
            warnings: vec!["未知の項目".into()],
            ..Settings::default()
        };
        let app = App::with_settings(cli, settings, Some("settings.toml が壊れている".into()));
        let (severity, text) = app.status_view().message.unwrap();
        assert_eq!(severity, Severity::Error);
        assert!(text.contains("settings.toml が壊れている"), "{text}");
        assert!(!text.contains("ログを開始した"), "{text}");
        assert_eq!(Some(&Message { severity, text }), app.startup.as_ref());
    }

    /// The only test that touches the process-wide termination flag.
    #[test]
    fn a_termination_signal_makes_the_quit_check_true() {
        let app = App::with_settings(Cli::default(), Settings::default(), None);
        assert!(!app.should_quit());
        crate::signal::request_for_test(true);
        assert!(app.should_quit());
        crate::signal::request_for_test(false);
        assert!(!app.should_quit());
        #[cfg(unix)]
        {
            crate::signal::install().unwrap();
            // SAFETY: raising a signal whose handler was just installed.
            assert_eq!(unsafe { libc::raise(libc::SIGTERM) }, 0);
            assert!(app.should_quit());
            assert_eq!(unsafe { libc::raise(libc::SIGHUP) }, 0);
            crate::signal::request_for_test(false);
            assert!(!app.should_quit());
        }
    }

    fn picker_app(ports: usize) -> App {
        let mut app = App::with_settings(Cli::default(), Settings::default(), None);
        app.ports = (0..ports).map(|i| format!("/dev/ttyS{i}")).collect();
        app.port_note = None;
        app.picker_index = 0;
        app.input.clear();
        app.modal = Some(Modal::Picker);
        app
    }

    fn press(app: &mut App, code: KeyCode) {
        app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    #[test]
    fn picker_enter_with_nothing_listed_and_nothing_typed_does_nothing() {
        let mut app = picker_app(0);
        app.session.clear_note();
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.modal, Some(Modal::Picker));
        assert!(app.modal_error.is_none());
        assert!(app.session.note().is_none());
        assert!(matches!(app.session.link(), Link::Disconnected));
    }

    #[test]
    fn picker_selection_returns_to_the_top_when_the_filter_changes() {
        let mut app = picker_app(30);
        for _ in 0..7 {
            press(&mut app, KeyCode::Down);
        }
        assert_eq!(app.picker_index, 7);
        press(&mut app, KeyCode::Char('S'));
        assert_eq!(app.picker_index, 0);
        for _ in 0..3 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.picker_index, 0);
        app.picker_index = 4;
        app.on_paste("ttyS1".into());
        assert_eq!(app.picker_index, 0);
        // The index is also clamped to a shrunken list.
        app.picker_index = 99;
        let selected = app
            .modal_body()
            .unwrap()
            .lines
            .iter()
            .filter(|(_, selected)| *selected)
            .count();
        assert_eq!(selected, 1);
    }

    #[test]
    fn the_default_log_name_without_a_port_has_a_placeholder() {
        let mut app = picker_app(0);
        app.close_modal();
        assert!(app.session.params().path.is_empty());
        app.ask_log();
        assert_eq!(app.modal, Some(Modal::LogPath));
        let name = Path::new(&app.input)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(name.starts_with("rs-term_"), "{name}");
    }
}
