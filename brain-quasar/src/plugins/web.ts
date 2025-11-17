import { WebPlugin } from '@capacitor/core';
import type { MusicPlaybackPlugin } from './music-playback';
import type { BlePeripheralPlugin } from './ble-peripheral';

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
