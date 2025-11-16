import { registerPlugin } from '@capacitor/core';
import type { NativeAudio } from '@capacitor-community/native-audio';

/**
 * Extended NativeAudio plugin that supports background playback.
 * This is a wrapper around our custom Android plugin NativeAudioExtended
 * which doesn't pause audio when the app goes to the background.
 */
const NativeAudioExtended = registerPlugin<NativeAudio>('NativeAudioExtended', {
  web: async () => {
    // For web, use the original NativeAudio web implementation
    const { NativeAudio: NativeAudioWeb } = await import('@capacitor-community/native-audio');
    return NativeAudioWeb;
  },
});

export default NativeAudioExtended;
