# Shadow Warrior Brain (Rust)

Modern Rust rewrite of the Shadow Warrior training system brain with improved architecture and new game mechanics.

## Features

### Architecture
- **Async/await** throughout using Tokio runtime
- **Event-driven** pub/sub architecture
- **State machine** with lifecycle hooks
- **Modular services** for each hardware component
- **Type-safe configuration** with serde

### Hardware Integration
- **BLE** - Punching bag via btleplug (Nordic UART Service)
- **WLED** - LED controllers via HTTP JSON API + UDP realtime (WARLS protocol)
- **Tasmota** - Smart plugs via HTTP
- **Audio** - Microphone capture via cpal with shout detection
- **Music** - Playlist playback via rodio

### Game Logic

**States:** `SUSPENDED` ↔ `IDLE` → `WARMING` → `FIGHT` → `COOLDOWN` → `IDLE`

1. **SUSPENDED** - System idle, no presence detected
2. **IDLE** - Waiting for player, breathing LED effect
3. **WARMING** - Player building energy by shouting
   - Energy bar fills with shout intensity
   - Energy decays over time
   - Transitions to FIGHT when bar is full
4. **FIGHT** - Active fighting session
   - Music starts playing from playlist
   - Tracks punches and activity
   - Ends after configured duration OR inactivity timeout
5. **COOLDOWN** - Post-fight recovery period
   - Automatically transitions back to IDLE

### Web Dashboard
- Real-time state visualization
- Energy bar display (WARMING state)
- Fight timer and statistics
- Hardware connection status
- Manual controls
- SSE (Server-Sent Events) for live updates

## Configuration

Edit `config.yaml`:

```yaml
server:
  host: "0.0.0.0"
  port: 8000

arena:
  warming_energy_threshold: 100.0
  fight_duration_sec: 180
  fight_inactivity_timeout_sec: 30
  cooldown_duration_sec: 60
  shout_energy_multiplier: 10.0
  energy_decay_rate: 0.5

ble:
  auto_connect: true
  device_name_patterns: ["shadow", "warrior", "punch", "bag"]
  alpha: 0.8
  threshold: 2.0

wled:
  controllers:
    - ip: "192.168.1.100"
      num_leds: 150

tasmota:
  plugs:
    - ip: "192.168.1.200"
      name: "main_lights"

music:
  playlist_dir: "./music"
  shuffle: true
  volume: 0.8
```

## Development

### Build
```bash
cargo build --release
```

### Run
```bash
cargo run
```

### Test
```bash
cargo test
```

## API Endpoints

### Status API
- `GET /api/state` - Complete brain state snapshot
- `GET /api/events` - SSE stream for real-time updates
- `GET /api/stats` - Detailed statistics

### Control API
- `POST /api/arena/presence` - Trigger presence detection
  ```json
  {"detected": true}
  ```
- `POST /api/arena/suspend` - Force suspend arena

## Project Structure

```
brain-rs/
├── src/
│   ├── main.rs              # Entry point + axum server
│   ├── config.rs            # Configuration management
│   ├── models.rs            # Data structures
│   ├── core/
│   │   ├── events.rs        # Event bus
│   │   └── state_machine.rs # State machine
│   ├── services/
│   │   ├── arena_manager.rs # Orchestration
│   │   ├── ble_manager.rs   # BLE communication
│   │   ├── audio_manager.rs # Audio + energy bar
│   │   ├── wled_manager.rs  # LED effects
│   │   ├── tasmota_manager.rs # Smart plugs
│   │   └── music_manager.rs # Music playback
│   ├── api/
│   │   ├── status.rs        # Status endpoints
│   │   └── arena.rs         # Control endpoints
│   └── web/                 # Web dashboard
│       ├── index.html
│       ├── style.css
│       └── app.js
├── config.yaml              # Configuration
└── Cargo.toml
```

## License

MIT
