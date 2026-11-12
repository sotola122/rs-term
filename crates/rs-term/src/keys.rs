//! Crossterm keys to session input, and the prefix-key state machine.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use rs_term_core::{encode_key, Direction, Input, KeyInput};

/// Ctrl plus an ASCII letter (stored lowercase), `\` or `]`. Default is
/// Ctrl+A. `cli::parse_prefix` is the only place that builds a non-default one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prefix {
    pub ctrl: char,
}

impl Prefix {
    /// The control byte the prefix stands for (Ctrl+A is 0x01), from the same
    /// table the session uses for Ctrl keys.
    pub fn byte(self) -> u8 {
        encode_key(&KeyInput::Ctrl(self.ctrl), false)
            .ok()
            .and_then(|bytes| bytes.first().copied())
            .unwrap_or(0)
    }

    /// Some terminals report Ctrl+\ and Ctrl+] as Ctrl+4 and Ctrl+5.
    fn digit_alias(self) -> Option<char> {
        match self.ctrl {
            '\\' => Some('4'),
            ']' => Some('5'),
            _ => None,
        }
    }
}

impl Default for Prefix {
    fn default() -> Self {
        Self { ctrl: 'a' }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Terminal,
    Prefix,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Connect,
    Disconnect,
    Reconnect,
    Break,
    LogStart,
    LogStop,
    SendString,
    SendFile,
    Baud,
    Encoding,
    Help,
    Quit,
    Paste,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    ArmPrefix,
    LiteralPrefix,
    Run(Command),
    Input(Input),
    /// `page` is PgUp / PgDn, with or without Shift. Otherwise the caller uses a few lines.
    Scroll {
        up: bool,
        page: bool,
    },
    Ignore,
}

pub fn classify(key: &KeyEvent, prefix: Prefix, focus: Focus) -> Action {
    if key.kind == KeyEventKind::Release {
        return Action::Ignore;
    }
    match focus {
        Focus::Help => match key.code {
            KeyCode::Esc | KeyCode::Char('?') => Action::Run(Command::Cancel),
            KeyCode::Char('q') => Action::Run(Command::Quit),
            KeyCode::Up | KeyCode::PageUp => Action::Scroll {
                up: true,
                page: key.code == KeyCode::PageUp,
            },
            KeyCode::Down | KeyCode::PageDown => Action::Scroll {
                up: false,
                page: key.code == KeyCode::PageDown,
            },
            _ => Action::Ignore,
        },
        Focus::Prefix => prefix_action(key, prefix),
        Focus::Terminal => {
            if is_prefix(key, prefix) {
                return Action::ArmPrefix;
            }
            if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) {
                return Action::Scroll {
                    up: key.code == KeyCode::PageUp,
                    page: true,
                };
            }
            match key_to_input(key) {
                Some(input) => Action::Input(input),
                None => Action::Ignore,
            }
        }
    }
}

pub fn key_to_input(key: &KeyEvent) -> Option<Input> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let input = match key.code {
        KeyCode::Enter => KeyInput::Enter,
        KeyCode::Backspace => KeyInput::Backspace,
        KeyCode::Tab if !ctrl && !alt => KeyInput::Tab,
        KeyCode::Esc => KeyInput::Escape,
        KeyCode::Delete => KeyInput::Delete,
        KeyCode::Home => KeyInput::Home,
        KeyCode::End => KeyInput::End,
        KeyCode::Up => KeyInput::Arrow(Direction::Up),
        KeyCode::Down => KeyInput::Arrow(Direction::Down),
        KeyCode::Left => KeyInput::Arrow(Direction::Left),
        KeyCode::Right => KeyInput::Arrow(Direction::Right),
        KeyCode::F(n) if (1..=12).contains(&n) => KeyInput::Function(n),
        KeyCode::Char(ch) if alt && !ctrl => KeyInput::Alt(ch),
        KeyCode::Char(ch) if ctrl => KeyInput::Ctrl(ch),
        KeyCode::Char(ch) if !ctrl => KeyInput::Char(ch),
        _ => return None,
    };
    Some(Input::Key(input))
}

