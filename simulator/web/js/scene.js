// Arena scene (canvas 2D): participant's view of the bag with the shadow
// opponent and the four LED columns in an arc behind it, plus a top-down inset.
import { ArenaState } from './state_machine.js';
import { COLUMNS } from './wled.js';

const COL_ANGLES = [-62, -21, 21, 62].map((d) => d * Math.PI / 180); // around the bag, from "behind"
const ARC_R = 2.2;      // m, bag -> columns
const VIEW_Z = -2.3;    // m, participant eye in front of the bag
const CAM_H = 1.6;      // m, eye height
const COL_H = 1.9;      // m, column height
const BAG_TOP = 1.65, BAG_BOT = 0.55, BAG_R = 0.2;

export class Scene {
  constructor(canvas, rig) {
    this.canvas = canvas; this.ctx = canvas.getContext('2d'); this.rig = rig;
    this.swing = 0; this.swingV = 0; this.squash = 0; this.hitFlash = 0; this.shake = 0;
    this.particles = []; this.floaters = []; this.cracks = [];
    this.shadowAlpha = 0; this.eyeOpen = 0; this.hpShown = 1; this.koFlash = 0;
    this.charge = null; // {x, y, p}
    this.banner = null; // {text, sub, t0, dur}
    this.resize();
  }

  resize() {
    const r = this.canvas.getBoundingClientRect();
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    this.W = Math.max(1, r.width); this.H = Math.max(1, r.height);
    this.canvas.width = Math.round(this.W * dpr); this.canvas.height = Math.round(this.H * dpr);
    this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    this.f = Math.min(this.H * 0.95, this.W * 0.62);
    // Centre the arena vertically when the width limits the zoom (phones), leaving room for the HUD.
    const hud = 92, foot = 40;
    this.horizon = Math.max(this.H * 0.3, hud + 0.15 * this.f + ((this.H - hud - foot) - 0.87 * this.f) / 2);
    this.cx = this.W / 2;
  }

  /** world (x, y up, z away) -> screen */
  proj(x, y, z) {
    const d = z - VIEW_Z;
    return [this.cx + this.f * x / d, this.horizon - this.f * (y - CAM_H) / d, this.f / d];
  }

  bagGeom() {
    const [px, py] = this.proj(0, 2.6, 0);
    const [, top, s] = this.proj(0, BAG_TOP, 0);
    const [, bot] = this.proj(0, BAG_BOT, 0);
    return { px, py: Math.max(py, -20), top, bot, rw: BAG_R * s, s };
  }

  /** screen point -> is it near the bag / on the canvas: any tap counts, return hit side */
  hitInfo(x, y) {
    const g = this.bagGeom();
    const dir = x < this.cx ? 1 : -1; // hit from the left pushes bag right
    const hy = Math.min(Math.max(y, g.top + 10), g.bot - 10);
    const hx = this.cx + Math.min(Math.max(x - this.cx, -g.rw), g.rw);
    return { dir, hx, hy };
  }

  hit(power, info) {
    const p = power / 100;
    this.swingV += info.dir * (0.6 + 2.4 * p);
    this.squash = Math.max(this.squash, 0.25 + 0.5 * p);
    this.hitFlash = Math.max(this.hitFlash, 0.5 + 0.5 * p);
    this.shake = Math.max(this.shake, 4 + 16 * p);
    const n = Math.round(10 + 40 * p);
    for (let i = 0; i < n; i++) {
      const a = Math.random() * Math.PI * 2, v = 80 + Math.random() * 420 * (0.4 + p);
      this.particles.push({ x: info.hx, y: info.hy, vx: Math.cos(a) * v - info.dir * 120, vy: Math.sin(a) * v - 80, life: 0.35 + Math.random() * 0.5, age: 0,
        kind: Math.random() < 0.3 ? 'smoke' : 'spark', size: 1.5 + Math.random() * 2.5 * (0.5 + p) });
    }
    if (p > 0.55 && this.cracks.length < 14) {
      const g = this.bagGeom();
      this.cracks.push({ u: (info.hx - this.cx) / g.rw, v: (info.hy - g.top) / (g.bot - g.top), seed: Math.random() * 1000, size: p });
    }
  }

