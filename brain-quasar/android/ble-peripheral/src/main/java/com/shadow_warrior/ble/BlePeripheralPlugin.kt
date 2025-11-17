package com.shadow_warrior.ble

import android.Manifest
import android.bluetooth.*
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.bluetooth.le.BluetoothLeAdvertiser
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.os.ParcelUuid
import androidx.core.app.ActivityCompat
import com.getcapacitor.JSObject
import com.getcapacitor.Plugin
import com.getcapacitor.PluginCall
import com.getcapacitor.PluginMethod
import com.getcapacitor.annotation.CapacitorPlugin
import java.util.UUID

@CapacitorPlugin(
    name = "BlePeripheral",
    permissions = [
        com.getcapacitor.annotation.Permission(
            strings = [Manifest.permission.BLUETOOTH_ADMIN, Manifest.permission.BLUETOOTH],
            alias = "bluetooth"
        ),
        com.getcapacitor.annotation.Permission(
            strings = [Manifest.permission.BLUETOOTH_ADVERTISE],
            alias = "advertise"
        )
    ]
)
class BlePeripheralPlugin : Plugin() {
    var gattServer: BluetoothGattServer? = null
    private var bluetoothManager: BluetoothManager? = null
    private var bluetoothAdapter: BluetoothAdapter? = null
    private var advertiser: BluetoothLeAdvertiser? = null

    override fun load() {
        bluetoothManager = context.getSystemService(Context.BLUETOOTH_SERVICE) as BluetoothManager
        bluetoothAdapter = bluetoothManager?.adapter
        advertiser = bluetoothAdapter?.bluetoothLeAdvertiser
        android.util.Log.d("BlePeripheralPlugin", "Plugin loaded. Advertiser available: ${advertiser != null}")
    }

    @PluginMethod
    public fun startAdvertising(call: PluginCall) {
        android.util.Log.d("BlePeripheralPlugin", "startAdvertising called!")
        val serviceName = call.getString("serviceName") ?: "Shadow Warrior Arena"
        val serviceUuid = call.getString("serviceUuid") ?: "6E400001-B5A3-F393-E0A9-E50E24DCCA9E"

        android.util.Log.d("BlePeripheralPlugin", "Service: $serviceName, UUID: $serviceUuid")

        if (!hasPermissionPublic("bluetooth") || !hasPermissionPublic("advertise")) {
            android.util.Log.e("BlePeripheralPlugin", "Bluetooth permissions not granted")
            call.reject("Bluetooth permissions not granted")
            return
        }

        try {
            // Create GATT server
            val gattServerCallback = ArenaGattServerCallback(this)
            gattServer = bluetoothManager?.openGattServer(context, gattServerCallback)

            if (gattServer == null) {
                call.reject("Failed to open GATT server")
                return
            }

            // Add UART service to GATT server
            val service = createUartService(serviceUuid)
            gattServer?.addService(service)

            android.util.Log.d("BlePeripheralPlugin", "GATT service added: $serviceUuid")

            // Setup BLE advertising
            val advertiseSettings = AdvertiseSettings.Builder()
                .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY)
                .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_HIGH)
                .setConnectable(true)
                .build()

            val advertiseData = AdvertiseData.Builder()
                .setIncludeDeviceName(true)  // Include device name in main advertisement for external scanners
                .setIncludeTxPowerLevel(false)  // Reduce packet size
                .addServiceUuid(ParcelUuid(UUID.fromString(serviceUuid)))
                .build()

            // Use scan response with additional data if needed (has separate 31-byte limit)
            val scanResponse = AdvertiseData.Builder()
                .build()

