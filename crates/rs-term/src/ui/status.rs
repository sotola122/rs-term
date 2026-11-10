//! The status line at the bottom of each pane (design §5, screen design §3).
//!
//! This module is GPUI-free: it turns the facts about one pane into fields
//! with a colour role. The drawing code lays the fields out left to right.

use rs_term_core::{DataBits, Flow, LogSession, Parity, Severity, StopBits};

/// A colour role. The drawing code maps it to a colour of the theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// The status line text colour.
    Normal,
    /// Connected, green dot.
    Green,
    /// Opening, unconnected warning, info-yellow.
    Yellow,
    /// Disconnected by the user, unconnected.
    Gray,
    /// Lost, error, recording log.
    Red,
    /// Focused-pane badges.
    Accent,
}

/// Which optional field a field belongs to. Used for the drop order when the
/// pane is narrow (§5.2: 7 → 5 → 6 → 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Badge,
    Name,
    Link,
    Speed,
    Flow,
    Encoding,
    Extras,
    Log,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub slot: Slot,
    pub text: String,
    pub tone: Tone,
}

/// The link as the status line shows it (screen design §3.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkView {
    /// No port has been chosen yet.
    Unchosen,
    /// The port is remembered but the user disconnected.
    Disconnected(String),
    Opening(String),
    Open(String),
    Lost(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub focused_badge: Option<&'static str>,
    pub name: String,
    pub link: LinkView,
    /// Baud, data, parity, stop. `None` while no port is chosen.
    pub frame: Option<(u32, DataBits, Parity, StopBits)>,
    pub flow: Flow,
    pub encoding: &'static str,
    pub local_echo: bool,
    /// `Some` only when the newline setting is not the default.
    pub newline: Option<String>,
    pub dtr: bool,
    pub log: LogView,
    pub message: Option<(Severity, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogView {
    Off,
    Recording { kind: &'static str },
    Paused { kind: &'static str },
    Failed,
}

impl LogView {
    pub fn from_session(log: &LogSession) -> Self {
        let kind = |target: &rs_term_core::LogTarget| match target {
            rs_term_core::LogTarget::Binary { .. } => "bin",
            rs_term_core::LogTarget::Text { .. } => "text",
        };
        match log {
            LogSession::Stopped => LogView::Off,
            LogSession::Running(target) => LogView::Recording { kind: kind(target) },
            LogSession::Paused(target) => LogView::Paused { kind: kind(target) },
            LogSession::Failed { .. } => LogView::Failed,
        }
    }
}

/// Field widths are estimated in characters; the drawing code measures the
/// real text. `Line::fit` decides what to drop.
pub fn build(facts: &Facts) -> Vec<Field> {
    let mut out = Vec::new();
    if let Some(badge) = facts.focused_badge {
        out.push(Field {
            slot: Slot::Badge,
            text: badge.to_string(),
            tone: Tone::Accent,
        });
    }
    if !facts.name.is_empty() {
        out.push(Field {
            slot: Slot::Name,
            text: format!("[{}]", truncate(&facts.name, 16)),
            tone: Tone::Normal,
        });
    }
    let (dot, link_text, link_tone) = match &facts.link {
        LinkView::Unchosen => ("●", "未接続".to_string(), Tone::Gray),
        LinkView::Disconnected(port) => ("●", port.clone(), Tone::Gray),
        LinkView::Opening(port) => ("●", format!("{port} 接続中…"), Tone::Yellow),
        LinkView::Open(port) => ("●", port.clone(), Tone::Green),
        LinkView::Lost(port) => ("●", format!("{port} 切断"), Tone::Red),
    };
    out.push(Field {
        slot: Slot::Link,
        text: format!("{dot} {link_text}"),
        tone: link_tone,
    });
    out.push(Field {
        slot: Slot::Speed,
        text: match &facts.frame {
            Some((baud, data, parity, stop)) => format!(
                "{baud} {}{}{}",
                data_label(*data),
                parity_label(*parity),
                stop_label(*stop)
            ),
            None => "—".to_string(),
        },
        tone: Tone::Normal,
    });
    out.push(Field {
        slot: Slot::Flow,
        text: match (&facts.frame, facts.flow) {
            (None, _) => "—".to_string(),
            (Some(_), Flow::None) => "Flow: None".to_string(),
            (Some(_), Flow::XonXoff) => "Flow: XON/XOFF".to_string(),
            (Some(_), Flow::RtsCts) => "Flow: RTS/CTS（送信のみ）".to_string(),
        },
        tone: Tone::Normal,
    });
    out.push(Field {
        slot: Slot::Encoding,
        text: facts.encoding.to_string(),
        tone: Tone::Normal,
    });
    let mut extras = Vec::new();
    if facts.local_echo {
        extras.push("ECHO".to_string());
    }
    if let Some(newline) = &facts.newline {
        extras.push(newline.clone());
    }
    if facts.dtr {
        extras.push("DTR".to_string());
    }
    if !extras.is_empty() {
        out.push(Field {
            slot: Slot::Extras,
            text: extras.join(" "),
            tone: Tone::Normal,
        });
    }
    out.push(match &facts.log {
        LogView::Off => Field {
            slot: Slot::Log,
            text: "LOG off".into(),
            tone: Tone::Normal,
        },
        LogView::Recording { kind } => Field {
            slot: Slot::Log,
            text: format!("● LOG {kind}"),
            tone: Tone::Red,
        },
        LogView::Paused { kind } => Field {
            slot: Slot::Log,
            text: format!("LOG 一時停止 {kind}"),
            tone: Tone::Yellow,
        },
        LogView::Failed => Field {
            slot: Slot::Log,
            text: "LOG 失敗".into(),
            tone: Tone::Red,
        },
    });
    out
}

/// The message text as drawn: `エラー: 本文`, `注意: 本文`, or `本文`.
pub fn message_text(severity: Severity, text: &str) -> (String, Tone) {
    match severity {
        Severity::Error => (format!("エラー: {text}"), Tone::Red),
        Severity::Warn => (format!("注意: {text}"), Tone::Yellow),
        Severity::Info => (text.to_string(), Tone::Normal),
    }
}

/// Columns of the status line that may be dropped, in the order the design
/// gives: extras (7), flow (5), encoding (6), speed (4).
const DROP_ORDER: [Slot; 4] = [Slot::Extras, Slot::Flow, Slot::Encoding, Slot::Speed];

/// Keep the message at least `message_min` wide. `available` and the widths
/// are in the same unit. Returns the fields to draw; optional fields go in
/// the drop order until the rest fits.
pub fn fit(
    fields: Vec<Field>,
    available: f32,
    message_min: f32,
    width_of: impl Fn(&str) -> f32,
    message_width: f32,
) -> Vec<Field> {
    let gap = 14.0;
    let total = |fields: &[Field]| -> f32 {
        let widths: f32 = fields.iter().map(|field| width_of(&field.text)).sum();
        let gaps = gap * fields.len().saturating_sub(1) as f32;
        widths + gaps
    };
    let mut kept = fields;
    let mut next = 0;
    while total(&kept) + gap + message_width.max(message_min) > available && next < DROP_ORDER.len()
    {
        let slot = DROP_ORDER[next];
        kept.retain(|field| field.slot != slot);
        next += 1;
    }
    kept
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let head: String = text.chars().take(max).collect();
        format!("{head}…")
    }
}

pub fn data_label(bits: DataBits) -> &'static str {
    match bits {
        DataBits::Five => "5",
        DataBits::Six => "6",
        DataBits::Seven => "7",
        DataBits::Eight => "8",
    }
}

pub fn parity_label(parity: Parity) -> &'static str {
    match parity {
        Parity::None => "N",
        Parity::Even => "E",
        Parity::Odd => "O",
    }
}

pub fn stop_label(stop: StopBits) -> &'static str {
    match stop {
        StopBits::One => "1",
        StopBits::Two => "2",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Facts {
        Facts {
            focused_badge: None,
            name: String::new(),
            link: LinkView::Unchosen,
            frame: None,
            flow: Flow::None,
            encoding: "Shift_JIS",
            local_echo: false,
            newline: None,
            dtr: false,
            log: LogView::Off,
            message: None,
        }
    }

    fn texts(fields: &[Field]) -> Vec<String> {
        fields.iter().map(|field| field.text.clone()).collect()
    }

    /// Design screen §3.3: 起動直後（1 枚、未接続）.
    #[test]
    fn unconnected_without_port_reads_disconnected_text() {
        let fields = build(&base());
        assert_eq!(
            texts(&fields),
            ["● 未接続", "—", "—", "Shift_JIS", "LOG off"]
        );
        assert_eq!(fields[0].tone, Tone::Gray);
    }

    /// A chosen port that the user disconnected keeps its name, grey.
    #[test]
    fn user_disconnected_keeps_the_port_name_grey() {
        let facts = Facts {
            link: LinkView::Disconnected("COM3".into()),
            frame: Some((115200, DataBits::Eight, Parity::None, StopBits::One)),
            ..base()
        };
        let fields = build(&facts);
        assert_eq!(fields[0].text, "● COM3");
        assert_eq!(fields[0].tone, Tone::Gray);
    }

    /// Design screen §3.3: 接続（s1 左）.
    #[test]
    fn open_line_matches_mock_s1() {
        let facts = Facts {
            link: LinkView::Open("COM3".into()),
            frame: Some((115200, DataBits::Eight, Parity::None, StopBits::One)),
            encoding: "UTF-8",
            log: LogView::Recording { kind: "text" },
            ..base()
        };
        let fields = build(&facts);
        assert_eq!(
            texts(&fields),
            ["● COM3", "115200 8N1", "Flow: None", "UTF-8", "● LOG text"]
        );
        assert_eq!(fields[0].tone, Tone::Green);
        assert_eq!(fields[4].tone, Tone::Red);
    }

    /// Design screen §3.3 / s1 右: RTS/CTS shows that it only sends.
    #[test]
    fn rtscts_shows_send_only() {
        let facts = Facts {
            link: LinkView::Open("COM7".into()),
            frame: Some((921600, DataBits::Eight, Parity::None, StopBits::One)),
            flow: Flow::RtsCts,
            ..base()
        };
        assert!(texts(&build(&facts)).contains(&"Flow: RTS/CTS（送信のみ）".to_string()));
    }

    /// Local echo, a non-default newline, and DTR appear in one extras field.
    #[test]
    fn extras_are_one_field_in_order() {
        let facts = Facts {
            link: LinkView::Open("COM3".into()),
            frame: Some((115200, DataBits::Eight, Parity::None, StopBits::One)),
            local_echo: true,
            newline: Some("改行 受信:CRLF 送信:CRLF".into()),
            dtr: true,
            ..base()
        };
        let fields = build(&facts);
        let extras = fields
            .iter()
            .find(|field| field.slot == Slot::Extras)
            .unwrap();
        assert_eq!(extras.text, "ECHO 改行 受信:CRLF 送信:CRLF DTR");
    }

    /// Design §5.2: a lost port shows red and keeps its name.
    #[test]
    fn lost_link_is_red_with_name() {
        let facts = Facts {
            link: LinkView::Lost("COM3".into()),
            ..base()
        };
        let fields = build(&facts);
        assert_eq!(fields[0].text, "● COM3 切断");
        assert_eq!(fields[0].tone, Tone::Red);
    }

    #[test]
    fn opening_link_is_yellow_with_dots() {
        let facts = Facts {
            link: LinkView::Opening("COM3".into()),
            ..base()
        };
        let fields = build(&facts);
        assert_eq!(fields[0].text, "● COM3 接続中…");
        assert_eq!(fields[0].tone, Tone::Yellow);
    }

    #[test]
    fn name_is_truncated_at_sixteen_characters() {
        let facts = Facts {
            name: "あいうえおかきくけこさしすせそたち".into(),
            ..base()
        };
        let fields = build(&facts);
        assert_eq!(fields[0].text, "[あいうえおかきくけこさしすせそた…]");
    }

    /// Design §5.2: when the line is narrow the optional fields go in the
    /// order 7 → 5 → 6 → 4, and the message keeps its minimum width.
    #[test]
    fn fields_drop_in_order_when_narrow() {
        let facts = Facts {
            link: LinkView::Open("COM3".into()),
            frame: Some((115200, DataBits::Eight, Parity::None, StopBits::One)),
            local_echo: true,
            ..base()
        };
        let fields = build(&facts);
        let measure = |text: &str| text.chars().count() as f32 * 7.0;
        // Everything fits in a wide line.
        let wide = fit(fields.clone(), 2000.0, 160.0, measure, 0.0);
        assert_eq!(wide.len(), fields.len());
        // Narrow: extras goes first, then flow, encoding, speed.
        let narrow = fit(fields.clone(), 150.0 + 160.0, 160.0, measure, 0.0);
        assert!(narrow.iter().all(|field| field.slot != Slot::Extras));
        let tighter = fit(fields.clone(), 60.0 + 160.0, 160.0, measure, 0.0);
        assert!(tighter.iter().all(|field| !matches!(
            field.slot,
            Slot::Extras | Slot::Flow | Slot::Encoding | Slot::Speed
        )));
        assert!(tighter.iter().any(|field| field.slot == Slot::Link));
    }

    #[test]
    fn messages_are_prefixed_by_severity() {
        assert_eq!(
            message_text(Severity::Error, "未接続"),
            ("エラー: 未接続".into(), Tone::Red)
        );
        assert_eq!(message_text(Severity::Warn, "x").0, "注意: x");
        assert_eq!(message_text(Severity::Info, "x").0, "x");
    }
}
