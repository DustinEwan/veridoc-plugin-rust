//! `bivalence`: whether a claim's assertion can come out both ways.
//!
//! The rule is structural and deliberately narrow. A tree with at least one `equals` or
//! `contains` leaf is bivalent, because one side of the comparison comes from a run the project
//! performs and the other from the author, so both outcomes stay attainable until a run says
//! otherwise. A tree with no leaf is not bivalent: nothing in it can fail, so the claim would
//! pass on every run. A tree containing a `satisfies` leaf is answered `false` rather than
//! guessed, because settling that leaf means deciding a Rust predicate's two outcomes without
//! running them, which this plugin does not claim to do. The engine then refuses the claim the
//! scenario channel and says why (`veridoc-1.profile.json`, `bivalence.ifAbsent`), which is more
//! use to the author than a verdict the plugin cannot support.

use serde_json::{json, Value};

use crate::project::Project;
use crate::run::leaves;

/// Answer `bivalence` for one claim name. An unknown name is a normal answer, not a refusal: the
/// engine is asking about a claim it believes it bound, and a `false` with the reason reaches the
/// author as a diagnostic, where a `-32000` reaches them as a plugin failure.
pub fn bivalence(project: &Project, claim_name: &str) -> Value {
    let Some(claim) = project
        .claims
        .iter()
        .find(|candidate| project.claim_name(candidate) == claim_name)
    else {
        return json!({
            "bivalent": false,
            "method": "structural",
            "reason": format!("no claim {claim_name} was collected from this project"),
        });
    };
    let checks: Vec<&Value> = claim
        .channel
        .asserts
        .as_ref()
        .map(leaves)
        .unwrap_or_default();
    if checks.is_empty() {
        return json!({
            "bivalent": false,
            "method": "structural",
            "reason": "the claim declares no assertion tree, so nothing in it can fail",
        });
    }
    if let Some(source) = checks.iter().find_map(|check| {
        check
            .get("satisfies")
            .and_then(|spec| spec.get("source"))
            .and_then(Value::as_str)
    }) {
        return json!({
            "bivalent": false,
            "method": "structural",
            "reason": format!(
                "{source:?} is a predicate this plugin does not evaluate, so its two outcomes are \
                 not settled; state the check as equals or contains over an observable"
            ),
        });
    }
    json!({
        "bivalent": true,
        "method": "structural",
        "reason": format!(
            "{} check{} over observed values, each of which a run can satisfy or contradict",
            checks.len(),
            if checks.len() == 1 { "" } else { "s" }
        ),
    })
}
