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
from matplotlib.widgets import Button, Slider

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

class ButtonHandler:
    def __init__(self, fig, position, label, color, hover_color, callback, press_callback=None, release_callback=None):
        """
        Create a button handler with consistent click detection
        
        Args:
            fig: matplotlib figure
            position: [left, bottom, width, height] for button placement
            label: button text
            color: normal button color
            hover_color: hover button color
            callback: function to call when button is clicked (single press)
            press_callback: function to call when button is pressed (for hold detection)
            release_callback: function to call when button is released
        """
        self.ax = plt.axes(position)
        self.button = Button(self.ax, label, color=color, hovercolor=hover_color)
        self.button.on_clicked(self._on_clicked)
        self.callback = callback
        self.press_callback = press_callback
        self.release_callback = release_callback
        self.pressed = False
        self.press_time = 0
        self.is_held = False
    
    def _on_clicked(self, event):
        """Internal click handler that sets the pressed state"""
        self.pressed = True
        self.press_time = time.time()
        print(f"Button '{self.button.label.get_text()}' pressed at {self.press_time}")
        if self.callback:
            self.callback()
    
    def is_pressed(self):
        """Check if button was recently pressed (within last 0.5 seconds)"""
        if self.pressed and (time.time() - self.press_time) < 0.5:
            self.pressed = False  # Reset after checking
            return True
        return False
    
    def is_held_down(self):
        """Check if button is currently being held down"""
        return self.is_held
    
    def set_held(self, held):
        """Set the held state of the button"""
        if held != self.is_held:
            self.is_held = held
            if held and self.press_callback:
                self.press_callback()
            elif not held and self.release_callback:
                self.release_callback()
    
    def release(self):
        """Release the button (call this when button should be released)"""
        self.set_held(False)

class SliderHandler:
    def __init__(self, fig, position, label, min_val, max_val, initial_val, callback, valfmt='%.2f', color='lightblue'):
        """
        Create a slider handler with consistent value management
        
        Args:
            fig: matplotlib figure
            position: [left, bottom, width, height] for slider placement
            label: slider label text
            min_val: minimum value
            max_val: maximum value
            initial_val: initial value
            callback: function to call when slider value changes
            valfmt: value format string
            color: slider color
        """
        self.ax = plt.axes(position)
        self.slider = Slider(
            ax=self.ax,
            label=label,
            valmin=min_val,
            valmax=max_val,
            valinit=initial_val,
            valfmt=valfmt,
            color=color
        )
        self.slider.on_changed(self._on_changed)
        self.callback = callback
        self.current_value = initial_val
    
    def _on_changed(self, val):
        """Internal change handler that calls the callback"""
        self.current_value = val
        print(f"Slider '{self.slider.label.get_text()}' changed to {val}")
        if self.callback:
            self.callback(val)
    
    def get_value(self):
        """Get current slider value"""
        return self.current_value
    
    def set_value(self, val):
        """Set slider value and update display"""
        self.current_value = val
        self.slider.set_val(val)

