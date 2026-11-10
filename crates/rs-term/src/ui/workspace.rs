//! One window. Every operation is a palette action.
//!
//! Pane geometry follows herdr: side-by-side and stacked splits, directional
//! focus, swap, zoom, and a resize mode. The dividers are `h_resizable` and
//! `v_resizable`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use chrono::Local;
use gpui::{
    AnyElement, App, AppContext, Bounds, ClipboardItem, ContentMask, Context, Entity, EventEmitter,
    FocusHandle, Focusable, FontFallbacks, FontFeatures, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement,
    Pixels, Point, Render, ScrollDelta, ScrollEvent,
    SharedString, Styled, Task, TextAlign, TextRun, WeakEntity, Window, canvas, div, fill, font,
    px, size,
};
use gpui::prelude::FluentBuilder;
use gpui_kit::component::command::{Command, CommandItem, CommandState};
use gpui_kit::component::resizable::{ResizableState, h_resizable, resizable_panel, v_resizable};
use rs_term_core::{
    DataBits, EventRx, Flow, Input, IoEvent, KeyInput, LayoutNode, Link, LogCommand, LogEncoding,
    LogSession, LogTarget, PaneSettings, Parity, PortInfo, ReceiveNewline,
    Rotate, ScreenConfig, SerialParams, Session, SessionConfig, SessionEncoding, Settings,
    StopBits, Timestamp, TransmitNewline, UsbIdentity, expand_log_name, palette_by_name,
    port_token, save_settings_file, validate_log_name,
    windows_log_dir, Axis as SettingsAxis,
};

use super::actions;
use super::color::{self, resolve};
use super::layout::{Axis, Direction, Layout, Rect, ViewNode};

const BAUD_RATES: &[u32] = &[9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600];
const AREA: Rect = Rect { x: 0, y: 0, width: 1000, height: 800 };

pub struct Workspace {
    settings: Settings,
    settings_path: Option<PathBuf>,
    save_allowed: bool,
    layout: Layout,
    panes: HashMap<u64, Pane>,
    page: Page,
    rendered_page: Page,
    palette: bool,
    command: Entity<CommandState>,
    focus: FocusHandle,
    focused_once: bool,
    ports: Result<Vec<PortInfo>, String>,
    port_token: u64,
    port_task: Option<Task<()>>,
    splits: HashMap<String, Entity<ResizableState>>,
    restoring: bool,
    sync_splits: bool,
    selecting: Option<u64>,
    wizard: bool,
    terminal_font: Option<String>,
    cell_px: f32,
    line_px: f32,
    grid_bounds: Rc<RefCell<HashMap<u64, Bounds<Pixels>>>>,
    status: String,
    draft: Draft,
}

struct Pane {
    session: Session,
    _io: Option<Task<()>>,
}

#[derive(Clone, PartialEq)]
enum Page {
    Root,
    Ports,
    Baud,
    Data,
    Parity,
    Stop,
    ConnectFlow,
    ConnectDtr,
    FramingData,
    FramingParity,
    FramingStop,
    Flow,
    Dtr,
    Rts,
    Encoding,
    Receive,
    Transmit,
    Echo,
    SendText,
    FontTerminal,
    FontSize,
    FontUi,
    LogName { binary: bool },
    LogMode { binary: bool, name: String },
    LogPlain { name: String, append: bool },
    LogStamp { name: String, append: bool, plain: bool },
}

#[derive(Clone)]
struct Draft {
    path: String,
    usb: Option<UsbIdentity>,
    baud: u32,
    data: DataBits,
    parity: Parity,
    stop: StopBits,
    flow: Flow,
    dtr: Option<bool>,
}

impl EventEmitter<()> for Workspace {}

