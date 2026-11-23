//! JSON-RPC protocol layer: errors, handshake, protocol version negotiation, method dispatch (split out of main.rs, same logic).

use super::*;

/// One JSON-RPC error.
///
/// No longer a bare `(i64, String)`: `UnsupportedProtocolVersionError` must carry
/// `data.supported` and `data.requested`. The client uses these two to pick a version both sides support
/// and retry. Without `data` it can only give up.
#[derive(Debug)]
pub(super) struct RpcError {
    pub(super) code: i64,
    pub(super) message: String,
    pub(super) data: Option<Value>,
}

impl RpcError {
    pub(super) fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub(super) fn to_json(&self) -> Value {
        match &self.data {
            Some(data) => json!({ "code": self.code, "message": self.message, "data": data }),
            None => json!({ "code": self.code, "message": self.message }),
        }
    }
}

impl From<(i64, String)> for RpcError {
    fn from((code, message): (i64, String)) -> Self {
        Self::new(code, message)
    }
}

/// Server identity. Modern results put it in `_meta`, legacy results put it in `serverInfo`.
pub(super) fn server_info() -> Value {
    json!({ "name": "zeppbridge", "version": VERSION })
}

/// The boundaries and missing-value rules the client must see at the first handshake (or first call).
///
/// They are written here, so the caller does not have to guess from an empty series. When a model sees "no heart rate today",
/// the easiest mistake is to treat it as 0. The access scope is stated here too: the model must know whether it
/// sees the whole library or only the small part a task grants.
pub(super) fn instructions(scope: &AccessScope) -> String {
    let scope_note = match scope {
        AccessScope::FullReadOnly => {
            "full-readonly: all health data on this computer is read-only. Only the user's own ledgers are written: training plan drafts \
             (publish_training_plan works only when the user turns on \"Allow AI to publish directly\" in settings), coach profile, injuries / life events / races / shoes, \
             workout feel and type correction. Every write goes into the change ledger, and the user can undo each one in ZeppBridge; there is no delete tool. \
             sync_now pulls new data from the cloud once, export_data writes a summary to a local file."
        }
        AccessScope::TaskScoped => {
            "task: you see only data inside the workouts and date windows granted by tasks marked \"Share with MCP\" on the desktop; \
             calls outside the scope are denied with err.mcp.scope_denied / err.mcp.scope_no_grants, \
             and the permittedRanges in the denial are the windows actually granted."
        }
    };
    format!(
        "ZeppBridge health data and training plans. First read the resource {GUIDE_URI} (the guide: tool list, recommended order, data definitions, prohibitions and how to undo), \
         then start with get_training_context. {}\nTime: {}\nMissing values: {}\nSource: {}\nScope: {scope_note}",
        contract::PRIVACY_NOTE,
        contract::TIME_CONVENTION,
        contract::MISSING_VALUE_CONVENTION,
        contract::SOURCE_CONVENTION,
    )
}

/// The guide for the local AI only (`guide.md`, compiled into the program).
pub(super) const GUIDE_URI: &str = "zeppbridge://guide";
pub(super) const GUIDE: &str = include_str!("guide.md");

fn resource_list() -> Value {
    json!({ "resources": [{
        "uri": GUIDE_URI,
        "name": "guide",
        "title": "ZeppBridge guide for local AI",
        "description": "Tools, recommended order, data rules, the three prohibitions, and how the user undoes AI writes.",
        "mimeType": "text/markdown",
    }] })
}

fn resource_read(params: &Value) -> Result<Value, RpcError> {
    match params.get("uri").and_then(Value::as_str) {
        Some(GUIDE_URI) => Ok(json!({ "contents": [{
            "uri": GUIDE_URI,
            "mimeType": "text/markdown",
            "text": GUIDE,
        }] })),
        // The spec uses -32002 for a missing resource; in our code range -32002 already means "database", so use invalid params here.
        other => Err(RpcError::new(
            ERR_INVALID_PARAMS,
            format!("No such resource: {}", other.unwrap_or("(missing uri)")),
        )),
    }
}

/// The protocol version declared in the request `_meta`. If absent, this is a legacy client.
pub(super) fn requested_protocol_version(params: &Value) -> Option<&str> {
    params
        .get("_meta")
        .and_then(|meta| meta.get(META_PROTOCOL_VERSION))
        .and_then(Value::as_str)
}

/// The name the client reports (legacy: `clientInfo` in `initialize`; modern: `_meta` on each request).
/// Used only to label who proposed an entry in the "AI wants to remember" inbox. No decision depends on it.
static CLIENT_NAME: Mutex<Option<String>> = Mutex::new(None);

fn remember_client(params: &Value) {
    let name = params
        .get("clientInfo")
        .or_else(|| {
            params
                .get("_meta")
                .and_then(|meta| meta.get(META_CLIENT_INFO))
        })
        .and_then(|info| info.get("name"))
        .and_then(Value::as_str);
    if let (Some(name), Ok(mut slot)) = (name, CLIENT_NAME.lock()) {
        *slot = Some(name.to_string());
    }
}

