// UI glue: wires the ported brain-rs logic to the scene, LEDs, mic and panels.
import { TUNABLES, SECTIONS, loadTunables, saveTunables, exportYaml, formatValue, defaults } from './config.js';
import { EventBus } from './events.js';
import { StateMachine, ArenaState, canTransitionTo } from './state_machine.js';
import { ArenaManager } from './arena_manager.js';
import { PresenceService } from './presence.js';
import { ShoutDetector } from './shout_detector.js';
import { FeatureSource } from './feature_source.js';
import { FRAME_SECS } from './audio_frontend.js';
import { LedRig, COLUMNS } from './wled.js';
import { Scene } from './scene.js';
import { HitPicker, tierOf, VICTORY } from './hitfx.js';
import { PRESETS, PRESET_IDS, STAGES, STAGE_BY_ID, FX_PARAMS, hitTable } from './presets.js';
import { LiveAmbience } from './ambience_live.js';
import { IMPACT_CHOICES, OPEN_CHOICES, Sfx } from './sfx.js';
import { Playlist } from './music.js';
import { LiveLink, LIVE_REQUESTED, BRAIN_REQUESTED, unstripFrame } from './live.js';
import { BrainClient } from './brain.js';
import { initConnect } from './connect.js';

const $ = (id) => document.getElementById(id);
const S = ArenaState;
// Brain mode (bridge.py --brain): the real brain-rs runs the game; the JS engine stays idle.
const BRAIN = BRAIN_REQUESTED;
let brain = null;

// ------------------------------------------------------------------ model
const cfg = loadTunables();
const PREFS_KEY = 'sw-sim-prefs-v1';
const prefs = Object.assign({ fixInactivity: true, fixRearm: true, koEnds: false, musicPlayback: true, ledMode: 'proposal', overlays: 'minimal', sound: true, liveMirror: true, liveSerp: false },
  JSON.parse(localStorage.getItem(PREFS_KEY) || '{}'));
const savePrefs = () => localStorage.setItem(PREFS_KEY, JSON.stringify(prefs));

let simNow = 0;
const now = () => simNow;
const fixes = { resetInactivityOnFight: prefs.fixInactivity, rearmPresence: prefs.fixRearm };
const bus = new EventBus();
const sm = new StateMachine(bus, now);
const arena = new ArenaManager(cfg, sm, bus, now, fixes);
const presence = new PresenceService(cfg, bus, now);
const detector = new ShoutDetector();
const sfx = new Sfx(); sfx.enabled = prefs.sound;
const ambience = new LiveAmbience(sfx);
const playlist = new Playlist(sfx); sfx.playlist = playlist;
const nowPlaying = document.getElementById('nowPlaying');
playlist.onchange = () => {
  const n = playlist.tracks.length;
  if (!n) { nowPlaying.textContent = 'Fight music: synth loop'; nowPlaying.classList.remove('on'); return; }
  const t = playlist.current;
  nowPlaying.textContent = `${playlist.playing ? 'Now playing' : 'Next fight'}: ${t.title} (${playlist.idx + 1}/${n})`;
  nowPlaying.classList.toggle('on', playlist.playing);
};
// Brain mode: the brain plays the fight music itself.
(BRAIN ? Promise.resolve(false) : playlist.loadDefault().then((ok) => ok || playlist.loadSaved())).then((ok) => { if (ok && playlist.source === 'saved') log(`Restored ${playlist.tracks.length} saved music tracks`, 'state'); });
document.getElementById('musicFiles').addEventListener('change', (e) => {
  const n = playlist.loadFiles(e.target.files);
  if (n) log(`Loaded ${n} music tracks`, 'state');
  if (n && sm.currentState === ArenaState.Fight && sfx.enabled) { if (sfx.music) { clearInterval(sfx.music); sfx.music = null; } playlist.play(); }
});
document.getElementById('musicNext').addEventListener('click', () => playlist.skip());
const rig = new LedRig(cfg.leds_per_column); rig.mode = prefs.ledMode;
const scene = new Scene($('scene'), rig); scene.overlays = prefs.overlays === 'full' ? 'full' : 'minimal';
const hitPicker = new HitPicker();

const newRoundStats = () => ({ t0: simNow, punches: 0, maxCombo: 0, maxPower: 0 });
// round: 1.. (a KO plays the victory, then a tougher shadow rises); rs: this round's stats
const game = { hp: cfg.shadow_hp, maxHp: cfg.shadow_hp, kos: 0, combo: 0, lastHitAt: -9, respawnAt: null, summary: '', shouts: 0, lastShout: null, round: 1, rs: newRoundStats(), healing: false };
let lastFeat = null;
let sensorOn = false;
let errors = [];

if (!BRAIN) {
  arena.start();
  // main.rs: "Set initial state to IDLE" after the arena manager starts.
  sm.transition(S.Idle);
}

// ------------------------------------------------------------------ log
const logEl = $('log');
function log(text, cls = '') {
  const li = document.createElement('li');
  if (cls) li.className = cls;
  const tm = document.createElement('time'); tm.textContent = `${simNow.toFixed(1)}s`;
  li.append(tm, document.createTextNode(text));
  logEl.prepend(li);
  while (logEl.children.length > 200) logEl.lastChild.remove();
}
$('clearLog').onclick = () => { logEl.textContent = ''; };

// ------------------------------------------------------------------ events
bus.subscribe((e) => {
  switch (e.type) {
    case 'StateChanged':
      log(`${e.from} -> ${e.to}`, 'state');
      onStateEnter(e.to, e.from);
      break;
    case 'PresenceDetected': log(`PresenceDetected { detected: ${e.detected} }`, 'presence'); onPresence(e.detected); break;
    case 'ShoutDetected': {
      game.shouts++; game.lastShout = e;
      const st = sm.currentState;
      log(`ShoutDetected intensity=${e.intensity.toFixed(2)} dur=${e.duration_sec.toFixed(2)}s level=${e.level_db.toFixed(1)}dB${st === S.Warming ? ` (+${(arena.energyBar?.lastAdded ?? e.intensity * cfg.shout_energy_multiplier).toFixed(1)} energy)` : st === S.Fight ? ' (FIGHT, visual only)' : ` (ignored: ${st})`}`, st === S.Warming ? 'shout' : 'ignored');
      rig.shout(e.intensity, st); sfx.shout(e.intensity);
      if (st === S.Fight) scene.shoutWave(e.intensity);
      const fl = $('shoutFlash'); fl.classList.add('on'); setTimeout(() => fl.classList.remove('on'), 120);
      if (st === S.Warming) scene.floater(`+${(e.intensity * cfg.shout_energy_multiplier).toFixed(0)}`, scene.W / 2 + (Math.random() < 0.5 ? -1 : 1) * (40 + Math.random() * 50), scene.bagGeom().top + 30, '#d6a8ff');
      break;
    }
    case 'EnergyFull': log('EnergyFull', 'state'); break;
    case 'FightEnded': log(`FightEnded { reason: ${e.reason} }`, 'state'); break;
    case 'FightStarted':
      if (prefs.musicPlayback) { feats.setPlaybackActive(true); log('MusicStarted -> playback_active = true', 'ignored'); }
      sfx.startMusic();
      break;
    case 'SessionEnded':
      if (feats.playbackActive) { feats.setPlaybackActive(false); log('MusicStopped -> playback_active = false', 'ignored'); }
      sfx.stopMusic();
      break;
    default: break;
  }
});

// Someone arrives (presence detected outside a fight): the shadow greets them once per arrival.
// Line picked by Yarden 20.11 (sound lab zero3); swap for a friend's recording when it comes in.
const GREETING = 'sound-lab/voice/hebrew/zero3.mp3', GREET_GAP_MS = 30000;
let lastGreet = -Infinity, wasPresent = false;
function onPresence(detected) {
  const arrived = detected && !wasPresent; wasPresent = detected;
  const st = BRAIN ? brain.info().currentState : sm.currentState;
  if (!arrived || st === S.Fight || st === S.Cooldown || performance.now() - lastGreet < GREET_GAP_MS) return;
  lastGreet = performance.now(); sfx.voice(GREETING); log('Greeting: "סליחה... אַתְּ, אַתָּה, אַתֶּם... אפס."', 'presence');
}

