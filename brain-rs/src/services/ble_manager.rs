use anyhow::{Context, Result};
use btleplug::api::{
    Central, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::config::BleConfig;
use crate::core::{ArenaState, Event, SharedEventBus};

// Nordic UART Service UUIDs
const ACCELERATION_UUID: Uuid = Uuid::from_u128(0x6E400002_B5A3_F393_E0A9_E50E24DCCA9E);
const ALPHA_UUID: Uuid = Uuid::from_u128(0x6E400004_B5A3_F393_E0A9_E50E24DCCA9E);
const THRESHOLD_UUID: Uuid = Uuid::from_u128(0x6E400005_B5A3_F393_E0A9_E50E24DCCA9E);
const FIGHT_MODE_UUID: Uuid = Uuid::from_u128(0x6E400006_B5A3_F393_E0A9_E50E24DCCA9E);

/// Serialises scan/connect phases of the bag and the station provisioner, which may share one
/// BlueZ adapter (two concurrent discovery sessions and connects confuse cheap controllers).
pub(crate) static ADAPTER_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub struct BleManager {
    config: Arc<RwLock<BleConfig>>,
    event_bus: SharedEventBus,
    peripheral: Arc<RwLock<Option<Peripheral>>>,
    /// Keeps the btleplug Manager/Adapter (and its event loop) alive while connected;
    /// dropping them leaves the peripheral's connection state stale.
    keepalive: Arc<RwLock<Option<(Manager, Adapter)>>>,
    /// Listener for acceleration notifications. Exactly one may run: BlueZ keeps
    /// the same device object across reconnects, so a leftover listener would
    /// receive every notification again and double-count punches.
    notify_task: Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    connected: Arc<RwLock<bool>>,
    /// True while the arena is in FIGHT; the bag only streams while this is set.
    fight_wanted: Arc<AtomicBool>,
    /// Set by [`BleManager::shutdown`]; stops the auto-connect loop from reconnecting.
    shutting_down: Arc<AtomicBool>,
}

impl BleManager {
    pub fn new(config: BleConfig, event_bus: SharedEventBus) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            event_bus,
            peripheral: Arc::new(RwLock::new(None)),
            keepalive: Arc::new(RwLock::new(None)),
            notify_task: Arc::new(std::sync::Mutex::new(None)),
            connected: Arc::new(RwLock::new(false)),
            fight_wanted: Arc::new(AtomicBool::new(false)),
            shutting_down: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start the BLE manager (auto-connect loop if enabled)
    pub async fn start(&self) -> Result<()> {
        info!("Starting BLE manager");

        // Mirror arena FIGHT state onto the bag's fight-mode characteristic.
        let manager = self.clone_for_task();
        let mut rx = self.event_bus.subscribe();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(Event::StateChanged { to, .. }) => {
                        let fight = ArenaState::from_str(&to) == Ok(ArenaState::Fight);
                        if manager.fight_wanted.swap(fight, Ordering::SeqCst) != fight {
                            manager.sync_fight_mode().await;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("BLE fight-mode listener lagged by {n} events");
                    }
                    Err(_) => break,
                    _ => {}
                }
            }
        });

        if self.config.read().await.auto_connect {
            let manager = self.clone_for_task();
            tokio::spawn(async move {
                manager.auto_connect_loop().await;
            });
        }

        Ok(())
    }

    /// Auto-connect loop with retry logic
    async fn auto_connect_loop(&self) {
        loop {
            if self.shutting_down.load(Ordering::SeqCst) {
                return;
            }
            if *self.connected.read().await {
                if self.link_is_up().await {
                    // Idle bag sends nothing; keep health `last_seen` fresh.
                    self.event_bus.publish(Event::BleAlive);
                } else {
                    self.mark_disconnected("link lost").await;
                }
            }
            if !*self.connected.read().await {
                info!("Attempting to connect to punching bag...");
                match self.connect().await {
                    Ok(()) => {
                        info!("Successfully connected to punching bag");
                        // Bag powers up with fight mode off; match current arena state.
                        self.sync_fight_mode().await;
                        // Start listening for notifications
                        if let Err(e) = self.start_notifications().await {
                            error!("Failed to start notifications: {}", e);
                            self.event_bus.publish(Event::BleConnectionError {
                                error: format!("notifications: {e}"),
                            });
                        }
                    }
                    Err(e) => {
                        warn!("Failed to connect to punching bag: {}", e);
                        self.event_bus.publish(Event::BleConnectionError {
                            error: format!("{e:#}"),
                        });
                    }
                }
            }

            let retry_interval = self.config.read().await.retry_interval_sec;
            sleep(Duration::from_secs(retry_interval)).await;
        }
    }

    /// Resolve the configured adapter (default: first).
    async fn pick_adapter(&self, manager: &Manager) -> Result<Adapter> {
        let adapters = manager.adapters().await?;
        let mut names = Vec::with_capacity(adapters.len());
        for a in &adapters {
            names.push(a.adapter_info().await.unwrap_or_else(|_| "unknown".into()));
        }
        let wanted = self.config.read().await.adapter.clone();
        let idx = select_adapter(&names, wanted.as_deref()).map_err(anyhow::Error::msg)?;
        info!("Using Bluetooth adapter: {}", names[idx]);
        Ok(adapters.into_iter().nth(idx).expect("index validated"))
    }

    /// Connect to the punching bag
    async fn connect(&self) -> Result<()> {
        let _adapter_guard = ADAPTER_LOCK.lock().await;
        let manager = Manager::new().await?;
        let adapter = self.pick_adapter(&manager).await?;

        info!("Starting BLE scan...");
        adapter
            .start_scan(ScanFilter::default())
            .await
            .context("Failed to start scan")?;

        let scan_timeout = self.config.read().await.scan_timeout_sec;
        sleep(Duration::from_secs(scan_timeout)).await;

        let peripherals = adapter.peripherals().await?;
        info!("Found {} peripherals", peripherals.len());

        // Find device matching name patterns
        let peripheral = self.find_matching_peripheral(&peripherals).await?;

        // Connect to device
        peripheral
            .connect()
            .await
            .context("Failed to connect to device")?;
        peripheral
            .discover_services()
            .await
            .context("Failed to discover services")?;

        let device_name = peripheral
            .properties()
            .await?
            .and_then(|p| p.local_name)
            .unwrap_or_else(|| "Unknown".to_string());

        let address = peripheral
            .properties()
            .await?
            .map(|p| p.address.to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        *self.peripheral.write().await = Some(peripheral);
        *self.keepalive.write().await = Some((manager, adapter));
        *self.connected.write().await = true;

        self.event_bus.publish(Event::BleDeviceConnected {
            device_name: device_name.clone(),
            address,
        });

        // Set initial parameters
        let config = self.config.read().await;
        self.set_alpha(config.alpha).await?;
        self.set_threshold(config.threshold).await?;

        Ok(())
    }

    /// Find a peripheral matching the configured name patterns
    async fn find_matching_peripheral(&self, peripherals: &[Peripheral]) -> Result<Peripheral> {
        for peripheral in peripherals {
            if let Ok(Some(properties)) = peripheral.properties().await {
                if let Some(name) = properties.local_name {
                    let name_lower = name.to_lowercase();
                    let config = self.config.read().await;
                    for pattern in &config.device_name_patterns {
                        if name_lower.contains(&pattern.to_lowercase()) {
                            info!("Found matching device: {}", name);
                            return Ok(peripheral.clone());
                        }
                    }
                }
            }
        }

        Err(anyhow::anyhow!(
            "No device found matching patterns: {:?}",
            self.config.read().await.device_name_patterns
        ))
    }

    /// Explicitly scan for devices and return found punching bags
    pub async fn scan_for_devices(&self) -> Result<Vec<(String, String)>> {
        let manager = Manager::new().await?;
        let adapter = self.pick_adapter(&manager).await?;

        adapter.start_scan(ScanFilter::default()).await?;
        sleep(Duration::from_secs(5)).await;
        
        let peripherals = adapter.peripherals().await?;
        let mut found = Vec::new();
        
        for peripheral in peripherals {
            if let Ok(Some(properties)) = peripheral.properties().await {
                if let Some(name) = properties.local_name {
                    let name_lower = name.to_lowercase();
                    let config = self.config.read().await;
                    for pattern in &config.device_name_patterns {
                        if name_lower.contains(&pattern.to_lowercase()) {
                            found.push((name, properties.address.to_string()));
                            break;
                        }
                    }
                }
            }
        }
        
        Ok(found)
    }

    /// Start listening for acceleration notifications
    async fn start_notifications(&self) -> Result<()> {
        let peripheral_guard = self.peripheral.read().await;
        let peripheral = peripheral_guard
            .as_ref()
            .context("Not connected to device")?;

        // Find acceleration characteristic
        let chars = peripheral.characteristics();
        let accel_char = chars
            .iter()
            .find(|c| c.uuid == ACCELERATION_UUID)
            .context("Acceleration characteristic not found")?;

        // Subscribe to notifications
        peripheral.subscribe(accel_char).await?;

        // Spawn notification handler
        let event_bus = self.event_bus.clone();
        let manager = self.clone_for_task();
        let peripheral_clone = peripheral.clone();
        let accel_char_clone = accel_char.clone();
        let mut punch_gate = PunchGate::new(Duration::from_millis(self.config.read().await.punch_refractory_ms));

        let handle = tokio::spawn(async move {
            let mut notification_stream = match peripheral_clone.notifications().await {
                Ok(stream) => stream,
                Err(e) => {
                    error!("Failed to get notification stream: {}", e);
                    event_bus.publish(Event::BleConnectionError {
                        error: format!("notification stream: {e}"),
                    });
                    return;
                }
            };

            loop {
                use futures::StreamExt;
                match notification_stream.next().await {
                    Some(data) => {
                        if data.uuid == accel_char_clone.uuid {
                            Self::handle_acceleration_data(&event_bus, &data.value, &mut punch_gate);
                        }
                    }
                    None => {
                        warn!("Notification stream ended");
                        manager.mark_disconnected("notification stream ended").await;
                        break;
                    }
                }
            }
        });
        self.replace_notify_task(Some(handle));

        Ok(())
    }

    /// Install (or clear) the notification listener, aborting any previous one.
    fn replace_notify_task(&self, new: Option<tokio::task::JoinHandle<()>>) {
        let old = std::mem::replace(&mut *self.notify_task.lock().unwrap(), new);
        if let Some(old) = old {
            old.abort();
        }
    }

    /// Handle incoming acceleration data. The firmware sends gravity-free
    /// acceleration (m/s^2) only while above its threshold, about every 100 ms,
    /// so one hit arrives as a burst of samples; `gate` folds a burst into one punch.
    fn handle_acceleration_data(event_bus: &SharedEventBus, data: &[u8], gate: &mut PunchGate) {
        let Some((x, y, z, magnitude)) = parse_acceleration(data) else {
            debug!("Ignoring acceleration payload of {} bytes", data.len());
            return;
        };
        debug!("Acceleration: x={:.2}, y={:.2}, z={:.2}, mag={:.2}", x, y, z, magnitude);
        if magnitude > 1.0 && gate.accept(std::time::Instant::now()) {
            let power = (magnitude * 10.0).min(100.0); // Scale to 0-100
            event_bus.publish(Event::punch_detected(power, x, y, z));
        }
    }

    /// Set the alpha parameter (smoothing factor)
    pub async fn set_alpha(&self, alpha: f32) -> Result<()> {
        let peripheral_guard = self.peripheral.read().await;
        let peripheral = peripheral_guard
            .as_ref()
            .context("Not connected to device")?;

        let chars = peripheral.characteristics();
        let char = chars
            .iter()
            .find(|c| c.uuid == ALPHA_UUID)
            .context("Alpha characteristic not found")?;

        let bytes = alpha.to_le_bytes();
        peripheral.write(char, &bytes, WriteType::WithResponse).await?;

        info!("Set alpha to {}", alpha);
        Ok(())
    }

    /// Set the threshold parameter
    pub async fn set_threshold(&self, threshold: f32) -> Result<()> {
        let peripheral_guard = self.peripheral.read().await;
        let peripheral = peripheral_guard
            .as_ref()
            .context("Not connected to device")?;

        let chars = peripheral.characteristics();
        let char = chars
            .iter()
            .find(|c| c.uuid == THRESHOLD_UUID)
            .context("Threshold characteristic not found")?;

        let bytes = threshold.to_le_bytes();
        peripheral.write(char, &bytes, WriteType::WithResponse).await?;

        info!("Set threshold to {}", threshold);
        Ok(())
    }

    /// Update parameters dynamically
    pub async fn update_parameters(&self, alpha: f32, threshold: f32) -> Result<()> {
        {
            let mut config = self.config.write().await;
            config.alpha = alpha;
            config.threshold = threshold;
        }

        if *self.connected.read().await {
            self.set_alpha(alpha).await?;
            self.set_threshold(threshold).await?;
        }

        info!("BLE parameters updated: alpha={:.2}, threshold={:.2}", alpha, threshold);
        Ok(())
    }

    /// Get current config
    pub fn config(&self) -> Arc<RwLock<BleConfig>> {
        self.config.clone()
    }

    /// Whether the BLE link to the stored peripheral is still up.
    ///
    /// btleplug calls on a dead link can hang forever (observed on macOS after
    /// the bag reset: `is_connected()` never returned, freezing the reconnect
    /// loop), so the whole probe is bounded: a hang counts as "down".
    async fn link_is_up(&self) -> bool {
        let Some(p) = self.peripheral.read().await.clone() else {
            return false;
        };
        let probe = async {
            if !p.is_connected().await.unwrap_or(false) {
                return false;
            }
            let Some(ch) = p.characteristics().into_iter().find(|c| c.uuid == FIGHT_MODE_UUID)
            else {
                return false;
            };
            p.read(&ch).await.is_ok()
        };
        tokio::time::timeout(Duration::from_secs(3), probe).await.unwrap_or(false)
    }

    /// Release the bag on process exit.
    ///
    /// bluetoothd keeps a connection alive after its owner dies, and a connected
    /// bag stops advertising, so nothing could find it until a manual
    /// `bluetoothctl disconnect`. Bounded so a wedged stack cannot block exit.
    pub async fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        let peripheral = self.peripheral.read().await.clone();
        if let Some(p) = peripheral {
            match tokio::time::timeout(Duration::from_secs(3), p.disconnect()).await {
                Ok(Ok(())) => info!("Disconnected from punching bag"),
                Ok(Err(e)) => warn!("Failed to disconnect from punching bag: {e}"),
                Err(_) => warn!("Timed out disconnecting from punching bag"),
            }
        }
        self.mark_disconnected("shutdown").await;
    }

    /// Clear connection state so the auto-connect loop reconnects, and
    /// announce the drop once.
    async fn mark_disconnected(&self, reason: &str) {
        let was_connected = std::mem::replace(&mut *self.connected.write().await, false);
        *self.peripheral.write().await = None;
        *self.keepalive.write().await = None;
        if was_connected {
            self.event_bus.publish(Event::BleDeviceDisconnected {
                reason: reason.to_string(),
            });
        }
        // Last on purpose: if the listener itself called us, aborting only takes
        // effect at its next await point, after we have returned.
        self.replace_notify_task(None);
    }

    /// Push the desired fight mode to the bag if connected; log failures.
    async fn sync_fight_mode(&self) {
        if !*self.connected.read().await {
            return;
        }
        let want = self.fight_wanted.load(Ordering::SeqCst);
        if let Err(e) = self.set_fight_mode(want).await {
            warn!("Failed to set bag fight mode to {want}: {e:#}");
        }
    }

    /// Set fight mode
    pub async fn set_fight_mode(&self, enabled: bool) -> Result<()> {
        let peripheral_guard = self.peripheral.read().await;
        let peripheral = peripheral_guard
            .as_ref()
            .context("Not connected to device")?;

        let chars = peripheral.characteristics();
        let char = chars
            .iter()
            .find(|c| c.uuid == FIGHT_MODE_UUID)
            .context("Fight mode characteristic not found")?;

        let bytes = [if enabled { 1u8 } else { 0u8 }];
        peripheral.write(char, &bytes, WriteType::WithResponse).await?;

        info!("Set fight mode to {}", enabled);
        Ok(())
    }

    /// Helper to clone for async tasks
    fn clone_for_task(&self) -> Self {
        Self {
            config: self.config.clone(),
            event_bus: self.event_bus.clone(),
            peripheral: self.peripheral.clone(),
            keepalive: self.keepalive.clone(),
            notify_task: self.notify_task.clone(),
            connected: self.connected.clone(),
            fight_wanted: self.fight_wanted.clone(),
            shutting_down: self.shutting_down.clone(),
        }
    }
}