  floater(text, x, y, color, big = false) { this.floaters.push({ text, x, y, color, big, age: 0, life: big ? 1.4 : 0.9 }); }
  showBanner(text, sub, dur = 2.2) { this.banner = { text, sub, t0: performance.now() / 1000, dur }; }
  ko() { this.koFlash = 1; this.shake = 26; this.cracks = []; }
  resetDamage() { this.cracks = []; }

  step(dt) {
    // Damped pendulum + squash spring.
    const k = 18, damp = 2.6;
    this.swingV += (-k * this.swing - damp * this.swingV) * dt;
    this.swing += this.swingV * dt;
    this.swing = Math.max(-0.5, Math.min(0.5, this.swing));
    this.squash *= Math.pow(0.002, dt);
    this.hitFlash *= Math.pow(0.003, dt);
    this.shake *= Math.pow(0.001, dt);
    this.koFlash *= Math.pow(0.08, dt);
    for (const q of this.particles) {
      q.age += dt; q.x += q.vx * dt; q.y += q.vy * dt;
      if (q.kind === 'spark') { q.vy += 900 * dt; q.vx *= Math.pow(0.2, dt); } else { q.vx *= Math.pow(0.05, dt); q.vy -= 60 * dt; }
    }
    this.particles = this.particles.filter((q) => q.age < q.life);
    for (const fl of this.floaters) fl.age += dt;
    this.floaters = this.floaters.filter((fl) => fl.age < fl.life);
  }

  /**
   * @param {object} v view model: state, energyPct, hpPct, combo, present, fightRemaining, cooldownProgress, kos
   */
  draw(v, dt) {
    this.step(dt);
    const { ctx, W, H } = this;
    const t = performance.now() / 1000;
    const S = ArenaState;
    // shadow materialisation target per state
    const target = v.state === S.Fight ? (v.respawning ? 0.15 : 1) : v.state === S.Warming ? 0.15 + 0.75 * (v.energyPct || 0)
      : v.state === S.Cooldown ? Math.max(0, 0.35 - (v.cooldownProgress || 0)) : v.state === S.Idle ? 0.08 : 0;
    this.shadowAlpha += (target - this.shadowAlpha) * Math.min(1, dt * 3);
    const eyeT = v.state === S.Fight ? 1 : v.state === S.Warming ? (v.energyPct || 0) : 0;
    this.eyeOpen += (eyeT - this.eyeOpen) * Math.min(1, dt * 4);
    this.hpShown += ((v.hpPct ?? 1) - this.hpShown) * Math.min(1, dt * 6);

    ctx.save();
    ctx.clearRect(0, 0, W, H);
    const sx = (Math.random() - 0.5) * this.shake, sy = (Math.random() - 0.5) * this.shake;
    ctx.translate(sx, sy);

    // Night sky + floor
    const sky = ctx.createLinearGradient(0, 0, 0, H);
    sky.addColorStop(0, '#05060c'); sky.addColorStop(0.3, '#0b0b16'); sky.addColorStop(0.31, '#120d0c'); sky.addColorStop(1, '#1c1410');
    ctx.fillStyle = sky; ctx.fillRect(-30, -30, W + 60, H + 60);
    this.drawStars(t);

    // Columns sorted far -> near
    const cols = COL_ANGLES.map((a, c) => ({ c, x: ARC_R * Math.sin(a), z: ARC_R * Math.cos(a) }));
    cols.sort((a, b) => b.z - a.z);
    // floor glow first
    ctx.globalCompositeOperation = 'lighter';
    for (const col of cols) {
      const [r, g, b] = this.rig.average(col.c);
      const [x, y, s] = this.proj(col.x, 0, col.z);
      const rad = 1.6 * s;
      const grd = ctx.createRadialGradient(x, y, 0, x, y, rad);
      grd.addColorStop(0, `rgba(${r * 255 | 0},${g * 255 | 0},${b * 255 | 0},0.55)`);
      grd.addColorStop(1, 'rgba(0,0,0,0)');
      ctx.fillStyle = grd;
      ctx.beginPath(); ctx.ellipse(x, y, rad, rad * 0.28, 0, 0, Math.PI * 2); ctx.fill();
    }
    ctx.globalCompositeOperation = 'source-over';
    for (const col of cols) this.drawColumn(col);

    this.drawShadowAndBag(v, t);

    // particles
    ctx.globalCompositeOperation = 'lighter';
    for (const q of this.particles) {
      const a = 1 - q.age / q.life;
      if (q.kind === 'spark') { ctx.fillStyle = `rgba(255,${160 + 90 * a | 0},${60 * a | 0},${a})`; ctx.fillRect(q.x, q.y, q.size, q.size); }
    }
    ctx.globalCompositeOperation = 'source-over';
    for (const q of this.particles) {
      if (q.kind !== 'smoke') continue;
      const a = 1 - q.age / q.life;
      ctx.fillStyle = `rgba(10,6,14,${0.5 * a})`;
      ctx.beginPath(); ctx.arc(q.x, q.y, q.size * 4 * (1.5 - a), 0, Math.PI * 2); ctx.fill();
    }

    // charge ring
    if (this.charge) {
      const { x, y, p } = this.charge;
      ctx.strokeStyle = `rgba(255,${200 - 150 * p | 0},60,0.9)`; ctx.lineWidth = 4;
      ctx.beginPath(); ctx.arc(x, y, 26 + 10 * p, -Math.PI / 2, -Math.PI / 2 + p * Math.PI * 2); ctx.stroke();
    }

    // floaters
    ctx.textAlign = 'center';
    for (const fl of this.floaters) {
      const a = 1 - fl.age / fl.life;
      const sc = fl.big ? 1 + 0.4 * Math.min(1, fl.age * 6) : 1;
      ctx.font = `800 ${(fl.big ? 34 : 22) * sc | 0}px system-ui, sans-serif`;
      ctx.fillStyle = fl.color; ctx.globalAlpha = a;
      ctx.strokeStyle = 'rgba(0,0,0,0.8)'; ctx.lineWidth = 4;
      ctx.strokeText(fl.text, fl.x, fl.y - fl.age * 60); ctx.fillText(fl.text, fl.x, fl.y - fl.age * 60);
      ctx.globalAlpha = 1;
    }
    ctx.restore();

    if (this.koFlash > 0.02) { ctx.fillStyle = `rgba(255,240,220,${this.koFlash * 0.6})`; ctx.fillRect(0, 0, W, H); }
    if (v.state === S.Suspended) { ctx.fillStyle = 'rgba(0,0,0,0.55)'; ctx.fillRect(0, 0, W, H); }

    this.drawHud(v, t);
    this.drawMinimap(v);
  }

