//! `cite`: the two v1 citation stores, read the way a `because` claim names them.
//!
//! A `file` store is a directory of `<id>.jsonl` logs under the project root: turn `n` of the
//! log is line `n`, and its `text` is the cited passage. A `git` store is the repository the
//! project lives in: the object is checked with `git cat-file -e`, the answer is the commit
//! message, and `state` says whether the commit has landed (`merged`) or is still out on a
//! branch nobody merged (`dangling`).
//!
//! The stores come from the project's `.veridoc/config.json` — the file the host names in
//! `VERIDOC_CONFIG` when it starts the plugin, and the same file under the project root when it
//! starts the plugin with only a working directory, which is what the conformance driver does.
//! Only `citationStores` is read here; the rest of that file is the host's configuration.
//!
//! Every miss is an `unresolved` entry naming what was looked for, because the host turns the
//! reason into the diagnostic the author reads, and a bare failure would leave them guessing
//! which of the two stores they did not declare.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

/// One citation the host asks about, as `cite-req.json` carries it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Citation {
    pub store: String,
    pub id: String,
    #[serde(default)]
    pub turn: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedCitation {
    pub id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnresolvedCitation {
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CiteResult {
    pub resolved: Vec<ResolvedCitation>,
    pub unresolved: Vec<UnresolvedCitation>,
}

/// One declared store. `path` is the `file` store's directory, relative to the project root; a
/// `git` store reads the repository and names no path.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Store {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub path: Option<String>,
}

/// The rest of the host's config is its own to read, so unknown fields are left alone here.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    #[serde(default)]
    citation_stores: Vec<Store>,
}

/// Settle every citation against the stores the project declares.
pub fn cite(root: &Path, citations: &[Citation]) -> CiteResult {
    cite_in(root, &stores(root), citations)
}

/// The stores the project declares. A config that cannot be read or parsed declares none, which
/// reaches the author as `no store named …` on the citation they asked for rather than as a
/// refusal of the whole method.
pub fn stores(root: &Path) -> Vec<Store> {
    stores_at(&config_path(root))
}

fn config_path(root: &Path) -> PathBuf {
    match std::env::var("VERIDOC_CONFIG") {
        Ok(path) if !path.is_empty() => PathBuf::from(path),
        _ => root.join(".veridoc").join("config.json"),
    }
}

fn stores_at(path: &Path) -> Vec<Store> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Config>(&text)
        .map(|config| config.citation_stores)
        .unwrap_or_default()
}

fn cite_in(root: &Path, stores: &[Store], citations: &[Citation]) -> CiteResult {
    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();
    for citation in citations {
        let answer = match stores.iter().find(|store| store.name == citation.store) {
            None => Err(format!("no store named {}", citation.store)),
            Some(store) if store.kind == "file" => read_log(root, store, citation),
            Some(store) if store.kind == "git" => read_commit(root, citation),
            Some(store) => Err(format!("store kind {} is not readable", store.kind)),
        };
        match answer {
            Ok(entry) => resolved.push(entry),
            Err(reason) => unresolved.push(UnresolvedCitation {
                id: citation.id.clone(),
                reason,
            }),
        }
    }
    CiteResult {
        resolved,
        unresolved,
    }
}

/// Turn `n` of a `file` store's log. A turn with no line, a line that is not JSON, and an entry
/// that is not an object are three different misses, and the author needs the one that happened.
fn read_log(root: &Path, store: &Store, citation: &Citation) -> Result<ResolvedCitation, String> {
    let named = store.path.clone().unwrap_or_default();
    let file = root.join(&named).join(format!("{}.jsonl", citation.id));
    if !file.is_file() {
        return Err(format!(
            "{}.jsonl is not in {}",
            citation.id,
            store.path.as_deref().unwrap_or("(no path)")
        ));
    }
    let text = fs::read_to_string(&file)
        .map_err(|e| format!("{} could not be read: {e}", file.display()))?;
    let turn = citation.turn.unwrap_or(1);
    let line = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .nth(turn as usize - 1)
        .ok_or_else(|| format!("turn {turn} is not in the log"))?;
    let entry: serde_json::Value =
        serde_json::from_str(line).map_err(|_| format!("turn {turn} is not json"))?;
    let entry = entry
        .as_object()
        .ok_or_else(|| format!("turn {turn} is not an object"))?;
    let text = match entry.get("text") {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(value) if !value.is_null() => value.to_string(),
        _ => String::new(),
    };
    Ok(ResolvedCitation {
        id: citation.id.clone(),
        text,
        state: entry
            .get("state")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    })
}

/// The commit message of a `git` store's object, and whether the commit has landed.
fn read_commit(root: &Path, citation: &Citation) -> Result<ResolvedCitation, String> {
    if !git(root, &["cat-file", "-e", &citation.id]) {
        return Err(format!("git object {} not found", citation.id));
    }
    let text = git_output(root, &["log", "-1", "--format=%B", &citation.id])
        .unwrap_or_default()
        .trim()
        .to_string();
    let landed = git(root, &["merge-base", "--is-ancestor", &citation.id, "HEAD"]);
    Ok(ResolvedCitation {
        id: citation.id.clone(),
        text,
        state: Some(if landed { "merged" } else { "dangling" }.to_string()),
    })
}

