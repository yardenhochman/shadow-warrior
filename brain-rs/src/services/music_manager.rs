use anyhow::{Context, Result};
use rand::seq::SliceRandom;
use rand::{thread_rng, Rng};
use rodio::{Decoder, Player, DeviceSinkBuilder};
use std::fs::{self, File};
use std::io::BufReader;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::config::MusicConfig;
use crate::core::{Event, SharedEventBus};

/// Whether the n-th (0-based) output stream error gets logged: the first three,
/// then only at powers of two, so a dead device can't flood the log.
fn should_log_stream_error(n: u64) -> bool {
    n < 3 || (n + 1).is_power_of_two()
}

/// rodio's default callback eprintln!s every stream error; on a broken ALSA
/// device that is thousands per second (1.8M lines in a minute on the PC).
fn log_output_stream_error(err: rodio::cpal::StreamError) {
    static COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if should_log_stream_error(n) {
        error!("Music output stream error (#{}): {}", n + 1, err);
    }
}

/// Pure playlist cursor: decides which track follows the current one (wraps around).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PlaylistCursor {
    index: usize,
}

impl PlaylistCursor {
    #[cfg(test)]
    pub fn index(&self) -> usize {
        self.index
    }

    pub fn reset(&mut self) {
        self.index = 0;
    }

    /// Move to the next track, wrapping at the end. Returns None for an empty playlist.
    pub fn advance(&mut self, len: usize) -> Option<usize> {
        if len == 0 {
            return None;
        }
        self.index = (self.index + 1) % len;
        Some(self.index)
    }
}

/// Reshuffle the playlist for a new fight. Without this the order is fixed at start-up and every
/// fight begins with the same track. The new first track is never the one the previous fight began
/// with (when there is a choice), so back-to-back fights do not open with the same song.
pub fn reshuffle_for_session(tracks: &mut [PathBuf], previous_first: Option<&Path>, rng: &mut impl Rng) {
    tracks.shuffle(rng);
    if tracks.len() > 1 && previous_first.is_some_and(|p| tracks[0] == p) {
        let other = rng.gen_range(1..tracks.len());
        tracks.swap(0, other);
    }
}

/// How many tracks the player should hold (current + next) so the next one is
/// already queued when the current ends, giving a gapless transition.
pub const QUEUE_AHEAD: usize = 2;

/// Pure decision: should another track be queued? True while we believe we are
/// playing and the player holds fewer than `QUEUE_AHEAD` tracks.
pub fn should_queue_more(playing: bool, queued: usize) -> bool {
    playing && queued < QUEUE_AHEAD
}

/// Minimal audio player seam so the advance logic is testable without a device.
pub trait AudioPlayer: Send + Sync {
    /// Number of tracks queued, including the one currently playing.
    fn queued(&self) -> usize;
    /// Decode `path` and append it to the queue.
    fn enqueue(&self, path: &Path) -> Result<()>;
    fn stop(&self);
    /// Output volume (linear), applied now and to tracks queued later.
    fn set_volume(&self, volume: f32);
}

struct RodioPlayer(Player);

impl AudioPlayer for RodioPlayer {
    fn queued(&self) -> usize {
        self.0.len()
    }

    fn enqueue(&self, path: &Path) -> Result<()> {
        let file = File::open(path)?;
        let source = Decoder::new(BufReader::new(file))?;
        self.0.append(source);
        Ok(())
    }

    fn stop(&self) {
        self.0.stop();
    }

    fn set_volume(&self, volume: f32) {
        self.0.set_volume(volume);
    }
}

fn track_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Unknown")
        .to_string()
}

