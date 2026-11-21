//! Sensor station, stage 1 bring-up: 2x ICS-43434 (I2S0) + LD2410 radar (UART2).
//!
//! Output on UART0 (the USB serial console): human-readable log lines plus
//! `station-proto` frames (`0x00 <COBS> 0x00`). No Wi-Fi yet.
//!
//! Threads:
//! - audio: reads I2S DMA buffers, emits an `AudioLevel` every 100 ms and a
//!   `Direction` (GCC-PHAT) for every 1024-sample frame above the level gate.
//! - radar: identifies the LD2410 (enable config, read version, end config),
//!   then parses its reports and emits a `RadarEvent` per report. A missing
//!   radar is logged and retried; it never stops the station.
//! - main: periodic `Hello` and diagnostics.

use std::io::Write as _;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use esp_idf_svc::hal::delay::TickType;
use esp_idf_svc::hal::gpio::{AnyIOPin, Input, PinDriver, Pull};
use esp_idf_svc::hal::i2s::config::{
    Config as I2sChanConfig, DataBitWidth, SlotMode, StdClkConfig, StdConfig, StdGpioConfig, StdSlotConfig,
};
use esp_idf_svc::hal::i2s::{I2sDriver, I2sRx};
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::uart::{config::Config as UartConfig, UartDriver};
use esp_idf_svc::hal::units::Hertz;
use esp_idf_svc::sys;
use log::{info, warn};

use station_core::ld2410::{self, Frame, FwVersion, Parser};
use station_core::doa::{DoaConfig, DoaEstimator, FRAME_LEN as DOA_FRAME};
use station_core::level::LevelMeter;
use station_proto::{encode_frame, Direction, Envelope, Hello, Msg, MAX_FRAME, PROTO_VERSION};

const SAMPLE_RATE: u32 = 48_000;
/// 100 ms windows.
const LEVEL_WINDOW: u16 = (SAMPLE_RATE / 10) as u16;
/// 10 ms per DMA read: 480 frames x 2 slots x 4 bytes.
const I2S_READ_FRAMES: usize = 480;

const HELLO_EVERY: Duration = Duration::from_secs(5);
const RADAR_SILENT_WARN: Duration = Duration::from_secs(2);
const RADAR_WARN_EVERY: Duration = Duration::from_secs(5);
const RADAR_IDENTIFY_RETRY: Duration = Duration::from_secs(10);
const RADAR_ACK_TIMEOUT: Duration = Duration::from_millis(300);

const DOA_LOG_EVERY: Duration = Duration::from_millis(100);

/// Build-time f32 setting, e.g. `STATION_DOA_BIAS=0.8 just flash`.
fn env_f32(v: Option<&'static str>, default: f32) -> f32 {
    v.and_then(|s| s.trim().parse().ok()).unwrap_or(default)
}

/// Direction-finding settings, overridable at build time:
/// `STATION_MIC_MM` (mic spacing, default 130), `STATION_DOA_BIAS` (broadside lag
/// bias in samples, default 0), `STATION_DOA_GATE_DBFS` (level gate, default -50;
/// -200 analyses every frame, for measuring CPU cost).
fn doa_config() -> DoaConfig {
    let d = DoaConfig::default();
    DoaConfig {
        sample_rate_hz: SAMPLE_RATE as f32,
        mic_distance_m: env_f32(option_env!("STATION_MIC_MM"), d.mic_distance_m * 1000.0) / 1000.0,
        lag_bias_samples: env_f32(option_env!("STATION_DOA_BIAS"), d.lag_bias_samples),
        gate_dbfs: env_f32(option_env!("STATION_DOA_GATE_DBFS"), d.gate_dbfs),
        ..d
    }
}

/// Console baud after boot. Set at build time: `STATION_BAUD=921600 just build`.
fn console_baud() -> u32 {
    option_env!("STATION_BAUD").and_then(|s| s.parse().ok()).unwrap_or(115_200)
}

