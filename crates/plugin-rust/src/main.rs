//! veridoc-plugin-rust: the `veridoc/1` plugin for Rust projects, spoken over the pluginrpc
//! mechanism (`prpc/1`).
//!
//! The wire layer is the mechanism's own crate, so the framing, the envelope, the reserved id,
//! and the refusal codes here are the same code a host uses to check them. What this repository
//! owns is the method vocabulary, the claim format in Rust source, and the answers.

mod bivalence;
mod cite;
mod decl;
mod derivable;
mod ids;
mod init;
mod project;
mod resolve;
mod run;
mod surface;

use std::io::{self, Write};

use std::path::PathBuf;

use plugin_rpc::{
    buffered, buffered_writer, parse_request, read_line, write_line, MethodName, Request, Response,
    INVALID_REQUEST, METHOD_NOT_FOUND, PARSE_ERROR, PLUGIN_ERROR,
};

/// The profile this plugin answers. The mechanism uses this string to select the method set and
/// the payload types (§6), so it is the first thing a host checks.
const PROTOCOL_VERSION: &str = "veridoc/1";

/// The capabilities this build answers. Every name here is in `veridoc-1.profile.json`'s
/// vocabulary, and a method whose capability is absent is answered `-32601` rather than with a
/// result, because the profile defines what an absent capability costs and a made-up answer does
/// not appear in that list. The eight are the whole profile: every method `Method` names has an
/// arm in `dispatch`, so no capability is published for an answer the plugin does not give.
const CAPABILITIES: &[&str] = &[
    "initialization",
    "collect",
    "resolution",
    "derivability",
    "bivalence",
    "execution",
    "citation",
    "surface",
];

/// The `veridoc/1` method vocabulary. The mechanism transports these strings and gives them no
/// meaning; `from_wire` is what turns a name outside the profile into a `-32601` (§9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    Init,
    Collect,
    Resolve,
    Derivable,
    Bivalence,
    Run,
    Cite,
    Surface,
}

impl MethodName for Method {
    fn name(&self) -> &'static str {
        match self {
            Method::Init => "init",
            Method::Collect => "collect",
            Method::Resolve => "resolve",
            Method::Derivable => "derivable",
            Method::Bivalence => "bivalence",
            Method::Run => "run",
            Method::Cite => "cite",
            Method::Surface => "surface",
        }
    }

    fn from_wire(name: &str) -> Option<Self> {
        match name {
            "init" => Some(Method::Init),
            "collect" => Some(Method::Collect),
            "resolve" => Some(Method::Resolve),
            "derivable" => Some(Method::Derivable),
            "bivalence" => Some(Method::Bivalence),
            "run" => Some(Method::Run),
            "cite" => Some(Method::Cite),
            "surface" => Some(Method::Surface),
            _ => None,
        }
    }
}

fn main() {
    if let Err(error) = serve() {
        // The host reads stdout for messages; a failure of the process itself is a loss to it,
        // so the reason goes where the mechanism says plugin diagnostics go (§3).
        eprintln!("veridoc-plugin-rust: {error}");
        std::process::exit(1);
    }
}

/// Read requests from stdin and answer them on stdout until the host closes the pipe. A blank
/// line or an end of file ends the connection (§3); neither is an error to report.
///
/// `root` is the project the last `collect` read. `resolve` is sent only `refs`
/// (`resolve-req.json` allows nothing else), so the project it settles them against is the one
/// `collect` reported, and a `resolve` before any `collect` is refused rather than guessed.
fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = buffered(stdin.lock());
    let mut writer = buffered_writer(stdout.lock());
    let mut buf = String::new();
    let mut root: Option<PathBuf> = None;
    while let Some(line) = read_line(&mut reader, &mut buf)? {
        let response = dispatch(&line, &mut root);
        write_line(&mut writer, &response)?;
    }
    writer.flush()?;
    Ok(())
}

