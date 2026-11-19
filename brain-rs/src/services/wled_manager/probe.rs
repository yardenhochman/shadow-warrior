//! Layout probe: `GET /json/cfg` -> `hw.led.ins[]`, one entry per LED output.
//! Done once per connect (never polled: an ESP32-C3 reboots under fast GET polling).
//! Pure parsing and strip building; the manager does the HTTP and applies the result.
use serde_json::Value;

use crate::config::{next_free_column, StripConfig, WledController};
pub use crate::models::WledOutput;

/// Outputs of a `/json/cfg` document. `Err` says why the layout cannot be trusted; the
/// caller then falls back to the `/json/info` LED count.
pub(super) fn parse_cfg(cfg: &Value) -> Result<Vec<WledOutput>, String> {
    let ins = cfg["hw"]["led"]["ins"].as_array().ok_or("no hw.led.ins")?;
    let mut outputs = Vec::new();
    let mut next_start = 0usize;
    for (i, o) in ins.iter().enumerate() {
        let len = o["len"]
            .as_u64()
            .ok_or_else(|| format!("output {i} has no len"))? as usize;
        if len == 0 {
            continue; // an unused output
        }
        let start = o["start"].as_u64().unwrap_or(next_start as u64) as usize;
        if start != next_start {
            // The brain sends one frame from LED 0; gaps and overlaps cannot be mapped to strips.
            return Err(format!(
                "output {i} starts at {start}, expected {next_start} (not contiguous)"
            ));
        }
        next_start = start + len;
        outputs.push(WledOutput {
            start,
            len,
            rev: o["rev"].as_bool().unwrap_or(false),
            order: o["order"].as_u64().unwrap_or(0) as u8,
        });
    }
    if outputs.is_empty() {
        return Err("no output with LEDs".into());
    }
    Ok(outputs)
}

/// Strips for a controller with `auto_layout`: one per output, in LED order. The first keeps
/// the controller's own first column, the others take the lowest free columns
/// (`columns` limits them; outputs that find no column are left out, the second tuple
/// element counts them). `reverse` stays whatever the config has per position; the probed
/// `rev` is deliberately not applied (see docs/LEDS.md, "WLED reverse").
pub(super) fn auto_strips(
    controllers: &[WledController],
    ip: &str,
    outputs: &[WledOutput],
    columns: Option<usize>,
) -> (Vec<StripConfig>, usize) {
    let mut all: Vec<WledController> = controllers.to_vec();
    let Some(idx) = all.iter().position(|c| c.ip == ip) else {
        return (vec![], 0);
    };
    let old = std::mem::take(&mut all[idx].strips);
    let first_column = old.first().map_or(0, |s| s.column);
    let mut strips: Vec<StripConfig> = Vec::new();
    let mut dropped = 0;
    for (i, o) in outputs.iter().enumerate() {
        // `all[idx]` holds the strips placed so far, so next_free_column sees them.
        let Some(column) = (if i == 0 {
            Some(first_column)
        } else {
            next_free_column(&all, columns)
        }) else {
            dropped += 1;
            continue;
        };
        let reverse = old.get(i).is_some_and(|s| s.reverse);
        strips.push(StripConfig {
            column,
            leds: o.len,
            reverse,
        });
        all[idx].strips = strips.clone();
    }
    (strips, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: &str = include_str!("fixtures/cfg_0_15_4_one_strip.json");
    const TWO: &str = include_str!("fixtures/cfg_two_outputs.json");

    #[test]
    fn real_cfg_has_one_180_led_output() {
        let outs = parse_cfg(&serde_json::from_str(ONE).unwrap()).unwrap();
        assert_eq!(
            outs,
            vec![WledOutput {
                start: 0,
                len: 180,
                rev: false,
                order: 0
            }]
        );
    }

    #[test]
    fn two_outputs_and_their_rev() {
        let outs = parse_cfg(&serde_json::from_str(TWO).unwrap()).unwrap();
        assert_eq!(outs.len(), 2);
        assert_eq!((outs[1].start, outs[1].len, outs[1].rev), (180, 180, true));
    }

    #[test]
    fn unusable_cfg_is_an_error() {
        for bad in [
            "{}",
            r#"{"hw":{"led":{"ins":[]}}}"#,
            r#"{"hw":{"led":{"ins":[{"start":0,"len":0}]}}}"#,
            r#"{"hw":{"led":{"ins":[{"start":0,"len":60},{"start":100,"len":60}]}}}"#,
            r#"{"hw":{"led":{"ins":[{"start":0}]}}}"#,
        ] {
            assert!(
                parse_cfg(&serde_json::from_str(bad).unwrap()).is_err(),
                "{bad}"
            );
        }
        // a zero-length (unused) output is skipped
        let ok = r#"{"hw":{"led":{"ins":[{"start":0,"len":60},{"start":60,"len":0},{"start":60,"len":30}]}}}"#;
        assert_eq!(
            parse_cfg(&serde_json::from_str(ok).unwrap()).unwrap().len(),
            2
        );
    }

    fn ctrl(ip: &str, strips: Vec<StripConfig>) -> WledController {
        WledController {
            ip: ip.into(),
            num_leds: 180,
            strips,
            auto_layout: true,
        }
    }

    #[test]
    fn auto_strips_take_free_columns_and_keep_config_reverse() {
        let strip = |column, reverse| StripConfig {
            column,
            leds: 180,
            reverse,
        };
        let cs = vec![
            ctrl("a", vec![strip(0, false)]),
            ctrl("b", vec![strip(1, true)]),
        ];
        let out = |len| WledOutput {
            start: 0,
            len,
            rev: true,
            order: 0,
        };
        let (s, dropped) = auto_strips(&cs, "b", &[out(100), out(80), out(60)], Some(4));
        // first output keeps b's column and reverse; others take 2 and 3 (0 and 1 are taken)
        assert_eq!(
            s,
            vec![
                StripConfig {
                    column: 1,
                    leds: 100,
                    reverse: true
                },
                StripConfig {
                    column: 2,
                    leds: 80,
                    reverse: false
                },
                StripConfig {
                    column: 3,
                    leds: 60,
                    reverse: false
                },
            ]
        );
        assert_eq!(dropped, 0);
        let (s, dropped) = auto_strips(&cs, "b", &[out(100), out(80), out(60)], Some(3));
        assert_eq!(
            (s.len(), dropped),
            (2, 1),
            "no column left for the third output"
        );
    }
}
