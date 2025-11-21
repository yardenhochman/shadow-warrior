import { defineStore, acceptHMRUpdate } from 'pinia';
import {
  ArenaState,
  type StateTransition,
  type ArenaMetrics,
  type StateConfig,
  type ScheduleConfig,
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
}

export const useStateMachineStore = defineStore('stateMachine', {
  state: (): StateMachineState => ({
    currentState: ArenaState.IDLE,
    previousState: null,
    metrics: {
      shoutAmplitude: 0,
      punchForce: 0,
      punchMagnitude: 0,
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
      schedule: {
        enabled: false,
        dailyActiveStart: '09:00',
        dailyActiveEnd: '22:00',
      },
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
  }),

  getters: {
    isIdle: (state) => state.currentState === ArenaState.IDLE,
    isWarming: (state) => state.currentState === ArenaState.WARMING,
    isFight: (state) => state.currentState === ArenaState.FIGHT,
    isVictory: (state) => state.currentState === ArenaState.VICTORY,
    isCooldown: (state) => state.currentState === ArenaState.COOLDOWN,
    isSuspended: (state) => state.currentState === ArenaState.SUSPENDED,

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
        case ArenaState.SUSPENDED:
          this.onEnterSuspended();
          break;
      }
    },

    onEnterIdle(): void {
      // Reset metrics
      this.metrics.warmingPower = 0;
      this.metrics.fightPower = 0;
      this.metrics.shoutAmplitude = 0;
      this.metrics.punchForce = 0;
      this.metrics.punchMagnitude = 0;

      // Send LED command for idle mode
      eventBus.emit(Events.LED_COMMAND, {
        arenaState: ArenaState.IDLE,
        trigger: 'transition',
        currentPower: 0
      });

      // Turn off UV lights when idle
      eventBus.emit(Events.UV_LIGHT_COMMAND, { action: 'off' });
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

      // Send LED command for warming mode
      eventBus.emit(Events.LED_COMMAND, {
        arenaState: ArenaState.WARMING,
        trigger: 'transition',
        currentPower: 0
      });

      // Turn on UV lights
      eventBus.emit(Events.UV_LIGHT_COMMAND, { action: 'on' });
    },

    onEnterFight(): void {
      this.fightStartTime = Date.now();
      this.lastActivityTime = Date.now();
      this.metrics.fightPower = 0;

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

      // Send LED command for fight mode
      eventBus.emit(Events.LED_COMMAND, {
        arenaState: ArenaState.FIGHT,
        trigger: 'transition',
        currentPower: 0
      });

      // Start music
      eventBus.emit(Events.SPEAKER_COMMAND, {
        action: 'play',
        track: 'fight',
      });

      // Turn on UV lights
      eventBus.emit(Events.UV_LIGHT_COMMAND, { action: 'on' });
    },

    onEnterVictory(): void {
      // Send LED command for victory mode
      eventBus.emit(Events.LED_COMMAND, {
        arenaState: ArenaState.VICTORY,
        trigger: 'transition',
        currentPower: 100
      });

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
      eventBus.emit(Events.LED_COMMAND, {
        arenaState: ArenaState.COOLDOWN,
        trigger: 'transition',
        currentPower: 0
      });
      eventBus.emit(Events.SPEAKER_COMMAND, { action: 'stop' });

      // Set timer to return to idle after cooldown duration
      this.timers.cooldown = window.setTimeout(() => {
        if (this.currentState === ArenaState.COOLDOWN) {
          this.transition(ArenaState.IDLE);
        }
      }, this.config.cooldownDuration);
    },

    onEnterSuspended(): void {
      // Stop all timers
      this.clearTimers();
      this.stopDecayLoop();

      // Turn off all devices
      eventBus.emit(Events.LED_COMMAND, {
        arenaState: ArenaState.SUSPENDED,
        trigger: 'transition',
        currentPower: 0
      });
      eventBus.emit(Events.SPEAKER_COMMAND, { action: 'stop' });

      // Turn off UV lights
      eventBus.emit(Events.UV_LIGHT_COMMAND, { action: 'off' });

      // Reset metrics
      this.metrics.warmingPower = 0;
      this.metrics.fightPower = 0;
      this.metrics.shoutAmplitude = 0;
      this.metrics.punchForce = 0;
      this.metrics.punchMagnitude = 0;

      console.log('Arena SUSPENDED');
    },
    onPresenceDetected(): void {
      // If suspended, ignore all sensor input
      if (this.currentState === ArenaState.SUSPENDED) {
        return;
      }

      if (this.currentState === ArenaState.IDLE) {
        // Presence detected in idle, transition to warming
        this.transition(ArenaState.WARMING);
      }
    },

    // Handle shout detection
    onShoutDetected(amplitude: number): void {
      // If suspended, ignore all sensor input
      if (this.currentState === ArenaState.SUSPENDED) {
        return;
      }

      this.metrics.shoutAmplitude = amplitude;

      if (this.currentState === ArenaState.WARMING) {
        // Accumulate warming power based on shout amplitude and scale factor
        const powerGain = amplitude * this.config.warmingShoutScale;
        this.metrics.warmingPower = Math.min(
          100,
          this.metrics.warmingPower + powerGain
        );

        // Send LED command for shout detected
        console.log('StateMachine: Shout detected in WARMING, amplitude=%f, powerGain=%f', amplitude, powerGain);
        eventBus.emit(Events.LED_COMMAND, {
          arenaState: ArenaState.WARMING,
          trigger: 'shout_detected',
          triggerAmplitude: amplitude,
          currentPower: this.metrics.warmingPower
        });

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

        // Send LED command for shout detected
        eventBus.emit(Events.LED_COMMAND, {
          arenaState: ArenaState.FIGHT,
          trigger: 'shout_detected',
          triggerAmplitude: amplitude,
          currentPower: this.metrics.fightPower
        });

        // Bar can fill to 100% but fight continues until timeout or inactivity
      }

      // Emit metrics update
      eventBus.emit(Events.METRICS_UPDATED, this.metrics);
    },

    // Handle punch detection
    onPunchDetected(force: number, magnitude: number): void {
      // If suspended, ignore all sensor input
      if (this.currentState === ArenaState.SUSPENDED) {
        return;
      }

      this.metrics.punchForce = force;
      this.metrics.punchMagnitude = magnitude;

      if (this.currentState === ArenaState.FIGHT) {
        // Track activity
        this.lastActivityTime = Date.now();

        // Punches contribute to fight power using scale factor
        const powerGain = force * this.config.fightPunchScale;
        this.metrics.fightPower = Math.min(
          100,
          this.metrics.fightPower + powerGain
        );

        // Send LED command for punch detected
        eventBus.emit(Events.LED_COMMAND, {
          arenaState: ArenaState.FIGHT,
          trigger: 'punch_detected',
          triggerAmplitude: force,
          currentPower: this.metrics.fightPower
        });

        // Bar can fill to 100% but fight continues until timeout or inactivity
      }

      // Emit metrics update
      eventBus.emit(Events.METRICS_UPDATED, this.metrics);
    },

    // Update configuration
    updateConfig(newConfig: Partial<StateConfig>): void {
      this.config = { ...this.config, ...newConfig };
      eventBus.emit(Events.CONFIG_UPDATED, this.config);
    },

    // Update schedule configuration
    updateScheduleConfig(newScheduleConfig: ScheduleConfig): void {
      this.config.schedule = { ...this.config.schedule, ...newScheduleConfig };
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

          // Emit LED command for power decay
          eventBus.emit(Events.LED_COMMAND, {
            arenaState: ArenaState.WARMING,
            trigger: 'power_decay',
            currentPower: this.metrics.warmingPower
          });

          eventBus.emit(Events.METRICS_UPDATED, this.metrics);
        } else if (this.currentState === ArenaState.FIGHT) {
          const decayAmount = this.config.fightDecayRate * decayInterval;
          this.metrics.fightPower = Math.max(0, this.metrics.fightPower - decayAmount);

          // Emit LED command for power decay
          eventBus.emit(Events.LED_COMMAND, {
            arenaState: ArenaState.FIGHT,
            trigger: 'power_decay',
            currentPower: this.metrics.fightPower
          });

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

    // Suspend the arena (can be called manually or by schedule)
    suspend(): boolean {
      // Can suspend from any state
      return this.transition(ArenaState.SUSPENDED, true); // Force transition
    },

    // Resume from suspension (manual only)
    resumeFromSuspension(): boolean {
      if (this.currentState !== ArenaState.SUSPENDED) {
        return false;
      }
      return this.transition(ArenaState.IDLE, true); // Force to IDLE
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
