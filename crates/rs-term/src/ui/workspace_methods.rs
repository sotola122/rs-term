impl Workspace {
    fn split(&mut self, axis: Axis, cx: &mut Context<Self>) {
        let source = self.layout.focus();
        let id = self.layout.split(axis);
        let session = self.fresh_session(source);
        self.panes.insert(id, Pane { session, _io: None });
        self.remember_layout();
        self.persist();
        cx.notify();
    }

    fn fresh_session(&self, source: u64) -> Session {
        let source = self.panes.get(&source);
        let (encoding, newline, local_echo, mut params) = if let Some(pane) = source {
            (
                pane.session.encoding(),
                pane.session.newline(),
                pane.session.local_echo(),
                pane.session.params().clone(),
            )
        } else {
            (
                self.settings.encoding,
                self.settings.newline,
                self.settings.terminal.local_echo,
                params_from_settings(&self.settings, String::new()),
            )
        };
        params.path.clear();
        let mut session = Session::new(SessionConfig {
            screen: ScreenConfig {
                columns: self.settings.terminal.columns,
                rows: self.settings.terminal.rows,
                scrollback: self.settings.terminal.scrollback,
            },
            encoding,
            newline,
            local_echo,
        });
        session.set_reconnect_enabled(self.settings.serial.reconnect);
        let _ = session.set_serial_params(params);
        session
    }

    fn move_focus(&mut self, direction: Direction, cx: &mut Context<Self>) {
        self.layout.focus_in(direction, AREA);
        cx.notify();
    }

    fn swap(&mut self, direction: Direction, cx: &mut Context<Self>) {
        self.layout.swap(direction, AREA);
        self.remember_layout();
        self.persist();
        cx.notify();
    }

    fn close_pane(&mut self, cx: &mut Context<Self>) {
        if self.layout.close_focused() {
            let live = self.layout.panes();
            self.panes.retain(|id, _| live.contains(id));
            self.remember_layout();
            self.persist();
        } else if let Some(session) = self.focused_session() {
            session.disconnect();
        }
        cx.notify();
    }

    fn resize(&mut self, direction: Direction, cx: &mut Context<Self>) {
        self.layout.resize_focused(direction, AREA);
        self.sync_splits = true;
        self.remember_layout();
        self.persist();
        cx.notify();
    }

    fn set_theme(&mut self, mode: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.ui.theme_mode = mode.to_string();
        apply_theme(&self.settings, window, cx);
        self.persist();
        cx.notify();
    }

    fn log_simple(&mut self, command: LogCommand, cx: &mut Context<Self>) {
        if let Some(session) = self.focused_session() {
            if let Err(error) = session.log(command) {
                self.set_status(error.to_string());
            }
        }
        cx.notify();
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        let text = self
            .focused_session()
            .and_then(|session| session.selected_text());
        if let Some(text) = text {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        cx.notify();
    }

    fn paste(&mut self, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        let Some(text) = item.text() else {
            return;
        };
        if let Some(session) = self.focused_session() {
            if let Err(error) = session.apply_input(Input::Text(text)) {
                self.set_status(error.to_string());
            }
        }
        cx.notify();
    }

    fn on_key(&mut self, event: &gpui::KeyDownEvent, cx: &mut Context<Self>) {
        if self.palette {
            return;
        }
        let key = event.keystroke.key.as_str();
        let modifiers = &event.keystroke.modifiers;
        if self.resize {
            let direction = match key {
                "h" | "left" => Some(Direction::Left),
                "j" | "down" => Some(Direction::Down),
                "k" | "up" => Some(Direction::Up),
                "l" | "right" => Some(Direction::Right),
                _ => None,
            };
            if let Some(direction) = direction {
                if !modifiers.control && !modifiers.alt && !modifiers.platform {
                    self.resize(direction, cx);
                }
                return;
            }
            if matches!(key, "escape" | "enter") && !modifiers.control && !modifiers.alt {
                self.resize = false;
                cx.notify();
                return;
            }
            return;
        }
        if modifiers.platform || (modifiers.control && (modifiers.shift || modifiers.alt)) {
            return;
        }
        let input = if modifiers.control {
            key.chars().next().filter(|ch| ch.is_ascii_alphabetic()).map(|ch| {
                KeyInput::Ctrl(ch.to_ascii_lowercase())
            })
        } else if modifiers.alt {
            event
                .keystroke
                .key_char
                .as_deref()
                .and_then(|text| text.chars().next())
                .map(KeyInput::Alt)
        } else if let Some(function) = function_key(key) {
            Some(KeyInput::Function(function))
        } else {
            match key {
                "enter" => Some(KeyInput::Enter),
                "backspace" => Some(KeyInput::Backspace),
                "delete" => Some(KeyInput::Delete),
                "tab" => Some(KeyInput::Tab),
                "escape" => Some(KeyInput::Escape),
                "up" => Some(KeyInput::Arrow(rs_term_core::Direction::Up)),
                "down" => Some(KeyInput::Arrow(rs_term_core::Direction::Down)),
                "left" => Some(KeyInput::Arrow(rs_term_core::Direction::Left)),
                "right" => Some(KeyInput::Arrow(rs_term_core::Direction::Right)),
                "home" => Some(KeyInput::Home),
                "end" => Some(KeyInput::End),
                _ => event
                    .keystroke
                    .key_char
                    .as_deref()
                    .filter(|text| text.chars().count() == 1)
                    .and_then(|text| text.chars().next())
                    .map(KeyInput::Char),
            }
        };
        let Some(input) = input else {
            return;
        };
        if let Some(session) = self.focused_session() {
            if let Err(error) = session.apply_input(Input::Key(input)) {
                self.set_status(error.to_string());
            }
        }
        cx.notify();
    }

    fn items(&self, cx: &App) -> Vec<CommandItem> {
        let query = self.command.read(cx).query(cx).to_string();
        match &self.page {
            Page::Root => root_items(),
            Page::Ports => match &self.ports {
                Ok(ports) if ports.is_empty() => vec![plain("ポートが無い", &[])],
                Ok(ports) => ports.iter().map(port_item).collect(),
                Err(error) => vec![plain(error, &[])],
            },
            Page::Baud => baud_items(&query),
            Page::Data | Page::FramingData => data_items(),
            Page::Parity | Page::FramingParity => parity_items(),
            Page::Stop | Page::FramingStop => stop_items(),
            Page::ConnectFlow | Page::Flow => flow_items(),
            Page::ConnectDtr | Page::Dtr => dtr_items(),
            Page::Rts => vec![plain("RTS オフ", &[]), plain("RTS オン", &[])],
            Page::Encoding => vec![
                plain("Shift_JIS", &["session.encoding"]),
                plain("UTF-8", &["session.encoding"]),
            ],
            Page::Receive => vec![
                plain("受信 CR", &[]),
                plain("受信 CR+LF", &[]),
                plain("受信 LF", &[]),
                plain("受信 自動", &[]),
            ],
            Page::Transmit => vec![
                plain("送信 CR", &[]),
                plain("送信 CR+LF", &[]),
                plain("送信 LF", &[]),
            ],
            Page::Echo => vec![plain("ローカルエコー オフ", &[]), plain("ローカルエコー オン", &[])],
            Page::SendText => vec![plain("この文字列を送る", &["session.send_text"])],
            Page::FontTerminal | Page::FontUi => cx
                .text_system()
                .all_font_names()
                .into_iter()
                .map(|name| plain(name, &["font"]))
                .collect(),
            Page::FontSize => font_size_items(&query),
            Page::LogName { .. } => vec![plain("この名前で始める", &["log"])],
            Page::LogMode { .. } => vec![plain("上書き", &[]), plain("追記", &[])],
            Page::LogPlain { .. } => vec![plain("制御文字を残す", &[]), plain("印字できる文字だけ", &[])],
            Page::LogStamp { .. } => vec![plain("時刻なし", &[]), plain("行頭に時刻", &[])],
        }
    }

    fn confirm_row(&mut self, row: usize, cx: &mut Context<Self>) {
        let query = self.command.read(cx).query(cx).to_string();
        match self.page.clone() {
            Page::Ports => self.pick_port(row, cx),
            Page::Baud => self.pick_baud(row, &query, cx),
            Page::Data => self.pick_data(row, true, cx),
            Page::FramingData => self.pick_data(row, false, cx),
            Page::Parity => self.pick_parity(row, true, cx),
            Page::FramingParity => self.pick_parity(row, false, cx),
            Page::Stop => self.pick_stop(row, true, cx),
            Page::FramingStop => self.pick_stop(row, false, cx),
            Page::ConnectFlow => self.pick_flow(row, true, cx),
            Page::Flow => self.pick_flow(row, false, cx),
            Page::ConnectDtr => self.pick_dtr(row, true, cx),
            Page::Dtr => self.pick_dtr(row, false, cx),
            Page::Rts => self.pick_rts(row, cx),
            Page::Encoding => self.pick_encoding(row, cx),
            Page::Receive => self.pick_receive(row, cx),
            Page::Transmit => self.pick_transmit(row, cx),
            Page::Echo => self.pick_echo(row, cx),
            Page::SendText => self.send_query(&query, cx),
            Page::FontTerminal => self.pick_terminal_font(row, cx),
            Page::FontSize => self.pick_font_size(row, &query, cx),
            Page::FontUi => self.pick_ui_font(row, cx),
            Page::LogName { binary } => self.pick_log_name(binary, &query, cx),
            Page::LogMode { binary, name } => {
                if binary {
                    self.start_log(name, row == 1, false, false, true, cx);
                } else {
                    self.page = Page::LogPlain { name, append: row == 1 };
                    cx.notify();
                }
            }
            Page::LogPlain { name, append } => {
                self.page = Page::LogStamp {
                    name,
                    append,
                    plain: row == 1,
                };
                cx.notify();
            }
            Page::LogStamp { name, append, plain } => self.start_log(name, append, plain, row == 1, false, cx),
            Page::Root => {}
        }
    }

    fn cancel_page(&mut self, cx: &mut Context<Self>) {
        let wizard = self.wizard;
        self.page = match &self.page {
            Page::Root => {
                self.palette = false;
                self.wizard = false;
                Page::Root
            }
            Page::Ports => {
                self.wizard = false;
                Page::Root
            }
            Page::Baud if wizard => Page::Ports,
            Page::Data => Page::Baud,
            Page::Parity => Page::Data,
            Page::Stop => Page::Parity,
            Page::ConnectFlow => Page::Stop,
            Page::ConnectDtr => Page::ConnectFlow,
            Page::FramingParity => Page::FramingData,
            Page::FramingStop => Page::FramingParity,
            Page::Transmit => Page::Receive,
            Page::LogMode { binary, .. } => Page::LogName { binary: *binary },
            Page::LogPlain { name, .. } => Page::LogMode {
                binary: false,
                name: name.clone(),
            },
            Page::LogStamp { name, append, .. } => Page::LogPlain {
                name: name.clone(),
                append: *append,
            },
            _ => {
                self.wizard = false;
                Page::Root
            }
        };
        if matches!(self.page, Page::Ports) {
            self.poll_ports(cx);
        }
        cx.notify();
    }

    fn pick_port(&mut self, row: usize, cx: &mut Context<Self>) {
        let Ok(ports) = &self.ports else {
            return;
        };
        let Some(port) = ports.get(row) else {
            return;
        };
        self.draft.path = port.path.clone();
        self.draft.usb = port.usb.clone();
        self.page = Page::Baud;
        self.wizard = true;
        cx.notify();
    }

    fn pick_baud(&mut self, row: usize, query: &str, cx: &mut Context<Self>) {
        let Some(baud) = baud_at(row, query) else {
            return;
        };
        if self.wizard {
            self.draft.baud = baud;
            self.page = Page::Data;
        } else if let Some(session) = self.focused_session() {
            let mut params = session.params().clone();
            params.baud_rate = baud;
            if let Err(error) = session.set_serial_params(params) {
                self.set_status(error.to_string());
            }
            self.palette = false;
            self.page = Page::Root;
            self.remember_layout();
            self.persist();
        }
        cx.notify();
    }

    fn pick_data(&mut self, row: usize, wizard: bool, cx: &mut Context<Self>) {
        let Some(bits) = [DataBits::Five, DataBits::Six, DataBits::Seven, DataBits::Eight].get(row).copied() else {
            return;
        };
        if wizard {
            self.draft.data = bits;
            self.page = Page::Parity;
        } else {
            self.apply_params(|params| params.data_bits = bits);
            self.finish_page();
        }
        cx.notify();
    }

    fn pick_parity(&mut self, row: usize, wizard: bool, cx: &mut Context<Self>) {
        let Some(parity) = [Parity::None, Parity::Odd, Parity::Even].get(row).copied() else {
            return;
        };
        if wizard {
            self.draft.parity = parity;
            self.page = Page::Stop;
        } else {
            self.apply_params(|params| params.parity = parity);
            self.page = Page::FramingStop;
        }
        cx.notify();
    }

    fn pick_stop(&mut self, row: usize, wizard: bool, cx: &mut Context<Self>) {
        let Some(stop) = [StopBits::One, StopBits::Two].get(row).copied() else {
            return;
        };
        if wizard {
            self.draft.stop = stop;
            self.page = Page::ConnectFlow;
        } else {
            self.apply_params(|params| params.stop_bits = stop);
            self.finish_page();
        }
        cx.notify();
    }

    fn pick_flow(&mut self, row: usize, wizard: bool, cx: &mut Context<Self>) {
        let Some(flow) = [Flow::None, Flow::XonXoff, Flow::RtsCts].get(row).copied() else {
            return;
        };
        if wizard {
            self.draft.flow = flow;
            self.page = Page::ConnectDtr;
        } else {
            self.apply_params(|params| params.flow = flow);
            self.finish_page();
        }
        cx.notify();
    }

    fn pick_dtr(&mut self, row: usize, wizard: bool, cx: &mut Context<Self>) {
        let on = row == 1;
        if wizard {
            self.draft.dtr = if on { Some(true) } else { None };
            self.wizard = false;
            self.connect_focused(cx);
        } else if let Some(session) = self.focused_session() {
            if let Err(error) = session.set_dtr(on) {
                self.set_status(error.to_string());
            }
            self.finish_page();
            self.remember_layout();
            self.persist();
        }
        cx.notify();
    }

    fn pick_rts(&mut self, row: usize, cx: &mut Context<Self>) {
        if let Some(session) = self.focused_session() {
            if let Err(error) = session.set_rts(row == 1) {
                self.set_status(error.to_string());
            }
        }
        self.finish_page();
        self.remember_layout();
        self.persist();
        cx.notify();
    }

    fn pick_encoding(&mut self, row: usize, cx: &mut Context<Self>) {
        let encoding = if row == 0 { SessionEncoding::ShiftJis } else { SessionEncoding::Utf8 };
        if let Some(session) = self.focused_session() {
            session.set_encoding(encoding);
        }
        self.finish_page();
        self.remember_layout();
        self.persist();
        cx.notify();
    }

    fn pick_receive(&mut self, row: usize, cx: &mut Context<Self>) {
        let receive = match row {
            0 => ReceiveNewline::Cr,
            1 => ReceiveNewline::CrLf,
            2 => ReceiveNewline::Lf,
            _ => ReceiveNewline::Auto,
        };
        if let Some(session) = self.focused_session() {
            let mut newline = session.newline();
            newline.receive = receive;
            session.set_newline(newline);
        }
        self.page = Page::Transmit;
        cx.notify();
    }

    fn pick_transmit(&mut self, row: usize, cx: &mut Context<Self>) {
        let transmit = match row {
            0 => TransmitNewline::Cr,
            1 => TransmitNewline::CrLf,
            _ => TransmitNewline::Lf,
        };
        if let Some(session) = self.focused_session() {
            let mut newline = session.newline();
            newline.transmit = transmit;
            session.set_newline(newline);
        }
        self.finish_page();
        self.remember_layout();
        self.persist();
        cx.notify();
    }

    fn pick_echo(&mut self, row: usize, cx: &mut Context<Self>) {
        if let Some(session) = self.focused_session() {
            session.set_local_echo(row == 1);
        }
        self.finish_page();
        self.remember_layout();
        self.persist();
        cx.notify();
    }

    fn send_query(&mut self, query: &str, cx: &mut Context<Self>) {
        if query.is_empty() {
            self.set_status("送る文字列が空");
        } else if let Some(session) = self.focused_session() {
            if let Err(error) = session.apply_input(Input::Text(query.to_string())) {
                self.set_status(error.to_string());
            }
        }
        self.finish_page();
        cx.notify();
    }

    fn pick_terminal_font(&mut self, row: usize, cx: &mut Context<Self>) {
        let names = cx.text_system().all_font_names();
        let Some(name) = names.get(row) else {
            return;
        };
        self.settings.terminal.font_family = name.clone();
        self.terminal_font = Some(name.clone());
        self.finish_page();
        self.persist();
        cx.notify();
    }

    fn pick_font_size(&mut self, row: usize, query: &str, cx: &mut Context<Self>) {
        let Some(size) = font_size_at(row, query) else {
            return;
        };
        self.settings.terminal.font_size_px = size;
        self.finish_page();
        self.persist();
        cx.notify();
    }

    fn pick_ui_font(&mut self, row: usize, cx: &mut Context<Self>) {
        let names = cx.text_system().all_font_names();
        let Some(name) = names.get(row) else {
            return;
        };
        self.settings.ui.font_family = name.clone();
        gpui_kit::component::theme::Theme::update(cx, |theme| {
            theme.font_family = name.clone().into();
        });
        self.finish_page();
        self.persist();
        cx.notify();
    }

    fn pick_log_name(&mut self, binary: bool, query: &str, cx: &mut Context<Self>) {
        let name = if query.is_empty() {
            let path = self
                .focused_session()
                .map(|session| session.params().path.clone())
                .unwrap_or_default();
            expand_log_name(&self.settings.log.name_template, &port_token(&path), Local::now())
        } else {
            query.to_string()
        };
        if let Err(error) = validate_log_name(&name) {
            self.set_status(error.to_string());
            cx.notify();
            return;
        }
        self.page = Page::LogMode { binary, name };
        cx.notify();
    }

    fn start_log(&mut self, name: String, append: bool, plain: bool, stamp: bool, binary: bool, cx: &mut Context<Self>) {
        let dir = match log_dir(&self.settings) {
            Ok(dir) => dir,
            Err(error) => {
                self.set_status(error);
                cx.notify();
                return;
            }
        };
        let path = dir.join(name);
        let rotate = Rotate {
            max_bytes: self.settings.log.rotate_bytes,
            keep: self.settings.log.rotate_keep,
        };
        let target = if binary {
            LogTarget::Binary { path, append, rotate }
        } else {
            LogTarget::Text {
                path,
                append,
                rotate,
                encoding: LogEncoding::Utf8,
                plain,
                bom: false,
                timestamp: if stamp { Some(Timestamp::LocalFixed) } else { None },
            }
        };
        if let Some(session) = self.focused_session() {
            if let Err(error) = session.log(LogCommand::Start(target)) {
                self.set_status(error.to_string());
            }
        }
        self.finish_page();
        cx.notify();
    }

    fn apply_params(&mut self, edit: impl FnOnce(&mut SerialParams)) {
        if let Some(session) = self.focused_session() {
            let mut params = session.params().clone();
            edit(&mut params);
            if let Err(error) = session.set_serial_params(params) {
                self.set_status(error.to_string());
            }
        }
    }

    fn finish_page(&mut self) {
        self.palette = false;
        self.page = Page::Root;
        self.wizard = false;
    }

    fn remember_layout(&mut self) {}

    fn reopen_saved(&mut self, cx: &mut Context<Self>) {
        let ids = self.layout.panes();
        for id in ids {
            let Some(pane) = self.panes.get(&id) else {
                continue;
            };
            if pane.session.params().path.is_empty() {
                continue;
            }
            let params = pane.session.params().clone();
            let connected = self.panes.get_mut(&id).map(|pane| pane.session.connect(params));
            match connected {
                Some(Ok(rx)) => self.attach(id, rx, cx),
                Some(Err(error)) => self.set_status(error.to_string()),
                None => {}
            }
        }
    }

    fn render_tree(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let view = self.layout.view();
        let zoomed = self.layout.zoomed();
        let focus = self.layout.focus();
        let mut used = Vec::new();
        let mut pending = self.sync_splits;
        let element = if zoomed {
            self.pane_view(focus, window, cx)
        } else {
            self.render_node(&view, window, cx, &mut used, &mut pending)
        };
        self.splits.retain(|key, _| used.iter().any(|used| used == key));
        if self.sync_splits && !pending {
            self.sync_splits = false;
        }
        element
    }

    fn render_node(
        &mut self,
        node: &ViewNode,
        window: &mut Window,
        cx: &mut Context<Self>,
        used: &mut Vec<String>,
        pending: &mut bool,
    ) -> AnyElement {
        match node {
            ViewNode::Leaf(id) => self.pane_view(*id, window, cx),
            ViewNode::Split {
                axis,
                ratio,
                path,
                first,
                second,
            } => {
                let key = path_key(path);
                used.push(key.clone());
                let state = self
                    .splits
                    .entry(key.clone())
                    .or_insert_with(|| cx.new(|_| gpui_kit::component::resizable::ResizableState::default()))
                    .clone();
                let sizes = state.read(cx).sizes().clone();
                if self.sync_splits {
                    if sizes.len() >= 2 && sizes[0].as_f32() + sizes[1].as_f32() > 0.0 {
                        let total = sizes[0].as_f32() + sizes[1].as_f32();
                        let ratio = *ratio;
                        let state = state.clone();
                        cx.defer_in(window, move |this, window, cx| {
                            this.restoring = true;
                            state.update(cx, |state, cx| {
                                state.resize_panel(0, px(total * ratio), window, cx);
                            });
                            this.restoring = false;
                        });
                    } else {
                        *pending = true;
                    }
                }
                let first = self.render_node(first, window, cx, used, pending);
                let second = self.render_node(second, window, cx, used, pending);
                let weak = cx.entity().downgrade();
                let path = path.clone();
                let group = match axis {
                    Axis::SideBySide => h_resizable(SharedString::from(key.clone())),
                    Axis::Stacked => v_resizable(SharedString::from(key)),
                };
                group
                    .with_state(&state)
                    .child(
                        resizable_panel()
                            .size(px(1000.0 * ratio))
                            .size_range(px(1.0)..px(f32::MAX))
                            .child(first),
                    )
                    .child(
                        resizable_panel()
                            .size(px(1000.0 * (1.0 - ratio)))
                            .size_range(px(1.0)..px(f32::MAX))
                            .child(second),
                    )
                    .on_resize(move |state, _window, cx| {
                        let sizes = state.read(cx).sizes().clone();
                        if sizes.len() < 2 {
                            return;
                        }
                        let total = sizes[0].as_f32() + sizes[1].as_f32();
                        if total <= 0.0 {
                            return;
                        }
                        let ratio = sizes[0].as_f32() / total;
                        let path = path.clone();
                        let _ = weak.update(cx, |this, _cx| {
                            if this.restoring {
                                return;
                            }
                            this.layout.set_live_ratio(&path, ratio);
                            this.sync_splits = true;
                            this.remember_layout();
                            this.persist();
                        });
                    })
                    .into_any_element()
            }
        }
    }

    fn pane_view(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let focused = self.layout.focus() == id;
        let status_facts = self.status_facts(id, focused);
        let overlay = self.panes.get(&id).is_some_and(|pane| {
            matches!(
                pane.session.link(),
                Link::Disconnected
            ) && pane.session.screen_content().cells.iter().all(|cell| cell.text.is_empty() || cell.text == " ")
        });
        let snap = self.panes.get(&id).map(|pane| pane.session.screen_content());
        let palette = palette_by_name(&self.settings.terminal.palette).unwrap_or_else(rs_term_core::classic_dark);
        let font_name = self.terminal_font.clone();
        let font_px = self.settings.terminal.font_size_px;
        let fallbacks = self.settings.terminal.font_fallbacks.clone();
        if let (Some(name), true) = (font_name.as_deref(), self.cell_px <= 8.0 || self.line_px <= 0.0) {
            let shaped = window.text_system().shape_line(
                "0".into(),
                px(font_px),
                &[TextRun {
                    len: 1,
                    font: mono_font(name, &fallbacks),
                    color: gpui::black(),
                    ..Default::default()
                }],
                None,
            );
            if shaped.width().as_f32() > 0.0 {
                self.cell_px = shaped.width().as_f32();
                self.line_px = font_px * 1.4;
            }
        }
        if let Some(bounds) = self.grid_bounds.borrow().get(&id).copied() {
            let columns = (bounds.size.width.as_f32() / self.cell_px).floor() as usize;
            let rows = (bounds.size.height.as_f32() / self.line_px).floor() as usize;
            if columns > 0 && rows > 0 {
                if let Some(pane) = self.panes.get_mut(&id) {
                    pane.session.resize(columns.max(1), rows.max(1));
                }
            }
        }
        let cell_px = self.cell_px;
        let line_px = self.line_px;
        let slots = self.grid_bounds.clone();
        let weak = cx.entity().downgrade();
        let wheel = weak.clone();
        let drag = weak.clone();
        let release = weak.clone();
        div()
            .id(("pane", id))
            .size_full()
            .flex()
            .flex_col()
            .bg(to_rgba(palette.background))
            .border_1()
            .border_color(if focused { gpui::rgb(0x4c9aff) } else { gpui::rgb(0x333333) })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .on_mouse_down(MouseButton::Left, {
                        let weak = weak.clone();
                        move |event: &MouseDownEvent, _window, cx| {
                            let _ = weak.update(cx, |this, cx| this.pointer_down(id, event.position, cx));
                        }
                    })
                    .on_mouse_move(move |event: &MouseMoveEvent, _window, cx| {
                        let _ = drag.update(cx, |this, cx| this.pointer_move(id, event.position, cx));
                    })
                    .on_mouse_up(MouseButton::Left, move |_event: &MouseUpEvent, _window, cx| {
                        let _ = release.update(cx, |this, _cx| {
                            if this.selecting == Some(id) {
                                this.selecting = None;
                            }
                        });
                    })
                    .on_scroll(move |event: &ScrollEvent, _window, cx| {
                        let _ = wheel.update(cx, |this, cx| this.pointer_scroll(id, event, cx));
                    })
                    .child(canvas(
                        move |bounds, _window, _cx| {
                            slots.borrow_mut().insert(id, bounds);
                            bounds
                        },
                        move |bounds, _prepared, window, cx| {
                            paint_grid(
                                bounds,
                                &GridPaint {
                                    snap: snap.as_ref(),
                                    palette: &palette,
                                    font_name: font_name.as_deref(),
                                    font_px,
                                    fallbacks: &fallbacks,
                                    cell_px,
                                    line_px,
                                },
                                window,
                                cx,
                            );
                        },
                    ))
                    .when(overlay, |el| {
                        el.child(
                            div()
                                .absolute()
                                .top(px(8.0))
                                .left(px(10.0))
                                .text_color(gpui::rgb(0x7a7d86))
                                .child("（未接続）  Ctrl+Shift+K で接続"),
                        )
                    }),
            )
            .child(status_bar(&status_facts, focused))
            .into_any_element()
    }

    fn pointer_down(&mut self, id: u64, position: Point<gpui::Pixels>, cx: &mut Context<Self>) {
        self.layout.focus_id(id);
        if let Some((line, column)) = self.cell_at(id, position) {
            if let Some(pane) = self.panes.get_mut(&id) {
                pane.session.select_start(line, column);
            }
            self.selecting = Some(id);
        }
        cx.notify();
    }

    fn pointer_move(&mut self, id: u64, position: Point<gpui::Pixels>, cx: &mut Context<Self>) {
        if self.selecting != Some(id) {
            return;
        }
        if let Some((line, column)) = self.cell_at(id, position) {
            if let Some(pane) = self.panes.get_mut(&id) {
                pane.session.select_update(line, column);
                cx.notify();
            }
        }
    }

    fn pointer_scroll(&mut self, id: u64, event: &ScrollEvent, cx: &mut Context<Self>) {
        let delta = match event.delta {
            ScrollDelta::Lines(point) => point.y.round() as i32,
            ScrollDelta::Pixels(point) => (point.y.as_f32() / self.line_px).round() as i32,
        };
        if delta != 0 {
            if let Some(pane) = self.panes.get_mut(&id) {
                pane.session.scroll_lines(-delta);
                cx.notify();
            }
        }
    }

    fn cell_at(&self, id: u64, position: Point<gpui::Pixels>) -> Option<(i32, usize)> {
        let bounds = self.grid_bounds.borrow().get(&id).copied()?;
        let pane = self.panes.get(&id)?;
        if position.x < bounds.origin.x
            || position.y < bounds.origin.y
            || position.x >= bounds.origin.x + bounds.size.width
            || position.y >= bounds.origin.y + bounds.size.height
        {
            return None;
        }
        let column = ((position.x.as_f32() - bounds.origin.x.as_f32()) / self.cell_px).floor() as usize;
        let row = ((position.y.as_f32() - bounds.origin.y.as_f32()) / self.line_px).floor() as usize;
        let snap = pane.session.screen_content();
        if column >= snap.columns || row >= snap.rows {
            return None;
        }
        Some((row as i32 - snap.display_offset as i32, column))
    }

    /// The facts the status line of pane `id` shows (design §3).
    fn status_facts(&self, id: u64, focused: bool) -> status::Facts {
        let pane = self.panes.get(&id);
        let session = pane.map(|pane| &pane.session);
        let params = session.map(|session| session.params().clone());
        let port = params.as_ref().map(|params| params.path.clone()).unwrap_or_default();
        let link = match session.map(|session| session.link().clone()) {
            None | Some(Link::Disconnected) if port.is_empty() => status::LinkView::Unchosen,
            None | Some(Link::Disconnected) => status::LinkView::Disconnected(port),
            Some(Link::Opening { .. }) => status::LinkView::Opening(port),
            Some(Link::Open { .. }) => status::LinkView::Open(port),
            Some(Link::Lost { .. }) => status::LinkView::Lost(port),
        };
        let frame = params.as_ref().and_then(|params| {
            (!params.path.is_empty()).then_some((
                params.baud_rate,
                params.data_bits,
                params.parity,
                params.stop_bits,
            ))
        });
        let encoding = match session.map(|session| session.encoding()) {
            Some(rs_term_core::SessionEncoding::Utf8) => "UTF-8",
            _ => "Shift_JIS",
        };
        let message = session
            .and_then(|session| session.note())
            .map(|message| (message.severity, message.text.clone()));
        status::Facts {
            focused_badge: (focused && self.resize).then_some("RESIZE"),
            name: String::new(),
            link,
            frame,
            flow: params.as_ref().map(|params| params.flow).unwrap_or(Flow::None),
            encoding,
            local_echo: session.is_some_and(|session| session.local_echo()),
            newline: None,
            dtr: params.as_ref().is_some_and(|params| params.dtr_on_open == Some(true)),
            log: session
                .map(|session| status::LogView::from_session(session.log_state()))
                .unwrap_or(status::LogView::Off),
            message,
        }
    }
}

