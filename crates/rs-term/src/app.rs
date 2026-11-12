//! One serial session and the modal commands around it.

use std::path::PathBuf;
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
    OpenedPort, Parity, ScreenConfig, SerialParams, Session, SessionConfig, SessionEncoding,
    Settings, StopBits, TerminalPalette, MAX_PENDING,
};

use crate::cli::{self, Cli};
use crate::copy::{clipboard_text, copy_text};
use crate::keys::{self, Action, Command, Focus, Prefix};
use crate::status::StatusView;

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
    modal: Option<Modal>,
    input: String,
    modal_error: Option<String>,
    ports: Vec<String>,
    port_note: Option<String>,
    picker_index: usize,
    quit: bool,
    size: (u16, u16),
    last_tick: Instant,
}

impl App {
    pub fn new(cli: Cli) -> Self {
        let (settings, load_note) = load_settings();
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
        if let Some(note) = load_note {
            session.set_message(Message::error(note));
        } else if !settings.warnings.is_empty() {
            session.set_message(Message::warn(settings.warnings.join(" ")));
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
            modal: None,
            input: String::new(),
            modal_error: None,
            ports: Vec::new(),
            port_note: None,
            picker_index: 0,
            quit: false,
            size: (80, 24),
            last_tick: Instant::now(),
        };
        if let Some(path) = cli.log {
            app.start_log(path);
        }
        if params.path.is_empty() {
            app.open_picker();
        } else {
            app.start_connect(params);
        }
        app
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn show_help(&self) -> bool {
        self.help
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
            encoding: match self.session.encoding() {
                SessionEncoding::ShiftJis => "Shift_JIS".into(),
                SessionEncoding::Utf8 => "UTF-8".into(),
            },
            link: link_word(self.session.link()),
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
        Some(ModalBody { title, lines })
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
            }
            KeyCode::Up if picker => self.move_picker(-1),
            KeyCode::Down if picker => self.move_picker(1),
            KeyCode::Char(ch) if !ctrl && !alt => self.input.push(ch),
            _ => {}
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
            Command::Encoding => self.open_text(
                Modal::Encoding,
                encoding_token(self.session.encoding()).into(),
            ),
            Command::Help => self.help = true,
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
        if matches!(
            self.session.log_state(),
            LogSession::Running(_) | LogSession::Paused(_)
        ) {
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
                self.start_log(PathBuf::from(input));
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
                self.modal_error = Some("ポートを選ぶか、パスを入力する".into());
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
                match self.session.finish_connect(opened) {
                    Ok(events) => self.events = Some(events),
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
        if let Err(error) = self.session.set_serial_params(params) {
            self.session.set_message(Message::error(error.to_string()));
        }
    }

    fn start_log(&mut self, path: PathBuf) {
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
            Ok(()) => self.session.set_message(Message::info("ログを開始した")),
            Err(error) => self.session.set_message(Message::error(error.to_string())),
        }
    }

    fn send_file(&mut self, path: &str) {
        let meta = match std::fs::metadata(path) {
            Ok(meta) => meta,
            Err(error) => {
                self.session
                    .set_message(Message::error(format!("ファイルを読めない（OS: {error}）")));
                return;
            }
        };
        if meta.len() > MAX_PENDING as u64 {
            self.session.set_message(Message::error(format!(
                "{} バイトは上限 {} MiB を超えるので送らなかった",
                meta.len(),
                MAX_PENDING / (1024 * 1024)
            )));
            return;
        }
        match std::fs::read(path) {
            Ok(bytes) => self.send_input(Input::Bytes(bytes)),
            Err(error) => self
                .session
                .set_message(Message::error(format!("ファイルを読めない（OS: {error}）"))),
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
        dtr_on_open: None,
        rts: false,
        read_timeout: Duration::from_millis(settings.serial.read_timeout_ms),
    }
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

fn link_word(link: &Link) -> String {
    match link {
        Link::Opening { .. } => "接続中".into(),
        Link::Open { .. } => "接続".into(),
        Link::Lost { .. } => "喪失".into(),
        Link::Disconnected => "未接続".into(),
    }
}

fn log_word(log: &LogSession) -> String {
    match log {
        LogSession::Stopped => "LOG off".into(),
        LogSession::Running(LogTarget::Text { .. })
        | LogSession::Paused(LogTarget::Text { .. }) => {
            if matches!(log, LogSession::Paused(_)) {
                "LOG 一時停止".into()
            } else {
                "LOG on".into()
            }
        }
        LogSession::Running(LogTarget::Binary { .. }) => "LOG bin".into(),
        LogSession::Paused(LogTarget::Binary { .. }) => "LOG 一時停止".into(),
        LogSession::Failed { .. } => "LOG 失敗".into(),
    }
}

fn encoding_token(encoding: SessionEncoding) -> &'static str {
    match encoding {
        SessionEncoding::ShiftJis => "sjis",
        SessionEncoding::Utf8 => "utf8",
    }
}
