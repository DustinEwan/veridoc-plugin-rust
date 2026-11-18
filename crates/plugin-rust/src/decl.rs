//! Finding a declaration in a Rust source file, and the text a reader sees in it.
//!
//! The scan is deliberately plain: an item header is a line whose first token, after any
//! qualifiers, is one of the item keywords, and the declaration text runs from that line to the
//! body's opening brace or the statement's semicolon. That returns the text the author wrote,
//! which is what a claim's signature is for, and what `derivable` reads its facts out of. A
//! symbol introduced by a macro is not found, and the caller answers that reference as
//! unresolved with the reason rather than inventing a signature.
//!
//! Two limits are the price of reading text instead of parsing it. A brace inside a raw string
//! that is unbalanced on its own line moves a body's end, and a `mod` whose items are inline
//! reports no children of its own beyond the ones the scan finds at any indent. Neither changes
//! what a header states, which is all `resolve` and `derivable` answer from.

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
    /// Whether the header carries `pub`, in any form. `Visibility` says which.
    pub public: bool,
    /// The visibility the header states.
    pub visibility: Visibility,
    /// The 1-based line the header starts on.
    pub line: usize,
    /// The text between the item's braces, when it has braces. A `const`, a `type`, and a
    /// trait's `fn` signature have none.
    pub body: Option<Body>,
}

/// The visibility an item header states, which is what decides whether the item is on the
/// crate's surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Visibility {
    /// `pub`: reachable from outside the crate.
    Public,
    /// `pub(crate)`, `pub(super)`, or `pub(in path)`, with the scope the header names.
    Restricted(String),
    /// No `pub` on the header.
    Private,
}

impl Visibility {
    /// The visibility as a reader would name it: the header's own spelling, or `private` for
    /// the absence of one.
    pub fn text(&self) -> &str {
        match self {
            Visibility::Public => "pub",
            Visibility::Restricted(scope) => scope,
            Visibility::Private => "private",
        }
    }
}

/// The inside of an item's braces, kept with the line the opening brace sits on so a fact read
/// out of it can name the line the author wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Body {
    /// The 1-based line of the opening brace.
    pub line: usize,
    /// The text after the opening brace and before the closing one, line breaks kept.
    pub text: String,
}

impl Body {
    /// The body's lines, each paired with the file line it sits on.
    pub fn lines(&self) -> impl Iterator<Item = (usize, &str)> {
        self.text
            .lines()
            .enumerate()
            .map(move |(index, line)| (self.line + index, line))
    }
}

const KEYWORDS: [&str; 8] = [
    "fn", "struct", "enum", "trait", "const", "static", "type", "mod",
];

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
        let mut opened: Option<(usize, usize)> = None;
        while next < lines.len() {
            let line = lines[next];
            let cut = cut_of(line);
            match cut {
                Some((at, '{')) => {
                    parts.push(&line[..at]);
                    opened = Some((next, at));
                }
                Some((at, _)) => parts.push(&line[..at]),
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
        let body = opened.and_then(|(line, at)| body_of(&lines, line, at));
        found.push(Declaration {
            name: header.name,
            kind: header.kind,
            signature,
            public: header.visibility != Visibility::Private,
            line: start + 1,
            visibility: header.visibility,
            body,
        });
        start = next;
    }
    found
}

impl Declaration {
    /// The parameters a caller of this item passes, one entry per top-level comma of the
    /// header's parameter list, or nothing when the item is not a function. `self` is not one
    /// of them: it is the receiver, and a reader does not count it among the arguments.
    pub fn params(&self) -> Option<Vec<String>> {
        if self.kind != "fn" {
            return None;
        }
        let open = self.signature.find('(')?;
        let close = matching(&self.signature, open)?;
        let mut names = split_top(&self.signature[open + 1..close]);
        if names.first().is_some_and(|first| is_receiver(first)) {
            names.remove(0);
        }
        Some(names)
    }

    /// The return type the header declares, or nothing when it declares none, which is `()`. A
    /// `where` clause is not part of the type, so it is dropped.
    pub fn return_type(&self) -> Option<String> {
        if self.kind != "fn" {
            return None;
        }
        let open = self.signature.find('(')?;
        let close = matching(&self.signature, open)?;
        let tail = &self.signature[close + 1..];
        let arrow = tail.find("->")?;
        let stated = &tail[arrow + 2..];
        let end = top_level_where(stated).unwrap_or(stated.len());
        let text = stated[..end].trim();
        (!text.is_empty()).then(|| text.to_string())
    }

