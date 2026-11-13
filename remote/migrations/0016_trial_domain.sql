-- One mail domain starts at most a few self-serve trials a month: a
-- catch-all domain otherwise gets one per address it invents. `domain_hash` is the SHA-256 of the
-- domain of the account's verified mailbox (authority.ts `mailDomain`), never the domain itself, and
-- NULL for the large public mailbox providers, whose every address is its own account and who are not
-- counted this way. Written at sign-in like `mailbox_hash`, and learnt on the next sign-in by an
-- account made before it. Additive only: a nullable column and its index.
ALTER TABLE accounts ADD COLUMN domain_hash TEXT;
CREATE INDEX accounts_domain_hash ON accounts(domain_hash);
