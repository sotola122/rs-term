//! One status line: link symbol, port, frame and encoding on the left; log,
//! RX/TX and a message on the right, or the command list while the prefix is
//! armed.

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use rs_term_core::{Rgb, Severity, UiColors};

/// Cut text ends with ASCII dots: "…" is two cells wide on a Japanese
/// console (code page 932), which would overflow the row by one cell.
pub const ELLIPSIS: &str = "...";

/// What the right side shows while the prefix is armed.
pub const PREFIX_COMMANDS: &str = "c connect  d disconnect  r reconnect  b break  l log  L stop log  s send  f file  u baud  e encoding  p paste  t theme  ? help  q quit  Esc cancel";

/// The keys alone, for when the full list does not fit.
pub const PREFIX_KEYS: &str = "c d r b l L s f u e p t ? q Esc";

/// The link as the status line shows it: one ASCII symbol and its color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Disconnected,
    Opening,
    Open,
    Lost,
}

impl LinkState {
    fn symbol(self) -> &'static str {
        match self {
            Self::Disconnected => "-",
            Self::Opening => "~",
            Self::Open => "*",
            Self::Lost => "!",
        }
    }

    fn color(self, ui: &UiColors) -> Color {
        match self {
            Self::Open => color(ui.ok),
            Self::Opening => color(ui.warn),
            Self::Lost => color(ui.error),
            Self::Disconnected => color(ui.muted),
        }
    }
}

