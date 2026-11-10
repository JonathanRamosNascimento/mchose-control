//! Host-side audio processing.
//!
//! On Windows the MCHOSE software ships a C-Media "Xear" APO (`CmApoIce.dll`) that
//! performs EQ, virtual bass, clarity and microphone processing on the PC — none of
//! it runs inside the headset. On Linux the same role is played by PipeWire:
//! a filter-chain smart filter for playback and an echo-cancel (WebRTC) node for
//! the microphone.

pub mod pipewire;
pub mod response;
pub mod settings;

pub use settings::{DspSettings, MicSettings, Preset};