  drawStars(t) {
    const { ctx, W } = this;
    if (!this.stars) this.stars = Array.from({ length: 70 }, () => [Math.random(), Math.random() * 0.28, Math.random()]);
    for (const [x, y, p] of this.stars) {
      const a = 0.25 + 0.35 * (0.5 + 0.5 * Math.sin(t * (0.5 + p) + p * 10));
      ctx.fillStyle = `rgba(200,210,255,${a})`;
      ctx.fillRect(x * W, y * this.H, 1.2, 1.2);
    }
  }

  drawColumn(col) {
    const { ctx } = this;
    const [x, yb, s] = this.proj(col.x, 0, col.z);
    const [, yt] = this.proj(col.x, COL_H, col.z);
    const w = Math.max(6, 0.12 * s);
    const leds = this.rig.cols[col.c], n = this.rig.n;
    // Perspex tube
    ctx.fillStyle = 'rgba(160,180,220,0.06)';
    ctx.strokeStyle = 'rgba(200,220,255,0.18)'; ctx.lineWidth = 1;
    ctx.beginPath(); ctx.roundRect(x - w / 2, yt, w, yb - yt, w / 2); ctx.fill(); ctx.stroke();
    // glow pass
    ctx.globalCompositeOperation = 'lighter';
    const segH = (yb - yt) / n;
    const step = Math.max(1, Math.floor(n / 30));
    for (let i = 0; i < n; i += step) {
      const r = leds[i * 3], g = leds[i * 3 + 1], b = leds[i * 3 + 2];
      const m = Math.max(r, g, b); if (m < 0.03) continue;
      const y = yb - (i + 0.5) * segH;
      const gr = ctx.createRadialGradient(x, y, 0, x, y, w * 2.6);
      gr.addColorStop(0, `rgba(${r * 255 | 0},${g * 255 | 0},${b * 255 | 0},${0.22 * Math.min(1, step / 2 + 0.5)})`);
      gr.addColorStop(1, 'rgba(0,0,0,0)');
      ctx.fillStyle = gr; ctx.fillRect(x - w * 2.6, y - w * 2.6, w * 5.2, w * 5.2);
    }
    // per-LED core
    const lw = Math.max(2, w * 0.38);
    for (let i = 0; i < n; i++) {
      const r = leds[i * 3], g = leds[i * 3 + 1], b = leds[i * 3 + 2];
      const y = yb - (i + 1) * segH;
      ctx.fillStyle = `rgb(${r * 255 | 0},${g * 255 | 0},${b * 255 | 0})`;
      ctx.fillRect(x - lw / 2, y + segH * 0.12, lw, Math.max(1, segH * 0.76));
    }
    ctx.globalCompositeOperation = 'source-over';
    // base
    ctx.fillStyle = '#0a0a0e';
    ctx.fillRect(x - w * 0.8, yb - 2, w * 1.6, Math.max(4, 0.06 * s));
  }