/// The 24 px status line at the bottom of a pane (design §1.3, §3).
fn status_bar(facts: &status::Facts, focused: bool) -> impl IntoElement {
    let fields = status::build(facts);
    let width = 1000.0_f32;
    let fields = status::fit(
        fields,
        width,
        160.0,
        |text| text.chars().count() as f32 * 7.0,
        facts.message.as_ref().map_or(0.0, |(_, text)| text.chars().count() as f32 * 7.0),
    );
    let mut row = div()
        .h(px(24.0))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(14.0))
        .px(px(10.0))
        .overflow_hidden()
        .text_size(px(12.0))
        .bg(gpui::rgb(0x2a2c33))
        .border_t_1()
        .border_color(gpui::rgb(0x3b3d45))
        .text_color(gpui::rgb(0xaeb2bb));
    if focused {
        row = row.border_color(gpui::rgb(0x3b3d45));
    }
    for field in fields {
        row = row.child(div().flex_none().text_color(tone_color(field.tone)).child(field.text));
    }
    if let Some((severity, text)) = &facts.message {
        let (line, tone) = status::message_text(*severity, text);
        row = row.child(div().flex_1().min_w(px(160.0)).overflow_hidden().text_color(tone_color(tone)).child(line));
    }
    row
}

fn tone_color(tone: status::Tone) -> gpui::Rgba {
    gpui::rgb(match tone {
        status::Tone::Normal => 0xaeb2bb,
        status::Tone::Green => 0x57c25a,
        status::Tone::Yellow => 0xe5c07b,
        status::Tone::Gray => 0x777777,
        status::Tone::Red => 0xe06c75,
        status::Tone::Accent => 0x4a8cff,
    })
}

