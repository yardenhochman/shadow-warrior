use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Stream, StreamConfig, BufferSize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use anyhow::{Context, Result};

use earshot::Detector;

use super::audio_frontend::{ClipRecorder, FrameFeatures, FrontEnd, FrontEndConfig};

use crate::config::AudioConfig;
use crate::core::{Event, SharedEventBus};

pub struct AudioManager {
    config: Arc<RwLock<AudioConfig>>,
    event_bus: SharedEventBus,
    _stream: Arc<RwLock<Option<Stream>>>,
    connected: Arc<AtomicBool>,
    current_level_db: Arc<RwLock<f32>>,
    shout_score: Arc<RwLock<f32>>,
    last_event_time: Arc<RwLock<Instant>>,
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
            shout_score: Arc::new(RwLock::new(0.0)),
            last_event_time: Arc::new(RwLock::new(Instant::now())),
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

        let config_lock = self.config.clone();
        let event_bus = self.event_bus.clone();
        let current_level_db = self.current_level_db.clone();
        let shout_score = self.shout_score.clone();
        let last_event_time = self.last_event_time.clone();
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
                Self::process_audio_vad_sync(
                    data,
                    actual_ch,
                    &event_bus,
                    &current_level_db,
                    &shout_score,
                    &config_lock,
                    &last_event_time,
                    &frontend,
                    &features_tx,
                );
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

    /// Process audio: band-pass + resample + features via the shared front-end, then
    /// run the legacy energy shout logic on VAD-positive frames.
    fn process_audio_vad_sync(
        data: &[f32],
        channels: u16,
        event_bus: &SharedEventBus,
        current_level_db: &Arc<RwLock<f32>>,
        shout_score: &Arc<RwLock<f32>>,
        config_lock: &Arc<RwLock<AudioConfig>>,
        last_event_time: &Arc<RwLock<Instant>>,
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
        let vad_threshold = config_lock.try_read().map(|c| c.vad_threshold).unwrap_or(0.5);
        for (frame, feat) in frames {
            let speech = feat.vad_prob > vad_threshold;
            features_tx.send_replace(Some(feat));
            if speech {
                Self::handle_speech_frame(
                    &frame,
                    event_bus,
                    current_level_db,
                    shout_score,
                    config_lock,
                    last_event_time,
                );
            }
        }
    }

    /// Subscribe to per-frame (16ms) features.
    #[allow(dead_code)]
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

    fn handle_speech_frame(
        data: &[f32],
        event_bus: &SharedEventBus,
        current_level_db: &Arc<RwLock<f32>>,
        shout_score: &Arc<RwLock<f32>>,
        config_lock: &Arc<RwLock<AudioConfig>>,
        last_event_time: &Arc<RwLock<Instant>>,
    ) {
        // Get config parameters (use try_read to avoid blocking)
        let (shout_threshold_db, shout_sensitivity) = if let Ok(c) = config_lock.try_read() {
            (c.shout_threshold_db, c.shout_sensitivity)
        } else {
            (-15.0, 1.0) // Defaults
        };

        // Calculate RMS (root mean square)
        let sum_squares: f32 = data.iter().map(|&s| s * s).sum();
        let rms = (sum_squares / data.len() as f32).sqrt();

        // Convert to dB
        let db = if rms > 0.0 {
            20.0 * rms.log10()
        } else {
            -100.0
        };

        // Update current level
        if let Ok(mut level) = current_level_db.try_write() {
            *level = db;
        }

        // Emit level changed event (throttled)
        if let Ok(mut last_time) = last_event_time.try_write() {
            let now = Instant::now();
            if now.duration_since(*last_time) >= Duration::from_millis(100) {
                event_bus.publish(Event::AudioLevelChanged { level_db: db });
                *last_time = now;
            }
        }

        // Shout detection logic (only triggers if dB > threshold)
        if db > shout_threshold_db {
            if let Ok(mut score) = shout_score.try_write() {
                let strength = ((db - shout_threshold_db) / 10.0 + 1.0).min(2.0);
                *score += 1.0 * strength * shout_sensitivity;

                if *score > 10.0 {
                    let intensity = (*score / 20.0).min(1.0);
                    event_bus.publish(Event::shout_detected(intensity, 0.5, db));
                    *score = 0.0;
                }
            }
        } else {
            if let Ok(mut score) = shout_score.try_write() {
                *score *= 0.95;
                if *score < 0.1 {
                    *score = 0.0;
                }
            }
        }
    }

    /// Update audio parameters dynamically
    pub async fn update_parameters(&self, threshold_db: f32, sensitivity: f32, vad_threshold: f32) {
        let mut config = self.config.write().await;
        config.shout_threshold_db = threshold_db;
        config.shout_sensitivity = sensitivity;
        config.vad_threshold = vad_threshold;
        info!("Audio parameters updated: threshold={:.1}dB, sensitivity={:.1}, vad={:.2}", 
            threshold_db, sensitivity, vad_threshold);
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

    /// Get current audio level in dB
    #[allow(dead_code)]
    pub async fn current_level_db(&self) -> f32 {
        *self.current_level_db.read().await
    }

    /// Check if connected
    #[allow(dead_code)]
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
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

    /// Get energy percentage (0.0 to 1.0)
    #[allow(dead_code)]
    pub async fn percentage(&self) -> f32 {
        let current = self.current().await;
        let max = *self.max.read().await;
        if max > 0.0 {
            current / max
        } else {
            0.0
        }
    }

    /// Check if energy bar is full
    #[allow(dead_code)]
    pub async fn is_full(&self) -> bool {
        let current = self.current().await;
        let max = *self.max.read().await;
        current >= max
    }

    /// Reset energy bar
    #[allow(dead_code)]
    pub async fn reset(&self) {
        let mut energy = self.current.write().await;
        let max = *self.max.read().await;
        *energy = 0.0;
        self.event_bus.publish(Event::energy_changed(0.0, max));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::EventBus;

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
        let dir = std::env::temp_dir().join(format!("sw_clip_test_{}", std::process::id()));
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

// SAFETY: AudioManager is always wrapped in Arc and stream access is synchronized
unsafe impl Send for AudioManager {}
unsafe impl Sync for AudioManager {}

