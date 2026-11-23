use thiserror::Error;

/// The failures that only a headless environment hits.
///
/// They are listed on their own because they need **their own error codes**.
/// The desktop app looks up localized text by code. The CLI has no i18n layer
/// and prints `user_message()` as is. With a code, the CLI can print its own
/// English text, and the desktop app keeps working in both languages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadlessProblem {
    /// No Secret Service runs on this machine. Headless servers and containers usually have none.
    NoCredentialStore {
        /// The raw keyring text. It tells "not installed" apart from "blocked by policy".
        detail: String,
    },
    /// The database schema is older than this program, and a read-only connection cannot upgrade it.
    SchemaUpgradeRequired { found: i64, required: i64 },
    /// The database schema is newer than this program: an old CLI or MCP reads the database.
    ///
    /// Example: the desktop app was upgraded, but the AI tool still points at an
    /// old `zeppbridge-mcp`. The user can only point the AI tool at the new
    /// version. So the text says how, and names **which program** reports it
    /// (redacted path and version), so the user can find the old entry.
    ProgramOutdated {
        found: i64,
        supported: i64,
        /// The redacted program path (`%USERPROFILE%\...`). Empty when unknown.
        program: String,
        version: String,
    },
    /// `auth.json` exists, but the credential store has no token for this account.
    ///
    /// The usual cause is not a broken credential. Usually **someone copied the
    /// whole data folder from another machine**. The database and auth.json copy,
    /// but the token does not. It stays in the other machine's credential
    /// manager, keychain or Secret Service.
    TokenNotInStore,
}

impl HeadlessProblem {
    fn code(&self) -> &'static str {
        match self {
            Self::NoCredentialStore { .. } => "err.headless.no_credential_store",
            Self::SchemaUpgradeRequired { .. } => "err.headless.schema_upgrade",
            Self::ProgramOutdated { .. } => "err.headless.program_outdated",
            Self::TokenNotInStore => "err.headless.token_not_in_store",
        }
    }

    /// The fallback text. The desktop UI looks up its own localized text by code.
    fn message(&self) -> String {
        match self {
            Self::NoCredentialStore { detail } => format!(
                "{detail}. There may be no Secret Service running on this machine \
                 (GNOME Keyring / KWallet). Headless servers and containers usually \
                 have none. Use ZEPPBRIDGE_CREDENTIAL_STORE=file (the token is \
                 written 0600 into the data directory), \
                 or ZEPPBRIDGE_CREDENTIAL_STORE=env with ZEPPBRIDGE_APP_TOKEN"
            ),
            Self::SchemaUpgradeRequired { found, required } => format!(
                "The local database is still v{found}, and this program needs v{required}. \
                 A read-only connection cannot upgrade it. On a headless machine, run \
                 `zeppbridge-cli reprocess` once. With the desktop app, start it once. \
                 Both make a backup before the upgrade. Then try again."
            ),
            Self::ProgramOutdated {
                found,
                supported,
                program,
                version,
            } => format!(
                "The local database is v{found}, but this program ({program} {version}) only \
                 supports up to v{supported}. It is older than the desktop app. Usually an AI \
                 tool still points at an old version. Reconnect that AI tool in the desktop \
                 app's settings (for Claude Desktop, generate and install ZeppBridge.mcpb \
                 again; for other tools, copy the connect command again). Remove the old \
                 extension or any entry that points at an old install folder, then restart \
                 the AI tool."
            ),
            Self::TokenNotInStore => "The auth metadata exists, but the credential manager \
                 has no token for this account. A database can be copied between machines; \
                 a token cannot. Sign in again, or set \
                 ZEPPBRIDGE_CREDENTIAL_STORE=file / =env and retry"
                .to_string(),
        }
    }

    /// The English text. The CLI prints this one because it has no i18n layer.
    pub fn english(&self) -> String {
        match self {
            Self::NoCredentialStore { detail } => format!(
                "{detail}. There may be no Secret Service running on this machine \
                 (GNOME Keyring / KWallet) -- headless servers and containers \
                 usually have none. Use ZEPPBRIDGE_CREDENTIAL_STORE=file (the token \
                 is written 0600 into the data directory) or \
                 ZEPPBRIDGE_CREDENTIAL_STORE=env together with ZEPPBRIDGE_APP_TOKEN"
            ),
            Self::SchemaUpgradeRequired { found, required } => format!(
                "This library is still v{found} and this build needs v{required}. A \
                 read-only connection cannot upgrade it: run `zeppbridge-cli \
                 reprocess` on a headless machine, or launch the desktop app once. \
                 Both take a backup before upgrading. Then try again."
            ),
            Self::ProgramOutdated {
                found,
                supported,
                program,
                version,
            } => format!(
                "This library is v{found}, but this program ({program} {version}) only \
                 understands up to v{supported} -- it is older than the desktop app. \
                 Usually an AI tool still points at an old copy: reconnect that tool \
                 from the desktop app's settings (for Claude Desktop, generate and \
                 install ZeppBridge.mcpb again; for other tools, copy the connect \
                 command again), remove the old extension or any entry pointing at an \
                 old install folder, then restart the AI tool."
            ),
            Self::TokenNotInStore => "The account metadata is here but the credential \
                 store has no token for it. A library copies between machines; a token \
                 does not. Sign in again, or set ZEPPBRIDGE_CREDENTIAL_STORE=file / \
                 =env and retry"
                .to_string(),
        }
    }
}