  drawShadowAndBag(v, t) {
    const { ctx } = this;
    const g = this.bagGeom();
    const ang = this.swing;
    const sq = this.squash;
    ctx.save();
    ctx.translate(g.px, g.py);
    ctx.rotate(ang);
    ctx.translate(-g.px, -g.py);
    const cx = g.px, top = g.top, bot = g.bot, rw = g.rw * (1 + sq * 0.25), h = bot - top;
    const a = this.shadowAlpha;

    // chain
    ctx.strokeStyle = '#3a3a44'; ctx.lineWidth = 2;
    ctx.beginPath(); ctx.moveTo(cx, g.py); ctx.lineTo(cx, top); ctx.stroke();

    // Shadow body (behind the bag): smoky head + shoulders + arms in guard
    if (a > 0.01) {
      const sway = Math.sin(t * 1.7) * 4;
      const headY = top - h * 0.2;
      ctx.globalAlpha = a;
      for (let i = 0; i < 6; i++) { // smoke aura
        const ph = t * 0.8 + i * 1.3;
        const rx = cx + Math.sin(ph) * rw * 1.6, ry = top + h * 0.35 + Math.cos(ph * 1.3) * h * 0.4;
        const gr = ctx.createRadialGradient(rx, ry, 0, rx, ry, rw * 3.2);
        gr.addColorStop(0, 'rgba(40,0,30,0.35)'); gr.addColorStop(1, 'rgba(0,0,0,0)');
        ctx.fillStyle = gr; ctx.fillRect(rx - rw * 3.2, ry - rw * 3.2, rw * 6.4, rw * 6.4);
      }
      ctx.fillStyle = '#050307';
      ctx.strokeStyle = `rgba(255,40,40,${0.35 + 0.4 * this.hitFlash})`; ctx.lineWidth = 2;
      // shoulders/torso
      ctx.beginPath();
      ctx.moveTo(cx - rw * 2.6, top + h * 0.28 + sway * 0.3);
      ctx.quadraticCurveTo(cx - rw * 2.2, top - h * 0.02, cx - rw * 0.7, top - h * 0.06);
      ctx.lineTo(cx + rw * 0.7, top - h * 0.06);
      ctx.quadraticCurveTo(cx + rw * 2.2, top - h * 0.02, cx + rw * 2.6, top + h * 0.28 - sway * 0.3);
      ctx.lineTo(cx + rw * 1.3, bot - h * 0.1);
      ctx.lineTo(cx - rw * 1.3, bot - h * 0.1);
      ctx.closePath(); ctx.fill(); ctx.stroke();
      // arms in guard (fists in front of the face)
      for (const sgn of [-1, 1]) {
        const recoil = sgn * this.swing * 40;
        ctx.beginPath();
        ctx.moveTo(cx + sgn * rw * 2.4, top + h * 0.22);
        ctx.quadraticCurveTo(cx + sgn * rw * 3.4, top + h * 0.5 + recoil, cx + sgn * rw * 1.6, top + h * 0.12 - sway * sgn * 0.5);
        ctx.lineWidth = rw * 0.9; ctx.strokeStyle = '#070409'; ctx.stroke();
        ctx.lineWidth = 2;
        ctx.fillStyle = '#0b060d';
        ctx.beginPath(); ctx.arc(cx + sgn * rw * 1.5, top + h * 0.08 - sway * sgn * 0.5, rw * 0.62, 0, Math.PI * 2); ctx.fill();
        ctx.strokeStyle = `rgba(255,40,40,${0.3 + 0.4 * this.hitFlash})`; ctx.stroke();
      }
      // head
      ctx.fillStyle = '#050307';
      ctx.beginPath(); ctx.ellipse(cx + sway * 0.5, headY, rw * 0.95, rw * 1.15, 0, 0, Math.PI * 2); ctx.fill();
      ctx.strokeStyle = `rgba(255,40,40,${0.35 + 0.4 * this.hitFlash})`; ctx.stroke();
      // eyes
      const eo = this.eyeOpen * (this.hitFlash > 0.4 ? 0.25 : 1) * (0.85 + 0.15 * Math.sin(t * 9));
      for (const sgn of [-1, 1]) {
        const ex = cx + sway * 0.5 + sgn * rw * 0.42, ey = headY - rw * 0.05;
        ctx.globalCompositeOperation = 'lighter';
        const gr = ctx.createRadialGradient(ex, ey, 0, ex, ey, rw * 0.9);
        gr.addColorStop(0, `rgba(255,60,30,${0.9 * eo})`); gr.addColorStop(1, 'rgba(0,0,0,0)');
        ctx.fillStyle = gr; ctx.fillRect(ex - rw, ey - rw, rw * 2, rw * 2);
        ctx.fillStyle = `rgba(255,${200 + 55 * this.hitFlash | 0},180,${eo})`;
        ctx.beginPath(); ctx.ellipse(ex, ey, rw * 0.26, Math.max(0.6, rw * 0.11 * eo), sgn * 0.25, 0, Math.PI * 2); ctx.fill();
        ctx.globalCompositeOperation = 'source-over';
      }
      ctx.globalAlpha = 1;
    }

    // Bag (cylinder) on top of the shadow torso
    const bw = rw * 1.0;
    const body = ctx.createLinearGradient(cx - bw, 0, cx + bw, 0);
    body.addColorStop(0, '#1a0f0c'); body.addColorStop(0.35, '#5a2c1f'); body.addColorStop(0.55, '#6b3524'); body.addColorStop(1, '#140b09');
    ctx.fillStyle = body;
    ctx.beginPath(); ctx.roundRect(cx - bw, top, bw * 2, h * (1 - sq * 0.1), bw * 0.35); ctx.fill();
    // straps / bands
    ctx.fillStyle = 'rgba(0,0,0,0.45)';
    ctx.fillRect(cx - bw, top + h * 0.12, bw * 2, h * 0.04);
    ctx.fillRect(cx - bw, top + h * 0.82, bw * 2, h * 0.04);
    // shadow sigil on bag
    if (a > 0.05) {
      ctx.globalAlpha = a * 0.8;
      ctx.strokeStyle = `rgba(255,50,40,${0.5 + 0.5 * this.hitFlash})`; ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(cx, top + h * 0.3); ctx.lineTo(cx - bw * 0.45, top + h * 0.55); ctx.lineTo(cx, top + h * 0.72); ctx.lineTo(cx + bw * 0.45, top + h * 0.55); ctx.closePath();
      ctx.stroke(); ctx.globalAlpha = 1;
    }
    // cracks (damage)
    ctx.strokeStyle = 'rgba(255,120,60,0.85)'; ctx.lineWidth = 1.5;
    for (const c of this.cracks) {
      let x = cx + c.u * bw * 0.8, y = top + c.v * h, s = c.seed;
      ctx.beginPath(); ctx.moveTo(x, y);
      for (let i = 0; i < 4; i++) { s = (s * 9301 + 49297) % 233280; const r = s / 233280; x += (r - 0.5) * bw * 0.6 * c.size; y += (r - 0.3) * h * 0.08; ctx.lineTo(x, y); }
      ctx.stroke();
    }
    // hit flash
    if (this.hitFlash > 0.02) {
      ctx.globalCompositeOperation = 'lighter';
      ctx.fillStyle = `rgba(255,${120 + 100 * this.hitFlash | 0},80,${0.55 * this.hitFlash})`;
      ctx.beginPath(); ctx.roundRect(cx - bw, top, bw * 2, h, bw * 0.35); ctx.fill();
      ctx.globalCompositeOperation = 'source-over';
    }
    ctx.restore();

    // floor shadow of the bag
    const [fx, fy, fs] = this.proj(0, 0, 0);
    ctx.fillStyle = 'rgba(0,0,0,0.5)';
    ctx.beginPath(); ctx.ellipse(fx + this.swing * 60, fy, BAG_R * fs * 1.6, BAG_R * fs * 0.35, 0, 0, Math.PI * 2); ctx.fill();
  }

