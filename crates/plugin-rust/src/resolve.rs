//! `resolve`: settling the reference keys `collect` reported.
//!
//! A reference key is `<file>|<name>` for a symbol and `concept|<name>` for a concept, which is
//! `RefToken::key` in the engine (`crates/veridoc-protocol/src/contract.rs`). An answer
//! that cannot settle one is a normal answer carrying a reason, not a protocol error, because the
//! engine turns the reason into the diagnostic the author reads.

use std::path::Path;

use serde::Serialize;

use crate::decl::declarations_in;
use crate::ids::{concept_id, referent_id};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedRef {
    pub r#ref: String,
    pub referent_id: String,
    pub signature: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnresolvedRef {
    pub r#ref: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveResult {
    pub resolved: Vec<ResolvedRef>,
    pub unresolved: Vec<UnresolvedRef>,
}

/// Settle every reference. `concepts` is the vocabulary the project declares, so a claim about an
/// undeclared concept is reported as unresolved rather than answered with an invented identity.
pub fn resolve(root: &Path, refs: &[String], concepts: &[String]) -> ResolveResult {
    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();
    for reference in refs {
        let Some(name) = reference.strip_prefix("concept|") else {
            if let Some((from, name)) = reference.split_once('|') {
                settle_symbol(root, reference, from, name, &mut resolved, &mut unresolved);
            } else {
                unresolved.push(UnresolvedRef {
                    r#ref: reference.clone(),
                    reason: "the reference is neither <file>|<name> nor concept|<name>".to_string(),
                });
            }
            continue;
        };
        if concepts.iter().any(|declared| declared == name) {
            resolved.push(ResolvedRef {
                r#ref: reference.clone(),
                referent_id: concept_id(name),
                signature: format!("concept {name}"),
            });
        } else {
            unresolved.push(UnresolvedRef {
                r#ref: reference.clone(),
                reason: format!("the vocabulary file declares no concept {name:?}"),
            });
        }
    }
    ResolveResult {
        resolved,
        unresolved,
    }
}

fn settle_symbol(
    root: &Path,
    reference: &str,
    from: &str,
    name: &str,
    resolved: &mut Vec<ResolvedRef>,
    unresolved: &mut Vec<UnresolvedRef>,
) {
    let mut fail = |reason: String| {
        unresolved.push(UnresolvedRef {
            r#ref: reference.to_string(),
            reason,
        })
    };
    if from.is_empty() || name.is_empty() {
        fail(format!("{reference:?} has an empty file or name part"));
        return;
    }
    let declarations = match declarations_in(&root.join(from)) {
        Ok(declarations) => declarations,
        Err(reason) => {
            fail(reason);
            return;
        }
    };
    match declarations.into_iter().find(|d| d.name == name) {
        Some(declaration) => resolved.push(ResolvedRef {
            r#ref: reference.to_string(),
            referent_id: referent_id(from, name),
            signature: declaration.signature,
        }),
        None => fail(format!(
            "{from} declares no {name}; a claim that names it is left pointing at nothing"
        )),
    }
}
