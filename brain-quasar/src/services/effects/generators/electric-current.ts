// Electric Current Generator - Animated noise/lightning effect

import { FrameBuffer } from '../frame-buffer';
import type { EffectFrame, EffectGenerator, SensorState } from '../types';

export class ElectricCurrentGenerator implements EffectGenerator {
  private buffer: FrameBuffer;
  private ledCount: number;
  private time = 0;
  private noiseOffset = Math.random() * 1000;

  constructor(ledCount: number) {
    this.ledCount = ledCount;
    this.buffer = new FrameBuffer(ledCount);
  }

  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  next(_state: SensorState): EffectFrame {
    this.time++;

    // Clear buffer
    this.buffer.clear();

    // Generate electric current effect using multi-octave noise
    for (let i = 0; i < this.ledCount; i++) {
      const x = i / this.ledCount;
      const t = this.time * 0.05;

      // Multi-octave noise for more organic lightning
      const noise1 = this.noise(x * 2 + t, this.noiseOffset);
      const noise2 = this.noise(x * 4 + t * 1.5, this.noiseOffset + 100);
      const noise3 = this.noise(x * 8 + t * 2, this.noiseOffset + 200);

      const combined = (noise1 * 0.5 + noise2 * 0.3 + noise3 * 0.2) * 1.2;

      // Threshold to create sporadic sparks
      const spark = combined > 0.6 ? (combined - 0.6) / 0.4 : 0;

      if (spark > 0) {
        // Electric colors: Yellow to White
        const intensity = Math.pow(spark, 2) * 255; // Brightest sparks are whitest
        const r = 255;
        const g = 255;
        const b = intensity * 0.3; // Add blue for white at peak

        this.buffer.setPixel(i, r, g, b);
      }
    }

    return {
      pixels: this.buffer.getWireData(),
      timestamp: Date.now(),
    };
  }

  reset(): void {
    this.buffer.clear();
    this.time = 0;
    this.noiseOffset = Math.random() * 1000;
  }

  /**
   * Simple Perlin-like noise function
   */
  private noise(x: number, offset: number): number {
    // Simple pseudo-random noise using sine waves
    const n = Math.sin(x * 12.9898 + offset) * 43758.5453;
    return (n - Math.floor(n));
  }
}
