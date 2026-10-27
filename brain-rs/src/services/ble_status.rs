//! Read-only BLE GATT status service (BlueZ *peripheral* role).
//!
//! `ble_manager` is a BLE central (btleplug) and cannot advertise, so this
//! exposes a small GATT server through BlueZ (`bluer`) that a phone can read
//! with nRF Connect / LightBlue.
//!
//! Layout:
//! * this file: pure, platform independent encoding + change detection +
//!   MTU chunking (unit tested everywhere);
//! * `bluez` (Linux only): registers the application/advertisement;
//! * non-Linux stub: [`spawn`] logs and returns, so the crate builds on macOS.
//!
//! GATT layout (all characteristics read + notify, no writes):
//! * `STATE_UUID`   utf8 arena state, e.g. `fight`
//! * `SUMMARY_UUID` compact JSON, <= [`MAX_SUMMARY_BYTES`], see [`encode_summary`]
//! * `HEALTH_UUID`  device health bitmap, see [`encode_health`]
//!
//! Hook for later: a writable control characteristic can be added next to
//! these in `bluez.rs` (currently no characteristic has a write handler).

// The encoding layer is only consumed by the Linux BlueZ glue (and tests).
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use serde::Serialize;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

#[cfg(target_os = "linux")]
mod bluez;

pub const SERVICE_UUID: &str = "7b1f0000-5a57-4c1b-9d0e-534852415744";
pub const STATE_UUID: &str = "7b1f0001-5a57-4c1b-9d0e-534852415744";
pub const SUMMARY_UUID: &str = "7b1f0002-5a57-4c1b-9d0e-534852415744";
pub const HEALTH_UUID: &str = "7b1f0003-5a57-4c1b-9d0e-534852415744";

/// Hard cap of the summary characteristic (max GATT attribute length is 512).
pub const MAX_SUMMARY_BYTES: usize = 512;
/// Devices beyond this are not represented in the bitmap.
pub const MAX_HEALTH_DEVICES: usize = 64;
/// ATT notification overhead (opcode + handle).
pub const ATT_OVERHEAD: usize = 3;
/// Default ATT MTU when none was negotiated.
pub const DEFAULT_MTU: usize = 23;

/// Health of one device in the snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceFlag {
    #[serde(rename = "n")]
    pub name: String,
    #[serde(rename = "c")]
    pub connected: bool,
    #[serde(rename = "s")]
    pub stale: bool,
}

/// Everything the GATT service publishes, gathered from the existing sources
/// (arena state, energy bar, fight stats, schedule, presence, status registry).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StatusSnapshot {
    /// Lowercase arena state: suspended / idle / warming / fight / cooldown.
    pub state: String,
    pub energy_pct: Option<f32>,
    pub fight_remaining_sec: Option<f64>,
    pub punches: u32,
    pub max_power: f32,
    pub total_sessions: u64,
    pub schedule_active: bool,
    pub presence: bool,
    pub uptime_sec: f64,
    /// Stable order: ble, audio, music, then wled by ip, then tasmota by name.
    pub devices: Vec<DeviceFlag>,
}

/// Async source of snapshots, supplied by `main` so this module stays
/// decoupled from the API state.
pub type SnapshotFn = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = StatusSnapshot> + Send>> + Send + Sync>;

/// Characteristic values derived from one snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoded {
    pub state: Vec<u8>,
    pub summary: Vec<u8>,
    pub health: Vec<u8>,
}

/// Which characteristics changed versus the previous encoding (all three on the first call).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Changed {
    pub state: bool,
    pub summary: bool,
    pub health: bool,
}

impl Changed {
    pub fn any(&self) -> bool {
        self.state || self.summary || self.health
    }
}

pub fn encode(s: &StatusSnapshot) -> Encoded {
    Encoded { state: encode_state(s), summary: encode_summary(s), health: encode_health(&s.devices) }
}

pub fn diff(prev: Option<&Encoded>, new: &Encoded) -> Changed {
    match prev {
        None => Changed { state: true, summary: true, health: true },
        Some(p) => Changed {
            state: p.state != new.state,
            summary: p.summary != new.summary,
            health: p.health != new.health,
        },
    }
}

pub fn encode_state(s: &StatusSnapshot) -> Vec<u8> {
    if s.state.is_empty() {
        b"unknown".to_vec()
    } else {
        s.state.as_bytes().to_vec()
    }
}

