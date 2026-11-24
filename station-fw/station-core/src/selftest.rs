//! Power-on self test: pure verdict logic for the two mics and the radar.
//!
//! The firmware collects raw numbers (a 1 s probe of the I2S stream, the radar frame count over a
//! window) and these functions turn them into a level (`Ok`/`Warn`/`Fail`) plus a one-line hint
//! about the likely wiring fault. No hardware access, so it runs under `cargo test`.

use crate::level::{abs_to_dbfs, mean_square_to_dbfs, slot_to_sample};

/// Result severity, ordered so that `max` gives the worst.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Ok,
    Warn,
    Fail,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Ok => "ok",
            Level::Warn => "WARN",
            Level::Fail => "FAIL",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub level: Level,
    pub msg: &'static str,
}

const fn v(level: Level, msg: &'static str) -> Verdict {
    Verdict { level, msg }
}

/// Statistics of one channel over the probe window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChannelStats {
    pub frames: u32,
    pub min: i32,
    pub max: i32,
    /// RMS of the signal with the DC offset removed.
    pub ac_rms_dbfs: f32,
    pub dc_dbfs: f32,
    pub peak_dbfs: f32,
}

#[derive(Clone, Copy)]
struct Acc {
    n: u32,
    sum: i64,
    sumsq: u64,
    min: i32,
    max: i32,
}

impl Acc {
    const fn new() -> Self {
        Self { n: 0, sum: 0, sumsq: 0, min: i32::MAX, max: i32::MIN }
    }
}

/// Accumulates interleaved little-endian 32-bit I2S slots (L, R, L, R, ...).
/// Keep the window to about 1 s at 48 kHz: the squared sum is a `u64`.
pub struct MicProbe {
    ch: [Acc; 2],
    odd: bool,
}

impl Default for MicProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl MicProbe {
    pub const fn new() -> Self {
        Self { ch: [Acc::new(); 2], odd: false }
    }

    pub fn frames(&self) -> u32 {
        self.ch[0].n.min(self.ch[1].n)
    }

    pub fn push_bytes(&mut self, bytes: &[u8]) {
        for w in bytes.chunks_exact(4) {
            let s = slot_to_sample(i32::from_le_bytes([w[0], w[1], w[2], w[3]]));
            let a = &mut self.ch[self.odd as usize];
            self.odd = !self.odd;
            a.n += 1;
            a.sum += s as i64;
            a.sumsq = a.sumsq.saturating_add((s as i64 * s as i64) as u64);
            a.min = a.min.min(s);
            a.max = a.max.max(s);
        }
    }

    pub fn stats(&self) -> [ChannelStats; 2] {
        self.ch.map(|a| {
            if a.n == 0 {
                return ChannelStats {
                    frames: 0,
                    min: 0,
                    max: 0,
                    ac_rms_dbfs: -120.0,
                    dc_dbfs: -120.0,
                    peak_dbfs: -120.0,
                };
            }
            let n = a.n as f64;
            let mean = a.sum as f64 / n;
            let var = (a.sumsq as f64 / n - mean * mean).max(0.0);
            ChannelStats {
                frames: a.n,
                min: a.min,
                max: a.max,
                ac_rms_dbfs: mean_square_to_dbfs(var),
                dc_dbfs: abs_to_dbfs(libm::fabs(mean) as u32),
                peak_dbfs: abs_to_dbfs(a.min.unsigned_abs().max(a.max.unsigned_abs())),
            }
        })
    }
}

/// One microphone. Typical idle room: AC RMS -65 to -90 dBFS (mic self noise is about -88).
pub fn mic_verdict(s: &ChannelStats) -> Verdict {
    if s.frames == 0 {
        return v(Level::Fail, "no samples read from I2S");
    }
    if s.min == s.max {
        return if s.max == 0 {
            v(Level::Fail, "silent, all zero: check 3V, GND, DOUT, SEL and the BCLK/LRCL wires of this mic")
        } else {
            v(Level::Fail, "stuck at a constant value: DOUT shorted to 3V/GND, or mic unpowered")
        };
    }
    if s.dc_dbfs > -6.0 {
        return v(Level::Fail, "DC railed: DOUT shorted high/low or floating");
    }
    if s.peak_dbfs >= -1.0 {
        return v(Level::Warn, "near full scale: loud sound, or noisy/loose connection");
    }
    if s.dc_dbfs > -40.0 {
        return v(Level::Warn, "large DC offset");
    }
    if s.ac_rms_dbfs < -105.0 {
        return v(Level::Warn, "quieter than the mic's own noise (about -88 dBFS): suspicious");
    }
    if s.ac_rms_dbfs > -35.0 {
        return v(Level::Warn, "very noisy: loud room, or electrical noise on the wires");
    }
    v(Level::Ok, "")
}

/// Level match between the two mics (both usable). `None` when they agree.
pub fn balance_verdict(a: &ChannelStats, b: &ChannelStats) -> Option<Verdict> {
    let d = (a.ac_rms_dbfs - b.ac_rms_dbfs).abs();
    (d > 10.0).then_some(v(Level::Warn, "left/right levels differ by more than 10 dB: one mic is covered, damaged or miswired"))
}