/// Whether a `git` command succeeded in the project's repository. A git that cannot be started
/// failed the check, which is what the caller is told by the answer it produces.
fn git(root: &Path, arguments: &[&str]) -> bool {
    Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .is_ok_and(|output| output.status.success())
}

/// What a `git` command printed, when it succeeded.
fn git_output(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::{cite_in, stores_at, Citation, Store};
    use std::fs;
    use std::path::{Path, PathBuf};

    const LOG: &str = "\
{\"turn\": 1, \"text\": \"The refusal names the directory it refused.\", \"state\": \"done\"}
{\"turn\": 2, \"text\": \"A second turn, for the citation that asks for it.\"}
not json at all
\"a turn that is a string, not an object\"
";

    /// A project with one declared `file` store and the log that lives in it.
    fn project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("veridoc-cite-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        write(&root.join(".veridoc/config.json"), CONFIG);
        write(&root.join(".veridoc/citations/probe-refusal.jsonl"), LOG);
        root
    }

    const CONFIG: &str = r#"{
  "protocol": "veridoc/1",
  "plugin": { "command": ["target/release/veridoc-plugin-rust"] },
  "citationStores": [
    { "name": "session", "kind": "file", "path": ".veridoc/citations" },
    { "name": "history", "kind": "git" },
    { "name": "tracker", "kind": "jira", "path": "https://tracker.example" }
  ]
}
"#;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().expect("a config file has a parent")).unwrap();
        fs::write(path, text).unwrap();
    }

    fn citation(store: &str, id: &str, turn: Option<u32>) -> Citation {
        Citation {
            store: store.to_string(),
            id: id.to_string(),
            turn,
        }
    }

    /// The host's config holds the whole run's configuration; the plugin reads the one list out
    /// of it and leaves the rest alone.
    #[test]
    fn the_declared_stores_come_from_the_config() {
        let root = project("stores");
        let declared = stores_at(&root.join(".veridoc/config.json"));
        let kinds: Vec<String> = declared
            .iter()
            .map(|store| format!("{}:{}", store.name, store.kind))
            .collect();
        assert_eq!(
            kinds,
            vec!["session:file", "history:git", "tracker:jira"],
            "the plugin reads citationStores and ignores the host's other keys"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_store_answers_the_turn_it_is_asked_for() {
        let root = project("file");
        let stores = stores_at(&root.join(".veridoc/config.json"));
        let answer = cite_in(
            &root,
            &stores,
            &[
                citation("session", "probe-refusal", None),
                citation("session", "probe-refusal", Some(2)),
                citation("session", "probe-refusal", Some(3)),
                citation("session", "probe-refusal", Some(4)),
                citation("session", "probe-refusal", Some(9)),
            ],
        );
        assert_eq!(answer.unresolved.len(), 3, "{:?}", answer.unresolved);
        let first = &answer.resolved[0];
        assert_eq!(first.id, "probe-refusal");
        assert_eq!(first.text, "The refusal names the directory it refused.");
        assert_eq!(first.state.as_deref(), Some("done"));
        assert_eq!(
            answer.resolved[1].text,
            "A second turn, for the citation that asks for it."
        );
        assert_eq!(
            answer.resolved[1].state, None,
            "a turn with no state has none"
        );
        let reasons: Vec<&str> = answer
            .unresolved
            .iter()
            .map(|gap| gap.reason.as_str())
            .collect();
        assert_eq!(
            reasons,
            vec![
                "turn 3 is not json",
                "turn 4 is not an object",
                "turn 9 is not in the log"
            ]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_store_the_project_never_declared_is_answered_by_name() {
        let root = project("undeclared");
        let answer = cite_in(&root, &[], &[citation("session", "probe-refusal", Some(1))]);
        assert!(answer.resolved.is_empty());
        assert_eq!(answer.unresolved[0].reason, "no store named session");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_log_names_the_directory_it_was_looked_for_in() {
        let root = project("missing");
        let stores = stores_at(&root.join(".veridoc/config.json"));
        let answer = cite_in(&root, &stores, &[citation("session", "absent", Some(1))]);
        assert!(answer.resolved.is_empty());
        assert_eq!(
            answer.unresolved[0].reason,
            "absent.jsonl is not in .veridoc/citations"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// A `git` store in a project that is no repository, and a kind this plugin cannot read:
    /// both name what was looked for rather than answering with an empty text.
    #[test]
    fn an_unreadable_answer_names_what_was_looked_for() {
        let root = project("kinds");
        let declared = stores_at(&root.join(".veridoc/config.json"));
        let git = Store {
            name: "history".to_string(),
            kind: "git".to_string(),
            path: None,
        };
        let tracker = declared
            .iter()
            .find(|store| store.kind == "jira")
            .expect("the config declares the tracker store");
        let answer = cite_in(
            &root,
            &[git.clone(), tracker.clone()],
            &[
                citation("history", "deadbeef", None),
                citation("tracker", "V-1", None),
            ],
        );
        assert!(answer.resolved.is_empty(), "{:?}", answer.resolved);
        assert_eq!(
            answer.unresolved[0].reason, "git object deadbeef not found",
            "a project that is no repository has no such object"
        );
        assert_eq!(
            answer.unresolved[1].reason,
            "store kind jira is not readable"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
