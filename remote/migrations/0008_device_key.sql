-- The browser's device key (security review M1; the stream's A5). The page holds an ECDSA P-256
-- key it cannot export, and sends only its public half, as SPKI in lowercase hex, when it asks for a
-- device or confirms a pairing. The station pins its SHA-256 when the operator approves that browser
-- at the radio, and a stream offer must be signed by the key it pinned. This side only stores the
-- public key and lists it to a Nexus that asks: it is never what a stream is admitted on.
--
-- Additive and nullable, and deliberately NOT backfilled. A browser approved before this lands has no
-- key, and cannot start a stream until it has sent one and the operator has approved it again at the
-- radio. The pairing's column carries the confirming browser's key to the device the approval creates,
-- and moves with `device_hash` when another browser confirms.
ALTER TABLE devices ADD COLUMN public_key TEXT;
ALTER TABLE enrollments ADD COLUMN public_key TEXT;
