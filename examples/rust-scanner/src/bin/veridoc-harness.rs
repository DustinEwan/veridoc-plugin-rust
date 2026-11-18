//! The scenario harness veridoc builds and runs for each scenario.
//!
//! The plugin cannot call `parse_config` itself: Rust binds a call target at link time, so the
//! project supplies the one place where the request's JSON becomes the project's own types. This
//! file is the whole per-project cost of adopting veridoc in Rust, and it is written in the
//! project, where the argument types are already known.
//!
//! Contract, one line each way:
//!
//! ```text
//! stdin  {"scenarioId":"..","action":"..","args":[".."],"fixture":"<absolute dir>","deadlineMs":30000}
//! stdout {"observables":{"exit_code":2,"message":".."}}
//! ```
//!
//! The fixture directory also arrives as `argv[1]`. An action the harness does not implement is a
//! hard error rather than a zero, because a scenario that silently reports the wrong exit code
//! would mark a claim supported.

use std::io::Read;

use scanner::parse_config;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let request: serde_json::Value = serde_json::from_str(&input)?;
    let action = request["action"].as_str().unwrap_or_default();
    let fixture = request["fixture"]
        .as_str()
        .map(str::to_string)
        .or_else(|| std::env::args().nth(1))
        .unwrap_or_default();

    let observables = match action {
        "run-scanner" => {
            let outcome = parse_config(std::path::Path::new(&fixture));
            serde_json::json!({
                "exit_code": outcome.as_ref().map_or(2, |_| 0),
                "message": outcome.map_or_else(|reason| reason, |_| String::new()),
            })
        }
        other => return Err(format!("the harness implements no action {other:?}").into()),
    };
    println!("{}", serde_json::json!({ "observables": observables }));
    Ok(())
}
