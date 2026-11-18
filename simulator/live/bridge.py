#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = ["aiohttp>=3.9", "bleak>=1.1.0", "pyyaml>=6"]
# ///
"""Live-hardware bridge for the browser simulator.

    uv run simulator/live/bridge.py --wled 192.168.1.50                 # sim mode
    uv run simulator/live/bridge.py --wled 192.168.1.50 --brain http://127.0.0.1:8000   # brain mode

Serves the repo root over HTTP (so the fight music in music/ works too); / opens the simulator.

Sim mode (default): the page runs the game in JavaScript and streams every rendered LED frame
over a WebSocket (/live/ws, binary: <count> x RGB bytes, already laid out for the strip); the
bridge applies brightness and gamma and sends it to WLED as UDP realtime DNRGB (port 21324)
with a 2 s timeout, so WLED returns to its own effect when frames stop. The bridge also
connects to the punching bag over BLE (bleak), keeps its fight mode on while a page is
connected, folds each burst of acceleration notifications into one punch (as brain-rs
ble_manager.rs: power = min(100, 10 x m/s^2), 350 ms refractory) and pushes it to every page
as {"type": "punch", "power": ...}. POST /live/punch?power=60 sends the same message without
the bag; GET/POST /live/threshold[?value=10] reads/sets the bag's punch threshold (RAM, m/s^2).

Brain mode (--brain URL): the real brain-rs runs the game and the page shows and drives it.
The bridge stands in for WLED: point the brain's wled.controllers at <bridge host>:<port>.
It answers /json/info|state|effects|palettes (proxied to the real WLED), receives the brain's
UDP realtime on 21324, forwards every packet unchanged to the real strip and shows the frames
in the page; outside realtime the page shows WLED's own live view. /brain/* is proxied to the
brain API (same origin, so no CORS). The bag belongs to the brain (no BLE here).

Layout (--layout FILE|JSON, brain mode): the brain-rs `wled:` shape (controllers with ip /
num_leds / strips[{column, leds, reverse}], columns); a brain config.yaml works as is. One UDP
listener per controller (its ip, port 21324, or its `bind`), each controller's pixel array is cut
into strips and written into columns; the page gets four columns bottom-first, serpentine off.
See simulator/web/README.md. Without --layout one controller is split into 4 equal columns.

Stop the brain (sim mode) first: BLE allows one central and two writers fight over the strip.
"""
import argparse
import asyncio
import collections
import json
import logging
import os
import signal
import socket
import struct
import sys
import time
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from layout import Layout, LayoutError, apply_packet  # noqa: E402

import aiohttp
from aiohttp import WSMsgType, web

REPO = Path(__file__).resolve().parents[2]
SIM_PATH = "/simulator/web/index.html?live=1"
BRAIN_PATH = "/simulator/web/index.html?brain=1"
WLED_UDP_PORT = 21324

BAG_NAME = "ShadowWarriorPunchingBag"
ACCEL_UUID = "6e400002-b5a3-f393-e0a9-e50e24dcca9e"
ALPHA_UUID = "6e400004-b5a3-f393-e0a9-e50e24dcca9e"
THRESHOLD_UUID = "6e400005-b5a3-f393-e0a9-e50e24dcca9e"
FIGHT_MODE_UUID = "6e400006-b5a3-f393-e0a9-e50e24dcca9e"

DNRGB_MAX = 489
log = logging.getLogger("bridge")


