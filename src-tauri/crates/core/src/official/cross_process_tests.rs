//! Cross-process cancel / disconnect / refresh (desktop and CLI / MCP touch the same official tokens at once).
//!
//! "Another process" is a real other process: the test starts its own test binary again and runs only
//! [`child_process_entry`]; an environment variable sets its role. The shared credential store
//! is a fake store in the data directory (one file per entry). The in-process `Mutex` cannot see the other side;
//! only the file lock in the data directory makes both sides queue.
//!
//! The parent process stops in the middle of a write-back (the credential store `set` blocks), then the child process disconnects /
//! marks "needs reauthorization". The child must wait until the parent finishes. The result is either "fully before" or
//! "fully after", never a half state where "metadata says connected but the token is gone".

use super::*;
use crate::auth::CredentialBackend;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const ROLE_ENV: &str = "ZEPPBRIDGE_OFFICIAL_CHILD_ROLE";
const DIR_ENV: &str = "ZEPPBRIDGE_OFFICIAL_CHILD_DIR";
const RELAY_ENV: &str = "ZEPPBRIDGE_OFFICIAL_CHILD_RELAY";
const CHILD_TEST: &str = "official::cross_process_tests::child_process_entry";

/// A fake credential store: each entry is a file under `secrets/`, so another process can read it too.
struct DiskBackend {
    dir: PathBuf,
}

