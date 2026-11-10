// UI glue: wires the ported brain-rs logic to the scene, LEDs, mic and panels.
import { TUNABLES, loadTunables, saveTunables, exportYaml, formatValue, defaults } from './config.js';
import { EventBus } from './events.js';
import { StateMachine, ArenaState } from './state_machine.js';
import { ArenaManager } from './arena_manager.js';
import { PresenceService } from './presence.js';
import { ShoutDetector } from './shout_detector.js';
import { FeatureSource } from './feature_source.js';
import { FRAME_SECS } from './audio_frontend.js';
import { LedRig, COLUMNS, EFFECT_LABELS } from './wled.js';
import { Scene } from './scene.js';
import { Sfx } from './sfx.js';

const $ = (id) => document.getElementById(id);
const S = ArenaState;

// ------------------------------------------------------------------ model
const cfg = loadTunables();
const PREFS_KEY = 'sw-sim-prefs-v1';
const prefs = Object.assign({ fixInactivity: true, fixRearm: true, koEnds: false, musicPlayback: true, ledMode: 'proposal', sound: true },
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
const rig = new LedRig(cfg.leds_per_column); rig.mode = prefs.ledMode;
const scene = new Scene($('scene'), rig);

const game = { hp: cfg.shadow_hp, maxHp: cfg.shadow_hp, kos: 0, combo: 0, lastHitAt: -9, respawnAt: null, summary: '', shouts: 0, lastShout: null };
let lastFeat = null;
let sensorOn = false;
let errors = [];

arena.start();
// main.rs: "Set initial state to IDLE" after the arena manager starts.
sm.transition(S.Idle);

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
    case 'PresenceDetected': log(`PresenceDetected { detected: ${e.detected} }`, 'presence'); break;
    case 'ShoutDetected': {
      game.shouts++; game.lastShout = e;
      const st = sm.currentState;
      log(`ShoutDetected intensity=${e.intensity.toFixed(2)} dur=${e.duration_sec.toFixed(2)}s level=${e.level_db.toFixed(1)}dB${st === S.Warming ? ` (+${(e.intensity * cfg.shout_energy_multiplier).toFixed(1)} energy)` : ` (ignored: ${st})`}`, st === S.Warming ? 'shout' : 'ignored');
      rig.shout(e.intensity); sfx.shout(e.intensity);
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

function onStateEnter(to) {
  if (to === S.Fight) {
    game.maxHp = cfg.shadow_hp; game.hp = game.maxHp; game.kos = 0; game.combo = 0; game.respawnAt = null;
    scene.resetDamage();
    scene.showBanner('FIGHT!', 'punch the shadow');
    sfx.boom();
  } else if (to === S.Cooldown) {
    const reason = { TimeElapsed: 'time up', InactivityTimeout: 'no punches', KnockOut: 'knock-out' }[arena.lastEndReason] || '';
    game.summary = `${arena.currentSessionPunches} punches, max ${arena.currentSessionMaxPower.toFixed(0)}, ${game.kos} KO${game.kos === 1 ? '' : 's'}${reason ? `, ended: ${reason}` : ''}`;
    const title = { KnockOut: 'VICTORY', TimeElapsed: 'TIME UP', InactivityTimeout: 'TOO QUIET' }[arena.lastEndReason] || 'FIGHT OVER';
    scene.showBanner(title, game.summary, 3.5);
  } else if (to === S.Warming) {
    scene.showBanner('WAKE IT UP', 'shout!', 1.6);
  }
  if (to !== S.Fight) game.respawnAt = null;
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
function shoutDown() { sfx.ensure(); shoutDownAt = performance.now(); feats.setHeld(true); shoutBtn.classList.add('down'); }
function shoutUp() {
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
function setSensor(on) {
  sensorOn = on; presenceToggle.checked = on;
  log(`Sim presence sensor ${on ? 'ON' : 'OFF'}${on ? '' : ` (fusion leaves after ${cfg.leave_timeout_sec}s without evidence)`}`, 'presence');
}
presenceToggle.onchange = () => { sfx.ensure(); setSensor(presenceToggle.checked); };
$('forceAbsent').onclick = () => { if (sensorOn) setSensor(false); presence.forcePresent(false); log('Manual override: force absent', 'presence'); };
$('forcePresent').onclick = () => { presence.forcePresent(true); log('Manual override: force present', 'presence'); };

// ------------------------------------------------------------------ punches
function punch(power, info) {
  sfx.ensure();
  power = Math.max(1, Math.min(100, power));
  info = info || { dir: Math.random() < 0.5 ? 1 : -1, hx: scene.cx + (Math.random() - 0.5) * 20, hy: scene.H * (0.42 + Math.random() * 0.15) };
  bus.publish({ type: 'PunchDetected', power });
  const counted = sm.currentState === S.Fight;
  scene.hit(power, info); rig.punch(power); sfx.punch(power);
  if (!counted) { log(`PunchDetected power=${power.toFixed(0)} (not counted: ${sm.currentState})`, 'ignored'); return; }
  const t = performance.now() / 1000;
  game.combo = t - game.lastHitAt < 1.0 ? game.combo + 1 : 1;
  game.lastHitAt = t;
  const mult = 1 + Math.min(game.combo - 1, 10) * 0.1;
  let dmg = 0;
  if (game.respawnAt == null) {
    dmg = power * cfg.damage_per_power * mult;
    game.hp = Math.max(0, game.hp - dmg);
  }
  const col = power >= 75 ? '#ffdf6a' : power >= 45 ? '#ffae6a' : '#ffffff';
  scene.floater(dmg ? `-${dmg.toFixed(0)}` : `${power.toFixed(0)}`, info.hx + info.dir * -30, info.hy - 20, col, power >= 75);
  log(`PunchDetected power=${power.toFixed(0)} dmg=${dmg.toFixed(1)}${game.combo > 1 ? ` combo ${game.combo}` : ''}`, 'punch');
  if (game.hp <= 0 && game.respawnAt == null) {
    game.kos++;
    scene.ko(); sfx.ko();
    log(`Shadow knocked out (#${game.kos})`, 'ko');
    if (prefs.koEnds) { arena.endFightEarly('KnockOut'); return; }
    scene.showBanner('SHADOW BROKEN', 'it rises again, stronger...');
    game.respawnAt = t + 1.8;
  }
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
const soundBtn = $('soundBtn');
const renderSound = () => { soundBtn.textContent = sfx.enabled ? 'Sound on' : 'Sound off'; };
soundBtn.onclick = () => { sfx.enabled = !sfx.enabled; prefs.sound = sfx.enabled; savePrefs(); renderSound(); if (!sfx.enabled) sfx.stopMusic(); else { sfx.ensure(); if (sm.currentState === S.Fight) sfx.startMusic(); } };
renderSound();
const ledMode = $('ledMode');
ledMode.value = prefs.ledMode;
ledMode.onchange = () => { rig.mode = ledMode.value; prefs.ledMode = ledMode.value; savePrefs(); log(`LED effects: ${ledMode.value}`, 'ignored'); };

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
const GROUPS = { arena: 'arena (ArenaConfig)', audio: 'audio: shout detector', presence: 'presence (fusion)', sim: 'simulator only (not exported)' };
const tunEls = {};
function buildTunables() {
  const root = $('tunables'); root.textContent = '';
  for (const [sec, title] of Object.entries(GROUPS)) {
    const det = document.createElement('details'); det.className = 'group'; det.open = sec === 'arena' || sec === 'sim';
    const sum = document.createElement('summary'); sum.textContent = title; det.append(sum);
    for (const t of TUNABLES.filter((x) => x.section === sec)) {
      const row = document.createElement('div'); row.className = 'tun';
      const lab = document.createElement('label'); lab.textContent = t.key; lab.title = t.help; lab.htmlFor = `t_${t.key}`;
      const num = document.createElement('input'); num.type = 'number'; num.step = t.step; num.min = t.min; num.max = t.max;
      const rng = document.createElement('input'); rng.type = 'range'; rng.id = `t_${t.key}`; rng.min = t.min; rng.max = t.max; rng.step = t.step;
      const help = document.createElement('div'); help.className = 'help'; help.textContent = t.help;
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
      row.append(lab, num, rng, help); det.append(row);
      set(cfg[t.key]);
    }
    root.append(det);
  }
}
function onTunable(key) {
  if (['warming_energy_threshold', 'shout_energy_multiplier', 'energy_decay_rate'].includes(key)) arena.onTunablesChanged();
  if (key === 'leds_per_column' && rig.n !== cfg.leds_per_column) rig.resize(cfg.leds_per_column);
  if (key === 'synth_shout_db') feats.synthDb = cfg.synth_shout_db;
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
  $('effectLabel').textContent = `${st}: ${EFFECT_LABELS[rig.mode][st]}`;

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

// ------------------------------------------------------------------ main loop
let lastT = performance.now(), lastPanel = 0;
function frame(tms) {
  const dt = Math.min((tms - lastT) / 1000, 0.1); lastT = tms;
  try {
    simNow += dt * cfg.time_scale;
    feats.pump();
    presence.tick(simNow, sensorOn);
    arena.tick(simNow);
    const info = arena.getStateInfo();
    const st = info.currentState;
    // respawn after KO
    const t = performance.now() / 1000;
    if (game.respawnAt != null && t >= game.respawnAt) {
      game.respawnAt = null; game.maxHp = Math.round(game.maxHp * 1.25); game.hp = game.maxHp; scene.resetDamage();
      log(`Shadow returns with ${game.maxHp} HP`, 'ko');
    }
    if (t - game.lastHitAt > 1.0) game.combo = 0;
    if (pdown) scene.charge = { x: pdown.x, y: pdown.y, p: Math.min(1, (performance.now() - pdown.t) / 600) };
    else if (spaceDown) { const g = scene.bagGeom(); scene.charge = { x: scene.cx, y: (g.top + g.bot) / 2, p: Math.min(1, (performance.now() - spaceDown) / 600) }; }
    const level = lastFeat ? Math.min(1, Math.max(0, lastFeat.db_above_floor / 30)) : 0;
    const cooldownProgress = st === S.Cooldown ? 1 - info.cooldownRemainingSec / Math.max(1, cfg.cooldown_duration_sec) : 0;
    rig.render(st, { energyPct: info.energyPercentage, fightRemaining: info.fightRemainingSec, cooldownProgress, cooldownLeft: info.cooldownRemainingSec, level });
    scene.draw({
      state: st, energyPct: info.energyPercentage, hp: game.hp, hpPct: game.hp / game.maxHp, kos: game.kos,
      combo: game.combo, comboMult: 1 + Math.min(game.combo - 1, 10) * 0.1, present: presence.fusion.present || sensorOn,
      fightRemaining: info.fightRemainingSec, cooldownProgress, cooldownLeft: info.cooldownRemainingSec, summary: game.summary, respawning: game.respawnAt != null,
      inactiveFor: info.timeSinceLastActivitySec ?? 0, inactivityTimeout: cfg.fight_inactivity_timeout_sec,
    }, dt);
    drawStrips();
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
log('Simulator ready: brain-rs main.rs sets IDLE at startup', 'state');

// Test / console hook
window.sim = {
  get state() { return sm.currentState; }, get info() { return arena.getStateInfo(); }, get game() { return game; },
  get cfg() { return cfg; }, get errors() { return errors; }, get lastFeat() { return lastFeat; }, get detector() { return detector; },
  punch: (p) => punch(p), setSensor, shoutFor: (ms) => feats.pulse(ms), skip: (sec) => { simNow += sec; presence.tick(simNow, sensorOn); arena.tick(simNow); }, arena, presence, sm,
};
