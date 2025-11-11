#!/bin/bash
# Create and flash a formatted SPIFFS partition for the ESP32
# This script should be run during build time to prepare the SPIFFS partition

set -euo pipefail

# Configuration
SPIFFS_PARTITION_NAME="storage"
SPIFFS_SIZE="2097152"  # 2MB in bytes
SPIFFS_DIR="./spiffs_data"
SPIFFS_IMAGE="./spiffs.bin"

# Check if mkspiffs is available (part of ESP-IDF tools)
if ! command -v mkspiffs &> /dev/null; then
    echo "Error: mkspiffs not found. Please install ESP-IDF tools:"
    echo "  uv pip install esptool"
    echo "  Or install full ESP-IDF: https://docs.espressif.com/projects/esp-idf/en/latest/esp32/get-started/"
    exit 1
fi

echo "Creating SPIFFS directory structure..."
mkdir -p "$SPIFFS_DIR"

# Create a default wifi.conf template (optional)
cat > "$SPIFFS_DIR/wifi.conf.template" << 'EOF'
# WiFi Configuration Template
# Copy this to wifi.conf and fill in your credentials
# ssid="YourWiFiName"
# password="YourWiFiPassword"
EOF

echo "Creating SPIFFS image ($SPIFFS_SIZE bytes)..."
# Create empty SPIFFS image (formatted)
mkspiffs -c "$SPIFFS_DIR" -b 4096 -p 256 -s "$SPIFFS_SIZE" "$SPIFFS_IMAGE"

echo "SPIFFS image created: $SPIFFS_IMAGE"
echo "Image size: $(stat -f%z "$SPIFFS_IMAGE" 2>/dev/null || stat -c%s "$SPIFFS_IMAGE") bytes"

# Optional: Flash the SPIFFS image (uncomment to enable)
# echo "To flash the SPIFFS partition, run:"
# echo "  esptool.py --chip esp32 --port /dev/ttyUSB0 --baud 460800 write_flash 0x210000 $SPIFFS_IMAGE"

echo "SPIFFS partition ready for flashing at offset 0x210000"