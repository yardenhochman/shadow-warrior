//! Offline shout evaluation over WAV files (`shadow-warrior-brain eval-wav ...`, `just eval`).
//!
//! Runs the live code path unchanged: `FrontEnd` (band-pass -> resample to 16 kHz -> 256-sample
//! frames -> features -> adaptive noise floor, earshot VAD) and `ShoutDetector` with the
//! `AudioConfig` from config.yaml (+ `BRAIN_` env, + `--set` overrides). Samples are fed in
//! callback-sized chunks; time is the frame index (the detector never reads the wall clock),
//! so results are deterministic. Stereo files are evaluated as left, right and mix (the live
//! `mix_to_mono`) separately, to compare two microphones recorded at once.

use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::audio_frontend::{FrameFeatures, FrontEnd, FrontEndConfig, VadProbe, FRAME_SECS};
use super::audio_manager::AudioManager;
use super::shout_detector::{FrameVerdict, ShoutDetection, ShoutDetector};
use crate::config::{AudioConfig, Config};

/// Default feed chunk: a typical ALSA/cpal callback period.
pub const DEFAULT_CHUNK: usize = 1024;

// ---------------------------------------------------------------- WAV reader

/// Decoded WAV: interleaved samples in [-1, 1].
#[derive(Debug, Clone)]
pub struct Wav {
    pub rate: u32,
    pub channels: u16,
    pub bits: u16,
    pub samples: Vec<f32>,
}

impl Wav {
    #[cfg(test)]
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// Parse a RIFF/WAVE file: PCM 16/24/32-bit integer or 32-bit float, any rate and channel count.
pub fn parse_wav(b: &[u8]) -> Result<Wav> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        bail!("not a RIFF/WAVE file");
    }
    let mut fmt: Option<(u16, u16, u32, u16)> = None; // (format, channels, rate, bits)
    let mut i = 12;
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let size = u32_at(b, i + 4) as usize;
        let body = i + 8;
        let end = body.saturating_add(size).min(b.len()); // tolerate truncated / streaming sizes
        match id {
            b"fmt " => {
                if end - body < 16 {
                    bail!("short fmt chunk");
                }
                let mut tag = u16_at(b, body);
                if tag == 0xFFFE && end - body >= 26 {
                    tag = u16_at(b, body + 24); // WAVE_FORMAT_EXTENSIBLE sub-format
                }
                fmt = Some((tag, u16_at(b, body + 2), u32_at(b, body + 4), u16_at(b, body + 14)));
            }
            b"data" => {
                let (tag, channels, rate, bits) = fmt.context("data chunk before fmt chunk")?;
                if channels == 0 || rate == 0 {
                    bail!("invalid fmt: {channels} ch, {rate} Hz");
                }
                let data = &b[body..end];
                let samples: Vec<f32> = match (tag, bits) {
                    (1, 16) => data.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect(),
                    (1, 24) => data
                        .chunks_exact(3)
                        .map(|c| (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0)
                        .collect(),
                    (1, 32) => data
                        .chunks_exact(4)
                        .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32 / 2_147_483_648.0)
                        .collect(),
                    (3, 32) => data.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
                    _ => bail!("unsupported WAV encoding: format tag {tag}, {bits} bits"),
                };
                let whole = samples.len() - samples.len() % channels as usize;
                let mut samples = samples;
                samples.truncate(whole);
                return Ok(Wav { rate, channels, bits, samples });
            }
            _ => {}
        }
        i = body + size + (size & 1);
    }
    bail!("no data chunk")
}

pub fn read_wav(path: &Path) -> Result<Wav> {
    let b = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    parse_wav(&b).with_context(|| format!("parsing {}", path.display()))
}

