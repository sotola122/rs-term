// Not wired into the app yet: phase 2 (keymap) and phase 3 (layout.toml) connect it.
#![allow(dead_code)]

//! Pane keys, following herdr.
//!
//! herdr reserves one prefix key (default `ctrl+b`). The next key after it
//! is a pane command; everything else goes to the program in the pane. The
//! rules below are herdr's (`src/client/shell/input.rs`, the `Terminal`,
//! `Prefix`, and `Resize` arms of the key router):
//!
//! - Terminal mode: a direct binding runs its action; the prefix enters
//!   prefix mode; any other key goes to the pane.
//! - Prefix mode: the prefix again sends the prefix key itself to the pane;
//!   `esc` cancels; a `prefix+…` binding runs its action; any other key is
//!   dropped. Every case returns to terminal mode first.
//! - Resize mode: `esc`, `enter`, the prefix, or the resize-mode key leave
//!   it; `h/j/k/l` and the arrows resize; other keys are ignored.
//!
//! Defaults are herdr's `KeysConfig::default` (`src/config/model.rs`).
//! `keymap.toml` uses herdr's `[keys]` syntax.

use std::fmt;

/// Pane commands. The names are herdr's `[keys]` field names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneAction {
    SplitVertical,
    SplitHorizontal,
    FocusLeft,
    FocusDown,
    FocusUp,
    FocusRight,
    SwapLeft,
    SwapDown,
    SwapUp,
    SwapRight,
    CycleNext,
    CyclePrevious,
    ClosePane,
    Zoom,
    ResizeMode,
    ResizeLeft,
    ResizeDown,
    ResizeUp,
    ResizeRight,
    /// herdr `help` (`prefix+?`) lists the active bindings. rs-term opens
    /// the command palette, which shows every command with its keys.
    Help,
}

impl PaneAction {
    pub const ALL: [PaneAction; 20] = [
        PaneAction::SplitVertical,
        PaneAction::SplitHorizontal,
        PaneAction::FocusLeft,
        PaneAction::FocusDown,
        PaneAction::FocusUp,
        PaneAction::FocusRight,
        PaneAction::SwapLeft,
        PaneAction::SwapDown,
        PaneAction::SwapUp,
        PaneAction::SwapRight,
        PaneAction::CycleNext,
        PaneAction::CyclePrevious,
        PaneAction::ClosePane,
        PaneAction::Zoom,
        PaneAction::ResizeMode,
        PaneAction::ResizeLeft,
        PaneAction::ResizeDown,
        PaneAction::ResizeUp,
        PaneAction::ResizeRight,
        PaneAction::Help,
    ];

    /// The herdr `[keys]` field.
    pub fn key_name(self) -> &'static str {
        match self {
            PaneAction::SplitVertical => "split_vertical",
            PaneAction::SplitHorizontal => "split_horizontal",
            PaneAction::FocusLeft => "focus_pane_left",
            PaneAction::FocusDown => "focus_pane_down",
            PaneAction::FocusUp => "focus_pane_up",
            PaneAction::FocusRight => "focus_pane_right",
            PaneAction::SwapLeft => "swap_pane_left",
            PaneAction::SwapDown => "swap_pane_down",
            PaneAction::SwapUp => "swap_pane_up",
            PaneAction::SwapRight => "swap_pane_right",
            PaneAction::CycleNext => "cycle_pane_next",
            PaneAction::CyclePrevious => "cycle_pane_previous",
            PaneAction::ClosePane => "close_pane",
            PaneAction::Zoom => "zoom",
            PaneAction::ResizeMode => "resize_mode",
            PaneAction::ResizeLeft => "resize_pane_left",
            PaneAction::ResizeDown => "resize_pane_down",
            PaneAction::ResizeUp => "resize_pane_up",
            PaneAction::ResizeRight => "resize_pane_right",
            PaneAction::Help => "help",
        }
    }

    fn from_key_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|action| action.key_name() == name)
    }

    /// herdr's default binding (`KeysConfig::default`).
    fn herdr_default(self) -> &'static [&'static str] {
        match self {
            PaneAction::SplitVertical => &["prefix+v"],
            PaneAction::SplitHorizontal => &["prefix+minus"],
            PaneAction::FocusLeft => &["prefix+h"],
            PaneAction::FocusDown => &["prefix+j"],
            PaneAction::FocusUp => &["prefix+k"],
            PaneAction::FocusRight => &["prefix+l"],
            PaneAction::SwapLeft => &["prefix+shift+h"],
            PaneAction::SwapDown => &["prefix+shift+j"],
            PaneAction::SwapUp => &["prefix+shift+k"],
            PaneAction::SwapRight => &["prefix+shift+l"],
            PaneAction::CycleNext => &["prefix+tab"],
            PaneAction::CyclePrevious => &["prefix+shift+tab"],
            PaneAction::ClosePane => &["prefix+x"],
            PaneAction::Zoom => &["prefix+z"],
            PaneAction::ResizeMode => &["prefix+r"],
            PaneAction::ResizeLeft
            | PaneAction::ResizeDown
            | PaneAction::ResizeUp
            | PaneAction::ResizeRight => &[],
            PaneAction::Help => &["prefix+?"],
        }
    }
}

