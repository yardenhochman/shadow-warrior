# Punching Bag Project: CircuitPython & Seeed Studio XIAO Sense

This project uses [CircuitPython](https://circuitpython.org/) on the [Seeed Studio XIAO Sense](https://wiki.seeedstudio.com/XIAO_Sense/) to read IMU and microphone data for a punching bag sensor.

## CircuitPython development

### Install CircuitPython

- Download the latest CircuitPython `.uf2` firmware for XIAO Sense from [circuitpython.org](https://circuitpython.org/board/seeeduino_xiao_sense/).
- Enter bootloader mode: double-tap the reset button.
- Drag and drop the `.uf2` file onto the XIAO's USB drive.

### Set Up Your Development Environment

You can use any editor (I use VSCode with CircuitPython extension).

- Connect your XIAO Sense via USB. It will appear as a drive named `CIRCUITPY`.
- Use any serial modem tool to access the REPL (e.g. minicom, screen)

### Install Required Libraries

- Download the [Adafruit CircuitPython Bundle](https://circuitpython.org/libraries) for the correct CircuitPython version.
- Alternatively, use [Circup](https://circup.org/) to easily install and update libraries:
    - Install Circup: `pip install circup`
    - List available libraries: `circup list`
    - Install libraries (e.g. `adafruit_lsm6ds`): `circup install adafruit_lsm6ds`
- Copy the following libraries to the `lib` folder on your XIAO if not using Circup:
    - `adafruit_lsm6ds`
    - `adafruit_bus_device`
    - Any other dependencies your code requires.

### Upload Your Code

- Save your Python script as `code.py` on the `CIRCUITPY` drive.
- Example code reads IMU and microphone data (see `code.py` in this repo).

## Seeed Studio XIAO specifics

The microphone on the XIAO uses PDM interface. The IMU uses [I2C](https://en.wikipedia.org/wiki/I%C2%B2C)

```python
import board
import audiobusio
from adafruit_lsm6ds.lsm6ds3 import LSM6DS3

mic = audiobusio.PDMIn(board.PDM_CLK, board.PDM_DATA)
imu = LSM6DS3(board.I2C())

while True:
        print("Acceleration:", imu.acceleration)
        print("Gyro:", imu.gyro)
```

### IMU
The XIAO has two I2C buses - one for external devices and one for built in devices (like the IMU). The `board` module exports pins for that I2C interface e.g. `board.IMU_SCL`, `board.IMU_SDA` and `board.IMU_PWR`
Note that the IMU needs to be turned on by setting the digital pin `board.IMU_PWR` to 1:

```python
import board
import digitalio
dpwr = digitalio.DigitalInOut(board.IMU_PWR)
dpwr.direction = digitalio.Direction.OUTPUT
dpwr.value = 1
```

It may be required to `time.sleep(1)` before using the IMU after turning it on

## Resources

- [CircuitPython Docs](https://docs.circuitpython.org/)
- [Seeed Studio XIAO Sense Wiki](https://wiki.seeedstudio.com/XIAO_Sense/)
- [Adafruit CircuitPython Libraries](https://circuitpython.org/libraries)
- [Circup Documentation](https://circup.org/)

---