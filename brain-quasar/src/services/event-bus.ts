// Event bus for decoupled component communication
// Using mitt-like pattern for lightweight event emitting

import type { ArenaState } from 'src/types/state-machine';

// LED Command payload type
export interface LEDCommandPayload {
  arenaState: ArenaState;
  trigger: 'transition' | 'punch_detected' | 'shout_detected';
  triggerAmplitude?: number; // 0-1 for shouts/punches
  currentPower?: number; // 0-100 for warming/fight power
}

type EventHandler<T = unknown> = (payload: T) => void;

class EventBus {
  private events: Map<string, EventHandler[]>;

  constructor() {
    this.events = new Map();
  }

  on<T = unknown>(event: string, handler: EventHandler<T>): void {
    if (!this.events.has(event)) {
      this.events.set(event, []);
    }
    this.events.get(event)?.push(handler as EventHandler);
  }

  off<T = unknown>(event: string, handler: EventHandler<T>): void {
    const handlers = this.events.get(event);
    if (handlers) {
      const index = handlers.indexOf(handler as EventHandler);
      if (index > -1) {
        handlers.splice(index, 1);
      }
    }
  }

  emit<T = unknown>(event: string, payload?: T): void {
    const handlers = this.events.get(event);
    if (handlers) {
      handlers.forEach((handler) => handler(payload));
    }
  }

  clear(event?: string): void {
    if (event) {
      this.events.delete(event);
    } else {
      this.events.clear();
    }
  }
}

// Singleton instance
export const eventBus = new EventBus();

// Event names
export const Events = {
  // State machine events
  STATE_CHANGED: 'state:changed',
  STATE_TRANSITION_REQUESTED: 'state:transition-requested',

  // Sensor events
  PUNCH_DETECTED: 'sensor:punch',
  SHOUT_DETECTED: 'sensor:shout',
  PRESENCE_DETECTED: 'sensor:presence',

  // Device events
  LED_COMMAND: 'device:led',
  SPEAKER_COMMAND: 'device:speaker',
  UV_LIGHT_COMMAND: 'device:uv-light',
  CONTROLLER_CONNECTED: 'device:controller-connected',
  CONTROLLER_DISCONNECTED: 'device:controller-disconnected',
  CONTROLLER_ADDED: 'device:controller-added',
  CONTROLLER_REMOVED: 'device:controller-removed',

  // Metrics events
  METRICS_UPDATED: 'metrics:updated',

  // Config events
  CONFIG_UPDATED: 'config:updated',

  // Schedule events
  SCHEDULE_SUSPEND_REQUESTED: 'schedule:suspend-requested',
  SCHEDULE_ACTIVATE_AVAILABLE: 'schedule:activate-available',

  // Music events
  MUSIC_PLAYING: 'music:playing',
  MUSIC_PAUSED: 'music:paused',
  MUSIC_STOPPED: 'music:stopped',
  MUSIC_VOLUME_CHANGED: 'music:volume-changed',
  MUSIC_PLAYLIST_UPDATED: 'music:playlist-updated',
  MUSIC_TRACK_LOADED: 'music:track-loaded',
  MUSIC_ERROR: 'music:error',

  // Realtime effects events
  REALTIME_EFFECTS_STARTED: 'effects:realtime-started',
  REALTIME_EFFECTS_STOPPED: 'effects:realtime-stopped',
  REALTIME_EFFECTS_MODE_CHANGED: 'effects:mode-changed',
} as const;
