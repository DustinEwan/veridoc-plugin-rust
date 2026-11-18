//! `init`: create the files only this plugin can create — `veridoc.tree.json`, the document tree
//! `collect` reads, and the prose file the requested document names. The host writes neither, because
//! a host that authors a file it cannot parse hands the project a file that cannot be built
//! (`docs/decisions/0015` in the veridoc repository). A file that already exists is refused unless
//! `force` says otherwise, because a scaffold that overwrites an author's prose is worse than none.

use std::fs;
use std::path::{Component, Path, PathBuf};

use plugin_rpc::{INVALID_REQUEST, PLUGIN_ERROR};
use serde_json::{json, Value};

use crate::project::TREE_FILE;

/// Create the project's first two files. The error carries the JSON-RPC code with the reason: a
/// request with no root or no document is the host's mistake (-32600), while a file already on disk
/// is the plugin refusing to destroy the author's work (-32000).
pub fn init(params: &Value) -> Result<Value, (i64, String)> {
    let root = params["root"]
        .as_str()
        .filter(|root| !root.is_empty())
        .ok_or((INVALID_REQUEST, "init needs the project root".to_string()))?;
    let document = &params["document"];
    let path = document["path"]
        .as_str()
        .filter(|path| !path.is_empty())
        .ok_or((
            INVALID_REQUEST,
            "init needs the document's `.spec.md` path".to_string(),
        ))?;
    let audience = document["audience"]
        .as_str()
        .ok_or((INVALID_REQUEST, "init needs the document's audience".to_string()))?;
    let question = document["question"]
        .as_str()
        .ok_or((INVALID_REQUEST, "init needs the document's question".to_string()))?;
    let force = params["force"].as_bool().unwrap_or(false);

    let relative = Path::new(path);
    if relative.is_absolute() || relative.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err((
            INVALID_REQUEST,
            format!("the document path must stay inside the project: {path}"),
        ));
    }

    let root = Path::new(root);
    let concept = concept_for(root);
    let mut declared = json!({ "path": path, "audience": audience, "question": question });
    if let Some(not_applicable) = document["notApplicable"].as_str() {
        declared["notApplicable"] = json!(not_applicable);
    }
    let tree = json!({ "concepts": [concept], "documents": [declared] });
    let files: Vec<(PathBuf, String)> = vec![
        (
            root.join(TREE_FILE),
            format!(
                "{}\n",
                serde_json::to_string_pretty(&tree)
                    .map_err(|e| (PLUGIN_ERROR, format!("the document tree is not writable: {e}")))?
            ),
        ),
        (
            root.join(relative),
            format!(
                "<doc-section name=\"symbol-name\">\n\n## {question}\n\nWrite the prose here, and \
                 name a claim where a sentence states one: <doc-claim name=\"predicate\"/>.\nRename \
                 `symbol-name` and `predicate` to the subject and the predicate you declare in a \
                 `*.claims.json` sidecar.\n"
            ),
        ),
    ];

    let mut written = Vec::new();
    for (file, body) in files {
        if file.exists() && !force {
            return Err((
                PLUGIN_ERROR,
                format!("{} exists; pass force to replace it", file.display()),
            ));
        }
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                (
                    PLUGIN_ERROR,
                    format!("{} could not be created: {e}", parent.display()),
                )
            })?;
        }
        fs::write(&file, &body).map_err(|e| {
            (
                PLUGIN_ERROR,
                format!("{} could not be written: {e}", file.display()),
            )
        })?;
        written.push(
            file.strip_prefix(root)
                .unwrap_or(file.as_path())
                .display()
                .to_string(),
        );
    }
    Ok(json!({
        "written": written,
        "next": [
            format!(
                "declare the claims for a module in src/<module>.claims.json, naming the concept \
                 {concept:?}"
            ),
            "run `veridoc check`",
        ],
    }))
}

/// The project's own concept, from its directory name, so the first claim has a concept to name
/// before the author has chosen one.
fn concept_for(root: &Path) -> String {
    let slug: String = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "project".to_string()
    } else {
        trimmed.to_string()
    }
}
