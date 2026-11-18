//! Where each LED strip hangs in the arena. The punching bag is in the centre, so the middle
//! columns are closest to it and the outer ones furthest (punch shockwaves travel from the
//! inner columns outwards).
use crate::config::WledController;

/// One strip (column) of the arena as seen by a scene.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColumnLayout {
    /// Column number, 0-based, left to right.
    pub column: usize,
    pub columns_total: usize,
    /// 0 = nearest the bag, 1 = furthest: `|c - (N-1)/2| / ((N-1)/2)`. With an even column
    /// count no column is exactly 0 (the two middle ones tie).
    pub bag_distance: f32,
    pub leds: usize,
    /// The strip is wired top-down: its LED 0 is the top, so rendered frames are flipped.
    pub reverse: bool,
}

impl ColumnLayout {
    pub fn new(column: usize, columns_total: usize, leds: usize, reverse: bool) -> Self {
        let half = (columns_total.max(1) as f32 - 1.0) / 2.0;
        let bag_distance = if half > 0.0 {
            ((column as f32 - half).abs() / half).min(1.0)
        } else {
            0.0
        };
        ColumnLayout {
            column,
            columns_total,
            bag_distance,
            leds,
            reverse,
        }
    }

    /// A lone strip (tests, single-strip callers).
    pub fn single(leds: usize) -> Self {
        Self::new(0, 1, leds, false)
    }
}

/// The strips of controller number `index`, in the order they sit in its LED range.
pub fn controller_strips(
    c: &WledController,
    index: usize,
    columns_total: usize,
) -> Vec<ColumnLayout> {
    c.strip_specs(index)
        .into_iter()
        .map(|s| ColumnLayout::new(s.column, columns_total, s.leds, s.reverse))
        .collect()
}

/// Append `strip_rgb` (r,g,b per LED, top = last LED) to a controller frame, flipped when
/// the strip is wired `reverse`.
pub fn append_strip(frame: &mut Vec<u8>, strip_rgb: &[u8], reverse: bool) {
    if reverse {
        frame.extend(strip_rgb.chunks_exact(3).rev().flatten());
    } else {
        frame.extend_from_slice(strip_rgb);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::StripConfig;

    fn dists(n: usize) -> Vec<f32> {
        (0..n)
            .map(|c| ColumnLayout::new(c, n, 180, false).bag_distance)
            .collect()
    }

    #[test]
    fn five_columns_middle_is_at_the_bag() {
        let d = dists(5);
        assert_eq!(d, vec![1.0, 0.5, 0.0, 0.5, 1.0]);
    }

    #[test]
    fn four_columns_inner_pair_ties_and_is_closer_than_outer() {
        let d = dists(4);
        assert_eq!(d[1], d[2]);
        assert_eq!(d[0], d[3]);
        assert!(d[1] < d[0]);
        assert!((d[0] - 1.0).abs() < 1e-6 && (d[1] - 1.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn one_column_is_at_the_bag() {
        assert_eq!(ColumnLayout::new(0, 1, 10, false).bag_distance, 0.0);
    }

    #[test]
    fn controller_strips_follow_config_order() {
        let c = WledController {
            ip: "x".into(),
            num_leds: 120,
            strips: vec![
                StripConfig {
                    column: 3,
                    leds: 60,
                    reverse: false,
                },
                StripConfig {
                    column: 2,
                    leds: 60,
                    reverse: true,
                },
            ],
        };
        let s = controller_strips(&c, 0, 4);
        assert_eq!((s[0].column, s[0].reverse), (3, false));
        assert_eq!((s[1].column, s[1].reverse), (2, true));
        let single = WledController {
            ip: "y".into(),
            num_leds: 180,
            strips: vec![],
        };
        let s = controller_strips(&single, 2, 4);
        assert_eq!((s.len(), s[0].column, s[0].leds), (1, 2, 180));
    }

    #[test]
    fn append_strip_flips_reversed() {
        let mut f = vec![9];
        append_strip(&mut f, &[1, 2, 3, 4, 5, 6], true);
        assert_eq!(f, vec![9, 4, 5, 6, 1, 2, 3]);
    }
}
