//! Sensor station link: consumes `station-proto` frames (UDP datagrams and/or USB serial) and
//! turns them into brain events. See docs/STATION.md.
//!
//! The station only reports; every threshold stays in the brain. Radar becomes the `radar`
//! presence source (via `Event::StationRadar`, see `presence.rs`), raw audio goes through the
//! normal front-end + shout detector (`AudioManager::attach_external`), levels and direction
//! are published for the dashboard and are not used for game decisions.
//!
//! [`StationState`] is the pure core (frames in, events and audio out, time passed in);
//! the transports below are thin readers that feed it.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use station_proto::{
    encode_command, AudioChunk, Chunk, Command, CommandEnvelope, Envelope, Msg, RadarEvent,
    StreamSplitter, MAX_FRAME,
};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use super::audio_manager::AudioManager;
use crate::config::StationConfig;
use crate::core::{Event, SharedEventBus};

/// UDP port the station listens on for command frames.
pub const STATION_CMD_PORT: u16 = 4211;
/// Raw audio is always 16 kHz stereo i16.
pub const AUDIO_RATE: u32 = 16_000;
/// `StartAudio` must be re-sent well inside `AUDIO_KEEPALIVE_MS` (5 s).
const KEEPALIVE: Duration = Duration::from_secs(1);
/// A gap larger than this (5 s of audio) is a restart of the stream, not loss: no silence fill.
const MAX_FILL_FRAMES: u32 = 5 * AUDIO_RATE;
/// Minimum spacing of published sensor events (radar also publishes immediately on change).
const RADAR_EVERY: Duration = Duration::from_millis(500);
const LEVEL_EVERY: Duration = Duration::from_millis(500);
const DIRECTION_EVERY: Duration = Duration::from_millis(200);

/// What a transport hands to the state machine.
#[derive(Debug)]
pub enum Input {
    Frame(Box<Envelope>),
    /// Bytes that were neither a valid frame nor log text.
    Bad,
    /// Transport problem (open failed, read error), already de-duplicated by the transport.
    LinkError(String),
}

/// What the state machine wants done.
#[derive(Debug)]
pub enum Out {
    Event(Event),
    /// Mono samples, -1..1, at [`AUDIO_RATE`].
    Audio(Vec<f32>),
}

/// Link counters, published as `StationLinkStats`.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct LinkStats {
    pub frames: u64,
    /// Number of times the sequence number jumped forward.
    pub seq_gaps: u64,
    /// Frames missing in those jumps.
    pub frames_lost: u64,
    pub bad_frames: u64,
    /// Audio frames (samples per channel) replaced by silence.
    pub audio_gap_frames: u64,
}

/// Pure station link state.
#[derive(Debug, Default)]
pub struct StationState {
    pub stats: LinkStats,
    last_seq: Option<u16>,
    next_audio_frame: Option<u32>,
    connected: bool,
    last_frame: Option<Instant>,
    station_id: Option<u32>,
    fw_version: Option<String>,
    last_radar: Option<(Instant, bool)>,
    last_level: Option<Instant>,
    last_direction: Option<Instant>,
}

fn due(last: &mut Option<Instant>, now: Instant, every: Duration) -> bool {
    if last.is_none_or(|t| now.duration_since(t) >= every) {
        *last = Some(now);
        true
    } else {
        false
    }
}

/// Radar event as the brain sees it: presence only for target states 1..=3 (4..=6 are
/// background-correction progress on newer firmware), distance of the nearest reported target.
fn radar_event(r: &RadarEvent) -> Event {
    let present = r.present && (1..=3).contains(&r.state);
    let distance_cm = match (r.state & 1 != 0, r.state & 2 != 0) {
        _ if !present => None,
        (true, _) => Some(r.moving_dist_cm),
        (false, true) => Some(r.static_dist_cm),
        _ => None,
    };
    Event::StationRadar {
        present,
        distance_cm,
        moving_energy: r.moving_energy,
        static_energy: r.static_energy,
    }
}

impl StationState {
    pub fn on_bad(&mut self) {
        self.stats.bad_frames += 1;
    }

