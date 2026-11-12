//! `alacritty_terminal::Term` plus the VTE processor.
//!
//! Bytes that reach the processor are already UTF-8. Shift_JIS never does.
//! There is one processor. Its handler is `Logged`, which forwards every
//! call to `Term` and, in the same call, records the characters and newlines
//! for the text log (§5.10, review R19). CSI and OSC do not call `input`.

use std::cell::RefCell;
use std::rc::Rc;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Osc52, Term, TermMode};
use alacritty_terminal::vte::ansi::{
    self, Attr, CharsetIndex, ClearMode, Color, CursorShape, CursorStyle, Handler, Hyperlink,
    KeyboardModes, KeyboardModesApplyBehavior, LineClearMode, Mode, NamedColor, PrivateMode,
    Processor, Rgb as VteRgb, StandardCharset, TabulationClearMode,
};

use crate::codec::{Codec, Newline, NewlineConverter, SessionEncoding};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellColor {
    Named(AnsiName),
    Indexed(u8),
    Rgb(Rgb),
}

/// Names `vte` uses. Dim names stay distinct so the palette can prefer a
/// dynamic dim color without inventing a second palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsiName {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
    Foreground,
    Background,
    Cursor,
    DimBlack,
    DimRed,
    DimGreen,
    DimYellow,
    DimBlue,
    DimMagenta,
    DimCyan,
    DimWhite,
    BrightForeground,
    DimForeground,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellFlags {
    pub inverse: bool,
    pub bold: bool,
    pub italic: bool,
    pub dim: bool,
    pub underline: bool,
    pub strike: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenCell {
    pub text: String,
    pub width: u8,
    pub fg: CellColor,
    pub bg: CellColor,
    pub flags: CellFlags,
    pub selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenCursor {
    pub line: i32,
    pub column: usize,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenSnapshot {
    pub columns: usize,
    pub rows: usize,
    pub display_offset: usize,
    pub cells: Vec<ScreenCell>,
    /// Indexed overrides from `Term::colors`, including 256/257/258 and dim.
    pub color_overrides: Vec<Option<Rgb>>,
    pub cursor: ScreenCursor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TextAtom {
    Char(char),
    Cr,
    Lf,
    Bs,
    Tab,
}

pub(crate) struct ApplyOutcome {
    pub text: Vec<TextAtom>,
    pub replies: Vec<Vec<u8>>,
    pub notes: Vec<String>,
}

struct Size {
    columns: usize,
    screen_lines: usize,
}

impl Dimensions for Size {
    fn columns(&self) -> usize {
        self.columns
    }
    fn screen_lines(&self) -> usize {
        self.screen_lines
    }
    fn total_lines(&self) -> usize {
        self.screen_lines
    }
}

struct ReplyListener {
    replies: Rc<RefCell<Vec<Vec<u8>>>>,
    notes: Rc<RefCell<Vec<String>>>,
}

impl EventListener for ReplyListener {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(text) => {
                if text.is_ascii() {
                    self.replies.borrow_mut().push(text.into_bytes());
                } else {
                    self.notes
                        .borrow_mut()
                        .push("Dropped a non-ASCII terminal reply".to_string());
                }
            }
            Event::ClipboardStore(_, _)
            | Event::ClipboardLoad(_, _)
            | Event::ColorRequest(_, _)
            | Event::TextAreaSizeRequest(_)
            | Event::Bell
            | Event::Title(_)
            | Event::ResetTitle
            | Event::Wakeup
            | Event::MouseCursorDirty
            | Event::CursorBlinkingChange
            | Event::Exit
            | Event::ChildExit(_) => {}
        }
    }
}

/// G0 to G3 and the active set, so the log maps DEC line drawing the same
/// way `Term` does.
#[derive(Clone, Copy, Default)]
struct Charsets {
    sets: [StandardCharset; 4],
    active: usize,
}

impl Charsets {
    fn map(&self, c: char) -> char {
        self.sets[self.active].map(c)
    }
}

fn charset_slot(index: CharsetIndex) -> usize {
    match index {
        CharsetIndex::G0 => 0,
        CharsetIndex::G1 => 1,
        CharsetIndex::G2 => 2,
        CharsetIndex::G3 => 3,
    }
}

/// The handler `Processor::advance` drives. Every call goes to `Term`; the
/// text calls are also appended to `out` in the same order.
struct Logged<'a> {
    term: &'a mut Term<ReplyListener>,
    out: &'a mut Vec<TextAtom>,
    charsets: &'a mut Charsets,
}

macro_rules! forward {
    ($( fn $name:ident(&mut self $(, $arg:ident : $ty:ty)* ); )*) => {
        $(
            fn $name(&mut self $(, $arg: $ty)*) {
                <Term<ReplyListener> as Handler>::$name(self.term $(, $arg)*)
            }
        )*
    };
}

impl Handler for Logged<'_> {
    fn input(&mut self, c: char) {
        self.out.push(TextAtom::Char(self.charsets.map(c)));
        self.term.input(c);
    }
    fn carriage_return(&mut self) {
        self.out.push(TextAtom::Cr);
        self.term.carriage_return();
    }
    fn linefeed(&mut self) {
        self.out.push(TextAtom::Lf);
        self.term.linefeed();
    }
    fn newline(&mut self) {
        self.out.push(TextAtom::Lf);
        self.term.newline();
    }
    fn backspace(&mut self) {
        self.out.push(TextAtom::Bs);
        self.term.backspace();
    }
    fn put_tab(&mut self, count: u16) {
        for _ in 0..count {
            self.out.push(TextAtom::Tab);
        }
        self.term.put_tab(count);
    }
    fn set_active_charset(&mut self, index: CharsetIndex) {
        self.charsets.active = charset_slot(index);
        self.term.set_active_charset(index);
    }
    fn configure_charset(&mut self, index: CharsetIndex, charset: StandardCharset) {
        self.charsets.sets[charset_slot(index)] = charset;
        self.term.configure_charset(index, charset);
    }
    fn reset_state(&mut self) {
        *self.charsets = Charsets::default();
        self.term.reset_state();
    }

    // `Term` leaves set_mouse_cursor_icon, set_modify_other_keys,
    // report_modify_other_keys and set_scp at the trait defaults (no-ops), so
    // they are not forwarded.
    forward! {
        fn set_title(&mut self, title: Option<String>);
        fn set_cursor_style(&mut self, style: Option<CursorStyle>);
        fn set_cursor_shape(&mut self, shape: CursorShape);
        fn goto(&mut self, line: i32, col: usize);
        fn goto_line(&mut self, line: i32);
        fn goto_col(&mut self, col: usize);
        fn insert_blank(&mut self, count: usize);
        fn move_up(&mut self, count: usize);
        fn move_down(&mut self, count: usize);
        fn identify_terminal(&mut self, intermediate: Option<char>);
        fn device_status(&mut self, arg: usize);
        fn move_forward(&mut self, col: usize);
        fn move_backward(&mut self, col: usize);
        fn move_down_and_cr(&mut self, row: usize);
        fn move_up_and_cr(&mut self, row: usize);
        fn bell(&mut self);
        fn substitute(&mut self);
        fn set_horizontal_tabstop(&mut self);
        fn scroll_up(&mut self, count: usize);
        fn scroll_down(&mut self, count: usize);
        fn insert_blank_lines(&mut self, count: usize);
        fn delete_lines(&mut self, count: usize);
        fn erase_chars(&mut self, count: usize);
        fn delete_chars(&mut self, count: usize);
        fn move_backward_tabs(&mut self, count: u16);
        fn move_forward_tabs(&mut self, count: u16);
        fn save_cursor_position(&mut self);
        fn restore_cursor_position(&mut self);
        fn clear_line(&mut self, mode: LineClearMode);
        fn clear_screen(&mut self, mode: ClearMode);
        fn clear_tabs(&mut self, mode: TabulationClearMode);
        fn set_tabs(&mut self, interval: u16);
        fn reverse_index(&mut self);
        fn terminal_attribute(&mut self, attr: Attr);
        fn set_mode(&mut self, mode: Mode);
        fn unset_mode(&mut self, mode: Mode);
        fn report_mode(&mut self, mode: Mode);
        fn set_private_mode(&mut self, mode: PrivateMode);
        fn unset_private_mode(&mut self, mode: PrivateMode);
        fn report_private_mode(&mut self, mode: PrivateMode);
        fn set_scrolling_region(&mut self, top: usize, bottom: Option<usize>);
        fn set_keypad_application_mode(&mut self);
        fn unset_keypad_application_mode(&mut self);
        fn set_color(&mut self, index: usize, color: VteRgb);
        fn dynamic_color_sequence(&mut self, prefix: String, index: usize, terminator: &str);
        fn reset_color(&mut self, index: usize);
        fn clipboard_store(&mut self, clipboard: u8, data: &[u8]);
        fn clipboard_load(&mut self, clipboard: u8, terminator: &str);
        fn decaln(&mut self);
        fn push_title(&mut self);
        fn pop_title(&mut self);
        fn text_area_size_pixels(&mut self);
        fn text_area_size_chars(&mut self);
        fn set_hyperlink(&mut self, link: Option<Hyperlink>);
        fn report_keyboard_mode(&mut self);
        fn push_keyboard_mode(&mut self, mode: KeyboardModes);
        fn pop_keyboard_modes(&mut self, to_pop: u16);
        fn set_keyboard_mode(&mut self, mode: KeyboardModes, behavior: KeyboardModesApplyBehavior);
    }
}

