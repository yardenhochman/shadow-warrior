// LED controller service for WLED WiFi communication
import { ZeroConf } from 'capacitor-zeroconf';
import { eventBus, Events, type LEDCommandPayload } from './event-bus';
import { RealtimeEffectService, RealtimeEffectMode } from './effects/realtime-effect-service';
import { ArenaState } from 'src/types/state-machine';

// WLED controller interface
export interface WLEDController {
  ip: string;
  name?: string;
}

class LEDControllerService {
  private controllers: Map<string, WLEDController> = new Map();
  private readonly STORAGE_KEY = 'shadow-warrior-led-controllers';
  private realtimeEffectService: RealtimeEffectService | null = null;

  initialize(): void {
    try {
      console.log('LED controller service initialized');

      // Load saved controllers from localStorage
      this.loadSavedControllers();

      // Listen for LED commands from event bus
      eventBus.on(Events.LED_COMMAND, (payload: LEDCommandPayload) => {
        void this.handleLEDCommand(payload);
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
        ip: string;
        name?: string;
      }>;

      console.log('Loading saved controllers:', savedControllers.length, 'controllers from storage');

      for (const saved of savedControllers) {
        if (saved.ip) {
          // Restore WLED controller
          const wledController: WLEDController = {
            ip: saved.ip,
            name: saved.name || `WLED ${saved.ip}`,
          };
          this.controllers.set(saved.id, wledController);
          console.log('Restored WLED controller:', saved.id);
        }
      }

      console.log('Restored %d controllers from storage', this.controllers.size);

      // Create realtime effect service with restored controllers
      this.initializeRealtimeEffectService();

      // Emit event for each restored controller to trigger UI updates
      for (const id of this.controllers.keys()) {
        eventBus.emit(Events.CONTROLLER_ADDED, { id, type: 'wled' });
      }
    } catch (error) {
      console.error('Failed to load saved controllers:', error);
    }
  }

  // Save controllers to localStorage
  private saveControllers(): void {
    try {
      const toSave = Array.from(this.controllers.entries()).map(([id, wled]) => ({
        id,
        ip: wled.ip,
        name: wled.name,
      }));

      localStorage.setItem(this.STORAGE_KEY, JSON.stringify(toSave));
      console.log('Saved %d controllers to storage', toSave.length);
    } catch (error) {
      console.error('Failed to save controllers:', error);
    }
  }

  async scan(timeoutMs = 8000): Promise<WLEDController[]> {
    const controllers: WLEDController[] = [];

    try {
      // Start mDNS discovery for WLED controllers
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
                name: service.name || `WLED-${wledIP}`,
              };

              controllers.push(wledController);
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

      if (mdnsWatching) {
        try {
          console.log(`Unwatching mDNS... (found ${discoveredWledIPs.size} unique WLED IPs)`);
          await ZeroConf.unwatch(MDNS_CONFIG);
          console.log('mDNS unwatch complete');
        } catch (unwatchError) {
          console.warn('Error during mDNS unwatch:', unwatchError);
        }
      }

      console.log('Scan complete, found %d WLED controllers', controllers.length);

      return controllers;
    } catch (error) {
      console.error('Failed to scan for WLED controllers:', error);
      throw error;
    }
  }

  // Add discovered controllers to the service
  addDiscoveredControllers(discoveredControllers: WLEDController[]): void {
    let added = false;
    for (const device of discoveredControllers) {
      const controllerId = `wled-${device.ip}`;

      if (!this.controllers.has(controllerId)) {
        this.controllers.set(controllerId, device);
        console.log('Added discovered controller:', controllerId);
        added = true;
      }
    }

    // Save to localStorage if any controllers were added
    if (added) {
      this.saveControllers();
      this.initializeRealtimeEffectService();
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
      name: `WLED ${cleanIp}`,
    };

    this.controllers.set(controllerId, wledController);
    console.log('Added manual WLED controller:', controllerId);

    // Save to localStorage
    this.saveControllers();

    // Reinitialize effect service with new controller
    this.initializeRealtimeEffectService();

    // Emit event for UI updates
    eventBus.emit(Events.CONTROLLER_ADDED, { id: controllerId, type: 'wled' });

    return controllerId;
  }



  private async handleLEDCommand(payload: LEDCommandPayload): Promise<void> {
    console.log('LED command received:', payload.arenaState, 'trigger:', payload.trigger, 'controllers:', this.controllers.size);

    // Send command to all controllers (not just connected ones, since UDP works independently)
    const commands = Array.from(this.controllers.entries())
      .map(([controllerId, controller]) => {
        console.log(`Sending LED command to controller ${controllerId} (${controller.ip})`);
        return this.sendCommandToController(controllerId, payload);
      });

    await Promise.allSettled(commands);
  }

