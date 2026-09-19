-- The record log (ADR 0022): every record the decoder emitted for a project, kept so a
-- rebuild can replay them instead of re-reading the chain.
--
-- Keyed by project rather than by state schema. A rebuild builds into `S__next` and the
-- swap drops `S`, so records hung off a schema would be deleted by the very operation
-- that needs them. The whole point is that they outlive builds.

CREATE TABLE nineveh.records (
    project          text   NOT NULL,
    version          bigint NOT NULL CHECK (version >= 0),
    -- The record's position in its transaction, in the order the decoder emitted it:
    -- events before write-set changes, which is the order the fold must see.
    ord              integer NOT NULL CHECK (ord >= 0),
    -- Transaction-level facts a rule may read. Denormalized onto each record: the
    -- alternative is a join on the hot replay path to save a few bytes.
    timestamp_micros bigint NOT NULL CHECK (timestamp_micros >= 0),
    success          boolean NOT NULL,
    sender           text,
    -- The record itself, in the encoding `nineveh-decode::StoredRecord` round-trips.
    -- It names its source rather than numbering it: source ids are positional, so a
    -- reordered config would renumber them and misattribute everything already here.
    record           jsonb  NOT NULL,
    PRIMARY KEY (project, version, ord)
);

-- How far the log has been written, which is not how far the fold has committed. The
-- log leads; the fold follows with the cursor in `nineveh.projects`.
CREATE TABLE nineveh.record_cursors (
    project    text PRIMARY KEY,
    -- The last version whose records are all written. Null before the first.
    cursor     bigint CHECK (cursor >= 0),
    -- The lock the records were decoded against. Re-pinning layouts invalidates them,
    -- so a change here means the log has to be filled again from the stream.
    lock_hash  text NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now()
);
