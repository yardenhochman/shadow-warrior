import matplotlib.pyplot as plt
import matplotlib.patches as patches
import numpy as np
import colorsys
from matplotlib.animation import FuncAnimation
import math
import time
import random
from collections import deque
from dataclasses import dataclass
from enum import Enum

import numpy as np
# Force an interactive GUI backend (helps on macOS where default can be non-interactive)
import matplotlib

class Mode(Enum):
    IDLE = 0
    VOICE = 1
    FIGHT = 2

current_mode = Mode.IDLE

@dataclass
class Config:
    fps: int = 50
    gamma: float = 2.2
    global_brightness: float = 0.9  # master dimmer

    # Layout: total LEDs and named segments
    poles: int = 6
    leds_per_pole: int = 40
    entry_beacon_leds: int = 12  # small "traffic light" icon near entry
    bag_spot_leds: int = 10      # spotlight ring/narrow strip above bag

    # Game/scoring
    voice_window_sec: float = 10.0
    score_decay_per_sec: float = 0.0  # already handled by window; extra decay if you want
    score_threshold: float = 9000.0
    min_fight_seconds: float = 35.0
    max_fight_seconds: float = 90.0

    # Palettes (HSV base or direct RGB)
    sad_hsv: tuple = (250/360.0, 0.65, 0.7)  # blue-violet
    alert_hsv: tuple = (0/360.0, 1.0, 1.0)   # red
    action_hsv: tuple = (25/360.0, 1.0, 1.0) # orange

    # Breathing
    breathe_speed: float = 0.06
    breathe_min: float = 0.08
    breathe_max: float = 0.45

    # Voice-react
    voice_strobe_min_interval: float = 0.06
    voice_strobe_max_interval: float = 0.18

    # Ripple on punch
    ripple_speed_leds_per_sec: float = 120.0
    ripple_width_leds: int = 6


def hsv_to_rgb(h, s, v):
    r, g, b = colorsys.hsv_to_rgb(h, s, v)
    return np.array([r, g, b], dtype=np.float32)


def apply_gamma(rgb, gamma):
    return np.power(np.clip(rgb, 0.0, 1.0), 1.0 / gamma)




try:
    matplotlib.use('TkAgg')  # try Tk first (usually available)
except Exception:
    pass

