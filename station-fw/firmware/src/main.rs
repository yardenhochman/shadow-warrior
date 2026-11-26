//! Sensor station, stage 1 bring-up: 2x ICS-43434 (I2S0) + LD2410 radar (UART2).
//!
//! Output on UART0 (the USB serial console): human-readable log lines plus
//! `station-proto` frames (`0x00 <COBS> 0x00`). No Wi-Fi yet.
//!
//! UART0 runs the IDF UART driver (16 KB TX ring, so writes do not busy-wait) and
//! its RX carries host commands (`station-proto` `CommandEnvelope` frames).
//!
//! Threads:
//! - audio: reads I2S DMA buffers, emits an `AudioLevel` every 100 ms and a
//!   `Direction` (GCC-PHAT) for every 1024-sample frame above the level gate.
//!   While the host keeps raw streaming on (`Command::StartAudio`), it also
//!   decimates to 16 kHz / 16-bit and sends an `AudioChunk` every 10 ms; chunks
//!   that do not fit in the TX ring are dropped and counted, never waited for.
//! - cmd: reads host command frames from UART0 RX.
//! - radar: identifies the LD2410 (enable config, read version, end config),
//!   then parses its reports and emits a `RadarEvent` per report. A missing
//!   radar is logged and retried; it never stops the station.
//! - main: periodic `Hello` and diagnostics.

mod ble;
mod net;

use std::io::Write as _;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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
use station_core::decimate::{self, Decimator};
use station_core::doa::{DoaConfig, DoaEstimator, FRAME_LEN as DOA_FRAME};
use station_core::level::LevelMeter;
use station_core::selftest::{self, ChannelStats, Level, MicProbe, RadarStats};
use station_proto::{
    decode_command, encode_frame, AudioChunk, Command, Direction, Envelope, Hello, Msg, AUDIO_CHUNK_BYTES,
    AUDIO_KEEPALIVE_MS, MAX_FRAME, PROTO_VERSION,
};

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

/// Self test: runs once at boot, then every `SELFTEST_EVERY` (logs again only when the result changes).
/// The mic probe skips 0.5 s after the request (I2S settling), then reads 1 s.
const SELFTEST_EVERY: Duration = Duration::from_secs(60);
const PROBE_SKIP_READS: u32 = 50;
const PROBE_READS: u32 = 100;
/// Radar observation window of the self test.
const SELFTEST_RADAR_WINDOW: Duration = Duration::from_secs(3);

/// Console UART (UART0) driver ring buffers. 16 KB of TX is about 180 ms at 921600 baud.
const CONSOLE_UART: sys::uart_port_t = 0;
const CONSOLE_TX_RING: i32 = 16 * 1024;
const CONSOLE_RX_RING: i32 = 1024;
/// 16 kHz stereo 16-bit is 64 KB/s plus framing; 921600 baud carries 92 KB/s.
const MIN_STREAM_BAUD: u32 = 500_000;

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
///
/// Every frame goes to the UART driver in one `uart_write_bytes` call, which holds the
/// driver's TX lock for the whole buffer, so a frame is never split. Log text goes
/// through the VFS (newlib stdout) into the same driver and can only land between
/// frames; readers split on 0x00 and treat non-frames as text.
struct FrameTx {
    seq: u16,
    sent: u32,
    /// Set once the UART0 driver is installed; before that frames go through stdout.
    driver: bool,
    /// Connected UDP socket to the brain while Wi-Fi is up.
    udp: Option<UdpSocket>,
    /// Broadcast-enabled socket, created on first use, for finding a brain whose address changed.
    bcast: Option<UdpSocket>,
}