class Wled:
    """DNRGB realtime sender. Never polls the controller while streaming."""

    def __init__(self, host, brightness, gamma, timeout_s=2, leds=180):
        self.host = host  # None: no strip, frames are dropped
        self.addr = (socket.gethostbyname(host), WLED_UDP_PORT) if host else None
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.sock.setblocking(False)
        self.timeout_s = timeout_s
        self.lut = bytes(round(255 * brightness * (i / 255) ** gamma) for i in range(256))
        self.count = None
        self.default_count = leds
        self.frames = 0
        self.last_frame_at = 0.0
        self.fps = 0.0
        self._fps_t0 = time.monotonic()
        self._fps_n = 0

    def probe(self):
        """One GET at startup for the LED count (the C3 dislikes frequent polling)."""
        if not self.host:
            self.count = self.default_count
            log.warning("no --wled: strip output off, %d LEDs assumed", self.count)
            return
        try:
            with urllib.request.urlopen(f"http://{self.host}/json/info", timeout=3) as r:
                info = json.load(r)
            self.count = int(info["leds"]["count"])
            log.info("WLED %s %s, %d LEDs", self.host, info.get("ver"), self.count)
        except Exception as e:  # noqa: BLE001
            log.warning("WLED info unavailable (%s); assuming %d LEDs", e, self.default_count)
            self.count = self.default_count

    def forward(self, packet: bytes):
        """Pass a UDP realtime packet on unchanged (brain mode)."""
        if self.addr:
            try:
                self.sock.sendto(packet, self.addr)
            except (BlockingIOError, OSError) as e:
                log.debug("udp forward: %s", e)

    def send(self, rgb: bytes):
        if not self.addr:
            return
        px = rgb.translate(self.lut)
        n = len(px) // 3
        for start in range(0, n, DNRGB_MAX):
            chunk = px[start * 3:(start + DNRGB_MAX) * 3]
            hdr = bytes([4, self.timeout_s, start >> 8, start & 0xFF])
            try:
                self.sock.sendto(hdr + chunk, self.addr)
            except (BlockingIOError, OSError) as e:
                log.debug("udp send: %s", e)
        self.frames += 1
        self._fps_n += 1
        now = time.monotonic()
        self.last_frame_at = now
        if now - self._fps_t0 >= 1.0:
            self.fps = self._fps_n / (now - self._fps_t0)
            self._fps_t0, self._fps_n = now, 0

    def release(self):
        """Timeout byte 1 + black: WLED drops realtime after 1 s and resumes its effect."""
        if not self.addr:
            return
        n = self.count or self.default_count
        for start in range(0, n, DNRGB_MAX):
            m = min(DNRGB_MAX, n - start)
            try:
                self.sock.sendto(bytes([4, 1, start >> 8, start & 0xFF]) + bytes(m * 3), self.addr)
            except OSError:
                pass

    def status(self):
        streaming = time.monotonic() - self.last_frame_at < 1.0
        return {"host": self.host, "leds": self.count, "streaming": streaming,
                "fps": round(self.fps, 1) if streaming else 0, "frames": self.frames}