impl std::fmt::Display for HeadlessProblem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}

#[derive(Error, Debug)]
pub enum ZeppBridgeError {
    #[error("Authentication error: {0}")]
    AuthError(String),

    /// The failures that only a headless environment hits. See [`HeadlessProblem`].
    #[error("{0}")]
    Headless(HeadlessProblem),

    /// The system credential store (Windows Credential Manager / macOS Keychain)
    /// refused this read or write.
    ///
    /// It is separate from `AuthError` because the user fix is different. An auth
    /// error means "connect again". This one means "this machine cannot store the
    /// token": a group policy can block it, or the token can be too large for the
    /// credential manager.
    #[error("Credential store error: {0}")]
    CredentialStore(String),

    #[error("Network request failed: {0}")]
    NetworkError(#[from] reqwest::Error),

    /// The server explicitly told us that the saved credential cannot be used.
    /// Keeping this separate from a generic network error lets callers surface a
    /// re-authentication action without looking at an error string.
    #[error("Re-authentication required: {0}")]
    NeedsReauth(String),

    /// A request was well formed, but this regional account does not expose the
    /// requested capability (or the resource is not present).
    #[error("Data unavailable: {0}")]
    Unavailable(String),

    /// The user cancelled the in-flight sync. Kept distinct from a generic
    /// failure so callers can record a `cancelled` outcome instead of `failed`.
    #[error("Sync cancelled")]
    Cancelled,

    /// A retryable response remained retryable after the bounded retry budget.
    #[error("Zepp service temporarily unreachable (HTTP {status}): {message}")]
    RetryExhausted { status: u16, message: String },

    /// An endpoint returned a non-success status that is neither auth nor an
    /// optional/unavailable capability.
    #[error("Zepp service returned HTTP {status}: {message}")]
    HttpStatus { status: u16, message: String },

    /// HTTP is 200, but the payload itself says "not successful".
    ///
    /// It is separate from `HttpStatus`. That one is a transport rejection. This
    /// one is a transport success with a business rejection.
    ///
    /// It is also separate from `ParseError`. Before, these responses became a
    /// ParseError ("data cannot be parsed"). The user then saw no sign-in prompt
    /// and never got data.
    #[error("Zepp returned a business error code={code}: {message}")]
    CloudRejected { code: i64, message: String },

    #[error("Unsafe Zepp region host: {0}")]
    InvalidHost(String),

    #[error("Database error: {0}")]
    DatabaseError(#[from] rusqlite::Error),

    #[error("Data parse error: {0}")]
    ParseError(String),

    #[error("Data unavailable: {0}")]
    DataUnavailable(String),

    #[error("The workout details from before the correction are missing. Restore them from a backup or the original workout file if you have one.")]
    WorkoutFitOriginalMissing,

    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// A sync round used up its time budget. The remaining streams did not run this time.
    ///
    /// This is not a configuration problem. The next round continues the work.
    #[error("{0}")]
    TimedOut(String),

    /// Another process writes to the same database.
    ///
    /// It is separate from `ConfigError` because callers handle them differently.
    /// Busy means "come back later". A configuration error means "change something".
    /// Without the split, scheduler scripts must match on the error text.
    #[error("{0}")]
    Busy(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// This database already belongs to another Zepp account. Writes are refused and
    /// the database is not cleared. To switch accounts, the user moves the old `data` folder first.
    #[error("The database belongs to another Zepp account")]
    AccountMismatch,

    /// A business failure in the AI analysis task/template layer (`err.ai_task.*` / `err.ai_template.*`).
    /// [`crate::ai_tasks::AiTaskError`] sets the code and arguments. The command layer does not guess.
    #[error("{0}")]
    AiTask(#[from] crate::ai_tasks::AiTaskError),

    /// A business failure in the training plan layer (`err.training_plan.*`): no draft, already published, and so on.
    #[error("{message}")]
    TrainingPlan { code: &'static str, message: String },

    #[allow(dead_code)]
    #[error("Unknown error: {0}")]
    Unknown(String),
}

pub type Result<T> = std::result::Result<T, ZeppBridgeError>;

/// All `serde_json` failures become `ParseError`. A payload (de)serialization error
/// is the same class as "the cloud payload cannot be parsed".
impl From<serde_json::Error> for ZeppBridgeError {
    fn from(error: serde_json::Error) -> Self {
        Self::ParseError(format!("JSON processing failed: {error}"))
    }
}

impl ZeppBridgeError {
    pub fn needs_reauth(&self) -> bool {
        matches!(self, Self::NeedsReauth(_))
    }

    /// Another writer holds the database. It is retryable, not a failure.
    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Busy(_))
    }

    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable(_) | Self::DataUnavailable(_))
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }

    /// The stable error code.
    ///
    /// The UI looks up localized text by code. `user_message()` is only the
    /// fallback when no text is found.
    ///
    /// The codes are part of the public contract. A rename breaks the existing
    /// translations. A new code needs text in both languages (`npm run i18n:check`
    /// catches a missing one).
    pub fn code(&self) -> &'static str {
        match self {
            Self::NetworkError(_) => "err.core.network",
            Self::NeedsReauth(_) => "err.core.needs_reauth",
            Self::Unavailable(_) | Self::DataUnavailable(_) => "err.core.unavailable",
            Self::RetryExhausted { .. } => "err.core.retry_exhausted",
            Self::HttpStatus { .. } => "err.core.http_status",
            Self::CloudRejected { .. } => "err.core.cloud_rejected",
            Self::Cancelled => "err.core.cancelled",
            Self::AuthError(_) => "err.core.auth",
            Self::Headless(problem) => problem.code(),
            Self::CredentialStore(_) => "err.core.credential_store",
            Self::InvalidHost(_) => "err.core.invalid_host",
            Self::WorkoutFitOriginalMissing => "err.workout_fit.original_missing",
            Self::ConfigError(_) => "err.core.config",
            Self::TimedOut(_) => "err.core.timed_out",
            Self::Busy(_) => "err.core.busy",
            Self::ParseError(_) => "err.core.parse",
            Self::DatabaseError(_) => "err.core.database",
            Self::IoError(_) => "err.core.io",
            Self::AiTask(inner) => inner.code(),
            Self::TrainingPlan { code, .. } => code,
            Self::AccountMismatch => "err.core.account_mismatch",
            Self::Unknown(_) => "err.core.unknown",
        }
    }

