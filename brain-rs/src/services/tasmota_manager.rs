use anyhow::Result;
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use tracing::{error, info, warn};

use crate::config::{TasmotaConfig, TasmotaPlug};
use crate::core::{ArenaState, Event, SharedEventBus};

/// Desired power for `plug` when the arena enters `state`.
/// Plugs listing `on_states` are ON in those states and OFF otherwise.
/// Plugs with no `on_states` are only forced OFF in Idle/Suspended.
pub fn desired_power(plug: &TasmotaPlug, state: ArenaState) -> Option<bool> {
    if plug.on_states.is_empty() {
        return matches!(state, ArenaState::Idle | ArenaState::Suspended).then_some(false);
    }
    let on = plug
        .on_states
        .iter()
        .any(|s| s.trim().eq_ignore_ascii_case(&state.to_string()));
    Some(on)
}

pub struct TasmotaManager {
    config: TasmotaConfig,
    event_bus: SharedEventBus,
    http_client: Client,
    states: Arc<RwLock<Vec<PlugState>>>,
}

#[derive(Debug, Clone)]
struct PlugState {
    _name: String,
    ip: String,
    powered: bool,
    connected: bool,
}

impl TasmotaManager {
    pub fn new(config: TasmotaConfig, event_bus: SharedEventBus) -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(3))
            .connect_timeout(Duration::from_secs(2))
            .build()
            .expect("Failed to create HTTP client");

        for p in &config.plugs {
            for st in &p.on_states {
                if st.to_uppercase().parse::<ArenaState>().is_err() {
                    warn!("Tasmota plug {}: unknown state '{}' in on_states", p.name, st);
                }
            }
        }

        let states = config
            .plugs
            .iter()
            .map(|p| PlugState {
                _name: p.name.clone(),
                ip: p.ip.clone(),
                powered: false,
                connected: false,
            })
            .collect();

        Self {
            config,
            event_bus,
            http_client,
            states: Arc::new(RwLock::new(states)),
        }
    }

    /// Apply the per-state plug mapping. Plugs are driven concurrently, each
    /// with a bounded retry, so one dead plug cannot delay the others.
    /// Returns the number of plugs that could not be set.
    pub async fn apply_state(&self, state: ArenaState) -> usize {
        let futs = self.config.plugs.iter().filter_map(|p| {
            desired_power(p, state).map(|on| async move { self.set_plug_retry(p, on).await })
        });
        futures::future::join_all(futs)
            .await
            .into_iter()
            .filter(|ok| !ok)
            .count()
    }

    /// Set a plug, retrying once. Never returns an error: failures are logged
    /// and published as TasmotaError events. Returns success.
    async fn set_plug_retry(&self, plug: &TasmotaPlug, power_on: bool) -> bool {
        for attempt in 1..=2 {
            match self.set_plug(&plug.ip, &plug.name, power_on).await {
                Ok(()) => return true,
                Err(e) => warn!(
                    "Tasmota {} -> {} failed (attempt {}/2): {}",
                    plug.name,
                    if power_on { "on" } else { "off" },
                    attempt,
                    e
                ),
            }
        }
        false
    }

    /// Turn a specific plug on or off
    async fn set_plug(&self, ip: &str, name: &str, power_on: bool) -> Result<()> {
        let command = if power_on { "on" } else { "off" };
        let url = format!("http://{}/cm?cmnd=Power%20{}", ip, command);

        match self.http_client.get(&url).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    info!("Set {} to {}", name, command);

                    // Update state
                    let mut states = self.states.write().await;
                    if let Some(state) = states.iter_mut().find(|s| s.ip == ip) {
                        state.powered = power_on;
                        state.connected = true;
                    }

                    self.event_bus.publish(Event::TasmotaStateChanged {
                        name: name.to_string(),
                        powered: power_on,
                    });

                    Ok(())
                } else {
                    Err(anyhow::anyhow!("HTTP status: {}", resp.status()))
                }
            }
            Err(e) => {
                // Update connection state
                let mut states = self.states.write().await;
                if let Some(state) = states.iter_mut().find(|s| s.ip == ip) {
                    state.connected = false;
                }

                self.event_bus.publish(Event::TasmotaError {
                    name: name.to_string(),
                    error: e.to_string(),
                });

                Err(e.into())
            }
        }
    }

    #[cfg(test)]
    /// Get current states for all plugs
    pub async fn get_states(&self) -> Vec<(String, String, bool, bool)> {
        let states = self.states.read().await;
        states
            .iter()
            .map(|s| (s._name.clone(), s.ip.clone(), s.connected, s.powered))
            .collect()
    }

    /// Subscribe to StateChanged events and control plugs autonomously.
    pub fn start_event_loop(self: Arc<Self>) {
        let mut event_rx = self.event_bus.subscribe();
        tokio::spawn(async move {
            loop {
                match event_rx.recv().await {
                    Ok(Event::StateChanged { to, .. }) => {
                        if let Ok(state) = to.parse::<ArenaState>() {
                            let failed = self.apply_state(state).await;
                            if failed > 0 {
                                error!("Tasmota: {} plug(s) failed for state {}", failed, state);
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Tasmota event loop lagged, skipped {} events", n);
                    }
                    Err(e) => {
                        warn!("Tasmota event bus error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::EventBus;
    use crate::test_util::{mock_http, Seen};

    fn plug(name: &str, ip: &str, on: &[&str]) -> TasmotaPlug {
        TasmotaPlug {
            ip: ip.into(),
            name: name.into(),
            on_states: on.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn mapping_on_states() {
        let p = plug("uv", "x", &["warming", "FIGHT"]);
        assert_eq!(desired_power(&p, ArenaState::Warming), Some(true));
        assert_eq!(desired_power(&p, ArenaState::Fight), Some(true));
        assert_eq!(desired_power(&p, ArenaState::Cooldown), Some(false));
        assert_eq!(desired_power(&p, ArenaState::Idle), Some(false));
        assert_eq!(desired_power(&p, ArenaState::Suspended), Some(false));
    }

    #[test]
    fn mapping_legacy_plug() {
        let p = plug("p", "x", &[]);
        assert_eq!(desired_power(&p, ArenaState::Fight), None);
        assert_eq!(desired_power(&p, ArenaState::Idle), Some(false));
        assert_eq!(desired_power(&p, ArenaState::Suspended), Some(false));
    }

    /// Mock plug; returns its address and the log of requests received.
    async fn mock() -> (String, Seen) {
        let (addr, _, seen) = mock_http(&[("*", 200, "{}")]).await;
        (addr, seen)
    }

    /// The `cmnd` values received so far, in order.
    async fn cmnds(seen: &Seen) -> Vec<String> {
        seen.lock()
            .await
            .iter()
            .filter_map(|r| {
                let q = r.lines().next()?.split_once("cmnd=")?.1;
                let v = q.split([' ', '&']).next()?;
                Some(v.replace("%20", " ").replace('+', " "))
            })
            .collect()
    }

    #[tokio::test]
    async fn apply_state_drives_mock_and_survives_dead_plug() {
        let (addr, seen) = mock().await;
        let cfg = TasmotaConfig {
            plugs: vec![
                plug("dead", "127.0.0.1:1", &["fight"]),
                plug("uv", &addr, &["warming", "fight"]),
            ],
        };
        let mgr = TasmotaManager::new(cfg, Arc::new(EventBus::new(16)));

        let failed = mgr.apply_state(ArenaState::Fight).await;
        assert_eq!(failed, 1);
        assert_eq!(cmnds(&seen).await, ["Power on"]);

        mgr.apply_state(ArenaState::Cooldown).await;
        assert_eq!(cmnds(&seen).await, ["Power on", "Power off"]);
        let st = mgr.get_states().await;
        assert!(st.iter().any(|s| s.0 == "uv" && s.2 && !s.3));
        assert!(st.iter().any(|s| s.0 == "dead" && !s.2));
    }
}