/// 16-bit PCM WAV bytes from interleaved samples (tests and synthetic fixtures).
#[cfg(test)]
pub fn wav_bytes_i16(rate: u32, channels: u16, interleaved: &[f32]) -> Vec<u8> {
    let data_len = (interleaved.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&channels.to_le_bytes());
    v.extend_from_slice(&rate.to_le_bytes());
    v.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    v.extend_from_slice(&(channels * 2).to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in interleaved {
        v.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
    }
    v
}

/// Mono views to evaluate: `mono` for 1 channel; `left`, `right` (`ch<N>` beyond 2) and `mix`
/// (the live `mix_to_mono`) otherwise.
pub fn channel_views(wav: &Wav) -> Vec<(String, Vec<f32>)> {
    let ch = wav.channels as usize;
    if ch == 1 {
        return vec![("mono".into(), wav.samples.clone())];
    }
    let mut out: Vec<(String, Vec<f32>)> = (0..ch)
        .map(|c| {
            let label = match (ch, c) {
                (2, 0) => "left".to_string(),
                (2, 1) => "right".to_string(),
                _ => format!("ch{c}"),
            };
            (label, wav.samples.iter().skip(c).step_by(ch).copied().collect())
        })
        .collect();
    out.push(("mix".into(), AudioManager::mix_to_mono(&wav.samples, wav.channels)));
    out
}

// ---------------------------------------------------------------- evaluation

#[derive(Debug, Clone)]
pub struct EvalOptions {
    /// Samples (device rate, mono) per `FrontEnd::process` call.
    pub chunk: usize,
    /// Simulate our speakers playing (adds `shout_playback_margin_db`).
    pub playback_active: bool,
}

impl Default for EvalOptions {
    fn default() -> Self {
        Self { chunk: DEFAULT_CHUNK, playback_active: false }
    }
}

#[derive(Debug, Clone)]
pub struct FrameRow {
    /// End of the frame, seconds from file start.
    pub t: f32,
    pub features: FrameFeatures,
    pub verdict: FrameVerdict,
    pub detection: Option<ShoutDetection>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DetectionRow {
    /// Seconds from file start at which `ShoutDetected` would be published.
    pub t: f32,
    pub intensity: f32,
    pub duration_sec: f32,
    pub level_db: f32,
    pub db_above_floor: f32,
}

/// One second of frames. Spectral columns are means over the loudest quarter of the second's
/// frames (by band level), so they describe the voice, not the gaps.
#[derive(Debug, Clone, Serialize)]
pub struct SecondRow {
    pub second: u32,
    pub frames: u32,
    pub band_db_max: f32,
    pub floor_db: f32,
    pub margin_max_db: f32,
    pub vad_mean: f32,
    pub gate_frac: f32,
    /// Frames at or above the required margin.
    pub loud_frames: u32,
    pub centroid_hz: f32,
    pub tilt_db_per_khz: f32,
    pub flatness: f32,
    /// Median F0 of voiced frames; None when none were voiced.
    pub f0_hz: Option<f32>,
    pub shouty_frac: f32,
    pub detections: u32,
}

/// Among frames loud enough (above margin), why they were not shouty.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Blockers {
    pub gate_closed: u32,
    pub not_voiced: u32,
    pub centroid_low: u32,
    pub tilt_low: u32,
    pub flatness_high: u32,
    /// Gate open and loud, but fewer than `shout_min_features` checks passed.
    pub too_few_features: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub frames: u32,
    pub loud_frames: u32,
    pub shouty_frames: u32,
    pub detections: u32,
    pub margin_max_db: f32,
    pub margin_p95_db: f32,
    pub floor_min_db: f32,
    pub floor_max_db: f32,
    /// Counted over loud frames.
    pub blockers: Blockers,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelReport {
    pub label: String,
    pub sample_rate: u32,
    pub duration_sec: f32,
    pub peak_dbfs: f32,
    pub clipped: u32,
    pub seconds: Vec<SecondRow>,
    pub detections: Vec<DetectionRow>,
    pub summary: Summary,
    #[serde(skip)]
    pub frames: Vec<FrameRow>,
}

/// Run the live front-end + detector over one mono channel at `rate`.
pub fn evaluate(
    mono: &[f32],
    rate: u32,
    cfg: &AudioConfig,
    vad: Box<dyn VadProbe>,
    opts: &EvalOptions,
    label: &str,
) -> ChannelReport {
    let mut fe = FrontEnd::new(rate, FrontEndConfig::default(), vad);
    fe.set_playback_active(opts.playback_active);
    let mut det = ShoutDetector::new();
    let mut frames = Vec::new();
    for chunk in mono.chunks(opts.chunk.max(1)) {
        for (_, f) in fe.process(chunk) {
            let (d, v) = det.push_verbose(&f, cfg);
            frames.push(FrameRow { t: (f.index + 1) as f32 * FRAME_SECS, features: f, verdict: v, detection: d });
        }
    }
    let peak = mono.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let clipped = mono.iter().filter(|s| s.abs() >= 32767.0 / 32768.0).count() as u32;
    ChannelReport {
        label: label.to_string(),
        sample_rate: rate,
        duration_sec: mono.len() as f32 / rate as f32,
        peak_dbfs: 20.0 * peak.max(1.0 / 32768.0).log10(),
        clipped,
        seconds: per_second(&frames),
        detections: frames
            .iter()
            .filter_map(|r| {
                r.detection.as_ref().map(|d| DetectionRow {
                    t: r.t,
                    intensity: d.intensity,
                    duration_sec: d.duration_sec,
                    level_db: d.level_db,
                    db_above_floor: r.features.db_above_floor,
                })
            })
            .collect(),
        summary: summarise(&frames),
        frames,
    }
}

fn mean(it: impl Iterator<Item = f32>) -> f32 {
    let (s, n) = it.fold((0.0, 0u32), |(s, n), x| (s + x, n + 1));
    if n == 0 { f32::NAN } else { s / n as f32 }
}

fn percentile(mut v: Vec<f32>, p: f32) -> f32 {
    if v.is_empty() {
        return f32::NAN;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    v[((v.len() - 1) as f32 * p).round() as usize]
}

fn per_second(frames: &[FrameRow]) -> Vec<SecondRow> {
    let mut out = Vec::new();
    let mut start = 0;
    while start < frames.len() {
        let sec = (frames[start].features.index as f32 * FRAME_SECS).floor() as u32;
        let end = frames[start..]
            .iter()
            .position(|r| (r.features.index as f32 * FRAME_SECS).floor() as u32 != sec)
            .map_or(frames.len(), |p| start + p);
        let fr = &frames[start..end];
        let n = fr.len() as f32;
        let mut by_level: Vec<&FrameRow> = fr.iter().collect();
        by_level.sort_by(|a, b| b.features.band_db.total_cmp(&a.features.band_db));
        let top = &by_level[..(fr.len() / 4).max(1)];
        let voiced_f0: Vec<f32> = fr.iter().filter(|r| r.verdict.voiced).map(|r| r.features.pitch_hz).collect();
        out.push(SecondRow {
            second: sec,
            frames: fr.len() as u32,
            band_db_max: fr.iter().map(|r| r.features.band_db).fold(f32::MIN, f32::max),
            floor_db: fr.last().unwrap().features.noise_floor_db,
            margin_max_db: fr.iter().map(|r| r.features.db_above_floor).fold(f32::MIN, f32::max),
            vad_mean: mean(fr.iter().map(|r| r.features.vad_prob)),
            gate_frac: fr.iter().filter(|r| r.verdict.gate_open).count() as f32 / n,
            loud_frames: fr.iter().filter(|r| r.verdict.above_margin).count() as u32,
            centroid_hz: mean(top.iter().map(|r| r.features.spectral_centroid_hz)),
            tilt_db_per_khz: mean(top.iter().map(|r| r.features.spectral_tilt_db_per_khz)),
            flatness: mean(top.iter().map(|r| r.features.spectral_flatness)),
            f0_hz: (!voiced_f0.is_empty()).then(|| percentile(voiced_f0, 0.5)),
            shouty_frac: fr.iter().filter(|r| r.verdict.shouty).count() as f32 / n,
            detections: fr.iter().filter(|r| r.detection.is_some()).count() as u32,
        });
        start = end;
    }
    out
}

fn summarise(frames: &[FrameRow]) -> Summary {
    let mut b = Blockers::default();
    for r in frames.iter().filter(|r| r.verdict.above_margin && !r.verdict.shouty) {
        let v = &r.verdict;
        b.gate_closed += !v.gate_open as u32;
        b.not_voiced += !v.voiced as u32;
        b.centroid_low += !v.centroid_ok as u32;
        b.tilt_low += !v.tilt_ok as u32;
        b.flatness_high += !v.flatness_ok as u32;
        b.too_few_features += v.gate_open as u32;
    }
    let floors = frames.iter().map(|r| r.features.noise_floor_db);
    Summary {
        frames: frames.len() as u32,
        loud_frames: frames.iter().filter(|r| r.verdict.above_margin).count() as u32,
        shouty_frames: frames.iter().filter(|r| r.verdict.shouty).count() as u32,
        detections: frames.iter().filter(|r| r.detection.is_some()).count() as u32,
        margin_max_db: frames.iter().map(|r| r.features.db_above_floor).fold(f32::NAN, f32::max),
        margin_p95_db: percentile(frames.iter().map(|r| r.features.db_above_floor).collect(), 0.95),
        floor_min_db: floors.clone().fold(f32::NAN, f32::min),
        floor_max_db: floors.fold(f32::NAN, f32::max),
        blockers: b,
    }
}

// ---------------------------------------------------------------- output

fn opt(v: Option<f32>, prec: usize) -> String {
    v.map_or("-".into(), |x| format!("{x:.prec$}"))
}

pub fn format_report(path: &str, wav: &Wav, r: &ChannelReport) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "== {path} [{}] {:.1}s {} Hz {}ch {}-bit  peak {:.1} dBFS  clipped {}",
        r.label, r.duration_sec, wav.rate, wav.channels, wav.bits, r.peak_dbfs, r.clipped
    );
    let _ = writeln!(
        s,
        "  sec  maxdB  floor  margin  vad  gate%  loud  centroid   tilt  flat   f0  shouty%  det"
    );
    for x in &r.seconds {
        let _ = writeln!(
            s,
            "  {:>3}  {:>5.1}  {:>5.1}  {:>6.1}  {:>3.2}  {:>5.0}  {:>4}  {:>8.0}  {:>5.1}  {:>4.2}  {:>3}  {:>7.0}  {:>3}",
            x.second,
            x.band_db_max,
            x.floor_db,
            x.margin_max_db,
            x.vad_mean,
            x.gate_frac * 100.0,
            x.loud_frames,
            x.centroid_hz,
            x.tilt_db_per_khz,
            x.flatness,
            opt(x.f0_hz, 0),
            x.shouty_frac * 100.0,
            x.detections
        );
    }
    for d in &r.detections {
        let _ = writeln!(
            s,
            "  SHOUT t={:.2}s intensity={:.2} duration={:.2}s level={:.1} dB margin={:.1} dB",
            d.t, d.intensity, d.duration_sec, d.level_db, d.db_above_floor
        );
    }
    let m = &r.summary;
    let b = &m.blockers;
    let _ = writeln!(
        s,
        "  SUMMARY {path} [{}]: detections={} shouty={}/{} loud={} margin max {:.1} p95 {:.1} dB, floor {:.1}..{:.1} dB",
        r.label, m.detections, m.shouty_frames, m.frames, m.loud_frames, m.margin_max_db, m.margin_p95_db,
        m.floor_min_db, m.floor_max_db
    );
    let _ = writeln!(
        s,
        "    loud-but-not-shouty frames failing: gate={} voiced={} centroid={} tilt={} flatness={} (gate open, <min features: {})",
        b.gate_closed, b.not_voiced, b.centroid_low, b.tilt_low, b.flatness_high, b.too_few_features
    );
    s
}

const CSV_HEADER: &str = "file,channel,t,index,band_db,noise_floor_db,db_above_floor,vad_prob,gate_open,\
pitch_hz,pitch_conf,centroid_hz,tilt_db_per_khz,flatness,margin_db,above_margin,voiced,centroid_ok,\
tilt_ok,flatness_ok,features_passed,shouty,detection_intensity\n";

fn csv_rows(out: &mut String, path: &str, r: &ChannelReport) {
    for row in &r.frames {
        let (f, v) = (&row.features, &row.verdict);
        let _ = writeln!(
            out,
            "{path},{},{:.3},{},{:.2},{:.2},{:.2},{:.3},{},{:.1},{:.3},{:.0},{:.2},{:.4},{:.1},{},{},{},{},{},{},{},{}",
            r.label, row.t, f.index, f.band_db, f.noise_floor_db, f.db_above_floor, f.vad_prob,
            v.gate_open as u8, f.pitch_hz, f.pitch_conf, f.spectral_centroid_hz, f.spectral_tilt_db_per_khz,
            f.spectral_flatness, v.margin_db, v.above_margin as u8, v.voiced as u8, v.centroid_ok as u8,
            v.tilt_ok as u8, v.flatness_ok as u8, v.features_passed, v.shouty as u8,
            row.detection.as_ref().map_or(String::new(), |d| format!("{:.2}", d.intensity))
        );
    }
}

// ---------------------------------------------------------------- CLI

#[derive(Serialize)]
struct FileReport<'a> {
    file: String,
    sample_rate: u32,
    channels: u16,
    bits: u16,
    reports: &'a [ChannelReport],
}

