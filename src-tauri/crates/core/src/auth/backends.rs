//! Credential backends: Windows Credential Manager, macOS keychain, Secret Service, environment variable and file (split out of auth/mod.rs, same logic).

use super::*;

/// A small abstraction around the platform credential store.  Keeping this
/// boundary explicit makes tests deterministic without ever putting a token
/// in `auth.json`.
pub trait CredentialBackend: Send + Sync {
    fn set(&self, user_id: &str, token: &str) -> std::result::Result<(), String>;
    fn get(&self, user_id: &str) -> std::result::Result<Option<String>, String>;
    fn delete(&self, user_id: &str) -> std::result::Result<(), String>;

    /// Whether it can store several entries by name. The env backend returns the same legacy-channel
    /// token for any name, so an official OAuth token stored in it would read back as the legacy token. So it says no.
    fn supports_named_entries(&self) -> bool {
        true
    }
}

/// Carry the real failure reason out of the system credential store.
///
/// Every call site was once `map_err(|_| "cannot write to Windows Credential Manager")`: the low-level error was fully
/// dropped, including the Win32 error code and clear reasons such as "a field is too long".
/// A user reported only "cannot write", and neither we nor they could act on it.
#[cfg(any(windows, unix))]
pub(super) fn describe_keyring_error(action: &str, error: &keyring::Error) -> String {
    match error {
        keyring::Error::TooLong(attribute, limit) => {
            format!("{action}: {attribute} exceeds the system limit {limit}")
        }
        keyring::Error::Invalid(attribute, reason) => format!("{action}: {attribute} {reason}"),
        keyring::Error::NoStorageAccess(inner) => {
            format!("{action}: the system denied access to the credential store ({inner})")
        }
        keyring::Error::PlatformFailure(inner) => format!("{action}: {inner}"),
        other => format!("{action}: {other}"),
    }
}

#[cfg(windows)]
#[derive(Debug, Default)]
pub struct WindowsCredentialBackend;

#[cfg(windows)]
impl CredentialBackend for WindowsCredentialBackend {
    fn set(&self, user_id: &str, token: &str) -> std::result::Result<(), String> {
        let entry = keyring::Entry::new(CREDENTIAL_SERVICE, user_id)
            .map_err(|error| describe_keyring_error("Cannot open Windows Credential Manager entry", &error))?;
        entry
            .set_password(token)
            .map_err(|error| describe_keyring_error("Cannot write to Windows Credential Manager", &error))
    }

    fn get(&self, user_id: &str) -> std::result::Result<Option<String>, String> {
        let entry = keyring::Entry::new(CREDENTIAL_SERVICE, user_id)
            .map_err(|error| describe_keyring_error("Cannot open Windows Credential Manager entry", &error))?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(describe_keyring_error(
                "Cannot read Windows Credential Manager",
                &error,
            )),
        }
    }

    fn delete(&self, user_id: &str) -> std::result::Result<(), String> {
        let entry = keyring::Entry::new(CREDENTIAL_SERVICE, user_id)
            .map_err(|error| describe_keyring_error("Cannot open Windows Credential Manager entry", &error))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(describe_keyring_error(
                "Cannot delete Windows Credential Manager entry",
                &error,
            )),
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug, Default)]
pub struct MacOsCredentialBackend;

#[cfg(target_os = "macos")]
impl CredentialBackend for MacOsCredentialBackend {
    fn set(&self, user_id: &str, token: &str) -> std::result::Result<(), String> {
        let entry = keyring::Entry::new(CREDENTIAL_SERVICE, user_id)
            .map_err(|error| describe_keyring_error("Cannot open macOS keychain entry", &error))?;
        entry
            .set_password(token)
            .map_err(|error| describe_keyring_error("Cannot write to macOS keychain", &error))
    }

    fn get(&self, user_id: &str) -> std::result::Result<Option<String>, String> {
        let entry = keyring::Entry::new(CREDENTIAL_SERVICE, user_id)
            .map_err(|error| describe_keyring_error("Cannot open macOS keychain entry", &error))?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(describe_keyring_error("Cannot read macOS keychain", &error)),
        }
    }

    fn delete(&self, user_id: &str) -> std::result::Result<(), String> {
        let entry = keyring::Entry::new(CREDENTIAL_SERVICE, user_id)
            .map_err(|error| describe_keyring_error("Cannot open macOS keychain entry", &error))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(describe_keyring_error("Cannot delete macOS keychain entry", &error)),
        }
    }
}