function onStateEnter(to) {
  if (to === S.Fight) {
    game.punches = 0; game.maxPower = 0; game.endReason = null;
    game.maxHp = cfg.shadow_hp; game.hp = game.maxHp; game.kos = 0; game.combo = 0; game.respawnAt = null;
    game.round = 1; game.rs = newRoundStats(); scene.vic = null; scene.rise = null;
    scene.resetDamage();
    scene.showBanner('FIGHT!', 'punch the shadow');
    // Full bar -> FIGHT: one arena-opening moment (LED burst, screen ring, sound), synced on the impact.
    rig.fightStart(); scene.fightOpen(); sfx.arenaOpen();
  } else if (to === S.Cooldown) {
    const endReason = BRAIN ? game.endReason : arena.lastEndReason;
    const punches = BRAIN ? game.punches : arena.currentSessionPunches, maxPower = BRAIN ? game.maxPower : arena.currentSessionMaxPower;
    const reason = { TimeElapsed: 'time up', InactivityTimeout: 'no punches', KnockOut: 'knock-out' }[endReason] || '';
    game.summary = `${punches} punches, max ${maxPower.toFixed(0)}, ${game.kos} KO${game.kos === 1 ? '' : 's'}${reason ? `, ended: ${reason}` : ''}`;
    const title = { KnockOut: 'VICTORY', TimeElapsed: 'TIME UP', InactivityTimeout: 'TOO QUIET' }[endReason] || 'FIGHT OVER';
    scene.showBanner(title, game.summary, 3.5);
  } else if (to === S.Warming) {
    scene.showBanner('WAKE IT UP', 'shout!', 1.6);
  }
  if (to !== S.Fight) { game.respawnAt = null; if (to !== S.Cooldown) scene.vic = null; }
  if (to !== S.Fight && to !== S.Cooldown) sfx.stopMusic();
}

// ------------------------------------------------------------------ features / shouts
const feats = new FeatureSource((f) => {
  lastFeat = f;
  const d = detector.push(f, cfg);
  presence.onFrame(f);
  if (d) bus.publish({ type: 'ShoutDetected', ...d });
});
feats.synthDb = cfg.synth_shout_db;

$('micBtn').onclick = async () => {
  const btn = $('micBtn');
  if (BRAIN) { log('Brain mode: the brain listens with its own microphone; SHOUT sends a shout to it', 'ignored'); return; }
  if (feats.micActive) { feats.stopMic(); btn.textContent = 'Enable mic'; $('micStatus').textContent = 'synthetic frames'; log('Mic stopped', 'ignored'); return; }
  try {
    btn.textContent = '...';
    await feats.startMic(sfx.ensure());
    btn.textContent = 'Mic on';
    $('micStatus').textContent = feats.status;
    log(`Mic started (${feats.status}); frames now come from the ported front-end`, 'presence');
  } catch (err) {
    btn.textContent = 'Enable mic';
    $('micStatus').textContent = 'mic unavailable';
    log(`Mic error: ${err.message || err}`, 'ignored');
  }
};
$('resetFloor').onclick = () => { feats.resetFloor(); log('Noise floor reset', 'ignored'); };

// Shout button: hold = shout; quick tap = 450 ms shout.
const shoutBtn = $('shoutBtn');
let shoutDownAt = 0;
let brainShoutTimer = null;
function shoutDown() {
  sfx.ensure(); shoutDownAt = performance.now(); shoutBtn.classList.add('down');
  if (BRAIN) {
    // POST /api/arena/shout (intensity 1) now and every 350 ms while held.
    if (brainShoutTimer) return;
    brain.shout(1); brainShoutTimer = setInterval(() => brain.shout(1), 350);
    return;
  }
  feats.setHeld(true);
}
function shoutUp() {
  if (BRAIN) { clearInterval(brainShoutTimer); brainShoutTimer = null; shoutBtn.classList.remove('down'); return; }
  if (!feats.synthHeld) return;
  feats.setHeld(false); shoutBtn.classList.remove('down');
  if (performance.now() - shoutDownAt < 300) feats.pulse(450 - (performance.now() - shoutDownAt));
}
shoutBtn.addEventListener('pointerdown', (e) => { e.preventDefault(); shoutBtn.setPointerCapture?.(e.pointerId); shoutDown(); });
shoutBtn.addEventListener('pointerup', shoutUp);
shoutBtn.addEventListener('pointercancel', shoutUp);
shoutBtn.addEventListener('lostpointercapture', shoutUp);

// ------------------------------------------------------------------ presence
const presenceToggle = $('presenceToggle');
let brainPresenceTimer = null;
function setSensor(on) {
  sensorOn = on; presenceToggle.checked = on;
  if (BRAIN) {
    // The toggle forces the brain's presence fusion; re-asserted every 20 s so it outlasts leave_timeout_sec.
    clearInterval(brainPresenceTimer); brainPresenceTimer = null;
    brain.presence(on);
    if (on) brainPresenceTimer = setInterval(() => brain.presence(true), 20000);
    log(`Presence ${on ? 'ON' : 'OFF'} -> brain POST /api/arena/presence`, 'presence');
    return;
  }
  log(`Sim presence sensor ${on ? 'ON' : 'OFF'}${on ? '' : ` (fusion leaves after ${cfg.leave_timeout_sec}s without evidence)`}`, 'presence');
}
presenceToggle.onchange = () => { sfx.ensure(); setSensor(presenceToggle.checked); };
$('forceAbsent').onclick = () => { if (sensorOn) setSensor(false); if (BRAIN) brain.presence(false); else presence.forcePresent(false); log('Manual override: force absent', 'presence'); };
$('forcePresent').onclick = () => { if (BRAIN) brain.presence(true); else presence.forcePresent(true); log('Manual override: force present', 'presence'); };
document.querySelectorAll('#brainStates button').forEach((b) => { b.onclick = () => { log(`Set state ${b.dataset.s} -> brain POST /api/arena/state`, 'state'); brain?.setState(b.dataset.s); }; });

// ------------------------------------------------------------------ punches
let pendingHit = null;
function punch(power, info) {
  sfx.ensure();
  power = Math.max(1, Math.min(100, power));
  if (BRAIN) {
    // The brain counts it; its PunchDetected event (SSE) animates the hit, like a bag punch.
    pendingHit = info || null;
    brain.punch(Math.round(power));
    return;
  }
  bus.publish({ type: 'PunchDetected', power });
  landPunch(power, info, sm.currentState);
}

