# Sensor station hardware (perfboard + box)

Target: ESP32-WROOM 38-pin devkit, 2x ICS-43434 I2S mics (130 mm apart), LD2410 radar,
on a 5 x 7 cm perfboard in one box. Firmware pins are in `README.md`; this file is the
build spec. Pin positions below are for the common 38-pin devkit (DevKitC / NodeMCU-32S
clones): **check them against the silkscreen on your board before soldering**.

## Layout in the box

```
 front panel (faces the arena)
 +------------------------------------------------------------+
 | [mic A, SEL=GND]           [ radar ]           [mic B, SEL=3V3] |
 |  port hole + gasket     antenna face to wall     port hole + gasket
 +------------------------------------------------------------+
        ~8-10 cm cable            |                 ~8-10 cm cable
                           [ perfboard 5x7 ]
                           [ devkit on female headers, USB at side wall ]
```

- Mic centres 130.0 mm apart (firmware `d_mm`). Mic breakouts glued face-down on the inside
  of the front panel, port over a 1.5-2 mm hole, thin foam gasket around the port.
- Radar stands upright, antenna face against the panel (<= 2 mm plastic, or a window).
- Radar and mic line axes agree: 0 deg = straight out of the front panel.
- WROOM antenna end overhangs the perfboard edge; no copper or metal near it.
- Radar must not point at the bag if the bag is in range (see bead ll3 notes).

## Pins (38-pin devkit, USB at the bottom)

| Left header (top to bottom) | Right header (top to bottom) |
|---|---|
| 3V3, EN, 36, 39, 34, 35, 32, **33**, **25**, **26**, **27**, 14, 12, GND, 13, 9, 10, 11, 5V | GND, 23, 22, TX0, RX0, 21, GND, 19, 18, 5, **17**, **16**, 4, 0, 2, 15, 8, 7, 6 |

Mics use the left header (3V3 at the top, 33/25/26 in the middle); the radar UART is on the
right header, 5V at the bottom left. Strapping pins 0, 2, 12, 15 are unused. This assumes a
WROOM module (a WROVER uses GPIO16/17 for PSRAM).

## Netlist

| Net | From | To |
|---|---|---|
| VMIC | devkit 3V3 -> R3 22 ohm | C1 10 uF + C2 100 nF to GND; J_MIC_A pin 1; J_MIC_B pin 1 |
| BCLK | GPIO26 -> R1 33 ohm | J_MIC_A pin 3; J_MIC_B pin 3 |
| LRCL | GPIO25 -> R2 33 ohm | J_MIC_A pin 4; J_MIC_B pin 4 |
| DOUT | J_MIC_A pin 5 and J_MIC_B pin 5 joined | GPIO33 (no series part) |
| GND (mics) | J_MIC_A/B pins 2 and 6 | star ground point |
| 5V_RADAR | devkit 5V pin | J_RADAR VCC; C3 100 uF + C4 100 nF to GND |
| RADAR_TX | J_RADAR TX | GPIO16 (ESP RX) |
| RADAR_RX | J_RADAR RX | GPIO17 (ESP TX) |
| RADAR_OUT | J_RADAR OUT | GPIO27 (optional, unused by firmware) |
| GND (radar) | J_RADAR GND | star ground point (own wire, not shared with mic returns) |

Mic cable (6 wires, same on both sides): 1 VMIC, 2 GND, 3 BCLK, 4 LRCL, 5 DOUT, 6 GND.
Twist 2+3 (GND with BCLK) and 5+6 (DOUT with GND); LRCL runs beside them. SEL is wired at the
mic end, not through the cable: mic A SEL to its GND (left), mic B SEL to its 3V (right).
Put 100 nF across 3V/GND on each mic breakout. Keep each cable <= 10 cm, equal length.

## Parts

- ESP32 38-pin devkit + 2x 19-pin female headers
- 2x ICS-43434 breakouts (3V, GND, BCLK, DOUT, LRCL, SEL), LD2410 module (TX RX OUT GND VCC)
- R1, R2: 33 ohm; R3: 22 ohm (1/4 W through hole is fine)
- C1: 10 uF (ceramic or small electrolytic); C2, C4 + 2 at the mics: 100 nF; C3: 100 uF
- 2x 6-pin locking connectors (JST-XH or PH) for the mics, 1x 5-pin for the radar
- Foam gasket, M2/M2.5 standoffs for the perfboard, 3D printed or plastic box

## Perfboard notes

- Devkit on female headers; the low parts (R1-R3, ceramic caps) can sit under the devkit
  between the header rows. Connectors at the edge nearest the front panel.
- One star-ground pad; run mic returns and radar return to it separately.

## Measurements still needed for the box model

Devkit length/width and header row spacing, radar module size and hole pattern, mic
breakout size and port position (and hole size), perfboard thickness, USB port height.
