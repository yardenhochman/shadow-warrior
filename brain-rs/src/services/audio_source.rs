//! Telling a real microphone from a sink-monitor / loopback capture.
//!
//! With PipeWire and no real mic, the ALSA `default` capture is linked to the default
//! sink's monitor, so the brain would analyse its own playback. This module classifies
//! input device names and, on Linux, asks `pactl` whether the default source is a monitor.
//! Everything here is pure or hardware-free except `pactl_default_source_is_monitor`.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// Name says it captures playback ("...analog-stereo.monitor", "Monitor of X", loopback).
    Monitor,
    /// Alias resolved by the sound server ("default", "pipewire", "Default Audio Device"):
    /// can be a mic or a monitor; only the server knows.
    Generic,
    /// A named capture device.
    Real,
}

pub fn classify_name(name: &str) -> SourceKind {
    let n = name.trim().to_lowercase();
    if n.contains("monitor") || n.contains("loopback") || n.contains("stereo mix") || n.contains("what u hear") {
        return SourceKind::Monitor;
    }
    match n.as_str() {
        "default" | "pipewire" | "pulse" | "pulseaudio" | "sysdefault" | "default audio device" | "system default" => {
            SourceKind::Generic
        }
        _ => SourceKind::Real,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// Use `candidates[i]`.
    Use(usize),
    /// Refuse to capture; the string is the health error.
    Reject(String),
}

/// Pick the capture device. `candidates[default_idx]` is the host default; the rest are
/// alternatives. `default_is_monitor` is the sound server's verdict on the default source
/// (None = unknown). With `reject_monitor` false the default is always used.
pub fn decide(candidates: &[String], default_idx: usize, default_is_monitor: Option<bool>, reject_monitor: bool) -> Decision {
    let Some(default_name) = candidates.get(default_idx) else {
        return Decision::Reject("no input device available".into());
    };
    if !reject_monitor {
        return Decision::Use(default_idx);
    }
    let kind = classify_name(default_name);
    let default_bad = kind == SourceKind::Monitor || (kind == SourceKind::Generic && default_is_monitor == Some(true));
    if !default_bad {
        return Decision::Use(default_idx);
    }
    // A named non-default device that is neither a monitor nor an alias (could be a real mic).
    if let Some(i) = candidates
        .iter()
        .enumerate()
        .find(|(i, n)| *i != default_idx && classify_name(n) == SourceKind::Real)
        .map(|(i, _)| i)
    {
        return Decision::Use(i);
    }
    Decision::Reject(format!("no real microphone: capture is a sink monitor ({default_name})"))
}

/// Parse `pactl --format=json list sources` and report whether `source_name` is a monitor
/// (`monitor_of_sink` set, or name ends in ".monitor"). None if absent or unparsable.
pub fn source_is_monitor(json: &str, source_name: &str) -> Option<bool> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let src = v.as_array()?.iter().find(|s| s.get("name").and_then(|n| n.as_str()) == Some(source_name))?;
    let of_sink = src.get("monitor_of_sink").map(|m| !m.is_null()).unwrap_or(false);
    Some(of_sink || source_name.ends_with(".monitor"))
}

fn run_pactl(args: &[&str], timeout: Duration) -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut child = Command::new("pactl")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?; // pactl missing: skip
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait().ok()? {
            Some(st) if st.success() => break,
            Some(_) => return None,
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    Some(out)
}

/// Ask PipeWire/PulseAudio whether the default source is a monitor. Blocking (short
/// timeouts); None if `pactl` is missing, fails or the output is not understood.
pub fn pactl_default_source_is_monitor() -> Option<bool> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let t = Duration::from_secs(2);
    let name = run_pactl(&["get-default-source"], t)?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    match run_pactl(&["--format=json", "list", "sources"], t).and_then(|j| source_is_monitor(&j, &name)) {
        Some(b) => Some(b),
        None => Some(name.ends_with(".monitor")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn classifies_real_world_names() {
        use SourceKind::*;
        assert_eq!(classify_name("alsa_output.usb-Foo_Bar-00.analog-stereo.monitor"), Monitor);
        assert_eq!(classify_name("UE BOOM 4 Monitor"), Monitor);
        assert_eq!(classify_name("Monitor of Built-in Audio Analog Stereo"), Monitor);
        assert_eq!(classify_name("Loopback: PCM (hw:Loopback,1)"), Monitor);
        assert_eq!(classify_name("pipewire"), Generic);
        assert_eq!(classify_name("default"), Generic);
        assert_eq!(classify_name("Default Audio Device"), Generic);
        assert_eq!(classify_name("sysdefault:CARD=Device"), Real);
        assert_eq!(classify_name("USB PnP Sound Device Mono"), Real);
    }

    #[test]
    fn monitor_default_without_alternative_is_rejected() {
        let d = decide(&v(&["UE BOOM 4 Monitor"]), 0, None, true);
        assert!(matches!(d, Decision::Reject(ref m) if m.starts_with("no real microphone")));
    }

    #[test]
    fn generic_default_depends_on_server_verdict() {
        let c = v(&["Default Audio Device"]);
        assert_eq!(
            decide(&c, 0, Some(true), true),
            Decision::Reject("no real microphone: capture is a sink monitor (Default Audio Device)".into())
        );
        assert_eq!(decide(&c, 0, Some(false), true), Decision::Use(0));
        assert_eq!(decide(&c, 0, None, true), Decision::Use(0), "unknown verdict: do not block a possibly real mic");
    }

    #[test]
    fn falls_back_to_a_real_device_and_ignores_aliases() {
        let c = v(&["default", "pipewire", "sysdefault:CARD=Device"]);
        assert_eq!(decide(&c, 0, Some(true), true), Decision::Use(2));
        let c = v(&["default", "pipewire", "x.monitor"]);
        assert!(matches!(decide(&c, 0, Some(true), true), Decision::Reject(_)));
    }

    #[test]
    fn reject_monitor_false_keeps_old_behaviour() {
        assert_eq!(decide(&v(&["x.monitor"]), 0, Some(true), false), Decision::Use(0));
    }

    #[test]
    fn empty_list_rejects() {
        assert!(matches!(decide(&[], 0, None, true), Decision::Reject(_)));
    }

    #[test]
    fn parses_pactl_sources_json() {
        let j = r#"[{"name":"alsa_output.x.analog-stereo.monitor","monitor_of_sink":5},
                    {"name":"alsa_input.usb-mic","monitor_of_sink":null},
                    {"name":"bt.monitor"}]"#;
        assert_eq!(source_is_monitor(j, "alsa_output.x.analog-stereo.monitor"), Some(true));
        assert_eq!(source_is_monitor(j, "alsa_input.usb-mic"), Some(false));
        assert_eq!(source_is_monitor(j, "bt.monitor"), Some(true));
        assert_eq!(source_is_monitor(j, "missing"), None);
        assert_eq!(source_is_monitor("garbage", "x"), None);
    }
}