const USAGE: &str = "usage: shadow-warrior-brain eval-wav [options] <file.wav>...
  --config PATH     config file (default config.yaml; missing = defaults; BRAIN_* env applies)
  --set KEY=VALUE   override a config key, e.g. --set shout_margin_db=12 (audio. prefix implied)
  --json PATH       write the full report as JSON
  --csv PATH        write per-frame features + verdicts as CSV
  --chunk N         samples per front-end call (default 1024)
  --playback        simulate speakers playing (adds shout_playback_margin_db)";

/// Parse a `--set` value as a YAML scalar (number, bool, string).
fn parse_override(kv: &str) -> Result<(String, serde_yaml::Value)> {
    let (k, v) = kv.split_once('=').with_context(|| format!("--set expects KEY=VALUE, got {kv}"))?;
    let key = if k.contains('.') { k.to_string() } else { format!("audio.{k}") };
    Ok((key, serde_yaml::from_str(v).with_context(|| format!("bad value in {kv}"))?))
}

/// `eval-wav` entry point; `args` excludes the program name and subcommand.
pub fn cli(args: &[String]) -> Result<()> {
    let mut config = PathBuf::from("config.yaml");
    let mut overrides = Vec::new();
    let (mut json, mut csv) = (None::<PathBuf>, None::<PathBuf>);
    let mut opts = EvalOptions::default();
    let mut files = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().with_context(|| format!("{a} needs a value\n{USAGE}"));
        match a.as_str() {
            "--config" => config = val()?.into(),
            "--set" => overrides.push(parse_override(val()?)?),
            "--json" => json = Some(val()?.into()),
            "--csv" => csv = Some(val()?.into()),
            "--chunk" => opts.chunk = val()?.parse().context("--chunk")?,
            "--playback" => opts.playback_active = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            s if s.starts_with("--") => bail!("unknown option {s}\n{USAGE}"),
            _ => files.push(PathBuf::from(a)),
        }
    }
    if files.is_empty() {
        bail!("no input files\n{USAGE}");
    }
    let cfg = Config::load_with(&config, &overrides).map_err(|e| anyhow::anyhow!("config: {e}"))?.audio;
    println!(
        "config {}{}: margin {:.1} dB (+{:.1} playback{}), vad>{:.2} hangover {}, f0 {:.0}-{:.0} Hz conf>={:.2}, \
centroid>={:.0} Hz, tilt>={:.1} dB/kHz, flatness<={:.2}, features>={}, min {:.2}s, score>={:.1}, chunk {}",
        config.display(),
        if config.exists() { "" } else { " (missing: defaults)" },
        cfg.shout_margin_db, cfg.shout_playback_margin_db,
        if opts.playback_active { ", ACTIVE" } else { "" },
        cfg.vad_threshold, cfg.vad_hangover_frames, cfg.shout_f0_min_hz, cfg.shout_f0_max_hz,
        cfg.shout_pitch_conf_min, cfg.shout_centroid_min_hz, cfg.shout_tilt_min_db_per_khz,
        cfg.shout_flatness_max, cfg.shout_min_features, cfg.shout_min_duration_sec,
        cfg.shout_score_threshold, opts.chunk
    );

    let mut all = Vec::new();
    let mut csv_out = csv.as_ref().map(|_| CSV_HEADER.to_string());
    let mut totals = Vec::new();
    for path in &files {
        let wav = read_wav(path)?;
        let name = path.display().to_string();
        let reports: Vec<ChannelReport> = channel_views(&wav)
            .into_iter()
            .map(|(label, mono)| {
                evaluate(&mono, wav.rate, &cfg, Box::new(earshot::Detector::default()), &opts, &label)
            })
            .collect();
        for r in &reports {
            print!("{}", format_report(&name, &wav, r));
            if let Some(c) = csv_out.as_mut() {
                csv_rows(c, &name, r);
            }
            totals.push(format!(
                "{name} [{}]: {} detections, max margin {:.1} dB",
                r.label, r.summary.detections, r.summary.margin_max_db
            ));
        }
        all.push((name, wav, reports));
    }
    println!("== totals");
    for t in &totals {
        println!("  {t}");
    }
    if let (Some(p), Some(c)) = (&csv, csv_out) {
        std::fs::write(p, c).with_context(|| format!("writing {}", p.display()))?;
        println!("per-frame CSV: {}", p.display());
    }
    if let Some(p) = &json {
        let doc: Vec<FileReport> = all
            .iter()
            .map(|(n, w, r)| FileReport { file: n.clone(), sample_rate: w.rate, channels: w.channels, bits: w.bits, reports: r })
            .collect();
        std::fs::write(p, serde_json::to_string_pretty(&doc)?).with_context(|| format!("writing {}", p.display()))?;
        println!("JSON report: {}", p.display());
    }
    Ok(())
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::audio_frontend::ConstVad;
    use std::f32::consts::PI;

    fn cfg() -> AudioConfig {
        Config::default().audio
    }

    fn noise(n: usize, amp: f32, seed: u32) -> Vec<f32> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                amp * ((s >> 8) as f32 / (1 << 23) as f32 - 1.0)
            })
            .collect()
    }

    /// Voiced, bright, shout-like source: F0 with slight vibrato, harmonics to 4 kHz with a
    /// gentle roll-off (high centroid, shallow tilt, low flatness).
    fn shout_like(rate: u32, secs: f32, f0: f32, amp: f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        let mut phase = 0.0f32;
        let harmonics = (4000.0 / f0) as usize;
        let norm: f32 = (1..=harmonics).map(|k| 1.0 / (k as f32).sqrt()).sum();
        (0..n)
            .map(|i| {
                let t = i as f32 / rate as f32;
                let f = f0 * (1.0 + 0.02 * (2.0 * PI * 5.0 * t).sin());
                phase += 2.0 * PI * f / rate as f32;
                let v: f32 = (1..=harmonics).map(|k| (k as f32 * phase).sin() / (k as f32).sqrt()).sum();
                amp * v / norm
            })
            .collect()
    }

    /// `quiet` s of noise, then `loud` s of shout over the same noise.
    fn scene(rate: u32, quiet: f32, loud: f32) -> Vec<f32> {
        let nq = (rate as f32 * quiet) as usize;
        let mut sig = noise(nq, 0.002, 1);
        let s = shout_like(rate, loud, 320.0, 0.4);
        let bg = noise(s.len(), 0.002, 2);
        sig.extend(s.iter().zip(bg).map(|(a, b)| a + b));
        sig.extend(noise(rate as usize / 2, 0.002, 3));
        sig
    }

    fn earshot() -> Box<dyn VadProbe> {
        Box::new(earshot::Detector::default())
    }

    #[test]
    fn wav_roundtrip_16bit_stereo() {
        let inter: Vec<f32> = (0..200).map(|i| if i % 2 == 0 { 0.5 } else { -0.25 }).collect();
        let w = parse_wav(&wav_bytes_i16(48000, 2, &inter)).unwrap();
        assert_eq!((w.rate, w.channels, w.bits, w.frames()), (48000, 2, 16, 100));
        let v = channel_views(&w);
        let labels: Vec<&str> = v.iter().map(|x| x.0.as_str()).collect();
        assert_eq!(labels, ["left", "right", "mix"]);
        assert!((v[0].1[0] - 0.5).abs() < 1e-3 && (v[1].1[0] + 0.25).abs() < 1e-3);
        assert!((v[2].1[0] - 0.125).abs() < 1e-3);
        assert!(parse_wav(b"RIFF\0\0\0\0WAVEjunk").is_err());
    }

    #[test]
    fn silence_and_steady_noise_never_fire() {
        // Real earshot VAD, live front-end at 48 kHz.
        let silence = vec![0.0f32; 48000 * 3];
        let r = evaluate(&silence, 48000, &cfg(), earshot(), &EvalOptions::default(), "mono");
        assert_eq!((r.summary.detections, r.summary.shouty_frames), (0, 0));
        assert_eq!(r.seconds.len(), 3);
        let r = evaluate(&noise(48000 * 5, 0.01, 5), 48000, &cfg(), earshot(), &EvalOptions::default(), "mono");
        assert_eq!(r.summary.detections, 0, "{:?}", r.detections);
        assert_eq!(r.summary.shouty_frames, 0);
        assert!(r.summary.margin_max_db < 6.0, "{}", r.summary.margin_max_db);
    }

    #[test]
    #[ignore = "shadow-warrior-aei: noise floor starts at -100 dB on digital silence; noise then fires"]
    fn silence_then_noise_does_not_fire() {
        let mut sig = vec![0.0f32; 16000];
        sig.extend(noise(16000 * 4, 0.01, 5));
        let r = evaluate(&sig, 16000, &cfg(), earshot(), &EvalOptions::default(), "mono");
        assert_eq!(r.summary.detections, 0, "{:?}", r.detections);
    }

    #[test]
    fn loud_harmonic_shout_like_signal_fires() {
        // Shout-like spectrum 40+ dB over the floor. The VAD is stubbed open so the test pins
        // the front-end features + detector, not earshot's opinion of a synthetic signal.
        let sig = scene(48000, 2.0, 1.0);
        let r = evaluate(&sig, 48000, &cfg(), Box::new(ConstVad(0.9)), &EvalOptions::default(), "mono");
        assert!(r.summary.detections >= 1, "{}", format_report("synthetic", &dummy_wav(), &r));
        let d = &r.detections[0];
        assert!(d.t > 2.0 && d.t < 2.3, "fires within 300 ms of onset: {}", d.t);
        assert!(r.summary.margin_max_db > 30.0);
        // Same signal with the VAD gate shut: no detection, and the blocker says why.
        let shut = evaluate(&sig, 48000, &cfg(), Box::new(ConstVad(0.0)), &EvalOptions::default(), "mono");
        assert_eq!(shut.summary.detections, 0);
        assert!(shut.summary.blockers.gate_closed > 30);
    }

    #[test]
    fn chunk_size_does_not_change_the_result() {
        let sig = scene(48000, 1.5, 0.8);
        let a = evaluate(&sig, 48000, &cfg(), Box::new(ConstVad(0.9)), &EvalOptions { chunk: 160, ..Default::default() }, "a");
        let b = evaluate(&sig, 48000, &cfg(), Box::new(ConstVad(0.9)), &EvalOptions { chunk: 4096, ..Default::default() }, "b");
        assert_eq!(a.frames.len(), b.frames.len());
        let t = |r: &ChannelReport| r.detections.iter().map(|d| d.t).collect::<Vec<_>>();
        assert_eq!(t(&a), t(&b));
        assert!(!a.detections.is_empty());
    }

    #[test]
    fn stereo_file_is_evaluated_per_channel() {
        // Left mic hears the shout, right mic only noise.
        let left = scene(48000, 2.0, 1.0);
        let right = noise(left.len(), 0.002, 9);
        let inter: Vec<f32> = left.iter().zip(&right).flat_map(|(l, r)| [*l, *r]).collect();
        let wav = parse_wav(&wav_bytes_i16(48000, 2, &inter)).unwrap();
        let reports: Vec<ChannelReport> = channel_views(&wav)
            .into_iter()
            .map(|(l, m)| evaluate(&m, wav.rate, &cfg(), Box::new(ConstVad(0.9)), &EvalOptions::default(), &l))
            .collect();
        let by = |l: &str| reports.iter().find(|r| r.label == l).unwrap();
        assert!(by("left").summary.detections >= 1);
        assert_eq!(by("right").summary.detections, 0);
        assert!(by("mix").summary.detections >= 1, "mix keeps the margin");
        assert!((by("left").duration_sec - 3.5).abs() < 0.01);
    }

    #[test]
    fn overrides_reach_the_detector() {
        let dir = std::env::temp_dir().join(format!("sw_eval_cfg_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("c.yaml");
        std::fs::write(&p, "audio:\n  shout_margin_db: 20.0\n").unwrap();
        let c = Config::load_with(&p, &[]).unwrap().audio;
        assert_eq!(c.shout_margin_db, 20.0);
        let c = Config::load_with(&p, &[parse_override("shout_margin_db=60").unwrap()]).unwrap().audio;
        assert_eq!(c.shout_margin_db, 60.0);
        let sig = scene(16000, 2.0, 1.0);
        let r = evaluate(&sig, 16000, &c, Box::new(ConstVad(0.9)), &EvalOptions::default(), "mono");
        assert_eq!(r.summary.detections, 0, "60 dB margin silences it");
        std::fs::remove_dir_all(dir).ok();
    }

    fn dummy_wav() -> Wav {
        Wav { rate: 48000, channels: 1, bits: 16, samples: vec![] }
    }
}
