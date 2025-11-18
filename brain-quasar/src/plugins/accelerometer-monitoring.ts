import { registerPlugin, type PluginListenerHandle } from '@capacitor/core';

export interface PunchEvent {
  force: number; // 0-1 normalized
  magnitude: number; // G-force magnitude
  raw: {
    x: number;
    y: number;
    z: number;
  };
  timestamp: number;
}

export interface AccelerometerPlugin {
  startMonitoring(options: {
    threshold: number;
    cooldownMs: number;
    baselineAlpha: number;
    baselineX: number;
    baselineY: number;
    baselineZ: number;
    accelAlpha: number;
  }): Promise<void>;
  stopMonitoring(): Promise<void>;
  updateConfig(options: {
    threshold: number;
    cooldownMs: number;
    baselineAlpha: number;
    baselineX: number;
    baselineY: number;
    baselineZ: number;
    accelAlpha: number;
  }): Promise<void>;

  // Event listener for punch detection
  addListener(eventName: 'punchDetected', listenerFunc: (event: PunchEvent) => void): Promise<PluginListenerHandle>;
  removeAllListeners(): Promise<void>;
}

const AccelerometerMonitoring = registerPlugin<AccelerometerPlugin>('AccelerometerPlugin', {
  web: () => import('./web').then((m) => new m.AccelerometerMonitoringWeb()),
});

export default AccelerometerMonitoring;
