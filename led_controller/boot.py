
# boot.py

import network
import esp
import time
from micropython import const

DEVICE_NAME = const("ShadowLED")

network.hostname(DEVICE_NAME)

esp.osdebug(None)  # disable vendor O/S debugging messages

def connect_wifi(ssid, password, timeout=10):
    """Connect to WiFi with timeout to prevent indefinite blocking.

    Args:
        ssid: WiFi network name
        password: WiFi password
        timeout: Maximum seconds to wait for connection (default: 10)

    Returns:
        True if connected, False if timeout or error occurred
    """
    sta_if = network.WLAN(network.STA_IF)
    if sta_if.isconnected():
        print('Already connected to WiFi')
        print('network config:', sta_if.ifconfig())
        return True

    try:
        print('Connecting to network...')
        sta_if.active(True)
        sta_if.connect(ssid, password)

        start_time = time.time()
        while not sta_if.isconnected():
            if time.time() - start_time > timeout:
                print('WiFi connection timeout after %d seconds' % timeout)
                sta_if.active(False)
                return False
            time.sleep(0.1)

        print('Connected to WiFi')
        print('network config:', sta_if.ifconfig())
        return True
    except Exception as e:
        print('WiFi connection error:', str(e))
        sta_if.active(False)
        return False

# Connect to WiFi
connect_wifi("super_skunk", "0547407479")

