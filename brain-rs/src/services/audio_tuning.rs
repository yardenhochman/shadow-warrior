//! Live tuning of the shout meter and detector from the dashboard.
//!
//! The running brain already shares one `Arc<RwLock<AudioConfig>>` that the meter and the
//! detector re-read every frame (services/audio_manager.rs), so "live" means writing validated
//! values into it: they take effect on the next 16 ms frame, no restart. This module holds
//!
//! * [`PARAMS`], the single descriptor table (key, label, range, step, unit, help). The HTTP
//!   API and the dashboard are generated from it: a new numeric `audio.*` key appears in the UI
//!   by adding one row here;
//! * [`apply`] / [`values`] / [`reset`]: partial update with clamping, readout, defaults;
//! * [`save_into_yaml`]: persists values into the `audio:` section of a config file as a minimal
//!   text edit (comments and other content preserved).

use crate::config::AudioConfig;
use serde::Serialize;
use serde_json::{Map, Value};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Float,
    Int,
    Bool,
}

/// One tunable `audio.*` key.
#[derive(Serialize)]
pub struct Param {
    pub key: &'static str,
    pub label: &'static str,
    /// UI group: "meter" or "detector".
    pub group: &'static str,
    pub kind: Kind,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub unit: &'static str,
    pub help: &'static str,
    #[serde(skip)]
    get: fn(&AudioConfig) -> f64,
    #[serde(skip)]
    set: fn(&mut AudioConfig, f64),
}

macro_rules! param {
    (f32, $field:ident, $group:expr, $label:expr, $min:expr, $max:expr, $step:expr, $unit:expr, $help:expr) => {
        Param {
            key: stringify!($field), label: $label, group: $group, kind: Kind::Float,
            min: $min, max: $max, step: $step, unit: $unit, help: $help,
            get: |c| c.$field as f64,
            set: |c, v| c.$field = v as f32,
        }
    };
    (u32, $field:ident, $group:expr, $label:expr, $min:expr, $max:expr, $step:expr, $unit:expr, $help:expr) => {
        Param {
            key: stringify!($field), label: $label, group: $group, kind: Kind::Int,
            min: $min, max: $max, step: $step, unit: $unit, help: $help,
            get: |c| c.$field as f64,
            set: |c, v| c.$field = v as u32,
        }
    };
    (bool, $field:ident, $group:expr, $label:expr, $help:expr) => {
        Param {
            key: stringify!($field), label: $label, group: $group, kind: Kind::Bool,
            min: 0.0, max: 1.0, step: 1.0, unit: "", help: $help,
            get: |c| if c.$field { 1.0 } else { 0.0 },
            set: |c, v| c.$field = v >= 0.5,
        }
    };
}