impl DiskBackend {
    fn new(data_dir: &Path) -> Self {
        let dir = data_dir.join("secrets");
        std::fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    fn path(&self, key: &str) -> PathBuf {
        self.dir.join(key.replace(':', "_"))
    }

    fn count(&self) -> usize {
        std::fs::read_dir(&self.dir).map_or(0, |entries| entries.count())
    }
}

impl CredentialBackend for DiskBackend {
    fn set(&self, key: &str, token: &str) -> std::result::Result<(), String> {
        std::fs::write(self.path(key), token).map_err(|error| error.to_string())
    }
    fn get(&self, key: &str) -> std::result::Result<Option<String>, String> {
        match std::fs::read_to_string(self.path(key)) {
            Ok(token) => Ok(Some(token)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }
    fn delete(&self, key: &str) -> std::result::Result<(), String> {
        match std::fs::remove_file(self.path(key)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
    fn supports_named_entries(&self) -> bool {
        true
    }
}

/// Stops before it writes a given entry: reports "arrived" first, then waits for release before writing.
struct PausingBackend {
    inner: DiskBackend,
    key: String,
    gate: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>>,
}

impl CredentialBackend for PausingBackend {
    fn set(&self, key: &str, token: &str) -> std::result::Result<(), String> {
        if key == self.key {
            if let Some((reached, release)) = self.gate.lock().unwrap().take() {
                reached.send(()).unwrap();
                release.recv().unwrap();
            }
        }
        self.inner.set(key, token)
    }
    fn get(&self, key: &str) -> std::result::Result<Option<String>, String> {
        self.inner.get(key)
    }
    fn delete(&self, key: &str) -> std::result::Result<(), String> {
        self.inner.delete(key)
    }
    fn supports_named_entries(&self) -> bool {
        true
    }
}

/// Several tests run in parallel in one test process. The macOS clock has only microsecond resolution, so pid + timestamp alone
/// can give two tests the same directory, and one child process disconnects the other test's token.
fn temp_dir() -> PathBuf {
    static SERIAL: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "zeppbridge-official-xproc-{}-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed),
        crate::unique_suffix()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn used() -> OfficialTokens {
    OfficialTokens {
        access_token: "access-used".into(),
        refresh_token: Some("refresh-used".into()),
        expires_at: Some(10_000),
        user_id: "3000000002".into(),
    }
}

fn fresh() -> OfficialTokens {
    OfficialTokens {
        access_token: "access-fresh".into(),
        refresh_token: Some("refresh-fresh".into()),
        expires_at: Some(20_000),
        ..used()
    }
}

fn spawn_child(role: &str, dir: &Path, relay: Option<&str>) -> Child {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([CHILD_TEST, "--exact", "--test-threads", "1", "--nocapture"])
        .env(ROLE_ENV, role)
        .env(DIR_ENV, dir);
    if let Some(relay) = relay {
        command.env(RELAY_ENV, relay);
    }
    command.spawn().unwrap()
}

fn wait_success(mut child: Child) {
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "child process failed: {status}");
            return;
        }
        assert!(std::time::Instant::now() < deadline, "child process never finished");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The child process entry point. In a normal test run there is no role variable, so it passes at once.
#[test]
fn child_process_entry() {
    let Ok(role) = std::env::var(ROLE_ENV) else {
        return;
    };
    let dir = PathBuf::from(std::env::var(DIR_ENV).unwrap());
    let store = OfficialStore::with_backend(&dir, Arc::new(DiskBackend::new(&dir)));
    match role.as_str() {
        "disconnect" => {
            store.disconnect().unwrap();
        }
        "revoke" => {
            store.mark_needs_reauth_if_current(&used()).unwrap();
        }
        "refresh" => {
            let relay = std::env::var(RELAY_ENV).unwrap();
            let client = OfficialClient::with_bases(&relay, &relay).unwrap();
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let tokens = runtime
                .block_on(fresh_tokens(&store, &client, 10_000))
                .unwrap()
                .unwrap();
            assert_eq!(tokens.refresh_token.as_deref(), Some("refresh-1"));
        }
        other => panic!("unknown role {other}"),
    }
}

/// The parent starts a refresh write-back and stops before it writes the refresh token; meanwhile a child process with `role` starts.
/// Returns the write-back result. The child must wait (queued behind the lock) until the parent releases.
fn race_refresh_write_back(dir: &Path, role: &str) -> bool {
    OfficialStore::with_backend(dir, Arc::new(DiskBackend::new(dir)))
        .save(&used(), 1)
        .unwrap();
    let (reached_tx, reached_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let backend = Arc::new(PausingBackend {
        inner: DiskBackend::new(dir),
        key: "official:3000000002:refresh".into(),
        gate: Mutex::new(Some((reached_tx, release_rx))),
    });
    let store = OfficialStore::with_backend(dir, backend);
    let writer = std::thread::spawn(move || store.replace_if_current(&used(), &fresh(), 5));
    reached_rx.recv_timeout(Duration::from_secs(10)).unwrap();

    let mut child = spawn_child(role, dir, None);
    // Give the child enough time to start and hit the lock. If it has already finished now, it did not queue.
    std::thread::sleep(Duration::from_millis(1500));
    assert!(
        child.try_wait().unwrap().is_none(),
        "the other process acted before this side finished writing"
    );
    release_tx.send(()).unwrap();
    let written = writer.join().unwrap().unwrap();
    wait_success(child);
    written
}

/// The desktop disconnects while the CLI refreshes: the disconnect queues after the write-back, and the end state is a clean "not connected".
/// It never leaves "metadata says connected but the token is gone", and never writes the token back to a disconnected account.
#[test]
fn a_disconnect_in_another_process_never_lands_inside_a_refresh_write_back() {
    let dir = temp_dir();
    assert!(race_refresh_write_back(&dir, "disconnect"));
    let store = OfficialStore::with_backend(&dir, Arc::new(DiskBackend::new(&dir)));
    assert!(store.meta().unwrap().is_none());
    assert!(store.load().unwrap().is_none());
    assert_eq!(DiskBackend::new(&dir).count(), 0);
    let _ = std::fs::remove_dir_all(dir);
}

/// Another process is refused with the old refresh token (needs reauthorization) while this side just refreshed: the refusal queues after
/// the write-back, the check finds the new token in the store, and it does nothing. The new token is not erased.
#[test]
fn a_revocation_in_another_process_cannot_wipe_a_refresh_written_here() {
    let dir = temp_dir();
    assert!(race_refresh_write_back(&dir, "revoke"));
    let store = OfficialStore::with_backend(&dir, Arc::new(DiskBackend::new(&dir)));
    assert_eq!(store.load().unwrap(), Some(fresh()));
    assert!(!store.meta().unwrap().unwrap().needs_reauth);
    let _ = std::fs::remove_dir_all(dir);
}

/// A fake relay: the first refresh request waits until the second request arrives (or 3 seconds pass). Without cross-process
/// single-flight, the child's second refresh arrives in those 3 seconds.
async fn waiting_relay() -> (String, Arc<AtomicUsize>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let counter = counter.clone();
            tokio::spawn(async move {
                let mut buffer = vec![0u8; 8192];
                let _ = socket.read(&mut buffer).await;
                let n = counter.fetch_add(1, Ordering::SeqCst) + 1;
                for _ in 0..60 {
                    if counter.load(Ordering::SeqCst) > 1 {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                let body = format!(
                    r#"{{"status":"ready","access_token":"access-{n}","refresh_token":"refresh-{n}","expires_in":3600}}"#
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    (base, hits)
}

/// Desktop and CLI find the token expired at the same time: one refresh token is exchanged only once, and the later process waits
/// for the first to finish and uses the new token from the store.
#[tokio::test]
async fn two_processes_spend_a_refresh_token_only_once() {
    let dir = temp_dir();
    let store = OfficialStore::with_backend(&dir, Arc::new(DiskBackend::new(&dir)));
    store.save(&used(), 1).unwrap();
    let (base, hits) = waiting_relay().await;
    let client = OfficialClient::with_bases(&base, &base).unwrap();

    let child = spawn_child("refresh", &dir, Some(&base));
    let tokens = fresh_tokens(&store, &client, 10_000)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(tokens.refresh_token.as_deref(), Some("refresh-1"));
    tokio::task::spawn_blocking(move || wait_success(child))
        .await
        .unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    let _ = std::fs::remove_dir_all(dir);
}
