class BLEPeripheralClient {
    constructor() {
        this.device = null;
        this.server = null;
        this.service = null;
        this.characteristic = null;
        this.isConnected = false;
        
        // BLE Service and Characteristic UUIDs (matching the macOS app)
        this.serviceUUID = '12345678-1234-1234-1234-123456789abc';
        this.characteristicUUID = '87654321-4321-4321-4321-cba987654321';
        
        this.init();
    }
    
    init() {
        this.setupEventListeners();
        this.log('BLE Peripheral Client initialized', 'info');
        this.checkWebBluetoothSupport();
    }
    
    setupEventListeners() {
        document.getElementById('connect-btn').addEventListener('click', () => this.connect());
        document.getElementById('disconnect-btn').addEventListener('click', () => this.disconnect());
        document.getElementById('send-btn').addEventListener('click', () => this.sendMessage());
        document.getElementById('read-characteristic-btn').addEventListener('click', () => this.readCharacteristic());
        
        // Send message on Enter key
        document.getElementById('message-input').addEventListener('keypress', (e) => {
            if (e.key === 'Enter') {
                this.sendMessage();
            }
        });
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
        this.device = null;
        this.server = null;
        this.service = null;
        this.characteristic = null;
        
        this.updateUI();
        this.updateStatus('ble-status', 'Disconnected', 'disconnected');
        this.updateStatus('device-name', '-', 'disconnected');
        this.updateStatus('service-uuid', '-', 'disconnected');
        this.updateStatus('connection-state', '-', 'disconnected');
        
        document.getElementById('device-info').style.display = 'none';
        this.showNotification('Device disconnected', 'warning');
    }
    
    updateUI() {
        const connectBtn = document.getElementById('connect-btn');
        const disconnectBtn = document.getElementById('disconnect-btn');
        const sendBtn = document.getElementById('send-btn');
        const readBtn = document.getElementById('read-characteristic-btn');
        
        if (this.isConnected) {
            connectBtn.disabled = true;
            disconnectBtn.disabled = false;
            sendBtn.disabled = false;
            readBtn.disabled = false;
        } else {
            connectBtn.disabled = false;
            disconnectBtn.disabled = true;
            sendBtn.disabled = true;
            readBtn.disabled = true;
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
}

// Initialize the BLE client when the page loads
document.addEventListener('DOMContentLoaded', () => {
    window.bleClient = new BLEPeripheralClient();
});
