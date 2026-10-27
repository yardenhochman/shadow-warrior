//! Long-session recorder for on-site data collection (shadow-warrior-gxg).
//!
//! The audio callback hands `(raw mono block, frame features)` to a bounded channel via
//! `try_send` (never blocks; overflow drops and counts frames). A dedicated writer thread
//! writes rotating raw WAV chunks (`raw_0000.wav`, ...) plus a per-frame `features.jsonl`
//! into a per-session directory. Guards: free disk space, max session duration.
use serde::Serialize;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, mpsc};

use super::audio_frontend::{FrameFeatures, FrameTap, FRAME_SECS, FRAME_SIZE, TARGET_RATE};
use crate::core::Event;

/// Free bytes on the filesystem holding `path`; `None` if unknown. Injectable for tests.
pub type FreeSpaceFn = Arc<dyn Fn(&Path) -> Option<u64> + Send + Sync>;

#[cfg(unix)]
pub fn disk_free_bytes(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    Some((st.f_bavail as u64).saturating_mul(st.f_frsize as u64))
}
#[cfg(not(unix))]
pub fn disk_free_bytes(_path: &Path) -> Option<u64> {
    None
}

#[derive(Clone, Debug)]
pub struct SessionParams {
    pub dir: PathBuf,
    pub label: String,
    pub sample_rate: u32,
    pub chunk_sec: f32,
    pub min_free_bytes: u64,
    pub max_duration_sec: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StopReason {
    Requested,
    DiskLow { free: u64 },
    MaxDuration,
    Io(String),
}

impl StopReason {
    fn text(&self) -> String {
        match self {
            StopReason::Requested => "stopped".into(),
            StopReason::DiskLow { free } => format!("free disk space low ({} MiB left)", free / (1 << 20)),
            StopReason::MaxDuration => "max session duration reached".into(),
            StopReason::Io(e) => format!("write error: {e}"),
        }
    }
}

/// Counters shared between the audio callback, writer thread and API.
#[derive(Default)]
pub struct Shared {
    active: AtomicBool,
    bytes: AtomicU64,
    samples: AtomicU64,
    frames: AtomicU64,
    dropped_frames: AtomicU64,
    chunks: AtomicU32,
    rate: AtomicU32,
    info: Mutex<Info>,
}

#[derive(Default)]
struct Info {
    label: String,
    session_dir: String,
    stop_reason: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct RecordStatus {
    pub active: bool,
    pub label: String,
    pub session_dir: String,
    pub elapsed_sec: f32,
    pub bytes: u64,
    pub frames: u64,
    pub dropped_frames: u64,
    pub chunks: u32,
    /// Why the last session ended (None while running / never run).
    pub stop_reason: Option<String>,
    /// Set when the session ended abnormally (disk low, I/O error, max duration).
    pub error: Option<String>,
}

impl Shared {
    pub fn status(&self) -> RecordStatus {
        let info = self.info.lock().unwrap();
        let rate = self.rate.load(Ordering::Relaxed).max(1);
        RecordStatus {
            active: self.active.load(Ordering::Relaxed),
            label: info.label.clone(),
            session_dir: info.session_dir.clone(),
            elapsed_sec: self.samples.load(Ordering::Relaxed) as f32 / rate as f32,
            bytes: self.bytes.load(Ordering::Relaxed),
            frames: self.frames.load(Ordering::Relaxed),
            dropped_frames: self.dropped_frames.load(Ordering::Relaxed),
            chunks: self.chunks.load(Ordering::Relaxed),
            stop_reason: info.stop_reason.clone(),
            error: info.error.clone(),
        }
    }
}

// ---------------------------------------------------------------- WAV chunk

struct WavChunk {
    f: std::io::BufWriter<std::fs::File>,
    data_bytes: u32,
}

impl WavChunk {
    fn create(path: &Path, rate: u32) -> std::io::Result<Self> {
        let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
        f.write_all(b"RIFF\0\0\0\0WAVEfmt ")?;
        f.write_all(&16u32.to_le_bytes())?;
        f.write_all(&1u16.to_le_bytes())?;
        f.write_all(&1u16.to_le_bytes())?;
        f.write_all(&rate.to_le_bytes())?;
        f.write_all(&(rate * 2).to_le_bytes())?;
        f.write_all(&2u16.to_le_bytes())?;
        f.write_all(&16u16.to_le_bytes())?;
        f.write_all(b"data\0\0\0\0")?;
        Ok(Self { f, data_bytes: 0 })
    }

