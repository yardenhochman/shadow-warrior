#!/bin/bash
# Script to copy partitions.csv to the ESP-IDF build output directory

# Get the build directory from cargo - check both debug and release
for BUILD_TYPE in debug release; do
    BUILD_DIR="target/xtensa-esp32-espidf/$BUILD_TYPE/build"

    # Find the esp-idf-sys build directory
    ESP_IDF_SYS_DIR=$(find "$BUILD_DIR" -type d -name "esp-idf-sys-*" 2>/dev/null | head -1)

    if [ -n "$ESP_IDF_SYS_DIR" ]; then
        OUT_DIR="$ESP_IDF_SYS_DIR/out"
        mkdir -p "$OUT_DIR"
        cp partitions.csv "$OUT_DIR/"
        echo "Copied partitions.csv to $OUT_DIR"
    fi
done

echo "esp-idf-sys build directory not found - will be created during build"
