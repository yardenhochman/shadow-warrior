package org.shadowwarrior.brain;

/**
 * Interface for LED effects that generate pixel frames
 * Effects are composable and support alpha blending
 */
public interface LEDEffect {
    /**
     * Generate the next frame for this effect
     * Returns RGB pixel data (3 bytes per pixel) with alpha in [0, 1]
     * For compositing: alpha controls how much this effect contributes
     *
     * @param ledCount number of LEDs in the strip
     * @param time elapsed time in seconds
     * @return RGB pixel array: [R, G, B, R, G, B, ...] with implicit alpha from effect
     */
    byte[] generateFrame(int ledCount, float time);

    /**
     * Get the alpha (opacity) for this effect [0.0 - 1.0]
     * Used by compositor for blending
     */
    float getAlpha();

    /**
     * Update effect parameters (e.g., power level, intensity)
     */
    void update(float parameter);

    /**
     * Trigger a pulse event in this effect
     */
    void trigger(float intensity);
}
