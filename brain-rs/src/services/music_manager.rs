use anyhow::{Context, Result};
use rand::seq::SliceRandom;
use rand::thread_rng;
use rodio::{Decoder, Player, DeviceSinkBuilder};
use std::fs::{self, File};
use std::io::BufReader;
use std::path::PathBuf;
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

/// Pure decision: should the next track be queued? True when we believe we are
/// playing but the audio queue has drained (current track finished).
pub fn should_advance(playing: bool, queue_empty: bool) -> bool {
    playing && queue_empty
}

pub struct MusicManager {
    config: MusicConfig,
    event_bus: SharedEventBus,
    playlist: Arc<RwLock<Vec<PathBuf>>>,
    current_track: Arc<RwLock<Option<String>>>,
    sink: Arc<RwLock<Option<Player>>>,
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

        let track_name = track_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();

        // Initialize audio output and immediately leak it (before any await)
        // Initialize audio output using build container's native drivers
        let device_sink = DeviceSinkBuilder::open_default_sink()
            .context("Failed to create audio output")?;
        let player = Player::connect_new(device_sink.mixer());
        player.set_volume(self.config.volume);

        // Leak the device_sink to keep the audio stream alive
        Box::leak(Box::new(device_sink));

        // Load and play track
        match self.load_track(&track_path, &player) {
            Ok(()) => {
                info!("Playing: {}", track_name);

                *self.sink.write().await = Some(player);
                *self.current_track.write().await = Some(track_name.clone());
                *self.playing.write().await = true;

                self.event_bus
                    .publish(Event::MusicStarted { track_name });

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

    /// Check whether the current track finished and, if so, queue the next one
    /// (wrapping around the playlist). Call periodically; see `start_advance_loop`.
    pub async fn tick(&self) {
        let queue_empty = match self.sink.read().await.as_ref() {
            Some(p) => p.empty(),
            None => return,
        };
        if !should_advance(*self.playing.read().await, queue_empty) {
            return;
        }

        let playlist = self.playlist.read().await.clone();
        let len = playlist.len();
        // Try each track at most once so a run of broken files cannot spin forever.
        for _ in 0..len {
            let idx = match self.cursor.write().await.advance(len) {
                Some(i) => i,
                None => return,
            };
            let path = &playlist[idx];
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown")
                .to_string();
            let result = match self.sink.read().await.as_ref() {
                Some(player) => self.load_track(path, player),
                None => return,
            };
            match result {
                Ok(()) => {
                    info!("Advancing to: {}", name);
                    *self.current_track.write().await = Some(name.clone());
                    self.event_bus.publish(Event::MusicStarted { track_name: name });
                    return;
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

    /// Spawn a background loop that advances tracks automatically.
    pub fn start_advance_loop(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
            loop {
                interval.tick().await;
                self.tick().await;
            }
        });
    }

    /// Load and decode a track
    fn load_track(&self, path: &PathBuf, player: &Player) -> Result<()> {
        let file = File::open(path)?;
        let source = Decoder::new(BufReader::new(file))?;
        player.append(source);
        Ok(())
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
        *self.playing.write().await = false;

        self.event_bus.publish(Event::MusicStopped);
    }

    /// Check if music is playing
    #[allow(dead_code)]
    pub async fn is_playing(&self) -> bool {
        *self.playing.read().await
    }

    /// Get current track name
    #[allow(dead_code)]
    pub async fn current_track(&self) -> Option<String> {
        self.current_track.read().await.clone()
    }

    /// Get playlist size
    #[allow(dead_code)]
    pub async fn playlist_size(&self) -> usize {
        self.playlist.read().await.len()
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
    fn advance_decision() {
        assert!(should_advance(true, true));
        assert!(!should_advance(true, false));
        assert!(!should_advance(false, true));
        assert!(!should_advance(false, false));
    }
}
