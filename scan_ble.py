#! /usr/bin/env python3
# /// script
# dependencies = [
#     "click",
#     "bleak",]
# ///

import asyncio
import json
import struct
from bleak import BleakScanner, BleakClient
from bleak.exc import BleakError
import click

SW_SERVICE_UUID      = "6e400001-b5a3-f393-e0a9-e50e24dcca9e"
SW_ACCEL_CHAR_UUID   = "6e400002-b5a3-f393-e0a9-e50e24dcca9e"
SW_GYRO_CHAR_UUID    = "6e400003-b5a3-f393-e0a9-e50e24dcca9e"
SW_ALPHA_CHAR_UUID   = "6e400004-b5a3-f393-e0a9-e50e24dcca9e"
SW_THRESHOLD_CHAR_UUID = "6e400005-b5a3-f393-e0a9-e50e24dcca9e"
SW_FIGHT_MODE_CHAR_UUID = "6e400006-b5a3-f393-e0a9-e50e24dcca9e"


@click.group()
def cli():
    """BLE tool for Shadow Warrior devices."""
    pass


@cli.command()
@click.option('--timeout', default=5.0, help='Scan timeout in seconds.')
def scan(timeout):
    """Scan for nearby BLE devices."""
    async def run():
        try:
            devices = await BleakScanner.discover(timeout=timeout, return_adv=True)
            results = []
            for _, (device, adv_data) in devices.items():
                results.append({
                    "name": adv_data.local_name or device.name,
                    "address": device.address,
                    "rssi": adv_data.rssi,
                    "service_uuids": adv_data.service_uuids,
                })
            click.echo(json.dumps(results, indent=2))
        except BleakError as e:
            click.echo(json.dumps({"error": str(e)}))

    asyncio.run(run())


@cli.command()
@click.argument('address')
def read(address):
    """Read all attributes from a Shadow Warrior punching bag device."""
    async def run():
        try:
            async with BleakClient(address) as client:
                accel_raw = await client.read_gatt_char(SW_ACCEL_CHAR_UUID)
                gyro_raw  = await client.read_gatt_char(SW_GYRO_CHAR_UUID)
                alpha_raw = await client.read_gatt_char(SW_ALPHA_CHAR_UUID)
                thresh_raw = await client.read_gatt_char(SW_THRESHOLD_CHAR_UUID)
                fight_raw  = await client.read_gatt_char(SW_FIGHT_MODE_CHAR_UUID)

                accel = struct.unpack('<f', accel_raw)[0]
                gx, gy, gz = struct.unpack('<fff', gyro_raw)
                alpha = struct.unpack('<f', alpha_raw)[0]
                threshold = struct.unpack('<f', thresh_raw)[0]
                fight_mode = bool(fight_raw[0])

                click.echo(json.dumps({
                    "address": address,
                    "accel_ema": accel,
                    "gyro": {"x": gx, "y": gy, "z": gz},
                    "alpha": alpha,
                    "threshold": threshold,
                    "fight_mode": fight_mode,
                }, indent=2))
        except BleakError as e:
            click.echo(json.dumps({"error": str(e)}))

    asyncio.run(run())


if __name__ == "__main__":
    cli()
