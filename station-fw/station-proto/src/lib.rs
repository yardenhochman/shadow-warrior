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

pub mod provision;

use serde::{Deserialize, Serialize};

/// Bumped on any incompatible change to [`Msg`] or the framing.
pub const PROTO_VERSION: u8 = 1;

/// Stereo frames per [`AudioChunk`] (10 ms at 16 kHz).
pub const AUDIO_CHUNK_FRAMES: usize = 160;
/// Sample bytes per full [`AudioChunk`]: frames x 2 channels x 2 bytes.
pub const AUDIO_CHUNK_BYTES: usize = AUDIO_CHUNK_FRAMES * 2 * 2;

/// Largest encoded [`Envelope`] (before CRC and COBS) this crate produces or accepts.
/// Sized for a full [`AudioChunk`] (640 sample bytes + at most 18 bytes of header).
pub const MAX_PAYLOAD: usize = AUDIO_CHUNK_BYTES + 32;
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

/// Direction of arrival from GCC-PHAT on one audio frame (1024 samples, about 21 ms
/// at 48 kHz). Sent only for frames whose level passes the station's gate, so silence
/// and the room noise floor produce no messages. One message per frame, no smoothing.
///
/// Sign convention: `lag_samples` is the arrival time at the L mic minus the arrival
/// time at the R mic, in samples, after the station's fixed calibration bias was
/// subtracted. Positive lag means the sound reached R first, so the source is on the
/// R mic side and `angle_deg` is positive. 0 deg is broadside (perpendicular to the
/// mic axis). Two mics cannot tell front from back: the angle is a half-plane value.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Direction {
    /// Index of the audio frame since boot (frame length is fixed by the firmware,
    /// 1024 samples in stage 1); gaps mean gated or dropped frames.
    pub frame: u32,
    /// Sub-sample lag (parabolic peak interpolation), bias-corrected.
    pub lag_samples: f32,
    /// `asin(lag * c / (fs * d))`, clamped to -90..=90.
    pub angle_deg: f32,
    /// Correlation peak divided by the median |correlation| over all lags. Around
    /// 3-5 for uncorrelated noise; a clear single source gives much more.
    pub confidence: f32,
    /// Frame RMS over both channels after DC removal, dBFS (24-bit full scale).
    pub rms_dbfs: f32,
}

/// A block of raw audio, sent only while the host has streaming switched on
/// ([`Command::StartAudio`]). 16-bit PCM, little-endian, channels interleaved
/// (`L0 R0 L1 R1 ...`, L = mic with SELECT to GND). Produced by decimating the 48 kHz
/// 24-bit I2S stream by 3 behind a linear-phase anti-alias FIR (flat to 7 kHz, >= 64 dB
/// down from 8 kHz) and rounding to 16 bits; no AGC, noise suppression or other filtering.
///
/// `samples` is a byte string rather than `[i16]` because postcard would varint-encode
/// each i16; as bytes it is fixed-size and cheap to encode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioChunk {
    /// Output sample rate (16000).
    pub rate_hz: u16,
    /// Interleaved channels (2).
    pub channels: u8,
    /// Index of the first frame of this chunk since streaming started. A jump means the
    /// station dropped chunks (UART could not keep up); the reader can pad with silence.
    pub frame: u32,
    /// Samples in this chunk that saturated i16 (input at or near full scale).
    pub clipped: u16,
    /// `frames x channels` little-endian i16 (at most [`AUDIO_CHUNK_BYTES`]).
    pub samples: heapless::Vec<u8, AUDIO_CHUNK_BYTES>,
}

impl AudioChunk {
    /// Number of stereo (or `channels`-wide) frames in the chunk.
    pub fn frames(&self) -> usize {
        self.samples.len() / 2 / (self.channels.max(1) as usize)
    }

    /// Iterate the samples as i16, interleaved.
    pub fn iter_i16(&self) -> impl Iterator<Item = i16> + '_ {
        self.samples.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]))
    }
}