/// Serialises frame writes and owns the sequence counter.
struct FrameTx {
    seq: u16,
    sent: u32,
}

impl FrameTx {
    fn send(&mut self, msg: Msg) {
        let env = Envelope { seq: self.seq, msg };
        self.seq = self.seq.wrapping_add(1);
        let mut buf = [0u8; MAX_FRAME];
        match encode_frame(&env, &mut buf) {
            Ok(n) => {
                // One write call so the UART VFS lock keeps the frame contiguous.
                let mut out = std::io::stdout().lock();
                let _ = out.write_all(&buf[..n]);
                let _ = out.flush();
                self.sent = self.sent.wrapping_add(1);
            }
            Err(e) => warn!("frame encode failed: {e:?}"),
        }
    }
}

type Tx = Arc<Mutex<FrameTx>>;

fn send(tx: &Tx, msg: Msg) {
    // A poisoned lock only means another thread panicked mid-write; keep going.
    let mut g = tx.lock().unwrap_or_else(|p| p.into_inner());
    g.send(msg);
}

struct Shared {
    radar_fw: Mutex<Option<heapless::String<24>>>,
    radar_frames: AtomicU32,
    audio_reads: AtomicU32,
    audio_errors: AtomicU32,
    /// Stereo frames read from I2S; the stats line turns it into a rate per window.
    audio_frames: AtomicU32,
    /// DOA frames analysed (above the gate) / all frames, since the last stats line.
    doa_dirs: AtomicU32,
    doa_frames: AtomicU32,
    /// Time spent in reads that ran GCC-PHAT, since the last stats line (us).
    doa_us_sum: AtomicU32,
    doa_us_max: AtomicU32,
}

fn now_us() -> i64 {
    // SAFETY: esp_timer_get_time has no preconditions.
    unsafe { sys::esp_timer_get_time() }
}

fn station_id() -> u32 {
    let mut mac = [0u8; 6];
    // SAFETY: esp_efuse_mac_get_default writes exactly 6 bytes.
    let rc = unsafe { sys::esp_efuse_mac_get_default(mac.as_mut_ptr()) };
    if rc != sys::ESP_OK {
        warn!("esp_efuse_mac_get_default failed: {rc}");
    }
    u32::from_be_bytes([mac[2], mac[3], mac[4], mac[5]])
}

fn hello(shared: &Shared, id: u32) -> Msg {
    Msg::Hello(Hello {
        proto_version: PROTO_VERSION,
        station_id: id,
        fw_version: heapless::String::try_from(env!("CARGO_PKG_VERSION")).unwrap_or_default(),
        radar_fw: shared.radar_fw.lock().unwrap_or_else(|p| p.into_inner()).clone(),
    })
}

