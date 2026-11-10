//! Application state shared by every page.
//!
//! The [`Controller`] is the single source of truth: pages mutate settings through
//! it and subscribe to [`Change`] notifications to refresh their widgets. While a
//! notification is being delivered `syncing()` is true, which widget handlers use
//! to ignore the programmatic updates they receive.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::{gio, glib};
use mchose_core::config::AppConfig;
use mchose_core::device::{Command, Event, Monitor};
use mchose_core::dsp::pipewire::{self, HwNodes, LiveControl, Service, Volume};
use mchose_core::dsp::{DspSettings, MicSettings};
use mchose_core::hidraw::DeviceInfo;
use mchose_core::{BatteryStatus, FirmwareTarget, FirmwareVersion, SoundMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// EQ gains / preset selection / pre-amp.
    Eq,
    /// Preset list (user presets added or removed).
    Presets,
    Effects,
    /// DSP service state.
    Dsp,
    Mic,
    Device,
    Volume,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Conn {
    #[default]
    Searching,
    Denied,
    Connected,
}

#[derive(Debug, Clone, Default)]
pub struct DeviceView {
    pub conn: Conn,
    pub info: Option<DeviceInfo>,
    pub battery: Option<BatteryStatus>,
    pub mode: Option<SoundMode>,
    pub fw_receiver: Option<FirmwareVersion>,
    pub fw_headset: Option<FirmwareVersion>,
}

impl DeviceView {
    pub fn headset_awake(&self) -> bool {
        self.conn == Conn::Connected && self.battery.is_some_and(|b| !b.asleep)
    }
}

type Listener = Rc<dyn Fn(Change)>;

pub struct Controller {
    me: std::rc::Weak<Controller>,
    cfg: RefCell<AppConfig>,
    live: LiveControl,
    listeners: RefCell<Vec<Listener>>,
    syncing: Cell<bool>,
    save_pending: Cell<bool>,
    pub device: RefCell<DeviceView>,
    pub hw: RefCell<HwNodes>,
    pub dsp_active: Cell<bool>,
    pub mic_active: Cell<bool>,
    pub busy: Cell<bool>,
    pub volume: Cell<Option<Volume>>,
    pub mic_volume: Cell<Option<Volume>>,
    pending_volume: Cell<Option<(u32, f32)>>,
    monitor: RefCell<Option<Monitor>>,
    toasts: RefCell<Option<adw::ToastOverlay>>,
    /// Demo mode: synthetic device, nothing is written to disk or to services.
    demo: bool,
}

impl Controller {
    pub fn new() -> Rc<Self> {
        let ctl = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            cfg: RefCell::new(AppConfig::load()),
            live: LiveControl::new(pipewire::DSP_NODE),
            listeners: RefCell::new(Vec::new()),
            syncing: Cell::new(false),
            save_pending: Cell::new(false),
            device: RefCell::new(DeviceView::default()),
            hw: RefCell::new(HwNodes::default()),
            dsp_active: Cell::new(Service::dsp().is_active()),
            mic_active: Cell::new(Service::mic().is_active()),
            busy: Cell::new(false),
            volume: Cell::new(None),
            mic_volume: Cell::new(None),
            pending_volume: Cell::new(None),
            monitor: RefCell::new(None),
            toasts: RefCell::new(None),
            demo: crate::debug::demo_enabled(),
        });
        if ctl.demo {
            ctl.start_demo();
        } else {
            ctl.start_monitor();
        }
        ctl.refresh_hw_nodes();
        ctl.start_background_refresh();
        ctl
    }

    // ----------------------------------------------------------------- plumbing

    pub fn subscribe(&self, f: impl Fn(Change) + 'static) {
        self.listeners.borrow_mut().push(Rc::new(f));
    }

    pub fn notify(&self, change: Change) {
        let listeners: Vec<Listener> = self.listeners.borrow().clone();
        let was = self.syncing.replace(true);
        for l in listeners {
            l(change);
        }
        self.syncing.set(was);
    }

    /// Runs `f` with `syncing()` set, for programmatic widget updates.
    pub fn run_synced(&self, f: impl FnOnce()) {
        let was = self.syncing.replace(true);
        f();
        self.syncing.set(was);
    }

    /// True while widgets are being refreshed programmatically.
    pub fn syncing(&self) -> bool {
        self.syncing.get()
    }

    pub fn set_toast_overlay(&self, overlay: &adw::ToastOverlay) {
        *self.toasts.borrow_mut() = Some(overlay.clone());
    }

    pub fn toast(&self, msg: &str) {
        if let Some(o) = self.toasts.borrow().as_ref() {
            let t = adw::Toast::new(msg);
            t.set_timeout(3);
            o.add_toast(t);
        }
    }

    pub fn dsp(&self) -> DspSettings {
        self.cfg.borrow().dsp.clone()
    }

    pub fn mic(&self) -> MicSettings {
        self.cfg.borrow().mic.clone()
    }

    fn card_name(&self) -> Option<String> {
        self.hw.borrow().sink.as_ref().map(|n| n.card_name.clone()).filter(|c| !c.is_empty())
    }

    // ------------------------------------------------------------------- device

    fn start_monitor(self: &Rc<Self>) {
        let (tx, rx) = async_channel::unbounded::<Event>();
        let monitor = Monitor::spawn(move |ev| {
            let _ = tx.send_blocking(ev);
        });
        *self.monitor.borrow_mut() = Some(monitor);
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            while let Ok(ev) = rx.recv().await {
                let Some(ctl) = weak.upgrade() else { break };
                ctl.handle_device_event(ev);
            }
        });
    }

    fn start_demo(&self) {
        use mchose_core::protocol::{ChargeState, FirmwareVersion};
        let info = DeviceInfo {
            devnode: "/dev/hidraw3".into(),
            vendor_id: mchose_core::protocol::VENDOR_ID,
            product_id: mchose_core::protocol::PRODUCT_ID,
            product: "MCHOSE V9 PRO".into(),
            manufacturer: "C-Media Electronics Inc".into(),
            serial: "0123456789AB".into(),
            phys: "usb-0000:80:14.0-6.1/input0".into(),
        };
        let charging = std::env::var_os("MCHOSE_DEMO_CHARGING").is_some();
        for ev in [
            Event::Connected(info),
            Event::Firmware { target: FirmwareTarget::Receiver, version: FirmwareVersion { raw: [0, 0, 0, 8] } },
            Event::Firmware { target: FirmwareTarget::Headset, version: FirmwareVersion { raw: [0, 0, 2, 9] } },
            Event::Battery(BatteryStatus {
                level: 78,
                charge: if charging { ChargeState::Charging } else { ChargeState::Discharging },
                asleep: false,
                raw_status: 2,
            }),
            Event::Mode(Some(SoundMode::Game1)),
        ] {
            self.handle_device_event(ev);
        }
        if let Ok(p) = std::env::var("MCHOSE_DEMO_PRESET") {
            self.cfg.borrow_mut().dsp.apply_preset(&p);
        }
        if std::env::var_os("MCHOSE_DEMO_EFFECTS").is_some() {
            let mut c = self.cfg.borrow_mut();
            c.dsp.bass.enabled = true;
            c.dsp.crossfeed.enabled = true;
        }
        if std::env::var_os("MCHOSE_DEMO_DSP").is_some() {
            self.dsp_active.set(true);
        }
    }

    fn handle_device_event(&self, ev: Event) {
        let mut refresh_nodes = false;
        {
            let mut d = self.device.borrow_mut();
            match ev {
                Event::Searching => *d = DeviceView { conn: Conn::Searching, ..Default::default() },
                Event::PermissionDenied { .. } => *d = DeviceView { conn: Conn::Denied, ..Default::default() },
                Event::Connected(info) => {
                    *d = DeviceView { conn: Conn::Connected, info: Some(info), ..Default::default() };
                    refresh_nodes = true;
                }
                Event::Disconnected => {
                    *d = DeviceView { conn: Conn::Searching, ..Default::default() };
                    refresh_nodes = true;
                }
                Event::Battery(b) => {
                    d.battery = Some(b);
                    if b.asleep {
                        d.mode = None;
                    }
                }
                Event::Mode(m) => d.mode = m,
                Event::Firmware { target: FirmwareTarget::Receiver, version } => d.fw_receiver = Some(version),
                Event::Firmware { target: FirmwareTarget::Headset, version } => d.fw_headset = Some(version),
                Event::Error(e) => log::warn!("device: {e}"),
            }
        }
        self.notify(Change::Device);
        if refresh_nodes {
            // ALSA nodes appear a moment after the HID interface.
            let weak = self.weak();
            glib::timeout_add_local_once(Duration::from_millis(1200), move || {
                if let Some(c) = weak.upgrade() {
                    c.refresh_hw_nodes();
                }
            });
        }
    }

    fn weak(&self) -> std::rc::Weak<Self> {
        self.me.clone()
    }

    pub fn set_mode(&self, mode: SoundMode) {
        if let Some(m) = self.monitor.borrow().as_ref() {
            m.send(Command::SetMode(mode));
        }
        self.device.borrow_mut().mode = Some(mode);
        self.notify(Change::Device);
    }

    pub fn refresh_device(&self) {
        if let Some(m) = self.monitor.borrow().as_ref() {
            m.send(Command::Refresh);
        }
        self.refresh_hw_nodes();
    }

    // --------------------------------------------------------- pipewire nodes

    pub fn refresh_hw_nodes(&self) {
        let weak = self.weak();
        glib::spawn_future_local(async move {
            let res = gio::spawn_blocking(|| {
                let nodes = pipewire::find_hw_nodes();
                let vol = nodes.sink.as_ref().and_then(|n| pipewire::get_volume(n.id));
                let mic = nodes.source.as_ref().and_then(|n| pipewire::get_volume(n.id));
                (nodes, vol, mic, Service::dsp().is_active(), Service::mic().is_active())
            })
            .await;
            let (Some(ctl), Ok((nodes, vol, mic, dsp, mic_on))) = (weak.upgrade(), res) else { return };
            let nodes_changed = *ctl.hw.borrow() != nodes;
            *ctl.hw.borrow_mut() = nodes;
            if nodes_changed && ctl.hw.borrow().sink.is_some() {
                ctl.write_dsp_config();
            }
            if ctl.pending_volume.get().is_none() {
                ctl.volume.set(vol);
                ctl.mic_volume.set(mic);
                ctl.notify(Change::Volume);
            }
            if !ctl.busy.get() && !ctl.demo && (dsp != ctl.dsp_active.get() || mic_on != ctl.mic_active.get()) {
                ctl.dsp_active.set(dsp);
                ctl.mic_active.set(mic_on);
                ctl.notify(Change::Dsp);
                ctl.notify(Change::Mic);
            }
        });
    }

    fn start_background_refresh(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_secs(5), move || {
            let Some(ctl) = weak.upgrade() else { return glib::ControlFlow::Break };
            ctl.refresh_hw_nodes();
            glib::ControlFlow::Continue
        });
    }

    /// Hardware volume of the headset (`source = false`) or its microphone.
    pub fn set_volume(&self, source: bool, level: f32) {
        let id = {
            let hw = self.hw.borrow();
            let node = if source { hw.source.as_ref() } else { hw.sink.as_ref() };
            match node {
                Some(n) => n.id,
                None => return,
            }
        };
        let cell = if source { &self.mic_volume } else { &self.volume };
        cell.set(Some(Volume { level, muted: cell.get().is_some_and(|v| v.muted) }));
        if self.demo {
            return;
        }
        // Coalesce: only the latest value is applied every 40 ms.
        let first = self.pending_volume.replace(Some((id, level))).is_none();
        if first {
            let weak = self.weak();
            glib::timeout_add_local_once(Duration::from_millis(40), move || {
                let Some(ctl) = weak.upgrade() else { return };
                if let Some((id, level)) = ctl.pending_volume.take() {
                    gio::spawn_blocking(move || {
                        let _ = pipewire::set_volume(id, level);
                    });
                }
            });
        }
    }

    pub fn set_muted(&self, source: bool, muted: bool) {
        let hw = self.hw.borrow();
        let Some(id) = (if source { hw.source.as_ref() } else { hw.sink.as_ref() }).map(|n| n.id) else { return };
        let cell = if source { &self.mic_volume } else { &self.volume };
        if let Some(mut v) = cell.get() {
            v.muted = muted;
            cell.set(Some(v));
        }
        if self.demo {
            return;
        }
        gio::spawn_blocking(move || {
            let _ = pipewire::set_mute(id, muted);
        });
    }

    // ---------------------------------------------------------------------- dsp

    /// Mutates DSP settings, applies them live and schedules a save.
    pub fn update_dsp(&self, change: Change, f: impl FnOnce(&mut DspSettings)) {
        let controls = {
            let mut cfg = self.cfg.borrow_mut();
            f(&mut cfg.dsp);
            cfg.dsp.sanitize();
            cfg.dsp.controls(pipewire::SAMPLE_RATE)
        };
        if self.dsp_active.get() {
            self.live.push(controls);
        }
        self.schedule_save();
        self.notify(change);
    }

    fn write_dsp_config(&self) {
        let conf = pipewire::render_dsp_config(&self.cfg.borrow().dsp, self.card_name().as_deref());
        if let Err(e) = Service::dsp().write_config(&conf) {
            log::warn!("write dsp config: {e}");
        }
    }

    fn schedule_save(&self) {
        if self.save_pending.replace(true) {
            return;
        }
        let weak = self.weak();
        glib::timeout_add_local_once(Duration::from_millis(500), move || {
            let Some(ctl) = weak.upgrade() else { return };
            ctl.save_pending.set(false);
            ctl.save_now();
        });
    }

    pub fn save_now(&self) {
        if self.demo {
            return;
        }
        if let Err(e) = self.cfg.borrow().save() {
            log::warn!("save settings: {e}");
        }
        self.write_dsp_config();
    }

    pub fn set_dsp_enabled(&self, on: bool) {
        if self.demo {
            self.dsp_active.set(on);
            self.notify(Change::Dsp);
            return;
        }
        if self.busy.replace(true) {
            return;
        }
        self.cfg.borrow_mut().dsp.enabled = on;
        self.save_now();
        let weak = self.weak();
        glib::spawn_future_local(async move {
            let res = gio::spawn_blocking(move || {
                let svc = Service::dsp();
                let r = if on { svc.enable_now() } else { svc.disable_now() };
                (r.map_err(|e| e.to_string()), svc.is_active())
            })
            .await;
            let Some(ctl) = weak.upgrade() else { return };
            ctl.busy.set(false);
            match res {
                Ok((Ok(()), active)) => {
                    ctl.dsp_active.set(active);
                    ctl.toast(if active {
                        "Equalizador e efeitos ativados"
                    } else {
                        "Processamento de áudio desligado"
                    });
                }
                Ok((Err(e), active)) => {
                    ctl.dsp_active.set(active);
                    ctl.toast(&format!("Falha ao alternar o DSP: {e}"));
                }
                Err(_) => ctl.toast("Falha ao alternar o DSP"),
            }
            ctl.notify(Change::Dsp);
        });
    }

    // ---------------------------------------------------------------------- mic

    pub fn update_mic(&self, f: impl FnOnce(&mut MicSettings)) {
        let (before, after) = {
            let mut cfg = self.cfg.borrow_mut();
            let before = cfg.mic.clone();
            f(&mut cfg.mic);
            (before, cfg.mic.clone())
        };
        if before == after {
            return;
        }
        if self.demo {
            self.mic_active.set(after.enabled);
            self.notify(Change::Mic);
            return;
        }
        if let Err(e) = self.cfg.borrow().save() {
            log::warn!("save settings: {e}");
        }
        self.notify(Change::Mic);
        if self.busy.replace(true) {
            return;
        }
        let source = self.hw.borrow().source.clone();
        let weak = self.weak();
        glib::spawn_future_local(async move {
            let res = gio::spawn_blocking(move || apply_mic(&before, &after, source)).await;
            let Some(ctl) = weak.upgrade() else { return };
            ctl.busy.set(false);
            match res {
                Ok(Ok((active, previous))) => {
                    ctl.mic_active.set(active);
                    if let Some(prev) = previous {
                        ctl.cfg.borrow_mut().mic.previous_default = prev;
                        let _ = ctl.cfg.borrow().save();
                    }
                }
                Ok(Err(e)) => ctl.toast(&format!("Microfone: {e}")),
                Err(_) => ctl.toast("Falha ao configurar o microfone"),
            }
            ctl.notify(Change::Mic);
        });
    }
}

