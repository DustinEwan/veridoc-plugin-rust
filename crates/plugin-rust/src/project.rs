//! The project on disk and the `collect` answer built from it.
//!
//! Two kinds of file are read:
//!
//! - `<root>/veridoc.tree.json` — the concepts, the document tree, and the observable, action,
//!   and fixture vocabulary.
//! - `<root>/src/**/<module>.claims.json` — the claims, one file per module: the claims about
//!   `src/config.rs` live in `src/config.claims.json`.
//!
//! An author writes the symbol a claim is about and the predicate; the plugin supplies the
//! referent token, the file, and the line. That is what makes a rename hurt: the claim keeps
//! naming the symbol it was written against, `resolve` cannot find it, and the run reports the
//! claim rather than reading the claim as still describing whatever now occupies the file.

use std::fs;
use std::path::{Path, PathBuf};

use crate::ids;
use serde::{Deserialize, Serialize};

/// The vocabulary and document-tree file, read from the project root.
pub const TREE_FILE: &str = "veridoc.tree.json";
/// The suffix a claim sidecar carries, beside the source file it describes.
pub const SIDECAR_SUFFIX: &str = ".claims.json";

/// What `collect` answers: the index, the raw claims, and the vocabulary.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectResult {
    pub concepts: Vec<String>,
    pub documents: Vec<DocumentDecl>,
    pub claims: Vec<RawClaim>,
    pub observables: Vec<ObservableDecl>,
    pub actions: Vec<ActionDecl>,
    pub fixtures: Vec<String>,
    pub skipped: Vec<Skipped>,
}

/// One claim as `collect` reports it. `file` and `line` are the sidecar's, so the diagnostic the
/// author is sent lands on the line they wrote.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RawClaim {
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

/// The channel a claim's evidence arrives on. Exactly one field is set; the engine rejects a
/// claim with two or none, so `collect` refuses the sidecar before that rather than reporting a
/// claim whose evidence is undefined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Channel {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario: Option<String>,
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
    /// How many of the three channels are populated. `scenario` and `asserts` are one channel --
    /// the assertion tree is the scenario's evidence, not a second channel (`Channel::populated` in
    /// `crates/veridoc-protocol/src/contract.rs`) -- so a claim that names a scenario and its
    /// assertion tree counts once, and one that adds an artefact counts twice and is refused.
    fn set(&self) -> usize {
        usize::from(self.scenario.is_some() || self.asserts.is_some())
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservableDecl {
    pub name: String,
    #[serde(rename = "type")]
    pub value_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActionDecl {
    pub name: String,
    pub calls: Subject,
    pub produces: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    pub file: String,
    pub reason: String,
}

/// The vocabulary file, with every list optional so a project that declares no fixtures reports
/// an empty list rather than a parse failure.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct Tree {
    pub concepts: Vec<String>,
    pub documents: Vec<DocumentDecl>,
    pub observables: Vec<ObservableDecl>,
    pub actions: Vec<ActionDecl>,
    pub fixtures: Vec<String>,
    /// The harness binary `run` builds and executes. A project that declares none can still make
    /// every claim, but a scenario-channel claim cannot be run, and the run failure says so.
    #[serde(default)]
    pub harness: Option<String>,
}

/// One sidecar file.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaimsFile {
    pub claims: Vec<AuthorClaim>,
}

/// A claim as the author writes it: no referent token, no file, and no line, because the plugin
/// derives all three.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthorClaim {
    pub kind: String,
    /// The item the claim is about. Absent means the claim is about its concept itself.
    #[serde(default)]
    pub symbol: Option<String>,
    /// The source file the symbol lives in. Absent means the file the sidecar describes.
    #[serde(default)]
    pub from: Option<String>,
    pub concept: String,
    pub predicate: String,
    pub text: String,
    pub channel: Channel,
}

const KINDS: [&str; 5] = ["requires", "ensures", "invariant", "witness", "because"];

/// Read the project under `root` and build the `collect` answer. A sidecar that does not parse,
/// or that declares a claim with two channels or none, is an error: the author is better served
/// by a refusal naming the file and the reason than by a claim set with a hole in it.
pub fn collect(root: &Path) -> Result<CollectResult, String> {
    let (tree, claims, skipped) = gather(root)?;
    Ok(CollectResult {
        concepts: tree.concepts,
        documents: tree.documents,
        claims,
        observables: tree.observables,
        actions: tree.actions,
        fixtures: tree.fixtures,
        skipped,
    })
}