            // Start advertising with scan response
            advertiser?.startAdvertising(advertiseSettings, advertiseData, scanResponse, object : AdvertiseCallback() {
                override fun onStartSuccess(settingsInEffect: AdvertiseSettings?) {
                    super.onStartSuccess(settingsInEffect)
                    android.util.Log.d("BlePeripheralPlugin", "BLE advertising started successfully")
                    call.resolve()
                    emitEvent("onAdvertisingStarted", JSObject())
                }

                override fun onStartFailure(errorCode: Int) {
                    super.onStartFailure(errorCode)
                    val errorMsg = when (errorCode) {
                        ADVERTISE_FAILED_DATA_TOO_LARGE -> "Advertise data too large"
                        ADVERTISE_FAILED_TOO_MANY_ADVERTISERS -> "Too many advertisers"
                        ADVERTISE_FAILED_ALREADY_STARTED -> "Advertising already started"
                        ADVERTISE_FAILED_INTERNAL_ERROR -> "Internal error"
                        else -> "Unknown error: $errorCode"
                    }
                    android.util.Log.e("BlePeripheralPlugin", "BLE advertising failed: $errorMsg")
                    call.reject("Failed to start advertising: $errorMsg")
                }
            })

        } catch (e: Exception) {
            android.util.Log.e("BlePeripheralPlugin", "Error starting advertising: ${e.message}", e)
            call.reject("Error starting advertising: ${e.message}")
        }
    }

    @PluginMethod
    public fun stopAdvertising(call: PluginCall) {
        try {
            // Stop advertising callback
            gattServer?.close()
            gattServer = null
            call.resolve()
            emitEvent("onAdvertisingStopped", JSObject())
        } catch (e: Exception) {
            call.reject("Error stopping advertising: ${e.message}")
            android.util.Log.e("BlePeripheralPlugin", "Error stopping advertising: ${e.message}")
        }
    }

    @PluginMethod
    public fun sendData(call: PluginCall) {
        val dataArray = call.getArray("data")
        val txCharUuid = call.getString("txCharUuid") ?: "6E400003-B5A3-F393-E0A9-E50E24DCCA9E"

        try {
            if (dataArray == null) {
                call.reject("Data is required")
                return
            }

            val data = (0 until dataArray.length()).map { i ->
                dataArray.getInt(i)
            }

            android.util.Log.d("BlePeripheralPlugin", "sendData: sending ${data.size} bytes to $txCharUuid")

            val byteArray = data.map { it.toByte() }.toByteArray()
            val txChar = gattServer?.services?.find { service ->
                service.characteristics.any { it.uuid.toString() == txCharUuid }
            }?.characteristics?.find { it.uuid.toString() == txCharUuid }

            if (txChar != null) {
                txChar.value = byteArray
                val notified = gattServer?.notifyCharacteristicChanged(null, txChar, false) ?: false
                android.util.Log.d("BlePeripheralPlugin", "Characteristic notification sent: $notified")
                call.resolve()
            } else {
                android.util.Log.e("BlePeripheralPlugin", "TX characteristic not found: $txCharUuid")
                call.reject("TX characteristic not found")
            }
        } catch (e: Exception) {
            call.reject("Error sending data: ${e.message}")
            android.util.Log.e("BlePeripheralPlugin", "Error sending data: ${e.message}", e)
        }
    }

    private fun createUartService(serviceUuid: String): BluetoothGattService {
        val service = BluetoothGattService(
            java.util.UUID.fromString(serviceUuid),
            BluetoothGattService.SERVICE_TYPE_PRIMARY
        )

        // TX Characteristic (Peripheral → Central, notify)
        val txChar = BluetoothGattCharacteristic(
            java.util.UUID.fromString("6E400003-B5A3-F393-E0A9-E50E24DCCA9E"),
            BluetoothGattCharacteristic.PROPERTY_NOTIFY,
            BluetoothGattCharacteristic.PERMISSION_READ
        )
        txChar.addDescriptor(createClientCharacteristicConfigDescriptor())

        // RX Characteristic (Central → Peripheral, write)
        val rxChar = BluetoothGattCharacteristic(
            java.util.UUID.fromString("6E400002-B5A3-F393-E0A9-E50E24DCCA9E"),
            BluetoothGattCharacteristic.PROPERTY_WRITE,
            BluetoothGattCharacteristic.PERMISSION_WRITE
        )

        service.addCharacteristic(txChar)
        service.addCharacteristic(rxChar)

        return service
    }

    private fun createClientCharacteristicConfigDescriptor(): BluetoothGattDescriptor {
        return BluetoothGattDescriptor(
            java.util.UUID.fromString("00002902-0000-1000-8000-00805f9b34fb"),
            BluetoothGattDescriptor.PERMISSION_READ or BluetoothGattDescriptor.PERMISSION_WRITE
        )
    }

    fun emitEvent(eventName: String, data: JSObject) {
        notifyListeners(eventName, data)
    }

    public fun hasPermissionPublic(permission: String): Boolean {
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            ActivityCompat.checkSelfPermission(
                context,
                if (permission == "advertise") Manifest.permission.BLUETOOTH_ADVERTISE
                else Manifest.permission.BLUETOOTH
            ) == PackageManager.PERMISSION_GRANTED
        } else {
            true
        }
    }
}
