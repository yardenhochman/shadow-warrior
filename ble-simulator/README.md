# 🥋 Shadow Warrior BLE Simulator

A Bluetooth Low Energy (BLE) peripheral simulator for testing the Shadow Warrior app with Bluefy on iOS.

## 🚀 Quick Start

### Prerequisites
- macOS with Bluetooth enabled
- Node.js installed
- iOS device with Bluefy browser

### Setup
```bash
cd ble-simulator
npm install
sudo npm start
```

**Note**: `sudo` is required for BLE operations on macOS.

### Testing with Bluefy
1. **Start the simulator** on your Mac
2. **Open Bluefy** on your iOS device
3. **Navigate** to your Shadow Warrior app
4. **Click "Connect BLE"** in the app
5. **Select "Shadow Warrior LED"** from the device list
6. **Start training** to see energy levels transmitted to the simulator

## 🔧 What It Simulates

The simulator creates a BLE peripheral with:
- **Service UUID**: `12345678-1234-1234-1234-123456789abc`
- **Characteristic UUID**: `12345678-1234-1234-1234-123456789abd`
- **Device Name**: "Shadow Warrior LED"

## 📊 Features

- **Energy Level Reception**: Receives energy values (0-255) from the web app
- **Real-time Logging**: Shows energy levels in console
- **Connection Monitoring**: Tracks client connections/disconnections
- **Graceful Shutdown**: Handles Ctrl+C properly

## 🛠️ Troubleshooting

### "Bluetooth not available"
- Ensure Bluetooth is enabled in System Preferences
- Try restarting Bluetooth: `sudo pkill bluetoothd`

### "Permission denied"
- Make sure you're running with `sudo`
- Check that Node.js has necessary permissions

### "Device not found in Bluefy"
- Ensure the simulator is running and advertising
- Check that both devices are on the same network
- Try restarting the simulator

## 🔍 Testing Different Scenarios

The simulator will log energy levels as you train:
```
🎯 LED Energy Level: 128/255 (50.2%)
🎯 LED Energy Level: 200/255 (78.4%)
🎯 LED Energy Level: 45/255 (17.6%)
```

This helps you verify that:
- BLE connection is working
- Energy calculations are correct
- Data transmission is reliable

## 📱 iOS Testing Workflow

1. **Mac**: Start simulator
2. **iOS**: Open Bluefy → Navigate to app
3. **iOS**: Connect to "Shadow Warrior LED"
4. **iOS**: Start training session
5. **Mac**: Monitor energy levels in console
6. **iOS**: Verify LED effects (if implemented)

## 🎯 Next Steps

Once BLE connection is verified:
- Test with real LED hardware
- Implement LED matrix effects
- Add more BLE characteristics
- Test with multiple devices
