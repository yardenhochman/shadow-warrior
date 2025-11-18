This is a Rust embedded LED controller for ESP32.

## Hardware

- **Microcontroller**: ESP32 WROOM-32D chip
- **LED Strip**: WS2812B addressable RGB LEDs (180 LEDs) connected to GPIO pin 26
- **Interfaces**: BLE and Web server
- **Flash Size**: 4MB

## LED Effect Modes

- **Idle**: Clears all LEDs (off state)
- **Energy Bar**: Displays an animated bar showing energy level 0-100%
  - Gradient from white (start) to red (end)
  - Smooth animation with quadratic easing
- **Breathing**: Gentle breathing effect with configurable brightness
  - 5-second cycle by default
  - Red color breathing at 60 FPS
  - Runs continuously until stopped by sending another command (e.g., `idle`)
- **Energy Pulse**: Sends a red pulse down the entire strip

All effects run at 60 FPS for smooth animations.

## BLE Protocol

The device advertises as **"ShadowLED"** and implements a custom GATT service:

### Service UUID
`d08d81bb-7270-45de-a475-5b52feb820b6`

### Characteristics

1. **RX Characteristic** (Write): `8f97424f-8c2f-4a86-9e53-92059ccb1559`
   - Used to send commands to the device
   - Accepts text-based commands

2. **TX Characteristic** (Read/Notify): `6E400003-B5A3-F393-E0A9-E50E24DCCA9E`
   - Used for notifications from device (future use)

3. **IP Address Characteristic** (Read): `00000001-0000-1000-8000-00805f9b34fb`
   - Returns the device's WiFi IP address

### BLE Commands

Send text commands via the RX characteristic:

- `energy_bar <0-100>` - Display energy bar at specified percentage
  - Example: `energy_bar 75`
- `energy_pulse` - Send a pulse down the strip
- `breath` or `breathing` - Start breathing effect (runs until stopped)
- `idle` - Turn off all LEDs and stop any running effect

## Building and Deployment

### Prerequisites

- Rust with ESP32 toolchain (`esp` toolchain)
- `cargo-espflash` tool for flashing
- ESP-IDF v5.3.3 (automatically managed by build system)

### Build Configuration

The project uses a custom partition table to accommodate the larger binary size and provide storage:

- **NVS**: 24KB (0x6000) for WiFi credentials and system settings
- **PHY Init**: 4KB (0x1000) for WiFi/BLE calibration
- **Factory App**: 2MB (0x200000) for firmware
- **Storage (SPIFFS)**: ~2MB (0x1F0000) for configuration files and data

The partition file `partitions.csv` must be present in the project root and will be automatically copied to the build directory during compilation.

### Building

```bash
# Standard release build
cargo build --release

# After a clean build, copy partition table first
./copy_partitions.sh && cargo build --release
```

**Note**: After `cargo clean`, you must run `./copy_partitions.sh` before building to copy the custom partition table to the ESP-IDF build directory.

### Flashing to ESP32

```bash
# Flash and monitor serial output
cargo espflash flash --release --monitor

# Flash without monitoring
cargo espflash flash --release

# Save firmware image to file
cargo espflash save-image --release --chip esp32 shadow-led.bin
```

### Serial Monitor Only

```bash
# Monitor serial output (115200 baud)
cargo espflash monitor
```

## Configuration

### WiFi Settings

Edit `src/main.rs` to set your WiFi credentials:

```rust
const WIFI_SSID: &str = "your_ssid";
const WIFI_PASSWORD: &str = "your_password";
```

### LED Strip

To change the LED count, modify `src/main.rs`:

```rust
const LED_COUNT: usize = 180; // Change to your strip's LED count
```

### Device Name

To change the BLE advertising name, modify `src/main.rs`:

```rust
const DEVICE_NAME: &str = "ShadowLED"; // Change to your preferred name
```

## Testing BLE Interface

Use the included Python tool to test BLE commands:

```bash
# Scan for the device
cd /Users/avishai/code/shadow-warrior/led_controller
uv run --script ble_led_tool.py -- scan

# Send commands (replace <address> with device MAC address)
uv run --script ble_led_tool.py -- activate <address> energy_bar 75
uv run --script ble_led_tool.py -- activate <address> energy_pulse
uv run --script ble_led_tool.py -- activate <address> breathing_effect
uv run --script ble_led_tool.py -- activate <address> idle
```

## Project Structure

```
src/
├── main.rs              - Main entry point, WiFi setup, control loop
├── ble.rs              - BLE service and GATT characteristics
├── led_effects.rs      - LED effect implementations
└── command_handler.rs  - Command parsing and queue management

partitions.csv          - Custom partition table
copy_partitions.sh      - Helper script to copy partition file
sdkconfig.defaults      - ESP-IDF SDK configuration
build.rs               - Build script
Cargo.toml             - Project dependencies
```

## Troubleshooting

### Build Fails with "partitions.csv not found"

After `cargo clean`, run:
```bash
./copy_partitions.sh && cargo build --release
```

### Binary Too Large

The custom partition table provides 3MB for the app. If your binary exceeds this:

1. Check optimization settings in `Cargo.toml`
2. Consider removing unused features
3. Increase partition size in `partitions.csv` (if you have available flash)

### BLE Not Advertising

- Check that BLE is enabled in `sdkconfig.defaults`
- Verify WiFi doesn't conflict (ESP32 shares radio between WiFi and BLE)
- Check serial output for BLE initialization messages

### LEDs Not Working

- Verify GPIO pin 26 connection
- Check LED strip power supply (WS2812B strips require significant current)
- Ensure LED_COUNT matches your strip

## Development Notes

- The firmware size is approximately 1.18MB
- BLE and WiFi run concurrently (share ESP32 radio)
- All LED effects are non-blocking and run at 60 FPS
- Command processing happens in the main loop with 50ms polling interval