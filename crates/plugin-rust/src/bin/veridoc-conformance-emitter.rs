//! The conformance emitter: `veridoc-conformance-emitter --out <dir>` renders a whole Rust
//! project that exercises every `veridoc/1` method this plugin answers, writes the
//! `veridoc-conformance.json` that names its parts, and prints the directory it rendered.
//!
//! The contract is the host's (`drive` in `veridoc/crates/veridoc-conformance/src/lib.rs`): the
//! emitter runs with the plugin checkout as its working directory, the rendered tree is the only
//! project the suite ever collects, and the tree is deleted when the run ends. `veridoc
//! install-plugin`, `veridoc validate-plugin`, and the mechanism's suite-program mode all render
//! through here, so a fixture that fails locally fails the install too.
//!
//! Three choices are specific to Rust and are the reason this file is not a translation of
//! the Node plugin's emitter:
//!
//! - The fixture's `Cargo.toml` has no dependencies. The plugin builds the fixture's harness with
//!   `cargo build --release` inside the suite's `run` deadline (`harness_binary` in `run.rs`), so
//!   a dependency download would make conformance depend on the network and the clock. The
//!   rendered harness therefore reads its two request fields with
//!   `std` alone, which a real project has no reason to do — it already has its own argument
//!   types in scope.
//! - The four bivalence negatives are this plugin's own structural negatives. The `bivalence`
//!   answer in `bivalence.rs` is `false` for a claim with no leaf and for a claim whose tree
//!   contains a `satisfies` leaf, because the plugin does not evaluate Rust predicates; the Node
//!   plugin's negatives are static-analysis cases that would all be answered the same way here
//!   and prove nothing.
//! - The fixture declares one `pub` item and one private one, in the same file. `surface` answers
//!   the first and refuses the second, and `derivable` reads its facts out of the `pub` function's
//!   own header, so the two answers the suite checks are both decided by text the emitter owns.

use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The fixture's subject, and the left half of every claim name it declares.
const SUBJECT: &str = "probe";
/// The concept the fixture declares; a claim with no `symbol` is about this.
const CONCEPT: &str = "probe-scan";
/// The scenario the suite runs.
const SCENARIO: &str = "probe/refuses-missing-root";
/// The action the suite asks for, and the harness's one implemented branch.
const ACTION: &str = "run-probe";
/// The fixture directory the scenario runs against: it holds no `config.json`.
const FIXTURE: &str = "fixtures/empty-directory";
/// The harness binary the rendered project ships.
const HARNESS: &str = "veridoc-harness";

/// The citation store the manifest names, the log that answers it, and the turn the suite asks
/// for. A `file` store is a directory of `<id>.jsonl` logs read against the project root.
const CITATION_STORE: &str = "session";
const CITATION_PATH: &str = ".veridoc/citations";
const CITATION_ID: &str = "probe-refusal";
const CITATION_TURN: u32 = 1;
/// The one turn of that log. `cite` answers its `text`, and the suite fails a resolved citation
/// whose text is empty, so the sentence here is the answer the rule reads.
const CITATION_LOG: &str = concat!(
    "{\"turn\": 1, \"text\": \"The conformance fixture reserves exit code 2 for refusing a ",
    "directory that holds no config.json.\", \"state\": \"done\"}\n"
);

/// The project config: the citation store and nothing else, because the rest of that file
/// belongs to a run the suite does not drive.
fn config() -> String {
    format!(
        r#"{{
  "citationStores": [
    {{ "name": "{CITATION_STORE}", "kind": "file", "path": "{CITATION_PATH}" }}
  ]
}}
"#
    )
}

const CARGO_TOML: &str = r#"[package]
name = "veridoc-conformance-fixture"
version = "0.1.0"
edition = "2021"

# No dependencies, on purpose: the plugin builds this harness inside the suite's `run` deadline,
# so the harness decodes its request with `std` instead of pulling a JSON crate onto the clock.
[workspace]
"#;

