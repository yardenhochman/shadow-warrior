//! Wire protocol between a sensor station and the brain.
//!
//! A message is a postcard-encoded [`Envelope`] (sequence number + [`Msg`]),
//! followed by a CRC-32 (ISO-HDLC, the one `zlib.crc32` computes) of those
//! bytes, little-endian. The whole thing is COBS-encoded so it contains no
//! 0x00 byte, and written as `0x00 <cobs> 0x00`.
//!
//! The leading delimiter matters for the stage-1 USB serial link, where frames
//! share UART0 with human-readable log text: a reader splits the byte stream
//! on 0x00 and keeps every chunk that COBS-decodes and passes the CRC; all
//! other chunks are text. Over a datagram transport (later Wi-Fi UDP) one
//! datagram carries one frame and the delimiters are harmless.
//!
//! Postcard encodes integers as varints (signed ones zig-zag), `f32` as 4
//! little-endian bytes, `bool` as one byte, enums as a varint variant index
//! and strings as varint length + UTF-8. Variant order and field order are
//! therefore part of the wire format: only append, never reorder.
#![no_std]

#[cfg(any(test, feature = "std"))]
extern crate std;

use serde::{Deserialize, Serialize};

/// Bumped on any incompatible change to [`Msg`] or the framing.
pub const PROTO_VERSION: u8 = 1;

/// Largest encoded [`Envelope`] (before CRC and COBS) this crate produces or accepts.
pub const MAX_PAYLOAD: usize = 96;
/// CRC trailer length.
pub const CRC_LEN: usize = 4;
/// Largest frame on the wire including both 0x00 delimiters.
pub const MAX_FRAME: usize = cobs_max(MAX_PAYLOAD + CRC_LEN) + 2;

const fn cobs_max(n: usize) -> usize {
    // COBS adds at most one byte per 254 plus one.
    n + n / 254 + 1
}

const CRC32: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);

/// Sent at boot, whenever radar identification changes, and periodically so a
/// reader that attaches late learns who it is talking to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub proto_version: u8,
    /// Lower four bytes of the station's factory MAC, big-endian.
    pub station_id: u32,
    /// Firmware crate version, e.g. "0.1.0".
    pub fw_version: heapless::String<16>,
    /// LD2410 firmware version as ESPHome prints it ("2.04.23022511"),
    /// `None` while the radar has not answered.
    pub radar_fw: Option<heapless::String<24>>,
}

/// One LD2410 basic-mode report (the module sends them at about 10 Hz).
/// Distances and energies are copied as the module reports them; they are
/// only meaningful when the matching bit of `state` is set.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RadarEvent {
    /// `state != 0` (target state 1..=3).
    pub present: bool,
    pub moving_dist_cm: u16,
    pub static_dist_cm: u16,
    /// 0..=100
    pub moving_energy: u8,
    /// 0..=100
    pub static_energy: u8,
    /// Raw target state: 0 none, 1 moving, 2 static, 3 both,
    /// 4..=6 background-correction progress on newer firmware.
    pub state: u8,
}

/// Audio levels over one window (100 ms in stage 1). dBFS relative to 24-bit
/// full scale; silence is clamped to [`AudioLevel::FLOOR_DBFS`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AudioLevel {
    pub rms_dbfs_l: f32,
    pub rms_dbfs_r: f32,
    /// Largest |sample| over both channels, dBFS.
    pub peak_dbfs: f32,
    /// Window length in frames (samples per channel).
    pub frames: u16,
}

impl AudioLevel {
    pub const FLOOR_DBFS: f32 = -144.0;
}

/// Everything a station can send. Append new variants at the end only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Msg {
    Hello(Hello),
    Radar(RadarEvent),
    Audio(AudioLevel),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    /// Wraps at u16::MAX; a gap means frames were lost.
    pub seq: u16,
    pub msg: Msg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    /// Output buffer too small or message larger than [`MAX_PAYLOAD`].
    TooLarge,
    /// Not valid COBS.
    Cobs,
    /// Shorter than the CRC trailer.
    Short,
    /// CRC mismatch.
    Crc,
    /// CRC ok but postcard could not decode it (version skew?).
    Decode,
}

