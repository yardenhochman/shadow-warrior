//! Fused presence detection.
//!
//! Sources (mic, punches, later radar / motion) turn raw observations into
//! [`Evidence`] (weighted "evidence-seconds"). [`PresenceFusion`] is a pure
//! state machine: a leaky accumulator with an enter threshold and a leave
//! timeout. [`PresenceService`] wires it to the audio feature stream, the event
//! bus (punches) and an evidence channel, and publishes
//! `Event::PresenceDetected` only when the fused state changes.
//!
//! Adding a sensor: implement [`PresenceSource`] (or just hold a
//! [`PresenceService::evidence_sender`] and send `Evidence`) and give it a
//! `presence.source_weights` entry. `ArenaManager` is untouched; it only sees
//! `PresenceDetected`.
//!
//! Hook: a YAMNet classifier (Speech/Laughter/Shout/Footsteps) can sit inside
//! `MicSource::observe`, run only when the near-field gate passes, and scale
//! the returned units by class confidence.

use super::audio_frontend::{FrameFeatures, FRAME_SECS};
use crate::config::PresenceConfig;
use crate::core::{Event, SharedEventBus};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tracing::info;

pub const SOURCE_MIC: &str = "mic";
pub const SOURCE_PUNCH: &str = "punch";

/// One piece of presence evidence. `units` are evidence-seconds before the
/// source weight: 1.0 ~ one second of unambiguous presence.
#[derive(Clone, Debug, PartialEq)]
pub struct Evidence {
    pub source: &'static str,
    pub units: f32,
}

/// A presence sensor: turns its native input into evidence.
pub trait PresenceSource {
    type Input;
    fn name(&self) -> &'static str;
    /// Evidence units for this observation, if any.
    fn observe(&mut self, input: &Self::Input, cfg: &PresenceConfig) -> Option<f32>;
}

/// Near-field microphone source: VAD + margin over the adaptive noise floor.
/// Accepts any human sound, not just shouts. Distant (party) noise is absorbed
/// by the adaptive floor and fails the margin.
#[derive(Debug, Default)]
pub struct MicSource;

impl PresenceSource for MicSource {
    type Input = FrameFeatures;

    fn name(&self) -> &'static str {
        SOURCE_MIC
    }

    fn observe(&mut self, f: &FrameFeatures, cfg: &PresenceConfig) -> Option<f32> {
        let margin =
            cfg.mic_margin_db + if f.playback_active { cfg.mic_playback_margin_db } else { 0.0 };
        if f.vad_prob <= cfg.mic_vad_threshold || f.db_above_floor < margin {
            return None;
        }
        // 0.75 at the margin, 1.0 at margin + 5 dB.
        let strength = (0.75 +(f.db_above_floor - margin) / 20.0).min(1.0);
        Some(strength * FRAME_SECS)
    }
}

/// Punching-bag source: any punch is strong evidence.
#[derive(Debug, Default)]
pub struct PunchSource;

impl PresenceSource for PunchSource {
    type Input = f32; // punch power (unused for now; any punch counts)

    fn name(&self) -> &'static str {
        SOURCE_PUNCH
    }

    fn observe(&mut self, _power: &f32, cfg: &PresenceConfig) -> Option<f32> {
        Some(cfg.punch_units)
    }
}

/// Pure fusion core. Time is passed in (monotonic, since an arbitrary epoch),
/// so tests drive it with synthetic clocks.
#[derive(Debug, Default)]
pub struct PresenceFusion {
    score: f32,
    present: bool,
    last_update: Duration,
    last_evidence: Duration,
    started: bool,
}

