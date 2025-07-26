from bluepy.btle import UUID, Peripheral
import numpy as np
import os
import logging
import time
import board

logging.basicConfig(format="%(asctime)s: %(message)s", level=logging.DEBUG)


num_devices = 1

dev1 = "58:37:C7:68:4A:32"
dev2 = "C3:96:F9:08:7E:EC"
button_service_uuid = UUID(0x1100)
button_char_uuid    = UUID(0x2803)

p1 = None

def logic_thread():
    global p1
    logging.info("logic thread started")
    try:
        logging.info("about to get Peripheral")
        p1 = Peripheral(dev1, "public")
        logging.info("got Peripheral")
        logging.info("about to get Service1")
        time.sleep(1)
        Service1=p1.getServiceByUUID(button_service_uuid)
        logging.info("got Service1")
        
        logging.info("about to get charactistics")
        time.sleep(1)
        
        ch1 = Service1.getCharacteristics(button_char_uuid)[0]
        logging.info("getCharacteristics")

        logging.info("check for supported read")
        if (ch1.supportsRead()):
            float_array1 = np.frombuffer(ch1.read(), np.float32)
            logging.info("Successfully got data")
        else:
            logging.error("Error. No read")

    except Exception:
        logging.exception("Unexpected error")
        try_disconnect()

def try_disconnect():
    global p1
    try:
        if p1:
            logging.exception("Disconnecting p1")
            p1.disconnect()
    except Exception:
        logging.exception("Failed disconnecting from p1")

        
if __name__ == "__main__":
    logic_thread()
