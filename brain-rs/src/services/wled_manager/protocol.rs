//! WLED JSON-API protocol helpers: effect/palette name resolution, payload
//! construction and the thin HTTP calls. No manager state lives here.
use anyhow::Result;
use reqwest::Client;
use serde_json::{json, Value};
use tracing::warn;

use crate::config::{blend_mode_id, WledEffect};

/// Static fallback for common effects when the device list is unavailable.
///
/// WLED effect/palette ids are firmware specific. These are read from a controller running
/// WLED 0.15.4 (`GET /json/eff`, `/json/pal`); an older table (rainbow 8, fire 16, ocean 20...)
/// was wrong for that firmware and `fire` silently became `Saw`. Prefer exact device names in
/// config (`"Fire 2012"`); the device list is always tried first.
fn static_effect_id(name: &str) -> Option<u8> {
    Some(match name {
        "solid" => 0,
        "blink" => 1,
        "breathe" => 2,
        "wipe" => 3,
        "colorloop" => 8,
        "rainbow" => 9,
        "scan" => 10,
        "fade" => 12,
        "twinkle" => 17,
        "chase" => 28,
        "fireworks" => 42,
        "fire" | "fire 2012" => 66,
        "meteor" => 76,
        _ => return None,
    })
}

fn static_palette_id(name: &str) -> Option<u8> {
    Some(match name {
        "default" => 0,
        "party" => 6,
        "lava" => 8,
        "ocean" => 9,
        "forest" => 10,
        "rainbow" => 11,
        "fire" => 35,
        _ => return None,
    })
}

/// Resolve by numeric id, then device list (case-insensitive), then static table.
/// Unknown names fall back to 0 (solid / default) with a warning.
fn resolve_id(name: &str, device_list: &[String], fallback: fn(&str) -> Option<u8>) -> u8 {
    let name = name.trim();
    if let Ok(id) = name.parse::<u8>() {
        return id;
    }
    let lower = name.to_lowercase();
    if let Some(i) = device_list.iter().position(|n| n.to_lowercase() == lower) {
        return u8::try_from(i).unwrap_or(0);
    }
    fallback(&lower).unwrap_or_else(|| {
        warn!("Unknown WLED effect/palette '{}', using 0", name);
        0
    })
}

pub(super) fn resolve_effect_id(name: &str, device_list: &[String]) -> u8 {
    resolve_id(name, device_list, static_effect_id)
}

pub(super) fn resolve_palette_id(name: &str, device_list: &[String]) -> u8 {
    resolve_id(name, device_list, static_palette_id)
}

/// Build the `/json/state` payload for an effect. Unset fields use the
/// previous hardcoded defaults (bri 255, ix 128, transition 7, sx 128).
pub(super) fn build_effect_payload(effect: &WledEffect, effect_id: u8, palette_id: Option<u8>) -> Value {
    let mut seg = json!({
        "id": 0,
        "fx": effect_id,
        "sx": effect.speed.unwrap_or(128),
        "ix": effect.intensity.unwrap_or(128),
        "pal": palette_id.unwrap_or(0),
    });
    if let Some(colors) = &effect.colors {
        seg["col"] = json!(colors.iter().take(3).collect::<Vec<_>>());
    }
    json!({
        "on": true,
        "bri": effect.brightness.unwrap_or(255),
        "transition": effect.transition.unwrap_or(7),
        "seg": [seg],
    })
}

/// `(major, minor)` of a WLED `ver` string ("0.15.4", "16.0.1", "16.0.0-beta").
pub(super) fn parse_version(ver: &str) -> Option<(u32, u32)> {
    let mut parts = ver.trim().trim_start_matches('v').split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts
        .next()
        .map(|m| m.chars().take_while(char::is_ascii_digit).collect::<String>())
        .and_then(|m| m.parse().ok())
        .unwrap_or(0);
    Some((major, minor))
}

/// WLED 0.x has no segment blend modes (they arrived with 16). An unparsable version is
/// assumed new.
pub(super) fn is_pre_16(ver: &str) -> bool {
    parse_version(ver).is_some_and(|(major, _)| major == 0)
}

/// Segment ids removed when leaving an overlay (ids 1.. of up to four overlaid strips).
const CLEANUP_IDS: std::ops::RangeInclusive<u8> = 1..=8;

