//! Finding the brain after its IP changed (new router, DHCP lease).
//!
//! The brain's address is stored in flash at provisioning time, so a moved brain would leave the
//! station talking to nobody. Two rules fix that without any extra protocol:
//!
//! * **Broadcast when unanswered.** The brain sends a command frame about once a second. When none has
//!   arrived for [`SILENCE_MS`] (or no brain address is known at all), the station also sends its frames to the
//!   subnet broadcast address on the brain's UDP port. The brain listens on `0.0.0.0` and answers whoever sent.
//! * **Adopt the commander.** A command frame arriving from an address other than the stored brain makes
//!   that address the new brain (same port), saved to flash.
//!
//! No authentication: any host on the LAN that sends commands becomes the brain.

/// No command from the brain for this long: start broadcasting.
pub const SILENCE_MS: u32 = 5_000;

/// Milliseconds since `then`, wrap-safe (`u32` millisecond counters wrap after 49 days).
fn since(now_ms: u32, then_ms: u32) -> u32 {
    now_ms.wrapping_sub(then_ms)
}

/// Should frames also go to the broadcast address?
///
/// `up_since_ms`: when the Wi-Fi link came up. `last_cmd_ms`: when the last command frame arrived (any
/// time, possibly before the link came up: only commands after `up_since_ms` count).
pub fn broadcast_wanted(has_brain: bool, now_ms: u32, up_since_ms: u32, last_cmd_ms: Option<u32>) -> bool {
    if !has_brain {
        return true;
    }
    let last = match last_cmd_ms {
        Some(t) if (t.wrapping_sub(up_since_ms) as i32) >= 0 => t,
        _ => up_since_ms,
    };
    since(now_ms, last) > SILENCE_MS
}

/// A brain address is `ip:port`; `ip` as four octets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Brain {
    pub ip: [u8; 4],
    pub port: u16,
}

/// A command frame came from `from_ip`. Returns the new brain address if it differs from the current one
/// (the port stays the stored one, or `default_port` when there is none).
pub fn adopt(current: Option<Brain>, from_ip: [u8; 4], default_port: u16) -> Option<Brain> {
    let port = current.map_or(default_port, |b| b.port);
    let new = Brain { ip: from_ip, port };
    if from_ip == [0, 0, 0, 0] || current == Some(new) {
        None
    } else {
        Some(new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_brain_always_broadcasts() {
        assert!(broadcast_wanted(false, 100, 0, Some(90)));
    }

    #[test]
    fn quiet_after_link_up_then_broadcast() {
        assert!(!broadcast_wanted(true, 4_000, 0, None));
        assert!(!broadcast_wanted(true, 5_000, 0, None));
        assert!(broadcast_wanted(true, 5_001, 0, None));
    }

    #[test]
    fn commands_keep_it_quiet() {
        assert!(!broadcast_wanted(true, 20_000, 0, Some(19_000)));
        assert!(broadcast_wanted(true, 26_000, 0, Some(19_000)));
    }

    #[test]
    fn command_from_before_link_up_is_ignored() {
        // link came up at 30 s; the last command was at 10 s, before an outage
        assert!(!broadcast_wanted(true, 32_000, 30_000, Some(10_000)));
        assert!(broadcast_wanted(true, 36_000, 30_000, Some(10_000)));
    }

    #[test]
    fn wraps_around() {
        let up = u32::MAX - 1_000;
        assert!(!broadcast_wanted(true, 1_000, up, Some(u32::MAX - 500)));
        assert!(broadcast_wanted(true, 9_000, up, Some(u32::MAX - 500)));
    }

    #[test]
    fn adopt_new_address_keeps_port() {
        let cur = Brain { ip: [10, 0, 0, 36], port: 4210 };
        assert_eq!(adopt(Some(cur), [192, 168, 1, 238], 9999), Some(Brain { ip: [192, 168, 1, 238], port: 4210 }));
    }

    #[test]
    fn adopt_same_address_is_none() {
        let cur = Brain { ip: [192, 168, 1, 238], port: 4210 };
        assert_eq!(adopt(Some(cur), [192, 168, 1, 238], 4210), None);
    }

    #[test]
    fn adopt_without_brain_uses_default_port() {
        assert_eq!(adopt(None, [192, 168, 1, 5], 4210), Some(Brain { ip: [192, 168, 1, 5], port: 4210 }));
    }

    #[test]
    fn adopt_ignores_unspecified() {
        assert_eq!(adopt(None, [0, 0, 0, 0], 4210), None);
    }
}
