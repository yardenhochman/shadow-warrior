#!/usr/bin/env node

const WebSocket = require('ws');
const express = require('express');
const path = require('path');

// Create Express app
const app = express();
const PORT = 3001;

// Serve static files
app.use(express.static(path.join(__dirname, 'public')));

// Create WebSocket server
const wss = new WebSocket.Server({ port: 8080 });

console.log('🥋 Shadow Warrior WebSocket BLE Simulator');
console.log('==========================================');
console.log('');

// WebSocket connection handling
wss.on('connection', (ws) => {
  console.log('📱 Client connected via WebSocket');
  
  ws.on('message', (message) => {
    try {
      const data = JSON.parse(message);
      
      if (data.type === 'ble_write') {
        const energyLevel = data.data[0];
        console.log(`🎯 LED Energy Level: ${energyLevel}/255 (${(energyLevel/255*100).toFixed(1)}%)`);
        
        // Send acknowledgment back
        ws.send(JSON.stringify({
          type: 'ble_ack',
          energy: energyLevel,
          timestamp: Date.now()
        }));
      }
    } catch (error) {
      console.error('❌ Error parsing message:', error);
    }
  });
  
  ws.on('close', () => {
    console.log('📱 Client disconnected');
  });
  
  ws.on('error', (error) => {
    console.error('❌ WebSocket error:', error);
  });
});

// HTTP server for web interface
app.get('/', (req, res) => {
  res.send(`
    <!DOCTYPE html>
    <html>
    <head>
      <title>Shadow Warrior BLE Simulator</title>
      <style>
        body { 
          font-family: Arial, sans-serif; 
          max-width: 800px; 
          margin: 0 auto; 
          padding: 20px;
          background: #1a1a2e;
          color: white;
        }
        .status { 
          background: #2a2a3e; 
          padding: 20px; 
          border-radius: 10px; 
          margin: 20px 0;
        }
        .energy-display {
          font-size: 2em;
          color: #4ecdc4;
          text-align: center;
          margin: 20px 0;
        }
        .log {
          background: #000;
          padding: 15px;
          border-radius: 5px;
          font-family: monospace;
          max-height: 300px;
          overflow-y: auto;
        }
      </style>
    </head>
    <body>
      <h1>🥋 Shadow Warrior BLE Simulator</h1>
      
      <div class="status">
        <h3>📡 WebSocket Server Status</h3>
        <p>✅ Server running on ws://localhost:8080</p>
        <p>🌐 Web interface: http://localhost:3001</p>
      </div>
      
      <div class="energy-display" id="energy-display">
        Energy: 0/255 (0%)
      </div>
      
      <div class="status">
        <h3>📊 Connection Log</h3>
        <div class="log" id="log"></div>
      </div>
      
      <div class="status">
        <h3>📱 Testing Instructions</h3>
        <ol>
          <li>Open Bluefy on your iOS device</li>
          <li>Navigate to your Shadow Warrior app</li>
          <li>Click "Connect via WebSocket"</li>
          <li>Start training to see energy levels here</li>
        </ol>
      </div>
      
      <script>
        const ws = new WebSocket('ws://localhost:8080');
        const energyDisplay = document.getElementById('energy-display');
        const log = document.getElementById('log');
        
        ws.onopen = () => {
          addLog('✅ WebSocket connected');
        };
        
        ws.onmessage = (event) => {
          const data = JSON.parse(event.data);
          if (data.type === 'ble_ack') {
            const energy = data.energy;
            const percentage = (energy/255*100).toFixed(1);
            energyDisplay.textContent = \`Energy: \${energy}/255 (\${percentage}%)\`;
            addLog(\`🎯 Received energy level: \${energy}/255 (\${percentage}%)\`);
          }
        };
        
        ws.onclose = () => {
          addLog('❌ WebSocket disconnected');
        };
        
        function addLog(message) {
          const timestamp = new Date().toLocaleTimeString();
          log.innerHTML += \`[\${timestamp}] \${message}<br>\`;
          log.scrollTop = log.scrollHeight;
        }
      </script>
    </body>
    </html>
  `);
});

// Start HTTP server
app.listen(PORT, () => {
  console.log(`🌐 Web interface: http://localhost:${PORT}`);
  console.log(`📡 WebSocket server: ws://localhost:8080`);
  console.log('');
  console.log('📱 Testing Instructions:');
  console.log('1. Open Bluefy on your iOS device');
  console.log('2. Navigate to your Shadow Warrior app');
  console.log('3. Click "Connect via WebSocket"');
  console.log('4. Start training to see energy levels');
  console.log('');
  console.log('🛑 Press Ctrl+C to stop');
});

// Handle graceful shutdown
process.on('SIGINT', () => {
  console.log('\n🛑 Shutting down WebSocket simulator...');
  wss.close();
  process.exit(0);
});
