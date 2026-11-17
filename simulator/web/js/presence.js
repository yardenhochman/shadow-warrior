// Port of brain-rs/src/services/presence.rs
import { FRAME_SECS } from './audio_frontend.js';

export const SOURCE_MIC = 'mic';
export const SOURCE_PUNCH = 'punch';
export const SOURCE_SIM_SENSOR = 'sim_sensor'; // SIM PROPOSAL: the "someone is in the arena" toggle

const SOURCE_WEIGHTS = { mic: 1.0, punch: 1.0 }; // presence.source_weights default
const DEFAULT_WEIGHT = 1.0;

/** MicSource::observe: near-field gate (VAD + margin over adaptive floor). */
export function micObserve(f, cfg) {
  const margin = cfg.mic_margin_db + (f.playback_active ? cfg.mic_playback_margin_db : 0);
  if (f.vad_prob <= cfg.mic_vad_threshold || f.db_above_floor < margin) return null;
  const strength = Math.min(0.75 + (f.db_above_floor - margin) / 20, 1);
  return strength * FRAME_SECS;
}

/** PunchSource::observe: any punch counts. */
export function punchObserve(_power, cfg) { return cfg.punch_units; }

/** PresenceFusion: leaky accumulator + enter threshold + leave timeout. Time in seconds. */
export class PresenceFusion {
  constructor() { this.score = 0; this.present = false; this.lastUpdate = 0; this.lastEvidence = 0; this.started = false; }
  static weight(source) { return SOURCE_WEIGHTS[source] ?? DEFAULT_WEIGHT; }
  advance(now, cfg) {
    if (!this.started) { this.started = true; this.lastUpdate = now; return; }
    const dt = Math.max(now - this.lastUpdate, 0);
    this.lastUpdate = Math.max(now, this.lastUpdate);
    this.score = Math.max(this.score - cfg.decay_per_sec * dt, 0);
  }
  add(now, ev, cfg) {
    this.advance(now, cfg);
    const units = ev.units * PresenceFusion.weight(ev.source);
    if (units <= 0) return null;
    this.lastEvidence = now;
    this.score = Math.min(this.score + units, Math.max(cfg.max_score, cfg.enter_threshold));
    if (!this.present && this.score >= cfg.enter_threshold) { this.present = true; return true; }
    return null;
  }
  force(now, present, cfg) {
    this.advance(now, cfg);
    const changed = this.present !== present;
    this.present = present;
    if (present) { this.lastEvidence = now; this.score = cfg.enter_threshold; } else this.score = 0;
    return changed ? present : null;
  }
  tick(now, cfg) {
    this.advance(now, cfg);
    if (this.present && now - this.lastEvidence >= cfg.leave_timeout_sec) {
      this.present = false; this.score = 0; return false;
    }
    return null;
  }
}

/** The runtime `step` + PresenceService glue: publishes PresenceDetected on change. */
export class PresenceService {
  constructor(config, eventBus, now) {
    this.config = config; this.eventBus = eventBus; this.now = now;
    this.fusion = new PresenceFusion();
    this.lastIndex = null;
    this.nextTick = 0;
    eventBus.subscribe((e) => {
      if (e.type === 'PunchDetected') {
        const u = punchObserve(e.power, this.config);
        if (u != null) this.step([{ source: SOURCE_PUNCH, units: u }]);
      }
    });
  }
  step(evidence = [], force = null) {
    const now = this.now();
    const out = [];
    for (const e of evidence) { const r = this.fusion.add(now, e, this.config); if (r != null) out.push(r); }
    const t = this.fusion.tick(now, this.config); if (t != null) out.push(t);
    if (force != null) { const r = this.fusion.force(now, force, this.config); if (r != null) out.push(r); }
    for (const detected of out) this.eventBus.publish({ type: 'PresenceDetected', detected });
  }
  onFrame(f) {
    let missed = 1;
    if (this.lastIndex != null && f.index > this.lastIndex) missed = Math.min(Math.max(f.index - this.lastIndex, 1), 8);
    this.lastIndex = f.index;
    const u = micObserve(f, this.config);
    this.step(u != null ? [{ source: SOURCE_MIC, units: u * missed }] : []);
  }
  /** 250 ms ticker; `sensorOn` adds SIM PROPOSAL sensor evidence (1 evidence-second per second). */
  tick(now, sensorOn) {
    while (now >= this.nextTick) {
      this.nextTick += 0.25;
      this.step(sensorOn ? [{ source: SOURCE_SIM_SENSOR, units: 0.25 }] : []);
    }
  }
  forcePresent(p) { this.step([], p); }
  secondsSinceEvidence() { return this.now() - this.fusion.lastEvidence; }
}
