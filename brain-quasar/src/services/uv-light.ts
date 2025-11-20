// UV Light control service for WiFi smart relay
import axios from 'axios';
import { eventBus, Events } from './event-bus';

interface UVLightConfig {
  enabled: boolean;
  hosts: string[]; // List of UV smart plugs hosts
  timeout?: number; // Request timeout in ms
}

class UVLightService {
  private config: UVLightConfig = {
    enabled: false,
    hosts: [],
    timeout: 5000,
  };

  private isOn = false;

  constructor() {
    // Listen for UV light commands from event bus
    eventBus.on(Events.UV_LIGHT_COMMAND, (command: { action: 'on' | 'off'  }) => {
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

  private async handleCommand(command: { action: 'on' | 'off' }): Promise<void> {
    if (!this.config.enabled) {
      console.warn('UV light control not enabled');
      return;
    }
    const params = new URLSearchParams();
    params.append('cmnd', `Power ${command.action.toUpperCase()}`);
    const promises = [];
    for (const host of this.config.hosts) {
      promises.push(fetch(`http://${host}/cm?${params}`));
    }
    await Promise.all(promises);
  }


  async getStatus(): Promise<Array<boolean>> {
    if (!this.config.enabled) {
      return [];
    }

    try {
      const statusPromises = Promise.all(this.config.hosts.map(async (host) => {
        const response = await axios.get(`http://${host}/cm?cmnd=Power`, {
          timeout: this.config.timeout ?? 5000,
        });
        const powerState = response.data?.Power;
        return powerState === 'ON';
      }));

      const results = await statusPromises;

      return results;
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

  getHosts(): string[] {
    return [...this.config.hosts];
  }

  addHost(host: string): void {
    // Clean up the host address
    const cleanHost = host.replace(/^https?:\/\//, '').trim();
    
    if (!cleanHost) {
      throw new Error('Invalid host address');
    }

    // Check if already exists
    if (this.config.hosts.includes(cleanHost)) {
      throw new Error(`UV light host ${cleanHost} is already added`);
    }

    this.config.hosts.push(cleanHost);
    console.log(`Added UV light host: ${cleanHost}`);
    
    // Emit event for UI updates
    eventBus.emit('uv-light:host-added', { host: cleanHost });
  }

  removeHost(host: string): void {
    const index = this.config.hosts.indexOf(host);
    if (index === -1) {
      throw new Error(`UV light host ${host} not found`);
    }

    this.config.hosts.splice(index, 1);
    console.log(`Removed UV light host: ${host}`);
    
    // Emit event for UI updates
    eventBus.emit('uv-light:host-removed', { host });
  }
}

// Singleton instance
export const uvLightService = new UVLightService();
