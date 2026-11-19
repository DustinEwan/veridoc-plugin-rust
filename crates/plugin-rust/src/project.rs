//! The project on disk and the `collect` answer built from it.
//!
//! A Rust project adds three kinds of file (`docs/decisions/0006`), and this module reads two of
//! them:
//!
//! - `<root>/index.doc.rs` — the document tree and the concepts, and nothing else.
//! - `<root>/<package>/src/<module>.doc.rs` — the claims about that module, written as Rust with
//!   the `veridoc-claims` library, each claim carrying its own scenario.
//!
//! The third kind, `docs/<name>.spec.md`, holds prose and is the host's to read, not ours.
//!
//! The claims are not read out of those files. A claim's scenario is code, so the claim set is
//! whatever the compiled claim modules say it is: `collect` builds the project's harness target and
//! asks it `--list` (run.rs), which is why there is no second copy of the claim data for a data file
//! and a binary to disagree about. The plugin still reads the claim modules as text, for two things
//! the binary cannot answer: the file and the line each claim was written on, so a diagnostic lands
//! on the author's own line.
//!
//! An author writes the symbol a claim is about and the predicate; the plugin supplies the referent
//! token, the file, and the line. That is what makes a rename hurt: the claim keeps naming the symbol
//! it was written against, `resolve` cannot find it, and the run reports the claim rather than
//! reading the claim as still describing whatever now occupies the file.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::ids;
use serde::{Deserialize, Serialize};

/// The index file, read from the project root. It is Rust source, and it is the only place this
/// plugin reads the tree from: the host names no file, because the host cannot parse one
/// (`docs/decisions/0013`). It holds the document tree and the concepts and nothing else
/// (`docs/decisions/0001`, `0006:83`).
pub const INDEX_FILE: &str = "index.doc.rs";
/// The item inside the index file that holds it. The plugin reads this one item rather than parsing
/// the file, because the index sits at the project root, outside the crate, and nothing compiles it.
pub const INDEX_ITEM: &str = "VERIDOC_INDEX";
/// The suffix a claim module carries, beside the source file it describes: the claims about
/// `src/config.rs` live in `src/config.doc.rs`.
pub const CLAIM_SUFFIX: &str = ".doc.rs";
/// The directory name Cargo gives a package's sources. Every directory of this name in the project
/// is scanned, because a Cargo workspace has one per package and its root has none of its own. The
/// index names no source roots: a project's layout is a property of its package manager, not
/// something an author restates (`docs/decisions/0006:168-173`).
pub const PACKAGE_SOURCE_DIR: &str = "src";
/// Directories that are never a package's source: build output, the tool's own directory, and
/// version control.
const PRUNED_DIRS: [&str; 3] = ["target", ".veridoc", ".git"];

/// What `collect` answers: the index, the claims, and the scenario each claim carries.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectResult {
    pub concepts: Vec<String>,
    pub documents: Vec<DocumentDecl>,
    pub claims: Vec<RawClaim>,
    /// Each claim's scenario, keyed by the claim's name. A claim with no entry is judged by prose
    /// and source alone (`docs/decisions/0006:92-97`).
    pub scenarios: BTreeMap<String, ScenarioDecl>,
    pub skipped: Vec<Skipped>,
}

/// One claim as `collect` reports it. `id` is the name the engine binds for the claim and the
/// address of its scenario, so it travels with the claim rather than being recomputed by the host,
/// which would drift from the slug the claim's own runtime derived. `file` and `line` are the claim
/// module's, so the diagnostic the author is sent lands on the line they wrote.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RawClaim {
    pub id: String,
    pub kind: String,
    pub subject: Subject,
    pub concept: String,
    pub predicate: String,
    pub text: String,
    pub channel: Channel,
    pub file: String,
    pub line: usize,
}

