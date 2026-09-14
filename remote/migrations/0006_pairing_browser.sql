-- One approval (operator decision 2026-09-13): approving a pairing at the shack also approves the
-- browser that confirmed it. That browser has no device row yet, and cannot have one, because a
-- device references a station and the station does not exist until approval. So confirm hands the
-- browser its device credential as a cookie and keeps only the digest here; the approve batch turns
-- it into an approved device in the same step that creates the station.
--
-- Additive and nullable: an enrollment confirmed before this lands, or by a browser of that era,
-- simply has no browser to approve, and pairs exactly as it did.
ALTER TABLE enrollments ADD COLUMN device_hash TEXT;
