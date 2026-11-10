extern crate gpui_kit as gpui;

mod ui;

use gpui_kit::application;

fn main() {
    application().run(|cx| {
        gpui_kit::init(cx);
        ui::actions::bind_keys(cx);
        ui::workspace::open(cx);
    });
}