pub fn open(cx: &mut App) {
    // Design screen §1.1: 1200 × 708 client area (the 740 window less the
    // caption), minimum 640 × 400. The frame adds the caption height.
    let options = gpui::WindowOptions {
        window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::centered(
            None,
            gpui::size(px(1200.0), px(740.0)),
            cx,
        ))),
        window_min_size: Some(gpui::size(px(640.0), px(400.0))),
        ..Default::default()
    };
    if let Err(error) = gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| Workspace::new(window, cx))
    }) {
        eprintln!("rs-term: {error}");
    }
}

fn sessions_from_settings(settings: &rs_term_core::Settings) -> (Layout, std::collections::HashMap<u64, Pane>) {
    let layout = Layout::single(1);
    let mut panes = std::collections::HashMap::new();
    for id in layout.panes() {
        let mut session = Session::new(SessionConfig {
            screen: ScreenConfig {
                columns: settings.terminal.columns,
                rows: settings.terminal.rows,
                scrollback: settings.terminal.scrollback,
            },
            encoding: settings.encoding,
            newline: settings.newline,
            local_echo: settings.terminal.local_echo,
        });
        session.set_reconnect_enabled(settings.serial.reconnect);
        let _ = session.set_serial_params(params_from_settings(settings, String::new()));
        panes.insert(id, Pane { session, _io: None });
    }
    (layout, panes)
}

