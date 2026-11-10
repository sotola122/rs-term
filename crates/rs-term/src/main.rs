#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

extern crate gpui_kit as gpui;

mod ui;

use gpui_kit::application;

fn main() {
    application().run(|cx| {
        gpui_kit::init(cx);
        ui::actions::bind_keys(cx);
        // Closing the only window ends the app (review N26).
        cx.on_window_closed(|cx, _window| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        ui::workspace::open(cx);
    });
}