/** Show a punch on the arena (and, in FIGHT, damage the shadow). */
function landPunch(power, info, state) {
  info = info || { dir: Math.random() < 0.5 ? 1 : -1, hx: scene.cx + (Math.random() - 0.5) * 20, hy: scene.H * (0.42 + Math.random() * 0.15) };
  const counted = state === S.Fight;
  const t = performance.now() / 1000;
  const combo = counted && t - game.lastHitAt < 1.0 ? game.combo + 1 : 1;
  // One signature per punch (hitfx.js), shared by the LEDs, the arena and the sound.
  // The kinds come from the preset drawing this tier (per-stage mixing); its Colour shift moves the hue.
  const tier = tierOf(power);
  const sig = hitPicker.pick(power, { combo, rage: rig.rage, dir: info.dir, variety: cfg.hit_fx_variety >= 1, table: hitTable(rig.presetFor(tier)), hueShift: rig.P(tier).hue });
  scene.hit(power, info, sig); rig.punch(power, sig); sfx.punch(power, sig);
  if (!counted) { log(`PunchDetected power=${power.toFixed(0)} (not counted: ${state})`, 'ignored'); return; }
  game.punches = (game.punches || 0) + 1; game.maxPower = Math.max(game.maxPower || 0, power);
  game.combo = combo;
  game.lastHitAt = t;
  const rs = game.rs; rs.punches++; rs.maxCombo = Math.max(rs.maxCombo, combo); rs.maxPower = Math.max(rs.maxPower, power);
  const mult = Math.min(1 + (game.combo - 1) * 0.1, cfg.combo_max ?? 2);
  let dmg = 0;
  if (game.respawnAt == null) {
    dmg = power * cfg.damage_per_power * mult;
    game.hp = Math.max(0, game.hp - dmg);
  }
  const col = power >= 75 ? '#ffdf6a' : power >= 45 ? '#ffae6a' : '#ffffff';
  scene.floater(dmg ? `-${dmg.toFixed(0)}` : `${power.toFixed(0)}`, info.hx + info.dir * -30, info.hy - 20, col, power >= 75);
  log(`PunchDetected power=${power.toFixed(0)} dmg=${dmg.toFixed(1)}${game.combo > 1 ? ` combo ${game.combo}` : ''}${sig.kind !== 'classic' ? ` [${sig.tier} ${sig.kind}]` : ''}`, 'punch');
  if (game.hp <= 0 && game.respawnAt == null) {
    game.kos++;
    const st = { round: game.round, time: simNow - rs.t0, punches: rs.punches, maxCombo: rs.maxCombo, hardest: rs.maxPower };
    game.lastVictory = st;
    // Victory: LED finale, arena finale and fanfare on one clock (VICTORY in hitfx.js); the fight clock pauses.
    rig.victory(); scene.victory(st); sfx.victory();
    log(`Shadow defeated, round ${st.round}: ${st.time.toFixed(1)} s, ${st.punches} punches, max combo ${st.maxCombo}, hardest hit ${st.hardest.toFixed(0)}`, 'ko');
    game.respawnAt = t + VICTORY.dur;
  }
}

/** After the victory finale: a new shadow with more health (round_hp_growth) and healing (round_regen_per_sec). */
function nextRound() {
  game.round++; game.maxHp = Math.round(game.maxHp * (1 + cfg.round_hp_growth)); game.hp = game.maxHp;
  game.rs = newRoundStats(); game.combo = 0; scene.resetDamage();
  const regen = cfg.round_regen_per_sec * (game.round - 1);
  scene.newRound(game.round, `${game.maxHp} HP${regen > 0 ? `, heals ${regen} HP/s when you pause` : ''}`);
  log(`Round ${game.round}: the shadow rises with ${game.maxHp} HP${regen > 0 ? `, heals ${regen} HP/s after 1 s without punches` : ''}`, 'ko');
}

const powerFromHold = (ms) => 30 + 70 * Math.min(1, ms / 600);
const sceneEl = $('scene');
let pdown = null;
sceneEl.addEventListener('pointerdown', (e) => {
  e.preventDefault(); sceneEl.setPointerCapture?.(e.pointerId);
  const r = sceneEl.getBoundingClientRect();
  pdown = { t: performance.now(), x: e.clientX - r.left, y: e.clientY - r.top };
});
sceneEl.addEventListener('pointerup', (e) => {
  if (!pdown) return;
  const held = performance.now() - pdown.t;
  punch(powerFromHold(held) + (Math.random() - 0.5) * 6, scene.hitInfo(pdown.x, pdown.y));
  pdown = null; scene.charge = null;
});
sceneEl.addEventListener('pointercancel', () => { pdown = null; scene.charge = null; });

document.querySelectorAll('.btn.punch').forEach((b) => {
  b.addEventListener('pointerdown', (e) => { e.preventDefault(); punch(Number(b.dataset.power) + (Math.random() - 0.5) * 14); });
});

let spaceDown = null;
const isTyping = (el) => !!el?.closest?.('textarea, select, input[type=number], input[type=range], input[type=text]');
const keyPower = { j: 25, k: 55, l: 90 };
window.addEventListener('keydown', (e) => {
  if (isTyping(e.target)) return;
  const k = e.key.toLowerCase();
  if (keyPower[k] && !e.repeat) { punch(keyPower[k] + (Math.random() - 0.5) * 14); e.preventDefault(); }
  else if (k === ' ') { if (!spaceDown) spaceDown = performance.now(); e.preventDefault(); }
  else if (k === 's' && !e.repeat) shoutDown();
  else if (k === 'p' && !e.repeat) setSensor(!sensorOn);
});
window.addEventListener('keyup', (e) => {
  const k = e.key.toLowerCase();
  if (k === ' ' && spaceDown) { punch(powerFromHold(performance.now() - spaceDown)); spaceDown = null; scene.charge = null; }
  else if (k === 's') shoutUp();
});

// ------------------------------------------------------------------ sound / lights
const impactSel = $('impactSel');
for (const o of IMPACT_CHOICES) { const op = document.createElement('option'); op.value = o.id; op.textContent = o.name; impactSel.append(op); }
if (IMPACT_CHOICES.some((o) => o.id === prefs.impact)) sfx.impactId = prefs.impact;
impactSel.value = sfx.impactId;
impactSel.onchange = () => { sfx.impactId = impactSel.value; prefs.impact = sfx.impactId; savePrefs(); sfx.ensure(); sfx.punch(60, null); };
const openSel = $('openSel');
for (const o of OPEN_CHOICES) { const op = document.createElement('option'); op.value = o.id; op.textContent = o.name; openSel.append(op); }
if (OPEN_CHOICES.some((o) => o.id === prefs.fightOpen)) sfx.openId = prefs.fightOpen;
openSel.value = sfx.openId;
openSel.onchange = () => { sfx.openId = openSel.value; prefs.fightOpen = sfx.openId; savePrefs(); sfx.ensure(); sfx.lastOpen = 0; sfx.arenaOpen(); };
const soundBtn = $('soundBtn');
const renderSound = () => { soundBtn.textContent = sfx.enabled ? 'Sound on' : 'Sound off'; };
soundBtn.onclick = () => { sfx.enabled = !sfx.enabled; prefs.sound = sfx.enabled; savePrefs(); renderSound(); if (!sfx.enabled) sfx.stopMusic(); else { sfx.ensure(); if (sm.currentState === S.Fight) sfx.startMusic(); } };
renderSound();
const ledMode = $('ledMode');
for (const id of PRESET_IDS) if (!ledMode.querySelector(`option[value="${id}"]`)) ledMode.add(new Option(PRESETS[id].name, id));
ledMode.value = rig.mode;
ledMode.onchange = () => { rig.mode = ledMode.value; prefs.ledMode = rig.mode; savePrefs(); log(`LED effects: ${PRESETS[rig.mode].name}`, 'ignored'); buildFxStages(); };

// ------------------------------------------------------------------ effect presets: mixing, preview, fine-tuning
// Saved like the tunables: only per-stage overrides and parameters changed from the defaults.
const FX_KEY = 'sw-sim-effects-v1';
const clampP = (P, v) => Math.min(P.max, Math.max(P.min, v));
try {
  const o = JSON.parse(localStorage.getItem(FX_KEY) || '{}');
  for (const [st, id] of Object.entries(o.overrides || {})) if (STAGE_BY_ID[st] && PRESETS[id]) rig.overrides[st] = id;
  for (const [st, ps] of Object.entries(o.params || {})) {
    if (!STAGE_BY_ID[st]) continue;
    const d = {}; for (const P of FX_PARAMS) if (typeof ps?.[P.key] === 'number') d[P.key] = clampP(P, ps[P.key]);
    rig.params[st] = d;
  }
} catch { /* ignore */ }
function saveFx() {
  const params = {};
  for (const [st, ps] of Object.entries(rig.params)) {
    const d = {}; for (const P of FX_PARAMS) if (ps[P.key] !== undefined && ps[P.key] !== P.def) d[P.key] = ps[P.key];
    if (Object.keys(d).length) params[st] = d;
  }
  try { localStorage.setItem(FX_KEY, JSON.stringify({ overrides: rig.overrides, params })); } catch { /* ignore */ }
}
function setOverride(stage, id) {
  if (id && PRESETS[id]) rig.overrides[stage] = id; else delete rig.overrides[stage];
  saveFx(); buildFxStages();
  log(`Effects: ${STAGE_BY_ID[stage].name} from ${PRESETS[rig.presetFor(stage)].name}${id ? '' : ' (base preset)'}`, 'ignored');
}
function setFxParam(stage, key, v) {
  const P = FX_PARAMS.find((x) => x.key === key);
  (rig.params[stage] ||= {})[key] = clampP(P, v); saveFx();
}

