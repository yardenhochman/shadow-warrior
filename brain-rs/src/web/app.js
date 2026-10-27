// Shadow Warrior Brain - Web UI

let eventSource = null;
let reconnectAttempts = 0;
const MAX_RECONNECT_ATTEMPTS = 10;
let logEventSource = null;
const MAX_LOG_LINES = 500;

// Initial state and UI elements
let currentState = 'LOADING';
let devices = [];
let currentTunables = null;

// Tab management
function showTab(tabId) {
    // Update buttons
    document.querySelectorAll('.tab-btn').forEach(btn => {
        btn.classList.remove('active');
        if (btn.textContent.toLowerCase() === tabId) {
            btn.classList.add('active');
        } else if (tabId === 'arena' && btn.textContent === 'Arena') {
            btn.classList.add('active');
        } else if (tabId === 'hardware' && btn.textContent === 'Hardware') {
            btn.classList.add('active');
        } else if (tabId === 'config' && btn.textContent === 'Config') {
            btn.classList.add('active');
        } else if (tabId === 'logs' && btn.textContent === 'Logs') {
            btn.classList.add('active');
        }
    });

    // Update content
    document.querySelectorAll('.tab-content').forEach(content => {
        content.classList.remove('active');
    });
    document.getElementById(tabId + 'Tab').classList.add('active');

    // Load data if switching to config
    if (tabId === 'config') {
        loadConfig();
    } else if (tabId === 'schedule') {
        loadSchedule();
    } else if (tabId === 'lights') {
        loadWledLists();
    }
}

// Initialize
document.addEventListener('DOMContentLoaded', () => {
    console.log('Shadow Warrior Brain initializing...');
    initializeState();
    connectEventStream();
    connectLogStream();
    startUpdateLoop();
    showTab('arena'); // Default to arena tab
});

// Initialize state from API
async function initializeState() {
    try {
        const response = await fetch('/api/state');
        const data = await response.json();

        if (data.success) {
            updateState(data.data);
        }
    } catch (error) {
        console.error('Failed to initialize state:', error);
    }
}

// Connect to SSE event stream
function connectEventStream() {
    if (eventSource) {
        eventSource.close();
    }

    eventSource = new EventSource('/api/events');

    eventSource.onopen = () => {
        console.log('Event stream connected');
        reconnectAttempts = 0;
        updateConnectionStatus(true);
    };

    eventSource.onerror = (error) => {
        console.error('Event stream error:', error);
        updateConnectionStatus(false);

        eventSource.close();

        if (reconnectAttempts < MAX_RECONNECT_ATTEMPTS) {
            reconnectAttempts++;
            const delay = Math.min(1000 * Math.pow(2, reconnectAttempts), 30000);
            console.log(`Reconnecting in ${delay}ms...`);
            setTimeout(connectEventStream, delay);
        }
    };

    // Listen for specific event types
    eventSource.addEventListener('StateChanged', (event) => {
        const payload = JSON.parse(event.data);
        const data = payload.data;
        console.log('State changed:', data);
        refreshState();
    });

    eventSource.addEventListener('EnergyChanged', (event) => {
        const payload = JSON.parse(event.data);
        const data = payload.data;
        updateEnergy(data.energy, data.max_energy);
    });

    eventSource.addEventListener('PunchDetected', (event) => {
        const payload = JSON.parse(event.data);
        const data = payload.data;
        console.log('Punch detected:', data);
        animatePunch();
    });

    eventSource.addEventListener('AudioLevelChanged', (event) => {
        const payload = JSON.parse(event.data);
        const data = payload.data;
        updateAudioLevel(data.level_db);
    });

    eventSource.addEventListener('MusicStarted', (event) => {
        const payload = JSON.parse(event.data);
        const data = payload.data;
        updateMusicStatus(true, data.track_name);
    });

    eventSource.addEventListener('MusicStopped', () => {
        updateMusicStatus(false, null);
    });
}

