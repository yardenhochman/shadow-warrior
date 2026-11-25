//! `cargo run -q -p station-proto --example seal -- <passphrase> <challenge-hex> <ssid> <pass> <brain>` prints the sealed
//! BLE config as hex (test helper for bench provisioning from a script).
use station_proto::provision::{derive_key, seal, Config, CHALLENGE_LEN};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let key = derive_key(&a[0]);
    let mut ch = [0u8; CHALLENGE_LEN];
    for (i, b) in ch.iter_mut().enumerate() {
        *b = u8::from_str_radix(&a[1][2 * i..2 * i + 2], 16).expect("challenge hex");
    }
    let cfg = Config { ssid: a[2].as_str().try_into().unwrap(), pass: a[3].as_str().try_into().unwrap(), brain: a[4].as_str().try_into().unwrap() };
    let sealed = seal(&key, &ch, &cfg).expect("seal");
    println!("{}", sealed.iter().map(|b| format!("{b:02x}")).collect::<String>());
}
