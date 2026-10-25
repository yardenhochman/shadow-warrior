---
name: flash-firmware
description: Compile and upload the punching bag Arduino sketch (punching_bag/punching_bag.ino) to the Seeed XIAO nRF52840 Sense. Use when asked to flash, compile, or upload punching bag firmware.
disable-model-invocation: true
---

# Flash punching bag firmware

Target: `punching_bag/punching_bag.ino`, board Seeed XIAO nRF52840 Sense. Sketch uses Bluefruit52Lib (`bluefruit.h`) directly, not ArduinoBLE.

**FQBN must be the nrf52 core, not mbed** — the mbed core (`Seeeduino:mbed:xiaonRF52840Sense`) lacks `bluefruit.h` and will fail to compile. Use:

```
Seeeduino:nrf52:xiaonRF52840Sense
```

## Steps

1. Find the port:
   ```bash
   arduino-cli board list
   ```
2. Compile:
   ```bash
   arduino-cli compile --fqbn Seeeduino:nrf52:xiaonRF52840Sense punching_bag/punching_bag.ino
   ```
3. Upload (replace `<port>`):
   ```bash
   arduino-cli upload -p <port> --fqbn Seeeduino:nrf52:xiaonRF52840Sense punching_bag/punching_bag.ino
   ```

Required libraries: Bluefruit52Lib (ships with the nrf52 core), Adafruit_LSM6DS3. Install missing ones with `arduino-cli lib install "Adafruit LSM6DS3"`.

If compile fails with `bluefruit.h: No such file`, the wrong core/FQBN was selected — double check it's `Seeeduino:nrf52`, not `Seeeduino:mbed`.
