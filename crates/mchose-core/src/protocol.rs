//! Wire protocol of the MCHOSE "BLK" C-Media 2.4 GHz receiver.
//!
//! Reverse-engineered from the official MCHOSE HUB (web build, `protocolProfile: BLK`)
//! and cross-checked against the dongle's HID report descriptor. See `docs/PROTOCOL.md`.
//!
//! Every buffer handled here is a *full hidraw frame*: byte 0 is the report ID,
//! followed by the report payload. The vendor reports used are 63 bytes long, so a
//! frame is 64 bytes.
//!
//! This module is pure (no I/O) so it can be unit tested without hardware.

use std::fmt;

/// USB vendor ID of the C-Media based receiver.
pub const VENDOR_ID: u16 = 0x291D;
/// USB product ID shared by V9 PRO / V9 / G9 PRO / S9 PRO receivers.
pub const PRODUCT_ID: u16 = 0x385D;

/// Input + output report used for commands, responses and spontaneous notifications.
pub const REPORT_COMMAND: u8 = 0x55;
/// Feature report used to query firmware versions.
pub const REPORT_FIRMWARE: u8 = 0xAA;
/// Output report that carries firmware-update payloads. Never used by this crate.
pub const REPORT_OTA_DATA: u8 = 0x41;

/// Payload length of the vendor reports (without the report ID byte).
pub const PAYLOAD_LEN: usize = 63;
/// Full frame length (report ID + payload).
pub const FRAME_LEN: usize = PAYLOAD_LEN + 1;

/// Command bytes (first payload byte of report 0x55).
pub mod cmd {
    /// Query current sound mode. Response: `[0x11, mode]`.
    pub const MODE_QUERY: u8 = 0x11;
    /// Set sound mode: `[0x21, 0x01, mode]`.
    pub const MODE_SET: u8 = 0x21;
    /// Query battery: `[0x65, 0x01]`. Response: `[0x65, level, status]`.
    pub const BATTERY: u8 = 0x65;
    /// Firmware query sub-command inside feature report 0xAA: `[0x01, target]`.
    pub const FIRMWARE: u8 = 0x01;
}

/// Battery status byte values.
pub mod battery_status {
    pub const DISCHARGING: u8 = 0x02;
    pub const CHARGING: u8 = 0x03;
    pub const FULL: u8 = 0x04;
    /// Headset is off / asleep while the receiver is still plugged in.
    pub const ASLEEP: u8 = 0x1A;
}

/// Minimum receiver firmware that exposes battery + sound-mode commands ("P8" firmware).
/// Older ("P1") receivers only answer the firmware query.
pub const MIN_RECEIVER_FW_FOR_FEATURES: u32 = 8;

/// Delay the official software waits between SET_FEATURE and GET_FEATURE when
/// querying firmware versions.
pub const FIRMWARE_READ_DELAY_MS: u64 = 300;

/// The three on-headset sound profiles (selected by the mode button on the headset too).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundMode {
    Game1 = 0,
    Game2 = 1,
    Music = 2,
}

impl SoundMode {
    pub const ALL: [SoundMode; 3] = [SoundMode::Game1, SoundMode::Game2, SoundMode::Music];

    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::Game1),
            1 => Some(Self::Game2),
            2 => Some(Self::Music),
            _ => None,
        }
    }

    pub fn as_byte(self) -> u8 {
        self as u8
    }

    /// Stable identifier used by the CLI and config files.
    pub fn id(self) -> &'static str {
        match self {
            Self::Game1 => "game1",
            Self::Game2 => "game2",
            Self::Music => "music",
        }
    }

    pub fn from_id(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().replace([' ', '-', '_'], "").as_str() {
            "game1" | "g1" | "0" | "fps" => Some(Self::Game1),
            "game2" | "g2" | "1" => Some(Self::Game2),
            "music" | "musica" | "música" | "2" => Some(Self::Music),
            _ => None,
        }
    }
}

