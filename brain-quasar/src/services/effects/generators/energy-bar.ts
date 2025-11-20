// Energy Bar Generator - Fills strip based on percentage

import { FrameBuffer } from '../frame-buffer';
import type { EffectFrame, EffectGenerator, SensorState } from '../types';

export class EnergyBarGenerator implements EffectGenerator {
  private buffer: FrameBuffer;
  private ledCount: number;

  constructor(ledCount: number) {
    this.ledCount = ledCount;
    this.buffer = new FrameBuffer(ledCount);
  }

  next(state: SensorState): EffectFrame {
    const percentage = state.warmingProgress ?? state.energyLevel ?? 0;
    const fillLEDs = Math.floor(this.ledCount * (percentage / 100));
    console.debug(`EnergyBar: percentage=${percentage}, fillLEDs=${fillLEDs}/${this.ledCount}`);

    // Clear buffer
    this.buffer.clear();

    // Fill with gradient (blue → green → yellow → red)
    for (let i = 0; i < fillLEDs; i++) {
      const t = i / this.ledCount; // 0 to 1

      let r, g, b;
      if (t < 0.33) {
        // Blue to Green
        const localT = t / 0.33;
        r = 0;
        g = 255 * localT;
        b = 255 * (1 - localT);
      } else if (t < 0.66) {
        // Green to Yellow
        const localT = (t - 0.33) / 0.33;
        r = 255 * localT;
        g = 255;
        b = 0;
      } else {
        // Yellow to Red
        const localT = (t - 0.66) / 0.34;
        r = 255;
        g = 255 * (1 - localT);
        b = 0;
      }

      this.buffer.setPixel(i, r, g, b);
    }

    return {
      pixels: this.buffer.getWireData(),
      timestamp: Date.now(),
    };
  }

  reset(): void {
    this.buffer.clear();
  }
}
