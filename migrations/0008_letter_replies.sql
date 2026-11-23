-- Replies to the author's letters (the "letter box", functions/api/letters/[action].js).
--
-- A reply is a message from one person to another, so we store it apart from diagnostic reports. install_hash is a salted hash of the local random install id,
-- not linked to an account. The receipt returns only status for it, not the original text.
--
-- The author writes the last five columns when handling a reply (scripts/letters/replies.mjs): read_at (read), category (which group it goes to,
-- the #id in the last echo), status (next / later / asking / fixed / declined), fixed_in (the version with the fix),
-- handled_at (when it was handled). Rows handled 180 days ago or more are deleted online (a local export keeps a copy outside the repo).
CREATE TABLE IF NOT EXISTS letter_replies (
  id TEXT PRIMARY KEY,
  received_at TEXT NOT NULL,
  install_hash TEXT NOT NULL,
  app_version TEXT NOT NULL,
  letter_version TEXT NOT NULL,
  locale TEXT NOT NULL,
  body TEXT NOT NULL,
  allow_quote INTEGER NOT NULL DEFAULT 0,
  also_hit_json TEXT NOT NULL DEFAULT '[]',
  content_hash TEXT NOT NULL,
  read_at TEXT,
  category TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT '',
  fixed_in TEXT NOT NULL DEFAULT '',
  handled_at TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_letter_replies_content_hash ON letter_replies(content_hash);
CREATE INDEX IF NOT EXISTS idx_letter_replies_install ON letter_replies(install_hash, received_at);
CREATE INDEX IF NOT EXISTS idx_letter_replies_version ON letter_replies(letter_version, received_at);
