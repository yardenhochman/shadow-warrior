# 🥋 Shadow Warrior

A web-based training application that combines audio tracks with motion detection to create an immersive martial arts training experience. The app uses microphone input and accelerometer data to dynamically adjust audio volume based on your movement and vocal intensity.

## 🌟 Features

- **Motion-Responsive Audio**: Audio tracks that respond to your movement intensity
- **Microphone Integration**: Voice-activated volume control
- **Bluetooth LED Control**: Connect to BLE devices for visual feedback
- **Multiple Track Support**: Choose from various training tracks
- **Real-time Analytics**: Live graphs showing motion and audio data
- **Cross-Platform**: Works on Android, iOS (with limitations), and desktop

## 🚀 Quick Start

### Web App
1. Visit: https://leds-shadow.web.app
2. Grant microphone and motion permissions when prompted
3. Select a training track
4. Click "Start Training" and begin your workout!

### Local Development
```bash
cd mobile
npm install
npm run start
```

## 📱 Platform Support

### ✅ Android (Full Support)
- Web Bluetooth for LED control
- Accelerometer access
- Microphone access
- All features available

### ⚠️ iOS (Limited Support)
iOS browsers have significant limitations due to Apple's WebKit restrictions:

#### What Works:
- Basic web app functionality
- Audio playback
- UI interactions

#### What Doesn't Work:
- ❌ Web Bluetooth (no LED control)
- ❌ Accelerometer access
- ❌ Microphone access

#### iOS Solutions:

##### Option 1: Bluefy Browser (Recommended for BLE)
For Bluetooth LED control on iOS:
1. Download [Bluefy](https://apps.apple.com/app/bluefy/id1492822056) (Free)
2. Open the app in Bluefy
3. Connect to BLE devices

**Note**: Bluefy only enables Web Bluetooth - accelerometer and microphone are still unavailable.

##### Option 2: WebBLE Browser
Alternative BLE browser:
1. Download [WebBLE](https://apps.apple.com/app/webble/id1193531073) ($2.29)
2. Open the app in WebBLE
3. Connect to BLE devices

##### Option 3: Native iOS App
For full functionality on iOS, consider developing a native app with:
- Core Bluetooth for BLE
- Core Motion for accelerometer
- AVFoundation for microphone

### 🖥️ Desktop (Test Mode)
- Simulated motion data for testing
- Full audio functionality
- No real sensor access

## 🎵 Available Tracks

The app includes various training tracks optimized for different martial arts styles:
- War-themed tracks for intense training
- Various energy levels and genres
- Custom audio file upload support

## 🔧 Technical Details

### Web APIs Used
- **Web Bluetooth API**: BLE device communication
- **MediaDevices API**: Microphone access
- **DeviceMotionEvent**: Accelerometer data
- **Web Audio API**: Audio processing and analysis

### Browser Requirements
- **Chrome/Edge**: Full support (Android/Desktop)
- **Safari**: Limited support (iOS)
- **Firefox**: Limited support
- **Bluefy**: BLE only (iOS)

## 🛠️ Development

### Project Structure
```
shadow-warrior/
├── mobile/           # Web application
├── brain/           # Backend services
├── punching_bag/    # Hardware code
└── simulator/       # LED simulation
```

### Building
```bash
# Build web app
cd mobile
npm run build

# Deploy to Firebase
firebase deploy
```

## 📋 Permissions

The app requires the following permissions:
- **Microphone**: For voice-activated volume control
- **Motion**: For movement detection
- **Bluetooth**: For LED device connection (Android/Bluefy only)

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Test on multiple platforms
5. Submit a pull request

## 📄 License

This project is open source. See individual components for specific licensing.

## 🆘 Troubleshooting

### iOS Issues
- **No motion detection**: Use a native iOS app or test on Android
- **No microphone**: Use a native iOS app or test on Android
- **No BLE connection**: Use Bluefy or WebBLE browser

### Android Issues
- **Permission denied**: Check browser permissions in settings
- **BLE not working**: Ensure Bluetooth is enabled and device is discoverable

### General Issues
- **Audio not playing**: Check browser autoplay policies
- **Poor performance**: Close other tabs and applications
