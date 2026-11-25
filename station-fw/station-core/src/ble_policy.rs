//! When the station advertises its provisioning service over BLE.

/// Station state relevant to advertising.
#[derive(Clone, Copy, Debug)]
pub struct AdvInput {
    /// Wi-Fi settings are stored (flash or build time).
    pub configured: bool,
    pub link_up: bool,
    /// Milliseconds since the link was last up (or since boot if never).
    pub down_ms: u32,
    pub uptime_ms: u32,
    /// BOOT button was held at power-up: re-provisioning on purpose.
    pub button_override: bool,
}

pub const DOWN_ADVERTISE_MS: u32 = 60_000;
pub const OVERRIDE_WINDOW_MS: u32 = 10 * 60_000;

/// Unconfigured stations always advertise; a configured one only when Wi-Fi has been down for a minute
/// or for 10 minutes after a button-held boot. A connected station stays quiet (less 2.4 GHz traffic).
pub fn should_advertise(i: AdvInput) -> bool {
    if !i.configured {
        return true;
    }
    if i.link_up {
        return false;
    }
    i.down_ms >= DOWN_ADVERTISE_MS || (i.button_override && i.uptime_ms < OVERRIDE_WINDOW_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: AdvInput = AdvInput { configured: true, link_up: true, down_ms: 0, uptime_ms: 1_000_000, button_override: false };

    #[test]
    fn unconfigured_always_advertises() {
        assert!(should_advertise(AdvInput { configured: false, ..BASE }));
        assert!(should_advertise(AdvInput { configured: false, link_up: false, ..BASE }));
    }

    #[test]
    fn healthy_station_is_quiet() {
        assert!(!should_advertise(BASE));
        assert!(!should_advertise(AdvInput { button_override: true, uptime_ms: 1000, ..BASE }));
    }

    #[test]
    fn down_for_a_minute_advertises() {
        let down = AdvInput { link_up: false, ..BASE };
        assert!(!should_advertise(AdvInput { down_ms: 59_999, ..down }));
        assert!(should_advertise(AdvInput { down_ms: 60_000, ..down }));
    }

    #[test]
    fn button_window_is_ten_minutes() {
        let b = AdvInput { link_up: false, button_override: true, ..BASE };
        assert!(should_advertise(AdvInput { uptime_ms: 599_999, ..b }));
        assert!(!should_advertise(AdvInput { uptime_ms: 600_000, ..b }));
    }
}
