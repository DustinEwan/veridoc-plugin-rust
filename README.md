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

Five files, and one of them is Rust. Nothing in your library changes: the plugin only reads your
source, and the scenario harness is a binary that you write and veridoc runs.

| File | What it holds |
| --- | --- |
| `.veridoc/config.json` | The plugin command, the coverage floor, the scenario deadline, and the judge model |
| `veridoc.tree.json` | The concepts, the document tree, the source roots, the observables, the actions, the fixtures, and the harness name |
| `<sourceRoot>/<module>.claims.json` | The claims about that module, one sidecar per module. `sourceRoots` in the tree names the roots a project scans; `src` is the default, and a Cargo workspace whose veridoc project is the repository root names each crate, as in `crates/my-crate/src` |
| `docs/*.spec.md` | The prose, in `<doc-section>` sections that name claims with `<doc-claim>` |
| `features/*.feature` | The scenarios: a `@id:` tag, a `Given` that names a fixture, a `When` that names an action |
| `src/bin/<harness>.rs` | The scenario harness, about twenty lines |

### The claims

You write the symbol and the predicate. The plugin records the referent, the file, and the line.
The claim's name is `<subject>#<predicate>`, so the example below is `parse_config#rejects-missing-config`
wherever a report, a `<doc-claim>`, or a leaf id prints it. After you rename a symbol, the sidecar still
refers to the old one, and the run reports it as unresolved instead of reading the sentence as still
describing whatever now occupies the file.

```json
{
  "kind": "ensures",
  "symbol": "parse_config",
  "concept": "project-scan",
  "predicate": "rejects-missing-config",
  "text": "parse_config refuses a directory that holds no config.json, naming the directory in the message.",
  "channel": {
    "scenario": "config/rejects-missing",
    "asserts": { "obs": "exit_code", "is": { "equals": 2 } }
  }
}
```

A claim carries exactly one channel: a scenario with its assertion tree, an artefact file with its
hash, or a citation. If a sidecar sets two channels, the plugin stops and reports the file, because
the engine's error for that case arrives later and points at the claim rather than at the line you
wrote.

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

A fact your code decides does not belong in prose you copy: `<doc-each source="observables">` on a line of
its own repeats the one block after it once per value the collection holds, and `<doc-value field="name"/>`
marks where each value lands. The example's second section pulls in the observables this plugin declares.
The authored file keeps the placeholder and the engine never writes into it; the assembled values appear in
the report, and they are inside the hash that decides whether the section is re-judged. The collections and
their fields are in `../veridoc/docs/spec.md`.

### The scenario harness

Rust binds a call target at link time, so the plugin cannot call your function the way a Node plugin
can `import()` a module. Your project supplies the one place where the request's JSON becomes your
own types:

```rust
use std::io::Read;

use scanner::parse_config;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let request: serde_json::Value = serde_json::from_str(&input)?;
    let fixture = request["fixture"].as_str().unwrap_or_default();

    let observables = match request["action"].as_str().unwrap_or_default() {
        "run-scanner" => {
            let outcome = parse_config(std::path::Path::new(fixture));
            serde_json::json!({
                "exit_code": outcome.as_ref().map_or(2, |_| 0),
                "message": outcome.map_or_else(|reason| reason, |_| String::new()),
            })
        }
        other => return Err(format!("the harness implements no action {other:?}").into()),
    };
    println!("{}", serde_json::json!({ "observables": observables }));
    Ok(())
}
```

The contract is one line in each direction:

```text
stdin  {"scenarioId":"..","action":"..","args":[".."],"fixture":"<absolute dir>","deadlineMs":30000}
stdout {"observables":{"exit_code":2,"message":".."},"satisfied":{"<predicate source>":true}}
```

The fixture directory also arrives as `argv[1]`. `satisfied` is optional, and its keys are the
predicate texts that an author wrote in a `satisfies` check. The engine compares an `equals` or
`contains` check against the observables itself, so most harnesses leave `satisfied` out.

The plugin builds the harness once per run, into `.veridoc/target`, so a scenario build does not
invalidate your own incremental build. Each scenario then costs one process spawn. The plugin still
believes a harness that exits non-zero after printing its answer, because a refusal inside your code
is an observed value for the assertion to judge. Only a harness that answers nothing is a failure,
and the failure kind records which of the four causes in the profile applies: `build` when cargo
cannot compile the harness, `fixture` when the fixture is undeclared or absent, `deadline` when the
harness answers nothing within `deadlineMs`, and `plugin` when the vocabulary has no such action or
the harness has no such predicate.

## Capabilities

The handshake reports eight capabilities, one for every method of the profile, and every one of
them is answered by this build. A method whose capability is absent is answered `-32601`, because
the profile fixes what an absent capability costs and an invented answer is not in that list.

| Capability | Status | What the absence costs you |
| --- | --- | --- |
| `initialization` | implemented | without it `veridoc init` writes only `.veridoc/config.json` and tells you to create `veridoc.tree.json` yourself |
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
id the project declares nothing behind answers no facts, and a concept id answers the line that
declares it in `veridoc.tree.json`.

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

`examples/rust-scanner` is a complete adopting project: two scenario claims, one artefact claim, a
feature file, a fixture pair, and the harness. To see the two methods that the engine calls first, without
a full run:

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
| `crates/plugin-rust/src/project.rs` | The vocabulary file, the claim sidecars, and the `collect` answer |
| `crates/plugin-rust/src/decl.rs` | The declaration scan: headers, visibility, bodies, and the parameter lists `resolve` and `derivable` read |
| `crates/plugin-rust/src/resolve.rs` | Reference keys, referent ids, and signatures |
| `crates/plugin-rust/src/derivable.rs` | The facts a declaration states, and the referent id that finds it |
| `crates/plugin-rust/src/cite.rs` | The `file` and `git` citation stores, and the reasons a citation goes unanswered |
| `crates/plugin-rust/src/surface.rs` | The `pub` items, with the ids `resolve` reports |
| `crates/plugin-rust/src/run.rs` | The harness build, the scenario spawn, the deadline, and the failure kinds |
| `crates/plugin-rust/src/bivalence.rs` | The structural bivalence rule |
| `examples/rust-scanner/` | An adopting project, used as the end-to-end fixture |
