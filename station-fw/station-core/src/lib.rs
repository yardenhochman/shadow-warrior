//! Pure sensor-station logic, no hardware access, so it runs under `cargo test` on the host.
#![no_std]

extern crate alloc;

#[cfg(test)]
extern crate std;

pub mod decimate;
pub mod doa;
pub mod fft;
pub mod ld2410;
pub mod level;
pub mod selftest;
