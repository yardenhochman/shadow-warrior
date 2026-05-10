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

pub struct MusicManager {
    config: MusicConfig,
    event_bus: SharedEventBus,
    playlist: Arc<RwLock<Vec<PathBuf>>>,
    current_track: Arc<RwLock<Option<String>>>,
    sink: Arc<RwLock<Option<Player>>>,
    playing: Arc<RwLock<bool>>,
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

        // Pick first track
        let track_path = playlist.first().unwrap().clone();
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