/// Select credential storage: `keychain` / `file` on macOS, or
/// `secret-service` / `file` / `env` on Linux. Windows uses Credential Manager.
/// File storage must be opted into; a locked system store never enables it.
pub const CREDENTIAL_STORE_ENV: &str = "ZEPPBRIDGE_CREDENTIAL_STORE";

/// A token given directly by the environment (read-only store).
pub const APP_TOKEN_ENV: &str = "ZEPPBRIDGE_APP_TOKEN";

/// File name of the file store, in the data directory.
#[cfg(unix)]
pub const CREDENTIAL_FILE: &str = "credentials.json";

/// Secret Service (GNOME Keyring / KWallet). The default on Linux desktops.
///
/// It uses `org.freedesktop.secrets` over D-Bus. A Flatpak sandbox needs
/// `--talk-name=org.freedesktop.secrets`. The manifest already grants it.
#[cfg(all(unix, not(target_os = "macos")))]
#[derive(Debug, Default)]
pub struct SecretServiceCredentialBackend;

#[cfg(all(unix, not(target_os = "macos")))]
impl CredentialBackend for SecretServiceCredentialBackend {
    fn set(&self, user_id: &str, token: &str) -> std::result::Result<(), String> {
        let entry = keyring::Entry::new(CREDENTIAL_SERVICE, user_id)
            .map_err(|error| describe_secret_service_error("Cannot open system keyring entry", &error))?;
        entry
            .set_password(token)
            .map_err(|error| describe_secret_service_error("Cannot write to system keyring", &error))
    }

    fn get(&self, user_id: &str) -> std::result::Result<Option<String>, String> {
        let entry = keyring::Entry::new(CREDENTIAL_SERVICE, user_id)
            .map_err(|error| describe_secret_service_error("Cannot open system keyring entry", &error))?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(describe_secret_service_error("Cannot read system keyring", &error)),
        }
    }

    fn delete(&self, user_id: &str) -> std::result::Result<(), String> {
        let entry = keyring::Entry::new(CREDENTIAL_SERVICE, user_id)
            .map_err(|error| describe_secret_service_error("Cannot open system keyring entry", &error))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(describe_secret_service_error(
                "Cannot delete system keyring entry",
                &error,
            )),
        }
    }
}

/// Add a note after the keyring's own text about "what is probably wrong on this machine".
///
/// With no Secret Service, the raw error is a D-Bus level message (cannot reach the session bus,
/// interface not implemented). A person who reads it on a headless server or container cannot infer
/// "this machine should not use a keyring at all". So the other two options are written out here.
#[cfg(all(unix, not(target_os = "macos")))]
pub(super) fn describe_secret_service_error(action: &str, error: &keyring::Error) -> String {
    let base = describe_keyring_error(action, error);
    match error {
        // Use `HeadlessProblem`, so it has its own error code. The CLI has no i18n layer
        // and can show English only by code. The Linux user in issue #40 got the
        // raw Chinese text of this message on an English CLI.
        keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_) => {
            format!("{HEADLESS_NO_STORE_MARKER}{base}")
        }
        _ => base,
    }
}

/// The token comes from an environment variable and is read-only.
///
/// Made for containers and systemd units: both have ready, better ways to deliver secrets than a file
/// (`docker secret`, `LoadCredential=`), so the token does not need another copy on disk.
///
/// A write is not a "failure" but "nowhere to write": the caller gives the env var, and the process cannot change
/// it. So `set` is a no-op only when the value matches the current one (the legacy migration path in `load_auth`
/// calls it once like this), and returns an error otherwise.
#[cfg(all(unix, not(target_os = "macos")))]
#[derive(Debug, Default)]
pub struct EnvCredentialBackend;

