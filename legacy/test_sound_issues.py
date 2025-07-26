import numpy as np
#import pyttsx3
import os
import threading
import logging
import time
import board
import neopixel
import pygame


logging.basicConfig(format="%(asctime)s: %(message)s", level=logging.DEBUG)

logging.info("About to import UUID")
time.sleep(1)
from bluepy.btle import UUID
logging.info("imported UID")


# simulated leds
"""
init(autoreset=False)
text = ['o'] * 100
colors = [Back.RED] * len(text)
background_colors = [Back.RED,Back.GREEN,Back.YELLOW,Back.BLUE,Back.MAGENTA,Back.CYAN,Back.WHITE,]
color_txt = text+colors
color_txt[::2] = colors
color_txt[1::2] = text"""

#import vlc
#song = vlc.MediaPlayer("./kavinsky.mp3")
#song.set_time(20000)
#song.play()


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
#engine = pyttsx3.init()
p1 = None


