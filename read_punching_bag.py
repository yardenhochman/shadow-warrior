#!/usr/bin/env python3
# /// script
# dependencies = [
#     "bleak>=1.1.0",
#     "click>=8.0.0",
# ]
# ///
"""
Shadow Warrior Punching Bag Data Reader

This script connects to the ShadowWarrior BLE device and reads IMU data
from the custom acceleration and gyroscope characteristics.
"""

import asyncio
import struct
import click
from bleak import BleakClient, BleakScanner
from bleak.exc import BleakError

# Shadow Warrior BLE Service UUIDs
SW_SERVICE_UUID = "6E400001-B5A3-F393-E0A9-E50E24DCCA9E"
SW_ACCEL_CHAR_UUID = "6E400002-B5A3-F393-E0A9-E50E24DCCA9E"
SW_GYRO_CHAR_UUID = "6E400003-B5A3-F393-E0A9-E50E24DCCA9E"
SW_ALPHA_CHAR_UUID = "6E400004-B5A3-F393-E0A9-E50E24DCCA9E"
SW_THRESHOLD_CHAR_UUID = "6E400005-B5A3-F393-E0A9-E50E24DCCA9E"

def unpack_accel_data(data):
    if len(data) != 4:
        raise ValueError(f"Expected 4 bytes, got {len(data)}")
    return struct.unpack('<f', data)[0]

def unpack_imu_data(data):
    """Unpack 12 bytes of IMU data into X, Y, Z float values"""
    if len(data) != 12:
        raise ValueError(f"Expected 12 bytes, got {len(data)}")
    
    x, y, z = struct.unpack('<fff', data)
    return x, y, z

def unpack_float_data(data):
    """Unpack 4 bytes into a single float value"""
    if len(data) != 4:
        raise ValueError(f"Expected 4 bytes, got {len(data)}")
    
    return struct.unpack('<f', data)[0]

def pack_float_data(value):
    """Pack a float value into 4 bytes"""
    return struct.pack('<f', value)

def acceleration_callback(sender, data):
    """Callback for acceleration data notifications"""
    try:
        accel = unpack_accel_data(data)
        print(f"Acceleration: {accel:6.2f} m/s²")
    except Exception as e:
        print(f"Error unpacking acceleration data: {e}")

def gyroscope_callback(sender, data):
    """Callback for gyroscope data notifications"""
    try:
        x, y, z = unpack_imu_data(data)
        print(f"Gyroscope:    X={x:6.2f}, Y={y:6.2f}, Z={z:6.2f} rad/s")
    except Exception as e:
        print(f"Error unpacking gyroscope data: {e}")

async def scan_for_shadow_warrior():
    """Scan for ShadowWarrior devices"""
    print("Scanning for ShadowWarrior devices...")
    
    devices = await BleakScanner.discover(timeout=5.0)
    shadow_warrior_devices = []
    
    for device in devices:
        if device.name and "ShadowWarrior" in device.name:
            shadow_warrior_devices.append(device)
            print(f"Found ShadowWarrior device: {device.name} ({device.address})")
    
    return shadow_warrior_devices

async def connect_and_read_data(device_address):
    """Connect to ShadowWarrior device and read IMU data"""
    print(f"Attempting to connect to {device_address}...")
    
    try:
        async with BleakClient(device_address) as client:
            print(f"Connected to {device_address}")
            
            # Check if our service is available
            services = client.services
            shadow_service = None
            
            for service in services:
                if service.uuid.lower() == SW_SERVICE_UUID.lower():
                    shadow_service = service
                    break
            
            if not shadow_service:
                print(f"Shadow Warrior service not found!")
                return
            
            print(f"Found Shadow Warrior service: {shadow_service.uuid}")
            
            # Check characteristics
            accel_char = None
            gyro_char = None
            
            for char in shadow_service.characteristics:
                if char.uuid.lower() == SW_ACCEL_CHAR_UUID.lower():
                    accel_char = char
                    print(f"Found acceleration characteristic: {char.uuid}")
                elif char.uuid.lower() == SW_GYRO_CHAR_UUID.lower():
                    gyro_char = char
                    print(f"Found gyroscope characteristic: {char.uuid}")
            
            if not accel_char or not gyro_char:
                print("Missing required characteristics!")
                return
            
            # Subscribe to notifications
            print("Subscribing to IMU data notifications...")
            await client.start_notify(accel_char, acceleration_callback)
            await client.start_notify(gyro_char, gyroscope_callback)
            
            print("Receiving IMU data (press Ctrl+C to stop):")
            print("-" * 50)
            
            # Keep connection alive and receive data
            try:
                while True:
                    await asyncio.sleep(1)
            except KeyboardInterrupt:
                print("\nStopping data collection...")
            
            # Unsubscribe from notifications
            await client.stop_notify(accel_char)
            await client.stop_notify(gyro_char)
            
    except BleakError as e:
        print(f"BLE error: {e}")
    except Exception as e:
        print(f"Unexpected error: {e}")