/// How an adapter is attached, read from sysfs (the BlueZ info string is useless
/// for this: every adapter reports the same `usb:v1D6Bp0246...` root-hub id).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdapterHw {
    /// USB adapter with `(vendor id, product id)`.
    Usb(u16, u16),
    /// Anything else: UART/serdev combo chip, SDIO, virtual.
    Builtin,
}

impl AdapterHw {
    fn describe(self) -> String {
        match self {
            AdapterHw::Usb(v, p) => format!("usb {v:04x}:{p:04x}"),
            AdapterHw::Builtin => "builtin".into(),
        }
    }
}

/// Look up `hciN` in `/sys/class/bluetooth`. `None` when sysfs has no such
/// adapter (non-Linux, or a name we cannot map).
fn sysfs_adapter_hw(hci: &str) -> Option<AdapterHw> {
    let path = std::fs::canonicalize(std::path::Path::new("/sys/class/bluetooth").join(hci)).ok()?;
    let read_hex = |dir: &std::path::Path, f: &str| {
        u16::from_str_radix(std::fs::read_to_string(dir.join(f)).ok()?.trim(), 16).ok()
    };
    for dir in path.ancestors() {
        if let (Some(v), Some(p)) = (read_hex(dir, "idVendor"), read_hex(dir, "idProduct")) {
            return Some(AdapterHw::Usb(v, p));
        }
    }
    Some(AdapterHw::Builtin)
}

