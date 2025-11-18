package org.shadowwarrior.brain

import com.getcapacitor.Plugin
import com.getcapacitor.PluginCall
import com.getcapacitor.PluginMethod
import com.getcapacitor.annotation.CapacitorPlugin

@CapacitorPlugin(name = "ForegroundService")
class ForegroundServicePlugin : Plugin() {

    @PluginMethod
    fun start(call: PluginCall) {
        try {
            ShadowWarriorForegroundService.start(context)
            call.resolve()
        } catch (e: Exception) {
            call.reject("Failed to start foreground service: ${e.message}")
        }
    }

    @PluginMethod
    fun stop(call: PluginCall) {
        try {
            ShadowWarriorForegroundService.stop(context)
            call.resolve()
        } catch (e: Exception) {
            call.reject("Failed to stop foreground service: ${e.message}")
        }
    }
}
