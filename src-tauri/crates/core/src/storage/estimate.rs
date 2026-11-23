//! Storage space estimate (daily size per stream, free disk space) (split out of storage/mod.rs by domain, logic unchanged).

use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CachedPayloadStats {
    pub(super) token: String,
    pub(super) streams: HashMap<String, (u64, i64)>,
}

pub(super) fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.0} MB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{bytes} B")
    }
}

pub(super) fn disk_free_bytes(path: &std::path::Path) -> Option<u64> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" {
            fn GetDiskFreeSpaceExW(
                directory: *const u16,
                free_bytes_available: *mut u64,
                total_bytes: *mut u64,
                total_free_bytes: *mut u64,
            ) -> i32;
        }
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        wide.push(0);
        let mut free = 0u64;
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut free,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        (ok != 0).then_some(free)
    }
    // macOS / Linux use statvfs.
    //
    // Before, this returned None directly, so `free_bytes` was always 0 on every Mac, and the backfill estimate
    // could only say "could not read free disk space". It gave no size estimate, and `allow_long_history`
    // had nothing to decide with. The README says macOS is supported, so this cannot work only on Windows.
    #[cfg(not(windows))]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
            return None;
        }
        // f_bavail is the block count a **non-privileged user** can really use. f_bfree includes the part reserved for root,
        // so reporting it to the user is too high. On file systems where f_frsize is 0, fall back to f_bsize.
        let block = if stat.f_frsize > 0 {
            stat.f_frsize
        } else {
            stat.f_bsize
        };
        if block == 0 {
            return None;
        }
        // Use u128 in between.
        //
        // The width of these fields changes by platform: on macOS fsblkcnt_t is u32 and c_ulong is u64,
        // on Linux both are u64. So "convert to u64" is necessary on one platform
        // and redundant on the other: `as u64` fails unnecessary_cast,
        // `u64::from` fails useless_conversion, and CI uses `-D warnings`,
        // so each form fails on at least one platform.
        //
        // Converting to u128 is a real widening on every platform, so no lint can call it redundant.
        // It also prevents a possible overflow of "block count times block size".
        let free = u128::from(stat.f_bavail) * u128::from(block);
        u64::try_from(free).ok()
    }
}

