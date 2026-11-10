// Tunables: defaults mirror brain-rs/config.yaml (and brain-rs/src/config.rs defaults).
// `section` + `key` are the config.yaml location; `int` keys are integers in Rust.
// Keys with section "sim" exist only in the simulator and are never exported.

export const TUNABLES = [
  // arena (brain-rs/src/config.rs ArenaConfig)
  { section: 'arena', key: 'warming_energy_threshold', def: 100.0, min: 10, max: 300, step: 5, help: 'Energy points to trigger FIGHT' },
  { section: 'arena', key: 'shout_energy_multiplier', def: 10.0, min: 1, max: 50, step: 0.5, help: 'Energy per shout = intensity (0.25..1) x this' },
  { section: 'arena', key: 'energy_decay_rate', def: 0.5, min: 0, max: 10, step: 0.1, help: 'Energy points lost per second in WARMING' },
  { section: 'arena', key: 'fight_duration_sec', def: 180, min: 10, max: 600, step: 5, int: true, help: 'FIGHT ends after this long' },
  { section: 'arena', key: 'fight_inactivity_timeout_sec', def: 30, min: 3, max: 180, step: 1, int: true, help: 'FIGHT ends if no punch this long' },
  { section: 'arena', key: 'cooldown_duration_sec', def: 60, min: 3, max: 300, step: 1, int: true, help: 'COOLDOWN -> IDLE after this long' },

  // audio (brain-rs/src/config.rs AudioConfig, used by shout_detector.rs)
  { section: 'audio', key: 'vad_threshold', def: 0.5, min: 0, max: 1, step: 0.05, help: 'VAD prob to open the gate' },
  { section: 'audio', key: 'vad_hangover_frames', def: 6, min: 0, max: 30, step: 1, int: true, help: 'Gate stays open N frames (16ms) after VAD' },
  { section: 'audio', key: 'shout_margin_db', def: 15.0, min: 0, max: 40, step: 0.5, help: 'dB above adaptive noise floor' },
  { section: 'audio', key: 'shout_playback_margin_db', def: 8.0, min: 0, max: 30, step: 0.5, help: 'Extra margin while speakers play (FIGHT)' },
  { section: 'audio', key: 'shout_f0_min_hz', def: 150.0, min: 50, max: 500, step: 10, help: 'Voiced pitch range (min)' },
  { section: 'audio', key: 'shout_f0_max_hz', def: 700.0, min: 200, max: 1000, step: 10, help: 'Voiced pitch range (max; front-end only reports <=500)' },
  { section: 'audio', key: 'shout_pitch_conf_min', def: 0.4, min: 0, max: 1, step: 0.05, help: 'Autocorr confidence to count as voiced' },
  { section: 'audio', key: 'shout_centroid_min_hz', def: 900.0, min: 200, max: 3000, step: 50, help: 'Spectral centroid floor' },
  { section: 'audio', key: 'shout_tilt_min_db_per_khz', def: -25.0, min: -60, max: 0, step: 1, help: 'Spectral tilt floor' },
  { section: 'audio', key: 'shout_flatness_max', def: 0.5, min: 0, max: 1, step: 0.05, help: 'Reject noise-like frames' },
  { section: 'audio', key: 'shout_min_features', def: 3, min: 0, max: 4, step: 1, int: true, help: 'of 4 checks required' },
  { section: 'audio', key: 'shout_min_duration_sec', def: 0.2, min: 0, max: 1.5, step: 0.016, help: 'Sustained shouty time before firing' },
  { section: 'audio', key: 'shout_gap_frames', def: 3, min: 0, max: 20, step: 1, int: true, help: 'Non-shouty frames tolerated in a shout' },
  { section: 'audio', key: 'shout_sensitivity', def: 1.0, min: 0.1, max: 5, step: 0.1, help: 'Score gain per shouty frame' },
  { section: 'audio', key: 'shout_score_threshold', def: 8.0, min: 1, max: 40, step: 0.5, help: 'Score at which ShoutDetected fires' },
  { section: 'audio', key: 'shout_score_decay', def: 0.9, min: 0.5, max: 1, step: 0.01, help: 'Per-frame decay when not shouty' },

  // presence (brain-rs/src/config.rs PresenceConfig, used by presence.rs)
  { section: 'presence', key: 'mic_margin_db', def: 10.0, min: 0, max: 40, step: 0.5, help: 'dB above floor for a mic frame to count' },
  { section: 'presence', key: 'mic_playback_margin_db', def: 8.0, min: 0, max: 30, step: 0.5, help: 'Extra margin while speakers play' },
  { section: 'presence', key: 'mic_vad_threshold', def: 0.5, min: 0, max: 1, step: 0.05, help: 'VAD prob for a mic frame to count' },
  { section: 'presence', key: 'punch_units', def: 1.0, min: 0, max: 5, step: 0.1, help: 'Evidence-seconds per punch' },
  { section: 'presence', key: 'enter_threshold', def: 1.0, min: 0.1, max: 10, step: 0.1, help: 'Evidence-seconds to enter' },
  { section: 'presence', key: 'decay_per_sec', def: 0.2, min: 0, max: 2, step: 0.05, help: 'Accumulator leak per second' },
  { section: 'presence', key: 'max_score', def: 2.0, min: 0.1, max: 10, step: 0.1, help: 'Accumulator cap' },
  { section: 'presence', key: 'leave_timeout_sec', def: 60.0, min: 2, max: 300, step: 1, help: 'No evidence this long -> absent' },

  // simulator-only (not in config.yaml)
  { section: 'sim', key: 'time_scale', def: 1, min: 1, max: 20, step: 1, int: true, help: 'Fast-forward the game clock (mic stays real time)' },
  { section: 'sim', key: 'shadow_hp', def: 250, min: 20, max: 2000, step: 10, int: true, help: 'Opponent health (proposal)' },
  { section: 'sim', key: 'damage_per_power', def: 0.3, min: 0.05, max: 2, step: 0.05, help: 'HP damage per punch power point' },
  { section: 'sim', key: 'synth_shout_db', def: 24, min: 0, max: 45, step: 1, int: true, help: 'Shout button: dB above floor' },
  { section: 'sim', key: 'leds_per_column', def: 90, min: 10, max: 300, step: 5, int: true, help: 'LEDs per Perspex column' },
];

