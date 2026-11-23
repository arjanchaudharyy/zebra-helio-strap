-- Zepp official authorization relay (D1 zeppbridge-official). Separate from the feedback database.
--
-- oauth_pending: temporary storage for one authorization, from /start until the desktop app claims it at /claim. 10 minutes at most.
-- state is stored only as a hash. Tokens are stored encrypted with RELAY_KEY and deleted when claimed.
CREATE TABLE oauth_pending (
  state_hash TEXT PRIMARY KEY,
  claim_hash TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'ready', 'denied', 'failed')),
  error TEXT,
  tokens_enc TEXT,
  created_at INTEGER NOT NULL
);
CREATE INDEX idx_oauth_pending_created ON oauth_pending (created_at);

-- official_users: Zepp accounts that completed authorization. Only the SHA-256 of user_id is stored.
-- The data relay accepts pushes only for accounts listed here.
CREATE TABLE official_users (
  user_hash TEXT PRIMARY KEY,
  registered_at INTEGER NOT NULL,
  last_seen_at INTEGER NOT NULL
);