/// The project as the methods after `collect` need it. `run` is sent a scenario id and `bivalence`
/// a claim name, and nothing else, so the vocabulary and the claim set are read again here rather
/// than carried in the answer.
pub struct Project {
    pub root: PathBuf,
    pub tree: Tree,
    pub claims: Vec<RawClaim>,
}

impl Project {
    /// The claims whose scenario channel is `scenario`, in sidecar order.
    pub fn scenario(&self, scenario: &str) -> Vec<&RawClaim> {
        self.claims
            .iter()
            .filter(|claim| claim.channel.scenario.as_deref() == Some(scenario))
            .collect()
    }

    /// The name the engine binds for `claim` in this project: `<subject>#<slug>` of the author's
    /// subject name, and when two collected claims slug alike, that name plus the identity suffix on
    /// every one of them, exactly as bind does (the renaming pass over collided ids in
    /// `crates/veridoc-engine/src/bind.rs`). `bivalence` is addressed by name and a leaf id carries a
    /// name, so a claim whose name collided must still be findable by the name the engine printed.
    /// Bind drops a claim whose subject did not resolve before it suffixes, but that drop is itself a
    /// bind error and bind errors stop the run before either method is called, so the collected set
    /// here is the set bind named.
    pub fn claim_name(&self, claim: &RawClaim) -> String {
        let subject = claim.subject.subject_name();
        let bare = ids::claim_name(subject, &claim.predicate);
        let shared = self
            .claims
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
}

/// Read the project for a method that runs after `collect`.
pub fn load(root: &Path) -> Result<Project, String> {
    let (tree, claims, _skipped) = gather(root)?;
    Ok(Project {
        root: root.to_path_buf(),
        tree,
        claims,
    })
}

fn gather(root: &Path) -> Result<(Tree, Vec<RawClaim>, Vec<Skipped>), String> {
    let (tree, mut skipped) = read_tree(root);
    let mut claims = Vec::new();
    for sidecar in sidecars(root)? {
        let relative = relative(root, &sidecar);
        let text = fs::read_to_string(&sidecar)
            .map_err(|e| format!("{relative} could not be read: {e}"))?;
        let parsed: ClaimsFile = serde_json::from_str(&text)
            .map_err(|e| format!("{relative} is not a claims file: {e}"))?;
        let described = described_source(root, &sidecar);
        let mut cursor = 0;
        for claim in parsed.claims {
            if !KINDS.contains(&claim.kind.as_str()) {
                return Err(format!(
                    "{relative}: kind {:?} is not one of {KINDS:?}",
                    claim.kind
                ));
            }
            if claim.channel.set() != 1 {
                return Err(format!(
                    "{relative}: claim {:?} sets {} channels; exactly one is the channel, and it \
                     is scenario with its asserts, artefact, or citation",
                    claim.predicate,
                    claim.channel.set()
                ));
            }
            let from = claim.from.clone().unwrap_or_else(|| described.clone());
            let subject = match claim.symbol {
                Some(name) => Subject::Symbol { name, from },
                None => Subject::Concept {
                    name: claim.concept.clone(),
                },
            };
            let line = locate(&text, &mut cursor, &claim.predicate);
            claims.push(RawClaim {
                kind: claim.kind,
                subject,
                concept: claim.concept,
                predicate: claim.predicate,
                text: claim.text,
                channel: claim.channel,
                file: relative.clone(),
                line,
            });
        }
    }
    if !sidecars_exist(root)? && claims.is_empty() {
        skipped.push(Skipped {
            file: "src".to_string(),
            reason: format!(
                "no *{SIDECAR_SUFFIX} file under src/, so the project declares no claim"
            ),
        });
    }
    Ok((tree, claims, skipped))
}

/// The vocabulary file, and the `skipped` entry it earns when it is missing.
fn read_tree(root: &Path) -> (Tree, Vec<Skipped>) {
    let path = root.join(TREE_FILE);
    match fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<Tree>(&text) {
            Ok(tree) => (tree, Vec::new()),
            Err(e) => (
                Tree::default(),
                vec![Skipped {
                    file: TREE_FILE.to_string(),
                    reason: format!("the vocabulary file is not readable as a tree: {e}"),
                }],
            ),
        },
        Err(_) => (
            Tree::default(),
            vec![Skipped {
                file: TREE_FILE.to_string(),
                reason: "the project declares no vocabulary file".to_string(),
            }],
        ),
    }
}

