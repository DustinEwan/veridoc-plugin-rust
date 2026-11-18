//! `surface`: the items the crate puts in front of a caller, so a run can name the ones no
//! claim covers.
//!
//! An item is on the surface when its header carries `pub` with no scope on it. `pub(crate)`,
//! `pub(super)`, and `pub(in path)` are not: the header says who may reach them, and it does not
//! say the caller outside the crate. A private item is not on the surface either, and the run
//! that reports uncovered symbols does not report them.
//!
//! The ids are the ones `resolve` answers with, computed by the same `referent_id` over the same
//! project-relative path, because the host matches a surface entry against the referent id of a
//! claim: an id computed differently would report every public symbol as uncovered.

use std::collections::HashSet;
use std::path::Path;

use serde::Serialize;

use crate::decl::{self, Visibility};
use crate::ids::referent_id;
use crate::project;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceReferent {
    pub referent_id: String,
    pub name: String,
    pub public: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceResult {
    pub referents: Vec<SurfaceReferent>,
}

/// Every public item under the declared source roots, in path order, each named once.
pub fn surface(root: &Path) -> SurfaceResult {
    let mut referents = Vec::new();
    let mut seen = HashSet::new();
    for file in project::sources(root) {
        let relative = project::relative(root, &file);
        let Ok(declarations) = decl::declarations_in(&file) else {
            continue;
        };
        for declaration in declarations {
            if declaration.visibility != Visibility::Public {
                continue;
            }
            let id = referent_id(&relative, &declaration.name);
            if seen.insert(id.clone()) {
                referents.push(SurfaceReferent {
                    referent_id: id,
                    name: declaration.name,
                    public: true,
                });
            }
        }
    }
    SurfaceResult { referents }
}

#[cfg(test)]
mod tests {
    use super::surface;
    use crate::ids::referent_id;
    use crate::resolve::resolve;
    use std::fs;
    use std::path::{Path, PathBuf};

    const LIB_RS: &str = "\
//! The crate's surface.
pub mod config;

pub use config::{parse_config, ScannerConfig};

pub struct Report {
    pub lines: usize,
}

pub(crate) fn shared() {}

pub(super) fn parent_only() {}

fn hidden() {}

pub enum Mode {
    Strict,
}
";

    const CONFIG_RS: &str = "\
pub fn parse_config(dir: &Path) -> Result<Config, String> {
    Ok(Config)
}

fn normalise(name: &str) -> String {
    name.trim().to_string()
}
";

    fn project() -> PathBuf {
        let root = std::env::temp_dir().join(format!("veridoc-surface-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        write(&root.join("src/lib.rs"), LIB_RS);
        write(&root.join("src/config.rs"), CONFIG_RS);
        root
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().expect("a source file has a parent")).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn the_surface_is_the_public_items_with_the_ids_resolve_reports() {
        let root = project();
        let listed = surface(&root);
        let named: Vec<(&str, &str)> = listed
            .referents
            .iter()
            .map(|entry| (entry.name.as_str(), entry.referent_id.as_str()))
            .collect();
        let expected = [
            ("parse_config", "src/config.rs"),
            ("config", "src/lib.rs"),
            ("Report", "src/lib.rs"),
            ("Mode", "src/lib.rs"),
        ]
        .map(|(name, file)| (name, referent_id(file, name)));
        let expected: Vec<(&str, &str)> = expected
            .iter()
            .map(|(name, id)| (*name, id.as_str()))
            .collect();
        assert_eq!(
            named, expected,
            "path order, one entry per item, nothing private"
        );
        assert!(listed.referents.iter().all(|entry| entry.public));
        for absent in ["shared", "parent_only", "hidden", "normalise"] {
            assert!(
                !named.iter().any(|(name, _)| *name == absent),
                "{absent} is not on the surface"
            );
        }
        // The id a claim carries is the one `resolve` answers with; a surface entry computed
        // differently would report the symbol as uncovered in its own project.
        let settled = resolve(&root, &["src/config.rs|parse_config".to_string()], &[]);
        assert_eq!(
            settled.resolved[0].referent_id, named[0].1,
            "surface and resolve agree on the id"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
