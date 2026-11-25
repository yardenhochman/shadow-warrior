//! Offline shout detector tuning on recordings: see brain-rs/docs/SOUND.md ("Tuning on recordings").
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = shadow_warrior_brain::services::shout_replay::cli(&args) {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
