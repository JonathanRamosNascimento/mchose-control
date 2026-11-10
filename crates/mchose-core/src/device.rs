//! High level access to the headset: a synchronous [`Headset`] handle (used by the
//! CLI) and a background [`Monitor`] that handles hot-plug, polling and spontaneous
//! notifications (used by the GUI).

use std::io;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::hidraw::{self, DeviceInfo, Hidraw};
use crate::protocol::{self, BatteryStatus, FirmwareTarget, FirmwareVersion, InputReport, SoundMode};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("nenhum receptor MCHOSE encontrado")]
    NoDevice,
    #[error("sem permissão para abrir {0} (instale a regra udev)")]
    PermissionDenied(PathBuf),
    #[error("tempo esgotado aguardando resposta: {0}")]
    Timeout(&'static str),
    #[error("resposta inválida do dispositivo: {0}")]
    BadResponse(&'static str),
    #[error(transparent)]
    Io(#[from] io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Synchronous handle to one receiver.
pub struct Headset {
    hid: Hidraw,
    info: DeviceInfo,
}

impl Headset {
    pub fn open(info: DeviceInfo) -> Result<Self> {
        match Hidraw::open(&info.devnode) {
            Ok(hid) => Ok(Self { hid, info }),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => Err(Error::PermissionDenied(info.devnode.clone())),
            Err(e) => Err(e.into()),
        }
    }

    pub fn open_first() -> Result<Self> {
        let info = hidraw::enumerate().into_iter().next().ok_or(Error::NoDevice)?;
        Self::open(info)
    }

    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }

    pub fn send_battery_query(&self) -> Result<()> {
        Ok(self.hid.write(&protocol::battery_query_frame())?)
    }

    pub fn send_mode_query(&self) -> Result<()> {
        Ok(self.hid.write(&protocol::mode_query_frame())?)
    }

    pub fn send_mode_set(&self, mode: SoundMode) -> Result<()> {
        Ok(self.hid.write(&protocol::mode_set_frame(mode))?)
    }

    /// Reads a firmware version through feature report 0xAA (blocks ~300 ms).
    pub fn read_firmware(&self, target: FirmwareTarget) -> Result<FirmwareVersion> {
        self.hid.set_feature(&protocol::firmware_query_frame(target))?;
        thread::sleep(Duration::from_millis(protocol::FIRMWARE_READ_DELAY_MS));
        let frame = self.hid.get_feature(protocol::REPORT_FIRMWARE, protocol::FRAME_LEN)?;
        protocol::parse_firmware_frame(&frame).ok_or(Error::BadResponse("firmware"))
    }

    /// Next decoded input report, or `None` after `timeout`.
    pub fn next_report(&self, timeout: Duration) -> Result<Option<InputReport>> {
        let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
        Ok(self.hid.read_timeout(ms)?.and_then(|f| protocol::parse_input(&f)))
    }

    /// Reads raw frames (debugging / protocol exploration).
    pub fn next_raw(&self, timeout: Duration) -> Result<Option<Vec<u8>>> {
        let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
        Ok(self.hid.read_timeout(ms)?)
    }

    fn wait_for<T>(
        &self,
        what: &'static str,
        timeout: Duration,
        mut pick: impl FnMut(InputReport) -> Option<T>,
    ) -> Result<T> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Error::Timeout(what));
            }
            if let Some(r) = self.next_report(left)? {
                if let Some(v) = pick(r) {
                    return Ok(v);
                }
            }
        }
    }

    pub fn battery(&self, timeout: Duration) -> Result<BatteryStatus> {
        self.send_battery_query()?;
        self.wait_for("bateria", timeout, |r| match r {
            InputReport::Battery(b) => Some(b),
            _ => None,
        })
    }

    /// Returns the raw mode byte together with its decoded value.
    pub fn mode(&self, timeout: Duration) -> Result<(Option<SoundMode>, u8)> {
        self.send_mode_query()?;
        self.wait_for("modo de som", timeout, |r| match r {
            InputReport::Mode { mode, raw } => Some((mode, raw)),
            _ => None,
        })
    }

    /// Sets the mode and confirms it by reading it back.
    pub fn set_mode(&self, mode: SoundMode, timeout: Duration) -> Result<Option<SoundMode>> {
        self.send_mode_set(mode)?;
        thread::sleep(Duration::from_millis(150));
        Ok(self.mode(timeout)?.0)
    }
}

/// Requests accepted by the [`Monitor`].
#[derive(Debug, Clone)]
pub enum Command {
    /// Re-read everything (firmware, battery, mode).
    Refresh,
    ReadBattery,
    SetMode(SoundMode),
    Shutdown,
}

/// Notifications produced by the [`Monitor`].
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// No receiver plugged in.
    Searching,
    /// Receiver found, but the hidraw node is not accessible.
    PermissionDenied {
        devnode: PathBuf,
    },
    Connected(DeviceInfo),
    Disconnected,
    Firmware {
        target: FirmwareTarget,
        version: FirmwareVersion,
    },
    Battery(BatteryStatus),
    Mode(Option<SoundMode>),
    Error(String),
}

/// Background worker that owns the device.
pub struct Monitor {
    tx: mpsc::Sender<Command>,
    handle: Option<JoinHandle<()>>,
}

