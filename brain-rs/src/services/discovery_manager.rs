use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, error, debug};
use ssdp_client::SearchTarget;
use std::time::Duration;
use futures::StreamExt;

use crate::services::database_manager::SharedDatabaseManager;
use crate::services::ble_manager::BleManager;
use crate::core::SharedEventBus;

pub struct DiscoveryManager {
    db: SharedDatabaseManager,
    ble: Arc<BleManager>,
    _event_bus: SharedEventBus,
    is_scanning: Arc<RwLock<bool>>,
}

impl DiscoveryManager {
    pub fn new(db: SharedDatabaseManager, ble: Arc<BleManager>, event_bus: SharedEventBus) -> Self {
        Self {
            db,
            ble,
            _event_bus: event_bus,
            is_scanning: Arc::new(RwLock::new(false)),
        }
    }

    pub async fn start_scan(&self) -> Result<()> {
        let mut scanning = self.is_scanning.write().await;
        if *scanning {
            return Ok(());
        }
        *scanning = true;
        
        info!("Starting hardware discovery scan...");
        
        let db = self.db.clone();
        let ble = self.ble.clone();
        let is_scanning = self.is_scanning.clone();
        
        tokio::spawn(async move {
            // 1. SSDP/UPnP Scan for WLED and Tasmota
            if let Err(e) = Self::scan_ssdp(db.clone()).await {
                error!("SSDP scan failed: {}", e);
            }
            
            // 2. BLE Scan for Punching Bag
            // Note: BleManager needs to be updated to return discovered devices
            if let Err(e) = Self::scan_ble(db.clone(), ble).await {
                error!("BLE scan failed: {}", e);
            }
            
            let mut scanning = is_scanning.write().await;
            *scanning = false;
            info!("Hardware discovery scan completed");
        });
        
        Ok(())
    }

    async fn scan_ssdp(db: SharedDatabaseManager) -> Result<()> {
        info!("Scanning for SSDP devices...");
        
        let search_target = SearchTarget::RootDevice;
        let responses = ssdp_client::search(&search_target, Duration::from_secs(5), 3, None).await?;
        tokio::pin!(responses);
        
        while let Some(response) = responses.next().await {
            match response {
                Ok(resp) => {
                    let location = resp.location().to_string();
                    let usn = resp.usn().to_string();
                    let server = Some(resp.server().to_string());
                    
                    debug!("Found SSDP device: USN={}, Location={}, Server={:?}", usn, location, server);
                    
                    if location.contains("wled") || server.as_ref().map_or(false, |s| s.to_lowercase().contains("wled")) {
                        Self::register_discovered_device(&db, "WLED Device", "wled", &location).await;
                    } else if location.contains("tasmota") || usn.to_lowercase().contains("tasmota") {
                        Self::register_discovered_device(&db, "Tasmota Plug", "tasmota", &location).await;
                    }
                }
                Err(e) => warn!("SSDP response error: {}", e),
            }
        }
        
        Ok(())
    }

    async fn scan_ble(db: SharedDatabaseManager, ble: Arc<BleManager>) -> Result<()> {
        info!("Scanning for BLE punching bags...");
        match ble.scan_for_devices().await {
            Ok(devices) => {
                for (name, address) in devices {
                    info!("Found BLE device: {} ({})", name, address);
                    Self::register_discovered_device(&db, &name, "punching_bag", &format!("ble://{}", address)).await;
                }
            }
            Err(e) => error!("BLE scan failed: {}", e),
        }
        Ok(())
    }

    async fn register_discovered_device(db: &SharedDatabaseManager, name: &str, device_type: &str, location: &str) {
        // Parse host/port from location URL (e.g., http://192.168.1.50:80/xml or ble://address)
        let (host, port) = if location.starts_with("ble://") {
            (location.trim_start_matches("ble://").to_string(), 0)
        } else {
            match reqwest::Url::parse(location) {
                Ok(u) => (u.host_str().unwrap_or("unknown").to_string(), u.port().unwrap_or(80) as i32),
                Err(_) => return,
            }
        };
        
        info!("Registering discovered {} at {}:{}", device_type, host, port);
        if let Err(e) = db.add_device(name, device_type, &host, port, None).await {
            error!("Failed to save discovered device: {}", e);
        }
    }
}
