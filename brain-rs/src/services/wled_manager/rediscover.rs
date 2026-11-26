//! Finding a WLED controller again after its IP changed (new router, DHCP). Entries are
//! matched by mDNS instance/host name first, then by MAC (`/json/info` `mac` of each
//! candidate). The network side sits behind [`Locator`] so tests use a fake.
use async_trait::async_trait;
use std::time::{Duration, Instant};

use crate::config::WledController;
use crate::services::discovery_manager::{browse_wled, WledFound};

/// Consecutive failed contacts before a controller is looked for on the network.
pub(super) const REDISCOVER_AFTER_FAILS: u32 = 3;
/// At most one mDNS browse per this interval, however many controllers are down.
pub(super) const MIN_BROWSE_INTERVAL: Duration = Duration::from_secs(10);
const BROWSE_TIMEOUT: Duration = Duration::from_secs(3);

#[async_trait]
pub(crate) trait Locator: Send + Sync {
    /// Controllers currently announcing `_wled._tcp`.
    async fn browse(&self) -> Vec<WledFound>;
    /// MAC reported by the WLED at `ip` (`GET /json/info`), if reachable.
    async fn mac_at(&self, ip: &str) -> Option<String>;
}

/// mDNS browse plus a plain HTTP query.
pub(super) struct MdnsLocator {
    http: reqwest::Client,
}

impl MdnsLocator {
    pub(super) fn new() -> Self {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(2)).build().expect("http client");
        Self { http }
    }
}

#[async_trait]
impl Locator for MdnsLocator {
    async fn browse(&self) -> Vec<WledFound> {
        match browse_wled(BROWSE_TIMEOUT).await {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("WLED mDNS browse failed: {e}");
                Vec::new()
            }
        }
    }

    async fn mac_at(&self, ip: &str) -> Option<String> {
        let v: serde_json::Value = self.http.get(format!("http://{ip}/json/info")).send().await.ok()?.json().await.ok()?;
        v["mac"].as_str().map(str::to_string)
    }
}

/// Lowercase hex without separators, so "AA:BB:cc..." equals "aabbcc...".
pub(super) fn norm_mac(mac: &str) -> String {
    mac.chars().filter(|c| c.is_ascii_hexdigit()).collect::<String>().to_ascii_lowercase()
}

fn norm_name(n: &str) -> String {
    n.trim().trim_end_matches('.').trim_end_matches(".local").to_ascii_lowercase()
}

/// Rate limit for browsing: `allow` is true at most once per `min` interval.
pub(super) struct BrowseGate {
    min: Duration,
    last: std::sync::Mutex<Option<Instant>>,
}

impl BrowseGate {
    pub(super) fn new(min: Duration) -> Self {
        Self { min, last: Default::default() }
    }

    pub(super) fn allow(&self) -> bool {
        let mut last = self.last.lock().unwrap();
        if last.is_some_and(|t| t.elapsed() < self.min) {
            return false;
        }
        *last = Some(Instant::now());
        true
    }
}

pub(super) fn host_of(addr: &str) -> &str {
    addr.rsplit_once(':').map_or(addr, |(h, _)| h)
}

/// The new address of `c` among `found`, or `None` when it is not on the network.
/// `taken` holds addresses of the other configured controllers, never offered to `c`.
/// An old address with an HTTP port keeps one (the announced port).
pub(super) async fn locate(loc: &dyn Locator, c: &WledController, found: &[WledFound], taken: &[String]) -> Option<String> {
    let free = |f: &&WledFound| !taken.iter().any(|t| host_of(t) == f.ip);
    let mut hit: Option<WledFound> = None;
    if let Some(name) = c.name.as_deref().map(norm_name).filter(|n| !n.is_empty()) {
        hit = found.iter().filter(free).find(|f| norm_name(&f.instance) == name || norm_name(&f.hostname) == name).cloned();
    }
    if hit.is_none() {
        if let Some(want) = c.mac.as_deref().map(norm_mac).filter(|m| !m.is_empty()) {
            for f in found.iter().filter(free) {
                if loc.mac_at(&f.ip).await.is_some_and(|got| norm_mac(&got) == want) {
                    hit = Some(f.clone());
                    break;
                }
            }
        }
    }
    let hit = hit?;
    Some(if c.ip.contains(':') { format!("{}:{}", hit.ip, hit.port) } else { hit.ip })
}