/// Everything a station can send. Append new variants at the end only.
// AudioChunk makes this ~670 bytes. Boxing needs alloc, which this crate avoids so it
// stays usable without a heap; a message lives on the stack for one encode or decode.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Msg {
    Hello(Hello),
    Radar(RadarEvent),
    Audio(AudioLevel),
    /// Added after proto v1 without a version bump: appending a variant does not
    /// change the encoding of the others. A v1 reader that predates it rejects
    /// these frames as undecodable and keeps going.
    Direction(Direction),
    /// Raw audio while streaming is on (see [`AudioChunk`]). Appended like `Direction`.
    AudioChunk(AudioChunk),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    /// Wraps at u16::MAX; a gap means frames were lost.
    pub seq: u16,
    pub msg: Msg,
}

/// Host -> station commands, framed like station frames (`0x00 COBS(postcard(CommandEnvelope)
/// ++ crc32) 0x00`) and written to the station's serial RX. Append new variants at the end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    /// Start (or keep alive) the raw [`AudioChunk`] stream. The station stops on its own
    /// when no `StartAudio` arrived for [`AUDIO_KEEPALIVE_MS`], so a host that dies does not
    /// leave it streaming: re-send this about once a second.
    StartAudio,
    StopAudio,
}

/// The station stops streaming this long after the last [`Command::StartAudio`].
pub const AUDIO_KEEPALIVE_MS: u32 = 5000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandEnvelope {
    /// Chosen by the host; echoed only in the station's log for now.
    pub seq: u16,
    pub cmd: Command,
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
    encode_any(env, out)
}

/// Decode one frame body: the bytes between two 0x00 delimiters (without them).
pub fn decode_frame(cobs_bytes: &[u8]) -> Result<Envelope, FrameError> {
    decode_any(cobs_bytes)
}

/// Encode a host -> station command frame.
pub fn encode_command(env: &CommandEnvelope, out: &mut [u8]) -> Result<usize, FrameError> {
    encode_any(env, out)
}

/// Decode a host -> station command frame body (between the 0x00 delimiters).
pub fn decode_command(cobs_bytes: &[u8]) -> Result<CommandEnvelope, FrameError> {
    decode_any(cobs_bytes)
}

