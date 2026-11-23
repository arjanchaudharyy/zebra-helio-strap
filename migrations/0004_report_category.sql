-- The problem type that the user picks.
--
-- Auto-detection only finds "unknown device or workout codes". Users can hit other problems:
-- data that does not match, or a field that is always empty. Without this column these users
-- cannot report at all: the server decides the report has "nothing to process" and rejects it.
--
-- Fixed values: device / workout / data / other, or an empty string (auto-detected reports).
ALTER TABLE feedback_reports ADD COLUMN category TEXT NOT NULL DEFAULT '';
