-- Browser approval lifetime (operator decision 2026-09-14): an approval renews on use, capped at
-- ninety days since the shack approved it. `approved_at` is when that approval was given, and the
-- ninety days are measured from it. Only approving again at the shack writes it; use never does.
--
-- Additive and nullable, and deliberately NOT backfilled. An approval given before this lands, or by
-- a Nexus that does not send the lifetime header, has no approval time and so never renews: that
-- Nexus binds its restored grants to the approval's expiry, and a renewal would silently drop them.
-- The approval generation needs no column: `devices.generation` already moves only on approve and
-- revoke, and a renewal never touches it.
ALTER TABLE devices ADD COLUMN approved_at INTEGER;