/// What a claim is about: a resolved symbol or a declared concept. Internally tagged on `kind`,
/// which is the wire shape `ref-token.json` fixes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Subject {
    #[serde(rename = "symbol")]
    Symbol { name: String, from: String },
    #[serde(rename = "concept")]
    Concept { name: String },
}

impl Subject {
    /// The author's name for the thing the claim is about: the symbol's name for a symbol subject,
    /// the concept's name for a concept subject. It is the left half of the claim's readable name
    /// and of the identity hash, and it comes from the author rather than from the path the code
    /// sits in (`RefToken::subject_name` in `crates/veridoc-protocol/src/contract.rs`). The
    /// path-derived referent id is what `resolve` reports, and nothing here derives from it.
    pub fn subject_name(&self) -> &str {
        match self {
            Subject::Symbol { name, .. } | Subject::Concept { name } => name,
        }
    }
}

/// The channel a claim's evidence arrives on. Exactly one field is set; the engine rejects a claim
/// with two or none, so `collect` refuses the claim before that rather than reporting a claim whose
/// evidence is undefined. A run claim sets `asserts` and nothing else: the scenario has no identity
/// of its own, so there is no scenario name to point at (`docs/decisions/0006:50-55`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Channel {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artefact: Option<Artefact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citation: Option<Citation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asserts: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Artefact {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Citation {
    pub store: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<u64>,
}

impl Channel {
    /// How many channels are populated: the assertion tree is a run claim's evidence, an artefact is
    /// a witness claim's, and a citation is a quoted claim's (`Channel::populated` in
    /// `crates/veridoc-protocol/src/contract.rs`). Exactly one is set on a claim that can be judged.
    fn set(&self) -> usize {
        usize::from(self.asserts.is_some())
            + usize::from(self.artefact.is_some())
            + usize::from(self.citation.is_some())
    }
}

/// A document node the tree declares.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentDecl {
    pub path: String,
    pub audience: String,
    pub question: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_applicable: Option<String>,
}

/// The scenario a claim carries: its two step labels and the values its code returns. The labels are
/// plain strings; the engine prints them and evaluates nothing from them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScenarioDecl {
    pub given: String,
    pub when: String,
    pub values: Vec<ValueDecl>,
}

/// One value a scenario returns, with the type the assertion is checked against. The type is what
/// makes `contains` over a number a refusal instead of a verdict that could never have passed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValueDecl {
    pub name: String,
    #[serde(rename = "type")]
    pub value_type: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    pub file: String,
    pub reason: String,
}

/// The document tree, as the index declares it: the concepts and the documents. A concept names
/// the document that declares it, because `derivable` answers that file when a claim cites the
/// concept (`docs/decisions/0006:83`).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct Tree {
    pub concepts: BTreeMap<String, String>,
    pub documents: Vec<DocumentDecl>,
}

/// One claim as the project's harness answers it from `--list`. The claim's file and line are not
/// here: a compiled binary cannot say where its source sat, so the plugin finds those itself.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListedClaim {
    pub kind: String,
    pub subject: Subject,
    pub concept: String,
    pub predicate: String,
    pub text: String,
    pub channel: Channel,
    #[serde(default)]
    pub scenario: Option<ScenarioDecl>,
}

const KINDS: [&str; 5] = ["requires", "ensures", "invariant", "witness", "because"];

/// Read the project under `root` and build the `collect` answer. A claim that sets two channels or
/// none, and a claim whose predicate the plugin cannot find in any claim module, is an error: the
/// author is better served by a refusal naming the claim than by a claim set with a hole in it.
pub fn collect(root: &Path) -> Result<CollectResult, String> {
    let (tree, claims, scenarios, skipped) = gather(root)?;
    Ok(CollectResult {
        concepts: tree.concepts.keys().cloned().collect(),
        documents: tree.documents,
        claims,
        scenarios,
        skipped,
    })
}

