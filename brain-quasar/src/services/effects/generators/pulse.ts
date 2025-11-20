// Pulse Generator - Strip-wide pulses triggered by shout amplitude

import { FrameBuffer } from '../frame-buffer';
import type { EffectFrame, EffectGenerator, SensorState } from '../types';

interface Pulse {
  position: number; // 0-1 position along strip
  intensity: number; // 0-1 brightness
  age: number; // frames since creation
}

export class PulseGenerator implements EffectGenerator {
  private buffer: FrameBuffer;
  private ledCount: number;
  private pulses: Pulse[] = [];
  private readonly PULSE_THRESHOLD = 0.3; // Shout amplitude threshold
  private readonly PULSE_DURATION = 20; // frames (~0.66s at 30fps)
  private readonly PULSE_WIDTH = 0.15; // Pulse width as fraction of strip
  private lastShoutAmplitude = 0;

  constructor(ledCount: number) {
    this.ledCount = ledCount;
    this.buffer = new FrameBuffer(ledCount);
  }

  next(state: SensorState): EffectFrame {
    const shoutAmplitude = state.shoutAmplitude ?? 0;

    // Trigger new pulse if amplitude crosses threshold
    if (shoutAmplitude > this.PULSE_THRESHOLD && this.lastShoutAmplitude <= this.PULSE_THRESHOLD) {
      this.pulses.push({
        position: 0,
        intensity: Math.min(1, shoutAmplitude * 1.5),
        age: 0,
      });
    }
    this.lastShoutAmplitude = shoutAmplitude;

    // Clear buffer
    this.buffer.clear();

    // Update and render each pulse
    for (let i = this.pulses.length - 1; i >= 0; i--) {
      const pulse = this.pulses[i]!;

      // Advance pulse
      pulse.age++;
      pulse.position = pulse.age / this.PULSE_DURATION;

      // Remove expired pulses
      if (pulse.age >= this.PULSE_DURATION) {
        this.pulses.splice(i, 1);
        continue;
      }

      // Calculate fade (brightest in middle of lifecycle)
      const lifecycle = pulse.age / this.PULSE_DURATION;
      const fade = Math.sin(lifecycle * Math.PI); // 0 -> 1 -> 0

      // Render pulse as gaussian-like wave
      const centerLED = Math.floor(pulse.position * this.ledCount);
      const pulseWidth = Math.floor(this.ledCount * this.PULSE_WIDTH);

      for (let j = 0; j < this.ledCount; j++) {
        const distance = Math.abs(j - centerLED);
        if (distance < pulseWidth) {
          const gaussian = Math.exp(-(distance * distance) / (2 * (pulseWidth / 3) ** 2));
          const brightness = 255 * gaussian * fade * pulse.intensity;

          // White pulse
          const [r, g, b] = this.buffer.getPixel(j);
          this.buffer.setPixel(
            j,
            Math.max(r, brightness),
            Math.max(g, brightness),
            Math.max(b, brightness)
          );
        }
      }
    }

    return {
      pixels: this.buffer.getWireData(),
      timestamp: Date.now(),
    };
  }

  reset(): void {
    this.buffer.clear();
    this.pulses = [];
    this.lastShoutAmplitude = 0;
  }
}
