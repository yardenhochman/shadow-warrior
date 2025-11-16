import { registerPlugin } from '@capacitor/core';

export interface MusicPlaybackPlugin {
  startForegroundService(options: { trackName: string }): Promise<void>;
  stopForegroundService(): Promise<void>;
}

const MusicPlayback = registerPlugin<MusicPlaybackPlugin>('MusicPlaybackPlugin', {
  web: () => import('./web').then((m) => new m.MusicPlaybackWeb()),
});

export default MusicPlayback;
