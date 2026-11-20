//! Pure sensor-station logic, no hardware access, so it runs under `cargo test` on the host.
#![no_std]

#[cfg(test)]
extern crate std;

pub mod ld2410;
pub mod level;
