//! Offline shout detector tuning on labelled recordings (`shout_replay` bin, docs/SOUND.md
//! "Tuning on recordings").
//!
//! Runs the live code path unchanged: `FrontEnd` (band-pass, resample, 256-sample frames,
//! features, adaptive noise floor, earshot VAD) and `ShoutDetector`. Stereo files are mixed like
//! `station_manager::on_chunk` does ((l+r)/2). Time is audio time: frame end = (index+1)*16 ms.
//! The front-end does not depend on the detector tunables, so features are extracted once and
//! the detector is re-run per parameter combination (`--sweep`).
//!
//! Labels: JSON `{"cues":[{"kind":"SHOUT","t":12.3,"prompt":"NEAR ..."}], "start_offset_sec":N}`
//! (written by station-fw/tools/dual_record.py). Cue time in audio = `t + start_offset_sec +
//! --offset`. A SHOUT cue is a hit when a detection falls in `[t-0.3, t+2.5]`; a detection in no
//! SHOUT window is a false positive, attributed to the kind of the window it falls in.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use super::audio_frontend::{FrameFeatures, FrontEnd, FrontEndConfig, VadProbe, FRAME_SECS};
use super::adaptive::{self, AmbientTracker};
use super::audio_manager::AudioManager;
use super::shout_detector::{FrameVerdict, ShoutDetector};
use super::wav_eval::{parse_override, read_wav, DEFAULT_CHUNK};
use crate::config::{AudioConfig, Config};

/// Cue window: a detection from `t - WINDOW_PRE` to `t + WINDOW_POST` belongs to the cue.
pub const WINDOW_PRE: f32 = 0.3;
pub const WINDOW_POST: f32 = 2.5;

// ---------------------------------------------------------------- labels

#[derive(Debug, Clone, Deserialize)]
pub struct RawCue {
    pub kind: String,
    pub t: f32,
    #[serde(default)]
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawLabels {
    #[serde(default)]
    cues: Vec<RawCue>,
    #[serde(default)]
    start_offset_sec: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    /// Upper-case kind: SHOUT, CLAP, SOFT, KNOCK, QUIET, MOVE, ...
    pub kind: String,
    /// Cue time in audio seconds (offsets applied).
    pub t: f32,
    pub prompt: Option<String>,
}

impl Cue {
    /// NEAR / MID / FAR when the prompt mentions it.
    pub fn block(&self) -> Option<&'static str> {
        let p = self.prompt.as_deref()?.to_uppercase();
        ["NEAR", "MID", "FAR"].into_iter().find(|b| p.contains(b))
    }
}

/// Parse a cues file; `extra_offset` is the `--offset` argument.
pub fn parse_labels(json: &str, extra_offset: f32) -> Result<Vec<Cue>> {
    let raw: RawLabels = serde_json::from_str(json).context("parsing labels JSON")?;
    let shift = raw.start_offset_sec + extra_offset;
    Ok(raw
        .cues
        .into_iter()
        .map(|c| Cue { kind: c.kind.to_uppercase(), t: c.t + shift, prompt: c.prompt })
        .collect())
}

// ---------------------------------------------------------------- audio -> features

/// Mono samples as the brain sees them: mono as is, stereo (l+r)/2.
pub fn to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    AudioManager::mix_to_mono(samples, channels)
}

/// Run the live front-end over mono audio at `rate`; one `FrameFeatures` per 16 ms frame.
pub fn extract_features(mono: &[f32], rate: u32, chunk: usize, vad: Box<dyn VadProbe>) -> Vec<FrameFeatures> {
    let mut fe = FrontEnd::new(rate, FrontEndConfig::default(), vad);
    let mut out = Vec::new();
    for c in mono.chunks(chunk.max(1)) {
        out.extend(fe.process(c).into_iter().map(|(_, f)| f));
    }
    out
}

/// End of frame `f` in audio seconds (when the detector can first act on it).
pub fn frame_end(f: &FrameFeatures) -> f32 {
    (f.index + 1) as f32 * FRAME_SECS
}

// ---------------------------------------------------------------- detector run

#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    pub t: f32,
    pub intensity: f32,
    pub duration_sec: f32,
    pub level_db: f32,
}

#[derive(Debug, Clone)]
pub struct DetRow {
    pub verdict: FrameVerdict,
    pub score: f32,
    /// Adaptive sensitivity state used for this frame (`adaptive.rs`).
    pub ambient_db: f32,
    pub offset_db: f32,
}

