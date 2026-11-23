//! Region probe after login: try hosts batch by batch, from hints and the allow list.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RegionProbeFailure {
    Rejected,
    Transient,
    Other,
}

#[derive(Debug, Default)]
pub(super) struct RegionProbeFailures {
    pub(super) rejected: usize,
    pub(super) transient: usize,
    pub(super) other: usize,
}

impl RegionProbeFailures {
    pub(super) fn record(&mut self, failure: RegionProbeFailure) {
        match failure {
            RegionProbeFailure::Rejected => self.rejected += 1,
            RegionProbeFailure::Transient => self.transient += 1,
            RegionProbeFailure::Other => self.other += 1,
        }
    }

    pub(super) fn into_login_failure(self) -> LoginFailure {
        // An explicit 401/403 is stronger evidence than failures from the
        // fallback hosts. Do not hide it behind unrelated 404s or timeouts.
        if self.rejected > 0 {
            return LoginFailure::fatal(AppError::new(
                "err.login.credentials_rejected",
                "Zepp rejected these login credentials. Close the login window and sign in again.",
            ));
        }
        // The only case worth a retry in place: the credentials were not rejected, the region service is just out of reach now.
        if self.transient > 0 {
            return LoginFailure::retryable(AppError::new(
                "err.login.region_unreachable",
                "Cannot reach the Zepp region service now. Check your network and try again.",
            ));
        }
        LoginFailure::fatal(AppError::new(
            "err.login.region_probe_failed",
            "Read the credentials, but cannot confirm the account region. Sign in again, or enter the App Token manually.",
        ))
    }
}

pub(super) fn classify_region_probe_error(error: &ZeppBridgeError) -> RegionProbeFailure {
    match error {
        ZeppBridgeError::NeedsReauth(_) => RegionProbeFailure::Rejected,
        ZeppBridgeError::NetworkError(_) | ZeppBridgeError::RetryExhausted { .. } => {
            RegionProbeFailure::Transient
        }
        _ => RegionProbeFailure::Other,
    }
}

/// The region host that we select at the end, and how strong the reason for it is.
///
/// `confidence` is a stable code for the UI, not display text:
/// * `identified`: this host returned the devices bound to this account for `user_id`;
/// * `hinted`: Zepp named this host in this login response and the request worked, but this
///   account has no devices;
/// * `unconfirmed`: a guess from the fallback list. Nothing proves that it belongs to this account.
pub(super) struct RegionWinner {
    pub(super) auth: AuthInfo,
    pub(super) confidence: &'static str,
}

/// What remains after a batch of hosts is probed.
#[derive(Default)]
pub(super) struct RegionBatchOutcome {
    /// The host that claimed this account. If we have it, we do not need to look at others.
    pub(super) identified: Option<AuthInfo>,
    /// Of the hosts that answered without devices, the one first in preference order.
    pub(super) fallback: Option<(usize, AuthInfo)>,
}

impl RegionBatchOutcome {
    /// Take one probe result. Returns `true` when we have the strongest evidence and need not wait for others.
    ///
    /// Results arrive in completion order, but the choice follows preference order. So weak evidence
    /// compares by `rank`, not by arrival. This was the original bug: the first host to answer won,
    /// and a region that does not know this user does not look up data, so it often answers fastest.
    pub(super) fn record(&mut self, rank: usize, auth: AuthInfo, evidence: RegionEvidence) -> bool {
        match evidence {
            RegionEvidence::Identified => {
                self.identified = Some(auth);
                true
            }
            RegionEvidence::Empty => {
                let better = match self.fallback.as_ref() {
                    Some((current, _)) => rank < *current,
                    None => true,
                };
                if better {
                    self.fallback = Some((rank, auth));
                }
                false
            }
        }
    }

    /// Convert to the final result.
    ///
    /// `authoritative` tells if Zepp itself named this batch of hosts in this login response.
    /// If yes, the host still has Zepp's backing, even with no devices (the account can have no watch).
    /// If no, it is only a guess from the fallback list, and we must mark it so.
    pub(super) fn into_winner(self, authoritative: bool) -> Option<RegionWinner> {
        if let Some(auth) = self.identified {
            return Some(RegionWinner {
                auth,
                confidence: "identified",
            });
        }
        let (_, auth) = self.fallback?;
        Some(RegionWinner {
            auth,
            confidence: if authoritative {
                "hinted"
            } else {
                "unconfirmed"
            },
        })
    }
}

