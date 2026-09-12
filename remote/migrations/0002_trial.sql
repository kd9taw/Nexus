-- The trial becomes self-serve here. Two additive columns, and deliberately no backfill:
-- `expires_at - 14 days` would invent a start date for the pilot rows, and an invented date
-- is worse than an absent one because nothing downstream could tell them apart. `started_at`
-- stays NULL for every row written before this migration; the browser renders that as an
-- unknown start rather than guessing. `source` records who wrote the row, so a hand-enabled
-- pilot grant and a self-serve trial are never confused for one another.
ALTER TABLE trials ADD COLUMN started_at INTEGER;
ALTER TABLE trials ADD COLUMN source TEXT NOT NULL DEFAULT 'manual';
