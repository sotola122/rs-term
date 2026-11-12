//! SIGTERM and SIGHUP (the terminal or pane was closed) ask for the same
//! orderly shutdown as prefix `q`. The handler only sets a flag; the main
//! loop checks it every iteration. Windows has no equivalent: the OS releases
//! the port when the process ends.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossterm::event;

static TERMINATE: AtomicBool = AtomicBool::new(false);

/// True once a termination signal has arrived.
pub fn requested() -> bool {
    TERMINATE.load(Ordering::Relaxed)
}

#[cfg(unix)]
extern "C" fn on_signal(_signal: libc::c_int) {
    // An atomic store is async-signal-safe.
    TERMINATE.store(true, Ordering::Relaxed);
}

/// Route SIGTERM and SIGHUP to the flag. The handler is installed without
/// `SA_RESTART`, so a blocked `poll` returns and the loop sees the flag.
#[cfg(unix)]
pub fn install() -> io::Result<()> {
    for signal in [libc::SIGTERM, libc::SIGHUP] {
        // SAFETY: `action` is fully initialised before use, and the handler
        // is async-signal-safe.
        let rc = unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as usize;
            libc::sigemptyset(&mut action.sa_mask);
            action.sa_flags = 0;
            libc::sigaction(signal, &action, std::ptr::null_mut())
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Wait up to `timeout` for the next terminal event. Returns `false` on a
/// timeout, a signal, or a hung-up terminal (which also sets the quit flag).
///
/// On Unix the wait is our own `poll` on stdin and crossterm is entered only
/// when input is ready. A closed terminal is reported as readable forever,
/// the kernel wakes readers before it delivers SIGHUP, and crossterm's `poll`
/// and `read` spin on such a tty and never return, so the main loop could not
/// reach its quit check. Here the hang-up is seen first and the loop leaves
/// through the same orderly shutdown with or without the signal.
#[cfg(unix)]
pub fn wait_event(timeout: Duration) -> io::Result<bool> {
    if stdin_state(Duration::ZERO).1 {
        return Ok(false);
    }
    // Events crossterm already parsed, or input that is ready now.
    if event::poll(Duration::ZERO)? {
        return Ok(true);
    }
    let (readable, hung_up) = stdin_state(timeout);
    Ok(readable && !hung_up && event::poll(Duration::ZERO)?)
}

/// `(readable, hung_up)` for stdin. A hang-up sets the quit flag. A signal
/// that interrupts the wait reports neither.
#[cfg(unix)]
fn stdin_state(timeout: Duration) -> (bool, bool) {
    let mut fd = libc::pollfd {
        fd: libc::STDIN_FILENO,
        events: libc::POLLIN,
        revents: 0,
    };
    let millis = timeout.as_millis().min(i32::MAX as u128) as libc::c_int;
    // SAFETY: one valid pollfd.
    let ready = unsafe { libc::poll(&mut fd, 1, millis) };
    if ready <= 0 {
        return (false, false);
    }
    let hung_up = fd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0;
    if hung_up {
        TERMINATE.store(true, Ordering::Relaxed);
    }
    (fd.revents & libc::POLLIN != 0, hung_up)
}

#[cfg(not(unix))]
pub fn wait_event(timeout: Duration) -> io::Result<bool> {
    event::poll(timeout)
}

#[cfg(not(unix))]
pub fn install() -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
pub fn request_for_test(on: bool) {
    TERMINATE.store(on, Ordering::Relaxed);
}
