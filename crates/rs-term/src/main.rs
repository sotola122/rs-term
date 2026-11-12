//! Full-screen serial terminal. One process, one port.

mod app;
mod cli;
mod copy;
mod keys;
mod redraw;
mod signal;
mod status;
mod term;
mod view;

use std::io::{self, stdout, Write};
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
        // `eprintln!` panics when stderr is a terminal that has gone away.
        let _ = writeln!(io::stderr(), "Cannot exit cleanly (OS: {error})");
        std::process::exit(1);
    }
}

fn run(cli: cli::Cli) -> io::Result<()> {
    term::install_panic_hook();
    signal::install()?;
    let _guard = TermGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    let _ = terminal.hide_cursor();
    let mut app = App::new(cli);
    let result = event_loop(&mut terminal, &mut app);
    // Close the port before `terminal` and `_guard` are dropped, on every
    // way out, so a failed draw cannot leave the port held.
    app.shutdown();
    // ratatui shows the cursor in its `Drop` and `eprintln!`s a failure, which
    // panics when the terminal is already gone (SIGHUP). Do it here and skip
    // the drop.
    let _ = terminal.show_cursor();
    std::mem::forget(terminal);
    match result {
        // A terminal that hung up is a quit, not a failure.
        Err(error) if signal::terminal_gone(&error) => Ok(()),
        other => other,
    }
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> io::Result<()> {
    let size = terminal.size()?;
    app.on_resize(size.width, size.height);
    let mut gate = RedrawGate::default();
    let mut dirty = true;
    while !app.should_quit() {
        dirty |= app.drain_serial();
        if gate.should_draw(dirty, app.status_view(), Instant::now()) {
            terminal.draw(|frame| view::draw(frame, app))?;
            dirty = false;
        }
        if signal::wait_event(Duration::from_millis(10))? {
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
    Ok(())
}
