package org.shadowwarrior.brain;

import android.app.*;
import android.content.Context;
import android.content.Intent;
import android.os.Build;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.os.PowerManager;
import androidx.core.app.NotificationCompat;
import android.util.Log;
import java.util.ArrayList;
import java.util.List;

/**
 * Background service for LED effect rendering.
 * Handles full effect rendering and UDP transmission natively, independent of WebView.
 * Keeps rendering running even when screen is off or app is in background.
 */
public class LEDEffectService extends Service {
    private static final String TAG = "LEDEffectService";
    private static final int NOTIFICATION_ID = 2001;
    private static final String CHANNEL_ID = "led_effect_service";
    private static final String CHANNEL_NAME = "LED Effect Service";
    private static final int LED_COUNT = 180;

    private static LEDEffectPlugin pluginInstance = null;
    private String currentMode = "none";
    private boolean isRendering = false;
    private Handler renderHandler;

    private LEDEffectGenerator effectGenerator;
    private List<UDPFrameTransmitter> transmitters = new ArrayList<>();
    private PowerManager.WakeLock wakeLock;

    public static void setPluginInstance(LEDEffectPlugin plugin) {
        pluginInstance = plugin;
    }

    @Override
    public void onCreate() {
        super.onCreate();
        Log.d(TAG, "LED Effect Service created");
        createNotificationChannel();
        renderHandler = new Handler(Looper.getMainLooper());
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent == null) {
            Log.w(TAG, "Intent is null, returning START_STICKY");
            return START_STICKY;
        }

        String action = intent.getAction();
        Log.d(TAG, "LED Effect Service started with action: " + action);

        if ("START_EFFECT".equals(action)) {
            handleStartEffect(intent);
        } else if ("STOP_EFFECT".equals(action)) {
            handleStopEffect();
        } else if ("UPDATE_STATE".equals(action)) {
            handleUpdateState(intent);
        }

        return START_STICKY;
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    @Override
    public void onDestroy() {
        super.onDestroy();
        Log.d(TAG, "LED Effect Service destroyed");
        isRendering = false;
        stopForeground(true);
    }

    /**
     * Handle starting LED effect rendering
     */
    private void handleStartEffect(Intent intent) {
        final String mode = intent.getStringExtra("mode");
        final String finalMode = (mode == null) ? "warmup" : mode;

        currentMode = finalMode;
        isRendering = true;

        // Acquire WakeLock to keep CPU awake when screen is off
        try {
            PowerManager pm = (PowerManager) getSystemService(Context.POWER_SERVICE);
            if (pm != null && wakeLock == null) {
                wakeLock = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "shadowwarrior:led-rendering");
                wakeLock.acquire();
                Log.d(TAG, "WakeLock acquired for LED rendering");
            }
        } catch (Exception e) {
            Log.e(TAG, "Failed to acquire WakeLock", e);
        }

        // Start foreground service to keep running in background
        Notification notification = createNotification();
        startForeground(NOTIFICATION_ID, notification);

        Log.d(TAG, "Starting LED effect rendering in " + finalMode + " mode");

        // Initialize native effect generator
        effectGenerator = new LEDEffectGenerator(LED_COUNT);
        effectGenerator.setMode(finalMode);

        // Initialize UDP transmitters for controllers
        transmitters.clear();

        // Try to get controllers from intent
        String controllersJson = intent.getStringExtra("controllers");
        if (controllersJson != null && !controllersJson.isEmpty()) {
            try {
                // Parse JSON array of controllers
                org.json.JSONArray jsonArray = new org.json.JSONArray(controllersJson);
                for (int i = 0; i < jsonArray.length(); i++) {
                    org.json.JSONObject controller = jsonArray.getJSONObject(i);
                    String host = controller.getString("host");
                    int port = controller.optInt("port", 21324);

                    UDPFrameTransmitter transmitter = new UDPFrameTransmitter(host, port);
                    try {
                        transmitter.connect();
                        transmitters.add(transmitter);
                        Log.d(TAG, "Initialized UDP transmitter for " + host + ":" + port);
                    } catch (Exception e) {
                        Log.e(TAG, "Failed to initialize UDP transmitter for " + host + ":" + port, e);
                    }
                }
            } catch (Exception e) {
                Log.e(TAG, "Failed to parse controllers JSON: " + controllersJson, e);
            }
        }

