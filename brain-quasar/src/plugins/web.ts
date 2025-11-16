import { WebPlugin } from '@capacitor/core';
import type { MusicPlaybackPlugin } from './music-playback';

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
