//! `derivable`: the facts a reader can already see in one declaration.
//!
//! The ban these facts feed rejects a claim whose text a reader could read off the code, so a
//! fact states only what the declaration's own text says. The TypeScript plugin reads its facts
//! out of `ts-morph`'s type checker; Rust has no checker standing behind the plugin, so the
//! four families here are read out of the header and the body the author wrote:
//!
//! - the visibility the header carries, which is what `surface` lists and what a restricted
//!   `pub(crate)` says about the item,
//! - the parameter list and the declared return type, one sentence, as the header spells them,
//! - the variants an enum accepts, which is the Rust shape of the union the TypeScript plugin
//!   reports,
//! - each explicit `panic!` or `unreachable!` in the body, and the first `?` that hands a
//!   failure to the caller when the header declares a fallible return type.
//!
//! Nothing inferred is reported: a type the author did not write, a bound the compiler would
//! derive, or a failure a code path could reach without stating is left out, because a ban that
//! over-rejects teaches the author to switch it off.
//!
//! A referent id is a digest, so the declaration it names is found by recomputing the id of
//! every item under the declared source roots and keeping the one that matches -- the same ids
//! `resolve` answers with, which is the only way the host's claim and this answer can meet.

use std::path::Path;

use serde::Serialize;

use crate::decl::{self, Declaration};
use crate::ids::{concept_id, referent_id};
use crate::project;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivableResult {
    pub facts: Vec<String>,
}

/// The facts visible in the declaration `referent` names. An id that names no item the project
/// declares answers no facts, which is the answer the host reads as "nothing here is redundant":
/// inventing a fact would reject a claim about code this plugin has not seen.
pub fn facts(root: &Path, referent: &str) -> DerivableResult {
    if let Some((name, document)) = declared_concept(root, referent) {
        return DerivableResult {
            facts: vec![format!("concept {name} is declared in {document}.")],
        };
    }
    match declaration_of(root, referent) {
        None => DerivableResult { facts: Vec::new() },
        Some((file, declaration)) => DerivableResult {
            facts: stated(&file, &declaration),
        },
    }
}

/// The concept whose identity is `referent`, with the document that declares it.
fn declared_concept(root: &Path, referent: &str) -> Option<(String, String)> {
    project::concept_declarations(root)
        .into_iter()
        .find(|(name, _)| concept_id(name) == referent)
}

/// The declaration an id names, with the project-relative path it was hashed from.
fn declaration_of(root: &Path, referent: &str) -> Option<(String, Declaration)> {
    for file in project::sources(root) {
        let relative = project::relative(root, &file);
        let Ok(declarations) = decl::declarations_in(&file) else {
            continue;
        };
        if let Some(declaration) = declarations
            .into_iter()
            .find(|item| referent_id(&relative, &item.name) == referent)
        {
            return Some((relative, declaration));
        }
    }
    None
}

/// The sentences the declaration states, in the order a reader meets them.
fn stated(file: &str, declaration: &Declaration) -> Vec<String> {
    let name = &declaration.name;
    let mut facts = vec![format!(
        "{name} is {} in {file}.",
        declaration.visibility.text()
    )];
    if let Some(params) = declaration.params() {
        let taken = if params.is_empty() {
            "no parameters".to_string()
        } else {
            params.join(", ")
        };
        let returned = declaration
            .return_type()
            .unwrap_or_else(|| "()".to_string());
        facts.push(format!("{name} takes {taken} and returns {returned}."));
    }
    let variants = declaration.variants();
    if !variants.is_empty() {
        facts.push(format!("{name} accepts one of: {}.", variants.join(", ")));
    }
    facts.extend(fallible(file, declaration));
    facts
}