pub(crate) struct Screen {
    term: Term<ReplyListener>,
    parser: Processor,
    charsets: Charsets,
    codec: Codec,
    newlines: NewlineConverter,
    replies: Rc<RefCell<Vec<Vec<u8>>>>,
    notes: Rc<RefCell<Vec<String>>>,
    columns: usize,
    rows: usize,
}

impl Screen {
    pub(crate) fn new(
        columns: usize,
        rows: usize,
        scrollback: usize,
        encoding: SessionEncoding,
        newline: Newline,
    ) -> Self {
        let columns = columns.max(1);
        let rows = rows.max(1);
        let replies = Rc::new(RefCell::new(Vec::new()));
        let notes = Rc::new(RefCell::new(Vec::new()));
        let config = Config {
            scrolling_history: scrollback,
            osc52: Osc52::Disabled,
            kitty_keyboard: false,
            ..Config::default()
        };
        let term = Term::new(
            config,
            &Size {
                columns,
                screen_lines: rows,
            },
            ReplyListener {
                replies: Rc::clone(&replies),
                notes: Rc::clone(&notes),
            },
        );
        Self {
            term,
            parser: Processor::new(),
            charsets: Charsets::default(),
            codec: Codec::new(encoding),
            newlines: NewlineConverter::new(newline.receive),
            replies,
            notes,
            columns,
            rows,
        }
    }