const LS_KEY = 'sw-sim-tunables-v1';

export function defaults() {
  const o = {};
  for (const t of TUNABLES) o[t.key] = t.def;
  return o;
}

export function loadTunables() {
  const o = defaults();
  try {
    const saved = JSON.parse(localStorage.getItem(LS_KEY) || '{}');
    for (const t of TUNABLES) if (typeof saved[t.key] === 'number') o[t.key] = saved[t.key];
  } catch { /* ignore */ }
  return o;
}

export function saveTunables(cfg) {
  try { localStorage.setItem(LS_KEY, JSON.stringify(cfg)); } catch { /* ignore */ }
}

function fmt(t, v) {
  if (t.int) return String(Math.round(v));
  const s = String(Math.round(v * 1000) / 1000);
  return s.includes('.') ? s : s + '.0';
}

export function formatValue(t, v) { return fmt(t, v); }

/** Changed (non-sim) keys as config.yaml fragments, grouped by section. */
export function exportYaml(cfg) {
  const bySection = new Map();
  for (const t of TUNABLES) {
    if (t.section === 'sim') continue;
    const v = cfg[t.key];
    if (fmt(t, v) === fmt(t, t.def)) continue;
    if (!bySection.has(t.section)) bySection.set(t.section, []);
    bySection.get(t.section).push(`  ${t.key}: ${fmt(t, v)}  # default ${fmt(t, t.def)}`);
  }
  if (!bySection.size) return '# No changes from brain-rs/config.yaml defaults.';
  const out = ['# Paste/merge into brain-rs/config.yaml (changed keys only)'];
  for (const [s, lines] of bySection) out.push(`${s}:`, ...lines);
  return out.join('\n');
}
