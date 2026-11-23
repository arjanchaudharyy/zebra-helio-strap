//! Hand-written HTTP handling: accept connections, read the request line and headers, route, write responses (split out of local_api.rs, logic unchanged).

use super::*;

pub(crate) fn serve(listener: TcpListener, context: Arc<ServerContext>, stop: Arc<AtomicBool>) {
    // Accepted connections go to a fixed-size worker pool. The accept thread only
    // accepts and goes straight back to accept. A slow client can hold at most one
    // worker and cannot block the others.
    let (sender, receiver) = mpsc::sync_channel::<(TcpStream, SocketAddr)>(ACCEPT_QUEUE);
    // `Receiver` is not `Sync`, so the workers share one lock and take turns. The
    // lock is held only during `recv`. The real request handling runs outside the lock.
    let receiver = Arc::new(Mutex::new(receiver));
    let mut workers = Vec::with_capacity(WORKER_THREADS);
    for _ in 0..WORKER_THREADS {
        let receiver = Arc::clone(&receiver);
        let context = Arc::clone(&context);
        workers.push(std::thread::spawn(move || loop {
            let next = {
                let guard = receiver.lock().unwrap_or_else(|e| e.into_inner());
                guard.recv()
            };
            // When the sender is dropped (the accept loop ended), recv fails and the worker exits.
            let Ok((mut stream, peer)) = next else { break };
            if let Err(error) = handle_connection(&mut stream, &context, peer) {
                eprintln!("local API request handling failed: {error}");
            }
        }));
    }

    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, peer)) => {
                // The desktop accepts loopback sources only. If the listen address is wrong, this is a second guard.
                if !context.allow_remote && !peer.ip().is_loopback() {
                    continue;
                }
                if stream.set_nonblocking(false).is_err() {
                    continue;
                }
                match sender.try_send((stream, peer)) {
                    Ok(()) => {}
                    Err(mpsc::TrySendError::Full(_)) => {
                        // The queue is full: drop this connection so the accept loop can still respond to stop.
                    }
                    Err(mpsc::TrySendError::Disconnected(_)) => break,
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            Err(error) => {
                eprintln!("local API connection failed: {error}");
                break;
            }
        }
    }

    // Drop the listener first, so the port is released at once. Workers exit on
    // their own after their current connection. Do not join here: `set_enabled(false)`
    // holds the controller lock, and a join on a read timeout would freeze the settings page.
    drop(listener);
    drop(sender);
    drop(workers);
}

pub(crate) fn handle_connection(
    stream: &mut TcpStream,
    context: &ServerContext,
    peer: SocketAddr,
) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    if let Verdict::Throttled(seconds) = context.limiter.admit(peer.ip()) {
        return write_response(stream, HttpResponse::too_many_requests(seconds));
    }
    let request = match read_request(stream) {
        Ok(request) => request,
        Err(error) => {
            write_response(stream, HttpResponse::bad_request("invalid_request", &error))?;
            return Ok(());
        }
    };
    let tokens = context
        .tokens
        .read()
        .map(|value| value.clone())
        .unwrap_or_else(|e| e.into_inner().clone());
    let outcome = dispatch(
        &request,
        &tokens,
        &context.cache,
        |records| compute_records(&context.data_dir, records),
        |workout_id| load_workout_series(&context.data_dir, workout_id),
    );
    if outcome.auth_failed {
        context.limiter.record_failure(peer.ip());
    }
    if context.log_requests {
        let path = request
            .target
            .split_once('?')
            .map_or(request.target.as_str(), |(path, _)| path);
        // Log only the path: no query parameters, no token, no data.
        let path: String = path.chars().take(80).collect();
        match outcome.records {
            Some(count) => eprintln!(
                "serve: {} {} {} records={count}",
                request.method, path, outcome.response.status
            ),
            None => eprintln!(
                "serve: {} {} {}",
                request.method, path, outcome.response.status
            ),
        }
    }
    write_response(stream, outcome.response)
}

/// The result of handling one request.
pub struct Outcome {
    pub response: HttpResponse,
    /// The record count of this `/v1/records` page (for the log only).
    pub records: Option<usize>,
    /// Auth failed (counts toward the rate limit).
    pub auth_failed: bool,
}

