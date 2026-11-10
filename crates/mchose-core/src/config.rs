//! Persistent application settings (`~/.config/mchose/settings.json`).

use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::dsp::pipewire::{app_config_dir, write_atomic};
use crate::dsp::{DspSettings, MicSettings};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub dsp: DspSettings,
    pub mic: MicSettings,
}

impl AppConfig {
    pub fn path() -> PathBuf {
        app_config_dir().join("settings.json")
    }

    /// Loads the config; a missing or corrupt file yields defaults.
    pub fn load() -> Self {
        let mut cfg: AppConfig =
            fs::read_to_string(Self::path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        cfg.dsp.sanitize();
        cfg
    }

    pub fn save(&self) -> io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        write_atomic(&Self::path(), &json)
    }
}
