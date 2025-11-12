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
use std::sync::{Arc, Mutex};
use std::thread;
use ws2812_esp32_rmt_driver::Ws2812Esp32Rmt;

use crate::command_handler::CommandHandler;
use crate::effect_state::EffectState;
use crate::led_effects::{FRAME_DURATION_MS, FRAME_RATE};

const WIFI_SSID: &str = "super_skunk"; // Replace with your WiFi SSID
const WIFI_PASSWORD: &str = "0547407479"; // Replace with your WiFi password
const WIFI_CONNECT_TIMEOUT_MS: u32 = 30_000; // 30 seconds timeout for WiFi connection
const LED_COUNT: usize = 180;
const DEVICE_NAME: &str = "ShadowLED";

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

    // Initialize event loop
    let sys_loop = EspSystemEventLoop::take()?;

    // Initialize LED strip on GPIO 26
    let led_pin = pins.gpio26;
    let rmt_channel = peripherals.rmt.channel0;
    let mut ws2812 = initialize_ws2812(led_pin, rmt_channel)?;

    // Blink LEDs on boot
    blink_leds_on_boot(&mut ws2812)?;

    // Initialize command handler
    let command_handler = CommandHandler::new();
    let command_tx = command_handler.get_sender();

    // Initialize BLE service FIRST (before WiFi)
    log::info!("Starting BLE service...");
    let ble_service = Arc::new(Mutex::new(ble::BleService::new(
        DEVICE_NAME,
        Arc::new(CommandHandler::create_ble_callback(command_tx.clone())),
    )?));
    ble_service.lock().unwrap().update_status("Starting WiFi...")?;
    log::info!("BLE advertising as: {}", DEVICE_NAME);

    // Start WiFi connection in background thread (non-blocking)
    let ble_service_clone = Arc::clone(&ble_service);
    let _wifi_thread = thread::spawn(move || {
        log::info!("Starting WiFi connection in background...");
        
        match EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs.clone())) {
            Ok(mut wifi) => {
                match connect_wifi_with_timeout(&mut wifi, WIFI_CONNECT_TIMEOUT_MS) {
                    Ok(true) => {
                        let ip_address = get_ip_address(&wifi);
                        log::info!("WiFi connected! IP address: {}", ip_address);
                        
                        // Update BLE with IP address
                        if let Ok(ble) = ble_service_clone.lock() {
                            let _ = ble.update_ip_address(&ip_address);
                            let _ = ble.update_status("WiFi Connected");
                        }
                        
                        // Initialize HTTP server
                        match http_server::HttpServer::new(command_tx.clone()) {
                            Ok(_http_server) => {
                                log::info!("HTTP server running on port 80");
                                // Keep WiFi and HTTP server alive by holding references
                                loop {
                                    FreeRtos::delay_ms(1000);
                                }
                            }
                            Err(e) => {
                                log::error!("Failed to start HTTP server: {:?}", e);
                            }
                        }
                    }
                    Ok(false) => {
                        log::warn!("WiFi connection timed out after {}ms", WIFI_CONNECT_TIMEOUT_MS);
                        if let Ok(ble) = ble_service_clone.lock() {
                            let _ = ble.update_status("WiFi Timeout - BLE Only");
                        }
                    }
                    Err(e) => {
                        log::error!("WiFi connection error: {:?}", e);
                        if let Ok(ble) = ble_service_clone.lock() {
                            let _ = ble.update_status("WiFi Error - BLE Only");
                        }
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to initialize WiFi: {:?}", e);
                if let Ok(ble) = ble_service_clone.lock() {
                    let _ = ble.update_status("WiFi Init Failed - BLE Only");
                }
            }
        }
    });

    // Update BLE status to ready (WiFi is optional)
    ble_service.lock().unwrap().update_status("Ready")?;

    log::info!("ShadowLED Controller initialized successfully!");
    log::info!("Frame rate set to {} FPS", FRAME_RATE);
    log::info!("WiFi connecting in background (device is operational via BLE)");

    // Initialize effect state machine
    let mut effect_state = EffectState::new(LED_COUNT);

    // Main render loop (60 FPS)
    loop {
        while let Some(command) = command_handler.try_recv() {
            log::info!("Processing command: {:?}", command);
            effect_state.transition_to(command);
            if let Ok(ble) = ble_service.lock() {
                let _ = ble.update_status(&format!("Mode {}", effect_state.mode));
            }
        }

        if let Some(pixels) = &effect_state.next_frame() {
            let frame_buffer = led_effects::vec_srgbu8_to_vec_rgb8(pixels);
            ws2812.write(frame_buffer.into_iter())?;
        }
        // Sync to desired frame rate
        FreeRtos::delay_ms(FRAME_DURATION_MS);
    }
}

fn connect_wifi_with_timeout(wifi: &mut EspWifi, timeout_ms: u32) -> anyhow::Result<bool> {
    log::info!("Connecting to WiFi (timeout: {}ms)...", timeout_ms);

    let ssid: String<32> = WIFI_SSID
        .try_into()
        .map_err(|_| anyhow::anyhow!("SSID too long"))?;
    let password: String<64> = WIFI_PASSWORD
        .try_into()
        .map_err(|_| anyhow::anyhow!("Password too long"))?;

    let wifi_configuration = Configuration::Client(ClientConfiguration {
        ssid,
        password,
        auth_method: AuthMethod::WPA2Personal,
        ..Default::default()
    });

    wifi.set_configuration(&wifi_configuration)?;
    wifi.start()?;
    wifi.connect()?;

    // Wait for connection with timeout
    let mut elapsed_ms = 0u32;
    let check_interval_ms = 500u32;
    
    while !wifi.is_connected()? {
        if elapsed_ms >= timeout_ms {
            log::warn!("WiFi connection timeout reached");
            return Ok(false);
        }
        
        if elapsed_ms % 5000 == 0 {
            log::info!(
                "Waiting for WiFi connection... ({}/{}ms)",
                elapsed_ms,
                timeout_ms
            );
        }
        
        FreeRtos::delay_ms(check_interval_ms);
        elapsed_ms += check_interval_ms;
    }

    log::info!("WiFi connected successfully!");
    Ok(true)
}

fn get_ip_address(wifi: &EspWifi) -> std::string::String {
    match wifi.sta_netif().get_ip_info() {
        Ok(ip_info) => format!("{}", ip_info.ip),
        Err(_) => "0.0.0.0".to_string(),
    }
}

fn initialize_ws2812<'a>(pin: Gpio26, channel: CHANNEL0) -> anyhow::Result<Ws2812Esp32Rmt<'a>> {
    log::info!(
        "Initializing WS2812B LED strip with {} LEDs on GPIO 26",
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