/// Health bitmap: byte 0 = device count (clamped to [`MAX_HEALTH_DEVICES`]),
/// then 2 bits per device, device `i` in byte `1 + i/4`, shift `(i%4)*2`:
/// bit0 = connected, bit1 = stale. Device order is that of `StatusSnapshot::devices`
/// (names are in the summary JSON when it fits).
pub fn encode_health(devices: &[DeviceFlag]) -> Vec<u8> {
    let n = devices.len().min(MAX_HEALTH_DEVICES);
    let mut out = vec![0u8; 1 + n.div_ceil(4)];
    out[0] = n as u8;
    for (i, d) in devices.iter().take(n).enumerate() {
        let bits = (d.connected as u8) | ((d.stale as u8) << 1);
        out[1 + i / 4] |= bits << ((i % 4) * 2);
    }
    out
}

#[derive(Serialize)]
struct Summary<'a> {
    s: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    e: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rem: Option<u32>,
    p: u32,
    sch: u8,
    pr: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    mp: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ts: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    up: Option<u64>,
    /// devices connected / total / stale
    dc: usize,
    dn: usize,
    ds: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    d: Option<&'a [DeviceFlag]>,
}

/// Compact summary JSON, always valid and <= [`MAX_SUMMARY_BYTES`].
///
/// Truncation rule (applied in order until it fits):
/// 1. drop the per-device list `d` (the health bitmap still has every device),
/// 2. drop `up` (uptime), `ts` (total sessions) and `mp` (max power),
/// 3. as a last resort emit only `{"s":..}` with the state truncated.
///
/// The counts `dc`/`dn`/`ds` (connected/total/stale) are kept in steps 1-2.
pub fn encode_summary(s: &StatusSnapshot) -> Vec<u8> {
    let state = if s.state.is_empty() { "unknown" } else { s.state.as_str() };
    let mut sum = Summary {
        s: state,
        e: s.energy_pct.map(|v| v.clamp(0.0, 100.0).round() as u32),
        rem: s.fight_remaining_sec.map(|v| v.max(0.0).round() as u32),
        p: s.punches,
        sch: s.schedule_active as u8,
        pr: s.presence as u8,
        mp: Some(s.max_power.max(0.0).round() as u32),
        ts: Some(s.total_sessions),
        up: Some(s.uptime_sec.max(0.0) as u64),
        dc: s.devices.iter().filter(|d| d.connected).count(),
        dn: s.devices.len(),
        ds: s.devices.iter().filter(|d| d.stale).count(),
        d: Some(&s.devices),
    };
    let ser = |x: &Summary| serde_json::to_vec(x).unwrap_or_default();
    let mut out = ser(&sum);
    if out.len() <= MAX_SUMMARY_BYTES {
        return out;
    }
    sum.d = None;
    out = ser(&sum);
    if out.len() <= MAX_SUMMARY_BYTES {
        return out;
    }
    sum.up = None;
    sum.ts = None;
    sum.mp = None;
    out = ser(&sum);
    if out.len() <= MAX_SUMMARY_BYTES {
        return out;
    }
    // Only reachable with an absurdly long state string.
    let short: String = state.chars().take(64).collect();
    serde_json::to_vec(&serde_json::json!({ "s": short })).unwrap_or_default()
}

/// Split a value into notification-sized frames for the negotiated `mtu`.
///
/// Values that fit in one notification (`mtu - 3` bytes) are sent as-is;
/// larger ones use frames `[index, total, payload...]` so a client can
/// reassemble them. Reads are never framed (BlueZ serves long reads itself).
pub fn chunk_frames(value: &[u8], mtu: usize) -> Vec<Vec<u8>> {
    let max = mtu.saturating_sub(ATT_OVERHEAD).max(1);
    if value.len() <= max {
        return vec![value.to_vec()];
    }
    let mut payload = max.saturating_sub(2).max(1);
    // index/total are single bytes: widen the payload for pathological MTUs.
    if value.len().div_ceil(payload) > 255 {
        payload = value.len().div_ceil(255);
    }
    let total = value.len().div_ceil(payload);
    value
        .chunks(payload)
        .enumerate()
        .map(|(i, c)| {
            let mut f = Vec::with_capacity(c.len() + 2);
            f.push(i as u8);
            f.push(total as u8);
            f.extend_from_slice(c);
            f
        })
        .collect()
}