fn main() -> anyhow::Result<()> {
    sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let id = station_id();
    info!("station-fw {} station_id={id:08x} proto v{PROTO_VERSION}", env!("CARGO_PKG_VERSION"));

    let baud = console_baud();
    if baud != 115_200 {
        info!("console: switching UART0 to {baud} baud");
        thread::sleep(Duration::from_millis(50));
        // SAFETY: plain register write on the console UART; no driver owns UART0.
        unsafe {
            sys::uart_wait_tx_idle_polling(0);
            sys::uart_set_baudrate(0, baud);
        }
    }

    let p = Peripherals::take()?;
    let tx: Tx = Arc::new(Mutex::new(FrameTx { seq: 0, sent: 0 }));
    let shared = Arc::new(Shared {
        radar_fw: Mutex::new(None),
        radar_frames: AtomicU32::new(0),
        audio_reads: AtomicU32::new(0),
        audio_errors: AtomicU32::new(0),
        audio_frames: AtomicU32::new(0),
        doa_dirs: AtomicU32::new(0),
        doa_frames: AtomicU32::new(0),
        doa_us_sum: AtomicU32::new(0),
        doa_us_max: AtomicU32::new(0),
    });
    send(&tx, hello(&shared, id));

    // --- audio: I2S0, BCLK 26, WS 25, DIN 33, no MCLK ---
    let i2s_cfg = StdConfig::new(
        I2sChanConfig::default().dma_buffer_count(6).frames_per_buffer(I2S_READ_FRAMES as u32),
        StdClkConfig::from_sample_rate_hz(SAMPLE_RATE),
        StdSlotConfig::philips_slot_default(DataBitWidth::Bits32, SlotMode::Stereo),
        StdGpioConfig::default(),
    );
    let mut i2s =
        I2sDriver::new_std_rx(p.i2s0, &i2s_cfg, p.pins.gpio26, p.pins.gpio33, AnyIOPin::none(), p.pins.gpio25)?;
    i2s.rx_enable()?;
    info!("i2s: 48 kHz, 32-bit Philips stereo, BCLK=26 WS=25 DIN=33");
    {
        let tx = tx.clone();
        let shared = shared.clone();
        thread::Builder::new().name("audio".into()).stack_size(8192).spawn(move || audio_task(i2s, tx, shared))?;
    }

    // --- radar: UART2 256000 8N1, TX 17, RX 16, OUT 27 ---
    let uart_cfg = UartConfig::default().baudrate(Hertz(ld2410::DEFAULT_BAUD)).rx_fifo_size(1024);
    let uart = UartDriver::new(
        p.uart2,
        p.pins.gpio17,
        p.pins.gpio16,
        Option::<AnyIOPin>::None,
        Option::<AnyIOPin>::None,
        &uart_cfg,
    )?;
    let out_pin = PinDriver::input(p.pins.gpio27, Pull::Down)?;
    info!("radar: UART2 {} baud TX=17 RX=16 OUT=27", ld2410::DEFAULT_BAUD);
    {
        let tx = tx.clone();
        let shared = shared.clone();
        thread::Builder::new()
            .name("radar".into())
            .stack_size(8192)
            .spawn(move || radar_task(uart, out_pin, tx, shared, id))?;
    }

    // --- main: hello + diagnostics ---
    let mut last = now_us();
    loop {
        thread::sleep(HELLO_EVERY);
        send(&tx, hello(&shared, id));
        let sent = tx.lock().unwrap_or_else(|p| p.into_inner()).sent;
        let t = now_us();
        let window_us = (t - last).max(1) as f32;
        last = t;
        let frames = shared.audio_frames.swap(0, Ordering::Relaxed);
        let dirs = shared.doa_dirs.swap(0, Ordering::Relaxed);
        let doa_frames = shared.doa_frames.swap(0, Ordering::Relaxed);
        let us_sum = shared.doa_us_sum.swap(0, Ordering::Relaxed);
        let us_max = shared.doa_us_max.swap(0, Ordering::Relaxed);
        info!(
            "stats: frames_sent={sent} radar_frames={} i2s_reads={} i2s_errors={} i2s_rate={:.0}/s \
             doa {dirs}/{doa_frames} frames, {}us avg {us_max}us max, cpu {:.1}% heap_free={} heap_min={}",
            shared.radar_frames.load(Ordering::Relaxed),
            shared.audio_reads.load(Ordering::Relaxed),
            shared.audio_errors.load(Ordering::Relaxed),
            frames as f32 * 1e6 / window_us,
            us_sum.checked_div(dirs).unwrap_or(0),
            us_sum as f32 * 100.0 / window_us,
            unsafe { sys::esp_get_free_heap_size() },
            unsafe { sys::esp_get_minimum_free_heap_size() },
        );
    }
}