// Refresh complete state
async function refreshState() {
    try {
        const response = await fetch('/api/state');
        const data = await response.json();

        if (data.success) {
            updateState(data.data);
        }
    } catch (error) {
        console.error('Failed to refresh state:', error);
    }
}

// Update UI with state data
function updateState(state) {
    updateArenaState(state.arena);
    updateHardwareStatus(state.hardware);
    updateStatistics(state.statistics);
}

// Update arena state display
function updateArenaState(arena) {
    const stateBadge = document.getElementById('stateBadge');
    const stateText = document.getElementById('stateText');
    const stateTimer = document.getElementById('stateTimer');
    const energySection = document.getElementById('energySection');
    const fightSection = document.getElementById('fightSection');

    // Update state badge
    const state = arena.current_state.toLowerCase();
    stateText.textContent = arena.current_state;
    stateBadge.className = 'state-badge ' + state;

    // Update timer
    const minutes = Math.floor(arena.time_in_state_sec / 60);
    const seconds = Math.floor(arena.time_in_state_sec % 60);
    stateTimer.textContent = `${minutes}:${seconds.toString().padStart(2, '0')}`;

    // Show/hide state-specific sections
    energySection.style.display = state === 'warming' ? 'block' : 'none';
    fightSection.style.display = state === 'fight' ? 'block' : 'none';

    // Update energy bar
    if (state === 'warming' && arena.energy !== undefined) {
        updateEnergy(arena.energy, arena.max_energy);
    }

    // Update fight stats
    if (state === 'fight') {
        if (arena.fight_remaining_sec !== undefined) {
            const mins = Math.floor(arena.fight_remaining_sec / 60);
            const secs = Math.floor(arena.fight_remaining_sec % 60);
            document.getElementById('fightTimeRemaining').textContent =
                `${mins}:${secs.toString().padStart(2, '0')}`;
        }

        if (arena.time_since_last_activity_sec !== undefined) {
            document.getElementById('inactivityTime').textContent =
                Math.floor(arena.time_since_last_activity_sec) + 's';
        }
    }
}

// Update energy bar
function updateEnergy(current, max) {
    let percentage = 0;
    if (typeof current === 'number' && typeof max === 'number' && max > 0) {
        percentage = (current / max) * 100;
    }

    if (isNaN(percentage)) {
        percentage = 0;
    }

    document.getElementById('energyFill').style.width = percentage + '%';
    document.getElementById('energyText').textContent = Math.round(percentage) + '%';
}

// Update hardware status
function updateHardwareStatus(hardware) {
    // BLE
    const bleCard = document.getElementById('bleCard');
    const bleStatus = document.getElementById('bleStatus');
    if (hardware.ble.connected) {
        bleCard.classList.add('connected');
        bleStatus.textContent = 'Connected';
        bleStatus.classList.add('connected');
    } else {
        bleCard.classList.remove('connected');
        bleStatus.textContent = 'Disconnected';
        bleStatus.classList.remove('connected');
    }

    // Audio
    const audioCard = document.getElementById('audioCard');
    const audioStatus = document.getElementById('audioStatus');
    if (hardware.audio.connected) {
        audioCard.classList.add('connected');
        audioStatus.textContent = 'Connected';
        audioStatus.classList.add('connected');
    } else {
        audioCard.classList.remove('connected');
        audioStatus.textContent = 'Disconnected';
        audioStatus.classList.remove('connected');
    }

    // WLED
    const wledCard = document.getElementById('wledCard');
    const wledStatus = document.getElementById('wledStatus');
    
    // Check if any WLED controller is connected
    const wledConnected = hardware.wled && hardware.wled.length > 0 && hardware.wled.some(w => w.connected);
    
    if (wledConnected) {
        wledCard.classList.add('connected');
        wledStatus.textContent = `Connected (${hardware.wled.length})`;
        wledStatus.classList.add('connected');
    } else {
        wledCard.classList.remove('connected');
        wledStatus.textContent = 'Disconnected';
        wledStatus.classList.remove('connected');
    }

    // Music
    if (hardware.music.playing) {
        updateMusicStatus(true, hardware.music.current_track);
    } else {
        updateMusicStatus(false, null);
    }

    renderDeviceHealth(hardware.health);
}