pub struct MusicManager {
    config: MusicConfig,
    event_bus: SharedEventBus,
    playlist: Arc<RwLock<Vec<PathBuf>>>,
    current_track: Arc<RwLock<Option<String>>>,
    sink: Arc<RwLock<Option<Box<dyn AudioPlayer>>>>,
    /// Names of tracks held by the player; front is the one currently playing.
    queued_names: Arc<RwLock<VecDeque<String>>>,
    playing: Arc<RwLock<bool>>,
    cursor: Arc<RwLock<PlaylistCursor>>,
    /// Ducking factor 0..1 from the sound manager, times `config.volume` (1.0 = not ducked).
    duck: Arc<RwLock<f32>>,
    /// The track the last session started with (see `reshuffle_for_session`).
    last_first: Arc<RwLock<Option<PathBuf>>>,
}

impl MusicManager {
    pub fn new(config: MusicConfig, event_bus: SharedEventBus) -> Self {
        Self {
            config,
            event_bus,
            playlist: Arc::new(RwLock::new(Vec::new())),
            current_track: Arc::new(RwLock::new(None)),
            sink: Arc::new(RwLock::new(None)),
            queued_names: Arc::new(RwLock::new(VecDeque::new())),
            playing: Arc::new(RwLock::new(false)),
            cursor: Arc::new(RwLock::new(PlaylistCursor::default())),
            duck: Arc::new(RwLock::new(1.0)),
            last_first: Arc::new(RwLock::new(None)),
        }
    }

    /// Duck the music under a sound cue: `factor` 1.0 restores the configured volume.
    pub async fn set_duck(&self, factor: f32) {
        let factor = factor.clamp(0.0, 1.0);
        *self.duck.write().await = factor;
        if let Some(sink) = self.sink.read().await.as_ref() {
            sink.set_volume(self.config.volume * factor);
        }
    }

    /// Load playlist from configured directory
    pub async fn load_playlist(&self) -> Result<()> {
        info!("Loading playlist from {:?}", self.config.playlist_dir);

        if !self.config.playlist_dir.exists() {
            warn!(
                "Playlist directory does not exist: {:?}",
                self.config.playlist_dir
            );
            return Ok(());
        }

        let mut tracks = Vec::new();

        for entry in fs::read_dir(&self.config.playlist_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                if let Some(ext) = path.extension() {
                    let ext_str = ext.to_string_lossy().to_lowercase();
                    if self.config.formats.contains(&ext_str) {
                        tracks.push(path);
                    }
                }
            }
        }

        if self.config.shuffle {
            let mut rng = thread_rng();
            tracks.shuffle(&mut rng);
        }

        let count = tracks.len();
        *self.playlist.write().await = tracks;