fn encode_any<T: Serialize>(env: &T, out: &mut [u8]) -> Result<usize, FrameError> {
    let mut raw = [0u8; MAX_PAYLOAD + CRC_LEN];
    let n = postcard::to_slice(env, &mut raw[..MAX_PAYLOAD]).map_err(|_| FrameError::TooLarge)?.len();
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

fn decode_any<T: serde::de::DeserializeOwned>(cobs_bytes: &[u8]) -> Result<T, FrameError> {
    let mut raw = [0u8; MAX_PAYLOAD + CRC_LEN];
    if cobs_bytes.len() > cobs_max(raw.len()) {
        return Err(FrameError::TooLarge);
    }
    let n = cobs::decode(cobs_bytes, &mut raw).map_err(|_| FrameError::Cobs)?.frame_size();
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

#[allow(clippy::large_enum_variant)] // see Msg
#[derive(Debug, PartialEq)]
pub enum Chunk<'a> {
    Frame(Envelope),
    /// Non-frame bytes (log text, or a damaged frame). `overflow` is set when
    /// the chunk was longer than the buffer and only its start is kept.
    Other {
        bytes: &'a [u8],
        overflow: bool,
    },
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

    fn chunk(frames: usize) -> AudioChunk {
        let mut samples = heapless::Vec::new();
        for i in 0..frames * 2 {
            let v = (i as i16).wrapping_mul(997).wrapping_sub(16000);
            samples.extend_from_slice(&v.to_le_bytes()).unwrap();
        }
        AudioChunk { rate_hz: 16000, channels: 2, frame: 1_234_560, clipped: 3, samples }
    }

    fn all() -> [Envelope; 5] {
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
            Envelope {
                seq: 1,
                msg: Msg::Direction(Direction {
                    frame: 123_456,
                    lag_samples: -7.25,
                    angle_deg: -23.5,
                    confidence: 42.0,
                    rms_dbfs: -31.5,
                }),
            },
            Envelope { seq: 2, msg: Msg::AudioChunk(chunk(AUDIO_CHUNK_FRAMES)) },
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
    fn known_direction_layout() {
        let env = Envelope {
            seq: 2,
            msg: Msg::Direction(Direction {
                frame: 300,
                lag_samples: 1.5,
                angle_deg: -90.0,
                confidence: 0.0,
                rms_dbfs: -50.0,
            }),
        };
        let mut buf = [0u8; 32];
        let b = postcard::to_slice(&env, &mut buf).unwrap();
        let mut want = std::vec![0x02, 0x03, 0xAC, 0x02];
        for f in [1.5f32, -90.0, 0.0, -50.0] {
            want.extend_from_slice(&f.to_le_bytes());
        }
        assert_eq!(b, want.as_slice());
    }

    #[test]
    fn known_audio_chunk_layout() {
        let mut samples = heapless::Vec::new();
        for v in [1i16, -2, 0x1234, i16::MIN] {
            samples.extend_from_slice(&v.to_le_bytes()).unwrap();
        }
        let env = Envelope {
            seq: 5,
            msg: Msg::AudioChunk(AudioChunk { rate_hz: 16000, channels: 2, frame: 300, clipped: 0, samples }),
        };
        let mut buf = [0u8; 32];
        let b = postcard::to_slice(&env, &mut buf).unwrap();
        // seq 5, variant 4, 16000 = 80 7D, 2, 300 = AC 02, 0, len 8, raw LE i16 bytes
        assert_eq!(
            b,
            &[0x05, 0x04, 0x80, 0x7D, 0x02, 0xAC, 0x02, 0x00, 0x08, 0x01, 0x00, 0xFE, 0xFF, 0x34, 0x12, 0x00, 0x80]
        );
        let Msg::AudioChunk(c) = env.msg else { unreachable!() };
        assert_eq!(c.frames(), 2);
        assert_eq!(c.iter_i16().collect::<std::vec::Vec<_>>(), [1, -2, 0x1234, i16::MIN]);
    }

    #[test]
    fn full_audio_chunk_fits() {
        let env = Envelope {
            seq: u16::MAX,
            msg: Msg::AudioChunk(AudioChunk { frame: u32::MAX, clipped: u16::MAX, ..chunk(AUDIO_CHUNK_FRAMES) }),
        };
        let mut out = [0u8; MAX_FRAME];
        let n = encode_frame(&env, &mut out).unwrap();
        assert_eq!(decode_frame(&out[1..n - 1]).unwrap(), env);
    }

    #[test]
    fn command_roundtrip_and_layout() {
        for (cmd, variant) in [(Command::StartAudio, 0u8), (Command::StopAudio, 1)] {
            let env = CommandEnvelope { seq: 9, cmd };
            let mut buf = [0u8; 8];
            assert_eq!(postcard::to_slice(&env, &mut buf).unwrap(), &[9, variant]);
            let mut out = [0u8; 16];
            let n = encode_command(&env, &mut out).unwrap();
            assert_eq!(decode_command(&out[1..n - 1]).unwrap(), env);
        }
    }

    #[test]
    fn command_frame_bytes_match_python() {
        // tools/station_monitor.py encode_command(1, 0) / (2, 1) produce exactly these.
        let mut out = [0u8; 16];
        let n = encode_command(&CommandEnvelope { seq: 1, cmd: Command::StartAudio }, &mut out).unwrap();
        assert_eq!(&out[..n], &[0x00, 0x02, 0x01, 0x05, 0xbe, 0x23, 0xc2, 0x58, 0x00]);
        let n = encode_command(&CommandEnvelope { seq: 2, cmd: Command::StopAudio }, &mut out).unwrap();
        assert_eq!(&out[..n], &[0x00, 0x07, 0x02, 0x01, 0xeb, 0x40, 0xe8, 0x04, 0x00]);
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

        let mut sp = StreamSplitter::<MAX_FRAME>::new();
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
        // boot line, 5 log lines (the last one runs into the 0x55 garbage), [13 37]
        assert_eq!(texts, 1 + 5 + 1);
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
