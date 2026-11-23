use crate::models::{error::*, AuthInfo};

use reqwest::{header, Client, Url};

use serde_json::Value;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use std::sync::Arc;

use std::time::Duration;

mod policy;

pub use policy::*;

const MAX_ATTEMPTS: usize = 3;

const RETRY_BACKOFF_MS: [u64; 2] = [50, 150];

/// The maximum number of same-domain redirects one request follows.
///
/// Redirects and retries once shared one budget: a valid 3xx took `continue` and also used up one
/// network retry. If a region added two more redirect hops one day, the real request got no retries at all. The two
/// failure modes are different: a redirect loop must stop at once, and network jitter needs a backoff and a new try.
/// So each has its own count.
const MAX_REDIRECTS: usize = 5;

/// The redirect budget must be larger than the retry budget.
///
/// If it were smaller, the two would share one budget again, which is the bug this fixes.
/// This is a compile-time assertion, not a test: it is a relation between two constants and must not wait for a test run.
const _: () = assert!(MAX_REDIRECTS > MAX_ATTEMPTS);

/// The upper limit for 429 / 503 backoff.
///
/// The server's `Retry-After` is clamped to it too: a response with `Retry-After: 3600`
/// must not hang the sync for an hour. The user closes the window before that.
const MAX_BACKOFF_MS: u64 = 8_000;

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Zepp Cloud API connector.  The type is cloneable so network requests can
/// happen without holding a database mutex in the synchronizer.
#[derive(Clone)]
pub struct ZeppConnector {
    client: Client,
    auth: AuthInfo,
    base_url: Url,
    cancel: Arc<AtomicBool>,
}

impl ZeppConnector {
    pub fn new(auth: AuthInfo) -> Result<Self> {
        Self::with_cancel(auth, Arc::new(AtomicBool::new(false)))
    }