/// Encode `env` as a delimited frame into `out`; returns the bytes written.
pub fn encode_frame(env: &Envelope, out: &mut [u8]) -> Result<usize, FrameError> {
    let mut raw = [0u8; MAX_PAYLOAD + CRC_LEN];
    let n = postcard::to_slice(env, &mut raw[..MAX_PAYLOAD])
        .map_err(|_| FrameError::TooLarge)?
        .len();
    let crc = CRC32.checksum(&raw[..n]);
    raw[n..n + CRC_LEN].copy_from_slice(&crc.to_le_bytes());
    let raw = &raw[..n + CRC_LEN];
    if out.len() < cobs_max(raw.len()) + 2 {
        return Err(FrameError::TooLarge);
    }
    out[0] = 0;
    let m = cobs::encode(raw, &mut out[1..]);
    out[1 + m] = 0;
    Ok(m + 2)
}

/// Decode one frame body: the bytes between two 0x00 delimiters (without them).
pub fn decode_frame(cobs_bytes: &[u8]) -> Result<Envelope, FrameError> {
    let mut raw = [0u8; MAX_PAYLOAD + CRC_LEN];
    if cobs_bytes.len() > cobs_max(raw.len()) {
        return Err(FrameError::TooLarge);
    }
    let n = cobs::decode(cobs_bytes, &mut raw)
        .map_err(|_| FrameError::Cobs)?
        .frame_size();
    if n < CRC_LEN + 1 {
        return Err(FrameError::Short);
    }
    let (body, tail) = raw[..n].split_at(n - CRC_LEN);
    let want = u32::from_le_bytes([tail[0], tail[1], tail[2], tail[3]]);
    if CRC32.checksum(body) != want {
        return Err(FrameError::Crc);
    }
    postcard::from_bytes(body).map_err(|_| FrameError::Decode)
}

/// Incremental splitter for a byte stream that mixes frames and text.
/// Feed bytes; each completed chunk (delimited by 0x00) is offered back as a
/// frame if it decodes, otherwise as text. Chunks longer than the buffer are
/// text by definition and are passed through truncated.
pub struct StreamSplitter<const N: usize> {
    buf: heapless::Vec<u8, N>,
    overflow: bool,
    clear_next: bool,
}

#[derive(Debug, PartialEq)]
pub enum Chunk<'a> {
    Frame(Envelope),
    /// Non-frame bytes (log text, or a damaged frame). `overflow` is set when
    /// the chunk was longer than the buffer and only its start is kept.
    Other { bytes: &'a [u8], overflow: bool },
}

impl<const N: usize> Default for StreamSplitter<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> StreamSplitter<N> {
    pub const fn new() -> Self {
        Self { buf: heapless::Vec::new(), overflow: false, clear_next: false }
    }

    /// Push one byte; returns a chunk when `b` is a delimiter and the chunk is non-empty.
    pub fn push(&mut self, b: u8) -> Option<Chunk<'_>> {
        if core::mem::take(&mut self.clear_next) {
            self.buf.clear();
        }
        if b != 0 {
            if self.buf.push(b).is_err() {
                self.overflow = true;
            }
            return None;
        }
        if self.buf.is_empty() {
            return None;
        }
        let overflow = core::mem::take(&mut self.overflow);
        // Cleared on the next push: the returned slice borrows the buffer.
        self.clear_next = true;
        let bytes = self.buf.as_slice();
        if !overflow {
            if let Ok(env) = decode_frame(bytes) {
                return Some(Chunk::Frame(env));
            }
        }
        Some(Chunk::Other { bytes, overflow })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello() -> Envelope {
        Envelope {
            seq: 7,
            msg: Msg::Hello(Hello {
                proto_version: PROTO_VERSION,
                station_id: 0xDEADBEEF,
                fw_version: "0.1.0".try_into().unwrap(),
                radar_fw: Some("2.04.23022511".try_into().unwrap()),
            }),
        }
    }

    fn all() -> [Envelope; 3] {
        [
            hello(),
            Envelope {
                seq: 65535,
                msg: Msg::Radar(RadarEvent {
                    present: true,
                    moving_dist_cm: 0x1234,
                    static_dist_cm: 0,
                    moving_energy: 100,
                    static_energy: 0,
                    state: 1,
                }),
            },
            Envelope {
                seq: 0,
                msg: Msg::Audio(AudioLevel {
                    rms_dbfs_l: -42.5,
                    rms_dbfs_r: AudioLevel::FLOOR_DBFS,
                    peak_dbfs: -0.0,
                    frames: 4800,
                }),
            },
        ]
    }

