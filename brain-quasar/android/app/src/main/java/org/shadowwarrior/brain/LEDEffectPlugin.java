package org.shadowwarrior.brain;

import android.content.Context;
import android.content.Intent;
import android.os.Build;
import com.getcapacitor.JSObject;
import com.getcapacitor.JSArray;
import com.getcapacitor.Plugin;
import com.getcapacitor.PluginCall;
import com.getcapacitor.PluginMethod;
import com.getcapacitor.annotation.CapacitorPlugin;
import android.util.Log;

/**
 * Plugin to control native LED effect rendering service.
 * This service runs independently of WebView and keeps rendering LED effects when screen is off.
 * Future extension: Can be expanded to handle full effect rendering and UDP transmission natively.
 */
@CapacitorPlugin(name = "LEDEffectPlugin")
public class LEDEffectPlugin extends Plugin {
    private static final String TAG = "LEDEffectPlugin";

    @Override
    public void load() {
        LEDEffectService.setPluginInstance(this);
        Log.d(TAG, "Plugin loaded and registered with service");
    }

    /**
     * Start background LED effect rendering
     * @param call PluginCall with:
     *   - mode: "warmup" or "fight"
     *   - controllers: array of {host, port} objects
     */
    @PluginMethod
    public void startEffectService(PluginCall call) {
        try {
            Context context = getContext();
            String mode = call.getString("mode", "warmup");

            // Get controllers array from call parameters
            JSArray controllers = call.getArray("controllers");
            String controllersList = "";
            if (controllers != null) {
                controllersList = controllers.toString();
                Log.d(TAG, "Controllers from plugin call: " + controllersList);
            }

            Intent serviceIntent = new Intent(context, LEDEffectService.class);
            serviceIntent.setAction("START_EFFECT");
            serviceIntent.putExtra("mode", mode);
            if (!controllersList.isEmpty()) {
                serviceIntent.putExtra("controllers", controllersList);
            }

            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(serviceIntent);
            } else {
                context.startService(serviceIntent);
            }

            Log.d(TAG, "Started LED effect service with mode: " + mode + ", controllers: " + controllersList);
            call.resolve();
        } catch (Exception e) {
            Log.e(TAG, "Error starting LED effect service", e);
            call.reject("Failed to start LED effect service: " + e.getMessage());
        }
    }

    /**
     * Stop background LED effect rendering
     */
    @PluginMethod
    public void stopEffectService(PluginCall call) {
        try {
            Context context = getContext();
            Intent serviceIntent = new Intent(context, LEDEffectService.class);
            serviceIntent.setAction("STOP_EFFECT");

            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(serviceIntent);
            } else {
                context.startService(serviceIntent);
            }

            Log.d(TAG, "Stopped LED effect service");
            call.resolve();
        } catch (Exception e) {
            Log.e(TAG, "Error stopping LED effect service", e);
            call.reject("Failed to stop LED effect service: " + e.getMessage());
        }
    }

    /**
     * Update LED effect state (frame data, punch detected, etc.)
     * @param call PluginCall with:
     *   - type: "frame" | "punch" | "shout" | "state"
     *   - data: relevant data for the update
     */
    @PluginMethod
    public void updateEffectState(PluginCall call) {
        try {
            Context context = getContext();
            String type = call.getString("type", "state");

            Intent serviceIntent = new Intent(context, LEDEffectService.class);
            serviceIntent.setAction("UPDATE_STATE");
            serviceIntent.putExtra("updateType", type);
            serviceIntent.putExtra("data", call.getString("data", ""));

            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(serviceIntent);
            } else {
                context.startService(serviceIntent);
            }

            Log.d(TAG, "Updated LED effect state: " + type);
            call.resolve();
        } catch (Exception e) {
            Log.e(TAG, "Error updating LED effect state", e);
            call.reject("Failed to update LED effect state: " + e.getMessage());
        }
    }

    /**
     * Called from native service to notify JavaScript of events
     */
    public void notifyEventToJS(String eventName, JSObject data) {
        notifyListeners(eventName, data);
    }
}