const PREVIEW_ENERGY = [0.12, 0.37, 0.62, 0.9];
const PREVIEW_POWER = { jab: 25, hook: 55, heavy: 90 };
const PREVIEW_G = { fightRemaining: 99 };
/** Fire a stage's effect now, whatever the arena is doing (the LEDs show that stage for a moment). */
function previewStage(stage, arg) {
  sfx.ensure();
  const t = performance.now() / 1000;
  switch (stage) {
    case 'idle': rig.previewFor(4, S.Idle); break;
    case 'warming': rig.wStage = arg - 1; rig.previewFor(3.5, S.Warming, { energyPct: PREVIEW_ENERGY[arg] }); rig.shoutWaves.push({ t0: t, k: 1 }); break;
    case 'open': rig.previewFor(2.4, S.Fight, PREVIEW_G); rig.fightStart(); scene.fightOpen(); sfx.arenaOpen(); break;
    case 'fight': rig.previewFor(4, S.Fight, PREVIEW_G); rig.rage = Math.max(rig.rage, 0.8); break;
    case 'shout': rig.previewFor(1.8, S.Fight, PREVIEW_G); rig.shout(1, S.Fight); scene.shoutWave(1); sfx.shout(1); break;
    case 'victory':
      rig.previewFor(VICTORY.dur + 0.3, S.Fight, PREVIEW_G); rig.victory(); sfx.victory();
      scene.victory({ round: game.round || 1, time: 42, punches: 57, maxCombo: 9, hardest: 98 });
      break;
    default: { // jab / hook / heavy: one signature of the chosen kind
      const sig = hitPicker.make(stage, arg, { table: hitTable(rig.presetFor(stage)), hueShift: rig.P(stage).hue, dir: Math.random() < 0.5 ? 1 : -1 });
      const power = PREVIEW_POWER[stage], g = scene.bagGeom(), info = { dir: sig.dir, hx: scene.cx, hy: (g.top + g.bot) / 2 };
      rig.previewFor(1.9, S.Fight, PREVIEW_G);
      const v = rig.variety; rig.variety = true; rig.punch(power, sig); rig.variety = v;
      scene.hit(power, info, sig); sfx.punch(power, sig);
    }
  }
  log(`Preview: ${STAGE_BY_ID[stage].name}${arg != null ? ` ${arg}` : ''} (${PRESETS[rig.presetFor(stage)].name})`, 'ignored');
}

const fxOpen = new Set(); // fine-tune sections kept open across rebuilds
function buildFxStages() {
  const root = $('fxStages'); root.textContent = '';
  for (const st of STAGES) {
    const pid = rig.presetFor(st.id), def = PRESETS[pid][st.id];
    const box = document.createElement('div'); box.className = 'fxStage'; box.dataset.stage = st.id;
    const row = document.createElement('div'); row.className = 'fxRow';
    const nm = document.createElement('label'); nm.className = 'fxName'; nm.textContent = st.name; nm.htmlFor = `fxsel_${st.id}`;
    const sel = document.createElement('select'); sel.id = `fxsel_${st.id}`; sel.setAttribute('aria-label', `${st.name}: preset`);
    sel.add(new Option(`Base (${PRESETS[rig.preset].name})`, ''));
    for (const id of PRESET_IDS) sel.add(new Option(PRESETS[id].name, id));
    sel.value = rig.overrides[st.id] || '';
    sel.onchange = () => setOverride(st.id, sel.value);
    row.append(nm, sel);
    const eff = document.createElement('div'); eff.className = 'fxEff';
    eff.textContent = def.kinds ? Object.values(def.kinds).map((k) => k.name).join(' / ') : def.name;
    box.append(row, eff);
    // preview buttons
    const prev = document.createElement('div'); prev.className = 'fxPrev';
    const btn = (label, arg, title) => { const b = document.createElement('button'); b.type = 'button'; b.className = 'btn small'; b.textContent = label; b.dataset.preview = `${st.id}${arg != null ? `:${arg}` : ''}`; b.title = title || `Preview ${st.name}`; b.onclick = () => previewStage(st.id, arg); prev.append(b); };
    if (def.none) { const n = document.createElement('span'); n.className = 'fxNone'; n.textContent = 'nothing to preview'; prev.append(n); }
    else if (st.id === 'warming') ['25%', '50%', '75%', '100%'].forEach((l, i) => btn(`Stage ${i + 1}`, i, `Preview the bar at stage ${i + 1} (${l} full)`));
    else if (def.kinds) for (const [k, kd] of Object.entries(def.kinds)) btn(kd.label || kd.name, k, `Preview ${st.name}: ${kd.name}`);
    else btn('Preview', null);
    box.append(prev);
    // fine-tuning
    const det = document.createElement('details'); det.className = 'fxTune'; det.open = fxOpen.has(st.id);
    det.ontoggle = () => { if (det.open) fxOpen.add(st.id); else fxOpen.delete(st.id); };
    const sum = document.createElement('summary'); sum.textContent = 'Fine-tune'; det.append(sum);
    const cur = rig.P(st.id);
    for (const P of FX_PARAMS) det.append(paramRow(st.id, P, cur[P.key]));
    if (Object.keys(rig.params[st.id] || {}).some((k) => rig.params[st.id][k] !== FX_PARAMS.find((x) => x.key === k).def)) sum.classList.add('changed');
    box.append(det);
    root.append(box);
  }
}
function paramRow(stage, P, value) {
  const id = `fx_${stage}_${P.key}`;
  const row = document.createElement('div'); row.className = 'tun';
  const lab = document.createElement('label'); lab.htmlFor = id; lab.className = 'tunName';
  const nm = document.createElement('span'); nm.className = 'nm'; nm.textContent = P.name;
  const kc = document.createElement('code'); kc.className = 'key'; kc.textContent = `${stage}.${P.key}`; lab.append(nm, kc);
  const num = document.createElement('input'); num.type = 'number'; num.step = P.step; num.min = P.min; num.max = P.max; num.setAttribute('aria-label', `${STAGE_BY_ID[stage].name}: ${P.name}`);
  const unit = document.createElement('span'); unit.className = 'unit'; unit.textContent = P.unit;
  const val = document.createElement('div'); val.className = 'val'; val.append(num, unit);
  const rng = document.createElement('input'); rng.type = 'range'; rng.id = id; rng.min = P.min; rng.max = P.max; rng.step = P.step;
  const help = document.createElement('div'); help.className = 'help'; help.textContent = P.help;
  const more = document.createElement('button'); more.type = 'button'; more.className = 'moreBtn'; more.textContent = 'More…';
  const moreBox = document.createElement('div'); moreBox.className = 'more'; moreBox.hidden = true; moreBox.id = `more_${id}`; moreBox.textContent = P.more;
  more.setAttribute('aria-expanded', 'false'); more.setAttribute('aria-controls', moreBox.id);
  more.onclick = () => { moreBox.hidden = !moreBox.hidden; more.setAttribute('aria-expanded', String(!moreBox.hidden)); more.textContent = moreBox.hidden ? 'More…' : 'Less'; };
  const set = (v, src) => {
    v = Number(v); if (!Number.isFinite(v)) return;
    setFxParam(stage, P.key, v); v = rig.P(stage)[P.key];
    if (src !== rng) rng.value = v;
    if (src !== num) num.value = String(v);
    lab.classList.toggle('changed', v !== P.def);
  };
  rng.oninput = () => set(rng.value, rng);
  num.onchange = () => set(num.value, num);
  rng.value = value; num.value = String(value); lab.classList.toggle('changed', value !== P.def);
  row.append(lab, val, rng, help, more, moreBox);
  return row;
}
buildFxStages();
const overlayMode = $('overlayMode');
overlayMode.value = scene.overlays;
overlayMode.onchange = () => { scene.overlays = prefs.overlays = overlayMode.value; savePrefs(); log(`Screen overlays: ${overlayMode.value}`, 'ignored'); };
$('fxReset').onclick = () => { rig.overrides = {}; rig.params = {}; saveFx(); buildFxStages(); log('Effects: per-stage choices and fine-tuning reset', 'ignored'); };

