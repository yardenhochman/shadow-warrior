package org.shadowwarrior.brain;

/**
 * Energy Bar effect - color gradient from blue to red based on power level
 * Fills the strip from bottom up according to power parameter
 */
public class EnergyBarEffect implements LEDEffect {
    private static final String TAG = "EnergyBarEffect";

    private float powerLevel = 0.0f; // 0.0 - 1.0
    private float alpha = 1.0f;

    public EnergyBarEffect() {
    }

    @Override
    public byte[] generateFrame(int ledCount, float time) {
        byte[] pixels = new byte[ledCount * 3];

        // Calculate how many LEDs to fill
        int filledLeds = (int) (ledCount * powerLevel);

        for (int i = 0; i < ledCount; i++) {
            if (i < filledLeds) {
                // LED is filled - calculate color based on position in bar
                float position = i / (float) filledLeds; // 0.0 - 1.0

                // Color gradient: white-blue (low power) -> cyan -> green -> yellow -> red (high power)
                int r, g, b;

                if (position < 0.25f) {
                    // White-blue: fade white to cyan
                    float t = position / 0.25f;
                    r = (int) (200 * (1.0f - t * 0.8f)); // 200 -> 40
                    g = (int) (200 * (1.0f - t * 0.8f)); // 200 -> 40
                    b = 255;
                } else if (position < 0.5f) {
                    // Cyan -> green
                    float t = (position - 0.25f) / 0.25f;
                    r = (int) (40 * (1.0f - t));
                    g = (int) (255 * (0.2f + t * 0.8f)); // 51 -> 255
                    b = (int) (255 * (1.0f - t * 0.8f)); // 255 -> 51
                } else if (position < 0.75f) {
                    // Green -> yellow
                    float t = (position - 0.5f) / 0.25f;
                    r = (int) (255 * t); // 0 -> 255
                    g = 255;
                    b = 0;
                } else {
                    // Yellow -> red
                    float t = (position - 0.75f) / 0.25f;
                    r = 255;
                    g = (int) (255 * (1.0f - t)); // 255 -> 0
                    b = 0;
                }

                pixels[i * 3] = (byte) Math.min(255, r);
                pixels[i * 3 + 1] = (byte) Math.min(255, g);
                pixels[i * 3 + 2] = (byte) Math.min(255, b);
            } else {
                // Unfilled LED is black
                pixels[i * 3] = 0;
                pixels[i * 3 + 1] = 0;
                pixels[i * 3 + 2] = 0;
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
        // Parameter is power level 0.0 - 1.0
        this.powerLevel = Math.max(0.0f, Math.min(1.0f, parameter));
    }

    @Override
    public void trigger(float intensity) {
        // Energy bar doesn't use triggers
    }

    public void setPowerLevel(float power) {
        this.powerLevel = Math.max(0.0f, Math.min(1.0f, power));
    }

    public void setAlpha(float alpha) {
        this.alpha = Math.max(0.0f, Math.min(1.0f, alpha));
    }
}