/// The project as the methods after `collect` need it. `run` is sent a claim name and `bivalence` a
/// claim name, and nothing else, so the claim set and the scenarios are read again here rather
/// than carried in the answer.
pub struct Project {
    pub root: PathBuf,
    pub claims: Vec<RawClaim>,
    pub scenarios: BTreeMap<String, ScenarioDecl>,
}

impl Project {
    /// The scenario the claim named `name` carries, if it carries one.
    pub fn scenario(&self, name: &str) -> Option<&ScenarioDecl> {
        self.scenarios.get(name)
    }

    /// The claim the engine addresses by `name`, if this project declares it.
    pub fn claim(&self, name: &str) -> Option<&RawClaim> {
        self.claims.iter().find(|claim| claim.id == name)
    }
}

/// The name the engine binds for each claim of `claims`, in the same order. A collision is a fact
/// about the set rather than about one claim, so `gather` needs the whole set before it can key a
/// scenario by name.
fn claim_names(claims: &[RawClaim]) -> Vec<String> {
    claims.iter().map(|claim| name_of(claims, claim)).collect()
}

/// One claim's name, measured against the set it belongs to: `<subject>#<slug>` of the author's
/// subject name, and when two collected claims slug alike, that name plus the identity suffix on
/// every one of them, exactly as bind does (the renaming pass over collided ids in
/// `crates/veridoc-engine/src/bind.rs`). `bivalence` is addressed by name and a leaf id carries a
/// name, so a claim whose name collided must still be findable by the name the engine printed. Bind
/// drops a claim whose subject did not resolve before it suffixes, but that drop is itself a bind
/// error and bind errors stop the run before either method is called, so the collected set here is
/// the set bind named.
fn name_of(claims: &[RawClaim], claim: &RawClaim) -> String {
    let subject = claim.subject.subject_name();
    let bare = ids::claim_name(subject, &claim.predicate);
    let shared = claims
        .iter()
        .filter(|other| ids::claim_name(other.subject.subject_name(), &other.predicate) == bare)
        .count()
        > 1;
    if shared {
        format!(
            "{bare}{}",
            ids::suffix(&ids::claim_identity(subject, &claim.predicate))
        )
    } else {
        bare
    }
}

/// Read the project for a method that runs after `collect`.
pub fn load(root: &Path) -> Result<Project, String> {
    let (_, claims, scenarios, _) = gather(root)?;
    Ok(Project {
        root: root.to_path_buf(),
        claims,
        scenarios,
    })
}

/// What one read of a project yields: the index, the claims it declares, each claim's own
/// scenario, and the files that were passed over with the reason.
type Gathered = (
    Tree,
    Vec<RawClaim>,
    BTreeMap<String, ScenarioDecl>,
    Vec<Skipped>,
);

fn gather(root: &Path) -> Result<Gathered, String> {
    let (tree, mut skipped) = read_tree(root);
    let modules = claim_modules(root);
    let listed = crate::run::listed_claims(root)?;
    let mut claims = Vec::new();
    let mut declared: Vec<Option<ScenarioDecl>> = Vec::new();
    for claim in listed {
        if !KINDS.contains(&claim.kind.as_str()) {
            return Err(format!(
                "the harness reports kind {:?}, which is not one of {KINDS:?}",
                claim.kind
            ));
        }
        if claim.channel.set() != 1 {
            return Err(format!(
                "claim {:?} sets {} channels; exactly one is the channel, and it is the assertion \
                 tree, an artefact, or a citation",
                claim.predicate,
                claim.channel.set()
            ));
        }
        let (file, line) = locate_claim(root, &modules, &claim.predicate)?;
        let raw = RawClaim {
            id: String::new(),
            kind: claim.kind,
            subject: claim.subject,
            concept: claim.concept,
            predicate: claim.predicate,
            text: claim.text,
            channel: claim.channel,
            file,
            line,
        };
        declared.push(claim.scenario);
        claims.push(raw);
    }
    if modules.is_empty() {
        skipped.push(Skipped {
            file: PACKAGE_SOURCE_DIR.to_string(),
            reason: format!(
                "no *{CLAIM_SUFFIX} claim module under any {PACKAGE_SOURCE_DIR} directory, so the \
                 project declares no claim"
            ),
        });
    }
    // The name is a fact about the whole set, because two claims that slug alike both carry the
    // collision suffix, so it is computed here and carried on each claim from here on.
    let names = claim_names(&claims);
    for (claim, name) in claims.iter_mut().zip(names.iter()) {
        claim.id = name.clone();
    }
    let mut scenarios = BTreeMap::new();
    for (index, scenario) in declared.into_iter().enumerate() {
        if let Some(scenario) = scenario {
            scenarios.insert(names[index].clone(), scenario);
        }
    }
    Ok((tree, claims, scenarios, skipped))
}

