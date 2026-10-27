use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Stream, StreamConfig, BufferSize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use anyhow::{Context, Result};

use earshot::Detector;

use super::shout_detector::ShoutDetector;
use super::audio_frontend::{ClipRecorder, FrameFeatures, FrontEnd, FrontEndConfig};

use crate::config::AudioConfig;
use crate::core::{Event, SharedEventBus};

pub struct AudioManager {
    config: Arc<RwLock<AudioConfig>>,
    event_bus: SharedEventBus,
    _stream: Arc<RwLock<Option<Stream>>>,
    connected: Arc<AtomicBool>,
    current_level_db: Arc<RwLock<f32>>,
    frontend: Arc<std::sync::Mutex<Option<FrontEnd>>>,
    features_tx: tokio::sync::watch::Sender<Option<FrameFeatures>>,
    actual_sample_rate: Arc<RwLock<u32>>,
    actual_channels: Arc<RwLock<u16>>,
}

impl AudioManager {
    pub fn new(config: AudioConfig, event_bus: SharedEventBus) -> Self {
        let config = Arc::new(RwLock::new(config));

        Self {
            config,
            event_bus,
            _stream: Arc::new(RwLock::new(None)),
            connected: Arc::new(AtomicBool::new(false)),
            current_level_db: Arc::new(RwLock::new(-100.0)),
            frontend: Arc::new(std::sync::Mutex::new(None)),
            features_tx: tokio::sync::watch::channel(None).0,
            actual_sample_rate: Arc::new(RwLock::new(16000)),
            actual_channels: Arc::new(RwLock::new(1)),
        }
    }

    /// Start audio capture — negotiates best supported config with device
    pub async fn start(&self) -> Result<()> {
        info!("Starting audio manager");

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("No input device available")?;

        let device_name = device.description().map(|d| d.to_string()).unwrap_or_else(|_| "Unknown".to_string());
        info!("Using audio device: {}", device_name);

        let stream_config = Self::negotiate_config(&device)?;
        let actual_rate = stream_config.sample_rate;
        let actual_ch = stream_config.channels;
        info!("Audio stream config: {}Hz, {} ch", actual_rate, actual_ch);

        *self.actual_sample_rate.write().await = actual_rate;
        *self.actual_channels.write().await = actual_ch;

        let frontend = self.frontend.clone();
        let features_tx = self.features_tx.clone();
        *frontend.lock().unwrap() = Some(FrontEnd::new(
            actual_rate,
            FrontEndConfig::default(),
            Box::new(Detector::default()),
        ));

        let stream = device.build_input_stream(
            &stream_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                Self::process_audio_sync(data, actual_ch, &frontend, &features_tx);
            },
            move |err| {
                error!("Audio stream error: {}", err);
            },
            None,
        )?;

        stream.play()?;

        *self._stream.write().await = Some(stream);
        self.connected.store(true, Ordering::Relaxed);

        self.event_bus.publish(Event::AudioDeviceConnected { device_name });

        self.start_shout_detector();
        self.start_energy_decay_task();

