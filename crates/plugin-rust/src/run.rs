//! `run`: executing one scenario through the project's own harness binary.
//!
//! Rust binds a call target at link time, so this plugin cannot do what the Node plugin does and
//! `import()` the project's module inside its own process. The project instead ships a harness: a
//! small binary that decodes the request with its own types, calls the action, and prints the
//! declared observables as one JSON line. The plugin builds it once per connection, into
//! `.veridoc/target` so a scenario build never invalidates the author's own incremental build, and
//! spawns it per scenario. That keeps the plugin's failure surface finite -- the harness is
//! missing, does not build, answers nothing, or answers badly -- instead of making the plugin
//! compile generated glue against arbitrary Rust.
//!
//! The harness contract, in one line each way:
//!
//! ```text
//! stdin  {"scenarioId":"..","action":"..","args":[".."],"fixture":"<absolute dir>","deadlineMs":30000}
//! stdout {"observables":{"exit_code":2},"satisfied":{"<predicate source>":true}}
//! ```
//!
//! `satisfied` is optional and keyed by the predicate text the author wrote. A harness that exits
//! non-zero but has already printed its answer is still believed, because a refusal inside the
//! project's code is an observed value the assertion decides, not a run failure (`run.ts:2`).
//! Only a harness that answers nothing is a failure, and the kind separates the four causes the
//! profile names (`docs/schema/veridoc-1/run-failure-kind.json`).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use crate::project::{self, Project};

/// Where the harness is built, kept out of the project's own `target/`.
const HARNESS_TARGET_DIR: &str = ".veridoc/target";

/// How much of the harness's stderr is quoted back. A compile error is in the last lines.
const STDERR_TAIL: usize = 2000;

/// The `params` of `run`, as the engine sends them (`run-request.json`).
pub struct Request {
    pub scenario_id: String,
    pub fixture: String,
    pub action: String,
    pub args: Vec<String>,
    pub deadline_ms: u64,
    pub leaves: Vec<String>,
}

/// What the harness answered.
struct Answers {
    observables: HashMap<String, Value>,
    satisfied: HashMap<String, bool>,
}

/// The harness is built once per connection. The host starts one plugin process per run, so a
/// second scenario in the same run pays only the spawn, and a build failure stays failed for the
/// rest of the run because the author's fix needs a new process to take effect anyway.
static HARNESS: OnceLock<Result<PathBuf, String>> = OnceLock::new();

/// Answer one `run`. Every path returns the response shape: a run failure is a normal answer,
/// because the engine turns it into a red run and the reason the author reads.
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
    let action = project
        .tree
        .actions
        .iter()
        .find(|declared| declared.name == request.action)
        .ok_or_else(|| {
            (
                "plugin",
                format!(
                    "no action named {:?} is declared in {}",
                    request.action,
                    project::TREE_FILE
                ),
            )
        })?;
    if !project.tree.fixtures.iter().any(|f| f == &request.fixture) {
        return Err((
            "fixture",
            format!(
                "no fixture named {:?} is declared in {}",
                request.fixture,
                project::TREE_FILE
            ),
        ));
    }
    let fixture = project.root.join(&request.fixture);
    if !fixture.is_dir() {
        return Err((
            "fixture",
            format!("the fixture directory {} does not exist", fixture.display()),
        ));
    }
    let harness = project.tree.harness.as_deref().ok_or_else(|| {
        (
            "plugin",
            format!(
                "{} declares no harness, so the plugin cannot execute a scenario; add \
                 \"harness\": \"<bin name>\" and a src/bin/<bin name>.rs",
                project::TREE_FILE
            ),
        )
    })?;
    let binary = harness_binary(&project.root, harness)?;
    let answers = spawn(&binary, request, &fixture)?;

    let mut observed = Vec::new();
    for declared in &project.tree.observables {
        match answers.observables.get(&declared.name) {
            Some(value) => observed.push(json!({
                "observable": declared.name,
                "value": value.to_string(),
            })),
            None if action.produces.contains(&declared.name) => {
                return Err((
                    "plugin",
                    format!(
                        "the harness {harness} answered no value for {}, which action {} declares \
                         it produces",
                        declared.name, action.name
                    ),
                ))
            }
            None => observed.push(json!({
                "observable": declared.name,
                "value": "null",
            })),
        }
    }

    let mut satisfied = Vec::new();
    for leaf_id in &request.leaves {
        let (claim_part, index) = split_leaf(leaf_id);
        let owner = project
            .scenario(&request.scenario_id)
            .into_iter()
            .find(|claim| project.claim_name(claim) == claim_part)
            .ok_or_else(|| {
                (
                    "plugin",
                    format!(
                        "leaf {leaf_id} names claim {claim_part}, which declares no scenario \
                         {:?} in this project",
                        request.scenario_id
                    ),
                )
            })?;
        let tree = owner.channel.asserts.as_ref().ok_or_else(|| {
            (
                "plugin",
                format!("claim {claim_part} declares no assertion tree to answer from"),
            )
        })?;
        let Some(check) = leaves(tree).into_iter().nth(index) else {
            return Err((
                "plugin",
                format!("leaf {leaf_id} is past the end of claim {claim_part}'s assertion tree"),
            ));
        };
        // The engine decides `equals` and `contains` itself from the observed values
        // (`evaluate` in `crates/veridoc-engine/src/execute.rs`); only a `satisfies` leaf needs the
        // plugin's answer.
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
                    format!("the harness {harness} implements no predicate {source:?}"),
                ))
            }
        }
    }

    Ok(json!({ "ok": true, "observed": observed, "satisfied": satisfied }))
}