    /// Process one decoded frame received at `now`.
    pub fn on_frame(&mut self, env: Envelope, now: Instant) -> Vec<Out> {
        let mut out = Vec::new();
        self.stats.frames += 1;
        self.last_frame = Some(now);
        if let Some(last) = self.last_seq {
            let gap = env.seq.wrapping_sub(last.wrapping_add(1));
            // A backwards jump (>= 32768) is a station reboot or reordering, not loss.
            if gap != 0 && gap < 0x8000 {
                self.stats.seq_gaps += 1;
                self.stats.frames_lost += gap as u64;
            }
        }
        self.last_seq = Some(env.seq);
        let first = !self.connected;
        self.connected = true;
        if first {
            out.push(self.connected_event());
        }
        match env.msg {
            Msg::Hello(h) => {
                self.station_id = Some(h.station_id);
                self.fw_version = Some(h.fw_version.as_str().to_string());
                // Re-published on every (periodic) Hello: cheap, and fills in the name.
                if !first {
                    out.push(self.connected_event());
                }
            }
            Msg::Radar(r) => {
                let ev = radar_event(&r);
                let present = matches!(ev, Event::StationRadar { present: true, .. });
                let changed = self.last_radar.is_none_or(|(_, p)| p != present);
                if changed {
                    self.last_radar = Some((now, present));
                    out.push(Out::Event(ev));
                } else if let Some((t, _)) = &mut self.last_radar {
                    if now.duration_since(*t) >= RADAR_EVERY {
                        *t = now;
                        out.push(Out::Event(ev));
                    }
                }
            }
            Msg::Audio(l) => {
                if due(&mut self.last_level, now, LEVEL_EVERY) {
                    out.push(Out::Event(Event::StationLevel {
                        rms_dbfs_l: l.rms_dbfs_l,
                        rms_dbfs_r: l.rms_dbfs_r,
                        peak_dbfs: l.peak_dbfs,
                    }));
                }
            }
            Msg::Direction(d) => {
                if due(&mut self.last_direction, now, DIRECTION_EVERY) {
                    out.push(Out::Event(Event::StationDirection {
                        angle_deg: d.angle_deg,
                        confidence: d.confidence,
                        rms_dbfs: d.rms_dbfs,
                    }));
                }
            }
            Msg::AudioChunk(c) => {
                if let Some(a) = self.on_chunk(&c) {
                    out.push(Out::Audio(a));
                }
            }
        }
        out
    }

    fn connected_event(&self) -> Out {
        Out::Event(Event::StationConnected {
            station_id: self.station_id,
            fw_version: self.fw_version.clone(),
        })
    }

    /// Chunk to mono samples, with silence in place of dropped chunks so the audio keeps
    /// real-time (the same thing `station_record.py` does).
    fn on_chunk(&mut self, c: &AudioChunk) -> Option<Vec<f32>> {
        if c.rate_hz as u32 != AUDIO_RATE || c.channels != 2 {
            self.stats.bad_frames += 1;
            return None;
        }
        let n = c.frames() as u32;
        let mut mono = Vec::with_capacity(n as usize);
        if let Some(next) = self.next_audio_frame {
            if c.frame > next && c.frame - next <= MAX_FILL_FRAMES {
                let gap = c.frame - next;
                self.stats.audio_gap_frames += gap as u64;
                mono.resize(gap as usize, 0.0);
            }
            // c.frame < next: stream restarted (or reordered); resync without fill.
        }
        self.next_audio_frame = Some(c.frame.wrapping_add(n));
        let mut it = c.iter_i16();
        while let (Some(l), Some(r)) = (it.next(), it.next()) {
            mono.push((l as f32 + r as f32) / (2.0 * 32768.0));
        }
        Some(mono)
    }

    /// Periodic check; returns a disconnect event when nothing arrived for `timeout`.
    pub fn tick(&mut self, now: Instant, timeout: Duration) -> Option<Event> {
        let silent = self
            .last_frame
            .is_some_and(|t| now.duration_since(t) > timeout);
        if self.connected && silent {
            self.connected = false;
            // A reconnect starts a new audio stream with a fresh frame counter.
            self.next_audio_frame = None;
            self.last_seq = None;
            self.last_radar = None;
            return Some(Event::StationDisconnected {
                reason: format!("no frames for {:.1}s", timeout.as_secs_f32()),
            });
        }
        None
    }

    pub fn stats_event(&self) -> Event {
        let s = self.stats;
        Event::StationLinkStats {
            frames: s.frames,
            seq_gaps: s.seq_gaps,
            frames_lost: s.frames_lost,
            bad_frames: s.bad_frames,
            audio_gap_frames: s.audio_gap_frames,
        }
    }
}