/// One key with modifiers. `key` is normalized: lowercase letters, `-` for
/// `minus`, `escape`, `enter`, `tab`, `space`, arrow names, `f1`…
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub platform: bool,
    pub key: String,
    /// The character the key types, when it types one (`?` for shift+/).
    pub text: Option<String>,
}

impl Chord {
    pub fn plain(key: &str) -> Self {
        Self {
            ctrl: false,
            alt: false,
            shift: false,
            platform: false,
            key: key.to_string(),
            text: None,
        }
    }

    /// From a GPUI keystroke: `key`, `key_char`, and modifiers.
    pub fn from_event(
        key: &str,
        key_char: Option<&str>,
        ctrl: bool,
        alt: bool,
        shift: bool,
        platform: bool,
    ) -> Self {
        Self {
            ctrl,
            alt,
            shift,
            platform,
            key: normalize_key(key),
            text: key_char.map(str::to_string),
        }
    }

    /// Does an event match this binding?
    fn matches(&self, event: &Chord) -> bool {
        if self.ctrl != event.ctrl || self.alt != event.alt || self.platform != event.platform {
            return false;
        }
        if self.key == event.key && self.shift == event.shift {
            return true;
        }
        // A symbol bound without `shift+` (herdr `prefix+?`) matches the
        // character it types, whatever the layout needs to type it.
        !self.shift
            && self.key.chars().count() == 1
            && !self.key.chars().all(|ch| ch.is_ascii_alphanumeric())
            && event.text.as_deref() == Some(self.key.as_str())
    }
}

impl fmt::Display for Chord {
    /// `Ctrl+B`, `Shift+Tab`, `-`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<String> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".into());
        }
        if self.alt {
            parts.push("Alt".into());
        }
        if self.platform {
            parts.push("Super".into());
        }
        if self.shift {
            parts.push("Shift".into());
        }
        let key = match self.key.as_str() {
            "escape" => "Esc".to_string(),
            "enter" => "Enter".to_string(),
            "tab" => "Tab".to_string(),
            "space" => "Space".to_string(),
            "left" => "←".to_string(),
            "right" => "→".to_string(),
            "up" => "↑".to_string(),
            "down" => "↓".to_string(),
            other if other.len() == 1 && (self.ctrl || self.alt || self.shift) => {
                other.to_ascii_uppercase()
            }
            other => other.to_string(),
        };
        parts.push(key);
        write!(f, "{}", parts.join("+"))
    }
}

fn normalize_key(key: &str) -> String {
    let lower = key.to_ascii_lowercase();
    match lower.as_str() {
        "minus" => "-".into(),
        "plus" => "+".into(),
        "comma" => ",".into(),
        "period" => ".".into(),
        "backtick" => "`".into(),
        "ampersand" => "&".into(),
        "question" => "?".into(),
        "esc" => "escape".into(),
        "return" => "enter".into(),
        "arrowleft" => "left".into(),
        "arrowright" => "right".into(),
        "arrowup" => "up".into(),
        "arrowdown" => "down".into(),
        _ => lower,
    }
}

/// Where a binding fires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binding {
    /// `prefix+…`
    Prefix(Chord),
    /// A chord pressed in terminal mode.
    Direct(Chord),
}

