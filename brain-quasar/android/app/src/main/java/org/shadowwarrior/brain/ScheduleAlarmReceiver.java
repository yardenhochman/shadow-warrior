package org.shadowwarrior.brain;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.util.Log;

public class ScheduleAlarmReceiver extends BroadcastReceiver {
    private static final String TAG = "ScheduleAlarmReceiver";

    @Override
    public void onReceive(Context context, Intent intent) {
        String alarmId = intent.getStringExtra("alarm_id");
        String alarmType = intent.getStringExtra("alarm_type");

        Log.d(TAG, "Alarm triggered: " + alarmId + " type: " + alarmType);

        // Send broadcast to app
        Intent appIntent = new Intent("org.shadowwarrior.brain.SCHEDULE_ALARM");
        appIntent.putExtra("alarm_id", alarmId);
        appIntent.putExtra("alarm_type", alarmType);
        context.sendBroadcast(appIntent);
    }
}