const LIB_RS: &str = r#"//! The fixture's library: one module, so `collect` has one file to walk.

pub mod probe;

pub use probe::{probe, Config};
"#;

/// The fixture's source. `probe` is the `pub` item, so it is the one entry `surface` reports for
/// this file and the one declaration `derivable` reads facts out of; `config_of` is private, so
/// the same two answers must leave it out.
const PROBE_RS: &str = r#"//! The fixture's subject. `probe` refuses a directory that holds no `config.json`, and the
//! refusal names the directory, so a leaf over `message` has something to compare.

use std::path::{Path, PathBuf};

/// The configuration `probe` reads from one directory.
pub struct Config {
    pub root: String,
}

/// Refuses a directory that holds no `config.json`.
pub fn probe(dir: &Path) -> Result<Config, String> {
    let path = config_of(dir);
    if !path.is_file() {
        return Err(format!("{} holds no config.json", dir.display()));
    }
    Ok(Config {
        root: dir.display().to_string(),
    })
}

/// Where a directory's configuration would sit. Private, so the crate does not promise it.
fn config_of(dir: &Path) -> PathBuf {
    dir.join("config.json")
}
"#;

/// The rendered harness. It answers the two observables the vocabulary declares and nothing else:
/// the scenario claim's tree holds only `equals` and `contains` leaves, which the plugin answers
/// from the observed values itself, so no `satisfied` entry is asked for. The suite's
/// `run-returns-values` rule compiles and runs this text; no unit test here does, because
/// compiling it means building the whole fixture.
const HARNESS_RS: &str = r#"//! The fixture's scenario harness. Contract, one line each way:
//!
//! ```text
//! stdin  {"scenarioId":"..","action":"..","args":[],"fixture":"<absolute dir>","deadlineMs":..}
//! stdout {"observables":{"exit_code":2,"message":".."}}
//! ```

use std::io::Read;

use veridoc_conformance_fixture::probe;

fn main() {
    let mut request = String::new();
    if std::io::stdin().read_to_string(&mut request).is_err() {
        eprintln!("the harness has no stdin to read the request from");
        std::process::exit(127);
    }
    let action = string_field(&request, "action").unwrap_or_default();
    let fixture = string_field(&request, "fixture")
        .or_else(|| std::env::args().nth(1))
        .unwrap_or_default();
    if action != "run-probe" {
        eprintln!("the harness implements no action {action:?}");
        std::process::exit(127);
    }
    let (code, message) = match probe(std::path::Path::new(&fixture)) {
        Ok(_) => (0, String::new()),
        Err(reason) => (2, reason),
    };
    println!(
        "{{\"observables\":{{\"exit_code\":{code},\"message\":{}}}}}",
        quote(&message)
    );
}

/// The unescaped value of one top-level string field of the request line. The fixture keeps the
/// request flat, so the first occurrence of `"key":"` is the field.
fn string_field(text: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\":\"");
    let rest = text.find(&marker)? + marker.len();
    let bytes = text.as_bytes();
    let mut value = String::new();
    let mut at = rest;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' if at + 1 < bytes.len() => {
                match bytes[at + 1] {
                    b'n' => value.push('\n'),
                    b't' => value.push('\t'),
                    b'r' => value.push('\r'),
                    b'"' => value.push('"'),
                    b'\\' => value.push('\\'),
                    b'/' => value.push('/'),
                    other => value.push(other as char),
                }
                at += 2;
            }
            b'"' => return Some(value),
            other => value.push(other as char),
        }
        at += 1;
    }
    None
}

/// A JSON string literal for `text`.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
"#;

