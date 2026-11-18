//! Event-triggered sound effects and voice lines, played from files (docs/SOUND.md).
//!
//! Mixing: cues play on their own rodio output stream, opened lazily on the first cue and
//! kept for the life of the process, next to the music stream. The OS (PipeWire/ALSA/
//! CoreAudio) mixes the two, which keeps `MusicManager` untouched except for a volume hook.
//! Sharing music's mixer would mean rebuilding it, because music opens (and leaks) a fresh
//! device sink on every `play()`. While a cue plays the music is ducked through
//! `MusicManager::set_duck`.
//!
//! The decision logic lives in [`SoundEngine`], synchronous and driven with explicit
//! `Instant`s over a [`CueOutput`] seam, so tests need no audio device. [`SoundManager`]
//! is the thin async shell: event bus in, ducking out.
use anyhow::{Context, Result};
use rand::seq::SliceRandom;
use rand::Rng;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use crate::config::{CueConfig, SoundConfig};
use crate::core::events::FightEndReason;
use crate::core::{Event, SharedEventBus};
use crate::services::MusicManager;

/// A cue that is playing; `finished` turns true once its audio has ended.
pub trait CueHandle: Send {
    fn finished(&self) -> bool;
}

/// Where cues are played. Real: [`RodioOutput`]. Tests: a fake.
pub trait CueOutput: Send {
    /// Start `path` at linear `gain`. Errors for missing or undecodable files.
    fn play(&mut self, path: &Path, gain: f32) -> Result<Box<dyn CueHandle>>;
}

/// Cue name for an event, or None when the event has no sound.
/// Fight end falls back to `fight_ended` when the reason-specific cue is not configured.
pub fn cue_for_event(event: &Event, cfg: &SoundConfig) -> Option<String> {
    let name = match event {
        Event::SessionStarted { .. } => "session_started".to_string(),
        Event::EnergyFull => "energy_full".to_string(),
        Event::FightStarted { .. } => "fight_started".to_string(),
        Event::FightEnded { reason, .. } => {
            let specific = match reason {
                FightEndReason::TimeElapsed => "fight_ended_time_elapsed",
                FightEndReason::InactivityTimeout => "fight_ended_inactivity",
                FightEndReason::ManualStop => "fight_ended_manual",
            };
            if cfg.cues.contains_key(specific) {
                specific.to_string()
            } else {
                "fight_ended".to_string()
            }
        }
        Event::PunchDetected { power, .. } => punch_cue(*power, cfg).to_string(),
        _ => return None,
    };
    Some(name)
}

/// Punch tier cue: jab below `hook_from`, heavy from `heavy_from`, hook in between.
pub fn punch_cue(power: f32, cfg: &SoundConfig) -> &'static str {
    if power >= cfg.heavy_from {
        "punch_heavy"
    } else if power >= cfg.hook_from {
        "punch_hook"
    } else {
        "punch_jab"
    }
}

fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Music ducking state: duck as soon as a cue plays, restore `hold` after the last one ends.
#[derive(Debug)]
struct Ducker {
    factor: f32,
    hold: Duration,
    ducked: bool,
    release_at: Option<Instant>,
}

impl Ducker {
    fn new(duck_db: f32, hold: Duration) -> Self {
        Self { factor: db_to_gain(duck_db).clamp(0.0, 1.0), hold, ducked: false, release_at: None }
    }

    /// New music factor if it changed (`1.0` = restore).
    fn step(&mut self, now: Instant, active_cues: usize) -> Option<f32> {
        if active_cues > 0 {
            self.release_at = None;
            if !self.ducked {
                self.ducked = true;
                return Some(self.factor);
            }
        } else if self.ducked {
            let at = *self.release_at.get_or_insert(now + self.hold);
            if now >= at {
                self.ducked = false;
                self.release_at = None;
                return Some(1.0);
            }
        }
        None
    }
}

/// Which file a cue plays: random, never the one it played last (when it has a choice).
fn pick_file<'a, R: Rng>(files: &'a [String], last: Option<&str>, rng: &mut R) -> Option<&'a str> {
    let candidates: Vec<&String> = files.iter().filter(|f| Some(f.as_str()) != last).collect();
    let pool: Vec<&String> = if candidates.is_empty() { files.iter().collect() } else { candidates };
    pool.choose(rng).map(|f| f.as_str())
}

pub struct SoundEngine<R: Rng = rand::rngs::StdRng> {
    cfg: SoundConfig,
    output: Box<dyn CueOutput>,
    rng: R,
    active: Vec<Box<dyn CueHandle>>,
    last_played: HashMap<String, Instant>,
    last_file: HashMap<String, String>,
    warned: HashSet<PathBuf>,
    ducker: Ducker,
}

