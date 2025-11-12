#!/usr/bin/env bash
# upload_spiffs.sh - Helper script to create and upload SPIFFS image with config.toml

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SPIFFS_DATA_DIR="${SCRIPT_DIR}/spiffs_data"
SPIFFS_IMAGE="${SCRIPT_DIR}/spiffs.bin"
CONFIG_FILE="${SCRIPT_DIR}/config.toml"
CONFIG_EXAMPLE="${SCRIPT_DIR}/config.toml.example"

# SPIFFS parameters (must match partitions.csv)
BLOCK_SIZE=4096
PAGE_SIZE=256
PARTITION_SIZE=0x1F0000  # ~2MB
PARTITION_OFFSET=0x210000

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "=================================="
echo "ShadowLED SPIFFS Upload Script"
echo "=================================="
echo

# Check if config.toml exists
if [ ! -f "$CONFIG_FILE" ]; then
    echo -e "${YELLOW}Warning: config.toml not found${NC}"
    if [ -f "$CONFIG_EXAMPLE" ]; then
        echo "Creating config.toml from example..."
        cp "$CONFIG_EXAMPLE" "$CONFIG_FILE"
        echo -e "${RED}Please edit config.toml with your WiFi credentials before running this script again!${NC}"
        exit 1
    else
        echo -e "${RED}Error: config.toml.example not found${NC}"
        exit 1
    fi
fi

# Check for mkspiffs
if ! command -v mkspiffs &> /dev/null; then
    echo -e "${RED}Error: mkspiffs not found${NC}"
    echo "Please install mkspiffs:"
    echo "  macOS: brew install mkspiffs"
    echo "  Or download from: https://github.com/igrr/mkspiffs/releases"
    exit 1
fi

# Check for esptool.py
if ! uv run esptool version &> /dev/null; then
    echo -e "${RED}Error: esptool.py not found${NC}"
    echo "Please install esptool:"
    echo "  uv pip install esptool"
    exit 1
fi

# Create spiffs_data directory
echo "Creating SPIFFS data directory..."
rm -rf "$SPIFFS_DATA_DIR"
mkdir -p "$SPIFFS_DATA_DIR"
cp "$CONFIG_FILE" "$SPIFFS_DATA_DIR/"

# Display config contents
echo
echo "Configuration to be uploaded:"
echo "----------------------------"
cat "$CONFIG_FILE"
echo "----------------------------"
echo

# Generate SPIFFS image
echo
echo "Generating SPIFFS image..."
mkspiffs -c "$SPIFFS_DATA_DIR" \
         -b $BLOCK_SIZE \
         -p $PAGE_SIZE \
         -s $PARTITION_SIZE \
         "$SPIFFS_IMAGE"

if [ $? -ne 0 ]; then
    echo -e "${RED}Error: Failed to create SPIFFS image${NC}"
    exit 1
fi

echo -e "${GREEN}✓ SPIFFS image created successfully${NC}"

# Find USB serial port
echo
echo "Looking for ESP32 device..."
PORT=""

# Try common port patterns
if [ "$(uname)" == "Darwin" ]; then
    # macOS
    PORT=$(ls /dev/tty.usbserial-* 2>/dev/null | head -n 1)
    if [ -z "$PORT" ]; then
        PORT=$(ls /dev/tty.SLAB_USBtoUART 2>/dev/null | head -n 1)
    fi
elif [ "$(expr substr $(uname -s) 1 5)" == "Linux" ]; then
    # Linux
    PORT=$(ls /dev/ttyUSB* 2>/dev/null | head -n 1)
fi

if [ -z "$PORT" ]; then
    echo -e "${YELLOW}Could not automatically detect port${NC}"
    read -p "Enter serial port (e.g., /dev/tty.usbserial-0001): " PORT
fi

echo "Using port: $PORT"

# Flash SPIFFS image
echo
echo "Flashing SPIFFS image to ESP32..."
echo "Offset: $PARTITION_OFFSET"
echo

uv run esptool --chip esp32 \
           --port "$PORT" \
           --baud 460800 \
           write_flash $PARTITION_OFFSET "$SPIFFS_IMAGE"

if [ $? -ne 0 ]; then
    echo -e "${RED}Error: Failed to flash SPIFFS image${NC}"
    exit 1
fi

# Cleanup
echo
echo "Cleaning up..."
rm -rf "$SPIFFS_DATA_DIR"

echo
echo -e "${GREEN}✓ SPIFFS upload complete!${NC}"
echo
echo "The device will now boot with your configuration."
echo "Monitor output with: cargo espflash monitor"
echo