    fn write(&mut self, samples: &[f32]) -> std::io::Result<()> {
        for &s in samples {
            self.f.write_all(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())?;
        }
        self.data_bytes += (samples.len() * 2) as u32;
        Ok(())
    }

    /// Patch the RIFF/data sizes so the file is valid even if the process dies later.
    fn checkpoint(&mut self) -> std::io::Result<()> {
        self.f.flush()?;
        let pos = self.f.stream_position()?;
        self.f.seek(SeekFrom::Start(4))?;
        self.f.write_all(&(36 + self.data_bytes).to_le_bytes())?;
        self.f.seek(SeekFrom::Start(40))?;
        self.f.write_all(&self.data_bytes.to_le_bytes())?;
        self.f.seek(SeekFrom::Start(pos))?;
        self.f.flush()
    }
}

// ---------------------------------------------------------------- writer

#[derive(Serialize)]
struct Row {
    t: String,
    t_sec: f32,
    frame: u64,
    chunk: u32,
    band_db: f32,
    db_above_floor: f32,
    noise_floor_db: f32,
    vad_prob: f32,
    f0_hz: f32,
    f0_conf: f32,
    centroid_hz: f32,
    tilt_db_per_khz: f32,
    flatness: f32,
    playback_active: bool,
    shout: bool,
}

pub struct SessionWriter {
    p: SessionParams,
    free: FreeSpaceFn,
    shared: Arc<Shared>,
    chunk_samples: usize,
    check_every: usize,
    since_check: usize,
    wav: Option<WavChunk>,
    in_chunk: usize,
    chunk_idx: u32,
    total_samples: u64,
    feats: std::io::BufWriter<std::fs::File>,
    first_frame: Option<u64>,
}

impl SessionWriter {
    pub fn new(p: SessionParams, free: FreeSpaceFn, shared: Arc<Shared>) -> std::io::Result<Self> {
        std::fs::create_dir_all(&p.dir)?;
        let feats = std::io::BufWriter::new(std::fs::File::create(p.dir.join("features.jsonl"))?);
        let rate = p.sample_rate;
        shared.rate.store(rate, Ordering::Relaxed);
        for c in [&shared.bytes, &shared.samples, &shared.frames, &shared.dropped_frames] {
            c.store(0, Ordering::Relaxed);
        }
        shared.chunks.store(0, Ordering::Relaxed);
        {
            let mut i = shared.info.lock().unwrap();
            *i = Info {
                label: p.label.clone(),
                session_dir: p.dir.display().to_string(),
                ..Default::default()
            };
        }
        let w = Self {
            chunk_samples: ((p.chunk_sec * rate as f32) as usize).max(1),
            check_every: (rate as usize * 5).max(1),
            since_check: usize::MAX, // check before the first write
            free,
            shared,
            wav: None,
            in_chunk: 0,
            chunk_idx: 0,
            total_samples: 0,
            feats,
            first_frame: None,
            p,
        };
        w.write_meta(None)?;
        Ok(w)
    }

    fn write_meta(&self, reason: Option<&StopReason>) -> std::io::Result<()> {
        let st = self.shared.status();
        let meta = serde_json::json!({
            "label": self.p.label,
            "written": chrono::Utc::now().to_rfc3339(),
            "raw_sample_rate": self.p.sample_rate,
            "chunk_sec": self.p.chunk_sec,
            "feature_sample_rate": TARGET_RATE,
            "frame_size": FRAME_SIZE,
            "first_frame": self.first_frame,
            "chunks": st.chunks,
            "elapsed_sec": st.elapsed_sec,
            "dropped_frames": st.dropped_frames,
            "stop_reason": reason.map(StopReason::text),
        });
        std::fs::write(self.p.dir.join("session.json"), serde_json::to_vec_pretty(&meta)?)
    }

    fn guard(&mut self) -> Result<(), StopReason> {
        if self.total_samples as f32 >= self.p.max_duration_sec * self.p.sample_rate as f32 {
            return Err(StopReason::MaxDuration);
        }
        if self.since_check >= self.check_every {
            self.since_check = 0;
            if let Some(free) = (self.free)(&self.p.dir) {
                if free < self.p.min_free_bytes {
                    return Err(StopReason::DiskLow { free });
                }
            }
            if let Some(w) = self.wav.as_mut() {
                w.checkpoint().map_err(io_err)?;
            }
        }
        Ok(())
    }

