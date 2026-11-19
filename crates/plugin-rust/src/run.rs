//! `run` and `collect`'s claim list: talking to the project's own harness binary.
//!
//! Rust binds a call target at link time, so this plugin cannot do what a plugin for a hosted
//! language does and load the project's module inside its own process. The project instead ships one
//! harness target — `examples/veridoc.rs`, one call into the `veridoc-claims` library — which knows
//! the claims of every claim module it links. The plugin builds it into `.veridoc/target` so a
//! scenario build never invalidates the author's own incremental build, and asks it two questions
//! (`docs/decisions/0006:57-74`):
//!
//! ```text
//! <harness> --list              {"libraryVersion":"0.1.0","claims":[{..},..]}
//! <harness> --run <claim name>  {"values":{"exit_code":2},"satisfied":{"<predicate>":true}}
//! ```
//!
//! `--list` is what `collect` reports, so the claim set is whatever the compiled claim modules say it
//! is rather than a copy of them in a data file. `--run` answers one claim by the name `--list`
//! reported: the scenario has no identity of its own, so the claim is the address
//! (`docs/decisions/0006:50-55`).
//!
//! The failure surface stays finite — the harness does not build, answers nothing, or answers badly —
//! because the plugin never compiles generated glue against arbitrary Rust. A harness that exits
//! non-zero but has already printed its answer is still believed, because a refusal inside the
//! project's code is an observed value the assertion decides, not a run failure. Only a harness that
//! answers nothing is a failure, and the kind separates the causes the profile names
//! (`docs/schema/veridoc-1/run-failure-kind.json`).

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::project::{self, Project};

/// Where the harness is built, kept out of the project's own `target/`.
const HARNESS_TARGET_DIR: &str = ".veridoc/target";

/// The harness target a Rust project ships: `examples/veridoc.rs`, built with the project's
/// test-only dependencies available, which is what a claim module needs to call the project's code.
/// Where the target sits is this plugin's convention rather than something the author restates
/// (`docs/decisions/0006:168-173`).
pub const HARNESS_EXAMPLE: &str = "veridoc";

/// How much of the harness's stderr is quoted back. A compile error is in the last lines.
const STDERR_TAIL: usize = 2000;

/// The time the plugin waits for `--list`. It is not a scenario deadline: it covers a build the
/// author has already paid for plus one process start, and a project that takes longer than this to
/// start is reported rather than waited on forever.
const LIST_TIMEOUT: Duration = Duration::from_secs(120);

/// The `params` of `run`, as the engine sends them (`run-request.json`). The claim is addressed by
/// name, because the scenario belongs to the claim.
pub struct Request {
    pub claim: String,
    pub timeout_ms: u64,
}

/// What the harness answered for one claim.
#[derive(Debug)]
struct Answers {
    values: HashMap<String, Value>,
    satisfied: HashMap<String, bool>,
}

/// The harness is built once per connection. The host starts one plugin process per run, so a second
/// claim in the same run pays only the spawn, and a build failure stays failed for the rest of the run
/// because the author's fix needs a new process to take effect anyway.
static HARNESS: OnceLock<Result<PathBuf, String>> = OnceLock::new();

/// The claims the project's harness answers for, as `collect` needs them. A project whose claim
/// modules do not compile stops collection with the compiler's own reason, which is the price
/// `docs/decisions/0006:145` accepts for a sidecar that is executable code.
pub fn listed_claims(root: &Path) -> Result<Vec<project::ListedClaim>, String> {
    let binary = harness_binary(root)?;
    let answered = spawn(&binary, root, &["--list".to_string()], LIST_TIMEOUT)
        .map_err(|(_, detail)| detail)?;
    let parsed: Value = serde_json::from_str(answered.trim())
        .map_err(|e| format!("the harness answered --list with text that is not JSON: {e}"))?;
    let library = parsed["libraryVersion"].as_str().unwrap_or_default();
    if library != env!("CARGO_PKG_VERSION") {
        return Err(format!(
            "the project's harness was built against veridoc-claims {library}, which this plugin \
             (version {}) does not speak; update the project's dev-dependency",
            env!("CARGO_PKG_VERSION")
        ));
    }
    let claims = parsed["claims"]
        .as_array()
        .ok_or_else(|| "the harness answered --list without a claims array".to_string())?;
    serde_json::from_value(Value::Array(claims.clone())).map_err(|e| {
        format!("the harness answered --list with a claim this plugin cannot read: {e}")
    })
}