#[cfg(all(unix, not(target_os = "macos")))]
impl EnvCredentialBackend {
    pub(super) fn token() -> Option<String> {
        let value = std::env::var(APP_TOKEN_ENV).ok()?;
        let value = value.trim().to_string();
        if value.is_empty() {
            None
        } else {
            Some(value)
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
impl CredentialBackend for EnvCredentialBackend {
    fn set(&self, _user_id: &str, token: &str) -> std::result::Result<(), String> {
        match Self::token() {
            Some(existing) if existing == token.trim() => Ok(()),
            _ => Err(format!(
                "{CREDENTIAL_STORE_ENV}=env is a read-only store: the token comes from {APP_TOKEN_ENV}, \
                 and the process cannot change the caller's environment. To save the token on this machine use \
                 {CREDENTIAL_STORE_ENV}=file"
            )),
        }
    }

    fn get(&self, _user_id: &str) -> std::result::Result<Option<String>, String> {
        Ok(Self::token())
    }

    fn supports_named_entries(&self) -> bool {
        false
    }

    fn delete(&self, _user_id: &str) -> std::result::Result<(), String> {
        // "Already gone" and "deleted" mean the same to the caller. The environment never had it, so
        // an error would only make clear_auth fail for no reason.
        match Self::token() {
            None => Ok(()),
            Some(_) => Err(format!(
                "{CREDENTIAL_STORE_ENV}=env is a read-only store: remove {APP_TOKEN_ENV} from the deployment config"
            )),
        }
    }
}

/// The token is written to a 0600 file in the data directory.
///
/// This is **a clear downgrade**, not an option equal to the keyring: the token in the file is protected only by file
/// permissions, and any process that can read the file gets it. It is still offered because on headless Linux,
/// or on macOS with a keychain that cannot be unlocked, the system store may not work at all. Putting the token in
/// shell history or somewhere `docker inspect` can see is worse.
///
/// So it must be chosen explicitly (`ZEPPBRIDGE_CREDENTIAL_STORE=file`). It is never enabled silently when the keyring
/// is not available.
#[cfg(unix)]
#[derive(Debug)]
pub struct FileCredentialBackend {
    pub(super) path: PathBuf,
}

#[cfg(unix)]
struct FileCredentialWriteGuard {
    // Closing the descriptor releases flock, including after a process crash.
    _file: fs::File,
}

#[cfg(unix)]
#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct StoredCredentials {
    #[serde(default = "default_credential_file_version")]
    pub(super) version: u32,
    #[serde(default)]
    pub(super) tokens: std::collections::BTreeMap<String, String>,
}

#[cfg(unix)]
pub(super) fn default_credential_file_version() -> u32 {
    1
}

#[cfg(unix)]
impl FileCredentialBackend {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join(CREDENTIAL_FILE),
        }
    }

    fn lock_write(&self) -> std::result::Result<FileCredentialWriteGuard, String> {
        use std::os::{
            fd::AsRawFd,
            unix::fs::{OpenOptionsExt, PermissionsExt},
        };
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "Invalid credential directory".to_string())?;
        fs::create_dir_all(parent).map_err(|error| format!("Cannot create credential directory: {error}"))?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("Cannot protect credential directory: {error}"))?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(parent.join("credentials.json.lock"))
            .map_err(|error| format!("Cannot lock credential file: {error}"))?;
        loop {
            // Serialize the whole read-modify-write across official/Advanced
            // logins and CLI/MCP processes so one token cannot overwrite another.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
                return Ok(FileCredentialWriteGuard { _file: file });
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(format!("Cannot lock credential file: {error}"));
            }
        }
    }

    pub(super) fn read(&self) -> std::result::Result<StoredCredentials, String> {
        match fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
                // Do not fall back to "treat as empty": that makes a parse failure look like
                // "you are not signed in". The user pairs again as told, and the old file is overwritten.
                format!("{} failed to parse: {error}", self.path.display())
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Ok(StoredCredentials::default())
            }
            Err(error) => Err(format!("Cannot read {}: {error}", self.path.display())),
        }
    }

    pub(super) fn write(&self, stored: &StoredCredentials) -> std::result::Result<(), String> {
        let json = serde_json::to_vec_pretty(stored)
            .map_err(|error| format!("Cannot serialize credential file: {error}"))?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| format!("{} has no parent directory", self.path.display()))?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("Cannot create {}: {error}", parent.display()))?;
        // Tighten the directory too. The file is 0600, but a 0755 parent directory lets others see
        // "a credential is here", and leaves the file-replace path open.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                .map_err(|error| format!("Cannot protect credential directory {}: {error}", parent.display()))?;
        }

        let temp = parent.join(format!(
            ".{CREDENTIAL_FILE}.tmp-{}-{}",
            std::process::id(),
            crate::unique_suffix()
        ));
        let result = (|| -> io::Result<()> {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            // The temp file is 0600 from the moment it is created. Creating it with default permissions and then chmod
            // leaves a window in which the token is world-readable.
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temp)?;
            file.write_all(&json)?;
            file.sync_all()?;
            drop(file);
            replace_file(&temp, &self.path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result.map_err(|error| format!("Cannot write {}: {error}", self.path.display()))
    }
}

