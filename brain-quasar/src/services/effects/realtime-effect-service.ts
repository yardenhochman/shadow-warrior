// Realtime Effect Service - Manages 30 FPS render loop and UDP transmission for multiple controllers

import { UDPService } from '../udp';
import { EffectCompositor, type CompositorLayer } from './compositor';
import type { EffectConfig, SensorState } from './types';
import { EnergyBarGenerator } from './generators/energy-bar';
import { PulseGenerator } from './generators/pulse';
import { ElectricCurrentGenerator } from './generators/electric-current';
import { EnergyPulseGenerator } from './generators/energy-pulse';
import { eventBus, Events } from '../event-bus';

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
  private compositor: EffectCompositor;
  private currentState: SensorState = {};

  // Generators (shared across all controllers)
  private energyBarGen: EnergyBarGenerator;
  private pulseGen: PulseGenerator;
  private electricCurrentGen: ElectricCurrentGenerator;
  private energyPulseGen: EnergyPulseGenerator;

  // Per-controller state
  private controllers = new Map<string, UDPService>();
  private renderInterval: number | null = null;

  constructor(
    controllerHosts: Array<{ host: string; port?: number }> = [],
    config?: Partial<EffectConfig>
  ) {
    if (config) {
      this.config = { ...this.config, ...config };
    }

    this.compositor = new EffectCompositor(this.config.ledCount);

    // Initialize generators
    this.energyBarGen = new EnergyBarGenerator(this.config.ledCount);
    this.pulseGen = new PulseGenerator(this.config.ledCount);
    this.electricCurrentGen = new ElectricCurrentGenerator(this.config.ledCount);
    this.energyPulseGen = new EnergyPulseGenerator(this.config.ledCount);
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
   * Update sensor state (called by state machine/sensors)
   */
  updateState(state: Partial<SensorState>): void {
    this.currentState = { ...this.currentState, ...state };
    console.log('RealtimeEffectService state updated:', JSON.stringify(this.currentState));
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

    // Render first frame immediately
    await this.renderFrame()

    // Start render loop
    const frameTime = 1000 / this.config.fps;
    this.renderInterval = window.setInterval(() => {
      void this.renderFrame();
    }, frameTime);

    console.log(`Realtime effects started: ${mode} mode at ${this.config.fps} FPS`);
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
    if (this.renderInterval !== null) {
      clearInterval(this.renderInterval);
      this.renderInterval = null;
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
   * Switch effect mode for a specific controller
   */
  switchMode(mode: RealtimeEffectMode): void {
    this.mode = mode;
    // Reset generators for clean transition
    this.energyBarGen.reset();
    this.pulseGen.reset();
    this.electricCurrentGen.reset();
    this.energyPulseGen.reset();

    console.log(`Switched to ${mode} mode`);
    eventBus.emit(Events.REALTIME_EFFECTS_MODE_CHANGED, { mode });
  }

  /**
   * Check if a specific controller is running
  */
 isRunning(): boolean {
    return this.mode !== RealtimeEffectMode.NONE && this.renderInterval !== null;
  }

  /**
   * Get list of running controllers
   */
  getRunningControllers(): string[] {
    return Array.from(this.controllers.keys());
  }

  /**
   * Render and send a single frame to a specific controller
   */
  private async renderFrame(): Promise<void> {
    if (this.mode === RealtimeEffectMode.NONE) {
      return;
    }
    try {
      // Build layer stack based on current mode
      const layers = this.buildLayers(this.mode);

      if (layers.length === 0) {
        console.warn('No layers built for mode:', this.mode);
        return;
      }

      // Compose frame
      const frame = this.compositor.compose(layers, this.currentState);
      console.debug(`Frame composed: ${frame.pixels.length} bytes, first 6 RGB values: [${Array.from(frame.pixels.slice(0, 6)).join(', ')}]`);
      // Send via UDP
      const promises = [];
      for (const controller of this.controllers.values()) {
        promises.push(controller.sendFrame(frame.pixels));
      }
      const results = await Promise.allSettled(promises);
      const failures = results.filter(r => r.status === 'rejected');
      if (failures.length > 0) {
        console.error(`${failures.length} controllers failed to send frame`);
      }
    } catch (error) {
      console.error(`Frame render error`, error);
      // Don't stop on single frame error, but log it
    }
  }

  /**
   * Build compositor layers based on mode
   */
  private buildLayers(mode: RealtimeEffectMode): CompositorLayer[] {
    switch (mode) {
      case RealtimeEffectMode.WARMUP:
        return [
          { generator: this.energyBarGen, alpha: 1.0 }, // Base: Energy bar
          { generator: this.pulseGen, alpha: 0.6 },     // Overlay: Shout pulses
        ];

      case RealtimeEffectMode.FIGHT:
        return [
          { generator: this.electricCurrentGen, alpha: 1.0 }, // Base: Electric current
          { generator: this.energyPulseGen, alpha: 0.8 },     // Overlay: Punch pulses
        ];

      default:
        return [];
    }
  }
}
