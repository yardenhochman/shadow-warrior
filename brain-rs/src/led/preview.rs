//! Test-only helper: dump a run as a "filmstrip" image (one row per frame, one column per LED)
//! so a look can be judged without a strip. Written as a binary PPM; convert with
//! `ffmpeg -i x.ppm x.png`.
use std::io::Write;
use std::path::Path;

/// `frames` are `len * 3` RGB bytes each. Each LED becomes `sx` pixels wide, each frame `sy` tall.
pub fn write_ppm(path: &Path, frames: &[Vec<u8>], sx: usize, sy: usize) -> std::io::Result<()> {
    let leds = frames.first().map(|f| f.len() / 3).unwrap_or(0);
    let mut f = std::fs::File::create(path)?;
    write!(f, "P6\n{} {}\n255\n", leds * sx, frames.len() * sy)?;
    let mut row = Vec::with_capacity(leds * sx * 3);
    for frame in frames {
        row.clear();
        for led in 0..leds {
            for _ in 0..sx {
                row.extend_from_slice(&frame[led * 3..led * 3 + 3]);
            }
        }
        for _ in 0..sy {
            f.write_all(&row)?;
        }
    }
    Ok(())
}
