//! Link-health tracking for WLED controllers. WLED has no push channel, so the
//! manager reports the outcome of each HTTP/UDP call here; after
//! `FAIL_THRESHOLD` consecutive failures the controller is published as
//! disconnected, and the next success publishes a reconnect. Publishes events
//! only; the status registry folds them.
use std::collections::HashMap;
use std::sync::Mutex;

use crate::core::{Event, SharedEventBus};

/// Consecutive failures before a controller is declared disconnected.
pub(super) const FAIL_THRESHOLD: u32 = 3;

#[derive(Default)]
struct Entry {
    fails: u32,
    down: bool,
}

pub(super) struct LinkHealth {
    bus: SharedEventBus,
    threshold: u32,
    inner: Mutex<HashMap<String, Entry>>,
}

impl LinkHealth {
    pub(super) fn new(bus: SharedEventBus) -> Self {
        Self::with_threshold(bus, FAIL_THRESHOLD)
    }

    pub(super) fn with_threshold(bus: SharedEventBus, threshold: u32) -> Self {
        Self { bus, threshold, inner: Mutex::new(HashMap::new()) }
    }

    /// A call to `ip` succeeded. Always publishes `WledConnected`, which
    /// refreshes `last_seen` in the registry (and is a no-op in the manager's
    /// own event loop for already-known controllers).
    pub(super) fn ok(&self, ip: &str) {
        let mut m = self.inner.lock().unwrap();
        let e = m.entry(ip.to_string()).or_default();
        e.fails = 0;
        e.down = false;
        drop(m);
        self.bus.publish(Event::WledConnected { ip: ip.to_string() });
    }

    /// A call to `ip` failed.
    pub(super) fn fail(&self, ip: &str, error: impl std::fmt::Display) {
        let mut m = self.inner.lock().unwrap();
        let e = m.entry(ip.to_string()).or_default();
        e.fails += 1;
        let go_down = e.fails >= self.threshold && !e.down;
        if go_down {
            e.down = true;
        }
        drop(m);
        self.bus.publish(Event::WledError { ip: ip.to_string(), error: error.to_string() });
        if go_down {
            self.bus.publish(Event::WledDisconnected { ip: ip.to_string() });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::EventBus;
    use std::sync::Arc;

    fn drain(rx: &mut tokio::sync::broadcast::Receiver<Event>) -> Vec<String> {
        let mut v = Vec::new();
        while let Ok(e) = rx.try_recv() {
            v.push(e.event_type().to_string());
        }
        v
    }

    fn setup(threshold: u32) -> (LinkHealth, tokio::sync::broadcast::Receiver<Event>) {
        let bus: SharedEventBus = Arc::new(EventBus::new(64));
        let rx = bus.subscribe();
        (LinkHealth::with_threshold(bus, threshold), rx)
    }

    #[test]
    fn disconnects_after_threshold_then_reconnects() {
        let (h, mut rx) = setup(3);
        h.fail("1.1.1.1", "timeout");
        h.fail("1.1.1.1", "timeout");
        assert_eq!(drain(&mut rx), ["WledError", "WledError"]);
        h.fail("1.1.1.1", "timeout");
        assert_eq!(drain(&mut rx), ["WledError", "WledDisconnected"]);
        // already down: errors keep flowing, no duplicate disconnect
        h.fail("1.1.1.1", "timeout");
        assert_eq!(drain(&mut rx), ["WledError"]);
        h.ok("1.1.1.1");
        assert_eq!(drain(&mut rx), ["WledConnected"]);
        // and it can go down again afterwards
        for _ in 0..3 {
            h.fail("1.1.1.1", "timeout");
        }
        assert!(drain(&mut rx).contains(&"WledDisconnected".to_string()));
    }

    #[test]
    fn success_resets_failure_count() {
        let (h, mut rx) = setup(3);
        h.fail("a", "x");
        h.fail("a", "x");
        h.ok("a");
        h.fail("a", "x");
        h.fail("a", "x");
        assert!(!drain(&mut rx).contains(&"WledDisconnected".to_string()));
    }

    #[test]
    fn controllers_are_tracked_independently() {
        let (h, mut rx) = setup(1);
        h.fail("a", "x");
        h.ok("b");
        assert_eq!(drain(&mut rx), ["WledError", "WledDisconnected", "WledConnected"]);
    }
}
