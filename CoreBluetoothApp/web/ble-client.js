class BLEPeripheralClient {
    constructor() {
        this.device = null;
        this.server = null;
        this.service = null;
        this.characteristic = null;
        this.isConnected = false;
        
        // Audio properties
        this.audioContext = null;
        this.analyser = null;
        this.microphoneStream = null;
        this.loudnessInterval = null;
        
        // BLE Service and Characteristic UUIDs (matching the macOS app)
        this.serviceUUID = '12345678-1234-1234-1234-123456789ABC';
        this.characteristicUUID = '87654321-4321-4321-4321-CBA987654321';
        
        this.init();
    }
    
    init() {
        this.setupEventListeners();
        this.log('BLE Peripheral Client initialized', 'info');
        this.checkWebBluetoothSupport();
    }
    
    setupEventListeners() {
        // Only add event listeners for elements that exist
        const connectBtn = document.getElementById('connect-btn');
        const disconnectBtn = document.getElementById('disconnect-btn');
        const startTrainingBtn = document.getElementById('start-training');
        const stopTrainingBtn = document.getElementById('stop-training');
        
        if (connectBtn) {
            connectBtn.addEventListener('click', () => this.connect());
        }
        if (disconnectBtn) {
            disconnectBtn.addEventListener('click', () => this.disconnect());
        }
        if (startTrainingBtn) {
            startTrainingBtn.addEventListener('click', () => this.startTraining());
        }
        if (stopTrainingBtn) {
            stopTrainingBtn.addEventListener('click', () => this.stopTraining());
        }
    }
    
    checkWebBluetoothSupport() {
        if (!navigator.bluetooth) {
            this.log('Web Bluetooth API not supported in this browser', 'error');
            this.showNotification('Web Bluetooth not supported. Please use Bluefy browser on iOS.', 'error');
            document.getElementById('connect-btn').disabled = true;
            return false;
        }
        
        this.log('Web Bluetooth API is supported', 'info');
        return true;
    }
    
    async connect() {
        if (!this.checkWebBluetoothSupport()) {
            return;
        }
        
        try {
            this.log('Starting BLE device discovery...', 'info');
            this.updateStatus('ble-status', 'Connecting...', 'connecting');
            document.getElementById('connect-btn').disabled = true;
            
            // Request device with specific service UUID
            this.device = await navigator.bluetooth.requestDevice({
                filters: [
                    { namePrefix: 'ShadowWarrior' },
                    { name: 'ShadowWarrior-BLE' }
                ],
                optionalServices: [this.serviceUUID]
            });
            
            this.log(`Found device: ${this.device.name}`, 'info');
            this.updateStatus('device-name', this.device.name, 'connected');
            
            // Listen for disconnection
            this.device.addEventListener('gattserverdisconnected', () => {
                this.onDisconnected();
            });
            
            // Connect to GATT server
            this.log('Connecting to GATT server...', 'info');
            this.server = await this.device.gatt.connect();
            
            this.log('Connected to GATT server', 'info');
            this.updateStatus('connection-state', 'Connected', 'connected');
            
            // Get the service
            this.log(`Getting service: ${this.serviceUUID}`, 'info');
            this.service = await this.server.getPrimaryService(this.serviceUUID);
            
            this.log('Service obtained', 'info');
            this.updateStatus('service-uuid', this.serviceUUID, 'connected');
            
            // Get the characteristic
            this.log(`Getting characteristic: ${this.characteristicUUID}`, 'info');
            this.characteristic = await this.service.getCharacteristic(this.characteristicUUID);
            
            this.log('Characteristic obtained', 'info');
            this.updateStatus('characteristic-uuid', this.characteristicUUID, 'connected');
            
            // Set up characteristic notifications
            await this.setupNotifications();
            
            this.isConnected = true;
            this.updateUI();
            this.showNotification('Successfully connected to BLE device!', 'success');
            
            // Show device info
            document.getElementById('device-info').style.display = 'block';
            document.getElementById('device-id').textContent = this.device.id;
            
        } catch (error) {
            this.log(`Connection failed: ${error.message}`, 'error');
            this.updateStatus('ble-status', 'Connection Failed', 'disconnected');
            this.showNotification(`Connection failed: ${error.message}`, 'error');
            document.getElementById('connect-btn').disabled = false;
        }
    }
    
    async setupNotifications() {
        try {
            // Start notifications
            await this.characteristic.startNotifications();
            this.log('Notifications started', 'info');
            
            // Listen for notifications
            this.characteristic.addEventListener('characteristicvaluechanged', (event) => {
                this.onCharacteristicValueChanged(event);
            });
            
        } catch (error) {
            this.log(`Failed to setup notifications: ${error.message}`, 'warning');
        }
    }
    
    onCharacteristicValueChanged(event) {
        const value = event.target.value;
        const data = new TextDecoder().decode(value);
        this.log(`Received notification: ${data}`, 'info');
        this.showNotification(`Received: ${data}`, 'success');
    }
    
    async readCharacteristic() {
        if (!this.characteristic) {
            this.log('No characteristic available', 'error');
            return;
        }
        
        try {
            this.log('Reading characteristic value...', 'info');
            const value = await this.characteristic.readValue();
            const data = new TextDecoder().decode(value);
            this.log(`Read value: ${data}`, 'info');
            this.showNotification(`Read: ${data}`, 'success');
        } catch (error) {
            this.log(`Read failed: ${error.message}`, 'error');
            this.showNotification(`Read failed: ${error.message}`, 'error');
        }
    }
    
    async sendMessage() {
        if (!this.characteristic) {
            this.log('No characteristic available', 'error');
            return;
        }
        
        const messageInput = document.getElementById('message-input');
        const message = messageInput.value.trim();
        
        if (!message) {
            this.log('No message to send', 'warning');
            return;
        }
        
        try {
            this.log(`Sending message: ${message}`, 'info');
            
            // Convert message to Uint8Array
            const data = new TextEncoder().encode(message);
            
            // Write to characteristic
            await this.characteristic.writeValue(data);
            
            this.log('Message sent successfully', 'info');
            this.showNotification(`Sent: ${message}`, 'success');
            
            // Clear input
            messageInput.value = '';
            
        } catch (error) {
            this.log(`Send failed: ${error.message}`, 'error');
            this.showNotification(`Send failed: ${error.message}`, 'error');
        }
    }
    
    async sendLoudnessLevel(level) {
        if (!this.characteristic) {
            return; // Silently fail if not connected
        }
        
        try {
            // Convert loudness level (0-1) to byte (0-255)
            const loudnessByte = Math.floor(Math.max(0, Math.min(1, level)) * 255);
            const data = new Uint8Array([loudnessByte]);
            
            // Write to characteristic
            await this.characteristic.writeValue(data);
            
            // Log occasionally to avoid spam
            if (Math.random() < 0.01) { // Log 1% of the time
                this.log(`Sent loudness level: ${level.toFixed(2)} (${loudnessByte})`, 'info');
            }
            
        } catch (error) {
            this.log(`Failed to send loudness level: ${error.message}`, 'error');
        }
    }
    
    async disconnect() {
        if (this.device && this.device.gatt.connected) {
            try {
                this.log('Disconnecting from device...', 'info');
                this.device.gatt.disconnect();
            } catch (error) {
                this.log(`Disconnect error: ${error.message}`, 'error');
            }
        }
        
        this.onDisconnected();
    }
    
    onDisconnected() {
        this.log('Device disconnected', 'warning');
        this.isConnected = false;
        
        // Stop microphone if running
        this.stopMicrophone();
        
        this.device = null;
        this.server = null;
        this.service = null;
        this.characteristic = null;
        
        this.updateUI();
        this.updateStatus('ble-status', 'Disconnected', 'disconnected');
        this.updateStatus('device-name', '-', 'disconnected');
        this.updateStatus('service-uuid', '-', 'disconnected');
        this.updateStatus('connection-state', '-', 'disconnected');
        this.updateStatus('sending-status', 'No', 'disconnected');
        
        const deviceInfo = document.getElementById('device-info');
        if (deviceInfo) deviceInfo.style.display = 'none';
        this.showNotification('Device disconnected', 'warning');
    }
    
    updateUI() {
        const connectBtn = document.getElementById('connect-btn');
        const disconnectBtn = document.getElementById('disconnect-btn');
        const startBtn = document.getElementById('start-training');
        const stopBtn = document.getElementById('stop-training');
        
        if (this.isConnected) {
            if (connectBtn) connectBtn.disabled = true;
            if (disconnectBtn) disconnectBtn.disabled = false;
            if (startBtn) startBtn.disabled = false;
            if (stopBtn) stopBtn.disabled = true;
        } else {
            if (connectBtn) connectBtn.disabled = false;
            if (disconnectBtn) disconnectBtn.disabled = true;
            if (startBtn) startBtn.disabled = true;
            if (stopBtn) stopBtn.disabled = true;
        }
    }
    
    updateStatus(elementId, text, type) {
        const element = document.getElementById(elementId);
        if (element) {
            element.textContent = text;
            element.className = `status-value status-${type}`;
        }
    }
    
    log(message, type = 'info') {
        const timestamp = new Date().toLocaleTimeString();
        const logContainer = document.getElementById('log-container');
        
        const logEntry = document.createElement('div');
        logEntry.className = `log-entry log-${type}`;
        logEntry.innerHTML = `
            <span class="log-timestamp">[${timestamp}]</span>
            <span class="log-${type}">${message}</span>
        `;
        
        logContainer.appendChild(logEntry);
        logContainer.scrollTop = logContainer.scrollHeight;
        
        // Keep only last 100 log entries
        while (logContainer.children.length > 100) {
            logContainer.removeChild(logContainer.firstChild);
        }
    }
    
    showNotification(message, type = 'info') {
        // Remove existing notifications
        const existingNotifications = document.querySelectorAll('.notification');
        existingNotifications.forEach(notification => {
            notification.remove();
        });
        
        const notification = document.createElement('div');
        notification.className = `notification notification-${type}`;
        notification.textContent = message;
        
        document.body.appendChild(notification);
        
        // Remove after 3 seconds
        setTimeout(() => {
            if (notification.parentNode) {
                notification.remove();
            }
        }, 3000);
    }
    
    startTraining() {
        if (!this.isConnected) {
            this.showNotification('Please connect to BLE device first', 'error');
            return;
        }
        
        this.log('Starting training mode...', 'info');
        this.updateStatus('sending-status', 'Yes', 'connected');
        
        // Enable/disable buttons
        const startBtn = document.getElementById('start-training');
        const stopBtn = document.getElementById('stop-training');
        if (startBtn) startBtn.disabled = true;
        if (stopBtn) stopBtn.disabled = false;
        
        // Start microphone and send loudness data
        this.startMicrophone();
    }
    
    stopTraining() {
        this.log('Stopping training mode...', 'info');
        this.updateStatus('sending-status', 'No', 'disconnected');
        
        // Enable/disable buttons
        const startBtn = document.getElementById('start-training');
        const stopBtn = document.getElementById('stop-training');
        if (startBtn) startBtn.disabled = false;
        if (stopBtn) stopBtn.disabled = true;
        
        // Stop microphone
        this.stopMicrophone();
    }
    
    async startMicrophone() {
        try {
            const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
            const audioContext = new (window.AudioContext || window.webkitAudioContext)();
            const analyser = audioContext.createAnalyser();
            const microphone = audioContext.createMediaStreamSource(stream);
            
            analyser.fftSize = 256;
            analyser.smoothingTimeConstant = 0.8;
            microphone.connect(analyser);
            
            this.audioContext = audioContext;
            this.analyser = analyser;
            this.microphoneStream = stream;
            
            this.log('Microphone started', 'info');
            this.startLoudnessMonitoring();
            
        } catch (error) {
            this.log('Failed to start microphone: ' + error.message, 'error');
            this.showNotification('Microphone access denied', 'error');
        }
    }
    
    stopMicrophone() {
        if (this.microphoneStream) {
            this.microphoneStream.getTracks().forEach(track => track.stop());
            this.microphoneStream = null;
        }
        if (this.audioContext) {
            this.audioContext.close();
            this.audioContext = null;
        }
        if (this.loudnessInterval) {
            clearInterval(this.loudnessInterval);
            this.loudnessInterval = null;
        }
        
        this.log('Microphone stopped', 'info');
    }
    
    startLoudnessMonitoring() {
        this.loudnessInterval = setInterval(() => {
            if (!this.analyser || !this.isConnected) return;
            
            const dataArray = new Uint8Array(this.analyser.frequencyBinCount);
            this.analyser.getByteFrequencyData(dataArray);
            
            // Calculate loudness level
            const sum = dataArray.reduce((acc, value) => acc + value, 0);
            const loudness = (sum / dataArray.length) / 255;
            
            // Update UI
            this.updateLoudnessMeter(loudness);
            
            // Send to BLE device
            this.sendLoudnessLevel(loudness);
            
        }, 100); // Update every 100ms
    }
    
    updateLoudnessMeter(level) {
        const micLevelDisplay = document.getElementById('mic-level-display');
        const micMeterFill = document.getElementById('mic-meter-fill');
        const combinedLevelDisplay = document.getElementById('combined-level-display');
        const combinedMeterFill = document.getElementById('combined-meter-fill');
        
        if (micLevelDisplay) {
            micLevelDisplay.textContent = level.toFixed(2);
        }
        if (micMeterFill) {
            micMeterFill.style.width = `${Math.min(100, level * 100)}%`;
        }
        if (combinedLevelDisplay) {
            combinedLevelDisplay.textContent = level.toFixed(2);
        }
        if (combinedMeterFill) {
            combinedMeterFill.style.width = `${Math.min(100, level * 100)}%`;
        }
        
        // Update audio level status
        this.updateStatus('audio-level', level.toFixed(2), 'connected');
    }
}

// Initialize the BLE client when the page loads
document.addEventListener('DOMContentLoaded', () => {
    window.bleClient = new BLEPeripheralClient();
});
