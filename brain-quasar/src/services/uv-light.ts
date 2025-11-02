// UV Light control service for WiFi smart relay
import axios from 'axios';
import { eventBus, Events } from './event-bus';

interface UVLightConfig {
  enabled: boolean;
  relayUrl?: string; // URL of the WiFi relay (e.g., http://192.168.1.100)
  onEndpoint?: string; // Endpoint to turn on (e.g., /relay/on)
  offEndpoint?: string; // Endpoint to turn off (e.g., /relay/off)
  statusEndpoint?: string; // Endpoint to get status (e.g., /relay/status)
  timeout?: number; // Request timeout in ms
}

class UVLightService {
  private config: UVLightConfig = {
    enabled: false,
    relayUrl: 'http://192.168.1.100',
    onEndpoint: '/relay/on',
    offEndpoint: '/relay/off',
    statusEndpoint: '/relay/status',
    timeout: 5000,
  };

  private isOn = false;

  constructor() {
    // Listen for UV light commands from event bus
    eventBus.on(Events.UV_LIGHT_COMMAND, (command: { action: 'on' | 'off' | 'toggle' }) => {
      void this.handleCommand(command);
    });
  }

  async initialize(config: Partial<UVLightConfig>): Promise<void> {
    this.config = { ...this.config, ...config };

    if (!this.config.enabled) {
      console.log('UV light control is disabled');
      return;
    }

    try {
      // Test connection to relay
      await this.getStatus();
      console.log('UV light controller initialized');
    } catch (error) {
      console.error('Failed to initialize UV light controller:', error);
      console.warn('UV light control will be disabled');
      this.config.enabled = false;
    }
  }

  private async handleCommand(command: { action: 'on' | 'off' | 'toggle' }): Promise<void> {
    switch (command.action) {
      case 'on':
        await this.turnOn();
        break;
      case 'off':
        await this.turnOff();
        break;
      case 'toggle':
        await this.toggle();
        break;
    }
  }

  async turnOn(): Promise<void> {
    if (!this.config.enabled || !this.config.relayUrl) {
      console.warn('UV light control not enabled');
      return;
    }

    try {
      const url = `${this.config.relayUrl}${this.config.onEndpoint}`;
      const timeout = this.config.timeout ?? 5000;
      await axios.post(url, null, { timeout });
      this.isOn = true;
      console.log('UV light turned on');
    } catch (error) {
      console.error('Failed to turn on UV light:', error);
      throw error;
    }
  }

  async turnOff(): Promise<void> {
    if (!this.config.enabled || !this.config.relayUrl) {
      console.warn('UV light control not enabled');
      return;
    }

    try {
      const url = `${this.config.relayUrl}${this.config.offEndpoint}`;
      const timeout = this.config.timeout ?? 5000;
      await axios.post(url, null, { timeout });
      this.isOn = false;
      console.log('UV light turned off');
    } catch (error) {
      console.error('Failed to turn off UV light:', error);
      throw error;
    }
  }

  async toggle(): Promise<void> {
    if (this.isOn) {
      await this.turnOff();
    } else {
      await this.turnOn();
    }
  }

  async getStatus(): Promise<boolean> {
    if (!this.config.enabled || !this.config.relayUrl || !this.config.statusEndpoint) {
      return false;
    }

    try {
      const url = `${this.config.relayUrl}${this.config.statusEndpoint}`;
      const timeout = this.config.timeout ?? 5000;
      const response = await axios.get(url, { timeout });

      // Assume response contains { status: 'on' | 'off' } or { relay: true | false }
      const status = response.data.status === 'on' || response.data.relay === true;
      this.isOn = status;

      return status;
    } catch (error) {
      console.error('Failed to get UV light status:', error);
      throw error;
    }
  }

  getConfig(): UVLightConfig {
    return { ...this.config };
  }

  updateConfig(config: Partial<UVLightConfig>): void {
    this.config = { ...this.config, ...config };
    console.log('UV light config updated:', this.config);
  }

  isEnabled(): boolean {
    return this.config.enabled;
  }

  getState(): boolean {
    return this.isOn;
  }
}

// Singleton instance
export const uvLightService = new UVLightService();
