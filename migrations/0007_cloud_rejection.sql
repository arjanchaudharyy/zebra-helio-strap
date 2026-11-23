-- The "not successful" that the cloud writes inside an HTTP 200.
--
-- The client's `classify_business_code` can already detect "HTTP 200 + non-success code",
-- but it **does not on purpose treat this code as "needs a new login"**: in one real local database,
-- 1075 kept wrapped responses all had code 1. Not one failure code was seen.
-- To send the user back to a new QR login because of a number we never saw is to trade a sure bad experience
-- for a guess. So the data must come back from users who really hit this case.
--
-- Matching real feedback: `c1f03eb2` "All my readings are showing empty" (v2.0.0),
-- Reddit `u/WatercressAromatic79` "I log in and the account is empty, no data at all".
--
-- We collect only three things, none of them free text: which stream, which code, and when.
-- **We do not collect the cloud's message text.** That is free text from the server, and this report promises users
-- that it sends only allowlisted fields.
--
-- The defaults keep old rows and old clients unaffected: a NULL code means "never seen".
ALTER TABLE feedback_reports ADD COLUMN cloud_rejection_code INTEGER;
ALTER TABLE feedback_reports ADD COLUMN cloud_rejection_stream TEXT NOT NULL DEFAULT '';
ALTER TABLE feedback_reports ADD COLUMN cloud_rejection_at TEXT NOT NULL DEFAULT '';