  private async sendCommandToController(controllerId: string, payload: LEDCommandPayload): Promise<void> {
    const controller = this.controllers.get(controllerId);
    if (!controller) {
      return;
    }

    try {
      await this.sendWLEDCommand(payload);
    } catch (error) {
      console.error('Failed to send LED command to controller:', controllerId, error);
    }
  }

  private async sendWLEDCommand(payload: LEDCommandPayload): Promise<void> {
    console.log(`sendWLEDCommand state: ${payload.arenaState}`);

    try {
      // Map arena state to realtime effect mode
      switch (payload.arenaState) {
        case ArenaState.IDLE:
          // Stop realtime effects for this controller
          if (this.realtimeEffectService?.isRunning()) {
            await this.realtimeEffectService.clearStrip();
            await this.realtimeEffectService.stop();
          }
          // Set WLED to breathing effect (red)
          await this.setWLEDEffect({
            on: true,
            bri: 40,
            seg: [{
              col: [[255, 0, 0], [0, 0, 0], [86, 68, 0]], // Red color
              pal: 4,
              fx: 2, // Breathing effect ID
              sx: 40, // Speed (default)
              ix: 128, // Intensity (default)
            }]
          });
          break;

        case ArenaState.COOLDOWN:
        case ArenaState.SUSPENDED:
          // Stop realtime effects and turn off LEDs
          if (this.realtimeEffectService?.isRunning()) {
            await this.realtimeEffectService.clearStrip();
            await this.realtimeEffectService.stop();
          }
          // Turn off LEDs explicitly
          await this.setWLEDEffect({ on: false });
          break;

        case ArenaState.WARMING: {
          // Warmup mode with realtime effects
          if (!this.realtimeEffectService) {
            console.warn('RealtimeEffectService not initialized');
            break;
          }

          if (!this.realtimeEffectService.isRunning()) {
            // Enable UDP realtime mode in WLED first
            await this.enableUDPRealtimeMode();
            await this.realtimeEffectService.start(RealtimeEffectMode.WARMUP);
          } else {
            await this.realtimeEffectService.switchMode(RealtimeEffectMode.WARMUP);
          }

          // Send power level updates for energy bar effect
          if (payload.currentPower !== undefined) {
            this.realtimeEffectService.sendPowerLevel(payload.currentPower);
          }

          // Send shout event if this is a shout trigger
          if (payload.trigger === 'shout_detected' && payload.triggerAmplitude !== undefined) {
            this.realtimeEffectService.sendShoutEvent(payload.triggerAmplitude);
          }
          break;
        }

        case ArenaState.FIGHT:
          // Fight mode with realtime effects
          if (!this.realtimeEffectService) {
            console.warn('RealtimeEffectService not initialized');
            break;
          }

          if (!this.realtimeEffectService.isRunning()) {
            // Enable UDP realtime mode in WLED first
            await this.enableUDPRealtimeMode();
            await this.realtimeEffectService.start(RealtimeEffectMode.FIGHT);
          } else {
            await this.realtimeEffectService.switchMode(RealtimeEffectMode.FIGHT);
          }

          // Send event based on trigger type
          if (payload.trigger === 'punch_detected' && payload.triggerAmplitude !== undefined) {
            this.realtimeEffectService.sendPunchEvent(payload.triggerAmplitude * 10); // Scale 0-1 to 0-10
          } else if (payload.trigger === 'shout_detected' && payload.triggerAmplitude !== undefined) {
            this.realtimeEffectService.sendShoutEvent(payload.triggerAmplitude);
          }
          break;


        case ArenaState.VICTORY:
          // Victory mode - could add special effect mode later
          if (!this.realtimeEffectService) {
            console.warn('RealtimeEffectService not initialized');
            break;
          }
          if (!this.realtimeEffectService.isRunning()) {
            await this.realtimeEffectService.start(RealtimeEffectMode.FIGHT);
          }
          // Send max intensity shout event for victory
          this.realtimeEffectService.sendShoutEvent(1.0);
          break;

        default:
          console.warn('Unknown arena state for WLED:', payload.arenaState);
      }

      console.log('Sent WLED realtime command for arena state:', payload.arenaState, 'trigger:', payload.trigger);
    } catch (error) {
      console.error('Failed to send WLED realtime command:', error);
    }
  }