/// Feed features through a fresh `ShoutDetector`.
pub fn run_detector(frames: &[FrameFeatures], cfg: &AudioConfig) -> (Vec<Detection>, Vec<DetRow>) {
    let mut det = ShoutDetector::new();
    let mut ambient = AmbientTracker::new();
    let mut dets = Vec::new();
    let mut rows = Vec::with_capacity(frames.len());
    for f in frames {
        let st = ambient.push(f, cfg);
        let (d, verdict) = det.push_verbose(f, &adaptive::apply(cfg, st.offset_db));
        if let Some(d) = d {
            dets.push(Detection { t: frame_end(f), intensity: d.intensity, duration_sec: d.duration_sec, level_db: d.level_db });
        }
        rows.push(DetRow { verdict, score: det.score(), ambient_db: st.ambient_db, offset_db: st.offset_db });
    }
    (dets, rows)
}

/// Same as `run_detector` but without per-frame rows (sweeps).
fn detections_only(frames: &[FrameFeatures], cfg: &AudioConfig) -> Vec<Detection> {
    run_detector(frames, cfg).0
}

// ---------------------------------------------------------------- evaluation

#[derive(Debug, Clone)]
pub struct CueResult {
    pub cue: Cue,
    /// SHOUT cues only.
    pub hit: bool,
    /// First detection in the window.
    pub det_t: Option<f32>,
    /// Acoustic onset: first frame start with level >= onset_db above the floor in the window.
    pub onset_t: Option<f32>,
}

