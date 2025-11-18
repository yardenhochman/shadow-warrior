import BlePeripheral from 'src/plugins/ble-peripheral';
import { useStateMachineStore } from 'src/stores/state-machine';
import type { PluginListenerHandle } from '@capacitor/core';

const UART_SERVICE_UUID = '6E400001-B5A3-F393-E0A9-E50E24DCCA9E';
const UART_TX_CHAR_UUID = '6E400003-B5A3-F393-E0A9-E50E24DCCA9E';

interface DeviceConnectionEvent {
  deviceAddress: string;
  deviceName: string;
}

class BleArenaPeripheral {
  private isAdvertising = false;
  private streamInterval: number | null = null;
  private connectedDevices: Set<string> = new Set();
  private arenaName = 'Shadow Warrior Arena';
  private enabled = false;
  private listeners: PluginListenerHandle[] = [];

  async initialize(arenaName: string, enabled: boolean): Promise<void> {
    this.arenaName = arenaName;
    this.enabled = enabled;

    // Set up event listeners for device connections using Capacitor's plugin API
    const blePeripheralAny = BlePeripheral as unknown as {
      addListener: (eventName: string, callback: (data: DeviceConnectionEvent) => void) => Promise<PluginListenerHandle>;
    };

    const connectedListener = await blePeripheralAny.addListener(
      'onDeviceConnected',
      (data: DeviceConnectionEvent) => {
        console.log('[BLE Peripheral] Device connected:', data.deviceAddress);
        this.connectedDevices.add(data.deviceAddress);
        console.log('[BLE Peripheral] Connected devices:', this.connectedDevices.size);
      }
    );
    this.listeners.push(connectedListener);

    const disconnectedListener = await blePeripheralAny.addListener(
      'onDeviceDisconnected',
      (data: DeviceConnectionEvent) => {
        console.log('[BLE Peripheral] Device disconnected:', data.deviceAddress);
        this.connectedDevices.delete(data.deviceAddress);
        console.log('[BLE Peripheral] Connected devices:', this.connectedDevices.size);
      }
    );
    this.listeners.push(disconnectedListener);

    if (enabled) {
      await this.startAdvertising();
    }
  }

  async startAdvertising(): Promise<void> {
    if (this.isAdvertising) return;

    try {
      await BlePeripheral.startAdvertising({
        serviceName: this.arenaName,
        serviceUuid: UART_SERVICE_UUID,
      });

      this.isAdvertising = true;
      this.startStreaming();
      console.log('[BLE Peripheral] Started advertising:', this.arenaName);
    } catch (error) {
      console.error('[BLE Peripheral] Error starting advertising:', error);
    }
  }

  async stopAdvertising(): Promise<void> {
    if (!this.isAdvertising) return;

    try {
      await BlePeripheral.stopAdvertising();
      this.isAdvertising = false;
      this.stopStreaming();
      console.log('[BLE Peripheral] Stopped advertising');
    } catch (error) {
      console.error('[BLE Peripheral] Error stopping advertising:', error);
    }
  }

  startStreaming(): void {
    if (this.streamInterval !== null) return;

    this.streamInterval = window.setInterval(() => {
      if (this.connectedDevices.size === 0) return;

      void (async () => {
        try {
          const state = this.encodeArenaState();
          await BlePeripheral.sendData({
            data: Array.from(state),
            txCharUuid: UART_TX_CHAR_UUID,
          });
        } catch (error) {
          console.error('[BLE Peripheral] Error streaming state:', error);
        }
      })();
    }, 500) as unknown as number;
  }

  private stopStreaming(): void {
    if (this.streamInterval !== null) {
      clearInterval(this.streamInterval);
      this.streamInterval = null;
    }
  }

  private encodeArenaState(): Uint8Array {
    const stateMachine = useStateMachineStore();
    const now = Date.now();

    const buffer = new ArrayBuffer(40);
    const view = new DataView(buffer);
    let offset = 0;

    // State enum (1 byte)
    view.setUint8(offset++, this.encodeStateEnum(stateMachine.currentState));

    // Metrics (4 bytes each = 16 bytes)
    view.setFloat32(offset, stateMachine.metrics.shoutAmplitude, true);
    offset += 4;
    view.setFloat32(offset, stateMachine.metrics.punchForce, true);
    offset += 4;
    view.setFloat32(offset, stateMachine.metrics.warmingPower, true);
    offset += 4;
    view.setFloat32(offset, stateMachine.metrics.fightPower, true);
    offset += 4;

    // Progress (1 byte each = 2 bytes)
    view.setUint8(offset++, Math.round(stateMachine.warmingProgress));
    view.setUint8(offset++, Math.round(stateMachine.fightProgress));

    // Timers (4 bytes each = 12 bytes)
    const fightElapsed = stateMachine.fightStartTime ? now - stateMachine.fightStartTime : 0;
    const warmingElapsed = stateMachine.warmingStartTime ? now - stateMachine.warmingStartTime : 0;
    const cooldownRemaining = stateMachine.cooldownTimeRemaining || 0;

    view.setUint32(offset, fightElapsed, true);
    offset += 4;
    view.setUint32(offset, warmingElapsed, true);
    offset += 4;
    view.setUint32(offset, cooldownRemaining, true);
    offset += 4;

    // Timestamp (8 bytes)
    view.setBigUint64(offset, BigInt(now), true);

    return new Uint8Array(buffer);
  }

  private encodeStateEnum(state: string): number {
    const stateMap: Record<string, number> = {
      IDLE: 0,
      WARMING: 1,
      FIGHT: 2,
      COOLDOWN: 3,
    };
    return stateMap[state] || 0;
  }

  getStatus(): { isAdvertising: boolean; connectedDevices: number; arenaName: string } {
    return {
      isAdvertising: this.isAdvertising,
      connectedDevices: this.connectedDevices.size,
      arenaName: this.arenaName,
    };
  }

  setArenaName(name: string): void {
    this.arenaName = name;
  }

  async toggle(enabled: boolean): Promise<void> {
    this.enabled = enabled;
    if (enabled) {
      await this.startAdvertising();
    } else {
      await this.stopAdvertising();
    }
  }
}

export const bleArenaPeripheral = new BleArenaPeripheral();