fn color(rgb: Rgb) -> Color {
    Color::Rgb(rgb.r, rgb.g, rgb.b)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusView {
    pub colors: UiColors,
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

const GAP: usize = 2;

/// `text` cut to `room` cells with `...`, never splitting a wide character,
/// or `None` if even the dots do not fit.
fn cut(text: &str, room: usize) -> Option<String> {
    if Span::raw(text).width() <= room {
        return Some(text.to_string());
    }
    let budget = room.checked_sub(ELLIPSIS.len())?;
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
    Some(shown)
}

/// The status line for a row `width` cells wide. The left items (link
/// symbol, port, frame, encoding) are whole. The state items (`LOG`, `RX`,
/// `TX`) sit at the right edge and are dropped if they do not fit beside the
/// left items. A message goes just left of them in the room that remains,
/// cut with `...` by display width, or dropped if even `...` does not fit.
///
/// The link symbol is always the first cell. While the prefix is armed the
/// symbol is followed by `PREFIX`, and the commands come first; the state
/// items and message are not shown. The order is: the normal left items with
/// the full list, with the keys alone, then only `* PREFIX` with the full
/// list, with the keys alone, and last the keys cut with `...`.
pub fn status_line(view: &StatusView, width: u16) -> Line<'static> {
    let ui = &view.colors;
    let normal = Style::default()
        .fg(color(ui.status_fg))
        .bg(color(ui.status_bg));
    let warn = color(ui.warn);
    let width = width as usize;
    let mut spans = vec![Span::styled(
        view.link.symbol(),
        normal.fg(view.link.color(ui)),
    )];
    if view.prefix {
        spans.push(Span::styled(" PREFIX  ", normal.fg(warn)));
    } else {
        spans.push(Span::styled(" ", normal));
    }
    spans.push(Span::styled(
        format!("{}  {}  {}", view.port, view.frame, view.encoding),
        normal,
    ));
    let mut left: usize = spans.iter().map(Span::width).sum();
    if view.prefix {
        let fits = |left: usize, list: &str| left + GAP + Span::raw(list).width() <= width;
        let mut list = PREFIX_COMMANDS;
        if !fits(left, list) {
            list = PREFIX_KEYS;
            if !fits(left, list) {
                // Give the whole row to the list: keep the symbol and PREFIX.
                spans.truncate(2);
                spans[1] = Span::styled(" PREFIX", normal.fg(warn));
                left = spans.iter().map(Span::width).sum();
                if fits(left, PREFIX_COMMANDS) {
                    list = PREFIX_COMMANDS;
                }
            }
        }
        let room = width.saturating_sub(left);
        if let Some(shown) = cut(list, room.saturating_sub(GAP)).filter(|_| room >= GAP) {
            let pad = room - Span::raw(shown.as_str()).width();
            spans.push(Span::styled(" ".repeat(pad), normal));
            spans.push(Span::styled(shown, normal));
        }
        return Line::from(spans);
    }
    let state = format!("{}  RX {}  TX {}", view.log, view.rx, view.tx);
    let state_w = Span::raw(state.as_str()).width();
    if left + GAP + state_w > width {
        return Line::from(spans);
    }
    let mut used = left + state_w;
    let mut shown = None;
    if let Some((severity, text)) = &view.message {
        let (text, fg) = match severity {
            Severity::Error => (format!("ERR {text}"), Some(color(ui.error))),
            Severity::Warn => (format!("WARN {text}"), Some(warn)),
            Severity::Info => (text.clone(), None),
        };
        let room = (width - left - state_w).saturating_sub(2 * GAP);
        if let Some(text) = cut(&text, room) {
            used += GAP + Span::raw(text.as_str()).width();
            shown = Some(Span::styled(text, fg.map_or(normal, |fg| normal.fg(fg))));
        }
    }
    spans.push(Span::styled(" ".repeat(width - used), normal));
    if let Some(message) = shown {
        spans.push(message);
        spans.push(Span::styled(" ".repeat(GAP), normal));
    }
    spans.push(Span::styled(state, normal));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::widgets::Paragraph;
    use ratatui::Terminal;
    use rs_term_core::{default_palette, palette_by_name};

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
            colors: default_palette().ui,
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

    fn cells(text: &str) -> usize {
        Line::from(text).width()
    }

    const LEFT: &str = "* /dev/ttyUSB0  9600 8N1  Shift_JIS";
    const ARMED: &str = "* PREFIX  /dev/ttyUSB0  9600 8N1  Shift_JIS";
    const STATE: &str = "LOG on  RX 12  TX 3";

    /// The row for `message` (already formatted, may be empty) at `width`.
    fn expected(message: &str, width: usize) -> String {
        let right = if message.is_empty() {
            STATE.to_string()
        } else {
            format!("{message}  {STATE}")
        };
        let pad = " ".repeat(width - cells(LEFT) - cells(&right));
        format!("{LEFT}{pad}{right}")
    }

    #[test]
    fn the_idle_row_has_the_left_items_and_the_state_at_the_right_edge() {
        assert_eq!((cells(LEFT), cells(STATE)), (35, 19));
        for width in [80u16, 120] {
            let row = render(&sample(None), width);
            assert_eq!(row, expected("", width as usize));
            assert!(!row.contains("help"), "{row:?}");
        }
    }

    #[test]
    fn messages_sit_left_of_the_state_with_their_severity_tag() {
        for (message, shown) in [
            (Some((Severity::Error, "port busy")), "ERR port busy"),
            (Some((Severity::Warn, "careful")), "WARN careful"),
            (Some((Severity::Info, "Theme: nord")), "Theme: nord"),
        ] {
            for width in [80u16, 120] {
                let row = render(&sample(message), width);
                assert_eq!(row, expected(shown, width as usize), "{row:?}");
                assert_eq!(cells(&row), width as usize);
            }
        }
    }

    #[test]
    fn a_long_message_is_cut_with_three_dots() {
        let view = sample(Some((
            Severity::Error,
            "Port is busy (opened by another process)",
        )));
        // 80 - 35 - 19 - 4 = 22 cells: 19 for text and 3 for the dots.
        let row = render(&view, 80);
        assert_eq!(row, expected("ERR Port is busy (o...", 80));
        // Exactly 3 cells (the dots alone) still shows them.
        let width = 35 + 19 + 4 + 3;
        let row = render(&view, width);
        assert_eq!(row, expected("...", width as usize));
    }

    #[test]
    fn the_message_is_dropped_when_not_even_the_dots_fit() {
        let view = sample(Some((Severity::Warn, "broken")));
        let width = 35 + 19 + 4 + 2;
        let row = render(&view, width);
        assert_eq!(row, expected("", width as usize));
    }

    #[test]
    fn the_state_is_dropped_when_it_does_not_fit_and_the_left_stays_whole() {
        let view = sample(Some((Severity::Error, "port busy")));
        assert_eq!(
            render(&view, 35 + 2 + 19 - 1),
            LEFT.to_string() + &" ".repeat(19 + 2 - 1)
        );
        assert_eq!(render(&view, 40).trim_end(), LEFT);
        // Exactly the left plus gap plus state: state shown, no message.
        let row = render(&view, 35 + 2 + 19);
        assert_eq!(row, format!("{LEFT}  {STATE}"));
        // Narrower than the left: the left is still not cut by us.
        let line = status_line(&sample(None), 20);
        assert_eq!(line.to_string(), LEFT);
    }

    #[test]
    fn a_wide_char_is_never_split() {
        let view = sample(Some((Severity::Info, "ああああ")));
        // Room for the message is width - 35 - 19 - 4.
        let row = render(&view, 35 + 19 + 4 + 8);
        assert!(row.contains("ああああ  LOG"), "{row:?}");
        let row = render(&view, 35 + 19 + 4 + 7);
        assert!(row.contains("ああ...  LOG"), "{row:?}");
        let row = render(&view, 35 + 19 + 4 + 6);
        assert!(row.contains("あ...  LOG"), "{row:?}");
        let row = render(&view, 35 + 19 + 4 + 5);
        assert!(row.contains("あ...  LOG"), "{row:?}");
    }

    #[test]
    fn the_link_symbol_and_its_color_follow_the_state() {
        let ui = sample(None).colors;
        for (link, symbol, rgb) in [
            (LinkState::Open, "*", ui.ok),
            (LinkState::Opening, "~", ui.warn),
            (LinkState::Lost, "!", ui.error),
            (LinkState::Disconnected, "-", ui.muted),
        ] {
            let mut view = sample(None);
            view.link = link;
            let line = status_line(&view, 80);
            assert_eq!(line.spans[0].content, symbol);
            assert_eq!(line.spans[0].style.fg, Some(color(rgb)));
            let row = render(&view, 80);
            assert!(
                row.starts_with(&format!("{symbol} /dev/ttyUSB0")),
                "{row:?}"
            );
            for word in ["connected", "connecting", "lost", "disconnected"] {
                assert!(!row.contains(word), "{row:?}");
            }
        }
    }

    #[test]
    fn an_armed_prefix_shows_the_commands_instead_of_state_and_message() {
        let mut view = sample(Some((Severity::Error, "port busy")));
        view.prefix = true;
        let row = render(&view, 120);
        assert!(row.starts_with(ARMED), "{row:?}");
        assert!(!row.contains("port busy") && !row.contains("RX"), "{row:?}");
        // 120 - 43 = 77 cells: the full list (146) does not fit, the keys do.
        assert!(row.ends_with(&format!("  {PREFIX_KEYS}")), "{row:?}");
        assert_eq!(cells(&row), 120);
        let row = render(&view, 250);
        assert!(row.ends_with("q quit  Esc cancel"), "{row:?}");
        assert!(row.contains("c connect  d disconnect"), "{row:?}");
        assert!(!row.contains("..."), "{row:?}");
        // The message and state return once the prefix is released.
        view.prefix = false;
        assert_eq!(render(&view, 120), expected("ERR port busy", 120));
    }

    #[test]
    fn the_prefix_list_falls_back_to_keys_and_then_to_dots() {
        let mut view = sample(None);
        view.prefix = true;
        let left = cells(ARMED);
        let keys = cells(PREFIX_KEYS);
        let full = cells(PREFIX_COMMANDS);
        assert_eq!((left, keys, full), (43, 31, 146));
        let row = render(&view, (left + 2 + full) as u16);
        assert_eq!(row, format!("{ARMED}  {PREFIX_COMMANDS}"));
        let row = render(&view, (left + 1 + full) as u16);
        assert!(row.starts_with("* PREFIX  /dev"), "{row:?}");
        assert!(row.ends_with(&format!("  {PREFIX_KEYS}")), "{row:?}");
        // The keys fit exactly beside the left items.
        let row = render(&view, (left + 2 + keys) as u16);
        assert_eq!(row, format!("{ARMED}  {PREFIX_KEYS}"));
        let row = render(&view, 80);
        let pad = " ".repeat(80 - left - keys);
        assert_eq!(row, format!("{ARMED}{pad}{PREFIX_KEYS}"));
        // One cell short of the keys beside the left items: collapsed.
        let row = render(&view, (left + 1 + keys) as u16);
        assert!(row.starts_with("* PREFIX "), "{row:?}");
        assert!(!row.contains("/dev/ttyUSB0"), "{row:?}");
        assert!(row.ends_with(PREFIX_KEYS), "{row:?}");
        // Collapsed rows: exact fit, then cut with dots, then nothing.
        let row = render(&view, 8 + 2 + 31);
        assert_eq!(row, format!("* PREFIX  {PREFIX_KEYS}"));
        let row = render(&view, 8 + 2 + 30);
        assert_eq!(row, "* PREFIX  c d r b l L s f u e p t ? q...");
        let row = render(&view, 8 + 2 + 3 - 1);
        assert_eq!(row.trim_end(), "* PREFIX");
    }

    #[test]
    fn the_colors_follow_the_theme_and_stay_readable() {
        for name in ["dark", "light", "solarized-light"] {
            let mut view = sample(Some((Severity::Error, "x")));
            view.colors = palette_by_name(name).unwrap().ui;
            let line = status_line(&view, 80);
            for span in &line.spans {
                assert_eq!(span.style.bg, Some(color(view.colors.status_bg)), "{name}");
            }
            let err = line.spans.iter().find(|s| s.content == "ERR x").unwrap();
            assert_eq!(err.style.fg, Some(color(view.colors.error)), "{name}");
            let state = line.spans.last().unwrap().style;
            assert_eq!(state.fg, Some(color(view.colors.status_fg)), "{name}");
            assert_ne!(state.fg, state.bg, "{name}");
            view.message = Some((Severity::Warn, "x".into()));
            let line = status_line(&view, 80);
            let warn = line.spans.iter().find(|s| s.content == "WARN x").unwrap();
            assert_eq!(warn.style.fg, Some(color(view.colors.warn)), "{name}");
        }
        let dark = status_line(&sample(None), 80).spans[0].style.bg;
        let mut light = sample(None);
        light.colors = palette_by_name("light").unwrap().ui;
        assert_ne!(dark, status_line(&light, 80).spans[0].style.bg);
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