impl CueResult {
    pub fn delay_from_cue(&self) -> Option<f32> {
        self.det_t.map(|d| d - self.cue.t)
    }
    /// Detection time minus acoustic onset: the responsiveness number.
    pub fn latency(&self) -> Option<f32> {
        Some(self.det_t? - self.onset_t?)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Eval {
    pub cues: Vec<CueResult>,
    /// False positives per kind of the window they fell into ("none" = no window).
    pub fp_by_kind: BTreeMap<String, u32>,
    pub detections: usize,
}

impl Eval {
    pub fn shout_cues(&self) -> impl Iterator<Item = &CueResult> {
        self.cues.iter().filter(|c| c.cue.kind == "SHOUT")
    }
    pub fn n_shout(&self) -> usize {
        self.shout_cues().count()
    }
    pub fn hits(&self) -> usize {
        self.shout_cues().filter(|c| c.hit).count()
    }
    pub fn hit_rate(&self) -> f32 {
        if self.n_shout() == 0 { 0.0 } else { self.hits() as f32 / self.n_shout() as f32 }
    }
    pub fn false_positives(&self) -> u32 {
        self.fp_by_kind.values().sum()
    }
    pub fn median_latency(&self) -> Option<f32> {
        let mut v: Vec<f32> = self.shout_cues().filter_map(CueResult::latency).collect();
        if v.is_empty() {
            return None;
        }
        v.sort_by(|a, b| a.total_cmp(b));
        Some(if v.len() % 2 == 1 { v[v.len() / 2] } else { (v[v.len() / 2 - 1] + v[v.len() / 2]) / 2.0 })
    }
}

fn in_window(cue: &Cue, t: f32) -> bool {
    t >= cue.t - WINDOW_PRE && t <= cue.t + WINDOW_POST
}

/// Match detections against cues. `onset_db`: dB above the detector's noise floor that marks the
/// acoustic onset.
pub fn evaluate_cues(cues: &[Cue], dets: &[Detection], frames: &[FrameFeatures], onset_db: f32) -> Eval {
    let mut ev = Eval { detections: dets.len(), ..Default::default() };
    for cue in cues {
        let first = dets.iter().find(|d| in_window(cue, d.t)).map(|d| d.t);
        let mut r = CueResult { cue: cue.clone(), hit: false, det_t: first, onset_t: None };
        if cue.kind == "SHOUT" {
            r.hit = first.is_some();
            let lo = cue.t - WINDOW_PRE;
            let hi = first.unwrap_or(cue.t + WINDOW_POST);
            r.onset_t = frames
                .iter()
                .find(|f| {
                    let start = f.index as f32 * FRAME_SECS;
                    start >= lo && start <= hi && f.db_above_floor >= onset_db
                })
                .map(|f| f.index as f32 * FRAME_SECS);
        }
        ev.cues.push(r);
    }
    for d in dets {
        if cues.iter().any(|c| c.kind == "SHOUT" && in_window(c, d.t)) {
            continue;
        }
        let kind = cues.iter().find(|c| in_window(c, d.t)).map_or("none".to_string(), |c| c.kind.clone());
        *ev.fp_by_kind.entry(kind).or_default() += 1;
    }
    ev
}

// ---------------------------------------------------------------- sweep

#[derive(Debug, Clone)]
pub struct SweepResult {
    pub params: Vec<(String, String)>,
    pub hits: usize,
    pub n_shout: usize,
    pub false_positives: u32,
    pub median_latency: Option<f32>,
}

impl SweepResult {
    fn hit_rate(&self) -> f32 {
        if self.n_shout == 0 { 0.0 } else { self.hits as f32 / self.n_shout as f32 }
    }
    pub fn label(&self) -> String {
        self.params.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" ")
    }
}

/// Sort by hit rate desc, false positives asc, median latency asc (no latency sorts last).
pub fn rank(results: &mut [SweepResult]) {
    results.sort_by(|a, b| {
        b.hit_rate()
            .total_cmp(&a.hit_rate())
            .then(a.false_positives.cmp(&b.false_positives))
            .then(a.median_latency.unwrap_or(f32::INFINITY).total_cmp(&b.median_latency.unwrap_or(f32::INFINITY)))
    });
}

/// Cartesian product of `key=v1,v2,...` specs.
pub fn grid(specs: &[(String, Vec<String>)]) -> Vec<Vec<(String, String)>> {
    let mut out: Vec<Vec<(String, String)>> = vec![vec![]];
    for (k, vals) in specs {
        out = out
            .into_iter()
            .flat_map(|base| {
                vals.iter().map(move |v| {
                    let mut b = base.clone();
                    b.push((k.clone(), v.clone()));
                    b
                })
            })
            .collect();
    }
    out
}

/// `base` with `key=value` pairs applied (keys as in the config `audio:` block).
pub fn apply_params(base: &AudioConfig, params: &[(String, String)]) -> Result<AudioConfig> {
    let mut v = serde_json::to_value(base)?;
    for (k, s) in params {
        let key = k.strip_prefix("audio.").unwrap_or(k);
        let obj = v.as_object_mut().context("audio config is not an object")?;
        if !obj.contains_key(key) {
            bail!("unknown audio config key {key}");
        }
        let y: serde_yaml::Value = serde_yaml::from_str(s).with_context(|| format!("bad value {k}={s}"))?;
        obj.insert(key.to_string(), serde_json::to_value(y)?);
    }
    serde_json::from_value(v).context("applying parameters")
}

/// Run every combination in parallel (std threads), return them ranked.
pub fn sweep(
    base: &AudioConfig,
    frames: &[FrameFeatures],
    cues: &[Cue],
    specs: &[(String, Vec<String>)],
    onset_db: f32,
) -> Result<Vec<SweepResult>> {
    let combos = grid(specs);
    let cfgs: Vec<AudioConfig> = combos.iter().map(|c| apply_params(base, c)).collect::<Result<_>>()?;
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<SweepResult>> = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).min(combos.len().max(1));
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= combos.len() {
                    break;
                }
                let dets = detections_only(frames, &cfgs[i]);
                let ev = evaluate_cues(cues, &dets, frames, onset_db);
                out.lock().unwrap().push(SweepResult {
                    params: combos[i].clone(),
                    hits: ev.hits(),
                    n_shout: ev.n_shout(),
                    false_positives: ev.false_positives(),
                    median_latency: ev.median_latency(),
                });
            });
        }
    });
    let mut res = out.into_inner().unwrap();
    // Stable base order before the (stable) ranking sort so ties are deterministic.
    res.sort_by_key(|r| combos.iter().position(|c| *c == r.params));
    rank(&mut res);
    Ok(res)
}

// ---------------------------------------------------------------- output

fn ms(v: Option<f32>) -> String {
    v.map_or("-".into(), |x| format!("{:.0}", x * 1000.0))
}