class Bag:
    """BLE link to the punching bag with reconnect, fight-mode control and punch folding."""

    def __init__(self, hub, threshold, alpha, refractory_ms=350):
        self.hub = hub
        self.cfg_threshold, self.cfg_alpha = threshold, alpha
        self.last_accel = None
        self.state = "starting"
        self.detail = ""
        self.client = None
        self.want_fight = False
        self.fight_on = None
        self.refractory = refractory_ms / 1000
        self.last_punch = 0.0
        self.punches = 0
        self.samples = 0
        self._wake = asyncio.Event()

    def status(self):
        return {"state": self.state, "detail": self.detail, "fight": bool(self.fight_on), "punches": self.punches,
                "samples": self.samples, "last_accel": self.last_accel}

    def set_fight(self, on):
        if on != self.want_fight:
            self.want_fight = on
            self._wake.set()

    def _set(self, state, detail=""):
        if (state, detail) != (self.state, self.detail):
            self.state, self.detail = state, detail
            log.info("bag: %s %s", state, detail)
            self.hub.broadcast_status()

    def _on_accel(self, _char, data: bytearray):
        self.samples += 1
        if len(data) == 4:
            mag = struct.unpack("<f", data)[0]
        elif len(data) >= 12:
            x, y, z = struct.unpack("<fff", data[:12])
            mag = (x * x + y * y + z * z) ** 0.5
        else:
            return
        self.last_accel = round(mag, 2)
        now = time.monotonic()
        if mag > 1.0 and now - self.last_punch >= self.refractory:
            self.last_punch = now
            self.punches += 1
            power = min(100.0, mag * 10.0)
            log.info("punch %.0f (%.2f m/s^2)", power, mag)
            self.hub.broadcast({"type": "punch", "power": round(power, 1), "accel": round(mag, 2), "n": self.punches})

    async def run(self):
        try:
            from bleak import BleakClient, BleakScanner
        except Exception as e:  # noqa: BLE001
            self._set("unavailable", f"bleak: {e}")
            return
        backoff = 2
        while True:
            try:
                self._set("scanning")
                dev = await BleakScanner.find_device_by_name(BAG_NAME, timeout=10)
                if dev is None:
                    self._set("not found", "bag off or out of range")
                    await asyncio.sleep(backoff)
                    backoff = min(backoff * 2, 15)
                    continue
                self._set("connecting", dev.address)
                gone = asyncio.Event()
                async with BleakClient(dev, disconnected_callback=lambda _c: gone.set(), timeout=15) as client:
                    self.client = client
                    self.fight_on = None
                    # As brain-rs ble_manager.rs on connect: bags.alpha / bags.threshold from config.yaml.
                    await client.write_gatt_char(ALPHA_UUID, struct.pack("<f", self.cfg_alpha), response=True)
                    await client.write_gatt_char(THRESHOLD_UUID, struct.pack("<f", self.cfg_threshold), response=True)
                    await client.start_notify(ACCEL_UUID, self._on_accel)
                    self._set("connected", dev.address)
                    backoff = 2
                    while not gone.is_set():
                        if self.fight_on != self.want_fight:
                            want = self.want_fight
                            await client.write_gatt_char(FIGHT_MODE_UUID, bytes([1 if want else 0]), response=True)
                            await asyncio.sleep(0.4)  # the bag applies it in loop() and powers the IMU
                            self.fight_on = bool((await client.read_gatt_char(FIGHT_MODE_UUID))[0])
                            log.info("bag fight mode %s%s", "ON" if self.fight_on else "OFF",
                                     "" if self.fight_on == want else " (bag refused: IMU init failed?)")
                            if self.fight_on != want:
                                self.fight_on = want  # don't hammer it; retried on the next change
                            self.hub.broadcast_status()
                        self._wake.clear()
                        try:
                            await asyncio.wait_for(self._wake.wait(), timeout=1.0)
                        except asyncio.TimeoutError:
                            pass
                self.client = None
                self.fight_on = None
                self._set("disconnected", "reconnecting")
            except asyncio.CancelledError:
                raise
            except Exception as e:  # noqa: BLE001
                self.client = None
                self.fight_on = None
                self._set("error", str(e)[:120] or type(e).__name__)
                await asyncio.sleep(backoff)
                backoff = min(backoff * 2, 15)

    async def threshold(self, value=None):
        """Read, or write then read, the firmware punch threshold (m/s^2, RAM only)."""
        c = self.client
        if c is None or not c.is_connected:
            raise RuntimeError(f"bag {self.state}")
        if value is not None:
            await c.write_gatt_char(THRESHOLD_UUID, struct.pack("<f", value), response=True)
            log.info("bag threshold -> %.2f", value)
        return struct.unpack("<f", bytes(await c.read_gatt_char(THRESHOLD_UUID)))[0]

    async def shutdown(self):
        c = self.client
        if c is not None and c.is_connected:
            try:
                await asyncio.wait_for(c.write_gatt_char(FIGHT_MODE_UUID, b"\x00", response=True), 3)
                log.info("bag fight mode OFF (shutdown)")
            except Exception as e:  # noqa: BLE001
                log.warning("fight mode off failed: %s", e)


FRAME_BRAIN, FRAME_WLED = 1, 2  # page frame tags: brain UDP realtime, WLED's own live view