    /// The variants an enum declares, in the order its body lists them. Any other item has
    /// none to report.
    pub fn variants(&self) -> Vec<String> {
        if self.kind != "enum" {
            return Vec::new();
        }
        let body = match &self.body {
            Some(body) => body.text.clone(),
            None => return Vec::new(),
        };
        split_top(&body)
            .into_iter()
            .filter_map(|entry| {
                let name = entry
                    .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .next()
                    .unwrap_or_default()
                    .to_string();
                (!name.is_empty()).then_some(name)
            })
            .collect()
    }
}

/// Whether a parameter entry is the receiver rather than an argument.
fn is_receiver(entry: &str) -> bool {
    let entry = entry.trim();
    entry == "self"
        || entry == "mut self"
        || entry == "&self"
        || entry == "&mut self"
        || entry.starts_with("self:")
}

struct Header {
    name: String,
    kind: &'static str,
    visibility: Visibility,
}

/// The item header on one line, or nothing when the line does not start an item.
fn header_of(line: &str) -> Option<Header> {
    let mut rest = line.trim_start();
    let mut visibility = Visibility::Private;
    loop {
        if visibility == Visibility::Private {
            if let Some((stated, tail)) = pub_qualifier(rest) {
                visibility = stated;
                rest = tail;
                continue;
            }
        }
        let Some(next) = rest
            .strip_prefix("async ")
            .or_else(|| rest.strip_prefix("unsafe "))
            .or_else(|| rest.strip_prefix("default "))
            // `const` is a qualifier only in front of `fn`; in front of a name it is the item
            // keyword, and `const EXIT_USAGE` is a constant, not a function named `fn`.
            .or_else(|| {
                rest.strip_prefix("const ")
                    .filter(|rest| rest.starts_with("fn "))
            })
        else {
            break;
        };
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
            visibility,
        });
    }
    None
}

/// The `pub` qualifier at the head of `line`, with the visibility it states and the text that
/// follows it. A word that merely starts with `pub` is not a qualifier, so it needs the space
/// or the scope that follows it.
fn pub_qualifier(line: &str) -> Option<(Visibility, &str)> {
    let after = line.strip_prefix("pub")?;
    let (scope, tail) = match after.strip_prefix('(') {
        Some(scope) => {
            let end = scope.find(')')?;
            (Some(&scope[..end]), &scope[end + 1..])
        }
        None => (None, after),
    };
    let tail = tail.strip_prefix(char::is_whitespace)?;
    let visibility = match scope {
        None => Visibility::Public,
        Some(scope) => Visibility::Restricted(format!("pub({scope})")),
    };
    Some((visibility, tail))
}

/// Whether `at` sits inside a `//` comment, so a brace in prose does not end a declaration.
fn in_comment(line: &str, at: usize) -> bool {
    line.find("//").is_some_and(|start| start < at)
}

/// Where a header ends on this line: the body's opening brace or the statement's semicolon,
/// whichever comes first outside a comment.
fn cut_of(line: &str) -> Option<(usize, char)> {
    let brace = line.find('{').filter(|at| !in_comment(line, *at));
    let semicolon = line.find(';').filter(|at| !in_comment(line, *at));
    match (brace, semicolon) {
        (Some(at), Some(other)) if at <= other => Some((at, '{')),
        (Some(_), Some(other)) => Some((other, ';')),
        (Some(at), None) => Some((at, '{')),
        (None, Some(other)) => Some((other, ';')),
        (None, None) => None,
    }
}

/// The body the `{` at `at` on `lines[line]` opens: everything until the brace that closes it,
/// with the line the opening brace sits on. Braces inside a string, a character literal, or a
/// comment do not count, so `format!("{} holds no config.json", path)` leaves the body where
/// the author closed it. Nothing is returned when the braces never close: a body the scanner
/// cannot end is a body it must not report.
fn body_of(lines: &[&str], line: usize, at: usize) -> Option<Body> {
    let mut text = String::new();
    let mut depth = 1i32;
    let mut index = line;
    let mut from = at + 1;
    while index < lines.len() {
        let text_of_line = lines[index];
        let mut closed: Option<usize> = None;
        for (offset, ch) in code_of(text_of_line) {
            if offset < from {
                continue;
            }
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        closed = Some(offset);
                        break;
                    }
                }
                _ => {}
            }
        }
        match closed {
            Some(end) => {
                text.push_str(&text_of_line[from..end]);
                return Some(Body {
                    line: line + 1,
                    text,
                });
            }
            None => {
                text.push_str(&text_of_line[from..]);
                text.push('\n');
                index += 1;
                from = 0;
            }
        }
    }
    None
}

