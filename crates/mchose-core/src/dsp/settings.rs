//! User-facing DSP settings, presets and the mapping to filter-graph controls.

use serde::{Deserialize, Serialize};

use super::response::{self, Biquad};

/// Centre frequencies of the 10-band graphic EQ (same layout as Xear on Windows).
pub const EQ_FREQS: [f32; 10] = [31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
/// Quality factor of each band (≈ one octave wide).
pub const EQ_Q: f32 = 1.41;
pub const EQ_MIN_DB: f32 = -12.0;
pub const EQ_MAX_DB: f32 = 12.0;
pub const PREAMP_MIN_DB: f32 = -15.0;
pub const PREAMP_MAX_DB: f32 = 6.0;

pub const BASS_MAX_DB: f32 = 12.0;
pub const BASS_FREQ_RANGE: (f32, f32) = (40.0, 250.0);
pub const CLARITY_MAX_DB: f32 = 10.0;
pub const CLARITY_FREQ_RANGE: (f32, f32) = (2500.0, 12000.0);
/// Shelf slope ("Q" of PipeWire's shelf filters behaves as the RBJ shelf slope S).
pub const SHELF_SLOPE: f32 = 0.9;

/// Crossfeed: low-passed, slightly delayed copy of each side fed into the other ear.
pub const CROSSFEED_LOWPASS_HZ: f32 = 700.0;
pub const CROSSFEED_DELAY_S: f32 = 0.0003;
pub const CROSSFEED_MAX_GAIN: f32 = 0.45;

pub fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub fn lin_to_db(lin: f32) -> f32 {
    20.0 * lin.max(1e-9).log10()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BassSettings {
    pub enabled: bool,
    pub gain_db: f32,
    pub freq: f32,
}

impl Default for BassSettings {
    fn default() -> Self {
        Self { enabled: false, gain_db: 6.0, freq: 100.0 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaritySettings {
    pub enabled: bool,
    pub gain_db: f32,
    pub freq: f32,
}

impl Default for ClaritySettings {
    fn default() -> Self {
        Self { enabled: false, gain_db: 4.0, freq: 6000.0 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CrossfeedSettings {
    pub enabled: bool,
    /// 0.0 – 1.0
    pub amount: f32,
}

impl Default for CrossfeedSettings {
    fn default() -> Self {
        Self { enabled: false, amount: 0.5 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub gains: [f32; 10],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DspSettings {
    /// Master switch: when off the filter is removed from the audio path.
    pub enabled: bool,
    pub eq_enabled: bool,
    pub eq_gains: [f32; 10],
    /// Id of the selected preset, or "custom".
    pub preset: String,
    /// Automatically lower the input level to avoid clipping when boosting.
    pub auto_preamp: bool,
    pub preamp_db: f32,
    /// -1.0 (left) … +1.0 (right)
    pub balance: f32,
    pub bass: BassSettings,
    pub clarity: ClaritySettings,
    pub crossfeed: CrossfeedSettings,
    pub user_presets: Vec<Preset>,
}

impl Default for DspSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            eq_enabled: true,
            eq_gains: [0.0; 10],
            preset: "flat".into(),
            auto_preamp: true,
            preamp_db: 0.0,
            balance: 0.0,
            bass: BassSettings::default(),
            clarity: ClaritySettings::default(),
            crossfeed: CrossfeedSettings::default(),
            user_presets: Vec::new(),
        }
    }
}

macro_rules! preset {
    ($id:literal, $name:literal, [$($g:expr),* $(,)?]) => {
        Preset { id: $id.into(), name: $name.into(), gains: [$($g as f32),*] }
    };
}

/// Built-in presets.
pub fn builtin_presets() -> Vec<Preset> {
    vec![
        preset!("flat", "Neutro", [0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        preset!("fps", "FPS · Passos", [-4, -3, -2, -1, 0, 2, 4, 5, 3, 1]),
        preset!("bass", "Graves", [7, 6, 4, 2, 0, -1, -1, 0, 0, 0]),
        preset!("music", "Música", [4, 3, 1, 0, -1, -1, 0, 2, 3, 3]),
        preset!("vocal", "Voz · Podcast", [-3, -2, -1, 1, 3, 4, 3, 1, 0, -1]),
        preset!("movie", "Filmes", [5, 4, 2, 0, -1, 1, 2, 3, 3, 2]),
        preset!("rpg", "RPG · Imersivo", [4, 3, 2, 1, 0, 0, 1, 2, 2, 1]),
        preset!("electronic", "Eletrônica", [6, 5, 2, 0, -2, 0, 1, 2, 4, 4]),
        preset!("acoustic", "Acústico", [3, 2, 1, 1, 2, 2, 2, 2, 1, 1]),
        preset!("treble", "Agudos", [0, 0, 0, 0, 0, 1, 3, 5, 6, 6]),
    ]
}

impl DspSettings {
    pub fn all_presets(&self) -> Vec<Preset> {
        let mut v = builtin_presets();
        v.extend(self.user_presets.iter().cloned());
        v
    }

    pub fn find_preset(&self, id: &str) -> Option<Preset> {
        self.all_presets().into_iter().find(|p| p.id == id)
    }

    pub fn apply_preset(&mut self, id: &str) -> bool {
        match self.find_preset(id) {
            Some(p) => {
                self.eq_gains = p.gains;
                self.preset = p.id;
                true
            }
            None => false,
        }
    }

    /// Marks the EQ as "custom" unless the gains still match the selected preset.
    pub fn refresh_preset_match(&mut self) {
        let matches = self
            .find_preset(&self.preset)
            .is_some_and(|p| p.gains.iter().zip(&self.eq_gains).all(|(a, b)| (a - b).abs() < 0.05));
        if !matches {
            self.preset = self
                .all_presets()
                .into_iter()
                .find(|p| p.gains.iter().zip(&self.eq_gains).all(|(a, b)| (a - b).abs() < 0.05))
                .map(|p| p.id)
                .unwrap_or_else(|| "custom".into());
        }
    }

    pub fn save_user_preset(&mut self, name: &str) -> String {
        let base: String =
            name.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
        let id = format!("user-{}", base.trim_matches('-'));
        self.user_presets.retain(|p| p.id != id);
        self.user_presets.push(Preset { id: id.clone(), name: name.trim().to_string(), gains: self.eq_gains });
        self.preset = id.clone();
        id
    }

    pub fn delete_user_preset(&mut self, id: &str) {
        self.user_presets.retain(|p| p.id != id);
        if self.preset == id {
            self.preset = "custom".into();
        }
    }

    /// Clamp every value into its legal range (guards against hand-edited files).
    pub fn sanitize(&mut self) {
        for g in &mut self.eq_gains {
            *g = if g.is_finite() { g.clamp(EQ_MIN_DB, EQ_MAX_DB) } else { 0.0 };
        }
        self.preamp_db = self.preamp_db.clamp(PREAMP_MIN_DB, PREAMP_MAX_DB);
        self.balance = self.balance.clamp(-1.0, 1.0);
        self.bass.gain_db = self.bass.gain_db.clamp(0.0, BASS_MAX_DB);
        self.bass.freq = self.bass.freq.clamp(BASS_FREQ_RANGE.0, BASS_FREQ_RANGE.1);
        self.clarity.gain_db = self.clarity.gain_db.clamp(0.0, CLARITY_MAX_DB);
        self.clarity.freq = self.clarity.freq.clamp(CLARITY_FREQ_RANGE.0, CLARITY_FREQ_RANGE.1);
        self.crossfeed.amount = self.crossfeed.amount.clamp(0.0, 1.0);
    }

    fn eq_gain(&self, band: usize) -> f32 {
        if self.eq_enabled { self.eq_gains[band] } else { 0.0 }
    }

    fn bass_gain(&self) -> f32 {
        if self.bass.enabled { self.bass.gain_db } else { 0.0 }
    }

    fn clarity_gain(&self) -> f32 {
        if self.clarity.enabled { self.clarity.gain_db } else { 0.0 }
    }

    pub fn crossfeed_gain(&self) -> f32 {
        if self.crossfeed.enabled { self.crossfeed.amount * CROSSFEED_MAX_GAIN } else { 0.0 }
    }

    /// All biquads of one channel, in processing order.
    pub fn biquads(&self, sample_rate: f32) -> Vec<Biquad> {
        let mut v: Vec<Biquad> =
            EQ_FREQS.iter().enumerate().map(|(i, &f)| Biquad::peaking(sample_rate, f, EQ_Q, self.eq_gain(i))).collect();
        v.push(Biquad::low_shelf(sample_rate, self.bass.freq, SHELF_SLOPE, self.bass_gain()));
        v.push(Biquad::high_shelf(sample_rate, self.clarity.freq, SHELF_SLOPE, self.clarity_gain()));
        v
    }

    /// Magnitude response (dB) of the tone-shaping chain at `freqs`, without preamp.
    pub fn response_db(&self, freqs: &[f32], sample_rate: f32) -> Vec<f32> {
        let filters = self.biquads(sample_rate);
        let xf = self.crossfeed_gain();
        let lp = Biquad::low_pass(sample_rate, CROSSFEED_LOWPASS_HZ, 0.5);
        freqs
            .iter()
            .map(|&f| {
                let mut db: f32 = filters.iter().map(|b| b.magnitude_db(f, sample_rate)).sum();
                if xf > 0.0 {
                    // Worst case (mono content): direct + low-passed cross path add up.
                    let cross = xf * db_to_lin(lp.magnitude_db(f, sample_rate));
                    db += lin_to_db(1.0 + cross);
                }
                db
            })
            .collect()
    }

    /// Peak gain of the chain; used by the automatic pre-amp.
    pub fn peak_gain_db(&self, sample_rate: f32) -> f32 {
        let freqs = response::log_space(20.0, 20000.0, 256);
        self.response_db(&freqs, sample_rate).into_iter().fold(f32::MIN, f32::max)
    }

    /// Pre-amp that will actually be applied.
    pub fn effective_preamp_db(&self, sample_rate: f32) -> f32 {
        if self.auto_preamp { -self.peak_gain_db(sample_rate).max(0.0) } else { self.preamp_db }
    }

    /// Flattened `(control, value)` list for every node of the filter graph.
    pub fn controls(&self, sample_rate: f32) -> Vec<(String, f32)> {
        let pre = db_to_lin(self.effective_preamp_db(sample_rate));
        let (bl, br) = if self.balance >= 0.0 { (1.0 - self.balance, 1.0) } else { (1.0, 1.0 + self.balance) };
        let xf = self.crossfeed_gain();
        let mut c = Vec::with_capacity(64);
        for ch in ["L", "R"] {
            let bal = if ch == "L" { bl } else { br };
            c.push((format!("pre{ch}:Mult"), pre * bal));
            for (i, &f) in EQ_FREQS.iter().enumerate() {
                c.push((format!("eq{ch}{i}:Freq"), f));
                c.push((format!("eq{ch}{i}:Q"), EQ_Q));
                c.push((format!("eq{ch}{i}:Gain"), self.eq_gain(i)));
            }
            c.push((format!("bass{ch}:Freq"), self.bass.freq));
            c.push((format!("bass{ch}:Q"), SHELF_SLOPE));
            c.push((format!("bass{ch}:Gain"), self.bass_gain()));
            c.push((format!("clar{ch}:Freq"), self.clarity.freq));
            c.push((format!("clar{ch}:Q"), SHELF_SLOPE));
            c.push((format!("clar{ch}:Gain"), self.clarity_gain()));
            c.push((format!("mix{ch}:Gain 1"), 1.0));
            c.push((format!("mix{ch}:Gain 2"), xf));
        }
        c
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MicSettings {
    /// Creates the noise-suppressed virtual microphone.
    pub enabled: bool,
    pub noise_suppression: bool,
    pub gain_control: bool,
    pub high_pass: bool,
    pub transient_suppression: bool,
    /// Make the processed microphone the system default input.
    pub set_default: bool,
    /// Default input before we replaced it (restored when `set_default` is turned off).
    pub previous_default: Option<String>,
}

impl Default for MicSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            noise_suppression: true,
            gain_control: true,
            high_pass: true,
            transient_suppression: true,
            set_default: false,
            previous_default: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_settings_are_transparent() {
        let s = DspSettings::default();
        let freqs = response::log_space(20.0, 20000.0, 64);
        assert!(s.response_db(&freqs, 48000.0).iter().all(|d| d.abs() < 1e-3));
        assert!(s.effective_preamp_db(48000.0).abs() < 1e-3);
    }

    #[test]
    fn auto_preamp_compensates_boost() {
        let mut s = DspSettings::default();
        s.apply_preset("bass");
        let peak = s.peak_gain_db(48000.0);
        assert!(peak > 5.0, "peak {peak}");
        assert!((s.effective_preamp_db(48000.0) + peak).abs() < 1e-4);
    }

    #[test]
    fn preset_matching() {
        let mut s = DspSettings::default();
        s.apply_preset("fps");
        s.eq_gains[3] += 2.0;
        s.refresh_preset_match();
        assert_eq!(s.preset, "custom");
        s.eq_gains = builtin_presets()[2].gains;
        s.refresh_preset_match();
        assert_eq!(s.preset, "bass");
    }

    #[test]
    fn user_presets_roundtrip() {
        let mut s = DspSettings::default();
        s.eq_gains[0] = 3.0;
        let id = s.save_user_preset("Meu Preset!");
        assert_eq!(id, "user-meu-preset");
        assert_eq!(s.find_preset(&id).unwrap().gains[0], 3.0);
        s.delete_user_preset(&id);
        assert!(s.find_preset(&id).is_none());
    }

    #[test]
    fn controls_cover_both_channels() {
        let s = DspSettings::default();
        let c = s.controls(48000.0);
        assert!(c.iter().any(|(k, _)| k == "eqL9:Gain"));
        assert!(c.iter().any(|(k, _)| k == "mixR:Gain 2"));
        assert_eq!(c.len(), 2 * (1 + 30 + 6 + 2));
    }
}
