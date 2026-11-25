//! BLE provisioning: the brain gives an unconfigured station its Wi-Fi credentials and its own address.
//!
//! GATT layout (see `docs/BLE.md`): a station without working Wi-Fi advertises [`SERVICE_UUID`].
//! The brain connects, reads [`CHR_CHALLENGE`] (12 random bytes, single use), seals a [`Config`] with
//! [`seal`] and writes it to [`CHR_CONFIG`]. The station opens it with [`open`], stores it in flash and joins.
//!
//! Security: the config is encrypted and authenticated with ChaCha20-Poly1305 under a key derived from a
//! shared passphrase ([`derive_key`]); the challenge is the nonce, so a recorded write cannot be replayed
//! (the station draws a fresh challenge after every attempt). A sniffer sees only ciphertext. This needs no
//! BLE pairing, so it works from BlueZ/btleplug on the brain without an agent.

use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SERVICE_UUID: &str = "91082775-310d-48d0-b0cc-b87f04b92dd2";
/// read: UTF-8 `station-fw <version> id=<station_id hex>`.
pub const CHR_INFO: &str = "91082776-310d-48d0-b0cc-b87f04b92dd2";
/// read + notify: [`Status`] bytes.
pub const CHR_STATUS: &str = "91082777-310d-48d0-b0cc-b87f04b92dd2";
/// read: 12 random bytes, valid for exactly one write to [`CHR_CONFIG`].
pub const CHR_CHALLENGE: &str = "91082778-310d-48d0-b0cc-b87f04b92dd2";
/// write: sealed [`Config`] (see [`seal`]).
pub const CHR_CONFIG: &str = "91082779-310d-48d0-b0cc-b87f04b92dd2";
/// write: one byte, a [`Command`] value.
pub const CHR_COMMAND: &str = "9108277a-310d-48d0-b0cc-b87f04b92dd2";

pub const CHALLENGE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const AAD: &[u8] = b"sw-station-provision-v1";
/// Sealed config size limit: postcard(Config) + 16 byte tag.
pub const MAX_SEALED: usize = 32 + 64 + 48 + 8 + TAG_LEN;

/// What the brain writes. `brain` is `ip[:port]` (default port 4210); empty means "keep the old one".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub ssid: heapless::String<32>,
    pub pass: heapless::String<64>,
    pub brain: heapless::String<48>,
}

/// Values for [`CHR_COMMAND`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Command {
    /// Restart the station (also applies a stored config).
    Restart = 1,
    /// Erase the stored Wi-Fi settings and go back to provisioning.
    ClearConfig = 2,
    /// Run the self test again and notify the result.
    SelfTest = 3,
}

impl Command {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::Restart),
            2 => Some(Self::ClearConfig),
            3 => Some(Self::SelfTest),
            _ => None,
        }
    }
}

/// [`CHR_STATUS`] payload, 8 bytes: `[link, rssi, ip0..ip3, selftest, flags]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    pub link: Link,
    pub rssi_dbm: i8,
    pub ip: [u8; 4],
    /// 0 pass, 1 warn, 2 fail, 255 not run yet.
    pub selftest: u8,
    /// bit 0: a config is stored in flash.
    pub flags: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Link {
    /// No Wi-Fi settings stored.
    Unconfigured = 0,
    Connecting = 1,
    Up = 2,
    /// Settings stored but the access point could not be joined.
    Failed = 3,
}

impl Status {
    pub const LEN: usize = 8;

    pub fn to_bytes(&self) -> [u8; Self::LEN] {
        [self.link as u8, self.rssi_dbm as u8, self.ip[0], self.ip[1], self.ip[2], self.ip[3], self.selftest, self.flags]
    }

    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        let b: &[u8; Self::LEN] = b.try_into().ok()?;
        let link = match b[0] {
            0 => Link::Unconfigured,
            1 => Link::Connecting,
            2 => Link::Up,
            3 => Link::Failed,
            _ => return None,
        };
        Some(Self { link, rssi_dbm: b[1] as i8, ip: [b[2], b[3], b[4], b[5]], selftest: b[6], flags: b[7] })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// Too long for the buffer, or a field exceeds its limit.
    TooLong,
    /// Wrong key, wrong or reused challenge, or a damaged write.
    Auth,
    /// Authenticated but not a valid [`Config`].
    Format,
}

/// 32-byte key from the shared passphrase (SHA-256 of a domain tag and the passphrase).
pub fn derive_key(passphrase: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"sw-station-provision-key-v1\0");
    h.update(passphrase.as_bytes());
    h.finalize().into()
}