    pub(crate) fn apply_bytes(&mut self, bytes: &[u8]) -> ApplyOutcome {
        let decoded = self.codec.push(bytes);
        self.feed_utf8(&decoded)
    }

    /// End of a connection: an incomplete lead becomes U+FFFD, a held CR is
    /// completed, and the parser forgets a half escape sequence. The charset
    /// state stays, because `Term` keeps it too.
    pub(crate) fn finish_stream(&mut self) -> ApplyOutcome {
        let flushed = self.codec.reset();
        let mut converted = self.newlines.push(&flushed);
        converted.extend(self.newlines.finish());
        let outcome = self.feed_converted(&converted);
        self.parser = Processor::new();
        outcome
    }

    pub(crate) fn set_encoding(&mut self, encoding: SessionEncoding) -> ApplyOutcome {
        let flushed = self.codec.set_encoding(encoding);
        self.feed_utf8(&flushed)
    }

    pub(crate) fn set_newline(&mut self, newline: Newline) -> ApplyOutcome {
        let held = self.newlines.finish();
        let outcome = self.feed_converted(&held);
        self.newlines.set_mode(newline.receive);
        outcome
    }

    fn feed_utf8(&mut self, decoded: &[u8]) -> ApplyOutcome {
        let converted = self.newlines.push(decoded);
        self.feed_converted(&converted)
    }

    fn feed_converted(&mut self, converted: &[u8]) -> ApplyOutcome {
        let mut text = Vec::new();
        let mut handler = Logged {
            term: &mut self.term,
            out: &mut text,
            charsets: &mut self.charsets,
        };
        self.parser.advance(&mut handler, converted);
        ApplyOutcome {
            text,
            replies: self.replies.borrow_mut().drain(..).collect(),
            notes: self.notes.borrow_mut().drain(..).collect(),
        }
    }

    /// Local echo. This does not touch the receive codec or the text log.
    pub(crate) fn echo_text(&mut self, text: &str) {
        for ch in text.chars() {
            self.term.input(ch);
        }
    }

    pub(crate) fn echo_newline(&mut self) {
        self.term.carriage_return();
        self.term.linefeed();
    }

    pub(crate) fn resize(&mut self, columns: usize, rows: usize) {
        let columns = columns.max(1);
        let rows = rows.max(1);
        if columns == self.columns && rows == self.rows {
            return;
        }
        self.columns = columns;
        self.rows = rows;
        self.term.resize(Size {
            columns,
            screen_lines: rows,
        });
    }

