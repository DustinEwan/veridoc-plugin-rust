//! veridoc-plugin-rust: the `vd/1` plugin for Rust projects, spoken over the pluginrpc
//! mechanism (`prpc/1`).
//!
//! The wire layer is the mechanism's own crate, so the framing, the envelope, the reserved id,
//! and the refusal codes here are the same code a host uses to check them. What this repository
//! owns is the method vocabulary, the claim format in Rust source, and the answers.

use std::io::{self, Write};

use plugin_rpc::{
    buffered, buffered_writer, parse_request, read_line, write_line, MethodName, Response,
    INVALID_REQUEST, METHOD_NOT_FOUND, PARSE_ERROR,
};

/// The profile this plugin answers. The mechanism uses this string to select the method set and
/// the payload types (§6), so it is the first thing a host checks.
const PROTOCOL_VERSION: &str = "vd/1";

/// The capabilities this build answers. Every name here is in `vd-1.profile.json`'s vocabulary,
/// and a method whose capability is absent is answered `-32601` rather than with an invented
/// result, because the profile defines what an absent capability costs and a made-up answer does
/// not appear in that list.
const CAPABILITIES: &[&str] = &[];

/// The `vd/1` method vocabulary. The mechanism transports these strings and gives them no
/// meaning; `from_wire` is what turns a name outside the profile into a `-32601` (§9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
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
fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = buffered(stdin.lock());
    let mut writer = buffered_writer(stdout.lock());
    let mut buf = String::new();
    while let Some(line) = read_line(&mut reader, &mut buf)? {
        let response = dispatch(&line);
        write_line(&mut writer, &response)?;
    }
    writer.flush()?;
    Ok(())
}

/// Answer one line. Every path returns a response: a plugin that stays silent on a message it
/// cannot use leaves the host waiting for a deadline instead of reading a reason (§5).
fn dispatch(line: &str) -> Response {
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
        None => Response::refusal(
            request.id,
            METHOD_NOT_FOUND,
            format!("{} is not a method of {}", request.method, PROTOCOL_VERSION),
        ),
        Some(method) => Response::refusal(
            request.id,
            METHOD_NOT_FOUND,
            format!(
                "veridoc-plugin-rust {} answers no {} yet; it declares the capabilities {:?}, \
                 and the profile defines what each absent one costs",
                env!("CARGO_PKG_VERSION"),
                method.name(),
                CAPABILITIES,
            ),
        ),
    }
}

/// The handshake result: the four fields the mechanism owns, plus `languages`, which `vd/1`
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
