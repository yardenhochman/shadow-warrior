//! Writes a sample stream (log text interleaved with one frame of each type) to stdout,
//! for checking `tools/station_monitor.py` against the Rust encoder (`just test` diffs its
//! JSON output against `tools/testdata/sample_stream.json`):
//! `cargo run -q -p station-proto --example sample_stream > s.bin && uv run tools/station_monitor.py --file s.bin`
use std::io::Write;

use station_proto::*;

fn main() {
    let msgs = [
        Msg::Hello(Hello {
            proto_version: PROTO_VERSION,
            station_id: 0x1234_abcd,
            fw_version: "0.1.0".try_into().unwrap(),
            radar_fw: Some("2.04.23022511".try_into().unwrap()),
        }),
        Msg::Radar(RadarEvent {
            present: true,
            moving_dist_cm: 150,
            static_dist_cm: 310,
            moving_energy: 55,
            static_energy: 9,
            state: 3,
        }),
        Msg::Audio(AudioLevel { rms_dbfs_l: -42.5, rms_dbfs_r: -60.25, peak_dbfs: -12.0, frames: 4800 }),
        Msg::Direction(Direction {
            frame: 70_000,
            lag_samples: -9.125,
            angle_deg: -30.0,
            confidence: 123.5,
            rms_dbfs: -27.75,
        }),
        Msg::AudioChunk(AudioChunk {
            rate_hz: 16000,
            channels: 2,
            frame: 480,
            clipped: 1,
            // L: 1000, -2000, 32767; R: -1, 0, -32768
            samples: [1000i16, -1, -2000, 0, 32767, -32768].iter().flat_map(|v| v.to_le_bytes()).collect(),
        }),
    ];
    let mut out = std::io::stdout().lock();
    let mut buf = [0u8; MAX_FRAME];
    out.write_all(b"I (100) station: boot\n").unwrap();
    for (seq, msg) in msgs.into_iter().enumerate() {
        let n = encode_frame(&Envelope { seq: seq as u16, msg }, &mut buf).unwrap();
        out.write_all(&buf[..n]).unwrap();
        out.write_all(b"I (200) station: a log line\n").unwrap();
    }
    // trailing frame so the last text chunk is delimited
    let n = encode_frame(
        &Envelope {
            seq: 5,
            msg: Msg::Audio(AudioLevel { rms_dbfs_l: -144.0, rms_dbfs_r: -144.0, peak_dbfs: -144.0, frames: 4800 }),
        },
        &mut buf,
    )
    .unwrap();
    out.write_all(&buf[..n]).unwrap();
}
