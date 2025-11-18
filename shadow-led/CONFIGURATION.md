# Configuration Management Quick Start

## TL;DR

```bash
# 1. Create your config file from example
cp config.toml.example config.toml

# 2. Edit with your WiFi credentials
nano config.toml  # or use your favorite editor

# 3. Upload to device (requires mkspiffs and esptool.py)
./upload_spiffs.sh
```

## Files Created

- **`config.toml.example`** - Template configuration file with comments
- **`CONFIG.md`** - Complete documentation for configuration management
- **`upload_spiffs.sh`** - Helper script to upload config to device

## What Changed

WiFi credentials and device settings are now loaded from a TOML file on the SPIFFS partition instead of being hardcoded. This means:

✅ No recompilation needed to change WiFi networks  
✅ Easy device configuration management  
✅ Support for multiple devices with different settings  
✅ Secure: config.toml is in .gitignore (credentials not committed)

## Default Configuration Values

If no config file exists, these defaults are used:
- **WiFi SSID**: "YourSSID" (placeholder - device will timeout and run BLE-only)
- **WiFi Password**: "YourPassword" (placeholder)
- **WiFi Timeout**: 30000ms (30 seconds)
- **Device Name**: "ShadowLED" (BLE advertising name)
- **LED Count**: 180

## See Also

- **CONFIG.md** - Full documentation with troubleshooting
- **src/config.rs** - Configuration loading implementation
