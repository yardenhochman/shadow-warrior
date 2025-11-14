package org.shadowwarrior.brain;

import android.os.Bundle;
import android.webkit.WebSettings;
import android.webkit.WebView;
import com.getcapacitor.BridgeActivity;

public class MainActivity extends BridgeActivity {
    @Override
    public void onCreate(Bundle savedInstanceState) {
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
