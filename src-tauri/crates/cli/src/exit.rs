//! Exit code contract: maps errors to exit codes and error kinds, finds the data directory and gives hints (split out of main.rs, same logic).

use super::*;

/// Map a core error to an exit code.
///
/// The classes let scheduler scripts write logic like "on busy, wait five minutes and retry; on cloud failure, back off;
/// on database error, raise an alert", instead of guessing from the same 1.
pub(super) fn exit_code_for(error: &ZeppBridgeError) -> (u8, &'static str) {
    match error {
        ZeppBridgeError::AuthError(_) | ZeppBridgeError::NeedsReauth(_) => {
            (EXIT_NOT_CONFIGURED, "auth")
        }
        ZeppBridgeError::NetworkError(_)
        | ZeppBridgeError::RetryExhausted { .. }
        | ZeppBridgeError::HttpStatus { .. }
        | ZeppBridgeError::Unavailable(_)
        | ZeppBridgeError::TimedOut(_) => (EXIT_CLOUD, "cloud"),
        ZeppBridgeError::DataUnavailable(_) => (EXIT_FAILED, "data_unavailable"),
        ZeppBridgeError::Busy(_) => (EXIT_BUSY, "busy"),
        ZeppBridgeError::DatabaseError(_) => (EXIT_DATABASE, "database"),
        ZeppBridgeError::ConfigError(_) | ZeppBridgeError::InvalidHost(_) => (EXIT_USAGE, "usage"),
        // "The database must be upgraded first" and "cannot get the token" are different: retry does not help the first, which needs
        // a reprocess first (scheduler scripts use this code to stop retrying). The second needs a person to supply credentials.
        // A database newer than the program is also 7 (docs: "or upgrade the command line to the same version"). Retry does not help.
        ZeppBridgeError::Headless(
            HeadlessProblem::SchemaUpgradeRequired { .. } | HeadlessProblem::ProgramOutdated { .. },
        ) => (EXIT_SCHEMA, "schema"),
        ZeppBridgeError::Headless(_) => (EXIT_NOT_CONFIGURED, "auth"),
        // The database belongs to another account: retry does not help. A person must change the data directory or switch back to the original account.
        ZeppBridgeError::AccountMismatch => (EXIT_NOT_CONFIGURED, "account_mismatch"),
        _ => (EXIT_FAILED, "failed"),
    }
}

/// The line that the command line user sees for this error.
///
/// `user_message()` used to be Chinese. The desktop app gets localized text by error code, so it never had
/// a problem. The command line has no i18n layer, so printing it directly gave English users Chinese text. That is how the
/// Linux user in issue #40 got Chinese messages about the system keyring and a v19 local database.
///
/// So here each variant gets its own English text. For the variants with free text (config, auth, data unavailable, ...),
/// the original text came from hundreds of places in core and cannot be translated line by line. The English line says which kind of problem it is.
/// If the original text is ASCII, we append it. If it is Chinese, we use the stable error code instead. Mixed languages in one line are
/// harder to read than one language (see the note at the top of main.rs), and the error code is searchable in docs and feedback.
///
/// The match has no `_` arm: when core adds a new variant, this does not compile, which forces an English line.
/// Every arm with original text goes through core's redaction first (removes addresses, limits length). We must not write a Display
/// that has a request address to stdout / stderr as is.
pub(super) fn user_text(error: &ZeppBridgeError) -> String {
    let code = error.code();
    match error {
        ZeppBridgeError::Headless(problem) => problem.english(),
        ZeppBridgeError::WorkoutFitOriginalMissing => "The original details are missing. Check an earlier backup or the original activity file.".into(),
        ZeppBridgeError::AccountMismatch => "This database already holds another Zepp account's             data, so nothing was written for this account. To switch accounts, move or rename             the data folder, then connect again."
            .to_string(),
        ZeppBridgeError::TimedOut(_) => {
            "This sync ran out of time; the remaining streams will continue next time.".to_string()
        }
        ZeppBridgeError::NetworkError(_) => {
            "Could not reach the Zepp region service. Check the network and try again.".to_string()
        }
        ZeppBridgeError::NeedsReauth(_) => "The saved Zepp sign-in is no longer valid. Reconnect             the account in the desktop app, or supply a fresh ZEPPBRIDGE_APP_TOKEN, then try again."
            .to_string(),
        ZeppBridgeError::RetryExhausted { status, .. } => {
            format!("The Zepp service is temporarily unavailable (HTTP {status}). Try again later.")
        }
        ZeppBridgeError::HttpStatus { status, .. } => {
            format!("The Zepp service returned HTTP {status}. Try again later.")
        }
        ZeppBridgeError::CloudRejected {
            code: cloud_code,
            message,
        } => with_detail(
            &format!(
                "Zepp's cloud rejected the request (cloud code {cloud_code}); if this keeps                  happening, reconnect the Zepp account in the desktop app"
            ),
            message,
            code,
        ),
        ZeppBridgeError::Cancelled => "The sync was cancelled.".to_string(),
        ZeppBridgeError::ParseError(_) => "The data Zepp returned could not be parsed.".to_string(),
        // SQLite error class, extended code and original text (English), plus a masked path when open fails.
        // With only "temporarily unavailable", the exit 6 that one user reported could not be traced.
        ZeppBridgeError::DatabaseError(inner) => format!(
            "The local database is temporarily unavailable ({}).",
            zeppbridge_core::storage::sqlite_error_detail(inner)
        ),
        ZeppBridgeError::IoError(_) => "Reading or writing a local file failed.".to_string(),
        ZeppBridgeError::Busy(_) => "Another ZeppBridge process is writing to this library.             Try again in a moment."
            .to_string(),
        ZeppBridgeError::AuthError(message) => with_detail("Authentication failed", message, code),
        ZeppBridgeError::CredentialStore(message) => with_detail(
            "The system credential store refused to read or write the token",
            message,
            code,
        ),
        ZeppBridgeError::InvalidHost(message) => {
            with_detail("The Zepp region host is not allowed", message, code)
        }
        ZeppBridgeError::ConfigError(message) => {
            with_detail("Configuration problem", message, code)
        }
        ZeppBridgeError::Unavailable(message) | ZeppBridgeError::DataUnavailable(message) => {
            with_detail(
                "Not available: this library or account does not provide the requested data",
                message,
                code,
            )
        }
        ZeppBridgeError::TrainingPlan { message, .. } => {
            with_detail("The training-plan action failed", message, code)
        }
        ZeppBridgeError::AiTask(inner) => with_detail(
            "The AI task could not be completed",
            &inner.user_message(),
            code,
        ),
        ZeppBridgeError::Unknown(message) => with_detail("Unexpected error", message, code),
    }
}

