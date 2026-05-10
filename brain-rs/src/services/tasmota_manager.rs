use anyhow::Result;
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::config::TasmotaConfig;
use crate::core::{ArenaState, Event, SharedEventBus};

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
            .timeout(Duration::from_secs(5))
            .build()
            .expect("Failed to create HTTP client");

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

    /// Turn all plugs on or off
    pub async fn set_all(&self, power_on: bool) -> Result<()> {
        let command = if power_on { "on" } else { "off" };

        for plug in &self.config.plugs {
            if let Err(e) = self.set_plug(&plug.ip, &plug.name, power_on).await {
                error!("Failed to set {} to {}: {}", plug.name, command, e);
            }
        }

        Ok(())
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

    /// Get current states for all plugs
    #[allow(dead_code)]
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
                            match state {
                                ArenaState::Suspended | ArenaState::Idle => {
                                    if let Err(e) = self.set_all(false).await {
                                        error!("Tasmota set_all error: {}", e);
                                    }
                                }
                                _ => {}
                            }
                        }
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
