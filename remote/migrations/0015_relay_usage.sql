-- What the stream's relay (Cloudflare Realtime TURN) has been asked for, a month an account: the
-- credentials minted and the seconds they were minted for (ruling 1406). The account is the one whose
-- plan the station runs on, which pays for the relay: a guest's mint counts against the owner. Support
-- reads it with the query in STAGING.md. It counts what the Worker hands out; the bytes the relay then
-- carries are metered by Cloudflare alone and never reach this table. Additive only: a new table.
CREATE TABLE relay_usage (
  account_id TEXT NOT NULL, month TEXT NOT NULL, mints INTEGER NOT NULL DEFAULT 0,
  granted_seconds INTEGER NOT NULL DEFAULT 0, updated_at INTEGER NOT NULL,
  PRIMARY KEY (account_id, month)
);
