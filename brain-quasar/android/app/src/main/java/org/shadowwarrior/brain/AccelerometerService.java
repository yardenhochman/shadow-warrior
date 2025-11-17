package org.shadowwarrior.brain;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.hardware.Sensor;
import android.hardware.SensorEvent;
import android.hardware.SensorEventListener;
import android.hardware.SensorManager;
import android.os.Build;
import android.os.IBinder;
import android.os.PowerManager;
import androidx.core.app.NotificationCompat;

/**
 * Native foreground service for accelerometer-based punch detection.
 * This runs independently of the WebView and works even when screen is off.
 * Punch events are sent to JavaScript via the AccelerometerPlugin.
 */
public class AccelerometerService extends Service implements SensorEventListener {
    private static final String CHANNEL_ID = "accelerometer_monitoring_channel";
    private static final int NOTIFICATION_ID = 1002;
    
    // Punch detection configuration
    private static final float DEFAULT_THRESHOLD = 2.0f; // 2G threshold
    private static final long DEFAULT_COOLDOWN_MS = 200; // 200ms between punches
    
    private PowerManager.WakeLock wakeLock;
    private SensorManager sensorManager;
    private Sensor accelerometer;
    
    // Punch detection state
    private float threshold = DEFAULT_THRESHOLD;
    private long cooldownMs = DEFAULT_COOLDOWN_MS;
    private long lastPunchTime = 0;
    
    // Baseline for drift correction (gravity)
    private float baselineX = 0f;
    private float baselineY = 0f;
    private float baselineZ = 9.81f;
    private static final float BASELINE_ALPHA = 0.01f; // Smoothing factor
    
    // Plugin instance for sending events
    private static AccelerometerPlugin pluginInstance = null;

    public static void setPluginInstance(AccelerometerPlugin plugin) {
        pluginInstance = plugin;
    }

    @Override
    public void onCreate() {
        super.onCreate();
        createNotificationChannel();
        
        // Acquire wake lock to keep CPU and sensors active even with screen off
        PowerManager powerManager = (PowerManager) getSystemService(Context.POWER_SERVICE);
        wakeLock = powerManager.newWakeLock(
            PowerManager.PARTIAL_WAKE_LOCK, 
            "ShadowWarrior::AccelerometerService"
        );
        wakeLock.setReferenceCounted(false);
        wakeLock.acquire();
        
        // Initialize sensor manager
        sensorManager = (SensorManager) getSystemService(Context.SENSOR_SERVICE);
        accelerometer = sensorManager.getDefaultSensor(Sensor.TYPE_ACCELEROMETER);
        
        if (accelerometer != null) {
            // Register with SENSOR_DELAY_GAME (20ms, ~50Hz) - good balance for punch detection
            boolean registered = sensorManager.registerListener(
                this,
                accelerometer,
                SensorManager.SENSOR_DELAY_GAME
            );
            android.util.Log.d("AccelerometerService", "Sensor registered: " + registered);
        } else {
            android.util.Log.e("AccelerometerService", "No accelerometer sensor found!");
        }
        
        android.util.Log.d("AccelerometerService", "Service created with native sensor monitoring");
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        // Get configuration from intent if provided
        if (intent != null) {
            threshold = intent.getFloatExtra("threshold", DEFAULT_THRESHOLD);
            cooldownMs = intent.getLongExtra("cooldownMs", DEFAULT_COOLDOWN_MS);
            android.util.Log.d("AccelerometerService", 
                "Config - threshold: " + threshold + "G, cooldown: " + cooldownMs + "ms");
        }
        
        // Create notification
        Notification notification = createNotification();
        
        // Start foreground service with SPECIAL_USE type
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(
                NOTIFICATION_ID, 
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE
            );
        } else {
            startForeground(NOTIFICATION_ID, notification);
        }
        
        android.util.Log.d("AccelerometerService", "Foreground service started with native accelerometer");
        
