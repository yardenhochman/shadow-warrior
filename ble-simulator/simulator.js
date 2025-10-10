#!/usr/bin/env node

const bleno = require('@abandonware/bleno');

// Shadow Warrior LED Service UUID (custom)
const LED_SERVICE_UUID = '12345678-1234-1234-1234-123456789abc';
const LED_CHARACTERISTIC_UUID = '12345678-1234-1234-1234-123456789abd';

// LED Control Characteristic
class LEDControlCharacteristic extends bleno.Characteristic {
  constructor() {
    super({
      uuid: LED_CHARACTERISTIC_UUID,
      properties: ['write', 'notify'],
      descriptors: [
        new bleno.Descriptor({
          uuid: '2901',
          value: 'LED Control - Send energy level (0-255)'
        })
      ]
    });
    
    this._value = Buffer.alloc(1, 0);
    this._updateValueCallback = null;
  }

  onWriteRequest(data, offset, withoutResponse, callback) {
    if (data.length === 1) {
      const energyLevel = data.readUInt8(0);
      console.log(`🎯 LED Energy Level: ${energyLevel}/255 (${(energyLevel/255*100).toFixed(1)}%)`);
      
      // Simulate LED response
      this._value = data;
      
      if (this._updateValueCallback) {
        this._updateValueCallback(this._value);
      }
      
      callback(this.RESULT_SUCCESS);
    } else {
      callback(this.RESULT_INVALID_ATTRIBUTE_LENGTH);
    }
  }

  onSubscribe(maxValueSize, updateValueCallback) {
    console.log('📱 Client subscribed to LED updates');
    this._updateValueCallback = updateValueCallback;
  }

  onUnsubscribe() {
    console.log('📱 Client unsubscribed from LED updates');
    this._updateValueCallback = null;
  }
}

// Main LED Service
class LEDService extends bleno.PrimaryService {
  constructor() {
    super({
      uuid: LED_SERVICE_UUID,
      characteristics: [
        new LEDControlCharacteristic()
      ]
    });
  }
}

// BLE Peripheral Setup
bleno.on('stateChange', (state) => {
  console.log(`🔵 BLE State: ${state}`);
  
  if (state === 'poweredOn') {
    console.log('🚀 Starting Shadow Warrior LED Simulator...');
    
    bleno.startAdvertising('Shadow Warrior LED', [LED_SERVICE_UUID], (error) => {
      if (error) {
        console.error('❌ Advertising error:', error);
      } else {
        console.log('✅ Advertising as "Shadow Warrior LED"');
        console.log('📱 Ready for Bluefy connection!');
        console.log('');
        console.log('Instructions:');
        console.log('1. Open Bluefy on your iOS device');
        console.log('2. Navigate to your Shadow Warrior app');
        console.log('3. Click "Connect BLE"');
        console.log('4. Select "Shadow Warrior LED"');
        console.log('5. Start training to see energy levels!');
        console.log('');
      }
    });
  } else {
    console.log('❌ Bluetooth not available');
    bleno.stopAdvertising();
  }
});

bleno.on('advertisingStart', (error) => {
  if (error) {
    console.error('❌ Advertising start error:', error);
  } else {
    console.log('📡 Advertising started successfully');
  }
});

bleno.on('advertisingStop', () => {
  console.log('📡 Advertising stopped');
});

bleno.on('servicesSet', (error) => {
  if (error) {
    console.error('❌ Services set error:', error);
  } else {
    console.log('✅ LED Service registered');
  }
});

bleno.on('accept', (clientAddress) => {
  console.log(`📱 Client connected: ${clientAddress}`);
});

bleno.on('disconnect', (clientAddress) => {
  console.log(`📱 Client disconnected: ${clientAddress}`);
});

// Handle graceful shutdown
process.on('SIGINT', () => {
  console.log('\n🛑 Shutting down BLE simulator...');
  bleno.stopAdvertising();
  bleno.disconnect();
  process.exit(0);
});

console.log('🥋 Shadow Warrior BLE Simulator');
console.log('================================');
console.log('Make sure Bluetooth is enabled on your Mac');
console.log('This will simulate a LED device for testing with Bluefy');
console.log('');
