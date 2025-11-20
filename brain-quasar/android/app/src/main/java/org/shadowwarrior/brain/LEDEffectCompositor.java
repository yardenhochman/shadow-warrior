package org.shadowwarrior.brain;

import java.util.ArrayList;
import java.util.List;

/**
 * Compositor for blending multiple LED effects with alpha transparency
 * Implements proper alpha blending to layer effects on top of each other
 */
public class LEDEffectCompositor {
    private static final String TAG = "LEDEffectCompositor";

    private List<LEDEffect> effects = new ArrayList<>();
    private int ledCount;
    private float time = 0.0f;

    public LEDEffectCompositor(int ledCount) {
        this.ledCount = ledCount;
    }

    /**
     * Add an effect to the compositor
     */
    public void addEffect(LEDEffect effect) {
        if (!effects.contains(effect)) {
            effects.add(effect);
        }
    }

    /**
     * Remove an effect from the compositor
     */
    public void removeEffect(LEDEffect effect) {
        effects.remove(effect);
    }

    /**
     * Clear all effects
     */
    public void clearEffects() {
        effects.clear();
    }

    /**
     * Composite all effects into a single frame
     * Uses alpha blending to layer effects
     */
    public byte[] composite() {
        byte[] result = new byte[ledCount * 3];

        if (effects.isEmpty()) {
            return result; // All black if no effects
        }

        // Process each LED position
        for (int i = 0; i < ledCount; i++) {
            float r = 0.0f;
            float g = 0.0f;
            float b = 0.0f;
            float accumulatedAlpha = 0.0f;

            // Blend all effects
            for (LEDEffect effect : effects) {
                byte[] frameData = effect.generateFrame(ledCount, time);
                float effectAlpha = effect.getAlpha();

                // Get this LED's color from this effect
                int effectR = frameData[i * 3] & 0xFF;
                int effectG = frameData[i * 3 + 1] & 0xFF;
                int effectB = frameData[i * 3 + 2] & 0xFF;

                // Alpha blend: newColor = (effect * alpha) + (accumulated * (1 - alpha))
                // But we use additive blending for light effects
                float remainingAlpha = 1.0f - accumulatedAlpha;
                float blendAmount = effectAlpha * remainingAlpha;

                r += effectR * blendAmount;
                g += effectG * blendAmount;
                b += effectB * blendAmount;

                accumulatedAlpha += blendAmount;

                // Early exit if we've reached full opacity
                if (accumulatedAlpha >= 0.99f) {
                    break;
                }
            }

            // Clamp to 0-255
            result[i * 3] = (byte) Math.max(0, Math.min(255, (int) r));
            result[i * 3 + 1] = (byte) Math.max(0, Math.min(255, (int) g));
            result[i * 3 + 2] = (byte) Math.max(0, Math.min(255, (int) b));
        }

        time += 0.033f; // ~30 FPS
        return result;
    }

    /**
     * Get the number of effects currently composited
     */
    public int getEffectCount() {
        return effects.size();
    }

    /**
     * Reset time
     */
    public void resetTime() {
        time = 0.0f;
    }
}
