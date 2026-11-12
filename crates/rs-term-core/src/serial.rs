//! One OS thread per open port.
//!
//! The thread owns `Box<dyn PortIo>`. There is no Tokio and no `try_clone`.
//! A read of `TimedOut` or `Interrupted` leaves the link open. Any other read
//! error is `PortLost`. A full data channel blocks this thread (and therefore
//! the next read) instead of dropping bytes. Shutdown is still taken from the
//! command channel while that send is waiting.

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use futures_channel::mpsc as async_mpsc;

use crate::error::PortError;
use crate::IoEvent;

pub const READ_TIMEOUT: Duration = Duration::from_millis(20);
/// 256 chunks of `MAX_CHUNK` is about 1 MiB waiting for the UI.
const DATA_DEPTH: usize = 256;
const COMMAND_DEPTH: usize = 32;
const MAX_CHUNK: usize = 4096;
/// Upper bound of bytes waiting for the port. One `Input` larger than this
/// is refused as a whole.
pub const MAX_PENDING: usize = 16 * 1024 * 1024;
/// How long a full data channel waits before trying to send again.
/// Short enough that a draining UI unblocks the read without a long stall,
/// long enough that shutdown is observed while the buffer stays full.
const SEND_WAIT: Duration = Duration::from_micros(200);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataBits {
    Five,
    Six,
    Seven,
    Eight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parity {
    None,
    Odd,
    Even,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopBits {
    One,
    Two,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    None,
    XonXoff,
    RtsCts,
}

// The spellings below are the one list the command line and `settings.toml`
// share. Case does not matter.

impl DataBits {
    pub fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "5" | "five" => Some(Self::Five),
            "6" | "six" => Some(Self::Six),
            "7" | "seven" => Some(Self::Seven),
            "8" | "eight" => Some(Self::Eight),
            _ => None,
        }
    }
}

impl Parity {
    pub fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "n" | "none" => Some(Self::None),
            "o" | "odd" => Some(Self::Odd),
            "e" | "even" => Some(Self::Even),
            _ => None,
        }
    }
}

impl StopBits {
    pub fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "1" | "one" => Some(Self::One),
            "2" | "two" => Some(Self::Two),
            _ => None,
        }
    }
}

