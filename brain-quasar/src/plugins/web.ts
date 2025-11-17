import { WebPlugin } from '@capacitor/core';
import type { MusicPlaybackPlugin } from './music-playback';
import type { BlePeripheralPlugin } from './ble-peripheral';
import type { AccelerometerPlugin } from './accelerometer-monitoring';

export class MusicPlaybackWeb extends WebPlugin implements MusicPlaybackPlugin {
  startForegroundService(options: { trackName: string }): Promise<void> {
    console.log('[MusicPlaybackWeb] startForegroundService called for:', options.trackName);
    console.log('Web platform: Foreground service not needed');
    return Promise.resolve();
  }

  stopForegroundService(): Promise<void> {
    console.log('[MusicPlaybackWeb] stopForegroundService called');
    console.log('Web platform: Foreground service not needed');
    return Promise.resolve();
  }
}

export class AccelerometerMonitoringWeb extends WebPlugin implements AccelerometerPlugin {
  startMonitoring(options: { threshold: number; cooldownMs: number }): Promise<void> {
    console.log('[AccelerometerMonitoringWeb] startMonitoring called', options);
    console.log('Web platform: Native accelerometer not available, would use DeviceMotion API');
    return Promise.resolve();
  }

  stopMonitoring(): Promise<void> {
    console.log('[AccelerometerMonitoringWeb] stopMonitoring called');
    return Promise.resolve();
  }

  updateConfig(options: { threshold: number; cooldownMs: number }): Promise<void> {
    console.log('[AccelerometerMonitoringWeb] updateConfig called', options);
    return Promise.resolve();
  }
}

export class BlePeripheralWeb extends WebPlugin implements BlePeripheralPlugin {
  startAdvertising(): Promise<void> {
    console.log('[BlePeripheralWeb] BLE peripheral not supported on web platform');
    return Promise.resolve();
  }

  stopAdvertising(): Promise<void> {
    console.log('[BlePeripheralWeb] BLE peripheral not supported on web platform');
    return Promise.resolve();
  }

  sendData(): Promise<void> {
    console.log('[BlePeripheralWeb] BLE peripheral not supported on web platform');
    return Promise.resolve();
  }
}