/// Authenticate and dispatch to the routes. Both tokens can read `/v1/*` and `/health`. Other routes accept only the local token.
pub fn dispatch<R, W>(
    request: &ParsedRequest,
    tokens: &TokenSet,
    cache: &RecordsCache,
    compute: R,
    lookup: W,
) -> Outcome
where
    R: FnOnce(&RecordsRequest) -> Result<Computed, String>,
    W: FnOnce(&str) -> Result<Option<WorkoutSeries>, String>,
{
    let done = |response| Outcome {
        response,
        records: None,
        auth_failed: false,
    };
    let Some(scope) = request
        .authorization
        .as_deref()
        .and_then(bearer_value)
        .and_then(|provided| tokens.authorize(provided))
    else {
        return Outcome {
            response: HttpResponse::unauthorized(),
            records: None,
            auth_failed: true,
        };
    };
    if request.method != "GET" {
        return done(HttpResponse::method_not_allowed());
    }
    let target = request.target.as_str();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    match path {
        "/health" => done(health_response()),
        "/v1" | "/v1/" => done(v1_index()),
        "/v1/records" => {
            let (response, records) = records_response(query, cache, compute);
            Outcome {
                response,
                records,
                auth_failed: false,
            }
        }
        _ if scope == TokenScope::Pull => done(HttpResponse::forbidden(
            "token_scope",
            "the pull token can only read /v1/* and /health",
        )),
        _ => {
            let local = tokens.local.as_deref().unwrap_or_default();
            done(route_request(request, local, lookup))
        }
    }
}

fn health_response() -> HttpResponse {
    HttpResponse::json(
        200,
        "OK",
        json!({
            "status": "ok",
            "service": "ZeppBridge local API",
            "version": env!("CARGO_PKG_VERSION")
        }),
    )
}

/// A parsed request. It keeps only the fields that routing and auth need. Other headers are read and dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRequest {
    pub method: String,
    pub target: String,
    /// `None` = no Authorization header.
    pub authorization: Option<String>,
}

pub(super) fn read_request(stream: &mut TcpStream) -> Result<ParsedRequest, String> {
    let capped = (MAX_REQUEST_LINE_BYTES + MAX_HEADER_TOTAL_BYTES + 2) as u64;
    let mut reader = BufReader::new(stream).take(capped);
    parse_request(&mut reader)
}

