//! Database ownership: one data directory holds the data of only one Zepp account.
//!
//! Natural keys of raw records carry no account (stream + source_key, official per-day keys, sleep ID).
//! Two accounts writing to one database would overwrite each other and mix on one timeline, and neither the UI nor exports could tell
//! whose row is whose. So the database records its owner (`app_meta`) on the first write by an account. After that:
//!
//! - Check before sync: a different owner is refused, and not one row is written;
//! - Check before saving new credentials: a different owner is refused, and the user must first move or rename the `data` folder.
//!
//! The legacy channel and the official channel share one user ID space (verified to match), so the IDs compare directly.
//! **Never clear an existing database automatically.** To switch accounts, the user must move the old database explicitly.

use super::*;

/// User ID of the database owner.
const LIBRARY_OWNER_KEY: &str = "library_owner_user_id";
/// How the owner was assigned: `first_write` (first write to an empty database) / `upgrade` (data existed before the upgrade,
/// claimed by the account that kept syncing it. Ownership is presumed, not verified).
const LIBRARY_OWNER_SOURCE_KEY: &str = "library_owner_source";

impl Database {
    /// Database owner. An old database has none before its first sync.
    pub fn library_owner(&self) -> Result<Option<String>> {
        self.get_app_meta(LIBRARY_OWNER_KEY)
    }

    fn has_any_raw_record(&self) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT 1 FROM raw_records LIMIT 1", [], |_| Ok(()))
            .optional()?
            .is_some())
    }

    fn set_library_owner(&self, user_id: &str, source: &str) -> Result<()> {
        self.set_app_meta(LIBRARY_OWNER_KEY, user_id)?;
        self.set_app_meta(LIBRARY_OWNER_SOURCE_KEY, source)
    }

    /// Before a sync write: with no owner, claim this account (an old database is claimed by the account that kept syncing it).
    /// A different owner is refused. The caller must hold the write lock.
    pub fn claim_library_for_sync(&self, user_id: &str) -> Result<()> {
        let user_id = user_id.trim();
        match self.library_owner()? {
            Some(owner) if owner == user_id => Ok(()),
            Some(_) => Err(ZeppBridgeError::AccountMismatch),
            None => {
                let source = if self.has_any_raw_record()? {
                    "upgrade"
                } else {
                    "first_write"
                };
                self.set_library_owner(user_id, source)
            }
        }
    }

    /// Before saving new credentials: `known_accounts` are the accounts already configured in this data directory
    /// (legacy channel `auth.json`, official `official.json`).
    ///
    /// - Has an owner: must be the same account;
    /// - No owner, database has data: the data most likely came from the configured account. A new account that differs
    ///   is refused. With no configured account to compare, claim the new account (presumed);
    /// - Empty database: claim it directly.
    ///
    /// The caller must hold the write lock.
    pub fn claim_library_for_login(&self, user_id: &str, known_accounts: &[String]) -> Result<()> {
        let user_id = user_id.trim();
        match self.library_owner()? {
            Some(owner) if owner == user_id => Ok(()),
            Some(_) => Err(ZeppBridgeError::AccountMismatch),
            None if !self.has_any_raw_record()? => self.set_library_owner(user_id, "first_write"),
            None => {
                let others = known_accounts
                    .iter()
                    .map(|account| account.trim())
                    .any(|account| !account.is_empty() && account != user_id);
                if others {
                    return Err(ZeppBridgeError::AccountMismatch);
                }
                self.set_library_owner(user_id, "upgrade")
            }
        }
    }
}

/// Account IDs already configured in this data directory (one for the legacy channel and one official, possibly the same).
/// Reads only metadata files. Does not touch the credential store.
pub fn configured_accounts(data_dir: &std::path::Path) -> Vec<String> {
    let mut accounts = Vec::new();
    if let Some(user_id) = crate::auth::AuthManager::new(data_dir.to_path_buf()).saved_user_id() {
        accounts.push(user_id);
    }
    if let Ok(Some(meta)) = crate::official::OfficialStore::new(data_dir).meta() {
        accounts.push(meta.user_id);
    }
    accounts
}
