// UDP Service for WLED Realtime Communication
import { UdpSocket } from 'capacitor-udp-socket';
import { Capacitor } from '@capacitor/core';

export interface UDPConfig {
  host: string;
  port: number;
  timeout?: number;
}

export class UDPService {
  private socketId: number | null = null;
  public config: UDPConfig | null = null;
  private isConnected = false;
  constructor(config: UDPConfig) {
    this.config = config;
  }
  /**
   * Check if UDP is available on this platform
   */
  isAvailable(): boolean {
    return Capacitor.isNativePlatform();
  }

  /**
   * Initialize UDP socket for WLED Realtime
   * @param config UDP configuration (host, port)
   */
  async connect(): Promise<void> {
    if (!this.isAvailable()) {
      throw new Error('UDP sockets are only available on native platforms (Android/iOS). Please run the app on a device.');
    }

    // Disconnect any existing socket first
    if (this.isConnected || this.socketId !== null) {
      console.log('Disconnecting existing socket before creating new one...');
      await this.disconnect();
    }

    try {
      console.log('Creating UDP socket...');

      // Create UDP socket with explicit properties
      const createResult = await UdpSocket.create({
        properties: {
          name: 'wled-udp',
          bufferSize: 4096
        }
      });

      console.log('UDP socket create result:', JSON.stringify(createResult));
      const socketId = createResult.socketId;

      if (socketId === undefined || socketId === null) {
        throw new Error('Socket ID is undefined after creation');
      }

      console.log(`UDP socket created with ID: ${socketId}`);

      // Bind the socket to allow sending data
      // Using port 0 to let the OS assign a random available port for sending
      console.log('Binding UDP socket...');
      await UdpSocket.bind({ socketId, address: '0.0.0.0', port: 0 });
      console.log(`UDP socket bound successfully`);

      this.socketId = socketId;
      this.isConnected = true;

      console.log(`✓ UDP service ready - Socket ${socketId} will send to ${this.config?.host}:${this.config?.port}`);
    } catch (error) {
      // Clean up on error
      this.socketId = null;
      this.config = null;
      this.isConnected = false;

      console.error('Failed to create/bind UDP socket - Full error:', error);
      console.error('Error type:', typeof error);
      console.error('Error keys:', error ? Object.keys(error) : 'null');

      const errorMsg = error instanceof Error ? error.message : String(error);
      const errorDetails = error ? JSON.stringify(error, Object.getOwnPropertyNames(error)) : 'unknown';

      throw new Error(`Failed to setup UDP socket: ${errorMsg} | Details: ${errorDetails}`);
    }
  }

  /**
   * Send DRGB protocol frame to WLED
   * @param frameData RGB data for all LEDs (3 bytes per LED)
   * @param timeout Timeout in seconds (0 = no timeout, use live mode)
   */
  async sendFrame(frameData: Uint8Array, timeout = 1): Promise<void> {
    console.debug(`sendFrame called - socketId: ${this.socketId}, isConnected: ${this.isConnected}, config: ${this.config ? 'set' : 'null'}`);

    if (!this.socketId && this.socketId !== 0) {
      const errorMsg = `UDP socket ID is invalid: ${this.socketId}`;
      console.error(errorMsg);
      throw new Error(errorMsg);
    }

    if (!this.config) {
      const errorMsg = 'UDP config is not set';
      console.error(errorMsg);
      throw new Error(errorMsg);
    }

    if (!this.isConnected) {
      const errorMsg = 'UDP socket connection flag is false';
      console.error(errorMsg);
      throw new Error(errorMsg);
    }

    try {
      // Build DRGB packet
      // Byte 0: Protocol ID (0x02 for DRGB)
      // Byte 1: Timeout in seconds
      // Byte 2+: RGB data
      const packet = new Uint8Array(2 + frameData.length);
      packet[0] = 0x02; // DRGB protocol (corrected from 0x01)
      packet[1] = timeout;
      packet.set(frameData, 2);

      console.debug(`Sending UDP packet: ${packet.length} bytes to ${this.config.host}:${this.config.port} via socket ${this.socketId}`);

      // Convert to base64 for capacitor plugin
      const base64Data = this.uint8ArrayToBase64(packet);

      // Send UDP packet
      await UdpSocket.send({
        socketId: this.socketId,
        address: this.config.host,
        port: this.config.port,
        buffer: base64Data,
      });

      console.log('✓ UDP packet sent successfully');
    } catch (error) {
      console.error('Failed to send UDP frame:', error);
      throw error;
    }
  }

  /**
   * Send a black frame to clear the LED strip
   * @param ledCount Number of LEDs to clear
   */
  async sendBlackFrame(ledCount: number): Promise<void> {
    const blackFrame = new Uint8Array(ledCount * 3); // All zeros (black)
    await this.sendFrame(blackFrame);
    console.log(`Sent black frame to clear ${ledCount} LEDs`);
  }

  /**
   * Close UDP socket
   */
  async disconnect(): Promise<void> {
    if (this.socketId !== null) {
      const socketIdToClose = this.socketId;
      // Clear state immediately to prevent race conditions
      this.socketId = null;
      this.isConnected = false;

      try {
        await UdpSocket.close({ socketId: socketIdToClose });
        console.log(`UDP socket closed: ${socketIdToClose}`);
      } catch (error) {
        // Socket might already be closed, log but don't throw
        console.warn('Failed to close UDP socket (might already be closed):', error);
      }
    }
  }

  /**
   * Check if UDP socket is connected
   */
  connected(): boolean {
    return this.isConnected && this.socketId !== null;
  }

  /**
   * Convert Uint8Array to base64 string for capacitor plugin
   */
  private uint8ArrayToBase64(array: Uint8Array): string {
    let binary = '';
    for (let i = 0; i < array.length; i++) {
      binary += String.fromCharCode(array[i]!);
    }
    return btoa(binary);
  }
}

