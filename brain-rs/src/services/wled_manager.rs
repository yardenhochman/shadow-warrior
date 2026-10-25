use anyhow::Result;
use reqwest::Client;
use serde_json::{json, Value};
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
    current_state: Arc<RwLock<ArenaState>>,
    effect_cache: RwLock<Vec<String>>,
    palette_cache: RwLock<Vec<String>>,
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
            current_state: Arc::new(RwLock::new(ArenaState::Idle)),
            effect_cache: RwLock::new(Vec::new()),
            palette_cache: RwLock::new(Vec::new()),
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

    /// Add a newly discovered controller, flash green for 2s, then apply current state effect.
    pub async fn add_controller(&self, ip: String) -> Result<()> {
        {
            let mut controllers = self.controllers.write().await;
            if controllers.iter().any(|c| c.ip == ip) {
                return Ok(());
            }
            controllers.push(WledController { ip: ip.clone(), num_leds: 150 });
        }
        self.rebuild_sockets().await?;

        // Flash solid green
        let green_payload = serde_json::json!({
            "on": true, "bri": 255, "transition": 0,
            "seg": [{ "id": 0, "fx": 0, "col": [[0, 255, 0]] }]
        });
        let url = format!("http://{}/json/state", ip);
        if let Err(e) = self.http_client.post(&url).json(&green_payload).send().await {
            warn!("Green flash failed for {}: {}", ip, e);
        } else {
            info!("Flashed green on new controller {}", ip);
        }

        // After 2s, apply current arena state effect to all controllers
        let http_client = self.http_client.clone();
        let controllers = self.controllers.clone();
        let config = self.config.clone();
        let current_state = self.current_state.clone();
        let udp_sockets = self.udp_sockets.clone();
        let realtime_task_handle = self.realtime_task_handle.clone();
        let event_bus = self.event_bus.clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(2)).await;
            // Re-apply state effect (rebuild a minimal WledManager-like context)
            let current = *current_state.read().await;
            let effect = match current {
                ArenaState::Suspended => {
                    let payload = serde_json::json!({ "on": false });
                    let ctls = controllers.read().await;
                    for c in ctls.iter() {
                        let url = format!("http://{}/json/state", c.ip);
                        let _ = http_client.post(&url).json(&payload).send().await;
                    }
                    return;
                }
                ArenaState::Idle => &config.effects.idle as &WledEffect,
                ArenaState::Warming => &config.effects.warming,
                ArenaState::Fight => &config.effects.fight,
                ArenaState::Cooldown => &config.effects.cooldown,
            };

            if effect.effect != "realtime" {
                let effect_id: u8 = match effect.effect.to_lowercase().as_str() {
                    "solid" => 0, "blink" => 1, "breathe" => 2, "wipe" => 3,
                    "scan" => 5, "rainbow" => 8, "twinkle" => 15, "fire" | "fireworks" => 16,
                    _ => 0,
                };
                let payload = serde_json::json!({
                    "on": true, "bri": 255, "transition": 7,
                    "seg": [{ "id": 0, "fx": effect_id, "sx": effect.speed.unwrap_or(128), "ix": 128 }]
                });
                let ctls = controllers.read().await;
                for c in ctls.iter() {
                    let url = format!("http://{}/json/state", c.ip);
                    let _ = http_client.post(&url).json(&payload).send().await;
                }
            }
            // realtime mode: existing task already sends frames to all sockets incl. new one
            drop(udp_sockets);
            drop(realtime_task_handle);
            drop(event_bus);
        });

        Ok(())
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

    /// Apply an arbitrary effect (stops realtime mode first). Used by the API.
    /// Returns the number of controllers that accepted the change.
    pub async fn apply_effect(&self, effect: &WledEffect) -> Result<usize> {
        if effect.effect.eq_ignore_ascii_case("realtime") {
            anyhow::bail!("realtime effect can only be set via arena state");
        }
        self.stop_realtime_mode().await;
        self.set_http_effect(effect).await
    }

    /// Effect names as reported by the first reachable controller
    /// (index == WLED effect id). Falls back to the cached list.
    pub async fn list_effects(&self) -> Result<Vec<String>> {
        self.fetch_names("effects", &self.effect_cache).await
    }

    /// Palette names as reported by the first reachable controller.
    pub async fn list_palettes(&self) -> Result<Vec<String>> {
        self.fetch_names("palettes", &self.palette_cache).await
    }

    async fn fetch_names(&self, path: &str, cache: &RwLock<Vec<String>>) -> Result<Vec<String>> {
        let ips: Vec<String> = self.controllers.read().await.iter().map(|c| c.ip.clone()).collect();
        for ip in &ips {
            match fetch_name_list(&self.http_client, ip, path).await {
                Ok(list) => {
                    *cache.write().await = list.clone();
                    return Ok(list);
                }
                Err(e) => warn!("Failed to fetch WLED {} from {}: {}", path, ip, e),
            }
        }
        let cached = cache.read().await.clone();
        if cached.is_empty() {
            anyhow::bail!("no WLED controller reachable and no cached {}", path);
        }
        Ok(cached)
    }

    /// Set effect via HTTP/JSON API. Returns number of controllers updated.
    async fn set_http_effect(&self, effect: &WledEffect) -> Result<usize> {
        // Resolve names against the device's list when we have one (cached or fetched).
        let effects = self.list_effects().await.unwrap_or_default();
        let palettes = match effect.palette {
            Some(_) => self.list_palettes().await.unwrap_or_default(),
            None => Vec::new(),
        };
        let effect_id = resolve_effect_id(&effect.effect, &effects);
        let palette_id = effect.palette.as_deref().map(|p| resolve_palette_id(p, &palettes));
        let payload = build_effect_payload(effect, effect_id, palette_id);

        let ips: Vec<String> = self.controllers.read().await.iter().map(|c| c.ip.clone()).collect();
        let mut ok = 0;
        for ip in &ips {
            match post_state(&self.http_client, ip, &payload).await {
                Ok(()) => {
                    info!("Set effect on {}: {}", ip, effect.effect);
                    ok += 1;
                }
                Err(e) => error!("Failed to set effect on {}: {}", ip, e),
            }
        }
        Ok(ok)
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

    /// Subscribe to StateChanged and WledConnected events.
    pub fn start_event_loop(self: Arc<Self>) {
        let mut event_rx = self.event_bus.subscribe();
        tokio::spawn(async move {
            loop {
                match event_rx.recv().await {
                    Ok(Event::StateChanged { to, .. }) => {
                        if let Ok(state) = to.parse::<ArenaState>() {
                            *self.current_state.write().await = state;
                            if let Err(e) = self.set_state_effect(state).await {
                                error!("WLED set_state_effect error: {}", e);
                            }
                        }
                    }
                    Ok(Event::WledConnected { ip }) => {
                        if let Err(e) = self.add_controller(ip).await {
                            error!("WLED add_controller error: {}", e);
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

/// Static fallback for common effects when the device list is unavailable.
fn static_effect_id(name: &str) -> Option<u8> {
    Some(match name {
        "solid" => 0,
        "blink" => 1,
        "breathe" => 2,
        "wipe" => 3,
        "scan" => 5,
        "rainbow" => 8,
        "twinkle" => 15,
        "fire" | "fireworks" => 16,
        _ => return None,
    })
}

fn static_palette_id(name: &str) -> Option<u8> {
    Some(match name {
        "default" => 0,
        "random" => 5,
        "rainbow" => 9,
        "party" => 12,
        "ocean" => 20,
        "lava" => 22,
        "forest" => 25,
        _ => return None,
    })
}

/// Resolve by numeric id, then device list (case-insensitive), then static table.
/// Unknown names fall back to 0 (solid / default) with a warning.
fn resolve_id(name: &str, device_list: &[String], fallback: fn(&str) -> Option<u8>) -> u8 {
    let name = name.trim();
    if let Ok(id) = name.parse::<u8>() {
        return id;
    }
    let lower = name.to_lowercase();
    if let Some(i) = device_list.iter().position(|n| n.to_lowercase() == lower) {
        return u8::try_from(i).unwrap_or(0);
    }
    fallback(&lower).unwrap_or_else(|| {
        warn!("Unknown WLED effect/palette '{}', using 0", name);
        0
    })
}

fn resolve_effect_id(name: &str, device_list: &[String]) -> u8 {
    resolve_id(name, device_list, static_effect_id)
}

fn resolve_palette_id(name: &str, device_list: &[String]) -> u8 {
    resolve_id(name, device_list, static_palette_id)
}

/// Build the `/json/state` payload for an effect. Unset fields use the
/// previous hardcoded defaults (bri 255, ix 128, transition 7, sx 128).
fn build_effect_payload(effect: &WledEffect, effect_id: u8, palette_id: Option<u8>) -> Value {
    let mut seg = json!({
        "id": 0,
        "fx": effect_id,
        "sx": effect.speed.unwrap_or(128),
        "ix": effect.intensity.unwrap_or(128),
        "pal": palette_id.unwrap_or(0),
    });
    if let Some(colors) = &effect.colors {
        seg["col"] = json!(colors.iter().take(3).collect::<Vec<_>>());
    }
    json!({
        "on": true,
        "bri": effect.brightness.unwrap_or(255),
        "transition": effect.transition.unwrap_or(7),
        "seg": [seg],
    })
}

async fn post_state(client: &Client, ip: &str, payload: &Value) -> Result<()> {
    let resp = client
        .post(format!("http://{}/json/state", ip))
        .json(payload)
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("status {}", resp.status());
    }
    Ok(())
}

/// GET http://ip/json/{effects|palettes} -> JSON array of names.
async fn fetch_name_list(client: &Client, ip: &str, path: &str) -> Result<Vec<String>> {
    let resp = client.get(format!("http://{}/json/{}", ip, path)).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("status {}", resp.status());
    }
    Ok(resp.json::<Vec<String>>().await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn resolves_numeric_device_and_static() {
        let list = names(&["Solid", "Blink", "Colorloop"]);
        assert_eq!(resolve_effect_id("42", &list), 42);
        assert_eq!(resolve_effect_id("colorloop", &list), 2);
        assert_eq!(resolve_effect_id("Fire", &[]), 16);
        assert_eq!(resolve_effect_id("nonsense", &list), 0);
        assert_eq!(resolve_palette_id("Lava", &[]), 22);
    }

    #[test]
    fn payload_defaults_match_legacy() {
        let e = WledEffect { effect: "fire".into(), ..Default::default() };
        let p = build_effect_payload(&e, 16, None);
        assert_eq!(p["bri"], 255);
        assert_eq!(p["transition"], 7);
        assert_eq!(p["seg"][0]["fx"], 16);
        assert_eq!(p["seg"][0]["sx"], 128);
        assert_eq!(p["seg"][0]["ix"], 128);
        assert!(p["seg"][0].get("col").is_none());
    }

    #[test]
    fn payload_uses_all_fields() {
        let e = WledEffect {
            effect: "x".into(),
            palette: Some("y".into()),
            speed: Some(10),
            intensity: Some(20),
            brightness: Some(30),
            transition: Some(0),
            colors: Some(vec![[255, 0, 0], [0, 255, 0], [0, 0, 255], [1, 1, 1]]),
        };
        let p = build_effect_payload(&e, 7, Some(3));
        assert_eq!(p["bri"], 30);
        assert_eq!(p["transition"], 0);
        assert_eq!(p["seg"][0]["sx"], 10);
        assert_eq!(p["seg"][0]["ix"], 20);
        assert_eq!(p["seg"][0]["pal"], 3);
        assert_eq!(p["seg"][0]["col"], json!([[255, 0, 0], [0, 255, 0], [0, 0, 255]]));
    }

    #[test]
    fn effect_config_parses_minimal_and_full() {
        let e: WledEffect = serde_yaml::from_str("effect: realtime").unwrap();
        assert_eq!(e.effect, "realtime");
        assert!(e.intensity.is_none());
        let e: WledEffect = serde_yaml::from_str(
            "effect: '66'\npalette: ocean\nspeed: 5\nintensity: 6\nbrightness: 7\ntransition: 8\ncolors: [[1,2,3]]",
        )
        .unwrap();
        assert_eq!(e.intensity, Some(6));
        assert_eq!(e.colors, Some(vec![[1, 2, 3]]));
    }

    /// Minimal HTTP mock (one request per connection); returns (addr, captured requests).
    async fn mock_server(
        status: &'static str,
        body: &'static str,
    ) -> (String, Arc<tokio::sync::Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let seen = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 8192];
                let mut total = 0;
                loop {
                    let n = sock.read(&mut buf[total..]).await.unwrap();
                    total += n;
                    let text = String::from_utf8_lossy(&buf[..total]).to_string();
                    if let Some(h) = text.find("\r\n\r\n") {
                        let len = text
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if total >= h + 4 + len || n == 0 {
                            break;
                        }
                    } else if n == 0 {
                        break;
                    }
                }
                seen2.lock().await.push(String::from_utf8_lossy(&buf[..total]).to_string());
                let resp = format!(
                    "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    body.len(),
                    body
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            }
        });
        (addr, seen)
    }

    #[tokio::test]
    async fn fetches_effect_list_from_device() {
        let (addr, seen) = mock_server("200 OK", r#"["Solid","Blink","Breathe"]"#).await;
        let list = fetch_name_list(&Client::new(), &addr, "effects").await.unwrap();
        assert_eq!(list, names(&["Solid", "Blink", "Breathe"]));
        assert!(seen.lock().await[0].starts_with("GET /json/effects"));
    }

    #[tokio::test]
    async fn posts_state_and_reports_http_errors() {
        let (addr, seen) = mock_server("200 OK", "{}").await;
        let e = WledEffect { effect: "fire".into(), speed: Some(99), ..Default::default() };
        let payload = build_effect_payload(&e, 16, None);
        post_state(&Client::new(), &addr, &payload).await.unwrap();
        let req = seen.lock().await[0].clone();
        assert!(req.starts_with("POST /json/state"));
        assert!(req.contains("\"sx\":99"));

        let (bad, _) = mock_server("500 Internal Server Error", "{}").await;
        assert!(post_state(&Client::new(), &bad, &payload).await.is_err());
    }
}