/// Printable text (a log line) as opposed to a damaged frame.
fn looks_like_text(b: &[u8]) -> bool {
    b.iter()
        .all(|&c| c == b'\n' || c == b'\r' || c == b'\t' || (0x20..0x7f).contains(&c))
}

/// Feed `bytes` through `splitter`, sending frames (and damaged-frame markers) to `tx`.
/// Returns false when the consumer is gone.
fn split_into(
    splitter: &mut StreamSplitter<MAX_FRAME>,
    bytes: &[u8],
    mut send: impl FnMut(Input) -> bool,
) -> bool {
    for &b in bytes {
        let item = match splitter.push(b) {
            Some(Chunk::Frame(env)) => Input::Frame(Box::new(env)),
            Some(Chunk::Other { bytes, .. }) if !looks_like_text(bytes) => Input::Bad,
            _ => continue,
        };
        if !send(item) {
            return false;
        }
    }
    true
}

fn command_frame(cmd: Command) -> Vec<u8> {
    let mut buf = [0u8; MAX_FRAME];
    let n = encode_command(&CommandEnvelope { seq: 0, cmd }, &mut buf).expect("command fits");
    buf[..n].to_vec()
}

/// Keeps transports running; dropping it stops the serial thread.
pub struct StationHandle {
    /// Local address of the bound UDP socket (useful with port 0).
    #[allow(dead_code)] // read by tests
    pub udp_local_addr: Option<SocketAddr>,
    stop: Arc<AtomicBool>,
}