        Ok(())
    }

    /// Negotiate best supported input config — prefers 16kHz mono, falls back to device default.
    fn negotiate_config(device: &cpal::Device) -> Result<StreamConfig> {
        const PREFERRED_RATES: &[u32] = &[16000, 48000, 44100, 22050, 8000];

        if let Ok(supported) = device.supported_input_configs() {
            let supported: Vec<_> = supported.collect();
            for &rate in PREFERRED_RATES {
                for range in &supported {
                    if range.channels() == 1
                        && range.min_sample_rate() <= rate
                        && range.max_sample_rate() >= rate
                    {
                        let cfg = StreamConfig {
                            channels: 1,
                            sample_rate: rate,
                            buffer_size: BufferSize::Default,
                        };
                        info!("Selected audio config: {}Hz mono", rate);
                        return Ok(cfg);
                    }
                }
            }
            // Try any supported config at preferred rates (stereo fallback)
            for &rate in PREFERRED_RATES {
                for range in &supported {
                    if range.min_sample_rate() <= rate
                        && range.max_sample_rate() >= rate
                    {
                        let cfg = StreamConfig {
                            channels: range.channels(),
                            sample_rate: rate,
                            buffer_size: BufferSize::Default,
                        };
                        warn!("Selected audio config: {}Hz {} ch (stereo fallback)", rate, range.channels());
                        return Ok(cfg);
                    }
                }
            }
        }

        // Last resort: device default
        let default = device.default_input_config().context("No supported input config")?;
        warn!("Using device default audio config: {}Hz {} ch", default.sample_rate(), default.channels());
        Ok(StreamConfig {
            channels: default.channels(),
            sample_rate: default.sample_rate(),
            buffer_size: BufferSize::Default,
        })
    }

    /// Mix interleaved stereo (or N-ch) to mono
    fn mix_to_mono(data: &[f32], channels: u16) -> Vec<f32> {
        if channels == 1 {
            return data.to_vec();
        }
        let ch = channels as usize;
        (0..data.len() / ch)
            .map(|i| data[i * ch..i * ch + ch].iter().sum::<f32>() / ch as f32)
            .collect()
    }

    /// Run the shared front-end (band-pass, resample, features) and publish per-frame
    /// features. Shout detection consumes them via `start_shout_detector`.
    fn process_audio_sync(
        data: &[f32],
        channels: u16,
        frontend: &Arc<std::sync::Mutex<Option<FrontEnd>>>,
        features_tx: &tokio::sync::watch::Sender<Option<FrameFeatures>>,
    ) {
        let mono = Self::mix_to_mono(data, channels);
        let frames = match frontend.try_lock() {
            Ok(mut g) => match g.as_mut() {
                Some(fe) => fe.process(&mono),
                None => return,
            },
            Err(_) => return,
        };
        for (_frame, feat) in frames {
            features_tx.send_replace(Some(feat));
        }
    }

    /// Spawn the shout detector: consumes `subscribe_features()` and publishes
    /// `AudioLevelChanged` (throttled) and `ShoutDetected`.
    fn start_shout_detector(&self) {
        let mut rx = self.subscribe_features();
        let config = self.config.clone();
        let event_bus = self.event_bus.clone();
        let level = self.current_level_db.clone();
        tokio::spawn(async move {
            let mut det = ShoutDetector::new();
            let mut last_level_event = Instant::now();
            while rx.changed().await.is_ok() {
                let Some(feat) = rx.borrow_and_update().clone() else { continue };
                let cfg = config.read().await.clone();
                *level.write().await = feat.band_db;
                if last_level_event.elapsed() >= Duration::from_millis(100) {
                    event_bus.publish(Event::AudioLevelChanged { level_db: feat.band_db });
                    last_level_event = Instant::now();
                }
                if let Some(d) = det.push(&feat, &cfg) {
                    event_bus.publish(Event::shout_detected(d.intensity, d.duration_sec, d.level_db));
                }
            }
        });
    }

    /// Subscribe to per-frame (16ms) features.
    pub fn subscribe_features(&self) -> tokio::sync::watch::Receiver<Option<FrameFeatures>> {
        self.features_tx.subscribe()
    }

    /// Tell the front-end whether our speakers are playing.
    pub fn set_playback_active(&self, active: bool) {
        if let Ok(mut g) = self.frontend.lock() {
            if let Some(fe) = g.as_mut() {
                fe.set_playback_active(active);
            }
        }
    }

    /// Dump the last `seconds` of audio + features to `dir` (detector triggers / on demand).
    pub fn record_clip(&self, dir: &std::path::Path, reason: &str, seconds: f32) -> Result<std::path::PathBuf> {
        let (audio, feats) = {
            let g = self.frontend.lock().map_err(|_| anyhow::anyhow!("frontend poisoned"))?;
            g.as_ref().context("audio not started")?.snapshot_clip(seconds)
        };
        let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%.3f").to_string();
        Ok(ClipRecorder::new(dir).record(reason, &stamp, &audio, &feats)?)
    }

    /// Record a clip using the configured directory and length.
    pub async fn record_clip_default(&self, reason: &str) -> Result<std::path::PathBuf> {
        let (dir, seconds) = {
            let c = self.config.read().await;
            (c.clip_dir.clone(), c.clip_seconds)
        };
        self.record_clip(&dir, reason, seconds)
    }

    /// Record a clip on every shout (rate limited by `clip_cooldown_sec`).
    pub fn start_shout_clip_trigger(self: Arc<Self>) {
        let mut rx = self.event_bus.subscribe();
        tokio::spawn(async move {
            let mut last: Option<Instant> = None;
            loop {
                match rx.recv().await {
                    Ok(Event::ShoutDetected { intensity, .. }) => {
                        let (enabled, cooldown) = {
                            let c = self.config.read().await;
                            (c.clip_on_shout, c.clip_cooldown_sec)
                        };
                        if !clip_due(enabled, last, Instant::now(), cooldown) {
                            continue;
                        }
                        last = Some(Instant::now());
                        match self.record_clip_default("shout").await {
                            Ok(p) => info!("Shout clip saved ({:.2}): {}", intensity, p.display()),
                            Err(e) => warn!("Shout clip failed: {}", e),
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    _ => {}
                }
            }
        });
    }

    /// Apply shout-detector tunables live. The detector re-reads the shared
    /// config every frame, so no restart is needed. Values are clamped to sane ranges.
    pub async fn update_shout_tunables(&self, t: &crate::models::GameTunables) {
        let mut c = self.config.write().await;
        c.shout_margin_db = t.shout_margin_db.clamp(3.0, 40.0);
        c.shout_playback_margin_db = t.shout_playback_margin_db.clamp(0.0, 30.0);
        c.shout_min_duration_sec = t.shout_min_duration_sec.clamp(0.05, 0.28);
        c.shout_min_features = t.shout_min_features.clamp(1, 4);
        c.shout_gap_frames = t.shout_gap_frames.min(20);
        c.shout_score_threshold = t.shout_score_threshold.clamp(1.0, 50.0);
        c.shout_sensitivity = t.shout_sensitivity.clamp(0.1, 5.0);
        c.vad_threshold = t.vad_threshold.clamp(0.0, 1.0);
        info!(
            "Shout tunables updated: margin={:.1}dB playback+={:.1}dB min_dur={:.2}s features>={} gap={} score>={:.1} sens={:.2} vad={:.2}",
            c.shout_margin_db, c.shout_playback_margin_db, c.shout_min_duration_sec,
            c.shout_min_features, c.shout_gap_frames, c.shout_score_threshold,
            c.shout_sensitivity, c.vad_threshold
        );
    }

    /// Start background task for energy decay
    fn start_energy_decay_task(&self) {
        // This will be used by arena manager to decay energy bar
        // For now, just a placeholder
    }

    /// Get current config
    pub fn config(&self) -> Arc<RwLock<AudioConfig>> {
        self.config.clone()
    }

}

/// Whether an automatic clip should be recorded now.
fn clip_due(enabled: bool, last: Option<Instant>, now: Instant, cooldown_sec: f32) -> bool {
    enabled
        && last.is_none_or(|l| now.duration_since(l) >= Duration::from_secs_f32(cooldown_sec.max(0.0)))
}

/// Energy bar tracker for WARMING state
pub struct EnergyBar {
    current: Arc<RwLock<f32>>,
    max: Arc<RwLock<f32>>,
    multiplier: Arc<RwLock<f32>>,
    decay_rate: Arc<RwLock<f32>>, // Points per second
    event_bus: SharedEventBus,
}

impl EnergyBar {
    pub fn new(max: f32, multiplier: f32, decay_rate: f32, event_bus: SharedEventBus) -> Self {
        Self {
            current: Arc::new(RwLock::new(0.0)),
            max: Arc::new(RwLock::new(max)),
            multiplier: Arc::new(RwLock::new(multiplier)),
            decay_rate: Arc::new(RwLock::new(decay_rate)),
            event_bus,
        }
    }

    /// Update parameters dynamically
    pub async fn update_parameters(&self, max: f32, multiplier: f32, decay_rate: f32) {
        *self.max.write().await = max;
        *self.multiplier.write().await = multiplier;
        *self.decay_rate.write().await = decay_rate;
    }

    /// Add energy from a shout
    pub async fn add_energy(&self, intensity: f32) {
        let mut energy = self.current.write().await;
        let max = *self.max.read().await;
        let multiplier = *self.multiplier.read().await;
        let added = intensity * multiplier;
        *energy = (*energy + added).min(max);

        debug!("Energy: {:.1}/{:.1} (+{:.1})", *energy, max, added);

        self.event_bus
            .publish(Event::energy_changed(*energy, max));

        if *energy >= max {
            self.event_bus.publish(Event::EnergyFull);
        }
    }

    /// Decay energy over time
    pub async fn decay(&self, delta_sec: f32) {
        let mut energy = self.current.write().await;
        let max = *self.max.read().await;
        let decay_rate = *self.decay_rate.read().await;
        *energy = (*energy - decay_rate * delta_sec).max(0.0);

        self.event_bus
            .publish(Event::energy_changed(*energy, max));
    }

    /// Get current energy
    pub async fn current(&self) -> f32 {
        *self.current.read().await
    }

    #[cfg(test)]
    /// Get energy percentage (0.0 to 1.0)
    pub async fn percentage(&self) -> f32 {
        let current = self.current().await;
        let max = *self.max.read().await;
        if max > 0.0 {
            current / max
        } else {
            0.0
        }
    }

    #[cfg(test)]
    /// Check if energy bar is full
    pub async fn is_full(&self) -> bool {
        let current = self.current().await;
        let max = *self.max.read().await;
        current >= max
    }

    #[cfg(test)]
    /// Reset energy bar
    pub async fn reset(&self) {
        let mut energy = self.current.write().await;
        let max = *self.max.read().await;
        *energy = 0.0;
        self.event_bus.publish(Event::energy_changed(0.0, max));
    }
}

// SAFETY: AudioManager is always wrapped in Arc and stream access is synchronized
unsafe impl Send for AudioManager {}
unsafe impl Sync for AudioManager {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::EventBus;

    fn tunables(am_cfg: &AudioConfig) -> crate::models::GameTunables {
        crate::models::GameTunables {
            warming_energy_threshold: 100.0,
            shout_energy_multiplier: 10.0,
            energy_decay_rate: 0.5,
            fight_duration_sec: 180,
            fight_inactivity_timeout_sec: 30,
            cooldown_duration_sec: 60,
            shout_sensitivity: am_cfg.shout_sensitivity,
            vad_threshold: am_cfg.vad_threshold,
            shout_margin_db: am_cfg.shout_margin_db,
            shout_playback_margin_db: am_cfg.shout_playback_margin_db,
            shout_min_duration_sec: am_cfg.shout_min_duration_sec,
            shout_min_features: am_cfg.shout_min_features,
            shout_gap_frames: am_cfg.shout_gap_frames,
            shout_score_threshold: am_cfg.shout_score_threshold,
            punch_threshold: 1.0,
            punch_alpha: 0.5,
        }
    }

    fn shout_frame(i: u64) -> crate::services::audio_frontend::FrameFeatures {
        crate::services::audio_frontend::FrameFeatures {
            index: i,
            band_db: -22.0,
            noise_floor_db: -50.0,
            db_above_floor: 28.0,
            vad_prob: 0.9,
            pitch_hz: 380.0,
            pitch_conf: 0.8,
            spectral_centroid_hz: 1800.0,
            spectral_tilt_db_per_khz: -8.0,
            spectral_flatness: 0.15,
            playback_active: false,
        }
    }

    #[tokio::test]
    async fn shout_tunables_apply_live_and_reach_detector() {
        let am = AudioManager::new(crate::config::Config::default().audio, Arc::new(EventBus::new(10)));
        let fires = |cfg: &AudioConfig| {
            let mut det = ShoutDetector::new();
            (0..100).filter_map(|i| det.push(&shout_frame(i), cfg)).count()
        };
        let base = am.config().read().await.clone();
        assert!(fires(&base) > 0, "28 dB above floor fires at default margin");

        let mut t = tunables(&base);
        t.shout_margin_db = 35.0;
        t.shout_min_duration_sec = 5.0; // clamped
        t.shout_min_features = 9; // clamped
        t.vad_threshold = 2.0; // clamped
        am.update_shout_tunables(&t).await;
        let c = am.config().read().await.clone();
        assert_eq!(c.shout_margin_db, 35.0);
        assert_eq!(c.shout_min_duration_sec, 0.28);
        assert_eq!(c.shout_min_features, 4);
        assert_eq!(c.vad_threshold, 1.0);
        assert_eq!(fires(&c), 0, "raised margin silences the same input");
    }

    #[test]
    fn tunables_json_roundtrip_and_legacy_key() {
        let am_cfg = crate::config::Config::default().audio;
        let t = tunables(&am_cfg);
        let back: crate::models::GameTunables =
            serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
        assert_eq!(back.shout_margin_db, t.shout_margin_db);
        assert_eq!(back.shout_gap_frames, t.shout_gap_frames);
        // Old dashboard payload: legacy key present, new keys absent.
        let old = r#"{"warming_energy_threshold":100,"shout_energy_multiplier":10,"energy_decay_rate":0.5,
            "fight_duration_sec":180,"fight_inactivity_timeout_sec":30,"cooldown_duration_sec":60,
            "shout_threshold_db":-15.0,"shout_sensitivity":1.0,"vad_threshold":0.5,
            "punch_threshold":1.0,"punch_alpha":0.5}"#;
        let o: crate::models::GameTunables = serde_json::from_str(old).unwrap();
        assert_eq!(o.shout_margin_db, 15.0);
        assert_eq!(o.shout_min_features, 3);
    }

    #[test]
    fn clip_throttle() {
        let t0 = Instant::now();
        assert!(clip_due(true, None, t0, 10.0));
        assert!(!clip_due(false, None, t0, 10.0));
        assert!(!clip_due(true, Some(t0), t0 + Duration::from_secs(5), 10.0));
        assert!(clip_due(true, Some(t0), t0 + Duration::from_secs(10), 10.0));
    }

    #[tokio::test]
    async fn record_clip_writes_files_from_synthetic_frames() {
        use crate::services::audio_frontend::ConstVad;
        let bus = Arc::new(EventBus::new(10));
        let am = AudioManager::new(crate::config::Config::default().audio, bus);
        let mut fe = FrontEnd::new(16000, FrontEndConfig::default(), Box::new(ConstVad(0.9)));
        let tone: Vec<f32> = (0..16000).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
        fe.process(&tone);
        *am.frontend.lock().unwrap() = Some(fe);
        am.set_playback_active(true);
        let dir = std::env::temp_dir().join(format!("sw_clip_am_test_{}", std::process::id()));
        let p = am.record_clip(&dir, "unit", 0.5).unwrap();
        assert!(p.exists());
        assert!(p.with_extension("json").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn test_energy_bar() {
        let event_bus = Arc::new(EventBus::new(10));
        let energy_bar = EnergyBar::new(100.0, 10.0, 0.5, event_bus);

        // Initial state
        assert_eq!(energy_bar.current().await, 0.0);
        assert!(!energy_bar.is_full().await);

        // Add energy
        energy_bar.add_energy(5.0).await; // 5.0 * 10.0 = 50.0
        assert_eq!(energy_bar.current().await, 50.0);
        assert_eq!(energy_bar.percentage().await, 0.5);

        // Add more to fill
        energy_bar.add_energy(10.0).await; // Would add 100, but caps at max
        assert_eq!(energy_bar.current().await, 100.0);
        assert!(energy_bar.is_full().await);

        // Decay
        energy_bar.decay(10.0).await; // 10s * 0.5 = 5.0 decay
        assert_eq!(energy_bar.current().await, 95.0);

        // Reset
        energy_bar.reset().await;
        assert_eq!(energy_bar.current().await, 0.0);
    }
}