/// Build the harness, or return the reason it cannot be built. The kind is `build` for every
/// failure here, including a missing `Cargo.toml`, because the cause is always the project's
/// build and the detail quotes cargo.
fn harness_binary(root: &Path, harness: &str) -> Result<PathBuf, (&'static str, String)> {
    HARNESS
        .get_or_init(|| {
            let target = root.join(HARNESS_TARGET_DIR);
            let output = Command::new("cargo")
                .arg("build")
                .arg("--release")
                .arg("--manifest-path")
                .arg(root.join("Cargo.toml"))
                .arg("--bin")
                .arg(harness)
                .arg("--target-dir")
                .arg(&target)
                .output()
                .map_err(|e| format!("cargo could not be started to build {harness}: {e}"))?;
            if !output.status.success() {
                return Err(format!(
                    "the harness {harness} does not build: {}",
                    tail(&output.stderr)
                ));
            }
            let binary = target.join("release").join(harness);
            if !binary.is_file() {
                return Err(format!(
                    "cargo reported success but {} is not there",
                    binary.display()
                ));
            }
            Ok(binary)
        })
        .clone()
        .map_err(|detail| ("build", detail))
}

/// Run the harness under the scenario's deadline. The deadline covers the scenario, not the build:
/// a cold build of a real crate is longer than any scenario deadline, which is why the build is
/// done once and outside it.
fn spawn(
    binary: &Path,
    request: &Request,
    fixture: &Path,
) -> Result<Answers, (&'static str, String)> {
    let mut child = Command::new(binary)
        .arg(fixture)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            (
                "plugin",
                format!("the harness {} could not be started: {e}", binary.display()),
            )
        })?;
    let payload = json!({
        "scenarioId": request.scenario_id,
        "action": request.action,
        "args": request.args,
        "fixture": fixture,
        "deadlineMs": request.deadline_ms,
    });
    let mut stdin = child.stdin.take().ok_or((
        "plugin",
        "the harness has no stdin to write the request to".to_string(),
    ))?;
    stdin
        .write_all(payload.to_string().as_bytes())
        .map_err(|e| {
            (
                "plugin",
                format!("the harness request could not be written: {e}"),
            )
        })?;
    drop(stdin);

    let mut stdout = child.stdout.take().ok_or((
        "plugin",
        "the harness has no stdout to read the answer from".to_string(),
    ))?;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        let _ = sender.send(bytes);
    });
    let mut stderr = child
        .stderr
        .take()
        .ok_or(("plugin", "the harness has no stderr".to_string()))?;
    let collected = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&collected);
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.read_to_end(&mut bytes);
        if let Ok(mut held) = sink.lock() {
            *held = bytes;
        }
    });

    let waited = Duration::from_millis(request.deadline_ms.max(1));
    let output = match receiver.recv_timeout(waited) {
        Ok(output) => output,
        Err(_) => {
            // Killing the child rather than the plugin is the point of running the scenario out
            // of process: the deadline is a run failure the engine reports, not a lost plugin.
            let _ = child.kill();
            let _ = child.wait();
            return Err((
                "deadline",
                format!(
                    "the harness answered nothing within {} ms",
                    request.deadline_ms
                ),
            ));
        }
    };
    let status = child
        .wait()
        .map_err(|e| ("plugin", format!("waiting for the harness failed: {e}")))?;
    let stderr = collected
        .lock()
        .map(|bytes| tail(&bytes))
        .unwrap_or_else(|_| String::from("its stderr could not be read"));

    let answer = String::from_utf8_lossy(&output)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_string();
    if answer.is_empty() {
        return Err((
            "plugin",
            if status.success() {
                format!(
                    "the harness {harness} answered no JSON line",
                    harness = harness_name(binary)
                )
            } else {
                format!(
                    "the harness {harness} exited {status} without answering: {stderr}",
                    harness = harness_name(binary)
                )
            },
        ));
    }
    let value: Value = serde_json::from_str(&answer)
        .map_err(|e| ("plugin", format!("the harness answer is not JSON: {e}")))?;
    Ok(Answers {
        observables: value
            .get("observables")
            .and_then(Value::as_object)
            .map(|entries| {
                entries
                    .iter()
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect()
            })
            .unwrap_or_default(),
        satisfied: value
            .get("satisfied")
            .and_then(Value::as_object)
            .map(|entries| {
                entries
                    .iter()
                    .map(|(name, value)| {
                        (
                            name.clone(),
                            value
                                .as_bool()
                                .unwrap_or_else(|| value.as_str() == Some("true")),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// The assertion tree's leaves in preorder, each as its `is` object, which is the order the engine
/// numbers them in (`evaluate` in `crates/veridoc-engine/src/execute.rs`, and `run.ts:26-33` for the
/// same walk).
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
    if let Some(inner) = node.get("not") {
        walk(inner, found);
        return;
    }
    if let Some(check) = node.get("is") {
        found.push(check);
    }
}

/// `<claim name>#l<n>` into its two parts, split at the last `#l` because a claim name carries a
/// `#` of its own. A leaf id with no marker is treated as leaf 0 of the whole id, which is what the
/// engine sends when a claim has one leaf.
fn split_leaf(leaf_id: &str) -> (&str, usize) {
    match leaf_id.rsplit_once("#l") {
        Some((claim, index)) => (claim, index.parse().unwrap_or(0)),
        None => (leaf_id, 0),
    }
}

fn harness_name(binary: &Path) -> String {
    binary
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("the harness")
        .to_string()
}

/// The last bytes of the harness's stderr, which is where cargo puts the reason.
fn tail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim();
    if trimmed.len() <= STDERR_TAIL {
        return trimmed.to_string();
    }
    let start = trimmed.len() - STDERR_TAIL;
    let start = trimmed
        .char_indices()
        .take_while(|(index, _)| *index <= start)
        .map(|(index, _)| index)
        .last()
        .unwrap_or(0);
    format!("...{}", &trimmed[start..])
}

#[cfg(test)]
mod tests {
    use super::{leaves, split_leaf};
    use serde_json::json;

    #[test]
    fn leaves_are_numbered_in_preorder() {
        // The engine numbers leaves by walking `all`, then `any`, then `not`, then the leaf
        // itself (`evaluate` in `crates/veridoc-engine/src/execute.rs`). A different order answers
        // the wrong leaf.
        let tree = json!({ "all": [
            { "obs": "exit_code", "is": { "equals": 2 } },
            { "any": [
                { "obs": "message", "is": { "contains": "config.json" } },
                { "not": { "obs": "exit_code", "is": { "equals": 0 } } },
            ] },
        ] });
        let checks = leaves(&tree);
        assert_eq!(checks.len(), 3, "{checks:?}");
        assert_eq!(checks[1], &json!({ "contains": "config.json" }));
        assert_eq!(checks[2], &json!({ "equals": 0 }));
    }

    #[test]
    fn a_leaf_id_splits_into_its_claim_and_its_index() {
        assert_eq!(
            split_leaf("probe#rejects-missing-project-root#l2"),
            ("probe#rejects-missing-project-root", 2)
        );
        assert_eq!(
            split_leaf("probe#rejects-missing-project-root"),
            ("probe#rejects-missing-project-root", 0)
        );
    }
}
