# BLE provisioning

A station that has no working Wi-Fi settings advertises a BLE GATT service. The brain finds it, reads a one-time
challenge, writes the Wi-Fi SSID/password and its own address (sealed), and the station stores them in flash, joins
Wi-Fi and starts sending UDP frames to the brain. Phones can do the same with a generic BLE app only if they can
seal the payload, so the brain is the normal provisioner.

Code: `station-proto/src/provision.rs` (UUIDs, `Config`, `Status`, `seal`/`open`, tests).

## Service `91082775-310d-48d0-b0cc-b87f04b92dd2`

| Characteristic (UUID suffix `-310d-48d0-b0cc-b87f04b92dd2`) | Props | Content |
|---|---|---|
| `91082776` info | read | UTF-8 `station-fw <version> id=<station_id hex>` |
| `91082777` status | read, notify | 8 bytes: link (0 unconfigured, 1 connecting, 2 up, 3 failed), rssi i8, ip[4], selftest (0 pass, 1 warn, 2 fail, 255 not run), flags (bit0 config stored) |
| `91082778` challenge | read | 12 random bytes. Valid for ONE write to `config`; a fresh one is drawn after every attempt |
| `91082779` config | write | `seal(key, challenge, Config)`: postcard(Config) encrypted with ChaCha20-Poly1305, tag appended (max 168 bytes; use write-with-response, long write is fine) |
| `9108277a` command | write | 1 byte: 1 restart, 2 clear stored Wi-Fi settings, 3 re-run the self test |

`Config { ssid: String<32>, pass: String<64>, brain: String<48> }`; `brain` is `ip[:port]` (default port 4210), empty
keeps the previous value. Key = `derive_key(passphrase)` (SHA-256 with a domain tag). The passphrase is shared: the firmware
gets it at build time (`STATION_PROVISION_KEY`), the brain from `station.provision_key`.

## Boot modes (RAM: the classic ESP32 cannot run Wi-Fi and BLE together)
- **Normal boot** (Wi-Fi settings stored): Wi-Fi only, BLE off and its RAM returned to the heap. UDP frames to the brain.
- **Provisioning boot** (no settings, or BOOT held at power-up, or the one-shot flag set): BLE only, Wi-Fi off, advertising
  `sw-station-<last 4 hex of the station id>`. A valid `config` write is saved to flash and the station reboots
  into a normal boot about 1.5 s later; the BLE link drops then.
- A normal boot whose Wi-Fi fails for 5 minutes (wrong password, AP moved) sets the flag and reboots into provisioning.
  A provisioning boot with settings already stored (button or flag) gives up after 10 minutes and retries Wi-Fi.

## Flow
1. Scan for the service UUID, connect.
2. Read `info` (station id), `status`.
3. Read `challenge`, `seal`, write `config` (with response). Wrong passphrase = the write fails with an ATT error and the challenge rotates.
4. The station reboots; the link drops without a status notification. Success = the station's UDP frames show up at the brain
   (match the station id from `info`), usually within 10-20 s. If they do not within about 60 s the Wi-Fi settings are wrong:
   the station will return to provisioning after 5 minutes.
