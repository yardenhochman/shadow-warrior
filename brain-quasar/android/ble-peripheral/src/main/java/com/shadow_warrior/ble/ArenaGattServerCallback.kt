package com.shadow_warrior.ble

import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattServerCallback
import android.util.Log
import com.getcapacitor.JSObject

class ArenaGattServerCallback(private val plugin: BlePeripheralPlugin) : BluetoothGattServerCallback() {
    companion object {
        private const val TAG = "ArenaGattServer"
    }

    override fun onConnectionStateChange(device: BluetoothDevice?, status: Int, newState: Int) {
        super.onConnectionStateChange(device, status, newState)

        val stateStr = when (newState) {
            BluetoothGatt.STATE_CONNECTED -> "CONNECTED"
            BluetoothGatt.STATE_DISCONNECTED -> "DISCONNECTED"
            else -> "UNKNOWN($newState)"
        }
        Log.d(TAG, "onConnectionStateChange: device=${device?.address}, status=$status, state=$stateStr")

        val obj = JSObject()
        obj.put("deviceAddress", device?.address ?: "unknown")
        obj.put("deviceName", device?.name ?: "Unknown Device")

        when (newState) {
            BluetoothGatt.STATE_CONNECTED -> {
                Log.d(TAG, "Device connected: ${device?.address}")
                plugin.emitEvent("onDeviceConnected", obj)
            }
            BluetoothGatt.STATE_DISCONNECTED -> {
                Log.d(TAG, "Device disconnected: ${device?.address}")
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

        Log.d(TAG, "onCharacteristicReadRequest: device=${device?.address}, uuid=${characteristic?.uuid}, offset=$offset, requestId=$requestId")

        // Send response to client
        if (device != null && characteristic != null) {
            val value = characteristic.value ?: byteArrayOf()
            val responseData = if (offset < value.size) {
                value.copyOfRange(offset, value.size)
            } else {
                byteArrayOf()
            }
            Log.d(TAG, "Sending characteristic read response: ${responseData.size} bytes")
            val success = plugin.gattServer?.sendResponse(
                device,
                requestId,
                BluetoothGatt.GATT_SUCCESS,
                offset,
                responseData
            ) ?: false
            Log.d(TAG, "Characteristic read response sent: $success")
        } else {
            Log.w(TAG, "onCharacteristicReadRequest: device or characteristic is null")
        }
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

        Log.d(TAG, "onCharacteristicWriteRequest: device=${device?.address}, uuid=${characteristic?.uuid}, size=${value?.size ?: 0}, responseNeeded=$responseNeeded, requestId=$requestId")

        if (value != null) {
            val obj = JSObject()
            obj.put("data", value.map { it.toInt() })
            obj.put("deviceAddress", device?.address ?: "unknown")
            Log.d(TAG, "Emitting onDataReceived event: ${value.size} bytes from ${device?.address}")
            plugin.emitEvent("onDataReceived", obj)
        }

        // Send response to client
        if (responseNeeded && device != null) {
            Log.d(TAG, "Sending characteristic write response")
            val success = plugin.gattServer?.sendResponse(
                device,
                requestId,
                BluetoothGatt.GATT_SUCCESS,
                offset,
                value
            ) ?: false
            Log.d(TAG, "Characteristic write response sent: $success")
        } else if (!responseNeeded) {
            Log.d(TAG, "Response not needed for this write request")
        }
    }

    override fun onDescriptorWriteRequest(
        device: BluetoothDevice?,
        requestId: Int,
        descriptor: android.bluetooth.BluetoothGattDescriptor?,
        preparedWrite: Boolean,
        responseNeeded: Boolean,
        offset: Int,
        value: ByteArray?
    ) {
        super.onDescriptorWriteRequest(device, requestId, descriptor, preparedWrite, responseNeeded, offset, value)

        Log.d(TAG, "onDescriptorWriteRequest: device=${device?.address}, uuid=${descriptor?.uuid}, size=${value?.size ?: 0}, responseNeeded=$responseNeeded, requestId=$requestId")

        // Handle CCCD (Client Characteristic Configuration Descriptor) writes for notifications
        if (descriptor != null && value != null) {
            descriptor.value = value
            Log.d(TAG, "CCCD updated: ${descriptor.uuid} = ${value.joinToString(",")}")
        } else {
            Log.w(TAG, "onDescriptorWriteRequest: descriptor or value is null")
        }

        // Send response to client
        if (responseNeeded && device != null) {
            Log.d(TAG, "Sending descriptor write response")
            val success = plugin.gattServer?.sendResponse(
                device,
                requestId,
                BluetoothGatt.GATT_SUCCESS,
                offset,
                value
            ) ?: false
            Log.d(TAG, "Descriptor write response sent: $success")
        } else if (!responseNeeded) {
            Log.d(TAG, "Response not needed for this descriptor write")
        }
    }
}