/// Parse one herdr binding string. `""` and `"unset"` are no binding.
pub fn parse_binding(text: &str) -> Result<Option<Binding>, String> {
    let text = text.trim();
    if text.is_empty() || text == "unset" {
        return Ok(None);
    }
    let (prefixed, rest) = match text.strip_prefix("prefix+") {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let chord = parse_chord(rest)?;
    Ok(Some(if prefixed {
        Binding::Prefix(chord)
    } else {
        Binding::Direct(chord)
    }))
}

/// `ctrl+alt+d`, `shift+tab`, `minus`, `?`.
pub fn parse_chord(text: &str) -> Result<Chord, String> {
    let mut chord = Chord::plain("");
    // `+` alone, or a trailing `++`, is the plus key.
    let (mods, key) = if text == "+" {
        ("", "+")
    } else if let Some(stripped) = text.strip_suffix("++") {
        (stripped, "+")
    } else {
        match text.rsplit_once('+') {
            Some((mods, key)) => (mods, key),
            None => ("", text),
        }
    };
    for part in mods.split('+').filter(|part| !part.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => chord.ctrl = true,
            "alt" | "option" | "meta" => chord.alt = true,
            "shift" => chord.shift = true,
            "cmd" | "super" | "win" => chord.platform = true,
            other => return Err(format!("修飾キー `{other}` は使えない")),
        }
    }
    if key.is_empty() {
        return Err(format!("キーが無い: `{text}`"));
    }
    chord.key = normalize_key(key);
    // A single uppercase letter means shift plus that letter.
    if key.len() == 1 && key.chars().all(|ch| ch.is_ascii_uppercase()) {
        chord.shift = true;
    }
    Ok(chord)
}

#[derive(Debug, Clone)]
pub struct Keymap {
    pub prefix: Vec<Chord>,
    bindings: Vec<(Binding, PaneAction)>,
}

impl Keymap {
    /// herdr's defaults: prefix `ctrl+b`, every pane command behind it, no
    /// direct chords.
    pub fn herdr_default() -> Self {
        let mut bindings = Vec::new();
        for action in PaneAction::ALL {
            for text in action.herdr_default() {
                if let Ok(Some(binding)) = parse_binding(text) {
                    bindings.push((binding, action));
                }
            }
        }
        Self {
            prefix: vec![parse_chord("ctrl+b").expect("default prefix")],
            bindings,
        }
    }

    /// Apply the `[keys]` table of keymap.toml on top of the defaults. A
    /// field replaces that action's default bindings, as in herdr. Problems
    /// are returned in Japanese; the bad entry is skipped and the rest
    /// applies.
    pub fn with_keys_table(mut self, table: &toml::Table) -> (Self, Vec<String>) {
        let mut notes = Vec::new();
        for (name, value) in table {
            let texts: Vec<String> = match value {
                toml::Value::String(text) => vec![text.clone()],
                toml::Value::Array(items) => {
                    let mut texts = Vec::new();
                    for item in items {
                        match item.as_str() {
                            Some(text) => texts.push(text.to_string()),
                            None => notes.push(format!("[keys] {name}: 文字列の配列にする")),
                        }
                    }
                    texts
                }
                _ => {
                    notes.push(format!("[keys] {name}: 文字列か文字列の配列にする"));
                    continue;
                }
            };
            if name == "prefix" {
                let mut prefix = Vec::new();
                for text in &texts {
                    match parse_chord(text) {
                        Ok(chord) => prefix.push(chord),
                        Err(error) => notes.push(format!("[keys] prefix: {error}")),
                    }
                }
                if prefix.is_empty() {
                    notes.push("[keys] prefix が空なので既定の ctrl+b のまま".into());
                } else {
                    self.prefix = prefix;
                }
                continue;
            }
            let Some(action) = PaneAction::from_key_name(name) else {
                notes.push(format!("[keys] {name}: rs-term に無い操作なので無視"));
                continue;
            };
            self.bindings.retain(|(_, bound)| *bound != action);
            for text in &texts {
                match parse_binding(text) {
                    Ok(Some(binding)) => self.bindings.push((binding, action)),
                    Ok(None) => {}
                    Err(error) => notes.push(format!("[keys] {name}: {error}")),
                }
            }
        }
        (self, notes)
    }

    fn is_prefix(&self, event: &Chord) -> bool {
        self.prefix.iter().any(|chord| chord.matches(event))
    }

    fn direct(&self, event: &Chord) -> Option<PaneAction> {
        self.bindings
            .iter()
            .find_map(|(binding, action)| match binding {
                Binding::Direct(chord) if chord.matches(event) => Some(*action),
                _ => None,
            })
    }

    fn after_prefix(&self, event: &Chord) -> Option<PaneAction> {
        self.bindings
            .iter()
            .find_map(|(binding, action)| match binding {
                Binding::Prefix(chord) if chord.matches(event) => Some(*action),
                _ => None,
            })
    }

