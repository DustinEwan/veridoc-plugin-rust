//! The scanner example's harness: one call into the claim library (`docs/decisions/0006:67-74`).
//!
//! veridoc builds this target itself — `cargo build --release --example veridoc` — and asks it what
//! the project's claim modules declare, so the project never runs it by hand.

#[path = "../src/config.doc.rs"]
mod config_doc;

fn main() -> std::process::ExitCode {
    veridoc_claims::harness(veridoc_claims::merge([config_doc::claims()]))
}