/// Overlay payload: every strip range (`start`, `stop`) gets the effect's layers as
/// consecutive segment ids (`strip * layers + layer`), bottom layer first. `ids` is the
/// resolved `(effect id, palette id)` per layer.
pub(super) fn build_overlay_payload(
    effect: &WledEffect,
    ids: &[(u8, Option<u8>)],
    ranges: &[(usize, usize)],
) -> Value {
    let mut segs = Vec::new();
    for (strip, (start, stop)) in ranges.iter().enumerate() {
        for (layer, (l, (fx, pal))) in effect.segments.iter().zip(ids).enumerate() {
            let mut seg = json!({
                "id": strip * effect.segments.len() + layer,
                "start": start,
                "stop": stop,
                "on": true,
                "fx": fx,
                "sx": l.speed.unwrap_or(128),
                "ix": l.intensity.unwrap_or(128),
                "pal": pal.unwrap_or(0),
                "bri": l.brightness.unwrap_or(255),
                "bm": l.blend.as_deref().and_then(blend_mode_id).unwrap_or(0),
            });
            if let Some(colors) = &l.colors {
                seg["col"] = json!(colors.iter().take(3).collect::<Vec<_>>());
            }
            segs.push(seg);
        }
    }
    // Stale layers from an earlier, larger overlay.
    for id in (effect.segments.len() * ranges.len()) as u8..=*CLEANUP_IDS.end() {
        segs.push(json!({ "id": id, "stop": 0 }));
    }
    json!({
        "on": true,
        "bri": effect.brightness.unwrap_or(255),
        "transition": effect.transition.unwrap_or(7),
        "seg": segs,
    })
}

/// Make a single-effect payload safe after an overlay: remove segments 1.. (they would
/// multiply into the new look), reset segment 0's blend mode, and when `total` is given
/// widen segment 0 back over the whole controller.
pub(super) fn add_overlay_cleanup(payload: &mut Value, total: Option<usize>) {
    let seg0 = &mut payload["seg"][0];
    seg0["bm"] = json!(0);
    if let Some(total) = total {
        seg0["start"] = json!(0);
        seg0["stop"] = json!(total);
    }
    let segs = payload["seg"].as_array_mut().expect("seg is an array");
    // The effect stays at index 0; removals follow, highest id first.
    segs.extend(CLEANUP_IDS.rev().map(|id| json!({ "id": id, "stop": 0 })));
}

pub(super) async fn post_state(client: &Client, ip: &str, payload: &Value) -> Result<()> {
    let resp = client
        .post(format!("http://{}/json/state", ip))
        .json(payload)
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("status {}", resp.status());
    }
    Ok(())
}