/// The English line, plus the redacted original text (when ASCII) or the error code (when the original text is Chinese).
fn with_detail(lead: &str, detail: &str, code: &str) -> String {
    // Core redaction used to replace addresses with a Chinese "[address hidden]" placeholder (escaped below). The CLI replaces it with English, so that
    // an English original is not treated as Chinese and dropped because of the placeholder. Current core already writes "[address hidden]".
    let detail = sanitize_user_text(detail).replace("[\u{5df2}\u{9690}\u{85cf}\u{5730}\u{5740}]", "[address hidden]");
    if detail.is_empty() {
        format!("{lead}.")
    } else if detail.chars().any(is_cjk) {
        format!("{lead} ({code}).")
    } else {
        format!("{lead}: {detail}")
    }
}

/// CJK unified ideographs, CJK punctuation and fullwidth forms: if any one is present, treat the line as Chinese.
pub(super) fn is_cjk(character: char) -> bool {
    matches!(character, '\u{3000}'..='\u{303f}' | '\u{3400}'..='\u{9fff}' | '\u{f900}'..='\u{faff}' | '\u{ff00}'..='\u{ffef}')
}

/// `ExportScope::validated()` lives in core for the GUI. It used to return Chinese by convention:
/// the UI gets localized text by `code()`, and the original text is only the fallback (see its own
/// doc comment). But the CLI has no such layer. It puts the text straight into a usage error, so Chinese would appear
/// in English command line output. This is the same problem that the note on `user_text()` solves.
///
/// The message set is much smaller than the `ZeppBridgeError` variants, so we match the original text directly. The keys below are the
/// old Chinese core messages, written as Unicode escapes. An unknown message that is Chinese becomes a general English line; anything else passes through as is.
/// No guessing, no recursion. Current core returns English, which passes through unchanged.
pub(super) fn translate_export_scope_error(message: String) -> String {
    match message.as_str() {
        "\u{5bfc}\u{51fa}\u{5f00}\u{59cb}\u{65e5}\u{671f}\u{65e0}\u{6548}" => "Invalid export start date",
        "\u{5bfc}\u{51fa}\u{7ed3}\u{675f}\u{65e5}\u{671f}\u{65e0}\u{6548}" => "Invalid export end date",
        "\u{5bfc}\u{51fa}\u{7ed3}\u{675f}\u{65e5}\u{671f}\u{4e0d}\u{80fd}\u{65e9}\u{4e8e}\u{5f00}\u{59cb}\u{65e5}\u{671f}" => {
            "Export end date cannot be before the start date"
        }
        "\u{5355}\u{6b21}\u{5bfc}\u{51fa}\u{8303}\u{56f4}\u{4e0d}\u{80fd}\u{8d85}\u{8fc7} 366 \u{5929}" => {
            "A single export cannot span more than 366 days"
        }
        "workout id \u{4e0d}\u{80fd}\u{4e3a}\u{7a7a}" => "Workout id cannot be empty",
        // The text changed or a new arm was added: we do not guess which one, but we also do not print Chinese.
        _ if message.chars().any(is_cjk) => "Invalid export range or workout id",
        _ => return message,
    }
    .to_string()
}