    /// Construct a connector whose requests abort early when `cancel` is set.
    pub fn with_cancel(auth: AuthInfo, cancel: Arc<AtomicBool>) -> Result<Self> {
        let base = validate_region_host(&auth.region_host)?;
        if auth.user_id.trim().is_empty()
            || !auth
                .user_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        {
            return Err(ZeppBridgeError::ConfigError("Invalid user_id".into()));
        }

        let mut defaults = header::HeaderMap::new();
        // The version comes from crate metadata, not a hand-written value. The hand-written one stayed at `0.2.1`
        // while the app was already at 1.x. Server logs, packet captures and rate-limit diagnostics always showed
        // a version that did not exist, so problems never matched a release.
        defaults.insert(
            header::USER_AGENT,
            header::HeaderValue::from_str(concat!("ZeppBridge/", env!("CARGO_PKG_VERSION")))
                .map_err(|_| ZeppBridgeError::ConfigError("Cannot build User-Agent".into()))?,
        );
        // Redirects are disabled so a 3xx can never forward the custom
        // `apptoken` header to a host outside the validated region. Manual,
        // same-origin redirect handling lives in `get_json`.
        //
        // Env/system proxies are disabled on purpose. This product talks to
        // Zepp directly; silently honouring HTTP_PROXY would let a local MITM
        // see `apptoken`. Embedders that need a proxy must not reuse this
        // constructor — there is no `with_client` bypass.
        let client = Client::builder()
            .default_headers(defaults)
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .no_proxy()
            .cookie_store(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let base_url = Url::parse(&base)
            .map_err(|_| ZeppBridgeError::InvalidHost("Host URL cannot be parsed".into()))?;
        let connector = Self {
            client,
            auth,
            base_url,
            cancel,
        };
        // Validate the token as a header value during construction, rather than
        // panicking later when the first request is made.
        connector.build_headers()?;
        Ok(connector)
    }

    pub fn build_headers(&self) -> Result<header::HeaderMap> {
        let mut headers = header::HeaderMap::new();
        let token = header::HeaderValue::from_str(&self.auth.app_token)
            .map_err(|_| ZeppBridgeError::ConfigError("app_token contains invalid header characters".into()))?;
        headers.insert("apptoken", token);
        headers.insert(
            "appname",
            header::HeaderValue::from_static("com.huami.midong"),
        );
        headers.insert("appplatform", header::HeaderValue::from_static("ios_phone"));
        headers.insert("accept", header::HeaderValue::from_static("*/*"));
        headers.insert("v", header::HeaderValue::from_static("2.0"));
        headers.insert("vn", header::HeaderValue::from_static("10.2.5"));
        headers.insert("cv", header::HeaderValue::from_static("1722_10.2.5"));
        headers.insert("vb", header::HeaderValue::from_static("202604132257"));
        headers.insert("lang", header::HeaderValue::from_static("en"));
        headers.insert("country", header::HeaderValue::from_static(""));
        headers.insert("timezone", header::HeaderValue::from_static("UTC"));
        Ok(headers)
    }

    fn path_url(&self, path: &str) -> Result<Url> {
        validate_api_path(path)?;
        let url = self
            .base_url
            .join(path)
            .map_err(|_| ZeppBridgeError::ConfigError("Cannot build API path".into()))?;
        if !same_region_origin(&url, &self.base_url) {
            return Err(ZeppBridgeError::ConfigError("Invalid API path".into()));
        }
        Ok(url)
    }

    /// Wait for backoff, and respond to cancel while waiting.
    ///
    /// With a plain `sleep`, a cancel during the backoff window takes effect only after the sleep ends.
    async fn backoff(&self, millis: u64) -> Result<()> {
        if millis == 0 {
            return Ok(());
        }
        // Poll the cancel flag in small steps. No cancellation token here, because this
        // connector's cancel is already an `AtomicBool` shared across threads.
        const TICK_MS: u64 = 50;
        let mut left = millis;
        while left > 0 {
            if self.cancel.load(Ordering::SeqCst) {
                return Err(ZeppBridgeError::Cancelled);
            }
            let step = left.min(TICK_MS);
            tokio::time::sleep(Duration::from_millis(step)).await;
            left -= step;
        }
        if self.cancel.load(Ordering::SeqCst) {
            return Err(ZeppBridgeError::Cancelled);
        }
        Ok(())
    }

    /// A future that is ready only when cancel is requested.
    ///
    /// It goes into `tokio::select!` with the network request. The cancel flag is an `AtomicBool`
    /// shared across threads and is not a future, so short-step polling makes it one.
    async fn cancelled(&self) {
        loop {
            if self.cancel.load(Ordering::SeqCst) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn request_id() -> String {
        let n = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("ZEPBRIDGE-{n:016X}")
    }

    /// Send one GET and parse the JSON.
    ///
    /// The two budgets are counted apart: `attempt` is network retries, `redirects` is same-domain redirects. When they were
    /// counted together, redirects silently used up retries. That was one source of reports like "sync sometimes fails,
    /// then works when I click again".
    async fn get_json(&self, path: &str, params: Vec<(&str, String)>) -> Result<Value> {
        self.get_json_timed(path, params, DEFAULT_TIMING).await
    }

    async fn get_json_timed(
        &self,
        path: &str,
        params: Vec<(&str, String)>,
        timing: RequestTiming,
    ) -> Result<Value> {
        self.get_json_with_headers(path, params, timing, &[]).await
    }

    /// Same as [`Self::get_json_timed`], but a few fixed request headers take values from the caller
    /// (now only the strength exercise catalog changes `lang` / `cv` / `vn`). Only existing header names can be overridden.
    /// This cannot add new credential headers other than `apptoken`.
    async fn get_json_with_headers(
        &self,
        path: &str,
        params: Vec<(&str, String)>,
        timing: RequestTiming,
        overrides: &[(&'static str, String)],
    ) -> Result<Value> {
        let mut url = self.path_url(path)?;
        let mut headers = self.build_headers()?;
        for (name, value) in overrides {
            if !matches!(*name, "lang" | "cv" | "vn") {
                return Err(ZeppBridgeError::ConfigError("This request header cannot be overridden".into()));
            }
            let value = header::HeaderValue::from_str(value)
                .map_err(|_| ZeppBridgeError::ConfigError("Request header contains invalid characters".into()))?;
            headers.insert(*name, value);
        }
        let mut last_retry_status = None;
        let mut attempt = 0usize;
        let mut redirects = 0usize;

        while attempt < MAX_ATTEMPTS {
            // How long to wait if this try fails. For 429/503 the server's `Retry-After`
            // replaces it.
            let backoff_ms = RETRY_BACKOFF_MS.get(attempt).copied().unwrap_or(0);
            if self.cancel.load(Ordering::SeqCst) {
                return Err(ZeppBridgeError::Cancelled);
            }
            let mut query = params.clone();
            query.push(("r", Self::request_id()));
            // Select on cancel and the request together. Cancel was once checked only between streams, so a request
            // already sent waited for its full timeout. On a bad network, the user pressed cancel and still
            // watched the app spin for tens of seconds. When `select!` takes the cancel branch, the request future is
            // dropped and the connection closes.
            let send = tokio::time::timeout(
                timing.guard(),
                self.client
                    .get(url.clone())
                    .headers(headers.clone())
                    .query(&query)
                    .timeout(timing.timeout)
                    .send(),
            );
            let outcome = tokio::select! {
                // Check cancel first: when both are ready, take cancel. Do not decode a response
                // the user already gave up on.
                biased;
                _ = self.cancelled() => return Err(ZeppBridgeError::Cancelled),
                outcome = send => outcome,
            };
            let response = match outcome {
                Ok(Ok(response)) => response,
                Ok(Err(error)) => {
                    attempt += 1;
                    if timing.may_retry(attempt, error.is_timeout()) {
                        self.backoff(backoff_ms).await?;
                        continue;
                    }
                    return Err(ZeppBridgeError::NetworkError(error));
                }
                Err(_elapsed) => {
                    attempt += 1;
                    if timing.may_retry(attempt, true) {
                        self.backoff(backoff_ms).await?;
                        continue;
                    }
                    return Err(ZeppBridgeError::RetryExhausted {
                        status: 504,
                        message: "Request timed out".into(),
                    });
                }
            };

            let status = response.status().as_u16();
            // Redirects are disabled at the client level so the custom
            // `apptoken` header is never forwarded cross-origin by reqwest.
            // Follow a 3xx manually only when the target stays on the same
            // HTTPS host as the validated regional base URL.
            if (300..=399).contains(&status) {
                // Redirects use their own budget and **do not change `attempt`**.
                redirects += 1;
                if redirects > MAX_REDIRECTS {
                    return Err(ZeppBridgeError::HttpStatus {
                        status,
                        message: "Too many redirects".into(),
                    });
                }
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok());
                match location {
                    Some(location) => match url.join(location) {
                        Ok(next) if same_region_origin(&next, &self.base_url) => {
                            url = next;
                            continue;
                        }
                        _ => {
                            return Err(ZeppBridgeError::HttpStatus {
                                status,
                                message: "Redirect target is not in an allowed region".into(),
                            })
                        }
                    },
                    None => {
                        return Err(ZeppBridgeError::HttpStatus {
                            status,
                            message: "Redirect has no target address".into(),
                        })
                    }
                }
            }
            match classify_status(status) {
                None => {
                    let body = read_json_body(response).await?;
                    if let Some(error) = classify_business_code(&body) {
                        return Err(error);
                    }
                    return Ok(body);
                }
                Some(ZeppBridgeError::NeedsReauth(message)) => {
                    return Err(ZeppBridgeError::NeedsReauth(message));
                }
                Some(ZeppBridgeError::Unavailable(message)) => {
                    return Err(ZeppBridgeError::Unavailable(message));
                }
                Some(ZeppBridgeError::RetryExhausted { .. }) => {
                    last_retry_status = Some(status);
                    // Wait as long as the server says (clamped to the limit). Use our own
                    // exponential backoff only when it says nothing. 429 and 503 once waited only 50/150 ms,
                    // which is no backoff. Three retries against a service that is rate limiting only
                    // make the rate-limit window last longer.
                    let wait = retry_after_ms(response.headers())
                        .unwrap_or_else(|| exponential_backoff_ms(attempt, backoff_ms));
                    attempt += 1;
                    if attempt < MAX_ATTEMPTS {
                        self.backoff(wait).await?;
                        continue;
                    }
                    return Err(ZeppBridgeError::RetryExhausted {
                        status,
                        message: "Service temporarily unavailable, retry limit reached".into(),
                    });
                }
                Some(error) => return Err(error),
            }
        }

        Err(ZeppBridgeError::RetryExhausted {
            status: last_retry_status.unwrap_or(503),
            message: "Service temporarily unavailable".into(),
        })
    }

    pub async fn fetch_devices(&self) -> Result<Value> {
        let path = format!("/users/{}/devices", self.auth.user_id);
        self.get_json(
            &path,
            vec![
                ("enableMultiDevice", "true".into()),
                ("device_type", "android_phone".into()),
            ],
        )
        .await
    }

    /// Real Zepp endpoint: `/users/{id}/heartRate`.
    pub async fn fetch_heart_rate(
        &self,
        start_timestamp: i64,
        end_timestamp: i64,
    ) -> Result<Value> {
        self.fetch_heart_rate_with_options(start_timestamp, end_timestamp, 1000, 2)
            .await
    }

    pub async fn fetch_heart_rate_with_options(
        &self,
        start_timestamp: i64,
        end_timestamp: i64,
        limit: i64,
        hr_type: i64,
    ) -> Result<Value> {
        let path = format!("/users/{}/heartRate", self.auth.user_id);
        self.get_json(
            &path,
            vec![
                ("startTime", start_timestamp.to_string()),
                ("endTime", end_timestamp.to_string()),
                ("limit", limit.max(1).to_string()),
                ("type", hr_type.to_string()),
            ],
        )
        .await
    }

    /// Candidate blood-pressure surface, with the date parameters used by
    /// m4ary/zepp-health-cli. Still unverified on a blood-pressure account.
    pub async fn fetch_blood_pressure_days(&self, days: i64, to_date: &str) -> Result<Value> {
        self.get_json(
            "/users/me/bloodPressure",
            vec![
                ("days", days.clamp(1, 365).to_string()),
                ("toDate", to_date.to_owned()),
                (
                    "sourceArrayStr",
                    "com.huami.midong.associated,com.huami.midong".into(),
                ),
            ],
        )
        .await
    }

    /// Alternative parameter contract used by Thejuampi/icu. Keep this
    /// separate: an empty response to one contract does not validate the other.
    pub async fn fetch_blood_pressure_range(&self, from_ms: i64, to_ms: i64) -> Result<Value> {
        self.get_json(
            "/users/me/bloodPressure",
            vec![("from", from_ms.to_string()), ("to", to_ms.to_string())],
        )
        .await
    }

    /// Weight and body composition: `/users/{id}/members/{member}/weightRecords`.
    ///
    /// **A different surface, not another event type.** ZeppBridge probed
    /// `/v2/users/me/events?eventType=weight&subType=summary` for a year and got
    /// an empty page every time, which the UI reported — correctly, for what it
    /// was asked — as "No measurement in the last 365 days", to four people who
    /// owned a scale and had years of readings sitting in the Zepp app. Verified
    /// on a live account 2026-09-04: the v2 page returns `items: []` while this
    /// one returns the records.
    ///
    /// **The window is in Unix *seconds*.** Every event surface above takes
    /// milliseconds; this one does not, and passing milliseconds asks for a
    /// window fifty thousand years wide — which the server answers with nothing.
    /// That is the easiest way to reintroduce the exact bug this method exists
    /// to fix, so the seconds are in the parameter names.
    ///
    /// `member_id` is `-1` for the account holder. A scale can be shared and
    /// `/users/{id}/members` lists the rest; only the account holder is read,
    /// because the others are other people. Records of other members can still
    /// come back on this path; `Normalizer::normalize_weight` filters them.
    pub async fn fetch_weight_records(
        &self,
        member_id: &str,
        from_seconds: i64,
        to_seconds: i64,
        limit: i64,
    ) -> Result<Value> {
        let member_id = validate_member_id(member_id)?;
        let path = format!(
            "/users/{}/members/{member_id}/weightRecords",
            self.auth.user_id
        );
        self.get_json_timed(
            &path,
            vec![
                ("fromTime", from_seconds.to_string()),
                ("toTime", to_seconds.to_string()),
                ("limit", limit.max(1).to_string()),
                // 0 = newest first. A truncated page then still holds the
                // readings most likely to be new to us.
                ("isForward", "0".to_owned()),
            ],
            WEIGHT_TIMING,
        )
        .await
    }

    /// Real raw band synchronization endpoint.  Its payload may be compressed;
    /// callers must not infer sleep from it unless normalization verifies it.
    pub async fn fetch_band_data(
        &self,
        from_date: &str,
        to_date: &str,
        query_type: &str,
        byte_length: i64,
        device_type: i64,
    ) -> Result<Value> {
        let path = "/v1/data/band_data.json";
        self.get_json(
            path,
            vec![
                ("userid", self.auth.user_id.clone()),
                ("from_date", from_date.to_owned()),
                ("to_date", to_date.to_owned()),
                ("query_type", query_type.to_owned()),
                ("byteLength", byte_length.max(0).to_string()),
                ("device_type", device_type.to_string()),
            ],
        )
        .await
    }

    /// Real workout summary endpoint. `sport` is the URL segment (run,
    /// walking, ride, swimming, …); track ids are the API's actual cursor
    /// parameters and are intentionally not called timestamps here.
    pub async fn fetch_sport_history(
        &self,
        sport: &str,
        start_track_id: i64,
        stop_track_id: i64,
        need_sub_data: i64,
    ) -> Result<Value> {
        if sport.is_empty()
            || !sport
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        {
            return Err(ZeppBridgeError::ConfigError("Invalid sport type".into()));
        }
        let path = format!("/v1/sport/{sport}/history.json");
        self.get_json(
            &path,
            vec![
                ("userid", self.auth.user_id.clone()),
                ("startTrackId", start_track_id.to_string()),
                ("stopTrackId", stop_track_id.to_string()),
                ("need_sub_data", need_sub_data.to_string()),
                ("type", String::new()),
            ],
        )
        .await
    }

    /// Strength exercise catalog (exercise name and primary / secondary muscle position numbers).
    ///
    /// The language comes only from the `lang` request header (`en_US`, `pt_BR`...). Query parameters do nothing. Without the app build
    /// version `cv`, Zepp returns only the old ~125 exercises. With it, you get the full set of more than one thousand
    /// (GitHub #163, tested by felipepmdias).
    pub async fn fetch_strength_catalog(&self, lang: &str) -> Result<Value> {
        if !crate::decoder::strength_catalog::is_valid_zepp_lang(lang) {
            return Err(ZeppBridgeError::ConfigError("Invalid language code".into()));
        }
        use crate::decoder::strength_catalog::{STRENGTH_CATALOG_CV, STRENGTH_CATALOG_VN};
        self.get_json_with_headers(
            "/sport/config/muscle",
            Vec::new(),
            DEFAULT_TIMING,
            &[
                ("lang", lang.to_string()),
                ("cv", STRENGTH_CATALOG_CV.to_string()),
                ("vn", STRENGTH_CATALOG_VN.to_string()),
            ],
        )
        .await
    }

    /// Single-workout delta payload. Path is always `/run/detail.json`
    /// regardless of sport; `trackid` + `source` come from history.
    pub async fn fetch_sport_detail(&self, track_id: &str, source: &str) -> Result<Value> {
        let track_id = validate_track_id(track_id)?;
        let source = validate_detail_source(source)?;
        self.get_json(
            "/v1/sport/run/detail.json",
            vec![("trackid", track_id), ("source", source)],
        )
        .await
    }

    pub async fn fetch_watch_statistics(
        &self,
        statistic: &str,
        start_day: &str,
        end_day: &str,
        limit: i64,
        reverse: bool,
    ) -> Result<Value> {
        let statistic = match statistic {
            "SPORT_LOAD" | "VO2_MAX" => statistic,
            _ => {
                return Err(ZeppBridgeError::ConfigError(
                    "Invalid WatchSportStatistics type".into(),
                ))
            }
        };
        let path = format!(
            "/v2/watch/users/{}/WatchSportStatistics/{statistic}",
            self.auth.user_id
        );
        self.get_json(
            &path,
            vec![
                ("startDay", start_day.to_owned()),
                ("endDay", end_day.to_owned()),
                ("limit", limit.max(1).to_string()),
                (
                    "isReverse",
                    if reverse { "true" } else { "false" }.to_owned(),
                ),
            ],
        )
        .await
    }

    /// `sub_type` is optional here, exactly as it is on `fetch_user_events`.
    ///
    /// Most v2 streams do name one, but not all of them: the `Food` stream is
    /// addressed by `eventType` alone. Sending a made-up `subType=real_data`
    /// to those returns an empty page, which reads as "this account has no
    /// food log" — a false negative that looks exactly like a true one.
    pub async fn fetch_events(
        &self,
        event_type: &str,
        sub_type: Option<&str>,
        from_ms: i64,
        to_ms: i64,
        limit: i64,
        reverse: bool,
    ) -> Result<Value> {
        let mut params = vec![("eventType", event_type.to_owned())];
        if let Some(sub_type) = sub_type {
            params.push(("subType", sub_type.to_owned()));
        }
        params.push(("from", from_ms.to_string()));
        params.push(("to", to_ms.to_string()));
        params.push(("limit", limit.max(1).to_string()));
        params.push(("reverse", if reverse { "1" } else { "0" }.to_owned()));
        self.get_json("/v2/users/me/events", params).await
    }

    /// The user-scoped event timeline: `/users/{id}/events`.
    ///
    /// This is a different surface from `/v2/users/me/events`, not a variant of
    /// it — blood oxygen, all-day stress and PAI live here and are invisible to
    /// the v2 path. Endpoint shape confirmed against two independent
    /// reverse-engineering projects (see `docs/reference/architecture.md`).
    /// `sub_type` is genuinely optional here: `all_day_stress` takes none.
    pub async fn fetch_user_events(
        &self,
        event_type: &str,
        sub_type: Option<&str>,
        from_ms: i64,
        to_ms: i64,
        limit: i64,
        reverse: bool,
    ) -> Result<Value> {
        let path = format!("/users/{}/events", self.auth.user_id);
        let mut params = vec![
            ("eventType", event_type.to_owned()),
            ("from", from_ms.to_string()),
            ("to", to_ms.to_string()),
            ("limit", limit.max(1).to_string()),
            ("reverse", if reverse { "1" } else { "0" }.to_owned()),
            ("userId", self.auth.user_id.clone()),
        ];
        if let Some(sub_type) = sub_type {
            params.push(("subType", sub_type.to_owned()));
        }
        self.get_json(&path, params).await
    }

    /// `/users/{id}/events/dateString` — the same timeline addressed by a
    /// calendar-date window plus an IANA timezone instead of epoch milliseconds.
    /// Timestamp strings can be rejected or silently return an empty page.
    /// The nightly SpO2 desaturation (`odi`) and apnea (`osa_event`) windows
    /// are only served here.
    pub async fn fetch_user_events_date_string(
        &self,
        event_type: &str,
        sub_type: &str,
        from_date: chrono::NaiveDate,
        to_date: chrono::NaiveDate,
        time_zone: &str,
        limit: i64,
    ) -> Result<Value> {
        let path = format!("/users/{}/events/dateString", self.auth.user_id);
        self.get_json(
            &path,
            vec![
                ("eventType", event_type.to_owned()),
                ("subType", sub_type.to_owned()),
                ("from", from_date.to_string()),
                ("to", to_date.to_string()),
                ("timeZone", time_zone.to_owned()),
                ("limit", limit.max(1).to_string()),
                ("reverse", "0".to_owned()),
                ("userId", self.auth.user_id.clone()),
            ],
        )
        .await
    }

    /// `/users/me/fileInfo/events` — an index of stored measurement files
    /// rather than the measurements themselves.
    ///
    /// Dense series (per-second heart rate, and possibly the continuous blood
    /// oxygen the app charts) are not served inline anywhere; this endpoint
    /// lists the files that hold them. Probing it answers whether a stream
    /// exists at all before any file is downloaded.
    pub async fn fetch_file_info_events(
        &self,
        event_type: &str,
        sub_type: &str,
        from_ms: i64,
        to_ms: i64,
        limit: i64,
    ) -> Result<Value> {
        self.get_json(
            "/users/me/fileInfo/events",
            vec![
                ("eventType", event_type.to_owned()),
                ("subType", sub_type.to_owned()),
                ("from", from_ms.to_string()),
                ("to", to_ms.to_string()),
                ("limit", limit.max(1).to_string()),
            ],
        )
        .await
    }

    pub async fn fetch_hrv(&self, start_date: &str, end_date: &str) -> Result<Value> {
        let start = chrono::NaiveDate::parse_from_str(start_date, "%Y-%m-%d")
            .map_err(|_| ZeppBridgeError::ConfigError("Invalid start_date".into()))?
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| ZeppBridgeError::ConfigError("Invalid start_date".into()))?
            .and_utc()
            .timestamp_millis();
        let end = chrono::NaiveDate::parse_from_str(end_date, "%Y-%m-%d")
            .map_err(|_| ZeppBridgeError::ConfigError("Invalid end_date".into()))?
            .and_hms_opt(23, 59, 59)
            .ok_or_else(|| ZeppBridgeError::ConfigError("Invalid end_date".into()))?
            .and_utc()
            .timestamp_millis();
        self.fetch_events("hrv_sdnn", Some("real_data"), start, end, 2000, true)
            .await
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod retry_policy_tests {
    use super::*;

    /// #157: the weight endpoint gets two minutes and no retry on timeout. The outer fallback is always longer than the per-request timeout.
    /// Other endpoints still retry on timeout.
    #[test]
    fn the_weight_endpoint_waits_longer_and_does_not_retry_a_timeout() {
        assert_eq!(WEIGHT_TIMING.timeout, Duration::from_secs(120));
        for timing in [DEFAULT_TIMING, WEIGHT_TIMING] {
            assert!(timing.guard() > timing.timeout);
        }
        assert!(!WEIGHT_TIMING.may_retry(1, true));
        assert!(WEIGHT_TIMING.may_retry(1, false), "connection failures still retry");
        assert!(DEFAULT_TIMING.may_retry(1, true));
        assert!(!DEFAULT_TIMING.may_retry(MAX_ATTEMPTS, false));
    }

    #[test]
    fn retry_after_seconds_are_honoured_and_capped() {
        let mut headers = header::HeaderMap::new();
        headers.insert(header::RETRY_AFTER, header::HeaderValue::from_static("2"));
        assert_eq!(retry_after_ms(&headers), Some(2_000));

        // When the server says wait one hour, do not really wait one hour.
        headers.insert(
            header::RETRY_AFTER,
            header::HeaderValue::from_static("3600"),
        );
        assert_eq!(retry_after_ms(&headers), Some(MAX_BACKOFF_MS));
    }

    #[test]
    fn a_past_http_date_means_retry_now_and_garbage_means_no_opinion() {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::RETRY_AFTER,
            header::HeaderValue::from_static("Wed, 21 Oct 2020 07:28:00 GMT"),
        );
        assert_eq!(retry_after_ms(&headers), Some(0));

        headers.insert(
            header::RETRY_AFTER,
            header::HeaderValue::from_static("soon-ish"),
        );
        assert_eq!(
            retry_after_ms(&headers),
            None,
            "an unreadable Retry-After must fall back to our own backoff, not count as 0"
        );

        assert_eq!(retry_after_ms(&header::HeaderMap::new()), None);
    }

    /// Backoff must really grow with the try count and always stay within the limit.
    #[test]
    fn exponential_backoff_grows_and_stays_bounded() {
        // Jitter is random, so assert on a range, not on an exact value.
        for attempt in 0..6 {
            let value = exponential_backoff_ms(attempt, 200);
            assert!(
                value <= MAX_BACKOFF_MS,
                "backoff {value} on try {attempt} is above the limit"
            );
        }
        let early: u64 = (0..12).map(|_| exponential_backoff_ms(0, 200)).sum();
        let later: u64 = (0..12).map(|_| exponential_backoff_ms(3, 200)).sum();
        assert!(later > early, "backoff did not grow with the try count: {early} -> {later}");

        // When the base is 0 (retry table used up), it must still wait a little. Otherwise the "backoff" is fake.
        assert!(exponential_backoff_ms(0, 0) > 0);
    }
}