/// Every sidecar under `<root>/src`, in path order, so the claim order a project sees does not
/// depend on the directory listing.
fn sidecars(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    walk(&root.join("src"), &mut found)?;
    found.sort();
    Ok(found)
}

fn sidecars_exist(root: &Path) -> Result<bool, String> {
    Ok(!sidecars(root)?.is_empty())
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(());
    };
    let mut children: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    children.sort();
    for child in children {
        if child.is_dir() {
            walk(&child, found)?;
        } else if child
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(SIDECAR_SUFFIX))
        {
            found.push(child);
        }
    }
    Ok(())
}

/// The source file a sidecar describes: the same directory, the same stem with the sidecar
/// suffix removed, `.rs`. `file_stem` is not used because it strips only the last extension, and
/// `config.claims.json` would then describe `config.claims.rs`.
fn described_source(root: &Path, sidecar: &Path) -> String {
    let name = sidecar
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .trim_end_matches(SIDECAR_SUFFIX)
        .to_string();
    let parent = sidecar.parent().unwrap_or(root);
    relative(root, &parent.join(format!("{name}.rs")))
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

/// The 1-based line of the next occurrence of the predicate value at or after `cursor`. The
/// sidecar's own text is the authority for where a claim was written; a predicate that cannot be
/// located reports line 1 rather than a wrong line.
fn locate(text: &str, cursor: &mut usize, predicate: &str) -> usize {
    let needle = format!("\"{predicate}\"");
    let haystack = &text[*cursor..];
    let found = match haystack.find(&needle) {
        Some(at) => {
            let absolute = *cursor + at;
            *cursor = absolute + needle.len();
            absolute
        }
        None => match text.find(&needle) {
            Some(at) => at,
            None => return 1,
        },
    };
    text[..found].matches('\n').count() + 1
}

/// The concepts the project declares. `resolve` needs them to answer a `concept:` reference,
/// which arrives without a root of its own.
pub fn concepts(root: &Path) -> Vec<String> {
    read_tree(root).0.concepts
}

#[cfg(test)]
mod tests {
    use super::{Channel, Project, RawClaim, Subject};
    use crate::ids;
    use std::path::PathBuf;

    /// One collected claim about `parse_config`, with only the predicate the author wrote varying.
    fn claim(predicate: &str) -> RawClaim {
        RawClaim {
            kind: "requires".to_string(),
            subject: Subject::Symbol {
                name: "parse_config".to_string(),
                from: "src/config.rs".to_string(),
            },
            concept: "configuration".to_string(),
            predicate: predicate.to_string(),
            text: "The parser refuses a missing project root.".to_string(),
            channel: Channel {
                scenario: None,
                artefact: None,
                citation: None,
                asserts: None,
            },
            file: "src/config.claims.json".to_string(),
            line: 1,
        }
    }

    fn project(predicates: &[&str]) -> Project {
        Project {
            root: PathBuf::from("/project"),
            tree: Default::default(),
            claims: predicates.iter().map(|p| claim(p)).collect(),
        }
    }

    #[test]
    fn two_claims_that_slug_alike_are_named_apart_by_their_suffixes() {
        let project = project(&[
            "rejects missing project root",
            "Rejects-Missing: PROJECT root",
            "reports the offending path",
        ]);
        let first = &project.claims[0];
        let second = &project.claims[1];
        let bare_first = ids::claim_name(first.subject.subject_name(), &first.predicate);
        let bare_second = ids::claim_name(second.subject.subject_name(), &second.predicate);
        assert_eq!(bare_first, bare_second, "the two predicates slug alike");

        let named_first = project.claim_name(first);
        let named_second = project.claim_name(second);
        assert_ne!(
            named_first, named_second,
            "a collided name must still reach each claim"
        );
        for (named, bare, claim) in [
            (&named_first, &bare_first, first),
            (&named_second, &bare_second, second),
        ] {
            assert!(
                named.starts_with(bare) && named != bare,
                "{named} is not {bare} plus a suffix"
            );
            assert_eq!(
                named[bare.len()..],
                ids::suffix(&ids::claim_identity(
                    claim.subject.subject_name(),
                    &claim.predicate
                )),
                "{named} carries the wrong suffix"
            );
        }

        let third = &project.claims[2];
        let named_third = project.claim_name(third);
        assert_eq!(
            named_third,
            ids::claim_name(third.subject.subject_name(), &third.predicate),
            "a name nothing shares keeps its bare spelling"
        );
        assert!(!named_third.contains('~'), "{named_third}");
    }
}
