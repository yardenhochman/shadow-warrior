// Brain mode client: the real brain-rs runs the game. The bridge proxies its API under
// /brain/ (same origin, no CORS). State comes from GET /api/state (polled), events from the
// SSE stream /api/events, and the page's controls call the brain's manual-control API.
const toApiState = (s) => s[0] + s.slice(1).toLowerCase(); // 'FIGHT' -> 'Fight' (serde name)
const fromApiState = (s) => String(s || '').toUpperCase();

// Event types the page reacts to or logs (brain-rs core/events.rs, serde tag "type").
const EVENT_TYPES = ['StateChanged', 'PresenceDetected', 'ShoutDetected', 'EnergyFull', 'FightStarted', 'FightEnded',
  'SessionStarted', 'SessionEnded', 'PunchDetected', 'MusicStarted', 'MusicStopped', 'BleDeviceConnected',
  'BleDeviceDisconnected', 'WledConnected', 'WledDisconnected', 'AudioError'];

export class BrainClient {
  constructor({ base = '/brain', onEvent, onState, log, pollMs = 250 }) {
    this.base = base; this.onEvent = onEvent; this.onState = onState; this.log = log; this.pollMs = pollMs;
    this.state = null; this.fetchedAt = 0; this.ok = null; this.tunables = null; this.events = 0; this.present = false;
    this.lastError = '';
  }

  start() {
    this.poll();
    this.loadTunables();
    this.connectEvents();
  }

  async poll() {
    try {
      const r = await fetch(`${this.base}/api/state`, { cache: 'no-store' });
      const j = await r.json();
      if (!j.success) throw new Error(j.error || `HTTP ${r.status}`);
      this.state = j.data; this.fetchedAt = performance.now();
      if (this.ok !== true) this.log?.('Brain connected: the real brain-rs runs the game', 'state');
      this.ok = true; this.lastError = '';
    } catch (e) {
      if (this.ok !== false) this.log?.(`Brain unreachable (${e.message || e}); retrying`, 'ignored');
      this.ok = false; this.lastError = String(e.message || e);
    }
    this.onState?.(this);
    setTimeout(() => this.poll(), this.pollMs);
  }

  async loadTunables() {
    try { const j = await (await fetch(`${this.base}/api/arena/config`)).json(); if (j.success) this.tunables = j.data; } catch { /* retried below */ }
    if (!this.tunables) setTimeout(() => this.loadTunables(), 2000);
  }

  connectEvents() {
    const es = new EventSource(`${this.base}/api/events`);
    for (const t of EVENT_TYPES) {
      es.addEventListener(t, (e) => {
        let m; try { m = JSON.parse(e.data); } catch { return; }
        this.events++;
        if (m.type === 'PresenceDetected') this.present = !!m.data?.detected;
        this.onEvent?.(m.type, m.data || {});
      });
    }
    es.onerror = () => { /* EventSource reconnects by itself */ };
    this.es = es;
  }

  async post(path, body) {
    try {
      const r = await fetch(`${this.base}${path}`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body ?? {}) });
      const j = await r.json().catch(() => ({}));
      if (!r.ok || j.success === false) { this.log?.(`Brain ${path}: ${j.error || `HTTP ${r.status}`}`, 'ignored'); return false; }
      return true;
    } catch (e) { this.log?.(`Brain ${path}: ${e.message || e}`, 'ignored'); return false; }
  }

  presence(detected) { return this.post('/api/arena/presence', { detected }); }
  shout(intensity = 1) { return this.post('/api/arena/shout', { intensity }); }
  punch(power) { return this.post('/api/arena/punch', { power }); }
  setState(s) { return this.post('/api/arena/state', { state: toApiState(s) }); }

  /** The brain's arena info in the shape of the JS ArenaManager.getStateInfo(), advanced locally between polls. */
  info() {
    const a = this.state?.arena, st = this.state?.statistics || {};
    if (!a) return { currentState: 'IDLE', timeInStateSec: 0, punches: 0, maxPower: 0, sessions: 0 };
    const dt = (performance.now() - this.fetchedAt) / 1000;
    const s = fromApiState(a.current_state), tu = this.tunables;
    const info = { currentState: s, timeInStateSec: a.time_in_state_sec + dt,
      punches: st.current_session_punches ?? 0, maxPower: st.current_session_max_power ?? 0, sessions: st.total_sessions ?? 0 };
    if (a.energy != null) { info.energy = a.energy; info.maxEnergy = a.max_energy; info.energyPercentage = a.energy_percentage ?? a.energy / a.max_energy; }
    if (a.fight_remaining_sec != null) {
      info.fightRemainingSec = Math.max(0, a.fight_remaining_sec - dt);
      info.timeSinceLastActivitySec = (a.time_since_last_activity_sec ?? 0) + dt;
    }
    if (s === 'COOLDOWN' && tu) info.cooldownRemainingSec = Math.max(0, tu.cooldown_duration_sec - info.timeInStateSec);
    return info;
  }
}
