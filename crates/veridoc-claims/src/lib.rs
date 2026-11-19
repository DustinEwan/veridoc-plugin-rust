//! The claim library a Rust project adds as a dev-dependency (`docs/decisions/0006`).
//!
//! A project's claims live in `src/<module>.doc.rs`, beside the code they describe, and a scenario
//! is declared with the claim it proves: its two step labels, and the names and types of the values
//! it returns. The project ships one harness target that calls [`harness`] with the claims of every
//! module it links, and the veridoc plugin builds that target and asks it two questions:
//!
//! ```text
//! <harness> --list              {"libraryVersion":"..","claims":[{..},..]}
//! <harness> --run <claim name>  {"values":{"exit_code":2},"satisfied":{"exit_code >= 0":true}}
//! ```
//!
//! `--list` answers the claim set, so there is no second copy of the claim data for a data file and
//! a binary to disagree about. `--run` answers one claim's scenario by the name `--list` reported,
//! which is `<subject>#<slug>` — the rule lives here, in [`claim_name`], because the harness has to
//! resolve the names it is given. The scenario's code is ordinary Rust in the project's own module,
//! so it calls the project directly: no action-name string and no fixture registry stands between
//! the claim and the call it makes.
//!
//! A scenario that cannot run prints `{"error":".."}` and exits non-zero. The plugin records that as
//! one claim's failure and keeps judging the rest (`0006:100-106`).
//!
//! This crate depends on `std` alone: the plugin builds it inside the author's project, where a
//! dependency download would make a documentation check depend on the network.

use std::process::ExitCode;

/// The channel a claim's evidence arrives on, exactly the five the host names.
#[derive(Clone, Copy)]
pub enum Kind {
    /// A precondition the subject states.
    Requires,
    /// A postcondition the subject states.
    Ensures,
    /// A property that holds throughout.
    Invariant,
    /// A fact pinned to a file in the repository.
    Witness,
    /// A fact quoted out of a citation store.
    Because,
}

impl Kind {
    fn wire(self) -> &'static str {
        match self {
            Kind::Requires => "requires",
            Kind::Ensures => "ensures",
            Kind::Invariant => "invariant",
            Kind::Witness => "witness",
            Kind::Because => "because",
        }
    }
}

/// What a claim is about: a named symbol in a named file, or a declared concept. The symbol's name
/// is the author's, never a path (`docs/decisions/0009`), and `from` is the project-relative file
/// that holds it.
pub enum Subject {
    Symbol {
        name: &'static str,
        from: &'static str,
    },
    Concept {
        name: &'static str,
    },
}

impl Subject {
    pub fn symbol(name: &'static str, from: &'static str) -> Self {
        Subject::Symbol { name, from }
    }

    pub fn concept(name: &'static str) -> Self {
        Subject::Concept { name }
    }

    /// The author's name for the thing, which is the left half of the claim's name.
    pub fn name(&self) -> &'static str {
        match self {
            Subject::Symbol { name, .. } | Subject::Concept { name } => name,
        }
    }

    fn write_json(&self, out: &mut String) {
        match self {
            Subject::Symbol { name, from } => {
                out.push_str("{\"kind\":\"symbol\",\"name\":");
                write_str(name, out);
                out.push_str(",\"from\":");
                write_str(from, out);
                out.push('}');
            }
            Subject::Concept { name } => {
                out.push_str("{\"kind\":\"concept\",\"name\":");
                write_str(name, out);
                out.push('}');
            }
        }
    }
}

/// The value a check compares against.
pub enum Exp {
    Text(String),
    Number(f64),
    Flag(bool),
}

impl From<&str> for Exp {
    fn from(value: &str) -> Self {
        Exp::Text(value.to_string())
    }
}

impl From<String> for Exp {
    fn from(value: String) -> Self {
        Exp::Text(value)
    }
}

impl From<i32> for Exp {
    fn from(value: i32) -> Self {
        Exp::Number(value as f64)
    }
}

impl From<i64> for Exp {
    fn from(value: i64) -> Self {
        Exp::Number(value as f64)
    }
}

impl From<usize> for Exp {
    fn from(value: usize) -> Self {
        Exp::Number(value as f64)
    }
}

impl From<f64> for Exp {
    fn from(value: f64) -> Self {
        Exp::Number(value)
    }
}

impl From<bool> for Exp {
    fn from(value: bool) -> Self {
        Exp::Flag(value)
    }
}

