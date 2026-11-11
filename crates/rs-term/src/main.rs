//! Full-screen serial terminal. One process, one port.

mod app;
mod cli;
mod copy;
mod keys;
mod status;
mod term;
mod view;

use std::io::{self, stdout};
use std::time::Duration;

use crossterm::event::{self, Event};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::app::App;
use crate::term::TermGuard;

fn main() {
    let cli = match cli::parse(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    if cli.help {
        print!("{}", cli::HELP);
        return;
    }
    if let Err(error) = run(cli) {
        eprintln!("終了できない（OS: {error}）");
        std::process::exit(1);
    }
}

fn run(cli: cli::Cli) -> io::Result<()> {
    term::install_panic_hook();
    let _guard = TermGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    let _ = terminal.hide_cursor();
    let mut app = App::new(cli);
    let size = terminal.size()?;
    app.on_resize(size.width, size.height);
    while !app.should_quit() {
        let _drained = app.drain_serial();
        terminal.draw(|frame| view::draw(frame, &app))?;
        let timeout = Duration::from_millis(10);
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => app.on_key(key),
                Event::Mouse(mouse) => app.on_mouse(mouse),
                Event::Resize(width, height) => app.on_resize(width, height),
                Event::Paste(text) => app.on_paste(text),
                _ => {}
            }
        }
    }
    Ok(())
}
