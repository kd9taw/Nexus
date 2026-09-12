-- "One trial, ever" was one trial per PROVIDER SUBJECT, not per person. Signing up again with a
-- different Auth0 connection - the SAME email through Google instead of a password, say - yields a
-- different `sub`, and UNIQUE(issuer, subject) treats that as an unrelated human. A fresh account,
-- a fresh 14-day clock, repeatable for the cost of one sign-up.
--
-- A verified email is the thing those two identities actually share, so eligibility keys on it.
-- The column holds a SHA-256 of the lower-cased address and never the address itself: it only ever
-- needs comparing, and an entitlement table is a poor place to accumulate contactable personal
-- data. It is nullable because an account created before this, or a provider that supplies no
-- verified address, has none - those keep the old per-subject behaviour rather than being locked
-- out, and the ceiling on this defence is the tenant enforcing verification at all.
ALTER TABLE accounts ADD COLUMN email_hash TEXT;
CREATE INDEX accounts_email_hash ON accounts(email_hash);
