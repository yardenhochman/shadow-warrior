package com.shadow_warrior.ble

import android.Manifest
import android.bluetooth.*
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.ActivityCompat
import com.getcapacitor.JSObject
import com.getcapacitor.Plugin
import com.getcapacitor.PluginCall
import com.getcapacitor.annotation.CapacitorPlugin
import com.getcapacitor.annotation.Permission
import com.getcapacitor.annotation.PermissionCallback

@CapacitorPlugin(
    permissions = [
        Permission(
            strings = [Manifest.permission.BLUETOOTH_ADMIN, Manifest.permission.BLUETOOTH],
            alias = "bluetooth"
        ),
        Permission(
            strings = [Manifest.permission.BLUETOOTH_ADVERTISE],
            alias = "advertise"
        )
    ]
)
class BlePeripheralPlugin : Plugin() {
    private var gattServer: BluetoothGattServer? = null
    private var bluetoothManager: BluetoothManager? = null
    private var bluetoothAdapter: BluetoothAdapter? = null
    private var advertiser: BluetoothLeAdvertiser? = null

    override fun load() {
        bluetoothManager = context.getSystemService(Context.BLUETOOTH_SERVICE) as BluetoothManager
        bluetoothAdapter = bluetoothManager?.adapter
        advertiser = bluetoothAdapter?.bluetoothLeAdvertiser
    }

    @PluginMethod
    fun startAdvertising(call: PluginCall) {
        val serviceName = call.getString("serviceName") ?: "Shadow Warrior"
        val serviceUuid = call.getString("serviceUuid") ?: "6E400001-B5A3-F393-E0A9-E50E24DCCA9E"

        if (!hasPermission("bluetooth") || !hasPermission("advertise")) {
            requestPermissions("bluetooth")
            call.reject("Bluetooth permissions not granted")
            return
        }

        try {
            val gattServerCallback = ArenaGattServerCallback(this)
            gattServer = bluetoothManager?.openGattServer(context, gattServerCallback)

            // Add UART service
            val service = createUartService(serviceUuid)
            gattServer?.addService(service)

            // Start BLE advertising
            val settings = AdvertiseSettings.Builder()
                .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY)
                .setConnectable(true)
                .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_HIGH)
                .build()

            val data = AdvertiseData.Builder()
                .setIncludeDeviceName(true)
                .setIncludeTxPowerLevel(true)
                .addServiceUuid(android.os.ParcelUuid(java.util.UUID.fromString(serviceUuid)))
                .build()

            advertiser?.startAdvertising(settings, data, object : AdvertiseCallback() {
                override fun onStartSuccess(settingsInEffect: AdvertiseSettings) {
                    super.onStartSuccess(settingsInEffect)
                    call.resolve()
                    notifyListeners("onAdvertisingStarted", JSObject())
                }

                override fun onStartFailure(errorCode: Int) {
                    super.onStartFailure(errorCode)
                    call.reject("Failed to start advertising: $errorCode")
                }
            })
        } catch (e: Exception) {
            call.reject("Error starting advertising: ${e.message}")
        }
    }

    @PluginMethod
    fun stopAdvertising(call: PluginCall) {
        try {
            advertiser?.stopAdvertising(object : AdvertiseCallback() {
                override fun onStartFailure(errorCode: Int) {
                    call.reject("Failed to stop advertising")
                }
            })
            gattServer?.close()
            gattServer = null
            call.resolve()
        } catch (e: Exception) {
            call.reject("Error stopping advertising: ${e.message}")
        }
    }

    @PluginMethod
    fun sendData(call: PluginCall) {
        val data = call.getArray("data")?.toList() as? List<Int>
            ?: return call.reject("Invalid data")
        val txCharUuid = call.getString("txCharUuid")
            ?: "6E400003-B5A3-F393-E0A9-E50E24DCCA9E"

        try {
            val byteArray = data.map { it.toByte() }.toByteArray()
            (gattServer?.services?.find { service ->
                service.characteristics.any { it.uuid.toString() == txCharUuid }
            }?.characteristics?.find { it.uuid.toString() == txCharUuid })?.apply {
                value = byteArray
                gattServer?.notifyCharacteristicChanged(null, this, false)
            }
            call.resolve()
        } catch (e: Exception) {
            call.reject("Error sending data: ${e.message}")
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

    private fun hasPermission(permission: String): Boolean {
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
