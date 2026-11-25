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
    /// ALSA null device ("Discard all samples (playback) or generate zero samples (capture)"):
    /// captures silence forever, never a microphone.
    Null,
    /// A named capture device.
    Real,
}

pub fn classify_name(name: &str) -> SourceKind {
    let n = name.trim().to_lowercase();
    if n.contains("discard all samples") || n.contains("generate zero samples") || n == "null" || n.starts_with("null:") {
        return SourceKind::Null;
    }
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
        return Decision::Reject(format!("{NO_MIC_PREFIX}: no input device available"));
    };
    let kind = classify_name(default_name);
    // A null device is never a microphone, whatever `reject_monitor` says.
    let default_bad = kind == SourceKind::Null
        || (reject_monitor
            && (kind == SourceKind::Monitor || (kind == SourceKind::Generic && default_is_monitor == Some(true))));
    if !default_bad {
        return Decision::Use(default_idx);
    }
    // A named non-default device that is neither a monitor, null nor an alias (could be a real mic).
    if let Some(i) = candidates
        .iter()
        .enumerate()
        .find(|(i, n)| *i != default_idx && classify_name(n) == SourceKind::Real)
        .map(|(i, _)| i)
    {
        return Decision::Use(i);
    }
    Decision::Reject(format!("{NO_MIC_PREFIX}: only null/monitor devices present ({default_name})"))
}

/// Start of every "there is no microphone" reason (health `audio.last_error`).
pub const NO_MIC_PREFIX: &str = "no microphone";

/// Typed "no microphone" failure of `AudioManager::start`, so the retry loop can tell it
/// from a device that exists but cannot be opened.
#[derive(Debug)]
pub struct NoMicrophone(pub String);

impl std::fmt::Display for NoMicrophone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NoMicrophone {}

/// Parse `pactl --format=json list sources` and report whether `source_name` is a monitor
/// (`monitor_of_sink` set, or name ends in ".monitor"). None if absent or unparsable.
pub fn source_is_monitor(json: &str, source_name: &str) -> Option<bool> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let src = v.as_array()?.iter().find(|s| s.get("name").and_then(|n| n.as_str()) == Some(source_name))?;
    let of_sink = src.get("monitor_of_sink").map(|m| !m.is_null()).unwrap_or(false);
    Some(of_sink || source_name.ends_with(".monitor"))
}

pub(crate) fn run_pactl(args: &[&str], timeout: Duration) -> Option<String> {
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

/// Name of the PipeWire/Pulse default source and its description (what an alias capture
/// device such as "default" or "pipewire" really is). Blocking; None without `pactl`.
pub fn pactl_default_source_info() -> Option<(String, Option<String>)> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let t = Duration::from_secs(2);
    let name = run_pactl(&["get-default-source"], t)?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let desc = run_pactl(&["--format=json", "list", "sources"], t).and_then(|j| source_description(&j, &name));
    Some((name, desc))
}

/// `description` of the named source in `pactl --format=json list sources`.
pub fn source_description(json: &str, source_name: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let src = v.as_array()?.iter().find(|s| s.get("name").and_then(|n| n.as_str()) == Some(source_name))?;
    src.get("description").and_then(|d| d.as_str()).map(str::to_string)
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
        assert_eq!(classify_name(NULL_DEV), Null);
        assert_eq!(classify_name("null"), Null);
    }

    const NULL_DEV: &str = "Discard all samples (playback) or generate zero samples (capture)";

    #[test]
    fn null_device_default_is_rejected_with_reason() {
        let d = decide(&v(&[NULL_DEV]), 0, None, true);
        assert!(
            matches!(d, Decision::Reject(ref m) if m.starts_with("no microphone: only null/monitor devices present")),
            "{d:?}"
        );
        // the guard flag does not matter: a null device is never a microphone
        assert!(matches!(decide(&v(&[NULL_DEV]), 0, None, false), Decision::Reject(_)));
    }

    #[test]
    fn null_default_falls_back_to_a_real_mic_when_plugged_in() {
        let c = v(&[NULL_DEV, "default", "sysdefault:CARD=BOMGE"]);
        assert_eq!(decide(&c, 0, None, true), Decision::Use(2));
    }

    #[test]
    fn describes_a_pulse_source() {
        let j = r#"[{"name":"alsa_input.usb-BOMGE","description":"BOMGE USB Mic Mono"}]"#;
        assert_eq!(source_description(j, "alsa_input.usb-BOMGE").as_deref(), Some("BOMGE USB Mic Mono"));
        assert_eq!(source_description(j, "x"), None);
    }

    #[test]
    fn monitor_default_without_alternative_is_rejected() {
        let d = decide(&v(&["UE BOOM 4 Monitor"]), 0, None, true);
        assert!(matches!(d, Decision::Reject(ref m) if m.starts_with("no microphone")));
    }

    #[test]
    fn generic_default_depends_on_server_verdict() {
        let c = v(&["Default Audio Device"]);
        assert_eq!(
            decide(&c, 0, Some(true), true),
            Decision::Reject("no microphone: only null/monitor devices present (Default Audio Device)".into())
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
