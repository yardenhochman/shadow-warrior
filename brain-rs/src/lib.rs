//! Library target: lets auxiliary binaries (`src/bin/*`, e.g. `shout_replay`) reuse the brain's
//! modules. `main.rs` still declares the same modules itself, so the main binary is unchanged.
//! Unit tests run from the main binary only (`test = false` in Cargo.toml).
#![allow(dead_code)]

pub mod api;
pub mod config;
pub mod core;
pub mod led;
pub mod models;
pub mod services;
#[cfg(test)]
mod test_util;
