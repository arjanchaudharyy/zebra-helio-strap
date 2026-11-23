use std::collections::{BTreeMap, BTreeSet};

use std::io::{self, BufRead, Write};
use std::sync::Mutex;

use chrono::{Duration, Local, NaiveDate};

use serde_json::{json, Value};

use zeppbridge_core::access::{self, AccessCategory, AccessScope, DataRequest, Permit};

use zeppbridge_core::contract;

use zeppbridge_core::paths;

use zeppbridge_core::storage::Database;

mod actions;
mod browse;
mod coach;
mod protocol;
mod runners;
mod schema;
mod schema_writes;
mod strap;
#[cfg(test)]
mod tests;
mod tools;
mod writes;

use actions::*;
use browse::*;
use coach::*;
use protocol::*;
use runners::*;
use schema::*;
use tools::*;
use writes::*;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The modern (no handshake) protocol version.
const MODERN_PROTOCOL_VERSION: &str = "2026-07-28";

/// The version to reply with when a legacy `initialize` arrives without a version.
const LEGACY_PROTOCOL_VERSION: &str = "2024-11-05";

/// Every version whose semantics we are willing to answer by, newest first.
///
/// This server's surface is only `tools/list` and `tools/call`, and the shape of these two methods
/// had no incompatible change from 2024-11-05 to 2025-11-25, so all these versions are supported as is.
/// A version not in this table gets `UnsupportedProtocolVersionError`. **Refuse clearly
/// rather than pretend to answer with semantics we did not implement.**
const SUPPORTED_PROTOCOL_VERSIONS: [&str; 5] = [
    MODERN_PROTOCOL_VERSION,
    "2025-11-25",
    "2025-06-18",
    "2025-03-26",
    LEGACY_PROTOCOL_VERSION,
];

/// Prefix of the reserved keys in `_meta`.
const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";

const META_SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";

const META_CLIENT_INFO: &str = "io.modelcontextprotocol/clientInfo";

/// Cache hint for list results. Tool definitions change only with the build and do not change while the process runs,
/// but clients must not cache them past the next upgrade. One hour is a compromise that saves round trips and does not
/// leave people with an old tool list.
const LIST_TTL_MS: i64 = 3_600_000;

/// JSON-RPC error codes. The first three come from the protocol; the -32000 range is for the application.
const ERR_METHOD_NOT_FOUND: i64 = -32601;

const ERR_INVALID_PARAMS: i64 = -32602;

const ERR_NOT_CONFIGURED: i64 = -32001;

const ERR_DATABASE: i64 = -32002;

/// `UnsupportedProtocolVersionError` as the 2026-07-28 spec defines it.
///
/// It is in the -32020..-32099 range reserved for the spec, and does not clash with the two application codes above
/// (-32000..-32019, explicitly grandfathered).
const ERR_UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;

const MAX_REQUEST_BYTES: usize = 1024 * 1024;

enum RequestFrame {
    Message(Vec<u8>),
    TooLarge,
}

// Drain oversized lines without retaining them, then resume at the next message.
fn read_frame(reader: &mut impl BufRead) -> io::Result<Option<RequestFrame>> {
    let mut bytes = Vec::new();
    let mut oversized = false;
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return Ok(if oversized {
                Some(RequestFrame::TooLarge)
            } else if bytes.is_empty() {
                None
            } else {
                Some(RequestFrame::Message(bytes))
            });
        }
        let newline = chunk.iter().position(|byte| *byte == b'\n');
        let count = newline.unwrap_or(chunk.len());
        if !oversized {
            if count > MAX_REQUEST_BYTES - bytes.len() {
                oversized = true;
                bytes.clear();
            } else {
                bytes.extend_from_slice(&chunk[..count]);
            }
        }
        reader.consume(count + usize::from(newline.is_some()));
        if newline.is_some() {
            return Ok(Some(if oversized {
                RequestFrame::TooLarge
            } else {
                RequestFrame::Message(bytes)
            }));
        }
    }
}

fn main() {
    // argv is the only way to grant access to this process. The server reads stdin only after the scope is set:
    // a parse failure never enters the server loop.
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The desktop app uses this to check if this sidecar was upgraded too (while Claude Desktop holds the old file, an update cannot replace it).
    if args == ["--version"] {
        println!("zeppbridge-mcp {VERSION}");
        return;
    }
    let scope = match parse_scope_args(&args) {
        Ok(scope) => scope,
        Err(message) => {
            eprintln!("zeppbridge-mcp: {message}");
            std::process::exit(2);
        }
    };
    let stdin = io::stdin();
    let stdout = io::stdout();
    let _ = serve(&mut stdin.lock(), &mut stdout.lock(), &scope);
}

/// `--scope full-readonly|task`. The default is `full-readonly`, so old configs with no flag
/// behave the same. Any unknown argument, value or repeated `--scope` fails closed.
fn parse_scope_args(args: &[String]) -> Result<AccessScope, String> {
    const USAGE: &str = "Usage: zeppbridge-mcp [--scope full-readonly|task]";
    let mut scope = AccessScope::FullReadOnly;
    let mut scope_seen = false;
    let mut index = 0;
    while index < args.len() {
        let value = if args[index] == "--scope" {
            index += 1;
            args.get(index)
                .map(String::as_str)
                .ok_or_else(|| format!("--scope needs a value (full-readonly or task). {USAGE}"))?
        } else if let Some(value) = args[index].strip_prefix("--scope=") {
            value
        } else {
            return Err(format!("Unknown argument `{}`. {USAGE}", args[index]));
        };
        if scope_seen {
            return Err(format!("--scope can be given only once. {USAGE}"));
        }
        scope_seen = true;
        scope = AccessScope::parse(value).ok_or_else(|| {
            format!("Invalid --scope value `{value}`: it must be full-readonly or task. {USAGE}")
        })?;
        index += 1;
    }
    Ok(scope)
}

fn serve(
    reader: &mut impl BufRead,
    stdout: &mut impl Write,
    scope: &AccessScope,
) -> io::Result<()> {
    while let Some(frame) = read_frame(reader)? {
        let parsed: Result<Value, (i64, &str)> = match frame {
            RequestFrame::Message(bytes) => {
                if bytes.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                serde_json::from_slice(&bytes)
                    .map_err(|_| (-32700, "Request is not valid JSON or UTF-8"))
            }
            RequestFrame::TooLarge => Err((-32600, "Request exceeds the 1 MiB limit")),
        };
        let request = match parsed {
            Ok(value) => value,
            Err((code, message)) => {
                writeln!(
                    stdout,
                    "{}",
                    json!({"jsonrpc":"2.0", "id":null, "error":{"code":code,"message":message}})
                )?;
                stdout.flush()?;
                continue;
            }
        };
        // Notifications have no response.
        let Some(id) = request.get("id").cloned() else {
            continue;
        };
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let params = request.get("params").cloned().unwrap_or(json!({}));
        let response = match handle_scoped(method, &params, scope) {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error.to_json() }),
        };
        writeln!(stdout, "{response}")?;
        stdout.flush()?;
    }
    Ok(())
}