/// Radar numbers over the observation window.
#[derive(Clone, Copy, Debug)]
pub struct RadarStats {
    /// Version read answered at boot.
    pub identified: bool,
    pub frames: u32,
    pub window_ms: u32,
    /// Bytes the parser threw away (not part of a valid frame) since boot.
    pub dropped_bytes: u32,
    pub bad_frames: u32,
}

/// The LD2410 reports about 10 times per second.
pub fn radar_verdict(r: &RadarStats) -> Verdict {
    let hz = r.frames as f32 * 1000.0 / r.window_ms.max(1) as f32;
    if r.frames == 0 {
        return if r.dropped_bytes > 0 || r.bad_frames > 0 {
            v(Level::Fail, "bytes arrive but no valid frames: wrong baud, or floating/noisy TX wire")
        } else {
            v(Level::Fail, "silent: check radar 5V and GND, and that radar TX goes to GPIO16")
        };
    }
    if hz < 5.0 {
        return v(Level::Warn, "report rate is low: weak 5V supply or a loose wire");
    }
    if !r.identified {
        return v(Level::Warn, "reports data but did not answer the version read: radar RX not wired to GPIO17?");
    }
    v(Level::Ok, "")
}

pub fn worst(levels: &[Level]) -> Level {
    levels.iter().copied().max().unwrap_or(Level::Ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// 32-bit slot words with the 24-bit sample MSB aligned.
    fn words(l: impl Fn(usize) -> i32, r: impl Fn(usize) -> i32, n: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for i in 0..n {
            out.extend_from_slice(&(l(i) << 8).to_le_bytes());
            out.extend_from_slice(&(r(i) << 8).to_le_bytes());
        }
        out
    }

    fn noise(i: usize, amp: i32) -> i32 {
        // cheap deterministic pseudo noise
        let x = (i as u32).wrapping_mul(2654435761) >> 8;
        (x as i32 % (2 * amp + 1)) - amp
    }

    fn probe(l: impl Fn(usize) -> i32, r: impl Fn(usize) -> i32) -> [ChannelStats; 2] {
        let mut p = MicProbe::new();
        p.push_bytes(&words(l, r, 4800));
        p.stats()
    }

    #[test]
    fn healthy_mics_pass() {
        let s = probe(|i| noise(i, 2000), |i| noise(i + 7, 2200));
        assert_eq!(mic_verdict(&s[0]).level, Level::Ok, "{:?}", s[0]);
        assert_eq!(mic_verdict(&s[1]).level, Level::Ok);
        assert!(balance_verdict(&s[0], &s[1]).is_none());
    }

    #[test]
    fn all_zero_channel_is_silent_fail() {
        let s = probe(|i| noise(i, 2000), |_| 0);
        assert_eq!(mic_verdict(&s[0]).level, Level::Ok);
        let r = mic_verdict(&s[1]);
        assert_eq!(r.level, Level::Fail);
        assert!(r.msg.starts_with("silent"));
    }

    #[test]
    fn constant_and_railed() {
        let s = probe(|_| 1234, |_| -8_000_000);
        assert!(mic_verdict(&s[0]).msg.starts_with("stuck"));
        assert_eq!(mic_verdict(&s[1]).level, Level::Fail);
        let s = probe(|i| -8_000_000 + noise(i, 50), |i| noise(i, 100));
        assert!(mic_verdict(&s[0]).msg.starts_with("DC railed"));
    }

    #[test]
    fn loud_and_quiet_warn() {
        let s = probe(|i| if i % 2 == 0 { 8_300_000 } else { -8_300_000 }, |i| noise(i, 1));
        assert_eq!(mic_verdict(&s[0]).level, Level::Warn);
        assert!(mic_verdict(&s[0]).msg.starts_with("near full"));
        assert_eq!(mic_verdict(&s[1]).level, Level::Warn);
        assert!(mic_verdict(&s[1]).msg.starts_with("quieter"));
    }

    #[test]
    fn imbalance_warns() {
        let s = probe(|i| noise(i, 200), |i| noise(i, 20_000));
        assert!(balance_verdict(&s[0], &s[1]).is_some());
    }

    #[test]
    fn rms_matches_amplitude() {
        // square wave of +-A has RMS A: 2^20 / 2^23 = -18.06 dBFS
        let s = probe(|i| if i % 2 == 0 { 1 << 20 } else { -(1 << 20) }, |_| 1);
        assert!((s[0].ac_rms_dbfs + 18.06).abs() < 0.1, "{}", s[0].ac_rms_dbfs);
    }

    #[test]
    fn radar_cases() {
        let ok = RadarStats { identified: true, frames: 30, window_ms: 3000, dropped_bytes: 0, bad_frames: 0 };
        assert_eq!(radar_verdict(&ok).level, Level::Ok);
        assert_eq!(radar_verdict(&RadarStats { identified: false, ..ok }).level, Level::Warn);
        assert_eq!(radar_verdict(&RadarStats { frames: 6, ..ok }).level, Level::Warn);
        let none = RadarStats { frames: 0, ..ok };
        assert!(radar_verdict(&none).msg.starts_with("silent"));
        assert!(radar_verdict(&RadarStats { dropped_bytes: 125, ..none }).msg.starts_with("bytes arrive"));
    }

    #[test]
    fn worst_picks_max() {
        assert_eq!(worst(&[Level::Ok, Level::Fail, Level::Warn]), Level::Fail);
        assert_eq!(worst(&[]), Level::Ok);
    }
}