/// All live-tunable keys, in UI order. Add a row here to expose a new numeric `audio.*` key.
pub static PARAMS: &[Param] = &[
    param!(f32, shout_meter_lo_db, "meter", "Meter start (lo)", 0.0, 80.0, 0.5, "dB",
        "dB above the noise floor where the meter starts; raise to ignore clicks, typing and talking"),
    param!(f32, shout_meter_hi_db, "meter", "Meter full (hi)", 1.0, 100.0, 0.5, "dB",
        "dB above the noise floor where the meter is full; lower so a normal shout reaches 100 %"),
    param!(f32, shout_meter_attack_ms, "meter", "Attack", 0.0, 500.0, 5.0, "ms",
        "rise time; higher filters key clicks and claps shorter than this, 0 = immediate"),
    param!(f32, shout_meter_release_ms, "meter", "Release", 16.0, 3000.0, 10.0, "ms",
        "fall time; higher holds the meter longer after a shout"),
    param!(bool, shout_meter_gate, "meter", "Spectral gate",
        "dim frames that do not look like a shout (voiced/bright/flat checks); off = loudness only"),
    param!(f32, shout_margin_db, "detector", "Margin over floor", 3.0, 40.0, 0.5, "dB",
        "required dB above the noise floor for a frame to count; raise if it fires on noise, lower if shouts are missed"),
    param!(f32, shout_playback_margin_db, "detector", "Playback margin", 0.0, 30.0, 0.5, "dB",
        "extra margin (also added to the meter start) while our own speakers play"),
    param!(f32, shout_score_threshold, "detector", "Score threshold", 1.0, 50.0, 0.5, "",
        "accumulated score at which ShoutDetected fires; lower = more sensitive"),
    param!(f32, shout_min_duration_sec, "detector", "Min duration", 0.02, 1.0, 0.01, "s",
        "a shout must last this long before it fires; keep below ~0.3 s for responsiveness"),
    param!(u32, vad_hangover_frames, "detector", "VAD hangover", 0.0, 60.0, 1.0, "frames",
        "keep the voice gate open this many 16 ms frames after the last voiced frame"),
    param!(f32, shout_sensitivity, "detector", "Sensitivity", 0.1, 5.0, 0.05, "x",
        "score gain per shouty frame; higher fires sooner"),
    param!(f32, vad_threshold, "detector", "VAD threshold", 0.0, 1.0, 0.01, "",
        "voice-activity probability that opens the gate; lower is more permissive"),
    param!(f32, shout_score_decay, "detector", "Score decay", 0.5, 0.999, 0.005, "",
        "per-frame score decay on non-shouty frames; closer to 1 = longer memory"),
    param!(u32, shout_gap_frames, "detector", "Gap tolerance", 0.0, 30.0, 1.0, "frames",
        "non-shouty frames tolerated inside a sustained shout"),
    param!(u32, shout_min_features, "detector", "Features required", 1.0, 4.0, 1.0, "of 4",
        "how many of the 4 checks (voiced, centroid, tilt, flatness) must pass"),
    param!(f32, shout_f0_min_hz, "detector", "Pitch min", 50.0, 600.0, 5.0, "Hz",
        "lowest voiced pitch that counts as a shout"),
    param!(f32, shout_f0_max_hz, "detector", "Pitch max", 200.0, 1500.0, 5.0, "Hz",
        "highest voiced pitch that counts as a shout"),
    param!(f32, shout_pitch_conf_min, "detector", "Pitch confidence", 0.0, 1.0, 0.01, "",
        "minimum autocorrelation confidence to count a frame as voiced"),
    param!(f32, shout_centroid_min_hz, "detector", "Centroid min", 0.0, 4000.0, 25.0, "Hz",
        "minimum spectral centroid; shouts are bright, rumble and bass are not"),
    param!(f32, shout_tilt_min_db_per_khz, "detector", "Tilt min", -60.0, 10.0, 0.5, "dB/kHz",
        "minimum spectral tilt; lower accepts darker sounds"),
    param!(f32, shout_flatness_max, "detector", "Flatness max", 0.0, 1.0, 0.01, "",
        "maximum spectral flatness; lower rejects noise-like frames (wind, crowd) more strictly"),
];

pub fn param(key: &str) -> Option<&'static Param> {
    PARAMS.iter().find(|p| p.key == key)
}

fn json_value(p: &Param, v: f64) -> Value {
    match p.kind {
        Kind::Bool => Value::Bool(v >= 0.5),
        Kind::Int => Value::from(v.round() as i64),
        // Round-trip through f32 so 0.12f32 shows as 0.12, not 0.11999999731779099.
        Kind::Float => Value::from(format!("{}", v as f32).parse::<f64>().unwrap_or(v)),
    }
}

/// Current values of all params, keyed by name.
pub fn values(cfg: &AudioConfig) -> Map<String, Value> {
    PARAMS.iter().map(|p| (p.key.to_string(), json_value(p, (p.get)(cfg)))).collect()
}

/// Full state for the API: values, defaults and descriptors.
#[derive(Serialize)]
pub struct TuningState {
    pub values: Map<String, Value>,
    pub defaults: Map<String, Value>,
    pub params: &'static [Param],
    /// Keys whose current value differs from the default.
    pub modified: Vec<&'static str>,
}

pub fn state(cfg: &AudioConfig, defaults: &AudioConfig) -> TuningState {
    let modified = PARAMS.iter().filter(|p| (p.get)(cfg) != (p.get)(defaults)).map(|p| p.key).collect();
    TuningState { values: values(cfg), defaults: values(defaults), params: PARAMS, modified }
}

fn as_number(key: &str, v: &Value) -> Result<f64, String> {
    match v {
        Value::Number(n) => n.as_f64().filter(|x| x.is_finite()).ok_or_else(|| format!("{key}: not a finite number")),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        _ => Err(format!("{key}: expected a number or boolean")),
    }
}