fn params_from_settings(settings: &rs_term_core::Settings, path: String) -> SerialParams {
    SerialParams {
        path,
        baud_rate: settings.serial.baud_rate,
        data_bits: settings.serial.data_bits,
        parity: settings.serial.parity,
        stop_bits: settings.serial.stop_bits,
        flow: settings.serial.flow,
        dtr_on_open: None,
        rts: false,
        read_timeout: std::time::Duration::from_millis(settings.serial.read_timeout_ms),
    }
}

fn load_settings() -> (rs_term_core::Settings, Option<PathBuf>, bool, String) {
    let Some(path) = config_file("settings.toml") else {
        return (
            rs_term_core::Settings::default(),
            None,
            false,
            "設定パスが無い".into(),
        );
    };
    if !path.exists() {
        return (rs_term_core::Settings::default(), Some(path), true, String::new());
    }
    match rs_term_core::load_settings_file(&path) {
        Ok(settings) => (settings, Some(path), true, String::new()),
        Err(error) => (rs_term_core::Settings::default(), Some(path), false, error),
    }
}

fn load_keymap(cx: &mut App) -> String {
    let Some(path) = config_file("keymap.toml") else {
        return String::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return String::new();
    };
    let mut bindings = Vec::new();
    let mut notes = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, action)) = line.split_once('=') else {
            notes.push(format!("{}行目を無視", index + 1));
            continue;
        };
        match action_by_id(action.trim()) {
            Some(action) => match gpui::KeyBinding::load(key.trim(), action, None, false, None, cx) {
                Ok(binding) => bindings.push(binding),
                Err(_) => notes.push(format!("{}行目のキーを無視", index + 1)),
            },
            None => notes.push(format!("{}行目のアクションを無視", index + 1)),
        }
    }
    if !bindings.is_empty() {
        cx.bind_keys(bindings);
    }
    notes.join(" ")
}

