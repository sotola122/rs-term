//! Palette actions.
//!
//! GPUI action names cannot contain `::`. The design ids use a dot, so each
//! action sets `name = "group.action"`. Keywords repeat that id for search.

use gpui::Action;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "palette.toggle", no_json)]
pub struct PaletteToggle;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "app.about", no_json)]
pub struct About;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "session.connect", no_json)]
pub struct Connect;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "session.disconnect", no_json)]
pub struct Disconnect;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "session.baud", no_json)]
pub struct Baud;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "session.framing", no_json)]
pub struct Framing;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "session.flow", no_json)]
pub struct Flow;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "session.dtr", no_json)]
pub struct Dtr;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "session.rts", no_json)]
pub struct Rts;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "session.encoding", no_json)]
pub struct Encoding;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "session.newline", no_json)]
pub struct Newline;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "session.local_echo", no_json)]
pub struct LocalEcho;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "session.send_text", no_json)]
pub struct SendText;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "session.break", no_json)]
pub struct Break;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "pane.split_vertical", no_json)]
pub struct SplitVertical;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "pane.split_horizontal", no_json)]
pub struct SplitHorizontal;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "pane.focus_left", no_json)]
pub struct FocusLeft;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "pane.focus_down", no_json)]
pub struct FocusDown;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "pane.focus_up", no_json)]
pub struct FocusUp;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "pane.focus_right", no_json)]
pub struct FocusRight;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "pane.swap_left", no_json)]
pub struct SwapLeft;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "pane.swap_down", no_json)]
pub struct SwapDown;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "pane.swap_up", no_json)]
pub struct SwapUp;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "pane.swap_right", no_json)]
pub struct SwapRight;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "pane.cycle_next", no_json)]
pub struct CycleNext;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "pane.cycle_previous", no_json)]
pub struct CyclePrevious;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "pane.close", no_json)]
pub struct ClosePane;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "pane.zoom", no_json)]
pub struct Zoom;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "pane.resize_mode", no_json)]
pub struct ResizeMode;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "pane.resize_left", no_json)]
pub struct ResizeLeft;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "pane.resize_down", no_json)]
pub struct ResizeDown;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "pane.resize_up", no_json)]
pub struct ResizeUp;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "pane.resize_right", no_json)]
pub struct ResizeRight;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "theme.dark", no_json)]
pub struct ThemeDark;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "theme.light", no_json)]
pub struct ThemeLight;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "font.terminal", no_json)]
pub struct FontTerminal;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "font.size", no_json)]
pub struct FontSize;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "font.ui", no_json)]
pub struct FontUi;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "log.start_binary", no_json)]
pub struct LogBinary;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "log.start_text", no_json)]
pub struct LogText;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "log.pause", no_json)]
pub struct LogPause;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "log.resume", no_json)]
pub struct LogResume;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "log.stop", no_json)]
pub struct LogStop;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "edit.copy", no_json)]
pub struct Copy;

#[derive(Clone, PartialEq, Eq, Default, Debug, Action)]
#[action(name = "edit.paste", no_json)]
pub struct Paste;

pub fn bind_keys(cx: &mut gpui::App) {
    cx.bind_keys([
        gpui::KeyBinding::new("ctrl-shift-p", PaletteToggle, None),
        gpui::KeyBinding::new("ctrl-shift-k", Connect, None),
        gpui::KeyBinding::new("ctrl-shift-d", Disconnect, None),
        gpui::KeyBinding::new("ctrl-shift-b", Break, None),
        gpui::KeyBinding::new("ctrl-alt-v", SplitVertical, None),
        gpui::KeyBinding::new("ctrl-alt-minus", SplitHorizontal, None),
        gpui::KeyBinding::new("ctrl-alt-h", FocusLeft, None),
        gpui::KeyBinding::new("ctrl-alt-j", FocusDown, None),
        gpui::KeyBinding::new("ctrl-alt-k", FocusUp, None),
        gpui::KeyBinding::new("ctrl-alt-l", FocusRight, None),
        gpui::KeyBinding::new("ctrl-alt-shift-h", SwapLeft, None),
        gpui::KeyBinding::new("ctrl-alt-shift-j", SwapDown, None),
        gpui::KeyBinding::new("ctrl-alt-shift-k", SwapUp, None),
        gpui::KeyBinding::new("ctrl-alt-shift-l", SwapRight, None),
        gpui::KeyBinding::new("ctrl-shift-right", CycleNext, None),
        gpui::KeyBinding::new("ctrl-shift-left", CyclePrevious, None),
        gpui::KeyBinding::new("ctrl-alt-x", ClosePane, None),
        gpui::KeyBinding::new("ctrl-alt-z", Zoom, None),
        gpui::KeyBinding::new("ctrl-alt-r", ResizeMode, None),
        gpui::KeyBinding::new("ctrl-shift-c", Copy, None),
        gpui::KeyBinding::new("ctrl-shift-v", Paste, None),
    ]);
}