/// Apply a partial update, clamping every value to its range. Unknown keys or non-numeric values
/// reject the whole request (nothing is applied). Keeps `hi >= lo + 1` and `f0_max >= f0_min`.
pub fn apply(cfg: &mut AudioConfig, updates: &Map<String, Value>) -> Result<(), String> {
    let mut staged = Vec::with_capacity(updates.len());
    for (k, v) in updates {
        let p = param(k).ok_or_else(|| format!("unknown tuning key '{k}'"))?;
        let n = as_number(k, v)?;
        let n = match p.kind {
            Kind::Bool => if n >= 0.5 { 1.0 } else { 0.0 },
            Kind::Int => n.round().clamp(p.min, p.max),
            Kind::Float => n.clamp(p.min, p.max),
        };
        staged.push((p, n));
    }
    for (p, n) in staged {
        (p.set)(cfg, n);
    }
    if cfg.shout_meter_hi_db < cfg.shout_meter_lo_db + 1.0 {
        cfg.shout_meter_hi_db = cfg.shout_meter_lo_db + 1.0;
    }
    if cfg.shout_f0_max_hz < cfg.shout_f0_min_hz {
        cfg.shout_f0_max_hz = cfg.shout_f0_min_hz;
    }
    Ok(())
}

/// Copy every tunable from `defaults` into `cfg`.
pub fn reset(cfg: &mut AudioConfig, defaults: &AudioConfig) {
    for p in PARAMS {
        (p.set)(cfg, (p.get)(defaults));
    }
}

fn yaml_scalar(p: &Param, cfg: &AudioConfig) -> String {
    let v = (p.get)(cfg);
    match p.kind {
        Kind::Bool => (v >= 0.5).to_string(),
        Kind::Int => format!("{}", v.round() as i64),
        Kind::Float => {
            let s = format!("{}", v as f32);
            if s.contains('.') || s.contains('e') { s } else { format!("{s}.0") }
        }
    }
}

fn is_top_level_content(line: &str) -> bool {
    let t = line.trim_end();
    !t.is_empty() && !t.starts_with(' ') && !t.starts_with('\t') && !t.starts_with('#')
}

