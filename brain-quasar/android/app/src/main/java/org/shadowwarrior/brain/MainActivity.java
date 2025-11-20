package org.shadowwarrior.brain;

import android.os.Bundle;
import android.webkit.WebSettings;
import android.webkit.WebView;
import com.getcapacitor.BridgeActivity;
import com.shadow_warrior.ble.BlePeripheralPlugin;

public class MainActivity extends BridgeActivity {
    @Override
    public void onCreate(Bundle savedInstanceState) {
        // Register custom plugins before super.onCreate()
        registerPlugin(MusicPlaybackPlugin.class);
        registerPlugin(NativeAudioExtended.class);
        registerPlugin(BlePeripheralPlugin.class);
        registerPlugin(AccelerometerPlugin.class);
        registerPlugin(LEDEffectPlugin.class);

        super.onCreate(savedInstanceState);
        android.util.Log.d("MainActivity", "onCreate called - Native accelerometer implementation");

        // Start foreground service for background operation
        ShadowWarriorForegroundService.start(this);

        // Configure WebView after initialization
        configureMixedContent();
    }
    
    @Override
    public void onResume() {
        super.onResume();
        configureMixedContent();
    }
    
    private void configureMixedContent() {
        if (this.bridge != null && this.bridge.getWebView() != null) {
            WebView webView = this.bridge.getWebView();
            WebSettings webSettings = webView.getSettings();
            webSettings.setMixedContentMode(WebSettings.MIXED_CONTENT_ALWAYS_ALLOW);
        }
    }
}
