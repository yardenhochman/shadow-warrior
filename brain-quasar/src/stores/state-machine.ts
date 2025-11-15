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
  lastActivityTime: number | null; // Track last punch or shout time for inactivity detection
  timers: {
    cooldown: number | null;
    warming: number | null;
    fight: number | null;
    inactivity: number | null; // Timer for checking fight inactivity
    decay: number | null;
  };
  lastEffectTrigger: number;
  accumulatedPower: number;
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
      fightInactivityTimeout: 60 * 1000, // 1 minute - no punches or shouts
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
    lastActivityTime: null,
    timers: {
      cooldown: null,
      warming: null,
      fight: null,
      inactivity: null,
      decay: null,
    },
    lastEffectTrigger: 0,
    accumulatedPower: 0,
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
      this.lastEffectTrigger = 0;
      this.accumulatedPower = 0;

      // Send LED command for idle mode
      eventBus.emit(Events.LED_COMMAND, { mode: 'idle' });
    },

    onEnterWarming(): void {
      this.warmingStartTime = Date.now();
      this.metrics.warmingPower = 0;
      this.lastEffectTrigger = 0;
      this.accumulatedPower = 0;

      // Start the power decay loop
      this.startDecayLoop();

      // Set timeout to revert to idle if threshold not reached
      this.timers.warming = window.setTimeout(() => {
        if (this.currentState === ArenaState.WARMING) {
          this.transition(ArenaState.IDLE);
        }
      }, this.config.warmingTimeout);

      // Send LED command for breathing mode (warming up)
      eventBus.emit(Events.LED_COMMAND, { mode: 'breathing' });
    },

    onEnterFight(): void {
      this.fightStartTime = Date.now();
      this.lastActivityTime = Date.now();
      this.metrics.fightPower = 0;
      this.lastEffectTrigger = 0;
      this.accumulatedPower = 0;

      // Start the power decay loop
      this.startDecayLoop();

      // Set timeout for maximum fight duration
      this.timers.fight = window.setTimeout(() => {
        if (this.currentState === ArenaState.FIGHT) {
          console.log('Fight timeout reached - ending fight');
          this.transition(ArenaState.VICTORY);
        }
      }, this.config.fightTimeout);

      // Set up inactivity check - runs every second
      this.timers.inactivity = window.setInterval(() => {
        if (this.currentState === ArenaState.FIGHT && this.lastActivityTime !== null) {
          const timeSinceActivity = Date.now() - this.lastActivityTime;
          if (timeSinceActivity >= this.config.fightInactivityTimeout) {
            console.log('Fight inactivity timeout reached - ending fight');
            this.transition(ArenaState.VICTORY);
          }
        }
      }, 1000) as unknown as number;

      // Send LED command for electricity mode (fight mode)
      eventBus.emit(Events.LED_COMMAND, { mode: 'electricity' });

      // Start music
      eventBus.emit(Events.SPEAKER_COMMAND, {
        action: 'play',
        track: 'fight',
      });
    },

    onEnterVictory(): void {
      // Send LED command for energy pulse (victory pattern)
      eventBus.emit(Events.LED_COMMAND, { mode: 'energy_pulse' });

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
      eventBus.emit(Events.LED_COMMAND, { mode: 'idle' });
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
        const powerGain = amplitude * this.config.warmingShoutScale;
        this.metrics.warmingPower = Math.min(
          100,
          this.metrics.warmingPower + powerGain
        );

        // Accumulate power for effect triggering with 100ms debounce
        this.triggerEffectDebounced(powerGain);

        // Check if threshold reached
        if (this.metrics.warmingPower >= this.config.warmingThreshold) {
          this.transition(ArenaState.FIGHT);
        }
      } else if (this.currentState === ArenaState.FIGHT) {
        // Track activity
        this.lastActivityTime = Date.now();

        // Shouts during fight contribute to fight power using scale factor
        const powerGain = amplitude * this.config.fightShoutScale;
        this.metrics.fightPower = Math.min(
          100,
          this.metrics.fightPower + powerGain
        );

        // Accumulate power for effect triggering with 100ms debounce
        this.triggerEffectDebounced(powerGain);

        // Bar can fill to 100% but fight continues until timeout or inactivity
      }

      // Emit metrics update
      eventBus.emit(Events.METRICS_UPDATED, this.metrics);
    },

    // Handle punch detection
    onPunchDetected(force: number): void {
      this.metrics.punchForce = force;

      if (this.currentState === ArenaState.FIGHT) {
        // Track activity
        this.lastActivityTime = Date.now();

        // Punches contribute to fight power using scale factor
        const powerGain = force * this.config.fightPunchScale;
        this.metrics.fightPower = Math.min(
          100,
          this.metrics.fightPower + powerGain
        );

        // Accumulate power for effect triggering with 100ms debounce
        this.triggerEffectDebounced(powerGain);

        // Bar can fill to 100% but fight continues until timeout or inactivity
      }

      // Emit metrics update
      eventBus.emit(Events.METRICS_UPDATED, this.metrics);
    },

    // Trigger LED effect with 100ms debounce based on accumulated power
    triggerEffectDebounced(powerGain: number): void {
      const now = Date.now();
      this.accumulatedPower += powerGain;

      // Debounce: only trigger effect if 100ms has passed since last trigger
      if (now - this.lastEffectTrigger >= 100) {
        this.lastEffectTrigger = now;

        // Determine which effect to trigger based on current state and accumulated power
        if (this.currentState === ArenaState.WARMING) {
          // Map accumulated power to appropriate warming effect
          // Higher power = more intense effect
          if (this.accumulatedPower >= 15) {
            // High intensity shout - electricity effect
            eventBus.emit(Events.LED_COMMAND, { mode: 'electricity' });
          } else if (this.accumulatedPower >= 8) {
            // Medium intensity - energy pulse
            eventBus.emit(Events.LED_COMMAND, { mode: 'energy_pulse' });
          } else {
            // Low intensity - breathing
            eventBus.emit(Events.LED_COMMAND, { mode: 'breathing' });
          }

          console.log('Warming effect triggered with accumulated power:', this.accumulatedPower);
        } else if (this.currentState === ArenaState.FIGHT) {
          // Map accumulated power to appropriate fight effect
          // Higher power = more intense effect
          if (this.accumulatedPower >= 20) {
            // High power punch/shout - electricity effect
            eventBus.emit(Events.LED_COMMAND, { mode: 'electricity' });
          } else if (this.accumulatedPower >= 10) {
            // Medium power - energy pulse
            eventBus.emit(Events.LED_COMMAND, { mode: 'energy_pulse' });
          } else {
            // Low power - electricity (default fight mode)
            eventBus.emit(Events.LED_COMMAND, { mode: 'electricity' });
          }

          console.log('Fight effect triggered with accumulated power:', this.accumulatedPower);
        }

        // Reset accumulated power after triggering effect
        this.accumulatedPower = 0;
      }
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
      Object.entries(this.timers).forEach(([key, timer]) => {
        if (timer !== null) {
          if (key === 'decay' || key === 'inactivity') {
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
        inactivity: null,
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