        info!("Loaded {} tracks to playlist", count);
        Ok(())
    }

    /// Start playing music from playlist
    pub async fn play(&self) -> Result<()> {
        if *self.playing.read().await {
            info!("Music already playing");
            return Ok(());
        }

        if self.config.shuffle {
            let previous = self.last_first.read().await.clone();
            reshuffle_for_session(&mut self.playlist.write().await, previous.as_deref(), &mut thread_rng());
        }

        let playlist = self.playlist.read().await;
        if playlist.is_empty() {
            warn!("Playlist is empty, cannot play");
            return Ok(());
        }

        // Start from the beginning of the (re)shuffled playlist
        self.cursor.write().await.reset();
        let track_path = playlist[0].clone();
        drop(playlist); // Release lock
        *self.last_first.write().await = Some(track_path.clone());

        let track_name = track_name(&track_path);

        // Initialize audio output and immediately leak it (before any await)
        // Initialize audio output using build container's native drivers
        let device_sink = DeviceSinkBuilder::from_default_device()
            .and_then(|b| {
                b.with_error_callback(log_output_stream_error as fn(rodio::cpal::StreamError))
                    .open_sink_or_fallback()
            })
            .context("Failed to create audio output")?;
        let player = Player::connect_new(device_sink.mixer());
        player.set_volume(self.config.volume * *self.duck.read().await);

        // Leak the device_sink to keep the audio stream alive
        Box::leak(Box::new(device_sink));
        let player = RodioPlayer(player);

        // Load and play track
        match player.enqueue(&track_path) {
            Ok(()) => {
                info!("Playing: {}", track_name);

                *self.sink.write().await = Some(Box::new(player));
                *self.queued_names.write().await = VecDeque::from([track_name.clone()]);
                *self.current_track.write().await = Some(track_name.clone());
                *self.playing.write().await = true;

                self.event_bus
                    .publish(Event::MusicStarted { track_name });

                // Queue the following track right away so the transition is gapless.
                self.tick().await;
                Ok(())
            }
            Err(e) => {
                error!("Failed to load track {}: {}", track_name, e);
                self.event_bus.publish(Event::MusicError {
                    error: e.to_string(),
                });
                Err(e)
            }
        }
    }

    /// Keep the next track queued on the player before the current one ends
    /// (wrapping around the playlist) and report track changes. Call periodically;
    /// see `start_advance_loop`.
    pub async fn tick(&self) {
        let sink_guard = self.sink.read().await;
        let Some(player) = sink_guard.as_ref() else {
            return;
        };
        let playing = *self.playing.read().await;
        if !playing {
            return;
        }

        // Tracks that finished since the last tick drop off the front of the name queue.
        let mut names = self.queued_names.write().await;
        let mut changed = false;
        while names.len() > player.queued() {
            names.pop_front();
            changed = true;
        }
        if changed {
            if let Some(current) = names.front().cloned() {
                info!("Advancing to: {}", current);
                *self.current_track.write().await = Some(current.clone());
                self.event_bus.publish(Event::MusicStarted { track_name: current });
            }
        }

        let playlist = self.playlist.read().await.clone();
        let len = playlist.len();
        // Try each track at most once per tick so a run of broken files cannot spin forever.
        let mut attempts = 0;
        while should_queue_more(playing, names.len()) && attempts < len {
            attempts += 1;
            let Some(idx) = self.cursor.write().await.advance(len) else {
                return;
            };
            let path = &playlist[idx];
            let name = track_name(path);
            match player.enqueue(path) {
                Ok(()) => {
                    names.push_back(name.clone());
                    if names.len() == 1 {
                        // Queue had fully drained: this track starts immediately.
                        info!("Advancing to: {}", name);
                        *self.current_track.write().await = Some(name.clone());
                        self.event_bus.publish(Event::MusicStarted { track_name: name });
                    }
                }
                Err(e) => {
                    error!("Failed to load track {}: {}", name, e);
                    self.event_bus.publish(Event::MusicError {
                        error: e.to_string(),
                    });
                }
            }
        }
    }

    /// Spawn a background loop that keeps the next track queued.
    pub fn start_advance_loop(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
            loop {
                interval.tick().await;
                self.tick().await;
            }
        });
    }

    #[cfg(test)]
    pub async fn current_track(&self) -> Option<String> {
        self.current_track.read().await.clone()
    }

    /// Stop music playback
    pub async fn stop(&self) {
        if !*self.playing.read().await {
            return;
        }

        info!("Stopping music");

        if let Some(sink) = self.sink.write().await.take() {
            sink.stop();
        }

        *self.current_track.write().await = None;
        self.queued_names.write().await.clear();
        *self.playing.write().await = false;

        self.event_bus.publish(Event::MusicStopped);
    }

}

#[cfg(test)]
mod tests {
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn tracks(n: usize) -> Vec<PathBuf> {
        (0..n).map(|i| PathBuf::from(format!("t{i}.m4a"))).collect()
    }

    #[test]
    fn a_new_session_never_starts_with_the_previous_first_track() {
        let mut rng = StdRng::seed_from_u64(7);
        let prev = PathBuf::from("t0.m4a");
        for _ in 0..200 {
            let mut t = tracks(5);
            reshuffle_for_session(&mut t, Some(&prev), &mut rng);
            assert_ne!(t[0], prev);
            assert_eq!(t.len(), 5);
        }
    }

