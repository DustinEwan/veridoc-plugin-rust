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
//! - The fixture's only dependency is the plugin's own claim library, reached by path. The plugin
//!   builds the fixture's harness with `cargo build --release --example veridoc` inside the suite's
//!   `run` deadline (`harness_binary` in `run.rs`), so a registry download would make conformance
//!   depend on the network and the clock. The claim library depends on `std` alone, which is what
//!   keeps that path dependency enough.
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
/// The directory a scenario runs against: it holds no `config.json`. It is a project asset the
/// scenario opens by path, not a declared fixture (`docs/decisions/0006:47-48`).
const FIXTURE: &str = "fixtures/empty-directory";
/// The fixture's claim module: the claims about `src/probe.rs` and the scenarios they carry.
const CLAIM_MODULE: &str = "src/probe.doc.rs";
/// The fixture's harness target: one call into the claim library. Where the target sits is this
/// plugin's convention (`HARNESS_EXAMPLE` in `run.rs`), so the emitter renders it there.
const HARNESS_TARGET: &str = "examples/veridoc.rs";
/// The index file the plugin reads, and the item inside it. These mirror `INDEX_FILE` and
/// `INDEX_ITEM` in `crates/plugin-rust/src/project.rs`, and `index_body` mirrors `init::index_body`:
/// this program renders a project without linking the plugin crate.
const INDEX_FILE: &str = "index.doc.rs";
const INDEX_ITEM: &str = "VERIDOC_INDEX";

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

/// The fixture's manifest. Its one dependency is the plugin's claim library, by path, so building
/// the fixture's harness needs no registry.
fn cargo_toml(claims_crate: &str) -> String {
    format!(
        r#"[package]
name = "veridoc-conformance-fixture"
version = "0.1.0"
edition = "2021"

[dev-dependencies]
veridoc-claims = {{ path = "{claims_crate}" }}

[workspace]
"#
    )
}

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

/// The fixture's harness: the claims of every claim module it links, in one call into the claim
/// library (`docs/decisions/0006:67-74`). The plugin builds this target and asks it `--list` and
/// `--run <claim name>`; the fixture never runs it by hand.
const HARNESS_RS: &str = r#"//! The fixture's harness: one call into the claim library.

#[path = "../src/probe.doc.rs"]
mod probe_doc;

fn main() -> std::process::ExitCode {
    veridoc_claims::harness(veridoc_claims::merge([probe_doc::claims()]))
}
"#;

/// The tree the index declares: the concept and the document node, and nothing else
/// (`docs/decisions/0001`, `0006:83`).
fn tree() -> String {
    format!(
        r#"{{
  "concepts": {{ "{CONCEPT}": "docs/architecture.spec.md" }},
  "documents": [
    {{
      "path": "docs/architecture.spec.md",
      "audience": "the reader of a conformance report",
      "question": "What does this plugin answer for a Rust project?"
    }}
  ]
}}
"#
    )
}

/// The index file's text, in the shape `project::index_text` reads back.
fn index_body(tree: &str) -> String {
    format!(
        "// The document tree, created by `veridoc init` through the plugin. Edit it; do not \
         regenerate it.\n\n/// The concepts and the documents this project declares. The plugin reads \
         this item; `cargo` does not compile this file, because the index sits at the project root \
         rather than under `src/`.\npub const {INDEX_ITEM}: &str = r##\"{tree}\"##;\n"
    )
}

/// The artefact the `no-assertion-tree` witness points at. Its hash is computed as the fixture is
/// rendered, so the claim cannot drift from the file beside it.
const EVIDENCE: &str = "probe refused fixtures/empty-directory: holds no config.json\n";

