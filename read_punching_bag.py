#!/usr/bin/env python3
# /// script
# dependencies = [
#     "bleak>=1.1.0",
# ]
# ///
"""
Shadow Warrior Punching Bag Data Reader

This script connects to the ShadowWarrior BLE device and reads IMU data
from the custom acceleration and gyroscope characteristics.
"""

import asyncio
import struct
from bleak import BleakClient, BleakScanner
from bleak.exc import BleakError

# Shadow Warrior BLE Service UUIDs
SW_SERVICE_UUID = "6E400001-B5A3-F393-E0A9-E50E24DCCA9E"
SW_ACCEL_CHAR_UUID = "6E400002-B5A3-F393-E0A9-E50E24DCCA9E"
SW_GYRO_CHAR_UUID = "6E400003-B5A3-F393-E0A9-E50E24DCCA9E"

def unpack_imu_data(data):
    """Unpack 12 bytes of IMU data into X, Y, Z float values"""
    if len(data) != 12:
        raise ValueError(f"Expected 12 bytes, got {len(data)}")
    
    x, y, z = struct.unpack('<fff', data)
    return x, y, z

def acceleration_callback(sender, data):
    """Callback for acceleration data notifications"""
    try:
        x, y, z = unpack_imu_data(data)
        print(f"Acceleration: X={x:6.2f}, Y={y:6.2f}, Z={z:6.2f} m/s²")
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

async def main():
    """Main function"""
    print("Shadow Warrior Punching Bag Data Reader")
    print("=" * 40)
    
    # Scan for devices
    devices = await scan_for_shadow_warrior()
    
    if not devices:
        print("No ShadowWarrior devices found!")
        print("Make sure the punching bag is powered on and advertising.")
        return
    
    # Use the first found device
    device = devices[0]
    print(f"\nUsing device: {device.name} ({device.address})")
    
    while True:
        print("\nOptions:")
        print("1. Stream real-time IMU data")
        print("2. Read single IMU values")
        print("3. Scan for devices again")
        print("4. Exit")
        
        choice = input("\nEnter your choice (1-4): ").strip()
        
        if choice == "1":
            await connect_and_read_data(device.address)
        elif choice == "2":
            await read_single_values(device.address)
        elif choice == "3":
            devices = await scan_for_shadow_warrior()
            if devices:
                device = devices[0]
                print(f"Using device: {device.name} ({device.address})")
        elif choice == "4":
            print("Goodbye!")
            break
        else:
            print("Invalid choice. Please try again.")

if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        print("\nExiting...")