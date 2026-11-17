// "Connect to device" (static / GitHub Pages build only): an https page may not fetch or
// open a WebSocket to a plain-http device on the LAN (mixed content, Private Network
// Access), so this navigates the tab to the simulator the device's bridge serves itself.
// ?device=<host[:port]> pre-fills it (deep link); the last address is remembered.
const KEY = 'sw-sim-device-v1';
const DEFAULT_ADDR = 'shadow-warrior.local', DEFAULT_PORT = 8090;
const $ = (id) => document.getElementById(id);

/** 'pi.local' / 'pi.local:9000' / 'http://pi.local:9000/x' + port -> 'http://pi.local:9000/' */
export function deviceUrl(addr, port) {
  let a = String(addr || '').trim().replace(/^[a-z]+:\/\//i, '').replace(/\/.*$/, '') || DEFAULT_ADDR;
  if (!/:\d+$/.test(a) || a.startsWith('[')) a = `${a}:${Number(port) || DEFAULT_PORT}`;
  return `http://${a}/`;
}

export function initConnect({ log } = {}) {
  const btn = $('connectBtn'), panel = $('connectPanel'), addr = $('connectAddr'), port = $('connectPort'), url = $('connectUrl');
  let saved = {};
  try { saved = JSON.parse(localStorage.getItem(KEY) || '{}'); } catch { /* private mode */ }
  const qs = new URLSearchParams(location.search).get('device');
  addr.value = qs || saved.addr || DEFAULT_ADDR;
  port.value = saved.port || DEFAULT_PORT;
  const show = () => { url.textContent = deviceUrl(addr.value, port.value); };
  addr.oninput = show; port.oninput = show; show();
  btn.hidden = false;
  btn.onclick = () => { panel.hidden = !panel.hidden; if (!panel.hidden) { panel.scrollIntoView?.({ block: 'start', behavior: 'smooth' }); addr.focus({ preventScroll: true }); } };
  $('connectClose').onclick = () => { panel.hidden = true; };
  if (qs) { panel.hidden = false; panel.scrollIntoView?.({ block: 'start' }); }
  $('connectForm').onsubmit = (e) => {
    e.preventDefault();
    const target = deviceUrl(addr.value, port.value);
    try { localStorage.setItem(KEY, JSON.stringify({ addr: addr.value.trim(), port: Number(port.value) || DEFAULT_PORT })); } catch { /* ignore */ }
    log?.(`Opening ${target}`, 'state');
    location.assign(target);
  };
}