class LEDStripSimulator:
    def __init__(self, num_leds=50, led_size=20, spacing=5):
        self.num_leds = num_leds
        self.led_size = led_size
        self.spacing = spacing
        self.pixels = np.zeros((num_leds, 3))  # RGB values 0-255

        # Setup the plot
        self.fig, self.ax = plt.subplots(figsize=(16, 4))
        self.ax.set_xlim(-spacing, num_leds * (led_size + spacing))
        self.ax.set_ylim(-led_size//2, led_size + led_size//2)
        self.ax.set_aspect('equal')
        self.ax.axis('off')
        self.ax.set_facecolor('black')
        self.power_level = 0.0
        self.trigger_held = False  # Track trigger state
        self.trigger_press_time = 0  # Track when trigger was last pressed
        self.power_charge_rate = 0.02  # Power increase per frame when held
        self.power_decay_rate = 0.01   # Power decrease per frame when not held
        self.power_zero_start_time = None  # Track when power level first reached 0
        self.power_level_text = self.fig.text(
            0.5, 0.80, f"Power: {self.power_level*100:.0f}%",  # inside the figure, top area
            ha="center", va="top",
            color="black", fontsize=14
        )
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
        
        # Create reset button using ButtonHandler class
        self.reset_button = ButtonHandler(
            self.fig, 
            [0.35, 0.02, 0.15, 0.06], 
            'RESET', 
            'lightcoral', 
            'red',
            self._on_reset_pressed
        )
        
        # Create power level slider using SliderHandler class
        self.power_slider = SliderHandler(
            self.fig,
            [0.25, 0.12, 0.35, 0.03],  # [left, bottom, width, height]
            'Power Level',
            0.0,  # min_val
            1.0,  # max_val
            self.power_level,  # initial_val
            self._on_slider_changed,  # callback
            '%.0f%%',  # valfmt
            'lightblue'  # color
        )
        
        # self.fig.suptitle(f'LED Strip Simulator - {num_leds} LEDs', color='black')
        self.fig.subplots_adjust(top=0.88, bottom=0.15)
    
    def set_mode_label(self, mode: Mode):
        self.mode_text.set_text(f"Mode: {mode.name}")

    def set_trigger_held(self, held):
        """Set the trigger held state"""
        self.trigger_held = held
        if held:
            self.trigger_press_time = time.time()

    def is_trigger_held(self):
        """Check if trigger is currently held"""
        return self.trigger_held

    def check_trigger_timeout(self):
        """Check if trigger should be released due to timeout"""
        if self.trigger_held and (time.time() - self.trigger_press_time) > 0.2:  # 200ms timeout
            self.trigger_held = False
            return True
        return False





    def _on_reset_pressed(self):
        """Callback for reset button press"""
        self.power_level = 0.0
        self.power_level_text.set_text(f"Power: {self.power_level*100:.0f}%")
        self.power_slider.set_value(0.0)  # Update slider position

    def _on_slider_changed(self, val):
        """Callback for slider value change"""
        # Slider now sets the initial power level, but charging/decay will continue
        self.power_level = val
        self.power_level_text.set_text(f"Power: {self.power_level*100:.0f}%")

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
        
    def flash_effect(self, color=(255, 255, 255), duration=0.3):
        """Flash effect triggered by button press"""
        flash_time = time.time()
        while time.time() - flash_time < duration:
            # Flash white
            self.sim.fill_solid(*color)
            self.sim.show(delay=0.05)
            # Flash off
            self.sim.clear()
            self.sim.show(delay=0.05)

class LEDEffect:
    """Individual LED effect with configurable parameters"""
    
    def __init__(self, effects_library):
        self.effects = effects_library
        self.frame = 0
        
        # Default parameters for each effect
        self.breathing_color = (128, 0, 255)  # Purple
        self.breathing_speed = 0.08
        
        self.rainbow_speed = 3
        
        self.fire_intensity = 1.0
        
        self.scanner_color = (255, 0, 0)  # Red
        self.scanner_width = 3
        
        self.wave_color = (0, 255, 255)  # Cyan
        self.wave_length = 10
        
        self.flash_color = (255, 255, 255)  # White
        self.flash_duration = 0.3
    
    def breath(self):
        """Breathing effect with smooth HSV color variation"""
        hue = 0.8  # purple base (0.0–1.0 scale)
        # Smooth hue variation using sine wave
        hue_variation = 0.02 * np.sin(self.frame * 0.02)  # slow, smooth variation
        varied_hue = hue + hue_variation
        r, g, b = colorsys.hsv_to_rgb(varied_hue, 1.0, 1.0)
        rgb = (int(r*255), int(g*255), int(b*255))
        self.effects.breathing(color=rgb, speed=self.breathing_speed)
        self.frame += 1
    
    def rainbow(self, power_level=1.0):
        """Rainbow wave effect that reflects power level"""
        # Calculate how many LEDs should be lit based on power level
        num_lit_leds = int(self.effects.sim.num_leds * power_level)
        
        # Apply rainbow effect only to the lit LEDs
        for i in range(self.effects.sim.num_leds):
            if i < num_lit_leds:
                # Rainbow effect for lit LEDs
                hue = (self.frame * self.rainbow_speed + i * 10) % 360
                self.effects.sim.set_pixel_hsv(i, hue, 1.0, 1.0)
            else:
                # Turn off remaining LEDs
                self.effects.sim.set_pixel(i, 0, 0, 0)
        
        self.frame += 1

    def voice_power_effect(self, power_level=1.0):
        """Multi-stage voice power effect based on power level"""
        # Calculate how many LEDs should be lit based on power level
        num_lit_leds = int(self.effects.sim.num_leds * power_level)

        # Stage 1: Rainbow (0-40%)
        if power_level <= 0.3:
            for i in range(self.effects.sim.num_leds):
                if i < num_lit_leds:
                    # Rainbow effect for lit LEDs
                    hue = (self.frame * self.rainbow_speed + i * 10) % 360
                    self.effects.sim.set_pixel_hsv(i, hue, 1.0, 1.0)
                else:
                    # Turn off remaining LEDs
                    self.effects.sim.set_pixel(i, 0, 0, 0)
        
        # Stage 2: Fire effect (40-80%)
        elif power_level <= 0.6:
            for i in range(self.effects.sim.num_leds):
                if i < num_lit_leds:
                    # Fire effect for lit LEDs
                    flicker = np.random.random() * 0.5 + 0.5
                    hue = np.random.randint(0, 60)  # Red to yellow range
                    sat = 1.0
                    val = flicker * (0.7 + 0.3 * np.random.random())
                    self.effects.sim.set_pixel_hsv(i, hue, sat, val)
                else:
                    # Turn off remaining LEDs
                    self.effects.sim.set_pixel(i, 0, 0, 0)
        
        # Stage 3: Flash effect (60-100%)
        else:
            # Flash effect with speed scaling based on power level
            # Calculate flash frequency based on how close to 100% we are
            power_ratio = (power_level - 0.6) / 0.4  # 0.6 to 1.0 becomes 0.0 to 1.0
            base_frequency = 2.0
            max_frequency = 8.0
            flash_frequency = base_frequency + (max_frequency - base_frequency) * power_ratio
            
            flash_intensity = (np.sin(self.frame * flash_frequency) + 1) / 3  # Dynamic flashing
            
            for i in range(self.effects.sim.num_leds):
                if i < num_lit_leds:
                    # White flash for lit LEDs
                    intensity = int(255 * flash_intensity)
                    self.effects.sim.set_pixel(i, intensity, intensity, intensity)
                else:
                    # Turn off remaining LEDs
                    self.effects.sim.set_pixel(i, 0, 0, 0)
        
        self.frame += 1
    
    def fire(self):
        """Fire effect with current settings"""
        self.effects.fire_effect()
    
    def scan(self):
        """Scanner effect with current settings"""
        self.effects.scanner(color=self.scanner_color, width=self.scanner_width)
    
    def wave(self):
        """Wave effect with current settings"""
        self.effects.wave(color=self.wave_color, length=self.wave_length)
    
    def flash(self):
        """Flash effect with current settings"""
        self.effects.flash_effect(color=self.flash_color, duration=self.flash_duration)
    
    # Parameter setters
    def set_breathing_color(self, color):
        self.breathing_color = color
    
    def set_breathing_speed(self, speed):
        self.breathing_speed = speed
    
    def set_rainbow_speed(self, speed):
        self.rainbow_speed = speed
    
    def set_fire_intensity(self, intensity):
        self.fire_intensity = intensity
    
    def set_scanner_color(self, color):
        self.scanner_color = color
    
    def set_scanner_width(self, width):
        self.scanner_width = width
    
    def set_wave_color(self, color):
        self.wave_color = color
    
    def set_wave_length(self, length):
        self.wave_length = length
    
    def set_flash_color(self, color):
        self.flash_color = color
    
    def set_flash_duration(self, duration):
        self.flash_duration = duration

# Demo Usage
if __name__ == "__main__":
    # Create simulator
    sim = LEDStripSimulator(num_leds=60)
    effects = LEDEffects(sim)
    led_effect = LEDEffect(effects)
    
    # Show the simulator
    plt.ion()  # Interactive mode
    plt.show()

    sim.set_mode_label(current_mode)

    def on_key_press(event):
        global current_mode
        print(f"Key pressed: {event.key}")  # Debug output
        if event.key == '1':
            current_mode = Mode.IDLE
            sim.set_trigger_held(False)  # Release trigger when going to idle
        elif event.key == '2' and current_mode != Mode.FIGHT:
            current_mode = Mode.VOICE
            sim.set_trigger_held(True)   # Trigger held when entering voice mode
            print("Key 2 pressed - voice mode and trigger held")
        elif event.key == '3':
            current_mode = Mode.FIGHT
            sim.set_trigger_held(True)  # Release trigger when going to fight mode
            print("Key 2 pressed - punch triggered")
        else:
            # Any other key press releases the trigger
            sim.set_trigger_held(False)
        # update the on-screen text immediately
        sim.set_mode_label(current_mode)

    def on_key_release(event):
        print(f"Key released: {event.key}")  # Debug output
        if event.key == '2':
            sim.set_trigger_held(False)  # Release trigger when key 2 is released
            print("Key 2 released - trigger released")
        elif event.key == '3':
            sim.set_trigger_held(False)  # Release trigger when key 3 is released
            print("Key 3 released - trigger released")

    # Connect keyboard events for mode switching and trigger control
    sim.fig.canvas.mpl_connect('key_press_event', on_key_press)
    sim.fig.canvas.mpl_connect('key_release_event', on_key_release)   
    try:
        print("Running LED effects simulation...")
        print("Close the matplotlib window to stop")
        print("Press 1 to go to IDLE mode")
        print("Press and hold 2 to charge power level in VOICE mode")
        print("Release 2 to let power level decay")
        print("Press 3 to go to FIGHT mode")
        print("Use the slider to set initial power level, or click RESET button")
        
        while plt.get_fignums():  # While window is open
            # Check for reset button press
            sim.reset_button.is_pressed()

            # Update power level based on trigger state
            if sim.is_trigger_held():
                if current_mode == Mode.VOICE:
                    # Increase power when trigger is held
                    sim.power_level = min(sim.power_level + sim.power_charge_rate, 1.0)
                elif current_mode == Mode.FIGHT:
                    # Increase power when trigger is held
                    sim.power_level = min(sim.power_level + sim.power_charge_rate * 10, 1.0)
                # Reset zero timer when power increases
                sim.power_zero_start_time = None
            elif current_mode == Mode.FIGHT:
                sim.power_level = max(sim.power_level - sim.power_decay_rate * 10, 0.0)
                sim.power_zero_start_time = None
            else:
                # Decrease power when trigger is not held
                sim.power_level = max(sim.power_level - sim.power_decay_rate, 0.0)
            
            # Update power display
            sim.power_level_text.set_text(f"Power: {sim.power_level*100:.0f}%")
            sim.power_slider.set_value(sim.power_level)

            # Track when power level reaches 0
            if sim.power_level == 0.0:
                if sim.power_zero_start_time is None:
                    sim.power_zero_start_time = time.time()
                    print("Power level reached 0 - starting 10 second timer")
                elif time.time() - sim.power_zero_start_time >= 10.0:
                    current_mode = Mode.IDLE
                    sim.power_zero_start_time = None
                    print("10 seconds elapsed - returning to IDLE mode")
            else:
                # Reset timer when power level is not 0
                sim.power_zero_start_time = None
            

            if current_mode == Mode.IDLE:
                led_effect.breath()
                sim.set_mode_label(Mode.IDLE)
            elif current_mode == Mode.VOICE:
                if sim.power_level == 1.0:
                    current_mode = Mode.FIGHT
                led_effect.voice_power_effect(power_level=sim.power_level)
            elif current_mode == Mode.FIGHT:
                led_effect.voice_power_effect(power_level=sim.power_level)
            
            sim.show(delay=0.05)
            
    except KeyboardInterrupt:
        print("Simulation stopped")
    
    plt.ioff()
    plt.close('all')