//! `alacritty_terminal::Term` plus the VTE processor.
//!
//! Bytes that reach the processor are already UTF-8. Shift_JIS never does.
//! A second processor records the characters and newlines the handler is
//! asked to show, which is the text log. CSI and OSC do not call `input`.

use std::cell::RefCell;
use std::rc::Rc;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Osc52, Term, TermMode};
use alacritty_terminal::vte::ansi::{self, Color, Handler, NamedColor, Processor};

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
                        .push("非 ASCII の端末応答は捨てた".to_string());
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

struct TextLog<'a> {
    out: &'a mut Vec<TextAtom>,
}

impl Handler for TextLog<'_> {
    fn input(&mut self, c: char) {
        self.out.push(TextAtom::Char(c));
    }
    fn carriage_return(&mut self) {
        self.out.push(TextAtom::Cr);
    }
    fn linefeed(&mut self) {
        self.out.push(TextAtom::Lf);
    }
    fn newline(&mut self) {
        self.out.push(TextAtom::Lf);
    }
    fn backspace(&mut self) {
        self.out.push(TextAtom::Bs);
    }
    fn put_tab(&mut self, count: u16) {
        for _ in 0..count {
            self.out.push(TextAtom::Tab);
        }
    }
}

pub(crate) struct Screen {
    term: Term<ReplyListener>,
    parser: Processor,
    log_parser: Processor,
    codec: Codec,
    newlines: NewlineConverter,
    replies: Rc<RefCell<Vec<Vec<u8>>>>,
    notes: Rc<RefCell<Vec<String>>>,
    columns: usize,
    rows: usize,
}

impl Screen {
    pub(crate) fn new(columns: usize, rows: usize, scrollback: usize, encoding: SessionEncoding, newline: Newline) -> Self {
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
            log_parser: Processor::new(),
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

    pub(crate) fn finish_codec(&mut self) -> ApplyOutcome {
        let flushed = self.codec.reset();
        self.feed_utf8(&flushed)
    }

    pub(crate) fn set_encoding(&mut self, encoding: SessionEncoding) -> ApplyOutcome {
        let flushed = self.codec.set_encoding(encoding);
        self.feed_utf8(&flushed)
    }

    pub(crate) fn set_newline(&mut self, newline: Newline) {
        self.newlines.set_mode(newline.receive);
    }

    fn feed_utf8(&mut self, decoded: &[u8]) -> ApplyOutcome {
        let converted = self.newlines.push(decoded);
        let mut text = Vec::new();
        self.log_parser.advance(&mut TextLog { out: &mut text }, &converted);
        self.parser.advance(&mut self.term, &converted);
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
        self.term.scroll_display(if up { Scroll::PageUp } else { Scroll::PageDown });
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
