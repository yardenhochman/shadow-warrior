package com.shadow_warrior.ble

import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattServerCallback
import com.getcapacitor.JSObject

class ArenaGattServerCallback(private val plugin: BlePeripheralPlugin) : BluetoothGattServerCallback() {

    override fun onConnectionStateChange(device: BluetoothDevice?, status: Int, newState: Int) {
        super.onConnectionStateChange(device, status, newState)

        val obj = JSObject()
        obj.put("deviceAddress", device?.address ?: "unknown")
        obj.put("deviceName", device?.name ?: "Unknown Device")

        when (newState) {
            BluetoothGatt.STATE_CONNECTED -> {
                plugin.emitEvent("onDeviceConnected", obj)
            }
            BluetoothGatt.STATE_DISCONNECTED -> {
                plugin.emitEvent("onDeviceDisconnected", obj)
            }
        }
    }

    override fun onCharacteristicReadRequest(
        device: BluetoothDevice?,
        requestId: Int,
        offset: Int,
        characteristic: android.bluetooth.BluetoothGattCharacteristic?
    ) {
        super.onCharacteristicReadRequest(device, requestId, offset, characteristic)
    }

    override fun onCharacteristicWriteRequest(
        device: BluetoothDevice?,
        requestId: Int,
        characteristic: android.bluetooth.BluetoothGattCharacteristic?,
        preparedWrite: Boolean,
        responseNeeded: Boolean,
        offset: Int,
        value: ByteArray?
    ) {
        super.onCharacteristicWriteRequest(
            device,
            requestId,
            characteristic,
            preparedWrite,
            responseNeeded,
            offset,
            value
        )

        if (value != null) {
            val obj = JSObject()
            obj.put("data", value.map { it.toInt() })
            obj.put("deviceAddress", device?.address ?: "unknown")
            plugin.emitEvent("onDataReceived", obj)
        }
    }
}