/// Bounded parsing of the request line and headers.
///
/// Refuses: an overlong request line, an overlong single header, too many headers,
/// too many total header bytes, and duplicate `Authorization` headers with
/// conflicting values (to avoid parse ambiguity from two headers).
pub fn parse_request<R: BufRead>(reader: &mut R) -> Result<ParsedRequest, String> {
    let mut line = String::new();
    let bytes = read_capped_line(reader, &mut line, MAX_REQUEST_LINE_BYTES)
        .map_err(|_| "cannot read the HTTP request".to_string())?;
    if bytes == 0 || bytes > MAX_REQUEST_LINE_BYTES {
        return Err("HTTP request line is empty or too long".to_string());
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().ok_or_else(|| "missing HTTP method".to_string())?;
    let target = parts.next().ok_or_else(|| "missing request path".to_string())?;
    let version = parts.next().ok_or_else(|| "missing HTTP version".to_string())?;
    if parts.next().is_some() || (version != "HTTP/1.1" && version != "HTTP/1.0") {
        return Err("invalid HTTP request line format".to_string());
    }
    let method = method.to_string();
    let target = target.to_string();

    let mut authorization: Option<String> = None;
    let mut header_lines = 0usize;
    let mut header_bytes = 0usize;
    loop {
        let mut header = String::new();
        let read = read_capped_line(reader, &mut header, MAX_HEADER_LINE_BYTES)
            .map_err(|_| "cannot read the HTTP request headers".to_string())?;
        if read == 0 {
            return Err("HTTP request headers did not end correctly".to_string());
        }
        if read > MAX_HEADER_LINE_BYTES {
            return Err("HTTP request header line too long".to_string());
        }
        let trimmed = header.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        header_lines += 1;
        header_bytes += read;
        if header_lines > MAX_HEADER_LINES {
            return Err("too many HTTP request headers".to_string());
        }
        if header_bytes > MAX_HEADER_TOTAL_BYTES {
            return Err("HTTP request headers too large".to_string());
        }
        let Some((name, value)) = trimmed.split_once(':') else {
            return Err("invalid HTTP request header format".to_string());
        };
        if name.eq_ignore_ascii_case("authorization") {
            let value = value.trim().to_string();
            match &authorization {
                Some(existing) if existing != &value => {
                    return Err("duplicate Authorization request headers with conflicting values".to_string());
                }
                _ => authorization = Some(value),
            }
        }
    }

    Ok(ParsedRequest {
        method,
        target,
        authorization,
    })
}

/// Read one line, at most `limit` bytes. Over the limit, return `limit + 1` so the
/// caller treats it as too long. The remaining bytes are not read into memory.
pub(super) fn read_capped_line<R: BufRead>(
    reader: &mut R,
    out: &mut String,
    limit: usize,
) -> io::Result<usize> {
    let mut total = 0usize;
    let mut byte = [0u8; 1];
    loop {
        let read = reader.read(&mut byte)?;
        if read == 0 {
            break;
        }
        total += 1;
        if total > limit {
            return Ok(limit + 1);
        }
        out.push(byte[0] as char);
        if byte[0] == b'\n' {
            break;
        }
    }
    Ok(total)
}

pub(super) fn load_workout_series(
    data_dir: &Path,
    workout_id: &str,
) -> Result<Option<WorkoutSeries>, String> {
    let db_path = data_dir.join("zepp.db");
    if !db_path.exists() {
        // No data was synced yet. This is "no such record", not a service fault.
        return Ok(None);
    }
    let db = Database::open_read_only(db_path)
        .map_err(|error| format!("failed to open the local database: {error}"))?;
    if db
        .get_workout_detail(workout_id)
        .map_err(|error| format!("failed to query workout records: {error}"))?
        .is_none()
    {
        return Ok(None);
    }
    db.get_workout_series(workout_id)
        .map(Some)
        .map_err(|error| format!("failed to read workout series: {error}"))
}

pub fn route_request<F>(request: &ParsedRequest, token: &str, lookup: F) -> HttpResponse
where
    F: FnOnce(&str) -> Result<Option<WorkoutSeries>, String>,
{
    // Auth comes before the method and route checks: an unauthorized request must
    // not use the 405/404 difference to probe which routes the local API supports.
    let Some(provided) = request.authorization.as_deref().and_then(bearer_value) else {
        return HttpResponse::unauthorized();
    };
    if !tokens_match(token, provided) {
        return HttpResponse::unauthorized();
    }

    if request.method != "GET" {
        return HttpResponse::method_not_allowed();
    }
    let target = request.target.as_str();
    let path = target.split_once('?').map_or(target, |(path, _)| path);
    if path == "/" {
        return HttpResponse::json(
            200,
            "OK",
            json!({
                "service": "ZeppBridge local API",
                "version": env!("CARGO_PKG_VERSION"),
                "status": "ok",
                "base_url": LOCAL_API_BASE_URL,
                "authentication": "Authorization: Bearer <token>",
                "endpoints": {
                    "health": "/health",
                    "workout_series": "/workouts/{id}/series",
                    "records": "/v1/records",
                    "v1": "/v1"
                }
            }),
        );
    }
    if path == "/health" {
        return health_response();
    }

    let parts = path.split('/').collect::<Vec<_>>();
    if parts.len() != 4 || !parts[0].is_empty() || parts[1] != "workouts" || parts[3] != "series" {
        return HttpResponse::not_found("route_not_found", "this local API route was not found");
    }
    let workout_id = match decode_workout_id(parts[2]) {
        Ok(value) => value,
        Err(message) => return HttpResponse::bad_request("invalid_workout_id", message),
    };

    match lookup(&workout_id) {
        Ok(Some(series)) => HttpResponse::json(200, "OK", series),
        Ok(None) => HttpResponse::not_found("workout_not_found", "this workout id is not in the local database"),
        Err(error) => {
            eprintln!("local API failed to read workout series: {error}");
            HttpResponse::json(
                500,
                "Internal Server Error",
                json!({
                    "error": {
                        "code": "local_data_unavailable",
                        "message": "cannot read local workout data right now"
                    }
                }),
            )
        }
    }
}

pub(super) fn bearer_value(header: &str) -> Option<&str> {
    let (scheme, value) = header.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

pub(super) fn decode_workout_id(raw: &str) -> Result<String, &'static str> {
    if raw.is_empty() || raw.len() > MAX_WORKOUT_ID_BYTES * 3 {
        return Err("workout id must not be empty or longer than 256 bytes");
    }
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err("workout id has invalid percent encoding");
            }
            let high = hex_value(bytes[index + 1]).ok_or("workout id has invalid percent encoding")?;
            let low = hex_value(bytes[index + 2]).ok_or("workout id has invalid percent encoding")?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    if decoded.is_empty() || decoded.len() > MAX_WORKOUT_ID_BYTES {
        return Err("workout id must not be empty or longer than 256 bytes");
    }
    if decoded.iter().any(|byte| *byte == 0 || *byte == b'/') {
        return Err("workout id must not contain path separators or null bytes");
    }
    String::from_utf8(decoded).map_err(|_| "workout id is not valid UTF-8")
}

