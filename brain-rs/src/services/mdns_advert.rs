//! Advertise the brain on the LAN as `_shadow-brain._tcp` (mDNS / DNS-SD).
//!
//! Instance `shadow-brain`, host name `shadow-brain.local`, port = the HTTP API port, TXT records
//! `station_udp` (port stations send frames to) and `api` (`/api`). Stations and tools find the brain
//! by name instead of a fixed IP, so a DHCP change does not need re-provisioning. Not UPnP: this is
//! the same multicast DNS the WLED discovery uses.

use mdns_sd::{ServiceDaemon, ServiceInfo};
use std::collections::HashMap;
use tracing::{info, warn};

pub const SERVICE_TYPE: &str = "_shadow-brain._tcp.local.";
pub const DEFAULT_NAME: &str = "shadow-brain";

/// TXT properties: pure so a test can pin them.
pub fn txt(station_udp_port: Option<u16>) -> HashMap<String, String> {
    let mut p = HashMap::new();
    p.insert("api".to_string(), "/api".to_string());
    p.insert("ver".to_string(), env!("CARGO_PKG_VERSION").to_string());
    if let Some(port) = station_udp_port {
        p.insert("station_udp".to_string(), port.to_string());
    }
    p
}

/// Start advertising. Keep the returned daemon alive for as long as the brain runs; `None` when mDNS is
/// unavailable (logged, never fatal).
pub fn advertise(name: &str, http_port: u16, station_udp_port: Option<u16>) -> Option<ServiceDaemon> {
    let daemon = match ServiceDaemon::new() {
        Ok(d) => d,
        Err(e) => {
            warn!("mDNS advertising unavailable: {e}");
            return None;
        }
    };
    let host = format!("{name}.local.");
    let info = match ServiceInfo::new(SERVICE_TYPE, name, &host, "", http_port, txt(station_udp_port)) {
        Ok(i) => i.enable_addr_auto(),
        Err(e) => {
            warn!("mDNS service info invalid: {e}");
            return None;
        }
    };
    if let Err(e) = daemon.register(info) {
        warn!("mDNS register failed: {e}");
        return None;
    }
    info!("mDNS: advertising {name}.{SERVICE_TYPE} on port {http_port}");
    Some(daemon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txt_records() {
        let t = txt(Some(4210));
        assert_eq!(t["station_udp"], "4210");
        assert_eq!(t["api"], "/api");
        assert!(!txt(None).contains_key("station_udp"));
    }

    #[test]
    fn service_info_is_valid() {
        ServiceInfo::new(SERVICE_TYPE, DEFAULT_NAME, "shadow-brain.local.", "", 8000, txt(Some(4210))).unwrap();
    }
}
