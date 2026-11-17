// Port of brain-rs/src/services/arena_manager.rs and EnergyBar
// (brain-rs/src/services/audio_manager.rs). Tokio tasks become sim-clock
// timers driven by tick(now). Timer phases match tokio::time::interval
// started at process start (t = 0).
import { ArenaState } from './state_machine.js';

/** EnergyBar (audio_manager.rs) */
export class EnergyBar {
  constructor(max, multiplier, decayRate, eventBus) {
    this.current = 0; this.max = max; this.multiplier = multiplier; this.decayRate = decayRate;
    this.eventBus = eventBus;
  }
  updateParameters(max, multiplier, decayRate) { this.max = max; this.multiplier = multiplier; this.decayRate = decayRate; }
  addEnergy(intensity) {
    const added = intensity * this.multiplier;
    this.current = Math.min(this.current + added, this.max);
    this.eventBus.publish({ type: 'EnergyChanged', energy: this.current, max: this.max, added });
    if (this.current >= this.max) this.eventBus.publish({ type: 'EnergyFull' });
  }
  decay(deltaSec) {
    this.current = Math.max(this.current - this.decayRate * deltaSec, 0);
    this.eventBus.publish({ type: 'EnergyChanged', energy: this.current, max: this.max, added: 0 });
  }
}

/** chrono Duration::num_seconds truncates toward zero */
const numSeconds = (s) => Math.trunc(s);

export class ArenaManager {
  /**
   * @param {object} config live tunables object (read on every use, like the RwLock<ArenaConfig>)
   * @param {object} fixes sim toggles for brain-rs bugs, see README
   */
  constructor(config, stateMachine, eventBus, now, fixes) {
    this.config = config;
    this.stateMachine = stateMachine;
    this.eventBus = eventBus;
    this.now = now;
    this.fixes = fixes;
    this.energyBar = null;
    this.fightStartedAt = null;
    this.lastActivityAt = now(); // Utc::now() at construction; only punches update it
    this.totalSessions = 0;
    this.currentSessionPunches = 0;
    this.currentSessionMaxPower = 0;
    this.scheduleGate = true;
    this.present = false; // sim-only: last PresenceDetected value (used by the re-arm fix)
    this.cooldownTimers = [];
    this.nextDecayTick = 0;
    this.nextMonitorTick = 0;
    this.lastEndReason = null;
  }

  start() {
    this.stateMachine.registerHook(this.makeHook());
    this.eventBus.subscribe((e) => this.handleEvent(e));
  }

  handleEvent(event) {
    switch (event.type) {
      case 'PresenceDetected': this.handlePresence(event.detected); break;
      case 'ShoutDetected': this.handleShout(event.intensity); break;
      case 'PunchDetected': this.handlePunch(event.power); break;
      case 'EnergyFull': this.handleEnergyFull(); break;
      default: break;
    }
  }

  handlePresence(detected) {
    if (detected && !this.scheduleGate) return;
    this.present = detected;
    const S = ArenaState;
    const current = this.stateMachine.currentState;
    if (detected) {
      if (current === S.Suspended) {
        this.stateMachine.transition(S.Idle);
        // brain-rs stops here: PresenceDetected only fires on change, so a player
        // arriving while SUSPENDED never reaches WARMING. Sim fix continues.
        if (this.fixes.rearmPresence) this.stateMachine.transition(S.Warming);
      } else if (current === S.Idle) {
        this.stateMachine.transition(S.Warming);
      }
    } else if (current !== S.Suspended) {
      this.stateMachine.transition(S.Suspended);
    }
  }

  handleShout(intensity) {
    if (this.stateMachine.currentState === ArenaState.Warming && this.energyBar) {
      this.energyBar.addEnergy(intensity);
    }
  }

  /** returns true if the punch counted (FIGHT) */
  handlePunch(power) {
    if (this.stateMachine.currentState !== ArenaState.Fight) return false;
    this.currentSessionPunches += 1;
    if (power > this.currentSessionMaxPower) this.currentSessionMaxPower = power;
    this.lastActivityAt = this.now();
    return true;
  }

  handleEnergyFull() {
    if (this.stateMachine.currentState === ArenaState.Warming) this.stateMachine.transition(ArenaState.Fight);
  }

