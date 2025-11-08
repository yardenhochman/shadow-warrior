
# boot.py

import network
import esp
import bluetooth
from micropython import const
DEVICE_NAME = const("ShadowLED")

network.hostname(DEVICE_NAME)
ble = bluetooth.BLE()
ble.active(True)
ble.config(gap_name=DEVICE_NAME)

esp.osdebug(None)  # disable vendor O/S debugging messages

def connect_wifi(ssid, password):
    sta_if = network.WLAN(network.STA_IF)
    if not sta_if.isconnected():
        print('connecting to network...')
        sta_if.active(True)
        sta_if.connect(ssid, password)
        while not sta_if.isconnected():
            pass
    print('network config:', sta_if.ifconfig())

# Uncomment and set your WiFi credentials if you want to use WiFi
# connect_wifi("your_ssid", "your_password")

