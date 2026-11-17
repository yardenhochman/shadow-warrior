// Tunables: defaults mirror brain-rs/config.yaml (and brain-rs/src/config.rs defaults).
// `section` + `key` are the config.yaml location; `int` keys are integers in Rust.
// Keys with section "sim" exist only in the simulator and are never exported.

export const SECTIONS = {
  arena: 'Game flow',
  audio: 'Shout detection',
  presence: 'Presence',
  sim: 'Simulator only (not exported)',
};

// Per tunable: `name` (plain-English label), `unit` (shown next to the value), `help` (one line),
// `more` (2-4 sentences: what it does, game feel, interactions). `key` stays the exported config.yaml key.
export const TUNABLES = [
  // arena (brain-rs/src/config.rs ArenaConfig)
  { section: 'arena', key: 'warming_energy_threshold', def: 100.0, min: 10, max: 300, step: 5, name: 'Energy needed to start a fight', unit: 'pts',
    help: 'How full the energy bar must get while the shadow is warming up. When it fills, the fight starts.',
    more: 'This is the size of the energy bar. Each shout adds (shout intensity 0.5 to 1) x "Energy gained per shout", and the bar drains over time, so the number of shouts needed is roughly this value divided by the energy per shout. With the defaults (100 and 10) that is about 10 full-strength shouts, up to 20 barely-over-the-limit ones. Raise it for a longer build-up, lower it for a quick start.' },
  { section: 'arena', key: 'shout_energy_multiplier', def: 10.0, min: 1, max: 50, step: 0.5, name: 'Energy gained per shout', unit: 'pts',
    help: 'Energy added by one full-strength shout. Weaker shouts add less (down to half of this).',
    more: 'A detected shout has an intensity between 0.5 (barely over the loudness limit) and 1 (about 10 dB or more over it); the energy added is strength x this value. Higher means fewer shouts fill the bar. Compare it to "Energy needed to start a fight" and "Energy drain while waiting": shouts only win if their energy outpaces the drain. Shouts outside the warming-up phase add nothing.' },
  { section: 'arena', key: 'energy_decay_rate', def: 0.5, min: 0, max: 10, step: 0.1, name: 'Energy drain while waiting', unit: 'pts/s',
    help: 'Energy the bar loses every second while the shadow is warming up.',
    more: 'The bar leaks continuously during warming up (checked every 0.1 s), never below zero, so players must keep shouting rather than shout once and wait. At the default 0.5 pts/s a full-strength shout (10 pts) is lost after 20 s. Raise it to demand a steady stream of shouts, set 0 for a bar that never leaks. The bar starts empty each time warming up begins.' },
  { section: 'arena', key: 'fight_duration_sec', def: 180, min: 10, max: 600, step: 5, int: true, name: 'Fight length', unit: 's',
    help: 'Maximum time a fight lasts before it ends and the cooldown starts.',
    more: 'The fight is checked once per second and ends as soon as this many whole seconds have passed since it started, even if punches are still landing. It is a hard cap; the fight can also end sooner through "End fight after no punches for" (or a knockout, if that simulator option is on). After it ends the cooldown begins.' },
  { section: 'arena', key: 'fight_inactivity_timeout_sec', def: 30, min: 3, max: 180, step: 1, int: true, name: 'End fight after no punches for', unit: 's',
    help: 'If nobody punches the bag for this long, the fight ends early.',
    more: 'Measured from the last counted punch and checked once per second. Lower values end fights quickly when players stop; higher values are more forgiving. It should be shorter than "Fight length" to matter. Note: brain-rs does not restart this timer when a fight begins, so a long gap before the first punch can end the fight at once (the "reset inactivity timer" fidelity fix changes that).' },
  { section: 'arena', key: 'cooldown_duration_sec', def: 60, min: 3, max: 300, step: 1, int: true, name: 'Cooldown length', unit: 's',
    help: 'Pause after a fight before the arena goes back to idle and can start again.',
    more: 'During the cooldown shouts and new fights are ignored. When it runs out the arena returns to idle, and a new round needs people present again before it warms up. Lower means quicker repeat rounds; higher gives the station a longer rest between groups.' },

  // audio (brain-rs/src/config.rs AudioConfig, used by shout_detector.rs)
  { section: 'audio', key: 'vad_threshold', def: 0.5, min: 0, max: 1, step: 0.05, name: 'Voice detector confidence needed', unit: '0-1',
    help: 'How sure the voice detector must be that it hears a voice before shouts are considered.',
    more: 'The voice detector (VAD) gives each 16 ms frame a probability of containing speech. Above this value the gate opens and the frame can count as a shout; below it the frame is ignored, however loud. Raise it to ignore bangs and music, lower it to accept more sounds. In this simulator the detector is an approximation (the real brain uses a neural model), so exact values will not match the device.' },
  { section: 'audio', key: 'vad_hangover_frames', def: 6, min: 0, max: 30, step: 1, int: true, name: 'Voice gate hold time', unit: 'frames (16 ms)',
    help: 'How long the voice gate stays open after the voice detector stops hearing a voice.',
    more: 'Voice detectors flicker on and off within a single shout; holding the gate open for a few frames bridges those drops. The default 6 frames is about 0.1 s. Too high lets the tail of a shout or an echo count as voice; 0 closes the gate the instant the detector drops.' },
  { section: 'audio', key: 'shout_margin_db', def: 15.0, min: 0, max: 40, step: 0.5, name: 'Loudness above background needed', unit: 'dB',
    help: 'How much louder than the room noise a sound must be to count as a shout.',
    more: 'The detector tracks the background level (falls quickly, rises slowly), and a frame must exceed it by this many dB. It is the main sensitivity control: lower and quieter or more distant people trigger shouts, higher and only close, loud shouts do. Shout strength grows with every dB above this limit, up to 10 dB over it. While music plays, "Extra loudness needed while music plays" is added.' },
  { section: 'audio', key: 'shout_playback_margin_db', def: 8.0, min: 0, max: 30, step: 0.5, name: 'Extra loudness needed while music plays', unit: 'dB',
    help: 'Extra loudness required while the station speakers are playing fight music.',
    more: 'Added on top of "Loudness above background needed" whenever the speakers are playing, so the music itself is not heard as a shout. At the defaults a shout needs 15 dB over background, or 23 dB during music. Raise it if the music leaks into the microphone; lower it if players are hard to hear over the music.' },
  { section: 'audio', key: 'shout_f0_min_hz', def: 150.0, min: 50, max: 500, step: 10, name: 'Lowest voice pitch accepted', unit: 'Hz',
    help: 'Sounds with a pitch below this are not treated as a shout voice.',
    more: 'One of four voice checks ("Voice checks that must pass"): the "voiced" check needs a clear pitch (see "Pitch clarity needed") between this and "Highest voice pitch accepted". Raising it filters out low rumble and bass, but can reject deep voices; the default 150 Hz is a shouting voice, not a speaking one.' },
  { section: 'audio', key: 'shout_f0_max_hz', def: 700.0, min: 200, max: 1000, step: 10, name: 'Highest voice pitch accepted', unit: 'Hz',
    help: 'Sounds with a pitch above this are not treated as a shout voice.',
    more: 'Upper end of the pitch range of the "voiced" check. The pitch analysis in the audio front-end only searches 70 to 500 Hz, so values above 500 make no difference in practice. Lower it to exclude screams and whistles.' },
  { section: 'audio', key: 'shout_pitch_conf_min', def: 0.4, min: 0, max: 1, step: 0.05, name: 'Pitch clarity needed', unit: '0-1',
    help: 'How clear and steady the pitch must be to count as a voice rather than noise.',
    more: 'The pitch measurement comes with a confidence from 0 (noise) to 1 (a clean tone). A frame passes the "voiced" check only if its confidence is at least this and its pitch is inside the accepted range. Higher is stricter against noise but can reject rough, strained shouts.' },
  { section: 'audio', key: 'shout_centroid_min_hz', def: 900.0, min: 200, max: 3000, step: 50, name: 'Minimum sound brightness', unit: 'Hz',
    help: 'How bright (rich in high frequencies) the sound must be. Shouts are brighter than normal speech.',
    more: 'The spectral centroid is the "centre of gravity" of the sound; it is higher for sharp, strained sounds. It is one of the four voice checks. Raise it to demand brighter shouts and filter out dull rumble and music bass; lower it to accept duller voices.' },
  { section: 'audio', key: 'shout_tilt_min_db_per_khz', def: -25.0, min: -60, max: 0, step: 1, name: 'Minimum treble strength', unit: 'dB/kHz',
    help: 'How much high-frequency energy must remain. Closer to 0 is stricter; more negative is looser.',
    more: 'Spectral tilt is how steeply the sound fades towards high frequencies, in dB per kHz; a shout fades less than a muffled voice or room rumble. A frame passes this check when its tilt is above this value. Moving it toward 0 requires crisper sound, moving it toward -60 accepts almost anything. It is one of the four voice checks.' },
  { section: 'audio', key: 'shout_flatness_max', def: 0.5, min: 0, max: 1, step: 0.05, name: 'Maximum noisiness', unit: '0-1',
    help: 'Rejects hiss-like sounds. Lower values are stricter, 1 accepts any noisiness.',
    more: 'Spectral flatness is 0 for a pure tone and near 1 for white noise, so hissing, clapping or wind score high. A frame passes this check when its flatness is at or below this limit. It is one of the four voice checks, alongside pitch, brightness and treble strength.' },
  { section: 'audio', key: 'shout_min_features', def: 3, min: 0, max: 4, step: 1, int: true, name: 'Voice checks that must pass', unit: 'of 4',
    help: 'How many of the four sound-shape checks (pitch, brightness, treble strength, noisiness) must pass.',
    more: 'A frame must be loud enough, pass the voice gate, and pass at least this many of the four checks. 4 is strictest and may miss real shouts; 0 skips the checks so any loud voiced sound counts. Individual check limits are set by the pitch, brightness, treble and noisiness tunables.' },
  { section: 'audio', key: 'shout_min_duration_sec', def: 0.2, min: 0, max: 1.5, step: 0.016, name: 'Minimum shout length', unit: 's',
    help: 'How long the shouting must go on before it counts. Shorter sounds are ignored.',
    more: 'The length is the number of qualifying 16 ms frames in the current run, so short gaps (see "Pause allowed inside a shout") do not restart it. Both this and "Score needed to count a shout" must be met. Raise it to ignore claps and short yelps; lower it for a snappier response.' },
  { section: 'audio', key: 'shout_gap_frames', def: 3, min: 0, max: 20, step: 1, int: true, name: 'Pause allowed inside a shout', unit: 'frames (16 ms)',
    help: 'How many quiet or non-shouty frames in a row are tolerated before the shout is considered over.',
    more: 'Real shouts waver, so a few failed frames do not cancel the run. When more than this many frames fail in a row, the run (its length and strength) restarts; the score keeps fading by "Score kept per quiet frame". The default 3 frames is about 50 ms.' },
  { section: 'audio', key: 'shout_sensitivity', def: 1.0, min: 0.1, max: 5, step: 0.1, name: 'Score gain per shout frame', unit: 'x',
    help: 'How fast the shout score builds up while someone is shouting.',
    more: 'Every qualifying frame adds its strength (1 to 2) times this to the score. With defaults, a quiet shout needs 8 frames (about 0.13 s) to reach the score threshold, so the minimum shout length usually decides the timing. Raise it to trigger faster and with less effort; lower it to need longer shouting. Together with the threshold and fade it sets how much sustained shouting is needed.' },
  { section: 'audio', key: 'shout_score_threshold', def: 8.0, min: 1, max: 40, step: 0.5, name: 'Score needed to count a shout', unit: 'pts',
    help: 'Once the shout score reaches this, the shout is counted and adds energy.',
    more: 'The score rises with qualifying frames (see "Score gain per shout frame") and fades when frames fail. After a shout fires the score resets to zero. Raise it to need longer or louder shouting per counted shout, which also means fewer shouts per second of noise.' },
  { section: 'audio', key: 'shout_score_decay', def: 0.9, min: 0.5, max: 1, step: 0.01, name: 'Score kept per quiet frame', unit: 'x per frame',
    help: 'How much of the shout score survives each quiet frame. Lower fades faster; 1 never fades.',
    more: 'On every non-qualifying frame the score is multiplied by this. At 0.9 about half is gone after 6 frames (0.1 s). Values near 1 let on-and-off noise add up to a shout, while lower values need a continuous shout. Scores below 0.1 are reset to zero.' },

  // presence (brain-rs/src/config.rs PresenceConfig, used by presence.rs)
  { section: 'presence', key: 'mic_margin_db', def: 10.0, min: 0, max: 40, step: 0.5, name: 'Loudness above background to notice someone', unit: 'dB',
    help: 'How much louder than the room a voice must be for the microphone to count as evidence that someone is there.',
    more: 'Presence is an accumulator fed by microphone sound and punches. Frames at least this far above the background level (and passing the voice check) add presence time; louder frames add slightly more, up to 1.33x. Lower it to notice quiet speech, raise it to need louder noise.' },
  { section: 'presence', key: 'mic_playback_margin_db', def: 8.0, min: 0, max: 30, step: 0.5, name: 'Extra loudness needed while music plays (presence)', unit: 'dB',
    help: 'Extra loudness required while the station speakers are playing, so its own music does not count as a person.',
    more: 'Added to "Loudness above background to notice someone" while the speakers are playing, the same idea as the shout equivalent. Raise it if the music alone keeps the arena marked as occupied.' },
  { section: 'presence', key: 'mic_vad_threshold', def: 0.5, min: 0, max: 1, step: 0.05, name: 'Voice confidence to notice someone', unit: '0-1',
    help: 'How sure the voice detector must be before microphone sound counts as a person being there.',
    more: 'Same voice detector as for shouts, but with its own limit, so presence can be more or less picky than shout detection. Raise it to ignore non-voice noise; lower it to count more sounds.' },
  { section: 'presence', key: 'punch_units', def: 1.0, min: 0, max: 5, step: 0.1, name: 'Presence credit per punch', unit: 's',
    help: 'How much "someone is here" evidence one punch on the bag gives.',
    more: 'Evidence is measured in seconds of voice. At the default 1.0, one punch equals "Presence needed to count someone as here" on its own, so a single punch marks the arena occupied. Set it to 0 to ignore punches for presence.' },
  { section: 'presence', key: 'enter_threshold', def: 1.0, min: 0.1, max: 10, step: 0.1, name: 'Presence needed to count someone as here', unit: 's',
    help: 'How much evidence must build up before the arena treats someone as present and wakes up.',
    more: 'The presence score collects evidence from voice (up to one second of evidence per second of loud voice) and punches. When it reaches this, the arena wakes and moves toward warming up. Higher needs sustained noise or several punches; lower reacts to a short sound. It also acts as a floor for "Presence cap".' },
  { section: 'presence', key: 'decay_per_sec', def: 0.2, min: 0, max: 2, step: 0.05, name: 'Presence fade rate', unit: 'per s',
    help: 'How fast collected presence evidence drains away while it is quiet.',
    more: 'Prevents scattered noises from adding up over a long time into a false detection: the score loses this much per second, down to zero. Higher requires the noise to be closer together; 0 never fades. It only changes how easily presence is entered, not how long it lasts once entered (see "Leave after no sign of life for").' },
  { section: 'presence', key: 'max_score', def: 2.0, min: 0.1, max: 10, step: 0.1, name: 'Presence cap', unit: 's',
    help: 'The most presence evidence that can be stored.',
    more: 'Limits how much evidence can pile up, so that after a burst of noise the score returns below the entry level quickly rather than staying high. The cap is never lower than "Presence needed to count someone as here".' },
  { section: 'presence', key: 'leave_timeout_sec', def: 60.0, min: 2, max: 300, step: 1, name: 'Leave after no sign of life for', unit: 's',
    help: 'How long without any voice or punch before the arena decides everyone has left.',
    more: 'Counted from the last evidence (a loud voice frame or a punch). When it runs out the arena is marked empty and goes to suspended from any state, even mid-fight. Raise it so quiet or fighting players are not dropped; lower it to sleep sooner.' },

  // simulator-only (not in config.yaml)
  { section: 'sim', key: 'energy_gain_start', def: 1.7, min: 0.5, max: 3, step: 0.05, name: 'First shouts boost', unit: 'x',
    help: 'How much more an early shout fills the bar. Higher = the bar jumps up quickly at the start.',
    more: 'Each shout\'s energy is multiplied by this on an empty bar, sliding evenly to "Last stretch gain" on a full one. With 1.7 and 0.65 the first strong shout fills 17%, four shouts reach almost 60%, and the last 20% takes three more: people feel close at once and have to push at the end. Set both to 1 for the plain brain-rs bar. Sim proposal, not exported.' },
  { section: 'sim', key: 'energy_gain_end', def: 0.65, min: 0.2, max: 1.5, step: 0.05, name: 'Last stretch gain', unit: 'x',
    help: 'How much a shout fills the bar when it is almost full. Lower = the end takes more effort.',
    more: 'The multiplier on a nearly full bar; see "First shouts boost". Lowering it adds shouts at the very end without slowing the start.' },
  { section: 'sim', key: 'time_scale', def: 1, min: 1, max: 20, step: 1, int: true, name: 'Game speed-up', unit: 'x',
    help: 'Runs the game clock faster so you can test long timers quickly. The microphone stays real time.',
    more: 'Multiplies the simulated clock: timers like fight length, cooldown and energy drain all run this much faster, while the microphone and shout timing do not. Handy for waiting out a 3 minute fight; keep it at 1 when judging how shouting feels.' },
  { section: 'sim', key: 'shadow_hp', def: 900, min: 20, max: 2000, step: 10, int: true, name: 'Shadow health', unit: 'HP',
    help: 'How much damage the shadow takes before it is knocked out.',
    more: 'A proposal for the game, not part of the brain. Each punch removes power x "Damage per punch power" (more with a combo), so with the defaults a medium punch of 50 does 15 damage: about 60 punches, or 30 to 45 s of steady punching with combos (real bag hits may read stronger until the sensor is calibrated). After a knockout the shadow comes back, or the fight ends if "knocking out the shadow ends the fight" is ticked.' },
  { section: 'sim', key: 'combo_max', def: 1.5, min: 1, max: 3, step: 0.1, name: 'Maximum combo bonus', unit: 'x',
    help: 'The most a fast run of punches can multiply damage by.',
    more: 'Each punch within a second of the previous one adds +10% damage, up to this cap. At 1.5 a run of 6 quick punches reaches the cap; at 1 combos do nothing. A high cap rewards fast flurries and shortens fights a lot.' },
  { section: 'sim', key: 'damage_per_power', def: 0.3, min: 0.05, max: 2, step: 0.05, name: 'Damage per punch power', unit: 'HP/pt',
    help: 'How much health a punch takes off per point of punch power (1 to 100).',
    more: 'Damage is punch power x this, multiplied by a combo bonus of +10% per punch landed within a second of the previous one (capped by "Maximum combo bonus"). Together with "Shadow health" it sets how many punches a KO takes. Simulator only.' },
  { section: 'sim', key: 'synth_shout_db', def: 24, min: 0, max: 45, step: 1, int: true, name: 'Shout button loudness', unit: 'dB',
    help: 'How loud the on-screen SHOUT button pretends to be, compared to the background.',
    more: 'The button feeds fake audio frames through the real detector, so this must clear "Loudness above background needed" (15 dB, or 23 dB during music) for it to count. At 24 dB it passes in both cases; lower it to test how the detector reacts to weaker shouts.' },
  { section: 'sim', key: 'hit_fx_variety', def: 1, min: 0, max: 1, step: 1, int: true, name: 'Varied punch effects', unit: '0 off / 1 on',
    help: 'Gives every punch its own named light-and-screen effect. Off = the original white flash for every heavy punch.',
    more: 'On: jabs get a small coloured spark on one column, hooks a mid-size sweep, ring or double pop, and heavies one of six big signatures (THUNDER, GOLD RUSH, SHATTER, STROBE, SPLIT, SUPERNOVA). The same one never comes twice in a row, and deep in a combo the biggest ones come up more often and leave their colour on the column tops. Off brings back the plain white flash on heavies. Sim proposal, not exported.' },
  { section: 'sim', key: 'hit_fx_intensity', def: 1, min: 0.3, max: 1.5, step: 0.05, name: 'Punch effect strength', unit: 'x',
    help: 'How bright the punch effects are on the LED columns and how strong the screen flashes and shake are.',
    more: 'Scales the punch signatures on the LEDs and the matching screen moments. The whole LED frame stays under the strip brightness cap (mean level 0.72) whatever this is set to, so above 1 the effects get bolder but never a white wall. Screen shake is off for anyone with "reduce motion" set in their system. Sim proposal, not exported.' },
  { section: 'sim', key: 'leds_per_column', def: 90, min: 10, max: 300, step: 5, int: true, name: 'LEDs per column', unit: 'LEDs',
    help: 'How many LEDs the simulator draws in each of the four columns.',
    more: 'Only changes the on-screen LED preview. When the bridge drives a real strip it resamples the picture to the strip length, so this does not change the hardware.' },
];

const LS_KEY = 'sw-sim-tunables-v1';

export function defaults() {
  const o = {};
  for (const t of TUNABLES) o[t.key] = t.def;
  return o;
}

// Defaults that changed: a saved value equal to its old default is dropped so the new default applies.
const OLD_DEFAULTS = { shadow_hp: [250] };
export function loadTunables() {
  const o = defaults();
  try {
    const saved = JSON.parse(localStorage.getItem(LS_KEY) || '{}');
    for (const [k, olds] of Object.entries(OLD_DEFAULTS)) if (olds.includes(saved[k])) delete saved[k];
    for (const t of TUNABLES) if (typeof saved[t.key] === 'number') o[t.key] = saved[t.key];
  } catch { /* ignore */ }
  return o;
}

export function saveTunables(cfg) {
  // Only values changed from the defaults, so later default changes still reach this browser.
  const d = defaults(), diff = {};
  for (const t of TUNABLES) if (cfg[t.key] !== d[t.key]) diff[t.key] = cfg[t.key];
  try { localStorage.setItem(LS_KEY, JSON.stringify(diff)); } catch { /* ignore */ }
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
