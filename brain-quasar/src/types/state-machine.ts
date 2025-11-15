// State machine types for Shadow Warrior arena

export enum ArenaState {
  IDLE = 'idle',
  WARMING = 'warming',
  FIGHT = 'fight',
  VICTORY = 'victory',
  COOLDOWN = 'cooldown',
}

export interface StateTransition {
  from: ArenaState;
  to: ArenaState;
  timestamp: number;
}

export interface ArenaMetrics {
  shoutAmplitude: number; // 0-1 normalized amplitude
  punchForce: number; // 0-1 normalized force
  warmingPower: number; // 0-100 warming threshold
  fightPower: number; // 0-100 fight threshold
}

export interface StateConfig {
  warmingThreshold: number; // Power level to trigger fight mode
  fightThreshold: number; // Power level to trigger victory (deprecated - fight now ends on timeout/inactivity)
  cooldownDuration: number; // Duration in milliseconds (default 5 minutes)
  warmingTimeout: number; // Time before reverting to idle if threshold not reached
  fightTimeout: number; // Maximum fight duration before ending (default 3 minutes)
  fightInactivityTimeout: number; // Time without punches/shouts before ending fight (default 1 minute)
  warmingDecayRate: number; // Power decrease per second during warming (default 5)
  fightDecayRate: number; // Power decrease per second during fight (default 3)
  warmingShoutScale: number; // Power gain multiplier for shouts during warming (default 10)
  fightPunchScale: number; // Power gain multiplier for punches during fight (default 10)
  fightShoutScale: number; // Power gain multiplier for shouts during fight (default 2, which is 0.2x of punch)
  presenceDetectionThreshold: number; // Shout amplitude threshold to trigger IDLE -> WARMING transition (default 0.3)
}

export interface ArenaEvent {
  type: 'punch' | 'shout' | 'state-change' | 'config-update';
  payload: unknown;
  timestamp: number;
}

// Valid state transitions according to AGENTS.md
export const VALID_TRANSITIONS: Record<ArenaState, ArenaState[]> = {
  [ArenaState.IDLE]: [ArenaState.WARMING],
  [ArenaState.WARMING]: [ArenaState.FIGHT, ArenaState.IDLE],
  [ArenaState.FIGHT]: [ArenaState.IDLE, ArenaState.VICTORY],
  [ArenaState.VICTORY]: [ArenaState.COOLDOWN],
  [ArenaState.COOLDOWN]: [ArenaState.IDLE],
};
