//! One status line: port, frame, encoding, link, log, RX/TX, and a right-hand
//! hint, message or command list.

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use rs_term_core::Severity;

/// Cut text ends with ASCII dots: "…" is two cells wide on a Japanese
/// console (code page 932), which would overflow the row by one cell.
pub const ELLIPSIS: &str = "...";

/// What the right side shows while the prefix is armed.
pub const PREFIX_COMMANDS: &str = "c connect  d disconnect  r reconnect  b break  l log  L stop log  s send  f file  u baud  e encoding  p paste  ? help  q quit  Esc cancel";

/// The keys alone, for when the full list does not fit.
pub const PREFIX_KEYS: &str = "c d r b l L s f u e p ? q Esc";

/// The link as the status line shows it: a word and its color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Disconnected,
    Opening,
    Open,
    Lost,
}

impl LinkState {
    fn word(self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Opening => "connecting",
            Self::Open => "connected",
            Self::Lost => "lost",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Open => Color::Rgb(0x40, 0xC0, 0x40),
            Self::Opening => Color::Rgb(0xFF, 0xD7, 0x00),
            Self::Lost => Color::Rgb(0xFF, 0x60, 0x60),
            Self::Disconnected => Color::Rgb(0x80, 0x80, 0x80),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusView {
    pub prefix: bool,
    /// The configured prefix, e.g. "Ctrl+A", for the idle hint.
    pub prefix_label: String,
    pub port: String,
    pub frame: String,
    pub encoding: String,
    pub link: LinkState,
    pub log: String,
    pub rx: u64,
    pub tx: u64,
    pub message: Option<(Severity, String)>,
}

/// The status line for a row `width` cells wide. The left items are always
/// whole. The right side is the command list while the prefix is armed, else
/// the message, else the `<prefix> ? help` hint. It sits at the right edge;
/// the command list falls back to the keys alone when the full list does not
/// fit. If the text does not fit it is cut with `...` by display width, and if even
/// `...` does not fit it is dropped.
pub fn status_line(view: &StatusView, width: u16) -> Line<'static> {
    let normal = Style::default()
        .fg(Color::Rgb(0xD4, 0xD4, 0xD4))
        .bg(Color::Rgb(0x20, 0x20, 0x20));
    let mut spans = Vec::new();
    if view.prefix {
        spans.push(Span::styled(
            "PREFIX  ",
            normal.fg(Color::Rgb(0xFF, 0xD7, 0x00)),
        ));
    }
    spans.push(Span::styled(
        format!("{}  {}  {}  ", view.port, view.frame, view.encoding),
        normal,
    ));
    spans.push(Span::styled(
        format!("{}  ", view.link.word()),
        normal.fg(view.link.color()),
    ));
    spans.push(Span::styled(
        format!("{}  RX {}  TX {}", view.log, view.rx, view.tx),
        normal,
    ));
    let dim = Color::Rgb(0x80, 0x80, 0x80);
    let left: usize = spans.iter().map(Span::width).sum();
    let room = (width as usize).saturating_sub(left);
    const GAP: usize = 2;
    let (text, color) = if view.prefix {
        let full = Span::raw(PREFIX_COMMANDS).width();
        let list = if room >= GAP + full {
            PREFIX_COMMANDS
        } else {
            PREFIX_KEYS
        };
        (list.to_string(), normal.fg)
    } else {
        match &view.message {
            Some((severity, message)) => match severity {
                Severity::Error => (
                    format!("Error: {message}"),
                    Some(Color::Rgb(0xFF, 0x60, 0x60)),
                ),
                Severity::Warn => (
                    format!("Warning: {message}"),
                    Some(Color::Rgb(0xFF, 0xD7, 0x00)),
                ),
                Severity::Info => (message.clone(), normal.fg),
            },
            None => (format!("{} ? help", view.prefix_label), Some(dim)),
        }
    };
    let style = color.map_or(normal, |color| normal.fg(color));
    let full = Span::raw(text.as_str()).width();
    if room >= GAP + full {
        spans.push(Span::styled(" ".repeat(room - full), normal));
        spans.push(Span::styled(text, style));
    } else if room >= GAP + ELLIPSIS.len() {
        let budget = room - GAP - ELLIPSIS.len();
        let mut shown = String::new();
        let mut used = 0;
        for ch in text.chars() {
            let w = Span::raw(ch.to_string()).width();
            if used + w > budget {
                break;
            }
            shown.push(ch);
            used += w;
        }
        shown.push_str(ELLIPSIS);
        spans.push(Span::styled(" ".repeat(GAP), normal));
        spans.push(Span::styled(shown, style));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::widgets::Paragraph;
    use ratatui::Terminal;

    fn buffer_text(buffer: &Buffer) -> String {
        let mut text = String::new();
        let mut skip = 0u16;
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                let symbol = buffer[(x, y)].symbol();
                let width = symbol_width(symbol);
                text.push_str(symbol);
                if width > 1 {
                    skip = (width as u16) - 1;
                }
            }
        }
        text
    }

