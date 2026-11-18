//! Finding a declaration in a Rust source file, and the signature text a reader sees.
//!
//! The scan is deliberately plain: an item header is a line whose first token, after any
//! qualifiers, is one of the item keywords, and the declaration text runs from that line to the
//! body's opening brace or the statement's semicolon. That returns the text the author wrote,
//! which is what a claim's signature is for. A symbol introduced by a macro is not found, and
//! the caller answers that reference as unresolved with the reason rather than inventing a
//! signature.

use std::fs;
use std::path::Path;

/// One item found in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub name: String,
    /// The item keyword: `fn`, `struct`, `enum`, `trait`, `const`, `static`, `type`, `mod`.
    pub kind: &'static str,
    /// The declaration header with the body removed, whitespace collapsed to one space.
    pub signature: String,
    /// Whether the header carries `pub`.
    pub public: bool,
    /// The 1-based line the header starts on.
    pub line: usize,
}

const KEYWORDS: [&str; 8] = ["fn", "struct", "enum", "trait", "const", "static", "type", "mod"];

/// Read `path` and return every item declaration in it.
pub fn declarations_in(path: &Path) -> Result<Vec<Declaration>, String> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("{} could not be read: {e}", path.display()))?;
    Ok(declarations(&text))
}

/// Every item declaration in `text`, in the order they appear.
pub fn declarations(text: &str) -> Vec<Declaration> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    let mut start = 0;
    while start < lines.len() {
        let Some(header) = header_of(lines[start]) else {
            start += 1;
            continue;
        };
        // A declaration ends at the body's brace or at the statement's semicolon, and either
        // may sit on a later line when the signature wraps.
        let mut parts = Vec::new();
        let mut next = start;
        while next < lines.len() {
            let line = lines[next];
            let cut = line
                .find('{')
                .or_else(|| line.find(';'))
                .filter(|at| !in_comment(line, *at));
            match cut {
                Some(at) => parts.push(&line[..at]),
                None => parts.push(line),
            }
            next += 1;
            if cut.is_some() {
                break;
            }
        }
        let signature = parts
            .iter()
            .map(|part| part.trim())
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        found.push(Declaration {
            name: header.name,
            kind: header.kind,
            signature,
            public: header.public,
            line: start + 1,
        });
        start = next;
    }
    found
}

struct Header {
    name: String,
    kind: &'static str,
    public: bool,
}

/// The item header on one line, or nothing when the line does not start an item.
fn header_of(line: &str) -> Option<Header> {
    let mut rest = line.trim_start();
    let mut public = false;
    loop {
        let stripped = rest
            .strip_prefix("pub(crate) ")
            .or_else(|| rest.strip_prefix("pub(super) "))
            .or_else(|| rest.strip_prefix("pub(in "))
            .or_else(|| rest.strip_prefix("pub "))
            .or_else(|| rest.strip_prefix("async "))
            .or_else(|| rest.strip_prefix("unsafe "))
            .or_else(|| rest.strip_prefix("default "));
        let Some(next) = stripped else { break };
        if rest.starts_with("pub") {
            public = true;
        }
        rest = next;
    }
    for keyword in KEYWORDS {
        let Some(after) = rest.strip_prefix(keyword) else {
            continue;
        };
        let Some(after) = after.strip_prefix(char::is_whitespace) else {
            continue;
        };
        // `impl<T> Trait for Type` and `struct S<T>` put generics between the keyword and the
        // name, so the name is the first token that is not a generic parameter list.
        let name = after
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .find(|token| {
                !token.is_empty() && !token.chars().next().is_some_and(|c| c.is_ascii_digit())
            })
            .unwrap_or_default()
            .to_string();
        if name.is_empty() {
            return None;
        }
        return Some(Header {
            name,
            kind: keyword,
            public,
        });
    }
    None
}

/// Whether `at` sits inside a `//` comment, so a brace in prose does not end a declaration.
fn in_comment(line: &str, at: usize) -> bool {
    line.find("//").is_some_and(|start| start < at)
}

#[cfg(test)]
mod tests {
    use super::declarations;

    const SOURCE: &str = "\
use std::path::Path;

/// Reads the config.
pub fn parse_config(dir: &Path) -> Result<Config, Problem> {
    let _ = 1;
    Ok(Config)
}

fn helper(a: u8) -> bool {
    a > 1
}

pub struct ScannerConfig {
    pub root: String,
}

pub enum Problem {
    Missing,
}

pub const EXIT_USAGE: i32 = 2;

// fn commented_out() {}
";

    #[test]
    fn finds_each_item_with_its_header_text_and_line() {
        let found = declarations(SOURCE);
        let names: Vec<&str> = found.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "parse_config",
                "helper",
                "ScannerConfig",
                "Problem",
                "EXIT_USAGE"
            ]
        );
        let parse_config = &found[0];
        assert_eq!(parse_config.line, 4);
        assert!(parse_config.public);
        assert_eq!(
            parse_config.signature,
            "pub fn parse_config(dir: &Path) -> Result<Config, Problem>"
        );
        assert!(!found[1].public, "helper is private");
        assert_eq!(found[4].kind, "const");
    }

    #[test]
    fn a_signature_that_wraps_is_joined_into_one_line() {
        let source = "pub fn scan(\n    dir: &Path,\n) -> Vec<File> {\n    vec![]\n}\n";
        let found = declarations(source);
        assert_eq!(found[0].line, 1);
        assert_eq!(
            found[0].signature,
            "pub fn scan( dir: &Path, ) -> Vec<File>"
        );
    }
}