#[cfg(unix)]
impl CredentialBackend for FileCredentialBackend {
    fn set(&self, user_id: &str, token: &str) -> std::result::Result<(), String> {
        let _guard = self.lock_write()?;
        let mut stored = self.read()?;
        stored.version = default_credential_file_version();
        stored.tokens.insert(user_id.to_string(), token.to_string());
        self.write(&stored)
    }

    fn get(&self, user_id: &str) -> std::result::Result<Option<String>, String> {
        Ok(self.read()?.tokens.get(user_id).cloned())
    }

    fn delete(&self, user_id: &str) -> std::result::Result<(), String> {
        let _guard = self.lock_write()?;
        let mut stored = self.read()?;
        if stored.tokens.remove(user_id).is_none() {
            return Ok(());
        }
        if stored.tokens.is_empty() {
            // Do not leave an empty file after the last token is deleted. If it stays, the next automatic
            // store choice picks the file store again because "the file exists", but what the user just did
            // was "disconnect".
            return match fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(format!("Cannot delete {}: {error}", self.path.display())),
            };
        }
        self.write(&stored)
    }
}

/// `ZEPPBRIDGE_CREDENTIAL_STORE` has a value that is not recognized.
///
/// Do not fall back silently to the default: treating `ZEPPBRIDGE_CREDENTIAL_STORE=secretservice` as
/// "not set" lets one typo silently change where the token is stored.
#[cfg(unix)]
#[derive(Debug)]
pub(super) struct InvalidCredentialStoreBackend {
    pub(super) value: String,
}

#[cfg(unix)]
impl InvalidCredentialStoreBackend {
    pub(super) fn error(&self) -> String {
        #[cfg(target_os = "macos")]
        let choices = "keychain, file";
        #[cfg(not(target_os = "macos"))]
        let choices = "secret-service, file, env";
        format!(
            "{CREDENTIAL_STORE_ENV} value is not recognized: {}. Valid values are {choices}",
            self.value
        )
    }
}

#[cfg(unix)]
impl CredentialBackend for InvalidCredentialStoreBackend {
    fn set(&self, _user_id: &str, _token: &str) -> std::result::Result<(), String> {
        Err(self.error())
    }

    fn get(&self, _user_id: &str) -> std::result::Result<Option<String>, String> {
        Err(self.error())
    }

    fn delete(&self, _user_id: &str) -> std::result::Result<(), String> {
        Err(self.error())
    }
}

/// A platform that is not Windows/macOS and not unix. No known credential store is available.
#[cfg(all(not(windows), not(unix)))]
#[derive(Debug, Default)]
pub(super) struct UnavailableCredentialBackend;

#[cfg(all(not(windows), not(unix)))]
impl CredentialBackend for UnavailableCredentialBackend {
    fn set(&self, _user_id: &str, _token: &str) -> std::result::Result<(), String> {
        Err("No credential store is available on this platform. Tests must inject a CredentialBackend".to_string())
    }

    fn get(&self, _user_id: &str) -> std::result::Result<Option<String>, String> {
        Err("No credential store is available on this platform. Tests must inject a CredentialBackend".to_string())
    }

    fn delete(&self, _user_id: &str) -> std::result::Result<(), String> {
        Err("No credential store is available on this platform. Tests must inject a CredentialBackend".to_string())
    }
}

/// File-store tests run on both macOS and Linux CI, without system credentials.
#[cfg(all(test, unix))]
mod unix_credential_tests {
    use super::*;

