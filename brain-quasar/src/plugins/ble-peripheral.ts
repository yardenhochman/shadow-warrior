import { registerPlugin } from '@capacitor/core';

export interface BlePeripheralPlugin {
  startAdvertising(options: {
    serviceName: string;
    serviceUuid: string;
  }): Promise<void>;

  stopAdvertising(): Promise<void>;

  sendData(options: {
    data: number[];
    txCharUuid: string;
  }): Promise<void>;
}

const BlePeripheral = registerPlugin<BlePeripheralPlugin>('BlePeripheral', {
  web: () => import('./web').then(m => new m.BlePeripheralWeb()),
});

export default BlePeripheral;