// Now playing: the ambient of the shown state and every live effect, plus the last 8 fired.
const npEl = $('fxNow'), histEl = $('fxHistory');
const STAGE_TAG = Object.fromEntries(STAGES.map((s) => [s.id, s.id.toUpperCase()]));
function npRow(tag, preset, effect, detail, extra, cls) {
  const r = document.createElement('div'); r.className = `npRow${cls ? ` ${cls}` : ''}`;
  const b = document.createElement('b'); b.textContent = tag;
  const e = document.createElement('span'); e.className = 'npEff'; e.textContent = `${PRESETS[preset]?.name ?? preset} · ${effect}`;
  r.append(b, e);
  if (detail) { const d = document.createElement('span'); d.className = 'npDet'; d.textContent = detail; r.append(d); }
  if (extra) { const x = document.createElement('em'); x.textContent = extra; r.append(x); }
  return r;
}
function updateNowPlaying(st) {
  const np = rig.nowPlaying(st), rows = [];
  rows.push(npRow(STAGE_TAG[np.ambient.stage] || np.ambient.stage.toUpperCase(), np.ambient.preset, np.ambient.effect, np.ambient.detail, np.preview ? 'preview' : '', 'amb'));
  for (const f of np.fx) rows.push(npRow(STAGE_TAG[f.stage], f.preset, f.effect, f.detail, `${f.left.toFixed(1)} s left`, 'fx'));
  const lv = document.createElement('div'); lv.className = 'npLevel'; lv.textContent = `frame level ${np.level.toFixed(2)} (cap 0.72)`;
  rows.push(lv);
  npEl.replaceChildren(...rows);
  const now = performance.now() / 1000;
  histEl.replaceChildren(...rig.history.map((h) => {
    const li = document.createElement('li');
    const tm = document.createElement('time'); tm.textContent = `${(now - h.at).toFixed(1)} s`;
    li.append(tm, document.createTextNode(` ${STAGE_TAG[h.stage] || h.stage} · ${PRESETS[h.preset]?.name ?? h.preset} · ${h.effect}${h.detail ? ` (${h.detail})` : ''}`));
    return li;
  }));
}

// ------------------------------------------------------------------ live hardware
// Served by simulator/live/bridge.py with ?live=1: frames go to the real WLED strip and
// real bag punches enter through punch(), the same path as a click on the arena.
let live = null;
// Brain mode: the newest frame from the bridge (brain UDP realtime or WLED's live view).
const brainFrame = { seq: 0, source: null, at: -1e9, count: 0 };
if (LIVE_REQUESTED) {
  $('livePanel').hidden = false; $('liveTag').hidden = false;
  const dot = (id, cls) => { $(id).className = `dot ${cls}`; };
  const renderBrainLive = (l) => {
    const st = l.status, w = st?.wled, br = st?.brain, hw = brain?.state?.hardware;
    $('liveTag').classList.toggle('on', l.connected && brain?.ok === true);
    if (!l.connected) { dot('wledDot', 'bad'); $('wledStatus').textContent = 'Strip: bridge not connected (retrying)'; return; }
    const fresh = performance.now() - brainFrame.at < 1500;
    const strip = w?.host ? `WLED ${w.host}` : 'no WLED (--wled none)';
    if (br?.realtime) { dot('wledDot', 'ok'); $('wledStatus').textContent = `Strip: brain realtime ${br.udp_fps} fps from ${br.udp_from}, forwarded to ${strip}`; }
    else if (fresh && brainFrame.source === 'wled') { dot('wledDot', 'ok'); $('wledStatus').textContent = `Strip: WLED effect${br?.effect ? ` "${br.effect}"` : ''}, live view from ${strip}`; }
    else { dot('wledDot', 'warn'); $('wledStatus').textContent = `Strip: ${br?.effect ? `WLED effect "${br.effect}", ` : ''}${w?.host ? `no live view (${br?.live_view})` : 'no strip to show'}; brain ${br?.udp_packets ? 'not streaming' : 'has not sent realtime yet'}`; }
    const ble = hw?.ble;
    if (brain?.ok === false) { dot('bagDot', 'bad'); $('bagStatus').textContent = `Brain: unreachable at ${br?.url ?? '?'} (${brain.lastError})`; }
    else if (ble) { dot('bagDot', ble.connected ? 'ok' : 'warn'); $('bagStatus').textContent = `Brain ${br?.url ?? ''}: bag ${ble.connected ? `connected${ble.device_name ? ` (${ble.device_name})` : ''}` : `not connected${ble.last_error ? ` (${ble.last_error})` : ''}`}`; }
    else { dot('bagDot', 'warn'); $('bagStatus').textContent = 'Brain: connecting...'; }
  };
  const renderLive = (l) => {
    if (BRAIN) { renderBrainLive(l); return; }
    const st = l.status, w = st?.wled, b = st?.bag;
    $('liveTag').classList.toggle('on', l.connected && !!w?.streaming);
    if (!l.connected) { dot('wledDot', 'bad'); $('wledStatus').textContent = 'Strip: bridge not connected (retrying)'; dot('bagDot', 'bad'); $('bagStatus').textContent = 'Bag: -'; return; }
    $('liveTake').hidden = st?.owner !== false;
    if (st?.owner === false) { dot('wledDot', 'warn'); $('wledStatus').textContent = 'Strip: another tab is driving it'; }
    else if (!l.enabled) { dot('wledDot', 'warn'); $('wledStatus').textContent = `Strip: mirroring off (WLED ${w?.host ?? ''} back on its own effect)`; }
    else if (w?.streaming) { dot('wledDot', 'ok'); $('wledStatus').textContent = `Strip: live, ${w.fps} fps to WLED ${w.host} (${w.leds} LEDs, ${l.count / 4 | 0} per column)`; }
    else { dot('wledDot', 'warn'); $('wledStatus').textContent = `Strip: waiting for frames (WLED ${w?.host ?? '?'})`; }
    if (!b) return;
    const ok = b.state === 'connected';
    dot('bagDot', ok ? 'ok' : (b.state === 'disabled' || b.state === 'unavailable' || b.state === 'error') ? 'bad' : 'warn');
    $('bagStatus').textContent = ok ? `Bag: connected, fight mode ${b.fight ? 'on' : 'off'}, ${b.punches} punches` : `Bag: ${b.state}${b.detail ? ` (${b.detail})` : ''}`;
  };
  live = new LiveLink({
    log,
    onStatus: renderLive,
    onFrame: (f) => {
      if (!BRAIN) return;
      unstripFrame(f.rgb, f.rgb.length / 3, live.serpentine, rig);
      Object.assign(brainFrame, { seq: f.seq, source: f.source, at: performance.now(), count: f.rgb.length / 3 });
    },
    onPunch: (m) => { if (live.status?.owner === false) return; log(`Bag punch: ${m.accel} m/s^2 -> power ${m.power.toFixed(0)}`, 'punch'); punch(m.power); },
  });
  const mirror = $('liveMirror'), serp = $('liveSerp');
  mirror.checked = live.enabled = prefs.liveMirror; serp.checked = live.serpentine = prefs.liveSerp;
  mirror.onchange = () => { live.enabled = prefs.liveMirror = mirror.checked; savePrefs(); renderLive(live); };
  $('liveTake').onclick = () => live.take();
  serp.onchange = () => { live.serpentine = prefs.liveSerp = serp.checked; savePrefs(); };
}

