-- How many records each source has ever matched.
--
-- A source that matches nothing is not an error — a contract may simply be quiet — so
-- the pipeline says nothing about it, and a project runs perfectly while staying empty.
-- That silence is the commonest way to lose an afternoon: the usual cause is a type
-- name that is close but wrong, and nothing anywhere says so.
--
-- Cumulative rather than a count of rows still present: the question is "has this ever
-- matched", and the log is pruned. A counter answers it after pruning; a count of rows
-- would start lying.

ALTER TABLE nineveh.record_cursors
    ADD COLUMN matched jsonb NOT NULL DEFAULT '{}'::jsonb;
