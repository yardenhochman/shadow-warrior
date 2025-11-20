package org.shadowwarrior.brain;

import android.util.Log;

/**
 * Native LED effect generator for background rendering
 * Uses composable effects and compositor for layered rendering
 */
public class LEDEffectGenerator {
    private static final String TAG = "LEDEffectGenerator";

    private int ledCount;
    private String currentMode;

    // Effect instances
    private EnergyBarEffect energyBarEffect;
    private EnergyPulseEffect energyPulseEffect;
    private ElectricityEffect electricityEffect;

    // Compositor for blending effects
    private LEDEffectCompositor compositor;

    public LEDEffectGenerator(int ledCount) {
        this.ledCount = ledCount;
        this.currentMode = "warmup";

        // Initialize effects
        this.energyBarEffect = new EnergyBarEffect();
        this.energyPulseEffect = new EnergyPulseEffect();
        this.energyPulseEffect.setColor(100, 150, 255); // Blue pulse
        this.electricityEffect = new ElectricityEffect();

        // Initialize compositor
        this.compositor = new LEDEffectCompositor(ledCount);
    }

    /**
     * Set the current effect mode and configure effects accordingly
     */
    public void setMode(String mode) {
        this.currentMode = mode;
        compositor.clearEffects();

        switch (mode) {
            case "warmup":
                // Warmup: energy bar + energy pulse
                // Energy bar is the base (updated via power level)
                // Energy pulse is triggered by shouts
                compositor.addEffect(energyBarEffect);
                compositor.addEffect(energyPulseEffect);
                energyBarEffect.setAlpha(1.0f);
                energyPulseEffect.setAlpha(0.8f);
                Log.d(TAG, "Mode set to: warmup (energy bar + pulse)");
                break;

            case "fight":
                // Fight: electricity + energy pulse
                // Electricity is the base animation
                // Energy pulse is triggered by punches
                compositor.addEffect(electricityEffect);
                compositor.addEffect(energyPulseEffect);
                electricityEffect.setAlpha(0.9f);
                energyPulseEffect.setAlpha(0.8f);
                energyPulseEffect.setColor(255, 100, 100); // Red pulse for punches
                Log.d(TAG, "Mode set to: fight (electricity + pulse)");
                break;

            default:
                // Idle: just electricity dimmed
                compositor.addEffect(electricityEffect);
                electricityEffect.setAlpha(0.3f);
                Log.d(TAG, "Mode set to: idle");
        }
    }

    /**
     * Generate frame data for current mode using composited effects
     * Returns RGB pixel data (3 bytes per pixel)
     */
    public byte[] generateFrame() {
        return compositor.composite();
    }

    /**
     * Update power level (used in warmup mode)
     */
    public void updatePowerLevel(float power) {
        energyBarEffect.setPowerLevel(Math.max(0.0f, Math.min(1.0f, power)));
    }

    /**
     * Trigger a punch pulse (used in fight mode)
     */
    public void triggerPunch(float intensity) {
        energyPulseEffect.trigger(Math.max(0.0f, Math.min(1.0f, intensity)));
    }

    /**
     * Trigger a shout pulse (used in warmup mode)
     */
    public void triggerShout(float intensity) {
        energyPulseEffect.trigger(Math.max(0.0f, Math.min(1.0f, intensity)));
    }

    /**
     * Generate a black/off frame
     */
    public byte[] generateBlackFrame() {
        return new byte[ledCount * 3]; // All zeros = black
    }

    public int getLedCount() {
        return ledCount;
    }

    /**
     * Get the energy bar effect for direct manipulation if needed
     */
    public EnergyBarEffect getEnergyBarEffect() {
        return energyBarEffect;
    }

    /**
     * Get the energy pulse effect for direct manipulation if needed
     */
    public EnergyPulseEffect getEnergyPulseEffect() {
        return energyPulseEffect;
    }

    /**
     * Get the electricity effect for direct manipulation if needed
     */
    public ElectricityEffect getElectricityEffect() {
        return electricityEffect;
    }
}