fn prefix_action(key: &KeyEvent, prefix: Prefix) -> Action {
    if is_prefix(key, prefix) {
        return Action::LiteralPrefix;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) || key.modifiers.contains(KeyModifiers::ALT) {
        return Action::Ignore;
    }
    let command = match key.code {
        KeyCode::Esc => Command::Cancel,
        KeyCode::Char('c') => Command::Connect,
        KeyCode::Char('d') => Command::Disconnect,
        KeyCode::Char('r') => Command::Reconnect,
        KeyCode::Char('b') => Command::Break,
        KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::SHIFT) => Command::LogStop,
        KeyCode::Char('L') => Command::LogStop,
        KeyCode::Char('l') => Command::LogStart,
        KeyCode::Char('s') => Command::SendString,
        KeyCode::Char('f') => Command::SendFile,
        KeyCode::Char('u') => Command::Baud,
        KeyCode::Char('e') => Command::Encoding,
        KeyCode::Char('p') => Command::Paste,
        KeyCode::Char('?') => Command::Help,
        KeyCode::Char('q') => Command::Quit,
        _ => return Action::Ignore,
    };
    Action::Run(command)
}

pub fn is_prefix(key: &KeyEvent, prefix: Prefix) -> bool {
    if key.modifiers.contains(KeyModifiers::ALT) || key.modifiers.contains(KeyModifiers::SHIFT) {
        return false;
    }
    let KeyCode::Char(ch) = key.code else {
        return false;
    };
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return ch.to_ascii_lowercase() == prefix.ctrl || Some(ch) == prefix.digit_alias();
    }
    (ch as u32) < 32 && ch as u8 == prefix.byte()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rs_term_core::encode_key;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn bytes(input: &Input) -> Vec<u8> {
        let Input::Key(key) = input else {
            panic!("expected a key");
        };
        encode_key(key, false).unwrap()
    }

    #[test]
    fn keys_become_the_wire_bytes() {
        let prefix = Prefix::default();
        let up = classify(
            &key(KeyCode::Up, KeyModifiers::empty()),
            prefix,
            Focus::Terminal,
        );
        let Action::Input(input) = up else {
            panic!("up: {up:?}");
        };
        assert_eq!(bytes(&input), b"\x1b[A");

        let f1 = classify(
            &key(KeyCode::F(1), KeyModifiers::empty()),
            prefix,
            Focus::Terminal,
        );
        let Action::Input(input) = f1 else {
            panic!("f1: {f1:?}");
        };
        assert_eq!(bytes(&input), b"\x1bOP");

        let ctrl_c = classify(
            &key(KeyCode::Char('c'), KeyModifiers::CONTROL),
            prefix,
            Focus::Terminal,
        );
        let Action::Input(input) = ctrl_c else {
            panic!("ctrl-c: {ctrl_c:?}");
        };
        assert_eq!(bytes(&input), vec![0x03]);

        let kana = classify(
            &key(KeyCode::Char('あ'), KeyModifiers::empty()),
            prefix,
            Focus::Terminal,
        );
        let Action::Input(input) = kana else {
            panic!("kana: {kana:?}");
        };
        // Encoding a character is the session's job (core tests cover it).
        assert_eq!(input, Input::Key(KeyInput::Char('あ')));
    }

    #[test]
    fn prefix_dispatch() {
        let prefix = Prefix { ctrl: 'a' };
        let ctrl_a = key(KeyCode::Char('a'), KeyModifiers::CONTROL);
        assert_eq!(
            classify(&ctrl_a, prefix, Focus::Terminal),
            Action::ArmPrefix
        );
        assert_eq!(
            classify(&ctrl_a, prefix, Focus::Prefix),
            Action::LiteralPrefix
        );
        assert_eq!(encode_key(&KeyInput::Ctrl('a'), false).unwrap(), vec![0x01]);

        assert_eq!(
            classify(
                &key(KeyCode::Char('c'), KeyModifiers::empty()),
                prefix,
                Focus::Prefix
            ),
            Action::Run(Command::Connect)
        );
        assert_eq!(
            classify(
                &key(KeyCode::Char('q'), KeyModifiers::empty()),
                prefix,
                Focus::Prefix
            ),
            Action::Run(Command::Quit)
        );
        assert_eq!(
            classify(
                &key(KeyCode::Char('L'), KeyModifiers::SHIFT),
                prefix,
                Focus::Prefix
            ),
            Action::Run(Command::LogStop)
        );
        assert_eq!(
            classify(
                &key(KeyCode::Char('z'), KeyModifiers::empty()),
                prefix,
                Focus::Prefix
            ),
            Action::Ignore
        );
        assert_eq!(
            classify(
                &key(KeyCode::PageUp, KeyModifiers::empty()),
                prefix,
                Focus::Terminal
            ),
            Action::Scroll {
                up: true,
                page: true
            }
        );
        assert_eq!(
            classify(
                &key(KeyCode::PageDown, KeyModifiers::empty()),
                prefix,
                Focus::Terminal
            ),
            Action::Scroll {
                up: false,
                page: true
            }
        );
        assert_eq!(
            classify(
                &key(KeyCode::PageUp, KeyModifiers::SHIFT),
                prefix,
                Focus::Terminal
            ),
            Action::Scroll {
                up: true,
                page: true
            }
        );

        let mut release = ctrl_a;
        release.kind = KeyEventKind::Release;
        assert_eq!(classify(&release, prefix, Focus::Terminal), Action::Ignore);
    }

    #[test]
    fn help_scrolls_with_up_down_and_pages() {
        let prefix = Prefix::default();
        let scroll = |code| classify(&key(code, KeyModifiers::empty()), prefix, Focus::Help);
        assert_eq!(
            scroll(KeyCode::Up),
            Action::Scroll {
                up: true,
                page: false
            }
        );
        assert_eq!(
            scroll(KeyCode::PageDown),
            Action::Scroll {
                up: false,
                page: true
            }
        );
        assert_eq!(scroll(KeyCode::Esc), Action::Run(Command::Cancel));
        assert_eq!(scroll(KeyCode::Char('?')), Action::Run(Command::Cancel));
    }

    #[test]
    fn is_prefix_agrees_with_the_parsed_prefix() {
        let ctrl = |ch| key(KeyCode::Char(ch), KeyModifiers::CONTROL);
        let z = crate::cli::parse_prefix("C-z").unwrap();
        assert!(is_prefix(&ctrl('z'), z));
        assert!(is_prefix(&ctrl('Z'), z));
        assert!(!is_prefix(&ctrl('a'), z));
        assert!(!is_prefix(
            &key(KeyCode::Char('z'), KeyModifiers::empty()),
            z
        ));
        assert_eq!(z.byte(), 0x1A);

        let backslash = crate::cli::parse_prefix("ctrl+\\").unwrap();
        assert_eq!(backslash.byte(), 0x1C);
        assert!(is_prefix(&ctrl('\\'), backslash));
        assert!(is_prefix(&ctrl('4'), backslash));
        assert!(!is_prefix(&ctrl('5'), backslash));
        assert_eq!(
            classify(&ctrl('\\'), backslash, Focus::Terminal),
            Action::ArmPrefix
        );
        assert_eq!(
            classify(&ctrl('\\'), backslash, Focus::Prefix),
            Action::LiteralPrefix
        );

        let bracket = crate::cli::parse_prefix("ctrl-]").unwrap();
        assert_eq!(bracket.byte(), 0x1D);
        assert!(is_prefix(&ctrl(']'), bracket));
        assert!(is_prefix(&ctrl('5'), bracket));
        assert!(!is_prefix(&ctrl('4'), bracket));
        assert!(is_prefix(
            &key(KeyCode::Char('\u{1d}'), KeyModifiers::empty()),
            bracket
        ));
        assert_eq!(Prefix::default().byte(), 0x01);
    }
}
