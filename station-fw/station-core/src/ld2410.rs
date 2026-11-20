//! HLK-LD2410 (B / C) UART protocol: incremental frame parser and command builders.
//!
//! Layout cross-checked against ESPHome's `ld2410` / `ld24xx` components
//! (esphome/components/ld2410/ld2410.cpp, dev branch, fetched 2026-11-20):
//!
//! ```text
//! data frame:  F4 F3 F2 F1 | len u16 LE | type(01 eng / 02 basic) AA state
//!              mov_dist u16 | mov_energy u8 | still_dist u16 | still_energy u8
//!              detect_dist u16 | [engineering: gate energies, light, out] | 55 00 | F8 F7 F6 F5
//! cmd frame:   FD FC FB FA | len u16 LE | cmd u16 LE | value... | 04 03 02 01
//! ack frame:   FD FC FB FA | len u16 LE | (cmd | 0x0100) u16 LE | status u16 (0 = ok) | data... | 04 03 02 01
//! ```
//!
//! `len` counts the bytes between the length field and the tail. Basic data
//! frames have len 0x0D (23 bytes total). The parser resynchronises on any
//! garbage: a byte that cannot start or continue a header is dropped, an
//! implausible length or a bad tail drops one byte and rescans.

use heapless::Vec;
use station_proto::RadarEvent;

pub const DATA_HEADER: [u8; 4] = [0xF4, 0xF3, 0xF2, 0xF1];
pub const DATA_TAIL: [u8; 4] = [0xF8, 0xF7, 0xF6, 0xF5];
pub const CMD_HEADER: [u8; 4] = [0xFD, 0xFC, 0xFB, 0xFA];
pub const CMD_TAIL: [u8; 4] = [0x04, 0x03, 0x02, 0x01];

pub const CMD_ENABLE_CONFIG: u8 = 0xFF;
pub const CMD_END_CONFIG: u8 = 0xFE;
pub const CMD_READ_VERSION: u8 = 0xA0;

pub const DEFAULT_BAUD: u32 = 256_000;

/// Largest frame accepted. Engineering data frames are 45 bytes, the longest
/// ACK (parameter query 0x61) is 38.
pub const MAX_FRAME: usize = 64;
const OVERHEAD: usize = 4 + 2 + 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub engineering: bool,
    /// 0 none, 1 moving, 2 still, 3 both; 4..=6 background correction (fw >= 2.44).
    pub state: u8,
    pub moving_dist_cm: u16,
    pub moving_energy: u8,
    pub still_dist_cm: u16,
    pub still_energy: u8,
    pub detect_dist_cm: u16,
}

impl Target {
    pub fn present(&self) -> bool {
        matches!(self.state, 1..=3)
    }