    /// Short, token-free, URL-free copy for the desktop UI.
    ///
    /// The UI first uses `code()` to look up localized text; this is the fallback.
    /// The CLI and the logs always use it, whatever the UI language is.
    pub fn user_message(&self) -> String {
        match self {
            Self::NetworkError(_) => "Cannot connect to the Zepp region. Check the network and try again".into(),
            Self::NeedsReauth(_) => "Authentication expired. Reconnect Zepp".into(),
            Self::Unavailable(_) | Self::DataUnavailable(_) => {
                sanitize_user_text(&self.to_string())
            }
            Self::RetryExhausted { status, .. } => {
                format!("Zepp service temporarily unavailable (HTTP {status}). Try again later")
            }
            Self::HttpStatus { status, .. } => {
                format!("Zepp service returned HTTP {status}. Try again later")
            }
            // Include the code and the raw cloud text. This is the only way a
            // feedback report can show what the failure looks like.
            // `sanitize_user_text` removes any addresses in it.
            Self::CloudRejected { code, message } => sanitize_user_text(&format!(
                "The Zepp cloud rejected this request (code {code}): {message}. If this repeats, reconnect the Zepp account in Settings."
            )),
            Self::WorkoutFitOriginalMissing => self.to_string(),
            Self::Cancelled => "Sync cancelled".into(),
            Self::AuthError(message)
            | Self::CredentialStore(message)
            | Self::InvalidHost(message)
            | Self::ConfigError(message)
            | Self::TimedOut(message)
            | Self::Busy(message) => sanitize_user_text(message),
            // Do not use `sanitize_user_text`: it has a 140-character limit, and
            // these texts must name both environment variables and the command
            // in full. They are fixed constants with no addresses or cloud content.
            Self::Headless(problem) => problem.to_string(),
            // When the structure is unknown, include the top-level key names (names
            // only, no values). Without them, a feedback report gives no lead.
            Self::ParseError(detail) => match detail.split_once(PARSE_KEYS_MARKER) {
                Some((_, keys)) => sanitize_user_text(&format!(
                    "The data Zepp returned cannot be parsed (available fields: {})",
                    keys.trim()
                )),
                None => "The data Zepp returned cannot be parsed".into(),
            },
            // Include the SQLite error class and extended code (and the redacted
            // path when open fails). Without them, a feedback report has no lead.
            Self::DatabaseError(inner) => format!(
                "The local database is temporarily unavailable ({})",
                crate::storage::sqlite_error_detail(inner)
            ),
            Self::IoError(_) => "Failed to read or write a local file".into(),
            Self::AiTask(inner) => inner.user_message(),
            Self::TrainingPlan { message, .. } => sanitize_user_text(message),
            Self::AccountMismatch => "This database already holds data for another Zepp account. Nothing was written for the current account. \
                To switch accounts, quit ZeppBridge, move or rename the data folder, then reconnect."
                .into(),
            Self::Unknown(message) => sanitize_user_text(message),
        }
    }
}