/// `on` or `off`, for the DTR and RTS options (command line and settings).
pub fn parse_switch(text: &str) -> Option<bool> {
    match text.to_ascii_lowercase().as_str() {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

impl Flow {
    pub fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "xonxoff" | "xon/xoff" => Some(Self::XonXoff),
            "rtscts" | "rts/cts" => Some(Self::RtsCts),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialParams {
    pub path: String,
    pub baud_rate: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
    pub flow: Flow,
    /// Level of the DTR line after the port is opened (default off).
    pub dtr: bool,
    /// Level of the RTS line after the port is opened (default off). Not
    /// touched with `Flow::RtsCts`, where the driver owns the line.
    pub rts: bool,
    pub read_timeout: Duration,
}

impl Default for SerialParams {
    fn default() -> Self {
        Self {
            path: String::new(),
            baud_rate: 115_200,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
            flow: Flow::None,
            dtr: false,
            rts: false,
            read_timeout: READ_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortInfo {
    pub path: String,
    pub kind: PortKind,
    pub usb: Option<UsbIdentity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortKind {
    Usb,
    Pci,
    Bluetooth,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbIdentity {
    pub vid: u16,
    pub pid: u16,
    pub serial_number: Option<String>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
}

pub fn list_ports() -> Result<Vec<PortInfo>, PortError> {
    let ports = serialport::available_ports().map_err(map_serial_error)?;
    Ok(ports.into_iter().map(map_port_info).collect())
}

fn map_port_info(info: serialport::SerialPortInfo) -> PortInfo {
    match info.port_type {
        serialport::SerialPortType::UsbPort(usb) => PortInfo {
            path: info.port_name,
            kind: PortKind::Usb,
            usb: Some(UsbIdentity {
                vid: usb.vid,
                pid: usb.pid,
                serial_number: usb.serial_number,
                manufacturer: usb.manufacturer,
                product: usb.product,
            }),
        },
        serialport::SerialPortType::PciPort => PortInfo {
            path: info.port_name,
            kind: PortKind::Pci,
            usb: None,
        },
        serialport::SerialPortType::BluetoothPort => PortInfo {
            path: info.port_name,
            kind: PortKind::Bluetooth,
            usb: None,
        },
        serialport::SerialPortType::Unknown => PortInfo {
            path: info.port_name,
            kind: PortKind::Unknown,
            usb: None,
        },
    }
}

pub(crate) trait PortIo: Send {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;
    fn write(&mut self, buf: &[u8]) -> io::Result<usize>;
    #[allow(dead_code)]
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        let mut written = 0;
        while written < buf.len() {
            match self.write(&buf[written..])? {
                0 => return Err(io::Error::new(io::ErrorKind::WriteZero, "write returned 0")),
                count => written += count,
            }
        }
        Ok(())
    }
    fn apply(&mut self, params: &SerialParams) -> Result<(), PortError>;
    fn set_break(&mut self, on: bool) -> Result<(), PortError>;
    /// Drive the DTR line. Ports without modem lines may ignore it.
    fn set_dtr(&mut self, _on: bool) -> Result<(), PortError> {
        Ok(())
    }
    /// Drive the RTS line.
    fn set_rts(&mut self, _on: bool) -> Result<(), PortError> {
        Ok(())
    }
}

/// Put DTR and RTS at the configured levels. Windows opens with both low and
/// Linux raises both, so this makes every OS start the same way. RTS is left
/// alone with hardware flow control. A port without modem lines (a pty, a
/// virtual port) refuses the request; that is not an error for the link.
pub(crate) fn apply_lines(port: &mut dyn PortIo, params: &SerialParams) {
    let _ = port.set_dtr(params.dtr);
    if params.flow != Flow::RtsCts {
        let _ = port.set_rts(params.rts);
    }
}

pub(crate) fn open_port(params: &SerialParams) -> Result<Box<dyn PortIo>, PortError> {
    if !(1..=4_000_000).contains(&params.baud_rate) {
        return Err(PortError::Io {
            message: "Baud rate is out of range (1 to 4000000)".into(),
        });
    }
    let builder = serialport::new(&params.path, params.baud_rate)
        .data_bits(map_data_bits(params.data_bits))
        .parity(map_parity(params.parity))
        .stop_bits(map_stop(params.stop_bits))
        .flow_control(map_flow(params.flow))
        .timeout(params.read_timeout);
    let port = builder
        .open()
        .map_err(|error| map_open_error(&params.path, error))?;
    let mut io = SerialIo { port };
    apply_lines(&mut io, params);
    Ok(Box::new(io))
}

struct SerialIo {
    port: Box<dyn serialport::SerialPort>,
}

impl PortIo for SerialIo {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.port.read(buf)
    }

    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.port.write(buf)
    }

    fn apply(&mut self, params: &SerialParams) -> Result<(), PortError> {
        self.port
            .set_baud_rate(params.baud_rate)
            .map_err(|error| named("the baud rate", error))?;
        self.port
            .set_data_bits(map_data_bits(params.data_bits))
            .map_err(|error| named("the data bits", error))?;
        self.port
            .set_parity(map_parity(params.parity))
            .map_err(|error| named("the parity", error))?;
        self.port
            .set_stop_bits(map_stop(params.stop_bits))
            .map_err(|error| named("the stop bits", error))?;
        self.port
            .set_flow_control(map_flow(params.flow))
            .map_err(|error| named("the flow control", error))?;
        // A change of the frame must not move the modem lines.
        apply_lines(self, params);
        Ok(())
    }

    fn set_break(&mut self, on: bool) -> Result<(), PortError> {
        if on {
            self.port.set_break().map_err(map_serial_error)
        } else {
            self.port.clear_break().map_err(map_serial_error)
        }
    }

    fn set_dtr(&mut self, on: bool) -> Result<(), PortError> {
        self.port
            .write_data_terminal_ready(on)
            .map_err(|error| named("DTR", error))
    }

    fn set_rts(&mut self, on: bool) -> Result<(), PortError> {
        self.port
            .write_request_to_send(on)
            .map_err(|error| named("RTS", error))
    }
}

fn named(field: &str, error: serialport::Error) -> PortError {
    PortError::Io {
        message: format!("Port rejected {field} (OS: {})", error.description),
    }
}

/// `serialport` errors in English. The OS text stays in parentheses so the
/// cause can still be searched.
///
/// POSIX `EBUSY` and a failed `TIOCEXCL` / `flock` are both `NoDevice`, with
/// the OS sentence still saying "busy" or "lock" (`serialport` 4.10
/// `posix/error.rs`). That must not be translated as a missing port.
pub(crate) fn serial_error_text(kind: &serialport::ErrorKind, description: &str) -> String {
    open_error_text(kind, description, false)
}

/// As `serial_error_text`. `access_denied` says the open failed with
/// `ERROR_ACCESS_DENIED`, which `serialport` folds into `NoDevice` together
/// with "not found" and keeps only as localized text. On Windows that is how
/// a port held by another process looks, so it is reported as in use.
pub(crate) fn open_error_text(
    kind: &serialport::ErrorKind,
    description: &str,
    access_denied: bool,
) -> String {
    if access_denied || is_in_use(kind, description) {
        return with_os("Port is busy (opened by another process)", description);
    }
    let what = match kind {
        serialport::ErrorKind::NoDevice => "Port not found (unplugged or wrong name)",
        serialport::ErrorKind::InvalidInput => "Port rejected the settings",
        serialport::ErrorKind::Io(io::ErrorKind::NotFound) => "Port not found",
        serialport::ErrorKind::Io(io::ErrorKind::PermissionDenied) => {
            "Access denied (another app may be using the port)"
        }
        serialport::ErrorKind::Io(io::ErrorKind::TimedOut) => "Port not responding (timeout)",
        serialport::ErrorKind::Io(_) => "I/O error",
        serialport::ErrorKind::Unknown => "Unknown error",
    };
    with_os(what, description)
}

fn with_os(what: &str, description: &str) -> String {
    if description.is_empty() {
        what.to_string()
    } else {
        format!("{what} (OS: {description})")
    }
}

fn is_in_use(kind: &serialport::ErrorKind, description: &str) -> bool {
    let lower = description.to_ascii_lowercase();
    if lower.contains("busy") || lower.contains("ebusy") || lower.contains("lock") {
        return true;
    }
    matches!(kind, serialport::ErrorKind::Io(io::ErrorKind::ResourceBusy))
}

/// An open error. `serialport` drops the OS error code, so for `NoDevice`
/// the path is probed once more to see whether the OS says "access denied".
fn map_open_error(path: &str, error: serialport::Error) -> PortError {
    let denied = matches!(error.kind, serialport::ErrorKind::NoDevice) && probe_access_denied(path);
    PortError::Io {
        message: open_error_text(&error.kind, &error.description, denied),
    }
}

/// True for the errors a port held by another process gives on Windows:
/// `PermissionDenied`, or raw OS error 5 (`ERROR_ACCESS_DENIED`).
#[cfg(windows)]
pub(crate) fn is_access_denied(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::PermissionDenied || error.raw_os_error() == Some(5)
}

#[cfg(windows)]
fn probe_access_denied(path: &str) -> bool {
    let device = if path.starts_with(r"\\") {
        path.to_string()
    } else {
        format!(r"\\.\{path}")
    };
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(device)
    {
        Ok(_) => false,
        Err(error) => is_access_denied(&error),
    }
}

#[cfg(not(windows))]
fn probe_access_denied(_path: &str) -> bool {
    false
}

fn map_serial_error(error: serialport::Error) -> PortError {
    PortError::Io {
        message: serial_error_text(&error.kind, &error.description),
    }
}

fn map_data_bits(value: DataBits) -> serialport::DataBits {
    match value {
        DataBits::Five => serialport::DataBits::Five,
        DataBits::Six => serialport::DataBits::Six,
        DataBits::Seven => serialport::DataBits::Seven,
        DataBits::Eight => serialport::DataBits::Eight,
    }
}

fn map_parity(value: Parity) -> serialport::Parity {
    match value {
        Parity::None => serialport::Parity::None,
        Parity::Odd => serialport::Parity::Odd,
        Parity::Even => serialport::Parity::Even,
    }
}

fn map_stop(value: StopBits) -> serialport::StopBits {
    match value {
        StopBits::One => serialport::StopBits::One,
        StopBits::Two => serialport::StopBits::Two,
    }
}

fn map_flow(value: Flow) -> serialport::FlowControl {
    match value {
        Flow::None => serialport::FlowControl::None,
        Flow::XonXoff => serialport::FlowControl::Software,
        Flow::RtsCts => serialport::FlowControl::Hardware,
    }
}

pub(crate) enum Command {
    /// One whole `Input`. The thread writes it in pieces; a paste is never
    /// split across commands, so it is sent completely or not at all.
    Write {
        generation: u64,
        bytes: Vec<u8>,
    },
    Apply(SerialParams),
    Break {
        hold: Duration,
    },
    Cancel {
        generation: u64,
    },
    Shutdown,
}

pub(crate) struct WorkerHandle {
    pub commands: SyncSender<Command>,
    pub join: JoinHandle<()>,
}

/// Status events travel on their own unbounded channel. A full data channel
/// blocks the worker inside `send_data` instead of discarding the chunk, and
/// that wait still polls the command channel (review N2, design v7 §7.5).
pub(crate) struct Channels {
    pub data: async_mpsc::Receiver<IoEvent>,
    pub status: async_mpsc::UnboundedReceiver<IoEvent>,
}

/// Start the port thread. A thread that cannot be created is an error, and
/// the port is closed again (it was moved into the thread body).
pub(crate) fn spawn(
    port: Box<dyn PortIo>,
    generation: u64,
    written: Arc<AtomicU64>,
) -> Result<(WorkerHandle, Channels), PortError> {
    spawn_with(port, generation, written, |body| {
        thread::Builder::new()
            .name("rs-term-serial".into())
            .spawn(body)
    })
}

type ThreadBody = Box<dyn FnOnce() + Send>;

fn spawn_with(
    port: Box<dyn PortIo>,
    generation: u64,
    written: Arc<AtomicU64>,
    start: impl FnOnce(ThreadBody) -> io::Result<JoinHandle<()>>,
) -> Result<(WorkerHandle, Channels), PortError> {
    let (data_tx, data_rx) = async_mpsc::channel(DATA_DEPTH);
    let (status_tx, status_rx) = async_mpsc::unbounded();
    let (cmd_tx, cmd_rx) = mpsc::sync_channel(COMMAND_DEPTH);
    let join = start(Box::new(move || {
        let outlet = Outlet {
            written,
            data: data_tx,
            status: status_tx,
        };
        worker_loop(port, outlet, cmd_rx, generation)
    }))
    .map_err(|error| PortError::Io {
        message: format!("Cannot start the port thread (OS: {error})"),
    })?;
    Ok((
        WorkerHandle {
            commands: cmd_tx,
            join,
        },
        Channels {
            data: data_rx,
            status: status_rx,
        },
    ))
}

struct Outlet {
    /// Bytes the port has accepted, shared with the session (its TX count).
    written: Arc<AtomicU64>,
    data: async_mpsc::Sender<IoEvent>,
    status: async_mpsc::UnboundedSender<IoEvent>,
}

impl Outlet {
    fn status(&mut self, event: IoEvent) {
        let _ = self.status.unbounded_send(event);
    }

    /// Deliver `bytes` or wait until the UI takes them. Commands, including
    /// shutdown, are handled while the wait is blocked so a full buffer
    /// cannot pin the port open.
    fn send_data(
        &mut self,
        commands: &Receiver<Command>,
        ctx: &mut IoCtx<'_>,
        mut payload: Vec<u8>,
    ) -> Step {
        loop {
            match self.data.try_send(IoEvent::Data(payload)) {
                Ok(()) => return Step::Continue,
                Err(error) if error.is_full() => {
                    payload = match error.into_inner() {
                        IoEvent::Data(bytes) => bytes,
                        other => {
                            self.status(other);
                            return Step::Continue;
                        }
                    };
                    let step = match commands.try_recv() {
                        Ok(command) => handle_command(ctx, self, command),
                        Err(TryRecvError::Empty) => Step::Continue,
                        Err(TryRecvError::Disconnected) => Step::Stop,
                    };
                    if step == Step::Stop {
                        return Step::Stop;
                    }
                    pump_tx(ctx, self);
                    thread::park_timeout(SEND_WAIT);
                }
                Err(_) => return Step::Stop,
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Continue,
    Stop,
}

struct IoCtx<'a> {
    port: &'a mut Box<dyn PortIo>,
    pending: &'a mut Vec<u8>,
    pending_at: &'a mut usize,
    break_until: &'a mut Option<Instant>,
    generation: &'a mut u64,
}

fn read_lost(error: &io::Error) -> PortError {
    PortError::Io {
        message: format!("Port connection lost (OS: {error})"),
    }
}

fn handle_command(ctx: &mut IoCtx<'_>, out: &mut Outlet, command: Command) -> Step {
    match command {
        Command::Shutdown => Step::Stop,
        Command::Cancel { generation: next } => {
            *ctx.generation = next;
            drop_pending(ctx);
            Step::Continue
        }
        Command::Write {
            generation: write_generation,
            bytes,
        } => {
            if bytes.is_empty() {
                return Step::Continue;
            }
            if write_generation != *ctx.generation {
                return Step::Continue;
            }
            if *ctx.pending_at >= ctx.pending.len() {
                ctx.pending.clear();
                *ctx.pending_at = 0;
            }
            if ctx.pending.len() - *ctx.pending_at + bytes.len() > MAX_PENDING {
                out.status(IoEvent::CommandFailed(PortError::Io {
                    message: format!(
                        "Send backlog would exceed the {} MiB limit; {} bytes not sent",
                        MAX_PENDING / (1024 * 1024),
                        bytes.len()
                    ),
                }));
                return Step::Continue;
            }
            ctx.pending.extend(bytes);
            Step::Continue
        }
        Command::Apply(params) => {
            if let Err(error) = ctx.port.apply(&params) {
                out.status(IoEvent::ApplyFailed(error));
            }
            Step::Continue
        }
        Command::Break { hold } => {
            if let Err(error) = ctx.port.set_break(true) {
                out.status(IoEvent::CommandFailed(error));
            } else {
                *ctx.break_until = Some(Instant::now() + hold);
            }
            Step::Continue
        }
    }
}

/// Forget the bytes still waiting for the port. They were never counted in
/// the TX total, which only grows when the port accepts bytes.
fn drop_pending(ctx: &mut IoCtx<'_>) {
    ctx.pending.clear();
    *ctx.pending_at = 0;
}

fn pump_tx(ctx: &mut IoCtx<'_>, out: &mut Outlet) {
    if ctx.break_until.is_some_and(|until| Instant::now() >= until) {
        if let Err(error) = ctx.port.set_break(false) {
            out.status(IoEvent::CommandFailed(error));
        }
        *ctx.break_until = None;
    }

    if *ctx.pending_at < ctx.pending.len() {
        match ctx.port.write(&ctx.pending[*ctx.pending_at..]) {
            Ok(0) => {}
            Ok(count) => {
                out.written.fetch_add(count as u64, Ordering::Relaxed);
                *ctx.pending_at += count;
                if *ctx.pending_at >= ctx.pending.len() {
                    ctx.pending.clear();
                    *ctx.pending_at = 0;
                }
            }
            // CTS or XOFF holds the line. Keep the rest and retry.
            Err(error)
                if error.kind() == io::ErrorKind::TimedOut
                    || error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                out.status(IoEvent::CommandFailed(PortError::Io {
                    message: format!(
                        "Write failed; dropped the remaining {} bytes (OS: {error})",
                        ctx.pending.len() - *ctx.pending_at
                    ),
                }));
                drop_pending(ctx);
            }
        }
    }
}

fn worker_loop(
    mut port: Box<dyn PortIo>,
    mut out: Outlet,
    commands: Receiver<Command>,
    mut generation: u64,
) {
    let mut buffer = [0u8; MAX_CHUNK];
    let mut pending: Vec<u8> = Vec::new();
    let mut pending_at = 0usize;
    let mut break_until: Option<Instant> = None;
    loop {
        let mut read_was_idle = false;
        let read = port.read(&mut buffer);
        let mut ctx = IoCtx {
            port: &mut port,
            pending: &mut pending,
            pending_at: &mut pending_at,
            break_until: &mut break_until,
            generation: &mut generation,
        };
        match read {
            // `serialport` on Windows turns a zero-length ReadFile into
            // TimedOut, so Ok(0) never means idle there. On Unix it is EOF:
            // the device hung up (review R17).
            Ok(0) => {
                out.status(IoEvent::PortLost(PortError::Io {
                    message: "Port connection lost (EOF)".into(),
                }));
                break;
            }
            Ok(count) => {
                let bytes = buffer[..count.min(MAX_CHUNK)].to_vec();
                if out.send_data(&commands, &mut ctx, bytes) == Step::Stop {
                    return;
                }
            }
            Err(error)
                if error.kind() == io::ErrorKind::TimedOut
                    || error.kind() == io::ErrorKind::Interrupted =>
            {
                read_was_idle = true;
            }
            Err(error) => {
                out.status(IoEvent::PortLost(read_lost(&error)));
                // Release the handle now (review N3). Only Shutdown is
                // awaited below.
                break;
            }
        }

        let mut saw_command = false;
        for _ in 0..8 {
            match commands.try_recv() {
                Ok(command) => {
                    saw_command = true;
                    let mut ctx = IoCtx {
                        port: &mut port,
                        pending: &mut pending,
                        pending_at: &mut pending_at,
                        break_until: &mut break_until,
                        generation: &mut generation,
                    };
                    if handle_command(&mut ctx, &mut out, command) == Step::Stop {
                        return;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }

        let mut ctx = IoCtx {
            port: &mut port,
            pending: &mut pending,
            pending_at: &mut pending_at,
            break_until: &mut break_until,
            generation: &mut generation,
        };
        pump_tx(&mut ctx, &mut out);

        if read_was_idle && !saw_command && pending_at >= pending.len() {
            // A real port already blocked inside `read` for `read_timeout`.
            // This yield only matters for a mock that returns TimedOut
            // without blocking, so the test thread cannot spin a core.
            thread::park_timeout(Duration::from_millis(2));
        }
    }

    // Lost: the port is closed here, before waiting, so the device can be
    // opened again while the UI still holds the old session.
    drop(port);
    loop {
        match commands.recv() {
            Ok(Command::Shutdown) | Err(_) => return,
            Ok(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DropFlag(std::sync::Arc<std::sync::atomic::AtomicBool>);

    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    impl PortIo for DropFlag {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::TimedOut.into())
        }
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            Ok(buf.len())
        }
        fn apply(&mut self, _: &SerialParams) -> Result<(), PortError> {
            Ok(())
        }
        fn set_break(&mut self, _: bool) -> Result<(), PortError> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct LineLog(Vec<(&'static str, bool)>);

    impl PortIo for LineLog {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::TimedOut.into())
        }
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            Ok(buf.len())
        }
        fn apply(&mut self, _: &SerialParams) -> Result<(), PortError> {
            Ok(())
        }
        fn set_break(&mut self, _: bool) -> Result<(), PortError> {
            Ok(())
        }
        fn set_dtr(&mut self, on: bool) -> Result<(), PortError> {
            self.0.push(("dtr", on));
            Ok(())
        }
        fn set_rts(&mut self, on: bool) -> Result<(), PortError> {
            self.0.push(("rts", on));
            Ok(())
        }
    }

    #[test]
    fn lines_default_to_low_and_follow_the_params() {
        let mut log = LineLog::default();
        apply_lines(&mut log, &SerialParams::default());
        assert_eq!(log.0, [("dtr", false), ("rts", false)]);
        let mut log = LineLog::default();
        let params = SerialParams {
            dtr: true,
            rts: true,
            ..SerialParams::default()
        };
        apply_lines(&mut log, &params);
        assert_eq!(log.0, [("dtr", true), ("rts", true)]);
    }

    #[test]
    fn hardware_flow_control_leaves_rts_alone() {
        let mut log = LineLog::default();
        let params = SerialParams {
            dtr: true,
            rts: true,
            flow: Flow::RtsCts,
            ..SerialParams::default()
        };
        apply_lines(&mut log, &params);
        assert_eq!(log.0, [("dtr", true)]);
    }

    #[test]
    fn a_thread_that_cannot_start_is_an_error_and_closes_the_port() {
        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let port = Box::new(DropFlag(dropped.clone()));
        let result = spawn_with(port, 1, Arc::default(), |_body| {
            Err(io::Error::other("resource exhausted"))
        });
        let message = match result {
            Err(error) => error.to_string(),
            Ok(_) => panic!("the spawn must fail"),
        };
        assert!(
            message.contains("Cannot start the port thread"),
            "{message}"
        );
        assert!(message.contains("resource exhausted"), "{message}");
        assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn frame_spellings_are_shared_and_case_insensitive() {
        assert_eq!(DataBits::parse("8"), Some(DataBits::Eight));
        assert_eq!(DataBits::parse("Five"), Some(DataBits::Five));
        assert_eq!(DataBits::parse("9"), None);
        assert_eq!(Parity::parse("E"), Some(Parity::Even));
        assert_eq!(Parity::parse("none"), Some(Parity::None));
        assert_eq!(Parity::parse("mark"), None);
        assert_eq!(parse_switch("ON"), Some(true));
        assert_eq!(parse_switch("off"), Some(false));
        assert_eq!(parse_switch("1"), None);
        assert_eq!(StopBits::parse("2"), Some(StopBits::Two));
        assert_eq!(StopBits::parse("three"), None);
        assert_eq!(Flow::parse("RTS/CTS"), Some(Flow::RtsCts));
        assert_eq!(Flow::parse("xonxoff"), Some(Flow::XonXoff));
        assert_eq!(Flow::parse("dtr"), None);
    }

    #[test]
    fn busy_is_not_reported_as_missing() {
        let busy = serial_error_text(&serialport::ErrorKind::NoDevice, "Device or resource busy");
        assert!(busy.contains("busy"), "{busy}");
        assert!(!busy.contains("not found"), "{busy}");
        assert!(busy.contains("Device or resource busy"), "{busy}");

        let locked = serial_error_text(
            &serialport::ErrorKind::NoDevice,
            "Unable to acquire exclusive lock on serial port",
        );
        assert!(locked.contains("busy"), "{locked}");

        let missing = serial_error_text(&serialport::ErrorKind::NoDevice, "");
        assert!(missing.contains("not found"), "{missing}");
        assert!(!missing.contains("busy"), "{missing}");

        let resource =
            serial_error_text(&serialport::ErrorKind::Io(io::ErrorKind::ResourceBusy), "");
        assert!(resource.contains("busy"), "{resource}");
    }

    #[test]
    fn access_denied_on_open_is_busy_not_missing() {
        // Windows: serialport reports ERROR_ACCESS_DENIED as NoDevice with
        // localized text, so the text must not matter.
        let text = open_error_text(
            &serialport::ErrorKind::NoDevice,
            "アクセスが拒否されました。",
            true,
        );
        assert!(text.contains("Port is busy"), "{text}");
        assert!(!text.contains("not found"), "{text}");
        assert!(text.contains("アクセスが拒否されました。"), "{text}");
        // A missing port (COM77) stays "not found".
        let missing = open_error_text(
            &serialport::ErrorKind::NoDevice,
            "指定されたファイルが見つかりません。",
            false,
        );
        assert!(missing.contains("not found"), "{missing}");
        assert!(!missing.contains("busy"), "{missing}");
    }

    #[cfg(windows)]
    #[test]
    fn access_denied_is_detected_by_code_not_text() {
        assert!(is_access_denied(&io::Error::from(
            io::ErrorKind::PermissionDenied
        )));
        assert!(!is_access_denied(&io::Error::from(io::ErrorKind::NotFound)));
        assert!(is_access_denied(&io::Error::from_raw_os_error(5)));
        assert!(!is_access_denied(&io::Error::from_raw_os_error(2)));
    }

    #[cfg(windows)]
    #[test]
    fn a_missing_windows_port_is_not_probed_as_busy() {
        assert!(!probe_access_denied("COM77"));
    }
}
