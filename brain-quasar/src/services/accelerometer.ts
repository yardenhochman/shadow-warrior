// Accelerometer service for punch detection
import { Motion, type AccelListenerEvent } from '@capacitor/motion';
import { eventBus, Events } from './event-bus';

interface PunchDetectionConfig {
  threshold: number; // G-force threshold for punch detection (e.g., 2.0 = 2G)
  cooldownMs: number; // Minimum time between punch detections
  enabled: boolean;
}

class AccelerometerService {
  private config: PunchDetectionConfig = {
    threshold: 2.0, // 2G threshold
    cooldownMs: 200, // 200ms between punches
    enabled: false,
  };

  private lastPunchTime = 0;
  private listenerId: { remove: () => Promise<void> } | null = null;
  private baselineAccel = { x: 0, y: 0, z: 9.81 }; // Gravity baseline

  async start(): Promise<void> {
    if (this.config.enabled) {
      console.log('Accelerometer already started');
      return;
    }

    try {
      // Request DeviceMotion permission on platforms that require it
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const DeviceMotionEventWithPermission = DeviceMotionEvent as any;
      if (
        typeof DeviceMotionEvent !== 'undefined' &&
        typeof DeviceMotionEventWithPermission.requestPermission === 'function'
      ) {
        console.log('Requesting DeviceMotion permission...');
        try {
          const permission = await DeviceMotionEventWithPermission.requestPermission();
          console.log('DeviceMotion permission result:', permission);
          if (permission !== 'granted') {
            console.warn('DeviceMotion permission denied');
            throw new Error('DeviceMotion permission denied by user');
          }
        } catch (permissionError) {
          console.error('Failed to request DeviceMotion permission:', permissionError);
          throw permissionError;
        }
      }

      // Start listening to accelerometer
      this.listenerId = await Motion.addListener('accel', this.handleAcceleration.bind(this));

      this.config.enabled = true;
      console.log('Accelerometer service started');
    } catch (error) {
      console.error('Failed to start accelerometer:', error);
      throw error;
    }
  }

  async stop(): Promise<void> {
    if (!this.config.enabled) {
      return;
    }

    if (this.listenerId) {
      await this.listenerId.remove();
      this.listenerId = null;
    }

    this.config.enabled = false;
    console.log('Accelerometer service stopped');
  }

  private handleAcceleration(event: AccelListenerEvent): void {
    const { x, y, z } = event.acceleration;

    // Calculate magnitude of acceleration vector
    // Subtract gravity baseline for more accurate punch detection
    const deltaX = x - this.baselineAccel.x;
    const deltaY = y - this.baselineAccel.y;
    const deltaZ = z - this.baselineAccel.z;

    const magnitude = Math.sqrt(deltaX * deltaX + deltaY * deltaY + deltaZ * deltaZ);

    // Detect punch based on threshold
    if (magnitude > this.config.threshold) {
      const now = Date.now();

      // Check cooldown period to avoid duplicate detections
      if (now - this.lastPunchTime >= this.config.cooldownMs) {
        this.lastPunchTime = now;

        // Normalize force to 0-1 range (assume max 6G for normalization)
        const normalizedForce = Math.min(magnitude / 6.0, 1.0);

        // Emit punch event
        eventBus.emit(Events.PUNCH_DETECTED, {
          force: normalizedForce,
          magnitude,
          raw: { x, y, z },
          timestamp: now,
        });

        console.log('Punch detected: magnitude=%d, force=%f', magnitude, normalizedForce);
      }
    }

    // Update baseline using exponential moving average (for drift correction)
    const alpha = 0.01; // Smoothing factor
    this.baselineAccel.x = alpha * x + (1 - alpha) * this.baselineAccel.x;
    this.baselineAccel.y = alpha * y + (1 - alpha) * this.baselineAccel.y;
    this.baselineAccel.z = alpha * z + (1 - alpha) * this.baselineAccel.z;
  }

  updateConfig(config: Partial<PunchDetectionConfig>): void {
    this.config = { ...this.config, ...config };
    console.log('Accelerometer config updated:', this.config);
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
