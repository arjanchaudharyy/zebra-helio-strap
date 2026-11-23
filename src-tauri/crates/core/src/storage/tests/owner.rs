//! Database ownership: one database holds the data of only one account. Switching accounts must never mix databases silently.

use super::*;
use crate::models::CapabilityStatus;

fn with_one_raw_record(db: &Database) {
    db.insert_raw_record(&RawRecord {
        stream: "heart_rate".into(),
        source_key: "owner-test".into(),
        source_scope: SourceScope::UserFused,
        device_id: None,
        start_utc: ts(),
        end_utc: None,
        payload: serde_json::json!({"items": []}),
        capability: CapabilityStatus::Verified,
    })
    .unwrap();
}

#[test]
fn an_empty_library_is_claimed_by_the_first_account_and_refuses_others() {
    let db = Database::in_memory().unwrap();
    assert_eq!(db.library_owner().unwrap(), None);
    db.claim_library_for_login("1000", &[]).unwrap();
    assert_eq!(db.library_owner().unwrap().as_deref(), Some("1000"));

    // The same account (connected once via the legacy channel and once via the official channel) works as usual.
    db.claim_library_for_login("1000", &["1000".into()])
        .unwrap();
    db.claim_library_for_sync("1000").unwrap();

    // Another account: both sign-in and sync are refused, and the owner does not change.
    assert!(matches!(
        db.claim_library_for_login("2000", &[]),
        Err(ZeppBridgeError::AccountMismatch)
    ));
    assert!(matches!(
        db.claim_library_for_sync("2000"),
        Err(ZeppBridgeError::AccountMismatch)
    ));
    assert_eq!(db.library_owner().unwrap().as_deref(), Some("1000"));
}

#[test]
fn an_upgraded_library_with_data_refuses_a_new_account_that_differs_from_the_configured_one() {
    let db = Database::in_memory().unwrap();
    with_one_raw_record(&db);
    // Old database with data and no owner: the configured account is 1000, a new sign-in as 2000 -> refused, not claimed.
    assert!(matches!(
        db.claim_library_for_login("2000", &["1000".into()]),
        Err(ZeppBridgeError::AccountMismatch)
    ));
    assert_eq!(db.library_owner().unwrap(), None);
    // The same account signs in again -> claimed (presumed ownership).
    db.claim_library_for_login("1000", &["1000".into()])
        .unwrap();
    assert_eq!(db.library_owner().unwrap().as_deref(), Some("1000"));
    assert_eq!(
        db.get_app_meta("library_owner_source").unwrap().as_deref(),
        Some("upgrade")
    );
}

#[test]
fn the_first_sync_after_upgrade_claims_the_library_for_the_syncing_account() {
    let db = Database::in_memory().unwrap();
    with_one_raw_record(&db);
    db.claim_library_for_sync("1000").unwrap();
    assert_eq!(db.library_owner().unwrap().as_deref(), Some("1000"));
    assert!(db.claim_library_for_sync("2000").is_err());
}
