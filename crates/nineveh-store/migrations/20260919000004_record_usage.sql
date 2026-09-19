-- How much a project's record log holds, kept as a running total rather than measured
-- (ADR 0022, ADR 0024).
--
-- Measuring it meant `sum(pg_column_size(record))` over every row of a project, which
-- is a full heap scan with a detoast per row: ten seconds on a log of 860,000 records,
-- on a query Studio polls and the retention sweep calls. The totals are maintained by
-- the two places that change the log — appending and pruning — so reading them is one
-- row.
--
-- They are a gauge and a soft quota, not an accounting ledger: a drift costs a slightly
-- wrong bar and a slightly early or late prune, never a lost record.

ALTER TABLE nineveh.record_cursors
    ADD COLUMN record_count bigint NOT NULL DEFAULT 0,
    ADD COLUMN record_bytes bigint NOT NULL DEFAULT 0;

-- Existing logs are measured once, here, so the totals start correct.
UPDATE nineveh.record_cursors c
SET record_count = m.count,
    record_bytes = m.bytes
FROM (
    SELECT project,
           count(*)                                AS count,
           coalesce(sum(pg_column_size(record)), 0) AS bytes
    FROM nineveh.records
    GROUP BY project
) m
WHERE m.project = c.project;
