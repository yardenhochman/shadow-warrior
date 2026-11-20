# Sensor station firmware (stage 1: bring-up)

ESP32 firmware for a Shadow Warrior sensor station: two ICS-43434 I2S MEMS
microphones and an HLK-LD2410 24 GHz presence radar. Rust `std` on ESP-IDF
(`esp-idf-svc` / `esp-idf-hal`). Tracking issue: `bd show shadow-warrior-ll3`.

Stage 1 is a bring-up binary with no Wi-Fi. It writes, on the USB serial port (UART0):

- human-readable log lines: L/R RMS dBFS and peak every 100 ms, one line per radar
  report (about 10 Hz), stats every 5 s;
- `station-proto` binary frames for the same data (`Hello`, `RadarEvent`, `AudioLevel`),
  which `tools/station_monitor.py` decodes.

## Layout

| Path | What | Builds for |
|---|---|---|
| `station-proto/` | `no_std` wire types + framing (postcard, CRC-32, COBS). Will be shared with brain-rs. | host + ESP |
| `station-core/` | `no_std` pure logic: LD2410 frame parser and command builders, RMS/peak level meter. | host + ESP |
| `firmware/` | ESP32 binary `station-fw` (own Cargo project: `esp` toolchain, `build-std`, ESP-IDF v5.5.3). | ESP only |
| `tools/station_monitor.py` | Serial reader / frame decoder (uv script, PEP 723). | host |

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

No MCLK: the ICS-43434 does not use one. Mic spacing for later direction
finding (GCC-PHAT): 8-10 cm.

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
CRC-32 is ISO-HDLC (`zlib.crc32`). `Msg` is `Hello | Radar | Audio`. Variant and field order
are part of the format: only append. `PROTO_VERSION` (now 1) goes in `Hello`, which is sent at
boot, every 5 s and when the radar firmware is identified.

On UART0 the frames share the line with log text. A reader splits on `0x00`, keeps every chunk
that decodes and passes the CRC, and treats the rest as text (`StreamSplitter` in Rust,
`station_monitor.py` in Python). `sdkconfig.defaults` sets LF stdout line endings so the
console does not turn `0x0A` into `0x0D 0x0A` inside frames.

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
just test                        # host unit tests + python decoder cross-check
just build                       # cargo build --release for xtensa-esp32-espidf
just flash                       # espflash flash (auto-detects the port)
STATION_PORT=/dev/cu.usbserial-0001 just flash
just monitor                     # espflash monitor (text; frames appear as noise)
just decode                      # decoded frames + log text
just decode --json --only audio  # JSON lines
just decode --csv levels.csv --quiet-text
STATION_BAUD=921600 just flash decode   # faster console after boot (bootloader stays 115200)
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