/// Answer one `run`. Every path returns the response shape: a run failure is a normal answer,
/// because the engine turns it into one claim's red run and the reason the author reads, while every
/// other claim keeps its verdict (`docs/decisions/0006:100-106`).
pub fn run(project: &Project, request: &Request) -> Value {
    match execute(project, request) {
        Ok(answer) => answer,
        Err((kind, detail)) => json!({
            "ok": false,
            "observed": [],
            "satisfied": [],
            "failure": { "kind": kind, "detail": detail },
        }),
    }
}

fn execute(project: &Project, request: &Request) -> Result<Value, (&'static str, String)> {
    let owner = project.claim(&request.claim).ok_or_else(|| {
        (
            "plugin",
            format!("this project declares no claim named {}", request.claim),
        )
    })?;
    let scenario = project.scenario(&request.claim).ok_or_else(|| {
        (
            "plugin",
            format!("claim {} declares no scenario", request.claim),
        )
    })?;
    let tree = owner.channel.asserts.as_ref().ok_or_else(|| {
        (
            "plugin",
            format!(
                "claim {} declares no assertion tree to answer from",
                request.claim
            ),
        )
    })?;
    let binary = harness_binary(&project.root).map_err(|reason| ("build", reason))?;
    let answered = spawn(
        &binary,
        &project.root,
        &["--run".to_string(), request.claim.clone()],
        Duration::from_millis(request.timeout_ms.max(1)),
    )?;
    let answers = read_answers(&answered, &harness_name(&binary))?;

    // The engine decides `equals` and `contains` itself from the observed values (`evaluate` in
    // `crates/veridoc-engine/src/execute.rs`); only a `satisfies` leaf needs the plugin's answer.
    let mut observed = Vec::new();
    for declared in &scenario.values {
        match answers.values.get(&declared.name) {
            Some(value) => observed.push(json!({
                "observable": declared.name,
                "value": value.to_string(),
            })),
            None => {
                return Err((
                    "plugin",
                    format!(
                        "the harness {} answered no value for {}, which claim {} declares it \
                         returns",
                        harness_name(&binary),
                        declared.name,
                        request.claim
                    ),
                ))
            }
        }
    }

    let mut satisfied = Vec::new();
    for (index, check) in leaves(tree).into_iter().enumerate() {
        let leaf_id = format!("{}#l{index}", request.claim);
        let Some(spec) = check.get("satisfies") else {
            satisfied.push(json!({ "leafId": leaf_id, "ok": true }));
            continue;
        };
        let source = spec
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match answers.satisfied.get(source) {
            Some(ok) => satisfied.push(json!({ "leafId": leaf_id, "ok": *ok })),
            None => {
                return Err((
                    "plugin",
                    format!(
                        "the harness {} answers no predicate {source:?} for claim {}",
                        harness_name(&binary),
                        request.claim
                    ),
                ))
            }
        }
    }

    Ok(json!({ "ok": true, "observed": observed, "satisfied": satisfied }))
}

/// The harness answer for one claim: the declared values, and the predicate answers when the claim's
/// assertion tree asks the plugin to settle one. A harness that prints `{"error":..}` and exits
/// non-zero has run the scenario and been refused by the project's own code path, which is a run
/// failure naming the claim rather than a verdict.
fn read_answers(answered: &str, harness: &str) -> Result<Answers, (&'static str, String)> {
    let parsed: Value = serde_json::from_str(answered.trim()).map_err(|_| {
        (
            "plugin",
            format!("the harness {harness} answered a scenario with text that is not JSON"),
        )
    })?;
    if let Some(error) = parsed.get("error").and_then(Value::as_str) {
        return Err(("plugin", error.to_string()));
    }
    let values = match parsed.get("values") {
        Some(Value::Object(entries)) => entries
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect(),
        _ => {
            return Err((
                "plugin",
                format!("the harness {harness} answered a scenario with no values object"),
            ))
        }
    };
    let satisfied = match parsed.get("satisfied") {
        Some(Value::Object(entries)) => entries
            .iter()
            .filter_map(|(name, value)| value.as_bool().map(|ok| (name.clone(), ok)))
            .collect(),
        _ => HashMap::new(),
    };
    Ok(Answers { values, satisfied })
}