async def read_single_values(device_address):
    """Connect and read single values from characteristics"""
    print(f"Reading single values from {device_address}...")
    
    try:
        async with BleakClient(device_address) as client:
            print(f"Connected to {device_address}")
            
            # Read acceleration data
            try:
                accel_data = await client.read_gatt_char(SW_ACCEL_CHAR_UUID)
                x, y, z = unpack_imu_data(accel_data)
                print(f"Acceleration: X={x:6.2f}, Y={y:6.2f}, Z={z:6.2f} m/s²")
            except Exception as e:
                print(f"Error reading acceleration: {e}")
            
            # Read gyroscope data
            try:
                gyro_data = await client.read_gatt_char(SW_GYRO_CHAR_UUID)
                x, y, z = unpack_imu_data(gyro_data)
                print(f"Gyroscope:    X={x:6.2f}, Y={y:6.2f}, Z={z:6.2f} rad/s")
            except Exception as e:
                print(f"Error reading gyroscope: {e}")
                
    except Exception as e:
        print(f"Error: {e}")

async def read_and_modify_parameters(device_address):
    """Read current parameters and allow modification"""
    click.echo(f"Reading parameters from {device_address}...")
    
    try:
        async with BleakClient(device_address) as client:
            click.echo(f"Connected to {device_address}")
            
            # Read current parameters
            try:
                alpha_data = await client.read_gatt_char(SW_ALPHA_CHAR_UUID)
                alpha = unpack_float_data(alpha_data)
                click.echo(f"Current Alpha (EMA smoothing): {alpha:.3f}")
            except Exception as e:
                click.echo(f"Error reading alpha: {e}")
                alpha = None
            
            try:
                threshold_data = await client.read_gatt_char(SW_THRESHOLD_CHAR_UUID)
                threshold = unpack_float_data(threshold_data)
                click.echo(f"Current Threshold: {threshold:.2f}")
            except Exception as e:
                click.echo(f"Error reading threshold: {e}")
                threshold = None
            
            # Allow parameter modification
            while True:
                click.echo("\nParameter modification options:")
                click.echo("1. Update Alpha (EMA smoothing factor)")
                click.echo("2. Update Threshold (acceleration threshold)")
                click.echo("3. Refresh current values")
                click.echo("4. Back to main menu")
                
                choice = click.prompt("Enter your choice (1-4)", type=str).strip()
                
                if choice == "1" and alpha is not None:
                    try:
                        new_alpha = click.prompt(f"Enter new alpha value (current: {alpha:.3f}, range: 0.0-1.0)", type=float)
                        if 0.0 <= new_alpha <= 1.0:
                            packed_data = pack_float_data(new_alpha)
                            await client.write_gatt_char(SW_ALPHA_CHAR_UUID, packed_data)
                            alpha = new_alpha
                            click.echo(f"Alpha updated to: {alpha:.3f}")
                        else:
                            click.echo("Alpha must be between 0.0 and 1.0")
                    except click.Abort:
                        click.echo("Operation cancelled.")
                    except Exception as e:
                        click.echo(f"Error updating alpha: {e}")
                
                elif choice == "2" and threshold is not None:
                    try:
                        new_threshold = click.prompt(f"Enter new threshold value (current: {threshold:.2f})", type=float)
                        packed_data = pack_float_data(new_threshold)
                        await client.write_gatt_char(SW_THRESHOLD_CHAR_UUID, packed_data)
                        threshold = new_threshold
                        click.echo(f"Threshold updated to: {threshold:.2f}")
                    except click.Abort:
                        click.echo("Operation cancelled.")
                    except Exception as e:
                        click.echo(f"Error updating threshold: {e}")
                
                elif choice == "3":
                    try:
                        alpha_data = await client.read_gatt_char(SW_ALPHA_CHAR_UUID)
                        alpha = unpack_float_data(alpha_data)
                        threshold_data = await client.read_gatt_char(SW_THRESHOLD_CHAR_UUID)
                        threshold = unpack_float_data(threshold_data)
                        click.echo(f"Alpha: {alpha:.3f}, Threshold: {threshold:.2f}")
                    except Exception as e:
                        click.echo(f"Error refreshing values: {e}")
                
                elif choice == "4":
                    break
                else:
                    click.echo("Invalid choice or parameter unavailable.")
                    
    except Exception as e:
        click.echo(f"Error: {e}")