impl Focusable for Workspace {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (settings, path, save_allowed, mut status) = load_settings();
        apply_theme(&settings, window, cx);
        let (layout, mut panes) = sessions_from_settings(&settings);
        if let Some(pane) = panes.get_mut(&layout.focus()) {
            pane.session.apply_rx(b"hello\r\n");
        }
        window.set_window_title("rs-term");
        let names = cx.text_system().all_font_names();
        let terminal_font = color::resolve_font(&names, &settings.terminal.font_family).map(str::to_string);
        if terminal_font.is_none() {
            status = format!("{status} 端末フォントが無い");
        }
        for warning in &settings.warnings {
            status = format!("{status} {warning}");
        }
        status = format!("{status} {}", load_keymap(cx));
        let mut workspace = Self {
            draft: Draft {
                path: String::new(),
                usb: None,
                baud: settings.serial.baud_rate,
                data: settings.serial.data_bits,
                parity: settings.serial.parity,
                stop: settings.serial.stop_bits,
                flow: settings.serial.flow,
                dtr: None,
            },
            settings,
            settings_path: path,
            save_allowed,
            layout,
            panes,
            page: Page::Root,
            rendered_page: Page::Root,
            palette: false,
            command: cx.new(|cx| CommandState::new(window, cx)),
            focus: cx.focus_handle(),
            focused_once: false,
            ports: Ok(Vec::new()),
            port_token: 0,
            port_task: None,
            splits: HashMap::new(),
            restoring: false,
            sync_splits: false,
            selecting: None,
            wizard: false,
            terminal_font,
            cell_px: 8.0,
            line_px: 18.0,
            grid_bounds: Rc::new(RefCell::new(HashMap::new())),
            status,
        };
        if workspace.settings.serial.reopen_on_start {
            workspace.reopen_saved(cx);
        }
        workspace
    }

    fn focused_session(&mut self) -> Option<&mut Session> {
        let id = self.layout.focus();
        self.panes.get_mut(&id).map(|pane| &mut pane.session)
    }

    fn open_palette(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        self.palette = true;
        if matches!(self.page, Page::Ports) {
            self.poll_ports(cx);
        }
        cx.notify();
    }

    fn poll_ports(&mut self, cx: &mut Context<Self>) {
        self.port_token = self.port_token.wrapping_add(1);
        let token = self.port_token;
        self.port_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            loop {
                let listed = cx
                    .background_executor()
                    .spawn(async { rs_term_core::list_ports() })
                    .await;
                let keep = this
                    .update(cx, |this, cx| {
                        if this.port_token != token || !matches!(this.page, Page::Ports) {
                            return false;
                        }
                        this.ports = listed.map_err(|error| error.to_string());
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
                cx.background_executor().timer(Duration::from_secs(1)).await;
            }
        }));
    }

    fn attach(&mut self, id: u64, mut rx: EventRx, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            while let Some(first) = rx.recv().await {
                let mut batch = vec![first];
                while batch.len() < 32 {
                    match rx.try_recv() {
                        Ok(event) => batch.push(event),
                        Err(_) => break,
                    }
                }
                if this
                    .update(cx, |this, cx| {
                        this.apply_batch(id, batch);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        if let Some(pane) = self.panes.get_mut(&id) {
            pane._io = Some(task);
        }
    }

    fn apply_batch(&mut self, id: u64, batch: Vec<IoEvent>) {
        let Some(pane) = self.panes.get_mut(&id) else {
            return;
        };
        for event in batch {
            match event {
                IoEvent::Data(bytes) => pane.session.apply_rx(&bytes),
                IoEvent::PortLost(error) => {
                    pane.session.note_port_lost(error);
                    pane.session.release_port();
                }
                IoEvent::CommandFailed(error) => pane.session.note_command_failed(error),
                IoEvent::Pins(pins) => pane.session.note_pins(pins),
                IoEvent::Dropped { bytes } => pane.session.note_dropped(bytes),
            }
        }
    }

    fn connect_focused(&mut self, cx: &mut Context<Self>) {
        let id = self.layout.focus();
        let params = SerialParams {
            path: self.draft.path.clone(),
            baud_rate: self.draft.baud,
            data_bits: self.draft.data,
            parity: self.draft.parity,
            stop_bits: self.draft.stop,
            flow: self.draft.flow,
            dtr_on_open: self.draft.dtr,
            rts: false,
            read_timeout: Duration::from_millis(self.settings.serial.read_timeout_ms),
        };
        let usb = self.draft.usb.clone();
        let Some(pane) = self.panes.get_mut(&id) else {
            return;
        };
        pane.session.set_usb_identity(usb);
        match pane.session.connect(params) {
            Ok(rx) => {
                self.status.clear();
                self.attach(id, rx, cx);
                self.palette = false;
                self.page = Page::Root;
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn persist(&mut self) {
        if !self.save_allowed {
            return;
        }
        let Some(path) = self.settings_path.clone() else {
            self.status = "APPDATA が空なので設定を書けない".into();
            return;
        };
        if let Err(error) = save_settings_file(&path, &self.settings) {
            self.save_allowed = false;
            self.status = error;
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focused_once {
            self.focused_once = true;
            window.focus(&self.focus, cx);
        }
        self.rendered_page = self.page.clone();
        let palette = self.palette;
        let items = self.items(cx);
        let tree = self.render_tree(window, cx);
        div()
            .id("rs-term")
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &actions::PaletteToggle, _, cx| {
                this.palette = !this.palette;
                if this.palette {
                    this.page = Page::Root;
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &actions::About, _, cx| {
                this.status = format!("rs-term {}", env!("CARGO_PKG_VERSION"));
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &actions::Connect, _, cx| {
                this.open_palette(Page::Ports, cx);
            }))
            .on_action(cx.listener(|this, _: &actions::Disconnect, _, cx| {
                if let Some(session) = this.focused_session() {
                    session.disconnect();
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &actions::Break, _, cx| {
                if let Some(session) = this.focused_session() {
                    let _ = session.apply_input(Input::Break { hold: Duration::from_millis(250) });
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &actions::SplitVertical, _, cx| {
                this.split(Axis::SideBySide, cx);
            }))
            .on_action(cx.listener(|this, _: &actions::SplitHorizontal, _, cx| {
                this.split(Axis::Stacked, cx);
            }))
            .on_action(cx.listener(|this, _: &actions::FocusLeft, _, cx| this.move_focus(Direction::Left, cx)))
            .on_action(cx.listener(|this, _: &actions::FocusDown, _, cx| this.move_focus(Direction::Down, cx)))
            .on_action(cx.listener(|this, _: &actions::FocusUp, _, cx| this.move_focus(Direction::Up, cx)))
            .on_action(cx.listener(|this, _: &actions::FocusRight, _, cx| this.move_focus(Direction::Right, cx)))
            .on_action(cx.listener(|this, _: &actions::SwapLeft, _, cx| this.swap(Direction::Left, cx)))
            .on_action(cx.listener(|this, _: &actions::SwapDown, _, cx| this.swap(Direction::Down, cx)))
            .on_action(cx.listener(|this, _: &actions::SwapUp, _, cx| this.swap(Direction::Up, cx)))
            .on_action(cx.listener(|this, _: &actions::SwapRight, _, cx| this.swap(Direction::Right, cx)))
            .on_action(cx.listener(|this, _: &actions::CycleNext, _, cx| {
                this.layout.cycle(true);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &actions::CyclePrevious, _, cx| {
                this.layout.cycle(false);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &actions::ClosePane, _, cx| this.close_pane(cx)))
            .on_action(cx.listener(|this, _: &actions::Zoom, _, cx| {
                this.layout.toggle_zoom();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &actions::ResizeMode, _, cx| {
                this.layout.toggle_resize_mode();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &actions::ResizeLeft, _, cx| this.resize(Direction::Left, cx)))
            .on_action(cx.listener(|this, _: &actions::ResizeDown, _, cx| this.resize(Direction::Down, cx)))
            .on_action(cx.listener(|this, _: &actions::ResizeUp, _, cx| this.resize(Direction::Up, cx)))
            .on_action(cx.listener(|this, _: &actions::ResizeRight, _, cx| this.resize(Direction::Right, cx)))
            .on_action(cx.listener(|this, _: &actions::ThemeDark, window, cx| this.set_theme("dark", window, cx)))
            .on_action(cx.listener(|this, _: &actions::ThemeLight, window, cx| this.set_theme("light", window, cx)))
            .on_action(cx.listener(|this, _: &actions::FontTerminal, _, cx| this.open_palette(Page::FontTerminal, cx)))
            .on_action(cx.listener(|this, _: &actions::FontSize, _, cx| this.open_palette(Page::FontSize, cx)))
            .on_action(cx.listener(|this, _: &actions::FontUi, _, cx| this.open_palette(Page::FontUi, cx)))
            .on_action(cx.listener(|this, _: &actions::Baud, _, cx| this.open_palette(Page::Baud, cx)))
            .on_action(cx.listener(|this, _: &actions::Framing, _, cx| this.open_palette(Page::FramingData, cx)))
            .on_action(cx.listener(|this, _: &actions::Flow, _, cx| this.open_palette(Page::Flow, cx)))
            .on_action(cx.listener(|this, _: &actions::Dtr, _, cx| this.open_palette(Page::Dtr, cx)))
            .on_action(cx.listener(|this, _: &actions::Rts, _, cx| this.open_palette(Page::Rts, cx)))
            .on_action(cx.listener(|this, _: &actions::Encoding, _, cx| this.open_palette(Page::Encoding, cx)))
            .on_action(cx.listener(|this, _: &actions::Newline, _, cx| this.open_palette(Page::Receive, cx)))
            .on_action(cx.listener(|this, _: &actions::LocalEcho, _, cx| this.open_palette(Page::Echo, cx)))
            .on_action(cx.listener(|this, _: &actions::SendText, _, cx| this.open_palette(Page::SendText, cx)))
            .on_action(cx.listener(|this, _: &actions::LogBinary, _, cx| this.open_palette(Page::LogName { binary: true }, cx)))
            .on_action(cx.listener(|this, _: &actions::LogText, _, cx| this.open_palette(Page::LogName { binary: false }, cx)))
            .on_action(cx.listener(|this, _: &actions::LogPause, _, cx| this.log_simple(LogCommand::Pause, cx)))
            .on_action(cx.listener(|this, _: &actions::LogResume, _, cx| this.log_simple(LogCommand::Resume, cx)))
            .on_action(cx.listener(|this, _: &actions::LogStop, _, cx| this.log_simple(LogCommand::Stop, cx)))
            .on_action(cx.listener(|this, _: &actions::Copy, _, cx| this.copy(cx)))
            .on_action(cx.listener(|this, _: &actions::Paste, _, cx| this.paste(cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.on_key(event, cx)))
            .child(tree)
            .when(palette, |el| {
                let command = self.command.clone();
                let weak = cx.entity().downgrade();
                let confirm = weak.clone();
                el.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(div().w(px(560.0)).child(
                            Command::new(&command)
                                .items(items)
                                .placeholder("コマンド")
                                .on_cancel(move |_window, cx| {
                                    let _ = weak.update(cx, |this, cx| this.cancel_page(cx));
                                })
                                .on_confirm(move |path, _window, cx| {
                                    let _ = confirm.update(cx, |this, cx| {
                                        if !matches!(this.rendered_page, Page::Root) {
                                            this.confirm_row(path.row, cx);
                                        }
                                    });
                                }),
                        )),
                )
            })
    }
}

include!("workspace_methods.rs");
