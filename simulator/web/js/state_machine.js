// Port of brain-rs/src/core/state_machine.rs
export const ArenaState = Object.freeze({
  Suspended: 'SUSPENDED',
  Idle: 'IDLE',
  Warming: 'WARMING',
  Fight: 'FIGHT',
  Cooldown: 'COOLDOWN',
});

/** ArenaState::can_transition_to */
export function canTransitionTo(from, to) {
  const S = ArenaState;
  if (to === S.Suspended) return true; // (_, Suspended): can always suspend
  const ok = [
    [S.Suspended, S.Idle], [S.Idle, S.Suspended],
    [S.Idle, S.Warming], [S.Warming, S.Fight], [S.Fight, S.Cooldown], [S.Cooldown, S.Idle],
    [S.Warming, S.Idle], [S.Fight, S.Idle],
  ];
  return ok.some(([a, b]) => a === from && b === to);
}

export class StateMachine {
  /** @param {import('./events.js').EventBus} eventBus @param {() => number} now sim seconds */
  constructor(eventBus, now) {
    this.eventBus = eventBus;
    this.now = now;
    this.currentState = ArenaState.Suspended; // StateMachine::new starts Suspended
    this.stateEnteredAt = now();
    this.hooks = [];
  }
  registerHook(hook) { this.hooks.push(hook); }

  /** Returns true on success, false on InvalidTransition (Rust returns Err). */
  transition(to) {
    const from = this.currentState;
    if (from === to) return true;
    if (!canTransitionTo(from, to)) return false;
    for (const h of this.hooks) h.onExit?.(from);
    this.currentState = to;
    this.stateEnteredAt = this.now();
    this.eventBus.publish({ type: 'StateChanged', from, to });
    for (const h of this.hooks) h.onEnter?.(to);
    for (const h of this.hooks) h.onTransition?.(from, to);
    return true;
  }
}