fn audio_task(mut i2s: I2sDriver<'static, I2sRx>, tx: Tx, shared: Arc<Shared>) {
    let mut meter = LevelMeter::new(LEVEL_WINDOW);
    let cfg = doa_config();
    let mut doa = DoaEstimator::new(cfg);
    info!(
        "doa: GCC-PHAT {DOA_FRAME} samples/frame, d={:.0} mm, max lag {:.2} samples, search +-{}, bias {:.2}, gate {:.0} dBFS, band {:.0}-{:.0} Hz",
        cfg.mic_distance_m * 1000.0,
        cfg.max_lag_samples(),
        doa.search_range(),
        cfg.lag_bias_samples,
        cfg.gate_dbfs,
        cfg.band_lo_hz,
        cfg.band_hi_hz,
    );
    let mut last_doa_log = Instant::now() - DOA_LOG_EVERY;
    let mut buf = vec![0u8; I2S_READ_FRAMES * 2 * 4];
    let timeout = TickType::new_millis(1000).ticks();
    loop {
        match i2s.read(&mut buf, timeout) {
            Ok(n) => {
                shared.audio_reads.fetch_add(1, Ordering::Relaxed);
                shared.audio_frames.fetch_add((n / 8) as u32, Ordering::Relaxed);
                meter.push_bytes(&buf[..n], |lvl| {
                    info!(
                        "audio: L {:7.1} dBFS  R {:7.1} dBFS  peak {:7.1} dBFS",
                        lvl.rms_dbfs_l, lvl.rms_dbfs_r, lvl.peak_dbfs
                    );
                    send(&tx, Msg::Audio(lvl));
                });
                // A 480-frame read completes at most one 1024-sample frame; keep room for 2.
                // Results are collected first so the timing covers the analysis only,
                // not the (blocking) UART write.
                let mut dirs: heapless::Vec<Direction, 2> = heapless::Vec::new();
                let t0 = now_us();
                doa.push_bytes(&buf[..n], |o| {
                    shared.doa_frames.fetch_add(1, Ordering::Relaxed);
                    if let Some(d) = o.direction {
                        let _ = dirs.push(d);
                    }
                });
                if !dirs.is_empty() {
                    let dt = (now_us() - t0).clamp(0, u32::MAX as i64) as u32;
                    shared.doa_us_sum.fetch_add(dt, Ordering::Relaxed);
                    // Not fetch_max: atomicrmw umax makes the Xtensa backend (esp rustc 1.90.0.0)
                    // fail with "Undefined temporary symbol". fetch_update is a plain CAS loop.
                    let _ = shared.doa_us_max.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |m| (dt > m).then_some(dt));
                }
                for d in dirs {
                    shared.doa_dirs.fetch_add(1, Ordering::Relaxed);
                    send(&tx, Msg::Direction(d));
                    if last_doa_log.elapsed() >= DOA_LOG_EVERY {
                        last_doa_log = Instant::now();
                        info!(
                            "doa: {:+6.1} deg  lag {:+6.2}  conf {:6.1}  rms {:6.1} dBFS  frame {}",
                            d.angle_deg, d.lag_samples, d.confidence, d.rms_dbfs, d.frame
                        );
                    }
                }
            }
            Err(e) => {
                let n = shared.audio_errors.fetch_add(1, Ordering::Relaxed);
                if n % 10 == 0 {
                    warn!("i2s read error ({} so far): {e}", n + 1);
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

/// Read and parse until `want` is acknowledged or `timeout` passes.
/// Data frames seen meanwhile are dropped (the module is quiet in config mode).
fn wait_ack(uart: &UartDriver, parser: &mut Parser, want: u8, timeout: Duration) -> Option<ld2410::Ack> {
    let deadline = Instant::now() + timeout;
    let mut buf = [0u8; 64];
    let mut found = None;
    while found.is_none() && Instant::now() < deadline {
        let n = uart.read(&mut buf, TickType::new_millis(20).ticks()).unwrap_or(0);
        parser.feed(&buf[..n], |f| {
            if let Frame::Ack(a) = f {
                if a.cmd == want {
                    found = Some(a);
                }
            }
        });
    }
    found
}

fn command(uart: &UartDriver, parser: &mut Parser, frame: &[u8], cmd: u8) -> Option<ld2410::Ack> {
    if let Err(e) = uart.write(frame) {
        warn!("radar: uart write failed: {e}");
        return None;
    }
    wait_ack(uart, parser, cmd, RADAR_ACK_TIMEOUT)
}

/// enable config -> read version -> end config. Always tries to end config so
/// the module goes back to reporting even if the version read failed.
fn identify(uart: &UartDriver, parser: &mut Parser) -> Option<FwVersion> {
    let _ = uart.clear_rx();
    match command(uart, parser, &ld2410::enable_config(), ld2410::CMD_ENABLE_CONFIG) {
        Some(a) if a.ok() => {}
        Some(a) => {
            warn!("radar: enable-config refused (status {:#06x})", a.status);
            return None;
        }
        None => {
            warn!("radar: no ACK to enable-config within {RADAR_ACK_TIMEOUT:?} (radar missing or wrong baud?)");
            return None;
        }
    }
    let ver = command(uart, parser, &ld2410::read_version(), ld2410::CMD_READ_VERSION)
        .as_ref()
        .and_then(FwVersion::from_ack);
    if ver.is_none() {
        warn!("radar: no valid firmware-version ACK");
    }
    if command(uart, parser, &ld2410::end_config(), ld2410::CMD_END_CONFIG).is_none() {
        warn!("radar: no ACK to end-config");
    }
    ver
}

fn radar_task(uart: UartDriver<'static>, out_pin: PinDriver<'static, Input>, tx: Tx, shared: Arc<Shared>, id: u32) {
    let mut parser = Parser::new();
    let mut identified = false;
    let mut last_identify: Option<Instant> = None;
    let mut last_data = Instant::now();
    let mut last_warn = Instant::now();
    let mut buf = [0u8; 128];

    loop {
        if !identified && last_identify.is_none_or(|t| t.elapsed() >= RADAR_IDENTIFY_RETRY) {
            last_identify = Some(Instant::now());
            if let Some(v) = identify(&uart, &mut parser) {
                let s: heapless::String<24> = v.to_string();
                info!("radar: LD2410 firmware {s} (type {:#06x})", v.fw_type);
                *shared.radar_fw.lock().unwrap_or_else(|p| p.into_inner()) = Some(s);
                send(&tx, hello(&shared, id));
                identified = true;
            } else {
                info!("radar: not identified, retry in {RADAR_IDENTIFY_RETRY:?}; station keeps running");
            }
        }

        let n = match uart.read(&mut buf, TickType::new_millis(100).ticks()) {
            Ok(n) => n,
            // The driver reports "no bytes before the timeout" as ESP_ERR_TIMEOUT.
            Err(e) if e.code() == sys::ESP_ERR_TIMEOUT => 0,
            Err(e) => {
                warn!("radar: uart read error: {e}");
                0
            }
        };
        let mut got = 0u32;
        parser.feed(&buf[..n], |f| match f {
            Frame::Target(t) => {
                got += 1;
                let ev = t.to_event();
                info!(
                    "radar: present={} state={} moving {:4} cm e{:3}  static {:4} cm e{:3}  detect {:4} cm out={}",
                    ev.present as u8,
                    t.state,
                    t.moving_dist_cm,
                    t.moving_energy,
                    t.still_dist_cm,
                    t.still_energy,
                    t.detect_dist_cm,
                    out_pin.is_high() as u8,
                );
                send(&tx, Msg::Radar(ev));
            }
            Frame::Ack(a) => info!("radar: unsolicited ACK cmd={:#04x} status={:#06x}", a.cmd, a.status),
        });
        if got > 0 {
            shared.radar_frames.fetch_add(got, Ordering::Relaxed);
            last_data = Instant::now();
        } else if last_data.elapsed() >= RADAR_SILENT_WARN && last_warn.elapsed() >= RADAR_WARN_EVERY {
            last_warn = Instant::now();
            warn!(
                "radar: no data frames for {:.0?} (dropped {} bytes, {} bad frames); continuing",
                last_data.elapsed(),
                parser.stats.dropped_bytes,
                parser.stats.bad_frames
            );
        }
    }
}
