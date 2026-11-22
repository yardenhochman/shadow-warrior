# Sensor station firmware (stage 1: bring-up)

ESP32 firmware for a Shadow Warrior sensor station: two ICS-43434 I2S MEMS
microphones and an HLK-LD2410 24 GHz presence radar. Rust `std` on ESP-IDF
(`esp-idf-svc` / `esp-idf-hal`). Tracking issue: `bd show shadow-warrior-ll3`.

Stage 1 is a bring-up binary with no Wi-Fi. It writes, on the USB serial port (UART0):

- human-readable log lines: L/R RMS dBFS and peak every 100 ms, one line per radar
  report (about 10 Hz), stats every 5 s;
- `station-proto` binary frames for the same data (`Hello`, `RadarEvent`, `AudioLevel`,
  and `Direction` from GCC-PHAT, see below), which `tools/station_monitor.py` decodes;
- on request from the host only, raw 16 kHz stereo audio (`AudioChunk`, see
  [Raw audio recording](#raw-audio-recording)).

## Layout

| Path | What | Builds for |
|---|---|---|
| `station-proto/` | `no_std` wire types + framing (postcard, CRC-32, COBS). Will be shared with brain-rs. | host + ESP |
| `station-core/` | `no_std` + `alloc` pure logic: LD2410 frame parser and command builders, RMS/peak level meter, f32 FFT and GCC-PHAT direction finding, 48 -> 16 kHz FIR decimator. | host + ESP |
| `firmware/` | ESP32 binary `station-fw` (own Cargo project: `esp` toolchain, `build-std`, ESP-IDF v5.5.3). | ESP only |
| `tools/station_monitor.py` | Serial reader / frame decoder (uv script, PEP 723). | host |
| `tools/doa_view.py` | Direction view: one line per sound event, angle histogram, PNG plot, broadside calibration. | host |
| `tools/station_record.py` | Raw audio recorder: WAV + JSON sidecar in `brain-rs/clips/station/`. | host |

The top-level `Cargo.toml` is a host workspace with the two `no_std` crates, so
`cargo test` runs on the Mac. `firmware/` is excluded from it because its
`.cargo/config.toml` pins the Xtensa target and `build-std`.

## Wiring (ESP32-WROOM-32 dev board)

| Signal | ESP32 GPIO | Peripheral | Notes |
|---|---|---|---|
| I2S BCLK (SCK) | 26 | both mics SCK | |
| I2S WS (LRCL) | 25 | both mics WS | |
| I2S DIN (SD) | 33 | both mics SD (shared line) | each mic tri-states its SD outside its slot. Add a 100 kΩ pull-down on SD |
| Mic L select | GND | mic 1 `LR` / `SELECT` | left slot (WS low) |
| Mic R select | 3V3 | mic 2 `LR` / `SELECT` | right slot (WS high) |
| Mic VDD / GND | 3V3 / GND | both mics | ICS-43434 is 1.65-3.63 V |
| Radar TX → ESP RX | 16 | LD2410 `TX` | UART2 RX |
| Radar RX ← ESP TX | 17 | LD2410 `RX` | UART2 TX |
| Radar OUT | 27 | LD2410 `OUT` (optional) | presence pin, read as input with pull-down, shown in logs |
| Radar VCC / GND | 5V (VIN) / GND | LD2410 | 5 V supply, 3.3 V UART logic (HLK manual); peak draw is about 100 mA+ |

No MCLK: the ICS-43434 does not use one. Mic spacing on the bench station: **13 cm**
between the sound ports (the direction finder default; see `STATION_MIC_MM`).

## ICS-43434 notes

- The sample rate must be **23-51 kHz** (datasheet clock range). 16 kHz is not possible, so the
  station runs at **48 kHz** and any downsampling happens later in software.
- Standard Philips I2S, 64 BCLK per frame. The firmware uses **32-bit slots, stereo**. The mic
  sends 24-bit two's-complement data MSB-aligned in the slot, so `sample = word >> 8`
  (`station_core::level::slot_to_sample`). dBFS are relative to 24-bit full scale.
- A mic needs about 2^18 SCK cycles after power-up before its output is valid, so the first few
  ms are garbage.
- Floating or missing mics: DIN reads as noise, all zeros or all ones. The level meter reports a
  low level and nothing crashes (checked on hardware with no mics attached, see below).
- Not checked on hardware yet: on the classic ESP32 some IDF versions swap L/R for some slot
  widths. With mics attached, tap one mic and check which column moves.

## LD2410 notes

- Protocol from the HLK serial protocol doc, cross-checked against ESPHome's `ld2410` /
  `ld24xx` components (dev branch, 2026-11-20). The byte layout is in the header of
  `station-core/src/ld2410.rs`.
- Default 256000 baud 8N1. Data frames `F4 F3 F2 F1 … F8 F7 F6 F5`, command/ACK frames
  `FD FC FB FA … 04 03 02 01`, length u16 LE.
- At boot the firmware sends enable-config (`0xFF`, value `0x0001`), read firmware version
  (`0xA0`) and end-config (`0xFE`), and logs the version in ESPHome's format, e.g.
  `2.04.23022511`. If nothing answers it logs a timeout, retries every 10 s and keeps
  running. A missing radar never stops the audio side.
- Uncertain:
  - the modules on hand are an **LD2410C-P** and an **LD2410D-B**. The parser follows the
    LD2410/B/C protocol (ESPHome treats B and C the same), so the C-P should work as is. The D-B
    variant has not been confirmed to use the same frames. If it does not (the LD2410S, for
    example, has its own format), the parser needs a second mode. Check against each real
    module.
  - the meaning of the firmware-type word in the `0xA0` ACK is not documented. It is logged
    raw.
  - target states 4-6 (background correction, firmware 2.44+) are passed through in
    `RadarEvent.state` but do not count as presence.

## Wire protocol (`station-proto`)

A frame is `0x00 | COBS( postcard(Envelope{seq: u16, msg: Msg}) ++ crc32_le ) | 0x00`.
CRC-32 is ISO-HDLC (`zlib.crc32`). `Msg` is `Hello | Radar | Audio | Direction | AudioChunk`.
Variant and field order are part of the format: only append. `PROTO_VERSION` (now 1) goes in
`Hello`, which is sent at boot, every 5 s and when the radar firmware is identified.
`Msg::Direction` (variant 3) and `Msg::AudioChunk` (variant 4) were appended without a version
bump: the other variants encode exactly as before, and an older reader just fails to decode
the new frames and skips them. `MAX_PAYLOAD` is 672 bytes (a full `AudioChunk` is 658), so a
frame is at most 681 bytes on the wire.

The host can send commands the other way, on the same UART: `0x00 | COBS(
postcard(CommandEnvelope{seq: u16, cmd: Command}) ++ crc32_le ) | 0x00`, with `Command` =
`StartAudio | StopAudio` (append-only too). The station ignores anything else on its RX, so
typing into a serial terminal is harmless. For this the firmware installs the IDF UART driver
on UART0 at boot (16 KB TX ring, 1 KB RX ring) and routes the console through it. Log text
and frames then go into the TX ring instead of busy-waiting on the FIFO. Each frame is one
`uart_write_bytes` call, which holds the driver's TX lock, so log text can only land between
frames.

On UART0 the frames share the line with log text. A reader splits on `0x00`, keeps every chunk
that decodes and passes the CRC, and treats the rest as text (`StreamSplitter` in Rust,
`station_monitor.py` in Python). `sdkconfig.defaults` sets LF stdout line endings so the
console does not turn `0x0A` into `0x0D 0x0A` inside frames.

## Direction finding (GCC-PHAT)

The audio thread cuts the stereo stream into non-overlapping frames of 1024 samples
(21.3 ms, 46.9 frames/s) and, for each frame (`station-core/src/doa.rs`):

1. removes each channel's DC offset and computes the frame RMS over both channels;
2. **level gate**: below `STATION_DOA_GATE_DBFS` (default **-50 dBFS**, the idle floor is about
   -68) nothing else happens and nothing is sent. Silence and room noise give no direction;
3. GCC-PHAT: zero-pad to 2048, one complex FFT of `L + jR`, cross spectrum with PHAT weighting
   in the 150 Hz - 10 kHz band, inverse FFT;
4. peak search in +-20 samples only (max physical lag = d fs / c = 0.13 * 48000 / 343 = 18.2
   samples, plus the bias, plus 1), refined with a parabola through the peak and its neighbours;
5. sends one `Direction` frame: `frame` index, `lag_samples` (bias-corrected), `angle_deg =
   asin(lag c / (fs d))` clamped to +-90, `confidence` = peak / median |correlation|,
   `rms_dbfs`. Every gated frame is sent raw, with no smoothing; the text log shows at most
   10 `doa:` lines per second.

**Sign convention.** 0 deg is broadside (straight out, perpendicular to the line between the
mics). **Positive angle = toward the R mic (SEL = 3V3)**, negative = toward the L mic (SEL =
GND). Positive lag means the sound reached R first.

**Front/back ambiguity.** Two mics only measure the arrival time difference, so a source at
+30 deg in front and its mirror image behind the board (150 deg) give the same reading. The
output is a half-plane angle. A third mic or the radar would resolve it.

**Resolution.** One sample is 3.1 deg near broadside. Near the axis (+-60..90 deg) `asin` gets
steep, so small lag errors become large angle errors, and +-90 is reached at 18.2 samples.
Sub-sample interpolation in the host tests is better than 0.03 samples on clean broadband noise.

**Confidence.** Two uncorrelated noise channels score a median of 4 and at most 8.5 (200 host-test
frames). A single broadband source is above 100 without noise, and well above 10 at 0 dB SNR.
`station_core::doa::MIN_CONFIDENCE = 10` is the suggested consumer threshold. The station
does not filter on it.

**Spatial aliasing.** Above c / 2d = 1.3 kHz the phase difference wraps. Broadband sounds
(claps, clicks, shouts) still give one clear peak. A pure tone or whistle can give a wrong
lag with a high confidence.

**Build-time settings** (baked in, like `STATION_BAUD`; cargo rebuilds when they change):

| Env | Default | Meaning |
|---|---|---|
| `STATION_MIC_MM` | 130 | mic port spacing, mm |
| `STATION_DOA_BIAS` | 0 | raw lag at broadside, samples; subtracted from every frame (see calibration below) |
| `STATION_DOA_GATE_DBFS` | -50 | level gate; `-200` analyses every frame (for CPU measurement) |

The boot log prints the active values (`doa: GCC-PHAT 1024 samples/frame, d=130 mm, ...`). The
5 s stats line adds `i2s_rate` (stereo frames read per second, expect 48000: lower means
dropped DMA buffers), `doa A/B frames` (analysed / total), the average and maximum time of an
I2S read that ran GCC-PHAT, and `cpu %` (GCC-PHAT time / wall time, one core).

### Measured on the ESP32-WROOM (240 MHz, 2026-11-21)

| What | Result |
|---|---|
| `GccPhat::estimate` alone (on-device micro-benchmark) | 4.7 ms per frame (two 2048-point FFTs at 2.37 ms each are most of it, median 0.5 ms) |
| Worst case, every frame analysed (`STATION_DOA_GATE_DBFS=-200`) | 5.5 ms avg / 6.1 ms max per I2S read with a frame, **26 % of one core**, 47 `Direction` frames/s |
| I2S at the same time | `i2s_rate` 48000/s, `i2s_errors=0`, no `seq` gaps over 30 s (radar running too) |
| Default gate -50 dBFS, quiet room | 0 frames analysed (CPU 0 %), apart from frame 1 at boot (mic power-up transient) |
| Heap | about 50 KB for GCC-PHAT, 159 KB free afterwards, steady |

**Idle correlation at lag 0.** With the gate off, room-noise frames (-77..-46 dBFS) have a
median confidence of 9.5, and 45 % score >= 10, all at lag 0 (max 505). Something common to
both channels (supply or ground noise, or clock crosstalk) is correlated with no delay. The
gate hides it at idle, but a quiet sound just above the gate can be pulled toward 0 deg. This
is tracked in bd.

### Bench test: angle accuracy

You need a protractor (or a printed angle sheet), something that makes sharp broadband sounds
(hand claps, two pieces of wood, a clicker, or a phone playing a click track), and a quiet
room. Keep the station at least 1 m from walls if possible, because reflections pull the peak.

1. Put the board flat with both mics at the same height. Mark the midpoint between the two mic
   ports, and draw the mic axis (L to R) and the broadside line (perpendicular, 0 deg)
   through it. Mark -90 (L side, along the axis), -45, 0, +45 and +90 (R side) on a 1.5 m
   radius. Use the same side of the board (the "front") for every position.
2. Start a recording (this resets the board; wait for the `doa:` boot line):
   ```sh
   just doa --save cal0.bin            # = uv run tools/doa_view.py --save cal0.bin
   ```
3. **Broadside calibration first.** Stand at 0 deg, 1-2 m away, at about mic height. Clap 10
   times, about 1 s apart. Ctrl-C. The summary prints the median lag. Then:
   ```sh
   uv run tools/doa_view.py --file cal0.bin --calibrate
   STATION_DOA_BIAS=<printed value> STATION_PORT=/dev/cu.usbserial-0001 just flash
   ```
   Skip the reflash if the bias is within +-0.3 samples (about 1 deg). A larger bias comes from
   the mics not being exactly where you measured, unequal port tubes, or the mics' own
   group delay.
4. **Angle sweep.** `just doa --save sweep.bin`. Clap 10 times at each of -90, -45, 0, +45,
   +90 deg, in that order, with a pause of a few seconds between positions. Each clap prints
   one line (`t=  12.34s  L [----#----|---------] R  -44.8 deg ...`). Ctrl-C prints the
   histogram. Then make the plot:
   ```sh
   uv run tools/doa_view.py --file sweep.bin --png sweep.png
   ```
5. What to look for:
   - each position gives a tight cluster (spread < 5 deg) at 0 and +-45. At +-90 expect
     readings of 75-90, because the endfire region is compressed;
   - the sign is right: R side positive. If it is reversed, the mics or SEL pins are swapped;
   - confidence is well above 10 for claps, and frames between claps produce no lines (gate);
   - optional: repeat one position behind the board to see the front/back mirror, and at 3 m
     to see how far the gate reaches (raise or lower `STATION_DOA_GATE_DBFS`).
6. Put `sweep.bin`, `sweep.png` and the histogram output on bead `shadow-warrior-66b`.

`just decode --only direction --json` shows the raw per-frame values, and
`just decode --only direction --csv doa.csv --quiet-text` writes them to a CSV.

### Xtensa compiler workarounds

The esp toolchain (rustc 1.90.0.0) LLVM backend crashed on three constructs in this code. The
workarounds are commented in place:

- loops with a constant trip count of 1024 over zipped slices (`Cannot select: i32 =
  Constant<-8192>` / `<-4096>`, in `doa::load_input` and `doa::median_abs_corr`): use loop
  bounds taken from runtime slice lengths;
- `AtomicU32::fetch_max` (`Undefined temporary symbol`, firmware stats): use `fetch_update`
  instead.

If a build fails with one of these messages after a change, look for the same patterns.

## Raw audio recording

Records the two ICS-43434 mics to WAV on the Mac, to judge whether they pick up shouts well
enough or whether the arena needs the USB cardioid mic (beads `shadow-warrior-wnu`, `-c6y`).
The brain's offline evaluator reads the files from `brain-rs/clips/station/`.

```sh
just flash-fast                      # = STATION_BAUD=921600 just flash; needed once
just record shout                    # -> ../brain-rs/clips/station/shout_1.wav (+ shout_1.json); Enter stops
just record shout --mono --split     # also shout_1_mono.wav ((L+R)/2), shout_1_L.wav, shout_1_R.wav
just record idle -t 30               # stop after 30 s
STATION_PORT=/dev/cu.usbserial-0001 just record clap
```

Opening the port resets this dev board (CP2102 on macOS), so a recording starts about 1 s after
boot. The tool waits for the first chunk.

**What is in the file.** 16 kHz, 16-bit, stereo, channels L (mic SEL = GND) and R (SEL = 3V3).
Processing, all in `station-core/src/decimate.rs`: 24-bit I2S samples at 48 kHz -> linear-phase
FIR low-pass (191 taps, Kaiser beta 6.2, -6 dB at 7.5 kHz, flat within +-0.01 dB to 7 kHz, >=
64 dB down from 8 kHz) -> keep every third sample -> scale by 2^-8 and round to nearest
(saturating) i16. No AGC, no noise suppression, no DC removal, no other filtering. dBFS keep
their meaning (16-bit full scale = 24-bit full scale). The filter delays the audio by 2 ms.

**Protocol.** The recorder sends `StartAudio` and repeats it every second. The station stops
streaming 5 s after the last one (`AUDIO_KEEPALIVE_MS`), so a recorder that dies does not leave
it streaming. `StopAudio` stops at once. Streaming is off at boot, and DOA, levels and radar
run as usual while it is on. The 10 Hz `audio:` text line is muted while streaming. Each 10 ms
I2S read yields one `AudioChunk` of 160 stereo frames (640 sample bytes, about 66 KB/s with
framing; 921600 baud carries 92 KB/s). `AudioChunk.frame` is the index of its first frame
since the stream started. When the TX ring has no room for a chunk, the station drops the
chunk (counted) instead of blocking the audio thread, and the recorder fills the gap with
silence so the WAV stays in real time. A build with the default 115200 baud refuses
`StartAudio` with a warning in the log.

**Recorder output.** Duration vs wall time, chunks, dropped chunks (from `frame` gaps), seq gaps
and damaged frames (counted from the first chunk on, so the boot ROM noise is ignored),
frame counts per type, and per channel: peak, clipped samples, noise floor (10th percentile of
the 100 ms AC RMS over the clip), median and loudest 100 ms, DC. The `.json` sidecar holds the
same numbers plus station id, firmware version, radar firmware, port, baud and the processing
chain. WAV and JSON files in `brain-rs/clips/station/` are git-ignored.

**Station stats while streaming.** An extra line every 5 s: `stream: on chunks_sent=...
chunks_dropped=... clipped=... decim <avg>us avg <max>us max, cpu <%> cmds=<n>`.

### Measured on the ESP32-WROOM (2026-11-22, breadboard mics, radar not powered)

| What | Result |
|---|---|
| 30 s idle recording at 921600 | 3015 chunks, 0 dropped, 0 seq gaps, 0 damaged frames; WAV 30.15 s vs 30.13 s wall (+0.06 %) |
| Idle level (quiet room) | noise floor -72 / -73 dBFS (L / R), median 100 ms -69 / -70 dBFS, DC 0 |
| 1 kHz tone from the Mac's speakers (far away) | line at 999.7 Hz, -85 / -81 dBFS, about 33 dB over the local floor; -106 / -105 dBFS outside the tone |
| Decimator cost | 1.68 ms per 10 ms read (16.8 % of one core) |
| Worst case: streaming + GCC-PHAT on every frame (`STATION_DOA_GATE_DBFS=-200`) | i2s_errors 0, i2s_rate 48000/s, 0 dropped chunks over 30 s, CPU 26 % (DOA) + 17 % (decimator), heap 127.7 KB free and steady |

Not checked yet: a close tone, a clap and shouts (needs a person at the bench), and
streaming with the radar running.

## Toolchain setup (macOS, once)

```sh
just setup            # installs espup, ldproxy, espflash@4.5.0 if missing, then `espup install`
# or by hand:
cargo install espup ldproxy --locked
cargo install espflash@4.5.0 --locked   # 4.7 needs rustc >= 1.95
espup install         # Xtensa Rust fork ("esp" toolchain) + LLVM, writes ~/export-esp.sh
```

The first firmware build downloads ESP-IDF v5.5.3 and its tools into `~/.espressif`
(`ESP_IDF_TOOLS_INSTALL_DIR = "global"`), which takes several minutes. Later builds are
incremental.

## Build, flash, monitor

```sh
just test                        # host unit tests + python decoder cross-check (exact JSON diff)
just build                       # cargo build --release for xtensa-esp32-espidf
just flash                       # espflash flash (auto-detects the port)
STATION_PORT=/dev/cu.usbserial-0001 just flash
just monitor                     # espflash monitor (text; frames appear as noise)
just decode                      # decoded frames + log text
just decode --json --only audio  # JSON lines
just decode --csv levels.csv --quiet-text
just doa                         # direction per sound event (tools/doa_view.py)
STATION_BAUD=921600 just flash decode   # faster console after boot (bootloader stays 115200)
just flash-fast && just record test     # raw audio to WAV (see "Raw audio recording")
```

`firmware/` also works with plain cargo after `source ~/export-esp.sh`:
`cd firmware && cargo run --release` (the runner is `espflash flash --monitor`).

## ESP32-S3 fallback

Same code. Change:

1. `firmware/.cargo/config.toml`: `target = "xtensa-esp32s3-espidf"`, `MCU = "esp32s3"`.
2. `justfile`: the `target/xtensa-esp32-espidf/...` path in `flash`.
3. Pins: GPIO 26 does not exist on the S3 and GPIO 33-37 are taken by octal PSRAM/flash on many
   modules. Suggested pins: BCLK 4, WS 5, DIN 6, radar UART1 TX 17 / RX 18, OUT 8. The pin
   numbers are the `p.pins.gpioNN` arguments in `firmware/src/main.rs`.
4. The S3 has native USB (USB-Serial-JTAG). Either keep using the UART bridge port on the dev
   board, or set `CONFIG_ESP_CONSOLE_USB_SERIAL_JTAG=y` (then the console baud setting does not
   apply).

## Next stage (Wi-Fi), notes

- Turn Wi-Fi power save off (`esp_wifi_set_ps(WIFI_PS_NONE)`). The default modem sleep adds
  100+ ms of latency and jitter to UDP, and this matters for shout and level timing.
- Transport: the same `station-proto` frames, one per UDP datagram to the brain. The USB serial
  path stays for development and recording.
- Planned work is tracked in bd (`bd list | rg -i station`), not here.

## Hardware verification status

See the bd issue comments on `shadow-warrior-ll3` for the latest. As of stage 1: build, flash
and boot have been checked on a bare ESP32-WROOM dev board with nothing attached. There, the
missing radar logs a timeout and the station keeps running, and floating I2S input produces
levels without crashing. **Mic levels and LD2410 parsing on real sensors have NOT been
verified**: they are covered only by host unit tests.

Direction finding (bead `shadow-warrior-66b`): runs on the WROOM with both mics connected, with
no I2S drops and acceptable CPU (table above). **Angle accuracy has NOT been checked with real
sound sources yet**: GCC-PHAT correctness is covered by host tests with synthetic delays only.
Run the bench test above.