    #[test]
    fn roundtrip_all_variants() {
        for env in all() {
            let mut out = [0u8; MAX_FRAME];
            let n = encode_frame(&env, &mut out).unwrap();
            assert_eq!(out[0], 0);
            assert_eq!(out[n - 1], 0);
            assert!(!out[1..n - 1].contains(&0), "COBS body must be zero-free");
            assert_eq!(decode_frame(&out[1..n - 1]).unwrap(), env);
        }
    }

    #[test]
    fn crc_catches_bit_flip() {
        let mut out = [0u8; MAX_FRAME];
        let n = encode_frame(&hello(), &mut out).unwrap();
        // Flip a bit in the middle; keep it non-zero so COBS stays structurally plausible.
        let i = n / 2;
        out[i] ^= if out[i] == 0x01 { 0x02 } else { 0x01 };
        assert!(decode_frame(&out[1..n - 1]).is_err());
    }

    #[test]
    fn known_postcard_layout() {
        // Pin the wire format: the python tool decodes this by hand.
        let env = Envelope {
            seq: 300,
            msg: Msg::Radar(RadarEvent {
                present: true,
                moving_dist_cm: 150,
                static_dist_cm: 2,
                moving_energy: 55,
                static_energy: 9,
                state: 3,
            }),
        };
        let mut buf = [0u8; 32];
        let b = postcard::to_slice(&env, &mut buf).unwrap();
        // seq 300 = varint AC 02, variant 1, true, 150 = 96 01, 2, 55, 9, 3
        assert_eq!(b, &[0xAC, 0x02, 0x01, 0x01, 0x96, 0x01, 0x02, 55, 9, 3]);
    }

    #[test]
    fn splitter_separates_text_and_frames() {
        let mut stream = std::vec::Vec::new();
        stream.extend_from_slice(b"I (123) boot: hello\n");
        let mut out = [0u8; MAX_FRAME];
        for env in all() {
            let n = encode_frame(&env, &mut out).unwrap();
            stream.extend_from_slice(&out[..n]);
            stream.extend_from_slice(b"I (456) station: some log line\n");
        }
        // garbage with a stray zero
        stream.extend_from_slice(&[0x55, 0x00, 0x13, 0x37, 0x00]);

        let mut sp = StreamSplitter::<256>::new();
        let mut frames = std::vec::Vec::new();
        let mut texts = 0;
        for &b in &stream {
            match sp.push(b) {
                Some(Chunk::Frame(e)) => frames.push(e),
                Some(Chunk::Other { .. }) => texts += 1,
                None => {}
            }
        }
        assert_eq!(frames, all().to_vec());
        // boot line, 3 log lines (the last one runs into the 0x55 garbage), [13 37]
        assert_eq!(texts, 1 + 3 + 1);
    }

    #[test]
    fn splitter_overflow_is_text() {
        let mut sp = StreamSplitter::<8>::new();
        for _ in 0..20 {
            assert!(sp.push(b'x').is_none());
        }
        match sp.push(0) {
            Some(Chunk::Other { bytes, overflow }) => {
                assert!(overflow);
                assert_eq!(bytes.len(), 8);
            }
            other => panic!("{other:?}"),
        }
        // and recovers
        let mut out = [0u8; MAX_FRAME];
        let n = encode_frame(&hello(), &mut out).unwrap();
        let mut sp = StreamSplitter::<128>::new();
        let got: std::vec::Vec<_> = out[..n]
            .iter()
            .filter_map(|&b| match sp.push(b) {
                Some(Chunk::Frame(e)) => Some(e),
                _ => None,
            })
            .collect();
        assert_eq!(got, std::vec![hello()]);
    }

    #[test]
    fn max_sizes_fit() {
        let env = Envelope {
            seq: u16::MAX,
            msg: Msg::Hello(Hello {
                proto_version: 255,
                station_id: u32::MAX,
                fw_version: "0123456789abcdef".try_into().unwrap(),
                radar_fw: Some("0123456789abcdef01234567".try_into().unwrap()),
            }),
        };
        let mut out = [0u8; MAX_FRAME];
        encode_frame(&env, &mut out).unwrap();
    }
}
