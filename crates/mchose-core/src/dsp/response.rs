//! Biquad coefficient design (RBJ cookbook, as used by PipeWire's builtin filters)
//! and frequency-response evaluation. Used to draw the EQ curve and to compute the
//! automatic pre-amp.

use std::f64::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Biquad {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl Biquad {
    pub const IDENTITY: Biquad = Biquad { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };

    fn normalized(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
    }

    fn w0(fs: f32, f: f32) -> f64 {
        let nyq = fs as f64 / 2.0;
        PI * (f as f64 / nyq).clamp(1e-6, 0.999_999)
    }

    pub fn peaking(fs: f32, f: f32, q: f32, gain_db: f32) -> Self {
        if gain_db.abs() < 1e-6 {
            return Self::IDENTITY;
        }
        let a = 10f64.powf(gain_db as f64 / 40.0);
        let w0 = Self::w0(fs, f);
        let alpha = w0.sin() / (2.0 * q as f64);
        let cos = w0.cos();
        Self::normalized(1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a)
    }

    /// `slope` is the RBJ shelf slope S (PipeWire exposes it as the "Q" control).
    pub fn low_shelf(fs: f32, f: f32, slope: f32, gain_db: f32) -> Self {
        if gain_db.abs() < 1e-6 {
            return Self::IDENTITY;
        }
        let a = 10f64.powf(gain_db as f64 / 40.0);
        let w0 = Self::w0(fs, f);
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / 2.0 * ((a + 1.0 / a) * (1.0 / slope as f64 - 1.0) + 2.0).sqrt();
        let k = 2.0 * a.sqrt() * alpha;
        Self::normalized(
            a * ((a + 1.0) - (a - 1.0) * cos + k),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
            a * ((a + 1.0) - (a - 1.0) * cos - k),
            (a + 1.0) + (a - 1.0) * cos + k,
            -2.0 * ((a - 1.0) + (a + 1.0) * cos),
            (a + 1.0) + (a - 1.0) * cos - k,
        )
    }

    pub fn high_shelf(fs: f32, f: f32, slope: f32, gain_db: f32) -> Self {
        if gain_db.abs() < 1e-6 {
            return Self::IDENTITY;
        }
        let a = 10f64.powf(gain_db as f64 / 40.0);
        let w0 = Self::w0(fs, f);
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / 2.0 * ((a + 1.0 / a) * (1.0 / slope as f64 - 1.0) + 2.0).sqrt();
        let k = 2.0 * a.sqrt() * alpha;
        Self::normalized(
            a * ((a + 1.0) + (a - 1.0) * cos + k),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
            a * ((a + 1.0) + (a - 1.0) * cos - k),
            (a + 1.0) - (a - 1.0) * cos + k,
            2.0 * ((a - 1.0) - (a + 1.0) * cos),
            (a + 1.0) - (a - 1.0) * cos - k,
        )
    }

    pub fn low_pass(fs: f32, f: f32, q: f32) -> Self {
        let w0 = Self::w0(fs, f);
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q as f64);
        Self::normalized((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
    }

    /// |H(e^jw)| in dB at frequency `f`.
    pub fn magnitude_db(&self, f: f32, fs: f32) -> f32 {
        let w = 2.0 * PI * f as f64 / fs as f64;
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = -(self.b1 * s1 + self.b2 * s2);
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = -(self.a1 * s1 + self.a2 * s2);
        let num = nr * nr + ni * ni;
        let den = (dr * dr + di * di).max(1e-30);
        (10.0 * (num / den).max(1e-30).log10()) as f32
    }
}

/// `n` logarithmically spaced points between `lo` and `hi` (inclusive).
pub fn log_space(lo: f32, hi: f32, n: usize) -> Vec<f32> {
    let (l, h) = (lo.ln(), hi.ln());
    (0..n).map(|i| (l + (h - l) * i as f32 / (n.max(2) - 1) as f32).exp()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peaking_hits_gain_at_centre() {
        let b = Biquad::peaking(48000.0, 1000.0, 1.41, 6.0);
        assert!((b.magnitude_db(1000.0, 48000.0) - 6.0).abs() < 0.01);
        assert!(b.magnitude_db(20.0, 48000.0).abs() < 0.1);
    }

    #[test]
    fn shelves_reach_full_gain() {
        let lo = Biquad::low_shelf(48000.0, 100.0, 0.9, 8.0);
        assert!((lo.magnitude_db(20.0, 48000.0) - 8.0).abs() < 0.3);
        assert!(lo.magnitude_db(5000.0, 48000.0).abs() < 0.1);
        let hi = Biquad::high_shelf(48000.0, 6000.0, 0.9, 5.0);
        assert!((hi.magnitude_db(20000.0, 48000.0) - 5.0).abs() < 0.5);
        assert!(hi.magnitude_db(100.0, 48000.0).abs() < 0.1);
    }

    #[test]
    fn lowpass_attenuates() {
        let lp = Biquad::low_pass(48000.0, 700.0, 0.5);
        assert!(lp.magnitude_db(50.0, 48000.0).abs() < 0.2);
        assert!(lp.magnitude_db(10000.0, 48000.0) < -20.0);
    }

    #[test]
    fn log_space_endpoints() {
        let v = log_space(20.0, 20000.0, 5);
        assert!((v[0] - 20.0).abs() < 1e-3 && (v[4] - 20000.0).abs() < 0.5);
    }
}
