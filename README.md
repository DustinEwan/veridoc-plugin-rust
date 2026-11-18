# veridoc-plugin-rust

This repository holds the [veridoc](https://github.com/DustinEwan/veridoc) plugin for Rust projects.
veridoc is a checker that reads the claims you write about your code, finds the code that each claim
is about, and runs the scenarios that would show the claim to be false. The plugin is the part that
reads your Rust: it parses the source, answers what a symbol's declaration says, and runs your
scenarios. Verdicts come from veridoc's engine, which does not read Rust.

The wire mechanism, the envelope, and the conformance rules come from
[pluginrpc](https://github.com/DustinEwan/pluginrpc), which is where the `prpc/1` framing is defined.
This repository owns the `vd/1` method vocabulary, the claim format for Rust, and the answers.

## What you add to your project

Five files, and one of them is Rust. Nothing in your library changes: the plugin only reads your
source, and the scenario harness is a binary that you write and veridoc runs.

| File | What it holds |
| --- | --- |
| `.veridoc/config.json` | The plugin command, the coverage floor, the scenario deadline, and the judge |
| `veridoc.tree.json` | The concepts, the document tree, the observables, the actions, the fixtures, and the harness name |
| `src/<module>.claims.json` | The claims about that module, one sidecar per module |
| `docs/*.spec.md` | The prose, in sections that select claims by heading attribute |
| `features/*.feature` | The scenarios: a `@id:` tag, a `Given` that names a fixture, a `When` that names an action |
| `src/bin/<harness>.rs` | The scenario harness, about twenty lines |

### The claims

You write the symbol and the predicate. The plugin records the referent, the file, and the line.
After you rename a symbol, the sidecar still refers to the old one, and the run reports it as
unresolved instead of reading the sentence as still describing whatever now occupies the file.

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

The handshake reports four capabilities. A method whose capability is absent is answered `-32601`,
because the profile fixes what an absent capability costs and an invented answer is not in that list.

| Capability | Status | What the absence costs you |
| --- | --- | --- |
| `collect` | implemented | without it there is no build at all |
| `resolution` | implemented | without it the plugin cannot be used |
| `bivalence` | implemented | every checkable claim becomes an error rather than a pass |
| `execution` | implemented | scenarios are bound but never run |
| `derivability` | not yet | the redundancy ban is skipped, so a claim a declaration already states is kept |
| `surface` | not yet | the report says uncovered symbols are unknown |
| `cite` | not yet | a citation-channel claim cannot be answered |

`bivalence` answers structurally. A tree of `equals` and `contains` checks is bivalent, because one
side comes from a run and the other from you. A tree with no leaf is not, because nothing in it can
fail. A tree with a `satisfies` leaf is answered `false` with the reason, because deciding that leaf
would mean settling a Rust predicate's two outcomes without running them.

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
  ../veridoc/vd-1.profile.json veridoc-plugin.json
```

## Layout

| Path | What it is |
| --- | --- |
| `crates/plugin-rust/src/main.rs` | The `prpc/1` server: the method vocabulary, the handshake, the refusals |
| `crates/plugin-rust/src/project.rs` | The vocabulary file, the claim sidecars, and the `collect` answer |
| `crates/plugin-rust/src/decl.rs` | The declaration scan that answers `resolve` |
| `crates/plugin-rust/src/resolve.rs` | Reference keys, referent ids, and signatures |
| `crates/plugin-rust/src/run.rs` | The harness build, the scenario spawn, the deadline, and the failure kinds |
| `crates/plugin-rust/src/bivalence.rs` | The structural bivalence rule |
| `examples/rust-scanner/` | An adopting project, used as the end-to-end fixture |