class BrainLink:
    """Brain mode: a stand-in WLED for brain-rs plus a same-origin proxy to its API.

    The brain's realtime UDP packets are forwarded unchanged to the real strip and decoded
    into one frame for the page; its /json calls are proxied to the real WLED (or answered
    here when there is none). Outside realtime the page gets WLED's own live view (/ws lv).
    """

    def __init__(self, hub, url, wled, layout=None):
        self.hub, self.url, self.wled = hub, url.rstrip("/"), wled
        self.layout = layout
        self.session = None
        n = layout.count if layout else (wled.count or wled.default_count)
        self.frame = bytearray(n * 3)
        self.forwarders = {}  # layout: controller ip -> Wled (UDP forward only)
        if layout:
            for c in layout.controllers:
                self.forwarders[c.ip] = Wled(c.forward, 1.0, 1.0)
        self.seq = 0
        self.recent = collections.deque(maxlen=120)  # (seq, tag, rgb) sent to pages, for tests
        self.udp_packets = 0
        self.udp_from = None
        self.last_udp = 0.0
        self.rt_timeout = 2.0
        self.udp_fps = 0.0
        self._fps_t0, self._fps_n = time.monotonic(), 0
        self.live_frames = 0
        self.live_state = "off" if not wled.host else "idle"
        self.last_post = None  # last /json/state body the brain posted
        self.effects = []
        self.reachable = None
        self.brain_state = None
        self._last_push = 0.0

    # ---- UDP realtime from the brain
    def realtime(self):
        if self.layout:
            now = time.monotonic()
            return any(now - c.last_udp < c.rt_timeout for c in self.layout.controllers)
        return time.monotonic() - self.last_udp < self.rt_timeout

    def on_udp(self, data: bytes, addr, ctrl=None):
        if self.layout:
            return self.on_udp_layout(data, addr, ctrl)
        if len(data) < 2:
            return
        self.wled.forward(data)
        res = apply_packet(self.frame, data)
        if res is None:
            return
        t, complete = res
        self.rt_timeout = 3600.0 if t == 255 else max(t, 1)
        self.udp_packets += 1
        self.udp_from = addr[0]
        now = time.monotonic()
        self.last_udp = now
        # A frame is complete when a packet reaches the strip end (DNRGB chunks) or for DRGB/WARLS.
        if complete:
            self._count_frame(now)
            self.push(FRAME_BRAIN, self.frame)

    def _count_frame(self, now):
        self._fps_n += 1
        if now - self._fps_t0 >= 1.0:
            self.udp_fps = self._fps_n / (now - self._fps_t0)
            self._fps_t0, self._fps_n = now, 0

    def on_udp_layout(self, data: bytes, addr, ctrl):
        """One controller's realtime packet: update its pixel array, cut it into its strips,
        push the composed four-column frame (columns without data stay dark)."""
        fwd = self.forwarders[ctrl.ip]
        fwd.forward(data)
        res = apply_packet(ctrl.pixels, data)
        if res is None:
            return
        t, complete = res
        ctrl.rt_timeout = 3600.0 if t == 255 else max(t, 1)
        ctrl.packets += 1
        self.udp_packets += 1
        self.udp_from = addr[0]
        now = time.monotonic()
        ctrl.last_udp = self.last_udp = now
        if complete:
            self.layout.write(ctrl)
            self._count_frame(now)
            self.frame[:] = self.layout.frame(now)
            self.push(FRAME_BRAIN, self.frame)

    def push(self, tag, rgb):
        self.seq = (self.seq + 1) & 0xFFFFFFFF
        self.recent.append((self.seq, tag, bytes(rgb)))
        msg = bytes([tag]) + self.seq.to_bytes(4, "big") + bytes(rgb)
        for ws in list(self.hub.clients):
            if not ws.closed:
                asyncio.ensure_future(ws.send_bytes(msg))

    # ---- WLED live view (what the strip shows during its own effects)
    async def live_view(self):
        if not self.wled.host or self.layout:  # a layout has no single strip to mirror
            return
        backoff = 2
        while True:
            if not self.hub.clients:
                self.live_state = "idle"
                await asyncio.sleep(0.5)
                continue
            try:
                async with self.session.ws_connect(f"http://{self.wled.host}/ws", heartbeat=15, timeout=5) as ws:
                    await ws.send_str('{"lv":true}')
                    self.live_state = "on"
                    backoff = 2
                    async for m in ws:
                        if not self.hub.clients:
                            await ws.send_str('{"lv":false}')
                            break
                        if m.type == WSMsgType.BINARY and len(m.data) > 2 and m.data[0] == 76:  # 'L'
                            self.live_frames += 1
                            if not self.realtime():
                                rgb = m.data[2:2 + len(self.frame)]
                                if len(rgb) == len(self.frame):
                                    self.push(FRAME_WLED, rgb)
                        elif m.type in (WSMsgType.CLOSED, WSMsgType.ERROR):
                            break
            except asyncio.CancelledError:
                raise
            except Exception as e:  # noqa: BLE001
                self.live_state = f"error: {str(e)[:80] or type(e).__name__}"
            await asyncio.sleep(backoff)
            backoff = min(backoff * 2, 15)

    # ---- brain reachability (for the status panel)
    async def poll(self):
        while True:
            try:
                async with self.session.get(f"{self.url}/api/state", timeout=aiohttp.ClientTimeout(total=2)) as r:
                    d = (await r.json()).get("data") or {}
                    self.reachable = True
                    self.brain_state = (d.get("arena") or {}).get("current_state")
            except Exception:  # noqa: BLE001
                self.reachable, self.brain_state = False, None
            await asyncio.sleep(1.0)

    def effect_name(self):
        p = self.last_post or {}
        if p.get("on") is False:
            return "off"
        seg = (p.get("seg") or [{}])[0]
        fx = seg.get("fx")
        if fx is None:
            return None
        return self.effects[fx] if isinstance(fx, int) and fx < len(self.effects) else f"effect {fx}"

    def status(self):
        rt = self.realtime()
        st = {"url": self.url, "reachable": self.reachable, "state": self.brain_state,
                "realtime": rt, "udp_fps": round(self.udp_fps, 1) if rt else 0, "udp_packets": self.udp_packets,
                "udp_from": self.udp_from, "effect": self.effect_name(), "live_view": self.live_state,
                "live_frames": self.live_frames, "seq": self.seq, "posted": self.last_post}
        if self.layout:
            st["controllers"] = [{"ip": c.ip, "bind": f"{c.bind[0]}:{c.bind[1]}", "leds": c.num_leds, "packets": c.packets,
                                  "streaming": time.monotonic() - c.last_udp < c.rt_timeout,
                                  "strips": [[s.column, s.leds, s.reverse] for s in c.strips],
                                  "forward": c.forward} for c in self.layout.controllers]
            st["columns"] = self.layout.columns
        return st

    # ---- stand-in WLED JSON API (brain -> bridge -> real WLED)
    async def wled_json(self, req):
        path = req.match_info.get("path", "")
        body = await req.read() if req.method == "POST" else None
        if req.method == "POST" and path in ("state", "si", ""):
            try:
                self.last_post = json.loads(body or b"{}")
            except ValueError:
                pass
        if self.wled.host:
            try:
                async with self.session.request(req.method, f"http://{self.wled.host}/json/{path}", data=body,
                                                headers={"Content-Type": "application/json"} if body else None,
                                                timeout=aiohttp.ClientTimeout(total=4)) as r:
                    data = await r.read()
                    if path == "effects" and r.status == 200:
                        try:
                            self.effects = json.loads(data)
                        except ValueError:
                            pass
                    return web.Response(body=data, status=r.status, content_type="application/json")
            except Exception as e:  # noqa: BLE001
                return web.json_response({"error": f"WLED {self.wled.host}: {e}"}, status=502)
        # No real strip: answer like a minimal WLED.
        n = len(self.frame) // 3
        ctrl = None
        if self.layout:  # /json/info answers per controller (by the Host the brain used), else total LEDs
            ctrl = self.layout.controller_for_host(req.host.rpartition(":")[0] if ":" in req.host else req.host)
            n = ctrl.num_leds if ctrl else sum(c.num_leds for c in self.layout.controllers)
        if path == "info":
            rt = (time.monotonic() - ctrl.last_udp < ctrl.rt_timeout) if ctrl else self.realtime()
            return web.json_response({"ver": "sim-bridge", "name": "simulator bridge", "leds": {"count": n},
                                      "live": rt, "lm": "UDP" if rt else ""})
        if path in ("effects", "palettes", "eff", "pal"):
            return web.json_response([])
        if req.method == "POST":
            return web.json_response({"success": True})
        return web.json_response(self.last_post or {"on": True})

    async def proxy(self, req):
        """/brain/<path> -> <brain>/<path>, streamed (works for the SSE /api/events)."""
        url = f"{self.url}/{req.match_info['tail']}"
        body = await req.read()
        headers = {"Content-Type": req.headers.get("Content-Type", "application/json")} if body else None
        try:
            r = await self.session.request(req.method, url, params=req.query, data=body or None, headers=headers,
                                           timeout=aiohttp.ClientTimeout(total=None, sock_connect=3))
        except Exception as e:  # noqa: BLE001
            return web.json_response({"success": False, "error": f"brain {self.url} unreachable: {e}"}, status=502)
        resp = web.StreamResponse(status=r.status, headers={"Content-Type": r.headers.get("Content-Type", "application/octet-stream"),
                                                            "Cache-Control": "no-store"})
        try:
            await resp.prepare(req)
            async for chunk in r.content.iter_any():
                await resp.write(chunk)
        except (ConnectionResetError, asyncio.CancelledError):
            pass
        finally:
            r.release()
        return resp


