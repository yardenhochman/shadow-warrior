import { BleClient, type BleDevice } from '@capacitor-community/bluetooth-le';
import { useMonitorModeStore } from 'src/stores/monitor-mode';

const UART_SERVICE_UUID = '6E400001-B5A3-F393-E0A9-E50E24DCCA9E';
const UART_TX_CHAR_UUID = '6E400003-B5A3-F393-E0A9-E50E24DCCA9E';

interface ArenaState {
  currentState: string;
  metrics: {
    shoutAmplitude: number;
    punchForce: number;
    warmingPower: number;
    fightPower: number;
  };
  progress: {
    warmingProgress: number;
    fightProgress: number;
  };
  timers: {
    fightElapsed: number;
    warmingElapsed: number;
    cooldownRemaining: number;
  };
  timestamp: number;
}

class BleArenaMonitor {
  private device: BleDevice | null = null;
  private isConnected = false;
  private reconnectAttempts = 0;
  private maxReconnectAttempts = 3;

  async scanAndConnect(): Promise<void> {
    const store = useMonitorModeStore();
    store.setConnectionStatus('scanning');

    try {
      console.log('[BLE Monitor] Initializing BLE client...');
      await BleClient.initialize();
      console.log('[BLE Monitor] BLE client initialized');

      // Try to request permissions before device scan
      try {
        console.log('[BLE Monitor] Requesting BLE permissions...');
        // Some devices require this call before scanning
        const bleClientAny = BleClient as unknown as { requestBluetoothPermissions?: () => Promise<void> };
        await bleClientAny.requestBluetoothPermissions?.();
        console.log('[BLE Monitor] Permissions granted');
      } catch (permError) {
        console.warn('[BLE Monitor] Permission request failed (may not be required):', permError);
      }

      console.log('[BLE Monitor] Scanning for Shadow Warrior Arena...');

      const device = await BleClient.requestDevice({
        services: [UART_SERVICE_UUID],
        namePrefix: 'Shadow Warrior',
      });

      this.device = device;
      console.log('[BLE Monitor] Found arena:', device.name, device.deviceId);

      await this.connectToDevice();
    } catch (error) {
      console.error('[BLE Monitor] Scan error:', error);
      console.error('[BLE Monitor] Error details:', {
        message: error instanceof Error ? error.message : String(error),
        stack: error instanceof Error ? error.stack : 'N/A',
        errorCode: (error as { errorCode?: string }).errorCode,
        errorMessage: (error as { errorMessage?: string }).errorMessage,
      });
      store.setConnectionStatus('disconnected');
      throw error;
    }
  }

  private async connectToDevice(): Promise<void> {
    const store = useMonitorModeStore();

    if (!this.device) return;

    try {
      store.setConnectionStatus('connecting');

      await BleClient.connect(this.device.deviceId, () => {
        console.log('[BLE Monitor] Disconnected from arena');
        this.isConnected = false;
        store.setConnectionStatus('disconnected');
        void this.handleDisconnection();
      });

      this.isConnected = true;
      this.reconnectAttempts = 0;
      console.log('[BLE Monitor] Connected to arena');

      store.setConnectedArena(this.device.deviceId, this.device.name || 'Shadow Warrior Arena');
      store.setConnectionStatus('connected');

      await this.startUartReceive();
    } catch (error) {
      console.error('[BLE Monitor] Connection error:', error);
      store.setConnectionStatus('disconnected');
      throw error;
    }
  }

  private async startUartReceive(): Promise<void> {
    if (!this.device) return;

    try {
      console.log('[BLE Monitor] Starting notifications for', this.device.deviceId);

      let notificationCount = 0;

      // Start notifications with callback that receives data
      await BleClient.startNotifications(
        this.device.deviceId,
        UART_SERVICE_UUID,
        UART_TX_CHAR_UUID,
        (dataView: DataView) => {
          try {
            notificationCount++;
            if (notificationCount % 10 === 0) {
              console.log(`[BLE Monitor] Notification #${notificationCount}`);
            }

            const dataArray = new Uint8Array(dataView.buffer, dataView.byteOffset, dataView.byteLength);
            console.log('[BLE Monitor] Received', dataArray.length, 'bytes');
            const state = this.decodeState(dataArray);
            console.log('[BLE Monitor] Decoded state:', state.currentState);
            this.updateUI(state);
          } catch (err) {
            console.error('[BLE Monitor] Callback error:', err);
          }
        }
      );

      console.log('[BLE Monitor] UART RX started - streaming arena state');
    } catch (error) {
      console.error('[BLE Monitor] Error starting notifications:', error);
    }
  }

  private decodeState(data: Uint8Array): ArenaState {
    const view = new DataView(data.buffer);
    let offset = 0;

    const stateEnum = view.getUint8(offset++);
    const currentState = this.decodeStateEnum(stateEnum);

    return {
      currentState,
      metrics: {
        shoutAmplitude: view.getFloat32(offset, true),
        punchForce: view.getFloat32(offset + 4, true),
        warmingPower: view.getFloat32(offset + 8, true),
        fightPower: view.getFloat32(offset + 12, true),
      },
      progress: {
        warmingProgress: view.getUint8(offset + 16),
        fightProgress: view.getUint8(offset + 17),
      },
      timers: {
        fightElapsed: view.getUint32(offset + 18, true),
        warmingElapsed: view.getUint32(offset + 22, true),
        cooldownRemaining: view.getUint32(offset + 26, true),
      },
      timestamp: Number(view.getBigUint64(offset + 30, true)),
    };
  }

  private decodeStateEnum(state: number): string {
    const stateMap: Record<number, string> = {
      0: 'IDLE',
      1: 'WARMING',
      2: 'FIGHT',
      3: 'COOLDOWN',
    };
    return stateMap[state] || 'IDLE';
  }

  private updateUI(state: ArenaState): void {
    const store = useMonitorModeStore();
    store.updateRemoteArenaState(state);
  }

  private async handleDisconnection(): Promise<void> {
    if (this.reconnectAttempts >= this.maxReconnectAttempts) {
      console.log('[BLE Monitor] Max reconnection attempts reached');
      return;
    }

    this.reconnectAttempts++;
    const delay = 2000 * this.reconnectAttempts; // Exponential backoff

    console.log(`[BLE Monitor] Attempting to reconnect... (attempt ${this.reconnectAttempts})`);

    await new Promise((resolve) => setTimeout(resolve, delay));

    try {
      if (this.device) {
        await this.connectToDevice();
        console.log('[BLE Monitor] Reconnected successfully');
      }
    } catch (error) {
      console.error('[BLE Monitor] Reconnection failed:', error);
      await this.handleDisconnection();
    }
  }

  async disconnect(): Promise<void> {
    const store = useMonitorModeStore();

    try {
      if (this.device) {
        await BleClient.disconnect(this.device.deviceId);
      }
      this.isConnected = false;
      this.device = null;
      this.reconnectAttempts = 0;
      store.setConnectionStatus('disconnected');
      console.log('[BLE Monitor] Disconnected from arena');
    } catch (error) {
      console.error('[BLE Monitor] Error disconnecting:', error);
    }
  }

  getStatus(): { isConnected: boolean; deviceName: string | null } {
    return {
      isConnected: this.isConnected,
      deviceName: this.device?.name || null,
    };
  }
}

export const bleArenaMonitor = new BleArenaMonitor();