const SCAN_INTERVAL: Duration = Duration::from_millis(1500);
const POLL_AWAKE: Duration = Duration::from_secs(30);
const POLL_ASLEEP: Duration = Duration::from_secs(8);

impl Monitor {
    pub fn spawn(sink: impl Fn(Event) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let handle = thread::Builder::new()
            .name("mchose-monitor".into())
            .spawn(move || run(rx, sink))
            .expect("spawn monitor thread");
        Self { tx, handle: Some(handle) }
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

enum Exit {
    Shutdown,
    Lost,
}

fn run(rx: mpsc::Receiver<Command>, sink: impl Fn(Event)) {
    #[derive(PartialEq)]
    enum Last {
        None,
        Searching,
        Denied,
    }
    let mut last = Last::None;
    loop {
        match hidraw::enumerate().into_iter().next() {
            None => {
                if last != Last::Searching {
                    sink(Event::Searching);
                    last = Last::Searching;
                }
            }
            Some(info) => match Headset::open(info.clone()) {
                Ok(dev) => {
                    sink(Event::Connected(info));
                    let exit = session(&dev, &rx, &sink);
                    sink(Event::Disconnected);
                    last = Last::None;
                    if matches!(exit, Exit::Shutdown) {
                        return;
                    }
                    continue;
                }
                Err(Error::PermissionDenied(devnode)) => {
                    if last != Last::Denied {
                        sink(Event::PermissionDenied { devnode });
                        last = Last::Denied;
                    }
                }
                Err(e) => sink(Event::Error(e.to_string())),
            },
        }
        // Idle: wait for the next scan, but stay responsive to Shutdown.
        match rx.recv_timeout(SCAN_INTERVAL) {
            Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
            _ => {}
        }
    }
}

fn session(dev: &Headset, rx: &mpsc::Receiver<Command>, sink: &impl Fn(Event)) -> Exit {
    let mut headset_fw_known = false;
    let mut asleep = false;
    let mut next_poll = Instant::now();
    let mut pending_mode_query: Option<Instant> = None;

    let read_fw = |target| -> std::result::Result<(), Exit> {
        match dev.read_firmware(target) {
            Ok(version) => {
                sink(Event::Firmware { target, version });
                Ok(())
            }
            Err(Error::Io(e)) if is_gone(&e) => Err(Exit::Lost),
            Err(e) => {
                log::warn!("firmware {target}: {e}");
                Ok(())
            }
        }
    };

    if read_fw(FirmwareTarget::Receiver).is_err() {
        return Exit::Lost;
    }
    let initial = dev.send_mode_query();
    if let Err(Error::Io(e)) = initial {
        if is_gone(&e) {
            return Exit::Lost;
        }
    }

    loop {
        // Commands from the UI.
        loop {
            match rx.try_recv() {
                Ok(Command::Shutdown) | Err(mpsc::TryRecvError::Disconnected) => {
                    return Exit::Shutdown;
                }
                Ok(Command::Refresh) => {
                    if read_fw(FirmwareTarget::Receiver).is_err() {
                        return Exit::Lost;
                    }
                    headset_fw_known = false;
                    next_poll = Instant::now();
                    pending_mode_query = Some(Instant::now());
                }
                Ok(Command::ReadBattery) => next_poll = Instant::now(),
                Ok(Command::SetMode(m)) => {
                    if let Err(e) = dev.send_mode_set(m) {
                        sink(Event::Error(e.to_string()));
                    }
                    pending_mode_query = Some(Instant::now() + Duration::from_millis(200));
                }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }

        let now = Instant::now();
        if now >= next_poll {
            if let Err(Error::Io(e)) = dev.send_battery_query() {
                if is_gone(&e) {
                    return Exit::Lost;
                }
            }
            next_poll = now + if asleep { POLL_ASLEEP } else { POLL_AWAKE };
        }
        if pending_mode_query.is_some_and(|t| now >= t) {
            pending_mode_query = None;
            let _ = dev.send_mode_query();
        }

        match dev.next_report(Duration::from_millis(100)) {
            Ok(Some(InputReport::Battery(b))) => {
                let woke = asleep && !b.asleep;
                asleep = b.asleep;
                sink(Event::Battery(b));
                if !asleep && (!headset_fw_known || woke) {
                    headset_fw_known = read_fw(FirmwareTarget::Headset).is_ok();
                    if woke {
                        pending_mode_query = Some(Instant::now());
                    }
                }
            }
            Ok(Some(InputReport::Mode { mode, .. })) => sink(Event::Mode(mode)),
            Ok(Some(InputReport::Other { report_id, payload })) => {
                log::debug!("report 0x{report_id:02x}: {:02x?}", &payload[..payload.len().min(16)]);
            }
            Ok(None) => {}
            Err(Error::Io(e)) if is_gone(&e) => return Exit::Lost,
            Err(e) => {
                sink(Event::Error(e.to_string()));
                thread::sleep(Duration::from_millis(500));
            }
        }
    }
}

fn is_gone(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(libc::ENODEV) | Some(libc::EIO) | Some(libc::ENXIO))
        || e.kind() == io::ErrorKind::NotConnected
}