impl Drop for StationHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Start the station link. Returns an error only for unusable configuration (UDP bind
/// failure); a missing serial port is retried forever and reported as `StationError`.
pub async fn spawn(
    cfg: StationConfig,
    bus: SharedEventBus,
    audio: Option<Arc<AudioManager>>,
) -> anyhow::Result<StationHandle> {
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<Input>(512);

    let mut udp_local_addr = None;
    if let Some(bind) = cfg.udp_bind.as_deref() {
        let sock = Arc::new(
            UdpSocket::bind(bind)
                .await
                .map_err(|e| anyhow::anyhow!("station udp_bind {bind}: {e}"))?,
        );
        udp_local_addr = Some(sock.local_addr()?);
        info!("Station UDP listening on {}", sock.local_addr()?);
        let fixed: Option<SocketAddr> = match cfg.station_cmd_addr.as_deref() {
            Some(a) => Some(
                a.parse()
                    .map_err(|e| anyhow::anyhow!("station_cmd_addr {a}: {e}"))?,
            ),
            None => None,
        };
        let peer: Arc<Mutex<Option<SocketAddr>>> = Arc::new(Mutex::new(None));
        tokio::spawn(udp_reader(sock.clone(), tx.clone(), peer.clone()));
        // Always send the 1 Hz command: with audio off it is StopAudio, a no-op for the station, but it lets a
        // station whose stored brain address went stale find and adopt this host (station-fw discovery).
        tokio::spawn(udp_keepalive(sock, fixed, peer, cfg.stream_audio));
    }
    if let Some(port) = cfg.serial_port.clone() {
        let (tx, stop, baud, stream) =
            (tx.clone(), stop.clone(), cfg.serial_baud, cfg.stream_audio);
        std::thread::Builder::new()
            .name("station-serial".into())
            .spawn(move || serial_loop(&port, baud, stream, &tx, &stop))?;
    }
    drop(tx);
    tokio::spawn(consume(cfg, bus, audio, rx));
    Ok(StationHandle {
        udp_local_addr,
        stop,
    })
}

async fn consume(
    cfg: StationConfig,
    bus: SharedEventBus,
    audio: Option<Arc<AudioManager>>,
    mut rx: mpsc::Receiver<Input>,
) {
    let mut st = StationState::default();
    let timeout = Duration::from_secs_f32(cfg.link_timeout_sec.max(0.2));
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let mut stats = tokio::time::interval(Duration::from_secs_f32(cfg.stats_interval_sec.max(0.5)));
    loop {
        tokio::select! {
            i = rx.recv() => {
                let Some(i) = i else { break };
                match i {
                    Input::Frame(env) => {
                        for o in st.on_frame(*env, Instant::now()) {
                            match o {
                                Out::Event(e) => bus.publish(e),
                                Out::Audio(a) => if let Some(m) = &audio { m.push_external(&a) },
                            }
                        }
                    }
                    Input::Bad => st.on_bad(),
                    Input::LinkError(e) => bus.publish(Event::StationError { error: e }),
                }
            }
            _ = tick.tick() => {
                if let Some(e) = st.tick(Instant::now(), timeout) {
                    warn!("Station link down");
                    bus.publish(e);
                }
            }
            _ = stats.tick() => {
                if st.stats.frames > 0 || st.stats.bad_frames > 0 {
                    bus.publish(st.stats_event());
                }
            }
        }
    }
}

async fn udp_reader(
    sock: Arc<UdpSocket>,
    tx: mpsc::Sender<Input>,
    peer: Arc<Mutex<Option<SocketAddr>>>,
) {
    let mut buf = vec![0u8; 2048];
    let mut splitter = StreamSplitter::<MAX_FRAME>::new();
    loop {
        match sock.recv_from(&mut buf).await {
            Ok((n, from)) => {
                *peer.lock().unwrap() = Some(from);
                // One frame per datagram. Collect first so no borrow is held across await.
                let mut items = Vec::new();
                split_into(&mut splitter, &buf[..n], |i| {
                    items.push(i);
                    true
                });
                for i in items {
                    if tx.send(i).await.is_err() {
                        return;
                    }
                }
            }
            Err(e) => {
                warn!("Station UDP recv: {e}");
                if tx
                    .send(Input::LinkError(format!("udp recv: {e}")))
                    .await
                    .is_err()
                {
                    return;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

async fn udp_keepalive(
    sock: Arc<UdpSocket>,
    fixed: Option<SocketAddr>,
    peer: Arc<Mutex<Option<SocketAddr>>>,
    stream_audio: bool,
) {
    let frame = command_frame(if stream_audio { Command::StartAudio } else { Command::StopAudio });
    let mut t = tokio::time::interval(KEEPALIVE);
    loop {
        t.tick().await;
        let target = fixed.or_else(|| {
            peer.lock()
                .unwrap()
                .map(|p| SocketAddr::new(p.ip(), STATION_CMD_PORT))
        });
        if let Some(a) = target {
            if let Err(e) = sock.send_to(&frame, a).await {
                debug!("Station keepalive to {a}: {e}");
            }
        }
    }
}

/// Blocking serial reader/writer. Reopens the port after errors, so unplug and replug works.
fn serial_loop(
    port: &str,
    baud: u32,
    stream_audio: bool,
    tx: &mpsc::Sender<Input>,
    stop: &AtomicBool,
) {
    let mut last_err = String::new();
    let start = command_frame(Command::StartAudio);
    while !stop.load(Ordering::Relaxed) {
        let opened = serialport::new(port, baud)
            .timeout(Duration::from_millis(50))
            .open();
        let mut p = match opened {
            Ok(p) => p,
            Err(e) => {
                let msg = format!("serial {port}: {e}");
                if msg != last_err {
                    warn!("Station {msg}");
                    if tx.blocking_send(Input::LinkError(msg.clone())).is_err() {
                        return;
                    }
                    last_err = msg;
                }
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
        };
        // Opening must not reset the board (DTR/RTS wired to EN/IO0 on dev kits).
        let _ = p.write_data_terminal_ready(false);
        let _ = p.write_request_to_send(false);
        info!("Station serial {port} open at {baud} baud");
        last_err.clear();
        let mut splitter = StreamSplitter::<MAX_FRAME>::new();
        let mut buf = [0u8; 4096];
        let mut last_ka: Option<Instant> = None;
        let err = loop {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            if stream_audio && last_ka.is_none_or(|t| t.elapsed() >= KEEPALIVE) {
                last_ka = Some(Instant::now());
                if let Err(e) = p.write_all(&start) {
                    break format!("serial write: {e}");
                }
            }
            match p.read(&mut buf) {
                Ok(n) => {
                    if !split_into(&mut splitter, &buf[..n], |i| tx.blocking_send(i).is_ok()) {
                        return;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => break format!("serial read: {e}"),
            }
        };
        warn!("Station {err}; reopening");
        last_err = err.clone();
        if tx.blocking_send(Input::LinkError(err)).is_err() {
            return;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::EventBus;
    use station_proto::{encode_frame, AudioLevel, Direction, Hello, PROTO_VERSION};

    fn env(seq: u16, msg: Msg) -> Envelope {
        Envelope { seq, msg }
    }

    fn wire(e: &Envelope) -> Vec<u8> {
        let mut b = [0u8; MAX_FRAME];
        let n = encode_frame(e, &mut b).unwrap();
        b[..n].to_vec()
    }

    fn radar(present: bool, state: u8) -> Msg {
        Msg::Radar(RadarEvent {
            present,
            moving_dist_cm: 150,
            static_dist_cm: 310,
            moving_energy: 55,
            static_energy: 9,
            state,
        })
    }

    fn chunk(frame: u32, frames: usize, v: i16) -> Msg {
        Msg::AudioChunk(AudioChunk {
            rate_hz: 16000,
            channels: 2,
            frame,
            clipped: 0,
            samples: (0..frames)
                .flat_map(|_| [v, v])
                .flat_map(|s| s.to_le_bytes())
                .collect(),
        })
    }

    fn events(o: &[Out]) -> Vec<&Event> {
        o.iter()
            .filter_map(|o| if let Out::Event(e) = o { Some(e) } else { None })
            .collect()
    }

    fn audio_len(o: &[Out]) -> usize {
        o.iter()
            .map(|o| if let Out::Audio(a) = o { a.len() } else { 0 })
            .sum()
    }

    #[test]
    fn hello_names_the_station_and_connects_once() {
        let mut st = StationState::default();
        let t = Instant::now();
        let o = st.on_frame(env(0, radar(true, 1)), t);
        assert!(matches!(
            events(&o)[0],
            Event::StationConnected {
                station_id: None,
                ..
            }
        ));
        let hello = Msg::Hello(Hello {
            proto_version: PROTO_VERSION,
            station_id: 0x1234_abcd,
            fw_version: "0.1.0".try_into().unwrap(),
            radar_fw: None,
        });
        let o = st.on_frame(env(1, hello), t);
        assert!(matches!(
            events(&o)[0],
            Event::StationConnected { station_id: Some(0x1234_abcd), fw_version: Some(v) } if v == "0.1.0"
        ));
    }

    #[test]
    fn radar_uses_presence_bool_and_nearest_distance() {
        let r = |p, s| match radar_event(&RadarEvent {
            present: p,
            moving_dist_cm: 150,
            static_dist_cm: 310,
            moving_energy: 0,
            static_energy: 0,
            state: s,
        }) {
            Event::StationRadar {
                present,
                distance_cm,
                ..
            } => (present, distance_cm),
            _ => unreachable!(),
        };
        assert_eq!(r(true, 1), (true, Some(150)));
        assert_eq!(r(true, 2), (true, Some(310)));
        assert_eq!(r(true, 3), (true, Some(150)));
        assert_eq!(r(false, 0), (false, None));
        // background correction in progress is not a person
        assert_eq!(r(true, 5), (false, None));
    }

    #[test]
    fn radar_events_are_throttled_but_changes_pass() {
        let mut st = StationState::default();
        let t0 = Instant::now();
        let mut n = 0;
        for i in 0..20u16 {
            let o = st.on_frame(
                env(i, radar(true, 1)),
                t0 + Duration::from_millis(100 * i as u64),
            );
            n += events(&o)
                .iter()
                .filter(|e| matches!(e, Event::StationRadar { .. }))
                .count();
        }
        assert_eq!(n, 4, "first + one per 500 ms over 2 s");
        let o = st.on_frame(env(20, radar(false, 0)), t0 + Duration::from_millis(2001));
        assert!(events(&o)
            .iter()
            .any(|e| matches!(e, Event::StationRadar { present: false, .. })));
    }

    #[test]
    fn level_and_direction_pass_through_throttled() {
        let mut st = StationState::default();
        let t = Instant::now();
        let lvl = Msg::Audio(AudioLevel {
            rms_dbfs_l: -42.5,
            rms_dbfs_r: -60.25,
            peak_dbfs: -12.0,
            frames: 4800,
        });
        let dir = Msg::Direction(Direction {
            frame: 1,
            lag_samples: 1.0,
            angle_deg: 30.0,
            confidence: 9.0,
            rms_dbfs: -30.0,
        });
        let o = st.on_frame(env(0, lvl.clone()), t);
        assert!(events(&o).iter().any(|e| matches!(e, Event::StationLevel { rms_dbfs_l, rms_dbfs_r, peak_dbfs } if (*rms_dbfs_l, *rms_dbfs_r, *peak_dbfs) == (-42.5, -60.25, -12.0))));
        let o = st.on_frame(env(1, lvl), t + Duration::from_millis(100));
        assert!(!events(&o)
            .iter()
            .any(|e| matches!(e, Event::StationLevel { .. })));
        let o = st.on_frame(env(2, dir), t);
        assert!(events(&o).iter().any(|e| matches!(e, Event::StationDirection { angle_deg, confidence, rms_dbfs } if (*angle_deg, *confidence, *rms_dbfs) == (30.0, 9.0, -30.0))));
    }

    #[test]
    fn sequence_gaps_and_wraparound() {
        let mut st = StationState::default();
        let t = Instant::now();
        for s in [65534u16, 65535, 0, 1] {
            st.on_frame(env(s, radar(true, 1)), t);
        }
        assert_eq!(st.stats.seq_gaps, 0, "wrap is not a gap");
        st.on_frame(env(5, radar(true, 1)), t); // 2,3,4 lost
        assert_eq!((st.stats.seq_gaps, st.stats.frames_lost), (1, 3));
        st.on_frame(env(0, radar(true, 1)), t); // reboot: backwards, not loss
        assert_eq!((st.stats.seq_gaps, st.stats.frames_lost), (1, 3));
    }

    #[test]
    fn audio_mixes_to_mono_and_fills_gaps_with_silence() {
        let mut st = StationState::default();
        let t = Instant::now();
        let o = st.on_frame(env(0, chunk(0, 160, 16384)), t);
        let Out::Audio(a) = o.last().unwrap() else {
            panic!()
        };
        assert_eq!(a.len(), 160);
        assert!((a[0] - 0.5).abs() < 1e-4);
        // contiguous: no fill
        let o = st.on_frame(env(1, chunk(160, 160, 0)), t);
        assert_eq!(audio_len(&o), 160);
        // two chunks dropped (320 frames): silence + the chunk
        let o = st.on_frame(env(4, chunk(640, 160, 16384)), t);
        let Out::Audio(a) = o.last().unwrap() else {
            panic!()
        };
        assert_eq!(a.len(), 320 + 160);
        assert!(a[..320].iter().all(|&s| s == 0.0));
        assert!(a[320] > 0.4);
        assert_eq!(st.stats.audio_gap_frames, 320);
        // stream restart (frame counter back to 0): no fill
        let o = st.on_frame(env(5, chunk(0, 160, 1)), t);
        assert_eq!(audio_len(&o), 160);
        assert_eq!(st.stats.audio_gap_frames, 320);
        // absurd jump is a restart too
        let o = st.on_frame(env(6, chunk(1_000_000, 160, 1)), t);
        assert_eq!(audio_len(&o), 160);
    }

    #[test]
    fn wrong_audio_format_is_rejected() {
        let mut st = StationState::default();
        let Msg::AudioChunk(mut c) = chunk(0, 10, 1) else {
            unreachable!()
        };
        c.rate_hz = 48000;
        let o = st.on_frame(env(0, Msg::AudioChunk(c)), Instant::now());
        assert_eq!(audio_len(&o), 0);
        assert_eq!(st.stats.bad_frames, 1);
    }

    #[test]
    fn link_times_out_and_reconnects() {
        let mut st = StationState::default();
        let t = Instant::now();
        let to = Duration::from_secs(3);
        assert!(
            st.tick(t, to).is_none(),
            "never connected, nothing to report"
        );
        st.on_frame(env(0, radar(true, 1)), t);
        assert!(st.tick(t + Duration::from_secs(2), to).is_none());
        assert!(matches!(
            st.tick(t + Duration::from_secs(4), to),
            Some(Event::StationDisconnected { .. })
        ));
        assert!(
            st.tick(t + Duration::from_secs(5), to).is_none(),
            "reported once"
        );
        let o = st.on_frame(env(0, radar(true, 1)), t + Duration::from_secs(6));
        assert!(matches!(events(&o)[0], Event::StationConnected { .. }));
    }

    #[test]
    fn splitter_separates_text_frames_and_damage() {
        let mut sp = StreamSplitter::<MAX_FRAME>::new();
        let mut got = Vec::new();
        let mut stream = b"I (100) station: boot\n".to_vec();
        stream.extend(wire(&env(3, radar(true, 1))));
        stream.extend(b"I (200) a log line\n");
        let mut bad = wire(&env(4, radar(true, 1)));
        let k = bad.len() / 2;
        bad[k] ^= 0x55; // CRC fails
        stream.extend(bad);
        stream.extend(wire(&env(5, radar(false, 0))));
        split_into(&mut sp, &stream, |i| {
            got.push(i);
            true
        });
        let kinds: Vec<_> = got
            .iter()
            .map(|i| match i {
                Input::Frame(e) => format!("frame{}", e.seq),
                Input::Bad => "bad".into(),
                Input::LinkError(_) => "err".into(),
            })
            .collect();
        assert_eq!(kinds, ["frame3", "bad", "frame5"]);
    }

    async fn next_matching(
        rx: &mut tokio::sync::broadcast::Receiver<Event>,
        f: impl Fn(&Event) -> bool,
    ) -> Event {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let e = rx.recv().await.unwrap();
                if f(&e) {
                    return e;
                }
            }
        })
        .await
        .expect("event not seen")
    }

    #[tokio::test]
    async fn udp_loopback_publishes_events_and_sends_keepalive() {
        let bus: SharedEventBus = Arc::new(EventBus::new(100));
        let mut rx = bus.subscribe();
        let cfg = StationConfig {
            udp_bind: Some("127.0.0.1:0".into()),
            stream_audio: true,
            stats_interval_sec: 0.5,
            ..Default::default()
        };
        // Station-side socket: sends frames, and listens for commands on `cmd`.
        let station = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let cmd = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let cfg = StationConfig {
            station_cmd_addr: Some(cmd.local_addr().unwrap().to_string()),
            ..cfg
        };
        let h = spawn(cfg, bus.clone(), None).await.unwrap();
        let brain = h.udp_local_addr.unwrap();

        station
            .send_to(&wire(&env(0, radar(true, 2))), brain)
            .await
            .unwrap();
        let e = next_matching(&mut rx, |e| matches!(e, Event::StationRadar { .. })).await;
        assert!(matches!(
            e,
            Event::StationRadar {
                present: true,
                distance_cm: Some(310),
                moving_energy: 55,
                static_energy: 9
            }
        ));
        // garbage datagram, then a frame with a seq gap
        station
            .send_to(&[1, 2, 3, 4, 0xff, 0x00], brain)
            .await
            .unwrap();
        station
            .send_to(&wire(&env(3, radar(false, 0))), brain)
            .await
            .unwrap();
        let e = next_matching(&mut rx, |e| {
            matches!(
                e,
                Event::StationLinkStats {
                    seq_gaps: 1,
                    bad_frames: 1,
                    ..
                }
            )
        })
        .await;
        assert!(
            matches!(
                e,
                Event::StationLinkStats {
                    frames: 2,
                    frames_lost: 2,
                    ..
                }
            ),
            "{e:?}"
        );

        // StartAudio keepalive arrives as a decodable command frame
        let mut b = [0u8; 256];
        let (n, _) = tokio::time::timeout(Duration::from_secs(3), cmd.recv_from(&mut b))
            .await
            .unwrap()
            .unwrap();
        let body = &b[1..n - 1];
        assert_eq!(
            station_proto::decode_command(body).unwrap().cmd,
            Command::StartAudio
        );
    }

    #[tokio::test]
    async fn udp_audio_reaches_the_audio_pipeline() {
        let bus: SharedEventBus = Arc::new(EventBus::new(100));
        let audio = Arc::new(AudioManager::new(
            crate::config::Config::default().audio,
            bus.clone(),
        ));
        audio.attach_external("station", AUDIO_RATE).await;
        let mut feats = audio.subscribe_features();
        let cfg = StationConfig {
            udp_bind: Some("127.0.0.1:0".into()),
            ..Default::default()
        };
        let h = spawn(cfg, bus, Some(audio)).await.unwrap();
        let station = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        // 1 s of audio, 10 ms chunks, every 10th chunk dropped.
        for i in 0..100u16 {
            if i % 10 == 5 {
                continue;
            }
            let m = chunk(i as u32 * 160, 160, ((i as i32 * 37) % 2000 - 1000) as i16);
            station
                .send_to(&wire(&env(i, m)), h.udp_local_addr.unwrap())
                .await
                .unwrap();
        }
        tokio::time::timeout(Duration::from_secs(3), feats.changed())
            .await
            .unwrap()
            .unwrap();
        let f = feats.borrow().clone().unwrap();
        assert!(f.index < 1000);
    }
}
