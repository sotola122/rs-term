//! One OS thread per open port.
//!
//! The thread owns `Box<dyn PortIo>`. There is no Tokio and no `try_clone`.
//! A read of `TimedOut` or `Interrupted` leaves the link open. Any other read
//! error is `PortLost`. A full data channel blocks this thread (and therefore
//! the next read) instead of dropping bytes. Shutdown is still taken from the
//! command channel while that send is waiting.

use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialParams {
    pub path: String,
    pub baud_rate: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
    pub flow: Flow,
    pub dtr_on_open: Option<bool>,
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
            dtr_on_open: None,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinState {
    pub cts: bool,
    pub dsr: bool,
    pub cd: bool,
    pub ri: bool,
}

/// Previous USB identity matches the current enumeration only when vid, pid,
/// and serial were all present on both sides. A missing serial never matches
/// on the path alone.
pub fn usb_identity_matches(previous: &UsbIdentity, current: &UsbIdentity) -> bool {
    match (&previous.serial_number, &current.serial_number) {
        (Some(before), Some(now)) => {
            previous.vid == current.vid && previous.pid == current.pid && before == now
        }
        _ => false,
    }
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
    fn set_dtr(&mut self, on: bool) -> Result<(), PortError>;
    fn set_rts(&mut self, on: bool) -> Result<(), PortError>;
    fn set_break(&mut self, on: bool) -> Result<(), PortError>;
    fn pins(&mut self) -> Result<PinState, PortError>;
}

pub(crate) fn open_port(params: &SerialParams) -> Result<Box<dyn PortIo>, PortError> {
    if !(1..=4_000_000).contains(&params.baud_rate) {
        return Err(PortError::Io {
            message: "ボーレートが範囲外（1〜4000000）".into(),
        });
    }
    let mut builder = serialport::new(&params.path, params.baud_rate)
        .data_bits(map_data_bits(params.data_bits))
        .parity(map_parity(params.parity))
        .stop_bits(map_stop(params.stop_bits))
        .flow_control(map_flow(params.flow))
        .timeout(params.read_timeout);
    if let Some(dtr) = params.dtr_on_open {
        builder = builder.dtr_on_open(dtr);
    }
    let port = builder
        .open()
        .map_err(|error| map_open_error(&params.path, error))?;
    let mut io = SerialIo {
        port,
        flow: params.flow,
    };
    if params.flow == Flow::None && params.rts {
        io.set_rts(true)?;
    }
    Ok(Box::new(io))
}

struct SerialIo {
    port: Box<dyn serialport::SerialPort>,
    flow: Flow,
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
            .map_err(|error| named("ボーレート", error))?;
        self.port
            .set_data_bits(map_data_bits(params.data_bits))
            .map_err(|error| named("データビット", error))?;
        self.port
            .set_parity(map_parity(params.parity))
            .map_err(|error| named("パリティ", error))?;
        self.port
            .set_stop_bits(map_stop(params.stop_bits))
            .map_err(|error| named("ストップビット", error))?;
        self.port
            .set_flow_control(map_flow(params.flow))
            .map_err(|error| named("フロー制御", error))?;
        self.flow = params.flow;
        Ok(())
    }

    fn set_dtr(&mut self, on: bool) -> Result<(), PortError> {
        self.port
            .write_data_terminal_ready(on)
            .map_err(map_serial_error)
    }

    fn set_rts(&mut self, on: bool) -> Result<(), PortError> {
        if self.flow != Flow::None {
            return Err(PortError::Unsupported(
                "RTS を手で動かせるのはフロー制御がなしのときだけ",
            ));
        }
        self.port
            .write_request_to_send(on)
            .map_err(map_serial_error)
    }

    fn set_break(&mut self, on: bool) -> Result<(), PortError> {
        if on {
            self.port.set_break().map_err(map_serial_error)
        } else {
            self.port.clear_break().map_err(map_serial_error)
        }
    }

    fn pins(&mut self) -> Result<PinState, PortError> {
        Ok(PinState {
            cts: self.port.read_clear_to_send().map_err(map_serial_error)?,
            dsr: self.port.read_data_set_ready().map_err(map_serial_error)?,
            cd: self.port.read_carrier_detect().map_err(map_serial_error)?,
            ri: self.port.read_ring_indicator().map_err(map_serial_error)?,
        })
    }
}

fn named(field: &str, error: serialport::Error) -> PortError {
    PortError::Io {
        message: format!("{field}をポートが受け付けない（OS: {}）", error.description),
    }
}

/// `serialport` errors in Japanese. The OS text stays in parentheses so the
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
        return with_os("ポートは使用中（他のプロセスが開いている）", description);
    }
    let what = match kind {
        serialport::ErrorKind::NoDevice => "ポートが見つからない（抜かれたか、名前が違う）",
        serialport::ErrorKind::InvalidInput => "設定値をポートが受け付けない",
        serialport::ErrorKind::Io(io::ErrorKind::NotFound) => "ポートが見つからない",
        serialport::ErrorKind::Io(io::ErrorKind::PermissionDenied) => {
            "アクセスが拒否された（他のアプリが使用中の可能性）"
        }
        serialport::ErrorKind::Io(io::ErrorKind::TimedOut) => "応答がない（タイムアウト）",
        serialport::ErrorKind::Io(_) => "入出力エラー",
        serialport::ErrorKind::Unknown => "原因不明のエラー",
    };
    with_os(what, description)
}