  /** Drive the background tasks up to sim time `now`. */
  tick(now) {
    // start_energy_decay_task: 100 ms interval, decay(0.1) while WARMING
    while (now >= this.nextDecayTick) {
      this.nextDecayTick += 0.1;
      if (this.stateMachine.currentState === ArenaState.Warming && this.energyBar) this.energyBar.decay(0.1);
    }
    // start_fight_monitor_task: 1 s interval
    while (now >= this.nextMonitorTick) {
      const t = this.nextMonitorTick;
      this.nextMonitorTick += 1;
      this.fightMonitor(t);
    }
    // Cooldown sleep tasks spawned in on_enter(Cooldown)
    const due = this.cooldownTimers.filter((at) => now >= at);
    this.cooldownTimers = this.cooldownTimers.filter((at) => now < at);
    for (const _ of due) {
      if (this.stateMachine.currentState === ArenaState.Cooldown) this.stateMachine.transition(ArenaState.Idle);
    }
  }

  fightMonitor(t) {
    if (this.stateMachine.currentState !== ArenaState.Fight || this.fightStartedAt == null) return;
    const elapsed = t - this.fightStartedAt;
    if (numSeconds(elapsed) >= this.config.fight_duration_sec) {
      this.lastEndReason = 'TimeElapsed';
      this.eventBus.publish({ type: 'FightEnded', reason: 'TimeElapsed' });
      this.stateMachine.transition(ArenaState.Cooldown);
      return;
    }
    const inactive = t - this.lastActivityAt;
    if (numSeconds(inactive) >= this.config.fight_inactivity_timeout_sec) {
      this.lastEndReason = 'InactivityTimeout';
      this.eventBus.publish({ type: 'FightEnded', reason: 'InactivityTimeout' });
      this.stateMachine.transition(ArenaState.Cooldown);
    }
  }

  /** Sim-only (proposal): KO ends the fight early. */
  endFightEarly(reason) {
    if (this.stateMachine.currentState !== ArenaState.Fight) return;
    this.lastEndReason = reason;
    this.eventBus.publish({ type: 'FightEnded', reason });
    this.stateMachine.transition(ArenaState.Cooldown);
  }

  /** get_state_info */
  getStateInfo() {
    const now = this.now();
    const s = this.stateMachine.currentState;
    const info = { currentState: s, timeInStateSec: now - this.stateMachine.stateEnteredAt };
    if (s === ArenaState.Warming && this.energyBar) {
      info.energy = this.energyBar.current;
      info.maxEnergy = this.config.warming_energy_threshold;
      info.energyPercentage = info.energy / info.maxEnergy;
    } else if (s === ArenaState.Fight && this.fightStartedAt != null) {
      const elapsed = now - this.fightStartedAt;
      info.fightElapsedSec = elapsed;
      info.fightRemainingSec = Math.max(this.config.fight_duration_sec - elapsed, 0);
      info.timeSinceLastActivitySec = now - this.lastActivityAt;
    } else if (s === ArenaState.Cooldown) {
      const at = this.cooldownTimers.length ? Math.max(...this.cooldownTimers) : now;
      info.cooldownRemainingSec = Math.max(at - now, 0); // sim-only convenience
    }
    return info;
  }

  /** update_tunables: also pushes into the live energy bar */
  onTunablesChanged() {
    this.energyBar?.updateParameters(this.config.warming_energy_threshold, this.config.shout_energy_multiplier, this.config.energy_decay_rate);
  }

  makeHook() {
    const self = this;
    return {
      onEnter(state) {
        const S = ArenaState;
        switch (state) {
          case S.Idle:
            self.eventBus.publish({ type: 'SessionStarted' });
            // Sim fix: brain-rs never re-checks presence on entering IDLE, so a player
            // still present after COOLDOWN is stuck in IDLE until they leave.
            if (self.fixes.rearmPresence && self.present) {
              queueMicrotask(() => { if (self.stateMachine.currentState === S.Idle && self.present) self.stateMachine.transition(S.Warming); });
            }
            break;
          case S.Warming:
            self.energyBar = new EnergyBar(self.config.warming_energy_threshold, self.config.shout_energy_multiplier, self.config.energy_decay_rate, self.eventBus);
            break;
          case S.Fight:
            self.fightStartedAt = self.now();
            self.currentSessionPunches = 0;
            self.currentSessionMaxPower = 0;
            self.totalSessions += 1;
            // brain-rs bug: last_activity_at is not reset here (see README).
            if (self.fixes.resetInactivityOnFight) self.lastActivityAt = self.now();
            self.eventBus.publish({ type: 'FightStarted' });
            break;
          case S.Cooldown:
            self.eventBus.publish({ type: 'SessionEnded' });
            // tokio::spawn(sleep(cooldown_duration_sec)) -> Idle if still Cooldown
            self.cooldownTimers.push(self.now() + self.config.cooldown_duration_sec);
            break;
          default: break;
        }
      },
      onExit(state) {
        if (state === ArenaState.Warming) self.energyBar = null;
        if (state === ArenaState.Fight) self.fightStartedAt = null;
      },
    };
  }
}