/// The code a line holds, as its characters with their byte offsets: the `//` comment dropped,
/// and the inside of every string and character literal dropped so that only the opening quote
/// is kept. A fact read out of a body is read out of this, never out of prose.
pub fn code_of(line: &str) -> Vec<(usize, char)> {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut out = Vec::with_capacity(chars.len());
    let mut at = 0;
    while at < chars.len() {
        let (offset, ch) = chars[at];
        out.push((offset, ch));
        at = match ch {
            '/' if chars.get(at + 1).is_some_and(|(_, next)| *next == '/') => return out,
            '"' => past_literal(at + 1, &chars, '"'),
            '\'' if char_literal(line, offset) => past_literal(at + 1, &chars, '\''),
            _ => at + 1,
        };
    }
    out
}

/// The code a line holds, as text.
pub fn code(line: &str) -> String {
    code_of(line).into_iter().map(|(_, ch)| ch).collect()
}

/// The index just past the closing `quote` after `at`, where a backslash escapes the character
/// after it. An unterminated literal runs to the end of the line, which is where a Rust literal
/// ends when the author did not close it.
fn past_literal(mut at: usize, chars: &[(usize, char)], quote: char) -> usize {
    while at < chars.len() {
        match chars[at].1 {
            '\\' => at += 2,
            ch if ch == quote => return at + 1,
            _ => at += 1,
        }
    }
    at
}

/// Whether the `'` at `offset` opens a character literal rather than a lifetime, which is the
/// one place Rust text puts a quote in front of a name: `&'a str` is a lifetime, `'\n'` is a
/// character, and only the second one hides what follows it.
fn char_literal(line: &str, offset: usize) -> bool {
    let mut rest = line[offset + 1..].chars();
    match rest.next() {
        Some('\\') => rest.nth(1) == Some('\''),
        Some(_) => rest.next() == Some('\''),
        None => false,
    }
}

