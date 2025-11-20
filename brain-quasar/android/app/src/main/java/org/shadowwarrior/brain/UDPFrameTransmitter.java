package org.shadowwarrior.brain;

import android.util.Log;
import java.net.DatagramPacket;
import java.net.DatagramSocket;
import java.net.InetAddress;

/**
 * Handles UDP transmission of LED frame data to WLED controllers
 */
public class UDPFrameTransmitter {
    private static final String TAG = "UDPFrameTransmitter";
    private static final int WLED_UDP_PORT = 21324;

    private String controllerHost;
    private int controllerPort;
    private DatagramSocket socket;

    public UDPFrameTransmitter(String host, int port) {
        this.controllerHost = host;
        this.controllerPort = port;
    }

    /**
     * Connect and prepare UDP socket
     */
    public void connect() throws Exception {
        try {
            socket = new DatagramSocket();
            Log.d(TAG, "Connected to " + controllerHost + ":" + controllerPort);
        } catch (Exception e) {
            Log.e(TAG, "Failed to create UDP socket", e);
            throw e;
        }
    }

    /**
     * Disconnect UDP socket
     */
    public void disconnect() {
        if (socket != null && !socket.isClosed()) {
            socket.close();
            Log.d(TAG, "Disconnected from " + controllerHost);
        }
    }

    /**
     * Send frame data via UDP to WLED controller
     * Uses WLED realtime protocol (DRGB mode)
     * https://kno.wled.ge/interfaces/udp-realtime/
     *
     * @param pixelData RGB pixel data (3 bytes per pixel)
     */
    public void sendFrame(byte[] pixelData) throws Exception {
        if (socket == null || socket.isClosed()) {
            Log.w(TAG, "Socket not connected, reconnecting...");
            connect();
        }

        try {
            // Build WLED realtime protocol frame (DRGB mode)
            byte[] frameBuffer = buildWLEDFrame(pixelData);

            InetAddress address = InetAddress.getByName(controllerHost);
            DatagramPacket packet = new DatagramPacket(
                frameBuffer,
                frameBuffer.length,
                address,
                controllerPort
            );

            socket.send(packet);
            Log.d(TAG, "Sent frame to " + controllerHost + ":" + controllerPort +
                  " (" + pixelData.length + " bytes)");
        } catch (Exception e) {
            Log.e(TAG, "Failed to send frame", e);
            throw e;
        }
    }

    /**
     * Build WLED realtime protocol frame (DRGB mode)
     * https://kno.wled.ge/interfaces/udp-realtime/
     *
     * DRGB format:
     * - Byte 0: Protocol type (2 = DRGB, sequential RGB without indices)
     * - Byte 1: Timeout in seconds (0-255, how long to keep LEDs before returning to normal mode)
     * - Bytes 2+: Sequential RGB values (3 bytes per LED)
     *
     * Total frame: 2 byte header + (ledCount * 3) bytes of RGB data
     */
    private byte[] buildWLEDFrame(byte[] pixelData) {
        int dataLength = pixelData.length;
        byte[] frame = new byte[2 + dataLength]; // 2-byte header + RGB data

        // Byte 0: Protocol type = 2 (DRGB)
        // DRGB sends sequential RGB values without LED indices
        frame[0] = 0x02;

        // Byte 1: Timeout in seconds (2 seconds)
        // Controller will hold last received frame for 2 seconds
        // then return to normal operation if no new frame arrives
        frame[1] = 0x02;

        // Copy pixel RGB data directly (3 bytes per LED)
        System.arraycopy(pixelData, 0, frame, 2, dataLength);

        return frame;
    }

    public boolean isConnected() {
        return socket != null && !socket.isClosed();
    }
}