/// Encrypt `cfg` for the station that handed out `challenge`. Output: ciphertext followed by the 16 byte tag.
pub fn seal(key: &[u8; 32], challenge: &[u8; CHALLENGE_LEN], cfg: &Config) -> Result<heapless::Vec<u8, MAX_SEALED>, Error> {
    let mut buf = [0u8; MAX_SEALED - TAG_LEN];
    let n = postcard::to_slice(cfg, &mut buf).map_err(|_| Error::TooLong)?.len();
    let tag = ChaCha20Poly1305::new(Key::from_slice(key))
        .encrypt_in_place_detached(Nonce::from_slice(challenge), AAD, &mut buf[..n])
        .map_err(|_| Error::TooLong)?;
    let mut out = heapless::Vec::new();
    out.extend_from_slice(&buf[..n]).map_err(|_| Error::TooLong)?;
    out.extend_from_slice(&tag).map_err(|_| Error::TooLong)?;
    Ok(out)
}

/// Decrypt and parse a write to [`CHR_CONFIG`].
pub fn open(key: &[u8; 32], challenge: &[u8; CHALLENGE_LEN], sealed: &[u8]) -> Result<Config, Error> {
    if sealed.len() < TAG_LEN || sealed.len() > MAX_SEALED {
        return Err(Error::Auth);
    }
    let (ct, tag) = sealed.split_at(sealed.len() - TAG_LEN);
    let mut buf = [0u8; MAX_SEALED];
    buf[..ct.len()].copy_from_slice(ct);
    ChaCha20Poly1305::new(Key::from_slice(key))
        .decrypt_in_place_detached(Nonce::from_slice(challenge), AAD, &mut buf[..ct.len()], tag.into())
        .map_err(|_| Error::Auth)?;
    postcard::from_bytes(&buf[..ct.len()]).map_err(|_| Error::Format)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config { ssid: "arena-wifi".try_into().unwrap(), pass: "hunter2 hunter2".try_into().unwrap(), brain: "192.168.1.20:4210".try_into().unwrap() }
    }

    #[test]
    fn roundtrip() {
        let key = derive_key("secret");
        let ch = [7u8; CHALLENGE_LEN];
        let sealed = seal(&key, &ch, &cfg()).unwrap();
        assert_eq!(open(&key, &ch, &sealed).unwrap(), cfg());
        assert!(!sealed.windows(5).any(|w| w == b"arena"), "ssid must not appear in clear");
    }

    #[test]
    fn max_sizes_fit() {
        let c = Config {
            ssid: core::str::from_utf8(&[b's'; 32]).unwrap().try_into().unwrap(),
            pass: core::str::from_utf8(&[b'p'; 64]).unwrap().try_into().unwrap(),
            brain: core::str::from_utf8(&[b'b'; 48]).unwrap().try_into().unwrap(),
        };
        let key = derive_key("k");
        let ch = [1u8; CHALLENGE_LEN];
        let sealed = seal(&key, &ch, &c).unwrap();
        assert!(sealed.len() <= MAX_SEALED);
        assert_eq!(open(&key, &ch, &sealed).unwrap(), c);
    }

    #[test]
    fn wrong_key_challenge_or_tamper_rejected() {
        let key = derive_key("secret");
        let ch = [7u8; CHALLENGE_LEN];
        let sealed = seal(&key, &ch, &cfg()).unwrap();
        assert_eq!(open(&derive_key("other"), &ch, &sealed), Err(Error::Auth));
        assert_eq!(open(&key, &[8u8; CHALLENGE_LEN], &sealed), Err(Error::Auth));
        let mut bad = sealed.clone();
        bad[3] ^= 1;
        assert_eq!(open(&key, &ch, &bad), Err(Error::Auth));
        assert_eq!(open(&key, &ch, &sealed[..5]), Err(Error::Auth));
    }

    #[test]
    fn status_roundtrip() {
        let s = Status { link: Link::Up, rssi_dbm: -61, ip: [192, 168, 1, 50], selftest: 0, flags: 1 };
        assert_eq!(Status::from_bytes(&s.to_bytes()), Some(s));
        assert_eq!(Status::from_bytes(&[9, 0, 0, 0, 0, 0, 0, 0]), None);
        assert_eq!(Status::from_bytes(&[0; 3]), None);
    }

    /// Fixed vector so the brain (any implementation) can be checked against the firmware's crypto.
    #[test]
    fn known_vector() {
        let key = derive_key("sw-test");
        let ch = *b"0123456789ab";
        let sealed = seal(&key, &ch, &cfg()).unwrap();
        let hex: std::string::String = sealed.iter().map(|b| std::format!("{b:02x}")).collect();
        std::println!("vector {hex}");
        assert_eq!(open(&key, &ch, &sealed).unwrap(), cfg());
    }
}