/// The vocabulary file: the concept, the document node, the two observables, the action, the one
/// fixture, and the harness `run` builds.
fn tree() -> String {
    format!(
        r#"{{
  "concepts": ["{CONCEPT}"],
  "documents": [
    {{
      "path": "docs/architecture.spec.md",
      "audience": "the reader of a conformance report",
      "question": "What does this plugin answer for a Rust project?"
    }}
  ],
  "observables": [
    {{ "name": "exit_code", "type": "number" }},
    {{ "name": "message", "type": "string" }}
  ],
  "actions": [
    {{
      "name": "{ACTION}",
      "calls": {{ "kind": "symbol", "name": "{SUBJECT}", "from": "src/probe.rs" }},
      "produces": ["exit_code", "message"]
    }}
  ],
  "fixtures": ["{FIXTURE}"],
  "harness": "{HARNESS}"
}}
"#
    )
}

/// The artefact the `no-assertion-tree` witness points at. Its hash is computed as the fixture is
/// rendered, so the claim cannot drift from the file beside it.
const EVIDENCE: &str = "probe refused fixtures/empty-directory: holds no config.json\n";

/// The claims: the scenario claim the suite runs, the four structural negatives, and the one
/// positive probe. Every predicate named in `veridoc-conformance.json` is declared here, because
/// the suite looks each one up in the `collect` answer by predicate.
fn claims(artefact_hash: &str) -> String {
    format!(
        r#"{{
  "claims": [
    {{
      "kind": "ensures",
      "symbol": "{SUBJECT}",
      "concept": "{CONCEPT}",
      "predicate": "refuses-missing-config",
      "text": "probe refuses a directory that holds no config.json: the run exits 2 and the message names the file.",
      "channel": {{
        "scenario": "{SCENARIO}",
        "asserts": {{
          "all": [
            {{ "obs": "exit_code", "is": {{ "equals": 2 }} }},
            {{ "obs": "message", "is": {{ "contains": "config.json" }} }}
          ]
        }}
      }}
    }},
    {{
      "kind": "witness",
      "concept": "{CONCEPT}",
      "predicate": "no-assertion-tree",
      "text": "The refusal is recorded in evidence/refusals.txt, so this claim carries no assertion tree at all.",
      "channel": {{ "artefact": {{ "path": "evidence/refusals.txt", "sha256": "{artefact_hash}" }} }}
    }},
    {{
      "kind": "ensures",
      "symbol": "{SUBJECT}",
      "concept": "{CONCEPT}",
      "predicate": "empty-conjunction",
      "text": "An empty conjunction asserts nothing, so no run can contradict it.",
      "channel": {{ "scenario": "{SCENARIO}", "asserts": {{ "all": [] }} }}
    }},
    {{
      "kind": "ensures",
      "symbol": "{SUBJECT}",
      "concept": "{CONCEPT}",
      "predicate": "always-true-predicate",
      "text": "exit_code is never negative, so this predicate holds of every run and divides no pair of outcomes.",
      "channel": {{
        "scenario": "{SCENARIO}",
        "asserts": {{ "obs": "exit_code", "is": {{ "satisfies": {{ "source": "exit_code >= 0" }} }} }}
      }}
    }},
    {{
      "kind": "ensures",
      "symbol": "{SUBJECT}",
      "concept": "{CONCEPT}",
      "predicate": "predicate-inside-conjunction",
      "text": "One unevaluated predicate inside a conjunction is enough that the whole tree is not settled.",
      "channel": {{
        "scenario": "{SCENARIO}",
        "asserts": {{
          "all": [
            {{ "obs": "exit_code", "is": {{ "equals": 2 }} }},
            {{ "obs": "message", "is": {{ "satisfies": {{ "source": "message.len() > 0" }} }} }}
          ]
        }}
      }}
    }},
    {{
      "kind": "ensures",
      "symbol": "{SUBJECT}",
      "concept": "{CONCEPT}",
      "predicate": "separates-the-refusal",
      "text": "Comparing the exit code to 2 separates the refusal from every run that accepted the directory.",
      "channel": {{
        "scenario": "{SCENARIO}",
        "asserts": {{ "all": [{{ "obs": "exit_code", "is": {{ "equals": 2 }} }}] }}
      }}
    }}
  ]
}}
"#
    )
}

