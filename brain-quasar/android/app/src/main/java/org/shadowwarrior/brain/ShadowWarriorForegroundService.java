package org.shadowwarrior.brain;

import android.app.*;
import android.content.Context;
import android.content.Intent;
import android.os.Build;
import android.os.IBinder;
import androidx.core.app.NotificationCompat;
import android.util.Log;

public class ShadowWarriorForegroundService extends Service {

    private static final String TAG = "SWForegroundService";
    private static final int NOTIFICATION_ID = 1001;
    private static final String CHANNEL_ID = "shadow_warrior_service";
    private static final String CHANNEL_NAME = "Shadow Warrior Service";

    public static void start(Context context) {
        Intent intent = new Intent(context, ShadowWarriorForegroundService.class);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            context.startForegroundService(intent);
        } else {
            context.startService(intent);
        }
    }

    public static void stop(Context context) {
        Intent intent = new Intent(context, ShadowWarriorForegroundService.class);
        context.stopService(intent);
    }

    @Override
    public void onCreate() {
        super.onCreate();
        Log.d(TAG, "Foreground service created");
        createNotificationChannel();
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        Log.d(TAG, "Foreground service started");

        // Create notification for foreground service
        Notification notification = createNotification();
        startForeground(NOTIFICATION_ID, notification);

        // Service will continue running in the background
        return START_STICKY;  // Restart service if killed by system
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;  // We don't need binding
    }

    @Override
    public void onDestroy() {
        super.onDestroy();
        Log.d(TAG, "Foreground service destroyed");
    }

    private void createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            NotificationManager notificationManager = (NotificationManager) getSystemService(Context.NOTIFICATION_SERVICE);
            NotificationChannel channel = new NotificationChannel(
                CHANNEL_ID,
                CHANNEL_NAME,
                NotificationManager.IMPORTANCE_LOW  // Low priority, no sound
            );
            channel.setDescription("Keeps Shadow Warrior services running in background");
            channel.setShowBadge(false);
            notificationManager.createNotificationChannel(channel);
            Log.d(TAG, "Notification channel created");
        }
    }

    private Notification createNotification() {
        // Intent to open the app when notification is tapped
        Intent intent = getPackageManager().getLaunchIntentForPackage(getPackageName());
        PendingIntent pendingIntent = PendingIntent.getActivity(
            this,
            0,
            intent,
            PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT
        );

        return new NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("Shadow Warrior Active")
            .setContentText("Training services running")
            .setSmallIcon(android.R.drawable.ic_dialog_info)  // Use default Android icon for now
            .setContentIntent(pendingIntent)
            .setOngoing(true)  // Can't be swiped away
            .setPriority(NotificationCompat.PRIORITY_LOW)  // Low priority, no sound/vibration
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .build();
    }
}
