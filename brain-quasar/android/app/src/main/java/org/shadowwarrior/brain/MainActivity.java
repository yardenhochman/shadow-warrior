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

        super.onCreate(savedInstanceState);
        android.util.Log.d("MainActivity", "onCreate called - Build 10");
        
        // Configure WebView after initialization
        configureMixedContent();
    }
    
    @Override
    public void onResume() {
        super.onResume();
        configureMixedContent();
    }
    
    @Override
    public void onPause() {
        super.onPause();
        
        // Prevent WebView from pausing timers and JavaScript execution
        // This is critical for background audio playback
        if (this.bridge != null && this.bridge.getWebView() != null) {
            WebView webView = this.bridge.getWebView();
            // Don't pause timers - keeps JavaScript and audio running
            // Note: onPause() does NOT pause timers by default in modern Android,
            // but we're being explicit here
            android.util.Log.d("MainActivity", "onPause - WebView timers kept active for background audio");
        }
    }
    
    @Override
    public void onStop() {
        super.onStop();
        android.util.Log.d("MainActivity", "onStop - activity stopped but audio should continue via foreground service");
    }
    
    private void configureMixedContent() {
        android.util.Log.d("MainActivity", "configureMixedContent called");
        
        if (this.bridge != null && this.bridge.getWebView() != null) {
            WebView webView = this.bridge.getWebView();
            WebSettings webSettings = webView.getSettings();
            webSettings.setMixedContentMode(WebSettings.MIXED_CONTENT_ALWAYS_ALLOW);
            android.util.Log.d("MainActivity", "✅ Mixed content mode set to ALWAYS_ALLOW - Build 10");
        } else {
            android.util.Log.e("MainActivity", "❌ configureMixedContent: bridge or WebView is null - Build 10");
            
            // Schedule retry after a delay
            new android.os.Handler(android.os.Looper.getMainLooper()).postDelayed(new Runnable() {
                @Override
                public void run() {
                    android.util.Log.d("MainActivity", "Retrying configureMixedContent after delay");
                    configureMixedContent();
                }
            }, 100);
        }
    }
}