/// The machine-readable error kind for each exit code, for `--json` output.
pub(super) fn error_kind_for(code: u8) -> &'static str {
    match code {
        EXIT_NOT_CONFIGURED => "not_configured",
        EXIT_BUSY => "busy",
        EXIT_CLOUD => "cloud",
        EXIT_DATABASE => "database",
        EXIT_SCHEMA => "schema_mismatch",
        EXIT_USAGE => "usage",
        _ => "failed",
    }
}

/* ------------------------------ Shared setup ------------------------------ */

pub(super) fn data_dir() -> Result<std::path::PathBuf, String> {
    paths::resolve_data_dir()
        .map_err(|error| format!("Could not resolve the data directory: {error}"))
}

/// The "where we looked" line, added after every "not found".
///
/// Issue #40 lost two full hours on this. That user ran the CLI under
/// `~/ZeppBridge/src-tauri/target/release/`, and
/// `paths::is_build_artifact_dir()` saw that `target/release` is a build output directory
/// and redirected the data directory to `~/ZeppBridge/data/` at the **repo root**. That rule is correct
/// (or `cargo run` would drop a multi-GB database into the build cache), but the program said nothing.
/// He put the data folder next to the exe and saw "no local database",
/// with no way to know where to put it. In the end a maintainer told him by hand.
///
/// So every "not found" must include the resolved path. For a build directory it must also explain
/// why the path is not next to the exe.
/// What exactly is missing when "no account is connected".
///
/// To connect an account you need two things: `auth.json` (user id and region), and the token in the credential
/// store. Before, whichever one was missing, this printed the same line "set ZEPPBRIDGE_CREDENTIAL_STORE
/// =env and supply ZEPPBRIDGE_APP_TOKEN". But the user in issue #40 **had set both**.
/// The missing piece was `auth.json`. The program told him to do again what he had already done, for four rounds.
///
/// Now we first check whether `auth.json` exists, then choose which line to show.
pub(super) fn not_connected_message(dir: &std::path::Path) -> String {
    let auth_file = dir.join("auth.json");
    if !auth_file.is_file() {
        return format!(
            "No Zepp account is connected: {} does not exist. It holds the account id \
             and region, and it is not created by the command line -- sign in from the \
             desktop app, or copy auth.json here from a machine that already has one. \
             Note that the token is NOT in that file: it lives in the platform \
             credential store and does not copy across machines, so supply it with \
             ZEPPBRIDGE_CREDENTIAL_STORE=env and ZEPPBRIDGE_APP_TOKEN. {}",
            auth_file.display(),
            where_it_looked(dir)
        );
    }
    format!(
        "No Zepp account is connected. The command line does not sign in: sign in from \
         the desktop app, or set ZEPPBRIDGE_CREDENTIAL_STORE=env and supply \
         ZEPPBRIDGE_APP_TOKEN. Moving a library between machines: see \
         docs/guides/linux.md. {}",
        where_it_looked(dir)
    )
}

pub(super) fn where_it_looked(dir: &std::path::Path) -> String {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf));
    where_it_looked_from(dir, exe_dir.as_deref())
}

/// Same as above, but the caller gives the exe location. In tests `current_exe()` points at the test binary,
/// and the whole point of this branch is "the exe is in a build directory", which a test run cannot produce.
pub(super) fn where_it_looked_from(
    dir: &std::path::Path,
    exe_dir: Option<&std::path::Path>,
) -> String {
    let mut text = format!("Looked in: {}", dir.display());
    if exe_dir.is_some_and(paths::is_build_artifact_dir) {
        text.push_str(
            ". That is not next to the executable because this binary sits in a build \
             directory (target/debug, target/release or a cargo target cache), and \
             ZeppBridge keeps data out of build directories -- it uses the repository \
             data/ folder instead",
        );
    }
    text.push_str(&format!(". Override it with {}", paths::DATA_DIR_ENV));
    text
}