pub(super) async fn probe_region_hosts(
    user_id: &str,
    app_token: &str,
    hosts: &[String],
    authoritative_count: usize,
) -> std::result::Result<RegionWinner, LoginFailure> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
    let mut failures = RegionProbeFailures::default();
    let authoritative_count = authoritative_count.min(hosts.len());

    // A cname/domains/wf_baseUrl hint came from this login response (or from
    // the same already-saved user), so verify it before sending the token to
    // any fallback region. A short stage timeout leaves enough of the global
    // budget for recovery when Zepp returned a stale host.
    //
    // Zepp itself named the hosts in this stage in this login response, so they are identity
    // evidence. Use the first one that answers, even if the account has no watch. A blind scan of
    // the fallback list only makes the most common path wait tens of seconds.
    if authoritative_count > 0 {
        let stage_deadline = std::cmp::min(
            deadline,
            tokio::time::Instant::now() + Duration::from_secs(15),
        );
        let outcome = probe_region_batch(
            user_id,
            app_token,
            &hosts[..authoritative_count],
            0,
            stage_deadline,
            &mut failures,
        )
        .await;
        if let Some(winner) = outcome.into_winner(true) {
            return Ok(winner);
        }
    }

    // The fallback stage is a **guess**: no evidence says these hosts belong to this account. So we
    // cannot use "the first to answer wins". A region that does not know this user does not look up data,
    // and its structured empty response often arrives before real data from the correct region. Only a
    // host that returns devices counts. If none do, use the first in preference order and mark it unconfirmed.
    let outcome = probe_region_batch(
        user_id,
        app_token,
        &hosts[authoritative_count..],
        authoritative_count,
        deadline,
        &mut failures,
    )
    .await;
    if let Some(winner) = outcome.into_winner(false) {
        return Ok(winner);
    }
    Err(failures.into_login_failure())
}

/// Probe a batch of hosts at the same time.
///
/// `rank_offset` is the start position of this batch in the full preference list. Results arrive
/// in completion order, but the choice follows preference order, so each result carries its rank.
pub(super) async fn probe_region_batch(
    user_id: &str,
    app_token: &str,
    hosts: &[String],
    rank_offset: usize,
    deadline: tokio::time::Instant,
    failures: &mut RegionProbeFailures,
) -> RegionBatchOutcome {
    let mut outcome = RegionBatchOutcome::default();
    if hosts.is_empty() {
        return outcome;
    }
    type ProbeResult = std::result::Result<(usize, AuthInfo, RegionEvidence), RegionProbeFailure>;
    let (tx, mut rx) = tokio::sync::mpsc::channel::<ProbeResult>(hosts.len().max(1));
    let mut handles = Vec::new();
    for (index, host) in hosts.iter().enumerate() {
        let auth = AuthInfo {
            app_token: app_token.to_string(),
            user_id: user_id.to_string(),
            region_host: host.clone(),
        };
        let rank = rank_offset + index;
        let tx = tx.clone();
        handles.push(tokio::spawn(async move {
            let result = probe_region_evidence(&auth)
                .await
                .map(|evidence| (rank, auth, evidence))
                .map_err(|error| classify_region_probe_error(&error));
            let _ = tx.send(result).await;
        }));
    }
    drop(tx);

    for _ in 0..hosts.len() {
        let Some(remaining) = deadline.checked_duration_since(tokio::time::Instant::now()) else {
            failures.transient += 1;
            break;
        };
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(Ok((rank, auth, evidence)))) => {
                if outcome.record(rank, auth, evidence) {
                    break;
                }
            }
            Ok(Some(Err(failure))) => failures.record(failure),
            Ok(None) => break,
            Err(_) => {
                failures.transient += 1;
                break;
            }
        }
    }
    for handle in handles {
        handle.abort();
    }
    outcome
}

pub(super) async fn preferred_region_hosts(
    state: &AppState,
    user_id: &str,
    hint: Option<&str>,
) -> (Vec<String>, usize) {
    let mut hosts = Vec::new();
    // The current login response is authoritative. A saved host belongs to
    // the previous account and is only useful when that account id matches.
    if let Some(hint) = hint {
        for host in hosts_from_region_hint(hint) {
            push_unique_host(&mut hosts, &host);
        }
    }
    if let Ok(Some(saved)) = state.auth.load_auth() {
        if saved.user_id == user_id {
            push_unique_host(&mut hosts, &saved.region_host);
        }
    }
    let authoritative_count = hosts.len();
    for host in REGION_HOST_ALLOWLIST {
        push_unique_host(&mut hosts, host);
    }
    (hosts, authoritative_count)
}

pub(super) fn push_unique_host(hosts: &mut Vec<String>, raw: &str) {
    if let Ok(host) = validate_region_host(raw) {
        if !hosts.iter().any(|existing| existing == &host) {
            hosts.push(host);
        }
    }
}

/// Map a cookie hint onto the allow-listed regional API origins.
pub(crate) fn hosts_from_region_hint(hint: &str) -> Vec<String> {
    let trimmed = hint.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if let Ok(host) = validate_region_host(trimmed) {
        return vec![host];
    }

    let lowered = trimmed.to_ascii_lowercase();
    let token = lowered
        .rsplit(['/', '.', '-', '_'])
        .find(|part| {
            matches!(
                *part,
                "cn" | "cn2"
                    | "cn3"
                    | "us"
                    | "us2"
                    | "us3"
                    | "de"
                    | "de2"
                    | "sg"
                    | "sg2"
                    | "eu"
                    | "eu2"
                    | "in"
                    | "ru"
            )
        })
        .unwrap_or(lowered.as_str());

    REGION_HOST_ALLOWLIST
        .iter()
        .filter(|host| host.contains(&format!("-{token}.")))
        .map(|host| (*host).to_string())
        .collect()
}
