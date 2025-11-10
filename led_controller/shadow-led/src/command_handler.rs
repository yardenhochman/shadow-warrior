use std::sync::mpsc::{channel, Sender, Receiver};

#[derive(Debug, Clone)]
pub enum LedCommand {
    EnergyBar(u8),      // percentage 0-100
    EnergyPulse,
    Breathing,
    Idle,
    Electricity,        // Start electricity effect
}

pub struct CommandHandler {
    tx: Sender<LedCommand>,
    rx: Receiver<LedCommand>,
}

impl CommandHandler {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx,
        }
    }

    /// Get a sender for sending commands
    pub fn get_sender(&self) -> Sender<LedCommand> {
        self.tx.clone()
    }

    /// Try to receive a command (non-blocking)
    pub fn try_recv(&self) -> Option<LedCommand> {
        self.rx.try_recv().ok()
    }

    /// Parse a BLE command string into a LedCommand
    pub fn parse_ble_command(data: &[u8]) -> Option<LedCommand> {
        let command_str = std::str::from_utf8(data).ok()?.trim();
        let parts: Vec<&str> = command_str.split_whitespace().collect();

        if parts.is_empty() {
            return None;
        }

        match parts[0].to_lowercase().as_str() {
            "energy_bar" => {
                if parts.len() > 1 {
                    parts[1].parse::<u8>().ok().and_then(|p| {
                        if p <= 100 {
                            Some(LedCommand::EnergyBar(p))
                        } else {
                            None
                        }
                    })
                } else {
                    None
                }
            }
            "set_power" => {
                if parts.len() > 1 {
                    parts[1].parse::<u8>().ok().and_then(|p| {
                        if p <= 100 {
                            Some(LedCommand::EnergyBar(p))
                        } else {
                            None
                        }
                    })
                } else {
                    None
                }
            }
            "energy_pulse" => Some(LedCommand::EnergyPulse),
            "breath" | "breathing" => Some(LedCommand::Breathing),
            "electricity" => Some(LedCommand::Electricity),
            "idle" => Some(LedCommand::Idle),
            _ => {
                log::warn!("Unknown command: {}", command_str);
                None
            }
        }
    }

    /// Create a BLE callback handler
    pub fn create_ble_callback(tx: Sender<LedCommand>) -> impl Fn(&[u8]) + Send + Sync {
        move |data: &[u8]| {
            log::info!("Received BLE data: {} bytes", data.len());

            if let Some(command) = Self::parse_ble_command(data) {
                log::info!("Parsed command: {:?}", command);
                if let Err(e) = tx.send(command) {
                    log::error!("Failed to send command: {}", e);
                }
            } else {
                log::warn!("Failed to parse command from: {:?}", data);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_energy_bar() {
        let cmd = CommandHandler::parse_ble_command(b"energy_bar 50");
        assert!(matches!(cmd, Some(LedCommand::EnergyBar(50))));
    }

    #[test]
    fn test_parse_energy_pulse() {
        let cmd = CommandHandler::parse_ble_command(b"energy_pulse");
        assert!(matches!(cmd, Some(LedCommand::EnergyPulse)));
    }

    #[test]
    fn test_parse_breathing() {
        let cmd = CommandHandler::parse_ble_command(b"breath");
        assert!(matches!(cmd, Some(LedCommand::Breathing)));
    }

    #[test]
    fn test_parse_set_power() {
        let cmd = CommandHandler::parse_ble_command(b"set_power 75");
        assert!(matches!(cmd, Some(LedCommand::EnergyBar(75))));
    }

    #[test]
    fn test_parse_electricity() {
        let cmd = CommandHandler::parse_ble_command(b"electricity");
        assert!(matches!(cmd, Some(LedCommand::Electricity)));
    }

    #[test]
    fn test_parse_invalid() {
        let cmd = CommandHandler::parse_ble_command(b"invalid_command");
        assert!(cmd.is_none());
    }
}
