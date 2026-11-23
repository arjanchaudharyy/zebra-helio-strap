-- The short note that the user writes with an error report.
--
-- With only field structure and model codes, the person who reads the report often cannot tell which watch it is.
-- One line like "My watch is a Balance 2 but it shows as unknown" often helps more than ten fields.
--
-- The client redacts the text before it sends it (it removes local paths, emails, MACs, long hex/number
-- IDs) and cuts it to 500 characters. We check the same limit here again. We do not trust the client.
-- An empty note is an empty string, not NULL. Old rows are the same, so readers do not handle two cases.
ALTER TABLE feedback_reports ADD COLUMN user_note TEXT NOT NULL DEFAULT '';
