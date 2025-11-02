// LED controller service for BLE/WiFi communication
import { BleClient, type BleDevice } from '@capacitor-community/bluetooth-le';
import { eventBus, Events } from './event-bus';

// LED modes matching the state machine
export enum LEDMode {
  OFF = 'off',
  STANDBY = 'standby',
  PULSE = 'pulse',
  FIGHT = 'fight',
  VICTORY = 'victory',
}

interface LEDCommand {
  mode: LEDMode;
  intensity?: number; // 0-1
  color?: { r: number; g: number; b: number };
}

interface LEDControllerConfig {
  deviceId?: string;
  serviceUUID: string;
  characteristicUUID: string;
  autoReconnect: boolean;
}

class LEDControllerService {
  private config: LEDControllerConfig = {
    serviceUUID: '6E400001-B5A3-F393-E0A9-E50E24DCCA9E', // Shadow Warrior service UUID
    characteristicUUID: '6E400004-B5A3-F393-E0A9-E50E24DCCA9E', // LED control characteristic
    autoReconnect: true,
  };

  private device: BleDevice | null = null;
  private connected = false;
  private currentMode: LEDMode = LEDMode.OFF;

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
      await BleClient.requestLEScan(
        {
          services: [this.config.serviceUUID],
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

      // Send initial standby command
      await this.sendCommand({ mode: LEDMode.STANDBY });
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
      // Create command packet
      // Format: [mode (1 byte), intensity (1 byte), r (1 byte), g (1 byte), b (1 byte)]
      const modeMap: Record<LEDMode, number> = {
        [LEDMode.OFF]: 0,
        [LEDMode.STANDBY]: 1,
        [LEDMode.PULSE]: 2,
        [LEDMode.FIGHT]: 3,
        [LEDMode.VICTORY]: 4,
      };

      const intensity = Math.round((command.intensity || 0) * 255);
      const r = command.color?.r || 0;
      const g = command.color?.g || 0;
      const b = command.color?.b || 0;

      const data = new Uint8Array([
        modeMap[command.mode],
        intensity,
        r,
        g,
        b,
      ]);

      // Convert to DataView for BLE write
      const dataView = new DataView(data.buffer);

      await BleClient.write(
        this.device.deviceId,
        this.config.serviceUUID,
        this.config.characteristicUUID,
        dataView
      );

      this.currentMode = command.mode;
      console.log('Sent LED command:', command);
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
