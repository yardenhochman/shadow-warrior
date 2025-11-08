
# main.py

import bluetooth
import network
import machine
import neopixel
import asyncio
import aioble
import math
import time
from micropython import const

try:
    from web_api import init_web_api, app
    WEB_API_AVAILABLE = True
except ImportError:
    WEB_API_AVAILABLE = False
    print("Warning: web_api module not found, HTTP control disabled")
    app = None

DEVICE_NAME = const("ShadowLED")


class Mode:
    IDLE = 0
    ENERGY_BAR = 1
    ENERGY_PULSE = 2
    BREATHING = 3

_mode = Mode.IDLE


# LED strip configuration
LED_PIN = 26
LED_COUNT = 180

np = neopixel.NeoPixel(machine.Pin(LED_PIN, machine.Pin.OUT), LED_COUNT)

# BLE configuration using aioble
_UART_UUID = bluetooth.UUID("d08d81bb-7270-45de-a475-5b52feb820b6")
_UART_TX_UUID = bluetooth.UUID("6E400003-B5A3-F393-E0A9-E50E24DCCA9E")  # Read/Notify from device
_UART_RX_UUID = bluetooth.UUID("8f97424f-8c2f-4a86-9e53-92059ccb1559")  # Write to device
_IP_ADDR_UUID = bluetooth.UUID("00000001-0000-1000-8000-00805f9b34fb")  # IP address characteristic

ADV_INTERVAL_US = 250_000  # Advertising interval in microseconds

FRAME_RATE=60  # 60 FPS
FRAME_DURATION_US = int(1_000_000 / FRAME_RATE)
FRAME_DURATION_MS = 1_000 / FRAME_RATE

# Create UART service first
uart_service = aioble.Service(_UART_UUID)

# Characteristic property flags:
# 0x01 = READ, 0x02 = WRITE, 0x08 = NOTIFY

# RX characteristic: write-only (device receives commands)
# Arguments: service, uuid, properties
led_char = aioble.Characteristic(
    uart_service,
    _UART_RX_UUID,
    write=True,
    capture=True

)

# IP address characteristic: read-only
ip_addr_char = aioble.Characteristic(
    uart_service,
    _IP_ADDR_UUID,
    read=True,
    initial=b"0.0.0.0"
)

aioble.register_services(uart_service)


def easing_quad(current_frame: int, total_frames: int) -> float:
    """
    Calculates the eased progress percentage (0.0 to 1.0) for the current frame.
    """
    # Normalize current_frame to a value between 0.0 and 1.0
    normalized_t = current_frame / total_frames
    # Apply the quadratic ease-in function
    eased_progress = normalized_t ** 2 
    return eased_progress


# Lighting effects
async def energy_bar(percentage, duration=0.5):
    """Displays a bar of light corresponding to the given percentage."""
    global _mode

    frames = max(1, int(FRAME_RATE * duration))
    _mode = Mode.ENERGY_BAR
    num_leds = int(LED_COUNT * percentage / 100)

    np.fill((0, 0, 0))  # Clear all LEDs
    # Gradient from white (start) to red (end)
    _n_leds = max(1, num_leds - 1)
    led_colors = [
        (
            255,
            int(255 * (1 - i / _n_leds)),
            int(255 * (1 - i / _n_leds)),
        )
        for i in range(num_leds)
    ]
    step_frames = frames // 2
    for i in range(step_frames + 1):
        _start = time.ticks_us()
        x = easing_quad(i, step_frames)
        led = int(num_leds * x)
        for x in range(led):
            np[x] = led_colors[x]
        np.write()
        lag = time.ticks_diff(time.ticks_us(), _start)
        await asyncio.sleep((FRAME_DURATION_MS - lag/1000)/1000)

    for i in range(step_frames + 1):
        _start = time.ticks_us()
        led = num_leds - int(num_leds * easing_quad(i, step_frames))
        for x in range(num_leds - 1, led - 1, -1):
            np[x] = (0, 0, 0)
        np.write()
        lag = time.ticks_diff(time.ticks_us(), _start)
        await asyncio.sleep((FRAME_DURATION_MS - lag/1000)/1000)



