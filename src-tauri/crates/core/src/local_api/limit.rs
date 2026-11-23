//! Simple rate limits: requests per minute per source IP, and auth failures.
//!
//! The desktop app has only loopback clients, so the main threat is "a local
//! process that guesses the token in a loop". When the headless version listens
//! on a LAN or container network, the threat is a device on the same network
//! that brute-forces the token. Neither needs an exact token bucket. A fixed
//! one-minute window is enough, and memory has a bound (see [`MAX_TRACKED_PEERS`]).

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// After this many auth failures (in one minute), block this source for a while.
pub const MAX_AUTH_FAILURES_PER_MINUTE: u32 = 10;

/// How long to block.
pub const AUTH_LOCKOUT: Duration = Duration::from_secs(10 * 60);

/// The maximum number of tracked sources. When full, remove expired ones first. If none has expired, refuse new sources (no unbounded growth).
const MAX_TRACKED_PEERS: usize = 1024;

const WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    /// Too fast. Come back after `Retry-After` seconds.
    Throttled(u64),
}

#[derive(Debug)]
struct Peer {
    window_start: Instant,
    requests: u32,
    failures: u32,
    locked_until: Option<Instant>,
}

impl Peer {
    fn new(now: Instant) -> Self {
        Self {
            window_start: now,
            requests: 0,
            failures: 0,
            locked_until: None,
        }
    }

    fn roll(&mut self, now: Instant) {
        if now.duration_since(self.window_start) >= WINDOW {
            self.window_start = now;
            self.requests = 0;
            self.failures = 0;
        }
    }

    fn stale(&self, now: Instant) -> bool {
        now.duration_since(self.window_start) >= WINDOW
            && self.locked_until.is_none_or(|until| until <= now)
    }
}

#[derive(Debug)]
pub struct RateLimiter {
    /// The maximum requests per minute. `None` = no request limit, only auth failures are limited.
    requests_per_minute: Option<u32>,
    peers: Mutex<HashMap<IpAddr, Peer>>,
}

impl RateLimiter {
    pub fn new(requests_per_minute: Option<u32>) -> Self {
        Self {
            requests_per_minute,
            peers: Mutex::new(HashMap::new()),
        }
    }

    /// A request arrived (call this before auth).
    pub fn admit(&self, peer: IpAddr) -> Verdict {
        self.admit_at(peer, Instant::now())
    }

    /// This request failed auth.
    pub fn record_failure(&self, peer: IpAddr) {
        self.record_failure_at(peer, Instant::now());
    }

    pub(super) fn admit_at(&self, peer: IpAddr, now: Instant) -> Verdict {
        let mut peers = self.peers.lock().unwrap_or_else(|e| e.into_inner());
        if !peers.contains_key(&peer) && peers.len() >= MAX_TRACKED_PEERS {
            peers.retain(|_, entry| !entry.stale(now));
            if peers.len() >= MAX_TRACKED_PEERS {
                return Verdict::Throttled(WINDOW.as_secs());
            }
        }
        let entry = peers.entry(peer).or_insert_with(|| Peer::new(now));
        if let Some(until) = entry.locked_until {
            if until > now {
                return Verdict::Throttled(until.duration_since(now).as_secs().max(1));
            }
            entry.locked_until = None;
        }
        entry.roll(now);
        entry.requests += 1;
        match self.requests_per_minute {
            Some(limit) if entry.requests > limit => {
                let left = WINDOW.saturating_sub(now.duration_since(entry.window_start));
                Verdict::Throttled(left.as_secs().max(1))
            }
            _ => Verdict::Allow,
        }
    }

    pub(super) fn record_failure_at(&self, peer: IpAddr, now: Instant) {
        let mut peers = self.peers.lock().unwrap_or_else(|e| e.into_inner());
        let Some(entry) = peers.get_mut(&peer) else {
            return;
        };
        entry.roll(now);
        entry.failures += 1;
        if entry.failures >= MAX_AUTH_FAILURES_PER_MINUTE {
            entry.locked_until = Some(now + AUTH_LOCKOUT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([192, 168, 1, last])
    }

    #[test]
    fn request_budget_resets_after_a_minute() {
        let limiter = RateLimiter::new(Some(3));
        let start = Instant::now();
        for _ in 0..3 {
            assert_eq!(limiter.admit_at(ip(1), start), Verdict::Allow);
        }
        assert!(matches!(
            limiter.admit_at(ip(1), start),
            Verdict::Throttled(_)
        ));
        // Other sources are not affected.
        assert_eq!(limiter.admit_at(ip(2), start), Verdict::Allow);
        assert_eq!(
            limiter.admit_at(ip(1), start + Duration::from_secs(61)),
            Verdict::Allow
        );
    }

    #[test]
    fn repeated_auth_failures_lock_the_peer_out() {
        let limiter = RateLimiter::new(None);
        let start = Instant::now();
        for _ in 0..MAX_AUTH_FAILURES_PER_MINUTE {
            assert_eq!(limiter.admit_at(ip(9), start), Verdict::Allow);
            limiter.record_failure_at(ip(9), start);
        }
        match limiter.admit_at(ip(9), start + Duration::from_secs(1)) {
            Verdict::Throttled(seconds) => assert!(seconds > 60),
            Verdict::Allow => panic!("repeated wrong token guesses must block the source for a while"),
        }
        assert_eq!(
            limiter.admit_at(ip(9), start + AUTH_LOCKOUT + Duration::from_secs(1)),
            Verdict::Allow
        );
    }

    #[test]
    fn unlimited_requests_still_count_failures() {
        let limiter = RateLimiter::new(None);
        let start = Instant::now();
        for _ in 0..500 {
            assert_eq!(limiter.admit_at(ip(3), start), Verdict::Allow);
        }
    }
}