#[cfg(test)]
pub(super) mod fake {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// Scripted network: what mDNS lists and which MAC each address reports.
    #[derive(Default)]
    pub struct FakeLocator {
        pub found: Mutex<Vec<WledFound>>,
        pub macs: Mutex<HashMap<String, String>>,
        pub browses: AtomicUsize,
    }

    pub fn found(instance: &str, ip: &str) -> WledFound {
        WledFound {
            fullname: format!("{instance}._wled._tcp.local."),
            instance: instance.into(),
            hostname: instance.into(),
            ip: ip.into(),
            port: 80,
        }
    }

    #[async_trait]
    impl Locator for FakeLocator {
        async fn browse(&self) -> Vec<WledFound> {
            self.browses.fetch_add(1, Ordering::SeqCst);
            self.found.lock().unwrap().clone()
        }
        async fn mac_at(&self, ip: &str) -> Option<String> {
            self.macs.lock().unwrap().get(host_of(ip)).cloned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;

    fn entry(ip: &str, name: Option<&str>, mac: Option<&str>) -> WledController {
        WledController { ip: ip.into(), name: name.map(Into::into), mac: mac.map(Into::into), ..Default::default() }
    }

    #[tokio::test]
    async fn matches_by_name_case_insensitive() {
        let loc = FakeLocator::default();
        let f = vec![found("wled-other", "10.0.0.9"), found("WLED-AB12CD", "10.0.0.7")];
        let c = entry("192.168.1.5", Some("wled-ab12cd"), None);
        assert_eq!(locate(&loc, &c, &f, &[]).await.as_deref(), Some("10.0.0.7"));
    }

    #[tokio::test]
    async fn matches_by_mac_when_name_is_unknown() {
        let loc = FakeLocator::default();
        loc.macs.lock().unwrap().insert("10.0.0.8".into(), "AA:BB:CC:DD:EE:FF".into());
        loc.macs.lock().unwrap().insert("10.0.0.9".into(), "112233445566".into());
        let f = vec![found("x", "10.0.0.9"), found("y", "10.0.0.8")];
        let c = entry("192.168.1.5", None, Some("aabbccddeeff"));
        assert_eq!(locate(&loc, &c, &f, &[]).await.as_deref(), Some("10.0.0.8"));
    }

    #[tokio::test]
    async fn no_match_gives_none() {
        let loc = FakeLocator::default();
        loc.macs.lock().unwrap().insert("10.0.0.9".into(), "112233445566".into());
        let f = vec![found("other", "10.0.0.9")];
        assert_eq!(locate(&loc, &entry("a", Some("wled-ab12cd"), Some("aabbccddeeff")), &f, &[]).await, None);
        assert_eq!(locate(&loc, &entry("a", None, None), &f, &[]).await, None);
    }

    #[tokio::test]
    async fn never_steals_another_controllers_address() {
        let loc = FakeLocator::default();
        let f = vec![found("wled-a", "10.0.0.7")];
        let c = entry("a", Some("wled-a"), None);
        assert_eq!(locate(&loc, &c, &f, &["10.0.0.7:80".to_string()]).await, None);
    }

    #[test]
    fn gate_allows_one_browse_per_interval() {
        let g = BrowseGate::new(Duration::from_secs(10));
        assert!(g.allow());
        assert!(!g.allow());
        let g = BrowseGate::new(Duration::ZERO);
        assert!(g.allow() && g.allow());
    }
}
