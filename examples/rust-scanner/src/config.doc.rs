//! The claims about `src/config.rs`, and the scenario each one carries.
//!
//! A scenario is code in this file. It calls `parse_config` directly, so no action name and no
//! fixture registry stands between the claim and the call it makes (`docs/decisions/0006:57-74`),
//! and the two claims made against the same kind of world call the same function in this file
//! (`docs/decisions/0006:81-85`). The two directories it reads are project assets under `samples/`
//! that the scenario opens by path: a scenario's own code names them, and nothing declares them
//! (`docs/decisions/0006:47-49`).

use veridoc_claims::{Assert, Channel, Claim, Kind, Outcome, Scenario, Subject, Type};

/// The sample that holds no `config.json`, so `parse_config` refuses it.
const EMPTY: &str = "samples/empty-directory";
/// The sample that holds one, so `parse_config` accepts it.
const POPULATED: &str = "samples/populated-directory";

/// The two values the run claims declare, with the types their assertions are checked against.
const VALUES: &[(&str, Type)] = &[("exit_code", Type::Number), ("message", Type::String)];

/// The world both run claims share: `parse_config` against one sample directory, answered as the
/// exit code the scanner would exit with and the message it would print.
fn run(dir: &str) -> Result<Outcome, String> {
    let outcome = scanner::parse_config(std::path::Path::new(dir));
    Ok(Outcome::new()
        .value("exit_code", outcome.as_ref().map_or(2, |_| 0))
        .value(
            "message",
            outcome.map_or_else(|reason| reason, |_| String::new()),
        ))
}

/// `parse_config` against the directory that holds no configuration.
fn refusal() -> Result<Outcome, String> {
    run(EMPTY)
}

/// `parse_config` against the directory that holds one.
fn acceptance() -> Result<Outcome, String> {
    run(POPULATED)
}

/// The claims about `src/config.rs`.
pub fn claims() -> Vec<Claim> {
    vec![
        Claim {
            kind: Kind::Ensures,
            subject: Subject::symbol("parse_config", "src/config.rs"),
            concept: "project-scan",
            predicate: "rejects-missing-config",
            text: "parse_config refuses a directory that holds no config.json, naming the directory \
                   in the message.",
            channel: Channel::run(Assert::eq("exit_code", 2)),
            scenario: Some(Scenario {
                given: "the samples/empty-directory sample",
                when: "parse_config runs on it",
                returns: VALUES,
                run: refusal,
            }),
        },
        Claim {
            kind: Kind::Requires,
            subject: Subject::symbol("parse_config", "src/config.rs"),
            concept: "project-scan",
            predicate: "takes-one-directory",
            text: "parse_config is given exactly one directory and never searches for another.",
            channel: Channel::run(Assert::all([
                Assert::eq("exit_code", 0),
                Assert::eq("message", ""),
            ])),
            scenario: Some(Scenario {
                given: "the samples/populated-directory sample",
                when: "parse_config runs on it",
                returns: VALUES,
                run: acceptance,
            }),
        },
        Claim {
            kind: Kind::Invariant,
            subject: Subject::concept("project-scan"),
            concept: "project-scan",
            predicate: "names-what-it-refused",
            text: "Every refusal the scanner reports names the directory it refused.",
            channel: Channel::Witness {
                path: "evidence/refusals.txt",
                sha256: "979804f1e54922a74f73a46fe358a522cc7bdb27d82c4e71935b4836dc7d23e8",
            },
            scenario: None,
        },
    ]
}
