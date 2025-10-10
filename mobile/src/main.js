import './style.css'
import { KNOWN_TRACKS, getTrackById, getTracksByEnergy, getRandomTrack, getTrackStats } from './tracks.js'

class ShadowWarrior {
  constructor() {
    this.accelerometerData = { x: 0, y: 0, z: 0 };
    this.audioLevel = 0;
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
      audio: []
    };
    this.maxDataPoints = 100;
    
    // Audio scaling settings
    this.audioScale = 5.0;  // Multiplier to scale audio level
    
    // Accelerometer scaling settings
    this.accelScale = 2.0;  // Multiplier to scale accelerometer energy
    this.accelThreshold = 1.0;  // Minimum threshold for accelerometer
    
    // Track caching
    this.cachedTracks = new Map();
    this.defaultTrackId = 'war-is-coming';
    
    this.init();
  }

  init() {
    this.createUI();
    this.setupEventListeners();
    this.preloadDefaultTrack();
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
                <td id="accel-energy">0.00</td>
              </tr>
            </tbody>
          </table>
        </div>

        <div class="controls">
          <button id="start-btn" class="btn primary">Start Training</button>
          <button id="stop-btn" class="btn secondary" disabled>Stop Training</button>
          <button id="connect-ble" class="btn">Connect BLE</button>
          <button id="request-permission" class="btn" style="display: none;">Request Motion Permission</button>
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


        <div class="loudness-meter">
          <h3>Loudness Meter</h3>
          <div class="meter-container">
            <div class="meter-bar">
              <div class="meter-fill" id="meter-fill"></div>
              <div class="meter-zones">
                <div class="zone green"></div>
                <div class="zone yellow"></div>
                <div class="zone red"></div>
              </div>
            </div>
            <div class="meter-labels">
              <span>0</span>
              <span>0.3</span>
              <span>0.7</span>
              <span>1.0</span>
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
                <span id="accel-scale-value">2.0</span>
                <button id="accel-plus" class="btn-small">+</button>
              </div>
            </div>
            <div class="control-group">
              <label>Accel Threshold:</label>
              <div class="control-buttons">
                <button id="threshold-minus" class="btn-small">-</button>
                <span id="threshold-value">1.0</span>
                <button id="threshold-plus" class="btn-small">+</button>
              </div>
            </div>
          </div>
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
      </div>
    `;
  }

  setupEventListeners() {
    document.getElementById('start-btn').addEventListener('click', () => this.startTraining());
    document.getElementById('stop-btn').addEventListener('click', () => this.stopTraining());
    document.getElementById('connect-ble').addEventListener('click', () => this.connectBLE());
    document.getElementById('load-audio').addEventListener('click', () => this.loadAudio());
    document.getElementById('request-permission').addEventListener('click', () => this.requestMotionPermission());
    
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
    
    // Check if we need to show permission button for iOS
    this.checkPermissionRequirements();
  }

  async checkPermissionRequirements() {
    // Show permission button for iOS devices
    if (typeof DeviceMotionEvent !== 'undefined' && typeof DeviceMotionEvent.requestPermission === 'function') {
      document.getElementById('request-permission').style.display = 'block';
    }
  }

  async requestMotionPermission() {
    try {
      if (typeof DeviceMotionEvent.requestPermission === 'function') {
        const permission = await DeviceMotionEvent.requestPermission();
        console.log('Motion permission result:', permission);
        
        if (permission === 'granted') {
          document.getElementById('request-permission').style.display = 'none';
          document.getElementById('accel-status').textContent = 'Permission granted - ready to start';
        } else {
          document.getElementById('accel-status').textContent = 'Permission denied';
        }
      }
    } catch (error) {
      console.error('Permission request error:', error);
      document.getElementById('accel-status').textContent = 'Permission error: ' + error.message;
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

  enableTestMode() {
    // Simulate accelerometer data for desktop testing
    this.testMode = true;
    console.log('Test mode enabled - simulating accelerometer data');
    
    // Add test controls
    const testControls = `
      <div class="test-mode">
        <h4>Test Mode (Desktop)</h4>
        <p>Simulating accelerometer data for testing</p>
        <button id="simulate-motion" class="btn">Simulate Motion</button>
      </div>
    `;
    
    const existingTest = document.querySelector('.test-mode');
    if (!existingTest) {
      document.querySelector('.controls').insertAdjacentHTML('afterend', testControls);
      document.getElementById('simulate-motion').addEventListener('click', () => {
        this.simulateMotion();
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

  async startTraining() {
    this.isRunning = true;
    document.getElementById('start-btn').disabled = true;
    document.getElementById('stop-btn').disabled = false;

    await this.startAccelerometer();
    await this.startMicrophone();
    this.startDataProcessing();
  }

  stopTraining() {
    this.isRunning = false;
    document.getElementById('start-btn').disabled = false;
    document.getElementById('stop-btn').disabled = true;

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
        });

        await sensor.start();
        document.getElementById('accel-status').textContent = 'Active (Modern API)';
        console.log('Accelerometer started successfully');
        return;
      } catch (error) {
        console.error('Accelerometer start error:', error);
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
      }
    }
    
    // If both fail
    const isDesktop = !/Android|iPhone|iPad|iPod|BlackBerry|IEMobile|Opera Mini/i.test(navigator.userAgent);
    if (isDesktop) {
      document.getElementById('accel-status').textContent = 'Desktop - no accelerometer';
      console.log('Running on desktop - accelerometer not available');
      this.enableTestMode();
    } else {
      document.getElementById('accel-status').textContent = 'Not supported';
      console.error('No accelerometer API available');
    }
  }

  async startMicrophone() {
    try {
      console.log('Starting microphone...');
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
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
    }
  }

  startDataProcessing() {
    let lastAccelData = { x: 0, y: 0, z: 0 };
    let lastAudioLevel = 0;
    
    const processData = () => {
      if (!this.isRunning) return;

      // Store data for graphs
      this.addGraphData('accelX', this.accelerometerData.x);
      this.addGraphData('accelY', this.accelerometerData.y);
      this.addGraphData('accelZ', this.accelerometerData.z);
      this.addGraphData('audio', this.audioLevel);

      // Calculate accelerometer energy with enhanced sensitivity
      const rawAccelEnergy = Math.sqrt(
        this.accelerometerData.x ** 2 + 
        this.accelerometerData.y ** 2 + 
        this.accelerometerData.z ** 2
      );
      
      // Apply scaling and threshold for more responsive shaking detection
      const accelEnergy = Math.max(0, (rawAccelEnergy - this.accelThreshold) * this.accelScale);

      // Calculate audio level with enhanced sensitivity
      if (this.analyser) {
        const dataArray = new Uint8Array(this.analyser.frequencyBinCount);
        this.analyser.getByteFrequencyData(dataArray);
        const rawLevel = dataArray.reduce((sum, value) => sum + value, 0) / dataArray.length / 255;
        // Apply exponential scaling for more dramatic response to louder sounds
        this.audioLevel = Math.min(1, Math.pow(rawLevel * this.audioScale, 1.5));
      }

      // Combined energy with better balance between audio and accelerometer
      const normalizedAccel = Math.min(1, accelEnergy / 10); // Scale accelerometer to 0-1
      const combinedEnergy = Math.min(1, (normalizedAccel + this.audioLevel) / 2);

      // Log only on significant changes
      const accelChanged = Math.abs(this.accelerometerData.x - lastAccelData.x) > 0.1 ||
                          Math.abs(this.accelerometerData.y - lastAccelData.y) > 0.1 ||
                          Math.abs(this.accelerometerData.z - lastAccelData.z) > 0.1;
      
      const audioChanged = Math.abs(this.audioLevel - lastAudioLevel) > 0.05;
      
      if (accelChanged) {
        console.log('Accelerometer data changed:', this.accelerometerData);
        lastAccelData = { ...this.accelerometerData };
      }
      
      if (audioChanged) {
        console.log('Audio level changed:', this.audioLevel.toFixed(3));
        lastAudioLevel = this.audioLevel;
      }

      // Update UI
      document.getElementById('accel-x-value').textContent = this.accelerometerData.x.toFixed(2);
      document.getElementById('accel-y-value').textContent = this.accelerometerData.y.toFixed(2);
      document.getElementById('accel-z-value').textContent = this.accelerometerData.z.toFixed(2);
      document.getElementById('accel-energy').textContent = accelEnergy.toFixed(2);
      document.getElementById('audio-level').textContent = this.audioLevel.toFixed(2);
      document.getElementById('combined-energy').textContent = combinedEnergy.toFixed(2);

      // Update audio volume
      if (this.audioElement) {
        this.audioElement.volume = combinedEnergy;
      }

      // Update loudness meter
      this.updateLoudnessMeter(this.audioLevel);

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
    const percentage = Math.min(100, level * 100);
    
    // Set the fill width
    meterFill.style.width = `${percentage}%`;
    
    // Change color based on level
    if (level <= 0.3) {
      meterFill.style.background = 'linear-gradient(90deg, #4ecdc4, #44a08d)';
    } else if (level <= 0.7) {
      meterFill.style.background = 'linear-gradient(90deg, #f7b731, #f39c12)';
    } else {
      meterFill.style.background = 'linear-gradient(90deg, #ff6b6b, #ee5a24)';
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
    
    // Check if Web Bluetooth is supported (Android Chrome)
    if ('bluetooth' in navigator) {
      try {
        console.log('Using Web Bluetooth API');
        this.bleDevice = await navigator.bluetooth.requestDevice({
          filters: [{ namePrefix: 'LED' }],
          optionalServices: ['0000180f-0000-1000-8000-00805f9b34fb']
        });

        this.bleService = await this.bleDevice.gatt.connect();
        this.bleCharacteristic = await this.bleService.getCharacteristic('00002a19-0000-1000-8000-00805f9b34fb');
        
        document.getElementById('ble-status').textContent = 'Connected (Web Bluetooth)';
        console.log('Web Bluetooth connected successfully');
      } catch (error) {
        console.error('Web Bluetooth error:', error);
        document.getElementById('ble-status').textContent = 'Error: ' + error.message;
      }
    } else {
      // iOS/Safari fallback - show instructions or use alternative method
      console.log('Web Bluetooth not supported, using iOS fallback');
      this.showIOSBLEInstructions();
    }
  }

  showIOSBLEInstructions() {
    const isIOS = /iPad|iPhone|iPod/.test(navigator.userAgent);
    
    if (isIOS) {
      document.getElementById('ble-status').textContent = 'iOS detected - use companion app';
      
      // Show iOS-specific instructions
      const instructions = `
        <div class="ios-ble-instructions">
          <h4>iOS BLE Connection</h4>
          <p>Web Bluetooth is not supported on iOS Safari. Use one of these options:</p>
          <ul>
            <li>Use a companion iOS app</li>
            <li>Connect via WebSocket to a bridge app</li>
            <li>Use a different browser (Chrome on Android)</li>
          </ul>
          <div class="connection-options">
            <button id="simulate-ble" class="btn">Simulate BLE Connection</button>
            <button id="connect-websocket" class="btn">Connect via WebSocket</button>
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
      // Try to connect to a WebSocket bridge (you'll need to implement this)
      const wsUrl = 'ws://localhost:8080/ble-bridge'; // Change this to your bridge server
      
      this.websocket = new WebSocket(wsUrl);
      
      this.websocket.onopen = () => {
        console.log('WebSocket connected');
        document.getElementById('ble-status').textContent = 'Connected via WebSocket';
        
        // Create a mock BLE characteristic that sends via WebSocket
        this.bleCharacteristic = {
          writeValue: async (data) => {
            if (this.websocket && this.websocket.readyState === WebSocket.OPEN) {
              this.websocket.send(JSON.stringify({
                type: 'ble_write',
                data: Array.from(data)
              }));
              console.log('Sent BLE data via WebSocket:', data);
            }
          }
        };
      };
      
      this.websocket.onerror = (error) => {
        console.error('WebSocket error:', error);
        document.getElementById('ble-status').textContent = 'WebSocket connection failed';
      };
      
      this.websocket.onclose = () => {
        console.log('WebSocket disconnected');
        document.getElementById('ble-status').textContent = 'WebSocket disconnected';
      };
      
    } catch (error) {
      console.error('WebSocket connection error:', error);
      document.getElementById('ble-status').textContent = 'WebSocket error: ' + error.message;
    }
  }

  async sendBLECommand(energy) {
    if (!this.bleCharacteristic) return;

    const intensity = Math.floor(energy * 255);
    const command = new Uint8Array([intensity]);
    
    try {
      await this.bleCharacteristic.writeValue(command);
    } catch (error) {
      console.error('BLE write error:', error);
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
      });
      
      // Update status
      this.updateTrackStatus(trackName);
      
    } catch (error) {
      console.error('Error creating audio element:', error);
      alert('Error loading track. Please try again.');
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
      
      // Play the cached track
      this.audioElement.play().catch(e => {
        console.error('Error playing cached track:', e);
        alert('Error playing track. Please try again.');
      });
      
      return true;
    }
    return false;
  }
}

// Initialize the app
new ShadowWarrior();
