//! Runs when "Delete app data" is checked during uninstall.
//!
//! The Tauri NSIS template deletes only the folders named after the package under `%APPDATA%` and `%LOCALAPPDATA%`.
//! But the ZeppBridge library is in `data\` next to the install folder, and the tokens are in the system credential manager. So the option
//! deleted nothing, and after a reinstall the old account and devices came back.
//!
//! Before it deletes the exe, the uninstaller calls this program once with [`PURGE_CREDENTIALS_ARG`] to delete the tokens
//! (entry names use the account ID, and only this program knows which accounts this data folder registered).
//! Then it deletes `data\` itself (`src-tauri/windows/hooks.nsh`).

use crate::paths;
use zeppbridge_core::auth::{default_credential_backend_in, AuthManager, CredentialBackend};
use zeppbridge_core::local_api::{LOCAL_API_CREDENTIAL_ACCOUNT, PULL_CREDENTIAL_ACCOUNT};
use zeppbridge_core::official::OfficialStore;
use zeppbridge_core::push;

pub const PURGE_CREDENTIALS_ARG: &str = "--purge-credentials";

/// Credential store entries that belong to no Zepp account and have a fixed account name: the local API token and
/// the pull token of headless `serve`.
///
/// They are not registered in `auth.json` or `official.json`, so the two steps above cannot find them. Before, the local API token
/// stayed in the credential manager after an uninstall with "Delete app data" checked.
const FIXED_CREDENTIAL_ACCOUNTS: &[&str] = &[LOCAL_API_CREDENTIAL_ACCOUNT, PULL_CREDENTIAL_ACCOUNT];

/// Deletes the tokens of the two channels registered in this data folder, and the local API token. Touches only the credential
/// store and the two metadata files. The uninstaller deletes the database with the folder. Best effort: the uninstall must not
/// get stuck on this step.
pub fn purge_credentials_for_uninstall() {
    let Ok(data_dir) = paths::resolve_data_dir() else {
        return;
    };
    let _ = AuthManager::new(data_dir.clone()).clear_auth();
    let _ = OfficialStore::new(&data_dir).disconnect();
    // Push target secrets are named by target ID (`push:<id>`). The list is in data\push\targets.json,
    // so read it before the uninstaller deletes data\.
    let push_targets: Vec<String> = push::load_targets(&data_dir)
        .map(|targets| targets.into_iter().map(|target| target.id).collect())
        .unwrap_or_default();
    purge_unlisted_accounts(
        default_credential_backend_in(&data_dir).as_ref(),
        &push_targets,
    );
}

/// Deletes entries one by one. One failure does not stop the next.
fn purge_unlisted_accounts(backend: &dyn CredentialBackend, push_targets: &[String]) {
    let push_accounts = push_targets.iter().map(|id| push::secret_account(id));
    for account in FIXED_CREDENTIAL_ACCOUNTS
        .iter()
        .map(|account| account.to_string())
        .chain(push_accounts)
    {
        let _ = backend.delete(&account);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Memory {
        entries: Mutex<HashMap<String, String>>,
    }

    impl CredentialBackend for Memory {
        fn set(&self, user_id: &str, token: &str) -> Result<(), String> {
            self.entries
                .lock()
                .unwrap()
                .insert(user_id.into(), token.into());
            Ok(())
        }
        fn get(&self, user_id: &str) -> Result<Option<String>, String> {
            Ok(self.entries.lock().unwrap().get(user_id).cloned())
        }
        fn delete(&self, user_id: &str) -> Result<(), String> {
            self.entries.lock().unwrap().remove(user_id);
            Ok(())
        }
    }

    #[test]
    fn uninstall_removes_api_tokens_and_push_secrets() {
        let backend = Memory::default();
        backend
            .set(LOCAL_API_CREDENTIAL_ACCOUNT, "zbk_fixture")
            .unwrap();
        backend.set(PULL_CREDENTIAL_ACCOUNT, "zbp_fixture").unwrap();
        backend.set("push:home-nas", "{}").unwrap();
        backend.set("zepp-user-1", "kept-by-this-step").unwrap();
        purge_unlisted_accounts(&backend, &["home-nas".to_string()]);
        assert_eq!(backend.get(LOCAL_API_CREDENTIAL_ACCOUNT).unwrap(), None);
        assert_eq!(backend.get(PULL_CREDENTIAL_ACCOUNT).unwrap(), None);
        assert_eq!(backend.get("push:home-nas").unwrap(), None);
        // Account tokens are cleared by AuthManager and OfficialStore. This step does not touch their entries.
        assert!(backend.get("zepp-user-1").unwrap().is_some());
    }
}