/// `ShadowWarrior-1A2B` from a prefix and the last two address bytes.
pub fn local_name(prefix: &str, addr_tail: [u8; 2]) -> String {
    format!("{}-{:02X}{:02X}", prefix, addr_tail[0], addr_tail[1])
}

/// Handle to the running service; [`BleStatusHandle::shutdown`] unregisters
/// the advertisement and GATT application cleanly.
pub struct BleStatusHandle {
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    stop: tokio::sync::watch::Sender<bool>,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    task: tokio::task::JoinHandle<()>,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl BleStatusHandle {
    pub async fn shutdown(self) {
        let _ = self.stop.send(true);
        // Bounded: never hang daemon shutdown on a wedged bluetoothd.
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), self.task).await;
    }
}

/// Start the service in the background. Never blocks and never fails: a
/// missing BlueZ/adapter is logged by the task and the daemon carries on.
#[cfg(target_os = "linux")]
pub fn spawn(
    cfg: crate::config::BleStatusConfig,
    ble_adapter: Option<String>,
    snapshot: SnapshotFn,
) -> Option<BleStatusHandle> {
    let (stop, stop_rx) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(async move {
        if let Err(e) = bluez::run(cfg, ble_adapter, snapshot, stop_rx).await {
            tracing::warn!("BLE status service not running: {e:#}");
        }
    });
    Some(BleStatusHandle { stop, task })
}