    #[test]
    fn reshuffling_varies_the_first_track_and_keeps_every_track() {
        let mut rng = StdRng::seed_from_u64(1);
        let mut firsts = std::collections::HashSet::new();
        for _ in 0..100 {
            let mut t = tracks(8);
            reshuffle_for_session(&mut t, None, &mut rng);
            firsts.insert(t[0].clone());
            let mut sorted = t.clone();
            sorted.sort();
            assert_eq!(sorted, tracks(8));
        }
        assert!(firsts.len() >= 6, "first track should vary, got {}", firsts.len());
    }

    #[test]
    fn a_single_track_playlist_is_left_alone() {
        let mut rng = StdRng::seed_from_u64(3);
        let mut t = tracks(1);
        reshuffle_for_session(&mut t, Some(Path::new("t0.m4a")), &mut rng);
        assert_eq!(t, tracks(1));
        let mut empty: Vec<PathBuf> = vec![];
        reshuffle_for_session(&mut empty, None, &mut rng);
        assert!(empty.is_empty());
    }

    use super::*;

    #[test]
    fn stream_errors_are_rate_limited() {
        let logged = (0..1_000_000u64).filter(|&n| should_log_stream_error(n)).count();
        assert!(should_log_stream_error(0) && should_log_stream_error(2));
        assert!(logged <= 3 + 20, "logged {logged} of a million errors");
    }

    #[test]
    fn cursor_advances_and_wraps() {
        let mut c = PlaylistCursor::default();
        assert_eq!(c.advance(3), Some(1));
        assert_eq!(c.advance(3), Some(2));
        assert_eq!(c.advance(3), Some(0));
    }

    #[test]
    fn cursor_single_track_loops() {
        let mut c = PlaylistCursor::default();
        assert_eq!(c.advance(1), Some(0));
        assert_eq!(c.advance(1), Some(0));
    }

    #[test]
    fn cursor_empty_playlist() {
        let mut c = PlaylistCursor::default();
        assert_eq!(c.advance(0), None);
    }

    #[test]
    fn cursor_reset() {
        let mut c = PlaylistCursor::default();
        c.advance(4);
        c.reset();
        assert_eq!(c.index(), 0);
    }

    #[test]
    fn queue_more_decision() {
        assert!(should_queue_more(true, 0));
        assert!(should_queue_more(true, 1));
        assert!(!should_queue_more(true, 2));
        assert!(!should_queue_more(false, 0));
    }

    use std::sync::Mutex;

    /// Fake player: `queue` mirrors the rodio queue; tests pop to simulate a track ending.
    #[derive(Default)]
    struct FakePlayer {
        queue: Mutex<Vec<PathBuf>>,
        fail: Mutex<Vec<PathBuf>>,
        volume: Mutex<f32>,
    }

    impl AudioPlayer for Arc<FakePlayer> {
        fn queued(&self) -> usize {
            self.queue.lock().unwrap().len()
        }
        fn enqueue(&self, path: &Path) -> Result<()> {
            if self.fail.lock().unwrap().iter().any(|p| p == path) {
                anyhow::bail!("bad file");
            }
            self.queue.lock().unwrap().push(path.to_path_buf());
            Ok(())
        }
        fn stop(&self) {
            self.queue.lock().unwrap().clear();
        }
        fn set_volume(&self, volume: f32) {
            *self.volume.lock().unwrap() = volume;
        }
    }

    impl FakePlayer {
        fn finish_current(&self) {
            self.queue.lock().unwrap().remove(0);
        }
        fn names(&self) -> Vec<String> {
            self.queue.lock().unwrap().iter().map(|p| track_name(p)).collect()
        }
    }