impl fmt::Display for SoundMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Game1 => "Game 1",
            Self::Game2 => "Game 2",
            Self::Music => "Music",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChargeState {
    Discharging,
    Charging,
    Full,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BatteryStatus {
    /// Battery level in percent (0–100).
    pub level: u8,
    pub charge: ChargeState,
    /// Headset powered off / asleep (receiver still present).
    pub asleep: bool,
    /// Raw status byte, kept for diagnostics.
    pub raw_status: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FirmwareTarget {
    /// The headset itself (reached over the 2.4 GHz link).
    Headset = 0,
    /// The USB receiver (dongle).
    Receiver = 1,
}

impl fmt::Display for FirmwareTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Headset => "headset",
            Self::Receiver => "receiver",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FirmwareVersion {
    pub raw: [u8; 4],
}

impl FirmwareVersion {
    /// Numeric value as the official app computes it: the four bytes are printed as
    /// decimal numbers, concatenated, and parsed back (`[0,0,0,8]` → "0008" → 8).
    pub fn value(&self) -> u32 {
        self.to_string().parse().unwrap_or(0)
    }
}

impl fmt::Display for FirmwareVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.raw {
            write!(f, "{b}")?;
        }
        Ok(())
    }
}

/// A decoded input report coming from the receiver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputReport {
    Battery(BatteryStatus),
    /// Current sound mode. `None` if the byte is outside the known range.
    Mode {
        mode: Option<SoundMode>,
        raw: u8,
    },
    /// Any other report (media keys, telephony, OTA status...).
    Other {
        report_id: u8,
        payload: Vec<u8>,
    },
}

fn frame(report_id: u8, payload: &[u8]) -> [u8; FRAME_LEN] {
    debug_assert!(payload.len() <= PAYLOAD_LEN);
    let mut f = [0u8; FRAME_LEN];
    f[0] = report_id;
    f[1..1 + payload.len()].copy_from_slice(payload);
    f
}

/// Output frame asking for the current sound mode.
pub fn mode_query_frame() -> [u8; FRAME_LEN] {
    frame(REPORT_COMMAND, &[cmd::MODE_QUERY])
}

/// Output frame selecting a sound mode.
pub fn mode_set_frame(mode: SoundMode) -> [u8; FRAME_LEN] {
    frame(REPORT_COMMAND, &[cmd::MODE_SET, 0x01, mode.as_byte()])
}

/// Output frame asking for the battery state.
pub fn battery_query_frame() -> [u8; FRAME_LEN] {
    frame(REPORT_COMMAND, &[cmd::BATTERY, 0x01])
}

/// Feature frame (SET_FEATURE) that selects which firmware version the next
/// GET_FEATURE on report 0xAA will return.
pub fn firmware_query_frame(target: FirmwareTarget) -> [u8; FRAME_LEN] {
    frame(REPORT_FIRMWARE, &[cmd::FIRMWARE, target as u8])
}

/// Parse the GET_FEATURE answer of report 0xAA (frame includes the report ID).
pub fn parse_firmware_frame(frame: &[u8]) -> Option<FirmwareVersion> {
    if frame.len() < 6 || frame[0] != REPORT_FIRMWARE || frame[1] != cmd::FIRMWARE {
        return None;
    }
    Some(FirmwareVersion { raw: [frame[2], frame[3], frame[4], frame[5]] })
}

pub fn parse_battery_payload(p: &[u8]) -> Option<BatteryStatus> {
    if p.len() < 3 || p[0] != cmd::BATTERY {
        return None;
    }
    let status = p[2];
    let charge = match status {
        battery_status::CHARGING => ChargeState::Charging,
        battery_status::FULL => ChargeState::Full,
        battery_status::DISCHARGING => ChargeState::Discharging,
        _ => ChargeState::Unknown,
    };
    Some(BatteryStatus { level: p[1].min(100), charge, asleep: status == battery_status::ASLEEP, raw_status: status })
}

/// Decode a hidraw input frame (byte 0 = report ID).
pub fn parse_input(frame: &[u8]) -> Option<InputReport> {
    let (&report_id, payload) = frame.split_first()?;
    if report_id == REPORT_COMMAND {
        match payload.first() {
            Some(&cmd::BATTERY) => {
                if let Some(b) = parse_battery_payload(payload) {
                    return Some(InputReport::Battery(b));
                }
            }
            Some(&cmd::MODE_QUERY) if payload.len() >= 2 => {
                return Some(InputReport::Mode { mode: SoundMode::from_byte(payload[1]), raw: payload[1] });
            }
            _ => {}
        }
    }
    Some(InputReport::Other { report_id, payload: payload.to_vec() })
}