async def find_device():
    """Helper function to find first ShadowWarrior device"""
    devices = await scan_for_shadow_warrior()
    
    if not devices:
        click.echo("No ShadowWarrior devices found!")
        click.echo("Make sure the punching bag is powered on and advertising.")
        raise click.Abort()
    
    device = devices[0]
    click.echo(f"Using device: {device.name} ({device.address})")
    return device.address

@click.group()
@click.version_option()
def cli():
    """Shadow Warrior Punching Bag Data Reader
    
    Connect to ShadowWarrior BLE devices and interact with IMU data and parameters.
    """
    pass

@cli.command()
@click.option('--address', '-a', help='Device BLE address (will scan if not provided)')
@click.option('--timeout', '-t', default=5.0, help='Scan timeout in seconds', show_default=True)
def scan(address, timeout):
    """Scan for ShadowWarrior devices"""
    async def _scan():
        click.echo("Scanning for ShadowWarrior devices...")
        devices = await BleakScanner.discover(timeout=timeout)
        shadow_warrior_devices = []
        
        for device in devices:
            if device.name and "ShadowWarrior" in device.name:
                shadow_warrior_devices.append(device)
                click.echo(f"Found: {device.name} ({device.address})")
        
        if not shadow_warrior_devices:
            click.echo("No ShadowWarrior devices found!")
        
        return shadow_warrior_devices
    
    asyncio.run(_scan())

@cli.command()
@click.option('--address', '-a', help='Device BLE address (will scan if not provided)')
def stream(address):
    """Stream real-time IMU data"""
    async def _stream():
        device_address = address or await find_device()
        await connect_and_read_data(device_address)
    
    asyncio.run(_stream())

@cli.command()
@click.option('--address', '-a', help='Device BLE address (will scan if not provided)')
def read(address):
    """Read single IMU values"""
    async def _read():
        device_address = address or await find_device()
        await read_single_values(device_address)
    
    asyncio.run(_read())

@cli.command()
@click.option('--address', '-a', help='Device BLE address (will scan if not provided)')
@click.option('--alpha', type=float, help='Set alpha value (0.0-1.0)')
@click.option('--threshold', type=float, help='Set threshold value')
@click.option('--interactive', '-i', is_flag=True, help='Interactive parameter modification')
def params(address, alpha, threshold, interactive):
    """Read/modify parameters (Alpha, Threshold)"""
    async def _params():
        device_address = address or await find_device()
        
        if interactive:
            await read_and_modify_parameters(device_address)
        else:
            await set_parameters_direct(device_address, alpha, threshold)
    
    asyncio.run(_params())

@cli.command()
@click.option('--address', '-a', help='Device BLE address (will scan if not provided)')
def list_attrs(address):
    """List all device attributes (services and characteristics)"""
    async def _list_attrs():
        device_address = address or await find_device()
        await list_device_attributes(device_address)
    
    asyncio.run(_list_attrs())