/// Answer one line. Every path returns a response: a plugin that stays silent on a message it
/// cannot use leaves the host waiting for a deadline instead of reading a reason (§5).
fn dispatch(line: &str, root: &mut Option<PathBuf>) -> Response {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return Response::uncorrelated(PARSE_ERROR, "the line is not JSON");
    };
    let id = value.get("id").and_then(serde_json::Value::as_u64);
    let request = match parse_request(line) {
        Ok(request) => request,
        Err(error) => {
            let text = format!("the request is not a valid prpc/1 request: {error}");
            return match id {
                Some(id) if id > 0 => Response::refusal(id, INVALID_REQUEST, text),
                _ => Response::uncorrelated(INVALID_REQUEST, text),
            };
        }
    };
    if request.method == "handshake" {
        return Response::result(request.id, handshake());
    }
    match Method::from_wire(&request.method) {
        Some(Method::Init) => {
            let params = request.params.unwrap_or(serde_json::Value::Null);
            match init::init(&params) {
                Ok(result) => Response::result(request.id, result),
                Err((code, reason)) => Response::refusal(request.id, code, reason),
            }
        }
        None => Response::refusal(
            request.id,
            METHOD_NOT_FOUND,
            format!("{} is not a method of {}", request.method, PROTOCOL_VERSION),
        ),
        Some(Method::Collect) => match param_path(&request, "root", "root is required") {
            Ok(project_root) => match project::collect(&project_root) {
                Ok(result) => {
                    *root = Some(project_root);
                    Response::result(request.id, result)
                }
                Err(reason) => Response::refusal(request.id, PLUGIN_ERROR, reason),
            },
            Err(reason) => Response::refusal(request.id, INVALID_REQUEST, reason),
        },
        Some(Method::Resolve) => {
            let Some(project_root) = root.clone() else {
                return Response::refusal(
                    request.id,
                    PLUGIN_ERROR,
                    "collect has not run, so there is no project to settle references in",
                );
            };
            match param_strings(&request, "refs", "refs must be an array of strings") {
                Ok(refs) => Response::result(
                    request.id,
                    resolve::resolve(&project_root, &refs, &project::concepts(&project_root)),
                ),
                Err(reason) => Response::refusal(request.id, INVALID_REQUEST, reason),
            }
        }
        Some(Method::Bivalence) => {
            let Some(project_root) = root.clone() else {
                return Response::refusal(
                    request.id,
                    PLUGIN_ERROR,
                    "collect has not run, so there is no claim set to settle bivalence against",
                );
            };
            match param_string(&request, "claimId", "claimId is required") {
                Ok(claim_name) => match project::load(&project_root) {
                    Ok(loaded) => {
                        Response::result(request.id, bivalence::bivalence(&loaded, &claim_name))
                    }
                    Err(reason) => Response::refusal(request.id, PLUGIN_ERROR, reason),
                },
                Err(reason) => Response::refusal(request.id, INVALID_REQUEST, reason),
            }
        }
        Some(Method::Run) => {
            let Some(project_root) = root.clone() else {
                return Response::refusal(
                    request.id,
                    PLUGIN_ERROR,
                    "collect has not run, so there is no vocabulary to run a scenario from",
                );
            };
            let params = match run_params(&request) {
                Ok(params) => params,
                Err(reason) => return Response::refusal(request.id, INVALID_REQUEST, reason),
            };
            match project::load(&project_root) {
                Ok(loaded) => Response::result(request.id, run::run(&loaded, &params)),
                Err(reason) => Response::refusal(request.id, PLUGIN_ERROR, reason),
            }
        }
        Some(Method::Derivable) => {
            let Some(project_root) = root.clone() else {
                return Response::refusal(
                    request.id,
                    PLUGIN_ERROR,
                    "collect has not run, so there is no project whose declarations to read",
                );
            };
            match param_string(&request, "referentId", "referentId is required") {
                Ok(referent) => {
                    Response::result(request.id, derivable::facts(&project_root, &referent))
                }
                Err(reason) => Response::refusal(request.id, INVALID_REQUEST, reason),
            }
        }
        Some(Method::Cite) => {
            let Some(project_root) = root.clone() else {
                return Response::refusal(
                    request.id,
                    PLUGIN_ERROR,
                    "collect has not run, so there is no project whose citation stores to read",
                );
            };
            match param_citations(&request) {
                Ok(citations) => {
                    Response::result(request.id, cite::cite(&project_root, &citations))
                }
                Err(reason) => Response::refusal(request.id, INVALID_REQUEST, reason),
            }
        }
        Some(Method::Surface) => {
            let Some(project_root) = root.clone() else {
                return Response::refusal(
                    request.id,
                    PLUGIN_ERROR,
                    "collect has not run, so there is no source tree to list the surface of",
                );
            };
            Response::result(request.id, surface::surface(&project_root))
        }
    }
}