pub fn format_eval(ev: &Eval) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "  cue  kind   t(s)   result  det_t   from_cue(ms)  onset_t  latency(ms)  prompt");
    for (i, r) in ev.cues.iter().enumerate() {
        let result = if r.cue.kind == "SHOUT" {
            if r.hit { "HIT" } else { "MISS" }
        } else if r.det_t.is_some() {
            "FP"
        } else {
            "ok"
        };
        let _ = writeln!(
            s,
            "  {:>3}  {:<6} {:>6.2}  {:<6}  {:>6}  {:>12}  {:>7}  {:>11}  {}",
            i,
            r.cue.kind,
            r.cue.t,
            result,
            r.det_t.map_or("-".into(), |t| format!("{t:.2}")),
            ms(r.delay_from_cue()),
            r.onset_t.map_or("-".into(), |t| format!("{t:.2}")),
            ms(r.latency()),
            r.cue.prompt.as_deref().unwrap_or("")
        );
    }
    let _ = writeln!(
        s,
        "SUMMARY shout hits {}/{} ({:.0}%), detections {}, false positives {}, median latency {} ms",
        ev.hits(),
        ev.n_shout(),
        ev.hit_rate() * 100.0,
        ev.detections,
        ev.false_positives(),
        ms(ev.median_latency())
    );
    for b in ["NEAR", "MID", "FAR"] {
        let (n, h) = ev
            .shout_cues()
            .filter(|c| c.cue.block() == Some(b))
            .fold((0, 0), |(n, h), c| (n + 1, h + c.hit as usize));
        if n > 0 {
            let _ = writeln!(s, "  {b:<5} hits {h}/{n} ({:.0}%)", h as f32 / n as f32 * 100.0);
        }
    }
    if ev.fp_by_kind.is_empty() {
        let _ = writeln!(s, "  false positives by kind: none");
    } else {
        let kinds: Vec<String> = ev.fp_by_kind.iter().map(|(k, n)| format!("{k}={n}")).collect();
        let _ = writeln!(s, "  false positives by kind: {}", kinds.join(" "));
    }
    s
}

pub fn format_sweep(results: &[SweepResult], top: usize) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "  rank  hits  hit%   FP  median_latency(ms)  params");
    for (i, r) in results.iter().take(top).enumerate() {
        let _ = writeln!(
            s,
            "  {:>4}  {:>2}/{:<2} {:>4.0}  {:>3}  {:>18}  {}",
            i + 1,
            r.hits,
            r.n_shout,
            r.hit_rate() * 100.0,
            r.false_positives,
            ms(r.median_latency),
            r.label()
        );
    }
    s
}

const CSV_HEADER: &str = "time,band_db,noise_floor_db,db_above_floor,vad_prob,gate_open,f0_hz,pitch_conf,\
centroid_hz,tilt_db_per_khz,flatness,features_passed,shouty,score,detection,ambient_db,adaptive_offset_db\n";

pub fn features_csv(frames: &[FrameFeatures], rows: &[DetRow], dets: &[Detection]) -> String {
    let mut out = String::from(CSV_HEADER);
    for (f, r) in frames.iter().zip(rows) {
        let t = frame_end(f);
        let det = dets.iter().any(|d| d.t == t) as u8;
        let _ = writeln!(
            out,
            "{:.3},{:.2},{:.2},{:.2},{:.3},{},{:.1},{:.3},{:.0},{:.2},{:.4},{},{},{:.3},{},{:.2},{:.2}",
            t, f.band_db, f.noise_floor_db, f.db_above_floor, f.vad_prob, r.verdict.gate_open as u8,
            f.pitch_hz, f.pitch_conf, f.spectral_centroid_hz, f.spectral_tilt_db_per_khz,
            f.spectral_flatness, r.verdict.features_passed, r.verdict.shouty as u8, r.score, det, r.ambient_db, r.offset_db
        );
    }
    out
}

// ---------------------------------------------------------------- CLI