/// Non-Linux stub: BlueZ does not exist here.
#[cfg(not(target_os = "linux"))]
pub fn spawn(
    _cfg: crate::config::BleStatusConfig,
    _ble_adapter: Option<String>,
    _snapshot: SnapshotFn,
) -> Option<BleStatusHandle> {
    tracing::warn!("ble_status is enabled but the BLE GATT status service is Linux/BlueZ only; skipping");
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(n: &str, c: bool, s: bool) -> DeviceFlag {
        DeviceFlag { name: n.into(), connected: c, stale: s }
    }

    fn snap() -> StatusSnapshot {
        StatusSnapshot {
            state: "fight".into(),
            energy_pct: Some(99.6),
            fight_remaining_sec: Some(61.4),
            punches: 12,
            max_power: 7.8,
            total_sessions: 40,
            schedule_active: true,
            presence: true,
            uptime_sec: 3600.9,
            devices: vec![dev("ble", true, false), dev("audio", true, true), dev("music", false, false)],
        }
    }

    #[test]
    fn state_bytes() {
        assert_eq!(encode_state(&snap()), b"fight");
        assert_eq!(encode_state(&StatusSnapshot::default()), b"unknown");
    }

    #[test]
    fn summary_is_compact_valid_json() {
        let v: serde_json::Value = serde_json::from_slice(&encode_summary(&snap())).unwrap();
        assert_eq!(v["s"], "fight");
        assert_eq!(v["e"], 100);
        assert_eq!(v["rem"], 61);
        assert_eq!(v["p"], 12);
        assert_eq!(v["sch"], 1);
        assert_eq!(v["pr"], 1);
        assert_eq!(v["dc"], 2);
        assert_eq!(v["dn"], 3);
        assert_eq!(v["ds"], 1);
        assert_eq!(v["d"][1]["n"], "audio");
        assert_eq!(v["d"][1]["s"], true);
    }

    #[test]
    fn summary_omits_absent_options() {
        let mut s = snap();
        s.energy_pct = None;
        s.fight_remaining_sec = None;
        let v: serde_json::Value = serde_json::from_slice(&encode_summary(&s)).unwrap();
        assert!(v.get("e").is_none() && v.get("rem").is_none());
    }

    #[test]
    fn summary_drops_device_list_first_when_too_big() {
        let mut s = snap();
        s.devices = (0..40).map(|i| dev(&format!("wled-192.168.1.{i}"), i % 2 == 0, false)).collect();
        let out = encode_summary(&s);
        assert!(out.len() <= MAX_SUMMARY_BYTES);
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert!(v.get("d").is_none());
        // step 1 only: optional stats still present
        assert!(v.get("up").is_some() && v.get("ts").is_some());
        assert_eq!(v["dn"], 40);
        assert_eq!(v["dc"], 20);
    }

    #[test]
    fn summary_last_resort_with_huge_state() {
        let mut s = snap();
        s.state = "x".repeat(2000);
        let out = encode_summary(&s);
        assert!(out.len() <= MAX_SUMMARY_BYTES);
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["s"].as_str().unwrap().len(), 64);
    }

    #[test]
    fn summary_clamps_and_handles_negatives() {
        let mut s = snap();
        s.energy_pct = Some(250.0);
        s.fight_remaining_sec = Some(-3.0);
        s.max_power = -1.0;
        let v: serde_json::Value = serde_json::from_slice(&encode_summary(&s)).unwrap();
        assert_eq!(v["e"], 100);
        assert_eq!(v["rem"], 0);
        assert_eq!(v["mp"], 0);
    }

    #[test]
    fn health_bitmap_layout() {
        // ble: c, audio: c+stale, music: none
        assert_eq!(encode_health(&snap().devices), vec![3, 0b0000_1101]);
        // 5 devices -> 2 bytes of bits; 5th device sits in byte 2 bits 0-1
        let d: Vec<_> = (0..5).map(|i| dev("x", true, i == 4)).collect();
        assert_eq!(encode_health(&d), vec![5, 0b0101_0101, 0b0000_0011]);
        assert_eq!(encode_health(&[]), vec![0]);
    }

    #[test]
    fn health_bitmap_caps_devices() {
        let d: Vec<_> = (0..100).map(|_| dev("x", true, false)).collect();
        let b = encode_health(&d);
        assert_eq!(b[0] as usize, MAX_HEALTH_DEVICES);
        assert_eq!(b.len(), 1 + MAX_HEALTH_DEVICES / 4);
    }

    #[test]
    fn diff_first_then_only_changes() {
        let a = encode(&snap());
        assert_eq!(diff(None, &a), Changed { state: true, summary: true, health: true });
        assert!(!diff(Some(&a), &a).any());
        let mut s = snap();
        s.punches += 1;
        let b = encode(&s);
        assert_eq!(diff(Some(&a), &b), Changed { state: false, summary: true, health: false });
        s.devices[2].connected = true;
        let c = encode(&s);
        assert!(diff(Some(&b), &c).health);
        s.state = "cooldown".into();
        assert!(diff(Some(&c), &encode(&s)).state);
    }

    #[test]
    fn chunk_single_when_fits() {
        assert_eq!(chunk_frames(b"abcd", 23), vec![b"abcd".to_vec()]);
        assert_eq!(chunk_frames(&[1u8; 20], 23).len(), 1);
        assert_eq!(chunk_frames(b"", 23), vec![Vec::<u8>::new()]);
    }

    #[test]
    fn chunk_frames_reassemble() {
        let v: Vec<u8> = (0..=255u8).cycle().take(500).collect();
        for mtu in [23usize, 50, 185, 247, 517] {
            let frames = chunk_frames(&v, mtu);
            if v.len() <= mtu - 3 {
                assert_eq!(frames, vec![v.clone()]);
                continue;
            }
            let mut out = Vec::new();
            for (i, f) in frames.iter().enumerate() {
                assert!(f.len() <= mtu - 3, "mtu {mtu}");
                assert_eq!(f[0] as usize, i);
                assert_eq!(f[1] as usize, frames.len());
                out.extend_from_slice(&f[2..]);
            }
            assert_eq!(out, v, "mtu {mtu}");
        }
    }

    #[test]
    fn chunk_degenerate_mtu_still_terminates() {
        let v = vec![7u8; 600];
        let frames = chunk_frames(&v, 0);
        assert!(frames.len() <= 255);
        let joined: Vec<u8> = frames.iter().flat_map(|f| f[2..].to_vec()).collect();
        assert_eq!(joined, v);
    }

    #[test]
    fn names() {
        assert_eq!(local_name("ShadowWarrior", [0x1a, 0x2b]), "ShadowWarrior-1A2B");
    }

    #[test]
    fn config_defaults_disabled() {
        let c = crate::config::BleStatusConfig::default();
        assert!(!c.enabled);
        let parsed: crate::config::BleStatusConfig = serde_yaml::from_str("enabled: true").unwrap();
        assert!(parsed.enabled && parsed.name_prefix == "ShadowWarrior" && parsed.poll_ms == 1000);
    }

    #[test]
    fn uuids_are_valid() {
        for u in [SERVICE_UUID, STATE_UUID, SUMMARY_UUID, HEALTH_UUID] {
            uuid::Uuid::parse_str(u).unwrap();
        }
    }
}
