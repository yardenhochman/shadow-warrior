use anyhow::Result;
use reqwest::Client;
use serde_json::json;
use std::net::UdpSocket;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

use crate::config::{WledConfig, WledEffect, WledController};
use crate::core::{ArenaState, Event, SharedEventBus};
use crate::services::database_manager::SharedDatabaseManager;
use crate::models::WledStatus;

pub struct WledManager {
    config: WledConfig,
    db: SharedDatabaseManager,
    event_bus: SharedEventBus,
    http_client: Client,
    controllers: Arc<RwLock<Vec<WledController>>>,
    udp_sockets: Arc<RwLock<Vec<UdpSocket>>>,
    realtime_task_handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
}

impl WledManager {
    pub fn new(config: WledConfig, db: SharedDatabaseManager, event_bus: SharedEventBus) -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            config,
            db,
            event_bus,
            http_client,
            controllers: Arc::new(RwLock::new(Vec::new())),
            udp_sockets: Arc::new(RwLock::new(Vec::new())),
            realtime_task_handle: Arc::new(RwLock::new(None)),
        }
    }

    /// Initialize UDP sockets for realtime control
    pub async fn start(&self) -> Result<()> {
        let mut loaded_controllers = self.config.controllers.clone();
        
        // Add controllers from database
        if let Ok(db_devices) = self.db.get_devices().await {
            for dev in db_devices {
                if dev.device_type == "wled" {
                    // Check if already exists from config
                    if !loaded_controllers.iter().any(|c| c.ip == dev.host) {
                        loaded_controllers.push(WledController {
                            ip: dev.host.clone(),
                            num_leds: 150, // Default
                        });
                    }
                }
            }
        }
        
        info!("Starting WLED manager with {} controllers", loaded_controllers.len());
        
        *self.controllers.write().await = loaded_controllers.clone();

        // Create UDP sockets for each controller
        let mut sockets = Vec::new();
        for controller in &loaded_controllers {
            match UdpSocket::bind("0.0.0.0:0") {
                Ok(socket) => {
                    socket.set_nonblocking(true)?;
                    let addr = format!("{}:21324", controller.ip);
                    socket.connect(&addr)?;
                    info!("UDP socket connected to {}", addr);
                    sockets.push(socket);
                }
                Err(e) => {
                    error!("Failed to create UDP socket for {}: {}", controller.ip, e);
                }
            }
        }

        *self.udp_sockets.write().await = sockets;

        Ok(())
    }
    
    pub async fn get_status(&self) -> Vec<WledStatus> {
        let controllers = self.controllers.read().await;
        let socket_count = self.udp_sockets.read().await.len();
        controllers
            .iter()
            .enumerate()
            .map(|(i, c)| WledStatus {
                ip: c.ip.clone(),
                connected: i < socket_count,
                current_effect: None,
                last_error: None,
            })
            .collect()
    }

    /// Remove a controller by IP from the runtime list and rebuild UDP sockets.
    pub async fn remove_controller(&self, ip: &str) -> Result<()> {
        {
            let mut controllers = self.controllers.write().await;
            controllers.retain(|c| c.ip != ip);
        }
        self.rebuild_sockets().await
    }

    async fn rebuild_sockets(&self) -> Result<()> {
        let controllers = self.controllers.read().await;
        let mut sockets = Vec::new();
        for controller in controllers.iter() {
            match std::net::UdpSocket::bind("0.0.0.0:0") {
                Ok(socket) => {
                    socket.set_nonblocking(true)?;
                    let addr = format!("{}:21324", controller.ip);
                    if let Ok(()) = socket.connect(&addr) {
                        sockets.push(socket);
                    }
                }
                Err(e) => {
                    error!("Failed to create UDP socket for {}: {}", controller.ip, e);
                }
            }
        }
        *self.udp_sockets.write().await = sockets;
        Ok(())
    }

    /// Set effect for a specific state
    pub async fn set_state_effect(&self, state: ArenaState) -> Result<()> {
        let effect = match state {
            ArenaState::Suspended => {
                // Turn off all LEDs
                self.set_power(false).await?;
                return Ok(());
            }
            ArenaState::Idle => &self.config.effects.idle,
            ArenaState::Warming => &self.config.effects.warming,
            ArenaState::Fight => &self.config.effects.fight,
            ArenaState::Cooldown => &self.config.effects.cooldown,
        };

        // Check if we need realtime mode
        if effect.effect == "realtime" {
            self.start_realtime_mode(state).await?;
        } else {
            // Stop realtime if running
            self.stop_realtime_mode().await;
            // Set HTTP effect
            self.set_http_effect(effect).await?;
        }

        Ok(())
    }

    /// Set effect via HTTP/JSON API
    async fn set_http_effect(&self, effect: &WledEffect) -> Result<()> {
        // Convert effect name to ID (simplified mapping)
        let effect_id = self.effect_name_to_id(&effect.effect);
        let palette_id = effect.palette.as_ref().map(|p| self.palette_name_to_id(p));

        let payload = json!({
            "on": true,
            "bri": 255,
            "transition": 7,
            "seg": [{
                "id": 0,
                "fx": effect_id,
                "sx": effect.speed.unwrap_or(128),
                "ix": 128,
                "pal": palette_id.unwrap_or(0),
            }]
        });

        let controllers = self.controllers.read().await;
        for controller in controllers.iter() {
            let url = format!("http://{}/json/state", controller.ip);
            match self.http_client.post(&url).json(&payload).send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        info!("Set effect on {}: {}", controller.ip, effect.effect);
                    } else {
                        warn!("Failed to set effect on {}: status {}", controller.ip, resp.status());
                    }
                }
                Err(e) => {
                    error!("HTTP error for {}: {}", controller.ip, e);
                }
            }
        }

        Ok(())
    }

    /// Set power state
    async fn set_power(&self, on: bool) -> Result<()> {
        let payload = json!({ "on": on });

        let controllers = self.controllers.read().await;
        for controller in controllers.iter() {
            let url = format!("http://{}/json/state", controller.ip);
            match self.http_client.post(&url).json(&payload).send().await {
                Ok(_) => {
                    info!("Set power {} on {}", if on { "on" } else { "off" }, controller.ip);
                }
                Err(e) => {
                    error!("Failed to set power on {}: {}", controller.ip, e);
                }
            }
        }

        Ok(())
    }

    /// Start realtime UDP mode
    async fn start_realtime_mode(&self, state: ArenaState) -> Result<()> {
        // Stop any existing realtime task
        self.stop_realtime_mode().await;

        info!("Starting realtime mode for state: {}", state);

        let sockets = self.udp_sockets.clone();
        let fps = self.config.udp_fps;
        
        let controllers = self.controllers.read().await;
        let num_leds = controllers.first().map(|c| c.num_leds).unwrap_or(150);

        let handle = tokio::spawn(async move {
            let mut ticker = interval(Duration::from_millis(1000 / fps as u64));

            loop {
                ticker.tick().await;

                // Generate frame based on state
                let frame = match state {
                    ArenaState::Warming => Self::generate_warming_frame(num_leds),
                    ArenaState::Fight => Self::generate_fight_frame(num_leds),
                    _ => vec![0u8; num_leds * 3], // Black for others
                };

                // Send to all controllers
                let sockets_guard = sockets.read().await;
                for socket in sockets_guard.iter() {
                    if let Err(e) = socket.send(&frame) {
                        if e.kind() != std::io::ErrorKind::WouldBlock {
                            debug!("UDP send error: {}", e);
                        }
                    }
                }
            }
        });

        *self.realtime_task_handle.write().await = Some(handle);

        Ok(())
    }

    /// Stop realtime mode
    async fn stop_realtime_mode(&self) {
        if let Some(handle) = self.realtime_task_handle.write().await.take() {
            handle.abort();
            info!("Stopped realtime mode");
        }
    }

    /// Generate warming effect frame (rainbow chase that speeds up with energy)
    fn generate_warming_frame(num_leds: usize) -> Vec<u8> {
        // WARLS protocol: [protocol_id, timeout, r, g, b, r, g, b, ...]
        let mut frame = vec![1u8, 1]; // Protocol 1, timeout 1 second

        // Simple rainbow effect
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as f32;

        for i in 0..num_leds {
            let hue = (i as f32 / num_leds as f32 * 360.0 + time * 0.1) % 360.0;
            let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
            frame.extend_from_slice(&[r, g, b]);
        }

        frame
    }

    /// Generate fight effect frame (reactive pulses)
    fn generate_fight_frame(num_leds: usize) -> Vec<u8> {
        // WARLS protocol
        let mut frame = vec![1u8, 1];

        // Pulsing red effect
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as f32;

        let brightness = ((time * 0.005).sin() * 0.5 + 0.5) * 255.0;

        for _ in 0..num_leds {
            frame.extend_from_slice(&[brightness as u8, 0, 0]);
        }

        frame
    }

    /// Convert effect name to WLED effect ID
    fn effect_name_to_id(&self, name: &str) -> u8 {
        match name.to_lowercase().as_str() {
            "solid" => 0,
            "blink" => 1,
            "breathe" => 2,
            "wipe" => 3,
            "scan" => 5,
            "rainbow" => 8,
            "twinkle" => 15,
            "fire" | "fireworks" => 16,
            _ => 0,
        }
    }

    /// Convert palette name to WLED palette ID
    fn palette_name_to_id(&self, name: &str) -> u8 {
        match name.to_lowercase().as_str() {
            "default" => 0,
            "random" => 5,
            "rainbow" => 9,
            "party" => 12,
            "ocean" => 20,
            "lava" => 22,
            "forest" => 25,
            _ => 0,
        }
    }

    /// Subscribe to StateChanged events and apply LED effects autonomously.
    pub fn start_event_loop(self: Arc<Self>) {
        let mut event_rx = self.event_bus.subscribe();
        tokio::spawn(async move {
            loop {
                match event_rx.recv().await {
                    Ok(Event::StateChanged { to, .. }) => {
                        if let Ok(state) = to.parse::<ArenaState>() {
                            if let Err(e) = self.set_state_effect(state).await {
                                error!("WLED set_state_effect error: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        warn!("WLED event bus error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
        });
    }
}

/// Convert HSV to RGB
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;

    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}