impl Database {
    /// The real usage rate of each stream on this machine.
    ///
    /// Divides the length of the local raw payloads by **the calendar span those payloads cover**, not by a
    /// hard-coded constant: "how much space for three more years" only makes sense with this person's own data.
    ///
    /// The denominator is intentionally not "how many distinct fetch dates". Fetches run in monthly batches, so one
    /// `daily_summary` payload can cover a full month, and one year of data lands on only a dozen or so
    /// fetch days. Dividing 1.5 GB by 19 gives an absurd "76 MB per day",
    /// and times one year that is 27 GB, enough to scare anyone away from a backfill. The real question is "how much does each day of history
    /// take", so the denominator must be the number of days those payloads cover.
    ///
    /// Streams with a span shorter than `MIN_OBSERVED_DAYS` days get `measured: false`. It is better to say "unknown"
    /// than to multiply a rate from a few days of samples by three years.
    pub(super) fn payload_stats_token(&self) -> Result<String> {
        let (count, max_id): (i64, i64) = self.conn.query_row(
            "SELECT COUNT(*), COALESCE(MAX(id), 0) FROM raw_records",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let generation = self
            .get_app_meta(RAW_PAYLOAD_STATS_GEN_KEY)?
            .unwrap_or_else(|| "0".into());
        Ok(format!("{count}:{max_id}:{generation}"))
    }

    pub(super) fn compute_raw_payload_stats(
        &self,
    ) -> Result<std::collections::HashMap<String, (u64, i64)>> {
        let mut stmt = self.conn.prepare(
            // Count the size **actually on disk**: compressed rows count by their compressed bytes.
            // Otherwise the estimate quotes plain-text size, and the user sees a number several times larger than the real usage.
            "SELECT stream,
                    SUM(CASE
                          WHEN payload_zip IS NOT NULL AND LENGTH(payload_zip) > 0
                            THEN LENGTH(payload_zip)
                          ELSE LENGTH(CAST(payload AS BLOB))
                        END),
                    CAST(julianday(MAX(start_utc)) - julianday(MIN(start_utc)) AS INTEGER) + 1
             FROM raw_records
             GROUP BY stream",
        )?;
        let observed = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    (
                        row.get::<_, i64>(1).unwrap_or(0).max(0) as u64,
                        row.get::<_, i64>(2).unwrap_or(0).max(0),
                    ),
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .collect();
        Ok(observed)
    }

    pub(super) fn raw_payload_stats_by_stream(
        &self,
    ) -> Result<std::collections::HashMap<String, (u64, i64)>> {
        let token = self.payload_stats_token()?;
        if let Ok(guard) = PAYLOAD_STATS_MEM.lock() {
            if let Some((cached_token, stats)) = guard.as_ref() {
                if cached_token == &token {
                    return Ok(stats.clone());
                }
            }
        }
        if let Some(value) = self.get_app_meta(RAW_PAYLOAD_STATS_KEY)? {
            if let Ok(cached) = serde_json::from_str::<CachedPayloadStats>(&value) {
                if cached.token == token {
                    if let Ok(mut guard) = PAYLOAD_STATS_MEM.lock() {
                        *guard = Some((cached.token.clone(), cached.streams.clone()));
                    }
                    return Ok(cached.streams);
                }
            }
        }
        let computed = self.compute_raw_payload_stats()?;
        if let Ok(mut guard) = PAYLOAD_STATS_MEM.lock() {
            *guard = Some((token, computed.clone()));
        }
        Ok(computed)
    }

    pub(super) fn persist_raw_payload_stats(&self) -> Result<()> {
        let token = self.payload_stats_token()?;
        let streams = self.compute_raw_payload_stats()?;
        let encoded = serde_json::to_string(&CachedPayloadStats {
            token: token.clone(),
            streams: streams.clone(),
        })
        .unwrap_or_else(|_| "{}".into());
        self.set_app_meta(RAW_PAYLOAD_STATS_KEY, &encoded)?;
        if let Ok(mut guard) = PAYLOAD_STATS_MEM.lock() {
            *guard = Some((token, streams));
        }
        Ok(())
    }

    pub(super) fn bump_payload_stats_generation(&self) -> Result<()> {
        let next = self
            .get_app_meta(RAW_PAYLOAD_STATS_GEN_KEY)?
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0)
            .saturating_add(1);
        self.set_app_meta(RAW_PAYLOAD_STATS_GEN_KEY, &next.to_string())?;
        if let Ok(mut guard) = PAYLOAD_STATS_MEM.lock() {
            *guard = None;
        }
        Ok(())
    }

    pub(super) fn stream_storage_rates(&self, days: i64) -> Result<Vec<StreamStorageEstimate>> {
        let observed = self.raw_payload_stats_by_stream()?;

        Ok(coverage::BACKFILL_STREAMS
            .iter()
            .map(|stream| {
                let (bytes, observed_days) = observed.get(*stream).copied().unwrap_or((0, 0));
                let measured = observed_days >= MIN_OBSERVED_DAYS;
                let bytes_per_day = if measured {
                    bytes / observed_days.max(1) as u64
                } else {
                    0
                };
                StreamStorageEstimate {
                    stream: (*stream).to_string(),
                    observed_days,
                    observed_bytes: bytes,
                    bytes_per_day,
                    measured,
                    estimated_add_bytes: bytes_per_day.saturating_mul(days.max(0) as u64),
                }
            })
            .collect())
    }

