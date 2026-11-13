//! Full-screen layout: terminal grid, one status line, modal dialogs.

use std::num::NonZeroU16;

use ratatui::buffer::{Buffer, CellDiffOption};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, Padding, Paragraph, Widget, Wrap};
use ratatui::Frame;
use rs_term_core::{
    line_to_viewport, AnsiName, CellColor, Rgb, ScreenCell, ScreenSnapshot, TerminalPalette,
};

use crate::app::{App, ModalBody};
use crate::status::{status_line, ELLIPSIS};

pub const HELP: &str = "Prefix key (default Ctrl+A), then:

  c  connect          d  disconnect       r  reconnect
  b  break (250 ms)
  l  log (start)      L  stop log
  s  send (a string)  f  file (send a file)
  u  baud             e  encoding
  p  paste (clipboard); Shift+Insert does it without the prefix
  t  next theme       T  previous theme
  ?  help             q  quit
  Prefix again        send the prefix's control byte
  Esc                 cancel

Scrolling   mouse wheel, PgUp / PgDn; Enter jumps to the latest output
Copy        drag (OSC 52, and the clipboard when it can be opened)
Ctrl+C goes to the port; it does not quit. Quit with the prefix, then q.

Esc closes this (Up / Down scroll on a small screen)
";

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(area);
    frame.render_widget(TerminalPane { app }, chunks[0]);
    frame.render_widget(
        Paragraph::new(status_line(&app.status_view(), chunks[1].width)),
        chunks[1],
    );
    if app.show_help() {
        draw_help(frame, app.help_scroll(), app.palette());
    } else if let Some(body) = app.modal_body() {
        draw_modal(frame, &body, app.palette());
    }
}

struct TerminalPane<'a> {
    app: &'a App,
}

impl Widget for TerminalPane<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let palette = self.app.palette().clone();
        let bg = palette.background;
        let fg = palette.foreground;
        buf.set_style(
            area,
            Style::default()
                .fg(Color::Rgb(fg.r, fg.g, fg.b))
                .bg(Color::Rgb(bg.r, bg.g, bg.b)),
        );
        let snap = self.app.snapshot();
        let rows = (area.height as usize).min(snap.rows);
        let cols = (area.width as usize).min(snap.columns);
        for row in 0..rows {
            let mut col = 0;
            while col < cols {
                let cell = &snap.cells[row * snap.columns + col];
                if cell.width == 0 {
                    col += 1;
                    continue;
                }
                let width = (cell.width as usize).max(1);
                let style = cell_style(cell, &snap, row, col, &palette);
                let text = if cell.text.is_empty() {
                    " "
                } else {
                    cell.text.as_str()
                };
                put_cell(
                    buf,
                    area.x + col as u16,
                    area.y + row as u16,
                    text,
                    width,
                    style,
                );
                col += width;
            }
        }
    }
}

/// Draws one grid cell. A 2-cell cell is forced to two columns in the diff:
/// the grid says two, but ratatui may measure one (an ambiguous character),
/// and then the spacer is skipped and the next text is positioned absolutely.
fn put_cell(buf: &mut Buffer, x: u16, y: u16, text: &str, width: usize, style: Style) {
    buf.set_stringn(x, y, text, width, style);
    if width == 2 {
        buf[(x, y)].diff_option = CellDiffOption::ForcedWidth(NonZeroU16::new(2).unwrap());
    }
}

