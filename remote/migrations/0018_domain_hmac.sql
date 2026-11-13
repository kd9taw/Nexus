-- `accounts.domain_hash` (0016) is kept as an HMAC under TRIAL_KEY from here on, not a bare SHA-256: mail
-- domains are few and public, so a bare hash of one can be reversed from a list. A value written before
-- this is cleared, and the account writes the keyed one at its next sign-in, as an account made before 0016
-- learnt its first. Until then that account is not counted in its domain's trials, which can only let a
-- trial through, never refuse one. Changes no schema; the column and its index stay as they are.
UPDATE accounts SET domain_hash = NULL WHERE domain_hash IS NOT NULL;