// "12s ago", "5m ago", "3h ago" for an ISO timestamp
function relativeTime(iso) {
    if (!iso) return null;
    const sec = Math.max(0, Math.round((Date.now() - new Date(iso).getTime()) / 1000));
    if (sec < 5) return 'just now';
    if (sec < 60) return `${sec}s ago`;
    if (sec < 3600) return `${Math.floor(sec / 60)}m ago`;
    if (sec < 86400) return `${Math.floor(sec / 3600)}h ago`;
    return `${Math.floor(sec / 86400)}d ago`;
}

// Flatten hardware.health into [{label, device}] rows
function healthRows(health) {
    if (!health) return [];
    const rows = [
        { label: 'BLE' + (health.ble.name ? ` (${health.ble.name})` : ''), device: health.ble },
        { label: 'Audio' + (health.audio.name ? ` (${health.audio.name})` : ''), device: health.audio },
        { label: 'Music', device: health.music },
    ];
    for (const [ip, d] of Object.entries(health.wled || {})) rows.push({ label: `WLED ${ip}`, device: d });
    for (const [name, d] of Object.entries(health.tasmota || {})) rows.push({ label: `Tasmota ${name}`, device: d });
    return rows;
}

function healthBadge(d) {
    if (!d.connected) return ['Disconnected', 'down'];
    if (d.stale) return ['Stale', 'stale'];
    return ['Connected', 'up'];
}

// Render hardware.health (status registry) as one row per device.
// Uses textContent only, so device-supplied error strings cannot inject markup.
function renderDeviceHealth(health) {
    const root = document.getElementById('deviceHealth');
    if (!root) return;
    const el = (tag, cls, text) => {
        const n = document.createElement(tag);
        if (cls) n.className = cls;
        if (text !== undefined) n.textContent = text;
        return n;
    };
    const fact = (k, v, cls) => {
        const row = el('div', 'health-fact' + (cls ? ' ' + cls : ''));
        row.appendChild(el('span', 'health-k', k));
        row.appendChild(el('span', 'health-v', v));
        return row;
    };
    root.replaceChildren();
    const rows = healthRows(health);
    if (rows.length === 0) {
        root.appendChild(el('div', 'health-empty', 'No health data'));
        return;
    }
    for (const { label, device: d } of rows) {
        const [badgeText, badgeCls] = healthBadge(d);
        const card = el('div', `health-row ${badgeCls}`);
        const head = el('div', 'health-head');
        head.appendChild(el('span', 'health-name', label));
        head.appendChild(el('span', `health-badge ${badgeCls}`, badgeText));
        card.appendChild(head);

        const seen = relativeTime(d.last_seen);
        card.appendChild(fact('Last seen', seen || 'never', d.stale ? 'warn' : ''));
        if (!d.connected) {
            const when = relativeTime(d.disconnected_at);
            card.appendChild(fact('Disconnected', when || 'never connected'));
            if (d.disconnect_reason) card.appendChild(fact('Reason', d.disconnect_reason, 'err'));
        }
        if (d.last_error) {
            const when = relativeTime(d.last_error_at);
            card.appendChild(fact('Last error', when ? `${d.last_error} (${when})` : d.last_error, 'err'));
        }
        if (d.error_count > 0) card.appendChild(fact('Errors', String(d.error_count)));
        root.appendChild(card);
    }
}

// Update statistics
function updateStatistics(stats) {
    document.getElementById('totalSessions').textContent = stats.total_sessions;
    document.getElementById('punchCount').textContent = stats.current_session_punches;
    document.getElementById('maxPower').textContent = Math.round(stats.current_session_max_power);

    // Update uptime
    const hours = Math.floor(stats.uptime_sec / 3600);
    const minutes = Math.floor((stats.uptime_sec % 3600) / 60);
    document.getElementById('uptime').textContent = `${hours}h ${minutes}m`;
}