/// Pick an adapter from the names reported by BlueZ (`"hci0 (usb:...)"`).
/// `None` -> first. Digits-only -> index. Otherwise exact id (first token), then
/// a hardware selector, then case-insensitive substring. Errors list the
/// available adapters.
///
/// Hardware selectors survive `hciN` renumbering between boots (an onboard
/// UART radio and a USB dongle can enumerate in either order):
/// `usb` (first USB adapter), `usb:VVVV:PPPP` (hex ids, e.g. `usb:2357:0604`),
/// `builtin` (first non-USB adapter).
pub(crate) fn select_adapter(names: &[String], wanted: Option<&str>) -> std::result::Result<usize, String> {
    select_adapter_with(names, wanted, &|hci| sysfs_adapter_hw(hci))
}

pub(crate) fn select_adapter_with(
    names: &[String],
    wanted: Option<&str>,
    hw: &dyn Fn(&str) -> Option<AdapterHw>,
) -> std::result::Result<usize, String> {
    let hw_of = |n: &String| n.split_whitespace().next().and_then(hw);
    let list = || {
        if names.is_empty() { "none".to_string() } else {
            names
                .iter()
                .enumerate()
                .map(|(i, n)| match hw_of(n) {
                    Some(h) => format!("{i}: {n} [{}]", h.describe()),
                    None => format!("{i}: {n}"),
                })
                .collect::<Vec<_>>()
                .join(", ")
        }
    };
    let Some(w) = wanted.map(str::trim).filter(|w| !w.is_empty()) else {
        return if names.is_empty() { Err("No Bluetooth adapter found".into()) } else { Ok(0) };
    };
    let lw = w.to_lowercase();
    if let Ok(i) = w.parse::<usize>() {
        if i < names.len() { return Ok(i); }
    } else if let Some(i) = names.iter().position(|n| n.split_whitespace().next() == Some(w)) {
        return Ok(i);
    } else if lw == "usb" || lw.starts_with("usb:") || lw == "builtin" {
        let matches = |h: AdapterHw| match (lw.as_str(), h) {
            ("usb", AdapterHw::Usb(..)) | ("builtin", AdapterHw::Builtin) => true,
            (sel, AdapterHw::Usb(v, p)) if sel.starts_with("usb:") => {
                let id: Vec<&str> = sel[4..].split(':').collect();
                id.len() == 2
                    && u16::from_str_radix(id[0], 16) == Ok(v)
                    && u16::from_str_radix(id[1], 16) == Ok(p)
            }
            _ => false,
        };
        if let Some(i) = names.iter().position(|n| hw_of(n).is_some_and(matches)) {
            return Ok(i);
        }
    } else if let Some(i) = names.iter().position(|n| n.to_lowercase().contains(&lw)) {
        return Ok(i);
    }
    Err(format!("Configured Bluetooth adapter '{w}' not found; available: {}", list()))
}