/// Checks whether a raw HID report descriptor belongs to the vendor interface we
/// talk to (declares both report 0x55 and feature report 0xAA).
pub fn is_vendor_interface(report_descriptor: &[u8]) -> bool {
    let has = |id: u8| report_descriptor.windows(2).any(|w| w == [0x85, id]);
    has(REPORT_COMMAND) && has(REPORT_FIRMWARE)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Report descriptor dumped from a real MCHOSE V9 PRO receiver (fw "0008").
    const V9_PRO_DESCRIPTOR: &[u8] = &[
        0x06, 0x21, 0xff, 0x09, 0x01, 0xa1, 0x01, 0x15, 0x00, 0x26, 0xff, 0x00, 0x85, 0xed, 0x75, 0x08, 0x95, 0x01,
        0x09, 0x00, 0x81, 0x02, 0x09, 0x00, 0x91, 0x02, 0x09, 0x00, 0x95, 0x0f, 0xb2, 0x02, 0x01, 0xc0, 0x06, 0x22,
        0xff, 0x09, 0x01, 0xa1, 0x01, 0x15, 0x00, 0x26, 0xff, 0x00, 0x85, 0xaa, 0x75, 0x08, 0x09, 0x00, 0x95, 0x3f,
        0xb2, 0x02, 0x01, 0xc0, 0x06, 0x90, 0xff, 0x09, 0x01, 0xa1, 0x01, 0x09, 0x03, 0x85, 0x55, 0x15, 0x00, 0x26,
        0xff, 0x00, 0x75, 0x08, 0x95, 0x3f, 0x82, 0x02, 0x01, 0xc0,
    ];

    #[test]
    fn frames_have_expected_layout() {
        let f = mode_set_frame(SoundMode::Music);
        assert_eq!(f.len(), 64);
        assert_eq!(&f[..4], &[0x55, 0x21, 0x01, 0x02]);
        assert!(f[4..].iter().all(|&b| b == 0));

        assert_eq!(&mode_query_frame()[..3], &[0x55, 0x11, 0x00]);
        assert_eq!(&battery_query_frame()[..3], &[0x55, 0x65, 0x01]);
        assert_eq!(&firmware_query_frame(FirmwareTarget::Receiver)[..3], &[0xAA, 0x01, 0x01]);
        assert_eq!(&firmware_query_frame(FirmwareTarget::Headset)[..3], &[0xAA, 0x01, 0x00]);
    }

    #[test]
    fn parses_battery_reports() {
        let mut f = [0u8; 64];
        f[..4].copy_from_slice(&[0x55, 0x65, 87, 0x03]);
        assert_eq!(
            parse_input(&f),
            Some(InputReport::Battery(BatteryStatus {
                level: 87,
                charge: ChargeState::Charging,
                asleep: false,
                raw_status: 3
            }))
        );
        f[3] = 0x1A;
        match parse_input(&f) {
            Some(InputReport::Battery(b)) => assert!(b.asleep),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parses_mode_reports() {
        let f = [0x55, 0x11, 0x01, 0, 0];
        assert_eq!(parse_input(&f), Some(InputReport::Mode { mode: Some(SoundMode::Game2), raw: 1 }));
        let f = [0x55, 0x11, 0x09];
        assert_eq!(parse_input(&f), Some(InputReport::Mode { mode: None, raw: 9 }));
    }

    #[test]
    fn unknown_reports_are_passed_through() {
        let f = [0x06, 0x01, 0x00];
        assert_eq!(parse_input(&f), Some(InputReport::Other { report_id: 6, payload: vec![1, 0] }));
    }

    #[test]
    fn parses_firmware_versions() {
        let f = [0xAA, 0x01, 0, 0, 2, 9, 0, 0];
        let v = parse_firmware_frame(&f).unwrap();
        assert_eq!(v.to_string(), "0029");
        assert_eq!(v.value(), 29);
        assert!(parse_firmware_frame(&[0xAA, 0x02, 0, 0, 0, 8]).is_none());
    }

    #[test]
    fn recognises_vendor_interface() {
        assert!(is_vendor_interface(V9_PRO_DESCRIPTOR));
        assert!(!is_vendor_interface(&[0x05, 0x01, 0x09, 0x02, 0x85, 0x01]));
    }

    #[test]
    fn sound_mode_ids_roundtrip() {
        for m in SoundMode::ALL {
            assert_eq!(SoundMode::from_id(m.id()), Some(m));
            assert_eq!(SoundMode::from_byte(m.as_byte()), Some(m));
        }
        assert_eq!(SoundMode::from_id("Game 2"), Some(SoundMode::Game2));
    }
}
