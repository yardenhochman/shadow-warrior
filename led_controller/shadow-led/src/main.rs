mod ble;
mod led_effects;
mod command_handler;
mod effect_state;
mod transitions;

use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::gpio::*;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::rmt::*;
use esp_idf_svc::wifi::{AuthMethod, ClientConfiguration, Configuration, EspWifi};
use esp_idf_svc::{eventloop::EspSystemEventLoop, nvs::EspDefaultNvsPartition};
use heapless::String;
use ws2812_esp32_rmt_driver::Ws2812Esp32Rmt;
use smart_leds_trait::{SmartLedsWrite, RGB8};
use std::sync::Arc;

use crate::command_handler::{CommandHandler, LedCommand};
use crate::effect_state::{EffectState, EffectMode};
use crate::led_effects::FRAME_DURATION_MS;

const WIFI_SSID: &str = "super_skunk"; // Replace with your WiFi SSID
const WIFI_PASSWORD: &str = "0547407479"; // Replace with your WiFi password
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

    // Initialize WiFi
    let mut wifi = EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs.clone()))?;
    connect_wifi(&mut wifi)?;

    // Get IP address
    let ip_address = get_ip_address(&wifi);
    log::info!("Device IP address: {}", ip_address);

    // Initialize LED strip on GPIO 26
    let led_pin = pins.gpio26;
    let rmt_channel = peripherals.rmt.channel0;
    let mut ws2812 = initialize_ws2812(led_pin, rmt_channel)?;

    // Blink LEDs on boot
    blink_leds_on_boot(&mut ws2812)?;

    // Initialize command handler
    let command_handler = CommandHandler::new();
    let command_tx = command_handler.get_sender();

    // Initialize BLE service
    let _ble_service = ble::BleService::new(
        DEVICE_NAME,
        Arc::new(CommandHandler::create_ble_callback(command_tx)),
    )?;

    log::info!("ShadowLED Controller initialized successfully!");
    log::info!("BLE advertising as: {}", DEVICE_NAME);

    // Initialize effect state machine
    let mut effect_state = EffectState::new(LED_COUNT);
    let mut frame_counter: u64 = 0;

    // Main render loop (60 FPS)
    loop {
        let current_time = frame_counter;

        // 1. Process commands → trigger state transitions
        while let Some(command) = command_handler.try_recv() {
            log::info!("Processing command: {:?}", command);

            match command {
                LedCommand::EnergyBar(percentage) => {
                    // For backward compatibility, set power and transition to EnergyBar
                    effect_state.set_power(percentage);
                    effect_state.transition_to(EffectMode::EnergyBar, current_time);
                }
                LedCommand::EnergyPulse => {
                    effect_state.transition_to(EffectMode::EnergyPulse, current_time);
                }
                LedCommand::Breathing => {
                    effect_state.transition_to(EffectMode::Breathing, current_time);
                }
                LedCommand::Idle => {
                    effect_state.set_mode_instant(EffectMode::Idle, current_time);
                }
                LedCommand::SetPower(power) => {
                    effect_state.set_power(power);
                }
                LedCommand::Electricity => {
                    effect_state.transition_to(EffectMode::Electricity, current_time);
                }
            }
        }

        // 2. Update animation state for current frame
        effect_state.update_frame(current_time, LED_COUNT);

        // 3. Render current state (handles transitions internally)
        let pixels = effect_state.render(LED_COUNT, current_time);
        ws2812.write(pixels.iter().cloned())?;

        // 4. Sync to 60 FPS
        FreeRtos::delay_ms(FRAME_DURATION_MS);

        frame_counter += 1;
    }
}

fn connect_wifi(wifi: &mut EspWifi) -> anyhow::Result<()> {
    log::info!("Connecting to WiFi...");

    let ssid: String<32> = WIFI_SSID.try_into().map_err(|_| anyhow::anyhow!("SSID too long"))?;
    let password: String<64> = WIFI_PASSWORD.try_into().map_err(|_| anyhow::anyhow!("Password too long"))?;

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
        log::info!("Waiting for WiFi connection... Current config: {:?}", config);
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

fn initialize_ws2812<'a>(
    pin: Gpio26,
    channel: CHANNEL0,
) -> anyhow::Result<Ws2812Esp32Rmt<'a>> {
    log::info!("Initializing WS2812B LED strip with {} LEDs on GPIO 26", LED_COUNT);
    
    let ws2812 = Ws2812Esp32Rmt::new(channel, pin)?;
    
    log::info!("WS2812B LED strip initialized successfully");
    Ok(ws2812)
}

fn blink_leds_on_boot(ws2812: &mut Ws2812Esp32Rmt) -> anyhow::Result<()> {
    log::info!("Blinking LEDs on boot...");

    // Create pixel buffer using RGB8
    let mut pixels = vec![RGB8::default(); LED_COUNT];

    for i in 0..3 {
        log::info!("LED blink {}: ON - Setting all {} LEDs to white", i + 1, LED_COUNT);
        
        // Set all LEDs to white (RGB: 255, 255, 255)
        for pixel in pixels.iter_mut() {
            *pixel = RGB8::new(255, 255, 255);
        }
        ws2812.write(pixels.iter().cloned())?;
        FreeRtos::delay_ms(500);

        log::info!("LED blink {}: OFF - Setting all {} LEDs to black", i + 1, LED_COUNT);
        
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