    pub fn to_event(&self) -> RadarEvent {
        RadarEvent {
            present: self.present(),
            moving_dist_cm: self.moving_dist_cm,
            static_dist_cm: self.still_dist_cm,
            moving_energy: self.moving_energy,
            static_energy: self.still_energy,
            state: self.state,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ack {
    /// Command this answers (low byte of the command word).
    pub cmd: u8,
    /// Status word, 0 = success.
    pub status: u16,
    pub data: Vec<u8, MAX_FRAME>,
}

impl Ack {
    pub fn ok(&self) -> bool {
        self.status == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    Target(Target),
    Ack(Ack),
}

/// Diagnostics; counters saturate.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// Bytes thrown away while hunting for a header.
    pub dropped_bytes: u32,
    /// Frames whose header/length/tail looked fine but whose body did not parse.
    pub bad_frames: u32,
    pub data_frames: u32,
    pub acks: u32,
}

#[derive(Default)]
pub struct Parser {
    buf: Vec<u8, MAX_FRAME>,
    pub stats: Stats,
}

impl Parser {
    pub const fn new() -> Self {
        Self {
            buf: Vec::new(),
            stats: Stats { dropped_bytes: 0, bad_frames: 0, data_frames: 0, acks: 0 },
        }
    }

    /// Feed raw UART bytes; `on_frame` is called for every complete frame.
    pub fn feed(&mut self, bytes: &[u8], mut on_frame: impl FnMut(Frame)) {
        for &b in bytes {
            if self.buf.push(b).is_err() {
                // Cannot happen (poll keeps buf < MAX_FRAME), but never wedge.
                self.drop_front(1);
                let _ = self.buf.push(b);
            }
            while let Some(f) = self.poll() {
                on_frame(f);
            }
        }
    }

    fn drop_front(&mut self, n: usize) {
        let n = n.min(self.buf.len());
        self.buf.rotate_left(n);
        self.buf.truncate(self.buf.len() - n);
    }

    fn discard(&mut self, n: usize) {
        self.stats.dropped_bytes = self.stats.dropped_bytes.saturating_add(n as u32);
        self.drop_front(n);
    }

    /// Try to take one frame off the front of the buffer.
    fn poll(&mut self) -> Option<Frame> {
        loop {
            let b = &self.buf;
            if b.is_empty() {
                return None;
            }
            // Header (possibly partial).
            let hn = b.len().min(4);
            let is_data = b[..hn] == DATA_HEADER[..hn];
            let is_cmd = b[..hn] == CMD_HEADER[..hn];
            if !is_data && !is_cmd {
                self.discard(1);
                continue;
            }
            if b.len() < 6 {
                return None;
            }
            let len = u16::from_le_bytes([b[4], b[5]]) as usize;
            let total = len + OVERHEAD;
            if total > MAX_FRAME || len < 2 {
                self.discard(1);
                continue;
            }
            if b.len() < total {
                return None;
            }
            let tail = if is_data { &DATA_TAIL } else { &CMD_TAIL };
            if b[total - 4..total] != tail[..] {
                self.discard(1);
                continue;
            }
            let body = &b[6..6 + len];
            let parsed = if is_data { parse_data(body).map(Frame::Target) } else { parse_ack(body).map(Frame::Ack) };
            // A well-delimited frame is consumed whether or not its body parsed.
            self.drop_front(total);
            match parsed {
                Some(f) => {
                    match f {
                        Frame::Target(_) => self.stats.data_frames = self.stats.data_frames.saturating_add(1),
                        Frame::Ack(_) => self.stats.acks = self.stats.acks.saturating_add(1),
                    }
                    return Some(f);
                }
                None => {
                    self.stats.bad_frames = self.stats.bad_frames.saturating_add(1);
                    continue;
                }
            }
        }
    }
}

fn u16le(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn parse_data(p: &[u8]) -> Option<Target> {
    // type, AA, state, md(2), me, sd(2), se, dd(2), 55, 00
    if p.len() < 13 || p[1] != 0xAA || p[p.len() - 2] != 0x55 || p[p.len() - 1] != 0x00 {
        return None;
    }
    let engineering = match p[0] {
        0x01 => true,
        0x02 => false,
        _ => return None,
    };
    Some(Target {
        engineering,
        state: p[2],
        moving_dist_cm: u16le(p, 3),
        moving_energy: p[5],
        still_dist_cm: u16le(p, 6),
        still_energy: p[8],
        detect_dist_cm: u16le(p, 9),
    })
}

fn parse_ack(p: &[u8]) -> Option<Ack> {
    // cmd|0x0100 (2), status (2), data
    if p.len() < 4 || p[1] != 0x01 {
        return None;
    }
    Some(Ack { cmd: p[0], status: u16le(p, 2), data: Vec::from_slice(&p[4..]).ok()? })
}

/// Build a command frame. `value` is the command value (may be empty).
pub fn command(cmd: u8, value: &[u8]) -> Vec<u8, MAX_FRAME> {
    let mut v = Vec::new();
    let len = (2 + value.len()) as u16;
    // MAX_FRAME is far above any command we send; extend errors are impossible for value.len() <= 50.
    let _ = v.extend_from_slice(&CMD_HEADER);
    let _ = v.extend_from_slice(&len.to_le_bytes());
    let _ = v.extend_from_slice(&[cmd, 0x00]);
    let _ = v.extend_from_slice(value);
    let _ = v.extend_from_slice(&CMD_TAIL);
    v
}

/// `FD FC FB FA 04 00 FF 00 01 00 04 03 02 01`
pub fn enable_config() -> Vec<u8, MAX_FRAME> {
    command(CMD_ENABLE_CONFIG, &[0x01, 0x00])
}

/// `FD FC FB FA 02 00 FE 00 04 03 02 01`
pub fn end_config() -> Vec<u8, MAX_FRAME> {
    command(CMD_END_CONFIG, &[])
}

/// `FD FC FB FA 02 00 A0 00 04 03 02 01`
pub fn read_version() -> Vec<u8, MAX_FRAME> {
    command(CMD_READ_VERSION, &[])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FwVersion {
    pub fw_type: u16,
    pub major: u8,
    pub minor: u8,
    pub build: u32,
}

impl FwVersion {
    /// From the data of a successful 0xA0 ACK: type u16, minor u8, major u8, build u32 LE.
    pub fn from_ack(ack: &Ack) -> Option<Self> {
        if ack.cmd != CMD_READ_VERSION || !ack.ok() || ack.data.len() < 8 {
            return None;
        }
        let d = &ack.data;
        Some(Self {
            fw_type: u16le(d, 0),
            minor: d[2],
            major: d[3],
            build: u32::from_le_bytes([d[4], d[5], d[6], d[7]]),
        })
    }

    /// ESPHome's format, `"%u.%02X.%02X%02X%02X%02X"`, e.g. `2.04.23022511`.
    pub fn to_string<const N: usize>(&self) -> heapless::String<N> {
        use core::fmt::Write;
        let mut s = heapless::String::new();
        let _ = write!(s, "{}.{:02X}.{:08X}", self.major, self.minor, self.build);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec as SVec;

    /// Basic-mode frame as in the HLK manual example: moving target at 0x51 cm,
    /// energy 0x00... (values chosen to exercise LE decoding).
    fn basic_frame(state: u8, md: u16, me: u8, sd: u16, se: u8, dd: u16) -> SVec<u8> {
        let mut f = SVec::new();
        f.extend_from_slice(&DATA_HEADER);
        f.extend_from_slice(&[0x0D, 0x00, 0x02, 0xAA, state]);
        f.extend_from_slice(&md.to_le_bytes());
        f.push(me);
        f.extend_from_slice(&sd.to_le_bytes());
        f.push(se);
        f.extend_from_slice(&dd.to_le_bytes());
        f.extend_from_slice(&[0x55, 0x00]);
        f.extend_from_slice(&DATA_TAIL);
        f
    }

    fn collect(p: &mut Parser, bytes: &[u8]) -> SVec<Frame> {
        let mut out = SVec::new();
        p.feed(bytes, |f| out.push(f));
        out
    }

    #[test]
    fn command_bytes_match_manual() {
        assert_eq!(
            enable_config().as_slice(),
            &[0xFD, 0xFC, 0xFB, 0xFA, 0x04, 0x00, 0xFF, 0x00, 0x01, 0x00, 0x04, 0x03, 0x02, 0x01]
        );
        assert_eq!(end_config().as_slice(), &[0xFD, 0xFC, 0xFB, 0xFA, 0x02, 0x00, 0xFE, 0x00, 0x04, 0x03, 0x02, 0x01]);
        assert_eq!(read_version().as_slice(), &[0xFD, 0xFC, 0xFB, 0xFA, 0x02, 0x00, 0xA0, 0x00, 0x04, 0x03, 0x02, 0x01]);
    }

    #[test]
    fn parses_basic_frame() {
        let f = basic_frame(3, 0x0151, 60, 0x0033, 100, 0x0200);
        assert_eq!(f.len(), 23);
        let mut p = Parser::new();
        let got = collect(&mut p, &f);
        assert_eq!(
            got,
            [Frame::Target(Target {
                engineering: false,
                state: 3,
                moving_dist_cm: 0x0151,
                moving_energy: 60,
                still_dist_cm: 0x33,
                still_energy: 100,
                detect_dist_cm: 0x200
            })]
        );
        let Frame::Target(t) = got[0] else { unreachable!() };
        let e = t.to_event();
        assert!(e.present);
        assert_eq!(e.moving_dist_cm, 0x151);
        assert_eq!(p.stats.dropped_bytes, 0);
    }

    #[test]
    fn no_target_not_present() {
        let mut p = Parser::new();
        let got = collect(&mut p, &basic_frame(0, 0, 0, 0, 0, 0));
        let Frame::Target(t) = got[0] else { panic!() };
        assert!(!t.present());
        let got = collect(&mut p, &basic_frame(5, 0, 0, 0, 0, 0));
        let Frame::Target(t) = got[0] else { panic!() };
        assert!(!t.present(), "background correction states are not presence");
    }

    #[test]
    fn engineering_frame() {
        // 0x23 = 35 byte payload: 11 header bytes, maxgates(2), 9 moving + 9 still gate energies,
        // light, out, 55 00 (as in ESPHome: MOVING_SENSOR_START=19, OUT_PIN_SENSOR=38 absolute).
        let mut f = SVec::new();
        f.extend_from_slice(&DATA_HEADER);
        f.extend_from_slice(&[0x23, 0x00, 0x01, 0xAA, 0x01, 0x64, 0x00, 0x32, 0x00, 0x00, 0x00, 0x64, 0x00]);
        f.extend_from_slice(&[0x08, 0x08]);
        f.extend_from_slice(&[0u8; 18]);
        f.extend_from_slice(&[0x80, 0x01, 0x55, 0x00]);
        f.extend_from_slice(&DATA_TAIL);
        assert_eq!(f.len(), 45);
        let mut p = Parser::new();
        let got = collect(&mut p, &f);
        let Frame::Target(t) = got[0] else { panic!("{got:?}") };
        assert!(t.engineering);
        assert_eq!(t.moving_dist_cm, 100);
        assert_eq!(t.moving_energy, 50);
    }

    #[test]
    fn version_ack() {
        // ACK to A0: cmd A0 01, status 00 00, type 01 00, minor 04 major 02, build 11 25 02 23
        let ack = [
            0xFD, 0xFC, 0xFB, 0xFA, 0x0C, 0x00, 0xA0, 0x01, 0x00, 0x00, 0x01, 0x00, 0x04, 0x02, 0x11, 0x25, 0x02, 0x23,
            0x04, 0x03, 0x02, 0x01,
        ];
        let mut p = Parser::new();
        let got = collect(&mut p, &ack);
        let Frame::Ack(a) = &got[0] else { panic!() };
        assert!(a.ok());
        let v = FwVersion::from_ack(a).unwrap();
        assert_eq!(v.to_string::<24>().as_str(), "2.04.23022511");
    }

    #[test]
    fn failed_ack() {
        let ack = [0xFD, 0xFC, 0xFB, 0xFA, 0x04, 0x00, 0xFF, 0x01, 0x01, 0x00, 0x04, 0x03, 0x02, 0x01];
        let mut p = Parser::new();
        let got = collect(&mut p, &ack);
        let Frame::Ack(a) = &got[0] else { panic!() };
        assert_eq!(a.cmd, CMD_ENABLE_CONFIG);
        assert!(!a.ok());
        assert!(FwVersion::from_ack(a).is_none());
    }

    #[test]
    fn resyncs_through_garbage() {
        let good = basic_frame(1, 120, 40, 0, 0, 300);
        let mut stream = SVec::new();
        // Leading noise including partial headers and a fake header with crazy length.
        stream.extend_from_slice(&[0x00, 0xF4, 0xF3, 0x11, 0xFD, 0xFC, 0xFB, 0xFA, 0xFF, 0xFF, 0x42]);
        stream.extend_from_slice(&good);
        // Truncated frame (cut mid-way) immediately followed by a good one.
        stream.extend_from_slice(&good[..10]);
        stream.extend_from_slice(&good);
        // Frame with wrong tail, then good.
        let mut bad = good.clone();
        *bad.last_mut().unwrap() = 0x00;
        stream.extend_from_slice(&bad);
        stream.extend_from_slice(&good);
        // Header + valid tail but corrupt body (no AA): counted as bad frame.
        let mut nobody = good.clone();
        nobody[7] = 0x00;
        stream.extend_from_slice(&nobody);
        stream.extend_from_slice(&good);

        let mut p = Parser::new();
        let got = collect(&mut p, &stream);
        assert_eq!(got.len(), 4, "{got:?}");
        assert!(got.iter().all(|f| matches!(f, Frame::Target(t) if t.moving_dist_cm == 120)));
        assert_eq!(p.stats.bad_frames, 1);
        assert!(p.stats.dropped_bytes > 0);
    }

    #[test]
    fn byte_at_a_time_equals_bulk() {
        let mut stream = SVec::new();
        for i in 0..20u16 {
            stream.extend_from_slice(&basic_frame((i % 4) as u8, i * 10, i as u8, i, 0, 0));
            stream.extend_from_slice(&[0xF4, 0x01, 0xFD]); // noise between frames
        }
        let mut a = Parser::new();
        let bulk = collect(&mut a, &stream);
        let mut b = Parser::new();
        let mut single = SVec::new();
        for &x in &stream {
            b.feed(&[x], |f| single.push(f));
        }
        assert_eq!(bulk.len(), 20);
        assert_eq!(bulk, single);
    }

    #[test]
    fn random_noise_never_panics() {
        // xorshift so the test is deterministic without a rand dependency
        let mut s: u32 = 0x1234_5678;
        let mut p = Parser::new();
        let mut buf = [0u8; 4096];
        for b in buf.iter_mut() {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            // bias towards header bytes to exercise partial matches
            *b = match s % 8 {
                0 => 0xF4,
                1 => 0xFD,
                _ => (s >> 8) as u8,
            };
        }
        for _ in 0..50 {
            p.feed(&buf, |_| {});
        }
        // and a good frame still comes through afterwards
        let got = collect(&mut p, &[&[0u8; 70][..], &basic_frame(2, 0, 0, 77, 9, 0)].concat());
        assert!(matches!(got.last(), Some(Frame::Target(t)) if t.still_dist_cm == 77));
    }
}