// Update audio level meter
function updateAudioLevel(db) {
    // Convert dB to percentage (normalized from -60 to 0 dB)
    const percentage = Math.max(0, Math.min(100, ((db + 60) / 60) * 100));
    document.getElementById('audioMeter').style.width = percentage + '%';
}

// Update music status
function updateMusicStatus(playing, track) {
    const musicStatus = document.getElementById('musicStatus');
    const musicTrack = document.getElementById('musicTrack');
    const musicCard = document.getElementById('musicCard');

    if (playing) {
        musicStatus.textContent = 'Playing';
        musicStatus.classList.add('connected');
        musicCard.classList.add('connected');
        musicTrack.textContent = track || 'Unknown Track';
    } else {
        musicStatus.textContent = 'Stopped';
        musicStatus.classList.remove('connected');
        musicCard.classList.remove('connected');
        musicTrack.textContent = '';
    }
}

// Update connection status indicator
function updateConnectionStatus(connected) {
    const statusDot = document.getElementById('connectionStatus');
    const statusText = document.getElementById('connectionText');

    if (connected) {
        statusDot.classList.add('connected');
        statusText.textContent = 'Connected';
    } else {
        statusDot.classList.remove('connected');
        statusText.textContent = 'Disconnected';
    }
}

// Animate punch effect
function animatePunch() {
    const stateBadge = document.getElementById('stateBadge');
    stateBadge.style.transform = 'scale(1.1)';
    setTimeout(() => {
        stateBadge.style.transform = 'scale(1)';
    }, 100);
}

// Hardware Discovery & Management
async function scanHardware() {
    const scanBtn = document.getElementById('scanBtn');
    scanBtn.disabled = true;
    scanBtn.textContent = 'Scanning...';

    try {
        const response = await fetch('/api/discovery/scan', { method: 'POST' });
        const result = await response.json();
        if (result.success) {
            // Poll device list every 2s for 16s (covers mDNS 5s + SSDP 5s + BLE 5s)
            let polls = 0;
            const pollInterval = setInterval(async () => {
                await loadDevices();
                polls++;
                if (polls >= 8) clearInterval(pollInterval);
            }, 2000);
        }
    } catch (error) {
        console.error('Scan failed:', error);
    } finally {
        setTimeout(() => {
            scanBtn.disabled = false;
            scanBtn.textContent = 'Scan Hardware (mDNS/SSDP/BLE)';
        }, 16000);
    }
}

async function loadDevices() {
    try {
        const response = await fetch('/api/devices');
        const devices = await response.json();
        renderDevices(devices);
    } catch (e) {
        console.error('Failed to load devices:', e);
    }
}

function renderDevices(devices) {
    const container = document.getElementById('deviceListContainer');
    if (devices.length === 0) {
        container.innerHTML = '<p>No devices configured.</p>';
        return;
    }

    container.innerHTML = `
        <table class="table" style="width: 100%; border-collapse: collapse; margin-top: 10px;">
            <thead>
                <tr style="text-align: left; border-bottom: 1px solid #444;">
                    <th style="padding: 8px;">Name</th>
                    <th style="padding: 8px;">Type</th>
                    <th style="padding: 8px;">Address</th>
                    <th style="padding: 8px;">Actions</th>
                </tr>
            </thead>
            <tbody>
                ${devices.map(dev => `
                    <tr style="border-bottom: 1px solid #333;">
                        <td style="padding: 8px;">${dev.name}</td>
                        <td style="padding: 8px;"><span class="badge ${dev.device_type}">${dev.device_type}</span></td>
                        <td style="padding: 8px;">${dev.host}${dev.port > 0 ? ':' + dev.port : ''}</td>
                        <td style="padding: 8px;">
                            <button class="btn btn-small btn-danger" onclick="deleteDevice(${dev.id})">Remove</button>
                        </td>
                    </tr>
                `).join('')}
            </tbody>
        </table>
    `;
}