// SAFETY: BleManager is always wrapped in Arc and access is synchronized
unsafe impl Send for BleManager {}
unsafe impl Sync for BleManager {}


#[cfg(test)]
mod adapter_tests {
    use super::{select_adapter, select_adapter_with, AdapterHw};

    fn names() -> Vec<String> {
        vec!["hci0 (usb:v1D6Bp0246d0540)".into(), "hci1 (usb:v0A12p0001d8891)".into()]
    }

    #[test]
    fn default_is_first() {
        assert_eq!(select_adapter(&names(), None), Ok(0));
        assert_eq!(select_adapter(&names(), Some("  ")), Ok(0));
    }

    #[test]
    fn by_id_index_and_substring() {
        assert_eq!(select_adapter(&names(), Some("hci1")), Ok(1));
        assert_eq!(select_adapter(&names(), Some("1")), Ok(1));
        assert_eq!(select_adapter(&names(), Some("0A12")), Ok(1));
    }

    fn hw(hci: &str) -> Option<AdapterHw> {
        match hci {
            "hci0" => Some(AdapterHw::Builtin),
            "hci1" => Some(AdapterHw::Usb(0x2357, 0x0604)),
            _ => None,
        }
    }

    #[test]
    fn by_hardware_survives_renumbering() {
        let n = names();
        assert_eq!(select_adapter_with(&n, Some("usb"), &hw), Ok(1));
        assert_eq!(select_adapter_with(&n, Some("usb:2357:0604"), &hw), Ok(1));
        assert_eq!(select_adapter_with(&n, Some("USB:2357:0604"), &hw), Ok(1));
        assert_eq!(select_adapter_with(&n, Some("builtin"), &hw), Ok(0));
        // dongle enumerated first this boot: same config, right adapter
        let swapped = vec!["hci0 (x)".to_string(), "hci1 (x)".to_string()];
        let swapped_hw = |h: &str| match h {
            "hci0" => Some(AdapterHw::Usb(0x2357, 0x0604)),
            "hci1" => Some(AdapterHw::Builtin),
            _ => None,
        };
        assert_eq!(select_adapter_with(&swapped, Some("usb:2357:0604"), &swapped_hw), Ok(0));
        assert_eq!(select_adapter_with(&swapped, Some("builtin"), &swapped_hw), Ok(1));
    }