class LEDStripSimulator:
    def __init__(self, num_leds=50, led_size=20, spacing=5):
        self.num_leds = num_leds
        self.led_size = led_size
        self.spacing = spacing
        self.pixels = np.zeros((num_leds, 3))  # RGB values 0-255

        # Setup the plot
        self.fig, self.ax = plt.subplots(figsize=(16, 3))
        self.ax.set_xlim(-spacing, num_leds * (led_size + spacing))
        self.ax.set_ylim(-led_size//2, led_size + led_size//2)
        self.ax.set_aspect('equal')
        self.ax.axis('off')
        self.ax.set_facecolor('black')
        self.mode_text = self.fig.text(
            0.5, 0.95, "Mode: IDLE",  # inside the figure, top area
            ha="center", va="top",
            color="black", fontsize=14
        )
        # Create LED circles
        self.led_circles = []
        for i in range(num_leds):
            x = i * (led_size + spacing) + led_size // 2
            y = led_size // 2
            circle = patches.Circle((x, y), led_size // 2, 
                                  facecolor='black', edgecolor='gray', linewidth=1)
            self.ax.add_patch(circle)
            self.led_circles.append(circle)
        
        # self.fig.suptitle(f'LED Strip Simulator - {num_leds} LEDs', color='black')
        self.fig.subplots_adjust(top=0.88)
    
    def set_mode_label(self, mode: Mode):
        self.mode_text.set_text(f"Mode: {mode.name}")

    def set_pixel(self, index, r, g, b):
        """Set a single pixel color (r, g, b from 0-255)"""
        if 0 <= index < self.num_leds:
            self.pixels[index] = [r, g, b]
            color = (r/255.0, g/255.0, b/255.0)
            self.led_circles[index].set_facecolor(color)
    
    def set_pixel_hsv(self, index, h, s, v):
        """Set pixel using HSV (h: 0-360, s,v: 0-1)"""
        r, g, b = colorsys.hsv_to_rgb(h/360.0, s, v)
        self.set_pixel(index, int(r*255), int(g*255), int(b*255))
    
    def fill_solid(self, r, g, b):
        """Fill all LEDs with solid color"""
        for i in range(self.num_leds):
            self.set_pixel(i, r, g, b)
    
    def clear(self):
        """Turn off all LEDs"""
        self.fill_solid(0, 0, 0)
    
    def show(self, delay=0.05):
        """Update display"""
        plt.draw()
        plt.pause(delay)
    
    def rainbow_fill(self, hue_start=0, hue_step=7):
        """Fill strip with rainbow colors"""
        for i in range(self.num_leds):
            hue = (hue_start + i * hue_step) % 360
            self.set_pixel_hsv(i, hue, 1.0, 1.0)

# LED Effects Library
class LEDEffects:
    def __init__(self, simulator):
        self.sim = simulator
        self.frame = 0
        
    def rainbow_wave(self, speed=5):
        """Moving rainbow wave"""
        for i in range(self.sim.num_leds):
            hue = (self.frame * speed + i * 10) % 360
            self.sim.set_pixel_hsv(i, hue, 1.0, 1.0)
        self.frame += 1
        
    def breathing(self, color=(0, 100, 255), speed=0.05):
        """Breathing effect with specified color"""
        brightness = (np.sin(self.frame * speed) + 1) / 2
        r, g, b = [int(c * brightness) for c in color]
        self.sim.fill_solid(r, g, b)
        self.frame += 1
        
    def fire_effect(self):
        """Flickering fire effect"""
        for i in range(self.sim.num_leds):
            # Random flicker
            flicker = np.random.random() * 0.5 + 0.5
            # Fire colors (red to yellow)
            hue = np.random.randint(0, 60)  # Red to yellow range
            sat = 1.0
            val = flicker * (0.7 + 0.3 * np.random.random())
            self.sim.set_pixel_hsv(i, hue, sat, val)
            
    def scanner(self, color=(255, 0, 0), width=3):
        """KITT scanner effect"""
        self.sim.clear()
        pos = int(abs(np.sin(self.frame * 0.1)) * (self.sim.num_leds - width))
        
        for i in range(width):
            if pos + i < self.sim.num_leds:
                brightness = 1.0 - (i / width) * 0.7
                r, g, b = [int(c * brightness) for c in color]
                self.sim.set_pixel(pos + i, r, g, b)
        self.frame += 1
        
    def wave(self, color=(0, 255, 255), length=10):
        """Sine wave effect"""
        for i in range(self.sim.num_leds):
            brightness = (np.sin(2 * np.pi * i / length + self.frame * 0.2) + 1) / 2
            r, g, b = [int(c * brightness) for c in color]
            self.sim.set_pixel(i, r, g, b)
        self.frame += 1

# Demo Usage
if __name__ == "__main__":
    # Create simulator
    sim = LEDStripSimulator(num_leds=60)
    effects = LEDEffects(sim)
    
    # Show the simulator
    plt.ion()  # Interactive mode
    plt.show()

    sim.set_mode_label(current_mode)

    def on_key(event):
        global current_mode
        if event.key == '1':
            current_mode = Mode.IDLE
        elif event.key == '2':
            current_mode = Mode.VOICE
        elif event.key == '3':
            current_mode = Mode.FIGHT
        # update the on-screen text immediately
        sim.set_mode_label(current_mode)

    sim.fig.canvas.mpl_connect('key_press_event', on_key)   
    try:
        print("Running LED effects simulation...")
        print("Close the matplotlib window to stop")
        
        while plt.get_fignums():  # While window is open
            if current_mode == Mode.IDLE:
                effects.breathing(color=(255, 0, 100), speed=0.08)
            elif current_mode == Mode.VOICE:
                effects.rainbow_wave(speed=3)
            elif current_mode == Mode.FIGHT:
                effects.fire_effect()
            
            sim.show(delay=0.05)
            
    except KeyboardInterrupt:
        print("Simulation stopped")
    
    plt.ioff()
    plt.close('all')