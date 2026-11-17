#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = ["aiohttp>=3.9", "bleak>=1.1.0"]
# ///
"""Live-hardware bridge for the browser simulator.

    uv run simulator/live/bridge.py                  # http://localhost:8090
    uv run simulator/live/bridge.py --wled 192.168.1.203 --port 8090 --no-ble

Serves the repo root over HTTP (so the fight music in music/ works too) and opens
http://localhost:<port>/ on the simulator in live mode. The page streams every rendered
LED frame over a WebSocket (/live/ws, binary: <count> x RGB bytes, already laid out for the
strip); the bridge applies brightness and gamma and sends it to WLED as UDP realtime DNRGB
(port 21324) with a 2 s timeout, so WLED returns to its own effect when frames stop.

The bridge also connects to the punching bag over BLE (bleak), keeps its fight mode on
while a page is connected, folds each burst of acceleration notifications into one punch
(as brain-rs ble_manager.rs: power = min(100, 10 x m/s^2), 350 ms refractory) and pushes
it to every page as {"type": "punch", "power": ...}. Without Bluetooth the rest still works.
POST /live/punch?power=60 sends the same message without the bag (tests, phone);
GET/POST /live/threshold[?value=10] reads/sets the bag's punch threshold (RAM, m/s^2).

Stop the brain first: BLE allows one central and two writers fight over the strip.
"""
import argparse
import asyncio
import json
import logging
import signal
import socket
import struct
import time
import urllib.request
from pathlib import Path

from aiohttp import WSMsgType, web

REPO = Path(__file__).resolve().parents[2]
SIM_PATH = "/simulator/web/index.html?live=1"

BAG_NAME = "ShadowWarriorPunchingBag"
ACCEL_UUID = "6e400002-b5a3-f393-e0a9-e50e24dcca9e"
ALPHA_UUID = "6e400004-b5a3-f393-e0a9-e50e24dcca9e"
THRESHOLD_UUID = "6e400005-b5a3-f393-e0a9-e50e24dcca9e"
FIGHT_MODE_UUID = "6e400006-b5a3-f393-e0a9-e50e24dcca9e"

DNRGB_MAX = 489
log = logging.getLogger("bridge")


class Wled:
    """DNRGB realtime sender. Never polls the controller while streaming."""

    def __init__(self, host, brightness, gamma, timeout_s=2):
        self.host = host
        self.addr = (socket.gethostbyname(host), 21324)
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.sock.setblocking(False)
        self.timeout_s = timeout_s
        self.lut = bytes(round(255 * brightness * (i / 255) ** gamma) for i in range(256))
        self.count = None
        self.frames = 0
        self.last_frame_at = 0.0
        self.fps = 0.0
        self._fps_t0 = time.monotonic()
        self._fps_n = 0

    def probe(self):
        """One GET at startup for the LED count (the C3 dislikes frequent polling)."""
        try:
            with urllib.request.urlopen(f"http://{self.host}/json/info", timeout=3) as r:
                info = json.load(r)
            self.count = int(info["leds"]["count"])
            log.info("WLED %s %s, %d LEDs", self.host, info.get("ver"), self.count)
        except Exception as e:  # noqa: BLE001
            log.warning("WLED info unavailable (%s); assuming 180 LEDs", e)
            self.count = 180

    def send(self, rgb: bytes):
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
        n = self.count or 180
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


class Hub:
    def __init__(self, wled, use_ble):
        self.wled = wled
        self.clients = set()
        self.owner = None  # the newest page drives the strip; frames from older tabs are dropped
        self.bag = Bag(self, *use_ble) if use_ble else None

    def status(self):
        return {"type": "status", "wled": self.wled.status(),
                "bag": self.bag.status() if self.bag else {"state": "disabled", "detail": "--no-ble", "fight": False, "punches": 0},
                "clients": len(self.clients)}

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
                    if ws is self.owner:
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
        raise web.HTTPFound(SIM_PATH)

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

    async def no_cache(request, handler):
        resp = await handler(request)
        resp.headers["Cache-Control"] = "no-store"
        return resp

    app.middlewares.append(web.middleware(no_cache))
    app.router.add_get("/", root)
    app.router.add_get("/live/ws", hub.ws)
    app.router.add_get("/live/status", status)
    app.router.add_post("/live/punch", test_punch)
    app.router.add_route("*", "/live/threshold", threshold)
    app.router.add_static("/", REPO, show_index=False, follow_symlinks=False)
    return app


async def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--wled", default="192.168.1.203", help="WLED host or IP (default %(default)s)")
    ap.add_argument("--port", type=int, default=8090)
    ap.add_argument("--host", default="127.0.0.1", help="bind address (0.0.0.0 to open it from a phone)")
    ap.add_argument("--brightness", type=float, default=0.7, help="0..1 master scale (WLED ABL still applies)")
    ap.add_argument("--gamma", type=float, default=2.2, help="screen sRGB -> LED duty (1 = raw)")
    ap.add_argument("--no-ble", action="store_true", help="do not connect to the punching bag")
    ap.add_argument("--threshold", type=float, default=2.0, help="bag punch threshold, m/s^2 (brain-rs config.yaml)")
    ap.add_argument("--alpha", type=float, default=0.8, help="bag EMA alpha (brain-rs config.yaml)")
    a = ap.parse_args()
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(message)s", datefmt="%H:%M:%S")

    wled = Wled(a.wled, a.brightness, a.gamma)
    wled.probe()
    hub = Hub(wled, None if a.no_ble else (a.threshold, a.alpha))
    runner = web.AppRunner(make_app(hub), access_log=None)
    await runner.setup()
    await web.TCPSite(runner, a.host, a.port).start()
    log.info("simulator: http://localhost:%d/  (WLED %s, BLE %s)", a.port, a.wled, "off" if a.no_ble else "on")

    tasks = [asyncio.create_task(hub.status_loop())]
    if hub.bag:
        tasks.append(asyncio.create_task(hub.bag.run()))
    stop = asyncio.Event()
    loop = asyncio.get_running_loop()
    for sig in (signal.SIGINT, signal.SIGTERM):
        loop.add_signal_handler(sig, stop.set)
    await stop.wait()
    log.info("stopping")
    if hub.bag:
        await hub.bag.shutdown()
    for t in tasks:
        t.cancel()
    wled.release()
    await runner.cleanup()


if __name__ == "__main__":
    asyncio.run(main())