impl SoundEngine {
    pub fn new(cfg: SoundConfig, output: Box<dyn CueOutput>) -> Self {
        Self::with_rng(cfg, output, <rand::rngs::StdRng as rand::SeedableRng>::from_entropy())
    }
}

impl<R: Rng> SoundEngine<R> {
    pub fn with_rng(cfg: SoundConfig, output: Box<dyn CueOutput>, rng: R) -> Self {
        let ducker = Ducker::new(cfg.music_duck_db, Duration::from_millis(cfg.duck_ms));
        Self {
            cfg,
            output,
            rng,
            active: Vec::new(),
            last_played: HashMap::new(),
            last_file: HashMap::new(),
            warned: HashSet::new(),
            ducker,
        }
    }

    pub fn active_cues(&self) -> usize {
        self.active.len()
    }

    /// React to a game event. Returns a new music duck factor when it changed.
    pub fn on_event(&mut self, event: &Event, now: Instant) -> Option<f32> {
        if let Some(cue) = cue_for_event(event, &self.cfg) {
            self.trigger(&cue, now);
        }
        self.tick(now)
    }

    /// Reap finished cues and advance ducking. Call about every 50 ms.
    pub fn tick(&mut self, now: Instant) -> Option<f32> {
        self.active.retain(|h| !h.finished());
        self.ducker.step(now, self.active.len())
    }

    /// Play the cue unless it is unconfigured, too soon after its last play, or the
    /// concurrency cap is reached. Returns whether something started.
    pub fn trigger(&mut self, cue: &str, now: Instant) -> bool {
        let Some(cc) = self.cfg.cues.get(cue) else { return false };
        let CueConfig { files, gain, min_interval_ms } = cc.clone();
        if files.is_empty() {
            return false;
        }
        if let Some(prev) = self.last_played.get(cue) {
            if now.duration_since(*prev) < Duration::from_millis(min_interval_ms) {
                debug!("Cue {cue} dropped: min interval");
                return false;
            }
        }
        self.active.retain(|h| !h.finished());
        if self.active.len() >= self.cfg.max_concurrent {
            debug!("Cue {cue} dropped: {} cues already playing", self.active.len());
            return false;
        }
        let last = self.last_file.get(cue).map(String::as_str);
        let Some(file) = pick_file(&files, last, &mut self.rng).map(str::to_owned) else {
            return false;
        };
        let path = self.cfg.dir.join(&file);
        match self.output.play(&path, gain * self.cfg.volume) {
            Ok(handle) => {
                info!("Cue {cue}: {file}");
                self.active.push(handle);
                self.last_played.insert(cue.to_string(), now);
                self.last_file.insert(cue.to_string(), file);
                true
            }
            Err(e) => {
                // Warn once per file: a missing asset must not flood the log at punch rate.
                if self.warned.insert(path.clone()) {
                    warn!("Cue {cue}: cannot play {}: {e:#}", path.display());
                }
                false
            }
        }
    }
}

/// rodio output on its own stream, opened on the first cue. If no device can be opened the
/// error is returned for every cue (the engine logs it once per file) and nothing blocks.
pub struct RodioOutput {
    mixer: Option<rodio::mixer::Mixer>,
}

impl RodioOutput {
    pub fn new() -> Self {
        Self { mixer: None }
    }

    fn mixer(&mut self) -> Result<&rodio::mixer::Mixer> {
        if self.mixer.is_none() {
            let sink = rodio::DeviceSinkBuilder::from_default_device()
                .and_then(|b| b.open_sink_or_fallback())
                .context("no audio output for sound cues")?;
            let mixer = sink.mixer().clone();
            // Keep the stream alive for the process, as music does.
            Box::leak(Box::new(sink));
            self.mixer = Some(mixer);
        }
        Ok(self.mixer.as_ref().expect("just set"))
    }
}

struct RodioCue(rodio::Player);

impl CueHandle for RodioCue {
    fn finished(&self) -> bool {
        self.0.empty()
    }
}

impl CueOutput for RodioOutput {
    fn play(&mut self, path: &Path, gain: f32) -> Result<Box<dyn CueHandle>> {
        let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        let source = rodio::Decoder::new(BufReader::new(file)).context("decode")?;
        let player = rodio::Player::connect_new(self.mixer()?);
        player.set_volume(gain);
        player.append(source);
        Ok(Box::new(RodioCue(player)))
    }
}

pub struct SoundManager {
    engine: Mutex<SoundEngine>,
    event_bus: SharedEventBus,
    music: Arc<MusicManager>,
}