    async fn manager(tracks: &[&str]) -> (MusicManager, Arc<FakePlayer>) {
        let cfg = MusicConfig {
            playlist_dir: PathBuf::from("."),
            formats: vec![],
            shuffle: false,
            volume: 1.0,
        };
        let m = MusicManager::new(cfg, Arc::new(crate::core::EventBus::new(16)));
        *m.playlist.write().await = tracks.iter().map(PathBuf::from).collect();
        let fake = Arc::new(FakePlayer::default());
        // Simulate play(): first track queued, cursor at 0.
        fake.enqueue(Path::new(tracks[0])).unwrap();
        *m.sink.write().await = Some(Box::new(fake.clone()));
        *m.queued_names.write().await = VecDeque::from([track_name(Path::new(tracks[0]))]);
        *m.current_track.write().await = Some(track_name(Path::new(tracks[0])));
        *m.playing.write().await = true;
        (m, fake)
    }

    #[tokio::test]
    async fn set_duck_scales_configured_volume_and_restores() {
        let (m, fake) = manager(&["a.mp3"]).await;
        m.set_duck(0.25).await;
        assert_eq!(*fake.volume.lock().unwrap(), 0.25);
        m.set_duck(1.0).await;
        assert_eq!(*fake.volume.lock().unwrap(), 1.0);
        m.set_duck(7.0).await; // clamped
        assert_eq!(*fake.volume.lock().unwrap(), 1.0);
    }

    #[tokio::test]
    async fn tick_queues_next_before_current_ends() {
        let (m, fake) = manager(&["a.mp3", "b.mp3", "c.mp3"]).await;
        m.tick().await;
        assert_eq!(fake.names(), ["a.mp3", "b.mp3"]);
        // Idempotent while both are queued.
        m.tick().await;
        assert_eq!(fake.names(), ["a.mp3", "b.mp3"]);
        assert_eq!(m.current_track().await.as_deref(), Some("a.mp3"));
    }

    #[tokio::test]
    async fn tick_reports_track_change_and_refills() {
        let (m, fake) = manager(&["a.mp3", "b.mp3", "c.mp3"]).await;
        m.tick().await;
        let mut rx = m.event_bus.subscribe();
        fake.finish_current();
        m.tick().await;
        assert_eq!(m.current_track().await.as_deref(), Some("b.mp3"));
        assert_eq!(fake.names(), ["b.mp3", "c.mp3"]);
        assert!(matches!(rx.try_recv(), Ok(Event::MusicStarted { track_name }) if track_name == "b.mp3"));
    }

    #[tokio::test]
    async fn tick_wraps_and_single_track_loops() {
        let (m, fake) = manager(&["a.mp3"]).await;
        m.tick().await;
        assert_eq!(fake.names(), ["a.mp3", "a.mp3"]);
        fake.finish_current();
        m.tick().await;
        assert_eq!(fake.names(), ["a.mp3", "a.mp3"]);
    }

    #[tokio::test]
    async fn tick_recovers_from_fully_drained_queue() {
        let (m, fake) = manager(&["a.mp3", "b.mp3"]).await;
        fake.finish_current(); // late tick: queue ran dry
        m.tick().await;
        assert_eq!(fake.names(), ["b.mp3", "a.mp3"]);
        assert_eq!(m.current_track().await.as_deref(), Some("b.mp3"));
    }

    #[tokio::test]
    async fn tick_skips_broken_tracks_and_does_not_spin() {
        let (m, fake) = manager(&["a.mp3", "bad.mp3", "c.mp3"]).await;
        fake.fail.lock().unwrap().push(PathBuf::from("bad.mp3"));
        m.tick().await;
        assert_eq!(fake.names(), ["a.mp3", "c.mp3"]);
        // All broken: terminates.
        let (m, fake) = manager(&["a.mp3", "bad.mp3"]).await;
        fake.fail.lock().unwrap().push(PathBuf::from("a.mp3"));
        fake.fail.lock().unwrap().push(PathBuf::from("bad.mp3"));
        m.tick().await;
    }

    #[tokio::test]
    async fn tick_noop_when_not_playing() {
        let (m, fake) = manager(&["a.mp3", "b.mp3"]).await;
        *m.playing.write().await = false;
        m.tick().await;
        assert_eq!(fake.names(), ["a.mp3"]);
    }
}
