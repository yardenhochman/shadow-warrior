import board
import digitalio
import time
import audiobusio
import math
import array
import struct
import alarm
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
SW_ALPHA_CHAR_UUID = VendorUUID("6E400004-B5A3-F393-E0A9-E50E24DCCA9E")
SW_THRESHOLD_CHAR_UUID = VendorUUID("6E400005-B5A3-F393-E0A9-E50E24DCCA9E")

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
    
    alpha = Characteristic(
        uuid=SW_ALPHA_CHAR_UUID,
        properties=Characteristic.READ | Characteristic.WRITE,
        max_length=4  # 1 float * 4 bytes
    )
    
    threshold = Characteristic(
        uuid=SW_THRESHOLD_CHAR_UUID,
        properties=Characteristic.READ | Characteristic.WRITE,
        max_length=4  # 1 float * 4 bytes
    )



class IMU(LSM6DS3):
    def __init__(self):
        self._dpwr = digitalio.DigitalInOut(board.IMU_PWR)
        self._dpwr.direction = digitalio.Direction.OUTPUT
        self._i2c = None
        self._initialized = False
        self.turn_off()  # Start with IMU off to save power
    
    def turn_on(self):
        self._dpwr.value = True
        time.sleep(0.1)  # Wait for IMU to power up
        if not self._initialized:
            self._i2c = busio.I2C(board.IMU_SCL, board.IMU_SDA)
            super().__init__(self._i2c)
            self._initialized = True

    def turn_off(self):
        self._dpwr.value = False
        if self._i2c:
            self._i2c.deinit()
            self._i2c = None
        self._initialized = False
        



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


def exponential_moving_average(alpha, new_value, previous_ema):
    """
    Calculate exponential moving average.
    
    Args:
        alpha: Smoothing factor (0 < alpha <= 1)
               Higher alpha = more responsive to recent changes
               Lower alpha = more smoothing
        new_value: The new data point
        previous_ema: The previous EMA value
    
    Returns:
        Updated EMA value
    """
    return alpha * new_value + (1 - alpha) * previous_ema




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

acceleration_alpha = 0.8  # EWMA factor for acceleration smoothing
acceleration_threshold = 10  # Threshold for acceleration detection

# Initialize characteristics with default values
shadow_warrior_service.alpha = struct.pack('<f', acceleration_alpha)
shadow_warrior_service.threshold = struct.pack('<f', acceleration_threshold)

# Power management and sleep configuration
SLEEP_DURATION = 2.0  # Sleep for 2 seconds between advertising cycles
ADVERTISING_DURATION = 0.5  # Advertise for 0.5 seconds before sleeping

print("Starting power-optimized main loop...")

# Main loop
while True:
    # Power-saving advertising loop - IMU is off during this phase
    while not radio.connected:
        print("Advertising... (IMU off)")
        
        # Advertise for a short period
        start_time = time.monotonic()
        while time.monotonic() - start_time < ADVERTISING_DURATION:
            if radio.connected:
                break
            time.sleep(0.1)  # Small delay to check for connections
        
        # If still not connected, go to light sleep to save power
        if not radio.connected:
            print("Entering light sleep for power saving...")
            
            # Create time alarm for periodic wake-up
            time_alarm = alarm.time.TimeAlarm(monotonic_time=time.monotonic() + SLEEP_DURATION)
            
            # Enter light sleep (BLE advertising continues automatically)
            alarm.light_sleep_until_alarms(time_alarm)
            
            # Wake up and continue advertising cycle
            print("Woke up from light sleep")
    
    print("Connected! Turning on IMU...")
    
    # Turn on IMU when client connects
    imu.turn_on()
    
    # Connection loop - send IMU data updates
    while radio.connected:
        acceleration = 0
        try:
            # Check for parameter updates from BLE client
            if shadow_warrior_service.alpha:
                updated_alpha = struct.unpack('<f', shadow_warrior_service.alpha)[0]
                if updated_alpha != acceleration_alpha:
                    acceleration_alpha = updated_alpha
                    print(f"Alpha updated to: {acceleration_alpha}")

            if shadow_warrior_service.threshold:
                updated_threshold = struct.unpack('<f', shadow_warrior_service.threshold)[0]
                if updated_threshold != acceleration_threshold:
                    acceleration_threshold = updated_threshold
                    print(f"Threshold updated to: {acceleration_threshold}")
                            
            accel_data = imu.acceleration
            gyro_data = imu.gyro
            
            acceleration_current = math.sqrt(imu.acceleration[0]**2 + imu.acceleration[1]**2 + imu.acceleration[2]**2)
            acceleration = exponential_moving_average(acceleration_alpha, acceleration_current, acceleration)

            if acceleration > acceleration_threshold:
                # Pack data into bytes for BLE transmission
                accel_bytes = struct.pack('<f', acceleration)
                
                # Update characteristics with new data (this sends notifications to clients)
                shadow_warrior_service.acceleration = accel_bytes
                print(f"Acceleration: {acceleration}")

                # Print data for debugging
                print(f"Accel: X:{accel_data[0]:.2f}, Y:{accel_data[1]:.2f}, Z:{accel_data[2]:.2f} m/s²")
                print(f"Gyro: X:{gyro_data[0]:.2f}, Y:{gyro_data[1]:.2f}, Z:{gyro_data[2]:.2f} rad/s")
            
            time.sleep(0.1)  # 10Hz update rate
            
        except Exception as e:
            print(f"Error in main loop: {e}")
            break
    
    print("Disconnected! Turning off IMU to save power...")
    
    # Turn off IMU to save power when disconnected
    imu.turn_off()
    
    # Restart advertising
    radio.start_advertising(advertisement)