  drawHud(v, t) {
    const { ctx, W, H } = this;
    const S = ArenaState;
    ctx.textAlign = 'center';
    if (v.state === S.Fight) {
      // HP bar
      const bw = Math.min(W * 0.7, 520), bx = (W - bw) / 2, by = 14;
      ctx.fillStyle = 'rgba(0,0,0,0.6)'; ctx.fillRect(bx - 2, by - 2, bw + 4, 18);
      const hp = Math.max(0, this.hpShown);
      const grd = ctx.createLinearGradient(bx, 0, bx + bw, 0);
      grd.addColorStop(0, '#ff2a1a'); grd.addColorStop(1, '#ff9a2a');
      ctx.fillStyle = grd; ctx.fillRect(bx, by, bw * hp, 14);
      ctx.fillStyle = '#fff'; ctx.font = '700 11px system-ui, sans-serif';
      ctx.fillText(`SHADOW${v.kos ? ` x${v.kos + 1}` : ''}  ${Math.ceil(v.hp)} HP`, W / 2, by + 11);
      // timer
      const rem = Math.max(0, v.fightRemaining ?? 0);
      ctx.font = `800 ${Math.round(Math.min(W, 800) * 0.06)}px system-ui, sans-serif`;
      ctx.fillStyle = rem < 10 ? '#ff5040' : 'rgba(255,255,255,0.92)';
      const mm = Math.floor(rem / 60), ss = Math.floor(rem % 60);
      ctx.fillText(`${mm}:${String(ss).padStart(2, '0')}`, W / 2, by + 18 + Math.min(W, 800) * 0.06);
      if (v.combo > 1) {
        ctx.font = `800 ${Math.round(Math.min(20, W / 22))}px system-ui, sans-serif`; ctx.fillStyle = '#ffd040';
        ctx.fillText(`${v.combo} HIT COMBO  x${v.comboMult.toFixed(1)}`, W / 2, H - 22);
      } else if (v.inactiveFor > v.inactivityTimeout * 0.5) {
        ctx.font = `700 ${Math.round(Math.min(16, W / 28))}px system-ui, sans-serif`; ctx.fillStyle = '#ffb070';
        ctx.fillText(`Hit it! Fight ends in ${Math.max(0, v.inactivityTimeout - v.inactiveFor).toFixed(0)} s of no punches`, W / 2, H - 22);
      }
    } else {
      let line = '', sub = '';
      if (v.state === S.Suspended) { line = 'ARENA SUSPENDED'; sub = v.present ? 'presence detected...' : 'nobody here (toggle presence)'; }
      else if (v.state === S.Idle) { line = 'STEP INTO THE ARENA'; sub = v.present ? 'presence already on: brain-rs waits for a new presence event' : 'toggle "Someone in arena" or make noise'; }
      else if (v.state === S.Warming) { line = 'SHOUT TO WAKE THE SHADOW'; sub = `energy ${Math.round((v.energyPct || 0) * 100)}%`; }
      else if (v.state === S.Cooldown) { line = 'THE SHADOW RETREATS'; sub = `cooling down: ${Math.ceil(v.cooldownLeft ?? 0)} s`; }
      const pulse = 0.75 + 0.25 * Math.sin(t * 3);
      ctx.font = `800 ${Math.round(Math.min(W, 900) * 0.045)}px system-ui, sans-serif`;
      ctx.fillStyle = `rgba(255,255,255,${pulse})`;
      ctx.fillText(line, W / 2, 40);
      ctx.font = '500 14px system-ui, sans-serif'; ctx.fillStyle = 'rgba(220,220,235,0.8)';
      ctx.fillText(sub, W / 2, 62);
      if (v.state === S.Warming) {
        const bw = Math.min(W * 0.6, 420), bx = (W - bw) / 2, by = 72;
        ctx.fillStyle = 'rgba(0,0,0,0.55)'; ctx.fillRect(bx - 2, by - 2, bw + 4, 12);
        const grd = ctx.createLinearGradient(bx, 0, bx + bw, 0);
        grd.addColorStop(0, '#3050ff'); grd.addColorStop(0.6, '#c030ff'); grd.addColorStop(1, '#ff4020');
        ctx.fillStyle = grd; ctx.fillRect(bx, by, bw * Math.min(1, v.energyPct || 0), 8);
      }
    }
    if (this.banner) {
      const age = performance.now() / 1000 - this.banner.t0;
      if (age > this.banner.dur) this.banner = null;
      else {
        const a = Math.min(1, age * 5, (this.banner.dur - age) * 3);
        const sc = 1 + 0.15 * Math.max(0, 1 - age * 4);
        ctx.save(); ctx.globalAlpha = a; ctx.translate(W / 2, H * 0.45); ctx.scale(sc, sc);
        ctx.font = `900 ${Math.round(Math.min(W, 900) * 0.08)}px system-ui, sans-serif`;
        ctx.lineWidth = 6; ctx.strokeStyle = 'rgba(0,0,0,0.85)'; ctx.fillStyle = '#ffdf6a';
        ctx.strokeText(this.banner.text, 0, 0); ctx.fillText(this.banner.text, 0, 0);
        if (this.banner.sub) { ctx.font = `700 ${Math.round(Math.min(16, W / 27))}px system-ui, sans-serif`; ctx.fillStyle = '#fff'; ctx.strokeText(this.banner.sub, 0, 30); ctx.fillText(this.banner.sub, 0, 30); }
        ctx.restore();
      }
    }
  }