/// Build the project's harness target, or return the reason it cannot be built. The reason is a
/// `build` failure for every cause here, including a missing `Cargo.toml`, because the cause is
/// always the project's build and the detail quotes cargo.
fn harness_binary(root: &Path) -> Result<PathBuf, String> {
    HARNESS.get_or_init(|| build_harness(root)).clone()
}

fn build_harness(root: &Path) -> Result<PathBuf, String> {
    if !root.join("Cargo.toml").is_file() {
        return Err(format!(
            "{} holds no Cargo.toml, so the plugin cannot build the project's claim modules",
            root.display()
        ));
    }
    let example = root.join("examples").join(format!("{HARNESS_EXAMPLE}.rs"));
    if !example.is_file() {
        return Err(format!(
            "the project ships no {}, so the plugin cannot ask it what its claim modules declare; \
             the harness is one call into the veridoc-claims library",
            project::relative(root, &example)
        ));
    }
    let built = Command::new("cargo")
        .arg("build")
        .arg("--release")
        .arg("--offline")
        .arg("--example")
        .arg(HARNESS_EXAMPLE)
        .arg("--target-dir")
        .arg(HARNESS_TARGET_DIR)
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    let output = match built {
        Ok(output) => output,
        Err(e) => {
            return Err(format!(
                "cargo could not be started to build the project's claim modules: {e}"
            ))
        }
    };
    if !output.status.success() {
        return Err(format!(
            "the project's claim modules do not build: {}",
            tail(&output.stderr)
        ));
    }
    let binary = root
        .join(HARNESS_TARGET_DIR)
        .join("release")
        .join("examples")
        .join(HARNESS_EXAMPLE);
    if !binary.is_file() {
        return Err(format!(
            "cargo built the {} example but left no {} where the plugin looks for it",
            HARNESS_EXAMPLE,
            project::relative(root, &binary)
        ));
    }
    Ok(binary)
}

/// Run the harness under a deadline, from the project root, so a scenario that opens an asset of the
/// project finds it by the path the author wrote. The deadline covers the scenario, not the build: a
/// cold build of a real crate is longer than any scenario deadline, which is why the build is done
/// once and outside it.
fn spawn(
    binary: &Path,
    root: &Path,
    args: &[String],
    deadline: Duration,
) -> Result<String, (&'static str, String)> {
    let mut child = Command::new(binary)
        .args(args)
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            (
                "plugin",
                format!(
                    "the harness {} could not be started: {e}",
                    harness_name(binary)
                ),
            )
        })?;
    let mut stdout = child.stdout.take().expect("the harness's stdout is piped");
    let mut stderr = child.stderr.take().expect("the harness's stderr is piped");
    let collected = Arc::new(Mutex::new(String::new()));
    let err_collected = Arc::clone(&collected);
    let reader = thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        *err_collected
            .lock()
            .expect("the harness reader is the only holder") = text;
    });
    let err_sink = Arc::new(Mutex::new(Vec::new()));
    let err_reader = {
        let sink = Arc::clone(&err_sink);
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stderr.read_to_end(&mut bytes);
            *sink
                .lock()
                .expect("the harness error reader is the only holder") = bytes;
        })
    };

    // Poll rather than block on a thread: the deadline has to be able to kill the harness, and a
    // thread that owns `child` in order to wait on it cannot hand it back to be killed.
    let until = Instant::now() + deadline;
    let mut status = None;
    while Instant::now() < until {
        match child.try_wait() {
            Ok(Some(exit)) => {
                status = Some(exit);
                break;
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(e) => {
                return Err((
                    "plugin",
                    format!(
                        "the harness {} could not be waited on: {e}",
                        harness_name(binary)
                    ),
                ))
            }
        }
    }
    if status.is_none() {
        let _ = child.kill();
        let _ = child.wait();
        let _ = reader.join();
        let _ = err_reader.join();
        return Err((
            "deadline",
            format!(
                "the scenario outlived its {} ms deadline",
                deadline.as_millis()
            ),
        ));
    }
    let _ = reader.join();
    let _ = err_reader.join();
    let answered = Arc::try_unwrap(collected)
        .expect("the harness readers have been joined")
        .into_inner()
        .expect("the harness answer was never poisoned");
    if answered.trim().is_empty() {
        let bytes = Arc::try_unwrap(err_sink)
            .expect("the harness error reader has been joined")
            .into_inner()
            .expect("the harness error was never poisoned");
        return Err((
            "plugin",
            format!(
                "the harness {} answered nothing: {}",
                harness_name(binary),
                String::from_utf8_lossy(&bytes)
            ),
        ));
    }
    Ok(answered)
}

