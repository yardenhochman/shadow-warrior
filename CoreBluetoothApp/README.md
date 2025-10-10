# macOS BLE Peripheral App

A macOS application that exposes a Bluetooth Low Energy (BLE) peripheral endpoint for iOS devices to connect via Bluefy browser.

## Features

- **BLE Peripheral Server**: Acts as a GATT server with custom service and characteristics
- **Real-time Communication**: Send and receive messages between macOS and iOS
- **Web Interface**: HTML/JavaScript client that works with Bluefy browser on iOS
- **HTTPS Server**: Secure web server for Web Bluetooth API compatibility

## Project Structure

```
CoreBluetoothApp/
├── AppDelegate.swift          # Main macOS app with BLE peripheral logic
├── MainMenu.xib              # Interface Builder file
├── Info.plist                # App configuration and permissions
├── CoreBluetoothApp.entitlements  # App sandbox and Bluetooth permissions
├── Assets.xcassets/          # App icons and assets
├── CoreBluetoothApp.xcodeproj/   # Xcode project file
└── web/                      # Web client for iOS/Bluefy
    ├── index.html            # Web interface
    ├── ble-client.js         # BLE client JavaScript
    └── server.py             # HTTPS server for web client
```

## BLE Configuration

- **Service UUID**: `12345678-1234-1234-1234-123456789ABC`
- **Characteristic UUID**: `87654321-4321-4321-4321-CBA987654321`
- **Device Name**: `ShadowWarrior-BLE`
- **Capabilities**: Read, Write, Notify

## Setup Instructions

### 1. Build and Run macOS App

1. Open `CoreBluetoothApp.xcodeproj` in Xcode
2. Select your target device (Mac)
3. Build and run the project (⌘+R)
4. Grant Bluetooth permissions when prompted
5. Click "Start Advertising" to begin BLE advertising

### 2. Setup Web Server

```bash
cd CoreBluetoothApp/web
python3 server.py
```

The server will:
- Create a self-signed certificate for HTTPS
- Start HTTPS server on port 8443
- Serve the web client at `https://localhost:8443`

### 3. Connect from iOS with Bluefy

1. **Download Bluefy**: Install Bluefy browser from App Store
2. **Find your Mac's IP**: 
   - macOS: System Preferences → Network → Wi-Fi → Advanced → TCP/IP
   - Or run: `ifconfig | grep "inet " | grep -v 127.0.0.1`
3. **Open in Bluefy**: Navigate to `https://[YOUR_MAC_IP]:8443`
4. **Accept Certificate**: Click "Advanced" → "Proceed to [IP] (unsafe)"
5. **Connect**: Click "Connect to BLE Device" and select "ShadowWarrior-BLE"

## Usage

### macOS App
- **Start Advertising**: Begins BLE advertising for device discovery
- **Stop Advertising**: Stops BLE advertising
- **Send Message**: Type a message and click Send to transmit to connected devices
- **Log**: View real-time connection and communication logs

### Web Client (iOS/Bluefy)
- **Connect**: Discover and connect to the macOS BLE peripheral
- **Send Messages**: Type messages to send to the macOS app
- **Read Characteristic**: Read current value from the BLE characteristic
- **Activity Log**: View all BLE communication activity

## Integration with Shadow Warrior Mobile App

The web client uses the same BLE connection logic as the mobile app (`mobile/src/main.js`):

- **Service UUID**: Matches the mobile app's BLE service
- **Characteristic UUID**: Compatible with mobile app's BLE characteristic
- **Web Bluetooth API**: Same connection method used in mobile app
- **Message Format**: Compatible with mobile app's BLE message format

## Troubleshooting

### macOS App Issues
- **Bluetooth Permission**: Ensure app has Bluetooth permissions in System Preferences
- **Advertising Not Working**: Check that Bluetooth is enabled and no other app is using the same service UUID
- **Connection Drops**: Verify the app stays in foreground and doesn't go to sleep

### iOS/Bluefy Issues
- **Certificate Warning**: Accept the self-signed certificate warning
- **Device Not Found**: Ensure macOS app is advertising and devices are close together
- **Connection Fails**: Try restarting both the macOS app and Bluefy browser

### Network Issues
- **HTTPS Required**: Web Bluetooth API requires HTTPS - use the provided server.py
- **Firewall**: Ensure port 8443 is not blocked by firewall
- **IP Address**: Use the correct Mac IP address, not localhost

## Development Notes

- **Xcode Version**: Requires Xcode 12+ with macOS 11+ deployment target
- **Bluetooth**: Requires macOS with Bluetooth 4.0+ support
- **Web Bluetooth**: Only works in browsers that support Web Bluetooth API (Chrome, Bluefy)
- **HTTPS**: Web Bluetooth API requires secure context (HTTPS)

## Security Considerations

- **Self-signed Certificate**: The web server uses a self-signed certificate for development
- **Local Network**: Only works on local network - not accessible from internet
- **Bluetooth Range**: Limited to Bluetooth range (~10 meters)
- **Permissions**: Requires explicit user permission for BLE connections

## Future Enhancements

- **Authentication**: Add device authentication and pairing
- **Encryption**: Implement message encryption
- **Multiple Devices**: Support multiple concurrent connections
- **Data Logging**: Persistent logging of BLE communications
- **Configuration**: GUI for BLE service/characteristic configuration
