use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;

const SPIFFS_MOUNT_POINT: &str = "/spiffs";
const CONFIG_FILE_PATH: &str = "/spiffs/config.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WifiConfig {
    pub ssid: String,
    pub password: String,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u32,
}

fn default_timeout_ms() -> u32 {
    30_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    pub device_name: String,
    pub led_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub wifi: WifiConfig,
    pub device: DeviceConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            wifi: WifiConfig {
                ssid: "YourSSID".to_string(),
                password: "YourPassword".to_string(),
                timeout_ms: 30_000,
            },
            device: DeviceConfig {
                device_name: "ShadowLED".to_string(),
                led_count: 180,
            },
        }
    }
}

impl AppConfig {
    /// Load configuration from SPIFFS or create default
    pub fn load_or_default() -> Self {
        match Self::load() {
            Ok(config) => {
                log::info!("Configuration loaded from {}", CONFIG_FILE_PATH);
                config
            }
            Err(e) => {
                log::warn!("Failed to load config: {:?}. Using defaults (NOT saving to file).", e);
                Self::default()
            }
        }
    }

    /// Load configuration from SPIFFS
    pub fn load() -> Result<Self> {
        log::info!("Loading configuration from {}", CONFIG_FILE_PATH);
        
        let content = fs::read_to_string(CONFIG_FILE_PATH)
            .context("Failed to read config file")?;
        
        let config: AppConfig = toml::from_str(&content)
            .context("Failed to parse TOML config")?;
        
        log::info!("Config loaded - WiFi SSID: {}, Device: {}, LEDs: {}", 
                   config.wifi.ssid, config.device.device_name, config.device.led_count);
        
        Ok(config)
    }

    /// Save configuration to SPIFFS
    pub fn save(&self) -> Result<()> {
        log::info!("Saving configuration to {}", CONFIG_FILE_PATH);
        
        let toml_string = toml::to_string_pretty(self)
            .context("Failed to serialize config to TOML")?;
        
        fs::write(CONFIG_FILE_PATH, toml_string)
            .context("Failed to write config file")?;
        
        log::info!("Configuration saved successfully");
        Ok(())
    }
}

/// Mount SPIFFS partition
pub fn mount_spiffs() -> Result<()> {
    log::info!("Mounting SPIFFS partition...");
    
    use std::ffi::CString;
    
    let base_path = CString::new(SPIFFS_MOUNT_POINT)?;
    let partition_label = CString::new("storage")?;
    
    let conf = esp_idf_svc::sys::esp_vfs_spiffs_conf_t {
        base_path: base_path.as_ptr(),
        partition_label: partition_label.as_ptr(),
        max_files: 5,
        format_if_mount_failed: false, // Don't auto-format - preserve uploaded config
    };
    
    unsafe {
        let ret = esp_idf_svc::sys::esp_vfs_spiffs_register(&conf);
        if ret != esp_idf_svc::sys::ESP_OK {
            anyhow::bail!("Failed to register SPIFFS: error code {}", ret);
        }
    }
    
    log::info!("SPIFFS mounted successfully at {}", SPIFFS_MOUNT_POINT);
    
    Ok(())
}

/// Format the SPIFFS partition (can be triggered via BLE/WiFi)
pub fn format_spiffs() -> Result<()> {
    log::warn!("Formatting SPIFFS partition...");
    
    use std::ffi::CString;
    
    let partition_label = CString::new("storage")?;
    
    unsafe {
        let ret = esp_idf_svc::sys::esp_spiffs_format(partition_label.as_ptr());
        if ret != esp_idf_svc::sys::ESP_OK {
            anyhow::bail!("Failed to format SPIFFS: error code {}", ret);
        }
    }
    
    log::info!("SPIFFS formatted successfully");
    Ok(())
}
