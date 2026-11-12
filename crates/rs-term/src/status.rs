//! One status line: port, frame, encoding, link, log, RX/TX.

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use rs_term_core::Severity;

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
            Self::Disconnected => "未接続",
            Self::Opening => "接続中",
            Self::Open => "接続",
            Self::Lost => "喪失",
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
/// whole. The message sits at the right edge; if it does not fit it is cut
/// with `…` by display width, and if even `…` does not fit it is dropped.
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
    if let Some((severity, message)) = &view.message {
        let text = match severity {
            Severity::Error => format!("エラー: {message}"),
            Severity::Warn => format!("注意: {message}"),
            Severity::Info => message.clone(),
        };
        let color = match severity {
            Severity::Error => Color::Rgb(0xFF, 0x60, 0x60),
            Severity::Warn => Color::Rgb(0xFF, 0xD7, 0x00),
            Severity::Info => Color::Rgb(0xD4, 0xD4, 0xD4),
        };
        let left: usize = spans.iter().map(Span::width).sum();
        let room = (width as usize).saturating_sub(left);
        const GAP: usize = 2;
        let full = Span::raw(text.as_str()).width();
        if room >= GAP + full {
            spans.push(Span::styled(" ".repeat(room - full), normal));
            spans.push(Span::styled(text, normal.fg(color)));
        } else if room > GAP {
            let mut shown = String::new();
            let mut used = 0;
            for ch in text.chars() {
                let w = Span::raw(ch.to_string()).width();
                if used + w > room - GAP - 1 {
                    break;
                }
                shown.push(ch);
                used += w;
            }
            shown.push('…');
            spans.push(Span::styled(" ".repeat(GAP), normal));
            spans.push(Span::styled(shown, normal.fg(color)));
        }
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

    #[test]
    fn status_line_shows_port_frame_encoding_link_log_and_counts() {
        let view = StatusView {
            prefix: false,
            port: "/dev/ttyUSB0".into(),
            frame: "9600 8N1".into(),
            encoding: "Shift_JIS".into(),
            link: LinkState::Open,
            log: "LOG on".into(),
            rx: 12,
            tx: 3,
            message: Some((Severity::Error, "使用中".into())),
        };
        let backend = TestBackend::new(120, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                frame.render_widget(Paragraph::new(status_line(&view, 120)), frame.area());
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let text = buffer_text(&buffer);
        for needle in [
            "/dev/ttyUSB0",
            "9600 8N1",
            "Shift_JIS",
            "接続",
            "LOG on",
            "RX 12",
            "TX 3",
            "エラー: 使用中",
        ] {
            assert!(text.contains(needle), "missing {needle} in {text:?}");
        }
    }

    fn sample(message: Option<(Severity, &str)>) -> StatusView {
        StatusView {
            prefix: false,
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

    const LEFT: &str = "/dev/ttyUSB0  9600 8N1  Shift_JIS  接続  LOG on  RX 12  TX 3";

    #[test]
    fn message_is_right_aligned_when_it_fits() {
        let view = sample(Some((Severity::Error, "使用中")));
        let row = render(&view, 120);
        assert!(row.starts_with(LEFT), "{row:?}");
        assert!(row.ends_with("エラー: 使用中"), "{row:?}");
        assert_eq!(Line::from(row.as_str()).width(), 120);
    }

    #[test]
    fn long_message_is_cut_with_an_ellipsis_at_80() {
        let view = sample(Some((
            Severity::Error,
            "ポートは使用中（他のプロセスが開いている）",
        )));
        let row = render(&view, 80);
        // 80 - 60 left cells = 20; 2 gap, 17 for text: 16 cells of text and "…".
        assert!(row.starts_with(LEFT), "{row:?}");
        assert!(row.trim_end().ends_with("  エラー: ポートは…"), "{row:?}");
        assert!(Line::from(row.as_str()).width() <= 80);
    }

    #[test]
    fn message_is_dropped_when_there_is_no_room_at_40() {
        let view = sample(Some((Severity::Warn, "壊れている")));
        let row = render(&view, 40);
        assert!(!row.contains("注意"), "{row:?}");
        assert!(!row.contains('…'), "{row:?}");
        assert!(
            row.starts_with("/dev/ttyUSB0  9600 8N1  Shift_JIS  接続"),
            "{row:?}"
        );
        // Exactly the left width leaves no room for the gap and "…".
        let row = render(&view, 62);
        assert_eq!(row.trim_end(), LEFT);
        let row = render(&view, 63);
        assert_eq!(row.trim_end(), format!("{LEFT}  …"));
    }

    #[test]
    fn a_wide_char_is_never_split() {
        let view = sample(Some((Severity::Info, "ああああ")));
        // 60 left + 2 gap + budget 4 + "…": 5 cells of room for text.
        let row = render(&view, 68);
        assert!(row.trim_end().ends_with("  ああ…"), "{row:?}");
        let row = render(&view, 67);
        assert!(row.trim_end().ends_with("  ああ…"), "{row:?}");
        let row = render(&view, 66);
        assert!(row.trim_end().ends_with("  あ…"), "{row:?}");
    }
}
