# Shadow Warrior Brain - Implementation Details

This document describes the Capacitor/Quasar implementation of the Shadow Warrior arena controller.

## Architecture Overview

The application follows a modular, event-driven architecture with the following key components:

### Core Components

1. **State Machine** (`src/stores/state-machine.ts`)
   - Pinia store managing the arena state
   - Implements state transitions: Idle → Warming → Fight → Victory → Cooldown → Idle
   - Handles timers and timeouts for each state
   - Emits commands to devices (LEDs, speakers) via event bus

2. **Event Bus** (`src/services/event-bus.ts`)
   - Lightweight event emitter for decoupled component communication
   - Connects sensors, state machine, and device controllers
   - Events: punch detection, shout detection, state changes, device commands

3. **Sensor Services**
   - **Accelerometer** (`src/services/accelerometer.ts`): Capacitor Motion plugin for punch detection
   - **Microphone** (`src/services/microphone.ts`): Web Audio API for shout amplitude detection

4. **Device Controllers**
   - **LED Controller** (`src/services/led-controller.ts`): BLE communication with LED strips
   - **Speaker** (`src/services/speaker.ts`): HTML5 Audio for music playback
   - **UV Light** (`src/services/uv-light.ts`): HTTP-based WiFi relay control

5. **User Interface**
   - **Dashboard** (`src/pages/DashboardPage.vue`): Real-time state visualization and manual controls
   - **Settings** (`src/pages/SettingsPage.vue`): Configuration and calibration

## State Machine Flow

```
Idle → Warming (triggered by shout)
  Warming → Fight (threshold reached)
  Warming → Idle (timeout)

Fight → Victory (threshold reached)
Fight → Idle (timeout)

Victory → Cooldown (automatic after victory music)

Cooldown → Idle (after 5 minutes)
```

### State Behaviors

**Idle**
- LEDs in standby mode
- Sensors disabled or monitoring for initial shout
- All metrics reset

**Warming**
- LEDs pulse correlated with shout amplitude
- Warming power accumulates with shouts
- Transitions to Fight when threshold reached
- Timeout: Returns to Idle if threshold not reached within configured time

**Fight**
- Music playing from speakers
- LEDs pulse with punch/shout intensity
- Fight power accumulates with punches and shouts
- Transitions to Victory when threshold reached
- Timeout: Returns to Idle if threshold not reached

**Victory**
- Victory music playing
- LEDs show victory pattern
- Automatically transitions to Cooldown after music ends

**Cooldown**
- All systems off (LEDs, music)
- 5-minute timer (configurable)
- Returns to Idle when complete

## Sensor Integration

### Accelerometer (Punch Detection)
- Uses Capacitor Motion plugin
- Monitors 3-axis acceleration
- Calculates magnitude of acceleration spikes
- Configurable G-force threshold (default: 2.0G)
- Cooldown period prevents duplicate detections (default: 200ms)
- Normalizes force to 0-1 range for consistent behavior

### Microphone (Shout Detection)
- Uses Web Audio API
- Real-time FFT analysis of audio input
- Calculates RMS (Root Mean Square) amplitude
- Configurable threshold for shout detection (default: 0.3)
- Smoothing factor for noise reduction
- Emits continuous amplitude updates for visualization

## Device Communication

### LED Controller (BLE)
- Uses Capacitor Bluetooth LE plugin
- Service UUID: `6E400001-B5A3-F393-E0A9-E50E24DCCA9E`
- Characteristic UUID: `6E400004-B5A3-F393-E0A9-E50E24DCCA9E`
- Command format: `[mode, intensity, r, g, b]` (5 bytes)
- Auto-reconnect on disconnect
- Modes: off, standby, pulse, fight, victory

### Speaker (Audio)
- HTML5 Audio API
- Preloaded audio files for instant playback
- Tracks: fight music (looping), victory music (one-shot)
- Volume control
- Error handling for missing audio files

### UV Light (WiFi Relay)
- HTTP API communication
- Configurable endpoints (on, off, status)
- Optional feature (can be disabled)
- Timeout handling for network issues

## Configuration and Calibration

All parameters are configurable via the Settings page:

**State Machine**
- Warming threshold (0-100)
- Fight threshold (0-100)
- Cooldown duration (1-15 minutes)
- Warming timeout (10-120 seconds)
- Fight timeout (1-10 minutes)

**Accelerometer**
- Punch threshold (0.5-5.0 G)
- Detection cooldown (50-500ms)

**Microphone**
- Shout threshold (0.1-1.0)
- Smoothing factor (0-1.0)
- Update interval (20-200ms)

**UV Light**
- Enable/disable
- Relay URL
- Endpoints configuration

Settings are persisted to localStorage and loaded on app start.

## File Structure

```
src/
├── boot/
│   └── services.ts              # Service initialization
├── composables/
│   └── use-state-machine.ts     # State machine composable
├── pages/
│   ├── DashboardPage.vue        # Main dashboard
│   └── SettingsPage.vue         # Settings/configuration
├── services/
│   ├── event-bus.ts             # Event bus
│   ├── accelerometer.ts         # Punch detection
│   ├── microphone.ts            # Shout detection
│   ├── led-controller.ts        # LED BLE control
│   ├── speaker.ts               # Audio playback
│   └── uv-light.ts              # UV light HTTP control
├── stores/
│   └── state-machine.ts         # State machine store
└── types/
    └── state-machine.ts         # Type definitions
```

## Running the Application

### Development
```bash
npm install
npm run dev
```

### Build for Production
```bash
npm run build
```

### Android
```bash
quasar build -m capacitor -T android
npx cap sync android
npx cap open android
```

### iOS
```bash
quasar build -m capacitor -T ios
npx cap sync ios
npx cap open ios
```

## Testing

Manual testing workflow:

1. Start the app and navigate to Dashboard
2. Click "Start Sensors" to enable accelerometer and microphone
3. Test state transitions:
   - Shout to trigger Idle → Warming
   - Continue shouting to trigger Warming → Fight
   - Punch the bag or shout to accumulate fight power
   - Reach threshold to trigger Fight → Victory
   - Wait for automatic Victory → Cooldown → Idle

4. Test manual controls:
   - Use state buttons to force transitions
   - Verify metrics are displayed correctly
   - Check device status indicators

5. Test settings:
   - Adjust thresholds and timeouts
   - Verify changes take effect immediately
   - Save settings and reload page to verify persistence

## Troubleshooting

**Sensors not starting**
- Check browser permissions for motion and microphone access
- On mobile, permissions must be granted in system settings

**LED controller not connecting**
- Ensure Bluetooth is enabled
- Check that LED controller is advertising with correct service UUID
- Try manual scan and connect from device status section

**Audio not playing**
- Ensure audio files exist at `/public/audio/fight-music.mp3` and `/public/audio/victory-music.mp3`
- Check browser audio permissions
- Some browsers require user interaction before playing audio

**State transitions not working**
- Check that thresholds are configured correctly
- Verify sensors are running
- Check console logs for errors

## Next Steps

- Add audio files to `/public/audio/`
- Implement LED controller firmware matching the BLE protocol
- Add data export for training session statistics
- Implement WebRTC for remote monitoring
- Add haptic feedback on punch detection
