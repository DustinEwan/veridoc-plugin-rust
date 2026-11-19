# veridoc-plugin-rust

This repository holds the [veridoc](https://github.com/DustinEwan/veridoc) plugin for Rust projects.
veridoc is a checker that reads the claims you write about your code, finds the code that each claim
is about, and runs the scenarios that would show the claim to be false. The plugin is the part that
reads your Rust: it parses the source, answers what a symbol's declaration says, and runs your
scenarios. Verdicts come from veridoc's engine, which does not read Rust.

The wire mechanism, the envelope, and the conformance rules come from
[pluginrpc](https://github.com/DustinEwan/pluginrpc), which is where the `prpc/1` framing is defined.
This repository owns the `veridoc/1` method vocabulary, the claim format for Rust, and the answers.

## What you add to your project

Five files, and every one of them is yours. Three of them are what the project declares — the index,
the claim modules, and the prose — and the other two are the host's configuration and the harness
target the plugin builds. Nothing in your library changes: the plugin only reads your source, and the
code a scenario runs is code you wrote beside the claim it belongs to.

| File | What it holds |
| --- | --- |
| `.veridoc/config.json` | The plugin command, the coverage floor, the scenario deadline, and the judge model |
| `index.doc.rs` | One `pub const VERIDOC_INDEX: &str` whose JSON is the concepts, each mapped to the document that declares it, and the document tree — and nothing else |
| `src/<module>.doc.rs` | The claims about that module, one module per source module, each claim carrying its own scenario |
| `docs/*.spec.md` | The prose, in `<doc-section>` sections that name claims with `<doc-claim>` |
| `examples/veridoc.rs` | The harness target: one call into the claim library, which the plugin builds and asks `--list` and `--run <claim>` |

`index.doc.rs` is Rust that nothing compiles, so a project keeps it in version control and the plugin
reads the one item out of it. There is no source-root list: the plugin walks every `src` directory of
every Cargo package under your project and prunes `target`, `.veridoc`, and `.git`, because a
project's layout is a property of its package manager rather than something you restate.

### The claims

You write the symbol and the predicate. The plugin records the referent, the file, and the line. The
claim's name is `<subject>#<predicate>`, computed by the claim library rather than by the host, so
the example below is `parse_config#rejects-missing-config` wherever a report, a `<doc-claim>`, or a
leaf id prints it. After you rename a symbol, the module still names the old one, and the run reports
it as unresolved instead of reading the sentence as still describing whatever now occupies the file.

```rust
use veridoc_claims::{Assert, Channel, Claim, Kind, Outcome, Scenario, Subject, Type};

/// The values this claim's assertions read, with the types they are checked against.
const VALUES: &[(&str, Type)] = &[("exit_code", Type::Number), ("message", Type::String)];

/// The scenario's world: the call, and the values it answers.
fn refusal() -> Result<Outcome, String> {
    match scanner::parse_config(std::path::Path::new("samples/empty-directory")) {
        Ok(_) => Err("parse_config accepted a directory that holds no config.json".to_string()),
        Err(reason) => Ok(Outcome::new().value("exit_code", 2).value("message", reason)),
    }
}

pub fn claims() -> Vec<Claim> {
    vec![Claim {
        kind: Kind::Ensures,
        subject: Subject::symbol("parse_config", "src/config.rs"),
        concept: "project-scan",
        predicate: "rejects-missing-config",
        text: "parse_config refuses a directory that holds no config.json, naming the directory.",
        channel: Channel::run(Assert::eq("exit_code", 2)),
        scenario: Some(Scenario {
            given: "a directory that holds no config.json",
            when: "parse_config is called against it",
            returns: VALUES,
            run: refusal,
        }),
    }]
}
```

A claim carries exactly one channel: an assertion tree, an artefact file with its hash, or a
citation. The scenario is not a channel: it belongs to the claim and sits beside it, so a claim with
an assertion tree and no scenario is judged by its prose and source alone. If a claim sets two
channels, the plugin stops and reports the claim, because the engine's error for that case arrives
later and points at the claim rather than at the line you wrote.

### The prose

A section opens with `<doc-section name="SUBJECT">` alone on a line and ends at the next opener or the end
of the file. A heading inside it is prose and means nothing to the engine. A `<doc-claim name="PREDICATE"/>`
written inside a sentence names one claim, and the claim it names is `SUBJECT#PREDICATE`: the subject half
from the opening element, the predicate half the slug the report prints for the claim. Both directions are
checked, so a reference that names no claim stops the run and so does a claim that no section names. The
subject is the name you wrote for the thing the prose is about — the symbol for a claim about a symbol, the
concept for a claim about a concept, so the example's two sections open with `parse_config` and
`project-scan`. The referent id the plugin hashes from the file path (`veridoc:referent:` plus twelve hex
digits) is what `resolve` reports and nothing more, so renaming the file that holds the code costs a rebuild
and moves no claim name.

A fact your code decides does not belong in prose you copy: `<doc-each source="claims">` on a line of
its own repeats the one block after it once per item the collection holds, and
`<doc-value field="name"/>` marks where each value lands. The collections are `claims`, `concepts`, and
`documents`, because those are the three things a project declares; a scenario's values are not a
collection here, because they belong to one claim and the report prints them with it. The authored file
keeps the placeholder and the engine never writes into it; the assembled values appear in the report,
and they are inside the hash that decides whether the section is re-judged. The collections and their
fields are in `../veridoc/docs/spec.md`.

### The scenario

The scenario belongs to the claim, so its code sits in the same `src/<module>.doc.rs` as the claim it
proves: `run` is a function that calls your code and answers the values that scenario declared. No
scenario id, action name, or fixture name travels anywhere, and your project keeps no scenario grammar
for a plugin to parse. Two claims that need the same world call the same function.

Rust binds a call target at link time, so this plugin cannot load your module the way a Node plugin
`import()`s one. Your project ships one harness target, `examples/veridoc.rs`, whose whole body is one
call into the claim library:

```rust
#[path = "../src/config.doc.rs"]
mod config_doc;

fn main() -> std::process::ExitCode {
    veridoc_claims::harness(veridoc_claims::merge([config_doc::claims()]))
}
```

The plugin builds that target once per run, into `.veridoc/target`, so a scenario build does not
invalidate your own incremental build, and asks it two questions:

```text
<harness> --list              {"libraryVersion":"0.1.0","claims":[{..},..]}
<harness> --run <claim name>  {"values":{"exit_code":2},"satisfied":{"<predicate>":true}}
```

`--list` is what `collect` reports, so the claim set is whatever your compiled claim modules say it
is, rather than a copy of them in a data file. `--run` takes the claim's name, because that is the
only address a scenario has. `satisfied` is optional, and its keys are the predicate texts an author
wrote in a `satisfies` check; the engine compares an `equals` or `contains` check against the values
itself, so most projects leave `satisfied` out.

The plugin still believes a harness that exits non-zero after printing its answer, because a refusal
inside your code is an observed value for the assertion to judge. Only a harness that answers nothing
is a failure, and the failure kind records which of the four causes in the profile applies: `build`
when cargo cannot compile the target, `fixture` when the scenario's own world or the assets it reads
are not there, `deadline` when the target answers nothing within `timeoutMs`, and `plugin` when the
claim carries no scenario or a `satisfies` leaf has no answer.

## Capabilities

The handshake reports eight capabilities, one for every method of the profile, and every one of
them is answered by this build. A method whose capability is absent is answered `-32601`, because
the profile fixes what an absent capability costs and an invented answer is not in that list.

| Capability | Status | What the absence costs you |
| --- | --- | --- |
| `initialization` | implemented | without it `veridoc init` writes only `.veridoc/config.json` and tells you to create `index.doc.rs` yourself |
| `collect` | implemented | without it there is no build at all |
| `resolution` | implemented | without it the plugin cannot be used |
| `derivability` | implemented | the redundancy ban is skipped, so a claim a declaration already states is kept |
| `bivalence` | implemented | every checkable claim becomes an error rather than a pass |
| `execution` | implemented | scenarios are bound but never run |
| `citation` | implemented | a citation-channel claim cannot be answered |
| `surface` | implemented | the report says uncovered symbols are unknown |

`bivalence` answers structurally. A tree of `equals` and `contains` checks is bivalent, because one
side comes from a run and the other from you. A tree with no leaf is not, because nothing in it can
fail. A tree with a `satisfies` leaf is answered `false` with the reason, because deciding that leaf
would mean settling a Rust predicate's two outcomes without running them.

### What `derivable`, `surface`, and `cite` answer

`derivable` reports the facts a reader can already see in one declaration, and only those: the
visibility the header carries (`pub`, `pub(crate)`, or private), the parameter list and the declared
return type as the header spells them, the variants an enum accepts, and each `panic!` or
`unreachable!` the body states, with the line it sits on, plus the first `?` that hands a failure up
when the header declares a `Result` or an `Option` to hand it to. Nothing the compiler would infer
is reported, so a claim about a type you never wrote is never rejected for restating it. A referent
id the project declares nothing behind answers no facts, and a concept id answers the document the
index names as the one that declares it.

`surface` lists every item whose header says `pub`, with the same referent id `resolve` reports for
it, so the run can tell you which public symbol no claim covers. `pub(crate)`, `pub(super)`,
`pub(in path)`, and private items are not on the surface: the header says who may reach them, and it
does not say a caller outside the crate.

`cite` reads the two store kinds of the profile out of `citationStores` in `.veridoc/config.json`,
the file the host names in `VERIDOC_CONFIG`:

```json
{ "citationStores": [{ "name": "session", "kind": "file", "path": ".veridoc/citations" }] }
```

A `file` store is a directory of `<id>.jsonl` logs read against your project root, and turn `n` of
the log is its `n`th line: `{ "turn": 1, "text": "…", "state": "done" }`. A `git` store reads the
repository your project lives in: the answer is the commit message, and its `state` is `merged` or
`dangling`. A store you never declared, a log that is not there, and a kind this plugin cannot read
are each reported with the reason naming what was looked for, and the run reports that finding
rather than passing the claim.

## Running it

```sh
cargo build --release
veridoc check --project examples/rust-scanner
```

`examples/rust-scanner` is a complete adopting project: claims that carry their own scenarios, an
artefact claim, the sample directories its scenarios read, and the harness target. To see the two
methods that the engine calls first, without a full run:

```sh
veridoc probe-plugin --project examples/rust-scanner
```

The plugin passes the mechanism's core rules:

```sh
cargo run --manifest-path ../pluginrpc/Cargo.toml --bin prpc-core -- \
  ../veridoc/veridoc-1.profile.json veridoc-plugin.json
```

The whole profile is driven against this checkout by the host's own conformance suite, whose
fixture project is rendered by `crates/plugin-rust/src/bin/veridoc-conformance-emitter.rs`:

```sh
veridoc validate-plugin .
```

## Layout

| Path | What it is |
| --- | --- |
| `crates/plugin-rust/src/main.rs` | The `prpc/1` server: the method vocabulary, the handshake, the refusals |
| `crates/plugin-rust/src/project.rs` | The index, the claim modules, and the `collect` answer |
| `crates/plugin-rust/src/decl.rs` | The declaration scan: headers, visibility, bodies, and the parameter lists `resolve` and `derivable` read |
| `crates/plugin-rust/src/resolve.rs` | Reference keys, referent ids, and signatures |
| `crates/plugin-rust/src/derivable.rs` | The facts a declaration states, and the referent id that finds it |
| `crates/plugin-rust/src/cite.rs` | The `file` and `git` citation stores, and the reasons a citation goes unanswered |
| `crates/plugin-rust/src/surface.rs` | The `pub` items, with the ids `resolve` reports |
| `crates/plugin-rust/src/run.rs` | The harness build, the claim-addressed scenario run, the deadline, and the failure kinds |
| `crates/plugin-rust/src/ids.rs` | Claim names, referent ids, and concept ids |
| `crates/plugin-rust/src/init.rs` | The three files `veridoc init` creates, and the next step it names |
| `crates/veridoc-claims/src/lib.rs` | The claim library a project's claim module and harness target call |
| `crates/plugin-rust/src/bivalence.rs` | The structural bivalence rule |
| `examples/rust-scanner/` | An adopting project, used as the end-to-end fixture |
