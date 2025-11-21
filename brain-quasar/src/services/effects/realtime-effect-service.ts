// Realtime Effect Service - Manages LED effect rendering via native Android service

import { UDPService } from '../udp';
import type { EffectConfig } from './types';
import { eventBus, Events } from '../event-bus';
import { LEDEffectPlugin } from './led-effect-plugin';

export enum RealtimeEffectMode {
  WARMUP = 'warmup',
  FIGHT = 'fight',
  NONE = 'none',
}

export class RealtimeEffectService {
  private config: EffectConfig = {
    ledCount: 180,
    fps: 30,
  };

  private mode: RealtimeEffectMode;

  // Per-controller state
  private controllers = new Map<string, UDPService>();

  // Native service state
  private nativeServiceActive = false;

  constructor(
    controllerHosts: Array<{ host: string; port?: number }> = [],
    config?: Partial<EffectConfig>
  ) {
    if (config) {
      this.config = { ...this.config, ...config };
    }

    this.mode = RealtimeEffectMode.NONE;
    console.log('RealtimeEffectService created with', controllerHosts.length, 'controllers');
    this.controllers = new Map<string, UDPService>();
    for (const controllerHost of controllerHosts) {
      const { host, port = 21324 } = controllerHost;
      const controllerId = `${host}:${port}`;
      this.controllers.set(controllerId, new UDPService({ host, port }));
    }
  }

  /**
   * Start realtime effect rendering for a specific controller
   */
  async start(mode: RealtimeEffectMode): Promise<void> {
    try {
      // Create dedicated UDP service for this controller
      await Promise.all(
        Array.from(this.controllers.values()).map(async (controller) => {
          if (this.mode !== RealtimeEffectMode.NONE) {
            console.warn(`Controller ${controller.config?.host}:${controller.config?.port} already running, skipping start`);
            return;
          }

          await controller.connect();
        })
      );
      this.mode = mode;

    } catch (error) {
      // Clean up on error
      await this.stopAll();
      console.error('Failed to start realtime effects:', error);
      throw error;
    }

    // Start native background service - handles all rendering and UDP transmission natively
    try {
      await LEDEffectPlugin.startEffectService({
        mode: mode as 'warmup' | 'fight',
        controllers: Array.from(this.controllers.values()).map(controller => ({
          host: controller.config?.host || '127.0.0.1',
          port: controller.config?.port || 21324,
        })),
      });
      this.nativeServiceActive = true;
      console.log('Native LED effect service started - rendering on background thread');
    } catch (error) {
      console.error('Failed to start native LED effect service:', error);
      throw error;
    }

    console.log(`Realtime effects started: ${mode} mode (native rendering at ${this.config.fps} FPS)`);
    eventBus.emit(Events.REALTIME_EFFECTS_STARTED, { mode });

  }

  /**
   * Stop realtime effect rendering for a specific controller
   */
  async stop(): Promise<void> {
    await this.stopAll();

    console.log(`Realtime effects stopped`);
    eventBus.emit(Events.REALTIME_EFFECTS_STOPPED, {  });
  }

  /**
   * Stop all controllers
   */
  async stopAll(): Promise<void> {
    // Stop native background service
    if (this.nativeServiceActive) {
      try {
        await LEDEffectPlugin.stopEffectService();
        this.nativeServiceActive = false;
        console.log('Native LED effect service stopped');
      } catch (error) {
        console.warn('Failed to stop native LED effect service:', error);
      }
    }

    const stopPromises = Array.from(this.controllers.values()).map(async (controller) => {
      await controller.disconnect();
    });

    await Promise.allSettled(stopPromises);
    this.mode = RealtimeEffectMode.NONE;
    console.log('All realtime effects stopped');
    eventBus.emit(Events.REALTIME_EFFECTS_STOPPED, { all: true });
  }

  /**
   * Clear the LED strip by sending a black frame to all controllers
   */
  async clearStrip(): Promise<void> {
    console.log('Clearing LED strip with black frame');
    const clearPromises = Array.from(this.controllers.values()).map(async (controller) => {
      if (controller.connected()) {
        await controller.sendBlackFrame(this.config.ledCount);
      }
    });
    await Promise.allSettled(clearPromises);
  }

  /**
   * Switch effect mode by restarting the native service with new mode
   */
  async switchMode(mode: RealtimeEffectMode): Promise<void> {
    if (mode === this.mode) {
      console.log(`Already in ${mode} mode, skipping mode switch`);
      return;
    }

    try {
      // Stop the current service
      await this.stopAll();
      // Restart with new mode
      await this.start(mode);
      console.log(`Switched to ${mode} mode`);
      eventBus.emit(Events.REALTIME_EFFECTS_MODE_CHANGED, { mode });
    } catch (error) {
      console.error('Failed to switch mode:', error);
      throw error;
    }
  }

  /**
   * Check if a specific controller is running
  */
  isRunning(): boolean {
    return this.mode !== RealtimeEffectMode.NONE && this.nativeServiceActive;
  }

  /**
   * Get list of running controllers
   */
  getRunningControllers(): string[] {
    return Array.from(this.controllers.keys());
  }

  /**
   * Send punch event to native service
   */
  sendPunchEvent(intensity: number): void {
    if (this.nativeServiceActive) {
      try {
        LEDEffectPlugin.updateEffectState({
          type: 'punch',
          data: JSON.stringify({ intensity }),
        }).catch((error) => {
          console.warn('Failed to send punch event to native service:', error);
        });
      } catch (error) {
        console.warn('Error sending punch event:', error);
      }
    }
  }

  /**
   * Send shout event to native service
   */
  sendShoutEvent(intensity: number): void {
    console.log('sendShoutEvent called: intensity=%f, nativeServiceActive=%s', intensity, this.nativeServiceActive);
    if (this.nativeServiceActive) {
      try {
        console.log('Sending shout event to native service with intensity: %f', intensity);
        LEDEffectPlugin.updateEffectState({
          type: 'shout',
          data: JSON.stringify({ intensity }),
        }).catch((error) => {
          console.warn('Failed to send shout event to native service:', error);
        });
      } catch (error) {
        console.warn('Error sending shout event:', error);
      }
    } else {
      console.warn('Cannot send shout event - native service not active');
    }
  }

  /**
   * Send power level to native service (0-100)
   */
  sendPowerLevel(power: number): void {
    if (this.nativeServiceActive) {
      try {
        // Normalize to 0-1 range for native service
        const normalizedPower = Math.max(0, Math.min(1, power / 100));
        LEDEffectPlugin.updateEffectState({
          type: 'power',
          data: JSON.stringify({ intensity: normalizedPower }),
        }).catch((error) => {
          console.warn('Failed to send power level to native service:', error);
        });
      } catch (error) {
        console.warn('Error sending power level:', error);
      }
    }
  }

}