    #[test]
    fn hardware_selector_misses_are_errors_with_hw_listed() {
        let n = names();
        let e = select_adapter_with(&n, Some("usb:dead:beef"), &hw).unwrap_err();
        assert!(e.contains("usb:dead:beef") && e.contains("[builtin]") && e.contains("[usb 2357:0604]"), "{e}");
        assert!(select_adapter_with(&n, Some("usb:2357"), &hw).is_err());
        // no sysfs info (non-Linux): hardware selectors cannot match
        assert!(select_adapter_with(&n, Some("usb"), &|_| None).is_err());
    }

    #[test]
    fn missing_lists_available() {
        let e = select_adapter(&names(), Some("hci7")).unwrap_err();
        assert!(e.contains("hci7") && e.contains("0: hci0") && e.contains("1: hci1"), "{e}");
        assert!(select_adapter(&names(), Some("5")).is_err());
        assert!(select_adapter(&[], None).is_err());
    }
}

/// Parse an acceleration notification: 12 bytes = x,y,z f32, 4 bytes = magnitude f32.
fn parse_acceleration(data: &[u8]) -> Option<(f32, f32, f32, f32)> {
    let f = |i: usize| f32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
    match data.len() {
        12 => {
            let (x, y, z) = (f(0), f(4), f(8));
            Some((x, y, z, (x * x + y * y + z * z).sqrt()))
        }
        4 => Some((0.0, 0.0, f(0), f(0))),
        _ => None,
    }
}