class UdpIn(asyncio.DatagramProtocol):
    def __init__(self, brain, ctrl=None):
        self.brain, self.ctrl = brain, ctrl

    def datagram_received(self, data, addr):
        self.brain.on_udp(data, addr, self.ctrl)


class Hub:
    def __init__(self, wled, use_ble, brain_url=None, layout=None):
        self.wled = wled
        self.clients = set()
        self.brain = BrainLink(self, brain_url, wled, layout) if brain_url else None
        self.owner = None  # the newest page drives the strip; frames from older tabs are dropped
        self.bag = Bag(self, *use_ble) if use_ble else None

    def status(self):
        off = {"state": "disabled", "detail": "the brain holds the bag" if self.brain else "--no-ble", "fight": False, "punches": 0}
        st = {"type": "status", "mode": "brain" if self.brain else "sim", "wled": self.wled.status(),
              "bag": self.bag.status() if self.bag else off, "clients": len(self.clients)}
        if self.brain:
            st["brain"] = self.brain.status()
            st["wled"]["streaming"] = self.brain.realtime() and bool(self.wled.host or self.brain.layout)
            if self.brain.layout:
                st["wled"]["leds"] = self.brain.layout.count
            st["wled"]["fps"] = st["brain"]["udp_fps"]
        return st

    def broadcast(self, msg):
        for ws in list(self.clients):
            if not ws.closed:
                m = dict(msg, owner=ws is self.owner) if msg.get("type") == "status" else msg
                asyncio.ensure_future(ws.send_str(json.dumps(m)))

    def broadcast_status(self):
        self.broadcast(self.status())

    def _update_fight(self):
        if self.bag:
            # Fight mode (IMU on) while any page is live, so punches count in every state.
            self.bag.set_fight(len(self.clients) > 0)

    async def ws(self, request):
        ws = web.WebSocketResponse(heartbeat=10, max_msg_size=1 << 20)
        await ws.prepare(request)
        self.clients.add(ws)
        self.owner = ws
        self._update_fight()
        log.info("page connected (%d)", len(self.clients))
        self.broadcast_status()
        try:
            async for msg in ws:
                if msg.type == WSMsgType.BINARY:
                    if ws is self.owner and not self.brain:
                        self.wled.send(msg.data)
                elif msg.type == WSMsgType.TEXT:
                    try:
                        m = json.loads(msg.data)
                    except ValueError:
                        continue
                    if m.get("type") == "take":  # "drive the strip from this tab"
                        self.owner = ws
                        self.broadcast_status()
        finally:
            self.clients.discard(ws)
            if self.owner is ws:
                self.owner = next(iter(self.clients), None)
            self._update_fight()
            self.broadcast_status()
            log.info("page disconnected (%d)", len(self.clients))
        return ws

    async def status_loop(self):
        while True:
            await asyncio.sleep(1.0)
            if self.clients:
                self.broadcast_status()


