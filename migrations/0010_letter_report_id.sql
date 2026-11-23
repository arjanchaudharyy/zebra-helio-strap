-- A letter to the author can include a redacted diagnostic report ("letter = feedback"). This stores the report id that /api/feedback returns.
-- The author uses it to find that report in the feedback table. NULL if none is attached. Only adds a column. Does not change 0008 / 0009.
ALTER TABLE letter_replies ADD COLUMN report_id TEXT;