/// `veridoc-conformance.json`: the names the suite drives. The citation pair names the store and
/// the log this project renders beside it, so `cite-schema` reads an answer out of a file the
/// emitter wrote rather than a name with nothing behind it.
fn conformance_manifest() -> String {
    format!(
        r#"{{
  "scenarioClaim": "refuses-missing-config",
  "scenarioId": "{SCENARIO}",
  "fixture": "{FIXTURE}",
  "action": "{ACTION}",
  "observables": ["exit_code", "message"],
  "citationStore": "{CITATION_STORE}",
  "citationId": "{CITATION_ID}",
  "citationTurn": {CITATION_TURN},
  "bivalence": [
    {{ "predicate": "no-assertion-tree", "bivalent": false }},
    {{ "predicate": "empty-conjunction", "bivalent": false }},
    {{ "predicate": "always-true-predicate", "bivalent": false }},
    {{ "predicate": "predicate-inside-conjunction", "bivalent": false }},
    {{ "predicate": "separates-the-refusal", "bivalent": true, "method": "structural" }}
  ]
}}
"#
    )
}

const SPEC_MD: &str = "\
# The conformance fixture

<doc-section name=\"probe\">

## What the fixture claims

`probe` refuses a directory that holds no `config.json` <doc-claim name=\"refuses-missing-config\"/>, and
the message of the refusal names the directory it refused.

The suite runs the one scenario this project declares and asks `bivalence` about the five claims that
carry a predicate of their own.
";

/// Render the fixture into `dir` and return `dir`. The directory is created if it is absent, and
/// an existing tree is replaced, because the host removes it after the run and a stale tree would
/// report claims the current emitter never wrote.
pub fn render(dir: &Path) -> Result<PathBuf, String> {
    if dir.exists() {
        fs::remove_dir_all(dir)
            .map_err(|e| format!("{} could not be cleared: {e}", dir.display()))?;
    }
    write(dir, "Cargo.toml", CARGO_TOML)?;
    write(dir, "src/lib.rs", LIB_RS)?;
    write(dir, "src/probe.rs", PROBE_RS)?;
    write(dir, &format!("src/bin/{HARNESS}.rs"), HARNESS_RS)?;
    let hash = hex_sha256(EVIDENCE.as_bytes());
    write(dir, "evidence/refusals.txt", EVIDENCE)?;
    write(dir, "src/probe.claims.json", &claims(&hash))?;
    write(dir, "veridoc.tree.json", &tree())?;
    write(dir, "veridoc-conformance.json", &conformance_manifest())?;
    write(dir, ".veridoc/config.json", &config())?;
    write(
        dir,
        &format!("{CITATION_PATH}/{CITATION_ID}.jsonl"),
        CITATION_LOG,
    )?;
    write(dir, "docs/architecture.spec.md", SPEC_MD)?;
    fs::create_dir_all(dir.join(FIXTURE))
        .map_err(|e| format!("the fixture directory could not be created: {e}"))?;
    Ok(dir.to_path_buf())
}