fn config_file(name: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        rs_term_core::windows_settings_path(std::env::var("APPDATA").ok().as_deref())
            .map(|path| path.with_file_name(name))
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join("Library/Application Support/rs-term").join(name))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        Some(base.join("rs-term").join(name))
    }
}

fn log_dir(settings: &rs_term_core::Settings) -> Result<PathBuf, String> {
    if !settings.log.directory.is_empty() {
        return Ok(PathBuf::from(&settings.log.directory));
    }
    windows_log_dir(std::env::var("LOCALAPPDATA").ok().as_deref())
        .ok_or_else(|| "LOCALAPPDATA が空なのでログを始められない".into())
}

fn apply_theme(settings: &rs_term_core::Settings, window: &mut Window, cx: &mut App) {
    let mode = if settings.ui.theme_mode == "light" {
        gpui_kit::component::theme::ThemeMode::Light
    } else {
        gpui_kit::component::theme::ThemeMode::Dark
    };
    gpui_kit::component::theme::Theme::change(mode, Some(window), cx);
    if !settings.ui.font_family.is_empty() {
        let family = settings.ui.font_family.clone();
        gpui_kit::component::theme::Theme::update(cx, |theme| {
            theme.font_family = family.into();
        });
    }
    if settings.ui.font_size_px > 0.0 {
        let size = px(settings.ui.font_size_px);
        gpui_kit::component::theme::Theme::update(cx, |theme| theme.font_size = size);
    }
}