    pub(crate) fn app_cursor(&self) -> bool {
        self.term.mode().contains(TermMode::APP_CURSOR)
    }

    pub(crate) fn bracketed_paste(&self) -> bool {
        self.term.mode().contains(TermMode::BRACKETED_PASTE)
    }

    pub(crate) fn scroll_lines(&mut self, delta: i32) {
        self.term.scroll_display(Scroll::Delta(delta));
    }

    pub(crate) fn scroll_page(&mut self, up: bool) {
        self.term
            .scroll_display(if up { Scroll::PageUp } else { Scroll::PageDown });
    }

    pub(crate) fn scroll_top(&mut self) {
        self.term.scroll_display(Scroll::Top);
    }

    pub(crate) fn scroll_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    pub(crate) fn select_clear(&mut self) {
        self.term.selection = None;
    }

    pub(crate) fn select_start(&mut self, line: i32, column: usize) {
        let column = column.min(self.columns.saturating_sub(1));
        self.term.selection = Some(Selection::new(
            SelectionType::Simple,
            Point::new(Line(line), Column(column)),
            Side::Left,
        ));
    }

    pub(crate) fn select_update(&mut self, line: i32, column: usize) {
        let column = column.min(self.columns.saturating_sub(1));
        if let Some(selection) = self.term.selection.as_mut() {
            selection.update(Point::new(Line(line), Column(column)), Side::Right);
        }
    }

    pub(crate) fn selected_text(&self) -> Option<String> {
        self.term.selection_to_string()
    }

    pub(crate) fn snapshot(&self) -> ScreenSnapshot {
        let content = self.term.renderable_content();
        let mut cells = Vec::new();
        for indexed in content.display_iter {
            let cell = indexed.cell;
            let mut text = String::new();
            text.push(cell.c);
            if let Some(extra) = cell.zerowidth() {
                text.extend(extra.iter().copied());
            }
            let width = if cell.flags.contains(Flags::WIDE_CHAR) {
                2
            } else if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
            {
                0
            } else {
                1
            };
            let selected = content
                .selection
                .as_ref()
                .is_some_and(|range| range.contains(indexed.point));
            cells.push(ScreenCell {
                text,
                width,
                fg: map_color(cell.fg),
                bg: map_color(cell.bg),
                flags: CellFlags {
                    inverse: cell.flags.contains(Flags::INVERSE),
                    bold: cell.flags.contains(Flags::BOLD),
                    italic: cell.flags.contains(Flags::ITALIC),
                    dim: cell.flags.contains(Flags::DIM),
                    underline: cell.flags.intersects(Flags::ALL_UNDERLINES),
                    strike: cell.flags.contains(Flags::STRIKEOUT),
                },
                selected,
            });
        }
        let mut color_overrides = Vec::with_capacity(269);
        for index in 0..269 {
            color_overrides.push(content.colors[index].map(|rgb| Rgb {
                r: rgb.r,
                g: rgb.g,
                b: rgb.b,
            }));
        }
        let shape_hidden = content.cursor.shape == ansi::CursorShape::Hidden;
        ScreenSnapshot {
            columns: self.columns,
            rows: self.rows,
            display_offset: content.display_offset,
            cells,
            color_overrides,
            cursor: ScreenCursor {
                line: content.cursor.point.line.0,
                column: content.cursor.point.column.0,
                visible: !shape_hidden,
            },
        }
    }