/// Counts one punch per burst: a sample is a new punch only if the last punch
/// was at least `refractory` ago.
struct PunchGate {
    refractory: Duration,
    last_punch: Option<std::time::Instant>,
}

impl PunchGate {
    fn new(refractory: Duration) -> Self {
        Self { refractory, last_punch: None }
    }

    fn accept(&mut self, now: std::time::Instant) -> bool {
        if self.last_punch.is_some_and(|t| now.duration_since(t) < self.refractory) {
            return false;
        }
        self.last_punch = Some(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::EventBus;

    #[test]
    fn parses_both_payload_shapes() {
        let mag = 3.5f32.to_le_bytes();
        assert_eq!(parse_acceleration(&mag), Some((0.0, 0.0, 3.5, 3.5)));
        let xyz: Vec<u8> = [3.0f32, 4.0, 0.0].iter().flat_map(|v| v.to_le_bytes()).collect();
        assert_eq!(parse_acceleration(&xyz), Some((3.0, 4.0, 0.0, 5.0)));
        assert_eq!(parse_acceleration(&[0u8; 7]), None);
    }

    #[test]
    fn gate_folds_a_burst_into_one_punch() {
        let t0 = std::time::Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut gate = PunchGate::new(Duration::from_millis(350));
        // One hit: samples every 100 ms while the bag swings.
        let hits: Vec<bool> = [0, 100, 200, 300].iter().map(|&n| gate.accept(ms(n))).collect();
        assert_eq!(hits, vec![true, false, false, false]);
        // A second hit after the window counts.
        assert!(gate.accept(ms(400)));
    }

    #[tokio::test]
    async fn burst_of_samples_publishes_one_punch() {
        let bus: SharedEventBus = Arc::new(EventBus::new(100));
        let mut rx = bus.subscribe();
        let mut gate = PunchGate::new(Duration::from_secs(10));
        for _ in 0..4 {
            BleManager::handle_acceleration_data(&bus, &6.0f32.to_le_bytes(), &mut gate);
        }
        // Below 1.0 never counts.
        BleManager::handle_acceleration_data(&bus, &0.5f32.to_le_bytes(), &mut PunchGate::new(Duration::ZERO));
        let mut punches = 0;
        while let Ok(ev) = rx.try_recv() {
            if let Event::PunchDetected { power, .. } = ev {
                assert_eq!(power, 60.0);
                punches += 1;
            }
        }
        assert_eq!(punches, 1);
    }
}