/// The fixture's claim module: the scenario claim the suite runs, the four structural negatives,
/// and the one positive probe. Every predicate named in `veridoc-conformance.json` is declared here,
/// because the suite looks each one up in the `collect` answer by predicate.
fn claims(artefact_hash: &str) -> String {
    format!(
        r#"//! The fixture's claims about `src/probe.rs`, and the scenario each one carries.
//!
//! A scenario is code in this file. It calls the project directly, so no action name and no fixture
//! registry stands between the claim and the call it makes (`docs/decisions/0006:57-74`), and two
//! claims that need the same world call the same function (`docs/decisions/0006:81-85`).

use veridoc_claims::{{Assert, Channel, Claim, Kind, Outcome, Scenario, Subject, Type}};

/// The directory the run claims are made against: it holds no `config.json`.
const EMPTY: &str = "{FIXTURE}";

/// The two values every run claim declares, with the types its assertions are checked against.
const VALUES: &[(&str, Type)] = &[("exit_code", Type::Number), ("message", Type::String)];

/// The one world the run claims share: `probe` against a directory that holds no configuration.
fn refusal() -> Result<Outcome, String> {{
    let message = match veridoc_conformance_fixture::probe(std::path::Path::new(EMPTY)) {{
        Ok(_) => return Err("probe accepted a directory that holds no config.json".to_string()),
        Err(reason) => reason,
    }};
    Ok(Outcome::new().value("exit_code", 2).value("message", message))
}}

/// The same world, with the answer the `satisfies` leaf asks for. This plugin does not evaluate a
/// Rust predicate, so the scenario answers it, keyed by the leaf's own source text, and `bivalence`
/// reports the claim as not bivalent for that reason.
fn refusal_exit_code_is_never_negative() -> Result<Outcome, String> {{
    Ok(refusal()?.satisfied("exit_code >= 0", true))
}}

/// The same world again, answering the other predicate: one scenario function per leaf set, because
/// the answer is keyed by the leaf's source and not by the claim.
fn refusal_message_is_not_empty() -> Result<Outcome, String> {{
    Ok(refusal()?.satisfied("message.len() > 0", true))
}}

/// The claims the suite drives.
pub fn claims() -> Vec<Claim> {{
    vec![
        Claim {{
            kind: Kind::Ensures,
            subject: Subject::symbol("{SUBJECT}", "src/probe.rs"),
            concept: "{CONCEPT}",
            predicate: "refuses-missing-config",
            text: "probe refuses a directory that holds no config.json: the run exits 2 and the \
                   message names the file.",
            channel: Channel::run(Assert::all([
                Assert::eq("exit_code", 2),
                Assert::has("message", "config.json"),
            ])),
            scenario: Some(Scenario {{
                given: "a directory that holds no config.json",
                when: "probe is called against it",
                returns: VALUES,
                run: refusal,
            }}),
        }},
        Claim {{
            kind: Kind::Witness,
            subject: Subject::concept("{CONCEPT}"),
            concept: "{CONCEPT}",
            predicate: "no-assertion-tree",
            text: "The refusal is recorded in evidence/refusals.txt, so this claim carries no \
                   assertion tree at all.",
            channel: Channel::Witness {{
                path: "evidence/refusals.txt",
                sha256: "{artefact_hash}",
            }},
            scenario: None,
        }},
        Claim {{
            kind: Kind::Ensures,
            subject: Subject::symbol("{SUBJECT}", "src/probe.rs"),
            concept: "{CONCEPT}",
            predicate: "empty-conjunction",
            text: "An empty conjunction asserts nothing, so no run can contradict it.",
            channel: Channel::run(Assert::all([])),
            scenario: Some(Scenario {{
                given: "a directory that holds no config.json",
                when: "probe is called against it",
                returns: VALUES,
                run: refusal,
            }}),
        }},
        Claim {{
            kind: Kind::Ensures,
            subject: Subject::symbol("{SUBJECT}", "src/probe.rs"),
            concept: "{CONCEPT}",
            predicate: "always-true-predicate",
            text: "exit_code is never negative, so this predicate holds of every run and divides \
                   no pair of outcomes.",
            channel: Channel::run(Assert::satisfies("exit_code", "exit_code >= 0")),
            scenario: Some(Scenario {{
                given: "a directory that holds no config.json",
                when: "probe is called against it",
                returns: VALUES,
                run: refusal_exit_code_is_never_negative,
            }}),
        }},
        Claim {{
            kind: Kind::Ensures,
            subject: Subject::symbol("{SUBJECT}", "src/probe.rs"),
            concept: "{CONCEPT}",
            predicate: "predicate-inside-conjunction",
            text: "One unevaluated predicate inside a conjunction is enough that the whole tree is \
                   not settled.",
            channel: Channel::run(Assert::all([
                Assert::eq("exit_code", 2),
                Assert::satisfies("message", "message.len() > 0"),
            ])),
            scenario: Some(Scenario {{
                given: "a directory that holds no config.json",
                when: "probe is called against it",
                returns: VALUES,
                run: refusal_message_is_not_empty,
            }}),
        }},
        Claim {{
            kind: Kind::Ensures,
            subject: Subject::symbol("{SUBJECT}", "src/probe.rs"),
            concept: "{CONCEPT}",
            predicate: "separates-the-refusal",
            text: "Comparing the exit code to 2 separates the refusal from every run that accepted \
                   the directory.",
            channel: Channel::run(Assert::all([Assert::eq("exit_code", 2)])),
            scenario: Some(Scenario {{
                given: "a directory that holds no config.json",
                when: "probe is called against it",
                returns: VALUES,
                run: refusal,
            }}),
        }},
    ]
}}
"#
    )
}

