#!/bin/bash

echo "🥋 Shadow Warrior BLE Simulator Setup"
echo "====================================="
echo ""

# Check if Node.js is installed
if ! command -v node &> /dev/null; then
    echo "❌ Node.js is not installed"
    echo "Please install Node.js from: https://nodejs.org/"
    exit 1
fi

echo "✅ Node.js found: $(node --version)"

# Check if npm is available
if ! command -v npm &> /dev/null; then
    echo "❌ npm is not available"
    exit 1
fi

echo "✅ npm found: $(npm --version)"

# Install dependencies
echo ""
echo "📦 Installing dependencies..."
npm install

if [ $? -eq 0 ]; then
    echo "✅ Dependencies installed successfully"
else
    echo "❌ Failed to install dependencies"
    exit 1
fi

echo ""
echo "🚀 Setup complete!"
echo ""
echo "To start the BLE simulator:"
echo "  sudo npm start"
echo ""
echo "Make sure:"
echo "  - Bluetooth is enabled on your Mac"
echo "  - You have Bluefy installed on your iOS device"
echo "  - Both devices are on the same network"
echo ""
echo "Happy testing! 🎯"