impl FrameTx {
    /// Encode and write `msg`. With `droppable`, the frame is skipped (returns false,
    /// no seq consumed) when the TX ring has no room for it, instead of blocking.
    fn send(&mut self, msg: Msg, droppable: bool) -> bool {
        let env = Envelope { seq: self.seq, msg };
        let mut buf = [0u8; MAX_FRAME];
        let n = match encode_frame(&env, &mut buf) {
            Ok(n) => n,
            Err(e) => {
                warn!("frame encode failed: {e:?}");
                return false;
            }
        };
        // Wi-Fi first (never blocks: the socket is non-blocking), then the USB serial link.
        let mut udp_ok = false;
        if let Some(s) = &self.udp {
            match s.send(&buf[..n]) {
                Ok(_) => {
                    udp_ok = true;
                    net::UDP_SENT.fetch_add(1, Ordering::Relaxed);
                }
                Err(_) => {
                    net::UDP_DROPPED.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        if net::BCAST_ACTIVE.load(Ordering::Relaxed) {
            if self.bcast.is_none() {
                self.bcast = UdpSocket::bind("0.0.0.0:0")
                    .and_then(|s| {
                        s.set_broadcast(true)?;
                        s.set_nonblocking(true)?;
                        Ok(s)
                    })
                    .ok();
            }
            if let Some(s) = &self.bcast {
                let dest = (std::net::Ipv4Addr::BROADCAST, net::BRAIN_PORT.load(Ordering::Relaxed));
                if s.send_to(&buf[..n], dest).is_ok() {
                    udp_ok = true;
                }
            }
        }
        let mut serial_ok = true;
        if self.driver {
            if droppable {
                let mut free = 0usize;
                // SAFETY: driver installed on CONSOLE_UART; `free` is a valid out pointer.
                let rc = unsafe { sys::uart_get_tx_buffer_free_size(CONSOLE_UART, &mut free) };
                serial_ok = rc == sys::ESP_OK && free >= n;
            }
            if serial_ok {
                // SAFETY: buf[..n] is initialised; the driver copies it into its ring.
                unsafe { sys::uart_write_bytes(CONSOLE_UART, buf.as_ptr().cast(), n) };
            }
        } else {
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(&buf[..n]);
            let _ = out.flush();
        }
        if droppable && !serial_ok && !udp_ok {
            return false;
        }
        self.seq = self.seq.wrapping_add(1);
        self.sent = self.sent.wrapping_add(1);
        true
    }
}

type Tx = Arc<Mutex<FrameTx>>;

fn send(tx: &Tx, msg: Msg) {
    // A poisoned lock only means another thread panicked mid-write; keep going.
    let mut g = tx.lock().unwrap_or_else(|p| p.into_inner());
    g.send(msg, false);
}

fn try_send(tx: &Tx, msg: Msg) -> bool {
    let mut g = tx.lock().unwrap_or_else(|p| p.into_inner());
    g.send(msg, true)
}

/// Install the IDF UART driver on the console UART and route stdout/stdin through it.
fn install_console_driver() -> Result<(), sys::EspError> {
    let _ = std::io::stdout().flush();
    // SAFETY: UART0 has no driver yet (the console uses the polling VFS); the ring sizes
    // are valid; no event queue is requested.
    unsafe {
        sys::uart_wait_tx_idle_polling(CONSOLE_UART);
        sys::esp!(sys::uart_driver_install(
            CONSOLE_UART,
            CONSOLE_RX_RING,
            CONSOLE_TX_RING,
            0,
            std::ptr::null_mut(),
            0
        ))?;
        sys::uart_vfs_dev_use_driver(CONSOLE_UART as i32);
    }
    Ok(())
}

fn now_ms() -> u32 {
    (now_us() / 1000) as u32
}

/// Raw-audio stream switch, set by the cmd thread, read by the audio thread.
struct StreamCtl {
    on: AtomicBool,
    /// `now_ms()` after which the stream stops unless another StartAudio arrives.
    until_ms: AtomicU32,
}

impl StreamCtl {
    fn active(&self) -> bool {
        self.on.load(Ordering::Relaxed)
            && (self.until_ms.load(Ordering::Relaxed).wrapping_sub(now_ms()) as i32) > 0
    }
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
    stream: StreamCtl,
    /// Raw-audio stream counters since boot / since the last stats line.
    chunks_sent: AtomicU32,
    chunks_dropped: AtomicU32,
    chunk_clipped: AtomicU32,
    dec_us_sum: AtomicU32,
    dec_us_max: AtomicU32,
    dec_reads: AtomicU32,
    cmds_ok: AtomicU32,
    /// Self test requested over BLE.
    selftest_req: AtomicBool,
    /// Self test: set by main to ask the audio thread for a probe; the result lands in `probe_result`.
    probe_req: AtomicBool,
    probe_result: Mutex<Option<[ChannelStats; 2]>>,
    /// Radar parser counters, copied by the radar thread.
    radar_dropped: AtomicU32,
    radar_bad: AtomicU32,
    radar_identified: AtomicBool,
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

    let tx: Tx = Arc::new(Mutex::new(FrameTx { seq: 0, sent: 0, driver: false, udp: None, bcast: None }));
    match install_console_driver() {
        Ok(()) => tx.lock().unwrap_or_else(|p| p.into_inner()).driver = true,
        Err(e) => warn!("console: UART0 driver install failed ({e}); no host commands, no raw audio"),
    }

    let p = Peripherals::take()?;
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
        stream: StreamCtl { on: AtomicBool::new(false), until_ms: AtomicU32::new(0) },
        chunks_sent: AtomicU32::new(0),
        chunks_dropped: AtomicU32::new(0),
        chunk_clipped: AtomicU32::new(0),
        dec_us_sum: AtomicU32::new(0),
        dec_us_max: AtomicU32::new(0),
        dec_reads: AtomicU32::new(0),
        cmds_ok: AtomicU32::new(0),
        selftest_req: AtomicBool::new(false),
        probe_req: AtomicBool::new(true), // boot self test
        probe_result: Mutex::new(None),
        radar_dropped: AtomicU32::new(0),
        radar_bad: AtomicU32::new(0),
        radar_identified: AtomicBool::new(false),
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
        // 12 KB: the audio thread builds AudioChunk frames (about 2 KB) on its stack.
        thread::Builder::new().name("audio".into()).stack_size(12288).spawn(move || audio_task(i2s, tx, shared))?;
    }
    if tx.lock().unwrap_or_else(|p| p.into_inner()).driver {
        let shared = shared.clone();
        thread::Builder::new().name("cmd".into()).stack_size(4096).spawn(move || cmd_task(shared, baud))?;
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

    // --- network: a boot is either Wi-Fi (normal) or BLE provisioning, never both (RAM) ---
    // BOOT (GPIO0) held at power-up forces a provisioning boot.
    let button_override = PinDriver::input(p.pins.gpio0, Pull::Up).map(|b| b.is_low()).unwrap_or(false);
    match (esp_idf_svc::eventloop::EspSystemEventLoop::take(), esp_idf_svc::nvs::EspDefaultNvsPartition::take()) {
        (Ok(sysloop), Ok(nvs)) => {
            let cfg = net::load_cfg(&nvs);
            let flag = net::take_prov_flag(&nvs);
            match station_core::ble_policy::boot_mode(cfg.is_some(), button_override, flag) {
                station_core::ble_policy::Mode::Normal => {
                    // SAFETY: BLE is never started in this boot; returns the controller's RAM to the heap.
                    unsafe { sys::esp_bt_mem_release(sys::esp_bt_mode_t_ESP_BT_MODE_BTDM) };
                    let txp = tx.clone();
                    let started = cfg.map(|cfg| {
                        net::spawn(p.modem, sysloop, nvs, cfg, move |sock| {
                            txp.lock().unwrap_or_else(|e| e.into_inner()).udp = sock;
                        })
                    });
                    match started {
                        Some(Ok(())) => {
                            let sh = shared.clone();
                            let mut warned = false;
                            if let Err(e) = net::spawn_cmd_rx(move |f| {
                                if let Ok(env) = decode_command(f) {
                                    handle_command(&env, &sh, None, &mut warned);
                                }
                            }) {
                                warn!("net: command port failed: {e}");
                            }
                        }
                        Some(Err(e)) => warn!("net: Wi-Fi start failed: {e}; USB only"),
                        None => {}
                    }
                }
                station_core::ble_policy::Mode::Provision => {
                    info!("boot: BLE provisioning mode (Wi-Fi off); button={button_override} flag={flag}");
                    let nvs2 = nvs.clone();
                    let on_config = move |c: station_proto::provision::Config| {
                        net::apply_update(
                            &nvs2,
                            net::Update { ssid: c.ssid.as_str().into(), pass: c.pass.as_str().into(), brain: c.brain.as_str().into() },
                        );
                        // Give the GATT write response a moment to go out, then join Wi-Fi from a clean boot.
                        thread::spawn(|| {
                            thread::sleep(Duration::from_millis(1500));
                            // SAFETY: plain restart.
                            unsafe { sys::esp_restart() }
                        });
                    };
                    let sh = shared.clone();
                    let nvs3 = nvs.clone();
                    let on_cmd = move |c: station_proto::provision::Command| {
                        use station_proto::provision::Command as C;
                        info!("ble: command {c:?}");
                        match c {
                            C::SelfTest => sh.selftest_req.store(true, Ordering::Relaxed),
                            C::ClearConfig => {
                                net::clear_cfg(&nvs3);
                                // SAFETY: plain restart.
                                unsafe { sys::esp_restart() }
                            }
                            // SAFETY: plain restart.
                            C::Restart => unsafe { sys::esp_restart() },
                        }
                    };
                    if let Err(e) = ble::spawn(id, env!("CARGO_PKG_VERSION"), on_config, on_cmd) {
                        warn!("ble: provisioning service failed to start: {e}");
                    }
                    // Stored settings (button or flag boot): give the brain 10 minutes, then retry Wi-Fi.
                    let stored = cfg.is_some();
                    let _ = sysloop;
                    thread::spawn(move || loop {
                        thread::sleep(Duration::from_secs(5));
                        if station_core::ble_policy::provision_expired(stored, now_ms()) {
                            // SAFETY: plain restart.
                            unsafe { sys::esp_restart() }
                        }
                    });
                }
            }
        }
        (a, b) => warn!("net: event loop/NVS unavailable ({:?}, {:?}); USB only", a.err(), b.err()),
    }

    // --- main: self test, hello + diagnostics ---
    let mut last_selftest = String::new();
    run_selftest(&shared, &mut last_selftest, true);
    let mut next_selftest = Instant::now() + SELFTEST_EVERY;
    let mut last = now_us();
    loop {
        thread::sleep(HELLO_EVERY);
        if shared.selftest_req.swap(false, Ordering::Relaxed) || Instant::now() >= next_selftest {
            next_selftest = Instant::now() + SELFTEST_EVERY;
            shared.probe_req.store(true, Ordering::Relaxed);
            run_selftest(&shared, &mut last_selftest, false);
        }
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
        let dec_sum = shared.dec_us_sum.swap(0, Ordering::Relaxed);
        let dec_max = shared.dec_us_max.swap(0, Ordering::Relaxed);
        let dec_reads = shared.dec_reads.swap(0, Ordering::Relaxed);
        if dec_reads > 0 || shared.chunks_sent.load(Ordering::Relaxed) > 0 {
            info!(
                "stream: {} chunks_sent={} chunks_dropped={} clipped={} decim {}us avg {dec_max}us max, cpu {:.1}% cmds={}",
                if shared.stream.active() { "on" } else { "off" },
                shared.chunks_sent.load(Ordering::Relaxed),
                shared.chunks_dropped.load(Ordering::Relaxed),
                shared.chunk_clipped.load(Ordering::Relaxed),
                dec_sum.checked_div(dec_reads).unwrap_or(0),
                dec_sum as f32 * 100.0 / window_us,
                shared.cmds_ok.load(Ordering::Relaxed),
            );
        }
        info!(
            "net: {} rssi {} dBm udp_sent={} udp_dropped={} connects={}",
            if net::UP.load(Ordering::Relaxed) { "up" } else { "down" },
            net::RSSI.load(Ordering::Relaxed),
            net::UDP_SENT.load(Ordering::Relaxed),
            net::UDP_DROPPED.load(Ordering::Relaxed),
            net::CONNECTS.load(Ordering::Relaxed),
        );
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

/// Power-on self test: both mics (1 s probe of the I2S stream), the radar (frames over a window).
/// Logs one `selftest:` line per check and a `selftest: PASS|WARN|FAIL` summary. A repeat run
/// (`boot == false`) logs only when the summary differs from the previous one.
fn run_selftest(shared: &Shared, last: &mut String, boot: bool) {
    let r0 = shared.radar_frames.load(Ordering::Relaxed);
    let t0 = Instant::now();
    // Wait for the probe: 1.5 s of audio, with a timeout in case the I2S thread is stuck.
    let probe = loop {
        if let Some(p) = shared.probe_result.lock().unwrap_or_else(|e| e.into_inner()).take() {
            break Some(p);
        }
        if t0.elapsed() > Duration::from_secs(5) {
            break None;
        }
        thread::sleep(Duration::from_millis(50));
    };
    if let Some(rest) = SELFTEST_RADAR_WINDOW.checked_sub(t0.elapsed()) {
        thread::sleep(rest);
    }
    let radar = RadarStats {
        identified: shared.radar_identified.load(Ordering::Relaxed),
        frames: shared.radar_frames.load(Ordering::Relaxed).wrapping_sub(r0),
        window_ms: t0.elapsed().as_millis() as u32,
        dropped_bytes: shared.radar_dropped.load(Ordering::Relaxed),
        bad_frames: shared.radar_bad.load(Ordering::Relaxed),
    };

    let mut lines: Vec<(Level, String)> = Vec::new();
    match probe {
        Some(st) => {
            let names = ["mic A (left, SEL=GND)", "mic B (right, SEL=3V3)"];
            let vs = [selftest::mic_verdict(&st[0]), selftest::mic_verdict(&st[1])];
            for i in 0..2 {
                lines.push((
                    vs[i].level,
                    format!(
                        "{}: {} rms {:.1} dBFS peak {:.1} dBFS dc {:.1} dBFS{}{}",
                        names[i],
                        vs[i].level.as_str(),
                        st[i].ac_rms_dbfs,
                        st[i].peak_dbfs,
                        st[i].dc_dbfs,
                        if vs[i].msg.is_empty() { "" } else { " - " },
                        vs[i].msg
                    ),
                ));
            }
            if vs.iter().all(|v| v.level != Level::Fail) {
                if let Some(b) = selftest::balance_verdict(&st[0], &st[1]) {
                    lines.push((b.level, format!("balance: {} - {}", b.level.as_str(), b.msg)));
                }
            }
        }
        None => lines.push((Level::Fail, "i2s: no audio probe result (I2S thread stuck?)".to_string())),
    }
    let errs = shared.audio_errors.load(Ordering::Relaxed);
    if errs > 0 {
        lines.push((Level::Warn, format!("i2s: {errs} read errors since boot")));
    }
    let rv = selftest::radar_verdict(&radar);
    lines.push((
        rv.level,
        format!(
            "radar: {} {:.1} frames/s, version read {}, dropped bytes {}{}{}",
            rv.level.as_str(),
            radar.frames as f32 * 1000.0 / radar.window_ms.max(1) as f32,
            if radar.identified { "ok" } else { "no" },
            radar.dropped_bytes,
            if rv.msg.is_empty() { "" } else { " - " },
            rv.msg
        ),
    ));
    let heap = unsafe { sys::esp_get_free_heap_size() };
    if heap < 40_000 {
        lines.push((Level::Warn, format!("heap: only {heap} bytes free")));
    }

    let worst = selftest::worst(&lines.iter().map(|l| l.0).collect::<Vec<_>>());
    let summary = match worst {
        Level::Ok => "PASS",
        Level::Warn => "WARN",
        Level::Fail => "FAIL",
    };
    ble::SELFTEST.store(worst as u8, Ordering::Relaxed);
    let sig: String = lines.iter().map(|l| l.1.split_once(" rms").map_or(l.1.as_str(), |x| x.0).to_string() + &format!("{:?}", l.0)).collect();
    if !boot && sig == *last {
        return;
    }
    *last = sig;
    for (lvl, text) in &lines {
        if *lvl == Level::Ok {
            info!("selftest: {text}");
        } else {
            warn!("selftest: {text}");
        }
    }
    if worst == Level::Ok {
        info!("selftest: {summary}");
    } else {
        warn!("selftest: {summary}");
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
    let mut raw = RawStream::new();
    let mut probe: Option<(MicProbe, u32)> = None;
    loop {
        match i2s.read(&mut buf, timeout) {
            Ok(n) => {
                if shared.probe_req.swap(false, Ordering::Relaxed) {
                    probe = Some((MicProbe::new(), 0));
                }
                if let Some((p, reads)) = probe.as_mut() {
                    *reads += 1;
                    if *reads > PROBE_SKIP_READS {
                        p.push_bytes(&buf[..n]);
                    }
                    if *reads >= PROBE_SKIP_READS + PROBE_READS {
                        *shared.probe_result.lock().unwrap_or_else(|e| e.into_inner()) = Some(p.stats());
                        probe = None;
                    }
                }
                shared.audio_reads.fetch_add(1, Ordering::Relaxed);
                shared.audio_frames.fetch_add((n / 8) as u32, Ordering::Relaxed);
                // Raw stream first: it is the most timing-sensitive consumer of the TX ring.
                raw.push(&buf[..n], &tx, &shared);
                let quiet = raw.on;
                meter.push_bytes(&buf[..n], |lvl| {
                    // The 10 Hz level line is muted while streaming to keep text off the link.
                    if !quiet {
                        info!(
                            "audio: L {:7.1} dBFS  R {:7.1} dBFS  peak {:7.1} dBFS",
                            lvl.rms_dbfs_l, lvl.rms_dbfs_r, lvl.peak_dbfs
                        );
                    }
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

/// Raw 16 kHz stream state, owned by the audio thread.
struct RawStream {
    on: bool,
    dec: Option<Decimator>,
    /// Output frames since the stream started (index of the next frame).
    frame: u32,
    chunk: heapless::Vec<u8, AUDIO_CHUNK_BYTES>,
    chunk_frame: u32,
    pcm: [i16; Decimator::out_len(I2S_READ_FRAMES)],
}

impl RawStream {
    fn new() -> Self {
        Self {
            on: false,
            dec: None,
            frame: 0,
            chunk: heapless::Vec::new(),
            chunk_frame: 0,
            pcm: [0; Decimator::out_len(I2S_READ_FRAMES)],
        }
    }

    fn push(&mut self, bytes: &[u8], tx: &Tx, shared: &Shared) {
        let want = shared.stream.active();
        if want != self.on {
            self.on = want;
            if want {
                // Allocated on first use (about 3 KB), kept afterwards.
                let dec = self.dec.get_or_insert_with(Decimator::new);
                dec.reset();
                self.frame = 0;
                self.chunk.clear();
                self.chunk_frame = 0;
                info!("stream: raw audio on, {} Hz stereo i16, {} taps", decimate::OUTPUT_RATE_HZ, decimate::TAPS);
            } else {
                info!(
                    "stream: raw audio off after {} frames, chunks sent {} dropped {}",
                    self.frame,
                    shared.chunks_sent.load(Ordering::Relaxed),
                    shared.chunks_dropped.load(Ordering::Relaxed)
                );
            }
        }
        if !self.on {
            return;
        }
        let Some(dec) = self.dec.as_mut() else { return };
        let t0 = now_us();
        let k = dec.push_bytes(bytes, &mut self.pcm);
        let dt = (now_us() - t0).clamp(0, u32::MAX as i64) as u32;
        shared.dec_us_sum.fetch_add(dt, Ordering::Relaxed);
        shared.dec_reads.fetch_add(1, Ordering::Relaxed);
        let _ = shared.dec_us_max.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |m| (dt > m).then_some(dt));
        for lr in self.pcm[..k].chunks_exact(2) {
            let _ = self.chunk.extend_from_slice(&lr[0].to_le_bytes());
            let _ = self.chunk.extend_from_slice(&lr[1].to_le_bytes());
            self.frame = self.frame.wrapping_add(1);
            if self.chunk.is_full() {
                // A 480-frame read yields exactly one 160-frame chunk, so the clip count
                // taken here belongs to this chunk.
                let clipped = dec.take_clipped();
                shared.chunk_clipped.fetch_add(clipped, Ordering::Relaxed);
                let msg = Msg::AudioChunk(AudioChunk {
                    rate_hz: decimate::OUTPUT_RATE_HZ as u16,
                    channels: 2,
                    frame: self.chunk_frame,
                    clipped: clipped.min(u16::MAX as u32) as u16,
                    samples: core::mem::take(&mut self.chunk),
                });
                let ctr = if try_send(tx, msg) { &shared.chunks_sent } else { &shared.chunks_dropped };
                ctr.fetch_add(1, Ordering::Relaxed);
                self.chunk_frame = self.frame;
            }
        }
    }
}

/// Apply one decoded host command. `via_uart` commands are checked against the console baud
/// (raw audio does not fit 115200); UDP commands are not.
fn handle_command(env: &station_proto::CommandEnvelope, shared: &Shared, uart_baud: Option<u32>, warned_baud: &mut bool) {
    shared.cmds_ok.fetch_add(1, Ordering::Relaxed);
    match env.cmd {
        Command::StartAudio if uart_baud.is_some_and(|b| b < MIN_STREAM_BAUD) => {
            if !*warned_baud {
                *warned_baud = true;
                warn!(
                    "stream: StartAudio ignored, console is {} baud; build with STATION_BAUD=921600",
                    uart_baud.unwrap_or(0)
                );
            }
        }
        Command::StartAudio => {
            let st = &shared.stream;
            st.until_ms.store(now_ms().wrapping_add(AUDIO_KEEPALIVE_MS), Ordering::Relaxed);
            if !st.on.swap(true, Ordering::Relaxed) {
                info!("stream: StartAudio (cmd seq {})", env.seq);
            }
        }
        Command::StopAudio => {
            if shared.stream.on.swap(false, Ordering::Relaxed) {
                info!("stream: StopAudio (cmd seq {})", env.seq);
            }
        }
    }
}

/// Host commands on UART0 RX: `0x00 COBS(CommandEnvelope ++ crc) 0x00`. Anything else
/// (stray keystrokes from a terminal) is ignored.
fn cmd_task(shared: Arc<Shared>, baud: u32) {
    let mut frame: heapless::Vec<u8, 64> = heapless::Vec::new();
    let mut overflow = false;
    let mut buf = [0u8; 64];
    let mut warned_baud = false;
    loop {
        // SAFETY: driver installed on CONSOLE_UART; buf is valid for its length.
        let n = unsafe {
            sys::uart_read_bytes(CONSOLE_UART, buf.as_mut_ptr().cast(), buf.len() as u32, TickType::new_millis(100).ticks())
        };
        if n <= 0 {
            continue;
        }
        for &b in &buf[..n as usize] {
            if b != 0 {
                overflow |= frame.push(b).is_err();
                continue;
            }
            if frame.is_empty() {
                continue;
            }
            let ok = !core::mem::take(&mut overflow);
            let cmd = if ok { decode_command(&frame).ok() } else { None };
            frame.clear();
            let Some(env) = cmd else { continue };
            handle_command(&env, &shared, Some(baud), &mut warned_baud);
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
                shared.radar_identified.store(true, Ordering::Relaxed);
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
        shared.radar_dropped.store(parser.stats.dropped_bytes, Ordering::Relaxed);
        shared.radar_bad.store(parser.stats.bad_frames, Ordering::Relaxed);
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