/// The failures the body states: each explicit `panic!` or `unreachable!`, and the first `?`
/// that hands a failure up, reported only when the header declares a fallible return type to
/// hand it to. Only a function's own body fails: the `mod` that holds it reports its children's
/// panics as its own otherwise, and that is an invented fact.
fn fallible(file: &str, declaration: &Declaration) -> Vec<String> {
    if declaration.kind != "fn" {
        return Vec::new();
    }
    let Some(body) = &declaration.body else {
        return Vec::new();
    };
    let propagates = declaration
        .return_type()
        .is_some_and(|returned| returned.starts_with("Result") || returned.starts_with("Option"));
    let mut facts = Vec::new();
    let mut propagation_told = false;
    for (line, text) in body.lines() {
        let code = decl::code(text);
        for marker in ["panic!(", "unreachable!("] {
            if code.contains(marker) {
                facts.push(format!("{} panics at {file}:{line}.", declaration.name));
            }
        }
        if !propagation_told && propagates && code.contains('?') {
            propagation_told = true;
            facts.push(format!(
                "{} propagates a failure with the ? operator at {file}:{line}.",
                declaration.name
            ));
        }
    }
    facts
}

#[cfg(test)]
mod tests {
    use super::facts;
    use crate::ids::{concept_id, referent_id};
    use std::fs;
    use std::path::{Path, PathBuf};

    /// A project rendered for one test, under a directory no other test touches.
    fn project(name: &str, sources: &[(&str, &str)]) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("veridoc-derivable-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (file, text) in sources {
            write(&root.join(file), text);
        }
        root
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().expect("a source file has a parent")).unwrap();
        fs::write(path, text).unwrap();
    }

    const CONFIG_RS: &str = "\
use std::path::Path;

/// Refuses a directory that holds no `config.json`.
pub fn parse_config(dir: &Path, strict: bool) -> Result<Config, String> {
    let path = dir.join(\"config.json\");
    if strict && !path.is_file() {
        return Err(format!(\"{} holds no config.json\", dir.display()));
    }
    read(&path)?;
    Ok(Config)
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

pub enum Problem {
    Missing,
    Locked(u32),
}

pub fn guard(dir: &Path) -> Config {
    if dir.as_os_str().is_empty() {
        unreachable!(\"the host always names a directory\");
    }
    Config
}
";

    #[test]
    fn a_public_function_reports_what_its_header_states() {
        let root = project("header", &[("src/config.rs", CONFIG_RS)]);
        let answer = facts(&root, &referent_id("src/config.rs", "parse_config"));
        assert!(
            answer
                .facts
                .contains(&"parse_config is pub in src/config.rs.".to_string()),
            "{:?}",
            answer.facts
        );
        assert!(
            answer.facts.contains(
                &"parse_config takes dir: &Path, strict: bool and returns Result<Config, String>."
                    .to_string()
            ),
            "{:?}",
            answer.facts
        );
        assert!(
            answer.facts.iter().any(|fact| fact.starts_with(
                "parse_config propagates a failure with the ? operator at src/config.rs:9"
            )),
            "{:?}",
            answer.facts
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_body_that_states_a_refusal_names_the_line_it_sits_on() {
        let root = project("panic", &[("src/config.rs", CONFIG_RS)]);
        let answer = facts(&root, &referent_id("src/config.rs", "guard"));
        assert!(
            answer
                .facts
                .contains(&"guard panics at src/config.rs:24.".to_string()),
            "{:?}",
            answer.facts
        );
        let variants = facts(&root, &referent_id("src/config.rs", "Problem"));
        assert_eq!(
            variants.facts,
            vec![
                "Problem is pub in src/config.rs.".to_string(),
                "Problem accepts one of: Missing, Locked.".to_string()
            ]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unknown_referent_id_answers_no_facts() {
        let root = project("unknown", &[("src/config.rs", CONFIG_RS)]);
        let answer = facts(&root, &referent_id("src/absent.rs", "nothing"));
        assert_eq!(answer.facts, Vec::<String>::new());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_concept_answers_the_document_that_declares_it() {
        let root = project(
            "concept",
            &[
                ("src/config.rs", CONFIG_RS),
                (
                    crate::project::INDEX_FILE,
                    &crate::init::index_body(
                        r#"{ "concepts": { "project-scan": "docs/spec.md" }, "documents": [] }"#,
                    ),
                ),
            ],
        );
        let answer = facts(&root, &concept_id("project-scan"));
        assert_eq!(
            answer.facts,
            vec!["concept project-scan is declared in docs/spec.md.".to_string()]
        );
        let _ = fs::remove_dir_all(&root);
    }
}