fn root_items() -> Vec<CommandItem> {
    vec![
        acted("接続", &["session.connect"], actions::Connect),
        acted("切断", &["session.disconnect"], actions::Disconnect),
        acted("通信速度", &["session.baud"], actions::Baud),
        acted("フレーム", &["session.framing"], actions::Framing),
        acted("フロー制御", &["session.flow"], actions::Flow),
        acted("DTR", &["session.dtr"], actions::Dtr),
        acted("RTS", &["session.rts"], actions::Rts),
        acted("エンコーディング", &["session.encoding"], actions::Encoding),
        acted("改行", &["session.newline"], actions::Newline),
        acted("ローカルエコー", &["session.local_echo"], actions::LocalEcho),
        acted("文字列を送る", &["session.send_text"], actions::SendText),
        acted("ブレーク", &["session.break"], actions::Break),
        acted("左右に分割", &["pane.split_vertical"], actions::SplitVertical),
        acted("上下に分割", &["pane.split_horizontal"], actions::SplitHorizontal),
        acted("左のペイン", &["pane.focus_left"], actions::FocusLeft),
        acted("下のペイン", &["pane.focus_down"], actions::FocusDown),
        acted("上のペイン", &["pane.focus_up"], actions::FocusUp),
        acted("右のペイン", &["pane.focus_right"], actions::FocusRight),
        acted("左と入れ替え", &["pane.swap_left"], actions::SwapLeft),
        acted("下と入れ替え", &["pane.swap_down"], actions::SwapDown),
        acted("上と入れ替え", &["pane.swap_up"], actions::SwapUp),
        acted("右と入れ替え", &["pane.swap_right"], actions::SwapRight),
        acted("次のペイン", &["pane.cycle_next"], actions::CycleNext),
        acted("前のペイン", &["pane.cycle_previous"], actions::CyclePrevious),
        acted("ペインを閉じる", &["pane.close"], actions::ClosePane),
        acted("ズーム", &["pane.zoom"], actions::Zoom),
        acted("リサイズモード", &["pane.resize_mode"], actions::ResizeMode),
        acted("境界を左へ", &["pane.resize_left"], actions::ResizeLeft),
        acted("境界を下へ", &["pane.resize_down"], actions::ResizeDown),
        acted("境界を上へ", &["pane.resize_up"], actions::ResizeUp),
        acted("境界を右へ", &["pane.resize_right"], actions::ResizeRight),
        acted("ダーク", &["theme.dark"], actions::ThemeDark),
        acted("ライト", &["theme.light"], actions::ThemeLight),
        acted("端末フォント", &["font.terminal"], actions::FontTerminal),
        acted("端末フォントサイズ", &["font.size"], actions::FontSize),
        acted("UIフォント", &["font.ui"], actions::FontUi),
        acted("バイナリログ開始", &["log.start_binary"], actions::LogBinary),
        acted("テキストログ開始", &["log.start_text"], actions::LogText),
        acted("ログ一時停止", &["log.pause"], actions::LogPause),
        acted("ログ再開", &["log.resume"], actions::LogResume),
        acted("ログ停止", &["log.stop"], actions::LogStop),
        acted("コピー", &["edit.copy"], actions::Copy),
        acted("貼り付け", &["edit.paste"], actions::Paste),
        acted("バージョン", &["app.about"], actions::About),
    ]
}

