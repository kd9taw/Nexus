-- The owner moving the address that gets the "device added" notice (index.ts, account/notice/*). The
-- notice is what lets a new device's transmit hold lift, so a new address is taken only once a code
-- mailed to it has come back (`verified_at`), and only after a wait (`effective_at`) in which the old
-- address is told and the owner can cancel. One row an account at most; it goes once the new address
-- has taken over, or is cancelled, refused five times, or left past `expires_at` unconfirmed. Additive
-- only: a new table.
CREATE TABLE notice_changes (
  account_id TEXT PRIMARY KEY, address TEXT NOT NULL, code_hash TEXT NOT NULL, expires_at INTEGER NOT NULL,
  tries INTEGER NOT NULL DEFAULT 0, verified_at INTEGER, effective_at INTEGER, created_at INTEGER NOT NULL
);
