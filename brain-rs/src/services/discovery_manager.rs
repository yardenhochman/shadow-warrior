use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, error, debug};
use ssdp_client::SearchTarget;
use std::time::Duration;
use futures::StreamExt;
use mdns_sd::{ServiceDaemon, ServiceEvent};
use std::sync::OnceLock;

use crate::services::database_manager::SharedDatabaseManager;
use crate::services::ble_manager::BleManager;
use crate::core::{Event, SharedEventBus};

/// A WLED controller seen on mDNS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WledFound {
    /// Full service name, e.g. "wled-ab12cd._wled._tcp.local."
    pub fullname: String,
    /// Instance part of the full name ("wled-ab12cd").
    pub instance: String,
    /// Host name without the ".local." suffix.
    pub hostname: String,
    pub ip: String,
    pub port: u16,
}

/// The process-wide mDNS daemon, shared by every browse (one socket set, not one per scan).
fn mdns_daemon() -> Result<ServiceDaemon> {
    static DAEMON: OnceLock<ServiceDaemon> = OnceLock::new();
    if let Some(d) = DAEMON.get() {
        return Ok(d.clone());
    }
    let d = ServiceDaemon::new()?;
    Ok(DAEMON.get_or_init(|| d).clone())
}

/// Browse `_wled._tcp.local.` for `timeout`; one entry per resolved IPv4 address.
pub async fn browse_wled(timeout: Duration) -> Result<Vec<WledFound>> {
    const TYPE: &str = "_wled._tcp.local.";
    let mdns = mdns_daemon()?;
    let receiver = mdns.browse(TYPE)?;
    let deadline = tokio::time::Instant::now() + timeout;
    let mut found = Vec::new();
    while let Ok(ev) = tokio::time::timeout_at(deadline, receiver.recv_async()).await {
        match ev {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                let fullname = info.get_fullname().to_string();
                let instance = fullname.split("._").next().unwrap_or(&fullname).to_string();
                let hostname = info.get_hostname().trim_end_matches('.').trim_end_matches(".local").to_string();
                for addr in info.get_addresses().iter().filter(|a| a.is_ipv4()) {
                    found.push(WledFound {
                        fullname: fullname.clone(),
                        instance: instance.clone(),
                        hostname: hostname.clone(),
                        ip: addr.to_string(),
                        port: info.get_port(),
                    });
                }
            }
            Ok(_) => {}
            Err(e) => {
                warn!("mDNS recv error: {}", e);
                break;
            }
        }
    }
    let _ = mdns.stop_browse(TYPE);
    Ok(found)
}

pub struct DiscoveryManager {
    db: SharedDatabaseManager,
    ble: Arc<BleManager>,
    event_bus: SharedEventBus,
    is_scanning: Arc<RwLock<bool>>,
}

impl DiscoveryManager {
    pub fn new(db: SharedDatabaseManager, ble: Arc<BleManager>, event_bus: SharedEventBus) -> Self {
        Self {
            db,
            ble,
            event_bus,
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
        
        let event_bus = self.event_bus.clone();

        tokio::spawn(async move {
            // 1. mDNS scan for WLED (_wled._tcp)
            if let Err(e) = Self::scan_mdns(db.clone(), event_bus.clone()).await {
                error!("mDNS scan failed: {}", e);
            }

            // 2. SSDP/UPnP Scan for WLED and Tasmota
            if let Err(e) = Self::scan_ssdp(db.clone(), event_bus.clone()).await {
                error!("SSDP scan failed: {}", e);
            }

            // 3. BLE Scan for Punching Bag
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

    async fn scan_mdns(db: SharedDatabaseManager, event_bus: SharedEventBus) -> Result<()> {
        info!("Scanning for WLED devices via mDNS (_wled._tcp)...");
        for found in browse_wled(Duration::from_secs(5)).await? {
            debug!("Found WLED via mDNS: {} at {}:{}", found.instance, found.ip, found.port);
            if let Err(e) = db.add_device(&found.fullname, "wled", &found.ip, found.port as i32, None).await {
                error!("Failed to save mDNS WLED device: {}", e);
            } else {
                info!("Registered WLED {} at {}:{}", found.fullname, found.ip, found.port);
                event_bus.publish(Event::WledConnected { ip: found.ip.clone() });
            }
        }
        Ok(())
    }

    async fn scan_ssdp(db: SharedDatabaseManager, event_bus: SharedEventBus) -> Result<()> {
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
                    
                    if location.contains("wled") || server.as_ref().is_some_and(|s| s.to_lowercase().contains("wled")) {
                        Self::register_discovered_device(&db, "WLED Device", "wled", &location, Some(&event_bus)).await;
                    } else if location.contains("tasmota") || usn.to_lowercase().contains("tasmota") {
                        Self::register_discovered_device(&db, "Tasmota Plug", "tasmota", &location, None).await;
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
                    Self::register_discovered_device(&db, &name, "punching_bag", &format!("ble://{}", address), None).await;
                }
            }
            Err(e) => error!("BLE scan failed: {}", e),
        }
        Ok(())
    }

    async fn register_discovered_device(db: &SharedDatabaseManager, name: &str, device_type: &str, location: &str, event_bus: Option<&SharedEventBus>) {
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
        match db.add_device(name, device_type, &host, port, None).await {
            Ok(_) => {
                if let Some(bus) = event_bus {
                    bus.publish(Event::WledConnected { ip: host.clone() });
                }
            }
            Err(e) => error!("Failed to save discovered device: {}", e),
        }
    }
}