    fn symbol_width(symbol: &str) -> usize {
        symbol
            .chars()
            .map(|ch| if char_is_wide(ch) { 2 } else { 1 })
            .sum()
    }

    fn char_is_wide(ch: char) -> bool {
        matches!(
            ch,
            '\u{1100}'..='\u{115F}'
                | '\u{2329}'..='\u{232A}'
                | '\u{2E80}'..='\u{A4CF}'
                | '\u{AC00}'..='\u{D7A3}'
                | '\u{F900}'..='\u{FAFF}'
                | '\u{FE10}'..='\u{FE19}'
                | '\u{FE30}'..='\u{FE6F}'
                | '\u{FF00}'..='\u{FF60}'
                | '\u{FFE0}'..='\u{FFE6}'
                | '\u{1F300}'..='\u{1F64F}'
                | '\u{1F900}'..='\u{1F9FF}'
                | '\u{20000}'..='\u{3FFFD}'
        )
    }

    fn sample(message: Option<(Severity, &str)>) -> StatusView {
        StatusView {
            prefix: false,
            prefix_label: "Ctrl+A".into(),
            port: "/dev/ttyUSB0".into(),
            frame: "9600 8N1".into(),
            encoding: "Shift_JIS".into(),
            link: LinkState::Open,
            log: "LOG on".into(),
            rx: 12,
            tx: 3,
            message: message.map(|(severity, text)| (severity, text.to_string())),
        }
    }

    fn render(view: &StatusView, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
        terminal
            .draw(|frame| {
                frame.render_widget(
                    Paragraph::new(status_line(view, frame.area().width)),
                    frame.area(),
                );
            })
            .unwrap();
        buffer_text(terminal.backend().buffer())
    }

    fn cells(text: &str) -> usize {
        Line::from(text).width()
    }

    const LEFT: &str = "/dev/ttyUSB0  9600 8N1  Shift_JIS  connected  LOG on  RX 12  TX 3";

    #[test]
    fn status_line_shows_port_frame_encoding_link_log_and_counts() {
        let row = render(&sample(Some((Severity::Error, "port busy"))), 120);
        for needle in [
            "/dev/ttyUSB0",
            "9600 8N1",
            "Shift_JIS",
            "connected",
            "LOG on",
            "RX 12",
            "TX 3",
            "Error: port busy",
        ] {
            assert!(row.contains(needle), "missing {needle} in {row:?}");
        }
    }

    #[test]
    fn the_hint_is_right_aligned_at_80_and_120() {
        for width in [80u16, 120] {
            let row = render(&sample(None), width);
            assert!(row.starts_with(LEFT), "{row:?}");
            assert!(row.ends_with("Ctrl+A ? help"), "{row:?}");
            assert_eq!(cells(&row), width as usize, "{row:?}");
        }
    }

    #[test]
    fn the_hint_uses_the_configured_prefix() {
        let mut view = sample(None);
        view.prefix_label = "Ctrl+]".into();
        let row = render(&view, 120);
        assert!(row.ends_with("Ctrl+] ? help"), "{row:?}");
    }

    #[test]
    fn the_hint_is_dropped_at_40_and_the_left_stays_whole() {
        let row = render(&sample(None), 40);
        assert!(!row.contains("help"), "{row:?}");
        assert!(!row.contains("..."), "{row:?}");
        assert!(row.starts_with(&LEFT[..40]), "{row:?}");
        assert!(cells(&row) <= 40);
        // Exactly the left width leaves no room for the hint either.
        let row = render(&sample(None), cells(LEFT) as u16);
        assert_eq!(row, LEFT);
    }

    #[test]
    fn a_message_wins_over_the_hint() {
        let row = render(
            &sample(Some((Severity::Info, "Baud rate set to 9600"))),
            120,
        );
        assert!(row.ends_with("Baud rate set to 9600"), "{row:?}");
        assert!(!row.contains("? help"), "{row:?}");
        let row = render(&sample(Some((Severity::Warn, "careful"))), 120);
        assert!(row.ends_with("Warning: careful"), "{row:?}");
    }