if (BRAIN) {
  // The brain owns the game, the mic, the music and the tunables: hide the JS engine's panels.
  document.body.classList.add('brainMode');
  $('liveTag').textContent = 'BRAIN'; $('liveTag').title = 'Real brain-rs via the bridge';
  for (const id of ['detectorPanel', 'tunablesPanel', 'musicBar', 'ledMode', 'mirrorToggle', 'fxPanel', 'fxNowWrap']) $(id).hidden = true;
  $('brainStates').hidden = false; $('presRow').hidden = true;
  $('micBtn').disabled = true; $('micBtn').title = 'Brain mode: the brain uses its own microphone';
  document.querySelector('.brand small').textContent = 'live brain';
  brain = new BrainClient({
    log,
    onEvent: (type, d) => onBrainEvent(type, d),
  });
  brain.start();
}

function onBrainEvent(type, d) {
  switch (type) {
    case 'StateChanged': log(`${d.from} -> ${d.to}`, 'state'); onStateEnter(d.to, d.from); break;
    case 'PresenceDetected': log(`PresenceDetected { detected: ${d.detected} }`, 'presence'); onPresence(d.detected); break;
    case 'ShoutDetected': {
      const st = brain.info().currentState, mult = brain.tunables?.shout_energy_multiplier ?? 10;
      log(`ShoutDetected intensity=${(d.intensity ?? 0).toFixed(2)}${st === S.Warming ? ` (+${(d.intensity * mult).toFixed(1)} energy)` : st === S.Fight ? ' (FIGHT, visual only)' : ` (ignored: ${st})`}`, st === S.Warming ? 'shout' : 'ignored');
      sfx.shout(d.intensity ?? 1); rig.shout(d.intensity ?? 1, st);
      if (st === S.Fight) scene.shoutWave(d.intensity ?? 1);
      const fl = $('shoutFlash'); fl.classList.add('on'); setTimeout(() => fl.classList.remove('on'), 120);
      if (st === S.Warming) scene.floater(`+${(d.intensity * mult).toFixed(0)}`, scene.W / 2 + (Math.random() < 0.5 ? -1 : 1) * (40 + Math.random() * 50), scene.bagGeom().top + 30, '#d6a8ff');
      break;
    }
    case 'PunchDetected': {
      const info = pendingHit; pendingHit = null;
      landPunch(d.power ?? 0, info, brain.info().currentState);
      break;
    }
    case 'FightEnded': game.endReason = d.reason; log(`FightEnded { reason: ${d.reason} }`, 'state'); break;
    case 'EnergyFull': case 'FightStarted': case 'SessionEnded': case 'MusicStarted': case 'MusicStopped': log(type, type === 'EnergyFull' ? 'state' : 'ignored'); break;
    default: log(`${type} ${JSON.stringify(d)}`, 'ignored');
  }
}

// ------------------------------------------------------------------ fidelity toggles
function bindPref(id, key, onChange) {
  const el = $(id); el.checked = !!prefs[key];
  el.onchange = () => { prefs[key] = el.checked; savePrefs(); onChange?.(el.checked); };
}
bindPref('fixInactivity', 'fixInactivity', (v) => { fixes.resetInactivityOnFight = v; });
bindPref('fixRearm', 'fixRearm', (v) => { fixes.rearmPresence = v; });
bindPref('koEnds', 'koEnds');
bindPref('musicPlayback', 'musicPlayback', (v) => { if (!v) feats.setPlaybackActive(false); else if (sm.currentState === S.Fight) feats.setPlaybackActive(true); });

// ------------------------------------------------------------------ tunables UI
const tunEls = {};
function buildTunables() {
  const root = $('tunables'); root.textContent = '';
  for (const [sec, title] of Object.entries(SECTIONS)) {
    const det = document.createElement('details'); det.className = 'group'; det.open = sec === 'arena' || sec === 'sim';
    const sum = document.createElement('summary'); sum.textContent = title; det.append(sum);
    for (const t of TUNABLES.filter((x) => x.section === sec)) {
      const row = document.createElement('div'); row.className = 'tun';
      const lab = document.createElement('label'); lab.htmlFor = `t_${t.key}`; lab.className = 'tunName';
      const nm = document.createElement('span'); nm.className = 'nm'; nm.textContent = t.name;
      const kc = document.createElement('code'); kc.className = 'key'; kc.textContent = t.key; lab.append(nm, kc);
      const num = document.createElement('input'); num.type = 'number'; num.step = t.step; num.min = t.min; num.max = t.max; num.setAttribute('aria-label', t.name);
      const unit = document.createElement('span'); unit.className = 'unit'; unit.textContent = t.unit || '';
      const val = document.createElement('div'); val.className = 'val'; val.append(num, unit);
      const rng = document.createElement('input'); rng.type = 'range'; rng.id = `t_${t.key}`; rng.min = t.min; rng.max = t.max; rng.step = t.step;
      const help = document.createElement('div'); help.className = 'help'; help.textContent = t.help;
      const more = document.createElement('button'); more.type = 'button'; more.className = 'moreBtn'; more.textContent = 'More…';
      const moreBox = document.createElement('div'); moreBox.className = 'more'; moreBox.hidden = true; moreBox.id = `more_${t.key}`; moreBox.textContent = t.more;
      more.setAttribute('aria-expanded', 'false'); more.setAttribute('aria-controls', moreBox.id);
      more.onclick = () => { moreBox.hidden = !moreBox.hidden; more.setAttribute('aria-expanded', String(!moreBox.hidden)); more.textContent = moreBox.hidden ? 'More…' : 'Less'; };
      const set = (v, src) => {
        v = Number(v); if (!Number.isFinite(v)) return;
        if (t.int) v = Math.round(v);
        cfg[t.key] = v;
        if (src !== rng) rng.value = v;
        if (src !== num) num.value = formatValue(t, v);
        lab.classList.toggle('changed', formatValue(t, v) !== formatValue(t, t.def));
        onTunable(t.key);
        saveTunables(cfg);
      };
      rng.oninput = () => set(rng.value, rng);
      num.onchange = () => set(num.value, num);
      tunEls[t.key] = { set };
      row.append(lab, val, rng, help, more, moreBox); det.append(row);
      set(cfg[t.key]);
    }
    root.append(det);
  }
}
function onTunable(key) {
  if (['warming_energy_threshold', 'shout_energy_multiplier', 'energy_decay_rate'].includes(key)) arena.onTunablesChanged();
  if (key === 'leds_per_column' && rig.n !== cfg.leds_per_column) rig.resize(cfg.leds_per_column);
  if (key === 'synth_shout_db') feats.synthDb = cfg.synth_shout_db;
  if (key === 'hit_fx_variety') rig.variety = cfg.hit_fx_variety >= 1;
  if (key === 'hit_fx_intensity') { rig.intensity = cfg.hit_fx_intensity; scene.intensity = cfg.hit_fx_intensity; }
}
buildTunables();
$('exportBtn').onclick = () => { const box = $('yamlBox'); box.hidden = false; $('yamlOut').value = exportYaml(cfg); };
$('copyYaml').onclick = async () => { try { await navigator.clipboard.writeText($('yamlOut').value); $('copyYaml').textContent = 'Copied'; } catch { $('yamlOut').select(); } setTimeout(() => { $('copyYaml').textContent = 'Copy'; }, 1200); };
$('resetBtn').onclick = () => { const d = defaults(); for (const t of TUNABLES) tunEls[t.key].set(d[t.key]); log('Tunables reset to config.yaml defaults', 'ignored'); };