/// Rewrite the `audio:` section of `text`: replace the value of existing keys in place (keeping
/// indentation, comment column and the trailing comment), append missing keys that differ from
/// `defaults` at the end of the section. Everything else is untouched.
pub fn save_into_yaml(text: &str, cfg: &AudioConfig, defaults: &AudioConfig) -> Result<String, String> {
    let had_newline = text.ends_with('\n');
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let start = lines.iter().position(|l| l.trim_end() == "audio:" || l.starts_with("audio: #"));
    let Some(start) = start else {
        if lines.iter().any(|l| l.starts_with("audio:")) {
            return Err("config 'audio:' section is not a block mapping; edit it by hand".into());
        }
        // No audio section: append one with the modified keys only.
        let mut out = text.to_string();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("\naudio:\n");
        for p in PARAMS.iter().filter(|p| (p.get)(cfg) != (p.get)(defaults)) {
            out.push_str(&format!("  {}: {}\n", p.key, yaml_scalar(p, cfg)));
        }
        return Ok(out);
    };
    let mut end = start + 1;
    while end < lines.len() && !is_top_level_content(&lines[end]) {
        end += 1;
    }
    // New keys go right after the last non-blank line of the section.
    let mut insert_at = end;
    while insert_at > start + 1 && lines[insert_at - 1].trim().is_empty() {
        insert_at -= 1;
    }
    let indent = lines[start + 1..end]
        .iter()
        .find(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(|l| l[..l.len() - l.trim_start().len()].to_string())
        .unwrap_or_else(|| "  ".to_string());
    let mut appended = Vec::new();
    for p in PARAMS {
        let new = yaml_scalar(p, cfg);
        let prefix = format!("{indent}{}:", p.key);
        match (start + 1..end).find(|&i| lines[i].starts_with(&prefix)) {
            Some(i) => {
                let rest = &lines[i][prefix.len()..];
                let lead = rest.len() - rest.trim_start().len();
                let body = &rest[lead..];
                let (val, comment) = match body.find(" #") {
                    Some(c) => (&body[..c], &body[c..]),
                    None => (body, ""),
                };
                let val_trim = val.trim_end();
                if val_trim.is_empty() || val_trim.starts_with(['{', '[', '"', '\'', '|', '>', '&', '*']) {
                    return Err(format!("{}: unsupported existing value '{}', edit by hand", p.key, val_trim));
                }
                // Unchanged (also "200" vs "200.0"): leave the line alone.
                let same_num = val_trim.parse::<f32>().is_ok_and(|x| x as f64 == (p.get)(cfg) as f32 as f64);
                if val_trim == new || (p.kind != Kind::Bool && same_num) {
                    continue;
                }
                let spaces = " ".repeat(lead.max(1));
                lines[i] = if comment.is_empty() {
                    format!("{prefix}{spaces}{new}")
                } else {
                    // Keep the comment column while the new value fits the old field.
                    let pad = " ".repeat(val.len().saturating_sub(new.len()));
                    format!("{prefix}{spaces}{new}{pad}{comment}")
                };
            }
            None => {
                if (p.get)(cfg) != (p.get)(defaults) {
                    appended.push(format!("{prefix} {new}"));
                }
            }
        }
    }
    for (n, l) in appended.into_iter().enumerate() {
        lines.insert(insert_at + n, l);
    }
    let mut out = lines.join("\n");
    if had_newline {
        out.push('\n');
    }
    Ok(out)
}

/// Persist into the config file: `.bak` once, atomic replace.
pub fn save_to_file(path: &Path, cfg: &AudioConfig, defaults: &AudioConfig) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let out = save_into_yaml(&text, cfg, defaults)?;
    serde_yaml::from_str::<serde_yaml::Value>(&out).map_err(|e| format!("edited config does not parse: {e}"))?;
    let bak = path.with_extension("yaml.bak");
    if !bak.exists() {
        std::fs::write(&bak, &text).map_err(|e| format!("write {}: {e}", bak.display()))?;
    }
    let tmp = path.with_extension("yaml.tmp");
    std::fs::write(&tmp, &out).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename {}: {e}", tmp.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use serde_json::json;

    fn cfg() -> AudioConfig {
        Config::default().audio
    }
    fn upd(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn partial_update_changes_only_given_keys() {
        let mut c = cfg();
        let before = values(&c);
        apply(&mut c, &upd(json!({"shout_meter_lo_db": 30.0}))).unwrap();
        let after = values(&c);
        assert_eq!(after["shout_meter_lo_db"], json!(30.0));
        for (k, v) in &before {
            if k != "shout_meter_lo_db" {
                assert_eq!(&after[k], v, "{k} changed");
            }
        }
    }

    #[test]
    fn clamps_out_of_range_and_rounds_ints() {
        let mut c = cfg();
        apply(&mut c, &upd(json!({"shout_margin_db": 999, "vad_hangover_frames": 7.6, "vad_threshold": -3, "shout_min_features": 0}))).unwrap();
        assert_eq!(c.shout_margin_db, 40.0);
        assert_eq!(c.vad_hangover_frames, 8);
        assert_eq!(c.vad_threshold, 0.0);
        assert_eq!(c.shout_min_features, 1);
    }

    #[test]
    fn bool_accepts_bool_and_number() {
        let mut c = cfg();
        apply(&mut c, &upd(json!({"shout_meter_gate": true}))).unwrap();
        assert!(c.shout_meter_gate);
        apply(&mut c, &upd(json!({"shout_meter_gate": 0}))).unwrap();
        assert!(!c.shout_meter_gate);
    }

    #[test]
    fn invalid_request_applies_nothing() {
        let mut c = cfg();
        let before = values(&c);
        assert!(apply(&mut c, &upd(json!({"shout_margin_db": 20.0, "nope": 1}))).is_err());
        assert!(apply(&mut c, &upd(json!({"shout_margin_db": 20.0, "shout_meter_lo_db": "x"}))).is_err());
        assert_eq!(values(&c), before);
    }

    #[test]
    fn hi_stays_above_lo() {
        let mut c = cfg();
        apply(&mut c, &upd(json!({"shout_meter_lo_db": 60.0, "shout_meter_hi_db": 20.0}))).unwrap();
        assert!(c.shout_meter_hi_db >= c.shout_meter_lo_db + 1.0);
    }

    #[test]
    fn reset_restores_defaults_and_state_reports_modified() {
        let defaults = cfg();
        let mut c = defaults.clone();
        apply(&mut c, &upd(json!({"shout_score_threshold": 5.0, "shout_meter_gate": !defaults.shout_meter_gate}))).unwrap();
        let st = state(&c, &defaults);
        assert_eq!(st.modified.len(), 2);
        assert!(st.modified.contains(&"shout_score_threshold") && st.modified.contains(&"shout_meter_gate"));
        reset(&mut c, &defaults);
        assert!(state(&c, &defaults).modified.is_empty());
        assert_eq!(values(&c), values(&defaults));
    }

    #[test]
    fn descriptors_are_consistent() {
        let c = cfg();
        for p in PARAMS {
            let v = (p.get)(&c);
            assert!(v >= p.min && v <= p.max, "{} default {} outside {}..{}", p.key, v, p.min, p.max);
            assert!(p.step > 0.0 && p.min < p.max);
        }
        let keys: std::collections::HashSet<_> = PARAMS.iter().map(|p| p.key).collect();
        assert_eq!(keys.len(), PARAMS.len());
    }

    const SAMPLE: &str = "# top comment\nbrain:\n  name: x\n\naudio:\n  sample_rate: 16000\n  # section comment\n  shout_margin_db: 15.0            # required dB above floor\n  shout_meter_lo_db: 45.0          # lo\n  shout_meter_gate: false          # gate\n  vad_hangover_frames: 6\n  clip_dir: \"./clips\"\n\nwled:\n  controllers: []\n";

    fn line_with<'a>(s: &'a str, key: &str) -> &'a str {
        s.lines().find(|l| l.trim_start().starts_with(key)).unwrap()
    }

    #[test]
    fn yaml_edit_preserves_everything_else() {
        let defaults = cfg();
        let mut c = defaults.clone();
        apply(&mut c, &upd(json!({"shout_margin_db": 12.5, "shout_meter_lo_db": 8.0, "shout_meter_gate": true, "vad_hangover_frames": 10, "shout_score_threshold": 9.0}))).unwrap();
        let out = save_into_yaml(SAMPLE, &c, &defaults).unwrap();
        for (key, val, cmt) in [("shout_margin_db", "12.5", "# required dB above floor"), ("shout_meter_lo_db", "8.0", "# lo"), ("shout_meter_gate", "true", "# gate")] {
            let (old, new) = (line_with(SAMPLE, key), line_with(&out, key));
            assert!(new.starts_with(&format!("  {key}: {val}")), "{new}");
            assert!(new.ends_with(cmt), "{new}");
            assert_eq!(old.find('#'), new.find('#'), "comment column moved: {new}");
        }
        assert!(out.contains("  vad_hangover_frames: 10\n"), "{out}");
        // missing key that differs from the default is appended inside audio:, before the blank line
        assert!(out.contains("  clip_dir: \"./clips\"\n  shout_score_threshold: 9.0\n\nwled:"), "{out}");
        for keep in ["# top comment", "brain:\n  name: x", "  # section comment", "wled:\n  controllers: []"] {
            assert!(out.contains(keep), "lost {keep}");
        }
        let v: serde_yaml::Value = serde_yaml::from_str(&out).unwrap();
        assert_eq!(v["audio"]["shout_meter_lo_db"].as_f64(), Some(8.0));
        assert_eq!(v["audio"]["shout_score_threshold"].as_f64(), Some(9.0));
        assert_eq!(v["audio"]["shout_meter_gate"].as_bool(), Some(true));
        // saving again is idempotent
        assert_eq!(save_into_yaml(&out, &c, &defaults).unwrap(), out);
    }

    #[test]
    fn yaml_without_audio_section_gets_one() {
        let defaults = cfg();
        let mut c = defaults.clone();
        apply(&mut c, &upd(json!({"shout_margin_db": 20.0}))).unwrap();
        let out = save_into_yaml("wled:\n  x: 1\n", &c, &defaults).unwrap();
        let v: serde_yaml::Value = serde_yaml::from_str(&out).unwrap();
        assert_eq!(v["audio"]["shout_margin_db"].as_f64(), Some(20.0));
        assert_eq!(v["wled"]["x"].as_i64(), Some(1));
    }

    #[test]
    fn real_config_round_trips() {
        let real = include_str!("../../config.yaml");
        let defaults = cfg();
        let mut c = defaults.clone();
        apply(&mut c, &upd(json!({"shout_meter_lo_db": 40.0}))).unwrap();
        let out = save_into_yaml(real, &c, &defaults).unwrap();
        assert_eq!(out.lines().count(), real.lines().count());
        assert!(line_with(&out, "shout_meter_lo_db").contains("40.0"));
    }

    #[test]
    fn save_to_file_makes_one_backup() {
        let dir = std::env::temp_dir().join(format!("tuning-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        std::fs::write(&path, SAMPLE).unwrap();
        let defaults = cfg();
        let mut c = defaults.clone();
        apply(&mut c, &upd(json!({"shout_margin_db": 11.0}))).unwrap();
        save_to_file(&path, &c, &defaults).unwrap();
        apply(&mut c, &upd(json!({"shout_margin_db": 12.0}))).unwrap();
        save_to_file(&path, &c, &defaults).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("config.yaml.bak")).unwrap(), SAMPLE);
        assert!(std::fs::read_to_string(&path).unwrap().contains("shout_margin_db: 12.0"));
        assert!(!dir.join("config.yaml.tmp").exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