/// One path parameter. The engine sends the project root absolute; a relative value is read
/// against the working directory the host started the plugin in, which is the project root.
fn param_path(request: &Request, key: &str, missing: &str) -> Result<PathBuf, String> {
    let value = request
        .params
        .as_ref()
        .and_then(|params| params.get(key))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| missing.to_string())?;
    Ok(PathBuf::from(value))
}

/// One string parameter.
fn param_string(request: &Request, key: &str, missing: &str) -> Result<String, String> {
    request
        .params
        .as_ref()
        .and_then(|params| params.get(key))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| missing.to_string())
}

/// The `cite` parameters. Each entry is read as the profile's `Citation`, so a store, an id, or
/// a turn of the wrong shape is an invalid request naming the field rather than a citation the
/// plugin answers as unresolved.
fn param_citations(request: &Request) -> Result<Vec<cite::Citation>, String> {
    const MISSING: &str = "citations must be an array of {store, id, turn} objects";
    let citations = request
        .params
        .as_ref()
        .and_then(|params| params.get("citations"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| MISSING.to_string())?;
    citations
        .iter()
        .map(|value| serde_json::from_value(value.clone()).map_err(|e| format!("{MISSING}; {e}")))
        .collect()
}

/// The `run` parameters. A missing or mistyped field is an invalid request rather than a run
/// failure, because `run-request.json` fixes the shape and a plugin that guesses which scenario
/// the engine meant reports a verdict for the wrong claim.
fn run_params(request: &Request) -> Result<run::Request, String> {
    const MISSING: &str = "run requires scenarioId, fixture, action, args, deadlineMs, and leaves";
    let Some(params) = request.params.as_ref() else {
        return Err(MISSING.to_string());
    };
    let text = |key: &str| {
        params
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    let (Some(scenario_id), Some(fixture), Some(action)) =
        (text("scenarioId"), text("fixture"), text("action"))
    else {
        return Err(MISSING.to_string());
    };
    let args = strings(params, "args")?;
    let deadline_ms = params
        .get("deadlineMs")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| format!("{MISSING}; deadlineMs must be a number"))?;
    let leaves = params
        .get("leaves")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| format!("{MISSING}; leaves must be an array of objects"))?
        .iter()
        .map(|leaf| {
            leaf.get("leafId")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| "each leaf must carry a leafId".to_string())
        })
        .collect::<Result<Vec<String>, String>>()?;
    Ok(run::Request {
        scenario_id,
        fixture,
        action,
        args,
        deadline_ms,
        leaves,
    })
}

/// One array-of-strings parameter, read from an already-held `params` object.
fn strings(params: &serde_json::Value, key: &str) -> Result<Vec<String>, String> {
    params
        .get(key)
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| format!("{key} must be an array of strings"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{key} must be an array of strings"))
        })
        .collect()
}

/// One array-of-strings parameter.
fn param_strings(request: &Request, key: &str, missing: &str) -> Result<Vec<String>, String> {
    let values = request
        .params
        .as_ref()
        .and_then(|params| params.get(key))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| missing.to_string())?;
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| missing.to_string())
        })
        .collect()
}

/// The handshake result: the four fields the mechanism owns, plus `languages`, which `veridoc/1`
/// requires and the mechanism's open envelope allows (§6). `handshake.json` sets
/// `additionalProperties: false`, so these five are the whole object.
fn handshake() -> serde_json::Value {
    serde_json::json!({
        "protocolVersion": PROTOCOL_VERSION,
        "name": "veridoc-plugin-rust",
        "version": env!("CARGO_PKG_VERSION"),
        "languages": ["rust"],
        "capabilities": CAPABILITIES,
    })
}
