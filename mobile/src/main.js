import './style.css'
import { KNOWN_TRACKS, getTrackById, getTracksByEnergy, getRandomTrack, getTrackStats } from './tracks.js'
import firebaseManager from './firebase.js'

class ShadowWarrior {
  constructor() {
    this.accelerometerData = { x: 0, y: 0, z: 0 };
    this.rawAudioLevel = 0;  // Raw microphone level before gain
    this.audioLevel = 0;  // Processed audio level after gain and smoothing
    this.trackLevel = 0;  // Audio track volume level
    this.audioContext = null;
    this.analyser = null;
    this.microphone = null;
    this.audioElement = null;
    this.isRunning = false;
    this.bleDevice = null;
    this.bleService = null;
    this.bleCharacteristic = null;
    this.websocket = null;
    
    // Graph data storage
    this.graphData = {
      accelX: [],
      accelY: [],
      accelZ: [],
      audio: [],
      track: []
    };
    this.maxDataPoints = 100;
    
    // Audio scaling settings
    this.audioScale = 5.0;  // Multiplier to scale audio level
    this.audioMinLevel = 0.1;  // Minimum audio level to ensure responsiveness
    
    // Accelerometer scaling settings
    this.accelScale = 8.0;  // Multiplier to scale accelerometer energy (increased for beat alignment)
    this.accelThreshold = 0.1;  // Minimum threshold for accelerometer (reduced for beat alignment)
    
    // Energy smoothing state
    this.smoothedAudioLevel = 0;
    this.smoothedAccelEnergy = 0;
    
    // Track caching
    this.cachedTracks = new Map();
    this.defaultTrackId = 'war-is-coming';
    
    this.init();
  }

  async initFirebase() {
    try {
      await firebaseManager.initialize();
      console.log('Firebase initialized successfully');
      await firebaseManager.logEvent('app_started', { 
        timestamp: new Date().toISOString(),
        userAgent: navigator.userAgent 
      });
    } catch (error) {
      console.error('Firebase initialization failed:', error);
      // Don't throw here, just log the error
    }
  }

  async logError(error, context = '') {
    console.error('Error:', error, 'Context:', context);
    await firebaseManager.logError(error, context);
  }

  async logEvent(eventType, data = {}) {
    await firebaseManager.logEvent(eventType, data);
  }

  init() {
    this.createUI();
    this.setupEventListeners();
    this.preloadDefaultTrack();
    this.refreshAudioDevices();
    this.initFirebase();
  }

  createUI() {
    document.querySelector('#app').innerHTML = `
      <div class="container">
        <h1>🥋 Shadow Warrior</h1>
        <div class="status-table">
          <h3>System Status</h3>
          <table>
            <tbody>
              <tr>
                <th>Microphone</th>
                <td id="mic-status">Not started</td>
              </tr>
              <tr>
                <th>BLE</th>
                <td id="ble-status">Not connected</td>
              </tr>
              <tr>
                <th>Audio Level</th>
                <td id="audio-level">0.00</td>
              </tr>
              <tr>
                <th>Combined Energy</th>
                <td id="combined-energy">0.00</td>
              </tr>
            </tbody>
          </table>
        </div>

        <div class="accelerometer-table">
          <h3>Accelerometer Data</h3>
          <table>
            <thead>
              <tr>
                <th>Status</th>
                <th>X</th>
                <th>Y</th>
                <th>Z</th>
                <th>Energy</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td id="accel-status">Not started</td>
                <td id="accel-x-value">0.00</td>
                <td id="accel-y-value">0.00</td>
                <td id="accel-z-value">0.00</td>
                <td id="accel-energy-value">0.00</td>
              </tr>
              <tr>
                <td colspan="5">
                  <div class="energy-meter-row">
                    <label>Energy Level:</label>
                    <div class="energy-meter">
                      <div class="meter-fill" id="accel-energy-meter"></div>
                      <span class="meter-value" id="accel-energy-display">0.00</span>
                    </div>
                  </div>
                </td>
              </tr>
              <tr>
                <td colspan="5">
                  <div class="microphone-meter-row">
                    <label>Microphone Level:</label>
                    <div class="microphone-meter">
                      <div class="meter-fill" id="meter-fill"></div>
                      <span class="meter-value" id="microphone-display">0.00</span>
                    </div>
                  </div>
                </td>
              </tr>
              <tr>
                <td colspan="5">
                  <div class="microphone-output-meter-row">
                    <label>Microphone Output (with gain):</label>
                    <div class="microphone-output-meter">
                      <div class="meter-fill" id="microphone-output-meter-fill"></div>
                      <span class="meter-value" id="microphone-output-display">0.00</span>
                    </div>
                  </div>
                </td>
              </tr>
              <tr>
                <td colspan="5">
                  <div class="audio-track-meter-row">
                    <label>Audio Track Level:</label>
                    <div class="audio-track-meter">
                      <div class="meter-fill" id="track-meter-fill"></div>
                      <span class="meter-value" id="track-display">0.00</span>
                    </div>
                  </div>
                </td>
              </tr>
            </tbody>
          </table>
        </div>

        <div class="debug-graphs">
          <h3>Debug Graphs</h3>
          <div class="graph-container">
            <div class="graph">
              <h4>Audio Level</h4>
              <canvas id="audio-graph" width="300" height="100"></canvas>
            </div>
            <div class="graph">
              <h4>Accelerometer X</h4>
              <canvas id="accel-x-graph" width="300" height="100"></canvas>
            </div>
            <div class="graph">
              <h4>Accelerometer Y</h4>
              <canvas id="accel-y-graph" width="300" height="100"></canvas>
            </div>
            <div class="graph">
              <h4>Accelerometer Z</h4>
              <canvas id="accel-z-graph" width="300" height="100"></canvas>
            </div>
          </div>
        </div>

        <div class="controls">
          <button id="start-btn" class="btn primary">Start Training (Auto Audio)</button>
          <button id="stop-btn" class="btn secondary" disabled>Stop Training</button>
          <button id="connect-ble" class="btn">Connect BLE</button>
          <button id="connect-websocket" class="btn">Connect WebSocket</button>
          <button id="request-mic-permission" class="btn">Request Microphone Permission</button>
          <button id="request-accel-permission" class="btn" style="display: none;">Request Motion Permission</button>
        </div>

        <div class="audio-controls">
          <h3>🎵 Track Selection</h3>
          <div id="track-status" class="track-status">Preloading default track...</div>
          <div class="track-selection">
            <select id="track-select" class="track-dropdown">
              <option value="">Select a track...</option>
              ${KNOWN_TRACKS.map(track => 
                `<option value="${track.id}">${track.name} - ${track.artist} (${track.genre})</option>`
              ).join('')}
            </select>
            <button id="load-track" class="btn">Load Track</button>
            <button id="random-track" class="btn">Random Track</button>
          </div>
          <div class="custom-audio">
            <input type="file" id="audio-file" accept="audio/*" />
            <button id="load-audio" class="btn">Load Custom Audio</button>
          </div>
        </div>


          <div class="audio-controls">
            <div class="control-group">
              <label>Audio Scale:</label>
              <div class="control-buttons">
                <button id="audio-minus" class="btn-small">-</button>
                <span id="audio-scale-value">5.0</span>
                <button id="audio-plus" class="btn-small">+</button>
              </div>
            </div>
            <div class="control-group">
              <label>Accel Scale:</label>
              <div class="control-buttons">
                <button id="accel-minus" class="btn-small">-</button>
                <span id="accel-scale-value">5.0</span>
                <button id="accel-plus" class="btn-small">+</button>
              </div>
            </div>
            <div class="control-group">
              <label>Accel Threshold:</label>
              <div class="control-buttons">
                <button id="threshold-minus" class="btn-small">-</button>
                <span id="threshold-value">0.5</span>
                <button id="threshold-plus" class="btn-small">+</button>
              </div>
            </div>
            <div class="control-group">
              <label>Audio Min Level:</label>
              <div class="control-buttons">
                <button id="audio-min-minus" class="btn-small">-</button>
                <span id="audio-min-value">0.1</span>
                <button id="audio-min-plus" class="btn-small">+</button>
              </div>
            </div>
          </div>
        </div>

        <div class="audio-device-controls">
          <h3>🎤 Audio Device Selection</h3>
          <div class="device-selection">
            <div class="device-group">
              <label for="mic-device-select">Microphone Input:</label>
              <select id="mic-device-select" class="device-dropdown">
                <option value="">Select microphone...</option>
              </select>
            </div>
            <div class="device-group">
              <label for="speaker-device-select">Speaker Output:</label>
              <select id="speaker-device-select" class="device-dropdown">
                <option value="">Select speaker...</option>
              </select>
            </div>
            <button id="refresh-devices" class="btn">Refresh Devices</button>
          </div>
        </div>
      </div>
    `;
  }

