//! Boot modes of the station.
//!
//! The classic ESP32 does not have the RAM to run Wi-Fi and the BLE stack at the same time next to the
//! DSP buffers, so a boot is either `Normal` (Wi-Fi only, Bluetooth memory returned to the heap) or
//! `Provision` (BLE only, waiting for the brain to write the Wi-Fi settings, then reboot into `Normal`).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Provision,
}

/// `configured`: Wi-Fi settings are stored. `button`: BOOT held at power-up. `flag`: one-shot flag left by the
/// previous boot (Wi-Fi kept failing, or a BLE command asked for provisioning).
pub fn boot_mode(configured: bool, button: bool, flag: bool) -> Mode {
    if !configured || button || flag {
        Mode::Provision
    } else {
        Mode::Normal
    }
}

/// How long the Wi-Fi link may stay failed before the station reboots into provisioning (wrong password or
/// a moved access point), and how long a provisioning boot with stored settings waits for the brain.
pub const FAILED_BEFORE_PROVISION_MS: u32 = 5 * 60_000;
pub const PROVISION_WINDOW_MS: u32 = 10 * 60_000;

/// In provisioning mode with settings already stored (button or flag), give up and retry Wi-Fi after the window.
pub fn provision_expired(configured: bool, uptime_ms: u32) -> bool {
    configured && uptime_ms >= PROVISION_WINDOW_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes() {
        assert_eq!(boot_mode(true, false, false), Mode::Normal);
        assert_eq!(boot_mode(false, false, false), Mode::Provision);
        assert_eq!(boot_mode(true, true, false), Mode::Provision);
        assert_eq!(boot_mode(true, false, true), Mode::Provision);
    }

    #[test]
    fn expiry_only_with_stored_settings() {
        assert!(!provision_expired(false, u32::MAX));
        assert!(!provision_expired(true, PROVISION_WINDOW_MS - 1));
        assert!(provision_expired(true, PROVISION_WINDOW_MS));
    }
}
