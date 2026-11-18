//! The scanner example: one module, so the fixture shows the whole shape of a Rust project that
//! adopts veridoc. The library target is what the scenario harness calls.

pub mod config;

pub use config::{parse_config, ScannerConfig};
