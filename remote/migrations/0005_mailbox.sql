-- `email_hash` was one trial per ADDRESS, not per mailbox: it hashes the address after only a trim
-- and a lower-case, so op@gmail.com, op+2@gmail.com and o.p@gmail.com were three people, each with
-- their own trial, all delivered to one inbox that passes the tenant's verification link.
--
-- `mailbox_hash` is the same SHA-256 taken over the NORMALISED mailbox (authority.ts, mailbox()).
-- It is a new column rather than a rewrite of `email_hash` because a hash cannot be normalised
-- after the fact: rows written before this carry only the old form, and the sibling check has to
-- keep matching them. So `email_hash` keeps its exact old meaning for every row, old and new.
-- Nullable, and deliberately not backfilled here, because there is no address to normalise, only
-- its hash. An account learns its mailbox_hash on its next sign-in.
ALTER TABLE accounts ADD COLUMN mailbox_hash TEXT;
CREATE INDEX accounts_mailbox_hash ON accounts(mailbox_hash);
