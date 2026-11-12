//! One status line: port, frame, encoding, link, log, RX/TX.

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use rs_term_core::Severity;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusView {
    pub prefix: bool,
    pub port: String,
    pub frame: String,
    pub encoding: String,
    pub link: String,
    pub log: String,
    pub rx: u64,
    pub tx: u64,
    pub message: Option<(Severity, String)>,
}

pub fn status_line(view: &StatusView) -> Line<'static> {
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
        format!("{}  ", view.link),
        normal.fg(link_color(&view.link)),
    ));
    spans.push(Span::styled(
        format!("{}  RX {}  TX {}", view.log, view.rx, view.tx),
        normal,
    ));
    if let Some((severity, message)) = &view.message {
        let styled = match severity {
            Severity::Error => format!("  エラー: {message}"),
            Severity::Warn => format!("  注意: {message}"),
            Severity::Info => format!("  {message}"),
        };
        let color = match severity {
            Severity::Error => Color::Rgb(0xFF, 0x60, 0x60),
            Severity::Warn => Color::Rgb(0xFF, 0xD7, 0x00),
            Severity::Info => Color::Rgb(0xD4, 0xD4, 0xD4),
        };
        spans.push(Span::styled(styled, normal.fg(color)));
    }
    Line::from(spans)
}

fn link_color(link: &str) -> Color {
    match link {
        "接続" => Color::Rgb(0x40, 0xC0, 0x40),
        "接続中" => Color::Rgb(0xFF, 0xD7, 0x00),
        "喪失" => Color::Rgb(0xFF, 0x60, 0x60),
        _ => Color::Rgb(0x80, 0x80, 0x80),
    }
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
            link: "接続".into(),
            log: "LOG on".into(),
            rx: 12,
            tx: 3,
            message: Some((Severity::Error, "使用中".into())),
        };
        let backend = TestBackend::new(120, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                frame.render_widget(Paragraph::new(status_line(&view)), frame.area());
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
}