def make_app(hub):
    app = web.Application()

    async def root(_req):
        raise web.HTTPFound(BRAIN_PATH if hub.brain else SIM_PATH)

    async def status(_req):
        return web.json_response(hub.status())

    async def test_punch(req):
        # Same message a real bag punch produces; for tests and for punching from a phone.
        power = max(1.0, min(100.0, float(req.query.get("power", "60"))))
        hub.broadcast({"type": "punch", "power": power, "accel": round(power / 10, 2), "n": 0, "test": True})
        return web.json_response({"ok": True, "power": power, "clients": len(hub.clients)})

    async def threshold(req):
        # GET reads, POST ?value=10 sets the bag's punch threshold (lost on bag reboot).
        if not hub.bag:
            return web.json_response({"error": "BLE disabled"}, status=409)
        try:
            v = float(req.query["value"]) if req.method == "POST" else None
            return web.json_response({"threshold": round(await hub.bag.threshold(v), 3)})
        except (KeyError, ValueError):
            return web.json_response({"error": "POST needs ?value=<m/s^2>"}, status=400)
        except Exception as e:  # noqa: BLE001
            return web.json_response({"error": str(e)}, status=409)

    seg_file = REPO / "music" / "segments.json"  # gitignored, like the tracks

    async def music_segments(req):
        # GET: the music sections picked on music.html; POST: replace them (JSON object).
        if req.method == "POST":
            try:
                data = await req.json()
                if not isinstance(data, dict):
                    raise ValueError("expected an object")
                clean = {}
                for name, seg in data.items():
                    start = max(0.0, float(seg.get("start", 0)))
                    end = seg.get("end")
                    clean[str(Path(name).name)] = {"start": round(start, 2), "end": None if end is None else round(float(end), 2)}
            except (ValueError, TypeError, AttributeError, json.JSONDecodeError) as e:
                return web.json_response({"error": str(e)}, status=400)
            tmp = seg_file.with_suffix(".tmp")
            tmp.write_text(json.dumps(clean, indent=1, sort_keys=True))
            tmp.replace(seg_file)
            return web.json_response({"saved": len(clean)})
        try:
            return web.json_response(json.loads(seg_file.read_text()))
        except (OSError, ValueError):
            return web.json_response({})

    async def no_cache(request, handler):
        resp = await handler(request)
        if not resp.prepared:
            resp.headers["Cache-Control"] = "no-store"
        return resp

    app.middlewares.append(web.middleware(no_cache))
    app.router.add_get("/", root)
    app.router.add_get("/live/ws", hub.ws)
    app.router.add_get("/live/status", status)
    app.router.add_post("/live/punch", test_punch)
    app.router.add_route("*", "/live/threshold", threshold)
    app.router.add_route("*", "/live/music-segments", music_segments)
    if hub.brain:
        app.router.add_route("*", "/json/{path:.*}", hub.brain.wled_json)
        app.router.add_route("*", "/json", hub.brain.wled_json)
        app.router.add_route("*", "/brain/{tail:.*}", hub.brain.proxy)

        async def last_frame(req):  # tests: a frame sent to the pages (?seq=N, default the newest)
            b = hub.brain
            want = int(req.query.get("seq", b.seq))
            for seq, tag, rgb in reversed(b.recent):
                if seq == want:
                    return web.json_response({"seq": seq, "source": "brain" if tag == FRAME_BRAIN else "wled",
                                              "realtime": b.realtime(), "rgb": rgb.hex()})
            return web.json_response({"error": f"seq {want} not kept"}, status=404)
        app.router.add_get("/live/frame", last_frame)
    app.router.add_static("/", REPO, show_index=False, follow_symlinks=False)
    return app


