-- Reply image attachments: the images live in R2 (binding LETTER_ATTACHMENTS, private bucket). This table stores only ownership and checksum data.
-- At most 3 per reply. The same image in one reply is stored once (UNIQUE reply_id + sha256). Deleted with the R2 objects after 30 days.
CREATE TABLE IF NOT EXISTS letter_attachments (
  id TEXT PRIMARY KEY,
  reply_id TEXT NOT NULL,
  received_at TEXT NOT NULL,
  content_type TEXT NOT NULL,
  bytes INTEGER NOT NULL,
  sha256 TEXT NOT NULL,
  object_key TEXT NOT NULL,
  UNIQUE (reply_id, sha256)
);
CREATE INDEX IF NOT EXISTS idx_letter_attachments_reply ON letter_attachments (reply_id);
CREATE INDEX IF NOT EXISTS idx_letter_attachments_received ON letter_attachments (received_at);
