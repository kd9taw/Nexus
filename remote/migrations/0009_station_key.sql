-- The station's own signing key (security review S3-M1). Nexus at the shack makes an ECDSA P-256 key
-- once for each pairing, keeps the private half in its OS credential store, and sends the public half,
-- SPKI in lowercase hex, before it connects. The station signs its stream answers with it, and a page
-- takes only an answer this key signed. This side only stores it and lists it to the account's own
-- pages: the first key a station sends is kept, and any other is refused, so a key changes only by
-- pairing again, which is a new station.
--
-- Additive and nullable, and deliberately NOT backfilled. A station from before the key has none until
-- it is updated and connects, and until then pages refuse its streams, saying to update it.
ALTER TABLE stations ADD COLUMN public_key TEXT;