impl PresenceFusion {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(dead_code)]
    pub fn present(&self) -> bool {
        self.present
    }

    fn weight(cfg: &PresenceConfig, source: &str) -> f32 {
        cfg.source_weights.get(source).copied().unwrap_or(cfg.default_weight)
    }

    fn advance(&mut self, now: Duration, cfg: &PresenceConfig) {
        if !self.started {
            self.started = true;
            self.last_update = now;
            return;
        }
        let dt = now.saturating_sub(self.last_update).as_secs_f32();
        self.last_update = now.max(self.last_update);
        self.score = (self.score - cfg.decay_per_sec * dt).max(0.0);
    }

    /// Add evidence at `now`. Returns the new state if it changed.
    pub fn add(&mut self, now: Duration, ev: &Evidence, cfg: &PresenceConfig) -> Option<bool> {
        self.advance(now, cfg);
        let units = ev.units * Self::weight(cfg, ev.source);
        if units <= 0.0 {
            return None;
        }
        self.last_evidence = now;
        self.score = (self.score + units).min(cfg.max_score.max(cfg.enter_threshold));
        if !self.present && self.score >= cfg.enter_threshold {
            self.present = true;
            return Some(true);
        }
        None
    }

    /// Advance time without evidence (call periodically). Returns the new
    /// state if it changed.
    pub fn tick(&mut self, now: Duration, cfg: &PresenceConfig) -> Option<bool> {
        self.advance(now, cfg);
        if self.present
            && now.saturating_sub(self.last_evidence).as_secs_f32() >= cfg.leave_timeout_sec
        {
            self.present = false;
            self.score = 0.0;
            return Some(false);
        }
        None
    }
}

/// Runtime glue. Owns the fusion and publishes `PresenceDetected` on changes.
pub struct PresenceService {
    config: Arc<RwLock<PresenceConfig>>,
    event_bus: SharedEventBus,
    tx: mpsc::UnboundedSender<Evidence>,
    rx: std::sync::Mutex<Option<mpsc::UnboundedReceiver<Evidence>>>,
}

