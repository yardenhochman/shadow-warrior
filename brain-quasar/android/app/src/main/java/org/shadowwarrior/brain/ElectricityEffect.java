package org.shadowwarrior.brain;

import android.util.Log;

/**
 * Electricity effect - electric blue waves moving across the strip
 * Creates a dynamic, energetic look with traveling waves
 */
public class ElectricityEffect implements LEDEffect {
    private static final String TAG = "ElectricityEffect";

    private float time = 0.0f;
    private float waveSpeed = 1.5f; // Speed of wave animation
    private float alpha = 1.0f;

    public ElectricityEffect() {
    }

    @Override
    public byte[] generateFrame(int ledCount, float time) {
        byte[] pixels = new byte[ledCount * 3];

        // Create electric waves moving across the strip
        for (int i = 0; i < ledCount; i++) {
            // Multiple wave layers for electric effect
            float position = i / (float) ledCount;

            // Wave 1: Fast moving wave
            float wave1 = (float) Math.sin((position + time * waveSpeed) * Math.PI * 4);
            float intensity1 = (wave1 + 1.0f) / 2.0f; // Normalize to 0-1

            // Wave 2: Slower wave for complexity
            float wave2 = (float) Math.sin((position - time * waveSpeed * 0.5f) * Math.PI * 6);
            float intensity2 = (wave2 + 1.0f) / 2.0f;

            // Combine waves with emphasis on peaks
            float combined = intensity1 * 0.7f + intensity2 * 0.3f;
            combined = (float) Math.pow(combined, 0.8f); // Emphasize bright areas

            // Add a base brightness so it's never completely dark
            combined = 0.2f + combined * 0.8f;

            // Electric blue-cyan color
            int r = (int) (0 * combined);
            int g = (int) (150 + 105 * combined); // 150-255
            int b = (int) (255 * combined); // 0-255 based on intensity

            pixels[i * 3] = (byte) Math.max(0, Math.min(255, r));
            pixels[i * 3 + 1] = (byte) Math.max(0, Math.min(255, g));
            pixels[i * 3 + 2] = (byte) Math.max(0, Math.min(255, b));
        }

        return pixels;
    }

    @Override
    public float getAlpha() {
        return alpha;
    }

    @Override
    public void update(float parameter) {
        // Electricity doesn't use continuous parameters
        // It just animates on its own
    }

    @Override
    public void trigger(float intensity) {
        // Electricity doesn't respond to triggers
    }

    public void setWaveSpeed(float speed) {
        this.waveSpeed = Math.max(0.1f, speed);
    }

    public void setAlpha(float alpha) {
        this.alpha = Math.max(0.0f, Math.min(1.0f, alpha));
    }
}
