// The document tree, created by `veridoc init` through the plugin. Edit it; do not regenerate it.

/// The concepts and the documents this project declares, and the optional vocabulary. The plugin reads this item; `cargo` does not compile this file, because the index sits at the project root rather than under `src/`.
pub const VERIDOC_INDEX: &str = r##"{
  "concepts": { "project-scan": "docs/architecture.spec.md" },
  "documents": [
    {
      "path": "docs/architecture.spec.md",
      "audience": "a developer extending the scanner",
      "question": "What does the scanner refuse, and on what evidence?"
    }
  ]
}
"##;
