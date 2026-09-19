-- Which sources the record log covers (ADR 0022).
--
-- A rebuild may replay the log only if every source the config now has is one the log
-- already holds records for. Adding a source is the exception the rule names: there is
-- no history for something that was never followed, so it goes back to the stream.
-- Removing one is fine — its records are simply not replayed.

ALTER TABLE nineveh.record_cursors
    ADD COLUMN sources text NOT NULL DEFAULT '';