// ------------------------------------------------------------------ strips canvas
const strips = $('strips'); const sctx = strips.getContext('2d');
function drawStrips() {
  const r = strips.getBoundingClientRect(); const dpr = Math.min(window.devicePixelRatio || 1, 2);
  if (strips.width !== Math.round(r.width * dpr)) { strips.width = Math.round(r.width * dpr); strips.height = Math.round(r.height * dpr); }
  sctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  const W = r.width, H = r.height; sctx.clearRect(0, 0, W, H);
  // Four columns laid out horizontally (bottom LED on the left) so every LED is visible.
  const pad = 6, labelW = 26, rowH = (H - pad * 2) / COLUMNS, n = rig.n;
  const ledW = (W - pad * 2 - labelW) / n;
  sctx.font = '10px ui-monospace, monospace'; sctx.textBaseline = 'middle';
  for (let c = 0; c < COLUMNS; c++) {
    const y = pad + c * rowH;
    sctx.fillStyle = '#77768a'; sctx.fillText(`C${c + 1}`, pad, y + rowH / 2);
    const a = rig.cols[c];
    for (let i = 0; i < n; i++) {
      const R = a[i * 3] * 255 | 0, G = a[i * 3 + 1] * 255 | 0, B = a[i * 3 + 2] * 255 | 0;
      sctx.fillStyle = `rgb(${R},${G},${B})`;
      sctx.fillRect(pad + labelW + i * ledW + (ledW > 3 ? 0.5 : 0), y + rowH * 0.18, Math.max(1, ledW - (ledW > 3 ? 1 : 0)), rowH * 0.64);
    }
  }
}

// ------------------------------------------------------------------ panels
const chips = [...document.querySelectorAll('#stateChips span')];
// Click a state chip to jump there; clicking FIGHT during a fight restarts it. The state
// machine only allows certain steps, so walk the shortest allowed path (e.g. FIGHT ->
// COOLDOWN -> IDLE -> WARMING), running the normal enter/exit logic on every step.
const ALL_STATES = Object.values(S);
function statePath(from, to) {
  if (from === to) {
    if (to !== S.Fight) return [];
    return [...statePath(from, S.Idle), ...statePath(S.Idle, S.Fight)]; // restart the fight
  }
  const prev = new Map([[from, null]]), q = [from];
  while (q.length) {
    const a = q.shift();
    if (a === to) break;
    for (const b of ALL_STATES) if (!prev.has(b) && canTransitionTo(a, b)) { prev.set(b, a); q.push(b); }
  }
  const path = []; for (let x = to; x && x !== from; x = prev.get(x)) path.unshift(x);
  return path;
}
async function jumpTo(target) {
  const from = BRAIN ? (brain?.info().currentState || S.Idle) : sm.currentState;
  const path = statePath(from, target);
  if (!path.length) return;
  log(`Jump ${from} -> ${path.join(' -> ')}`, 'state');
  for (const st of path) {
    if (BRAIN) await brain?.setState(st);
    else sm.transition(st);
  }
}
chips.forEach((c) => {
  c.setAttribute('role', 'button'); c.tabIndex = 0;
  c.title = c.dataset.s === 'FIGHT' ? 'Jump to FIGHT (click again to restart the fight)' : `Jump to ${c.dataset.s}`;
  c.onclick = () => jumpTo(c.dataset.s);
  c.onkeydown = (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); jumpTo(c.dataset.s); } };
});
const STATE_COLOR = { SUSPENDED: 'var(--s-SUSPENDED)', IDLE: 'var(--s-IDLE)', WARMING: 'var(--s-WARMING)', FIGHT: 'var(--s-FIGHT)', COOLDOWN: 'var(--s-COOLDOWN)' };
const pct = (x) => `${Math.max(0, Math.min(100, x * 100)).toFixed(1)}%`;
const DB_MIN = -90, DB_MAX = 0;
const dbPos = (db) => (Math.min(Math.max(db, DB_MIN), DB_MAX) - DB_MIN) / (DB_MAX - DB_MIN);

function updatePanels(info) {
  const st = info.currentState;
  chips.forEach((c) => { const on = c.dataset.s === st; c.classList.toggle('on', on); c.style.setProperty('--c', STATE_COLOR[c.dataset.s]); });
  const tag = $('stateTag'); tag.textContent = st; tag.style.setProperty('--c', STATE_COLOR[st]);
  $('stState').textContent = st;
  $('stTime').textContent = `${info.timeInStateSec.toFixed(1)} s`;
  if (BRAIN) { updateBrainPanels(info); return; }
  const fu = presence.fusion;
  $('stPresence').textContent = fu.present ? `present (${Math.max(0, cfg.leave_timeout_sec - presence.secondsSinceEvidence()).toFixed(0)}s)` : 'absent';
  $('stSessions').textContent = arena.totalSessions;
  const cap = Math.max(cfg.max_score, cfg.enter_threshold);
  $('presBar').style.width = pct(fu.score / cap); $('presThr').style.left = pct(cfg.enter_threshold / cap);
  $('presVal').textContent = `${fu.score.toFixed(2)} / ${cfg.enter_threshold}`;
  if (st === S.Warming) {
    $('energyBar').style.width = pct(info.energyPercentage); $('energyVal').textContent = `${info.energy.toFixed(1)} / ${info.maxEnergy}`;
  } else { $('energyBar').style.width = st === S.Fight ? '100%' : '0%'; $('energyVal').textContent = st === S.Fight ? 'full' : '-'; }
  $('fightLeft').textContent = st === S.Fight ? `${info.fightRemainingSec.toFixed(0)} s` : '-';
  $('inactive').textContent = st === S.Fight ? `${info.timeSinceLastActivitySec.toFixed(0)} / ${cfg.fight_inactivity_timeout_sec} s` : '-';
  $('punches').textContent = arena.currentSessionPunches;
  $('maxPower').textContent = arena.currentSessionMaxPower.toFixed(0);
  $('coolLeft').textContent = st === S.Cooldown ? `${info.cooldownRemainingSec.toFixed(0)} s` : '-';
  $('kos').textContent = game.kos;
  { const np = rig.nowPlaying(st); $('effectLabel').textContent = `${np.preview ? `${np.ambient.stage.toUpperCase()} (preview)` : st}: ${np.ambient.effect}`; }
  updateNowPlaying(st);

  // shout detector
  const f = lastFeat, d = detector.last;
  if (f) {
    $('lvlBar').style.width = pct(dbPos(f.band_db));
    $('floorMark').style.left = pct(dbPos(f.noise_floor_db));
    $('marginMark').style.left = pct(dbPos(f.noise_floor_db + (d ? d.margin : cfg.shout_margin_db)));
    $('lvlVal').textContent = `${f.band_db.toFixed(1)} dB (floor ${f.noise_floor_db.toFixed(1)}, +${f.db_above_floor.toFixed(1)})`;
    $('vadBar').style.width = pct(f.vad_prob); $('vadThr').style.left = pct(cfg.vad_threshold); $('vadVal').textContent = f.vad_prob.toFixed(2);
    $('featVals').textContent = `f0 ${f.pitch_hz.toFixed(0)}Hz conf ${f.pitch_conf.toFixed(2)} | centroid ${f.spectral_centroid_hz.toFixed(0)}Hz\ntilt ${f.spectral_tilt_db_per_khz.toFixed(1)}dB/kHz | flat ${f.spectral_flatness.toFixed(2)}${f.synthetic ? ' | synthetic' : ''}${f.playback_active ? ' | playback' : ''}`;
  }
  if (d) {
    for (const el of document.querySelectorAll('#checks span')) {
      const k = el.dataset.k; const v = k === 'gate' ? d.gate : k === 'loud' ? d.loud : d.checks[k];
      el.classList.toggle('pass', !!v); el.classList.toggle('fail', !v);
    }
    $('checks').title = `${d.passed}/4 checks pass (need ${cfg.shout_min_features})`;
  }
  const thr = cfg.shout_score_threshold, scap = thr * 1.25;
  $('scoreBar').style.width = pct(detector.score / scap); $('scoreThr').style.left = pct(thr / scap);
  $('scoreVal').textContent = `${detector.score.toFixed(1)} / ${thr}${d ? `  (${d.passed}/4, need ${cfg.shout_min_features})` : ''}`;
  const dur = detector.runFrames * FRAME_SECS, dcap = Math.max(cfg.shout_min_duration_sec * 1.5, 0.05);
  $('runBar').style.width = pct(dur / dcap); $('runThr').style.left = pct(cfg.shout_min_duration_sec / dcap);
  $('runVal').textContent = `${dur.toFixed(2)} / ${cfg.shout_min_duration_sec} s`;
}