  setupEventListeners() {
    document.getElementById('start-btn').addEventListener('click', () => this.startTraining());
    document.getElementById('stop-btn').addEventListener('click', () => this.stopTraining());
    document.getElementById('connect-ble').addEventListener('click', () => this.connectBLE());
    document.getElementById('connect-websocket').addEventListener('click', () => this.connectWebSocket());
    document.getElementById('load-audio').addEventListener('click', () => this.loadAudio());
    document.getElementById('request-mic-permission').addEventListener('click', () => this.requestMicrophonePermission());
    document.getElementById('request-accel-permission').addEventListener('click', () => this.requestMotionPermission());
    
    // Track selection controls
    document.getElementById('load-track').addEventListener('click', () => this.loadSelectedTrack());
    document.getElementById('random-track').addEventListener('click', () => this.loadRandomTrack());
    
    // Audio scale controls
    document.getElementById('audio-minus').addEventListener('click', () => this.adjustAudioScale(-0.5));
    document.getElementById('audio-plus').addEventListener('click', () => this.adjustAudioScale(0.5));
    
    // Accelerometer scale controls
    document.getElementById('accel-minus').addEventListener('click', () => this.adjustAccelScale(-0.2));
    document.getElementById('accel-plus').addEventListener('click', () => this.adjustAccelScale(0.2));
    
    // Accelerometer threshold controls
    document.getElementById('threshold-minus').addEventListener('click', () => this.adjustAccelThreshold(-0.1));
    document.getElementById('threshold-plus').addEventListener('click', () => this.adjustAccelThreshold(0.1));
    
    // Audio minimum level controls
    document.getElementById('audio-min-minus').addEventListener('click', () => this.adjustAudioMinLevel(-0.05));
    document.getElementById('audio-min-plus').addEventListener('click', () => this.adjustAudioMinLevel(0.05));
    
    // Audio device selection controls
    document.getElementById('refresh-devices').addEventListener('click', () => this.refreshAudioDevices());
    document.getElementById('mic-device-select').addEventListener('change', (e) => this.selectMicrophoneDevice(e.target.value));
    document.getElementById('speaker-device-select').addEventListener('change', (e) => this.selectSpeakerDevice(e.target.value));
    
    // Check if we need to show permission button for iOS
    this.checkPermissionRequirements();
  }

  async checkPermissionRequirements() {
    // Show permission button for iOS devices
    if (typeof DeviceMotionEvent !== 'undefined' && typeof DeviceMotionEvent.requestPermission === 'function') {
      document.getElementById('request-accel-permission').style.display = 'block';
    }
  }

