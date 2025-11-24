package org.shadowwarrior.brain;

/**
 * Energy Pulse effect - a traveling wave of energy moving up the strip
 * Can be triggered to create pulses at the bottom
 */
public class EnergyPulseEffect implements LEDEffect {
    private static final String TAG = "EnergyPulseEffect";

    private float pulseTime = 0.0f; // Time since last pulse trigger
    private float pulseIntensity = 0.0f; // Intensity of current pulse (0.0 - 1.0)
    private float pulseSpeed = 2.0f; // Speed of pulse wave (LEDs per second)
    private float pulseDuration = 0.5f; // How long pulse lasts (seconds)
    private float alpha = 1.0f;

    // Color: default to redish, but configurable
    private int colorR = 250;
    private int colorG = 10;
    private int colorB = 10;

    public EnergyPulseEffect() {
    }

    @Override
    public byte[] generateFrame(int ledCount, float time) {
        byte[] pixels = new byte[ledCount * 3];

        // Update pulse decay
        if (pulseIntensity > 0.0f) {
            pulseTime += 0.033f; // ~30 FPS

            // Pulse fades out over pulseDuration
            float fadeAlpha = 1.0f - (pulseTime / pulseDuration);
            if (fadeAlpha <= 0.0f) {
                pulseIntensity = 0.0f;
                pulseTime = 0.0f;
            } else {
                pulseIntensity = fadeAlpha;
            }
        }

        // Draw the pulse wave
        if (pulseIntensity > 0.0f) {
            // Pulse position: starts at bottom (0) and travels upward
            float pulseFrontPosition = pulseTime * pulseSpeed * ledCount / pulseDuration;
            float pulseWidth = 0.15f * ledCount; // Width of the pulse

            for (int i = 0; i < ledCount; i++) {
                // Distance from pulse front
                float distance = Math.abs(i - pulseFrontPosition);

                if (distance < pulseWidth) {
                    // Inside pulse: bright color with falloff at edges
                    float falloff = 1.0f - (distance / pulseWidth);
                    float intensity = falloff * pulseIntensity;

                    pixels[i * 3] = (byte) (colorR * intensity);
                    pixels[i * 3 + 1] = (byte) (colorG * intensity);
                    pixels[i * 3 + 2] = (byte) (colorB * intensity);
                } else {
                    // Outside pulse: black (will be composited with other effects)
                    pixels[i * 3] = 0;
                    pixels[i * 3 + 1] = 0;
                    pixels[i * 3 + 2] = 0;
                }
            }
        }

        return pixels;
    }

    @Override
    public float getAlpha() {
        return alpha;
    }

    @Override
    public void update(float parameter) {
        // Pulse effect doesn't use continuous parameters
    }

    @Override
    public void trigger(float intensity) {
        // Trigger a new pulse with the given intensity
        this.pulseIntensity = Math.max(0.0f, Math.min(1.0f, intensity));
        this.pulseTime = 0.0f;
    }

    public void setColor(int r, int g, int b) {
        this.colorR = Math.max(0, Math.min(255, r));
        this.colorG = Math.max(0, Math.min(255, g));
        this.colorB = Math.max(0, Math.min(255, b));
    }

    public void setAlpha(float alpha) {
        this.alpha = Math.max(0.0f, Math.min(1.0f, alpha));
    }

    public void setPulseSpeed(float speed) {
        this.pulseSpeed = Math.max(0.1f, speed);
    }

    public void setPulseDuration(float duration) {
        this.pulseDuration = Math.max(0.1f, duration);
    }
}
