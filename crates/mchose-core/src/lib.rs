//! Core library of **MCHOSE Control** — a Linux companion for MCHOSE headsets that
//! use the C-Media 2.4 GHz receiver (`291d:385d`: V9 PRO, V9, G9 PRO, S9 PRO).
//!
//! * [`protocol`] — the vendor HID protocol (pure, unit tested)
//! * [`hidraw`] — Linux hidraw I/O and device discovery
//! * [`device`] — synchronous handle and background monitor
//! * [`dsp`] — EQ / effects / mic processing through PipeWire
//! * [`config`] — persisted user settings

pub mod config;
pub mod device;
pub mod dsp;
pub mod hidraw;
pub mod protocol;

pub use device::{Command, Event, Headset, Monitor};
pub use protocol::{BatteryStatus, ChargeState, FirmwareTarget, FirmwareVersion, SoundMode};

/// udev rule required for unprivileged access to the vendor HID interface.
pub const UDEV_RULE: &str = include_str!("../../../packaging/udev/70-mchose.rules");
pub const UDEV_RULE_PATH: &str = "/etc/udev/rules.d/70-mchose.rules";