    fn rotate(&mut self) -> Result<(), StopReason> {
        if let Some(mut w) = self.wav.take() {
            w.checkpoint().map_err(io_err)?;
        }
        let path = self.p.dir.join(format!("raw_{:04}.wav", self.chunk_idx));
        self.wav = Some(WavChunk::create(&path, self.p.sample_rate).map_err(io_err)?);
        self.shared.bytes.fetch_add(44, Ordering::Relaxed);
        self.shared.chunks.fetch_add(1, Ordering::Relaxed);
        self.chunk_idx += 1;
        self.in_chunk = 0;
        // Always re-check disk at chunk start.
        self.since_check = usize::MAX;
        Ok(())
    }

    /// Write one callback's worth of data. `shout`: a ShoutDetected event was observed
    /// since the previous block; it is marked on the first row of this block.
    pub fn write_block(&mut self, raw: &[f32], feats: &[FrameFeatures], shout: bool) -> Result<(), StopReason> {
        let mut rest = raw;
        while !rest.is_empty() {
            if self.wav.is_none() || self.in_chunk >= self.chunk_samples {
                self.rotate()?;
            }
            self.guard()?;
            let n = rest.len().min(self.chunk_samples - self.in_chunk);
            self.wav.as_mut().unwrap().write(&rest[..n]).map_err(io_err)?;
            rest = &rest[n..];
            self.in_chunk += n;
            self.total_samples += n as u64;
            self.since_check = self.since_check.saturating_add(n);
            self.shared.samples.store(self.total_samples, Ordering::Relaxed);
            self.shared.bytes.fetch_add((n * 2) as u64, Ordering::Relaxed);
        }
        for (i, f) in feats.iter().enumerate() {
            let first = *self.first_frame.get_or_insert(f.index);
            let row = Row {
                t: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                t_sec: (f.index - first) as f32 * FRAME_SECS,
                frame: f.index,
                chunk: self.chunk_idx.saturating_sub(1),
                band_db: f.band_db,
                db_above_floor: f.db_above_floor,
                noise_floor_db: f.noise_floor_db,
                vad_prob: f.vad_prob,
                f0_hz: f.pitch_hz,
                f0_conf: f.pitch_conf,
                centroid_hz: f.spectral_centroid_hz,
                tilt_db_per_khz: f.spectral_tilt_db_per_khz,
                flatness: f.spectral_flatness,
                playback_active: f.playback_active,
                shout: shout && i == 0,
            };
            let mut line = serde_json::to_vec(&row).map_err(|e| StopReason::Io(e.to_string()))?;
            line.push(b'\n');
            self.feats.write_all(&line).map_err(io_err)?;
            self.shared.bytes.fetch_add(line.len() as u64, Ordering::Relaxed);
        }
        self.shared.frames.fetch_add(feats.len() as u64, Ordering::Relaxed);
        Ok(())
    }