fn acted(label: &str, keywords: &[&str], action: impl gpui::Action) -> CommandItem {
    CommandItem::new()
        .label(label)
        .keywords(keywords.iter().copied())
        .action(Box::new(action))
}

fn plain(label: impl Into<SharedString>, keywords: &[&str]) -> CommandItem {
    CommandItem::new().label(label).keywords(keywords.iter().copied())
}

fn port_item(port: &rs_term_core::PortInfo) -> CommandItem {
    let usb = port
        .usb
        .as_ref()
        .map(|usb| {
            format!(
                " {:04X}:{:04X} {} {}",
                usb.vid,
                usb.pid,
                usb.serial_number.as_deref().unwrap_or(""),
                usb.product.as_deref().unwrap_or("")
            )
        })
        .unwrap_or_default();
    plain(format!("{}{}", port.path, usb), &[port.path.as_str()])
}

fn baud_items(query: &str) -> Vec<CommandItem> {
    let mut items: Vec<_> = BAUD_RATES.iter().map(|baud| plain(baud.to_string(), &["session.baud"])).collect();
    if let Some(baud) = query.parse::<u32>().ok().filter(|baud| (1..=4_000_000).contains(baud)) {
        if !BAUD_RATES.contains(&baud) {
            items.push(plain(format!("この値 {baud}"), &["session.baud"]));
        }
    }
    items
}

fn baud_at(row: usize, query: &str) -> Option<u32> {
    if row < BAUD_RATES.len() {
        return Some(BAUD_RATES[row]);
    }
    query.parse().ok().filter(|baud| (1..=4_000_000).contains(baud))
}

fn data_items() -> Vec<CommandItem> {
    ["5", "6", "7", "8"].into_iter().map(|label| plain(format!("{label} ビット"), &[])).collect()
}

fn parity_items() -> Vec<CommandItem> {
    ["なし", "奇数", "偶数"].into_iter().map(|label| plain(label, &[])).collect()
}

fn stop_items() -> Vec<CommandItem> {
    vec![plain("1 ストップビット", &[]), plain("2 ストップビット", &[])]
}

fn flow_items() -> Vec<CommandItem> {
    vec![
        plain("なし", &["session.flow"]),
        plain("XON/XOFF", &["session.flow"]),
        plain("RTS/CTS（CTS による送信制御。受信の自動 RTS はしない）", &["session.flow"]),
    ]
}

fn dtr_items() -> Vec<CommandItem> {
    vec![plain("DTR オフ", &[]), plain("DTR オン", &[])]
}

fn font_size_items(query: &str) -> Vec<CommandItem> {
    let mut items: Vec<_> = [10.0, 12.0, 14.0, 16.0, 18.0, 20.0]
        .into_iter()
        .map(|size| plain(format!("{size}"), &["font.size"]))
        .collect();
    if let Some(size) = query.parse::<f32>().ok().filter(|size| (1.0..=72.0).contains(size)) {
        items.push(plain(format!("この値 {size}"), &["font.size"]));
    }
    items
}

fn font_size_at(row: usize, query: &str) -> Option<f32> {
    const SIZES: [f32; 6] = [10.0, 12.0, 14.0, 16.0, 18.0, 20.0];
    if row < SIZES.len() {
        return Some(SIZES[row]);
    }
    query.parse().ok().filter(|size| (1.0..=72.0).contains(size))
}

fn function_key(key: &str) -> Option<u8> {
    let number = key.strip_prefix('f')?.parse::<u8>().ok()?;
    (1..=12).contains(&number).then_some(number)
}

fn path_key(path: &[bool]) -> String {
    path.iter().map(|right| if *right { 'r' } else { 'l' }).collect()
}

fn mono_font(name: &str, fallbacks: &[String]) -> gpui::Font {
    let mut font = font(name);
    font.features = FontFeatures(Arc::new(vec![
        ("calt".into(), 0),
        ("liga".into(), 0),
        ("clig".into(), 0),
    ]));
    font.fallbacks = Some(FontFallbacks::from_fonts(fallbacks.to_vec()));
    font
}