/// In a `ParseError`, the payload's top-level key names follow this marker, and
/// `user_message` shows them as is. Only `extract_items` uses it, when it does not
/// know the structure. Only key names may follow the marker, never values.
pub const PARSE_KEYS_MARKER: &str = "available fields: ";

pub fn sanitize_user_text(source: &str) -> String {
    let without_url = regex_replace_urls(source);
    let trimmed = without_url.trim();
    if trimmed.chars().count() > 140 {
        format!("{}…", trimmed.chars().take(137).collect::<String>())
    } else {
        trimmed.to_string()
    }
}

fn regex_replace_urls(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(start) = rest.find("http://").or_else(|| rest.find("https://")) {
        output.push_str(&rest[..start]);
        output.push_str("[address hidden]");
        let after = &rest[start..];
        let end = after
            .find(|character: char| {
                character.is_whitespace() || matches!(character, ')' | '"' | '\'' | '>' | ']')
            })
            .unwrap_or(after.len());
        rest = &after[end..];
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_message_strips_request_urls() {
        let error = ZeppBridgeError::ConfigError(
            "error sending request for url (https://api-mifit.huami.com/users/abc123/heartRate)"
                .into(),
        );
        let message = error.user_message();
        assert!(!message.contains("abc123"));
        assert!(!message.contains("https://"));
        assert!(message.contains("address hidden"));
    }

    #[test]
    fn an_unrecognised_structure_names_its_top_level_keys() {
        let error = ZeppBridgeError::ParseError(format!(
            "response has no items/data array, {PARSE_KEYS_MARKER}code, message"
        ));
        assert_eq!(
            error.user_message(),
            "The data Zepp returned cannot be parsed (available fields: code, message)"
        );
        let other = ZeppBridgeError::ParseError("JSON processing failed: eof".into());
        assert_eq!(other.user_message(), "The data Zepp returned cannot be parsed");
    }
}