impl SoundManager {
    pub fn new(cfg: SoundConfig, event_bus: SharedEventBus, music: Arc<MusicManager>) -> Self {
        Self::with_output(cfg, event_bus, music, Box::new(RodioOutput::new()))
    }

    pub fn with_output(
        cfg: SoundConfig,
        event_bus: SharedEventBus,
        music: Arc<MusicManager>,
        output: Box<dyn CueOutput>,
    ) -> Self {
        Self { engine: Mutex::new(SoundEngine::new(cfg, output)), event_bus, music }
    }

    /// Spawn the loop: game events trigger cues, a 50 ms tick reaps them and releases the duck.
    pub fn start(self: Arc<Self>) {
        let mut rx = self.event_bus.subscribe();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(50));
            loop {
                let ev = tokio::select! {
                    ev = rx.recv() => match ev {
                        Ok(ev) => Some(ev),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => None,
                        Err(_) => break,
                    },
                    _ = tick.tick() => None,
                };
                // Lock only after the await: the guard must not live across it.
                let duck = {
                    let mut engine = self.engine.lock().unwrap();
                    match ev {
                        Some(ev) => engine.on_event(&ev, Instant::now()),
                        None => engine.tick(Instant::now()),
                    }
                };
                if let Some(factor) = duck {
                    self.music.set_duck(factor).await;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::events::AccelerationData;
    use chrono::Utc;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    #[derive(Default)]
    struct Fake {
        played: Arc<Mutex<Vec<(PathBuf, f32)>>>,
        done: Arc<Mutex<Vec<Arc<Mutex<bool>>>>>,
        missing: Arc<Mutex<Vec<PathBuf>>>,
    }

    struct FakeHandle(Arc<Mutex<bool>>);
    impl CueHandle for FakeHandle {
        fn finished(&self) -> bool {
            *self.0.lock().unwrap()
        }
    }
    struct FakeOut(Arc<Fake>);
    impl CueOutput for FakeOut {
        fn play(&mut self, path: &Path, gain: f32) -> Result<Box<dyn CueHandle>> {
            if self.0.missing.lock().unwrap().iter().any(|p| p == path) {
                anyhow::bail!("no such file");
            }
            self.0.played.lock().unwrap().push((path.to_path_buf(), gain));
            let flag = Arc::new(Mutex::new(false));
            self.0.done.lock().unwrap().push(flag.clone());
            Ok(Box::new(FakeHandle(flag)))
        }
    }
    impl Fake {
        fn finish_all(&self) {
            self.done.lock().unwrap().iter().for_each(|f| *f.lock().unwrap() = true);
        }
        fn files(&self) -> Vec<String> {
            self.played
                .lock()
                .unwrap()
                .iter()
                .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect()
        }
    }

    fn cue(files: &[&str]) -> CueConfig {
        CueConfig { files: files.iter().map(|s| s.to_string()).collect(), ..Default::default() }
    }

    fn engine(cfg: SoundConfig) -> (SoundEngine<StdRng>, Arc<Fake>) {
        let fake = Arc::new(Fake::default());
        let e = SoundEngine::with_rng(cfg, Box::new(FakeOut(fake.clone())), StdRng::seed_from_u64(7));
        (e, fake)
    }

    fn punch(power: f32) -> Event {
        Event::PunchDetected {
            power,
            acceleration: AccelerationData { x: 0.0, y: 0.0, z: 0.0, magnitude: 0.0, timestamp: Utc::now() },
        }
    }

    fn ended(reason: FightEndReason) -> Event {
        Event::FightEnded { reason, timestamp: Utc::now() }
    }

    #[test]
    fn config_parses_from_yaml_with_defaults() {
        let cfg: SoundConfig = serde_yaml::from_str(
            "enabled: true\ndir: /snd\nmusic_duck_db: -12\ncues:\n  fight_started:\n    files: [a.wav, b.wav]\n    gain: 0.5\n  punch_jab:\n    files: [j.wav]\n    min_interval_ms: 150\n",
        )
        .unwrap();
        assert!(cfg.enabled);
        assert_eq!(cfg.dir, PathBuf::from("/snd"));
        assert_eq!(cfg.music_duck_db, -12.0);
        assert_eq!(cfg.duck_ms, 300, "defaulted");
        assert_eq!(cfg.cues["fight_started"].gain, 0.5);
        assert_eq!(cfg.cues["fight_started"].min_interval_ms, 0);
        assert_eq!(cfg.cues["punch_jab"].gain, 1.0);
        assert!(!SoundConfig::default().enabled, "off by default");
        // A config.yaml without a `sound:` section still parses.
        assert_eq!(serde_yaml::from_str::<SoundConfig>("{}").unwrap(), SoundConfig::default());
    }

    #[test]
    fn punch_tiers_follow_thresholds() {
        let cfg = SoundConfig::default();
        assert_eq!(punch_cue(0.0, &cfg), "punch_jab");
        assert_eq!(punch_cue(44.9, &cfg), "punch_jab");
        assert_eq!(punch_cue(45.0, &cfg), "punch_hook");
        assert_eq!(punch_cue(74.9, &cfg), "punch_hook");
        assert_eq!(punch_cue(75.0, &cfg), "punch_heavy");
        assert_eq!(punch_cue(100.0, &cfg), "punch_heavy");
        let custom = SoundConfig { hook_from: 20.0, heavy_from: 50.0, ..Default::default() };
        assert_eq!(punch_cue(30.0, &custom), "punch_hook");
        assert_eq!(punch_cue(50.0, &custom), "punch_heavy");
    }

    #[test]
    fn events_map_to_cues() {
        let mut cfg = SoundConfig::default();
        let now = Utc::now();
        assert_eq!(cue_for_event(&Event::SessionStarted { timestamp: now }, &cfg).as_deref(), Some("session_started"));
        assert_eq!(cue_for_event(&Event::EnergyFull, &cfg).as_deref(), Some("energy_full"));
        assert_eq!(cue_for_event(&Event::FightStarted { timestamp: now }, &cfg).as_deref(), Some("fight_started"));
        assert_eq!(cue_for_event(&punch(80.0), &cfg).as_deref(), Some("punch_heavy"));
        assert_eq!(cue_for_event(&Event::MusicStopped, &cfg), None);
        assert_eq!(cue_for_event(&Event::AudioLevelChanged { level_db: -20.0 }, &cfg), None);
        // fight end: generic fallback, then the reason-specific cue once configured
        assert_eq!(cue_for_event(&ended(FightEndReason::TimeElapsed), &cfg).as_deref(), Some("fight_ended"));
        cfg.cues.insert("fight_ended_inactivity".into(), cue(&["x.wav"]));
        assert_eq!(cue_for_event(&ended(FightEndReason::InactivityTimeout), &cfg).as_deref(), Some("fight_ended_inactivity"));
        assert_eq!(cue_for_event(&ended(FightEndReason::TimeElapsed), &cfg).as_deref(), Some("fight_ended"));
    }

    #[test]
    fn never_the_same_file_twice_in_a_row() {
        let mut cfg = SoundConfig::default();
        cfg.cues.insert("punch_jab".into(), cue(&["a.wav", "b.wav", "c.wav"]));
        let (mut e, fake) = engine(cfg);
        let t = Instant::now();
        for _ in 0..60 {
            assert!(e.trigger("punch_jab", t));
            fake.finish_all();
            e.tick(t);
        }
        let files = fake.files();
        assert!(files.windows(2).all(|w| w[0] != w[1]), "{files:?}");
        for f in ["a.wav", "b.wav", "c.wav"] {
            assert!(files.iter().any(|x| x == f), "{f} never picked");
        }
    }

    #[test]
    fn single_file_cue_repeats() {
        let mut cfg = SoundConfig::default();
        cfg.cues.insert("fight_started".into(), cue(&["only.wav"]));
        let (mut e, fake) = engine(cfg);
        let t = Instant::now();
        assert!(e.trigger("fight_started", t));
        assert!(e.trigger("fight_started", t));
        assert_eq!(fake.files(), ["only.wav", "only.wav"]);
    }

    #[test]
    fn min_interval_drops_close_triggers() {
        let mut cfg = SoundConfig::default();
        cfg.cues.insert("punch_jab".into(), CueConfig { min_interval_ms: 200, ..cue(&["j.wav"]) });
        let (mut e, fake) = engine(cfg);
        let t = Instant::now();
        assert!(e.trigger("punch_jab", t));
        assert!(!e.trigger("punch_jab", t + Duration::from_millis(199)));
        assert!(e.trigger("punch_jab", t + Duration::from_millis(200)));
        assert_eq!(fake.files().len(), 2);
    }

    #[test]
    fn concurrency_cap_drops_extra_cues_until_one_ends() {
        let mut cfg = SoundConfig { max_concurrent: 2, ..Default::default() };
        cfg.cues.insert("punch_jab".into(), cue(&["j.wav"]));
        let (mut e, fake) = engine(cfg);
        let t = Instant::now();
        assert!(e.trigger("punch_jab", t));
        assert!(e.trigger("punch_jab", t));
        assert!(!e.trigger("punch_jab", t), "cap of 2");
        assert_eq!(e.active_cues(), 2);
        fake.finish_all();
        assert!(e.trigger("punch_jab", t), "room again");
    }

    #[test]
    fn gain_multiplies_master_volume() {
        let mut cfg = SoundConfig { volume: 0.5, dir: PathBuf::from("/snd"), ..Default::default() };
        cfg.cues.insert("energy_full".into(), CueConfig { gain: 0.5, ..cue(&["e.wav"]) });
        let (mut e, fake) = engine(cfg);
        e.trigger("energy_full", Instant::now());
        let played = fake.played.lock().unwrap();
        assert_eq!(played[0], (PathBuf::from("/snd/e.wav"), 0.25));
    }

    #[test]
    fn unconfigured_and_empty_cues_do_nothing() {
        let mut cfg = SoundConfig::default();
        cfg.cues.insert("energy_full".into(), cue(&[]));
        let (mut e, fake) = engine(cfg);
        assert!(!e.trigger("fight_started", Instant::now()));
        assert!(!e.trigger("energy_full", Instant::now()));
        assert!(fake.files().is_empty());
        assert_eq!(e.on_event(&Event::EnergyFull, Instant::now()), None);
    }

    #[test]
    fn missing_file_is_skipped_without_panic_and_tried_again_later() {
        let mut cfg = SoundConfig { dir: PathBuf::from("/snd"), ..Default::default() };
        cfg.cues.insert("fight_started".into(), cue(&["gone.wav"]));
        let (mut e, fake) = engine(cfg);
        fake.missing.lock().unwrap().push(PathBuf::from("/snd/gone.wav"));
        let t = Instant::now();
        assert!(!e.trigger("fight_started", t));
        assert!(!e.trigger("fight_started", t));
        assert_eq!(e.warned.len(), 1, "warned once per file");
        assert_eq!(e.active_cues(), 0);
        // The file shows up (assets added while running): plays, no restart needed.
        fake.missing.lock().unwrap().clear();
        assert!(e.trigger("fight_started", t));
    }

    #[test]
    fn ducking_follows_cues_and_releases_after_hold() {
        let mut cfg = SoundConfig { music_duck_db: -20.0, duck_ms: 300, ..Default::default() };
        cfg.cues.insert("fight_started".into(), cue(&["f.wav"]));
        let (mut e, fake) = engine(cfg);
        let t = Instant::now();
        let factor = e.on_event(&Event::FightStarted { timestamp: Utc::now() }, t).expect("ducks on first cue");
        assert!((factor - 0.1).abs() < 1e-4, "-20 dB is 0.1, got {factor}");
        assert_eq!(e.tick(t + Duration::from_millis(100)), None, "still ducked, no repeat");
        fake.finish_all();
        assert_eq!(e.tick(t + Duration::from_millis(150)), None, "cue over, hold starts");
        assert_eq!(e.tick(t + Duration::from_millis(400)), None, "held 250 ms of 300");
        assert_eq!(e.tick(t + Duration::from_millis(450)), Some(1.0), "released");
        assert_eq!(e.tick(t + Duration::from_millis(900)), None);
    }

    #[test]
    fn a_new_cue_during_the_hold_keeps_the_duck() {
        let mut cfg = SoundConfig { duck_ms: 300, ..Default::default() };
        cfg.cues.insert("punch_jab".into(), cue(&["j.wav"]));
        let (mut e, fake) = engine(cfg);
        let t = Instant::now();
        assert!(e.on_event(&punch(10.0), t).is_some());
        fake.finish_all();
        assert_eq!(e.tick(t + Duration::from_millis(50)), None);
        // second punch before the hold is over: no release, no second duck
        assert_eq!(e.on_event(&punch(10.0), t + Duration::from_millis(200)), None);
        fake.finish_all();
        assert_eq!(e.tick(t + Duration::from_millis(300)), None);
        assert_eq!(e.tick(t + Duration::from_millis(600)), Some(1.0));
    }

    #[test]
    fn dropped_cue_does_not_duck() {
        let mut cfg = SoundConfig::default();
        cfg.cues.insert("punch_jab".into(), CueConfig { min_interval_ms: 1000, ..cue(&["j.wav"]) });
        let (mut e, fake) = engine(cfg);
        let t = Instant::now();
        e.on_event(&punch(10.0), t);
        fake.finish_all();
        e.tick(t + Duration::from_millis(400));
        assert_eq!(e.tick(t + Duration::from_millis(800)), Some(1.0));
        assert_eq!(e.on_event(&punch(10.0), t + Duration::from_millis(900)), None, "dropped by min interval");
    }
}
