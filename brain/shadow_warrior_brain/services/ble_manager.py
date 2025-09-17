"""
BLE connection manager for punching bag communication
"""

import asyncio
import struct
from datetime import datetime
from typing import List, Optional, Dict, Any
from collections import deque

from bleak import BleakClient, BleakScanner
from bleak.exc import BleakError

from shadow_warrior_brain.models.punching_bag import (
    PunchingBagDevice, PunchingBagStatus, PunchingBagParams, AccelerationData
)
from shadow_warrior_brain.core.config import settings
from shadow_warrior_brain.core.logging_config import get_logger

logger = get_logger(__name__)


class BLEManager:
    """Manages BLE connections to punching bag devices"""
    
    # Shadow Warrior BLE UUIDs
    SERVICE_UUID = "6E400001-B5A3-F393-E0A9-E50E24DCCA9E"
    ACCEL_CHAR_UUID = "6E400002-B5A3-F393-E0A9-E50E24DCCA9E"
    GYRO_CHAR_UUID = "6E400003-B5A3-F393-E0A9-E50E24DCCA9E"
    ALPHA_CHAR_UUID = "6E400004-B5A3-F393-E0A9-E50E24DCCA9E"
    THRESHOLD_CHAR_UUID = "6E400005-B5A3-F393-E0A9-E50E24DCCA9E"
    FIGHT_MODE_CHAR_UUID = "6E400006-B5A3-F393-E0A9-E50E24DCCA9E"
    
    def __init__(self):
        self.client: Optional[BleakClient] = None
        self.connected_device: Optional[PunchingBagDevice] = None
        self.connection_time: Optional[datetime] = None

        # Data storage
        self.acceleration_data: deque = deque(maxlen=settings.acceleration_data_retention)
        self.current_params = PunchingBagParams()
        self.fight_mode = False

        # Status tracking
        self._scanning = False
        self._auto_connect_task: Optional[asyncio.Task] = None
        self._connection_monitor_task: Optional[asyncio.Task] = None
        self._retry_attempts = 0
        self._last_seen_devices: List[PunchingBagDevice] = []
        self._shutting_down = False
        
    async def start_scanning(self):
        """Start background scanning for devices and auto-connection"""
        self._scanning = True
        self._shutting_down = False
        logger.info("BLE Manager: Started background scanning")

        if settings.ble_auto_connect:
            self._auto_connect_task = asyncio.create_task(self._auto_connect_loop())
            self._connection_monitor_task = asyncio.create_task(self._monitor_connection())
        
    async def cleanup(self):
        """Cleanup BLE resources"""
        self._shutting_down = True
        self._scanning = False

        # Cancel background tasks
        if self._auto_connect_task and not self._auto_connect_task.done():
            self._auto_connect_task.cancel()
            try:
                await self._auto_connect_task
            except asyncio.CancelledError:
                pass

        if self._connection_monitor_task and not self._connection_monitor_task.done():
            self._connection_monitor_task.cancel()
            try:
                await self._connection_monitor_task
            except asyncio.CancelledError:
                pass

        # Disconnect if connected
        if self.client and self.client.is_connected:
            try:
                await self.client.disconnect()
                logger.info("BLE Manager: Disconnected from device")
            except Exception as e:
                logger.error(f"BLE Manager: Error during disconnect: {e}")

        self.connected_device = None
        self.connection_time = None
        logger.info("BLE Manager: Cleanup complete")
        
    async def scan_for_punching_bags(self) -> List[PunchingBagDevice]:
        """Scan for available punching bag devices"""
        devices = []

        try:
            logger.info(f"BLE Manager: Scanning for devices (timeout: {settings.ble_scan_timeout}s)...")

            # Try to get RSSI data - method varies by platform
            try:
                # Method 1: Use return_adv=True (works on macOS/CoreBluetooth)
                discovered_data = await BleakScanner.discover(
                    timeout=settings.ble_scan_timeout,
                    return_adv=True
                )
                logger.debug(f"BLE Manager: Found {len(discovered_data)} total devices (with advertisement data)")

                # Log all discovered devices for debugging
                for device_address, (device, adv_data) in discovered_data.items():
                    device_name = device.name or "Unknown"
                    rssi = adv_data.rssi if adv_data else None
                    logger.debug(f"  - {device_name} ({device_address}) RSSI: {rssi}")

                # Look for Shadow Warrior devices (flexible matching)
                for device_address, (device, adv_data) in discovered_data.items():
                    if device.name:
                        # Check for various possible device names
                        name_lower = device.name.lower()
                        if any(keyword in name_lower for keyword in ["shadow", "warrior", "punch", "bag", "sw"]):
                            status = PunchingBagStatus.CONNECTED if (self.connected_device and
                                                                        self.connected_device.address == device_address) else PunchingBagStatus.DISCONNECTED
                            rssi = adv_data.rssi if adv_data else None
                            devices.append(PunchingBagDevice(
                                address=device_address,
                                name=device.name,
                                rssi=rssi,
                                status=status
                            ))
                            logger.info(f"BLE Manager: Found matching device: {device.name} ({device_address}) RSSI: {rssi}")

            except Exception as adv_error:
                logger.warning(f"BLE Manager: Advertisement data method failed: {adv_error}")

                # Method 2: Fallback to simple discover (works on Linux/BlueZ)
                discovered_devices = await BleakScanner.discover(timeout=settings.ble_scan_timeout)
                logger.debug(f"BLE Manager: Found {len(discovered_devices)} total devices (fallback method)")

                # Log all discovered devices for debugging
                for device in discovered_devices:
                    device_name = device.name or "Unknown"
                    rssi = getattr(device, 'rssi', None)
                    logger.debug(f"  - {device_name} ({device.address}) RSSI: {rssi}")

                # Look for Shadow Warrior devices (flexible matching)
                for device in discovered_devices:
                    if device.name:
                        # Check for various possible device names
                        name_lower = device.name.lower()
                        if any(keyword in name_lower for keyword in ["shadow", "warrior", "punch", "bag", "sw"]):
                            status = PunchingBagStatus.CONNECTED if (self.connected_device and
                                                                        self.connected_device.address == device.address) else PunchingBagStatus.DISCONNECTED
                            rssi = getattr(device, 'rssi', None)
                            devices.append(PunchingBagDevice(
                                address=device.address,
                                name=device.name,
                                rssi=rssi,
                                status=status
                            ))
                            logger.info(f"BLE Manager: Found matching device: {device.name} ({device.address}) RSSI: {rssi}")

            if not devices:
                logger.warning("BLE Manager: No Shadow Warrior devices found")

        except Exception as e:
            logger.error(f"BLE Manager: Scan error - {e}")

        return devices
        
    async def connect_to_punching_bag(self, device_address: str) -> bool:
        """Connect to a specific punching bag device"""
        try:
            # Disconnect if already connected
            if self.client and self.client.is_connected:
                await self.client.disconnect()
                await asyncio.sleep(0.5)  # Brief pause after disconnect

            self.client = BleakClient(
                device_address,
                disconnected_callback=self._disconnected_callback
            )

            # Connect with timeout
            await asyncio.wait_for(
                self.client.connect(),
                timeout=settings.ble_connection_timeout
            )

            if self.client.is_connected:
                # Find device info from cache or scan
                device_info = None
                if self._last_seen_devices:
                    device_info = next((d for d in self._last_seen_devices if d.address == device_address), None)

                if not device_info:
                    devices = await self.scan_for_punching_bags()
                    device_info = next((d for d in devices if d.address == device_address), None)

                if device_info:
                    self.connected_device = device_info
                    self.connected_device.status = PunchingBagStatus.CONNECTED
                    self.connection_time = datetime.now()

                    logger.debug(f"BLE Manager: Connected device info - Name: {device_info.name}, RSSI: {device_info.rssi}")

                    # Subscribe to notifications
                    await self._subscribe_to_notifications()

                    logger.info(f"Connected to punching bag: {device_address}")
                    return True

        except asyncio.TimeoutError:
            logger.warning(f"BLE connection timeout for device: {device_address}")
        except BleakError as e:
            logger.error(f"BLE connection error: {e}")
        except Exception as e:
            logger.error(f"Unexpected error during connection: {e}")

        # Clean up on failure
        if self.client:
            try:
                if self.client.is_connected:
                    await self.client.disconnect()
            except:
                pass
            self.client = None

        return False
        
    async def disconnect_punching_bag(self):
        """Disconnect from current punching bag device"""
        if self.client and self.client.is_connected:
            await self.client.disconnect()
            
        self.connected_device = None
        self.connection_time = None
        logger.info("Disconnected from punching bag")
        
    async def _subscribe_to_notifications(self):
        """Subscribe to acceleration data notifications"""
        if not self.client or not self.client.is_connected:
            return
            
        try:
            await self.client.start_notify(self.ACCEL_CHAR_UUID, self._acceleration_callback)
            logger.info("Subscribed to acceleration notifications")
        except Exception as e:
            logger.error(f"Failed to subscribe to notifications: {e}")
            
    def _acceleration_callback(self, sender, data: bytearray):
        """Handle incoming acceleration data"""
        try:
            # Unpack acceleration data according to BLE protocol:
            # 12 bytes (3 x 32-bit floats, little-endian) for [X, Y, Z] acceleration in m/s²
            if len(data) == 12:
                x, y, z = struct.unpack('<fff', data)
                # Calculate magnitude
                acceleration = (x**2 + y**2 + z**2) ** 0.5

                acceleration_data = AccelerationData(
                    timestamp=datetime.now(),
                    acceleration=acceleration,
                    x=x,
                    y=y,
                    z=z
                )
            elif len(data) == 4:
                # Fallback for single float format (legacy support)
                acceleration = struct.unpack('<f', data)[0]

                acceleration_data = AccelerationData(
                    timestamp=datetime.now(),
                    acceleration=acceleration
                )
            else:
                logger.warning(f"Unexpected acceleration data length: {len(data)} bytes")
                return

            self.acceleration_data.append(acceleration_data)

        except Exception as e:
            logger.error(f"Error processing acceleration data: {e}")
            
    async def get_punching_bag_status(self) -> Dict[str, Any]:
        """Get current punching bag status"""
        if not self.connected_device:
            # Check if we have any last seen devices to show info about
            if self._last_seen_devices:
                last_device = self._last_seen_devices[0]  # Most recent scan result
                return {
                    "connected": False,
                    "device_address": last_device.address,
                    "device_name": last_device.name,
                    "rssi": last_device.rssi,
                    "fight_mode": False,
                    "connection_time": None
                }
            else:
                return {
                    "connected": False,
                    "device_address": None,
                    "device_name": None,
                    "rssi": None,
                    "fight_mode": False,
                    "connection_time": None
                }

        # Get latest acceleration data if available
        latest_acceleration = None
        if self.acceleration_data:
            latest = self.acceleration_data[-1]
            latest_acceleration = {
                "x": getattr(latest, 'x', None),
                "y": getattr(latest, 'y', None),
                "z": getattr(latest, 'z', None),
                "acceleration": latest.acceleration,
                "timestamp": latest.timestamp.isoformat()
            }

        return {
            "connected": True,
            "device_address": self.connected_device.address,
            "device_name": self.connected_device.name,
            "rssi": self.connected_device.rssi,
            "fight_mode": self.fight_mode,
            "connection_time": self.connection_time,
            "latest_acceleration": latest_acceleration,
            "parameters": {
                "alpha": self.current_params.alpha,
                "threshold": self.current_params.threshold,
                "fight_mode": self.current_params.fight_mode
            }
        }
        
    async def set_parameters(self, params: PunchingBagParams) -> bool:
        """Set punching bag parameters"""
        if not self.client or not self.client.is_connected:
            return False
            
        try:
            if params.alpha is not None:
                data = struct.pack('<f', params.alpha)
                await self.client.write_gatt_char(self.ALPHA_CHAR_UUID, data)
                self.current_params.alpha = params.alpha
                
            if params.threshold is not None:
                data = struct.pack('<f', params.threshold)
                await self.client.write_gatt_char(self.THRESHOLD_CHAR_UUID, data)
                self.current_params.threshold = params.threshold
                
            if params.fight_mode is not None:
                data = struct.pack('B', 1 if params.fight_mode else 0)
                await self.client.write_gatt_char(self.FIGHT_MODE_CHAR_UUID, data)
                self.current_params.fight_mode = params.fight_mode
                self.fight_mode = params.fight_mode
                
            return True
            
        except Exception as e:
            logger.error(f"Error setting parameters: {e}")
            return False
            
    async def get_parameters(self) -> PunchingBagParams:
        """Get current parameters from device"""
        if not self.client or not self.client.is_connected:
            return self.current_params
            
        try:
            # Read current parameters from device
            if self.current_params.alpha is None:
                data = await self.client.read_gatt_char(self.ALPHA_CHAR_UUID)
                self.current_params.alpha = struct.unpack('<f', data)[0]
                
            if self.current_params.threshold is None:
                data = await self.client.read_gatt_char(self.THRESHOLD_CHAR_UUID)
                self.current_params.threshold = struct.unpack('<f', data)[0]
                
            if self.current_params.fight_mode is None:
                data = await self.client.read_gatt_char(self.FIGHT_MODE_CHAR_UUID)
                self.current_params.fight_mode = bool(struct.unpack('B', data)[0])
                self.fight_mode = self.current_params.fight_mode
                
        except Exception as e:
            logger.error(f"Error reading parameters: {e}")
            
        return self.current_params
        
    async def set_fight_mode(self, enabled: bool) -> bool:
        """Set fight mode specifically"""
        params = PunchingBagParams(fight_mode=enabled)
        return await self.set_parameters(params)
        
    async def get_recent_acceleration_data(self) -> List[AccelerationData]:
        """Get recent acceleration data"""
        return list(self.acceleration_data)

    async def _auto_connect_loop(self):
        """Background task that automatically connects to available punching bags"""
        while not self._shutting_down:
            try:
                # Skip if already connected
                if self.connected_device and self.client and self.client.is_connected:
                    await asyncio.sleep(settings.ble_retry_interval)
                    continue

                logger.info(f"BLE Manager: Auto-connect cycle {self._retry_attempts + 1}")
                devices = await self.scan_for_punching_bags()
                self._last_seen_devices = devices

                if devices:
                    # Try to connect to the first available device
                    device = devices[0]
                    logger.info(f"BLE Manager: Attempting to connect to {device.name} ({device.address})")

                    success = await self.connect_to_punching_bag(device.address)
                    if success:
                        logger.info(f"BLE Manager: Successfully connected to {device.name}")
                        self._retry_attempts = 0
                    else:
                        self._retry_attempts += 1
                        logger.warning(f"BLE Manager: Connection failed (attempt {self._retry_attempts})")

                        # Check max retry attempts
                        if (settings.ble_max_retry_attempts > 0 and
                            self._retry_attempts >= settings.ble_max_retry_attempts):
                            logger.warning("BLE Manager: Max retry attempts reached, stopping auto-connect")
                            break
                else:
                    logger.warning("BLE Manager: No punching bag devices found in scan")

                # Wait before next attempt
                await asyncio.sleep(settings.ble_retry_interval)

            except asyncio.CancelledError:
                logger.info("BLE Manager: Auto-connect loop cancelled")
                break
            except Exception as e:
                logger.error(f"BLE Manager: Error in auto-connect loop: {e}")
                await asyncio.sleep(settings.ble_retry_interval)

    async def _monitor_connection(self):
        """Background task that monitors connection health and handles disconnections"""
        while not self._shutting_down:
            try:
                await asyncio.sleep(5)  # Check every 5 seconds

                if self.connected_device and self.client:
                    # Check if connection is still alive
                    if not self.client.is_connected:
                        logger.warning("BLE Manager: Connection lost, will attempt to reconnect")
                        await self._handle_disconnection()

            except asyncio.CancelledError:
                logger.info("BLE Manager: Connection monitor cancelled")
                break
            except Exception as e:
                logger.error(f"BLE Manager: Error in connection monitor: {e}")

    async def _handle_disconnection(self):
        """Handle unexpected disconnection gracefully"""
        logger.info("BLE Manager: Handling disconnection...")

        # Clean up current connection state
        if self.client:
            try:
                if self.client.is_connected:
                    await self.client.disconnect()
            except Exception as e:
                logger.error(f"BLE Manager: Error during cleanup disconnect: {e}")

        # Reset connection state
        self.connected_device = None
        self.connection_time = None
        self.client = None

        logger.info("BLE Manager: Disconnection handled, auto-connect will retry")

    def _disconnected_callback(self, client):
        """Callback triggered when device disconnects unexpectedly"""
        logger.debug(f"BLE Manager: Device disconnected callback triggered for {client.address}")
        if not self._shutting_down:
            # Schedule disconnection handling in the event loop
            asyncio.create_task(self._handle_disconnection())