struct GridPaint<'a> {
    snap: Option<&'a rs_term_core::ScreenSnapshot>,
    palette: &'a rs_term_core::TerminalPalette,
    font_name: Option<&'a str>,
    font_px: f32,
    fallbacks: &'a [String],
    cell_px: f32,
    line_px: f32,
}

fn paint_grid(bounds: Bounds<gpui::Pixels>, grid: &GridPaint<'_>, window: &mut Window, cx: &mut App) {
    let snap = grid.snap;
    let palette = grid.palette;
    let font_name = grid.font_name;
    let font_px = grid.font_px;
    let fallbacks = grid.fallbacks;
    let cell_px = grid.cell_px;
    let line_px = grid.line_px;
    window.paint_quad(fill(bounds, to_rgba(palette.background)));
    let Some(name) = font_name else {
        return;
    };
    let Some(snap) = snap else {
        return;
    };
    let columns = snap.columns.max(1);
    for (index, cell) in snap.cells.iter().enumerate() {
        if cell.width == 0 {
            continue;
        }
        let row = index / columns;
        let column = index % columns;
        let origin = Point {
            x: bounds.origin.x + px(column as f32 * cell_px),
            y: bounds.origin.y + px(row as f32 * line_px),
        };
        let cell_bounds = Bounds {
            origin,
            size: size(px(cell_px * cell.width as f32), px(line_px)),
        };
        let row_bounds = Bounds {
            origin: Point { x: bounds.origin.x, y: origin.y },
            size: size(bounds.size.width, px(line_px)),
        };
        let (fg, bg) = resolve(&cell.fg, &cell.bg, cell.flags.inverse, &snap.color_overrides, palette);
        let bg = if cell.selected { palette.foreground } else { bg };
        let fg = if cell.selected { palette.background } else { fg };
        window.with_content_mask(Some(ContentMask { bounds: row_bounds }), |window| {
            window.with_content_mask(Some(ContentMask { bounds: cell_bounds }), |window| {
                window.paint_quad(fill(cell_bounds, to_rgba(bg)));
                if !cell.text.is_empty() && cell.text != " " {
                    let shaped = window.text_system().shape_line(
                        cell.text.clone().into(),
                        px(font_px),
                        &[TextRun {
                            len: cell.text.len(),
                            font: mono_font(name, fallbacks),
                            color: to_rgba(fg).into(),
                            underline: cell.flags.underline.then(underline),
                            strikethrough: cell.flags.strike.then(strike),
                            ..Default::default()
                        }],
                        None,
                    );
                    let _ = shaped.paint(origin, px(line_px), TextAlign::Left, None, window, cx);
                }
            });
        });
        let cursor_row = snap.cursor.line + snap.display_offset as i32;
        if snap.cursor.visible && cursor_row == row as i32 && snap.cursor.column == column {
            let cursor = Bounds {
                origin,
                size: size(px(cell_px), px(line_px)),
            };
            window.paint_quad(fill(cursor, to_rgba(palette.cursor)));
        }
    }
}

fn underline() -> gpui::UnderlineStyle {
    gpui::UnderlineStyle {
        thickness: px(1.0),
        color: None,
        wavy: false,
    }
}

fn strike() -> gpui::StrikethroughStyle {
    gpui::StrikethroughStyle {
        thickness: px(1.0),
        color: None,
    }
}

fn to_rgba(color: rs_term_core::Rgb) -> gpui::Rgba {
    gpui::rgb(u32::from_be_bytes([0, color.r, color.g, color.b]))
}




fn action_by_id(id: &str) -> Option<Box<dyn gpui::Action>> {
    let action: Box<dyn gpui::Action> = match id {
        "palette.toggle" => Box::new(actions::PaletteToggle),
        "app.about" => Box::new(actions::About),
        "session.connect" => Box::new(actions::Connect),
        "session.disconnect" => Box::new(actions::Disconnect),
        "session.baud" => Box::new(actions::Baud),
        "session.framing" => Box::new(actions::Framing),
        "session.flow" => Box::new(actions::Flow),
        "session.dtr" => Box::new(actions::Dtr),
        "session.rts" => Box::new(actions::Rts),
        "session.encoding" => Box::new(actions::Encoding),
        "session.newline" => Box::new(actions::Newline),
        "session.local_echo" => Box::new(actions::LocalEcho),
        "session.send_text" => Box::new(actions::SendText),
        "session.break" => Box::new(actions::Break),
        "pane.split_vertical" => Box::new(actions::SplitVertical),
        "pane.split_horizontal" => Box::new(actions::SplitHorizontal),
        "pane.focus_left" => Box::new(actions::FocusLeft),
        "pane.focus_down" => Box::new(actions::FocusDown),
        "pane.focus_up" => Box::new(actions::FocusUp),
        "pane.focus_right" => Box::new(actions::FocusRight),
        "pane.swap_left" => Box::new(actions::SwapLeft),
        "pane.swap_down" => Box::new(actions::SwapDown),
        "pane.swap_up" => Box::new(actions::SwapUp),
        "pane.swap_right" => Box::new(actions::SwapRight),
        "pane.cycle_next" => Box::new(actions::CycleNext),
        "pane.cycle_previous" => Box::new(actions::CyclePrevious),
        "pane.close" => Box::new(actions::ClosePane),
        "pane.zoom" => Box::new(actions::Zoom),
        "pane.resize_mode" => Box::new(actions::ResizeMode),
        "pane.resize_left" => Box::new(actions::ResizeLeft),
        "pane.resize_down" => Box::new(actions::ResizeDown),
        "pane.resize_up" => Box::new(actions::ResizeUp),
        "pane.resize_right" => Box::new(actions::ResizeRight),
        "theme.dark" => Box::new(actions::ThemeDark),
        "theme.light" => Box::new(actions::ThemeLight),
        "font.terminal" => Box::new(actions::FontTerminal),
        "font.size" => Box::new(actions::FontSize),
        "font.ui" => Box::new(actions::FontUi),
        "log.start_binary" => Box::new(actions::LogBinary),
        "log.start_text" => Box::new(actions::LogText),
        "log.pause" => Box::new(actions::LogPause),
        "log.resume" => Box::new(actions::LogResume),
        "log.stop" => Box::new(actions::LogStop),
        "edit.copy" => Box::new(actions::Copy),
        "edit.paste" => Box::new(actions::Paste),
        _ => return None,
    };
    Some(action)
}