/// The document tree, and the `skipped` entry it earns when it is missing or unreadable.
fn read_tree(root: &Path) -> (Tree, Vec<Skipped>) {
    let path = root.join(INDEX_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) => {
            return (
                Tree::default(),
                vec![Skipped {
                    file: INDEX_FILE.to_string(),
                    reason: "the project declares no index".to_string(),
                }],
            );
        }
    };
    match index_text(&text).and_then(|body| {
        serde_json::from_str::<Tree>(&body)
            .map_err(|e| format!("the index is not a readable tree: {e}"))
    }) {
        Ok(tree) => (tree, Vec::new()),
        Err(reason) => (
            Tree::default(),
            vec![Skipped {
                file: INDEX_FILE.to_string(),
                reason,
            }],
        ),
    }
}

/// The body of the index's `VERIDOC_INDEX` raw string. The index is Rust source that nothing
/// compiles, so the plugin takes the one item it names and reads the JSON inside it: a project keeps
/// its index in the language the rest of its documentation is written in, and the plugin still has
/// one predictable thing to read.
pub fn index_text(text: &str) -> Result<String, String> {
    let marker = format!("const {INDEX_ITEM}");
    let from = text.find(marker.as_str()).ok_or(format!(
        "`{INDEX_FILE}` declares no `const {INDEX_ITEM}: &str = r\"...\"`"
    ))?;
    let tail = &text[from..];
    let bytes = tail.as_bytes();
    let mut open = None;
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'r' {
            let mut hashes = at + 1;
            while hashes < bytes.len() && bytes[hashes] == b'#' {
                hashes += 1;
            }
            if hashes < bytes.len() && bytes[hashes] == b'"' {
                open = Some((at, hashes - at - 1, hashes + 1));
                break;
            }
        }
        at += 1;
    }
    let (_, hashes, body_start) = open.ok_or("the index is not written as a raw string")?;
    let close = format!("\"{}", "#".repeat(hashes));
    let body = &tail[body_start..];
    let end = body
        .find(close.as_str())
        .ok_or("the index raw string is never closed")?;
    Ok(body[..end].to_string())
}

/// Every claim module in the project, in path order, so the order a project sees does not depend on
/// the directory listing. A claim module is a `.doc.rs` file under a package's `src` directory.
pub fn claim_modules(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk_packages(root, CLAIM_SUFFIX, &mut found);
    found.sort();
    found
}

/// Descend the project until a package's `src` directory is reached, and collect the files of one
/// suffix under it. `target`, `.veridoc`, and `.git` are never descended: the first holds a copy of
/// every source file the build has seen, and the other two hold no project source.
fn walk_packages(dir: &Path, suffix: &str, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut children: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    children.sort();
    for child in children {
        let name = child
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if !child.is_dir() || PRUNED_DIRS.contains(&name) {
            continue;
        }
        if name == PACKAGE_SOURCE_DIR {
            walk(&child, suffix, found);
        } else {
            walk_packages(&child, suffix, found);
        }
    }
}

