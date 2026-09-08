-- Remote authority only. No QSO records, credentials in plaintext or telemetry.
PRAGMA foreign_keys = ON;
CREATE TABLE accounts (
  id TEXT PRIMARY KEY, issuer TEXT NOT NULL, subject TEXT NOT NULL,
  UNIQUE(issuer, subject)
);
CREATE TABLE trials (
  account_id TEXT PRIMARY KEY REFERENCES accounts(id),
  enabled INTEGER NOT NULL DEFAULT 0 CHECK(enabled IN (0,1)), expires_at INTEGER NOT NULL
);
CREATE TABLE enrollments (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, proof_hash TEXT NOT NULL,
  code_hash TEXT NOT NULL UNIQUE, expires_at INTEGER NOT NULL,
  account_id TEXT REFERENCES accounts(id), approved INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX enrollments_expiry ON enrollments(expires_at);
CREATE TABLE stations (
  id TEXT PRIMARY KEY, account_id TEXT NOT NULL REFERENCES accounts(id), name TEXT NOT NULL,
  credential_hash TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
  generation INTEGER NOT NULL DEFAULT 1, policy_version INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX stations_owner ON stations(account_id);
CREATE TABLE devices (
  id TEXT PRIMARY KEY, station_id TEXT NOT NULL REFERENCES stations(id),
  account_id TEXT NOT NULL REFERENCES accounts(id), name TEXT NOT NULL,
  credential_hash TEXT NOT NULL, approved INTEGER NOT NULL DEFAULT 0 CHECK(approved IN (0,1)),
  generation INTEGER NOT NULL DEFAULT 1, expires_at INTEGER NOT NULL
);
CREATE INDEX devices_station ON devices(station_id);
CREATE TABLE tickets (
  digest TEXT PRIMARY KEY, station_id TEXT NOT NULL REFERENCES stations(id),
  account_id TEXT NOT NULL, device_id TEXT NOT NULL, device_generation INTEGER NOT NULL,
  identity_until INTEGER NOT NULL, expires_at INTEGER NOT NULL
);
CREATE INDEX tickets_expiry ON tickets(expires_at);
CREATE TABLE rate_limits (id TEXT PRIMARY KEY, hits INTEGER NOT NULL, expires_at INTEGER NOT NULL);
CREATE INDEX rate_limits_expiry ON rate_limits(expires_at);
