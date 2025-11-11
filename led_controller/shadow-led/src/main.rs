mod ble;
mod command_handler;
mod effect_state;
mod http_server;
mod led_effects;
mod transitions;

use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::gpio::*;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::rmt::*;
use esp_idf_svc::wifi::{AuthMethod, ClientConfiguration, Configuration, EspWifi};
use esp_idf_svc::{eventloop::EspSystemEventLoop, nvs::EspDefaultNvsPartition};
use heapless::String;
use smart_leds::{SmartLedsWrite, RGB8};
use std::sync::Arc;
use ws2812_esp32_rmt_driver::Ws2812Esp32Rmt;
use esp_idf_sys as sys;

use crate::command_handler::CommandHandler;
use crate::effect_state::EffectState;
use crate::led_effects::{FRAME_DURATION_MS, FRAME_RATE};

const WIFI_CONFIG_PATH: &str = "/spiffs/wifi.conf";
const LED_COUNT: usize = 180;
const DEVICE_NAME: &str = "ShadowLED";

/// Safe wrapper for SPIFFS operations
struct SpiffsHandle;

impl SpiffsHandle {
    fn mount() -> anyhow::Result<Self> {
        unsafe {
            let mut spiffs_config = sys::esp_vfs_spiffs_conf_t {
                base_path: b"/spiffs\0".as_ptr() as *const u8,
                partition_label: b"storage\0".as_ptr() as *const u8,
                max_files: 5,
                format_if_mount_failed: false,
            };

            let ret = sys::esp_vfs_spiffs_register(&mut spiffs_config);
            if ret != 0 {
                return Err(anyhow::anyhow!("Failed to mount SPIFFS: {}", ret));
            }

            // Verify mount
            let mut total_bytes: usize = 0;
            let mut used_bytes: usize = 0;
            let ret = sys::esp_spiffs_info(b"storage\0".as_ptr() as *const u8, &mut total_bytes as *mut usize, &mut used_bytes as *mut usize);
            if ret != 0 {
                return Err(anyhow::anyhow!("SPIFFS partition not accessible: {}", ret));
            }

            log::info!("SPIFFS mounted successfully, total: {} bytes, used: {} bytes", total_bytes, used_bytes);
        }

        Ok(SpiffsHandle)
    }
}

impl Drop for SpiffsHandle {
    fn drop(&mut self) {
        unsafe {
            sys::esp_vfs_spiffs_unregister(b"storage\0".as_ptr() as *const u8);
        }
        log::info!("SPIFFS unmounted");
    }
}

fn read_wifi_config() -> anyhow::Result<Option<(String<32>, String<64>)>> {
    log::info!("Attempting to read WiFi config from {}", WIFI_CONFIG_PATH);

    // Mount SPIFFS safely
    let _spiffs = SpiffsHandle::mount()?;

    // Try to open the config file
    let file = match std::fs::File::open(WIFI_CONFIG_PATH) {
        Ok(f) => f,
        Err(e) => {
            log::info!("WiFi config file not found ({}), WiFi will be disabled", e);
            return Ok(None);
        }
    };

    // Read the file content
    use std::io::Read;
    let mut content = std::string::String::new();
    if file.take(1024).read_to_string(&mut content).is_err() {
        log::warn!("Failed to read WiFi config file");
        return Ok(None);
    }

    // Parse the config file (simple key=value format)
    let mut ssid: Option<String<32>> = None;
    let mut password: Option<String<64>> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"');

            match key {
                "ssid" => {
                    ssid = Some(String::try_from(value).map_err(|_| anyhow::anyhow!("SSID too long"))?);
                }
                "password" => {
                    password = Some(String::try_from(value).map_err(|_| anyhow::anyhow!("Password too long"))?);
                }
                _ => {}
            }
        }
    }

    // SPIFFS will be automatically unmounted when _spiffs goes out of scope

    match (ssid, password) {
        (Some(s), Some(p)) => {
            log::info!("WiFi config loaded successfully");
            Ok(Some((s, p)))
        }
        _ => {
            log::warn!("WiFi config file incomplete (missing ssid or password)");
            Ok(None)
        }
    }
}

