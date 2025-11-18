import { registerPlugin } from '@capacitor/core';

interface ForegroundServicePlugin {
  start(): Promise<void>;
  stop(): Promise<void>;
}

const ForegroundService = registerPlugin<ForegroundServicePlugin>('ForegroundService');

class ForegroundServiceManager {
  private isRunning = false;

  async start(): Promise<void> {
    if (this.isRunning) {
      console.log('[Foreground Service] Already running');
      return;
    }

    try {
      await ForegroundService.start();
      this.isRunning = true;
      console.log('[Foreground Service] Started successfully');
    } catch (error) {
      console.error('[Foreground Service] Failed to start:', error);
      throw error;
    }
  }

  async stop(): Promise<void> {
    if (!this.isRunning) {
      console.log('[Foreground Service] Not running');
      return;
    }

    try {
      await ForegroundService.stop();
      this.isRunning = false;
      console.log('[Foreground Service] Stopped successfully');
    } catch (error) {
      console.error('[Foreground Service] Failed to stop:', error);
      throw error;
    }
  }

  getStatus(): boolean {
    return this.isRunning;
  }
}

export const foregroundService = new ForegroundServiceManager();
