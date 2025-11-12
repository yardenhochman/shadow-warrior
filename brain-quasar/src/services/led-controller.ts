// LED controller service for BLE/WiFi communication
import { BleClient, type BleDevice } from '@capacitor-community/bluetooth-le';
import { Capacitor } from '@capacitor/core';
import { eventBus, Events } from './event-bus';

// LED modes matching the Rust command handler
export enum LEDMode {
  IDLE = 'idle',
  ENERGY_BAR = 'energy_bar',
  ENERGY_PULSE = 'energy_pulse',
  BREATHING = 'breathing',
  ELECTRICITY = 'electricity',
}

interface LEDCommand {
  mode: LEDMode;
  percentage?: number; // 0-100 for energy_bar
}

interface LEDControllerConfig {
  deviceId?: string;
  serviceUUID: string;
  characteristicUUID: string; // RX characteristic for writing commands
  autoReconnect: boolean;
}

class LEDControllerService {
  private config: LEDControllerConfig = {
    serviceUUID: 'd08d81bb-7270-45de-a475-5b52feb820b6', // Shadow Warrior service UUID
    characteristicUUID: '8f97424f-8c2f-4a86-9e53-92059ccb1559', // RX characteristic for writing commands
    autoReconnect: true,
  };

  private device: BleDevice | null = null;
  private connected = false;
  private currentMode: LEDMode = LEDMode.IDLE;

  async initialize(): Promise<void> {
    try {
      await BleClient.initialize();
      console.log('LED controller BLE client initialized');

      // Listen for LED commands from event bus
      eventBus.on(Events.LED_COMMAND, (command: LEDCommand) => {
        void this.handleLEDCommand(command);
      });
    } catch (error) {
      console.error('Failed to initialize LED controller:', error);
      throw error;
    }
  }

  async scan(timeoutMs = 5000): Promise<BleDevice[]> {
    const devices: BleDevice[] = [];

    try {
      // Initialize BLE client first
      // For Android 12+ (API 31+): androidNeverForLocation means we don't need location
      // For Android 11 and below (API 30-): Location permission AND location services must be enabled
      console.log('Initializing BLE client...');
      await BleClient.initialize({ androidNeverForLocation: true });

      console.log('Starting BLE scan...');
      console.log('Note: On Android 11 and below, ensure Location Services are enabled in Settings');

      await BleClient.requestLEScan(
        {
          namePrefix: 'ShadowLED',
          allowDuplicates: true,
        },
        (result) => {
          devices.push(result.device);
          console.log('Found LED controller:', result.device.name);
        }
      );

      // Wait for scan timeout
      await new Promise((resolve) => setTimeout(resolve, timeoutMs));

      await BleClient.stopLEScan();
      console.log('Scan complete, found %d devices', devices.length);

      return devices;
    } catch (error) {
      console.error('Failed to scan for LED controllers:', error);

      // Provide helpful error message for Android 11 and below
      if (Capacitor.getPlatform() === 'android') {
        const errorMessage = error instanceof Error ? error.message : String(error);
        const enhancedError = new Error(
          'Bluetooth scan failed. On Android 11 and below, make sure:\n' +
          '1. Location permission is granted\n' +
          '2. Location Services are turned ON in Settings\n' +
          '3. Bluetooth is enabled\n\n' +
          `Original error: ${errorMessage}`
        );
        throw enhancedError;
      }

      throw error;
    }
  }

  async connect(deviceId: string): Promise<void> {
    try {
      await BleClient.connect(deviceId, () => {
        console.log('LED controller disconnected');
        this.connected = false;
        this.device = null;

        // Auto-reconnect if enabled
        if (this.config.autoReconnect && this.config.deviceId) {
          setTimeout(() => {
            void this.connect(this.config.deviceId!);
          }, 5000);
        }
      });

      this.device = { deviceId } as BleDevice;
      this.config.deviceId = deviceId;
      this.connected = true;

      console.log('Connected to LED controller:', deviceId);

      // Send initial idle command
      await this.sendCommand({ mode: LEDMode.IDLE });
    } catch (error) {
      console.error('Failed to connect to LED controller:', error);
      throw error;
    }
  }

  async disconnect(): Promise<void> {
    if (!this.device) {
      return;
    }

    try {
      await BleClient.disconnect(this.device.deviceId);
      this.connected = false;
      this.device = null;
      console.log('Disconnected from LED controller');
    } catch (error) {
      console.error('Failed to disconnect from LED controller:', error);
    }
  }

  private async handleLEDCommand(command: LEDCommand): Promise<void> {
    if (!this.connected || !this.device) {
      console.warn('LED controller not connected, ignoring command:', command);
      return;
    }

    await this.sendCommand(command);
  }

  private async sendCommand(command: LEDCommand): Promise<void> {
    if (!this.device) {
      return;
    }

    try {
      // Create command string based on mode
      let commandString: string;

      switch (command.mode) {
        case LEDMode.IDLE:
          commandString = 'idle';
          break;
        case LEDMode.ENERGY_BAR:
          commandString = `energy_bar ${command.percentage || 0}`;
          break;
        case LEDMode.ENERGY_PULSE:
          commandString = 'energy_pulse';
          break;
        case LEDMode.BREATHING:
          commandString = 'breathing';
          break;
        case LEDMode.ELECTRICITY:
          commandString = 'electricity';
          break;
        default:
          console.error('Unknown LED mode:', command.mode);
          return;
      }

      // Convert string to UTF-8 bytes
      const encoder = new TextEncoder();
      const data = encoder.encode(commandString);

      // Convert to DataView for BLE write
      const dataView = new DataView(data.buffer);

      await BleClient.write(
        this.device.deviceId,
        this.config.serviceUUID,
        this.config.characteristicUUID,
        dataView
      );

      this.currentMode = command.mode;
      console.log('Sent LED command:', commandString);
    } catch (error) {
      console.error('Failed to send LED command:', error);
    }
  }

  isConnected(): boolean {
    return this.connected;
  }

  getCurrentMode(): LEDMode {
    return this.currentMode;
  }

  getDeviceId(): string | undefined {
    return this.config.deviceId;
  }
}

// Singleton instance
export const ledControllerService = new LEDControllerService();
