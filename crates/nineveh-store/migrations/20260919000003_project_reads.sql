-- When a project's API was last read (ADR 0023).
--
-- A project whose state nobody reads is folding for nobody, and folding is the part
-- worth stopping. There was no signal for it: `nineveh.api_keys.last_used_at` is per
-- key and only exists in hosted mode, and local mode recorded nothing at all.
--
-- Written at most once a minute per project, so a busy API doesn't turn every read into
-- a write.

CREATE TABLE nineveh.project_reads (
    project      text PRIMARY KEY,
    last_read_at timestamptz NOT NULL DEFAULT now()
);
