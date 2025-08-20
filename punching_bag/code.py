import board
import digitalio
import time
import audiobusio
import math
import array
import struct
from microcontroller import Pin
import busio
from adafruit_lsm6ds.lsm6ds3 import LSM6DS3
import adafruit_ble
from adafruit_ble.advertising.standard import Advertisement
from adafruit_ble.services.standard.device_info import DeviceInfoService
from adafruit_ble.services import Service
from adafruit_ble.characteristics import Characteristic


IMU_I2C_ADDRESS = 0x6A

# Custom BLE Service for Shadow Warrior IMU data
# Service UUID: 6E400001-B5A3-F393-E0A9-E50E24DCCA9E
# Acceleration Characteristic UUID: 6E400002-B5A3-F393-E0A9-E50E24DCCA9E  
# Gyroscope Characteristic UUID: 6E400003-B5A3-F393-E0A9-E50E24DCCA9E

from adafruit_ble.uuid import VendorUUID

# Define custom UUIDs for Shadow Warrior service
SW_SERVICE_UUID = VendorUUID("6E400001-B5A3-F393-E0A9-E50E24DCCA9E")
SW_ACCEL_CHAR_UUID = VendorUUID("6E400002-B5A3-F393-E0A9-E50E24DCCA9E")
SW_GYRO_CHAR_UUID = VendorUUID("6E400003-B5A3-F393-E0A9-E50E24DCCA9E")

class ShadowWarriorService(Service):
    uuid = SW_SERVICE_UUID
    
    acceleration = Characteristic(
        uuid=SW_ACCEL_CHAR_UUID,
        properties=Characteristic.READ | Characteristic.NOTIFY,
        max_length=12  # 3 floats * 4 bytes each
    )
    
    gyroscope = Characteristic(
        uuid=SW_GYRO_CHAR_UUID,
        properties=Characteristic.READ | Characteristic.NOTIFY,
        max_length=12  # 3 floats * 4 bytes each
    )



class IMU(LSM6DS3):
    def __init__(self):
        self._dpwr = digitalio.DigitalInOut(board.IMU_PWR)
        self._dpwr.direction = digitalio.Direction.OUTPUT
        self.turn_on()
        i2c = busio.I2C(board.IMU_SCL, board.IMU_SDA)
        super().__init__(i2c)
    
    def turn_on(self):
        self._dpwr.value = True
        time.sleep(0.1)

    def turn_off(self):
        self._dpwr.value = False
        



# Remove DC bias before computing RMS.
def mean(values):
    return sum(values) / len(values)


def normalized_rms(values):
    minbuf = int(mean(values))
    samples_sum = sum(
        float(sample - minbuf) * (sample - minbuf)
        for sample in values
    )

    return math.sqrt(samples_sum / len(values))




def is_hardware_PDM(clock, data):
    try:
        p = audiobusio.PDMIn(clock, data)
        p.deinit()
        return True
    except ValueError:
        return False
    except RuntimeError:
        return True


def get_unique_pins():
    exclude = ['NEOPIXEL', 'APA102_MOSI', 'APA102_SCK']
    pins = [pin for pin in [
        getattr(board, p) for p in dir(board) if p not in exclude]
            if isinstance(pin, Pin)]
    unique = []
    for p in pins:
        if p not in unique:
            unique.append(p)
    return unique


# for clock_pin in get_unique_pins():
#     for data_pin in get_unique_pins():
#         if clock_pin is data_pin:
#             continue
#         if is_hardware_PDM(clock_pin, data_pin):
#             print("Clock pin:", clock_pin, "\t Data pin:", data_pin)
#         else:
#             print("Not hardware PDM:", clock_pin, "\t", data_pin)


print("Loading...")


mic = audiobusio.PDMIn(board.PDM_CLK, board.PDM_DATA, sample_rate=16000, bit_depth=16, startup_delay=0.1)
samples = array.array('H', [0] * 1000)

imu = IMU()

def read_imu(imu):

    while True:
        print("Acceleration: X:%.2f, Y: %.2f, Z: %.2f m/s^2" % (imu.acceleration))
        print("Gyro X:%.2f, Y: %.2f, Z: %.2f radians/s" % (imu.gyro))
        print("")
        time.sleep(0.5)



def read_mic(mic):
    while True:
        mic.record(samples, len(samples))
        # print("Samples:", len(samples))
        # print(samples)  # Print first 10 samples for debugging
        magnitude = normalized_rms(samples)
        print((magnitude,))
        print("Max:", max(samples), "Min:", min(samples), "Mean:", mean(samples))
        time.sleep(0.1)


# Helper functions to pack IMU data into bytes
def pack_acceleration(accel_data):
    """Pack acceleration data (x, y, z) into 12 bytes (3 floats)"""
    return struct.pack('<fff', accel_data[0], accel_data[1], accel_data[2])

def pack_gyroscope(gyro_data):
    """Pack gyroscope data (x, y, z) into 12 bytes (3 floats)"""
    return struct.pack('<fff', gyro_data[0], gyro_data[1], gyro_data[2])

# Initialize BLE
radio = adafruit_ble.BLERadio()
radio.name = "ShadowWarrior"

# Create services - these will be available when a client connects
device_info = DeviceInfoService(manufacturer="shadow-warrior", model_number="xiao-sense-v1")
shadow_warrior_service = ShadowWarriorService()

# Start advertising
advertisement = Advertisement()
advertisement.connectable = True
advertisement.complete_name = "ShadowWarrior"
radio.start_advertising(advertisement)

print("Advertising as ShadowWarrior...")

# Main loop
while True:
    # Wait for connection
    while not radio.connected:
        pass
    
    print("Connected!")
    
    # Connection loop - send IMU data updates
    while radio.connected:
        try:
            # Read IMU data
            accel_data = imu.acceleration
            gyro_data = imu.gyro
            
            # Pack data into bytes for BLE transmission
            accel_bytes = pack_acceleration(accel_data)
            gyro_bytes = pack_gyroscope(gyro_data)
            
            # Update characteristics with new data (this sends notifications to clients)
            shadow_warrior_service.acceleration = accel_bytes
            shadow_warrior_service.gyroscope = gyro_bytes
            
            # Print data for debugging
            print(f"Accel: X:{accel_data[0]:.2f}, Y:{accel_data[1]:.2f}, Z:{accel_data[2]:.2f} m/s²")
            print(f"Gyro: X:{gyro_data[0]:.2f}, Y:{gyro_data[1]:.2f}, Z:{gyro_data[2]:.2f} rad/s")
                        
            time.sleep(0.1)  # 10Hz update rate
            
        except Exception as e:
            print(f"Error in main loop: {e}")
            break
    
    print("Disconnected!")
    
    # Restart advertising
    radio.start_advertising(advertisement)