/// Every Rust source file under every package's `src` directory, in path order. `derivable` and
/// `surface` read these, and a referent identity is hashed from the project-relative path they
/// carry, so the two methods can only report what `resolve` can settle.
pub fn sources(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk_packages(root, ".rs", &mut found);
    found.sort();
    found
}

fn walk(dir: &Path, suffix: &str, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut children: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    children.sort();
    for child in children {
        if child.is_dir() {
            walk(&child, suffix, found);
        } else if child
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(suffix))
        {
            found.push(child);
        }
    }
}

/// Where a claim was written: the claim module that names the predicate, and the line its predicate
/// string sits on. A harness that answers a claim no module declares is refused rather than reported
/// with a file name nobody can open.
fn locate_claim(
    root: &Path,
    modules: &[PathBuf],
    predicate: &str,
) -> Result<(String, usize), String> {
    let needle = format!("\"{predicate}\"");
    for module in modules {
        let Ok(text) = fs::read_to_string(module) else {
            continue;
        };
        if let Some(at) = text.find(&needle) {
            return Ok((relative(root, module), text[..at].matches('\n').count() + 1));
        }
    }
    Err(format!(
        "the harness reports the claim {predicate:?}, which no *{CLAIM_SUFFIX} module of this \
         project declares"
    ))
}

/// The project-relative POSIX path, which is what a referent identity is hashed from and what
/// a diagnostic prints.
pub fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// The concepts the project declares. `resolve` needs them to answer a `concept:` reference,
/// which arrives without a root of its own.
pub fn concepts(root: &Path) -> Vec<String> {
    read_tree(root).0.concepts.into_keys().collect()
}

/// Every concept with the document that declares it, in name order. `derivable` answers the
/// declaring document for a claim that cites a concept.
pub fn concept_declarations(root: &Path) -> Vec<(String, String)> {
    read_tree(root).0.concepts.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(body: &str) -> String {
        format!("pub const {INDEX_ITEM}: &str = r##\"{body}\"##;\n")
    }

    #[test]
    fn the_index_is_one_raw_string_and_nothing_else_is_read() {
        let text = index("{\"concepts\":{\"scan\":\"docs/spec.md\"},\"documents\":[]}");
        let tree: Tree = serde_json::from_str(&index_text(&text).unwrap()).unwrap();
        assert_eq!(
            tree.concepts.get("scan").map(String::as_str),
            Some("docs/spec.md")
        );
        assert!(tree.documents.is_empty());
    }

    #[test]
    fn an_index_that_declares_vocabulary_is_refused_by_name() {
        // `0006:83` leaves the index with the tree and the concepts, so a project that puts the old
        // vocabulary back is told why it was refused rather than handed a parse error.
        let text = index("{\"concepts\":{},\"documents\":[],\"fixtures\":[]}");
        let reason = index_text(&text)
            .and_then(|body| {
                serde_json::from_str::<Tree>(&body)
                    .map_err(|e| format!("the index is not a readable tree: {e}"))
            })
            .unwrap_err();
        assert!(reason.contains("fixtures"), "{reason}");
    }

    #[test]
    fn an_index_with_no_raw_string_names_the_item_it_wanted() {
        let reason = index_text("pub const OTHER: &str = \"{}\";").unwrap_err();
        assert!(reason.contains(INDEX_ITEM), "{reason}");
    }

    #[test]
    fn a_claim_counts_one_channel() {
        let run: Channel =
            serde_json::from_str("{\"asserts\":{\"obs\":\"x\",\"is\":{\"equals\":1}}}").unwrap();
        assert_eq!(run.set(), 1);
        let two: Channel =
            serde_json::from_str("{\"asserts\":null,\"artefact\":{\"path\":\"a\",\"sha256\":\"b\"},\"citation\":{\"store\":\"s\",\"id\":\"i\"}}")
                .unwrap();
        assert_eq!(two.set(), 2);
    }
}