impl Exp {
    fn write_json(&self, out: &mut String) {
        match self {
            Exp::Text(text) => write_str(text, out),
            Exp::Number(n) => write_num(*n, out),
            Exp::Flag(on) => out.push_str(if *on { "true" } else { "false" }),
        }
    }
}

/// One node of an assertion tree. The engine evaluates this; it is data, not code, which is the
/// reason a scenario must declare the values it produces (`0006:87-90`).
pub enum Assert {
    All(Vec<Assert>),
    Any(Vec<Assert>),
    Not(Box<Assert>),
    /// `{"obs":obs,"is":{"equals":is}}`
    Eq {
        obs: &'static str,
        is: Exp,
    },
    /// `{"obs":obs,"is":{"contains":part}}`
    Has {
        obs: &'static str,
        part: Exp,
    },
    /// A predicate the plugin settles. `source` is the predicate text the author wrote.
    Satisfies {
        obs: &'static str,
        source: &'static str,
        accepts: Option<Exp>,
        rejects: Option<Exp>,
    },
}

impl Assert {
    pub fn all(claims: impl IntoIterator<Item = Assert>) -> Self {
        Assert::All(claims.into_iter().collect())
    }

    pub fn any(claims: impl IntoIterator<Item = Assert>) -> Self {
        Assert::Any(claims.into_iter().collect())
    }

    // The assertion tree's node, not the trait: `!assert` over a claim reads as a negated claim,
    // and the wire shape this builds is `{"not": ...}`.
    #[allow(clippy::should_implement_trait)]
    pub fn not(inner: Assert) -> Self {
        Assert::Not(Box::new(inner))
    }

    pub fn eq(obs: &'static str, is: impl Into<Exp>) -> Self {
        Assert::Eq { obs, is: is.into() }
    }

    pub fn has(obs: &'static str, part: impl Into<Exp>) -> Self {
        Assert::Has {
            obs,
            part: part.into(),
        }
    }

    pub fn satisfies(obs: &'static str, source: &'static str) -> Self {
        Assert::Satisfies {
            obs,
            source,
            accepts: None,
            rejects: None,
        }
    }

    /// The witness pair that settles a predicate static analysis cannot.
    pub fn with_witness(mut self, accepts: impl Into<Exp>, rejects: impl Into<Exp>) -> Self {
        if let Assert::Satisfies { .. } = &mut self {
            let (obs, source) = match &self {
                Assert::Satisfies { obs, source, .. } => (*obs, *source),
                _ => unreachable!(),
            };
            self = Assert::Satisfies {
                obs,
                source,
                accepts: Some(accepts.into()),
                rejects: Some(rejects.into()),
            };
        }
        self
    }

    fn write_json(&self, out: &mut String) {
        match self {
            Assert::All(children) => write_list("all", children, out),
            Assert::Any(children) => write_list("any", children, out),
            Assert::Not(child) => {
                out.push_str("{\"not\":");
                child.write_json(out);
                out.push('}');
            }
            Assert::Eq { obs, is } => {
                out.push_str("{\"obs\":");
                write_str(obs, out);
                out.push_str(",\"is\":{\"equals\":");
                is.write_json(out);
                out.push_str("}}");
            }
            Assert::Has { obs, part } => {
                out.push_str("{\"obs\":");
                write_str(obs, out);
                out.push_str(",\"is\":{\"contains\":");
                part.write_json(out);
                out.push_str("}}");
            }
            Assert::Satisfies {
                obs,
                source,
                accepts,
                rejects,
            } => {
                out.push_str("{\"obs\":");
                write_str(obs, out);
                out.push_str(",\"is\":{\"satisfies\":{\"source\":");
                write_str(source, out);
                if let Some(accepts) = accepts {
                    out.push_str(",\"accepts\":");
                    accepts.write_json(out);
                }
                if let Some(rejects) = rejects {
                    out.push_str(",\"rejects\":");
                    rejects.write_json(out);
                }
                out.push_str("}}}");
            }
        }
    }
}

fn write_list(key: &str, children: &[Assert], out: &mut String) {
    out.push('{');
    write_str(key, out);
    out.push_str(":[");
    for (at, child) in children.iter().enumerate() {
        if at > 0 {
            out.push(',');
        }
        child.write_json(out);
    }
    out.push_str("]}");
}

/// How a scenario's value is typed, which is what makes `contains` over a number a refusal rather
/// than a verdict that could never have passed.
#[derive(Clone, Copy)]
pub enum Type {
    String,
    Number,
    Boolean,
    Array,
    Object,
}