async def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--wled", default=os.environ.get("WLED_HOST"),
                    help="real WLED host or IP (default $WLED_HOST; none: no strip, page only)")
    ap.add_argument("--brain", metavar="URL", help="brain mode: brain-rs API, e.g. http://127.0.0.1:8000 or http://<pi>:8000")
    ap.add_argument("--udp-port", type=int, default=WLED_UDP_PORT, help="brain mode: realtime UDP listen port (default %(default)s)")
    ap.add_argument("--layout", metavar="FILE|JSON", help="brain mode: column/strip layout, the brain-rs `wled:` shape (YAML/JSON file, "
                    "e.g. ../../brain-rs/config.yaml, or inline JSON); one UDP listener per controller ip")
    ap.add_argument("--leds", type=int, default=180, help="LED count when WLED is not reachable (default %(default)s)")
    ap.add_argument("--port", type=int, default=8090)
    ap.add_argument("--host", default="127.0.0.1", help="bind address (0.0.0.0 to open it from a phone)")
    ap.add_argument("--brightness", type=float, default=0.7, help="0..1 master scale (WLED ABL still applies)")
    ap.add_argument("--gamma", type=float, default=2.2, help="screen sRGB -> LED duty (1 = raw)")
    ap.add_argument("--no-ble", action="store_true", help="do not connect to the punching bag")
    ap.add_argument("--threshold", type=float, default=2.0, help="bag punch threshold, m/s^2 (brain-rs config.yaml)")
    ap.add_argument("--alpha", type=float, default=0.8, help="bag EMA alpha (brain-rs config.yaml)")
    a = ap.parse_args()
    if a.wled in ("", "none"):
        a.wled = None
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(message)s", datefmt="%H:%M:%S")

    layout = None
    if a.layout:
        if not a.brain:
            ap.error("--layout needs --brain")
        try:
            layout = Layout.load(a.layout, a.udp_port)
        except (LayoutError, OSError, KeyError, TypeError, ValueError) as e:
            ap.error(f"--layout: {e!r}")
        if a.wled:
            ap.error("--layout with --wled: give a controller a `forward` host instead")
    wled = Wled(a.wled, a.brightness, a.gamma, leds=a.leds)
    wled.probe()
    brain_mode = bool(a.brain)
    # Brain mode: the brain is the bag's BLE central (one central only).
    hub = Hub(wled, None if (a.no_ble or brain_mode) else (a.threshold, a.alpha), a.brain, layout)
    loop = asyncio.get_running_loop()
    tasks = []
    if hub.brain:
        hub.brain.session = aiohttp.ClientSession()
        if layout:
            for c in layout.controllers:
                try:
                    await loop.create_datagram_endpoint(lambda c=c: UdpIn(hub.brain, c), local_addr=c.bind)
                except OSError as e:
                    sys.exit(f"cannot listen on UDP {c.bind[0]}:{c.bind[1]} for controller {c.ip}: {e}. On macOS add a loopback "
                             f"alias first: sudo ifconfig lo0 alias {c.bind[0]} up")
                log.info("controller %s: UDP %s:%d, strips %s", c.ip, c.bind[0], c.bind[1],
                         [(s.column, s.leds, "rev" if s.reverse else "up") for s in c.strips])
        else:
            await loop.create_datagram_endpoint(lambda: UdpIn(hub.brain), local_addr=(a.host, a.udp_port))
        tasks += [asyncio.create_task(hub.brain.live_view()), asyncio.create_task(hub.brain.poll())]
    runner = web.AppRunner(make_app(hub), access_log=None)
    await runner.setup()
    await web.TCPSite(runner, a.host, a.port, shutdown_timeout=2).start()
    if layout:  # the brain also calls /json on each controller's ip (same HTTP port as the bridge)
        extra = {c.http_host for c in layout.controllers} - {a.host, "0.0.0.0"} if a.host != "0.0.0.0" else set()
        for h in sorted(extra):
            try:
                await web.TCPSite(runner, h, a.port, shutdown_timeout=2).start()
            except OSError as e:
                log.warning("HTTP on %s:%d unavailable (%s): the brain's /json calls to that controller will fail", h, a.port, e)
    if hub.brain:
        log.info("brain mode: brain %s; point its wled.controllers at <this host>:%d (UDP %d on %s); strip %s",
                 a.brain, a.port, a.udp_port, a.host, a.wled or "none")
    log.info("simulator: http://localhost:%d/  (WLED %s, BLE %s)", a.port, a.wled or "none",
             "brain" if brain_mode else "off" if a.no_ble else "on")

    tasks.append(asyncio.create_task(hub.status_loop()))
    if hub.bag:
        tasks.append(asyncio.create_task(hub.bag.run()))
    stop = asyncio.Event()
    for sig in (signal.SIGINT, signal.SIGTERM):
        loop.add_signal_handler(sig, stop.set)
    await stop.wait()
    log.info("stopping")
    if hub.bag:
        await hub.bag.shutdown()
    for t in tasks:
        t.cancel()
    if hub.brain:
        await hub.brain.session.close()
    else:
        wled.release()  # brain mode: the brain owns the strip; WLED times out on its own
    await runner.cleanup()


if __name__ == "__main__":
    asyncio.run(main())
