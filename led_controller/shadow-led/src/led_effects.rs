use smart_leds_trait::{SmartLedsWrite, RGB8};
use ws2812_esp32_rmt_driver::Ws2812Esp32Rmt;
use std::f32::consts::PI;

/// Quadratic easing function for smooth animations
pub fn easing_quad(current_frame: u32, total_frames: u32) -> f32 {
    let normalized_t = current_frame as f32 / total_frames as f32;
    normalized_t * normalized_t
}

/// Linear interpolation between two u8 values
pub fn lerp(a: u8, b: u8, t: f32) -> u8 {
    ((a as f32) * (1.0 - t) + (b as f32) * t) as u8
}

/// Render idle mode (all LEDs off)
pub fn render_idle(led_count: usize) -> Vec<RGB8> {
    vec![RGB8::default(); led_count]
}

/// Render breathing effect for a single frame
pub fn render_breathing(led_count: usize, phase: f32, max_brightness: u8) -> Vec<RGB8> {
    let envelope = ((2.0 * PI * phase).sin() + 1.0) / 2.0; // 0..1 sinusoid
    let brightness = (max_brightness as f32 * envelope) as u8;

    vec![RGB8 { r: brightness, g: 0, b: 0 }; led_count]
}

/// Render energy bar with smooth fill level
pub fn render_energy_bar(led_count: usize, fill_ratio: f32) -> Vec<RGB8> {
    let mut pixels = vec![RGB8::default(); led_count];
    let num_leds = (led_count as f32 * fill_ratio.min(1.0).max(0.0)) as usize;

    // Create gradient from white (start) to red (end)
    for i in 0..num_leds.min(led_count) {
        let ratio = if num_leds > 1 {
            i as f32 / (num_leds - 1) as f32
        } else {
            0.0
        };
        pixels[i] = RGB8 {
            r: 255,
            g: (255.0 * (1.0 - ratio)) as u8,
            b: (255.0 * (1.0 - ratio)) as u8,
        };
    }

    pixels
}

/// Render electricity effect (lightning bolts + sparks)
pub fn render_electricity(led_count: usize, frame_counter: u64) -> Vec<RGB8> {
    let mut pixels = vec![RGB8::default(); led_count];

    // Simple lightning effect: random sparks and bolts
    let time_seed = (frame_counter / 10) as u32; // Change every 10 frames

    // Add some random sparks (10-15 sparks)
    for i in 0..15 {
        let pos = ((time_seed.wrapping_mul(7).wrapping_add(i * 13)) % led_count as u32) as usize;
        let brightness = ((time_seed.wrapping_add(i)) % 200 + 55) as u8; // 55-255
        pixels[pos] = RGB8 {
            r: brightness,
            g: brightness / 2,
            b: brightness,
        };
    }

    // Add a traveling bolt
    let bolt_pos = (frame_counter / 3 % led_count as u64) as usize; // Move every 3 frames
    if bolt_pos < led_count {
        pixels[bolt_pos] = RGB8::new(255, 255, 255);
        // Add some spread
        if bolt_pos > 0 {
            pixels[bolt_pos - 1] = RGB8::new(200, 200, 255);
        }
        if bolt_pos < led_count - 1 {
            pixels[bolt_pos + 1] = RGB8::new(200, 200, 255);
        }
    }

    pixels
}

/// Render energy pulse at specific position
pub fn render_energy_pulse(led_count: usize, position: usize) -> Vec<RGB8> {
    let mut pixels = vec![RGB8::default(); led_count];

    if position < led_count {
        pixels[position] = RGB8::new(255, 0, 0);
        // Add some trail
        if position > 0 {
            pixels[position - 1] = RGB8::new(128, 0, 0);
        }
        if position > 1 {
            pixels[position - 2] = RGB8::new(64, 0, 0);
        }
    }

    pixels
}

/// Legacy blocking functions (to be removed after refactor)
/// These are kept temporarily for compatibility during the transition

pub const FRAME_RATE: u32 = 60; // 60 FPS
pub const FRAME_DURATION_MS: u32 = 1000 / FRAME_RATE;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Idle,
    EnergyBar,
    EnergyPulse,
    Breathing,
}

/// Display an energy bar with the given percentage (0-100)
/// LEGACY: Use render_energy_bar instead
pub fn energy_bar(
    ws2812: &mut Ws2812Esp32Rmt,
    led_count: usize,
    percentage: u8,
    duration_ms: u32,
) -> anyhow::Result<()> {
    log::info!("Energy bar effect: {}%", percentage);

    let frames = (FRAME_RATE * duration_ms / 1000).max(1);
    let fill_ratio = percentage as f32 / 100.0;

    for _ in 0..frames {
        let pixels = render_energy_bar(led_count, fill_ratio);
        ws2812.write(pixels.iter().cloned())?;
        esp_idf_hal::delay::FreeRtos::delay_ms(FRAME_DURATION_MS);
    }

    Ok(())
}

/// Send a pulse of red light down the strip
/// LEGACY: Use render_energy_pulse instead
pub fn energy_pulse(
    ws2812: &mut Ws2812Esp32Rmt,
    led_count: usize,
) -> anyhow::Result<()> {
    log::info!("Energy pulse effect");

    for i in 0..led_count {
        let pixels = render_energy_pulse(led_count, i);
        ws2812.write(pixels.iter().cloned())?;
        esp_idf_hal::delay::FreeRtos::delay_ms(20);
    }

    // Clear at the end
    let pixels = vec![RGB8::default(); led_count];
    ws2812.write(pixels.iter().cloned())?;
    Ok(())
}

/// Breathing effect with configurable brightness
/// LEGACY: Use render_breathing instead
pub fn breathing_cycle(
    ws2812: &mut Ws2812Esp32Rmt,
    led_count: usize,
    max_brightness: u8,
    should_stop: impl Fn() -> bool,
) -> anyhow::Result<()> {
    log::info!("Breathing effect");

    let cycle_seconds = 5;
    let frames_per_cycle = FRAME_RATE * cycle_seconds;
    let mut frame_counter = 0;

    loop {
        if should_stop() {
            // Clear LEDs before stopping
            let pixels = vec![RGB8::default(); led_count];
            ws2812.write(pixels.iter().cloned())?;
            return Ok(());
        }

        let phase = frame_counter as f32 / frames_per_cycle as f32;
        let pixels = render_breathing(led_count, phase, max_brightness);
        ws2812.write(pixels.iter().cloned())?;
        esp_idf_hal::delay::FreeRtos::delay_ms(FRAME_DURATION_MS);

        frame_counter += 1;
    }
}

/// Clear all LEDs (idle mode)
/// LEGACY: Use render_idle instead
pub fn idle_effect(
    ws2812: &mut Ws2812Esp32Rmt,
    led_count: usize,
) -> anyhow::Result<()> {
    log::info!("Idle effect - clearing LEDs");

    let pixels = render_idle(led_count);
    ws2812.write(pixels.iter().cloned())?;
    Ok(())
}
