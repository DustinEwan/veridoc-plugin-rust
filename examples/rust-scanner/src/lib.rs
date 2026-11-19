//! The scanner example: one module, so the example shows the whole shape of a Rust project that
//! adopts veridoc. The library target is what the claim module's scenarios call.

pub mod config;

pub use config::{parse_config, ScannerConfig};
