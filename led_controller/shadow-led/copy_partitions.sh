#!/bin/bash
# Copy partitions.csv to the esp-idf build output directory (debug & release)
# Exits non-zero if partitions.csv is missing or no esp-idf-sys build dirs were found.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
PARTITIONS_FILE="$ROOT_DIR/partitions.csv"

if [ ! -f "$PARTITIONS_FILE" ]; then
    echo "Error: partitions.csv not found at $PARTITIONS_FILE"
    exit 2
fi

copied_any=0

for BUILD_TYPE in debug release; do
    BUILD_DIR="$ROOT_DIR/target/xtensa-esp32-espidf/$BUILD_TYPE/build"

    if [ ! -d "$BUILD_DIR" ]; then
        # No build dir yet; skip
        continue
    fi

    # Find esp-idf-sys build dir(s) and copy into each found out/ directory.
    while IFS= read -r esp_dir; do
        OUT_DIR="$esp_dir/out"
        mkdir -p "$OUT_DIR"
        cp -f "$PARTITIONS_FILE" "$OUT_DIR/"
        echo "Copied partitions.csv to $OUT_DIR"
        copied_any=1
    done < <(find "$BUILD_DIR" -type d -name 'esp-idf-sys-*' 2>/dev/null)

done

if [ "$copied_any" -eq 0 ]; then
    echo "No esp-idf-sys build directory found yet. It will be created during build."
    exit 3
fi

exit 0