pub(super) fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub struct HttpResponse {
    pub status: u16,
    pub(super) reason: &'static str,
    pub body: Vec<u8>,
    pub(super) allow_get: bool,
    pub(super) challenge: bool,
    pub(super) retry_after: Option<u64>,
}

impl HttpResponse {
    pub(super) fn json<T: Serialize>(status: u16, reason: &'static str, value: T) -> Self {
        let body = serde_json::to_vec(&value).unwrap_or_else(|_| {
            r#"{"error":{"code":"serialization_failed","message":"cannot generate the JSON response"}}"#
                .as_bytes()
                .to_vec()
        });
        Self {
            status,
            reason,
            body,
            allow_get: false,
            challenge: false,
            retry_after: None,
        }
    }

    pub(super) fn forbidden(code: &str, message: &str) -> Self {
        Self::json(
            403,
            "Forbidden",
            json!({ "error": { "code": code, "message": message } }),
        )
    }

    pub(super) fn too_many_requests(seconds: u64) -> Self {
        let mut response = Self::json(
            429,
            "Too Many Requests",
            json!({ "error": {
                "code": "rate_limited",
                "message": "too many requests or failed sign-ins; retry later"
            }}),
        );
        response.retry_after = Some(seconds);
        response
    }

    pub(super) fn bad_request(code: &str, message: &str) -> Self {
        Self::json(
            400,
            "Bad Request",
            json!({ "error": { "code": code, "message": message } }),
        )
    }

    /// No token, a wrong token and an old token take the same path. The response
    /// does not reveal the expected value and does not tell "missing" from "wrong".
    pub(super) fn unauthorized() -> Self {
        let mut response = Self::json(
            401,
            "Unauthorized",
            json!({
                "error": {
                    "code": "unauthorized",
                    "message": "Authorization: Bearer <token> is required. Desktop app: the local API token from Settings. zeppbridge-cli serve: the pull token."
                }
            }),
        );
        response.challenge = true;
        response
    }

    pub(super) fn not_found(code: &str, message: &str) -> Self {
        Self::json(
            404,
            "Not Found",
            json!({ "error": { "code": code, "message": message } }),
        )
    }

    pub(super) fn method_not_allowed() -> Self {
        let mut response = Self::json(
            405,
            "Method Not Allowed",
            json!({
                "error": {
                    "code": "method_not_allowed",
                    "message": "the local API supports GET only"
                }
            }),
        );
        response.allow_get = true;
        response
    }
}

pub fn write_response<W: Write>(stream: &mut W, response: HttpResponse) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n",
        response.status,
        response.reason,
        response.body.len()
    )?;
    if response.allow_get {
        write!(stream, "Allow: GET\r\n")?;
    }
    if response.challenge {
        write!(stream, "WWW-Authenticate: Bearer\r\n")?;
    }
    if let Some(seconds) = response.retry_after {
        write!(stream, "Retry-After: {seconds}\r\n")?;
    }
    write!(stream, "\r\n")?;
    stream.write_all(&response.body)?;
    stream.flush()
}