  drawMinimap(v) {
    const { ctx, H } = this;
    const R = Math.min(52, this.W * 0.12), cx = R + 10, cy = H - R - 10;
    ctx.fillStyle = 'rgba(0,0,0,0.55)'; ctx.strokeStyle = 'rgba(255,255,255,0.15)';
    ctx.beginPath(); ctx.arc(cx, cy, R, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
    const sc = R / 3.0;
    // top-down: +z (behind bag) is up on the map
    COL_ANGLES.forEach((a, c) => {
      const [r, g, b] = this.rig.average(c);
      const m = Math.max(r, g, b, 0.001), k = Math.min(1, 0.35 + m * 2.5) / m;
      const x = cx + ARC_R * Math.sin(a) * sc, y = cy - ARC_R * Math.cos(a) * sc;
      ctx.fillStyle = `rgb(${Math.min(255, r * k * 255) | 0},${Math.min(255, g * k * 255) | 0},${Math.min(255, b * k * 255) | 0})`;
      ctx.beginPath(); ctx.arc(x, y, 4.5, 0, Math.PI * 2); ctx.fill();
    });
    ctx.fillStyle = '#8a4a30'; ctx.beginPath(); ctx.arc(cx + this.swing * 6, cy, 4, 0, Math.PI * 2); ctx.fill();
    ctx.fillStyle = v.present ? '#7cf2a0' : 'rgba(255,255,255,0.25)';
    ctx.beginPath(); ctx.arc(cx, cy - VIEW_Z * sc * 0.6, 3.5, 0, Math.PI * 2); ctx.fill();
    ctx.fillStyle = 'rgba(255,255,255,0.5)'; ctx.font = '9px system-ui, sans-serif'; ctx.textAlign = 'center';
    ctx.fillText('top view', cx, cy + R - 4);
  }
}
