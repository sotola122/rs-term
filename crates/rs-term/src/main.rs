//! Full-screen serial terminal. One process, one port.

mod app;
mod cli;
mod copy;
mod keys;
mod redraw;
mod status;
mod term;
mod view;

use std::io::{self, stdout};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, MouseEventKind};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::app::App;
use crate::redraw::RedrawGate;
use crate::term::TermGuard;

fn main() {
    let cli = match cli::parse_os(std::env::args_os().skip(1)) {
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
    let mut gate = RedrawGate::default();
    let mut dirty = true;
    while !app.should_quit() {
        dirty |= app.drain_serial();
        if gate.should_draw(dirty, app.status_view(), Instant::now()) {
            terminal.draw(|frame| view::draw(frame, &app))?;
            dirty = false;
        }
        if event::poll(Duration::from_millis(10))? {
            match event::read()? {
                Event::Key(key) => app.on_key(key),
                Event::Mouse(mouse) if mouse.kind == MouseEventKind::Moved => continue,
                Event::Mouse(mouse) => app.on_mouse(mouse),
                Event::Resize(width, height) => app.on_resize(width, height),
                Event::Paste(text) => app.on_paste(text),
                _ => continue,
            }
            dirty = true;
        }
    }
    // Close the port before `terminal` and `_guard` are dropped.
    app.shutdown();
    Ok(())
}