function toggleAddDeviceForm() {
    const form = document.getElementById('addDeviceForm');
    form.style.display = form.style.display === 'none' ? 'block' : 'none';
}

async function saveDevice() {
    const payload = {
        name: document.getElementById('devName').value,
        device_type: document.getElementById('devType').value,
        host: document.getElementById('devHost').value,
        port: parseInt(document.getElementById('devPort').value) || 0,
        metadata: null
    };

    if (!payload.name || !payload.host) {
        alert('Name and Host are required');
        return;
    }

    try {
        const response = await fetch('/api/devices', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(payload)
        });
        const data = await response.json();
        if (data.status === 'success') {
            loadDevices();
            toggleAddDeviceForm();
        }
    } catch (e) {
        console.error('Save failed:', e);
    }
}

async function deleteDevice(id) {
    if (!confirm('Remove this device?')) return;
    try {
        await fetch(`/api/devices/${id}`, { method: 'POST' }); // Using POST for delete as per route
        loadDevices();
    } catch (e) {
        console.error('Delete failed:', e);
    }
}

// Start periodic update loop (fallback)
function startUpdateLoop() {
    setInterval(refreshState, 5000); // Refresh every 5 seconds
    loadDevices(); // Initial load
}

// API Actions
async function triggerPresence(detected) {
    try {
        const response = await fetch('/api/arena/presence', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ detected })
        });

        const data = await response.json();
        if (!data.success) {
            console.error('Failed to trigger presence:', data.error);
        }
    } catch (error) {
        console.error('Failed to trigger presence:', error);
    }
}

async function suspend() {
    try {
        const response = await fetch('/api/arena/suspend', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' }
        });

        const data = await response.json();
        if (!data.success) {
            console.error('Failed to suspend:', data.error);
        }
    } catch (error) {
        console.error('Failed to suspend:', error);
    }
}

async function simulateShout() {
    try {
        await fetch('/api/arena/shout', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ intensity: 0.5 })
        });
    } catch (error) {
        console.error('Failed to simulate shout:', error);
    }
}

async function simulatePunch() {
    try {
        await fetch('/api/arena/punch', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ power: 50.0 })
        });
    } catch (error) {
        console.error('Failed to simulate punch:', error);
    }
}

async function resetStats() {
    if (!confirm('Are you sure you want to reset statistics?')) return;
    try {
        await fetch('/api/arena/reset_stats', {
            method: 'POST'
        });
        refreshState();
    } catch (error) {
        console.error('Failed to reset stats:', error);
    }
}

async function forceState() {
    const state = document.getElementById('stateSelect').value;
    try {
        const response = await fetch('/api/arena/state', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ state })
        });
        const data = await response.json();
        if (!data.success) {
            alert('Failed to change state: ' + data.error);
        }
    } catch (error) {
        console.error('Failed to force state:', error);
    }
}
// Config Management
async function loadConfig() {
    try {
        const response = await fetch('/api/arena/config');
        const result = await response.json();
        if (result.success) {
            currentTunables = result.data;
            populateConfigForm(currentTunables);
        }
    } catch (error) {
        console.error('Failed to load config:', error);
    }
}