/// GET http://ip/json/{effects|palettes} -> JSON array of names.
pub(super) async fn fetch_name_list(client: &Client, ip: &str, path: &str) -> Result<Vec<String>> {
    let resp = client.get(format!("http://{}/json/{}", ip, path)).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("status {}", resp.status());
    }
    Ok(resp.json::<Vec<String>>().await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{mock_http, Seen};

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn resolves_numeric_device_and_static() {
        let list = names(&["Solid", "Blink", "Colorloop"]);
        assert_eq!(resolve_effect_id("42", &list), 42);
        assert_eq!(resolve_effect_id("colorloop", &list), 2);
        // fallback ids are those of WLED 0.15.4 (see static_effect_id)
        assert_eq!(resolve_effect_id("Fire", &[]), 66);
        assert_eq!(resolve_effect_id("fade", &[]), 12);
        assert_eq!(resolve_effect_id("Rainbow", &[]), 9);
        assert_eq!(resolve_effect_id("nonsense", &list), 0);
        assert_eq!(resolve_palette_id("Lava", &[]), 8);
        assert_eq!(resolve_palette_id("ocean", &[]), 9);
    }

    #[test]
    fn payload_defaults_match_legacy() {
        let e = WledEffect { effect: "fire".into(), ..Default::default() };
        let p = build_effect_payload(&e, 16, None);
        assert_eq!(p["bri"], 255);
        assert_eq!(p["transition"], 7);
        assert_eq!(p["seg"][0]["fx"], 16);
        assert_eq!(p["seg"][0]["sx"], 128);
        assert_eq!(p["seg"][0]["ix"], 128);
        assert!(p["seg"][0].get("col").is_none());
    }

    #[test]
    fn payload_uses_all_fields() {
        let e = WledEffect {
            effect: "x".into(),
            palette: Some("y".into()),
            speed: Some(10),
            intensity: Some(20),
            brightness: Some(30),
            transition: Some(0),
            colors: Some(vec![[255, 0, 0], [0, 255, 0], [0, 0, 255], [1, 1, 1]]),
            ..Default::default()
        };
        let p = build_effect_payload(&e, 7, Some(3));
        assert_eq!(p["bri"], 30);
        assert_eq!(p["transition"], 0);
        assert_eq!(p["seg"][0]["sx"], 10);
        assert_eq!(p["seg"][0]["ix"], 20);
        assert_eq!(p["seg"][0]["pal"], 3);
        assert_eq!(p["seg"][0]["col"], json!([[255, 0, 0], [0, 255, 0], [0, 0, 255]]));
    }

    #[test]
    fn versions_parse_and_pre_16_is_major_zero() {
        assert_eq!(parse_version("0.15.4"), Some((0, 15)));
        assert_eq!(parse_version("16.0.1"), Some((16, 0)));
        assert_eq!(parse_version("16.0.0-beta1"), Some((16, 0)));
        assert_eq!(parse_version("v16.1"), Some((16, 1)));
        assert_eq!(parse_version("junk"), None);
        assert!(is_pre_16("0.15.4"));
        assert!(is_pre_16("0.14.0-b1"));
        assert!(!is_pre_16("16.0.1"));
        assert!(!is_pre_16("junk"));
    }

    fn overlay_effect() -> WledEffect {
        crate::config::Config::default().wled.effects.idle
    }

    #[test]
    fn overlay_payload_has_both_layers_per_strip() {
        let e = overlay_effect();
        let p = build_overlay_payload(&e, &[(0, None), (12, None)], &[(0, 180), (180, 360)]);
        let seg = p["seg"].as_array().unwrap();
        assert_eq!(seg[0]["id"], 0);
        assert_eq!((seg[0]["start"].as_u64(), seg[0]["stop"].as_u64()), (Some(0), Some(180)));
        assert_eq!((seg[0]["fx"].as_u64(), seg[0]["bri"].as_u64(), seg[0]["bm"].as_u64()), (Some(0), Some(90), Some(0)));
        assert_eq!(seg[0]["col"], json!([[255, 25, 0], [0, 0, 0], [0, 0, 0]]));
        assert_eq!(seg[1]["id"], 1);
        assert_eq!((seg[1]["fx"].as_u64(), seg[1]["sx"].as_u64(), seg[1]["bm"].as_u64()), (Some(12), Some(0), Some(6)));
        assert_eq!(seg[1]["on"], true);
        assert_eq!(seg[2]["id"], 2);
        assert_eq!((seg[2]["start"].as_u64(), seg[2]["stop"].as_u64()), (Some(180), Some(360)));
        assert_eq!(seg[3]["bm"], 6);
        // ids 4..=8 are removed
        assert_eq!(seg[4], json!({"id": 4, "stop": 0}));
        assert_eq!(p["bri"], 255);
    }

    #[test]
    fn cleanup_removes_overlay_layers_and_resets_blend() {
        let e = WledEffect { effect: "fire".into(), ..Default::default() };
        let mut p = build_effect_payload(&e, 66, Some(8));
        add_overlay_cleanup(&mut p, Some(360));
        let seg = p["seg"].as_array().unwrap();
        assert_eq!(seg.len(), 9);
        assert_eq!(seg[1], json!({"id": 8, "stop": 0}));
        assert_eq!(seg[8], json!({"id": 1, "stop": 0}));
        assert_eq!(seg[0]["id"], 0);
        assert_eq!(seg[0]["fx"], 66);
        assert_eq!(seg[0]["bm"], 0);
        assert_eq!((seg[0]["start"].as_u64(), seg[0]["stop"].as_u64()), (Some(0), Some(360)));
        let mut p = build_effect_payload(&e, 66, None);
        add_overlay_cleanup(&mut p, None);
        assert!(p["seg"][0].get("stop").is_none());
    }

    #[test]
    fn effect_config_parses_minimal_and_full() {
        let e: WledEffect = serde_yaml::from_str("effect: realtime").unwrap();
        assert_eq!(e.effect, "realtime");
        assert!(e.intensity.is_none());
        let e: WledEffect = serde_yaml::from_str(
            "effect: '66'\npalette: ocean\nspeed: 5\nintensity: 6\nbrightness: 7\ntransition: 8\ncolors: [[1,2,3]]",
        )
        .unwrap();
        assert_eq!(e.intensity, Some(6));
        assert_eq!(e.colors, Some(vec![[1, 2, 3]]));
    }

    /// Mock device answering every request with `status` and `body`.
    async fn mock_server(status: u16, body: &'static str) -> (String, Seen) {
        let (addr, _, seen) = mock_http(&[("*", status, body)]).await;
        (addr, seen)
    }

    #[tokio::test]
    async fn fetches_effect_list_from_device() {
        let (addr, seen) = mock_server(200, r#"["Solid","Blink","Breathe"]"#).await;
        let list = fetch_name_list(&Client::new(), &addr, "effects").await.unwrap();
        assert_eq!(list, names(&["Solid", "Blink", "Breathe"]));
        assert!(seen.lock().await[0].starts_with("GET /json/effects"));
    }

    #[tokio::test]
    async fn posts_state_and_reports_http_errors() {
        let (addr, seen) = mock_server(200, "{}").await;
        let e = WledEffect { effect: "fire".into(), speed: Some(99), ..Default::default() };
        let payload = build_effect_payload(&e, 16, None);
        post_state(&Client::new(), &addr, &payload).await.unwrap();
        let req = seen.lock().await[0].clone();
        assert!(req.starts_with("POST /json/state"));
        assert!(req.contains("\"sx\":99"));

        let (bad, _) = mock_server(500, "{}").await;
        assert!(post_state(&Client::new(), &bad, &payload).await.is_err());
    }
}
