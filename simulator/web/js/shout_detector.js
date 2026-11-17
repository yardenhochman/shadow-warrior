// Port of brain-rs/src/services/shout_detector.rs
import { FRAME_SECS } from './audio_frontend.js';

export class ShoutDetector {
  constructor() {
    this.lastIndex = null; this.hangoverLeft = 0; this.score = 0;
    this.runFrames = 0; this.gap = 0; this.strengthSum = 0;
    this.last = null; // sim-only: diagnostics of the last frame for the meter
  }

  /** Feed one frame; frames skipped (index gaps up to 8) are credited. */
  push(f, cfg) {
    let missed = 1;
    if (this.lastIndex != null && f.index > this.lastIndex) missed = Math.min(Math.max(f.index - this.lastIndex, 1), 8);
    this.lastIndex = f.index;
    let out = null;
    for (let i = 0; i < missed; i++) { const d = this.step(f, cfg); if (d) out = d; }
    return out;
  }

  gateOpen(f, cfg) {
    if (f.vad_prob > cfg.vad_threshold) { this.hangoverLeft = cfg.vad_hangover_frames; return true; }
    if (this.hangoverLeft > 0) { this.hangoverLeft -= 1; return true; }
    return false;
  }

  /** Returns strength 1..2 when the frame looks like a near shout, else null. */
  frameIsShouty(f, cfg) {
    const gate = this.gateOpen(f, cfg);
    const margin = cfg.shout_margin_db + (f.playback_active ? cfg.shout_playback_margin_db : 0);
    const voiced = f.pitch_conf >= cfg.shout_pitch_conf_min && f.pitch_hz >= cfg.shout_f0_min_hz && f.pitch_hz <= cfg.shout_f0_max_hz;
    const checks = {
      voiced,
      centroid: f.spectral_centroid_hz >= cfg.shout_centroid_min_hz,
      tilt: f.spectral_tilt_db_per_khz >= cfg.shout_tilt_min_db_per_khz,
      flatness: f.spectral_flatness <= cfg.shout_flatness_max,
    };
    const passed = Object.values(checks).filter(Boolean).length;
    const loud = f.db_above_floor >= margin;
    this.last = { gate, margin, loud, checks, passed };
    if (!gate || !loud) return null;
    if (passed < cfg.shout_min_features) return null;
    return Math.min((f.db_above_floor - margin) / 10 + 1, 2);
  }

  step(f, cfg) {
    const strength = this.frameIsShouty(f, cfg);
    this.last.shouty = strength != null;
    if (strength != null) {
      this.gap = 0;
      this.runFrames += 1;
      this.strengthSum += strength;
      this.score += strength * cfg.shout_sensitivity;
      const dur = this.runFrames * FRAME_SECS;
      if (this.score >= cfg.shout_score_threshold && dur >= cfg.shout_min_duration_sec) {
        const avg = this.strengthSum / this.runFrames;
        const det = { intensity: Math.min(Math.max(avg * 0.5, 0.25), 1), duration_sec: dur, level_db: f.band_db };
        this.resetRun();
        this.score = 0;
        return det;
      }
    } else {
      this.gap += 1;
      this.score *= cfg.shout_score_decay;
      if (this.score < 0.1) this.score = 0;
      if (this.gap > cfg.shout_gap_frames) this.resetRun();
    }
    return null;
  }

  resetRun() { this.runFrames = 0; this.gap = 0; this.strengthSum = 0; }
}