    /// Each test gets its own directory. A shared one would let the "delete the file when the last token is deleted"
    /// path affect other tests.
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zeppbridge-cred-{tag}-{}-{}",
            std::process::id(),
            crate::unique_suffix()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_file_store_round_trips_a_token() {
        let dir = temp_dir("roundtrip");
        let backend = FileCredentialBackend::new(&dir);

        assert_eq!(backend.get("user-1").unwrap(), None);
        backend.set("user-1", "token-1").unwrap();
        assert_eq!(backend.get("user-1").unwrap().as_deref(), Some("token-1"));

        // Switching accounts must not read the previous account's token.
        backend.set("user-2", "token-2").unwrap();
        assert_eq!(backend.get("user-1").unwrap().as_deref(), Some("token-1"));
        assert_eq!(backend.get("user-2").unwrap().as_deref(), Some("token-2"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_file_store_is_not_world_readable() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir("perms");
        let backend = FileCredentialBackend::new(&dir);
        backend.set("user-1", "token-1").unwrap();

        // This is the only protection this store has. If it gets loose, nothing else raises an alarm.
        let mode = fs::metadata(dir.join(CREDENTIAL_FILE))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "credential file mode is {mode:o}");

        let dir_mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700, "data directory mode is {dir_mode:o}");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn deleting_the_last_token_removes_the_file() {
        let dir = temp_dir("delete");
        let backend = FileCredentialBackend::new(&dir);
        backend.set("user-1", "token-1").unwrap();
        backend.set("user-2", "token-2").unwrap();

        backend.delete("user-1").unwrap();
        assert!(
            dir.join(CREDENTIAL_FILE).is_file(),
            "one token is left, so the file must stay"
        );

        // An empty file left behind would make the automatic store choice pick the file
        // store again because "the file exists", but the user just disconnected.
        backend.delete("user-2").unwrap();
        assert!(
            !dir.join(CREDENTIAL_FILE).exists(),
            "no empty file must stay after the last token is deleted"
        );

        // Deleting an account that is not there is not an error.
        backend.delete("user-3").unwrap();

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_corrupt_file_is_an_error_not_an_empty_store() {
        let dir = temp_dir("corrupt");
        fs::write(dir.join(CREDENTIAL_FILE), b"{ this is not json").unwrap();
        let backend = FileCredentialBackend::new(&dir);

        // Falling back to "empty" would disguise a parse failure as "you are not signed in". The user pairs
        // again as told, and the stored token gets overwritten.
        let error = backend.get("user-1").expect_err("a broken file must return an error");
        assert!(error.contains(CREDENTIAL_FILE), "{error}");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn an_unknown_store_name_refuses_instead_of_guessing() {
        let backend = InvalidCredentialStoreBackend {
            value: "secretservice-typo".to_string(),
        };
        for error in [
            backend.get("u").unwrap_err(),
            backend.set("u", "t").unwrap_err(),
            backend.delete("u").unwrap_err(),
        ] {
            // The error must list the valid values. If it only says "not recognized", the reader
            // has to read the source code to know what to write.
            assert!(error.contains("file"), "{error}");
            #[cfg(target_os = "macos")]
            assert!(error.contains("keychain"), "{error}");
            #[cfg(not(target_os = "macos"))]
            assert!(error.contains("secret-service"), "{error}");
            #[cfg(not(target_os = "macos"))]
            assert!(error.contains("env"), "{error}");
        }
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn the_env_store_refuses_writes_but_reports_a_matching_one_as_done() {
        // Do not touch real env vars: cargo runs tests in many threads of one process,
        // so set_var makes these tests interfere, and failures look like bugs in the code under test.
        // So this builds the return value semantics of both cases directly.
        let backend = EnvCredentialBackend;

        // With no token in the environment (true for this test process): get returns None,
        // delete is a no-op, and set is refused and points to the file store.
        assert_eq!(backend.get("user-1").unwrap(), None);
        backend.delete("user-1").unwrap();

        let error = backend.set("user-1", "token-1").unwrap_err();
        assert!(error.contains(APP_TOKEN_ENV), "{error}");
        assert!(
            error.contains("file"),
            "the error must name the store to use to save the token on this machine: {error}"
        );
    }
}