fn main() -> anyhow::Result<()> {
    // It is necessary to call this function once. Otherwise, some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("ShadowLED Controller starting...");

    // Initialize peripherals
    let peripherals = Peripherals::take()?;
    let pins = peripherals.pins;

    // Initialize NVS
    let nvs = EspDefaultNvsPartition::take()?;

    // Initialize system event loop
    let sys_loop = EspSystemEventLoop::take()?;

    // Try to read WiFi configuration from SPIFFS
    let wifi_config = read_wifi_config()?;
    let mut wifi: Option<EspWifi> = None;
    let mut ip_address = "No WiFi".to_string();

    if let Some((ssid, password)) = wifi_config {
        log::info!("WiFi config found, initializing WiFi...");
        let mut wifi_instance = EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs.clone()))?;
        connect_wifi(&mut wifi_instance, ssid, password)?;

        // Get IP address — wait a short while for IP assignment (avoid logging 0.0.0.0)
        ip_address = get_ip_address(&wifi_instance);
        let mut waited_ms = 0u32;
        while ip_address == "0.0.0.0" && waited_ms < 10_000 {
            FreeRtos::delay_ms(500);
            waited_ms += 500;
            ip_address = get_ip_address(&wifi_instance);
        }
        log::info!("Device IP address: {}", ip_address);
        wifi = Some(wifi_instance);
    } else {
        log::info!("No WiFi config found, running in offline mode");
    }

    // Initialize LED strip on GPIO 16
    let led_pin = pins.gpio16;
    let rmt_channel = peripherals.rmt.channel0;
    let mut ws2812 = initialize_ws2812(led_pin, rmt_channel)?;

    // Initialize command handler
    let command_handler = CommandHandler::new();
    let command_tx = command_handler.get_sender();

    // Initialize BLE service
    let _ble_service = ble::BleService::new(
        DEVICE_NAME,
        Arc::new(CommandHandler::create_ble_callback(command_tx.clone())),
    )?;
    _ble_service.update_ip_address(&ip_address)?;
    _ble_service.update_status("Ready")?;

    // Initialize HTTP server only if WiFi is available
    let _http_server = if wifi.is_some() {
        Some(http_server::HttpServer::new(command_tx)?)
    } else {
        log::info!("HTTP server not started (no WiFi)");
        None
    };

    // Blink LEDs on boot
    blink_leds_on_boot(&mut ws2812)?;

    log::info!("ShadowLED Controller initialized successfully!");
    log::info!("BLE advertising as: {}", DEVICE_NAME);
    if wifi.is_some() {
        log::info!("HTTP server running on port 80");
    } else {
        log::info!("Running in offline mode (BLE only)");
    }
    log::info!("Frame rate set to {} FPS", FRAME_RATE);

    // Initialize effect state machine
    let mut effect_state = EffectState::new(LED_COUNT);

    // Main render loop (60 FPS)
    loop {
        while let Some(command) = command_handler.try_recv() {
            log::info!("Processing command: {:?}", command);
            effect_state.transition_to(command);
            _ble_service.update_status(&format!("Mode {}", effect_state.mode))?;
        }

        if let Some(pixels) = &effect_state.next_frame() {
            let frame_buffer = led_effects::vec_srgbu8_to_vec_rgb8(pixels);
            ws2812.write(frame_buffer.into_iter())?;
        }
        // 4. Sync to desired frame rate
        FreeRtos::delay_ms(FRAME_DURATION_MS);
    }
}

fn connect_wifi(wifi: &mut EspWifi, ssid: String<32>, password: String<64>) -> anyhow::Result<()> {
    log::info!("Connecting to WiFi...");

    let wifi_configuration = Configuration::Client(ClientConfiguration {
        ssid,
        password,
        auth_method: AuthMethod::WPA2Personal,
        ..Default::default()
    });

    wifi.set_configuration(&wifi_configuration)?;

    wifi.start()?;
    wifi.connect()?;

    // Wait for connection
    while !wifi.is_connected()? {
        let config = wifi.get_configuration()?;
        log::info!(
            "Waiting for WiFi connection... Current config: {:?}",
            config
        );
        FreeRtos::delay_ms(500);
    }

    log::info!("WiFi connected!");
    Ok(())
}

fn get_ip_address(wifi: &EspWifi) -> std::string::String {
    match wifi.sta_netif().get_ip_info() {
        Ok(ip_info) => format!("{}", ip_info.ip),
        Err(_) => "0.0.0.0".to_string(),
    }
}

fn initialize_ws2812<'a, T: OutputPin>(pin: T, channel: CHANNEL0) -> anyhow::Result<Ws2812Esp32Rmt<'a>> {
    log::info!(
        "Initializing WS2812B LED strip with {} LEDs on GPIO 16",
        LED_COUNT
    );

    let ws2812 = Ws2812Esp32Rmt::new(channel, pin)?;

    log::info!("WS2812B LED strip initialized successfully");
    Ok(ws2812)
}

fn blink_leds_on_boot(ws2812: &mut Ws2812Esp32Rmt) -> anyhow::Result<()> {
    log::info!("Blinking LEDs on boot...");

    // Create pixel buffer using RGB8
    let mut pixels = vec![RGB8::default(); LED_COUNT];

    for i in 0..3 {
        log::info!(
            "LED blink {}: ON - Setting all {} LEDs to white",
            i + 1,
            LED_COUNT
        );

        // Set all LEDs to white (RGB: 255, 255, 255)
        for pixel in pixels.iter_mut() {
            *pixel = RGB8::new(255, 255, 255);
        }
        ws2812.write(pixels.iter().cloned())?;
        FreeRtos::delay_ms(500);

        log::info!(
            "LED blink {}: OFF - Setting all {} LEDs to black",
            i + 1,
            LED_COUNT
        );

        // Set all LEDs to black (off)
        for pixel in pixels.iter_mut() {
            *pixel = RGB8::default();
        }
        ws2812.write(pixels.iter().cloned())?;
        FreeRtos::delay_ms(500);
    }

    log::info!("Boot blink sequence completed");
    Ok(())
}
