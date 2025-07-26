import sys
import binascii
import struct
import time
from bluepy.btle import UUID, Peripheral
import numpy as np
import pyttsx3
import os
import vlc
import random
from colorama import init, Back, Fore, Style

init(autoreset=False)
text = ['o'] * 100
colors = [Back.RED] * len(text)
background_colors = [Back.RED,Back.GREEN,Back.YELLOW,Back.BLUE,Back.MAGENTA,Back.CYAN,Back.WHITE,]
color_txt = text+colors
color_txt[::2] = colors
color_txt[1::2] = text

song = vlc.MediaPlayer("./kavinsky.mp3")

os.system("bluetoothctl connect 41:42:19:BD:A0:F6")

num_devices = 1
dev1 = "58:37:C7:68:4A:32"
dev2 = "C3:96:F9:08:7E:EC"
button_service_uuid = UUID(0x1100)
button_char_uuid    = UUID(0x2803)
a_threshold = 1.2
v_threshold = 50
v_cnt_threshold = 5
a_cnt_threshold = 5
game_state = "shout"
engine = pyttsx3.init()

p1 = Peripheral(dev1, "public")
Service1=p1.getServiceByUUID(button_service_uuid)
p2 = Peripheral(dev2, "public")
Service2=p2.getServiceByUUID(button_service_uuid)
print("started")

try:
    ch1 = Service1.getCharacteristics(button_char_uuid)[0]
    ch2 = Service2.getCharacteristics(button_char_uuid)[0]
    v_cnt = 0
    a_cnt = 0

    if (ch1.supportsRead()):
        while 1:
            float_array1 = np.frombuffer(ch1.read(), np.float32)
            x1 = float_array1[1]
            y1 = float_array1[2]
            z1 = float_array1[3]
            v1 = float_array1[4]
            a1 = np.sqrt(x1**2 + y1**2 + z1**2)
            float_array2 = np.frombuffer(ch2.read(), np.float32)
            x2 = float_array2[1]
            y2 = float_array2[2]
            z2 = float_array2[3]
            v2 = float_array2[4]
            a2 = np.sqrt(x2**2 + y2**2 + z2**2)
            a = (a1 + a2)/2
            v = (v1 + v2)/2
            #print("x: {:10.4f}, y: {:10.4f}, z: {:10.4f}, a: {:10.4f}, v: {:10.4f}, ".format(x1, y1, z1, a1, v1))
            if game_state == "punch" and time.time() > end_time:
                song.stop()
                engine.say("you lose")
                engine.runAndWait()
                game_state = "shout"
            if game_state == "shout" and v1 > v_threshold:
                engine.say("shout")
                engine.runAndWait()
                #print("shout!")
                v_cnt = v_cnt + 1
                #print(v_cnt)
                if v_cnt > v_cnt_threshold:
                    game_state = "punch"
                    engine.say("level 2")
                    engine.runAndWait()
                    song.play()
                    song.set_time(20000)
                    a_cnt = 0
                    start_time = time.time() 
                    end_time = start_time + 60
            if a1 > a_threshold:
                #print("punch!")
                engine.say("punch")
                engine.runAndWait()
                a_cnt = a_cnt + 1
                for i in range(len(text)):
                    color_txt[i*2] = random.choice(background_colors)
                print(''.join(color_txt), end="\r")
                if a_cnt > a_cnt_threshold:
                    song.stop()
                    #print("you win")
                    engine.say("you win")
                    engine.runAndWait()
                    game_state = "shout"
                    v_cnt = 0
                    a_cnt = 0
    else:
        print("error! no read")

except Exception as e:
    print("exception")
    print(e)
    pass

finally:
    p1.disconnect()