    #[cfg(test)]
    pub(crate) fn visible_rows(&self) -> Vec<String> {
        let snap = self.snapshot();
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
}

fn map_color(color: Color) -> CellColor {
    match color {
        Color::Spec(rgb) => CellColor::Rgb(Rgb {
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
        }),
        Color::Indexed(index) => CellColor::Indexed(index),
        Color::Named(name) => CellColor::Named(map_named(name)),
    }
}

fn map_named(name: NamedColor) -> AnsiName {
    match name {
        NamedColor::Black => AnsiName::Black,
        NamedColor::Red => AnsiName::Red,
        NamedColor::Green => AnsiName::Green,
        NamedColor::Yellow => AnsiName::Yellow,
        NamedColor::Blue => AnsiName::Blue,
        NamedColor::Magenta => AnsiName::Magenta,
        NamedColor::Cyan => AnsiName::Cyan,
        NamedColor::White => AnsiName::White,
        NamedColor::BrightBlack => AnsiName::BrightBlack,
        NamedColor::BrightRed => AnsiName::BrightRed,
        NamedColor::BrightGreen => AnsiName::BrightGreen,
        NamedColor::BrightYellow => AnsiName::BrightYellow,
        NamedColor::BrightBlue => AnsiName::BrightBlue,
        NamedColor::BrightMagenta => AnsiName::BrightMagenta,
        NamedColor::BrightCyan => AnsiName::BrightCyan,
        NamedColor::BrightWhite => AnsiName::BrightWhite,
        NamedColor::Foreground => AnsiName::Foreground,
        NamedColor::Background => AnsiName::Background,
        NamedColor::Cursor => AnsiName::Cursor,
        NamedColor::DimBlack => AnsiName::DimBlack,
        NamedColor::DimRed => AnsiName::DimRed,
        NamedColor::DimGreen => AnsiName::DimGreen,
        NamedColor::DimYellow => AnsiName::DimYellow,
        NamedColor::DimBlue => AnsiName::DimBlue,
        NamedColor::DimMagenta => AnsiName::DimMagenta,
        NamedColor::DimCyan => AnsiName::DimCyan,
        NamedColor::DimWhite => AnsiName::DimWhite,
        NamedColor::BrightForeground => AnsiName::BrightForeground,
        NamedColor::DimForeground => AnsiName::DimForeground,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{Newline, ReceiveNewline, TransmitNewline};

    fn screen() -> Screen {
        Screen::new(
            80,
            24,
            1000,
            SessionEncoding::Utf8,
            Newline {
                receive: ReceiveNewline::Cr,
                transmit: TransmitNewline::Cr,
            },
        )
    }

    #[test]
    fn hello_crlf_is_visible_without_a_port() {
        let mut screen = screen();
        screen.apply_bytes(b"hello\r\n");
        assert_eq!(screen.visible_rows()[0], "hello");
    }

    #[test]
    fn sgr_red_then_reset() {
        let mut screen = screen();
        screen.apply_bytes(b"\x1b[31mR\x1b[0mD");
        let snap = screen.snapshot();
        assert_eq!(snap.cells[0].text, "R");
        assert_eq!(snap.cells[0].fg, CellColor::Named(AnsiName::Red));
        assert_eq!(snap.cells[1].fg, CellColor::Named(AnsiName::Foreground));
    }

    #[test]
    fn wide_character_occupies_two_cells() {
        let mut screen = screen();
        screen.apply_bytes("あ".as_bytes());
        let snap = screen.snapshot();
        assert_eq!(snap.cells[0].text, "あ");
        assert_eq!(snap.cells[0].width, 2);
        assert_eq!(snap.cells[1].width, 0);
    }

    #[test]
    fn combining_mark_stays_on_the_cell() {
        let mut screen = screen();
        screen.apply_bytes("e\u{0301}".as_bytes());
        let snap = screen.snapshot();
        assert!(snap.cells[0].text.contains('e'));
        assert!(snap.cells[0].text.contains('\u{0301}'));
        assert_eq!(snap.cells[0].width, 1);
    }

    #[test]
    fn primary_da_and_status_are_ascii_replies() {
        let mut screen = screen();
        let da = screen.apply_bytes(b"\x1b[c");
        assert_eq!(da.replies, vec![b"\x1b[?6c".to_vec()]);
        let status = screen.apply_bytes(b"\x1b[5n");
        assert_eq!(status.replies, vec![b"\x1b[0n".to_vec()]);
    }

    #[test]
    fn alt_screen_switch_restores_primary_text() {
        let mut screen = screen();
        screen.apply_bytes(b"primary");
        screen.apply_bytes(b"\x1b[?1049h");
        screen.apply_bytes(b"alternate");
        let alt = screen.visible_rows()[0].trim().to_string();
        screen.apply_bytes(b"\x1b[?1049l");
        let primary = screen.visible_rows()[0].trim().to_string();
        // Term clears the alternate screen on entry and restores the primary
        // grid on exit. The expectation follows Term, not a pixel match with
        // Tera Term.
        assert_eq!(alt, "alternate");
        assert_eq!(primary, "primary");
    }

    #[test]
    fn shift_jis_escape_is_a_cursor_command_not_a_prolonged_sound() {
        let mut screen = Screen::new(
            80,
            24,
            100,
            SessionEncoding::ShiftJis,
            Newline {
                receive: ReceiveNewline::Cr,
                transmit: TransmitNewline::Cr,
            },
        );
        screen.apply_bytes(&[0x81, 0x1B, 0x5B, 0x48]);
        let row = &screen.visible_rows()[0];
        assert!(row.contains('\u{FFFD}'));
        assert!(!row.contains('ー'), "{row}");
    }
}
