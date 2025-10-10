import './style.css'

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
    
    // Loudness curve settings
    this.loudnessCurve = {
      sensitivity: 1.0,  // Multiplier for sensitivity
      threshold: 0.01,   // Minimum threshold
      maxLevel: 1.0      // Maximum level
    };
    
    this.init();
  }

  init() {
    this.createUI();
    this.setupEventListeners();
  }

  createUI() {
    document.querySelector('#app').innerHTML = `
      <div class="container">
        <h1>🥋 Shadow Warrior</h1>
        <div class="status-panel">
          <div class="status-item">
            <label>Accelerometer:</label>
            <span id="accel-status">Not started</span>
          </div>
          <div class="status-item">
            <label>Microphone:</label>
            <span id="mic-status">Not started</span>
          </div>
          <div class="status-item">
            <label>BLE:</label>
            <span id="ble-status">Not connected</span>
          </div>
        </div>
        
        <div class="data-panel">
          <div class="data-item">
            <label>Accelerometer Energy:</label>
            <div class="value" id="accel-energy">0.00</div>
          </div>
          <div class="data-item">
            <label>Audio Level:</label>
            <div class="value" id="audio-level">0.00</div>
          </div>
          <div class="data-item">
            <label>Combined Energy:</label>
            <div class="value" id="combined-energy">0.00</div>
          </div>
        </div>

        <div class="controls">
          <button id="start-btn" class="btn primary">Start Training</button>
          <button id="stop-btn" class="btn secondary" disabled>Stop Training</button>
          <button id="connect-ble" class="btn">Connect BLE</button>
          <button id="request-permission" class="btn" style="display: none;">Request Motion Permission</button>
        </div>

        <div class="audio-controls">
          <input type="file" id="audio-file" accept="audio/*" />
          <button id="load-audio" class="btn">Load Audio</button>
        </div>

        <div class="led-preview">
          <h3>LED Matrix Preview</h3>
          <div id="led-grid"></div>
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
          <div class="loudness-controls">
            <div class="control-group">
              <label>Sensitivity:</label>
              <div class="control-buttons">
                <button id="sens-minus" class="btn-small">-</button>
                <span id="sens-value">1.0</span>
                <button id="sens-plus" class="btn-small">+</button>
              </div>
            </div>
            <div class="control-group">
              <label>Threshold:</label>
              <div class="control-buttons">
                <button id="thresh-minus" class="btn-small">-</button>
                <span id="thresh-value">0.01</span>
                <button id="thresh-plus" class="btn-small">+</button>
              </div>
            </div>
          </div>
        </div>

        <div class="debug-graphs">
          <h3>Debug Graphs</h3>
          <div class="graph-container">
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
            <div class="graph">
              <h4>Audio Level</h4>
              <canvas id="audio-graph" width="300" height="100"></canvas>
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
    
    // Loudness curve controls
    document.getElementById('sens-minus').addEventListener('click', () => this.adjustSensitivity(-0.1));
    document.getElementById('sens-plus').addEventListener('click', () => this.adjustSensitivity(0.1));
    document.getElementById('thresh-minus').addEventListener('click', () => this.adjustThreshold(-0.005));
    document.getElementById('thresh-plus').addEventListener('click', () => this.adjustThreshold(0.005));
    
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

  adjustSensitivity(delta) {
    this.loudnessCurve.sensitivity = Math.max(0.1, Math.min(3.0, this.loudnessCurve.sensitivity + delta));
    document.getElementById('sens-value').textContent = this.loudnessCurve.sensitivity.toFixed(1);
    console.log('Sensitivity adjusted to:', this.loudnessCurve.sensitivity);
  }

  adjustThreshold(delta) {
    this.loudnessCurve.threshold = Math.max(0.001, Math.min(0.1, this.loudnessCurve.threshold + delta));
    document.getElementById('thresh-value').textContent = this.loudnessCurve.threshold.toFixed(3);
    console.log('Threshold adjusted to:', this.loudnessCurve.threshold);
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
          console.log('Accelerometer reading:', this.accelerometerData);
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
            console.log('DeviceMotion reading:', this.accelerometerData);
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
    document.getElementById('accel-status').textContent = 'Not supported';
    console.error('No accelerometer API available');
  }

  async startMicrophone() {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      this.audioContext = new (window.AudioContext || window.webkitAudioContext)();
      this.analyser = this.audioContext.createAnalyser();
      this.microphone = this.audioContext.createMediaStreamSource(stream);
      
      this.analyser.fftSize = 256;
      this.microphone.connect(this.analyser);
      
      document.getElementById('mic-status').textContent = 'Active';
    } catch (error) {
      document.getElementById('mic-status').textContent = 'Error: ' + error.message;
    }
  }

  startDataProcessing() {
    const processData = () => {
      if (!this.isRunning) return;

      // Debug logging
      console.log('Accelerometer data:', this.accelerometerData);
      console.log('Audio level:', this.audioLevel);

      // Store data for graphs
      this.addGraphData('accelX', this.accelerometerData.x);
      this.addGraphData('accelY', this.accelerometerData.y);
      this.addGraphData('accelZ', this.accelerometerData.z);
      this.addGraphData('audio', this.audioLevel);

      // Calculate accelerometer energy
      const accelEnergy = Math.sqrt(
        this.accelerometerData.x ** 2 + 
        this.accelerometerData.y ** 2 + 
        this.accelerometerData.z ** 2
      );

      // Calculate audio level with logarithmic scaling
      if (this.analyser) {
        const dataArray = new Uint8Array(this.analyser.frequencyBinCount);
        this.analyser.getByteFrequencyData(dataArray);
        
        // Calculate RMS (Root Mean Square) for better audio level representation
        const sum = dataArray.reduce((sum, value) => sum + (value * value), 0);
        const rms = Math.sqrt(sum / dataArray.length) / 255;
        
        // Apply logarithmic scaling and loudness curve
        const logLevel = Math.log10(Math.max(this.loudnessCurve.threshold, rms * this.loudnessCurve.sensitivity));
        const maxLog = Math.log10(this.loudnessCurve.maxLevel);
        this.audioLevel = Math.min(1, Math.max(0, logLevel / maxLog));
      }

      // Combined energy (normalized 0-1)
      const combinedEnergy = Math.min(1, (accelEnergy / 20) + this.audioLevel);

      // Update UI
      document.getElementById('accel-energy').textContent = accelEnergy.toFixed(2);
      document.getElementById('audio-level').textContent = this.audioLevel.toFixed(2);
      document.getElementById('combined-energy').textContent = combinedEnergy.toFixed(2);

      // Update audio volume
      if (this.audioElement) {
        this.audioElement.volume = combinedEnergy;
      }

      // Update LED matrix
      this.updateLEDMatrix(combinedEnergy);

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
    }
  }
}

// Initialize the app
new ShadowWarrior();