fn cell_style(
    cell: &ScreenCell,
    snap: &ScreenSnapshot,
    row: usize,
    col: usize,
    palette: &TerminalPalette,
) -> Style {
    let mut fg = resolve(&cell.fg, palette, &snap.color_overrides);
    let mut bg = resolve(&cell.bg, palette, &snap.color_overrides);
    if cell.flags.inverse {
        std::mem::swap(&mut fg, &mut bg);
    }
    if cell.selected {
        bg = palette.selection;
    }
    let mut style = Style::default()
        .fg(Color::Rgb(fg.r, fg.g, fg.b))
        .bg(Color::Rgb(bg.r, bg.g, bg.b));
    if cell.flags.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if cell.flags.italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if cell.flags.underline {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if snap.display_offset == 0 && snap.cursor.visible {
        let on_row = line_to_viewport(snap.cursor.line, snap.rows, snap.display_offset);
        let width = (cell.width as usize).max(1);
        if on_row == Some(row) && (col..col + width).contains(&snap.cursor.column) {
            style = style.add_modifier(Modifier::REVERSED);
        }
    }
    style
}

fn resolve(color: &CellColor, palette: &TerminalPalette, overrides: &[Option<Rgb>]) -> Rgb {
    match color {
        CellColor::Rgb(rgb) => *rgb,
        CellColor::Indexed(index) => {
            if let Some(Some(rgb)) = overrides.get(*index as usize) {
                return *rgb;
            }
            indexed(*index, palette)
        }
        CellColor::Named(name) => named(*name, palette),
    }
}

fn named(name: AnsiName, palette: &TerminalPalette) -> Rgb {
    let ansi = |index: usize| palette.ansi[index];
    match name {
        AnsiName::Black | AnsiName::DimBlack => ansi(0),
        AnsiName::Red | AnsiName::DimRed => ansi(1),
        AnsiName::Green | AnsiName::DimGreen => ansi(2),
        AnsiName::Yellow | AnsiName::DimYellow => ansi(3),
        AnsiName::Blue | AnsiName::DimBlue => ansi(4),
        AnsiName::Magenta | AnsiName::DimMagenta => ansi(5),
        AnsiName::Cyan | AnsiName::DimCyan => ansi(6),
        AnsiName::White | AnsiName::DimWhite => ansi(7),
        AnsiName::BrightBlack => ansi(8),
        AnsiName::BrightRed => ansi(9),
        AnsiName::BrightGreen => ansi(10),
        AnsiName::BrightYellow => ansi(11),
        AnsiName::BrightBlue => ansi(12),
        AnsiName::BrightMagenta => ansi(13),
        AnsiName::BrightCyan => ansi(14),
        AnsiName::BrightWhite => ansi(15),
        AnsiName::Foreground | AnsiName::BrightForeground | AnsiName::DimForeground => {
            palette.foreground
        }
        AnsiName::Background => palette.background,
        AnsiName::Cursor => palette.cursor,
    }
}

fn indexed(index: u8, palette: &TerminalPalette) -> Rgb {
    if index < 16 {
        return palette.ansi[index as usize];
    }
    if index >= 232 {
        let level = 8 + (index - 232) * 10;
        return Rgb {
            r: level,
            g: level,
            b: level,
        };
    }
    let offset = index - 16;
    let red = offset / 36;
    let green = (offset % 36) / 6;
    let blue = offset % 6;
    let step = |value: u8| {
        if value == 0 {
            0
        } else {
            55 + value * 40
        }
    };
    Rgb {
        r: step(red),
        g: step(green),
        b: step(blue),
    }
}

fn rgb_color(rgb: Rgb) -> Color {
    Color::Rgb(rgb.r, rgb.g, rgb.b)
}

/// The boxes are drawn in the theme's default colors, so they stay readable
/// on any theme.
fn box_style(palette: &TerminalPalette) -> Style {
    Style::default()
        .fg(rgb_color(palette.foreground))
        .bg(rgb_color(palette.background))
}

fn draw_modal(frame: &mut Frame, body: &ModalBody, palette: &TerminalPalette) {
    let height = (body.lines.len() as u16 + 2).clamp(5, 16);
    let area = centered(frame.area(), 68, height);
    frame.render_widget(Clear, area);
    let inner = area.width.saturating_sub(2) as usize;
    let rows = area.height.saturating_sub(2) as usize;
    let lines: Vec<Line> = visible_lines(body, rows)
        .into_iter()
        .map(|(text, selected)| {
            let style = if *selected {
                Style::default()
                    .fg(rgb_color(palette.background))
                    .bg(rgb_color(palette.foreground))
            } else {
                box_style(palette)
            };
            let text = match text.strip_prefix("> ") {
                Some(input) => format!("> {}", fit_tail(input, inner.saturating_sub(2))),
                None => text.clone(),
            };
            Line::from(Span::styled(text, style))
        })
        .collect();
    let paragraph = Paragraph::new(lines)
        .block(Block::bordered().title(body.title))
        .style(box_style(palette))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

/// The lines to draw in `rows` rows. When they do not all fit, the pinned
/// head stays and the rest is a window that ends at the selected line, so the
/// selection is always on screen.
fn visible_lines(body: &ModalBody, rows: usize) -> Vec<&(String, bool)> {
    if body.lines.len() <= rows {
        return body.lines.iter().collect();
    }
    let pinned = body.pinned.min(body.lines.len());
    let (head, list) = body.lines.split_at(pinned);
    let room = rows.saturating_sub(pinned).max(1);
    let selected = list.iter().position(|(_, selected)| *selected).unwrap_or(0);
    let start = (selected + 1).saturating_sub(room);
    head.iter()
        .chain(list.iter().skip(start).take(room))
        .take(rows)
        .collect()
}

/// `text` if it fits in `width` cells, otherwise `...` and its tail, so the end
/// of a long input (where the cursor is) stays visible on one line.
fn fit_tail(text: &str, width: usize) -> String {
    if Line::from(text).width() <= width {
        return text.to_string();
    }
    let mut tail: Vec<char> = Vec::new();
    let mut used = ELLIPSIS.len();
    for ch in text.chars().rev() {
        let w = Span::raw(ch.to_string()).width();
        if used + w > width {
            break;
        }
        used += w;
        tail.push(ch);
    }
    format!("{ELLIPSIS}{}", tail.iter().rev().collect::<String>())
}

/// The help box, sized from `HELP`: widest line plus border and padding by
/// line count plus border, both clamped to the area. The second value is how
/// many help lines are visible.
pub fn help_geometry(area: Rect) -> (Rect, usize) {
    let text = Text::from(HELP);
    let width = text.lines.iter().map(Line::width).max().unwrap_or(0) as u16 + 4;
    let height = text.lines.len() as u16 + 2;
    let rect = centered(area, width, height);
    (rect, rect.height.saturating_sub(2) as usize)
}

/// How far the help can scroll in an area of this size.
pub fn help_max_scroll(area: Rect) -> usize {
    let (_, visible) = help_geometry(area);
    HELP.lines().count().saturating_sub(visible)
}

fn draw_help(frame: &mut Frame, scroll: usize, palette: &TerminalPalette) {
    let (area, _) = help_geometry(frame.area());
    let scroll = scroll.min(help_max_scroll(frame.area())) as u16;
    frame.render_widget(Clear, area);
    let paragraph = Paragraph::new(HELP)
        .block(
            Block::bordered()
                .title("Help")
                .padding(Padding::horizontal(1)),
        )
        .style(box_style(palette))
        .scroll((scroll, 0));
    frame.render_widget(paragraph, area);
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use rs_term_core::default_palette;

    fn rows(buffer: &Buffer) -> Vec<String> {
        (0..buffer.area.height)
            .map(|y| {
                let mut row = String::new();
                let mut x = 0;
                while x < buffer.area.width {
                    let symbol = buffer[(x, y)].symbol();
                    row.push_str(symbol);
                    x += (Line::from(symbol).width() as u16).max(1);
                }
                row
            })
            .collect()
    }

    #[test]
    fn a_two_cell_grid_cell_is_forced_to_two_columns() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        let style = Style::default();
        put_cell(&mut buf, 0, 0, "\u{2460}", 2, style);
        put_cell(&mut buf, 2, 0, "\u{3042}", 2, style);
        put_cell(&mut buf, 4, 0, "a", 1, style);
        let forced = CellDiffOption::ForcedWidth(NonZeroU16::new(2).unwrap());
        assert_eq!(buf[(0, 0)].symbol(), "\u{2460}");
        assert_eq!(buf[(0, 0)].diff_option, forced);
        // A real wide character keeps working: still two columns.
        assert_eq!(buf[(2, 0)].symbol(), "\u{3042}");
        assert_eq!(buf[(2, 0)].diff_option, forced);
        assert_eq!(buf[(4, 0)].diff_option, CellDiffOption::None);
    }

    fn render_help(width: u16, height: u16, scroll: usize) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| draw_help(frame, scroll, &default_palette()))
            .unwrap();
        rows(terminal.backend().buffer())
    }

    #[test]
    fn help_shows_every_line_at_80x24() {
        let screen = render_help(80, 24, 0);
        for line in HELP.lines().filter(|line| !line.trim().is_empty()) {
            assert!(
                screen.iter().any(|row| row.contains(line.trim_end())),
                "missing {line:?} in {screen:#?}"
            );
        }
        assert!(screen.iter().any(|row| row.contains("Esc closes")));
    }

    #[test]
    fn help_scrolls_when_the_frame_is_too_small() {
        let area = Rect::new(0, 0, 80, 10);
        let max = help_max_scroll(area);
        assert!(max > 0);
        let top = render_help(80, 10, 0);
        assert!(!top.iter().any(|row| row.contains("Esc closes")));
        let bottom = render_help(80, 10, max + 5);
        assert!(bottom.iter().any(|row| row.contains("Esc closes")));
        assert_eq!(help_max_scroll(Rect::new(0, 0, 80, 24)), 0);
    }

    #[test]
    fn fit_tail_keeps_the_end_and_never_splits_a_wide_char() {
        assert_eq!(fit_tail("abc", 5), "abc");
        assert_eq!(fit_tail("abcdef", 5), "...ef");
        assert_eq!(fit_tail("あいうえお", 7), "...えお");
        assert_eq!(fit_tail("あいうえお", 6), "...お");
        assert_eq!(fit_tail("abc", 0), "...");
    }

    #[test]
    fn a_long_modal_input_stays_on_one_line_with_its_tail_visible() {
        let long = format!("/very/long/{}/end.log█", "d".repeat(100));
        let body = ModalBody {
            title: "Start log",
            lines: vec![
                ("Path of the log file".into(), false),
                (format!("> {long}"), false),
            ],
            pinned: 2,
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| draw_modal(frame, &body, &default_palette()))
            .unwrap();
        let screen = rows(terminal.backend().buffer());
        let row = screen
            .iter()
            .find(|row| row.contains("> ..."))
            .unwrap_or_else(|| panic!("{screen:#?}"));
        assert!(row.contains("/end.log█"), "{row:?}");
        assert!(Line::from(row.trim_end()).width() <= 80);
        assert_eq!(screen.iter().filter(|row| row.contains("ddd")).count(), 1);
    }

    fn picker_body(count: usize, selected: usize) -> ModalBody {
        let mut lines = vec![("> ".to_string(), false)];
        for index in 0..count {
            lines.push((format!("/dev/ttyS{index}"), index == selected));
        }
        ModalBody {
            title: "Port",
            lines,
            pinned: 1,
        }
    }

    fn render_modal(body: &ModalBody) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| draw_modal(frame, body, &default_palette()))
            .unwrap();
        rows(terminal.backend().buffer())
    }

    #[test]
    fn the_picker_scrolls_to_keep_the_selection_visible() {
        for selected in [0, 5, 12, 13, 14, 25, 39] {
            let screen = render_modal(&picker_body(40, selected));
            let want = format!("/dev/ttyS{selected} ");
            let hits: Vec<&String> = screen
                .iter()
                .filter(|row| format!("{} ", row.trim_end()).contains(&want))
                .collect();
            assert_eq!(hits.len(), 1, "selected {selected}: {screen:#?}");
            assert!(
                screen.iter().any(|row| row.contains("> ")),
                "the input line must stay: {screen:#?}"
            );
        }
    }

    #[test]
    fn a_short_picker_list_is_not_scrolled() {
        let screen = render_modal(&picker_body(3, 2));
        for index in 0..3 {
            assert!(screen
                .iter()
                .any(|row| row.contains(&format!("/dev/ttyS{index}"))));
        }
    }

    /// Borders and the input cursor are one cell on conhost (measured) and
    /// stay; every other UI character must be the same width in CJK mode.
    #[test]
    fn dialog_chrome_has_no_ambiguous_width_characters() {
        use unicode_width::UnicodeWidthChar;
        let mut screens = render_help(80, 24, 0);
        screens.extend(render_modal(&picker_body(3, 1)));
        let long = format!("> {}", "d".repeat(100));
        let body = ModalBody {
            title: "Start log",
            lines: vec![("Path of the log file".into(), false), (long, false)],
            pinned: 2,
        };
        screens.extend(render_modal(&body));
        for row in &screens {
            for ch in row.chars() {
                if (0x2500..=0x257F).contains(&(ch as u32)) || ch == '█' {
                    continue;
                }
                assert_eq!(ch.width(), ch.width_cjk(), "{ch:?} in {row:?}");
            }
        }
    }
}