  /**
   * Set WLED effect using JSON API with timeout protection
   * @param ip WLED controller IP address
   * @param state WLED state object (partial)
   */
  private async setWLEDEffect(state: Record<string, unknown>): Promise<void> {
    const HTTP_TIMEOUT_MS = 5000; // 5 second timeout per request

    for (const controller of this.controllers.values()) {
      try {
        const url = `http://${controller.ip}/json/state`;
        console.log(`Setting WLED effect via JSON API: ${url}`, state);

        // Create abort controller for timeout
        const abortController = new AbortController();
        const timeoutId = setTimeout(() => abortController.abort(), HTTP_TIMEOUT_MS);

        try {
          const response = await fetch(url, {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
            },
            body: JSON.stringify(state),
            signal: abortController.signal,
          });

          clearTimeout(timeoutId);

          if (!response.ok) {
            console.warn(`WLED HTTP error for ${controller.ip}: ${response.status} ${response.statusText}`);
            // Don't throw - continue to next controller
            continue;
          }

          const result = await response.json();
          console.log(`WLED effect set for ${controller.ip}:`, result);
        } catch (fetchError) {
          clearTimeout(timeoutId);
          if (fetchError instanceof Error && fetchError.name === 'AbortError') {
            console.warn(`WLED request timeout for ${controller.ip} (>${HTTP_TIMEOUT_MS}ms)`);
          } else {
            console.warn(`Failed to set WLED effect for ${controller.ip}:`, fetchError);
          }
          // Don't throw - continue to next controller
          continue;
        }
      } catch (error) {
        console.error(`Unexpected error setting WLED effect for ${controller.ip}:`, error);
        // Don't throw - this is a non-critical operation
      }
    }
  }

  /**
   * Enable UDP realtime mode by turning off WLED's built-in effects
   */
  private async enableUDPRealtimeMode(): Promise<void> {
    console.log('Enabling UDP realtime mode - turning off WLED effects');
    const HTTP_TIMEOUT_MS = 5000; // 5 second timeout per request

    for (const controller of this.controllers.values()) {
      try {
        const url = `http://${controller.ip}/json/state`;
        // Turn on the strip and set effect to Solid (0) to allow UDP control
        const state = {
          on: true,
          bri: 255,
          seg: [{
            fx: 0, // Solid effect (no animation)
            col: [[0, 0, 0]] // Black, will be overridden by UDP
          }]
        };

        console.log(`Enabling UDP realtime for ${controller.ip}`);

        // Create abort controller for timeout
        const abortController = new AbortController();
        const timeoutId = setTimeout(() => abortController.abort(), HTTP_TIMEOUT_MS);

        try {
          const response = await fetch(url, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(state),
            signal: abortController.signal,
          });

          clearTimeout(timeoutId);

          if (!response.ok) {
            console.warn(`Failed to enable UDP realtime for ${controller.ip}: HTTP ${response.status}`);
          } else {
            console.log(`UDP realtime enabled for ${controller.ip}`);
          }
        } catch (fetchError) {
          clearTimeout(timeoutId);
          if (fetchError instanceof Error && fetchError.name === 'AbortError') {
            console.warn(`UDP realtime request timeout for ${controller.ip} (>${HTTP_TIMEOUT_MS}ms)`);
          } else {
            console.warn(`Failed to enable UDP realtime for ${controller.ip}:`, fetchError);
          }
        }
      } catch (error) {
        console.error(`Unexpected error enabling UDP realtime for ${controller.ip}:`, error);
      }
    }
  }

  getControllers(): { id: string; device: WLEDController }[] {
    const controllers = Array.from(this.controllers.entries()).map(([id, device]) => ({
      id,
      device,
    }));
    console.log('getControllers returning:', controllers.length, 'controllers');
    return controllers;
  }

  // Remove a controller
  removeController(controllerId: string): void {
    const controller = this.controllers.get(controllerId);
    if (!controller) {
      return;
    }

    // Remove from map
    this.controllers.delete(controllerId);
    console.log('Removed controller:', controllerId);

    // Save to localStorage
    this.saveControllers();

    // Reinitialize effect service without removed controller
    this.initializeRealtimeEffectService();
  }

  /**
   * Initialize or reinitialize the realtime effect service with current controllers
   */
  private initializeRealtimeEffectService(): void {
    // Stop existing service if any
    if (this.realtimeEffectService) {
      void this.realtimeEffectService.stopAll();
    }

    // Create new service with current controller IPs
    const controllerHosts = Array.from(this.controllers.values()).map((controller) => ({
      host: controller.ip,
      port: 21324,
    }));

    this.realtimeEffectService = new RealtimeEffectService(controllerHosts);
    console.log('RealtimeEffectService initialized with', controllerHosts.length, 'controllers');
  }
}

// Singleton instance
export const ledControllerService = new LEDControllerService();
