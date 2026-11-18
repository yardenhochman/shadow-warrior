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

mod health;
mod protocol;
use health::LinkHealth;

/// Idle liveness poll period; well under `server.health_stale_sec`.
const HEALTH_POLL_INTERVAL: Duration = Duration::from_secs(30);
use protocol::{build_effect_payload, fetch_name_list, post_state, resolve_effect_id, resolve_palette_id};

pub struct WledManager {
    config: WledConfig,
    db: SharedDatabaseManager,
    event_bus: SharedEventBus,
    http_client: Client,
    controllers: Arc<RwLock<Vec<WledController>>>,
    /// One connected socket per reachable controller, with that controller's LED count.
    udp_sockets: Arc<RwLock<Vec<(UdpSocket, usize)>>>,
    realtime_task_handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
    current_state: Arc<RwLock<ArenaState>>,
    effect_cache: RwLock<Vec<String>>,
    palette_cache: RwLock<Vec<String>>,
    /// Publishes disconnect/error/reconnect events from call outcomes.
    link: Arc<LinkHealth>,
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
            event_bus: event_bus.clone(),
            http_client,
            controllers: Arc::new(RwLock::new(Vec::new())),
            udp_sockets: Arc::new(RwLock::new(Vec::new())),
            realtime_task_handle: Arc::new(RwLock::new(None)),
            current_state: Arc::new(RwLock::new(ArenaState::Idle)),
            effect_cache: RwLock::new(Vec::new()),
            palette_cache: RwLock::new(Vec::new()),
            link: Arc::new(LinkHealth::new(event_bus.clone())),
        }
    }

    #[cfg(test)]
    pub(crate) async fn set_controllers_for_test(&self, ips: &[&str]) {
        *self.controllers.write().await = ips
            .iter()
            .map(|ip| WledController { ip: ip.to_string(), num_leds: 10 })
            .collect();
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

        // Offline-tolerant: an unreachable/unresolvable controller is skipped
        // (and retried by the health poll / discovery), never fatal at startup.
        self.rebuild_sockets().await
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
    pub async fn add_controller(self: &Arc<Self>, ip: String) -> Result<()> {
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
            self.link.fail(&ip, &e);
            warn!("Green flash failed for {}: {}", ip, e);
        } else {
            self.link.ok(&ip);
            info!("Flashed green on new controller {}", ip);
        }

        // After 2s, put the current arena state's effect back on all controllers.
        let this = Arc::clone(self);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(2)).await;
            this.reapply_state_effect().await;
        });

        Ok(())
    }

    /// Re-send the current arena state's effect to all controllers, through the same
    /// name resolution as every other effect change. Realtime states are left alone: the
    /// running realtime task already feeds every socket, including a new one.
    async fn reapply_state_effect(&self) {
        let state = *self.current_state.read().await;
        let is_realtime = match state {
            ArenaState::Suspended => false,
            ArenaState::Idle => self.config.effects.idle.effect == "realtime",
            ArenaState::Warming => self.config.effects.warming.effect == "realtime",
            ArenaState::Fight => self.config.effects.fight.effect == "realtime",
            ArenaState::Cooldown => self.config.effects.cooldown.effect == "realtime",
        };
        if is_realtime {
            return;
        }
        if let Err(e) = self.set_state_effect(state).await {
            warn!("WLED re-apply of {state} effect failed: {e}");
        }
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
                    if let Err(e) = socket.set_nonblocking(true) {
                        warn!("UDP socket for {} not usable: {}", controller.ip, e);
                        continue;
                    }
                    let addr = realtime_addr(&controller.ip);
                    match socket.connect(&addr) {
                        Ok(()) => {
                            info!("UDP socket connected to {}", addr);
                            sockets.push((socket, controller.num_leds));
                        }
                        Err(e) => warn!("UDP connect to {} failed (offline?): {}", addr, e),
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

    /// Fill the effect and palette caches from the first reachable controller.
    /// Never fails: unreachable devices are logged and the caches stay empty,
    /// so the next list/apply call retries. Returns (effects_ok, palettes_ok).
    pub async fn warm_caches(&self) -> (bool, bool) {
        let effects = self.list_effects().await.map(|l| l.len());
        let palettes = self.list_palettes().await.map(|l| l.len());
        match &effects {
            Ok(n) => info!("WLED effect cache warmed ({} effects)", n),
            Err(e) => warn!("WLED effect cache warm-up skipped: {}", e),
        }
        match &palettes {
            Ok(n) => info!("WLED palette cache warmed ({} palettes)", n),
            Err(e) => warn!("WLED palette cache warm-up skipped: {}", e),
        }
        (effects.is_ok(), palettes.is_ok())
    }

    /// Warm the caches in the background without blocking startup.
    pub fn spawn_cache_warmup(self: Arc<Self>) {
        tokio::spawn(async move {
            self.warm_caches().await;
        });
    }

    /// Cached list if present, otherwise try the device (empty on failure).
    async fn cached_or_fetch(&self, path: &str, cache: &RwLock<Vec<String>>) -> Vec<String> {
        let cached = cache.read().await.clone();
        if !cached.is_empty() {
            return cached;
        }
        self.fetch_names(path, cache).await.unwrap_or_default()
    }

    async fn fetch_names(&self, path: &str, cache: &RwLock<Vec<String>>) -> Result<Vec<String>> {
        let ips: Vec<String> = self.controllers.read().await.iter().map(|c| c.ip.clone()).collect();
        for ip in &ips {
            match fetch_name_list(&self.http_client, ip, path).await {
                Ok(list) => {
                    self.link.ok(ip);
                    *cache.write().await = list.clone();
                    return Ok(list);
                }
                Err(e) => {
                    self.link.fail(ip, &e);
                    warn!("Failed to fetch WLED {} from {}: {}", path, ip, e)
                }
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
        let effects = self.cached_or_fetch("effects", &self.effect_cache).await;
        let palettes = match effect.palette {
            Some(_) => self.cached_or_fetch("palettes", &self.palette_cache).await,
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
                    self.link.ok(ip);
                    info!("Set effect on {}: {}", ip, effect.effect);
                    ok += 1;
                }
                Err(e) => {
                    self.link.fail(ip, &e);
                    error!("Failed to set effect on {}: {}", ip, e)
                }
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
                Ok(resp) if resp.status().is_success() => {
                    self.link.ok(&controller.ip);
                    info!("Set power {} on {}", if on { "on" } else { "off" }, controller.ip);
                }
                Ok(resp) => {
                    self.link.fail(&controller.ip, format!("status {}", resp.status()));
                    error!("Set power on {} returned {}", controller.ip, resp.status());
                }
                Err(e) => {
                    self.link.fail(&controller.ip, &e);
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
        let link = self.link.clone();
        let fps = self.config.udp_fps;
        
        let handle = tokio::spawn(async move {
            let mut ticker = interval(Duration::from_millis(1000 / fps as u64));
            // UDP send errors are reported at most once per second per controller.
            let mut last_report: std::collections::HashMap<std::net::IpAddr, std::time::Instant> =
                std::collections::HashMap::new();

            loop {
                ticker.tick().await;

                // Send to all controllers, each frame sized to that controller's strip
                let sockets_guard = sockets.read().await;
                for (socket, num_leds) in sockets_guard.iter() {
                    let rgb = match state {
                        ArenaState::Warming => Self::generate_warming_frame(*num_leds),
                        ArenaState::Fight => Self::generate_fight_frame(*num_leds),
                        _ => vec![0u8; num_leds * 3], // Black for others
                    };
                    for frame in encode_realtime_frames(&rgb, REALTIME_TIMEOUT_SEC) {
                        let Err(e) = socket.send(&frame) else { continue };
                        if e.kind() == std::io::ErrorKind::WouldBlock {
                            continue;
                        }
                        debug!("UDP send error: {}", e);
                        if let Ok(peer) = socket.peer_addr() {
                            let due = last_report
                                .get(&peer.ip())
                                .is_none_or(|t| t.elapsed() >= Duration::from_secs(1));
                            if due {
                                last_report.insert(peer.ip(), std::time::Instant::now());
                                link.fail(&peer.ip().to_string(), format!("udp: {e}"));
                            }
                        }
                        break;
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

    /// Generate warming effect pixels, `r,g,b` per LED (rainbow chase that speeds up with energy)
    fn generate_warming_frame(num_leds: usize) -> Vec<u8> {
        let mut frame = Vec::with_capacity(num_leds * 3);

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

    /// Generate fight effect pixels, `r,g,b` per LED (reactive pulses)
    fn generate_fight_frame(num_leds: usize) -> Vec<u8> {
        let mut frame = Vec::with_capacity(num_leds * 3);

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

    /// Probe every controller once with `GET /json/info`, feeding the link
    /// health tracker. WLED has no push channel, so without this an idle
    /// controller (no effect changes) would only ever look stale.
    pub async fn poll_health_once(&self) {
        let ips: Vec<String> = self.controllers.read().await.iter().map(|c| c.ip.clone()).collect();
        for ip in ips {
            let url = format!("http://{ip}/json/info");
            match self.http_client.get(&url).send().await.and_then(|r| r.error_for_status()) {
                Ok(resp) => {
                    self.link.ok(&ip);
                    // The device knows its real strip length; config/discovery only guess.
                    if let Some(n) = resp
                        .json::<serde_json::Value>()
                        .await
                        .ok()
                        .and_then(|v| v["leds"]["count"].as_u64())
                        .filter(|n| *n > 0)
                    {
                        self.update_led_count(&ip, n as usize).await;
                    }
                }
                Err(e) => self.link.fail(&ip, &e),
            }
        }
    }

    /// Adopt the LED count the device reports and resize the realtime frames.
    async fn update_led_count(&self, ip: &str, count: usize) {
        let changed = {
            let mut controllers = self.controllers.write().await;
            match controllers.iter_mut().find(|c| c.ip == ip) {
                Some(c) if c.num_leds != count => {
                    info!("WLED {ip} reports {count} LEDs (was {})", c.num_leds);
                    c.num_leds = count;
                    true
                }
                _ => false,
            }
        };
        if changed {
            if let Err(e) = self.rebuild_sockets().await {
                warn!("WLED socket rebuild after LED count change failed: {e}");
            }
        }
    }

    /// Run [`Self::poll_health_once`] forever every `HEALTH_POLL_INTERVAL`.
    pub fn spawn_health_poll(self: Arc<Self>) {
        tokio::spawn(async move {
            loop {
                // First poll right away: learns the real LED counts at startup.
                self.poll_health_once().await;
                tokio::time::sleep(HEALTH_POLL_INTERVAL).await;
            }
        });
    }

    /// Rebuild UDP sockets if some controller has none (it was unreachable or
    /// unresolvable at startup and is reachable now).
    async fn ensure_sockets(&self) {
        let missing = self.udp_sockets.read().await.len() < self.controllers.read().await.len();
        if missing {
            if let Err(e) = self.rebuild_sockets().await {
                warn!("WLED socket rebuild failed: {}", e);
            }
        }
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
                        self.ensure_sockets().await;
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

/// WLED's UDP realtime port.
const REALTIME_PORT: u16 = 21324;

/// UDP realtime target for a controller address. `ip` may carry an HTTP port
/// (`"192.168.1.5:8090"`, e.g. the browser simulator's bridge standing in for WLED);
/// the HTTP calls use it as given and realtime goes to that host's port 21324.
fn realtime_addr(ip: &str) -> String {
    let host = match ip.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') && port.parse::<u16>().is_ok() => host,
        _ => ip,
    };
    format!("{host}:{REALTIME_PORT}")
}

/// Seconds WLED stays in realtime mode after the last frame before it resumes its own effect.
const REALTIME_TIMEOUT_SEC: u8 = 2;
/// DRGB carries at most 490 LEDs per packet; DNRGB at most 489 per packet.
const DRGB_MAX_LEDS: usize = 490;
const DNRGB_MAX_LEDS: usize = 489;

/// Wrap `r,g,b`-per-LED pixels in WLED UDP realtime packets: one DRGB (protocol 2)
/// packet up to 490 LEDs, otherwise DNRGB (protocol 4) chunks with a start index.
/// (Protocol 1, WARLS, wants `index,r,g,b` per LED and garbles a plain RGB layout.)
fn encode_realtime_frames(rgb: &[u8], timeout_sec: u8) -> Vec<Vec<u8>> {
    let leds = rgb.len() / 3;
    if leds <= DRGB_MAX_LEDS {
        let mut frame = Vec::with_capacity(2 + leds * 3);
        frame.extend_from_slice(&[2, timeout_sec]);
        frame.extend_from_slice(&rgb[..leds * 3]);
        return vec![frame];
    }
    rgb[..leds * 3]
        .chunks(DNRGB_MAX_LEDS * 3)
        .enumerate()
        .map(|(i, chunk)| {
            let start = (i * DNRGB_MAX_LEDS) as u16;
            let mut frame = Vec::with_capacity(4 + chunk.len());
            frame.extend_from_slice(&[4, timeout_sec]);
            frame.extend_from_slice(&start.to_be_bytes());
            frame.extend_from_slice(chunk);
            frame
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::core::status_registry::StatusRegistry;
    use crate::core::EventBus;
    use crate::services::database_manager::DatabaseManager;
    use crate::test_util::{dead_addr, mock_http};

    async fn wait_connected(reg: &StatusRegistry, addr: &str, want: bool) -> bool {
        for _ in 0..100 {
            if reg.snapshot().wled.get(addr).is_some_and(|d| d.connected == want) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        false
    }

    /// Failing controller -> disconnected after N failures; device coming back
    /// -> reconnected. Registry fed from the real bus.
    #[tokio::test]
    async fn http_failures_disconnect_and_success_reconnects() {
        let config = Config::default();
        let db = Arc::new(DatabaseManager::new("sqlite::memory:").await.unwrap());
        let bus: SharedEventBus = Arc::new(EventBus::new(100));
        let reg = Arc::new(StatusRegistry::new());
        reg.spawn(&bus);
        tokio::task::yield_now().await;
        let wled = WledManager::new(config.wled.clone(), db, bus.clone());

        let (addr, routes, _) = mock_http(&[("*", 200, "{}"), ("GET /json/effects", 200, r#"["Solid"]"#)]).await;
        wled.set_controllers_for_test(&[&addr]).await;
        let fx = WledEffect { effect: "solid".into(), ..Default::default() };

        assert_eq!(wled.apply_effect(&fx).await.unwrap(), 1);
        assert!(wait_connected(&reg, &addr, true).await);

        // device starts failing (HTTP 500): below the threshold stays connected
        routes.lock().await.insert("*".into(), (500, "{}".into()));
        for _ in 0..health::FAIL_THRESHOLD - 1 {
            wled.apply_effect(&fx).await.unwrap();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(reg.snapshot().wled[&addr].connected);
        wled.apply_effect(&fx).await.unwrap();
        assert!(wait_connected(&reg, &addr, false).await);
        let d = reg.snapshot().wled[&addr].clone();
        assert!(d.error_count >= health::FAIL_THRESHOLD as u64);
        assert!(d.disconnect_reason.is_some());

        // recovery
        routes.lock().await.insert("*".into(), (200, "{}".into()));
        wled.apply_effect(&fx).await.unwrap();
        assert!(wait_connected(&reg, &addr, true).await);
        assert!(reg.snapshot().wled[&addr].disconnect_reason.is_none());
    }

    /// A controller adopted at runtime gets the current state's effect resolved like any
    /// other (device list, then protocol table), not through a stale private id table.
    #[tokio::test]
    async fn reapply_after_add_resolves_effect_by_device_list() {
        let mut config = Config::default();
        config.wled.effects.cooldown = WledEffect { effect: "Fire 2012".into(), ..Default::default() };
        let db = Arc::new(DatabaseManager::new("sqlite::memory:").await.unwrap());
        let bus: SharedEventBus = Arc::new(EventBus::new(100));
        let wled = WledManager::new(config.wled.clone(), db, bus);
        let (addr, _routes, seen) = mock_http(&[
            ("*", 200, "{}"),
            ("GET /json/effects", 200, r#"["Solid","Blink","Fire 2012"]"#),
        ])
        .await;
        wled.set_controllers_for_test(&[&addr]).await;
        *wled.current_state.write().await = ArenaState::Cooldown;

        wled.reapply_state_effect().await;

        let seen = seen.lock().await;
        let post = seen.iter().find(|r| r.starts_with("POST /json/state")).expect("state posted");
        assert!(post.contains(r#""fx":2"#), "fx must be the device index of Fire 2012: {post}");
    }

    #[tokio::test]
    async fn connection_refused_counts_as_failure() {
        let config = Config::default();
        let db = Arc::new(DatabaseManager::new("sqlite::memory:").await.unwrap());
        let bus: SharedEventBus = Arc::new(EventBus::new(100));
        let mut rx = bus.subscribe();
        let wled = WledManager::new(config.wled.clone(), db, bus);
        let dead = dead_addr().await;
        wled.set_controllers_for_test(&[&dead]).await;
        let fx = WledEffect { effect: "solid".into(), ..Default::default() };
        for _ in 0..health::FAIL_THRESHOLD {
            assert_eq!(wled.apply_effect(&fx).await.unwrap(), 0);
        }
        let mut kinds = Vec::new();
        while let Ok(e) = rx.try_recv() {
            kinds.push(e.event_type().to_string());
        }
        assert!(kinds.contains(&"WledDisconnected".to_string()), "{kinds:?}");
    }

    /// Idle controller: the health poll refreshes last_seen, and failures
    /// eventually mark it disconnected without any effect traffic.
    #[tokio::test]
    async fn health_poll_tracks_idle_controller() {
        let config = Config::default();
        let db = Arc::new(DatabaseManager::new("sqlite::memory:").await.unwrap());
        let bus: SharedEventBus = Arc::new(EventBus::new(100));
        let reg = Arc::new(StatusRegistry::new());
        reg.spawn(&bus);
        tokio::task::yield_now().await;
        let wled = WledManager::new(config.wled.clone(), db, bus.clone());
        let (addr, routes, _) = mock_http(&[("GET /json/info", 200, "{}")]).await;
        wled.set_controllers_for_test(&[&addr]).await;

        wled.poll_health_once().await;
        assert!(wait_connected(&reg, &addr, true).await);

        routes.lock().await.insert("GET /json/info".into(), (500, "{}".into()));
        for _ in 0..health::FAIL_THRESHOLD {
            wled.poll_health_once().await;
        }
        assert!(wait_connected(&reg, &addr, false).await);

        routes.lock().await.insert("GET /json/info".into(), (200, "{}".into()));
        wled.poll_health_once().await;
        assert!(wait_connected(&reg, &addr, true).await);
    }

    /// No network / bad host must not abort startup.
    #[tokio::test]
    async fn start_survives_unconnectable_controller() {
        let mut config = Config::default();
        config.wled.controllers = vec![crate::config::WledController {
            ip: "no-such-host.invalid".into(),
            num_leds: 10,
        }];
        let db = Arc::new(DatabaseManager::new("sqlite::memory:").await.unwrap());
        let bus: SharedEventBus = Arc::new(EventBus::new(100));
        let wled = WledManager::new(config.wled.clone(), db, bus);
        wled.start().await.expect("start must not fail offline");
        assert_eq!(wled.get_status().await.len(), 1);
        assert!(!wled.get_status().await[0].connected);
    }

    /// A controller without a UDP socket (skipped at boot) gets one once it is
    /// seen alive again.
    #[tokio::test]
    async fn ensure_sockets_recovers_missing_socket() {
        let config = Config::default();
        let db = Arc::new(DatabaseManager::new("sqlite::memory:").await.unwrap());
        let bus: SharedEventBus = Arc::new(EventBus::new(100));
        let wled = WledManager::new(config.wled.clone(), db, bus);
        wled.set_controllers_for_test(&["127.0.0.1"]).await;
        assert!(!wled.get_status().await[0].connected);
        wled.ensure_sockets().await;
        assert!(wled.get_status().await[0].connected);
    }

    #[test]
    fn realtime_frames_use_drgb_up_to_490_leds() {
        let rgb: Vec<u8> = (0..150 * 3).map(|i| i as u8).collect();
        let frames = encode_realtime_frames(&rgb, 2);
        assert_eq!(frames.len(), 1);
        assert_eq!(&frames[0][..2], &[2, 2], "protocol 2 = DRGB, then timeout");
        assert_eq!(&frames[0][2..], &rgb[..]);
        assert_eq!(encode_realtime_frames(&vec![0; 490 * 3], 2).len(), 1);
    }

    #[test]
    fn realtime_frames_use_dnrgb_chunks_above_490_leds() {
        let leds = 1000;
        let rgb: Vec<u8> = (0..leds * 3).map(|i| (i % 251) as u8).collect();
        let frames = encode_realtime_frames(&rgb, 2);
        assert_eq!(frames.len(), 3); // 489 + 489 + 22
        let mut rebuilt = Vec::new();
        for (i, f) in frames.iter().enumerate() {
            assert_eq!(f[0], 4, "protocol 4 = DNRGB");
            let start = u16::from_be_bytes([f[2], f[3]]) as usize;
            assert_eq!(start, i * 489);
            assert!(f.len() - 4 <= 489 * 3);
            rebuilt.extend_from_slice(&f[4..]);
        }
        assert_eq!(rebuilt, rgb);
    }

    /// Each controller gets a frame sized to its own strip, not the first one's.
    /// Multi-threaded: the blocking `recv_from` must not starve the sender task.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn realtime_sends_per_controller_led_counts() {
        let a = std::net::UdpSocket::bind("127.0.0.2:21324").unwrap();
        let b = std::net::UdpSocket::bind("127.0.0.3:21324").unwrap();
        for s in [&a, &b] {
            s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        }
        let config = Config::default();
        let db = Arc::new(DatabaseManager::new("sqlite::memory:").await.unwrap());
        let bus: SharedEventBus = Arc::new(EventBus::new(100));
        let wled = WledManager::new(config.wled.clone(), db, bus);
        *wled.controllers.write().await = vec![
            WledController { ip: "127.0.0.2".into(), num_leds: 30 },
            WledController { ip: "127.0.0.3".into(), num_leds: 200 },
        ];
        wled.rebuild_sockets().await.unwrap();
        wled.start_realtime_mode(ArenaState::Fight).await.unwrap();
        let mut buf = [0u8; 2048];
        let (n, _) = a.recv_from(&mut buf).unwrap();
        assert_eq!((buf[0], n), (2, 2 + 30 * 3));
        let (n, _) = b.recv_from(&mut buf).unwrap();
        assert_eq!((buf[0], n), (2, 2 + 200 * 3));
        wled.stop_realtime_mode().await;
    }

    #[test]
    fn realtime_addr_strips_an_http_port() {
        assert_eq!(realtime_addr("192.168.1.203"), "192.168.1.203:21324");
        assert_eq!(realtime_addr("wled.local"), "wled.local:21324");
        assert_eq!(realtime_addr("127.0.0.1:8090"), "127.0.0.1:21324");
        assert_eq!(realtime_addr("pi.local:8090"), "pi.local:21324");
    }

    /// A controller given as host:port (HTTP port) still gets realtime on host:21324.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn realtime_goes_to_port_21324_of_a_host_port_controller() {
        let rx = std::net::UdpSocket::bind("127.0.0.4:21324").unwrap();
        rx.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let config = Config::default();
        let db = Arc::new(DatabaseManager::new("sqlite::memory:").await.unwrap());
        let bus: SharedEventBus = Arc::new(EventBus::new(100));
        let wled = WledManager::new(config.wled.clone(), db, bus);
        *wled.controllers.write().await = vec![WledController { ip: "127.0.0.4:8090".into(), num_leds: 12 }];
        wled.rebuild_sockets().await.unwrap();
        assert!(wled.get_status().await[0].connected);
        wled.start_realtime_mode(ArenaState::Fight).await.unwrap();
        let mut buf = [0u8; 256];
        let (n, _) = rx.recv_from(&mut buf).unwrap();
        assert_eq!((buf[0], n), (2, 2 + 12 * 3));
        wled.stop_realtime_mode().await;
    }

    /// The device's own LED count replaces the 150 guess (discovery) and the
    /// frames are resized; a device that does not report one keeps its value.
    #[tokio::test]
    async fn health_poll_adopts_reported_led_count() {
        let config = Config::default();
        let db = Arc::new(DatabaseManager::new("sqlite::memory:").await.unwrap());
        let bus: SharedEventBus = Arc::new(EventBus::new(100));
        let wled = WledManager::new(config.wled.clone(), db, bus);
        let (addr, routes, _) = mock_http(&[("GET /json/info", 200, r#"{"leds":{"count":180}}"#)]).await;
        wled.set_controllers_for_test(&[&addr]).await;
        assert_eq!(wled.controllers.read().await[0].num_leds, 10);

        wled.poll_health_once().await;
        assert_eq!(wled.controllers.read().await[0].num_leds, 180);

        routes.lock().await.insert("GET /json/info".into(), (200, "{}".into()));
        wled.poll_health_once().await;
        assert_eq!(wled.controllers.read().await[0].num_leds, 180, "missing count keeps the known one");
    }
}