const USAGE: &str = "usage: shout_replay [options] <file.wav>
  --config PATH       YAML config (default config.yaml; missing = Config::default; BRAIN_* env applies)
  --set KEY=VALUE     override an audio tunable, e.g. --set shout_margin_db=12 (repeatable)
  --labels PATH       cues JSON from dual_record.py ({\"cues\":[{kind,t,prompt}],\"start_offset_sec\":N})
  --offset SEC        extra shift added to every cue time (after start_offset_sec)
  --onset-db DB       acoustic onset = first frame this far above the noise floor (default 10)
  --features-csv PATH write the per-frame feature/verdict/score CSV
  --sweep KEY=v1,v2   grid over values (repeatable); needs --labels
  --top N             rows to print in sweep mode (default 20)
  --chunk N           samples per front-end call (default 1024)
  --gain-db DB        scale the whole recording by DB before analysis (simulates another mic gain / room;
                      clipped to full scale)
Stereo WAVs are mixed (l+r)/2 like the brain does.";

pub fn cli(args: &[String]) -> Result<()> {
    let mut config = PathBuf::from("config.yaml");
    let mut overrides = Vec::new();
    let (mut labels, mut csv, mut wav_path) = (None::<PathBuf>, None::<PathBuf>, None::<PathBuf>);
    let (mut offset, mut onset_db, mut top, mut chunk) = (0.0f32, 10.0f32, 20usize, DEFAULT_CHUNK);
    let mut gain_db = 0.0f32;
    let mut specs: Vec<(String, Vec<String>)> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().with_context(|| format!("{a} needs a value\n{USAGE}"));
        match a.as_str() {
            "--config" => config = val()?.into(),
            "--set" => overrides.push(parse_override(val()?)?),
            "--labels" => labels = Some(val()?.into()),
            "--offset" => offset = val()?.parse().context("--offset")?,
            "--onset-db" => onset_db = val()?.parse().context("--onset-db")?,
            "--features-csv" => csv = Some(val()?.into()),
            "--sweep" => {
                let v = val()?;
                let (k, vs) = v.split_once('=').with_context(|| format!("--sweep expects key=v1,v2, got {v}"))?;
                specs.push((k.to_string(), vs.split(',').map(|s| s.trim().to_string()).collect()));
            }
            "--top" => top = val()?.parse().context("--top")?,
            "--chunk" => chunk = val()?.parse().context("--chunk")?,
            "--gain-db" => gain_db = val()?.parse().context("--gain-db")?,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            s if s.starts_with("--") => bail!("unknown option {s}\n{USAGE}"),
            _ => wav_path = Some(PathBuf::from(a)),
        }
    }
    let wav_path = wav_path.with_context(|| format!("no input WAV\n{USAGE}"))?;
    let cfg = Config::load_with(&config, &overrides).map_err(|e| anyhow::anyhow!("config: {e}"))?.audio;
    let cues = match &labels {
        Some(p) => parse_labels(&std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?, offset)?,
        None => Vec::new(),
    };
    let wav = read_wav(&wav_path)?;
    let mut mono = to_mono(&wav.samples, wav.channels);
    if gain_db != 0.0 {
        let g = 10f32.powf(gain_db / 20.0);
        mono.iter_mut().for_each(|x| *x = (*x * g).clamp(-1.0, 1.0));
    }
    let frames = extract_features(&mono, wav.rate, chunk, Box::new(earshot::Detector::default()));
    println!(
        "{}: {:.1}s {} Hz {}ch, {} frames, {} cues, config {}{}",
        wav_path.display(),
        mono.len() as f32 / wav.rate as f32,
        wav.rate,
        wav.channels,
        frames.len(),
        cues.len(),
        config.display(),
        if config.exists() { "" } else { " (missing: defaults)" }
    );

    if !specs.is_empty() {
        if cues.is_empty() {
            bail!("--sweep needs --labels");
        }
        let res = sweep(&cfg, &frames, &cues, &specs, onset_db)?;
        println!("{} combinations", res.len());
        print!("{}", format_sweep(&res, top));
        return Ok(());
    }

    let (dets, rows) = run_detector(&frames, &cfg);
    if cues.is_empty() {
        for d in &dets {
            println!("  SHOUT t={:.2}s intensity={:.2} duration={:.2}s level={:.1} dB", d.t, d.intensity, d.duration_sec, d.level_db);
        }
        println!("{} detections (no labels)", dets.len());
    } else {
        print!("{}", format_eval(&evaluate_cues(&cues, &dets, &frames, onset_db)));
    }
    if let Some(last) = rows.last() {
        let (lo, hi) = rows.iter().fold((f32::MAX, f32::MIN), |(l, h), r| (l.min(r.offset_db), h.max(r.offset_db)));
        println!(
            "adaptive {}: gain {gain_db:+.0} dB, final ambient {:.1} dB, offset final {:+.1} (range {lo:+.1}..{hi:+.1}) dB",
            if cfg.adaptive_enabled { "on" } else { "off" },
            last.ambient_db,
            last.offset_db
        );
    }
    if let Some(p) = csv {
        std::fs::write(&p, features_csv(&frames, &rows, &dets)).with_context(|| format!("writing {}", p.display()))?;
        println!("per-frame CSV: {}", p.display());
    }
    Ok(())
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::audio_frontend::ConstVad;
    use std::f32::consts::PI;

    const RATE: u32 = 16_000;

    fn noise(n: usize, amp: f32, seed: u32) -> Vec<f32> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                amp * ((s >> 8) as f32 / (1 << 23) as f32 - 1.0)
            })
            .collect()
    }

    fn harmonic(secs: f32, f0: f32, amp: f32) -> Vec<f32> {
        let n = (RATE as f32 * secs) as usize;
        let harmonics = (4000.0 / f0) as usize;
        let norm: f32 = (1..=harmonics).map(|k| 1.0 / (k as f32).sqrt()).sum();
        let mut phase = 0.0f32;
        (0..n)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                phase += 2.0 * PI * f0 * (1.0 + 0.02 * (2.0 * PI * 5.0 * t).sin()) / RATE as f32;
                amp * (1..=harmonics).map(|k| (k as f32 * phase).sin() / (k as f32).sqrt()).sum::<f32>() / norm
            })
            .collect()
    }

    /// Noise with a shout of `secs` starting at `at` seconds.
    fn scene(total: f32, at: f32, secs: f32) -> Vec<f32> {
        let mut sig = noise((RATE as f32 * total) as usize, 0.002, 1);
        for (i, s) in harmonic(secs, 320.0, 0.4).into_iter().enumerate() {
            let j = (at * RATE as f32) as usize + i;
            if j < sig.len() {
                sig[j] += s;
            }
        }
        sig
    }

    fn features(sig: &[f32]) -> Vec<FrameFeatures> {
        extract_features(sig, RATE, 1024, Box::new(ConstVad(0.9)))
    }

    fn cfg() -> AudioConfig {
        Config::default().audio
    }

    fn cue(kind: &str, t: f32) -> Cue {
        Cue { kind: kind.into(), t, prompt: None }
    }

    fn det(t: f32) -> Detection {
        Detection { t, intensity: 0.5, duration_sec: 0.2, level_db: -20.0 }
    }

    #[test]
    fn harmonic_burst_on_noise_is_a_hit_with_small_latency() {
        let frames = features(&scene(8.0, 3.0, 1.0));
        let (dets, rows) = run_detector(&frames, &cfg());
        assert_eq!(rows.len(), frames.len());
        let ev = evaluate_cues(&[cue("SHOUT", 3.2)], &dets, &frames, 10.0);
        assert_eq!(ev.hits(), 1, "{dets:?}");
        assert_eq!(ev.false_positives(), 0);
        let r = &ev.cues[0];
        let onset = r.onset_t.expect("onset");
        assert!((onset - 3.0).abs() < 0.1, "onset {onset}");
        let lat = r.latency().unwrap();
        assert!(lat > 0.0 && lat < 0.5, "latency {lat}");
        assert!(format_eval(&ev).contains("HIT"));
    }

    #[test]
    fn steady_noise_has_no_hit() {
        let frames = features(&noise((RATE * 8) as usize, 0.01, 4));
        let (dets, _) = run_detector(&frames, &cfg());
        assert!(dets.is_empty(), "{dets:?}");
        let ev = evaluate_cues(&[cue("SHOUT", 3.0)], &dets, &frames, 10.0);
        assert_eq!((ev.hits(), ev.n_shout()), (0, 1));
        assert!(ev.cues[0].onset_t.is_none());
    }

    #[test]
    fn stereo_is_mixed_like_the_station_path() {
        let l = scene(2.0, 0.5, 0.5);
        let inter: Vec<f32> = l.iter().flat_map(|&x| [x, -x * 0.5]).collect();
        let mono = to_mono(&inter, 2);
        assert_eq!(mono.len(), l.len());
        assert!((mono[1000] - (l[1000] - l[1000] * 0.5) / 2.0).abs() < 1e-6);
    }

    #[test]
    fn label_matching() {
        let cues = vec![cue("SHOUT", 10.0), cue("SHOUT", 20.0), cue("CLAP", 30.0), cue("QUIET", 40.0)];
        let dets = vec![det(9.8), det(12.4), det(12.6), det(25.0), det(31.0), det(41.0), det(60.0)];
        let ev = evaluate_cues(&cues, &dets, &[], 10.0);
        assert!(ev.cues[0].hit && !ev.cues[1].hit);
        // First detection in the window, boundaries inclusive (t-0.3 .. t+2.5).
        assert_eq!(ev.cues[0].det_t, Some(9.8));
        assert_eq!(ev.fp_by_kind.get("none"), Some(&3)); // 12.6 (just past the window), 25.0, 60.0
        assert_eq!(ev.fp_by_kind.get("CLAP"), Some(&1));
        assert_eq!(ev.fp_by_kind.get("QUIET"), Some(&1));
        assert_eq!(ev.false_positives(), 5);
        assert!(!evaluate_cues(&[cue("SHOUT", 10.0)], &[det(12.6)], &[], 10.0).cues[0].hit);
        assert!(evaluate_cues(&[cue("SHOUT", 10.0)], &[det(12.5)], &[], 10.0).cues[0].hit);
    }

    #[test]
    fn labels_json_offsets_and_blocks() {
        let j = r#"{"cues":[{"kind":"shout","t":1.0,"prompt":"NEAR shout"},{"kind":"CLAP","t":2.0}],"start_offset_sec":0.5}"#;
        let cues = parse_labels(j, 0.25).unwrap();
        assert_eq!(cues[0].kind, "SHOUT");
        assert!((cues[0].t - 1.75).abs() < 1e-6);
        assert_eq!(cues[0].block(), Some("NEAR"));
        assert_eq!(cues[1].block(), None);
        let ev = evaluate_cues(&cues, &[det(2.0)], &[], 10.0);
        assert!(format_eval(&ev).contains("NEAR  hits 1/1"));
        assert!(parse_labels("{}", 0.0).unwrap().is_empty());
    }

    fn sr(label: &str, hits: usize, fp: u32, lat: Option<f32>) -> SweepResult {
        SweepResult { params: vec![("k".into(), label.into())], hits, n_shout: 4, false_positives: fp, median_latency: lat }
    }

    #[test]
    fn sweep_ranking_order() {
        let mut v = vec![
            sr("slow", 4, 0, Some(0.5)),
            sr("fp", 4, 2, Some(0.1)),
            sr("fast", 4, 0, Some(0.2)),
            sr("miss", 3, 0, Some(0.1)),
            sr("nolat", 4, 0, None),
        ];
        rank(&mut v);
        let order: Vec<String> = v.iter().map(|r| r.params[0].1.clone()).collect();
        assert_eq!(order, ["fast", "slow", "nolat", "fp", "miss"]);
    }

    #[test]
    fn grid_and_sweep_end_to_end() {
        let specs = vec![("shout_margin_db".to_string(), vec!["10".to_string(), "80".to_string()]), ("shout_min_features".to_string(), vec!["2".to_string(), "3".to_string()])];
        assert_eq!(grid(&specs).len(), 4);
        let frames = features(&scene(8.0, 3.0, 1.0));
        let cues = [cue("SHOUT", 3.2)];
        let res = sweep(&cfg(), &frames, &cues, &specs, 10.0).unwrap();
        assert_eq!(res.len(), 4);
        // An 80 dB margin can never be met: those combos miss and rank last.
        assert_eq!(res[0].hits, 1);
        assert_eq!(res[3].hits, 0);
        assert!(res[0].label().contains("shout_margin_db=10"));
        assert!(apply_params(&cfg(), &[("nope".into(), "1".into())]).is_err());
        assert_eq!(apply_params(&cfg(), &[("audio.shout_margin_db".into(), "7.5".into())]).unwrap().shout_margin_db, 7.5);
    }

    #[test]
    fn features_csv_has_a_row_per_frame() {
        let frames = features(&scene(3.0, 1.0, 0.8));
        let (dets, rows) = run_detector(&frames, &cfg());
        let csv = features_csv(&frames, &rows, &dets);
        assert_eq!(csv.lines().count(), frames.len() + 1);
        assert!(csv.starts_with("time,band_db,"));
        assert!(csv.lines().skip(1).any(|l| l.split(',').nth(14) == Some("1")), "a detection row");
    }
}
