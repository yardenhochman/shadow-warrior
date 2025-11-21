// TypeScript wrapper for native LED Effect service
// Communicates with LEDEffectPlugin on Android

import { registerPlugin } from '@capacitor/core';
import type { Plugin, PluginListenerHandle } from '@capacitor/core';

export interface LEDEffectPluginInterface extends Plugin {
  /**
   * Start background LED effect rendering
   */
  startEffectService(options: {
    mode: 'warmup' | 'fight';
    controllers?: Array<{ host: string; port?: number }>;
  }): Promise<void>;

  /**
   * Stop background LED effect rendering
   */
  stopEffectService(): Promise<void>;

  /**
   * Render a single frame (called from native service on background thread)
   * Returns frame data as base64 encoded pixel data
   */
  renderFrame(options: {
    mode: 'warmup' | 'fight';
  }): Promise<{ frameData: string }>;

  /**
   * Update LED effect state
   */
  updateEffectState(options: {
    type: 'frame' | 'punch' | 'shout' | 'power' | 'state';
    data: string;
  }): Promise<void>;

  /**
   * Listen for events from native service
   */
  addListener(
    eventName: 'ledEffectServiceStarted' | 'ledEffectServiceStopped' | 'ledEffectStateUpdated',
    listenerFunc: (event: { mode?: string; type?: string; data?: string }) => void
  ): Promise<PluginListenerHandle> & PluginListenerHandle;

  /**
   * Remove all listeners
   */
  removeAllListeners(): Promise<void>;
}

const LEDEffectPlugin = registerPlugin<LEDEffectPluginInterface>('LEDEffectPlugin', {
  web: {
    // Web fallback: no-op implementations
    async startEffectService() {
      await Promise.resolve();
      console.warn('LED Effect Plugin: Web platform does not support background service');
    },
    async stopEffectService() {
      await Promise.resolve();
      console.warn('LED Effect Plugin: Web platform does not support background service');
    },
    async renderFrame() {
      await Promise.resolve();
      return { frameData: '' };
    },
    async updateEffectState() {
      await Promise.resolve();
      console.warn('LED Effect Plugin: Web platform does not support background service');
    },
  },
});

export { LEDEffectPlugin };
