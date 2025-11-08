use esp_idf_hal::delay::FreeRtos;
use smart_leds_trait::{SmartLedsWrite, RGB8};
use ws2812_esp32_rmt_driver::Ws2812Esp32Rmt;
use std::f32::consts::PI;

pub const FRAME_RATE: u32 = 60; // 60 FPS
pub const FRAME_DURATION_MS: u32 = 1000 / FRAME_RATE;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Idle,
    EnergyBar,
    EnergyPulse,
    Breathing,
}

/// Quadratic easing function for smooth animations
fn easing_quad(current_frame: u32, total_frames: u32) -> f32 {
    let normalized_t = current_frame as f32 / total_frames as f32;
    normalized_t * normalized_t
}

/// Display an energy bar with the given percentage (0-100)
pub fn energy_bar(
    ws2812: &mut Ws2812Esp32Rmt,
    led_count: usize,
    percentage: u8,
    duration_ms: u32,
) -> anyhow::Result<()> {
    log::info!("Energy bar effect: {}%", percentage);

    let frames = (FRAME_RATE * duration_ms / 1000).max(1);
    let num_leds = (led_count * percentage as usize / 100).min(led_count);

    // Create gradient from white (start) to red (end)
    let mut led_colors: Vec<RGB8> = Vec::with_capacity(num_leds);
    for i in 0..num_leds {
        let ratio = if num_leds > 1 {
            i as f32 / (num_leds - 1) as f32
        } else {
            0.0
        };
        led_colors.push(RGB8 {
            r: 255,
            g: (255.0 * (1.0 - ratio)) as u8,
            b: (255.0 * (1.0 - ratio)) as u8,
        });
    }

    let mut pixels = vec![RGB8::default(); led_count];
    let step_frames = frames / 2;

    // Animate bar filling up
    for i in 0..=step_frames {
        let x = easing_quad(i, step_frames);
        let led = (num_leds as f32 * x) as usize;

        // Clear all pixels
        pixels.fill(RGB8::default());

        // Fill up to current position
        for idx in 0..led.min(num_leds) {
            pixels[idx] = led_colors[idx];
        }

        ws2812.write(pixels.iter().cloned())?;
        FreeRtos::delay_ms(FRAME_DURATION_MS);
    }

    // Animate bar emptying
    for i in 0..=step_frames {
        let x = easing_quad(i, step_frames);
        let led = num_leds - (num_leds as f32 * x) as usize;

        // Clear from the end
        for idx in led..num_leds {
            pixels[idx] = RGB8::default();
        }

        ws2812.write(pixels.iter().cloned())?;
        FreeRtos::delay_ms(FRAME_DURATION_MS);
    }

    Ok(())
}

/// Send a pulse of red light down the strip
pub fn energy_pulse(
    ws2812: &mut Ws2812Esp32Rmt,
    led_count: usize,
) -> anyhow::Result<()> {
    log::info!("Energy pulse effect");

    let mut pixels = vec![RGB8::default(); led_count];

    for i in 0..led_count {
        pixels[i] = RGB8 { r: 255, g: 0, b: 0 };
        ws2812.write(pixels.iter().cloned())?;
        FreeRtos::delay_ms(20);
        pixels[i] = RGB8::default();
    }

    ws2812.write(pixels.iter().cloned())?;
    Ok(())
}

/// Breathing effect with configurable brightness
pub fn breathing_cycle(
    ws2812: &mut Ws2812Esp32Rmt,
    led_count: usize,
    max_brightness: u8,
    should_stop: impl Fn() -> bool,
) -> anyhow::Result<()> {
    log::info!("Breathing effect");

    let cycle_seconds = 5;
    let frames_per_cycle = FRAME_RATE * cycle_seconds;
    let mut pixels = vec![RGB8::default(); led_count];

    loop {
        for frame in 0..frames_per_cycle {
            if should_stop() {
                // Clear LEDs before stopping
                pixels.fill(RGB8::default());
                ws2812.write(pixels.iter().cloned())?;
                return Ok(());
            }

            let t = frame as f32 / frames_per_cycle as f32;
            let envelope = ((2.0 * PI * t).sin() + 1.0) / 2.0; // 0..1 sinusoid
            let brightness = (max_brightness as f32 * envelope) as u8;

            pixels.fill(RGB8 {
                r: brightness,
                g: 0,
                b: 0,
            });

            ws2812.write(pixels.iter().cloned())?;
            FreeRtos::delay_ms(FRAME_DURATION_MS);
        }
    }
}

/// Clear all LEDs (idle mode)
pub fn idle_effect(
    ws2812: &mut Ws2812Esp32Rmt,
    led_count: usize,
) -> anyhow::Result<()> {
    log::info!("Idle effect - clearing LEDs");

    let pixels = vec![RGB8::default(); led_count];
    ws2812.write(pixels.iter().cloned())?;
    Ok(())
}
