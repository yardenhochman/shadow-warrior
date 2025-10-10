# Shadow Warrior - Interactive Punching Bag Training

A web-based interactive training app that responds to your movements and voice. The app uses accelerometer data and microphone input to dynamically control music volume and LED matrix displays.

## Features

- **Real-time Audio Control**: Music volume responds to your movements and voice intensity
- **Accelerometer Integration**: Detects device movement and shaking
- **Microphone Input**: Responds to voice levels and shouting
- **Energy Smoothing**: Fast attack, slow release for natural energy curves
- **Desktop Testing**: Simulate accelerometer data for development and testing
- **BLE LED Control**: Commands for LED matrix displays
- **Progressive Web App**: Can be installed on mobile devices

## Installation

### Prerequisites
- Node.js (v16 or higher)
- npm or yarn

### Setup
1. Clone the repository:
```bash
git clone <repository-url>
cd shadow-warrior/mobile
```

2. Install dependencies:
```bash
npm install
```

## Running the Application

### Development Mode
```bash
npm run dev
```
This starts the Vite development server, typically at `http://localhost:5173`

### Production Build
```bash
npm run build
```
Creates optimized production files in the `dist/` directory

### Preview Production Build
```bash
npm run preview
```
Serves the production build locally for testing

## Usage

1. **Start Training**: Click "Start Training (Auto Audio)" to begin
2. **Movement Detection**: Shake your device or move it around
3. **Voice Input**: Speak or shout into the microphone
4. **Volume Control**: Music volume increases with movement and voice intensity
5. **Desktop Testing**: Use "Simulate Z Move" button for testing without a mobile device

## Technical Details

- **Accelerometer**: Uses DeviceMotionEvent API for movement detection
- **Audio Processing**: Web Audio API for microphone input and volume control
- **Energy Smoothing**: Implements fast attack (0.2) and slow release (0.05) for natural response
- **Cross-Platform**: Works on mobile devices and desktop browsers
- **Real-time Processing**: 60fps data processing loop for responsive feedback

## Configuration

Adjust sensitivity using the on-screen controls:
- **Audio Scale**: Controls microphone sensitivity
- **Accel Scale**: Controls accelerometer sensitivity  
- **Accel Threshold**: Minimum movement threshold
- **Audio Min Level**: Minimum audio level for responsiveness

## Browser Compatibility

- **Mobile**: iOS Safari, Android Chrome, Samsung Internet
- **Desktop**: Chrome, Firefox, Safari, Edge
- **Features**: Accelerometer (mobile only), Microphone (all platforms), Audio playback (all platforms)

