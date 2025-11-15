// LED controller service for BLE/WiFi communication
import { BleClient, type BleDevice } from '@capacitor-community/bluetooth-le';
import { Capacitor } from '@capacitor/core';
import { ZeroConf } from 'capacitor-zeroconf';
import { eventBus, Events } from './event-bus';

// LED modes matching the Rust command handler
export enum LEDMode {
  IDLE = 'idle',
  ENERGY_BAR = 'energy_bar',
  ENERGY_PULSE = 'energy_pulse',
  BREATHING = 'breathing',
  ELECTRICITY = 'electricity',
}

// Controller types
export enum ControllerType {
  BLE = 'ble',
  WLED = 'wled',
}

interface LEDCommand {
  mode: LEDMode | string; // Accept both enum and string for flexibility
  percentage?: number; // 0-100 for energy_bar
}

interface LEDControllerConfig {
  deviceId?: string;
  serviceUUID: string;
  characteristicUUID: string; // RX characteristic for writing commands
  autoReconnect: boolean;
}

// WLED specific interfaces
export interface WLEDController {
  ip: string;
  ws: WebSocket | null;
  connected: boolean;
  name?: string;
}

interface WLEDState {
  on: boolean;
  bri: number; // brightness 0-255
  seg: WLEDSegment[];
}

interface WLEDSegment {
  fx: number; // effect ID
  sx?: number; // speed
  ix?: number; // intensity
  c1?: number; // custom parameter 1
  c2?: number; // custom parameter 2
  c3?: number; // custom parameter 3
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

  private controllers: Map<string, { type: ControllerType; device: BleDevice | WLEDController }> = new Map();
  private currentMode: LEDMode = LEDMode.IDLE;
  private readonly STORAGE_KEY = 'shadow-warrior-led-controllers';

  async initialize(): Promise<void> {
    try {
      await BleClient.initialize();
      console.log('LED controller BLE client initialized');

      // Load saved controllers from localStorage
      this.loadSavedControllers();

      // Listen for LED commands from event bus
      eventBus.on(Events.LED_COMMAND, (command: LEDCommand) => {
        void this.handleLEDCommand(command);
      });
    } catch (error) {
      console.error('Failed to initialize LED controller:', error);
      throw error;
    }
  }

  // Load saved controllers from localStorage
  private loadSavedControllers(): void {
    try {
      const saved = localStorage.getItem(this.STORAGE_KEY);
      console.log('Loading controllers from localStorage, key:', this.STORAGE_KEY, 'value:', saved);
      if (!saved) {
        console.log('No saved controllers found');
        return;
      }

      const savedControllers = JSON.parse(saved) as Array<{
        id: string;
        type: ControllerType;
        ip?: string;
        name?: string;
        deviceId?: string;
        deviceName?: string;
      }>;

      console.log('Loading saved controllers:', savedControllers.length, 'controllers from storage');

      for (const saved of savedControllers) {
        if (saved.type === ControllerType.WLED && saved.ip) {
          // Restore WLED controller
          const wledController: WLEDController = {
            ip: saved.ip,
            ws: null,
            connected: false,
            name: saved.name || `WLED ${saved.ip}`,
          };
          this.controllers.set(saved.id, { type: ControllerType.WLED, device: wledController });
          console.log('Restored WLED controller:', saved.id);
        } else if (saved.type === ControllerType.BLE && saved.deviceId) {
          // Restore BLE controller reference (will need to reconnect)
          const bleDevice: BleDevice = {
            deviceId: saved.deviceId,
            name: saved.deviceName || 'Unknown BLE Device',
          };
          this.controllers.set(saved.id, { type: ControllerType.BLE, device: bleDevice });
          console.log('Restored BLE controller reference:', saved.id);
        }
      }

      console.log('Restored %d controllers from storage', this.controllers.size);

      // Emit event for each restored controller to trigger UI updates
      for (const [id, { type }] of this.controllers.entries()) {
        eventBus.emit(Events.CONTROLLER_ADDED, { id, type });
      }
    } catch (error) {
      console.error('Failed to load saved controllers:', error);
    }
  }

