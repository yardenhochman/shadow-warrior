// Effect Compositor for Layering Multiple Generators

import { FrameBuffer } from './frame-buffer';
import type { EffectFrame, EffectGenerator, SensorState } from './types';

export interface CompositorLayer {
  generator: EffectGenerator;
  alpha?: number; // 0-1, default 1
  enabled?: boolean; // default true
}

export class EffectCompositor {
  private ledCount: number;
  private baseBuffer: FrameBuffer;
  private tempBuffer: FrameBuffer;

  constructor(ledCount: number) {
    this.ledCount = ledCount;
    this.baseBuffer = new FrameBuffer(ledCount);
    this.tempBuffer = new FrameBuffer(ledCount);
  }

  /**
   * Compose multiple effect layers into a single frame
   * @param layers Array of generators with optional alpha/enabled flags
   * @param state Current sensor state
   * @returns Combined effect frame
   */
  compose(layers: CompositorLayer[], state: SensorState): EffectFrame {
    // Clear base buffer
    this.baseBuffer.clear();

    // Render and blend each layer
    for (const layer of layers) {
      if (layer.enabled === false) continue;

      const frame = layer.generator.next(state);
      this.tempBuffer.pixels.set(frame.pixels);

      const alpha = layer.alpha ?? 1.0;
      if (alpha < 1.0) {
        this.baseBuffer.blendFrom(this.tempBuffer, alpha);
      } else {
        // Full opacity - copy directly
        this.baseBuffer.copyFrom(this.tempBuffer);
      }
    }

    return {
      pixels: new Uint8Array(this.baseBuffer.pixels),
      timestamp: Date.now(),
    };
  }

  /**
   * Reset all generators in layers
   */
  reset(layers: CompositorLayer[]): void {
    for (const layer of layers) {
      layer.generator.reset();
    }
  }
}