impl Type {
    fn wire(self) -> &'static str {
        match self {
            Type::String => "string",
            Type::Number => "number",
            Type::Boolean => "boolean",
            Type::Array => "array",
            Type::Object => "object",
        }
    }
}

/// A value a scenario produced.
pub enum Value {
    Text(String),
    Number(f64),
    Flag(bool),
    List(Vec<Value>),
    Map(Vec<(&'static str, Value)>),
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Value::Text(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::Text(value.to_string())
    }
}

impl From<i32> for Value {
    fn from(value: i32) -> Self {
        Value::Number(value as f64)
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Value::Number(value as f64)
    }
}

impl From<usize> for Value {
    fn from(value: usize) -> Self {
        Value::Number(value as f64)
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Value::Number(value)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Value::Flag(value)
    }
}

impl From<Vec<String>> for Value {
    fn from(values: Vec<String>) -> Self {
        Value::List(values.into_iter().map(Value::Text).collect())
    }
}

impl Value {
    fn write_json(&self, out: &mut String) {
        match self {
            Value::Text(text) => write_str(text, out),
            Value::Number(n) => write_num(*n, out),
            Value::Flag(on) => out.push_str(if *on { "true" } else { "false" }),
            Value::List(items) => {
                out.push('[');
                for (at, item) in items.iter().enumerate() {
                    if at > 0 {
                        out.push(',');
                    }
                    item.write_json(out);
                }
                out.push(']');
            }
            Value::Map(entries) => {
                out.push('{');
                for (at, (key, item)) in entries.iter().enumerate() {
                    if at > 0 {
                        out.push(',');
                    }
                    write_str(key, out);
                    out.push(':');
                    item.write_json(out);
                }
                out.push('}');
            }
        }
    }
}

/// What a scenario's code produced: the declared values, and, for each `satisfies` leaf in the
/// claim's assertion tree, whether that leaf's predicate holds.
pub struct Outcome {
    pub values: Vec<(&'static str, Value)>,
    /// One answer per `satisfies` leaf, keyed by the leaf's own `source` text — the string the
    /// author wrote, which is what the plugin matches an answer against.
    pub satisfied: Vec<(&'static str, bool)>,
}

impl Default for Outcome {
    fn default() -> Self {
        Self::new()
    }
}

impl Outcome {
    pub fn new() -> Self {
        Outcome {
            values: Vec::new(),
            satisfied: Vec::new(),
        }
    }

    /// Record one declared value. A name that the claim's `returns` list does not declare is
    /// dropped by the plugin, so the declaration stays the authority.
    pub fn value(mut self, name: &'static str, value: impl Into<Value>) -> Self {
        self.values.push((name, value.into()));
        self
    }

    /// Answer one `satisfies` leaf. `source` is the predicate text the leaf carries, which is not
    /// the claim's predicate: one claim's tree can ask about more than one predicate.
    pub fn satisfied(mut self, source: &'static str, holds: bool) -> Self {
        self.satisfied.push((source, holds));
        self
    }
}

/// The scenario beside the claim: the two step labels, the values it returns, and the code that
/// produces them. The labels are plain strings with no grammar in any language.
pub struct Scenario {
    pub given: &'static str,
    pub when: &'static str,
    pub returns: &'static [(&'static str, Type)],
    pub run: fn() -> Result<Outcome, String>,
}

/// The evidence a claim rests on.
pub enum Channel {
    /// A scenario ran and the assertion above decides it.
    Run(Assert),
    /// A file in the repository, pinned by its hash.
    Witness {
        path: &'static str,
        sha256: &'static str,
    },
    /// A turn quoted out of a citation store.
    Citation {
        store: &'static str,
        id: &'static str,
        turn: Option<u32>,
    },
}

impl Channel {
    /// A run claim: the scenario ran, and this assertion tree decides it.
    pub fn run(asserts: Assert) -> Self {
        Channel::Run(asserts)
    }

    fn write_json(&self, out: &mut String) {
        match self {
            Channel::Run(asserts) => {
                out.push_str("{\"asserts\":");
                asserts.write_json(out);
                out.push('}');
            }
            Channel::Witness { path, sha256 } => {
                out.push_str("{\"artefact\":{\"path\":");
                write_str(path, out);
                out.push_str(",\"sha256\":");
                write_str(sha256, out);
                out.push_str("}}");
            }
            Channel::Citation { store, id, turn } => {
                out.push_str("{\"citation\":{\"store\":");
                write_str(store, out);
                out.push_str(",\"id\":");
                write_str(id, out);
                if let Some(turn) = turn {
                    out.push_str(",\"turn\":");
                    out.push_str(&turn.to_string());
                }
                out.push_str("}}");
            }
        }
    }
}

/// One claim, as its module declares it.
pub struct Claim {
    pub kind: Kind,
    pub subject: Subject,
    pub concept: &'static str,
    pub predicate: &'static str,
    pub text: &'static str,
    pub channel: Channel,
    pub scenario: Option<Scenario>,
}

impl Claim {
    /// The claim's name: `<subject>#<slug>`, the rule `0006:111-119` makes normative. The harness
    /// is addressed by this name, and the plugin reports it as the claim's id.
    pub fn name(&self) -> String {
        claim_name(self.subject.name(), self.predicate)
    }

    fn write_json(&self, out: &mut String) {
        out.push_str("{\"kind\":");
        write_str(self.kind.wire(), out);
        out.push_str(",\"subject\":");
        self.subject.write_json(out);
        out.push_str(",\"concept\":");
        write_str(self.concept, out);
        out.push_str(",\"predicate\":");
        write_str(self.predicate, out);
        out.push_str(",\"text\":");
        write_str(self.text, out);
        out.push_str(",\"channel\":");
        self.channel.write_json(out);
        match &self.scenario {
            Some(scenario) => {
                out.push_str(",\"scenario\":{\"given\":");
                write_str(scenario.given, out);
                out.push_str(",\"when\":");
                write_str(scenario.when, out);
                out.push_str(",\"values\":[");
                for (at, (name, ty)) in scenario.returns.iter().enumerate() {
                    if at > 0 {
                        out.push(',');
                    }
                    out.push_str("{\"name\":");
                    write_str(name, out);
                    out.push_str(",\"type\":");
                    write_str(ty.wire(), out);
                    out.push('}');
                }
                out.push_str("]}");
            }
            None => out.push_str(",\"scenario\":null"),
        }
        out.push('}');
    }
}

/// Join the claim lists of the modules one harness answers for.
pub fn merge(modules: impl IntoIterator<Item = Vec<Claim>>) -> Vec<Claim> {
    modules.into_iter().flatten().collect()
}

/// A claim's name from its two parts. Kept here so the harness can resolve the name it is handed.
pub fn claim_name(subject: &str, predicate: &str) -> String {
    format!("{subject}#{}", slug(predicate, MAX_SLUG))
}

/// The longest predicate slug a name carries before it is cut at a word boundary.
const MAX_SLUG: usize = 48;

/// Lower-case, alphanumeric runs joined by `-`, cut at the last whole word that fits in `max`
/// bytes, so a cut name is always a prefix of the uncut one. A first word longer than `max` yields
/// no slug at all, which is what `hash.rs::slug` in the engine does, and the two must agree: the
/// engine binds the name the harness is later addressed by.
fn slug(text: &str, max: usize) -> String {
    let mut words: Vec<&str> = Vec::new();
    let lowered = text.to_lowercase();
    for run in lowered.split(|ch: char| !ch.is_alphanumeric()) {
        if !run.is_empty() {
            words.push(run);
        }
    }
    let mut kept = String::new();
    for word in words {
        let candidate = if kept.is_empty() {
            word.to_string()
        } else {
            format!("{kept}-{word}")
        };
        if candidate.len() > max {
            break;
        }
        kept = candidate;
    }
    kept
}

/// Answer the harness's two questions and return the process's exit code. A project's harness
/// target is one call to this.
pub fn harness(claims: Vec<Claim>) -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut out = String::new();
    match args.as_slice() {
        [flag] if flag == "--list" => {
            out.push_str("{\"libraryVersion\":");
            write_str(env!("CARGO_PKG_VERSION"), &mut out);
            out.push_str(",\"claims\":[");
            for (at, claim) in claims.iter().enumerate() {
                if at > 0 {
                    out.push(',');
                }
                claim.write_json(&mut out);
            }
            out.push_str("]}");
            print_one_line(&out);
            ExitCode::SUCCESS
        }
        [flag, name] if flag == "--run" => {
            match claims.iter().find(|claim| claim.name() == name.as_str()) {
                Some(claim) => match run_claim(claim) {
                    Ok(answer) => {
                        print_one_line(&answer);
                        ExitCode::SUCCESS
                    }
                    Err(reason) => {
                        out.push_str("{\"error\":");
                        write_str(&reason, &mut out);
                        out.push('}');
                        print_one_line(&out);
                        ExitCode::FAILURE
                    }
                },
                None => {
                    out.push_str("{\"error\":");
                    write_str(
                        &format!("no claim of this project is named {name}"),
                        &mut out,
                    );
                    out.push('}');
                    print_one_line(&out);
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("usage: veridoc harness --list | --run <claim name>");
            ExitCode::from(2)
        }
    }
}

/// Run one claim's scenario and render its answer. A scenario that returns an error, and a claim
/// with no scenario at all, both answer `{"error":..}` so the host can name the claim in the
/// verdict it records.
fn run_claim(claim: &Claim) -> Result<String, String> {
    let scenario = claim
        .scenario
        .as_ref()
        .ok_or_else(|| format!("{} declares no scenario", claim.name()))?;
    let outcome = (scenario.run)()?;
    let mut out = String::from("{\"values\":{");
    for (at, (name, value)) in outcome.values.iter().enumerate() {
        if at > 0 {
            out.push(',');
        }
        write_str(name, &mut out);
        out.push(':');
        value.write_json(&mut out);
    }
    out.push('}');
    if !outcome.satisfied.is_empty() {
        out.push_str(",\"satisfied\":{");
        for (at, (source, holds)) in outcome.satisfied.iter().enumerate() {
            if at > 0 {
                out.push(',');
            }
            write_str(source, &mut out);
            out.push(':');
            out.push_str(if *holds { "true" } else { "false" });
        }
        out.push('}');
    }
    out.push('}');
    Ok(out)
}

fn print_one_line(text: &str) {
    use std::io::Write;

    let stdout = std::io::stdout();
    let mut sink = stdout.lock();
    let _ = sink.write_all(text.as_bytes()).and_then(|()| {
        sink.write_all(b"\n")?;
        sink.flush()
    });
}

/// Write one JSON string, escaping what JSON requires.
fn write_str(text: &str, out: &mut String) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
}

/// Write one JSON number. A whole float is written without a fraction so a value the author wrote
/// as `2` comes back as `2`.
fn write_num(value: f64, out: &mut String) {
    if value.fract() == 0.0 && value.abs() < 9.0e15 {
        out.push_str(&(value as i64).to_string());
    } else {
        out.push_str(&value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_predicate_is_cut_at_a_word_boundary_and_a_long_word_is_cut_away() {
        // `hash.rs::slug` in the engine cuts the same way, and the engine binds the name the
        // harness is later addressed by, so a divergence here is a claim the harness cannot find.
        let name = claim_name(
            "probe",
            "refuses a directory that holds no config.json today",
        );
        assert_eq!(name, "probe#refuses-a-directory-that-holds-no-config-json");
        let long_word = "a".repeat(MAX_SLUG + 10);
        assert_eq!(claim_name("probe", &long_word), "probe#");
    }

    #[test]
    fn a_claim_is_written_in_the_shape_the_plugin_reads() {
        let claim = Claim {
            kind: Kind::Ensures,
            subject: Subject::symbol("probe", "src/probe.rs"),
            concept: "probe-scan",
            predicate: "refuses-missing-config",
            text: "probe refuses a directory that holds no \"config.json\".\n",
            channel: Channel::run(Assert::all([
                Assert::eq("exit_code", 2),
                Assert::satisfies("message", "message.len() > 0"),
            ])),
            scenario: Some(Scenario {
                given: "a directory that holds no config.json",
                when: "probe is called against it",
                returns: &[("exit_code", Type::Number)],
                run: || Ok(Outcome::new().value("exit_code", 2)),
            }),
        };
        let mut out = String::new();
        claim.write_json(&mut out);
        assert!(
            out.contains(r#""subject":{"kind":"symbol","name":"probe","from":"src/probe.rs"}"#),
            "{out}"
        );
        assert!(out.contains(r#"no \"config.json\".\n"#), "{out}");
        assert!(out.contains(r#""is":{"equals":2}"#), "{out}");
        assert!(
            out.contains(r#""satisfies":{"source":"message.len() > 0"}"#),
            "{out}"
        );
        assert!(
            out.contains(r#"{"name":"exit_code","type":"number"}"#),
            "{out}"
        );
    }

    #[test]
    fn a_whole_number_is_written_without_a_fraction() {
        let mut out = String::new();
        Value::Number(2.0).write_json(&mut out);
        assert_eq!(out, "2");
        let mut half = String::new();
        Value::Number(0.5).write_json(&mut half);
        assert_eq!(half, "0.5");
    }
}