    /// The label the palette shows, e.g. `Ctrl+B v` or `Ctrl+Alt+D`.
    pub fn label(&self, action: PaneAction) -> Option<String> {
        let prefix = self.prefix.first()?;
        self.bindings
            .iter()
            .find(|(_, bound)| *bound == action)
            .map(|(binding, _)| match binding {
                Binding::Prefix(chord) => format!("{prefix} {chord}"),
                Binding::Direct(chord) => chord.to_string(),
            })
    }

    pub fn prefix_label(&self) -> String {
        self.prefix
            .first()
            .map(ToString::to_string)
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Terminal,
    Prefix,
    Resize,
}

/// What to do with one key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Routed {
    /// Send the key to the focused pane.
    Pane,
    /// Prefix pressed twice: send the prefix key itself to the pane.
    SendPrefix,
    Action(PaneAction),
    /// Swallowed (mode change, cancel, or an unbound key).
    Consumed,
}

#[derive(Debug, Default)]
pub struct Router {
    pub mode: Mode,
}

impl Router {
    pub fn route(&mut self, keymap: &Keymap, event: &Chord) -> Routed {
        match self.mode {
            Mode::Terminal => {
                if let Some(action) = keymap.direct(event) {
                    return self.run(action);
                }
                if keymap.is_prefix(event) {
                    self.mode = Mode::Prefix;
                    return Routed::Consumed;
                }
                Routed::Pane
            }
            Mode::Prefix => {
                self.mode = Mode::Terminal;
                if keymap.is_prefix(event) {
                    return Routed::SendPrefix;
                }
                if event.key == "escape" && !event.ctrl && !event.alt {
                    return Routed::Consumed;
                }
                match keymap.after_prefix(event) {
                    Some(action) => self.run(action),
                    None => Routed::Consumed,
                }
            }
            Mode::Resize => {
                let leave = event.key == "escape"
                    || event.key == "enter"
                    || keymap.is_prefix(event)
                    || keymap.after_prefix(event) == Some(PaneAction::ResizeMode)
                    || keymap.direct(event) == Some(PaneAction::ResizeMode);
                if leave {
                    self.mode = Mode::Terminal;
                    return Routed::Consumed;
                }
                if event.ctrl || event.alt || event.platform {
                    return Routed::Consumed;
                }
                match event.key.as_str() {
                    "h" | "left" => Routed::Action(PaneAction::ResizeLeft),
                    "j" | "down" => Routed::Action(PaneAction::ResizeDown),
                    "k" | "up" => Routed::Action(PaneAction::ResizeUp),
                    "l" | "right" => Routed::Action(PaneAction::ResizeRight),
                    _ => Routed::Consumed,
                }
            }
        }
    }

