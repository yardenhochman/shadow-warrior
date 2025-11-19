package org.shadowwarrior.brain;

import android.content.Context;
import android.content.Intent;
import android.os.Build;
import com.getcapacitor.JSObject;
import com.getcapacitor.Plugin;
import com.getcapacitor.PluginCall;
import com.getcapacitor.PluginMethod;
import com.getcapacitor.annotation.CapacitorPlugin;

/**
 * Plugin to control native accelerometer monitoring service.
 * This service runs independently of WebView and detects punches even when screen is off.
 * Punch events are sent from native code to JavaScript.
 */
@CapacitorPlugin(name = "AccelerometerPlugin")
public class AccelerometerPlugin extends Plugin {

    @Override
    public void load() {
        // Register this plugin instance with the service so it can send events
        AccelerometerService.setPluginInstance(this);
        android.util.Log.d("AccelerometerPlugin", "Plugin loaded and registered with service");
    }

    @PluginMethod
    public void startMonitoring(PluginCall call) {
        Context context = getContext();
        
        // Get configuration parameters
        float threshold = call.getFloat("threshold", 2.0f);
        int cooldownMs = call.getInt("cooldownMs", 200);
        float baselineAlpha = call.getFloat("baselineAlpha", 0.005f);
        float baselineX = call.getFloat("baselineX", 0.0f);
        float baselineY = call.getFloat("baselineY", 0.0f);
        float baselineZ = call.getFloat("baselineZ", 9.81f);
        float accelAlpha = call.getFloat("accelAlpha", 0.4f);
        
        // Start the native foreground service
        Intent serviceIntent = new Intent(context, AccelerometerService.class);
        serviceIntent.putExtra("threshold", threshold);
        serviceIntent.putExtra("cooldownMs", (long) cooldownMs);
        serviceIntent.putExtra("baselineAlpha", baselineAlpha);
        serviceIntent.putExtra("baselineX", baselineX);
        serviceIntent.putExtra("baselineY", baselineY);
        serviceIntent.putExtra("baselineZ", baselineZ);
        serviceIntent.putExtra("accelAlpha", accelAlpha);
        
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            context.startForegroundService(serviceIntent);
        } else {
            context.startService(serviceIntent);
        }
        
        android.util.Log.d("AccelerometerPlugin", 
            "Started native accelerometer monitoring - threshold: " + threshold + "G, cooldown: " + cooldownMs + "ms, baselineAlpha: " + baselineAlpha);
        call.resolve();
    }

    @PluginMethod
    public void stopMonitoring(PluginCall call) {
        Context context = getContext();
        
        // Stop the service
        Intent serviceIntent = new Intent(context, AccelerometerService.class);
        context.stopService(serviceIntent);
        
        android.util.Log.d("AccelerometerPlugin", "Stopped native accelerometer monitoring");
        call.resolve();
    }
    
    @PluginMethod
    public void updateConfig(PluginCall call) {
        float threshold = call.getFloat("threshold", 2.0f);
        int cooldownMs = call.getInt("cooldownMs", 200);
        float baselineAlpha = call.getFloat("baselineAlpha", 0.01f);
        float baselineX = call.getFloat("baselineX", 0.0f);
        float baselineY = call.getFloat("baselineY", 0.0f);
        float baselineZ = call.getFloat("baselineZ", 9.81f);
        float accelAlpha = call.getFloat("accelAlpha", 0.3f);
        
        // Send config update to service via broadcast or direct call
        // For simplicity, we'll restart the service with new config
        Context context = getContext();
        Intent serviceIntent = new Intent(context, AccelerometerService.class);
        serviceIntent.putExtra("threshold", threshold);
        serviceIntent.putExtra("cooldownMs", (long) cooldownMs);
        serviceIntent.putExtra("baselineAlpha", baselineAlpha);
        serviceIntent.putExtra("baselineX", baselineX);
        serviceIntent.putExtra("baselineY", baselineY);
        serviceIntent.putExtra("baselineZ", baselineZ);
        serviceIntent.putExtra("accelAlpha", accelAlpha);
        
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            context.startForegroundService(serviceIntent);
        } else {
            context.startService(serviceIntent);
        }
        
        android.util.Log.d("AccelerometerPlugin", "Updated accelerometer config");
        call.resolve();
    }
    
    /**
     * Called by AccelerometerService when a punch is detected.
     * This sends the event to JavaScript.
     */
    public void notifyPunchDetected(float force, float magnitude, float x, float y, float z, long timestamp) {
        JSObject punchData = new JSObject();
        punchData.put("force", force);
        punchData.put("magnitude", magnitude);
        
        JSObject raw = new JSObject();
        raw.put("x", x);
        raw.put("y", y);
        raw.put("z", z);
        punchData.put("raw", raw);
        
        punchData.put("timestamp", timestamp);
        
        // Notify all listeners (JavaScript event listeners)
        notifyListeners("punchDetected", punchData);
        
        android.util.Log.d("AccelerometerPlugin", "Sent punch event to JavaScript - force: " + force);
    }
}
