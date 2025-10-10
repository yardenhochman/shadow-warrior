#!/bin/bash

# Build and Run Script for macOS BLE Peripheral App
# This script builds the Xcode project and starts the web server

set -e

echo "🚀 Building and Running macOS BLE Peripheral App"
echo "================================================"

# Get the directory where this script is located
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$SCRIPT_DIR"
WEB_DIR="$PROJECT_DIR/web"

echo "📁 Project directory: $PROJECT_DIR"
echo "🌐 Web directory: $WEB_DIR"

# Check if Xcode is installed
if ! command -v xcodebuild &> /dev/null; then
    echo "❌ Xcode command line tools not found. Please install Xcode."
    exit 1
fi

# Check if Python 3 is installed
if ! command -v python3 &> /dev/null; then
    echo "❌ Python 3 not found. Please install Python 3."
    exit 1
fi

echo ""
echo "🔨 Building Xcode project..."

# Build the Xcode project
cd "$PROJECT_DIR"
xcodebuild -project CoreBluetoothApp.xcodeproj -scheme CoreBluetoothApp -configuration Debug build

if [ $? -eq 0 ]; then
    echo "✅ Xcode project built successfully"
else
    echo "❌ Xcode build failed"
    exit 1
fi

echo ""
echo "🎯 Opening built app..."

# Open the built app
open build/Debug/CoreBluetoothApp.app

echo "✅ macOS app launched"
echo ""
echo "📱 Next steps:"
echo "1. Grant Bluetooth permissions when prompted"
echo "2. Click 'Start Advertising' in the macOS app"
echo "3. The web server will start automatically"
echo ""

# Wait a moment for the app to start
sleep 3

echo "🌐 Starting HTTPS web server..."

# Start the web server
cd "$WEB_DIR"
python3 server.py