    /// Flush, patch headers, write final meta.
    pub fn finish(mut self, reason: &StopReason) {
        if let Some(mut w) = self.wav.take() {
            let _ = w.checkpoint();
        }
        let _ = self.feats.flush();
        let _ = self.write_meta(Some(reason));
    }
}

fn io_err(e: std::io::Error) -> StopReason {
    StopReason::Io(e.to_string())
}

// ---------------------------------------------------------------- manager

struct Msg {
    raw: Vec<f32>,
    feats: Vec<FrameFeatures>,
}

#[derive(Default)]
pub struct SessionRecorder {
    shared: Arc<Shared>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl SessionRecorder {
    pub fn status(&self) -> RecordStatus {
        self.shared.status()
    }

    pub fn is_active(&self) -> bool {
        self.shared.active.load(Ordering::Relaxed)
    }

    /// Start a session. Returns the tap to install into the front-end. The tap stops
    /// producing when the session ends (writer error / guard) or the tap is dropped.
    pub fn start(
        &self,
        p: SessionParams,
        free: FreeSpaceFn,
        mut events: broadcast::Receiver<Event>,
        queue_len: usize,
    ) -> std::io::Result<FrameTap> {
        if self.is_active() {
            return Err(std::io::Error::other("a recording session is already running"));
        }
        if let Some(h) = self.thread.lock().unwrap().take() {
            let _ = h.join();
        }
        let mut writer = SessionWriter::new(p, free, self.shared.clone())?;
        let (tx, mut rx) = mpsc::channel::<Msg>(queue_len.max(1));
        let shared = self.shared.clone();
        shared.active.store(true, Ordering::Relaxed);
        let handle = std::thread::Builder::new().name("session-recorder".into()).spawn(move || {
            let mut reason = StopReason::Requested;
            while let Some(Msg { raw, feats }) = rx.blocking_recv() {
                let mut shout = false;
                while let Ok(ev) = events.try_recv() {
                    shout |= matches!(ev, Event::ShoutDetected { .. });
                }
                if let Err(r) = writer.write_block(&raw, &feats, shout) {
                    reason = r;
                    break;
                }
            }
            {
                let mut info = shared.info.lock().unwrap();
                info.stop_reason = Some(reason.text());
                if reason != StopReason::Requested {
                    info.error = Some(reason.text());
                }
            }
            writer.finish(&reason);
            shared.active.store(false, Ordering::Relaxed);
        })?;
        *self.thread.lock().unwrap() = Some(handle);

        let shared = self.shared.clone();
        Ok(Box::new(move |raw: &[f32], feats: &[FrameFeatures]| {
            if !shared.active.load(Ordering::Relaxed) {
                return;
            }
            let msg = Msg { raw: raw.to_vec(), feats: feats.to_vec() };
            if tx.try_send(msg).is_err() {
                shared.dropped_frames.fetch_add(feats.len() as u64, Ordering::Relaxed);
            }
        }))
    }

    /// Take the writer thread handle so the caller can join it off the async runtime.
    pub fn take_thread(&self) -> Option<std::thread::JoinHandle<()>> {
        self.thread.lock().unwrap().take()
    }

    /// Wait for the writer to finish (call after dropping the tap). Blocking.
    #[cfg(test)]
    pub fn join(&self) {
        if let Some(h) = self.take_thread() {
            let _ = h.join();
        }
    }
}

/// Make a label safe for a directory name.
pub fn sanitize_label(label: &str) -> String {
    let s: String = label
        .chars()
        .take(40)
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect();
    if s.is_empty() { "session".into() } else { s }
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    fn feat(i: u64) -> FrameFeatures {
        FrameFeatures {
            index: i, band_db: -30.0, noise_floor_db: -50.0, db_above_floor: 20.0, vad_prob: 0.7,
            pitch_hz: 300.0, pitch_conf: 0.8, spectral_centroid_hz: 1500.0,
            spectral_tilt_db_per_khz: -10.0, spectral_flatness: 0.2, playback_active: i.is_multiple_of(2),
        }
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sw_rec_{name}_{}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        d
    }

    fn params(dir: &Path, rate: u32, chunk: f32) -> SessionParams {
        SessionParams {
            dir: dir.to_path_buf(), label: "t".into(), sample_rate: rate, chunk_sec: chunk,
            min_free_bytes: 1000, max_duration_sec: 1e9,
        }
    }

    fn plenty() -> FreeSpaceFn {
        Arc::new(|_| Some(u64::MAX))
    }

    fn wav_samples(p: &Path) -> usize {
        let b = std::fs::read(p).unwrap();
        assert_eq!(&b[0..4], b"RIFF");
        let data = u32::from_le_bytes(b[40..44].try_into().unwrap()) as usize;
        assert_eq!(b.len(), 44 + data, "header data size must match");
        assert_eq!(u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize, 36 + data);
        data / 2
    }

    #[test]
    fn rotates_chunks_and_logs_features() {
        let dir = tmp("rot");
        let sh = Arc::new(Shared::default());
        // 1 s chunks at 1 kHz; feed 2.5 s in uneven blocks.
        let mut w = SessionWriter::new(params(&dir, 1000, 1.0), plenty(), sh.clone()).unwrap();
        let block = vec![0.25f32; 700];
        let mut fi = 10;
        for k in 0..4 {
            let feats: Vec<_> = (0..2).map(|j| feat(fi + j)).collect();
            fi += 2;
            w.write_block(&block[..if k == 3 { 400 } else { 700 }], &feats, k == 1).unwrap();
        }
        w.finish(&StopReason::Requested);
        assert_eq!(wav_samples(&dir.join("raw_0000.wav")), 1000);
        assert_eq!(wav_samples(&dir.join("raw_0001.wav")), 1000);
        assert_eq!(wav_samples(&dir.join("raw_0002.wav")), 500);
        assert!(!dir.join("raw_0003.wav").exists());
        let rows: Vec<serde_json::Value> = std::fs::read_to_string(dir.join("features.jsonl"))
            .unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(rows.len(), 8);
        assert_eq!(rows[0]["frame"], 10);
        assert_eq!(rows[0]["t_sec"], 0.0);
        assert!((rows[7]["t_sec"].as_f64().unwrap() - 7.0 * FRAME_SECS as f64).abs() < 1e-5);
        for key in ["t", "band_db", "db_above_floor", "noise_floor_db", "vad_prob", "f0_hz",
                    "f0_conf", "centroid_hz", "tilt_db_per_khz", "flatness", "playback_active", "shout"] {
            assert!(rows[0].get(key).is_some(), "missing {key}");
        }
        let shouts: Vec<_> = rows.iter().filter(|r| r["shout"] == true).map(|r| r["frame"].as_u64().unwrap()).collect();
        assert_eq!(shouts, vec![12]);
        let st = sh.status();
        assert_eq!((st.chunks, st.frames), (3, 8));
        assert!((st.elapsed_sec - 2.5).abs() < 1e-6);
        assert!(st.bytes > 5000);
        assert!(dir.join("session.json").exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn disk_guard_stops_session() {
        let dir = tmp("disk");
        let sh = Arc::new(Shared::default());
        let free = Arc::new(AtomicU64::new(1_000_000));
        let f2 = free.clone();
        let ff: FreeSpaceFn = Arc::new(move |_| Some(f2.load(Ordering::Relaxed)));
        let mut w = SessionWriter::new(params(&dir, 1000, 100.0), ff, sh).unwrap();
        w.write_block(&vec![0.0; 6000], &[], false).unwrap();
        free.store(10, Ordering::Relaxed); // fall below min_free (1000)
        let e = w.write_block(&vec![0.0; 6000], &[], false).unwrap_err();
        assert_eq!(e, StopReason::DiskLow { free: 10 });
        w.finish(&e);
        let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("session.json")).unwrap()).unwrap();
        assert!(meta["stop_reason"].as_str().unwrap().contains("disk"));
        wav_samples(&dir.join("raw_0000.wav")); // still a valid wav
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn disk_checked_at_start() {
        let dir = tmp("diskstart");
        let ff: FreeSpaceFn = Arc::new(|_| Some(5));
        let mut w = SessionWriter::new(params(&dir, 1000, 10.0), ff, Arc::new(Shared::default())).unwrap();
        assert!(matches!(w.write_block(&[0.0; 10], &[], false), Err(StopReason::DiskLow { .. })));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn max_duration_stops() {
        let dir = tmp("max");
        let mut p = params(&dir, 1000, 10.0);
        p.max_duration_sec = 1.0;
        let mut w = SessionWriter::new(p, plenty(), Arc::new(Shared::default())).unwrap();
        w.write_block(&vec![0.0; 1000], &[], false).unwrap();
        assert_eq!(w.write_block(&[0.0; 10], &[], false), Err(StopReason::MaxDuration));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn overflow_drops_and_counts_then_stop_flushes() {
        let dir = tmp("ovf");
        let rec = SessionRecorder::default();
        let bus = crate::core::EventBus::new(16);
        // A held lock inside the free-space fn stalls the writer so the 2-slot queue overflows.
        let gate = Arc::new(Mutex::new(()));
        let held = gate.lock().unwrap();
        let g2 = gate.clone();
        let ff: FreeSpaceFn = Arc::new(move |_| {
            let _g = g2.lock().unwrap();
            Some(u64::MAX)
        });
        let mut tap = rec.start(params(&dir, 1000, 10.0), ff, bus.subscribe(), 2).unwrap();
        for i in 0..20 {
            tap(&[0.1; 100], &[feat(i)]);
        }
        let dropped = rec.status().dropped_frames;
        assert!(dropped >= 17, "dropped {dropped}");
        drop(held);
        drop(tap);
        rec.join();
        let st = rec.status();
        assert!(!st.active && st.error.is_none(), "{st:?}");
        assert_eq!(st.frames + st.dropped_frames, 20);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn shout_event_marks_row() {
        let dir = tmp("shout");
        let rec = SessionRecorder::default();
        let bus = crate::core::EventBus::new(16);
        let mut tap = rec.start(params(&dir, 1000, 10.0), plenty(), bus.subscribe(), 8).unwrap();
        bus.publish(Event::shout_detected(1.0, 0.3, -20.0));
        tap(&[0.1; 100], &[feat(0), feat(1)]);
        drop(tap);
        rec.join();
        let s = std::fs::read_to_string(dir.join("features.jsonl")).unwrap();
        assert!(s.lines().next().unwrap().contains("\"shout\":true"), "{s}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn rejects_second_session_and_sanitizes_label() {
        let dir = tmp("two");
        let rec = SessionRecorder::default();
        let bus = crate::core::EventBus::new(4);
        let tap = rec.start(params(&dir, 1000, 10.0), plenty(), bus.subscribe(), 4).unwrap();
        assert!(rec.start(params(&dir, 1000, 10.0), plenty(), bus.subscribe(), 4).is_err());
        drop(tap);
        rec.join();
        assert_eq!(sanitize_label("a b/../c"), "a_b____c");
        assert_eq!(sanitize_label(""), "session");
        std::fs::remove_dir_all(dir).ok();
    }
}