pub(super) fn client_name() -> Option<String> {
    CLIENT_NAME.lock().ok().and_then(|slot| slot.clone())
}

/// Whether this server accepts this version.
pub(super) fn version_supported(version: &str) -> bool {
    SUPPORTED_PROTOCOL_VERSIONS.contains(&version)
}

pub(super) fn unsupported_version_error(requested: &str) -> RpcError {
    RpcError {
        code: ERR_UNSUPPORTED_PROTOCOL_VERSION,
        message: "Unsupported protocol version".to_string(),
        // Must include the versions we support: the client uses it to pick one and retry.
        data: Some(json!({
            "supported": SUPPORTED_PROTOCOL_VERSIONS,
            "requested": requested,
        })),
    }
}

/// Puts the required envelope on modern results: `resultType` and `_meta.serverInfo`.
///
/// Every modern result **must** have `resultType`; legacy results must not have it,
/// so this step runs only on the modern path.
pub(super) fn modern_result(mut result: Value) -> Value {
    if let Some(object) = result.as_object_mut() {
        object.insert("resultType".to_string(), json!("complete"));
        object.insert(
            "_meta".to_string(),
            json!({ META_SERVER_INFO: server_info() }),
        );
    }
    result
}

/// A full-readonly shortcut into `handle`, for callers that do not care about scope (tests).
/// The production path is only `serve` -> `handle_scoped`. The two protocol eras merge in `call_tool`
/// (R9).
#[cfg(test)]
pub(super) fn handle(method: &str, params: &Value) -> Result<Value, RpcError> {
    handle_scoped(method, params, &AccessScope::FullReadOnly)
}

pub(super) fn handle_scoped(
    method: &str,
    params: &Value,
    scope: &AccessScope,
) -> Result<Value, RpcError> {
    remember_client(params);
    // `server/discover` is itself the modern entry point, and also the era probe on stdio:
    // the client tries it. If it is recognized, the server is modern; if it reports an unknown method, the client falls back to initialize.
    if method == "server/discover" {
        if let Some(version) = requested_protocol_version(params) {
            if !version_supported(version) {
                return Err(unsupported_version_error(version));
            }
        }
        return Ok(modern_result(json!({
            "supportedVersions": SUPPORTED_PROTOCOL_VERSIONS,
            "capabilities": { "tools": {}, "resources": {} },
            "instructions": instructions(scope),
            // The handshake does not open the database, so this reports only the mode, not the grant count;
            // scope.grants in each tools/call result is the live value.
            "scope": { "mode": scope.as_str() },
            "ttlMs": LIST_TTL_MS,
            // This tool list is the same for everyone: it has no account content and does not change per connection.
            "cacheScope": "public",
        })));
    }

    // A request with a `_meta` version comes from a modern client. Treat one without it as legacy: that is how
    // most clients look today.
    if let Some(version) = requested_protocol_version(params) {
        if !version_supported(version) {
            return Err(unsupported_version_error(version));
        }
        return match method {
            "tools/list" => Ok(modern_result(json!({
                "tools": tool_definitions(),
                "ttlMs": LIST_TTL_MS,
                "cacheScope": "public",
            }))),
            "tools/call" => call_tool(params, scope)
                .map(modern_result)
                .map_err(RpcError::from),
            "resources/list" => Ok(modern_result(resource_list())),
            "resources/read" => resource_read(params).map(modern_result),
            // `initialize` / `ping` no longer exist in this version. Receiving them means the client
            // mixes the two eras. Saying so clearly is better than quietly going along.
            other => Err(RpcError::new(
                ERR_METHOD_NOT_FOUND,
                format!("Unsupported method: {other}. This server provides only tool calls and the guide resource."),
            )),
        };
    }

    match method {
        "initialize" => {
            // Legacy rule: if we support the version the client asks for, reply with it; if not,
            // reply with our own, and the client decides whether to continue.
            let requested = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .filter(|version| version_supported(version))
                .unwrap_or(LEGACY_PROTOCOL_VERSION);
            Ok(json!({
                "protocolVersion": requested,
                "capabilities": { "tools": {}, "resources": {} },
                "serverInfo": server_info(),
                "instructions": instructions(scope),
                // The handshake reports only the mode: the grant count comes live in each tools/call.
                "scope": { "mode": scope.as_str() },
            }))
        }
        "notifications/initialized" | "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_definitions() })),
        "tools/call" => call_tool(params, scope).map_err(RpcError::from),
        "resources/list" => Ok(resource_list()),
        "resources/read" => resource_read(params),
        other => Err(RpcError::new(
            ERR_METHOD_NOT_FOUND,
            format!("Unsupported method: {other}. This server provides only tool calls and the guide resource."),
        )),
    }
}

/* ------------------------------ Tool definitions ------------------------------ */