/// The offset of the bracket closing the one at `open`, or nothing when the header is cut off
/// before it.
fn matching(text: &str, open: usize) -> Option<usize> {
    let opener = text.as_bytes()[open] as char;
    let closer = match opener {
        '(' => ')',
        '<' => '>',
        '[' => ']',
        '{' => '}',
        _ => return None,
    };
    let mut depth = 0i32;
    for (offset, ch) in text.char_indices().skip(open) {
        match ch {
            c if c == opener => depth += 1,
            c if c == closer => {
                depth -= 1;
                if depth == 0 {
                    return Some(offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// The pieces of a type list, split at the commas that sit outside every bracket, so
/// `HashMap<String, u32>` is one piece and `a: u8, b: u8` is two. Each piece is trimmed, and an
/// empty one — the trailing comma of a wrapped list — is dropped.
fn split_top(text: &str) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (offset, ch) in text.char_indices() {
        match ch {
            '<' | '(' | '[' | '{' => depth += 1,
            '>' if !is_arrow(text, offset) => depth -= 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                push_piece(&mut pieces, &text[start..offset]);
                start = offset + 1;
            }
            _ => {}
        }
    }
    push_piece(&mut pieces, &text[start..]);
    pieces
}

fn push_piece(pieces: &mut Vec<String>, piece: &str) {
    let piece = piece.trim();
    if !piece.is_empty() {
        pieces.push(piece.to_string());
    }
}

/// The offset of a `where` clause that sits outside every bracket, which is where a declared
/// return type stops being a type.
fn top_level_where(text: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (offset, ch) in text.char_indices() {
        match ch {
            '<' | '(' | '[' | '{' => depth += 1,
            '>' if !is_arrow(text, offset) => depth -= 1,
            ')' | ']' | '}' => depth -= 1,
            'w' if depth == 0 && is_clause(text, offset) => return Some(offset),
            _ => {}
        }
    }
    None
}

/// Whether `where` begins at `offset` as its own word.
fn is_clause(text: &str, offset: usize) -> bool {
    let rest = &text[offset..];
    rest.starts_with("where")
        && rest[5..].chars().next().is_none_or(|c| c.is_whitespace())
        && text[..offset]
            .chars()
            .next_back()
            .is_none_or(|c| c.is_whitespace())
}

/// Whether the `>` at `offset` points rather than closes: the `>` of a `->` ends no generic
/// list, whatever the bracket depth says.
fn is_arrow(text: &str, offset: usize) -> bool {
    text[..offset].ends_with('-')
}

#[cfg(test)]
mod tests {
    use super::{code, declarations};

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

    #[test]
    fn the_visibility_a_header_states_is_kept() {
        let source = "\
pub fn open() {}
pub(crate) fn shared() {}
pub(super) fn parent() {}
pub(in crate::engine) fn inner() {}
fn hidden() {}
pub async fn awaited() {}
";
        let found = declarations(source);
        let stated: Vec<String> = found
            .iter()
            .map(|declaration| declaration.visibility.text().to_string())
            .collect();
        assert_eq!(
            stated,
            vec![
                "pub",
                "pub(crate)",
                "pub(super)",
                "pub(in crate::engine)",
                "private",
                "pub",
            ]
        );
        assert_eq!(found[5].name, "awaited", "async follows pub");
    }

    #[test]
    fn a_body_is_kept_with_the_lines_its_facts_sit_on() {
        let source = "\
pub fn probe(dir: &Path) -> Result<Config, String> {
    let path = dir.join(\"config.json\");
    if !path.is_file() {
        panic!(\"{} holds no config.json\", dir.display());
    }
    Ok(Config)
}
";
        let found = declarations(source);
        let body = found[0].body.as_ref().expect("a function body is kept");
        assert_eq!(body.line, 1, "the opening brace sits on line 1");
        let panics: Vec<usize> = body
            .lines()
            .filter(|(_, line)| code(line).contains("panic!("))
            .map(|(line, _)| line)
            .collect();
        assert_eq!(
            panics,
            vec![4],
            "the brace inside the format string is not the body's end"
        );
    }

    #[test]
    fn the_parameter_list_and_return_type_come_from_the_header() {
        let source = "\
pub fn read(dir: &Path, strict: bool) -> Result<Config, String> {
    Ok(Config)
}

impl Config {
    pub fn root(&self) -> &str {
        &self.path
    }

    pub fn pick<T: Clone>(item: T) -> Box<dyn Fn() -> T>
    where
        T: Send,
    {
        Box::new(move || item.clone())
    }

    pub fn touch(&mut self) {}
}
";
        let found = declarations(source);
        let read = &found[0];
        assert_eq!(
            read.params(),
            Some(vec!["dir: &Path".to_string(), "strict: bool".to_string()])
        );
        assert_eq!(
            read.return_type().as_deref(),
            Some("Result<Config, String>")
        );
        let root = &found[1];
        assert_eq!(root.name, "root");
        assert_eq!(
            root.params(),
            Some(Vec::new()),
            "the receiver is not a parameter a caller passes"
        );
        assert_eq!(root.return_type().as_deref(), Some("&str"));
        let pick = &found[2];
        assert_eq!(pick.params(), Some(vec!["item: T".to_string()]));
        assert_eq!(
            pick.return_type().as_deref(),
            Some("Box<dyn Fn() -> T>"),
            "the where clause is not part of the return type"
        );
        let touch = &found[3];
        assert_eq!(touch.return_type(), None, "no arrow declares ()");
    }

    #[test]
    fn an_enum_lists_the_variants_it_accepts() {
        let source = "\
pub enum Problem {
    Missing { dir: String },
    Locked(u32),
    Closed,
}

pub struct Config {
    pub root: String,
}
";
        let found = declarations(source);
        assert_eq!(found[0].variants(), vec!["Missing", "Locked", "Closed"]);
        assert_eq!(found[1].variants(), Vec::<String>::new());
    }

    /// A `const fn` is a function, and the facts a reader can see in it are the parameter list
    /// and the return type; a `const` name is not one, and has neither.
    #[test]
    fn a_const_function_is_a_function_and_a_constant_is_not() {
        let source = "\
pub const fn limit() -> u8 {
    2
}

pub const EXIT_USAGE: i32 = 2;
";
        let found = declarations(source);
        assert_eq!(found[0].name, "limit");
        assert_eq!(found[0].kind, "fn");
        assert_eq!(found[0].params(), Some(Vec::new()));
        assert_eq!(found[0].return_type().as_deref(), Some("u8"));
        assert_eq!(found[1].name, "EXIT_USAGE");
        assert_eq!(found[1].kind, "const");
        assert_eq!(found[1].params(), None);
    }
}
