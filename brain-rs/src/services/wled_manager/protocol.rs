//! WLED JSON-API protocol helpers: effect/palette name resolution, payload
//! construction and the thin HTTP calls. No manager state lives here.
use anyhow::Result;
use reqwest::Client;
use serde_json::{json, Value};
use tracing::warn;

use crate::config::WledEffect;

/// Static fallback for common effects when the device list is unavailable.
fn static_effect_id(name: &str) -> Option<u8> {
    Some(match name {
        "solid" => 0,
        "blink" => 1,
        "breathe" => 2,
        "wipe" => 3,
        "scan" => 5,
        "rainbow" => 8,
        "twinkle" => 15,
        "fire" | "fireworks" => 16,
        _ => return None,
    })
}

fn static_palette_id(name: &str) -> Option<u8> {
    Some(match name {
        "default" => 0,
        "random" => 5,
        "rainbow" => 9,
        "party" => 12,
        "ocean" => 20,
        "lava" => 22,
        "forest" => 25,
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
        assert_eq!(resolve_effect_id("Fire", &[]), 16);
        assert_eq!(resolve_effect_id("nonsense", &list), 0);
        assert_eq!(resolve_palette_id("Lava", &[]), 22);
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