function populateConfigForm(tunables) {
    document.getElementById('confEnergyThreshold').value = tunables.warming_energy_threshold;
    document.getElementById('confEnergyDecay').value = tunables.energy_decay_rate;
    document.getElementById('confFightDuration').value = tunables.fight_duration_sec;
    document.getElementById('confInactivityTimeout').value = tunables.fight_inactivity_timeout_sec;
    document.getElementById('confCooldownDuration').value = tunables.cooldown_duration_sec;
    document.getElementById('confPunchThreshold').value = tunables.punch_threshold;
    document.getElementById('confPunchAlpha').value = tunables.punch_alpha;

    // New shout fields
    document.getElementById('confShoutMargin').value = tunables.shout_margin_db;
    document.getElementById('confShoutPlaybackMargin').value = tunables.shout_playback_margin_db;
    document.getElementById('confShoutMinDuration').value = tunables.shout_min_duration_sec;
    document.getElementById('confShoutScoreThreshold').value = tunables.shout_score_threshold;
    document.getElementById('confShoutMinFeatures').value = tunables.shout_min_features;
    document.getElementById('confShoutGapFrames').value = tunables.shout_gap_frames;
    document.getElementById('confShoutSensitivity').value = tunables.shout_sensitivity;
    document.getElementById('confShoutEnergyMultiplier').value = tunables.shout_energy_multiplier;
    document.getElementById('confVadThreshold').value = tunables.vad_threshold;
}

async function saveConfig() {
    const tunables = {
        warming_energy_threshold: parseFloat(document.getElementById('confEnergyThreshold').value),
        shout_energy_multiplier: parseFloat(document.getElementById('confShoutEnergyMultiplier').value),
        energy_decay_rate: parseFloat(document.getElementById('confEnergyDecay').value),
        fight_duration_sec: parseInt(document.getElementById('confFightDuration').value),
        fight_inactivity_timeout_sec: parseInt(document.getElementById('confInactivityTimeout').value),
        cooldown_duration_sec: parseInt(document.getElementById('confCooldownDuration').value),
        punch_threshold: parseFloat(document.getElementById('confPunchThreshold').value),
        punch_alpha: parseFloat(document.getElementById('confPunchAlpha').value),

        // New shout fields
        shout_margin_db: parseFloat(document.getElementById('confShoutMargin').value),
        shout_playback_margin_db: parseFloat(document.getElementById('confShoutPlaybackMargin').value),
        shout_min_duration_sec: parseFloat(document.getElementById('confShoutMinDuration').value),
        shout_score_threshold: parseFloat(document.getElementById('confShoutScoreThreshold').value),
        shout_min_features: parseInt(document.getElementById('confShoutMinFeatures').value),
        shout_gap_frames: parseInt(document.getElementById('confShoutGapFrames').value),
        shout_sensitivity: parseFloat(document.getElementById('confShoutSensitivity').value),
        vad_threshold: parseFloat(document.getElementById('confVadThreshold').value)
    };

    try {
        const response = await fetch('/api/arena/config', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(tunables)
        });
        const result = await response.json();
        if (result.success) {
            alert('Configuration saved successfully!');
            currentTunables = tunables;
        } else {
            alert('Failed to save configuration: ' + result.error);
        }
    } catch (error) {
        console.error('Failed to save config:', error);
        alert('Failed to save configuration.');
    }
}

async function recordClip() {
    showMsg('clipError', '');
    showMsg('clipOk', '');
    const body = { reason: document.getElementById('clipReason').value.trim() || 'manual' };
    const secs = document.getElementById('clipSeconds').value;
    if (secs !== '') body.seconds = parseFloat(secs);
    const btn = document.getElementById('clipBtn');
    btn.disabled = true;
    try {
        const response = await fetch('/api/audio/clip', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(body)
        });
        const result = await response.json();
        if (result.success) {
            showMsg('clipOk', 'Saved: ' + result.data.path);
        } else {
            showMsg('clipError', 'Clip failed: ' + result.error);
        }
    } catch (e) {
        showMsg('clipError', 'Clip failed: ' + e.message);
    } finally {
        btn.disabled = false;
    }
}

// Connect to logs SSE stream
function connectLogStream() {
    if (logEventSource) {
        logEventSource.close();
    }

    logEventSource = new EventSource('/api/logs');

    logEventSource.onopen = () => {
        console.log('Logs stream connected');
    };

    logEventSource.onerror = (error) => {
        console.error('Logs stream error:', error);
        logEventSource.close();
        
        // Reconnect after 5 seconds on error
        setTimeout(connectLogStream, 5000);
    };

    logEventSource.onmessage = (event) => {
        addLogLine(event.data);
    };
}

