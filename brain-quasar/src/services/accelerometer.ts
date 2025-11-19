// Accelerometer service for punch detection using native Android sensors
import AccelerometerMonitoring, { type PunchEvent } from 'src/plugins/accelerometer-monitoring';
import { eventBus, Events } from './event-bus';
import type { PluginListenerHandle } from '@capacitor/core';

interface PunchDetectionConfig {
  threshold: number; // G-force threshold for punch detection (e.g., 2.0 = 2G)
  cooldownMs: number; // Minimum time between punch detections
  baselineAlpha: number; // Smoothing factor for baseline drift correction (0.0-1.0)
  baselineX: number; // Initial X baseline (typically 0)
  baselineY: number; // Initial Y baseline (typically 0)
  baselineZ: number; // Initial Z baseline (typically 9.81 for gravity)
  accelAlpha: number; // EWMA smoothing factor for accelerometer values (0.0-1.0)
  enabled: boolean;
}

class AccelerometerService {
  private config: PunchDetectionConfig = {
    threshold: 2.0, // 2G threshold
    cooldownMs: 200, // 200ms between punches
    baselineAlpha: 0.005, // Very slow smoothing factor for baseline drift correction
    baselineX: 0, // Initial X baseline
    baselineY: 0, // Initial Y baseline
    baselineZ: 9.81, // Initial Z baseline (gravity)
    accelAlpha: 0.4, // Moderate smoothing factor for accelerometer trend detection
    enabled: false,
  };

  private listenerHandle: PluginListenerHandle | null = null;

  async start(): Promise<void> {
    if (this.config.enabled) {
      console.log('Accelerometer already started');
      return;
    }

    try {
      // Start native accelerometer monitoring
      console.log('Starting native accelerometer monitoring...');
      await AccelerometerMonitoring.startMonitoring({
        threshold: this.config.threshold,
        cooldownMs: this.config.cooldownMs,
        baselineAlpha: this.config.baselineAlpha,
        baselineX: this.config.baselineX,
        baselineY: this.config.baselineY,
        baselineZ: this.config.baselineZ,
        accelAlpha: this.config.accelAlpha,
      });

      // Listen for punch events from native code
      this.listenerHandle = await AccelerometerMonitoring.addListener(
        'punchDetected',
        (event: PunchEvent) => {
          console.log('Native punch detected:', event);

          // Emit punch event to the app's event bus
          eventBus.emit(Events.PUNCH_DETECTED, {
            force: event.force,
            magnitude: event.magnitude,
            raw: event.raw,
            timestamp: event.timestamp,
          });
        }
      );

      this.config.enabled = true;
      console.log('Native accelerometer service started');
    } catch (error) {
      console.error('Failed to start native accelerometer:', error);
      throw error;
    }
  }

  async stop(): Promise<void> {
    if (!this.config.enabled) {
      return;
    }

    // Remove event listener
    if (this.listenerHandle) {
      await this.listenerHandle.remove();
      this.listenerHandle = null;
    }

    // Stop native monitoring
    try {
      console.log('Stopping native accelerometer monitoring...');
      await AccelerometerMonitoring.stopMonitoring();
      console.log('Native accelerometer monitoring stopped');
    } catch (error) {
      console.error('Failed to stop native accelerometer:', error);
    }

    this.config.enabled = false;
    console.log('Accelerometer service stopped');
  }

  async updateConfig(config: Partial<PunchDetectionConfig>): Promise<void> {
    this.config = { ...this.config, ...config };
    console.log('Accelerometer config updated:', this.config);

    // Update native config if monitoring is active
    if (this.config.enabled) {
      try {
        await AccelerometerMonitoring.updateConfig({
          threshold: this.config.threshold,
          cooldownMs: this.config.cooldownMs,
          baselineAlpha: this.config.baselineAlpha,
          baselineX: this.config.baselineX,
          baselineY: this.config.baselineY,
          baselineZ: this.config.baselineZ,
          accelAlpha: this.config.accelAlpha,
        });
        console.log('Native accelerometer config updated');
      } catch (error) {
        console.error('Failed to update native accelerometer config:', error);
      }
    }
  }

  getConfig(): PunchDetectionConfig {
    return { ...this.config };
  }

  isEnabled(): boolean {
    return this.config.enabled;
  }
}

// Singleton instance
export const accelerometerService = new AccelerometerService();
