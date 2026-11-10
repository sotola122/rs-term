//! One OS thread per open port.
//!
//! The thread owns `Box<dyn PortIo>`. There is no Tokio and no `try_clone`.
//! A read of `TimedOut` or `Interrupted` leaves the link open. Any other read
//! error is `PortLost`. Shutdown is taken from the command channel even when
//! the data channel is full.

use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use futures_channel::mpsc as async_mpsc;

use crate::error::PortError;
use crate::IoEvent;

pub const READ_TIMEOUT: Duration = Duration::from_millis(20);
const DATA_DEPTH: usize = 32;
const COMMAND_DEPTH: usize = 32;
const MAX_CHUNK: usize = 4096;

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
            message: "baud_rate が範囲外".into(),
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
    let port = builder.open().map_err(map_serial_error)?;
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
            .map_err(|error| named("baud_rate", error))?;
        self.port
            .set_data_bits(map_data_bits(params.data_bits))
            .map_err(|error| named("data_bits", error))?;
        self.port
            .set_parity(map_parity(params.parity))
            .map_err(|error| named("parity", error))?;
        self.port
            .set_stop_bits(map_stop(params.stop_bits))
            .map_err(|error| named("stop_bits", error))?;
        self.port
            .set_flow_control(map_flow(params.flow))
            .map_err(|error| named("flow", error))?;
        self.flow = params.flow;
        Ok(())
    }

    fn set_dtr(&mut self, on: bool) -> Result<(), PortError> {
        self.port
            .write_data_terminal_ready(on)
            .map_err(map_serial_error)
    }

    fn set_rts(&mut self, on: bool) -> Result<(), PortError> {
        if self.flow == Flow::RtsCts {
            return Err(PortError::Unsupported(
                "RTS/CTS のあいだ RTS は手で動かさない",
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
        message: format!("{field}: {error}"),
    }
}

fn map_serial_error(error: serialport::Error) -> PortError {
    PortError::Io {
        message: format!("{:?}: {}", error.kind, error.description),
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
    Write { generation: u64, bytes: Vec<u8> },
    Apply(SerialParams),
    Dtr(bool),
    Rts(bool),
    Break { hold: Duration },
    Cancel { generation: u64 },
    Shutdown,
}

pub(crate) struct WorkerHandle {
    pub commands: SyncSender<Command>,
    pub join: JoinHandle<()>,
}

pub(crate) fn spawn(
    port: Box<dyn PortIo>,
    generation: u64,
) -> (WorkerHandle, async_mpsc::Receiver<IoEvent>) {
    let (data_tx, data_rx) = async_mpsc::channel(DATA_DEPTH);
    let (cmd_tx, cmd_rx) = mpsc::sync_channel(COMMAND_DEPTH);
    let join = thread::Builder::new()
        .name("rs-term-serial".into())
        .spawn(move || worker_loop(port, data_tx, cmd_rx, generation))
        .expect("spawn serial thread");
    (
        WorkerHandle {
            commands: cmd_tx,
            join,
        },
        data_rx,
    )
}

fn worker_loop(
    mut port: Box<dyn PortIo>,
    mut data_tx: async_mpsc::Sender<IoEvent>,
    commands: Receiver<Command>,
    mut generation: u64,
) {
    let mut buffer = [0u8; MAX_CHUNK];
    let mut pending: Vec<u8> = Vec::new();
    let mut pending_at = 0usize;
    let mut break_until: Option<Instant> = None;
    let mut lost = false;
    let mut last_pins: Option<PinState> = None;
    let mut dropped = 0u64;
    loop {
        let mut read_was_idle = false;
        if !lost {
            match port.read(&mut buffer) {
                Ok(0) => read_was_idle = true,
                Ok(count) => {
                    let chunk = buffer[..count.min(MAX_CHUNK)].to_vec();
                    send_data(&mut data_tx, IoEvent::Data(chunk), &mut dropped);
                }
                Err(error)
                    if error.kind() == io::ErrorKind::TimedOut
                        || error.kind() == io::ErrorKind::Interrupted =>
                {
                    read_was_idle = true;
                }
                Err(error) => {
                    send_data(
                        &mut data_tx,
                        IoEvent::PortLost(PortError::Io {
                            message: error.to_string(),
                        }),
                        &mut dropped,
                    );
                    lost = true;
                    read_was_idle = true;
                }
            }
        } else {
            read_was_idle = true;
        }

        let mut saw_command = false;
        for _ in 0..8 {
            match commands.try_recv() {
                Ok(command) => {
                    saw_command = true;
                    match command {
                        Command::Shutdown => return,
                        Command::Cancel { generation: next } => {
                            generation = next;
                            pending.clear();
                            pending_at = 0;
                        }
                        Command::Write {
                            generation: write_generation,
                            bytes,
                        } => {
                            if write_generation == generation && !bytes.is_empty() {
                                if pending_at >= pending.len() {
                                    pending.clear();
                                    pending_at = 0;
                                }
                                pending.extend(bytes);
                            }
                        }
                        Command::Apply(params) => {
                            if let Err(error) = port.apply(&params) {
                                send_data(
                                    &mut data_tx,
                                    IoEvent::CommandFailed(error),
                                    &mut dropped,
                                );
                            }
                        }
                        Command::Dtr(on) => {
                            if let Err(error) = port.set_dtr(on) {
                                send_data(
                                    &mut data_tx,
                                    IoEvent::CommandFailed(error),
                                    &mut dropped,
                                );
                            }
                        }
                        Command::Rts(on) => {
                            if let Err(error) = port.set_rts(on) {
                                send_data(
                                    &mut data_tx,
                                    IoEvent::CommandFailed(error),
                                    &mut dropped,
                                );
                            }
                        }
                        Command::Break { hold } => {
                            if let Err(error) = port.set_break(true) {
                                send_data(
                                    &mut data_tx,
                                    IoEvent::CommandFailed(error),
                                    &mut dropped,
                                );
                            } else {
                                break_until = Some(Instant::now() + hold);
                            }
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }

        if break_until.is_some_and(|until| Instant::now() >= until) {
            let _ = port.set_break(false);
            break_until = None;
        }

        if pending_at < pending.len() {
            match port.write(&pending[pending_at..]) {
                Ok(0) => {}
                Ok(count) => {
                    pending_at += count;
                    if pending_at >= pending.len() {
                        pending.clear();
                        pending_at = 0;
                    }
                }
                Err(error)
                    if error.kind() == io::ErrorKind::TimedOut
                        || error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => {
                    send_data(
                        &mut data_tx,
                        IoEvent::CommandFailed(PortError::Io {
                            message: error.to_string(),
                        }),
                        &mut dropped,
                    );
                    pending.clear();
                    pending_at = 0;
                }
            }
        }

        if !lost {
            if let Ok(pins) = port.pins() {
                if last_pins != Some(pins) {
                    last_pins = Some(pins);
                    send_data(&mut data_tx, IoEvent::Pins(pins), &mut dropped);
                }
            }
        }

        if read_was_idle && !saw_command && pending_at >= pending.len() {
            // A real port already blocked inside `read` for `read_timeout`.
            // This yield only matters for a mock that returns TimedOut
            // without blocking, so the test thread cannot spin a core.
            thread::park_timeout(Duration::from_millis(2));
        }
    }
}

fn send_data(tx: &mut async_mpsc::Sender<IoEvent>, event: IoEvent, dropped: &mut u64) {
    match tx.try_send(event) {
        Ok(()) => {}
        Err(error) if error.is_full() => {
            if let IoEvent::Data(bytes) = error.into_inner() {
                *dropped += bytes.len() as u64;
                let _ = tx.try_send(IoEvent::Dropped { bytes: *dropped });
            }
        }
        Err(_) => {}
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
}