        return START_STICKY; // Keep service running
    }

    @Override
    public void onSensorChanged(SensorEvent event) {
        if (event.sensor.getType() != Sensor.TYPE_ACCELEROMETER) {
            return;
        }
        
        float x = event.values[0];
        float y = event.values[1];
        float z = event.values[2];
        
        // Calculate magnitude with baseline correction
        float deltaX = x - baselineX;
        float deltaY = y - baselineY;
        float deltaZ = z - baselineZ;
        
        float magnitude = (float) Math.sqrt(deltaX * deltaX + deltaY * deltaY + deltaZ * deltaZ);
        
        // Detect punch based on threshold
        if (magnitude > threshold) {
            long now = System.currentTimeMillis();
            
            // Check cooldown period
            if (now - lastPunchTime >= cooldownMs) {
                lastPunchTime = now;
                
                // Normalize force to 0-1 range (assume max 6G)
                float normalizedForce = Math.min(magnitude / 6.0f, 1.0f);
                
                // Send punch event to JavaScript
                sendPunchEvent(normalizedForce, magnitude, x, y, z, now);
                
                android.util.Log.d("AccelerometerService", 
                    "Punch detected! magnitude=" + magnitude + "G, force=" + normalizedForce);
            }
        }
        
        // Update baseline using exponential moving average (drift correction)
        baselineX = BASELINE_ALPHA * x + (1 - BASELINE_ALPHA) * baselineX;
        baselineY = BASELINE_ALPHA * y + (1 - BASELINE_ALPHA) * baselineY;
        baselineZ = BASELINE_ALPHA * z + (1 - BASELINE_ALPHA) * baselineZ;
    }

    @Override
    public void onAccuracyChanged(Sensor sensor, int accuracy) {
        // Not used, but required by SensorEventListener interface
    }
    
    private void sendPunchEvent(float force, float magnitude, float x, float y, float z, long timestamp) {
        if (pluginInstance != null) {
            pluginInstance.notifyPunchDetected(force, magnitude, x, y, z, timestamp);
        } else {
            android.util.Log.w("AccelerometerService", "Plugin instance not set, cannot send punch event");
        }
    }
    
    public void updateConfig(float newThreshold, long newCooldownMs) {
        this.threshold = newThreshold;
        this.cooldownMs = newCooldownMs;
        android.util.Log.d("AccelerometerService", 
            "Config updated - threshold: " + threshold + "G, cooldown: " + cooldownMs + "ms");
    }

    @Override
    public void onDestroy() {
        super.onDestroy();
        
        // Unregister sensor listener
        if (sensorManager != null) {
            sensorManager.unregisterListener(this);
            android.util.Log.d("AccelerometerService", "Sensor listener unregistered");
        }
        
        // Release wake lock
        if (wakeLock != null && wakeLock.isHeld()) {
            wakeLock.release();
        }
        
        stopForeground(true);
        android.util.Log.d("AccelerometerService", "Service destroyed");
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null; // Not a bound service
    }

    private void createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            NotificationChannel channel = new NotificationChannel(
                CHANNEL_ID,
                "Punch Detection",
                NotificationManager.IMPORTANCE_LOW
            );
            channel.setDescription("Native accelerometer monitoring for punch detection");
            channel.setShowBadge(false);
            
            NotificationManager manager = getSystemService(NotificationManager.class);
            if (manager != null) {
                manager.createNotificationChannel(channel);
            }
        }
    }

    private Notification createNotification() {
        Intent notificationIntent = new Intent(this, MainActivity.class);
        PendingIntent pendingIntent = PendingIntent.getActivity(
            this, 
            0, 
            notificationIntent, 
            PendingIntent.FLAG_IMMUTABLE
        );

        return new NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("Shadow Warrior")
            .setContentText("Monitoring for punches (Native)")
            .setSmallIcon(android.R.drawable.ic_menu_compass)
            .setContentIntent(pendingIntent)
            .setOngoing(true)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .build();
    }
}