function addLogLine(text) {
    const logViewer = document.getElementById('logViewer');
    if (!logViewer) return;

    // Detect level to style appropriately
    let levelClass = 'info';
    let lowerText = text.toLowerCase();
    if (lowerText.includes('warn') || lowerText.includes('⚠️')) {
        levelClass = 'warn';
    } else if (lowerText.includes('error') || lowerText.includes('fail') || lowerText.includes('🔥') || lowerText.includes('panic')) {
        levelClass = 'error';
    } else if (lowerText.includes('debug') || lowerText.includes('trace')) {
        levelClass = 'debug';
    } else if (lowerText.includes('info')) {
        levelClass = 'info';
    }

    const logEntry = document.createElement('div');
    logEntry.className = `log-entry ${levelClass}`;
    logEntry.textContent = text.trim();

    // Store level for filtering
    logEntry.dataset.level = levelClass;

    // Apply filtering if necessary
    const currentFilter = document.getElementById('logLevelSelect').value;
    if (currentFilter !== 'all' && levelClass !== currentFilter) {
        logEntry.style.display = 'none';
    }

    // Append to viewer
    logViewer.appendChild(logEntry);

    // Enforce ring buffer (MAX_LOG_LINES)
    while (logViewer.childNodes.length > MAX_LOG_LINES) {
        logViewer.removeChild(logViewer.firstChild);
    }

    // Auto-scroll
    const autoScroll = document.getElementById('autoScrollCheck').checked;
    if (autoScroll) {
        logViewer.scrollTop = logViewer.scrollHeight;
    }
}

function filterLogs() {
    const currentFilter = document.getElementById('logLevelSelect').value;
    const logViewer = document.getElementById('logViewer');
    if (!logViewer) return;

    const entries = logViewer.getElementsByClassName('log-entry');
    for (let entry of entries) {
        if (currentFilter === 'all' || entry.dataset.level === currentFilter) {
            entry.style.display = 'block';
        } else {
            entry.style.display = 'none';
        }
    }
    
    // Auto-scroll after filter change if enabled
    const autoScroll = document.getElementById('autoScrollCheck').checked;
    if (autoScroll) {
        logViewer.scrollTop = logViewer.scrollHeight;
    }
}

function clearLogs() {
    const logViewer = document.getElementById('logViewer');
    if (logViewer) {
        logViewer.innerHTML = '';
    }
}


// ---- Schedule ----
const SCHEDULE_DAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

async function apiJson(url, options) {
    const response = await fetch(url, options);
    let result;
    try {
        result = await response.json();
    } catch (e) {
        throw new Error(`HTTP ${response.status}`);
    }
    if (!result.success) {
        throw new Error(result.error || 'Request failed');
    }
    return result.data;
}

function showMsg(id, text) {
    const el = document.getElementById(id);
    if (el) el.textContent = text || '';
}

function addScheduleWindow(w) {
    if (!w || w instanceof Event) w = { days: [], start: '09:00', end: '17:00' };
    const days = (w.days || []).map(d => d.slice(0, 3).toLowerCase());
    const div = document.createElement('div');
    div.className = 'config-group schedule-window';
    const dayBoxes = SCHEDULE_DAYS.map(d =>
        `<label class="inline-check"><input type="checkbox" class="sw-day" value="${d}"${days.includes(d.toLowerCase()) ? ' checked' : ''}> ${d}</label>`
    ).join('');
    div.innerHTML = `
        <div class="sw-days">${dayBoxes}</div>
        <div class="sw-times">
            <input type="time" class="form-input sw-start" required>
            <span>to</span>
            <input type="time" class="form-input sw-end" required>
        </div>
        <label class="inline-check"><input type="checkbox" class="sw-enabled"> Enabled</label>
        <div class="hint">No days selected = every day. End before start = overnight.</div>
        <button type="button" class="btn btn-small btn-danger">Remove</button>`;
    div.querySelector('.sw-start').value = w.start;
    div.querySelector('.sw-end').value = w.end;
    div.querySelector('.sw-enabled').checked = w.enabled !== false;
    div.querySelector('button').onclick = () => div.remove();
    document.getElementById('scheduleWindows').appendChild(div);
}

