-- Two gates on the feedback intake: content dedup and per-source rate limits.
--
-- Validation is strict, but strict validation only makes sure "the fields are valid". It cannot stop a report with all valid fields
-- from being POSTed again and again. The real risk is not D1 space. It is pollution when we later learn device codes and
-- workout codes from these reports: the rule is "at least two independent reports for each code", and
-- one person who submits a hundred times makes any code look like it has "many reports".
--
-- CORS does not stop this: it only limits browser pages. curl and scripts are not affected.

-- Content digest of the normalized report.
--
-- When the same content comes again, we do not insert a new row. We return the id of the old row as is. For the client
-- this is a successful submit (a retry, a resend after a dropped connection, or a double click must not give an error). For statistics
-- it counts as one report.
--
-- NULL is allowed: rows from before this column have no digest, and a SQLite UNIQUE index treats
-- each NULL as different, so old rows do not conflict with each other.
ALTER TABLE feedback_reports ADD COLUMN content_hash TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_feedback_reports_content_hash
  ON feedback_reports(content_hash)
  WHERE content_hash IS NOT NULL;

-- Counter for per-source rate limits.
--
-- **This stores a salted hash, not an IP.** The salt changes every day (see `sourceKey` in
-- functions/api/feedback.js), so this table can find "the same source" in one window, but cannot be used
-- to find who it is: a reverse lookup needs to enumerate the IP space, and after one day even that path is gone.
--
-- It has no column linked to feedback_reports: from this table you cannot tell which report came from which source.
CREATE TABLE IF NOT EXISTS feedback_intake_counters (
  source_hash TEXT PRIMARY KEY,
  window_started_at TEXT NOT NULL,
  count INTEGER NOT NULL
);

-- Scan by window when we clean up expired counters.
CREATE INDEX IF NOT EXISTS idx_feedback_intake_counters_window
  ON feedback_intake_counters(window_started_at);
