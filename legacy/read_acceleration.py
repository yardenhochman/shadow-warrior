
from bluepy.btle import UUID, Peripheral
import numpy as np
import pyttsx3
import math
import os
import threading
import logging
import time
import random
import board
import neopixel


logging.basicConfig(format="%(asctime)s: %(message)s", level=logging.INFO)

import vlc
song = vlc.MediaPlayer("./kavinsky.mp3")
song.set_time(20000)

num_devices = 1

dev1 = "58:37:C7:68:4A:32"
dev2 = "C3:96:F9:08:7E:EC"
button_service_uuid = UUID(0x1100)
button_char_uuid    = UUID(0x2803)
a_threshold = 1.2
shout_threshold = 70
breath_audio_threshold = 250
v_cnt_threshold = 5 # TODO revert to 5
a_cnt_threshold = 10 # punches count
game_state = "breath"
engine = pyttsx3.init()
p1 = None
counting_quiet_seconds = 0
MAX_SHOUT_QUIET_TOLERANCE_SECONDS = 8
v_cnt = 0
last_yell_level = 0
current_yell_color = [255,0,0]


#TODO Globals to pass the strength of the punch



# ------ Leds configurations ------ #


LED_COUNT = 7*3*60-1


audio_level = 0
acceleration = 0
MAX_AUDIO_LEVEL = 160
MAX_ACCELERATION_LEVEL = 1.0 # after normalizing it (-1)

# under this audio level is considered silence
MINIMUM_AUDIO_LEVEL_THRESHOULD = 20


def start_music():
    song.play()
    pass

def stop_music():
    song.stop()
    pass


def test_shout_pass():						
    global game_state, a_cnt, v_cnt, end_time, counting_quiet_seconds
    counting_quiet_seconds = time.time()
    logging.debug(f"User is shouting at level: {audio_level}")
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

def flicker(pixels): 
    new_audio_level = audio_level
    new_audio_level = min(audio_level, MAX_AUDIO_LEVEL)
    if new_audio_level <= MINIMUM_AUDIO_LEVEL_THRESHOULD:
        new_audio_level = 0
    
    fastest_flicker = 0.1
    slowest_flicker = 1.0
    delta = slowest_flicker - fastest_flicker
    interval = (1 - new_audio_level / MAX_AUDIO_LEVEL) * delta + fastest_flicker
    
    #on_off = round((time.time() % interval) / interval)
    on_off = 1
    pixels.fill((current_yell_color[0] * on_off,current_yell_color[1] * on_off, current_yell_color[2] * on_off))
    pixels.show()
    logging.debug("flicker " + str(on_off))
    
    # R/G/B to change
    color_index = random.randint(0,2)
    
    # add / remove
    
    sign = 1 if random.randint(0,1) else -1
    current_yell_color[color_index] = current_yell_color[color_index] + 10 * sign
    current_yell_color[color_index] = max(0, min(current_yell_color[color_index], 255))
    

def led_thread():
    
    global game_state, audio_level, pixels1acceleration
    try:
        prev_state = game_state
        logging.info("led thread started")
        pixels1 = neopixel.NeoPixel(board.D21, LED_COUNT, brightness=0.1, auto_write=False)
        #pixels2 = neopixel.NeoPixel(board.D10, LED_COUNT, brightness=0.1, auto_write=False)
        while True:
            flicker(pixels1)
            continue
            
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
                  
                if game_state == "breath":
                    # todo move 5 to const
                    green_level = int(100 * (math.sin(time.time() % 5. * math.pi) + 1) / 2)
                    blue_level = int(255 * (math.sin(time.time() % 5. * math.pi / 5.) + 1) / 2)
                    pixels1.fill((0, green_level,blue_level))                        
                    
                elif game_state == "shout":               
                    pixels1.fill((int(255 * (new_audio_level / MAX_AUDIO_LEVEL)),0,0))
                                                    
                elif game_state == "punch":                                
                    pixels1.fill((int(255 * (new_audio_level / MAX_AUDIO_LEVEL) * 0.5),0, int(255 * (new_acceleration / MAX_ACCELERATION_LEVEL))))
                    
                pixels1.show()
                #pixels2.show()
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
                logging.info("about to get Peripheral")
                p1 = Peripheral(dev1, "public")
                logging.info("got Peripheral")
                break
            except:
                logging.exception("Failed connecting. Trying again")
                time.sleep(3)
        logging.info("about to get Service1")
        Service1=p1.getServiceByUUID(button_service_uuid)
        logging.info("got Service1")
        logging.info("about to get charactistics")
        
        ch1 = Service1.getCharacteristics(button_char_uuid)[0]
        logging.info("getCharacteristics")
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
                acceleration = a1
                audio_level = v1
                logging.debug(f"audio level: {audio_level}")
                
                if game_state == "breath":
                    if audio_level > breath_audio_threshold:
                        game_state = "shout"
                        logging.info("Detected sound, moving to shout")
                    else:
                        logging.debug("Detected sound but not strong enough " + str(audio_level))
                    
                if game_state == "shout":
                    if audio_level < shout_threshold:
                        if time.time() - counting_quiet_seconds > MAX_SHOUT_QUIET_TOLERANCE_SECONDS:
                            game_state = "breath"
                            logging.info("revert to breath")
                            
                    else:
                        test_shout_pass()
                
                if game_state == "punch":
                    if time.time() > end_time:
                        logging.info("User lost due to timeout")
                        stop_music()
                        engine.say("you lose")
                        #TODO loose animation
                    
                        engine.runAndWait()
                        game_state = "shout"
                    # if punch is strong enough
                    elif a1 > a_threshold:
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


def main():
    logging.exception("Starting punching bag")
        
    led_thread_handler = threading.Thread(target=led_thread)
    logic_thread_handler = threading.Thread(target=logic_thread)
    
    led_thread_handler.start()
    logic_thread_handler.start()
    
    
        
if __name__ == "__main__":
    main()