function updateBrainPanels(info) {
  const st = info.currentState, tu = brain.tunables || {};
  $('stPresence').textContent = brain.ok === false ? 'brain unreachable' : brain.present ? 'present' : 'absent';
  $('stSessions').textContent = info.sessions;
  if (st === S.Warming && info.maxEnergy) {
    $('energyBar').style.width = pct(info.energyPercentage); $('energyVal').textContent = `${info.energy.toFixed(1)} / ${info.maxEnergy}`;
  } else { $('energyBar').style.width = st === S.Fight ? '100%' : '0%'; $('energyVal').textContent = st === S.Fight ? 'full' : '-'; }
  $('fightLeft').textContent = info.fightRemainingSec != null ? `${info.fightRemainingSec.toFixed(0)} s` : '-';
  $('inactive').textContent = info.timeSinceLastActivitySec != null ? `${info.timeSinceLastActivitySec.toFixed(0)} / ${tu.fight_inactivity_timeout_sec ?? '?'} s` : '-';
  $('punches').textContent = info.punches;
  $('maxPower').textContent = Number(info.maxPower).toFixed(0);
  $('coolLeft').textContent = info.cooldownRemainingSec != null ? `${info.cooldownRemainingSec.toFixed(0)} s` : '-';
  $('kos').textContent = game.kos;
  const eff = live?.status?.brain?.effect;
  $('effectLabel').textContent = `${st}: ${live?.status?.brain?.realtime ? 'brain realtime (UDP)' : `WLED effect${eff ? ` "${eff}"` : ''}`} - ${brainFrame.source === 'brain' ? 'frames from the brain' : brainFrame.source === 'wled' ? 'frames from the WLED live view' : 'no frames (no live view)'}`;
}

// ------------------------------------------------------------------ main loop
let lastT = performance.now(), lastPanel = 0;
function frame(tms) {
  const dt = Math.min((tms - lastT) / 1000, 0.1); lastT = tms;
  try {
    simNow += dt * cfg.time_scale;
    // The fight clock (and the inactivity timer) pause while the victory finale plays.
    if (!BRAIN && game.respawnAt != null && sm.currentState === S.Fight && arena.fightStartedAt != null) { arena.fightStartedAt += dt * cfg.time_scale; arena.lastActivityAt = simNow; }
    if (!BRAIN) {
      feats.pump();
      presence.tick(simNow, sensorOn);
      arena.tick(simNow);
    }
    const info = BRAIN ? brain.info() : arena.getStateInfo();
    const st = info.currentState;
    // after the victory finale: the next round (or, with "knocking out ends the fight", the cooldown)
    const t = performance.now() / 1000;
    if (game.respawnAt != null && t >= game.respawnAt) {
      game.respawnAt = null;
      if (prefs.koEnds && !BRAIN && sm.currentState === S.Fight) arena.endFightEarly('KnockOut');
      else nextRound();
    }
    // from round 2 the shadow heals while nobody punches it
    game.healing = st === S.Fight && game.respawnAt == null && game.round > 1 && t - game.lastHitAt > 1 && game.hp < game.maxHp && cfg.round_regen_per_sec > 0;
    if (game.healing) game.hp = Math.min(game.maxHp, game.hp + cfg.round_regen_per_sec * (game.round - 1) * dt * cfg.time_scale);
    if (t - game.lastHitAt > 1.0) game.combo = 0;
    if (pdown) scene.charge = { x: pdown.x, y: pdown.y, p: Math.min(1, (performance.now() - pdown.t) / 600) };
    else if (spaceDown) { const g = scene.bagGeom(); scene.charge = { x: scene.cx, y: (g.top + g.bot) / 2, p: Math.min(1, (performance.now() - spaceDown) / 600) }; }
    const level = lastFeat ? Math.min(1, Math.max(0, lastFeat.db_above_floor / 30)) : 0;
    scene.micLevel = level;
    const coolTotal = BRAIN ? brain.tunables?.cooldown_duration_sec ?? 60 : cfg.cooldown_duration_sec;
    const cooldownProgress = st === S.Cooldown && info.cooldownRemainingSec != null ? 1 - info.cooldownRemainingSec / Math.max(1, coolTotal) : 0;
    if (!BRAIN) rig.render(st, { energyPct: info.energyPercentage, fightRemaining: info.fightRemainingSec, cooldownProgress, cooldownLeft: info.cooldownRemainingSec, level });
    // No frames for 1.5 s and WLED not holding a realtime frame: the strip shows nothing we can see.
    else if (tms - brainFrame.at > 1500 && brainFrame.at > -1e9 && !live?.status?.brain?.realtime) { rig.cols.forEach((c) => c.fill(0)); brainFrame.at = -1e9; brainFrame.source = null; }
    scene.draw({
      state: st, energyPct: info.energyPercentage, hp: game.hp, hpPct: game.hp / game.maxHp, kos: game.kos, round: game.round, healing: game.healing,
      combo: game.combo, comboMult: 1 + Math.min(game.combo - 1, 10) * 0.1, present: (BRAIN ? brain.present : presence.fusion.present) || sensorOn,
      fightRemaining: info.fightRemainingSec, cooldownProgress, cooldownLeft: info.cooldownRemainingSec, summary: game.summary, respawning: game.respawnAt != null,
      inactiveFor: info.timeSinceLastActivitySec ?? 0, inactivityTimeout: BRAIN ? brain.tunables?.fight_inactivity_timeout_sec ?? 30 : cfg.fight_inactivity_timeout_sec,
    }, dt);
    ambience.update(st, info.energyPercentage);
    drawStrips();
    if (!BRAIN) live?.frame(rig, tms);
    if (tms - lastPanel > 66) { lastPanel = tms; updatePanels(info); }
  } catch (err) {
    errors.push(String(err && err.stack || err));
    console.error(err);
  }
  requestAnimationFrame(frame);
}
window.addEventListener('resize', () => scene.resize());
new ResizeObserver(() => scene.resize()).observe($('scene'));
requestAnimationFrame(frame);
log(BRAIN ? 'Brain mode: state, LEDs and controls are the real brain-rs (via the bridge)' : 'Simulator ready: brain-rs main.rs sets IDLE at startup', 'state');
if (!LIVE_REQUESTED) initConnect({ log });

// Test / console hook
window.sim = {
  playlist,
  get state() { return BRAIN ? brain.info().currentState : sm.currentState; }, get info() { return BRAIN ? brain.info() : arena.getStateInfo(); }, get game() { return game; },
  get cfg() { return cfg; }, get errors() { return errors; }, get lastFeat() { return lastFeat; }, get detector() { return detector; },
  rig, scene, hitPicker, punch: (p) => punch(p), previewStage, setOverride, setFxParam, nextRound, setSensor, get live() { return live; }, get brain() { return brain; }, brainFrame, shoutDown, shoutUp, shoutFor: (ms) => feats.pulse(ms), skip: (sec) => { simNow += sec; presence.tick(simNow, sensorOn); arena.tick(simNow); }, arena, presence, sm,
};