async def energy_pulse():
    """Sends a pulse of light down the strip."""
    global _mode
    _mode = Mode.ENERGY_PULSE
    np.fill((0, 0, 0))
    for i in range(LED_COUNT):
        np[i] = (255, 0, 0)
        np.write()
        await asyncio.sleep_ms(20)
        np[i] = (0, 0, 0)
    np.write()


async def breathing_effect(max_brightness=160):
    """A gentle breathing effect."""
    global _mode
    _mode = Mode.BREATHING
    np.fill((0, 0, 0))
    while _mode == Mode.BREATHING:
        cycle_seconds = 5
        frames_per_cycle = max(1, int(FRAME_RATE * cycle_seconds))

        for frame in range(frames_per_cycle):
            _start = time.ticks_us()
            if _mode != Mode.BREATHING:
                break
            t = frame / frames_per_cycle
            envelope = (math.sin(2 * math.pi * t) + 1) / 2  # 0..1 sinusoid
            brightness = int(max_brightness * envelope)
            np.fill((brightness, 0, 0))
            np.write()
            lag = time.ticks_diff(time.ticks_us(), _start)
            await asyncio.sleep((FRAME_DURATION_MS - lag/1000)/1000)


async def idle_effect():
    """Stop all effects and clear LEDs."""
    global _mode
    _mode = Mode.IDLE
    np.fill((0, 0, 0))
    np.write()


async def handle_ble_command(command):
    """Handles commands received via BLE."""
    parts = command.decode().strip().split()
    if not parts:
        return

    cmd = parts[0].lower()
    if cmd == "energy_bar" and len(parts) > 1:
        try:
            percentage = int(parts[1])
            await energy_bar(percentage)
        except ValueError:
            print("Invalid percentage for energy_bar")
    elif cmd == "energy_pulse":
        await energy_pulse()
    elif cmd == "breath":
        await breathing_effect()
    elif cmd == "idle":
        await idle_effect()
    else:
        print("Unknown command:", command)




async def peripheral_task():
    print("Starting BLE advertising...")
    # Build advertisement data

    # Keep the task running
    while True:
        conn = await aioble.advertise(
            interval_us=ADV_INTERVAL_US,
            name=DEVICE_NAME, services=[_UART_UUID],
        )
        if conn:
            print("Connected:", conn.device)
            async def on_disconnect():
                await conn.disconnected()
                print("Disconnected:", conn.device)

            asyncio.create_task(on_disconnect())

async def control_task():
    while True:
        conn, data = await led_char.written()
        print("Command from BLE:", conn.device, data)
        asyncio.create_task(handle_ble_command(data))


def get_ip_address():
    """Get the device's IP address from WiFi or return 0.0.0.0 if not connected."""
    try:
        sta_if = network.WLAN(network.STA_IF)
        if sta_if.isconnected():
            ip = sta_if.ifconfig()[0]
            return ip
    except Exception as e:
        print("Error getting IP address:", str(e))
    return "0.0.0.0"


def update_ip_characteristic():
    """Update the BLE IP address characteristic with current IP."""
    ip = get_ip_address()
    try:
        ip_addr_char.write(ip.encode())
    except Exception as e:
        print("Error updating IP characteristic:", str(e))


async def main():
    """Main async function using aioble."""
    print("Running tasks...")
    # Initialize BLE
    aioble.config(gap_name=DEVICE_NAME)

    # Get and display IP address
    ip_address = get_ip_address()
    print("Device IP address:", ip_address)
    update_ip_characteristic()

    tasks = [
        asyncio.create_task(peripheral_task()),
        asyncio.create_task(control_task()),
    ]

    # Initialize and start web API if available
    if WEB_API_AVAILABLE:
        try:
            effect_functions = {
                'energy_bar': energy_bar,
                'energy_pulse': energy_pulse,
                'breathing': breathing_effect,
                'idle': idle_effect,
            }
            init_web_api(Mode, effect_functions)
            tasks.append(asyncio.create_task(app.start_server(host='0.0.0.0', port=80, debug=True)))
            print("Web API enabled on port 80")
        except Exception as e:
            print("Failed to start web API:", str(e))

    await asyncio.gather(*tasks)



def main_sync():
    """Synchronous wrapper to run the async main function."""
    asyncio.run(main())


if __name__ == "__main__":
    main_sync()