impl PresenceService {
    pub fn new(config: PresenceConfig, event_bus: SharedEventBus) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            config: Arc::new(RwLock::new(config)),
            event_bus,
            tx,
            rx: std::sync::Mutex::new(Some(rx)),
        }
    }

    /// Handle for additional sensors (radar, motion...) to report evidence.
    #[allow(dead_code)]
    pub fn evidence_sender(&self) -> mpsc::UnboundedSender<Evidence> {
        self.tx.clone()
    }

    /// Spawn the service. `features` is `AudioManager::subscribe_features()`.
    /// Call once.
    pub fn start(
        self: Arc<Self>,
        mut features: tokio::sync::watch::Receiver<Option<FrameFeatures>>,
    ) {
        let Some(mut ev_rx) = self.rx.lock().ok().and_then(|mut g| g.take()) else { return };
        let mut bus_rx = self.event_bus.subscribe();
        tokio::spawn(async move {
            let epoch = tokio::time::Instant::now();
            let mut fusion = PresenceFusion::new();
            let mut mic = MicSource;
            let mut punch = PunchSource;
            let mut last_index: Option<u64> = None;
            let mut ticker = tokio::time::interval(Duration::from_millis(250));
            let mut features_open = true;
            loop {
                let mut evidence: Vec<Evidence> = Vec::new();
                let cfg = self.config.read().await.clone();
                tokio::select! {
                    r = features.changed(), if features_open => {
                        if r.is_err() {
                            features_open = false;
                            continue;
                        }
                        let Some(f) = features.borrow_and_update().clone() else { continue };
                        // Credit frames skipped by the lossy watch channel (up to 8).
                        let missed = match last_index {
                            Some(l) if f.index > l => ((f.index - l) as usize).clamp(1, 8),
                            _ => 1,
                        };
                        last_index = Some(f.index);
                        if let Some(u) = mic.observe(&f, &cfg) {
                            evidence.push(Evidence { source: mic.name(), units: u * missed as f32 });
                        }
                    }
                    r = bus_rx.recv() => {
                        match r {
                            Ok(Event::PunchDetected { power, .. }) => {
                                if let Some(u) = punch.observe(&power, &cfg) {
                                    evidence.push(Evidence { source: punch.name(), units: u });
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            _ => {}
                        }
                    }
                    Some(e) = ev_rx.recv() => evidence.push(e),
                    _ = ticker.tick() => {}
                }
                let now = epoch.elapsed();
                let mut change = None;
                for e in &evidence {
                    change = fusion.add(now, e, &cfg).or(change);
                }
                change = fusion.tick(now, &cfg).or(change);
                if let Some(detected) = change {
                    info!("Fused presence -> {detected}");
                    self.event_bus.publish(Event::PresenceDetected { detected });
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> PresenceConfig {
        PresenceConfig::default()
    }

    fn frame(i: u64, above: f32, vad: f32) -> FrameFeatures {
        FrameFeatures {
            index: i,
            band_db: -50.0 + above,
            noise_floor_db: -50.0,
            db_above_floor: above,
            vad_prob: vad,
            pitch_hz: 0.0,
            pitch_conf: 0.0,
            spectral_centroid_hz: 600.0,
            spectral_tilt_db_per_khz: -20.0,
            spectral_flatness: 0.5,
            playback_active: false,
        }
    }

    /// Drive fusion with per-frame mic features; returns (time_sec, state) changes.
    fn run_mic(
        fusion: &mut PresenceFusion,
        cfg: &PresenceConfig,
        frames: impl Iterator<Item = FrameFeatures>,
    ) -> Vec<(f32, bool)> {
        let mut mic = MicSource;
        let mut out = vec![];
        for f in frames {
            let now = Duration::from_secs_f32(f.index as f32 * FRAME_SECS);
            if let Some(u) = mic.observe(&f, cfg) {
                if let Some(s) = fusion.add(now, &Evidence { source: SOURCE_MIC, units: u }, cfg) {
                    out.push((now.as_secs_f32(), s));
                }
            }
            if let Some(s) = fusion.tick(now, cfg) {
                out.push((now.as_secs_f32(), s));
            }
        }
        out
    }

    fn frames_for(s: f32) -> u64 {
        (s / FRAME_SECS) as u64
    }

    #[test]
    fn steady_party_noise_never_triggers() {
        // Distant party: VAD fires constantly (vocals) but the adaptive floor
        // follows, so the margin stays ~0-3 dB.
        let c = cfg();
        let mut f = PresenceFusion::new();
        let ev = run_mic(
            &mut f,
            &c,
            (0..frames_for(600.0)).map(|i| frame(i, (i % 7) as f32 * 0.5, 0.9)),
        );
        assert!(ev.is_empty());
        assert!(!f.present());
    }

    #[test]
    fn loud_non_voice_noise_does_not_trigger() {
        let c = cfg();
        let mut f = PresenceFusion::new();
        let ev = run_mic(&mut f, &c, (0..frames_for(30.0)).map(|i| frame(i, 25.0, 0.1)));
        assert!(ev.is_empty());
    }

    #[test]
    fn sporadic_bursts_do_not_trigger() {
        // 0.4 s near-field-looking burst every 8 s for 10 minutes.
        let c = cfg();
        let mut f = PresenceFusion::new();
        let ev = run_mic(
            &mut f,
            &c,
            (0..frames_for(600.0)).map(|i| {
                let t = i as f32 * FRAME_SECS;
                frame(i, if t % 8.0 < 0.4 { 15.0 } else { 0.0 }, 0.9)
            }),
        );
        assert!(ev.is_empty());
    }

    #[test]
    fn normal_talking_triggers_within_2s() {
        let c = cfg();
        let mut f = PresenceFusion::new();
        let start = frames_for(5.0);
        let ev = run_mic(
            &mut f,
            &c,
            (0..frames_for(10.0)).map(|i| {
                let on = i >= start;
                frame(i, if on { 12.0 } else { 0.0 }, if on { 0.9 } else { 0.1 })
            }),
        );
        assert_eq!(ev.len(), 1);
        assert!(ev[0].1);
        let latency = ev[0].0 - 5.0;
        assert!(latency <= 2.0, "latency {latency}");
        assert!(latency >= 0.5, "hysteresis: should not fire instantly, got {latency}");
    }

    #[test]
    fn intermittent_talking_still_triggers() {
        // 50% speech duty cycle (pauses between words), ~0.5 s on / 0.5 s off.
        let c = cfg();
        let mut f = PresenceFusion::new();
        let ev = run_mic(
            &mut f,
            &c,
            (0..frames_for(15.0)).map(|i| {
                let on = (i / 30) % 2 == 0;
                frame(i, if on { 14.0 } else { 0.0 }, if on { 0.9 } else { 0.1 })
            }),
        );
        assert_eq!(ev.first().map(|e| e.1), Some(true));
        assert!(ev[0].0 <= 8.0, "entered at {}", ev[0].0);
    }

    #[test]
    fn punch_counts_as_presence() {
        let c = cfg();
        let mut f = PresenceFusion::new();
        let u = PunchSource.observe(&5.0, &c).unwrap();
        let r = f.add(Duration::from_secs(3), &Evidence { source: SOURCE_PUNCH, units: u }, &c);
        assert_eq!(r, Some(true));
    }

    #[test]
    fn weights_scale_evidence() {
        let mut c = cfg();
        c.source_weights.insert(SOURCE_PUNCH.into(), 0.3);
        let mut f = PresenceFusion::new();
        let e = Evidence { source: SOURCE_PUNCH, units: 1.0 };
        assert_eq!(f.add(Duration::from_secs(0), &e, &c), None);
        assert_eq!(f.add(Duration::from_millis(100), &e, &c), None);
        assert_eq!(f.add(Duration::from_millis(200), &e, &c), None);
        assert_eq!(f.add(Duration::from_millis(300), &e, &c), Some(true));
    }

    #[test]
    fn unknown_source_uses_default_weight() {
        let mut c = cfg();
        c.default_weight = 2.0;
        let mut f = PresenceFusion::new();
        let e = Evidence { source: "radar", units: 0.5 };
        assert_eq!(f.add(Duration::ZERO, &e, &c), Some(true));
    }

    #[test]
    fn leave_after_timeout_and_events_only_on_change() {
        let mut c = cfg();
        c.leave_timeout_sec = 30.0;
        let mut f = PresenceFusion::new();
        let punch = Evidence { source: SOURCE_PUNCH, units: 1.0 };
        let mut changes = vec![];
        // Punches every 5 s for 20 s: one enter event, no repeats.
        for s in (0..=20).step_by(5) {
            if let Some(x) = f.add(Duration::from_secs(s), &punch, &c) {
                changes.push((s, x));
            }
        }
        assert_eq!(changes, vec![(0, true)]);
        // Last evidence at 20 s: still present at +29 s, gone at +30 s, one leave event.
        for s in 21..=100u64 {
            if let Some(x) = f.tick(Duration::from_secs(s), &c) {
                changes.push((s, x));
            }
        }
        assert_eq!(changes, vec![(0, true), (50, false)]);
        assert!(!f.present());
    }

    #[test]
    fn evidence_resets_leave_timer() {
        let mut c = cfg();
        c.leave_timeout_sec = 10.0;
        let mut f = PresenceFusion::new();
        let punch = Evidence { source: SOURCE_PUNCH, units: 1.0 };
        f.add(Duration::from_secs(0), &punch, &c);
        assert_eq!(f.tick(Duration::from_secs(9), &c), None);
        f.add(Duration::from_secs(9), &punch, &c);
        assert_eq!(f.tick(Duration::from_secs(18), &c), None);
        assert_eq!(f.tick(Duration::from_secs(19), &c), Some(false));
    }

    #[test]
    fn can_reenter_after_leaving() {
        let mut c = cfg();
        c.leave_timeout_sec = 5.0;
        let mut f = PresenceFusion::new();
        let punch = Evidence { source: SOURCE_PUNCH, units: 1.0 };
        assert_eq!(f.add(Duration::ZERO, &punch, &c), Some(true));
        assert_eq!(f.tick(Duration::from_secs(5), &c), Some(false));
        assert_eq!(f.add(Duration::from_secs(6), &punch, &c), Some(true));
    }

    #[test]
    fn playback_raises_mic_margin() {
        let c = cfg();
        let mut m = MicSource;
        let mut fr = frame(0, 12.0, 0.9);
        assert!(m.observe(&fr, &c).is_some());
        fr.playback_active = true;
        assert!(m.observe(&fr, &c).is_none());
    }

    #[test]
    fn config_defaults_via_serde() {
        let c: PresenceConfig = serde_yaml::from_str("leave_timeout_sec: 12").unwrap();
        assert_eq!(c.leave_timeout_sec, 12.0);
        assert_eq!(c.enter_threshold, 1.0);
        assert_eq!(c.source_weights.get("punch"), Some(&1.0));
    }
}