async def list_device_attributes(device_address):
    """List all device services and characteristics"""
    click.echo(f"Connecting to {device_address}...")
    
    # Known characteristic names for Shadow Warrior
    char_names = {
        SW_ACCEL_CHAR_UUID.lower(): "Acceleration data",
        SW_GYRO_CHAR_UUID.lower(): "Gyroscope data", 
        SW_ALPHA_CHAR_UUID.lower(): "Acceleration EWMA Alpha parameter",
        SW_THRESHOLD_CHAR_UUID.lower(): "Acceleration threshold parameter"
    }
    
    try:
        async with BleakClient(device_address) as client:
            click.echo(f"Connected to {device_address}")
            click.echo("\nDevice Services and Characteristics:")
            click.echo("=" * 50)
            
            for service in client.services:
                # Highlight Shadow Warrior service
                service_name = "Shadow Warrior Service" if service.uuid.lower() == SW_SERVICE_UUID.lower() else "Unknown Service"
                click.echo(f"\nService: {service_name}")
                click.echo(f"  UUID: {service.uuid}")
                click.echo(f"  Characteristics: {len(service.characteristics)}")
                
                for char in service.characteristics:
                    char_name = char_names.get(char.uuid.lower(), "Unknown")
                    
                    # Get properties
                    properties = []
                    if "read" in char.properties:
                        properties.append("READ")
                    if "write" in char.properties:
                        properties.append("WRITE")
                    if "notify" in char.properties:
                        properties.append("NOTIFY")
                    if "indicate" in char.properties:
                        properties.append("INDICATE")
                    
                    click.echo(f"    • {char_name}")
                    click.echo(f"      UUID: {char.uuid}")
                    click.echo(f"      Properties: {', '.join(properties) if properties else 'None'}")
                    
                    # Try to read current value for known characteristics
                    if service.uuid.lower() == SW_SERVICE_UUID.lower():
                        try:
                            if char.uuid.lower() == SW_ALPHA_CHAR_UUID.lower():
                                data = await client.read_gatt_char(char.uuid)
                                value = unpack_float_data(data)
                                click.echo(f"      Current Value: {value:.3f}")
                            elif char.uuid.lower() == SW_THRESHOLD_CHAR_UUID.lower():
                                data = await client.read_gatt_char(char.uuid)
                                value = unpack_float_data(data)
                                click.echo(f"      Current Value: {value:.2f}")
                            elif char.uuid.lower() == SW_ACCEL_CHAR_UUID.lower():
                                data = await client.read_gatt_char(char.uuid)
                                value = unpack_accel_data(data)
                                click.echo(f"      Current Value: {value:.2f} m/s²")
                        except Exception:
                            click.echo(f"      Current Value: Unable to read")
                    
                    # List descriptors if any
                    if char.descriptors:
                        click.echo(f"      Descriptors: {len(char.descriptors)}")
                        for desc in char.descriptors:
                            click.echo(f"        - {desc.uuid}")
                            
    except Exception as e:
        click.echo(f"Error: {e}")

async def set_parameters_direct(device_address, alpha, threshold):
    """Set parameters directly without interactive menu"""
    click.echo(f"Connecting to {device_address}...")
    
    try:
        async with BleakClient(device_address) as client:
            click.echo(f"Connected to {device_address}")
            
            # Read current values first
            try:
                alpha_data = await client.read_gatt_char(SW_ALPHA_CHAR_UUID)
                current_alpha = unpack_float_data(alpha_data)
                click.echo(f"Current Alpha: {current_alpha:.3f}")
            except Exception as e:
                click.echo(f"Error reading alpha: {e}")
                current_alpha = None
            
            try:
                threshold_data = await client.read_gatt_char(SW_THRESHOLD_CHAR_UUID)
                current_threshold = unpack_float_data(threshold_data)
                click.echo(f"Current Threshold: {current_threshold:.2f}")
            except Exception as e:
                click.echo(f"Error reading threshold: {e}")
                current_threshold = None
            
            # Update alpha if provided
            if alpha is not None:
                if 0.0 <= alpha <= 1.0:
                    try:
                        packed_data = pack_float_data(alpha)
                        await client.write_gatt_char(SW_ALPHA_CHAR_UUID, packed_data)
                        click.echo(f"Alpha updated: {current_alpha:.3f} → {alpha:.3f}")
                    except Exception as e:
                        click.echo(f"Error updating alpha: {e}")
                else:
                    click.echo("Error: Alpha must be between 0.0 and 1.0")
            
            # Update threshold if provided
            if threshold is not None:
                try:
                    packed_data = pack_float_data(threshold)
                    await client.write_gatt_char(SW_THRESHOLD_CHAR_UUID, packed_data)
                    click.echo(f"Threshold updated: {current_threshold:.2f} → {threshold:.2f}")
                except Exception as e:
                    click.echo(f"Error updating threshold: {e}")
                    
    except Exception as e:
        click.echo(f"Error: {e}")

if __name__ == "__main__":
    try:
        cli()
    except KeyboardInterrupt:
        click.echo("\nExiting...")