fn with_os(what: &str, description: &str) -> String {
    if description.is_empty() {
        what.to_string()
    } else {
        format!("{what}（OS: {description}）")
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
    Dtr(bool),
    Rts(bool),
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

pub(crate) fn spawn(port: Box<dyn PortIo>, generation: u64) -> (WorkerHandle, Channels) {
    let (data_tx, data_rx) = async_mpsc::channel(DATA_DEPTH);
    let (status_tx, status_rx) = async_mpsc::unbounded();
    let (cmd_tx, cmd_rx) = mpsc::sync_channel(COMMAND_DEPTH);
    let join = thread::Builder::new()
        .name("rs-term-serial".into())
        .spawn(move || {
            let outlet = Outlet {
                data: data_tx,
                status: status_tx,
            };
            worker_loop(port, outlet, cmd_rx, generation)
        })
        .expect("spawn serial thread");
    (
        WorkerHandle {
            commands: cmd_tx,
            join,
        },
        Channels {
            data: data_rx,
            status: status_rx,
        },
    )
}

struct Outlet {
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
        message: format!("ポートとの通信が切れた（OS: {error}）"),
    }
}

fn handle_command(ctx: &mut IoCtx<'_>, out: &mut Outlet, command: Command) -> Step {
    match command {
        Command::Shutdown => Step::Stop,
        Command::Cancel { generation: next } => {
            *ctx.generation = next;
            ctx.pending.clear();
            *ctx.pending_at = 0;
            Step::Continue
        }
        Command::Write {
            generation: write_generation,
            bytes,
        } => {
            if write_generation != *ctx.generation || bytes.is_empty() {
                return Step::Continue;
            }
            if *ctx.pending_at >= ctx.pending.len() {
                ctx.pending.clear();
                *ctx.pending_at = 0;
            }
            if ctx.pending.len() - *ctx.pending_at + bytes.len() > MAX_PENDING {
                out.status(IoEvent::CommandFailed(PortError::Io {
                    message: format!(
                        "送信待ちが上限 {} MiB を超えるので {} バイトを送らなかった",
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
        Command::Dtr(on) => {
            if let Err(error) = ctx.port.set_dtr(on) {
                out.status(IoEvent::CommandFailed(error));
            }
            Step::Continue
        }
        Command::Rts(on) => {
            if let Err(error) = ctx.port.set_rts(on) {
                out.status(IoEvent::CommandFailed(error));
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
                        "送信に失敗し、残り {} バイトを捨てた（OS: {error}）",
                        ctx.pending.len() - *ctx.pending_at
                    ),
                }));
                ctx.pending.clear();
                *ctx.pending_at = 0;
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
    let mut last_pins: Option<PinState> = None;
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
                    message: "ポートとの通信が切れた（EOF）".into(),
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

        if let Ok(pins) = port.pins() {
            if last_pins != Some(pins) {
                last_pins = Some(pins);
                out.status(IoEvent::Pins(pins));
            }
        }

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

    #[test]
    fn identity_requires_vid_pid_and_serial() {
        let full = UsbIdentity {
            vid: 0x0403,
            pid: 0x6001,
            serial_number: Some("ABC".into()),
            manufacturer: None,
            product: None,
        };
        let same = full.clone();
        assert!(usb_identity_matches(&full, &same));
        let mut missing = full.clone();
        missing.serial_number = None;
        assert!(!usb_identity_matches(&full, &missing));
        assert!(!usb_identity_matches(&missing, &full));
        let mut other = full.clone();
        other.serial_number = Some("XYZ".into());
        assert!(!usb_identity_matches(&full, &other));
    }

    #[test]
    fn busy_is_not_reported_as_missing() {
        let busy = serial_error_text(&serialport::ErrorKind::NoDevice, "Device or resource busy");
        assert!(busy.contains("使用中"), "{busy}");
        assert!(!busy.contains("見つからない"), "{busy}");
        assert!(busy.contains("Device or resource busy"), "{busy}");

        let locked = serial_error_text(
            &serialport::ErrorKind::NoDevice,
            "Unable to acquire exclusive lock on serial port",
        );
        assert!(locked.contains("使用中"), "{locked}");

        let missing = serial_error_text(&serialport::ErrorKind::NoDevice, "");
        assert!(missing.contains("見つからない"), "{missing}");
        assert!(!missing.contains("使用中"), "{missing}");

        let resource =
            serial_error_text(&serialport::ErrorKind::Io(io::ErrorKind::ResourceBusy), "");
        assert!(resource.contains("使用中"), "{resource}");
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
        assert!(text.contains("ポートは使用中"), "{text}");
        assert!(!text.contains("見つからない"), "{text}");
        assert!(text.contains("アクセスが拒否されました。"), "{text}");
        // A missing port (COM77) stays "not found".
        let missing = open_error_text(
            &serialport::ErrorKind::NoDevice,
            "指定されたファイルが見つかりません。",
            false,
        );
        assert!(missing.contains("見つからない"), "{missing}");
        assert!(!missing.contains("使用中"), "{missing}");
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