/// The assertion tree's leaves in preorder, each as its `is` object, which is the order the engine
/// numbers them in (`evaluate` in `crates/veridoc-engine/src/execute.rs`).
pub fn leaves(tree: &Value) -> Vec<&Value> {
    let mut found = Vec::new();
    walk(tree, &mut found);
    found
}

fn walk<'a>(node: &'a Value, found: &mut Vec<&'a Value>) {
    if let Some(children) = node.get("all").and_then(Value::as_array) {
        for child in children {
            walk(child, found);
        }
        return;
    }
    if let Some(children) = node.get("any").and_then(Value::as_array) {
        for child in children {
            walk(child, found);
        }
        return;
    }
    if let Some(child) = node.get("not") {
        walk(child, found);
        return;
    }
    if let Some(check) = node.get("is") {
        found.push(check);
    }
}

fn harness_name(binary: &Path) -> String {
    binary
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("harness")
        .to_string()
}

/// The last bytes of the harness's stderr, which is where cargo puts the reason.
fn tail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    if text.len() <= STDERR_TAIL {
        return text.trim().to_string();
    }
    let cut = text.len() - STDERR_TAIL;
    let start = text[cut..]
        .char_indices()
        .next()
        .map(|(at, _)| cut + at)
        .unwrap_or(cut);
    format!("..{}", text[start..].trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_leaves_are_numbered_in_the_order_the_engine_numbers_them() {
        let tree = json!({
            "all": [
                { "obs": "exit_code", "is": { "equals": 2 } },
                { "not": { "obs": "stderr", "is": { "contains": "panic" } } },
                { "any": [
                    { "obs": "stderr", "is": { "satisfies": { "source": "names the directory" } } }
                ] }
            ]
        });
        let found = leaves(&tree);
        assert_eq!(found.len(), 3);
        assert!(found[0].get("equals").is_some());
        assert!(found[1].get("contains").is_some());
        assert!(found[2]
            .get("satisfies")
            .and_then(|s| s.get("source"))
            .is_some());
    }

    #[test]
    fn a_harness_answer_of_values_is_believed_even_when_the_project_refused() {
        let answers = read_answers(
            "{\"values\":{\"exit_code\":2},\"satisfied\":{\"names the directory\":true}}",
            "veridoc",
        )
        .unwrap();
        assert_eq!(answers.values.get("exit_code").unwrap(), &json!(2));
        assert!(answers.satisfied.get("names the directory").unwrap());
    }

    #[test]
    fn a_harness_that_reports_an_error_is_one_claims_run_failure() {
        let (kind, detail) =
            read_answers("{\"error\":\"probe refused: no config.json\"}", "veridoc").unwrap_err();
        assert_eq!(kind, "plugin");
        assert!(detail.contains("refused"), "{detail}");
    }

    #[test]
    fn a_harness_that_answers_no_values_is_a_failure_not_an_empty_run() {
        let (kind, detail) = read_answers("{\"satisfied\":{}}", "veridoc").unwrap_err();
        assert_eq!(kind, "plugin");
        assert!(detail.contains("no values object"), "{detail}");
    }
}
