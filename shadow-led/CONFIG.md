# Configuration File Management

## Overview

ShadowLED uses a TOML configuration file stored on the SPIFFS partition to manage WiFi credentials and device settings. This eliminates the need to recompile firmware when changing WiFi networks or adjusting device parameters.

## Configuration File Location

- **Partition**: SPIFFS partition labeled "storage" (2MB, defined in `partitions.csv`)
- **Mount Point**: `/spiffs`
- **Config File Path**: `/spiffs/config.toml`

## Configuration Structure

The configuration file uses TOML format with two sections:

```toml
[wifi]
ssid = "YourNetworkName"
password = "YourPassword"
timeout_ms = 30000

[device]
device_name = "ShadowLED"
led_count = 180
```

### WiFi Section
- `ssid`: WiFi network name (max 32 characters)
- `password`: WiFi password (max 64 characters)
- `timeout_ms`: Connection timeout in milliseconds (default: 30000)

### Device Section
- `device_name`: BLE advertising name (how the device appears in Bluetooth scans)
- `led_count`: Number of LEDs in the WS2812B strip

## Default Behavior

On first boot or if the config file is missing:
1. SPIFFS partition is automatically formatted
2. A default `config.toml` is created with placeholder values:
   - WiFi SSID: "YourSSID"
   - WiFi Password: "YourPassword"
   - Device Name: "ShadowLED"
   - LED Count: 180

## How to Upload Configuration File

### Method 1: Using esptool.py (Recommended)

1. **Prepare your config file**
   ```bash
   cp config.toml.example config.toml
   # Edit config.toml with your actual WiFi credentials
   ```

2. **Create SPIFFS image with mkspiffs**
   
   First, install mkspiffs:
   ```bash
   # macOS (using Homebrew)
   brew install mkspiffs
   
   # Or download from: https://github.com/igrr/mkspiffs/releases
   ```

3. **Create a directory with your config file**
   ```bash
   mkdir spiffs_data
   cp config.toml spiffs_data/
   ```

4. **Generate SPIFFS image**
   ```bash
   mkspiffs -c spiffs_data -b 4096 -p 256 -s 0x1F0000 spiffs.bin
   ```
   
   Parameters explanation:
   - `-c spiffs_data`: Source directory
   - `-b 4096`: Block size (4KB, standard for ESP32)
   - `-p 256`: Page size (256 bytes, standard for ESP32)
   - `-s 0x1F0000`: Partition size (from partitions.csv: ~2MB)

5. **Flash SPIFFS image to ESP32**
   ```bash
   esptool.py --chip esp32 --port /dev/tty.usbserial-* \
     write_flash 0x210000 spiffs.bin
   ```
   
   Note: `0x210000` is the offset from `partitions.csv`

### Method 2: Using parttool.py (ESP-IDF)

If you have ESP-IDF installed:

```bash
# Create SPIFFS data directory
mkdir -p spiffs_data
cp config.toml spiffs_data/

# Generate and flash SPIFFS image
python $IDF_PATH/components/spiffs/spiffsgen.py \
  0x1F0000 spiffs_data spiffs.bin

esptool.py --chip esp32 --port /dev/tty.usbserial-* \
  write_flash 0x210000 spiffs.bin
```

### Method 3: Serial Upload Script (Future Enhancement)

A Python script could be created to upload files via serial connection without flashing:

```python
# upload_config.py (example - not yet implemented)
import serial
import sys

def upload_file(port, file_path):
    # Connect to device and upload via custom protocol
    # This would require implementing a file upload handler in the firmware
    pass
```

## Updating Configuration

### Option A: Modify and Reflash SPIFFS
1. Edit your `config.toml`
2. Recreate SPIFFS image with mkspiffs
3. Flash the new image at offset 0x210000

### Option B: Via BLE (Future Enhancement)
Future versions could support configuration updates via BLE characteristic writes.

### Option C: Via HTTP API (When WiFi Connected)
Future versions could provide a web interface for configuration management.

## Verifying Configuration

After flashing, monitor the serial output during boot:

```
I (xxx) shadow_led: Mounting SPIFFS partition...
I (xxx) shadow_led: SPIFFS mounted successfully at /spiffs
I (xxx) shadow_led: Loading configuration from /spiffs/config.toml
I (xxx) shadow_led: Config loaded - WiFi SSID: YourNetworkName, Device: ShadowLED, LEDs: 180
```

If the config file is missing or invalid:
```
W (xxx) shadow_led: Failed to load config: <error>. Using defaults and creating config file.
I (xxx) shadow_led: Default configuration saved to /spiffs/config.toml
```

## Troubleshooting

### SPIFFS Mount Failures
- **Symptom**: "Failed to mount SPIFFS" error
- **Solution**: The partition will auto-format on first boot. If issues persist, erase flash:
  ```bash
  esptool.py --chip esp32 --port /dev/tty.usbserial-* erase_region 0x210000 0x1F0000
  ```

### Config Parse Errors
- **Symptom**: Device uses default values despite having config file
- **Solution**: Check TOML syntax. Common issues:
  - Missing quotes around string values
  - Incorrect section headers `[wifi]` and `[device]`
  - Typos in parameter names

### WiFi Not Connecting with Custom Config
- Check SSID and password are correct
- Verify SSID is max 32 characters
- Verify password is max 64 characters
- Check serial monitor for specific error messages

## Quick Reference Commands

```bash
# Create SPIFFS image
mkspiffs -c spiffs_data -b 4096 -p 256 -s 0x1F0000 spiffs.bin

# Flash SPIFFS only (preserves application firmware)
esptool.py --chip esp32 --port /dev/tty.usbserial-* write_flash 0x210000 spiffs.bin

# Flash complete firmware (app + SPIFFS)
cargo espflash flash --release --monitor

# Erase SPIFFS partition
esptool.py --chip esp32 --port /dev/tty.usbserial-* erase_region 0x210000 0x1F0000
```

## Security Considerations

⚠️ **Important**: The config.toml file contains WiFi credentials in plain text. Consider:

1. **Don't commit config.toml to git**: Use `config.toml.example` as a template
2. **Physical security**: Anyone with serial access can read SPIFFS contents
3. **Future enhancement**: Consider encrypting sensitive values in flash

## File Structure

```
shadow-led/
├── config.toml.example    # Template with documentation
├── CONFIG.md              # This file
├── partitions.csv         # Partition table (defines SPIFFS location)
├── src/
│   ├── config.rs         # Configuration loading module
│   └── main.rs           # Uses config from SPIFFS
└── spiffs_data/          # (User-created) Files to be written to SPIFFS
    └── config.toml       # Your actual configuration
```