    pub fn storage_estimate(
        &self,
        days: i64,
        data_dir: &std::path::Path,
    ) -> Result<StorageEstimate> {
        // Use the backfill range here (up to ten years), not the retention range of 1-365.
        // "How much space for three years of backfill" is the main reason this estimate exists.
        let days = UserPrefs::clamp_history_days(days).map_err(ZeppBridgeError::ConfigError)?;
        let database_bytes = std::fs::metadata(data_dir.join("zepp.db"))
            .map(|meta| meta.len())
            .unwrap_or(0);

        let streams = self.stream_storage_rates(days)?;
        let measured_bytes: u64 = streams
            .iter()
            .map(|stream| stream.estimated_add_bytes)
            .sum();
        let observed_bytes: u64 = streams.iter().map(|stream| stream.observed_bytes).sum();
        let any_measured = streams.iter().any(|stream| stream.measured);
        let all_measured = streams.iter().all(|stream| stream.measured);

        // The database is larger than the raw payloads: it also has canonical rows, indexes and WAL. Scale by the ratio
        // measured on this machine, not by another guessed factor. Use the ratio only inside a sane range.
        let overhead = if observed_bytes > 0 && database_bytes > observed_bytes {
            ((database_bytes as f64) / (observed_bytes as f64)).clamp(1.0, 4.0)
        } else {
            1.0
        };
        let estimated_add_bytes = if any_measured {
            ((measured_bytes as f64) * overhead) as u64
        } else {
            (days as u64).saturating_mul(BYTES_PER_HISTORY_DAY)
        };

        let free_bytes = disk_free_bytes(data_dir).unwrap_or(0);
        // Keep some margin: filling the disk exactly is as bad as not fitting.
        let needed_bytes = estimated_add_bytes.saturating_add(SPACE_SAFETY_MARGIN_BYTES);
        let stop_reason = if free_bytes > 0 && needed_bytes > free_bytes {
            Some(format!(
                "This backfill needs about {} (including safety margin), but only {} is free on this drive. It will not start. Free up space or shorten the range first.",
                format_bytes(needed_bytes),
                format_bytes(free_bytes)
            ))
        } else {
            None
        };
        let stop_reason_code = stop_reason
            .as_ref()
            .map(|_| "ui.estimate.stop_no_space".to_string());

        let warn_tight_space =
            free_bytes < 1_073_741_824 || (free_bytes > 0 && estimated_add_bytes > free_bytes / 5);
        let allow_long_history = stop_reason.is_none()
            && !(free_bytes > 0 && free_bytes < 300 * 1024 * 1024 && days >= 90);
        // Send both the code and the text: the UI builds its own sentence from the code (it has the days and bytes),
        // and falls back to this text only when it does not know the code.
        let (message_code, message) = if let Some(reason) = &stop_reason {
            ("ui.estimate.stop_no_space", reason.clone())
        } else if free_bytes == 0 {
            (
                "ui.estimate.disk_unknown",
                "Could not read free disk space. Make sure this machine has enough space before backfilling.".to_string(),
            )
        } else if !allow_long_history {
            (
                "ui.estimate.disk_too_small",
                "Less than 300 MB of free disk space. Cannot backfill more than 90 days of history.".to_string(),
            )
        } else if !any_measured {
            (
                "ui.estimate.builtin_guess",
                format!(
                    "Not enough local samples yet, using the built-in rough estimate: {} days take about {}, {} free on this drive.",
                    days,
                    format_bytes(estimated_add_bytes),
                    format_bytes(free_bytes)
                ),
            )
        } else if all_measured {
            (
                "ui.estimate.measured",
                format!(
                    "Based on the real rate of local data, {} days take about {}, {} free on this drive.",
                    days,
                    format_bytes(estimated_add_bytes),
                    format_bytes(free_bytes)
                ),
            )
        } else {
            (
                "ui.estimate.partial",
                format!(
                    "Based only on the streams with local samples, {} days take about {} (other streams lack samples and are not counted), {} free on this drive.",
                    days,
                    format_bytes(estimated_add_bytes),
                    format_bytes(free_bytes)
                ),
            )
        };

        Ok(StorageEstimate {
            free_bytes,
            estimated_add_bytes,
            database_bytes,
            allow_long_history,
            warn_tight_space,
            message,
            message_code: message_code.to_string(),
            needed_bytes,
            requested_days: days,
            streams,
            measured: all_measured,
            stop_reason,
            stop_reason_code,
        })
    }
}
