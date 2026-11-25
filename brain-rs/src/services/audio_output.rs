//! Which output sink the brain's sound goes to. Music and cues are played through rodio on
//! the default output device, which on the Pi is the PipeWire/Pulse default sink: the
//! Bluetooth speaker (`bluez_output...`) when it is connected and default, otherwise the
//! built-in audio. This polls `pactl` every few seconds and publishes `AudioOutputSink`
//! when anything changed, for the dashboard (`hardware.health.audio_out`).
//! Parsing is pure; only `read_sink` runs `pactl`.

use std::time::Duration;

use super::audio_source::run_pactl;
use crate::core::{Event, SharedEventBus};

const POLL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq)]
pub struct SinkInfo {
    pub name: String,
    pub description: Option<String>,
    pub bluetooth: bool,
    pub volume_pct: Option<u32>,
    pub muted: Option<bool>,
}

/// Default sink from `pactl --format=json info` and `... list sinks`.
pub fn parse_sink(info_json: &str, sinks_json: &str) -> Option<SinkInfo> {
    let info: serde_json::Value = serde_json::from_str(info_json).ok()?;
    let name = info.get("default_sink_name")?.as_str()?.to_string();
    if name.is_empty() {
        return None;
    }
    let sinks: serde_json::Value = serde_json::from_str(sinks_json).unwrap_or(serde_json::Value::Null);
    let sink = sinks.as_array().and_then(|a| a.iter().find(|s| s.get("name").and_then(|n| n.as_str()) == Some(&name)));
    let description = sink.and_then(|s| s.get("description")).and_then(|d| d.as_str()).map(str::to_string);
    let muted = sink.and_then(|s| s.get("mute")).and_then(|m| m.as_bool());
    let volume_pct = sink
        .and_then(|s| s.get("volume"))
        .and_then(|v| v.as_object())
        .and_then(|channels| {
            channels
                .values()
                .filter_map(|c| c.get("value_percent").and_then(|p| p.as_str()))
                .filter_map(|p| p.trim_end_matches('%').trim().parse::<u32>().ok())
                .max()
        });
    let device_api = sink
        .and_then(|s| s.get("properties"))
        .and_then(|p| p.get("device.api"))
        .and_then(|a| a.as_str())
        .unwrap_or("");
    let bluetooth = device_api == "bluez" || name.starts_with("bluez_");
    Some(SinkInfo { name, description, bluetooth, volume_pct, muted })
}

fn read_sink() -> Option<SinkInfo> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let t = Duration::from_secs(2);
    let info = run_pactl(&["--format=json", "info"], t)?;
    let sinks = run_pactl(&["--format=json", "list", "sinks"], t).unwrap_or_default();
    parse_sink(&info, &sinks)
}

fn to_event(s: &SinkInfo) -> Event {
    Event::AudioOutputSink {
        sink: Some(s.name.clone()),
        description: s.description.clone(),
        bluetooth: s.bluetooth,
        volume_pct: s.volume_pct,
        muted: s.muted,
    }
}

/// Poll the default sink and publish changes. Without `pactl` nothing is published and
/// the dashboard shows the output as unknown.
pub fn spawn(bus: SharedEventBus) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut last: Option<SinkInfo> = None;
        loop {
            let now = tokio::task::spawn_blocking(read_sink).await.ok().flatten();
            if now != last {
                if let Some(s) = &now {
                    bus.publish(to_event(s));
                }
                last = now;
            }
            tokio::time::sleep(POLL).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SINKS: &str = r#"[
      {"name":"alsa_output.platform-bcm2835_audio.stereo-fallback","description":"Built-in Audio Stereo","mute":false,
       "volume":{"front-left":{"value_percent":"100%"},"front-right":{"value_percent":"100%"}},
       "properties":{"device.api":"alsa"}},
      {"name":"bluez_output.10_94_97_40_6F_95.1","description":"UE BOOM 4","mute":false,
       "volume":{"front-left":{"value_percent":"65%"},"front-right":{"value_percent":"60%"}},
       "properties":{"device.api":"bluez5"}}]"#;

    #[test]
    fn builtin_default_sink() {
        let info = r#"{"default_sink_name":"alsa_output.platform-bcm2835_audio.stereo-fallback"}"#;
        let s = parse_sink(info, SINKS).unwrap();
        assert_eq!(s.description.as_deref(), Some("Built-in Audio Stereo"));
        assert!(!s.bluetooth);
        assert_eq!(s.volume_pct, Some(100));
        assert_eq!(s.muted, Some(false));
    }

    #[test]
    fn bluetooth_default_sink_is_detected_by_name() {
        let info = r#"{"default_sink_name":"bluez_output.10_94_97_40_6F_95.1"}"#;
        let s = parse_sink(info, SINKS).unwrap();
        assert!(s.bluetooth);
        assert_eq!(s.description.as_deref(), Some("UE BOOM 4"));
        assert_eq!(s.volume_pct, Some(65), "loudest channel");
    }

    #[test]
    fn unknown_sink_list_still_reports_the_name() {
        let s = parse_sink(r#"{"default_sink_name":"bluez_output.x"}"#, "garbage").unwrap();
        assert!(s.bluetooth);
        assert_eq!(s.description, None);
        assert_eq!(s.volume_pct, None);
        assert!(parse_sink(r#"{"default_sink_name":""}"#, "[]").is_none());
        assert!(parse_sink("garbage", "[]").is_none());
    }
}