/// Applies microphone settings (runs on a worker thread). Returns whether the
/// processed microphone is active afterwards and, when the default input was
/// changed, the value to remember as "previous default" (to restore later).
fn apply_mic(
    before: &MicSettings,
    after: &MicSettings,
    source: Option<pipewire::NodeInfo>,
) -> Result<(bool, Option<Option<String>>), String> {
    let svc = Service::mic();
    let restore_previous = |prev: &Option<String>| {
        let id = prev.as_deref().and_then(pipewire::node_id).or(source.as_ref().map(|n| n.id));
        if let Some(id) = id {
            let _ = pipewire::set_default_node(id);
        }
    };
    if !after.enabled {
        svc.disable_now().map_err(|e| e.to_string())?;
        if before.enabled && before.set_default {
            restore_previous(&after.previous_default);
            return Ok((false, Some(None)));
        }
        return Ok((false, None));
    }
    let conf = pipewire::render_mic_config(
        after,
        source.as_ref().map(|n| n.name.as_str()),
        source.as_ref().map(|n| n.card_name.as_str()),
    );
    let changed = svc.write_config(&conf).map_err(|e| e.to_string())?;
    if svc.is_active() {
        if changed {
            svc.restart().map_err(|e| e.to_string())?;
        }
    } else {
        svc.enable_now().map_err(|e| e.to_string())?;
    }
    let mut previous = None;
    if after.set_default && (!before.set_default || !before.enabled || changed) {
        let current = pipewire::default_node_name(true).filter(|n| n != pipewire::MIC_NODE);
        // Wait for the virtual source to appear, then make it the default input.
        for _ in 0..30 {
            if let Some(id) = pipewire::node_id(pipewire::MIC_NODE) {
                pipewire::set_default_node(id).map_err(|e| e.to_string())?;
                if current.is_some() {
                    previous = Some(current);
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    } else if !after.set_default && before.set_default {
        restore_previous(&after.previous_default);
        previous = Some(None);
    }
    Ok((svc.is_active(), previous))
}