  // Save controllers to localStorage
  private saveControllers(): void {
    try {
      const toSave = Array.from(this.controllers.entries()).map(([id, { type, device }]) => {
        if (type === ControllerType.WLED) {
          const wled = device as WLEDController;
          return {
            id,
            type,
            ip: wled.ip,
            name: wled.name,
          };
        } else {
          const ble = device as BleDevice;
          return {
            id,
            type,
            deviceId: ble.deviceId,
            deviceName: ble.name,
          };
        }
      });

      localStorage.setItem(this.STORAGE_KEY, JSON.stringify(toSave));
      console.log('Saved %d controllers to storage', toSave.length);
    } catch (error) {
      console.error('Failed to save controllers:', error);
    }
  }

  async scan(timeoutMs = 8000): Promise<{ type: ControllerType; device: BleDevice | WLEDController }[]> {
    const controllers: { type: ControllerType; device: BleDevice | WLEDController }[] = [];

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
          controllers.push({ type: ControllerType.BLE, device: result.device });
          console.log('Found BLE LED controller:', result.device.name);
        }
      );

      // Start mDNS discovery for WLED controllers (optional, may not work on all networks)
      console.log('Starting mDNS scan for WLED controllers...');
      console.log('Note: mDNS discovery may not work on all networks. Use manual IP entry if needed.');

      let mdnsWatching = false;
      const discoveredWledIPs = new Set<string>(); // Track unique IPs
      const MDNS_CONFIG = { type: '_wled._tcp', domain: 'local.' };

      try {
        await ZeroConf.watch(MDNS_CONFIG, (result) => {
          // First callback is often undefined - this confirms watch started successfully
          if (!result) {
            console.log('mDNS watch callback initialized (first callback is empty, this is normal)');
            return;
          }

          console.log('mDNS callback triggered with data:', JSON.stringify(result));

          // Check if result is valid
          if (typeof result !== 'object') {
            console.warn('Invalid mDNS result type:', typeof result);
            return;
          }

          if (result.action === 'added' || result.action === 'resolved') {
            const service = result.service;
            if (!service) {
              console.warn('mDNS result missing service data');
              return;
            }

            console.log('mDNS service discovered:', {
              name: service.name,
              action: result.action,
              ipv4: service.ipv4Addresses,
              ipv6: service.ipv6Addresses,
              port: service.port
            });

            // WLED services are named with 'wled' in them
            const wledIP = service.ipv4Addresses?.[0] || service.ipv6Addresses?.[0] || '';

            if (wledIP && !discoveredWledIPs.has(wledIP)) {
              discoveredWledIPs.add(wledIP);

              const wledController: WLEDController = {
                ip: wledIP,
                ws: null,
                connected: false,
                name: service.name || `WLED-${wledIP}`,
              };

              controllers.push({ type: ControllerType.WLED, device: wledController });
              console.log('✓ Added WLED controller:', wledController.name, 'at', wledController.ip);
            } else if (!wledIP) {
              console.warn('WLED service found but no IP address available:', service.name);
            } else {
              console.log('Duplicate WLED controller ignored:', wledIP);
            }
          }
        });
        mdnsWatching = true;
        console.log('mDNS watch started successfully');
      } catch (mdnsError) {
        console.warn('mDNS discovery not available or failed:', mdnsError);
        console.log('You can still add WLED controllers manually by IP address');
      }

      // Wait for scan timeout to allow mDNS responses to arrive
      console.log(`Waiting ${timeoutMs}ms for device responses...`);
      await new Promise((resolve) => setTimeout(resolve, timeoutMs));

      console.log('Stopping BLE scan...');
      await BleClient.stopLEScan();

      if (mdnsWatching) {
        try {
          console.log(`Unwatching mDNS... (found ${discoveredWledIPs.size} unique WLED IPs)`);
          await ZeroConf.unwatch(MDNS_CONFIG);
          console.log('mDNS unwatch complete');
        } catch (unwatchError) {
          console.warn('Error during mDNS unwatch:', unwatchError);
        }
      }

      console.log('Scan complete, found %d controllers', controllers.length);
      console.log('- BLE controllers:', controllers.filter(c => c.type === ControllerType.BLE).length);
      console.log('- WLED controllers:', controllers.filter(c => c.type === ControllerType.WLED).length);

      return controllers;
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

  // Add discovered controllers to the service
  addDiscoveredControllers(discoveredControllers: { type: ControllerType; device: BleDevice | WLEDController }[]): void {
    let added = false;
    for (const { type, device } of discoveredControllers) {
      let controllerId: string;
      if (type === ControllerType.BLE) {
        const bleDevice = device as BleDevice;
        controllerId = `ble-${bleDevice.deviceId}`;
      } else {
        const wledDevice = device as WLEDController;
        controllerId = `wled-${wledDevice.ip}`;
      }

      if (!this.controllers.has(controllerId)) {
        this.controllers.set(controllerId, { type, device });
        console.log('Added discovered controller:', controllerId);
        added = true;
      }
    }

    // Save to localStorage if any controllers were added
    if (added) {
      this.saveControllers();
    }
  }

  // Add WLED controller manually by IP address
  addWledController(ip: string): string {
    // Strip http:// or https:// prefix if present
    const cleanIp = ip.replace(/^https?:\/\//, '');
    const controllerId = `wled-${cleanIp}`;

    if (this.controllers.has(controllerId)) {
      throw new Error(`WLED controller at ${cleanIp} is already added`);
    }

    const wledController: WLEDController = {
      ip: cleanIp,
      ws: null,
      connected: false,
      name: `WLED ${cleanIp}`,
    };

    this.controllers.set(controllerId, { type: ControllerType.WLED, device: wledController });
    console.log('Added manual WLED controller:', controllerId);

    // Save to localStorage
    this.saveControllers();

    // Emit event for UI updates
    eventBus.emit(Events.CONTROLLER_ADDED, { id: controllerId, type: ControllerType.WLED });

    return controllerId;
  }

  async connect(controllerId: string): Promise<void> {
    const controller = this.controllers.get(controllerId);
    if (!controller) {
      throw new Error(`Controller ${controllerId} not found`);
    }

    try {
      if (controller.type === ControllerType.BLE) {
        const bleDevice = controller.device as BleDevice;
        await BleClient.connect(bleDevice.deviceId, () => {
          console.log('BLE LED controller disconnected:', bleDevice.deviceId);
          eventBus.emit(Events.CONTROLLER_DISCONNECTED, { id: controllerId, type: ControllerType.BLE });

          // Auto-reconnect if enabled
          if (this.config.autoReconnect) {
            setTimeout(() => {
              void this.connect(controllerId);
            }, 5000);
          }
        });

        console.log('Connected to BLE LED controller:', bleDevice.deviceId);
        eventBus.emit(Events.CONTROLLER_CONNECTED, { id: controllerId, type: ControllerType.BLE });

      } else if (controller.type === ControllerType.WLED) {
        const wledDevice = controller.device as WLEDController;
        // Always use ws:// for WLED (it doesn't support SSL)
        // WLED typically runs on port 80 by default
        const wsUrl = `ws://${wledDevice.ip}:80/ws`;
        console.log('Attempting to connect to WLED at:', wsUrl);

        wledDevice.ws = new WebSocket(wsUrl);

        await new Promise<void>((resolve, reject) => {
          if (!wledDevice.ws) return reject(new Error('WebSocket not created'));

          const connectionTimeout = setTimeout(() => {
            wledDevice.ws?.close();
            reject(new Error(`Connection timeout to WLED at ${wledDevice.ip}`));
          }, 5000); // 5 second timeout

          wledDevice.ws.onopen = () => {
            clearTimeout(connectionTimeout);
            wledDevice.connected = true;
            console.log('Connected to WLED controller:', wledDevice.ip);
            eventBus.emit(Events.CONTROLLER_CONNECTED, { id: controllerId, type: ControllerType.WLED });
            resolve();
          };

          wledDevice.ws.onerror = (error) => {
            clearTimeout(connectionTimeout);
            console.error('WLED WebSocket error:', error);
            reject(new Error(`Failed to connect to WLED at ${wledDevice.ip}. Make sure WLED is running and accessible.`));
          };

          wledDevice.ws.onclose = (event) => {
            clearTimeout(connectionTimeout);
            console.log('WLED controller disconnected:', wledDevice.ip, 'Code:', event.code, 'Reason:', event.reason);
            wledDevice.connected = false;
            wledDevice.ws = null;
            eventBus.emit(Events.CONTROLLER_DISCONNECTED, { id: controllerId, type: ControllerType.WLED });

            // Auto-reconnect if enabled and not a manual disconnect
            if (this.config.autoReconnect && event.code !== 1000) {
              setTimeout(() => {
                void this.connect(controllerId);
              }, 5000);
            }
          };
        });
      }

      // Send initial idle command
      await this.sendCommandToController(controllerId, { mode: LEDMode.IDLE });
    } catch (error) {
      console.error('Failed to connect to LED controller:', error);
      throw error;
    }
  }

  async disconnect(controllerId: string): Promise<void> {
    const controller = this.controllers.get(controllerId);
    if (!controller) {
      return;
    }

    try {
      if (controller.type === ControllerType.BLE) {
        const bleDevice = controller.device as BleDevice;
        await BleClient.disconnect(bleDevice.deviceId);
        console.log('Disconnected from BLE LED controller:', bleDevice.deviceId);
        eventBus.emit(Events.CONTROLLER_DISCONNECTED, { id: controllerId, type: ControllerType.BLE });
      } else if (controller.type === ControllerType.WLED) {
        const wledDevice = controller.device as WLEDController;
        if (wledDevice.ws) {
          wledDevice.ws.close();
          wledDevice.ws = null;
          wledDevice.connected = false;
          console.log('Disconnected from WLED controller:', wledDevice.ip);
          eventBus.emit(Events.CONTROLLER_DISCONNECTED, { id: controllerId, type: ControllerType.WLED });
        }
      }
    } catch (error) {
      console.error('Failed to disconnect from LED controller:', error);
    }
  }

  private async handleLEDCommand(command: LEDCommand): Promise<void> {
    // Send command to all connected controllers
    const promises = Array.from(this.controllers.entries())
      .filter(([, controller]) => {
        if (controller.type === ControllerType.BLE) {
          // For BLE, we don't track connection status in the device object
          return true; // Assume connected if in map
        } else if (controller.type === ControllerType.WLED) {
          return (controller.device as WLEDController).connected;
        }
        return false;
      })
      .map(([controllerId]) => this.sendCommandToController(controllerId, command));

    await Promise.allSettled(promises);
  }

  private async sendCommandToController(controllerId: string, command: LEDCommand): Promise<void> {
    const controller = this.controllers.get(controllerId);
    if (!controller) {
      return;
    }

    try {
      if (controller.type === ControllerType.BLE) {
        const bleDevice = controller.device as BleDevice;
        await this.sendBLECommand(bleDevice, command);
      } else if (controller.type === ControllerType.WLED) {
        const wledDevice = controller.device as WLEDController;
        this.sendWLEDCommand(wledDevice, command);
      }
    } catch (error) {
      console.error('Failed to send LED command to controller:', controllerId, error);
    }
  }

  private async sendBLECommand(device: BleDevice, command: LEDCommand): Promise<void> {
    // Create command string based on mode
    let commandString: string;

    // Normalize mode to string for comparison
    const mode = typeof command.mode === 'string' ? command.mode : command.mode;

    switch (mode) {
      case 'idle':
        commandString = 'idle';
        break;
      case 'energy_bar':
        commandString = `energy_bar ${command.percentage || 0}`;
        break;
      case 'energy_pulse':
        commandString = 'energy_pulse';
        break;
      case 'breathing':
        commandString = 'breathing';
        break;
      case 'electricity':
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
      device.deviceId,
      this.config.serviceUUID,
      this.config.characteristicUUID,
      dataView
    );

    this.currentMode = mode as LEDMode;
    console.log('Sent BLE LED command:', commandString, 'to', device.deviceId);
  }

  private sendWLEDCommand(wledDevice: WLEDController, command: LEDCommand): void {
    if (!wledDevice.ws || !wledDevice.connected) {
      console.warn('WLED controller not connected, ignoring command:', command);
      return;
    }

    // Map LEDMode to WLED effect
    let wledCommand: WLEDState;

    // Normalize mode to string for comparison
    const mode = typeof command.mode === 'string' ? command.mode : command.mode;
    const power = Math.round((command.percentage || 0) * 2.55);
    switch (mode) {
      case 'idle':
        // Dim breathing effect for idle
        wledCommand = {
          on: true,
          bri: 50,
          seg: [{ fx: 1, sx: 128, ix: 128 }] // Breathing effect, medium speed
        };
        break;
      case 'energy_bar':
        // Solid color with brightness based on percentage
        wledCommand = {
          on: true,
          bri: power, // 0-100 to 0-255
          seg: [{ fx: 83, ix: power }] // Solid effect
        };
        break;
      case 'energy_pulse':
        // Pulse effect for energy/victory
        wledCommand = {
          on: true,
          bri: 255,
          seg: [{ fx: 2, sx: 200, ix: 200 }] // Pulse effect, fast
        };
        break;
      case 'breathing':
        // Breathing effect for warming mode
        wledCommand = {
          on: true,
          bri: 255,
          seg: [{ fx: 1, sx: 150, ix: 150 }] // Breathing effect, medium-slow
        };
        break;
      case 'electricity':
        // Lightning/electricity effect for fight mode
        wledCommand = {
          on: true,
          bri: 255,
          seg: [{ fx: 43, sx: 220, ix: 255 }] // Lightning effect (fx 43), high intensity
        };
        break;
      default:
        console.error('Unknown LED mode:', command.mode);
        return;
    }

    wledDevice.ws.send(JSON.stringify(wledCommand));
    this.currentMode = mode as LEDMode;
    console.log('Sent WLED command:', wledCommand, 'to', wledDevice.ip);
  }

  isControllerConnected(controllerId: string): boolean {
    const controller = this.controllers.get(controllerId);
    if (!controller) {
      return false;
    }

    if (controller.type === ControllerType.WLED) {
      return (controller.device as WLEDController).connected;
    }

    // For BLE, we can't easily check connection status, assume connected if in map
    return true;
  }

  getConnectedControllers(): string[] {
    return Array.from(this.controllers.entries())
      .filter(([controllerId]) => this.isControllerConnected(controllerId))
      .map(([controllerId]) => controllerId);
  }

  getCurrentMode(): LEDMode {
    return this.currentMode;
  }

  getControllers(): { id: string; type: ControllerType; device: BleDevice | WLEDController }[] {
    const controllers = Array.from(this.controllers.entries()).map(([id, { type, device }]) => ({
      id,
      type,
      device,
    }));
    console.log('getControllers returning:', controllers.length, 'controllers');
    return controllers;
  }

  // Remove a controller
  async removeController(controllerId: string): Promise<void> {
    const controller = this.controllers.get(controllerId);
    if (!controller) {
      return;
    }

    // Disconnect first if connected
    await this.disconnect(controllerId);

    // Remove from map
    this.controllers.delete(controllerId);
    console.log('Removed controller:', controllerId);

    // Save to localStorage
    this.saveControllers();
  }
}

// Singleton instance
export const ledControllerService = new LEDControllerService();