  async requestMicrophonePermission() {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      // Stop the stream immediately as we just wanted to check permission
      stream.getTracks().forEach(track => track.stop());
      console.log('Microphone permission granted');
      document.getElementById('mic-status').textContent = 'Permission granted - ready to start';
      alert('Microphone permission granted!');
    } catch (error) {
      console.error('Microphone permission denied:', error);
      document.getElementById('mic-status').textContent = 'Permission denied';
      alert('Microphone permission denied. Please allow microphone access to use this feature.');
      this.logError(error, 'microphone_permission_request');
    }
  }

  async requestMotionPermission() {
    try {
      if (typeof DeviceMotionEvent.requestPermission === 'function') {
        const permission = await DeviceMotionEvent.requestPermission();
        console.log('Motion permission result:', permission);
        
        if (permission === 'granted') {
          document.getElementById('request-accel-permission').style.display = 'none';
          document.getElementById('accel-status').textContent = 'Permission granted - ready to start';
          alert('Accelerometer permission granted!');
        } else {
          document.getElementById('accel-status').textContent = 'Permission denied';
          alert('Accelerometer permission denied. Please allow motion access to use this feature.');
        }
      }
    } catch (error) {
      console.error('Permission request error:', error);
      document.getElementById('accel-status').textContent = 'Permission error: ' + error.message;
      alert('Error requesting accelerometer permission: ' + error.message);
      this.logError(error, 'motion_permission_request');
    }
  }

  async checkMicrophonePermission() {
    try {
      // Try to get microphone permission
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      // If successful, stop the stream immediately as we just wanted to check permission
      stream.getTracks().forEach(track => track.stop());
      console.log('Microphone permission granted');
      return true;
    } catch (error) {
      console.log('Microphone permission denied or not available:', error.message);
      this.logError(error, 'microphone_permission_check');
      return false;
    }
  }

  async checkAccelerometerPermission() {
    try {
      // Check if we're on iOS and need to request permission
      if (typeof DeviceMotionEvent.requestPermission === 'function') {
        const permission = await DeviceMotionEvent.requestPermission();
        console.log('Accelerometer permission result:', permission);
        return permission === 'granted';
      }
      
      // For other platforms, check if accelerometer is available
      if ('Accelerometer' in window) {
        // Try to create an accelerometer instance to test availability
        const sensor = new Accelerometer({ frequency: 1 });
        await sensor.start();
        sensor.stop();
        console.log('Accelerometer available');
        return true;
      }
      
      // Check if DeviceMotionEvent is available
      if ('DeviceMotionEvent' in window) {
        console.log('DeviceMotionEvent available');
        return true;
      }
      
      console.log('No accelerometer API available');
      return false;
    } catch (error) {
      console.log('Accelerometer permission check failed:', error.message);
      this.logError(error, 'accelerometer_permission_check');
      return false;
    }
  }

  adjustAudioScale(delta) {
    this.audioScale = Math.max(0.5, Math.min(10.0, this.audioScale + delta));
    document.getElementById('audio-scale-value').textContent = this.audioScale.toFixed(1);
    console.log('Audio scale adjusted to:', this.audioScale);
  }

  adjustAccelScale(delta) {
    this.accelScale = Math.max(0.5, Math.min(5.0, this.accelScale + delta));
    document.getElementById('accel-scale-value').textContent = this.accelScale.toFixed(1);
    console.log('Accelerometer scale adjusted to:', this.accelScale);
  }

  adjustAccelThreshold(delta) {
    this.accelThreshold = Math.max(0.0, Math.min(3.0, this.accelThreshold + delta));
    document.getElementById('threshold-value').textContent = this.accelThreshold.toFixed(1);
    console.log('Accelerometer threshold adjusted to:', this.accelThreshold);
  }

  adjustAudioMinLevel(delta) {
    this.audioMinLevel = Math.max(0.0, Math.min(0.5, this.audioMinLevel + delta));
    document.getElementById('audio-min-value').textContent = this.audioMinLevel.toFixed(2);
    console.log('Audio minimum level adjusted to:', this.audioMinLevel);
  }

  applyEnergySmoothing(rawValue, currentSmoothed, isIncreasing) {
    // Much faster attack for beat alignment (0.4*state + 0.6*input)
    // Faster release for beat alignment (0.7*state + 0.3*input)
    const attackFactor = isIncreasing ? 0.6 : 0.3;
    const stateFactor = isIncreasing ? 0.4 : 0.7;
    
    return stateFactor * currentSmoothed + attackFactor * rawValue;
  }

  enableTestMode() {
    // Simulate accelerometer data for desktop testing
    this.testMode = true;
    this.motionSimulationActive = false;
    console.log('Test mode enabled - simulating accelerometer data');
    
    // Add test controls
    const testControls = `
      <div class="test-mode">
        <h4>Test Mode (Desktop)</h4>
        <p>Simulating accelerometer data for testing</p>
        <button id="simulate-motion" class="btn">Simulate Motion</button>
        <button id="start-continuous-motion" class="btn">Start Continuous Motion</button>
        <button id="stop-continuous-motion" class="btn" style="display: none;">Stop Continuous Motion</button>
      </div>
    `;
    
    const existingTest = document.querySelector('.test-mode');
    if (!existingTest) {
      document.querySelector('.controls').insertAdjacentHTML('afterend', testControls);
      document.getElementById('simulate-motion').addEventListener('click', () => {
        this.simulateMotion();
      });
      document.getElementById('start-continuous-motion').addEventListener('click', () => {
        this.startContinuousMotion();
      });
      document.getElementById('stop-continuous-motion').addEventListener('click', () => {
        this.stopContinuousMotion();
      });
    }
  }

  simulateMotion() {
    if (!this.testMode) return;
    
    // Simulate random motion
    this.accelerometerData = {
      x: (Math.random() - 0.5) * 20,
      y: (Math.random() - 0.5) * 20,
      z: (Math.random() - 0.5) * 20
    };
    
    console.log('Simulated motion:', this.accelerometerData);
  }

  startContinuousMotion() {
    if (!this.testMode) return;
    
    this.motionSimulationActive = true;
    document.getElementById('start-continuous-motion').style.display = 'none';
    document.getElementById('stop-continuous-motion').style.display = 'inline-block';
    
    // Start continuous motion simulation
    this.continuousMotionInterval = setInterval(() => {
      if (!this.motionSimulationActive) return;
      
      // Add small random variations to simulate natural motion
      const variation = 0.5;
      this.accelerometerData = {
        x: this.accelerometerData.x + (Math.random() - 0.5) * variation,
        y: this.accelerometerData.y + (Math.random() - 0.5) * variation,
        z: this.accelerometerData.z + (Math.random() - 0.5) * variation
      };
      
      // Apply decay to return to baseline over time
      const decayFactor = 0.95;
      this.accelerometerData.x *= decayFactor;
      this.accelerometerData.y *= decayFactor;
      this.accelerometerData.z *= decayFactor;
      
      // Occasionally add larger motion spikes
      if (Math.random() < 0.1) {
        this.accelerometerData.x += (Math.random() - 0.5) * 10;
        this.accelerometerData.y += (Math.random() - 0.5) * 10;
        this.accelerometerData.z += (Math.random() - 0.5) * 10;
      }
      
    }, 50); // Update every 50ms for smooth motion
    
    console.log('Continuous motion simulation started');
  }

  stopContinuousMotion() {
    this.motionSimulationActive = false;
    if (this.continuousMotionInterval) {
      clearInterval(this.continuousMotionInterval);
      this.continuousMotionInterval = null;
    }
    
    // Reset accelerometer data to 0 to allow natural decay
    this.accelerometerData = {
      x: 0,
      y: 0,
      z: 0
    };
    
    document.getElementById('start-continuous-motion').style.display = 'inline-block';
    document.getElementById('stop-continuous-motion').style.display = 'none';
    
    console.log('Continuous motion simulation stopped - accelerometer reset to 0');
  }

  async startTraining() {
    // Check permissions first before starting
    const hasMicPermission = await this.checkMicrophonePermission();
    const hasAccelPermission = await this.checkAccelerometerPermission();
    
    if (!hasMicPermission || !hasAccelPermission) {
      alert('Please grant microphone and accelerometer permissions to start training.');
      return;
    }

    this.isRunning = true;
    document.getElementById('start-btn').disabled = true;
    document.getElementById('stop-btn').disabled = false;

    // Auto-start audio if not already playing
    if (!this.audioElement || this.audioElement.paused) {
      await this.autoStartAudio();
    }

    // Start accelerometer (may fail on desktop, but that's OK)
    try {
      await this.startAccelerometer();
    } catch (error) {
      console.log('Accelerometer failed to start, continuing with test mode');
    }

    // Start microphone (may fail, but that's OK)
    try {
      await this.startMicrophone();
    } catch (error) {
      console.log('Microphone failed to start, continuing without audio input');
    }

    // Always start data processing loop, regardless of sensor success
    this.startDataProcessing();
  }

  stopTraining() {
    this.isRunning = false;
    document.getElementById('start-btn').disabled = false;
    document.getElementById('stop-btn').disabled = true;

    // Pause audio
    if (this.audioElement && !this.audioElement.paused) {
      this.audioElement.pause();
      console.log('Audio paused');
    }

    // Stop microphone
    if (this.microphone && this.microphone.mediaStream) {
      this.microphone.mediaStream.getTracks().forEach(track => track.stop());
    }
    if (this.audioContext) {
      this.audioContext.close();
    }
    if (this.audioElement) {
      this.audioElement.pause();
    }
    
    // Clean up DeviceMotionEvent listener
    if (this.motionHandler) {
      window.removeEventListener('devicemotion', this.motionHandler);
      this.motionHandler = null;
    }
    
    // Clean up WebSocket connection
    if (this.websocket) {
      this.websocket.close();
      this.websocket = null;
    }
    
    // Stop continuous motion simulation if running
    if (this.motionSimulationActive) {
      this.stopContinuousMotion();
    }
    
    // Reset status indicators
    document.getElementById('accel-status').textContent = 'Not started';
    document.getElementById('mic-status').textContent = 'Not started';
  }

  async startAccelerometer() {
    console.log('Starting accelerometer...');
    console.log('Accelerometer API available:', 'Accelerometer' in window);
    console.log('DeviceMotionEvent available:', 'DeviceMotionEvent' in window);
    
    // Try modern Accelerometer API first
    if ('Accelerometer' in window) {
      try {
        // Check permissions first
        if (navigator.permissions) {
          const permission = await navigator.permissions.query({ name: 'accelerometer' });
          console.log('Accelerometer permission:', permission.state);
        }

        const sensor = new Accelerometer({ frequency: 60 });
        console.log('Accelerometer sensor created:', sensor);
        
      sensor.addEventListener('reading', () => {
        this.accelerometerData = {
          x: sensor.x || 0,
          y: sensor.y || 0,
          z: sensor.z || 0
        };
      });

        sensor.addEventListener('error', (event) => {
          console.error('Accelerometer error:', event.error);
          document.getElementById('accel-status').textContent = 'Error: ' + event.error.message;
          this.logError(event.error, 'accelerometer_sensor_error');
          
          // If accelerometer fails, enable test mode for desktop testing
          const isDesktop = !/Android|iPhone|iPad|iPod|BlackBerry|IEMobile|Opera Mini/i.test(navigator.userAgent);
          if (isDesktop) {
            console.log('Accelerometer failed on desktop, enabling test mode');
            this.enableTestMode();
            document.getElementById('accel-status').textContent = 'Desktop - Test Mode';
          }
        });

        await sensor.start();
        document.getElementById('accel-status').textContent = 'Active (Modern API)';
        console.log('Accelerometer started successfully');
        return;
      } catch (error) {
        console.error('Accelerometer start error:', error);
        this.logError(error, 'accelerometer_start_error');
        // Fall through to DeviceMotionEvent
      }
    }
    
    // Fallback to DeviceMotionEvent for iOS and older browsers
    if ('DeviceMotionEvent' in window) {
      try {
        console.log('Using DeviceMotionEvent fallback');
        
        // Request permission for iOS 13+
        if (typeof DeviceMotionEvent.requestPermission === 'function') {
          const permission = await DeviceMotionEvent.requestPermission();
          console.log('DeviceMotionEvent permission:', permission);
          if (permission !== 'granted') {
            document.getElementById('accel-status').textContent = 'Permission denied';
            return;
          }
        }
        
        const handleMotion = (event) => {
          if (event.acceleration) {
            this.accelerometerData = {
              x: event.acceleration.x || 0,
              y: event.acceleration.y || 0,
              z: event.acceleration.z || 0
            };
          }
        };
        
        window.addEventListener('devicemotion', handleMotion);
        this.motionHandler = handleMotion; // Store for cleanup
        
        document.getElementById('accel-status').textContent = 'Active (DeviceMotion)';
        console.log('DeviceMotionEvent started successfully');
        return;
      } catch (error) {
        console.error('DeviceMotionEvent error:', error);
        document.getElementById('accel-status').textContent = 'Error: ' + error.message;
        this.logError(error, 'devicemotion_event_error');
        
        // If DeviceMotionEvent fails, enable test mode for desktop testing
        const isDesktop = !/Android|iPhone|iPad|iPod|BlackBerry|IEMobile|Opera Mini/i.test(navigator.userAgent);
        if (isDesktop) {
          console.log('DeviceMotionEvent failed on desktop, enabling test mode');
          this.enableTestMode();
          document.getElementById('accel-status').textContent = 'Desktop - Test Mode';
        }
      }
    }
    
    // If both fail
    const isDesktop = !/Android|iPhone|iPad|iPod|BlackBerry|IEMobile|Opera Mini/i.test(navigator.userAgent);
    if (isDesktop) {
      document.getElementById('accel-status').textContent = 'Desktop - Test Mode';
      console.log('Running on desktop - accelerometer not available, enabling test mode');
      this.enableTestMode();
    } else {
      document.getElementById('accel-status').textContent = 'Not supported';
      console.error('No accelerometer API available');
    }
  }

  async startMicrophone() {
    try {
      console.log('Starting microphone...');
      
      // Check if a specific microphone device is selected
      const micSelect = document.getElementById('mic-device-select');
      const selectedMicId = micSelect ? micSelect.value : null;
      
      const constraints = selectedMicId ? 
        { audio: { deviceId: { exact: selectedMicId } } } : 
        { audio: true };
      
      const stream = await navigator.mediaDevices.getUserMedia(constraints);
      console.log('Microphone stream obtained:', stream);
      
      this.audioContext = new (window.AudioContext || window.webkitAudioContext)();
      console.log('Audio context created:', this.audioContext.state);
      
      // Resume audio context if suspended (required for iOS)
      if (this.audioContext.state === 'suspended') {
        await this.audioContext.resume();
        console.log('Audio context resumed');
      }
      
      this.analyser = this.audioContext.createAnalyser();
      this.microphone = this.audioContext.createMediaStreamSource(stream);
      
      this.analyser.fftSize = 256;
      this.analyser.smoothingTimeConstant = 0.8;
      this.microphone.connect(this.analyser);
      
      console.log('Microphone connected to analyser');
      document.getElementById('mic-status').textContent = 'Active';
    } catch (error) {
      console.error('Microphone error:', error);
      document.getElementById('mic-status').textContent = 'Error: ' + error.message;
      this.logError(error, 'microphone_start_error');
    }
  }

  startDataProcessing() {
    let lastAccelData = { x: 0, y: 0, z: 0 };
    let lastAudioLevel = 0;
    
    const processData = () => {
      if (!this.isRunning) return;

      // Store data for graphs (track level will be updated after volume change)
      this.addGraphData('accelX', this.accelerometerData.x);
      this.addGraphData('accelY', this.accelerometerData.y);
      this.addGraphData('accelZ', this.accelerometerData.z);
      this.addGraphData('audio', this.rawAudioLevel);

      // Calculate accelerometer energy with enhanced sensitivity
      const rawAccelEnergy = Math.sqrt(
        this.accelerometerData.x ** 2 + 
        this.accelerometerData.y ** 2 + 
        this.accelerometerData.z ** 2
      );
      
      // Apply scaling and threshold for more responsive shaking detection with beat alignment sensitivity
      const scaledAccelEnergy = Math.max(0, (rawAccelEnergy - this.accelThreshold) * this.accelScale * 4); // 4x more sensitive for beat alignment
      
      // Apply energy smoothing to accelerometer
      const isAccelIncreasing = scaledAccelEnergy > this.smoothedAccelEnergy;
      this.smoothedAccelEnergy = this.applyEnergySmoothing(scaledAccelEnergy, this.smoothedAccelEnergy, isAccelIncreasing);
      const accelEnergy = this.smoothedAccelEnergy;

      // Calculate audio level with enhanced sensitivity and NaN protection
      if (this.analyser) {
        try {
          const dataArray = new Uint8Array(this.analyser.frequencyBinCount);
          this.analyser.getByteFrequencyData(dataArray);
          
          // Calculate raw level with safety checks
          const sum = dataArray.reduce((acc, value) => acc + value, 0);
          const rawLevel = (sum / dataArray.length) / 255;
          
          // Validate raw level
          if (isNaN(rawLevel) || !isFinite(rawLevel)) {
            console.warn('Invalid raw audio level detected, using fallback');
            this.audioLevel = this.audioMinLevel;
          } else {
            // Apply exponential scaling for more dramatic response to louder sounds
            const scaledLevel = Math.pow(Math.max(0, rawLevel) * this.audioScale, 1.5);
            
            // Validate scaled level and ensure it's within bounds
            if (isNaN(scaledLevel) || !isFinite(scaledLevel)) {
              console.warn('Invalid scaled audio level detected, using fallback');
              this.audioLevel = this.audioMinLevel;
            } else {
              // Store raw audio level (before gain and smoothing) - this is what the first meter shows
              this.rawAudioLevel = Math.max(this.audioMinLevel, Math.min(1, scaledLevel));
              
              // Apply energy smoothing to audio
              const isAudioIncreasing = this.rawAudioLevel > this.smoothedAudioLevel;
              this.smoothedAudioLevel = this.applyEnergySmoothing(this.rawAudioLevel, this.smoothedAudioLevel, isAudioIncreasing);
              this.audioLevel = this.smoothedAudioLevel;
            }
          }
        } catch (error) {
          console.error('Audio processing error:', error);
          this.audioLevel = this.audioMinLevel;
          this.logError(error, 'audio_processing_error');
        }
      }

      // Combined energy with better balance between audio and accelerometer
      const normalizedAccel = Math.min(1, accelEnergy / 200); // Scale accelerometer to 0-1 (increased divisor for proper scaling)
      let combinedEnergy = Math.min(1, (normalizedAccel + this.audioLevel) / 2);
      
      // Validate combined energy to prevent NaN
      if (isNaN(combinedEnergy) || !isFinite(combinedEnergy)) {
        console.warn('Invalid combined energy detected, using fallback');
        combinedEnergy = 0.1; // Safe fallback value
      }

      // Log only on significant changes
      const accelChanged = Math.abs(this.accelerometerData.x - lastAccelData.x) > 0.1 ||
                          Math.abs(this.accelerometerData.y - lastAccelData.y) > 0.1 ||
                          Math.abs(this.accelerometerData.z - lastAccelData.z) > 0.1;
      
      const audioChanged = Math.abs(this.rawAudioLevel - lastAudioLevel) > 0.05;
      
      if (accelChanged) {
        console.log('Accelerometer data changed:', this.accelerometerData);
        console.log('Raw accel energy:', rawAccelEnergy.toFixed(2), 'Scaled:', scaledAccelEnergy.toFixed(2), 'Smoothed:', accelEnergy.toFixed(2));
        console.log('Normalized accel:', normalizedAccel.toFixed(2), 'Raw audio:', this.rawAudioLevel.toFixed(2), 'Processed audio:', this.audioLevel.toFixed(2), 'Combined:', combinedEnergy.toFixed(2));
        lastAccelData = { ...this.accelerometerData };
      }
      
      if (audioChanged) {
        console.log('Raw audio level changed:', this.rawAudioLevel.toFixed(3), 'Processed audio level:', this.audioLevel.toFixed(3));
        lastAudioLevel = this.rawAudioLevel;
      }

      // Update UI
      document.getElementById('accel-x-value').textContent = this.accelerometerData.x.toFixed(2);
      document.getElementById('accel-y-value').textContent = this.accelerometerData.y.toFixed(2);
      document.getElementById('accel-z-value').textContent = this.accelerometerData.z.toFixed(2);
      // Update energy meter
      const energyPercentage = Math.min(100, (accelEnergy / 100) * 100); // Scale to 0-100%
      document.getElementById('accel-energy-meter').style.width = energyPercentage + '%';
      document.getElementById('accel-energy-value').textContent = accelEnergy.toFixed(1);
      document.getElementById('accel-energy-display').textContent = accelEnergy.toFixed(1);
      document.getElementById('audio-level').textContent = this.rawAudioLevel.toFixed(2);
      document.getElementById('combined-energy').textContent = combinedEnergy.toFixed(2);

      // Update audio volume with NaN protection and base volume
      if (this.audioElement && !isNaN(combinedEnergy) && isFinite(combinedEnergy)) {
        // Base volume of 0.3 + energy boost (0.7 max boost)
        const baseVolume = 0.3;
        const energyBoost = combinedEnergy * 0.7;
        const finalVolume = Math.max(0, Math.min(1, baseVolume + energyBoost));
        this.audioElement.volume = finalVolume;
        
        // Update track level after volume change
        this.trackLevel = finalVolume;
        
        // Debug logging for volume changes
        if (Math.abs(finalVolume - (this.lastTrackVolume || 0)) > 0.05) {
          console.log('Volume change - Combined energy:', combinedEnergy.toFixed(2), 'Final volume:', finalVolume.toFixed(2));
          this.lastTrackVolume = finalVolume;
        }
      } else {
        this.trackLevel = 0;
      }
      
      // Add track level to graph data
      this.addGraphData('track', this.trackLevel);

      // Update loudness meters
      this.updateLoudnessMeter(this.rawAudioLevel);
      this.updateMicrophoneOutputMeter(combinedEnergy);
      this.updateTrackMeter(this.trackLevel);

      // Update graphs
      this.updateGraphs();

      // Send BLE command
      this.sendBLECommand(combinedEnergy);

      requestAnimationFrame(processData);
    };

    processData();
  }

  updateLEDMatrix(energy) {
    const grid = document.getElementById('led-grid');
    const intensity = Math.floor(energy * 255);
    const color = `rgb(${intensity}, ${255 - intensity}, 0)`;
    grid.style.backgroundColor = color;
    grid.style.boxShadow = `0 0 20px ${color}`;
  }

  updateLoudnessMeter(level) {
    const meterFill = document.getElementById('meter-fill');
    const microphoneDisplay = document.getElementById('microphone-display');
    
    // Validate level to prevent NaN
    if (isNaN(level) || !isFinite(level)) {
      console.warn('Invalid audio level for meter, using fallback');
      level = this.audioMinLevel;
    }
    
    const percentage = Math.min(100, Math.max(0, level * 100));
    
    // Set the fill width
    meterFill.style.width = `${percentage}%`;
    
    // Update display value
    if (microphoneDisplay) {
      microphoneDisplay.textContent = level.toFixed(2);
    }
    
    // Change color based on level
    if (level <= 0.3) {
      meterFill.style.background = 'linear-gradient(90deg, #4ecdc4, #44a08d)';
    } else if (level <= 0.7) {
      meterFill.style.background = 'linear-gradient(90deg, #f7b731, #f39c12)';
    } else {
      meterFill.style.background = 'linear-gradient(90deg, #ff6b6b, #ee5a24)';
    }
  }

  updateMicrophoneOutputMeter(level) {
    const meterFill = document.getElementById('microphone-output-meter-fill');
    const microphoneOutputDisplay = document.getElementById('microphone-output-display');
    
    // Validate level to prevent NaN
    if (isNaN(level) || !isFinite(level)) {
      console.warn('Invalid microphone output level for meter, using fallback');
      level = this.audioMinLevel;
    }
    
    const percentage = Math.min(100, Math.max(0, level * 100));
    
    // Set the fill width
    meterFill.style.width = `${percentage}%`;
    
    // Update display value
    if (microphoneOutputDisplay) {
      microphoneOutputDisplay.textContent = level.toFixed(2);
    }
    
    // Change color based on level (different colors for microphone output)
    if (level <= 0.3) {
      meterFill.style.background = 'linear-gradient(90deg, #2ecc71, #27ae60)';
    } else if (level <= 0.7) {
      meterFill.style.background = 'linear-gradient(90deg, #f39c12, #e67e22)';
    } else {
      meterFill.style.background = 'linear-gradient(90deg, #e74c3c, #c0392b)';
    }
  }

  updateTrackMeter(level) {
    const meterFill = document.getElementById('track-meter-fill');
    const trackDisplay = document.getElementById('track-display');
    
    // Validate level to prevent NaN
    if (isNaN(level) || !isFinite(level)) {
      console.warn('Invalid track level for meter, using fallback');
      level = 0;
    }
    
    const percentage = Math.min(100, Math.max(0, level * 100));
    
    // Set the fill width
    meterFill.style.width = `${percentage}%`;
    
    // Update display value
    if (trackDisplay) {
      trackDisplay.textContent = level.toFixed(2);
    }
    
    // Change color based on level (different colors for track)
    if (level <= 0.3) {
      meterFill.style.background = 'linear-gradient(90deg, #3498db, #2980b9)';
    } else if (level <= 0.7) {
      meterFill.style.background = 'linear-gradient(90deg, #9b59b6, #8e44ad)';
    } else {
      meterFill.style.background = 'linear-gradient(90deg, #e67e22, #d35400)';
    }
  }

  addGraphData(type, value) {
    this.graphData[type].push(value);
    if (this.graphData[type].length > this.maxDataPoints) {
      this.graphData[type].shift();
    }
  }

  updateGraphs() {
    this.drawGraph('accel-x-graph', this.graphData.accelX, '#ff6b6b', -20, 20);
    this.drawGraph('accel-y-graph', this.graphData.accelY, '#4ecdc4', -20, 20);
    this.drawGraph('accel-z-graph', this.graphData.accelZ, '#45b7d1', -20, 20);
    this.drawGraph('audio-graph', this.graphData.audio, '#f7b731', 0, 1);
  }

  drawGraph(canvasId, data, color, min, max) {
    const canvas = document.getElementById(canvasId);
    if (!canvas) return;
    
    const ctx = canvas.getContext('2d');
    const width = canvas.width;
    const height = canvas.height;
    
    // Clear canvas
    ctx.clearRect(0, 0, width, height);
    
    if (data.length < 2) return;
    
    // Draw grid
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.1)';
    ctx.lineWidth = 1;
    for (let i = 0; i <= 4; i++) {
      const y = (height / 4) * i;
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(width, y);
      ctx.stroke();
    }
    
    // Draw data line
    ctx.strokeStyle = color;
    ctx.lineWidth = 2;
    ctx.beginPath();
    
    data.forEach((value, index) => {
      const x = (width / (data.length - 1)) * index;
      const normalizedValue = (value - min) / (max - min);
      const y = height - (normalizedValue * height);
      
      if (index === 0) {
        ctx.moveTo(x, y);
      } else {
        ctx.lineTo(x, y);
      }
    });
    
    ctx.stroke();
    
    // Draw current value
    if (data.length > 0) {
      const currentValue = data[data.length - 1];
      const normalizedValue = (currentValue - min) / (max - min);
      const y = height - (normalizedValue * height);
      
      ctx.fillStyle = color;
      ctx.beginPath();
      ctx.arc(width - 5, y, 3, 0, 2 * Math.PI);
      ctx.fill();
      
      // Draw value text
      ctx.fillStyle = 'white';
      ctx.font = '12px Arial';
      ctx.fillText(currentValue.toFixed(2), width - 50, y - 5);
    }
  }

  async connectBLE() {
    console.log('Attempting BLE connection...');
    console.log('Web Bluetooth available:', 'bluetooth' in navigator);
    
    // Check if Web Bluetooth is supported (Android Chrome or Bluefy on iOS)
    if ('bluetooth' in navigator) {
      try {
        console.log('Using Web Bluetooth API');
        this.bleDevice = await navigator.bluetooth.requestDevice({
          filters: [{ namePrefix: 'ShadowWarrior-BLE' }],
          optionalServices: ['12345678-1234-1234-1234-123456789ABC']
        });

        this.bleService = await this.bleDevice.gatt.connect();
        this.bleCharacteristic = await this.bleService.getCharacteristic('87654321-4321-4321-4321-CBA987654321');
        
        document.getElementById('ble-status').textContent = 'Connected (Web Bluetooth)';
        console.log('Web Bluetooth connected successfully');
        
        // Show success notification
        this.showNotification('BLE Connected! Ready to send audio levels.', 'success');
        
      } catch (error) {
        console.error('Web Bluetooth error:', error);
        document.getElementById('ble-status').textContent = 'Error: ' + error.message;
        this.showNotification('BLE Connection Failed: ' + error.message, 'error');
        this.logError(error, 'web_bluetooth_connection_error');
      }
    } else {
      // iOS/Safari fallback - show instructions for Bluefy
      console.log('Web Bluetooth not supported, showing Bluefy instructions');
      this.showIOSBLEInstructions();
    }
  }

  showIOSBLEInstructions() {
    const isIOS = /iPad|iPhone|iPod/.test(navigator.userAgent);
    
    if (isIOS) {
      document.getElementById('ble-status').textContent = 'iOS detected - use Bluefy browser';
      
      // Show iOS-specific instructions with Bluefy recommendation
      const instructions = `
        <div class="ios-ble-instructions">
          <h4>📱 iOS BLE Setup Instructions</h4>
          <div class="setup-steps">
            <div class="step">
              <span class="step-number">1</span>
              <div class="step-content">
                <h5>Download Bluefy Browser</h5>
                <p>Free browser with Web Bluetooth support for iOS</p>
                <a href="https://apps.apple.com/app/bluefy/id1492822056" target="_blank" class="btn">Download Bluefy</a>
              </div>
            </div>
            <div class="step">
              <span class="step-number">2</span>
              <div class="step-content">
                <h5>Open This App in Bluefy</h5>
                <p>Copy this URL and open it in Bluefy browser:</p>
                <div class="url-copy">
                  <input type="text" id="app-url" value="${window.location.href}" readonly>
                  <button id="copy-url" class="btn-small">Copy</button>
                </div>
              </div>
            </div>
            <div class="step">
              <span class="step-number">3</span>
              <div class="step-content">
                <h5>Connect to macOS BLE Device</h5>
                <p>Make sure your macOS BLE Loudness Meter is running and advertising, then click "Connect BLE" in Bluefy</p>
              </div>
            </div>
          </div>
          <div class="limitation-note">
            <h5>⚠️ iOS Limitations</h5>
            <p>iOS browsers cannot access accelerometer or microphone. This app will work with BLE connection only.</p>
          </div>
          <div class="connection-options">
            <button id="connect-websocket" class="btn primary">Connect via WebSocket (Alternative)</button>
            <button id="simulate-ble" class="btn">Simulate BLE Connection</button>
          </div>
        </div>
      `;
      
      // Add instructions to the UI
      const existingInstructions = document.querySelector('.ios-ble-instructions');
      if (!existingInstructions) {
        document.querySelector('.controls').insertAdjacentHTML('afterend', instructions);
        
        // Add event listeners
        document.getElementById('simulate-ble').addEventListener('click', () => {
          this.simulateBLEConnection();
        });
        document.getElementById('connect-websocket').addEventListener('click', () => {
          this.connectWebSocket();
        });
        document.getElementById('copy-url').addEventListener('click', () => {
          const urlInput = document.getElementById('app-url');
          urlInput.select();
          document.execCommand('copy');
          this.showNotification('URL copied to clipboard!', 'success');
        });
      }
    } else {
      document.getElementById('ble-status').textContent = 'Web Bluetooth not supported';
    }
  }

  simulateBLEConnection() {
    // Simulate BLE connection for testing purposes
    this.bleCharacteristic = {
      writeValue: async (data) => {
        console.log('Simulated BLE write:', data);
        // In a real implementation, this would send data via WebSocket or other method
      }
    };
    
    document.getElementById('ble-status').textContent = 'Simulated BLE Connected';
    console.log('BLE simulation active');
  }

  connectWebSocket() {
    try {
      // For deployed app, we need to connect to your Mac's IP address
      // You'll need to update this with your actual Mac's IP address
      const hostname = window.location.hostname;
      let wsUrl;
      
      if (hostname === 'localhost' || hostname === '127.0.0.1') {
        wsUrl = 'ws://localhost:8080';
      } else {
        // For deployed app, prompt user for Mac's IP address
        const macIP = prompt('Enter your Mac\'s IP address (find it in System Preferences > Network):', '192.168.1.166');
        if (macIP) {
          wsUrl = `ws://${macIP}:8080`;
        } else {
          throw new Error('Mac IP address required for WebSocket connection');
        }
      }
      
      console.log('🔌 Attempting WebSocket connection to:', wsUrl);
      document.getElementById('ble-status').textContent = 'Connecting to WebSocket...';
      
      this.websocket = new WebSocket(wsUrl);
      
      this.websocket.onopen = () => {
        console.log('✅ WebSocket connected to simulator');
        document.getElementById('ble-status').textContent = 'Connected via WebSocket ✅';
        document.getElementById('ble-status').style.color = '#4ecdc4';
        
        // Show success notification
        this.showNotification('WebSocket Connected!', 'success');
        
        // Create a mock BLE characteristic that sends via WebSocket
        this.bleCharacteristic = {
          writeValue: async (data) => {
            if (this.websocket && this.websocket.readyState === WebSocket.OPEN) {
              this.websocket.send(JSON.stringify({
                type: 'ble_write',
                data: Array.from(data)
              }));
              console.log('📤 Sent BLE data via WebSocket:', data);
            } else {
              console.warn('⚠️ WebSocket not ready, cannot send data');
            }
          }
        };
      };
      
      this.websocket.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data);
          if (data.type === 'ble_ack') {
            console.log('📥 Received BLE acknowledgment:', data);
            this.showNotification(`Energy: ${data.energy}/255`, 'info');
          }
        } catch (error) {
          console.error('❌ Error parsing WebSocket message:', error);
        }
      };
      
      this.websocket.onerror = (error) => {
        console.error('❌ WebSocket error:', error);
        document.getElementById('ble-status').textContent = 'WebSocket connection failed ❌';
        document.getElementById('ble-status').style.color = '#ff6b6b';
        this.showNotification('WebSocket connection failed', 'error');
        this.logError(error, 'websocket_connection_error');
      };
      
      this.websocket.onclose = (event) => {
        console.log('🔌 WebSocket disconnected:', event.code, event.reason);
        document.getElementById('ble-status').textContent = 'WebSocket disconnected';
        document.getElementById('ble-status').style.color = '#ffc107';
        this.showNotification('WebSocket disconnected', 'warning');
      };
      
    } catch (error) {
      console.error('❌ WebSocket connection error:', error);
      document.getElementById('ble-status').textContent = 'WebSocket error: ' + error.message;
      document.getElementById('ble-status').style.color = '#ff6b6b';
      this.showNotification('WebSocket error: ' + error.message, 'error');
      this.logError(error, 'websocket_connection_setup_error');
    }
  }

  showNotification(message, type = 'info') {
    // Create notification element
    const notification = document.createElement('div');
    notification.className = `notification notification-${type}`;
    notification.textContent = message;
    
    // Style the notification
    notification.style.cssText = `
      position: fixed;
      top: 20px;
      right: 20px;
      padding: 12px 20px;
      border-radius: 8px;
      color: white;
      font-weight: bold;
      z-index: 1000;
      max-width: 300px;
      word-wrap: break-word;
      box-shadow: 0 4px 12px rgba(0,0,0,0.3);
      animation: slideIn 0.3s ease-out;
    `;
    
    // Set colors based on type
    switch (type) {
      case 'success':
        notification.style.background = 'linear-gradient(45deg, #4ecdc4, #44a08d)';
        break;
      case 'error':
        notification.style.background = 'linear-gradient(45deg, #ff6b6b, #ee5a24)';
        break;
      case 'warning':
        notification.style.background = 'linear-gradient(45deg, #ffc107, #f39c12)';
        break;
      default:
        notification.style.background = 'linear-gradient(45deg, #3498db, #2980b9)';
    }
    
    // Add to page
    document.body.appendChild(notification);
    
    // Remove after 3 seconds
    setTimeout(() => {
      notification.style.animation = 'slideOut 0.3s ease-in';
      setTimeout(() => {
        if (notification.parentNode) {
          notification.parentNode.removeChild(notification);
        }
      }, 300);
    }, 3000);
  }

  async sendBLECommand(energy) {
    if (!this.bleCharacteristic) return;

    const intensity = Math.floor(energy * 255);
    const command = new Uint8Array([intensity]);
    
    try {
      await this.bleCharacteristic.writeValue(command);
    } catch (error) {
      console.error('BLE write error:', error);
      this.logError(error, 'ble_write_error');
    }
  }

  loadAudio() {
    const fileInput = document.getElementById('audio-file');
    const file = fileInput.files[0];
    
    if (file) {
      const url = URL.createObjectURL(file);
      this.audioElement = new Audio(url);
      this.audioElement.loop = true;
      this.audioElement.play();
      console.log('Custom audio loaded:', file.name);
    }
  }

  loadSelectedTrack() {
    const trackSelect = document.getElementById('track-select');
    const selectedTrackId = trackSelect.value;
    
    if (!selectedTrackId) {
      alert('Please select a track first!');
      return;
    }
    
    const track = getTrackById(selectedTrackId);
    if (track) {
      // Try to use cached track first
      if (this.loadCachedTrack(selectedTrackId)) {
        this.updateTrackStatus(track.name);
        console.log('Loaded cached track:', track.name, 'by', track.artist);
      } else {
        // Fallback to loading from URL
        this.loadTrackFromUrl(track.url, track.name);
        console.log('Loaded track from URL:', track.name, 'by', track.artist);
      }
    }
  }

  loadRandomTrack() {
    const randomTrack = getRandomTrack();
    if (randomTrack) {
      // Try to use cached track first
      if (this.loadCachedTrack(randomTrack.id)) {
        this.updateTrackStatus(randomTrack.name);
        console.log('Loaded cached random track:', randomTrack.name, 'by', randomTrack.artist);
      } else {
        // Fallback to loading from URL
        this.loadTrackFromUrl(randomTrack.url, randomTrack.name);
        console.log('Loaded random track from URL:', randomTrack.name, 'by', randomTrack.artist);
      }
      
      // Update the dropdown to show the selected track
      const trackSelect = document.getElementById('track-select');
      trackSelect.value = randomTrack.id;
    }
  }

  async autoStartAudio() {
    const trackSelect = document.getElementById('track-select');
    const selectedTrackId = trackSelect.value;
    
    // If no track is selected, load a random one
    if (!selectedTrackId) {
      console.log('No track selected, loading random track...');
      this.loadRandomTrack();
    } else {
      // Load the selected track
      console.log('Loading selected track:', selectedTrackId);
      this.loadSelectedTrack();
    }
    
    // Wait a moment for the audio to load, then play
    await new Promise(resolve => setTimeout(resolve, 500));
    
    if (this.audioElement) {
      try {
        await this.audioElement.play();
        console.log('Audio auto-started successfully');
      } catch (error) {
        console.error('Failed to auto-start audio:', error);
      }
    }
  }

  loadTrackFromUrl(url, trackName) {
    try {
      // Stop current audio if playing
      if (this.audioElement) {
        this.audioElement.pause();
        this.audioElement = null;
      }
      
      // Create new audio element
      this.audioElement = new Audio(url);
      this.audioElement.loop = true;
      
      // Apply preselected speaker device if available
      this.applySelectedSpeakerDevice();
      
      // Add event listeners
      this.audioElement.addEventListener('canplaythrough', () => {
        console.log('Track ready to play:', trackName);
        this.audioElement.play().catch(e => {
          console.error('Error playing track:', e);
          alert('Error playing track. Please try again.');
        });
      });
      
      this.audioElement.addEventListener('error', (e) => {
        console.error('Error loading track:', e);
        alert('Error loading track. Please check your connection and try again.');
        this.logError(e, 'audio_track_loading_error');
      });
      
      // Update status
      this.updateTrackStatus(trackName);
      
    } catch (error) {
      console.error('Error creating audio element:', error);
      alert('Error loading track. Please try again.');
      this.logError(error, 'audio_element_creation_error');
    }
  }

  updateTrackStatus(trackName) {
    // Update the status display to show current track
    const statusElement = document.getElementById('mic-status');
    if (statusElement && trackName) {
      statusElement.textContent = `Playing: ${trackName}`;
      statusElement.style.color = '#4ecdc4';
    }
  }

  preloadDefaultTrack() {
    const defaultTrack = getTrackById(this.defaultTrackId);
    if (defaultTrack) {
      console.log('Preloading default track:', defaultTrack.name);
      this.updateTrackStatusIndicator('Preloading default track...');
      
      this.cacheTrack(defaultTrack)
        .then(() => {
          this.updateTrackStatusIndicator(`Default track ready: ${defaultTrack.name}`);
          console.log('Default track preloaded successfully');
          
          // Set the default track as selected in the dropdown
          setTimeout(() => {
            const trackSelect = document.getElementById('track-select');
            if (trackSelect) {
              trackSelect.value = this.defaultTrackId;
            }
          }, 100);
        })
        .catch((error) => {
          this.updateTrackStatusIndicator('Error preloading default track');
          console.error('Failed to preload default track:', error);
          this.logError(error, 'default_track_preload_error');
        });
    }
  }

  updateTrackStatusIndicator(message) {
    const statusElement = document.getElementById('track-status');
    if (statusElement) {
      statusElement.textContent = message;
    }
  }

  cacheTrack(track) {
    return new Promise((resolve, reject) => {
      if (this.cachedTracks.has(track.id)) {
        console.log('Track already cached:', track.name);
        resolve(this.cachedTracks.get(track.id));
        return;
      }

      console.log('Caching track:', track.name);
      const audio = new Audio();
      audio.crossOrigin = 'anonymous';
      
      audio.addEventListener('canplaythrough', () => {
        console.log('Track cached successfully:', track.name);
        this.cachedTracks.set(track.id, audio);
        resolve(audio);
      });
      
      audio.addEventListener('error', (e) => {
        console.error('Error caching track:', track.name, e);
        this.logError(e, 'track_caching_error');
        reject(e);
      });
      
      // Start loading the track
      audio.src = track.url;
      audio.load();
    });
  }

  loadCachedTrack(trackId) {
    const cachedAudio = this.cachedTracks.get(trackId);
    if (cachedAudio) {
      // Stop current audio if playing
      if (this.audioElement) {
        this.audioElement.pause();
        this.audioElement = null;
      }
      
      // Clone the cached audio to avoid conflicts
      this.audioElement = cachedAudio.cloneNode();
      this.audioElement.loop = true;
      
      // Apply preselected speaker device if available
      this.applySelectedSpeakerDevice();
      
      // Play the cached track
      this.audioElement.play().catch(e => {
        console.error('Error playing cached track:', e);
        alert('Error playing track. Please try again.');
        this.logError(e, 'cached_track_play_error');
      });
      
      return true;
    }
    return false;
  }

  async refreshAudioDevices() {
    try {
      console.log('Refreshing audio devices...');
      
      // Get available audio input devices (microphones)
      const devices = await navigator.mediaDevices.enumerateDevices();
      const audioInputs = devices.filter(device => device.kind === 'audioinput');
      const audioOutputs = devices.filter(device => device.kind === 'audiooutput');
      
      // Populate microphone dropdown
      const micSelect = document.getElementById('mic-device-select');
      micSelect.innerHTML = '<option value="">Select microphone...</option>';
      
      audioInputs.forEach((device, index) => {
        const option = document.createElement('option');
        option.value = device.deviceId;
        option.textContent = device.label || `Microphone ${index + 1}`;
        micSelect.appendChild(option);
      });
      
      // Preselect first microphone if available
      if (audioInputs.length > 0) {
        micSelect.value = audioInputs[0].deviceId;
        console.log('Preselected microphone:', audioInputs[0].label || 'Default Microphone');
      }
      
      // Populate speaker dropdown
      const speakerSelect = document.getElementById('speaker-device-select');
      speakerSelect.innerHTML = '<option value="">Select speaker...</option>';
      
      audioOutputs.forEach((device, index) => {
        const option = document.createElement('option');
        option.value = device.deviceId;
        option.textContent = device.label || `Speaker ${index + 1}`;
        speakerSelect.appendChild(option);
      });
      
      // Preselect first speaker if available
      if (audioOutputs.length > 0) {
        speakerSelect.value = audioOutputs[0].deviceId;
        console.log('Preselected speaker:', audioOutputs[0].label || 'Default Speaker');
      }
      
      console.log(`Found ${audioInputs.length} audio inputs and ${audioOutputs.length} audio outputs`);
      
    } catch (error) {
      console.error('Error refreshing audio devices:', error);
      this.logError(error, 'audio_device_refresh_error');
    }
  }

  async selectMicrophoneDevice(deviceId) {
    if (!deviceId) return;
    
    try {
      console.log('Selecting microphone device:', deviceId);
      
      // Stop current microphone if running
      if (this.microphone && this.microphone.mediaStream) {
        this.microphone.mediaStream.getTracks().forEach(track => track.stop());
      }
      
      // Request new microphone with specific device
      const constraints = {
        audio: {
          deviceId: { exact: deviceId }
        }
      };
      
      const stream = await navigator.mediaDevices.getUserMedia(constraints);
      console.log('New microphone stream obtained with device:', deviceId);
      
      // Update audio context and analyser if they exist
      if (this.audioContext && this.analyser) {
        this.microphone = this.audioContext.createMediaStreamSource(stream);
        this.microphone.connect(this.analyser);
        console.log('Microphone device changed successfully');
      }
      
    } catch (error) {
      console.error('Error selecting microphone device:', error);
      alert('Failed to select microphone device. Please try again.');
      this.logError(error, 'microphone_device_selection_error');
    }
  }

  async selectSpeakerDevice(deviceId) {
    if (!deviceId) return;
    
    try {
      console.log('Selecting speaker device:', deviceId);
      
      // For audio output, we need to use the setSinkId method if available
      if (this.audioElement && 'setSinkId' in this.audioElement) {
        await this.audioElement.setSinkId(deviceId);
        console.log('Speaker device changed successfully');
      } else {
        console.warn('setSinkId not supported in this browser');
        alert('Speaker device selection not supported in this browser.');
      }
      
    } catch (error) {
      console.error('Error selecting speaker device:', error);
      alert('Failed to select speaker device. Please try again.');
      this.logError(error, 'speaker_device_selection_error');
    }
  }

  async applySelectedSpeakerDevice() {
    const speakerSelect = document.getElementById('speaker-device-select');
    const selectedSpeakerId = speakerSelect ? speakerSelect.value : null;
    
    if (selectedSpeakerId && this.audioElement && 'setSinkId' in this.audioElement) {
      try {
        await this.audioElement.setSinkId(selectedSpeakerId);
        console.log('Applied preselected speaker device:', selectedSpeakerId);
      } catch (error) {
        console.warn('Failed to apply preselected speaker device:', error);
        this.logError(error, 'preselected_speaker_device_error');
      }
    }
  }
}

// Initialize the app and make it globally accessible for testing
window.shadowWarrior = new ShadowWarrior();