function renderSchedule(status) {
    const cfg = status.config;
    document.getElementById('schedEnabled').checked = !!cfg.enabled;
    document.getElementById('scheduleWindows').innerHTML = '';
    (cfg.windows || []).forEach(addScheduleWindow);
    const el = document.getElementById('scheduleStatus');
    el.textContent = status.active ? 'Game is ACTIVE now' : 'Game is OFF (outside schedule)';
    el.className = 'schedule-status ' + (status.active ? 'on' : 'off');
}

async function loadSchedule() {
    showMsg('scheduleError', '');
    try {
        renderSchedule(await apiJson('/api/schedule'));
    } catch (e) {
        showMsg('scheduleError', 'Failed to load schedule: ' + e.message);
    }
}

async function saveSchedule() {
    const windows = Array.from(document.querySelectorAll('#scheduleWindows .schedule-window')).map(div => ({
        days: Array.from(div.querySelectorAll('.sw-day:checked')).map(c => c.value),
        start: div.querySelector('.sw-start').value,
        end: div.querySelector('.sw-end').value,
        enabled: div.querySelector('.sw-enabled').checked
    }));
    const body = { enabled: document.getElementById('schedEnabled').checked, windows };
    showMsg('scheduleError', '');
    try {
        renderSchedule(await apiJson('/api/schedule', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(body)
        }));
    } catch (e) {
        showMsg('scheduleError', 'Failed to save schedule: ' + e.message);
    }
}

// ---- WLED effects ----
async function loadWledLists() {
    showMsg('effectError', '');
    const errors = [];
    for (const [path, listId] of [['effects', 'fxEffectList'], ['palettes', 'fxPaletteList']]) {
        const list = document.getElementById(listId);
        try {
            const data = await apiJson('/api/wled/' + path);
            list.innerHTML = '';
            data.names.forEach(n => {
                const o = document.createElement('option');
                o.value = n;
                list.appendChild(o);
            });
        } catch (e) {
            errors.push(`Failed to load ${path}: ${e.message}`);
        }
    }
    showMsg('effectError', errors.join('; '));
}

function toggleEffectColors() {
    const on = document.getElementById('fxUseColors').checked;
    for (let i = 0; i < 3; i++) document.getElementById('fxColor' + i).disabled = !on;
}

async function applyEffect() {
    const num = id => parseInt(document.getElementById(id).value, 10);
    const effect = {
        effect: document.getElementById('fxEffect').value.trim(),
        speed: num('fxSpeed'),
        intensity: num('fxIntensity'),
        brightness: num('fxBrightness')
    };
    const palette = document.getElementById('fxPalette').value.trim();
    if (palette) effect.palette = palette;
    const tr = document.getElementById('fxTransition').value;
    if (tr !== '') effect.transition = parseInt(tr, 10);
    if (document.getElementById('fxUseColors').checked) {
        effect.colors = [0, 1, 2].map(i => {
            const v = document.getElementById('fxColor' + i).value;
            return [1, 3, 5].map(p => parseInt(v.slice(p, p + 2), 16));
        });
    }
    showMsg('effectError', '');
    showMsg('effectOk', '');
    try {
        const r = await apiJson('/api/wled/effect', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(effect)
        });
        showMsg('effectOk', `Applied to ${r.controllers_updated} controller(s)`);
    } catch (e) {
        showMsg('effectError', 'Failed to apply effect: ' + e.message);
    }
}

document.addEventListener('DOMContentLoaded', () => {
    ['Speed', 'Intensity', 'Brightness'].forEach(n => {
        const input = document.getElementById('fx' + n);
        if (input) input.addEventListener('input', () => {
            document.getElementById('fx' + n + 'Val').textContent = input.value;
        });
    });
});
