use anyhow::{Context, Result};
use rand::seq::SliceRandom;
use rand::thread_rng;
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

        let playlist = self.playlist.read().await;
        if playlist.is_empty() {
            warn!("Playlist is empty, cannot play");
            return Ok(());
        }

        // Start from the beginning of the playlist
        self.cursor.write().await.reset();
        let track_path = playlist[0].clone();
        drop(playlist); // Release lock

        let track_name = track_name(&track_path);

        // Initialize audio output and immediately leak it (before any await)
        // Initialize audio output using build container's native drivers
        let device_sink = DeviceSinkBuilder::open_default_sink()
            .context("Failed to create audio output")?;
        let player = Player::connect_new(device_sink.mixer());
        player.set_volume(self.config.volume);

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
    use super::*;

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
