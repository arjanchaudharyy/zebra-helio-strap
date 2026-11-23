-- The author's one-to-one answer to a reply.
--
-- The author writes it with scripts/letters/replies.mjs reply (translated into the sender's language). The user's app gets it with the local
-- install id when it fetches the receipt, and shows it on the "My reply" envelope. Empty string = no answer / withdrawn.
-- Only adds a column. Does not change the 0008 table structure.
ALTER TABLE letter_replies ADD COLUMN author_reply TEXT NOT NULL DEFAULT '';
ALTER TABLE letter_replies ADD COLUMN author_reply_at TEXT;