        // If no controllers provided or parsing failed, use default localhost
        if (transmitters.isEmpty()) {
            Log.w(TAG, "No controllers configured, using default localhost:21324");
            UDPFrameTransmitter defaultTransmitter = new UDPFrameTransmitter("127.0.0.1", 21324);
            try {
                defaultTransmitter.connect();
                transmitters.add(defaultTransmitter);
                Log.d(TAG, "Initialized default UDP transmitter");
            } catch (Exception e) {
                Log.e(TAG, "Failed to initialize default UDP transmitter", e);
            }
        }

        // Notify JavaScript that service started
        if (pluginInstance != null) {
            renderHandler.post(() -> {
                com.getcapacitor.JSObject data = new com.getcapacitor.JSObject();
                data.put("mode", finalMode);
                pluginInstance.notifyEventToJS("ledEffectServiceStarted", data);
            });
        }

        // Start a render loop that runs on a background thread
        // This keeps rendering even when screen is off
        startRenderLoop();
    }

    /**
     * Handle stopping LED effect rendering
     */
    private void handleStopEffect() {
        currentMode = "none";
        isRendering = false;

        // Release WakeLock when rendering stops
        if (wakeLock != null && wakeLock.isHeld()) {
            try {
                wakeLock.release();
                Log.d(TAG, "WakeLock released");
            } catch (Exception e) {
                Log.e(TAG, "Failed to release WakeLock", e);
            }
            wakeLock = null;
        }

        Log.d(TAG, "Stopping LED effect rendering");

        // Send black frame to clear LEDs
        if (!transmitters.isEmpty()) {
            try {
                byte[] blackFrame = effectGenerator.generateBlackFrame();
                for (UDPFrameTransmitter transmitter : transmitters) {
                    transmitter.sendFrame(blackFrame);
                }
            } catch (Exception e) {
                Log.e(TAG, "Failed to send black frame", e);
            }
        }

        // Disconnect UDP transmitters
        for (UDPFrameTransmitter transmitter : transmitters) {
            transmitter.disconnect();
        }
        transmitters.clear();

        // Notify JavaScript that service stopped
        if (pluginInstance != null) {
            renderHandler.post(() -> {
                com.getcapacitor.JSObject data = new com.getcapacitor.JSObject();
                data.put("mode", "none");
                pluginInstance.notifyEventToJS("ledEffectServiceStopped", data);
            });
        }

        // Stop foreground service
        stopForeground(true);
        stopSelf();
    }

    /**
     * Handle state updates (punch detected, shout detected, power level, etc.)
     */
    private void handleUpdateState(Intent intent) {
        String updateType = intent.getStringExtra("updateType");
        String data = intent.getStringExtra("data");

        Log.d(TAG, "Updating LED effect state: " + updateType + ", data: " + data);

        try {
            // Handle different update types and trigger effects
            if (effectGenerator != null) {
                switch (updateType) {
                    case "punch":
                        // Trigger punch pulse with intensity from data
                        // Data may be a JSON string or plain number
                        float punchIntensity = parseIntensity(data);
                        effectGenerator.triggerPunch(punchIntensity);
                        Log.d(TAG, "Punch triggered with intensity: " + punchIntensity);
                        break;

                    case "shout":
                        // Trigger shout pulse with intensity from data
                        float shoutIntensity = parseIntensity(data);
                        effectGenerator.triggerShout(shoutIntensity);
                        Log.d(TAG, "Shout triggered with intensity: " + shoutIntensity);
                        break;

                    case "power":
                        // Update power level for energy bar effect
                        float powerLevel = parseIntensity(data);
                        effectGenerator.updatePowerLevel(powerLevel);
                        Log.d(TAG, "Power level updated: " + powerLevel);
                        break;

                    case "state":
                        // Generic state update - pass to JavaScript
                        break;

                    default:
                        Log.w(TAG, "Unknown update type: " + updateType);
                }
            }
        } catch (Exception e) {
            Log.e(TAG, "Failed to parse update data: " + data, e);
        }

        if (pluginInstance != null) {
            renderHandler.post(() -> {
                com.getcapacitor.JSObject eventData = new com.getcapacitor.JSObject();
                eventData.put("type", updateType);
                eventData.put("data", data);
                pluginInstance.notifyEventToJS("ledEffectStateUpdated", eventData);
            });
        }
    }

    /**
     * Start background render loop
     * This runs independently and keeps rendering even when screen is off
     * Generates frames natively and sends via UDP
     */
    private void startRenderLoop() {
        new Thread(() -> {
            Log.d(TAG, "Native render loop started on background thread");
            final int FPS = 30;
            final long frameTimeMs = 1000 / FPS;
            long frameCount = 0;

            while (isRendering && !currentMode.equals("none")) {
                long frameStart = System.currentTimeMillis();

                try {
                    // Generate frame natively
                    if (effectGenerator != null) {
                        byte[] frameData = effectGenerator.generateFrame();

                        // Send to all UDP controllers
                        for (UDPFrameTransmitter transmitter : transmitters) {
                            try {
                                transmitter.sendFrame(frameData);
                            } catch (Exception e) {
                                Log.w(TAG, "Failed to send frame to transmitter", e);
                            }
                        }

                        frameCount++;
                        if (frameCount % 30 == 0) {
                            Log.d(TAG, "Native render loop active: " + frameCount + " frames rendered, mode=" + currentMode);
                        }
                    }
                } catch (Exception e) {
                    Log.e(TAG, "Error in render loop", e);
                }

                // Frame rate limiting
                long frameEnd = System.currentTimeMillis();
                long sleepTime = frameTimeMs - (frameEnd - frameStart);

                if (sleepTime > 0) {
                    try {
                        Thread.sleep(sleepTime);
                    } catch (InterruptedException e) {
                        Log.d(TAG, "Render loop interrupted");
                        break;
                    }
                }
            }

            Log.d(TAG, "Native render loop stopped after " + frameCount + " frames");
        }).start();
    }

    /**
     * Helper method to parse intensity from either JSON string or plain number
     */
    private float parseIntensity(String data) throws Exception {
        if (data == null || data.isEmpty()) {
            return 0.0f;
        }

        // Try to parse as JSON first (from {"intensity": 0.5})
        try {
            org.json.JSONObject json = new org.json.JSONObject(data);
            return (float) json.optDouble("intensity", 0.0);
        } catch (org.json.JSONException e) {
            // Not JSON, try as plain number
            return Float.parseFloat(data);
        }
    }

    private void createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            NotificationManager notificationManager =
                    (NotificationManager) getSystemService(Context.NOTIFICATION_SERVICE);

            NotificationChannel channel = new NotificationChannel(
                    CHANNEL_ID,
                    CHANNEL_NAME,
                    NotificationManager.IMPORTANCE_LOW
            );
            channel.setDescription("LED effects rendering in background");
            channel.setShowBadge(false);
            notificationManager.createNotificationChannel(channel);

            Log.d(TAG, "Notification channel created");
        }
    }

    private Notification createNotification() {
        Intent intent = getPackageManager().getLaunchIntentForPackage(getPackageName());
        PendingIntent pendingIntent = PendingIntent.getActivity(
                this,
                0,
                intent,
                PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT
        );

        String notificationText = "LED effects: " + currentMode;

        return new NotificationCompat.Builder(this, CHANNEL_ID)
                .setContentTitle("Shadow Warrior LEDs Active")
                .setContentText(notificationText)
                .setSmallIcon(android.R.drawable.ic_dialog_info)
                .setContentIntent(pendingIntent)
                .setOngoing(true)
                .setPriority(NotificationCompat.PRIORITY_LOW)
                .setCategory(NotificationCompat.CATEGORY_SERVICE)
                .build();
    }
}