    fn run(&mut self, action: PaneAction) -> Routed {
        if action == PaneAction::ResizeMode {
            self.mode = Mode::Resize;
        }
        Routed::Action(action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> Chord {
        let chord = parse_chord(text).unwrap();
        let typed = if chord.ctrl || chord.alt || chord.key.len() != 1 {
            None
        } else if chord.shift {
            Some(chord.key.to_ascii_uppercase())
        } else {
            Some(chord.key.clone())
        };
        Chord {
            text: typed,
            ..chord
        }
    }

    fn press(router: &mut Router, keymap: &Keymap, keys: &[&str]) -> Vec<Routed> {
        keys.iter()
            .map(|text| router.route(keymap, &key(text)))
            .collect()
    }

    #[test]
    fn herdr_defaults_are_prefix_bindings() {
        let keymap = Keymap::herdr_default();
        let mut router = Router::default();
        let table = [
            ("v", PaneAction::SplitVertical),
            ("minus", PaneAction::SplitHorizontal),
            ("h", PaneAction::FocusLeft),
            ("j", PaneAction::FocusDown),
            ("k", PaneAction::FocusUp),
            ("l", PaneAction::FocusRight),
            ("shift+h", PaneAction::SwapLeft),
            ("shift+j", PaneAction::SwapDown),
            ("shift+k", PaneAction::SwapUp),
            ("shift+l", PaneAction::SwapRight),
            ("tab", PaneAction::CycleNext),
            ("shift+tab", PaneAction::CyclePrevious),
            ("x", PaneAction::ClosePane),
            ("z", PaneAction::Zoom),
        ];
        for (second, action) in table {
            assert_eq!(
                press(&mut router, &keymap, &["ctrl+b", second]),
                vec![Routed::Consumed, Routed::Action(action)],
                "prefix+{second}"
            );
            assert_eq!(router.mode, Mode::Terminal);
        }
    }

    #[test]
    fn without_the_prefix_keys_go_to_the_pane() {
        let keymap = Keymap::herdr_default();
        let mut router = Router::default();
        for text in ["v", "h", "ctrl+alt+h", "ctrl+shift+h", "x", "ctrl+c"] {
            assert_eq!(router.route(&keymap, &key(text)), Routed::Pane, "{text}");
        }
    }

    #[test]
    fn prefix_twice_sends_the_prefix_and_esc_cancels() {
        let keymap = Keymap::herdr_default();
        let mut router = Router::default();
        assert_eq!(
            press(&mut router, &keymap, &["ctrl+b", "ctrl+b"]),
            vec![Routed::Consumed, Routed::SendPrefix]
        );
        assert_eq!(router.mode, Mode::Terminal);
        assert_eq!(
            press(&mut router, &keymap, &["ctrl+b", "esc", "v"]),
            vec![Routed::Consumed, Routed::Consumed, Routed::Pane]
        );
    }

    #[test]
    fn an_unbound_key_after_the_prefix_is_dropped() {
        let keymap = Keymap::herdr_default();
        let mut router = Router::default();
        assert_eq!(
            press(&mut router, &keymap, &["ctrl+b", "q", "q"]),
            vec![Routed::Consumed, Routed::Consumed, Routed::Pane]
        );
    }

    #[test]
    fn resize_mode_stays_until_esc_enter_prefix_or_r() {
        let keymap = Keymap::herdr_default();
        for leave in ["esc", "enter", "ctrl+b", "r"] {
            let mut router = Router::default();
            assert_eq!(
                press(&mut router, &keymap, &["ctrl+b", "r"])[1],
                Routed::Action(PaneAction::ResizeMode)
            );
            assert_eq!(router.mode, Mode::Resize);
            assert_eq!(
                press(&mut router, &keymap, &["h", "down", "k", "right", "a"]),
                vec![
                    Routed::Action(PaneAction::ResizeLeft),
                    Routed::Action(PaneAction::ResizeDown),
                    Routed::Action(PaneAction::ResizeUp),
                    Routed::Action(PaneAction::ResizeRight),
                    Routed::Consumed,
                ]
            );
            assert_eq!(router.route(&keymap, &key(leave)), Routed::Consumed);
            assert_eq!(router.mode, Mode::Terminal, "{leave}");
        }
    }

    #[test]
    fn question_mark_matches_by_the_typed_character() {
        let keymap = Keymap::herdr_default();
        let mut router = Router::default();
        router.route(&keymap, &key("ctrl+b"));
        let shifted_slash = Chord::from_event("/", Some("?"), false, false, true, false);
        assert_eq!(
            router.route(&keymap, &shifted_slash),
            Routed::Action(PaneAction::Help)
        );
    }

    #[test]
    fn keys_table_replaces_and_adds_direct_chords() {
        let table: toml::Table = toml::from_str(
            r#"
            prefix = ["ctrl+a"]
            split_vertical = ["prefix+v", "ctrl+alt+d"]
            zoom = "unset"
            new_tab = "prefix+c"
            focus_pane_left = 3
            "#,
        )
        .unwrap();
        let (keymap, notes) = Keymap::herdr_default().with_keys_table(&table);
        assert_eq!(notes.len(), 2, "{notes:?}");
        let mut router = Router::default();
        assert_eq!(
            router.route(&keymap, &key("ctrl+alt+d")),
            Routed::Action(PaneAction::SplitVertical)
        );
        assert_eq!(router.route(&keymap, &key("ctrl+b")), Routed::Pane);
        assert_eq!(
            press(&mut router, &keymap, &["ctrl+a", "z"]),
            vec![Routed::Consumed, Routed::Consumed]
        );
        assert_eq!(keymap.label(PaneAction::SplitVertical).unwrap(), "Ctrl+A v");
        assert_eq!(keymap.label(PaneAction::Zoom), None);
    }

    #[test]
    fn labels_follow_herdr_names() {
        let keymap = Keymap::herdr_default();
        assert_eq!(
            keymap.label(PaneAction::SplitHorizontal).unwrap(),
            "Ctrl+B -"
        );
        assert_eq!(
            keymap.label(PaneAction::SwapLeft).unwrap(),
            "Ctrl+B Shift+H"
        );
        assert_eq!(
            keymap.label(PaneAction::CyclePrevious).unwrap(),
            "Ctrl+B Shift+Tab"
        );
        assert_eq!(keymap.label(PaneAction::ResizeLeft), None);
    }
}
