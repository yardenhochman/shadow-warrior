
from bluepy.btle import UUID, Peripheral
import numpy as np
import pyttsx3
import os
import threading
import logging
import time
import board
import neopixel


logging.basicConfig(format="%(asctime)s: %(message)s", level=logging.DEBUG)



# simulated leds
"""
init(autoreset=False)
text = ['o'] * 100
colors = [Back.RED] * len(text)
background_colors = [Back.RED,Back.GREEN,Back.YELLOW,Back.BLUE,Back.MAGENTA,Back.CYAN,Back.WHITE,]
color_txt = text+colors
color_txt[::2] = colors
color_txt[1::2] = text"""

import vlc
song = vlc.MediaPlayer("./kavinsky.mp3")
song.set_time(20000)
song.play()


#os.system("bluetoothctl connect 41:42:19:BD:A0:F6")

num_devices = 1

dev1 = "58:37:C7:68:4A:32"
dev2 = "C3:96:F9:08:7E:EC"
button_service_uuid = UUID(0x1100)
button_char_uuid    = UUID(0x2803)
a_threshold = 1.2
v_threshold = 50
v_cnt_threshold = 20 # TODO revert to 5
a_cnt_threshold = 40 # punches count
game_state = "shout"
engine = pyttsx3.init()
p1 = None



#TODO Globals to pass the strength of the punch



# ------ Leds configurations ------ #


LED_COUNT = 5*60-1


start_time = time.time()
audio_level = 0
acceleration = 0
MAX_AUDIO_LEVEL = 160
MAX_ACCELERATION_LEVEL = 1.0 # after normalizing it (-1)

# under this audio level is considered silence
MINIMUM_AUDIO_LEVEL_THRESHOULD = 20


def start_music():
    #pygame.mixer.music.play()
    pass

def stop_music():
    #pygame.mixer.music.stop()
    pass


def led_thread():
    global game_state, audio_level, pixels1acceleration
    try:
        prev_state = game_state
        logging.info("led thread started")
        pixels1 = neopixel.NeoPixel(board.D18, LED_COUNT, brightness=0.1, auto_write=False)
        while True:
            try:
                #continue
                # clip function 
                new_audio_level = audio_level
                new_audio_level = min(audio_level, MAX_AUDIO_LEVEL)
                if new_audio_level <= MINIMUM_AUDIO_LEVEL_THRESHOULD:
                    new_audio_level = 0
                    
                # normalize acceleration
                new_acceleration = acceleration - 1
                new_acceleration = max(new_acceleration, 0)
                new_acceleration = min(new_acceleration, MAX_ACCELERATION_LEVEL)
                                                
                if game_state == "shout":               
                    pixels1.fill((int(255 * (new_audio_level / MAX_AUDIO_LEVEL)),0,0))
                                                    
                elif game_state == "punch":                                
                    pixels1.fill((int(255 * (new_audio_level / MAX_AUDIO_LEVEL) * 0.5),0, int(255 * (new_acceleration / MAX_ACCELERATION_LEVEL))))
                    
                pixels1.show()
            except:
                logging.exception("Failed setting leds")
        prev_state = game_state
    except:
        logging.exception("Error at leds thread")



def logic_thread():
    global game_state, p1, audio_level, acceleration
    logging.info("logic thread started")
    try:
        while True:
            try:
                #time.sleep(1)
                logging.info("about to get Peripheral")
                p1 = Peripheral(dev1, "public")
                logging.info("got Peripheral")
                break
            except:
                logging.exception("Failed connecting. Trying again")
                time.sleep(3)
        logging.info("about to get Service1")
        #time.sleep(1)
        Service1=p1.getServiceByUUID(button_service_uuid)
        logging.info("got Service1")
        #p2 = Peripheral(dev2, "public")
        #Service2=p2.getServiceByUUID(button_service_uuid)
        
        logging.info("about to get charactistics")
        #time.sleep(1)
        
        ch1 = Service1.getCharacteristics(button_char_uuid)[0]
        logging.info("getCharacteristics")
        #ch2 = Service2.getCharacteristics(button_char_uuid)[0]
        v_cnt = 0
        a_cnt = 0

        logging.info("check for supported read")
        if (ch1.supportsRead()):
            logging.info("supported read")
            while True:
                float_array1 = np.frombuffer(ch1.read(), np.float32)
                x1 = float_array1[1]
                y1 = float_array1[2]
                z1 = float_array1[3]
                v1 = float_array1[4]
                a1 = np.sqrt(x1**2 + y1**2 + z1**2)
                """float_array2 = np.frombuffer(ch2.read(), np.float32)
                x2 = float_array2[1]
                y2 = float_array2[2]
                z2 = float_array2[3]
                v2 = float_array2[4]
                a2 = np.sqrt(x2**2 + y2**2 + z2**2)"""
                #TODO set only when needed
                acceleration = a1 # (a1 + a2)/2
                audio_level = v1 #(v1 + v2)/2
                logging.info(f"audio level: {audio_level}")
                
                #print("x: {:10.4f}, y: {:10.4f}, z: {:10.4f}, a: {:10.4f}, v: {:10.4f}, ".format(x1, y1, z1, a1, v1))
                if game_state == "punch" and time.time() > end_time:
                    logging.info("User lost due to timeout")
                    stop_music()
                    engine.say("you lose")
                    #TODO loose animation
                    
                    engine.runAndWait()
                    game_state = "shout"
                # v1 is volume user is showint
                if game_state == "shout" and v1 > v_threshold:
                    logging.info(f"User is shouting at level: {audio_level}")
                    engine.say("shout")
                    engine.runAndWait()
                    #print("shout!")
                    v_cnt = v_cnt + 1
                    #print(v_cnt)
                    # if yelled enough times. Next level is now punch
                    if v_cnt > v_cnt_threshold:
                        logging.info("User shouted enough")
                        game_state = "punch"
                        logging.info("level 2")
                        engine.say("level 2")
                        engine.runAndWait()
                        start_music()
                        a_cnt = 0
                        start_time = time.time() 
                        end_time = start_time + 60


                # if punch is strong enough
                if a1 > a_threshold:
                    logging.info("Punch")
                    engine.say("punch")
                    engine.runAndWait()
                    a_cnt = a_cnt + 1

                    # if you have enought punches - finish game with a win
                    if a_cnt > a_cnt_threshold:
                        logging.info("User won")
                        #TODO win animation
                        
                        stop_music()
                        #print("you win")
                        engine.say("you win")
                        engine.runAndWait()
                        game_state = "shout"
                        v_cnt = 0
                        a_cnt = 0
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
        
    """try:
        if p2:
            p2.disconnect()
    except Exception:
        logging.exception("Failed disconnecting from p2")"""


def main():
    logging.exception("Starting punching bag")
        
    led_thread_handler = threading.Thread(target=led_thread)
    logic_thread_handler = threading.Thread(target=logic_thread)
    
    led_thread_handler.start()
    logic_thread_handler.start()
    
    
        
if __name__ == "__main__":
    main()
