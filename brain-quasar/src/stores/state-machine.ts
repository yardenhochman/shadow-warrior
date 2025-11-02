import { defineStore, acceptHMRUpdate } from 'pinia';
import {
  ArenaState,
  type StateTransition,
  type ArenaMetrics,
  type StateConfig,
  VALID_TRANSITIONS,
} from 'src/types/state-machine';
import { eventBus, Events } from 'src/services/event-bus';

interface StateMachineState {
  currentState: ArenaState;
  previousState: ArenaState | null;
  metrics: ArenaMetrics;
  config: StateConfig;
  history: StateTransition[];
  cooldownStartTime: number | null;
  warmingStartTime: number | null;
  fightStartTime: number | null;
  timers: {
    cooldown: number | null;
    warming: number | null;
    fight: number | null;
    decay: number | null;
  };
}

export const useStateMachineStore = defineStore('stateMachine', {
  state: (): StateMachineState => ({
    currentState: ArenaState.IDLE,
    previousState: null,
    metrics: {
      shoutAmplitude: 0,
      punchForce: 0,
      warmingPower: 0,
      fightPower: 0,
    },
    config: {
      warmingThreshold: 80,
      fightThreshold: 100,
      cooldownDuration: 5 * 60 * 1000, // 5 minutes
      warmingTimeout: 60 * 1000, // 1 minute
      fightTimeout: 3 * 60 * 1000, // 3 minutes
      warmingDecayRate: 5, // Points per second
      fightDecayRate: 3, // Points per second
      warmingShoutScale: 10, // Power multiplier for warming shouts
      fightPunchScale: 10, // Power multiplier for fight punches
      fightShoutScale: 2, // Power multiplier for fight shouts (0.2x of punches)
      presenceDetectionThreshold: 0.3, // Shout amplitude threshold for IDLE -> WARMING
    },
    history: [],
    cooldownStartTime: null,
    warmingStartTime: null,
    fightStartTime: null,
    timers: {
      cooldown: null,
      warming: null,
      fight: null,
      decay: null,
    },
  }),

  getters: {
    isIdle: (state) => state.currentState === ArenaState.IDLE,
    isWarming: (state) => state.currentState === ArenaState.WARMING,
    isFight: (state) => state.currentState === ArenaState.FIGHT,
    isVictory: (state) => state.currentState === ArenaState.VICTORY,
    isCooldown: (state) => state.currentState === ArenaState.COOLDOWN,

    canTransitionTo: (state) => (targetState: ArenaState) => {
      return VALID_TRANSITIONS[state.currentState].includes(targetState);
    },

    warmingProgress: (state) =>
      (state.metrics.warmingPower / state.config.warmingThreshold) * 100,

    fightProgress: (state) =>
      (state.metrics.fightPower / state.config.fightThreshold) * 100,

    cooldownTimeRemaining: (state) => {
      if (state.cooldownStartTime === null) return 0;
      const elapsed = Date.now() - state.cooldownStartTime;
      return Math.max(0, state.config.cooldownDuration - elapsed);
    },
  },

  actions: {
    // Transition to a new state
    transition(targetState: ArenaState, force = false): boolean {
      if (!force && !this.canTransitionTo(targetState)) {
        console.warn(
          'Invalid state transition from %s to %s',
          this.currentState,
          targetState
        );
        return false;
      }

      const previousState = this.currentState;
      this.previousState = previousState;
      this.currentState = targetState;

      // Record transition in history
      this.history.push({
        from: previousState,
        to: targetState,
        timestamp: Date.now(),
      });

      // Clear previous state timers
      this.clearTimers();

      // Handle state-specific logic
      this.onStateEnter(targetState);

      // Emit state change event
      eventBus.emit(Events.STATE_CHANGED, {
        from: previousState,
        to: targetState,
        timestamp: Date.now(),
      });

      console.log('State transition: %s -> %s', previousState, targetState);

      return true;
    },

    // Handle entering a new state
    onStateEnter(state: ArenaState): void {
      switch (state) {
        case ArenaState.IDLE:
          this.onEnterIdle();
          break;
        case ArenaState.WARMING:
          this.onEnterWarming();
          break;
        case ArenaState.FIGHT:
          this.onEnterFight();
          break;
        case ArenaState.VICTORY:
          this.onEnterVictory();
          break;
        case ArenaState.COOLDOWN:
          this.onEnterCooldown();
          break;
      }
    },

    onEnterIdle(): void {
      // Reset metrics
      this.metrics.warmingPower = 0;
      this.metrics.fightPower = 0;
      this.metrics.shoutAmplitude = 0;
      this.metrics.punchForce = 0;

      // Send LED command for standby mode
      eventBus.emit(Events.LED_COMMAND, { mode: 'standby' });
    },

    onEnterWarming(): void {
      this.warmingStartTime = Date.now();
      this.metrics.warmingPower = 0;

      // Start the power decay loop
      this.startDecayLoop();

      // Set timeout to revert to idle if threshold not reached
      this.timers.warming = window.setTimeout(() => {
        if (this.currentState === ArenaState.WARMING) {
          this.transition(ArenaState.IDLE);
        }
      }, this.config.warmingTimeout);

      // Send LED command for pulse mode
      eventBus.emit(Events.LED_COMMAND, { mode: 'pulse', intensity: 0 });
    },

    onEnterFight(): void {
      this.fightStartTime = Date.now();
      this.metrics.fightPower = 0;

      // Start the power decay loop
      this.startDecayLoop();

      // Set timeout to revert to idle if threshold not reached
      this.timers.fight = window.setTimeout(() => {
        if (this.currentState === ArenaState.FIGHT) {
          this.transition(ArenaState.IDLE);
        }
      }, this.config.fightTimeout);

      // Send LED command for fight mode
      eventBus.emit(Events.LED_COMMAND, { mode: 'fight' });

      // Start music
      eventBus.emit(Events.SPEAKER_COMMAND, {
        action: 'play',
        track: 'fight',
      });
    },

    onEnterVictory(): void {
      // Send LED command for victory pattern
      eventBus.emit(Events.LED_COMMAND, { mode: 'victory' });

      // Play victory music
      eventBus.emit(Events.SPEAKER_COMMAND, {
        action: 'play',
        track: 'victory',
      });

      // Automatically transition to cooldown after victory music (assume 10 seconds)
      setTimeout(() => {
        if (this.currentState === ArenaState.VICTORY) {
          this.transition(ArenaState.COOLDOWN);
        }
      }, 10000);
    },

    onEnterCooldown(): void {
      this.cooldownStartTime = Date.now();

      // Turn off LEDs and music
      eventBus.emit(Events.LED_COMMAND, { mode: 'off' });
      eventBus.emit(Events.SPEAKER_COMMAND, { action: 'stop' });

      // Set timer to return to idle after cooldown duration
      this.timers.cooldown = window.setTimeout(() => {
        if (this.currentState === ArenaState.COOLDOWN) {
          this.transition(ArenaState.IDLE);
        }
      }, this.config.cooldownDuration);
    },

    // Handle shout detection
    onShoutDetected(amplitude: number): void {
      this.metrics.shoutAmplitude = amplitude;

      if (this.currentState === ArenaState.IDLE && amplitude > this.config.presenceDetectionThreshold) {
        // Significant shout detected in idle, transition to warming
        this.transition(ArenaState.WARMING);
      } else if (this.currentState === ArenaState.WARMING) {
        // Accumulate warming power based on shout amplitude and scale factor
        this.metrics.warmingPower = Math.min(
          100,
          this.metrics.warmingPower + amplitude * this.config.warmingShoutScale
        );

        // Update LED pulse intensity
        eventBus.emit(Events.LED_COMMAND, {
          mode: 'pulse',
          intensity: amplitude,
        });

        // Check if threshold reached
        if (this.metrics.warmingPower >= this.config.warmingThreshold) {
          this.transition(ArenaState.FIGHT);
        }
      } else if (this.currentState === ArenaState.FIGHT) {
        // Shouts during fight contribute to fight power using scale factor
        this.metrics.fightPower = Math.min(
          100,
          this.metrics.fightPower + amplitude * this.config.fightShoutScale
        );

        // Update LED intensity
        eventBus.emit(Events.LED_COMMAND, {
          mode: 'fight',
          intensity: amplitude,
        });

        // Check if victory threshold reached
        if (this.metrics.fightPower >= this.config.fightThreshold) {
          this.transition(ArenaState.VICTORY);
        }
      }

      // Emit metrics update
      eventBus.emit(Events.METRICS_UPDATED, this.metrics);
    },

    // Handle punch detection
    onPunchDetected(force: number): void {
      this.metrics.punchForce = force;

      if (this.currentState === ArenaState.FIGHT) {
        // Punches contribute to fight power using scale factor
        this.metrics.fightPower = Math.min(
          100,
          this.metrics.fightPower + force * this.config.fightPunchScale
        );

        // Update LED intensity based on punch
        eventBus.emit(Events.LED_COMMAND, {
          mode: 'fight',
          intensity: force,
        });

        // Check if victory threshold reached
        if (this.metrics.fightPower >= this.config.fightThreshold) {
          this.transition(ArenaState.VICTORY);
        }
      }

      // Emit metrics update
      eventBus.emit(Events.METRICS_UPDATED, this.metrics);
    },

    // Update configuration
    updateConfig(newConfig: Partial<StateConfig>): void {
      this.config = { ...this.config, ...newConfig };
      eventBus.emit(Events.CONFIG_UPDATED, this.config);
    },

    // Start the decay loop for power degradation
    startDecayLoop(): void {
      // Clear any existing decay loop
      if (this.timers.decay !== null) {
        clearInterval(this.timers.decay as unknown as number);
      }

      // Run decay every 100ms (10 times per second for smooth decay)
      this.timers.decay = window.setInterval(() => {
        const decayInterval = 0.1; // 100ms in seconds

        if (this.currentState === ArenaState.WARMING) {
          const decayAmount = this.config.warmingDecayRate * decayInterval;
          this.metrics.warmingPower = Math.max(0, this.metrics.warmingPower - decayAmount);
          eventBus.emit(Events.METRICS_UPDATED, this.metrics);
        } else if (this.currentState === ArenaState.FIGHT) {
          const decayAmount = this.config.fightDecayRate * decayInterval;
          this.metrics.fightPower = Math.max(0, this.metrics.fightPower - decayAmount);
          eventBus.emit(Events.METRICS_UPDATED, this.metrics);
        }
      }, 100) as unknown as number;
    },

    // Stop the decay loop
    stopDecayLoop(): void {
      if (this.timers.decay !== null) {
        clearInterval(this.timers.decay as unknown as number);
        this.timers.decay = null;
      }
    },

    // Clear all timers
    clearTimers(): void {
      Object.values(this.timers).forEach((timer) => {
        if (timer !== null) {
          if (timer === this.timers.decay) {
            clearInterval(timer as unknown as number);
          } else {
            clearTimeout(timer);
          }
        }
      });
      this.timers = {
        cooldown: null,
        warming: null,
        fight: null,
        decay: null,
      };
    },

    // Manual state control (for testing/debugging)
    forceTransition(targetState: ArenaState): void {
      this.transition(targetState, true);
    },
  },
});

if (import.meta.hot) {
  import.meta.hot.accept(
    acceptHMRUpdate(useStateMachineStore, import.meta.hot)
  );
}
