package org.shadowwarrior.brain;

import android.content.Intent;
import android.os.Build;
import com.getcapacitor.Plugin;
import com.getcapacitor.PluginCall;
import com.getcapacitor.PluginMethod;
import com.getcapacitor.annotation.CapacitorPlugin;

/**
 * Plugin to control foreground music playback service
 */
@CapacitorPlugin(name = "MusicPlaybackPlugin")
public class MusicPlaybackPlugin extends Plugin {

    @PluginMethod
    public void startForegroundService(PluginCall call) {
        String trackName = call.getString("trackName", "Music");
        
        Intent serviceIntent = new Intent(getContext(), MusicPlaybackService.class);
        serviceIntent.putExtra("trackName", trackName);
        
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            getContext().startForegroundService(serviceIntent);
        } else {
            getContext().startService(serviceIntent);
        }
        
        call.resolve();
    }

    @PluginMethod
    public void stopForegroundService(PluginCall call) {
        Intent serviceIntent = new Intent(getContext(), MusicPlaybackService.class);
        getContext().stopService(serviceIntent);
        call.resolve();
    }
}
