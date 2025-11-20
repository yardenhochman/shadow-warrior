// Effect Generator Types and Interfaces

export interface EffectFrame {
  pixels: Uint8Array; // RGB data for all LEDs (ledCount * 3 bytes)
  timestamp: number;

  /**
   * Get pixel data in wire format (converts RGB to GRB if needed)
   */
  getWireData?(): Uint8Array;
}

export interface SensorState {
  // Warmup state
  warmingProgress?: number; // 0-100%
  shoutAmplitude?: number; // 0-1

  // Fight state
  punchMagnitude?: number; // 0-10+ G
  punchDetected?: boolean;

  // General
  energyLevel?: number; // 0-100%
}

export interface EffectGenerator {
  /**
   * Generate the next frame based on current state
   */
  next(state: SensorState): EffectFrame;

  /**
   * Reset generator to initial state
   */
  reset(): void;
}

export interface EffectConfig {
  ledCount: number; // Number of LEDs in the strip (default: 180)
  fps: number; // Target frame rate (default: 30)
}