    #[test]
    fn an_armed_prefix_shows_the_commands_even_with_a_message() {
        let mut view = sample(Some((Severity::Error, "port busy")));
        view.prefix = true;
        let left = format!("PREFIX  {LEFT}");
        let row = render(&view, 120);
        assert!(row.starts_with(&left), "{row:?}");
        assert!(!row.contains("port busy"), "{row:?}");
        // 120 - 73 = 47 cells: the full list (137) does not fit, the keys do.
        assert!(row.ends_with(&format!("  {PREFIX_KEYS}")), "{row:?}");
        assert!(!row.contains("..."), "{row:?}");
        assert_eq!(cells(&row), 120);
        // Room for everything: the full list, no dots.
        let row = render(&view, 250);
        assert!(row.ends_with("q quit  Esc cancel"), "{row:?}");
        assert!(row.contains("c connect  d disconnect"), "{row:?}");
        assert!(!row.contains("..."), "{row:?}");
        // The message returns once the prefix is released.
        view.prefix = false;
        assert!(render(&view, 120).ends_with("Error: port busy"));
    }

    #[test]
    fn the_prefix_list_falls_back_to_keys_and_then_to_dots() {
        let mut view = sample(None);
        view.prefix = true;
        let left = cells(&format!("PREFIX  {LEFT}"));
        assert_eq!(left, 73);
        let keys = cells(PREFIX_KEYS);
        let full = cells(PREFIX_COMMANDS);
        assert_eq!((keys, full), (29, 137));
        // The widest row where the full list still fits, and one cell less.
        let row = render(&view, (left + 2 + full) as u16);
        assert!(row.ends_with(&format!("  {PREFIX_COMMANDS}")), "{row:?}");
        assert_eq!(cells(&row), left + 2 + full);
        let row = render(&view, (left + 1 + full) as u16);
        assert!(row.ends_with(&format!("  {PREFIX_KEYS}")), "{row:?}");
        // The keys fit exactly, and one cell less cuts them.
        let row = render(&view, (left + 2 + keys) as u16);
        assert_eq!(row, format!("PREFIX  {LEFT}  {PREFIX_KEYS}"));
        let row = render(&view, (left + 1 + keys) as u16);
        assert!(row.ends_with("  c d r b l L s f u e p ? q..."), "{row:?}");
        assert_eq!(cells(&row), left + 1 + keys);
        // At 80 columns only seven cells are left after the left items.
        let row = render(&view, 80);
        assert_eq!(row, format!("PREFIX  {LEFT}  c ..."));
        assert_eq!(cells(&row), 80);
        // The left items are never cut, and nothing is shown without room.
        let row = render(&view, left as u16 + 4);
        assert_eq!(row.trim_end(), format!("PREFIX  {LEFT}"));
    }

    #[test]
    fn a_long_message_is_cut_with_three_dots_at_80() {
        let view = sample(Some((
            Severity::Error,
            "Port is busy (opened by another process)",
        )));
        let row = render(&view, 80);
        // 80 - 65 left cells = 15; 2 gap, 3 dots, 10 for text.
        assert!(row.starts_with(LEFT), "{row:?}");
        assert!(row.ends_with("  Error: Por..."), "{row:?}");
        assert_eq!(cells(&row), 80);
    }

    #[test]
    fn the_message_is_dropped_when_there_is_no_room_for_the_dots() {
        let view = sample(Some((Severity::Warn, "broken")));
        let left = cells(LEFT) as u16;
        let row = render(&view, left + 4);
        assert_eq!(row.trim_end(), LEFT);
        let row = render(&view, left + 5);
        assert_eq!(row.trim_end(), format!("{LEFT}  ..."));
    }

    #[test]
    fn a_wide_char_is_never_split() {
        let view = sample(Some((Severity::Info, "ああああ")));
        let left = cells(LEFT) as u16;
        // 2 gap + 3 dots + budget: 4 cells keep two wide chars, 3 and 2 keep one.
        let row = render(&view, left + 9);
        assert!(row.trim_end().ends_with("  ああ..."), "{row:?}");
        let row = render(&view, left + 8);
        assert!(row.trim_end().ends_with("  あ..."), "{row:?}");
        let row = render(&view, left + 7);
        assert!(row.trim_end().ends_with("  あ..."), "{row:?}");
    }

    #[test]
    fn the_status_line_has_no_ambiguous_width_characters() {
        use unicode_width::UnicodeWidthChar;
        let mut views = vec![sample(None)];
        for link in [
            LinkState::Disconnected,
            LinkState::Opening,
            LinkState::Open,
            LinkState::Lost,
        ] {
            let mut view = sample(None);
            view.link = link;
            views.push(view);
        }
        let mut armed = sample(None);
        armed.prefix = true;
        views.push(armed);
        views.push(sample(Some((Severity::Error, "x".repeat(200).as_str()))));
        views.push(sample(Some((Severity::Warn, "Port is busy"))));
        for view in views {
            for width in [40u16, 80, 120, 250] {
                for ch in render(&view, width).chars() {
                    assert_eq!(ch.width(), ch.width_cjk(), "{ch:?} in {view:?}");
                }
            }
        }
    }
}
