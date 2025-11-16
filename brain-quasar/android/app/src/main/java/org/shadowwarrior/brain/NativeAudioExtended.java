package org.shadowwarrior.brain;

import com.getcapacitor.annotation.CapacitorPlugin;
import com.getcapacitor.community.audio.NativeAudio;

/**
 * Extended NativeAudio plugin that doesn't pause audio when app goes to background.
 * Uses a custom plugin name "NativeAudioExtended" instead of "NativeAudio".
 * The original NativeAudio plugin is excluded via capacitor.config.ts includePlugins.
 */
@CapacitorPlugin(name = "NativeAudioExtended")
public class NativeAudioExtended extends NativeAudio {
    
    @Override
    protected void handleOnPause() {
        // Override to do nothing - don't pause audio when app goes to background
        // The foreground service keeps the audio playing
        android.util.Log.d("NativeAudioExtended", "handleOnPause - NOT pausing audio (foreground service active)");
    }
    
    @Override
    protected void handleOnResume() {
        // Override to do nothing - audio never paused, so no need to resume
        android.util.Log.d("NativeAudioExtended", "handleOnResume - audio was never paused");
    }
}