/// `veridoc-conformance.json`: the names the suite drives. `scenarioClaim` is a predicate, because
/// the suite finds the claim in the `collect` answer and then uses that claim's own id to address
/// `run`; `values` names what that claim's scenario must answer. The citation pair names the store
/// and the log this project renders beside it, so `cite-schema` reads an answer out of a file the
/// emitter wrote rather than a name with nothing behind it.
fn conformance_manifest() -> String {
    format!(
        r#"{{
  "scenarioClaim": "refuses-missing-config",
  "values": ["exit_code", "message"],
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

/// Where the plugin's claim library sits, so the rendered project can depend on it by path. The
/// compile-time path is the checkout the emitter was built from, which is where the library lives
/// when `veridoc install-plugin` builds a plugin from a checkout; the working-directory path is the
/// fallback for a checkout moved after the build, which is also where the emitter's own tests run.
fn claims_library() -> Result<PathBuf, String> {
    let built = Path::new(env!("CARGO_MANIFEST_DIR")).join("../veridoc-claims");
    let checkout = Path::new("crates/veridoc-claims");
    for candidate in [built.as_path(), checkout] {
        if candidate.join("Cargo.toml").is_file() {
            return candidate
                .canonicalize()
                .map_err(|e| format!("{} is not readable: {e}", candidate.display()));
        }
    }
    Err(format!(
        "no veridoc-claims crate at {} or {}, so the rendered project cannot depend on the claim \
         library by path",
        built.display(),
        checkout.display()
    ))
}

const SPEC_MD: &str = "\
# The conformance fixture

<doc-section name=\"probe\">

## What the fixture claims

`probe` refuses a directory that holds no `config.json` <doc-claim name=\"refuses-missing-config\"/>, and
the message of the refusal names the directory it refused.

The suite runs the scenario of one claim and asks `bivalence` about the five claims that carry a
predicate of their own.
";

/// Render the fixture into `dir` and return `dir`. The directory is created if it is absent, and
/// an existing tree is replaced, because the host removes it after the run and a stale tree would
/// report claims the current emitter never wrote.
pub fn render(dir: &Path) -> Result<PathBuf, String> {
    if dir.exists() {
        fs::remove_dir_all(dir)
            .map_err(|e| format!("{} could not be cleared: {e}", dir.display()))?;
    }
    let claims_crate = claims_library()?;
    write(
        dir,
        "Cargo.toml",
        &cargo_toml(&claims_crate.display().to_string()),
    )?;
    write(dir, "src/lib.rs", LIB_RS)?;
    write(dir, "src/probe.rs", PROBE_RS)?;
    let hash = hex_sha256(EVIDENCE.as_bytes());
    write(dir, "evidence/refusals.txt", EVIDENCE)?;
    write(dir, CLAIM_MODULE, &claims(&hash))?;
    write(dir, HARNESS_TARGET, HARNESS_RS)?;
    write(dir, INDEX_FILE, &index_body(&tree()))?;
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

    /// Read the rendered index back the way the plugin does: the JSON inside `VERIDOC_INDEX`.
    fn index_json_at(root: &Path) -> Result<serde_json::Value, String> {
        let text = fs::read_to_string(root.join(INDEX_FILE))
            .map_err(|e| format!("the rendered index is unreadable: {e}"))?;
        let body = text
            .split_once("r##\"")
            .ok_or("the rendered index holds no raw string")?
            .1
            .split_once("\"##")
            .ok_or("the rendered index raw string is never closed")?
            .0;
        serde_json::from_str(body).map_err(|e| format!("the rendered index is not readable: {e}"))
    }

    /// The rendered files have to name one project. A predicate the manifest probes and the claim
    /// module does not declare reaches the author as `collect reported no claim with predicate`,
    /// which points at the manifest rather than at the file that is wrong, and a stale artefact hash
    /// reaches them as a red witness claim in a project they never wrote.
    #[test]
    fn the_rendered_files_name_one_project() {
        let dir = std::env::temp_dir().join(format!("veridoc-emitter-test-{}", std::process::id()));
        let outcome = (|| -> Result<(), String> {
            render(&dir)?;
            let manifest = json_at(&dir, "veridoc-conformance.json")?;
            let index = index_json_at(&dir)?;
            let module = fs::read_to_string(dir.join(CLAIM_MODULE))
                .map_err(|e| format!("{CLAIM_MODULE} is unreadable: {e}"))?;

            // The index holds the document tree and the concepts and nothing else (`0006:83`).
            let keys: Vec<&str> = index
                .as_object()
                .map(|entries| entries.keys().map(String::as_str).collect())
                .unwrap_or_default();
            if keys != ["concepts", "documents"] {
                return Err(format!(
                    "the index declares {keys:?}, which is not the concepts and the document tree \
                     alone"
                ));
            }

            let declared = quoted_after(&module, "predicate: ");
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

            // The claim the suite runs must carry its own scenario, and that scenario must return
            // the values the manifest says the run answers.
            let scenario_claim = manifest["scenarioClaim"]
                .as_str()
                .ok_or("the manifest names no scenario claim")?;
            if !module.contains(&format!("predicate: \"{scenario_claim}\",")) {
                return Err(format!("no claim declares predicate {scenario_claim:?}"));
            }
            if !module.contains("scenario: Some(Scenario") {
                return Err("no claim of this project carries a scenario".to_string());
            }
            let values = manifest["values"]
                .as_array()
                .ok_or("the manifest names no values")?;
            for entry in values {
                let name = entry.as_str().ok_or("a value is not a name")?;
                if !module.contains(&format!("(\"{name}\", Type::")) {
                    return Err(format!(
                        "the manifest observes {name:?}, which no scenario declares it returns"
                    ));
                }
            }

            if !dir.join(FIXTURE).is_dir() {
                return Err(format!("the asset directory {FIXTURE:?} is not rendered"));
            }
            let harness = fs::read_to_string(dir.join(HARNESS_TARGET))
                .map_err(|e| format!("{HARNESS_TARGET} is unreadable: {e}"))?;
            if !harness.contains("#[path = \"../src/probe.doc.rs\"]") {
                return Err(format!(
                    "the harness does not name {CLAIM_MODULE}, so its claims are never listed"
                ));
            }

            let artefacts = quoted_after(&module, "sha256: ");
            let want = artefacts.first().ok_or("no claim pins an artefact hash")?;
            let path = quoted_after(&module, "path: ")
                .into_iter()
                .find(|name| name.ends_with(".txt"))
                .ok_or("no claim names an artefact path")?;
            let bytes =
                fs::read(dir.join(path)).map_err(|e| format!("{path} is unreadable: {e}"))?;
            if hex_sha256(&bytes) != *want {
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

    /// The string values that follow one key in the rendered claim module. The module is Rust, and
    /// the emitter writes one `predicate: "..."` per claim, so the names are readable without a Rust
    /// parser. The plugin reads them from the compiled binary instead; this check only needs the
    /// names the source states, so it can run without building the fixture.
    fn quoted_after<'a>(text: &'a str, key: &str) -> Vec<&'a str> {
        let mut found = Vec::new();
        let mut rest = text;
        while let Some(at) = rest.find(key) {
            let tail = &rest[at + key.len()..];
            let Some(open) = tail.find('"') else { break };
            let after_open = &tail[open + 1..];
            let Some(close) = after_open.find('"') else {
                break;
            };
            found.push(&after_open[..close]);
            rest = &after_open[close..];
        }
        found
    }
}
