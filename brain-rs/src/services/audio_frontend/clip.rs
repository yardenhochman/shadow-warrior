//! Clip recorder: WAV + per-frame feature JSON dumps of recent audio.
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::{FrameFeatures, FRAME_SIZE, TARGET_RATE};

#[derive(Serialize)]
struct ClipMeta<'a> {
    reason: &'a str,
    sample_rate: u32,
    samples: usize,
    frame_size: usize,
    frames: &'a [FrameFeatures],
    /// Raw (pre band-pass) WAV next to this clip, if written.
    raw_file: Option<String>,
    raw_sample_rate: Option<u32>,
}

/// Files written for one clip.
#[derive(Debug)]
pub struct ClipOutput {
    pub wav: PathBuf,
    pub raw: Option<PathBuf>,
}

/// Writes `<dir>/<stamp>_<reason>.wav` (band-passed, 16 kHz), a matching `.json` of per-frame
/// features and optionally `<stamp>_<reason>_raw.wav` (pre-filter, device rate).
pub struct ClipRecorder {
    dir: PathBuf,
}

impl ClipRecorder {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn record(
        &self,
        reason: &str,
        stamp: &str,
        audio: &[f32],
        features: &[FrameFeatures],
        raw: Option<(&[f32], u32)>,
    ) -> std::io::Result<ClipOutput> {
        std::fs::create_dir_all(&self.dir)?;
        let safe: String =
            reason.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
        let base = format!("{stamp}_{safe}");
        let wav_path = self.dir.join(format!("{base}.wav"));
        write_wav_16(&wav_path, audio, TARGET_RATE)?;
        let raw_path = match raw {
            Some((samples, rate)) => {
                let p = self.dir.join(format!("{base}_raw.wav"));
                write_wav_16(&p, samples, rate)?;
                Some(p)
            }
            None => None,
        };
        let meta = ClipMeta {
            reason,
            sample_rate: TARGET_RATE,
            samples: audio.len(),
            frame_size: FRAME_SIZE,
            frames: features,
            raw_file: raw_path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()),
            raw_sample_rate: raw.map(|(_, r)| r),
        };
        let json = serde_json::to_vec_pretty(&meta).map_err(std::io::Error::other)?;
        std::fs::write(self.dir.join(format!("{base}.json")), json)?;
        Ok(ClipOutput { wav: wav_path, raw: raw_path })
    }
}

/// Minimal mono 16-bit PCM WAV writer.
pub fn write_wav_16(path: &Path, samples: &[f32], rate: u32) -> std::io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?; // PCM
    f.write_all(&1u16.to_le_bytes())?; // mono
    f.write_all(&rate.to_le_bytes())?;
    f.write_all(&(rate * 2).to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for &s in samples {
        f.write_all(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())?;
    }
    f.flush()
}