fn write(root: &Path, relative: &str, text: &str) -> Result<(), String> {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("{} could not be created: {e}", parent.display()))?;
    }
    fs::write(&path, text).map_err(|e| format!("{} could not be written: {e}", path.display()))
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut out: Option<PathBuf> = None;
    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        if arg == "--out" {
            out = argv.next().map(PathBuf::from);
        }
    }
    let dir = render(&out.ok_or("no --out <dir> was given")?)?;
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    writeln!(writer, "{}", dir.display())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three files the suite reads have to name one project. A predicate the manifest probes
    /// and the sidecar does not declare reaches the author as `collect reported no claim with
    /// predicate`, which points at the manifest rather than at the file that is wrong, and a stale
    /// artefact hash reaches them as a red witness claim in a project they never wrote.
    #[test]
    fn the_rendered_files_name_one_project() {
        let dir = std::env::temp_dir().join(format!("veridoc-emitter-test-{}", std::process::id()));
        let outcome = (|| -> Result<(), String> {
            render(&dir)?;
            let manifest = json_at(&dir, "veridoc-conformance.json")?;
            let tree = json_at(&dir, "veridoc.tree.json")?;
            let sidecar = json_at(&dir, "src/probe.claims.json")?;

            let claims = sidecar["claims"]
                .as_array()
                .ok_or("the sidecar declares no claims array")?;
            let declared: Vec<&str> = claims
                .iter()
                .filter_map(|claim| claim["predicate"].as_str())
                .collect();
            let probes = manifest["bivalence"]
                .as_array()
                .ok_or("the manifest names no bivalence probes")?;
            for probe in probes {
                let predicate = probe["predicate"]
                    .as_str()
                    .ok_or("a probe names no predicate")?;
                if !declared.contains(&predicate) {
                    return Err(format!(
                        "the manifest probes {predicate:?}, which no claim declares: {declared:?}"
                    ));
                }
            }

            let scenario_claim = manifest["scenarioClaim"]
                .as_str()
                .ok_or("the manifest names no scenario claim")?;
            let scenario_id = manifest["scenarioId"]
                .as_str()
                .ok_or("the manifest names no scenario id")?;
            let run = claims
                .iter()
                .find(|claim| claim["predicate"].as_str() == Some(scenario_claim))
                .ok_or_else(|| format!("no claim declares predicate {scenario_claim:?}"))?;
            if run["channel"]["scenario"].as_str() != Some(scenario_id) {
                return Err(format!(
                    "claim {scenario_claim:?} declares {:?}, but the suite asks for {scenario_id:?}",
                    run["channel"]["scenario"].as_str()
                ));
            }

            let action = manifest["action"]
                .as_str()
                .ok_or("the manifest names no action")?;
            let actions = tree["actions"]
                .as_array()
                .ok_or("the tree declares no actions")?;
            if !actions
                .iter()
                .any(|entry| entry["name"].as_str() == Some(action))
            {
                return Err(format!("the tree declares no action {action:?}"));
            }
            let fixture = manifest["fixture"]
                .as_str()
                .ok_or("the manifest names no fixture")?;
            let fixtures = tree["fixtures"]
                .as_array()
                .ok_or("the tree declares no fixtures")?;
            if !fixtures.iter().any(|entry| entry.as_str() == Some(fixture)) {
                return Err(format!("the tree declares no fixture {fixture:?}"));
            }
            if !dir.join(fixture).is_dir() {
                return Err(format!("the fixture directory {fixture:?} is not rendered"));
            }
            let harness = tree["harness"]
                .as_str()
                .ok_or("the tree declares no harness")?;
            if !dir.join(format!("src/bin/{harness}.rs")).is_file() {
                return Err(format!(
                    "the tree names harness {harness:?}, which is not rendered"
                ));
            }
            let observed = manifest["observables"]
                .as_array()
                .ok_or("the manifest names no observables")?;
            let vocabulary = tree["observables"]
                .as_array()
                .ok_or("the tree declares no observables")?;
            for entry in observed {
                let name = entry.as_str().ok_or("an observable is not a name")?;
                if !vocabulary
                    .iter()
                    .any(|declared| declared["name"].as_str() == Some(name))
                {
                    return Err(format!(
                        "the manifest observes {name:?}, which the tree does not declare"
                    ));
                }
            }

            let artefact = claims
                .iter()
                .filter_map(|claim| claim["channel"]["artefact"].as_object())
                .next()
                .ok_or("no claim carries an artefact")?;
            let path = artefact["path"]
                .as_str()
                .ok_or("the artefact names no path")?;
            let want = artefact["sha256"]
                .as_str()
                .ok_or("the artefact names no hash")?;
            let bytes =
                fs::read(dir.join(path)).map_err(|e| format!("{path} is unreadable: {e}"))?;
            if hex_sha256(&bytes) != want {
                return Err(format!(
                    "{path} does not hash to the {want} the claim records"
                ));
            }
            Ok(())
        })();
        let _ = fs::remove_dir_all(&dir);
        outcome.unwrap();
    }

    /// The three methods this build answers out of the rendered tree, not out of a name the
    /// manifest carries: `cite` reads the log the declared store's directory holds, and `surface`
    /// and `derivable` read the source that declares one `pub` item and one private one. A fixture
    /// that stopped rendering either would reach the suite as a red rule in a project nobody
    /// edited, which is what this check is for.
    #[test]
    fn the_rendered_project_answers_cite_derivable_and_surface() {
        let dir =
            std::env::temp_dir().join(format!("veridoc-emitter-methods-{}", std::process::id()));
        let outcome = (|| -> Result<(), String> {
            render(&dir)?;
            let manifest = json_at(&dir, "veridoc-conformance.json")?;
            let config = json_at(&dir, ".veridoc/config.json")?;
            let store = manifest["citationStore"]
                .as_str()
                .ok_or("the manifest names no citation store")?;
            let id = manifest["citationId"]
                .as_str()
                .ok_or("the manifest names no citation id")?;
            let turn = manifest["citationTurn"].as_u64().unwrap_or(1) as usize;
            let declared = config["citationStores"]
                .as_array()
                .ok_or("the rendered config declares no citationStores")?
                .iter()
                .find(|entry| entry["name"].as_str() == Some(store))
                .ok_or_else(|| {
                    format!("the config declares no store {store:?}, so cite answers no store named {store}")
                })?;
            if declared["kind"].as_str() != Some("file") {
                return Err(format!(
                    "store {store:?} is kind {:?}, which this plugin cannot read",
                    declared["kind"].as_str()
                ));
            }
            let path = declared["path"]
                .as_str()
                .ok_or_else(|| format!("store {store:?} declares no path for its logs"))?;
            let log = dir.join(path).join(format!("{id}.jsonl"));
            let text = fs::read_to_string(&log)
                .map_err(|e| format!("{} is unreadable: {e}", log.display()))?;
            let lines: Vec<&str> = text
                .lines()
                .filter(|line| !line.trim().is_empty())
                .collect();
            let entry = lines.get(turn - 1).ok_or_else(|| {
                format!(
                    "turn {turn} is not in {path}, which has {} turns",
                    lines.len()
                )
            })?;
            let cited: serde_json::Value =
                serde_json::from_str(entry).map_err(|e| format!("turn {turn} is not json: {e}"))?;
            if cited["text"].as_str().unwrap_or_default().trim().is_empty() {
                return Err(format!(
                    "turn {turn} of {path}/{id}.jsonl states no text, and the suite fails a \
                     resolved citation with an empty one"
                ));
            }

            let source = fs::read_to_string(dir.join("src/probe.rs"))
                .map_err(|e| format!("src/probe.rs is unreadable: {e}"))?;
            if !source.contains("pub fn probe(dir: &Path) -> Result<Config, String> {") {
                return Err(
                    "the subject every claim names is no longer a pub fn with a declared return \
                     type, so derivable answers it without a parameter or return fact"
                        .to_string(),
                );
            }
            if !source.contains("\nfn config_of(") {
                return Err(
                    "the fixture declares no private item, so surface has nothing to leave out"
                        .to_string(),
                );
            }
            Ok(())
        })();
        let _ = fs::remove_dir_all(&dir);
        outcome.unwrap();
    }

    /// One rendered file, parsed.
    fn json_at(root: &Path, relative: &str) -> Result<serde_json::Value, String> {
        let path = root.join(relative);
        let text = fs::read_to_string(&path)
            .map_err(|e| format!("{} is unreadable: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{relative} is not valid JSON: {e}"))
    }
}
