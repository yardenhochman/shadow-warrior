# Shadow Warrior Brain - Quasar/Capacitor Implementation

A mobile-first Quasar/Capacitor application for controlling the Shadow Warrior art installation arena.

## Features

- **State Machine**: Manages arena states (Idle, Warming, Fight, Victory, Cooldown)
- **Sensor Integration**: Accelerometer for punch detection, microphone for shout detection
- **Device Control**: BLE LED controllers, audio playback, optional WiFi UV lights
- **Real-time UI**: Live metrics dashboard with state visualization
- **Configuration**: Full calibration and threshold settings
- **Persistence**: Settings saved to localStorage

## Architecture

### State Flow

```
Idle → Warming (shout detected)
  Warming → Fight (threshold reached) or Idle (timeout)
  Fight → Victory (threshold reached) or Idle (timeout)
  Victory → Cooldown (automatic)
  Cooldown → Idle (after 5 minutes)
```

### Components

- **State Machine** (`src/stores/state-machine.ts`): Pinia store managing arena state
- **Event Bus** (`src/services/event-bus.ts`): Decoupled event communication
- **Sensors**: Accelerometer and microphone services
- **Device Controllers**: LED (BLE), Speaker (HTML5 Audio), UV Light (HTTP)
- **UI**: Dashboard and Settings pages

## Quick Start

### Development

```bash
npm install
npm run dev
```

### Build

```bash
npm run build
```

### Android

```bash
npm install
quasar build -m capacitor -T android
npx cap sync android
npx cap open android
```

### iOS

```bash
npm install
quasar build -m capacitor -T ios
npx cap sync ios
npx cap open ios
```

## Setup Requirements

1. **Audio Files**: Add music files to `public/audio/`:
   - `fight-music.mp3` (looping fight music)
   - `victory-music.mp3` (victory fanfare)

2. **LED Controller**: BLE device with service UUID `6E400001-B5A3-F393-E0A9-E50E24DCCA9E`

3. **Permissions**: Grant microphone and motion permissions when prompted

## Usage

1. Open the app and navigate to the Dashboard
2. Click "Start Sensors" to enable accelerometer and microphone
3. Optionally connect to LED controller via "Connect" button
4. The arena will automatically transition through states based on input:
   - Shout loudly to enter Warming mode
   - Continue shouting to fill the warming bar
   - Once Fight mode activates, punch the bag and shout to accumulate power
   - Reach 100% to trigger Victory mode
   - After celebration, the arena enters a 5-minute Cooldown

## Configuration

Navigate to Settings to adjust:

- **State Machine**: Warming/fight thresholds, timeout durations
- **Accelerometer**: Punch force threshold, detection cooldown
- **Microphone**: Shout detection threshold, smoothing, update rate
- **UV Light**: Enable/disable, configure WiFi relay endpoints

All settings are automatically saved to browser storage.

## Manual Controls

Use the state buttons on the Dashboard to force state transitions for testing:

- Idle, Warming, Fight buttons
- Start/Stop Sensors
- Connect LED Controller

## Documentation

See [IMPLEMENTATION.md](./IMPLEMENTATION.md) for detailed architecture and implementation notes.

## Troubleshooting

**Sensors not starting?**

- Check browser permissions for motion and microphone
- On mobile, check system settings

**LED not connecting?**

- Enable Bluetooth
- Ensure LED controller is powered and advertising
- Check service UUID matches

**No audio?**

- Verify audio files exist in `public/audio/`
- Check browser audio permissions
- User interaction may be required before audio plays

**State transitions not working?**

- Verify thresholds in Settings
- Check that sensors are running
- Review console logs for errors
