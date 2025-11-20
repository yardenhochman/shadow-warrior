// Energy Pulse Generator - Traveling pulses triggered by punches

import { FrameBuffer } from '../frame-buffer';
import type { EffectFrame, EffectGenerator, SensorState } from '../types';

interface TravelingPulse {
  position: number; // 0-1 position along strip
  direction: number; // 1 or -1
  intensity: number; // 0-1 brightness
  age: number; // frames since creation
}

export class EnergyPulseGenerator implements EffectGenerator {
  private buffer: FrameBuffer;
  private ledCount: number;
  private pulses: TravelingPulse[] = [];
  private readonly PULSE_DURATION = 15; // frames (~0.5s at 30fps)
  private readonly PULSE_WIDTH = 0.08; // Pulse width as fraction of strip
  private readonly PUNCH_THRESHOLD = 2.0; // G force threshold
  private lastPunchDetected = false;

  constructor(ledCount: number) {
    this.ledCount = ledCount;
    this.buffer = new FrameBuffer(ledCount);
  }

  next(state: SensorState): EffectFrame {
    const punchDetected = state.punchDetected ?? false;
    const punchMagnitude = state.punchMagnitude ?? 0;

    // Trigger new pulse on punch detection
    if (punchDetected && !this.lastPunchDetected && punchMagnitude > this.PUNCH_THRESHOLD) {
      const direction = Math.random() > 0.5 ? 1 : -1; // Random direction
      const intensity = Math.min(1, punchMagnitude / 8); // Scale intensity

      this.pulses.push({
        position: direction > 0 ? 0 : 1,
        direction,
        intensity,
        age: 0,
      });
    }
    this.lastPunchDetected = punchDetected;

    // Clear buffer
    this.buffer.clear();

    // Update and render each pulse
    for (let i = this.pulses.length - 1; i >= 0; i--) {
      const pulse = this.pulses[i]!;

      // Advance pulse
      pulse.age++;
      const progress = pulse.age / this.PULSE_DURATION;
      pulse.position = pulse.direction > 0 ? progress : 1 - progress;

      // Remove expired pulses
      if (pulse.age >= this.PULSE_DURATION) {
        this.pulses.splice(i, 1);
        continue;
      }

      // Calculate fade (brightest at start, fade out)
      const fade = 1 - Math.pow(progress, 2);

      // Render pulse as gaussian-like wave
      const centerLED = Math.floor(pulse.position * this.ledCount);
      const pulseWidth = Math.floor(this.ledCount * this.PULSE_WIDTH);

      for (let j = 0; j < this.ledCount; j++) {
        const distance = Math.abs(j - centerLED);
        if (distance < pulseWidth) {
          const gaussian = Math.exp(-(distance * distance) / (2 * (pulseWidth / 3) ** 2));
          const brightness = 255 * gaussian * fade * pulse.intensity;

          // Red-orange pulse color (energy)
          const r = brightness;
          const g = brightness * 0.3;
          const b = 0;

          const [currR, currG, currB] = this.buffer.getPixel(j);
          this.buffer.setPixel(
            j,
            Math.max(currR, r),
            Math.max(currG, g),
            Math.max(currB, b)
          );
        }
      }
    }

    return {
      pixels: new Uint8Array(this.buffer.pixels),
      timestamp: Date.now(),
    };
  }

  reset(): void {
    this.buffer.clear();
    this.pulses = [];
    this.lastPunchDetected = false;
  }
}
