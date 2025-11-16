package org.shadowwarrior.brain;

import android.app.AlarmManager;
import android.app.PendingIntent;
import android.content.Context;
import android.content.Intent;
import android.os.Build;
import com.getcapacitor.JSObject;
import com.getcapacitor.Plugin;
import com.getcapacitor.PluginCall;
import com.getcapacitor.PluginMethod;
import com.getcapacitor.annotation.CapacitorPlugin;
import java.util.Calendar;

@CapacitorPlugin(name = "NativeSchedule")
public class NativeSchedulePlugin extends Plugin {

    @PluginMethod
    public void scheduleDailyAlarm(PluginCall call) {
        // Extract parameters
        String id = call.getString("id");
        int hour = call.getInt("hour");
        int minute = call.getInt("minute");
        String type = call.getString("type"); // 'activate' or 'suspend'

        // Get AlarmManager
        Context context = getContext();
        AlarmManager alarmManager = (AlarmManager) context.getSystemService(Context.ALARM_SERVICE);

        // Create intent for broadcast receiver
        Intent intent = new Intent(context, ScheduleAlarmReceiver.class);
        intent.putExtra("alarm_id", id);
        intent.putExtra("alarm_type", type);

        // Create PendingIntent with unique request code based on ID
        int requestCode = id.hashCode();
        PendingIntent pendingIntent = PendingIntent.getBroadcast(
            context,
            requestCode,
            intent,
            PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE
        );

        // Calculate next alarm time
        Calendar calendar = Calendar.getInstance();
        calendar.set(Calendar.HOUR_OF_DAY, hour);
        calendar.set(Calendar.MINUTE, minute);
        calendar.set(Calendar.SECOND, 0);

        // If time has passed today, schedule for tomorrow
        if (calendar.getTimeInMillis() <= System.currentTimeMillis()) {
            calendar.add(Calendar.DAY_OF_MONTH, 1);
        }

        // Schedule repeating daily alarm
        alarmManager.setRepeating(
            AlarmManager.RTC_WAKEUP,
            calendar.getTimeInMillis(),
            AlarmManager.INTERVAL_DAY,
            pendingIntent
        );

        call.resolve();
    }

    @PluginMethod
    public void cancelAllAlarms(PluginCall call) {
        Context context = getContext();
        AlarmManager alarmManager = (AlarmManager) context.getSystemService(Context.ALARM_SERVICE);

        // Cancel known alarm IDs
        cancelAlarmById(context, alarmManager, "schedule_activate");
        cancelAlarmById(context, alarmManager, "schedule_suspend");

        call.resolve();
    }

    @PluginMethod
    public void isWithinActiveHours(PluginCall call) {
        int startHour = call.getInt("startHour");
        int startMinute = call.getInt("startMinute");
        int endHour = call.getInt("endHour");
        int endMinute = call.getInt("endMinute");

        Calendar now = Calendar.getInstance();
        int currentMinutes = now.get(Calendar.HOUR_OF_DAY) * 60 + now.get(Calendar.MINUTE);
        int startMinutes = startHour * 60 + startMinute;
        int endMinutes = endHour * 60 + endMinute;

        boolean withinHours;
        if (endMinutes > startMinutes) {
            withinHours = currentMinutes >= startMinutes && currentMinutes < endMinutes;
        } else {
            // Midnight crossing
            withinHours = currentMinutes >= startMinutes || currentMinutes < endMinutes;
        }

        JSObject ret = new JSObject();
        ret.put("withinHours", withinHours);
        call.resolve(ret);
    }

    private void cancelAlarmById(Context context, AlarmManager alarmManager, String id) {
        Intent intent = new Intent(context, ScheduleAlarmReceiver.class);
        int requestCode = id.hashCode();
        PendingIntent pendingIntent = PendingIntent.getBroadcast(
            context,
            requestCode,
            intent,
            PendingIntent.FLAG_NO_CREATE | PendingIntent.FLAG_IMMUTABLE
        );

        if (pendingIntent != null) {
            alarmManager.cancel(pendingIntent);
            pendingIntent.cancel();
        }
    }
}