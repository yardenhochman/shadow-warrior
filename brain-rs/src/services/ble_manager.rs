use anyhow::{Context, Result};
use btleplug::api::{
    Central, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Manager, Peripheral};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::config::BleConfig;
use crate::core::{Event, SharedEventBus};

// Nordic UART Service UUIDs
#[allow(dead_code)]
const SERVICE_UUID: Uuid = Uuid::from_u128(0x6E400001_B5A3_F393_E0A9_E50E24DCCA9E);
const ACCELERATION_UUID: Uuid = Uuid::from_u128(0x6E400002_B5A3_F393_E0A9_E50E24DCCA9E);
const ALPHA_UUID: Uuid = Uuid::from_u128(0x6E400004_B5A3_F393_E0A9_E50E24DCCA9E);
const THRESHOLD_UUID: Uuid = Uuid::from_u128(0x6E400005_B5A3_F393_E0A9_E50E24DCCA9E);
#[allow(dead_code)]
const FIGHT_MODE_UUID: Uuid = Uuid::from_u128(0x6E400006_B5A3_F393_E0A9_E50E24DCCA9E);

pub struct BleManager {
    config: Arc<RwLock<BleConfig>>,
    event_bus: SharedEventBus,
    peripheral: Arc<RwLock<Option<Peripheral>>>,
    connected: Arc<RwLock<bool>>,
}

impl BleManager {
    pub fn new(config: BleConfig, event_bus: SharedEventBus) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            event_bus,
            peripheral: Arc::new(RwLock::new(None)),
            connected: Arc::new(RwLock::new(false)),
        }
    }

    /// Start the BLE manager (auto-connect loop if enabled)
    pub async fn start(&self) -> Result<()> {
        info!("Starting BLE manager");

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
            if !*self.connected.read().await {
                info!("Attempting to connect to punching bag...");
                match self.connect().await {
                    Ok(()) => {
                        info!("Successfully connected to punching bag");
                        // Start listening for notifications
                        if let Err(e) = self.start_notifications().await {
                            error!("Failed to start notifications: {}", e);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to connect to punching bag: {}", e);
                    }
                }
            }

            let retry_interval = self.config.read().await.retry_interval_sec;
            sleep(Duration::from_secs(retry_interval)).await;
        }
    }

    /// Connect to the punching bag
    async fn connect(&self) -> Result<()> {
        let manager = Manager::new().await?;
        let adapters = manager.adapters().await?;
        let adapter = adapters
            .into_iter()
            .next()
            .context("No Bluetooth adapter found")?;

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
            .and_then(|p| Some(p.address.to_string()))
            .unwrap_or_else(|| "Unknown".to_string());

        *self.peripheral.write().await = Some(peripheral);
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
        let adapters = manager.adapters().await?;
        let adapter = adapters
            .into_iter()
            .next()
            .context("No Bluetooth adapter found")?;

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
        let peripheral_clone = peripheral.clone();
        let accel_char_clone = accel_char.clone();

        tokio::spawn(async move {
            let mut notification_stream = match peripheral_clone.notifications().await {
                Ok(stream) => stream,
                Err(e) => {
                    error!("Failed to get notification stream: {}", e);
                    return;
                }
            };

            loop {
                use futures::StreamExt;
                match notification_stream.next().await {
                    Some(data) => {
                        if data.uuid == accel_char_clone.uuid {
                            Self::handle_acceleration_data(&event_bus, &data.value);
                        }
                    }
                    None => {
                        warn!("Notification stream ended");
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// Handle incoming acceleration data
    fn handle_acceleration_data(event_bus: &SharedEventBus, data: &[u8]) {
        if data.len() == 12 {
            // 3x float32 (x, y, z)
            let x = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            let y = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            let z = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);

            let magnitude = (x * x + y * y + z * z).sqrt();

            debug!("Acceleration: x={:.2}, y={:.2}, z={:.2}, mag={:.2}", x, y, z, magnitude);

            // Emit punch event if magnitude is significant
            if magnitude > 1.0 {
                let power = (magnitude * 10.0).min(100.0); // Scale to 0-100
                event_bus.publish(Event::punch_detected(power, x, y, z));
            }
        } else if data.len() == 4 {
            // Single float32 (magnitude only)
            let magnitude = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            debug!("Acceleration magnitude: {:.2}", magnitude);

            if magnitude > 1.0 {
                let power = (magnitude * 10.0).min(100.0);
                event_bus.publish(Event::punch_detected(power, 0.0, 0.0, magnitude));
            }
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

    /// Set fight mode
    #[allow(dead_code)]
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

    /// Check if connected
    #[allow(dead_code)]
    pub async fn is_connected(&self) -> bool {
        *self.connected.read().await
    }

    /// Helper to clone for async tasks
    fn clone_for_task(&self) -> Self {
        Self {
            config: self.config.clone(),
            event_bus: self.event_bus.clone(),
            peripheral: self.peripheral.clone(),
            connected: self.connected.clone(),
        }
    }
}

// SAFETY: BleManager is always wrapped in Arc and access is synchronized
unsafe impl Send for BleManager {}
unsafe impl Sync for BleManager {}

